// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Wave B, part 2: the staging/scan/recount legs of the C.3 pipeline — the
//! C.4 infra-vs-verdict boundary (crash-mid-scan is NEVER a moderation
//! code), the terminal moderation codes, `DATASET_INTEGRITY` both ways,
//! `DECLARED_TOKENS_MISMATCH` with `{declared, actual}`, plausibility, and
//! TD15 staging deletion on every terminal path.

use fabstir_llm_node::training::core::{
    accept_and_prepare, TrainReject, CONTENT_BLOCKED, CONTENT_FLAGGED, DATASET_INTEGRITY,
    DECLARED_TOKENS_MISMATCH, MODERATION_UNAVAILABLE, SIDECAR_UNAVAILABLE, VALIDATION_FAILED,
};

use super::support::{
    fixture, make_deps, model_id, passing_snapshot, CountBehaviour, Harness, MockSessions,
    ScanBehaviour, NOW,
};

fn good_sessions() -> MockSessions {
    MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    }
}

async fn settled_once(h: &Harness) -> bool {
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    h.completer.count() == 1
}

async fn run_reject(
    tamper: Option<&str>,
    scan: ScanBehaviour,
    count: CountBehaviour,
    job_id: u64,
) -> (Harness, TrainReject) {
    let fx = fixture(tamper).await;
    let h = make_deps(&fx, good_sessions(), scan, count);
    let reject = accept_and_prepare(&h.deps, job_id, &fx.job, NOW)
        .await
        .expect_err("row must reject");
    (h, reject)
}

#[tokio::test]
async fn wrong_manifest_sha_is_dataset_integrity() {
    let (h, reject) = run_reject(
        Some("manifest-sha"),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
        100,
    )
    .await;
    assert_eq!(reject.code, DATASET_INTEGRITY, "{reject:?}");
    assert!(
        reject.detail.contains("manifestSha256"),
        "{:?}",
        reject.detail
    );
    assert!(
        settled_once(&h).await,
        "C.3: terminal reject settles at zero"
    );
    assert!(!h.staging_dir.path().join("job-100").exists(), "TD15");
}

#[tokio::test]
async fn tampered_shard_is_dataset_integrity_and_staging_is_swept() {
    let (h, reject) = run_reject(
        Some("shard-sha"),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
        101,
    )
    .await;
    assert_eq!(reject.code, DATASET_INTEGRITY, "{reject:?}");
    assert!(reject.detail.contains("shard 0"), "{:?}", reject.detail);
    assert!(settled_once(&h).await);
    assert!(!h.staging_dir.path().join("job-101").exists(), "TD15");
}

#[tokio::test]
async fn implausible_manifest_bytes_reject_before_any_shard_lands() {
    // declared 5 tokens but ~69 bytes: totalBytes > declaredTokens × 8 (C.6).
    let (h, reject) = run_reject(
        Some("implausible-bytes"),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(5),
        102,
    )
    .await;
    assert_eq!(
        (reject.code, reject.reason),
        (VALIDATION_FAILED, Some("plausibility")),
        "{reject:?}"
    );
    assert!(settled_once(&h).await);
    // C.6: the gate runs BEFORE shard fetches — no dataset file was staged.
    assert!(!h
        .staging_dir
        .path()
        .join("job-102")
        .join("dataset.jsonl")
        .exists());
}

#[tokio::test]
async fn scan_blocked_and_flagged_are_terminal_moderation_codes() {
    let (h, reject) =
        run_reject(None, ScanBehaviour::Blocked, CountBehaviour::Tokens(9), 103).await;
    assert_eq!(reject.code, CONTENT_BLOCKED, "{reject:?}");
    assert!(settled_once(&h).await);
    assert!(!h.staging_dir.path().join("job-103").exists(), "TD15");

    let (h2, reject2) =
        run_reject(None, ScanBehaviour::Flagged, CountBehaviour::Tokens(9), 104).await;
    assert_eq!(reject2.code, CONTENT_FLAGGED, "{reject2:?}");
    assert!(settled_once(&h2).await);
}

#[tokio::test]
async fn scan_failure_envelope_is_moderation_unavailable() {
    // A LIVE scanner's explicit no-verdict (C.4): terminal, never re-shopped.
    let (h, reject) = run_reject(
        None,
        ScanBehaviour::FailEnvelope,
        CountBehaviour::Tokens(9),
        105,
    )
    .await;
    assert_eq!(reject.code, MODERATION_UNAVAILABLE, "{reject:?}");
    assert!(settled_once(&h).await);
}

#[tokio::test]
async fn sidecar_crash_mid_scan_is_sidecar_unavailable_never_moderation() {
    // THE C.4 boundary row: transport death must not brand the dataset.
    let (h, reject) = run_reject(None, ScanBehaviour::Drop, CountBehaviour::Tokens(9), 106).await;
    assert_eq!(reject.code, SIDECAR_UNAVAILABLE, "{reject:?}");
    assert_ne!(reject.code, MODERATION_UNAVAILABLE);
    assert!(settled_once(&h).await);
    assert!(!h.staging_dir.path().join("job-106").exists(), "TD15");
}

#[tokio::test]
async fn recount_mismatch_carries_declared_and_actual() {
    let (h, reject) =
        run_reject(None, ScanBehaviour::Cleared, CountBehaviour::Tokens(8), 107).await;
    assert_eq!(reject.code, DECLARED_TOKENS_MISMATCH, "{reject:?}");
    assert_eq!(
        reject.declared_actual,
        Some((9, 8)),
        "detail must carry {{declared, actual}}"
    );
    assert!(settled_once(&h).await);
    assert!(!h.staging_dir.path().join("job-107").exists(), "TD15");
}

#[tokio::test]
async fn count_dataset_malformed_is_validation_failed_dataset_format() {
    // §3.7: the sidecar's 400 DATASET_MALFORMED = the dataset genuinely is
    // not jsonl-text-v1 → VALIDATION_FAILED (detail datasetFormat), terminal.
    let (h, reject) =
        run_reject(None, ScanBehaviour::Cleared, CountBehaviour::Malformed, 108).await;
    assert_eq!(
        (reject.code, reject.reason),
        (VALIDATION_FAILED, Some("datasetFormat")),
        "{reject:?}"
    );
    assert!(settled_once(&h).await);
}

// --- the §3.7 catch-all rows (round-1 F7: previously unexercised arms) ---

#[tokio::test]
async fn count_envelope_catch_all_is_sidecar_unavailable() {
    // SOURCE_MUTATED (and its whole §3.7 catch-all class) = retryable infra,
    // never a dataset brand.
    let (h, reject) = run_reject(
        None,
        ScanBehaviour::Cleared,
        CountBehaviour::MutatedEnvelope,
        109,
    )
    .await;
    assert_eq!(reject.code, SIDECAR_UNAVAILABLE, "{reject:?}");
    assert!(settled_once(&h).await);
}

#[tokio::test]
async fn unrecognised_scan_verdict_is_sidecar_unavailable_never_a_brand() {
    let (h, reject) = run_reject(
        None,
        ScanBehaviour::UnknownVerdict,
        CountBehaviour::Tokens(9),
        110,
    )
    .await;
    assert_eq!(reject.code, SIDECAR_UNAVAILABLE, "{reject:?}");
    assert!(reject.detail.contains("verdict"), "{:?}", reject.detail);
    assert!(settled_once(&h).await);
}
