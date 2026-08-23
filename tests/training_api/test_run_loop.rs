// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T4.e: the slice loop (interface B.2) — settlement order
//! (delivery-before-settlement), the billing laws (forfeits bill), the
//! §3.7 k-split on stream death, cancel-at-boundary, the final slice's
//! adapter attestation, and per-slice work-root consumption.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use fabstir_llm_node::storage::s5_client::S5Storage;
use fabstir_llm_node::training::core::{
    run_training_session, AcceptedSession, PreparedTrain, RunEnd, RunProgress,
};
use fabstir_llm_node::training::trainer_client::TrainerClient;

use super::support::{
    fixture, line, make_deps, model_id, passing_snapshot, spawn_train_sidecar, CountBehaviour,
    Harness, MockSessions, ScanBehaviour, ScriptLine, TrainBehaviour, NOW,
};

pub(crate) fn good_sessions() -> MockSessions {
    MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    }
}

/// Build a slice/finalise script whose dirs+files exist under the work root.
pub(crate) fn slice_line(
    h: &Harness,
    index: u64,
    delay: u64,
    step_from: u64,
    step_to: u64,
) -> ScriptLine {
    let dir = format!("job-42/slice-{index}");
    let file_dir = h.deps.work_root.join(&dir);
    std::fs::create_dir_all(&file_dir).unwrap();
    std::fs::write(
        file_dir.join("adapter_model.safetensors"),
        vec![index as u8 + 1; 4096],
    )
    .unwrap();
    line(
        delay,
        &format!(
            r#"{{"event":"slice","index":{index},"stepFrom":{step_from},"stepTo":{step_to},"dir":"{dir}","files":[{{"name":"adapter_model.safetensors","relPath":"{dir}/adapter_model.safetensors","sizeBytes":4096}}]}}"#
        ),
    )
}

pub(crate) fn finalise_line(h: &Harness, delay: u64) -> ScriptLine {
    let dir = "job-42/adapter";
    let file_dir = h.deps.work_root.join(dir);
    std::fs::create_dir_all(&file_dir).unwrap();
    std::fs::write(file_dir.join("adapter_model.safetensors"), vec![0xAD; 4096]).unwrap();
    line(
        delay,
        &format!(
            r#"{{"event":"finalise","adapter":{{"dir":"{dir}","files":[{{"name":"adapter_model.safetensors","relPath":"{dir}/adapter_model.safetensors","sizeBytes":4096}}]}}}}"#
        ),
    )
}

pub(crate) fn prepared(h: &Harness) -> PreparedTrain {
    PreparedTrain {
        staged_dataset: h.deps.staging_root.join("job-42/dataset.jsonl"),
        training_tokens: 9,
        schedule: vec![5, 4],
        price_per_token: ethers::types::U256::from(904u64),
        verdict: "cleared".to_string(),
        policy_version: "structural-v0".to_string(),
    }
}

pub(crate) fn accepted(h: &Harness) -> AcceptedSession {
    AcceptedSession {
        snapshot: passing_snapshot(),
        training_tokens: 9,
        schedule: prepared(h).schedule,
    }
}

/// Drive a run over a scripted sidecar; returns (end, progress frames).
pub(crate) async fn drive(
    h: &Harness,
    script: Vec<ScriptLine>,
    cancel_after_ms: Option<u64>,
) -> (RunEnd, Vec<RunProgress>) {
    drive_with_schedule(h, script, cancel_after_ms, vec![5, 4]).await
}

pub(crate) async fn drive_with_schedule(
    h: &Harness,
    script: Vec<ScriptLine>,
    cancel_after_ms: Option<u64>,
    schedule: Vec<u64>,
) -> (RunEnd, Vec<RunProgress>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = spawn_train_sidecar(dir.path(), TrainBehaviour::Script(script));
    std::mem::forget(dir);
    let client = TrainerClient::new(sock, Duration::from_secs(5));
    let body = fabstir_llm_node::training::trainer_client::TrainWireRequest {
        job_id: 42,
        dataset_path: "/staging/job-42/dataset.jsonl".to_string(),
        declared_tokens: 9,
        epochs: 1,
        hyper: serde_json::from_value(serde_json::json!({
            "rank": 16, "alpha": 32, "lr": "0.000200", "seed": "13", "seqLen": 2048
        }))
        .unwrap(),
    };
    let stream = client
        .train(&body, Duration::from_secs(2))
        .await
        .expect("stream opens");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RunProgress>(64);
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(after_ms) = cancel_after_ms {
        let cancel_timer = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(after_ms)).await;
            cancel_timer.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }
    let collector = tokio::spawn(async move {
        let mut frames = Vec::new();
        while let Some(frame) = rx.recv().await {
            frames.push(frame);
        }
        frames
    });
    let mut prepared_run = prepared(h);
    prepared_run.schedule = schedule.clone();
    prepared_run.training_tokens = schedule.iter().sum();
    let mut accepted_run = accepted(h);
    accepted_run.schedule = schedule;
    let end = run_training_session(
        &h.deps,
        42,
        &fixture_job(),
        &prepared_run,
        &accepted_run,
        stream,
        tx,
        cancel,
        NOW,
    )
    .await;
    let frames = collector.await.unwrap();
    (end, frames)
}

pub(crate) fn fixture_job() -> fabstir_llm_node::training::types::TrainingJob {
    serde_json::from_value(serde_json::json!({
        "templateId": "train-qlora-synthetic-test-v1",
        "templateHash": "0xabababababababababababababababababababababababababababababababab",
        "dataset": {
            "manifestCID": "uAAA",
            "manifestSha256": format!("0x{}", "22".repeat(32)),
            "declaredTokens": 9u64,
            "samples": 3u64
        },
        "epochs": 1,
        "hyper": { "rank": 16, "alpha": 32, "lr": "0.000200", "seed": "13", "seqLen": 2048 },
        "output": "adapter-v1"
    }))
    .unwrap()
}

#[tokio::test]
async fn happy_two_slice_run_settles_in_order_and_sweeps_the_work_root() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        line(0, r#"{"event":"tick","stage":"loading"}"#),
        slice_line(&h, 0, 10, 0, 5),
        slice_line(&h, 1, 10, 5, 9),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    let (end, frames) = drive(&h, script, None).await;
    let RunEnd::Complete {
        adapter,
        billing,
        proof_cids,
        warnings,
    } = end
    else {
        panic!("expected Complete, got {end:?}");
    };
    assert!(warnings.is_empty());
    assert_eq!(proof_cids.len(), 2);
    assert_eq!(billing.settled_slices, 2);
    assert_eq!(billing.forfeited_slices, 0);
    assert_eq!(billing.billed_tokens, 9, "wire bill = schedule total");
    assert_eq!(billing.settled_tokens, 9);
    assert!(!adapter.manifest_cid.is_empty());
    // Delivery-before-settlement: Uploading(i) strictly precedes
    // SliceSettled(i) in the frame order for both slices.
    let position = |predicate: &dyn Fn(&RunProgress) -> bool| {
        frames
            .iter()
            .position(|f| predicate(f))
            .expect("frame present")
    };
    for index in [0u64, 1] {
        let up = position(
            &|f| matches!(f, RunProgress::Uploading { slice_index, .. } if *slice_index == index),
        );
        let settled =
            position(&|f| matches!(f, RunProgress::SliceSettled { index: i, .. } if *i == index));
        assert!(up < settled, "slice {index}: pointer before proof");
    }
    // The proofs carried the pinned deltas in order.
    let calls = h.proof.calls.lock().unwrap().clone();
    assert_eq!(
        calls.iter().map(|(_, t, _)| *t).collect::<Vec<_>>(),
        vec![5, 4]
    );
    // §5 consumption + TD15: nothing of job-42 remains under the work root.
    assert!(!h.deps.work_root.join("job-42").exists());
}

#[tokio::test]
async fn forfeited_slice_still_bills_and_the_run_completes() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    // Slice 0's proof fails ONCE with a NON-rate error: the conservative
    // law forfeits immediately (the tx may still mine — a retry could
    // double-claim; T4 converge round F1). ONE scripted error suffices:
    // under the old retry-anything behaviour the second attempt would pop
    // an empty script, succeed, and the forfeit asserts below would fail.
    h.proof
        .script
        .lock()
        .unwrap()
        .push_back(Err("submitProofOfWork unconfirmed (no receipt)".to_string()));
    let script = vec![
        slice_line(&h, 0, 0, 0, 5),
        slice_line(&h, 1, 10, 5, 9),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    let (end, frames) = drive(&h, script, None).await;
    let RunEnd::Complete { billing, .. } = end else {
        panic!("expected Complete, got {end:?}");
    };
    assert_eq!(billing.billed_tokens, 9, "the forfeited slice still BILLS");
    assert_eq!(
        billing.settled_tokens, 4,
        "on-chain = the landed delta only"
    );
    assert_eq!((billing.settled_slices, billing.forfeited_slices), (1, 1));
    let forfeited = frames.iter().any(|f| {
        matches!(
            f,
            RunProgress::SliceSettled {
                index: 0,
                submitted: false,
                ..
            }
        )
    });
    assert!(forfeited, "slice 0 must report submitted:false");
    // The no-retry pin: exactly ONE successful submit happened (slice 1);
    // slice 0's single failed attempt was NOT retried.
    assert_eq!(h.proof.count(), 1, "non-rate errors must not retry");
}

#[tokio::test]
async fn final_slice_attestation_carries_adapter_hash_and_moderation() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        slice_line(&h, 0, 0, 0, 5),
        slice_line(&h, 1, 10, 5, 9),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    let (end, _) = drive(&h, script, None).await;
    let RunEnd::Complete { adapter, .. } = end else {
        panic!("expected Complete, got {end:?}");
    };
    // The FINAL slice's attestation (index 1) carries the adapter hash +
    // the cleared moderation verdict (B.3).
    let stored = h
        .artifact_store
        .get("home/training/job_42_slice_1_attestation.json")
        .await
        .expect("final attestation uploaded");
    let value: serde_json::Value = serde_json::from_slice(&stored).unwrap();
    assert_eq!(value["adapterManifestSha256"], adapter.manifest_sha256);
    assert_eq!(value["moderation"]["status"], "cleared");
    assert_eq!(value["cumulativeTokens"], 9);
    // The NON-final attestation must NOT carry them.
    let stored0 = h
        .artifact_store
        .get("home/training/job_42_slice_0_attestation.json")
        .await
        .unwrap();
    let value0: serde_json::Value = serde_json::from_slice(&stored0).unwrap();
    assert!(value0.get("adapterManifestSha256").is_none());
}

#[tokio::test(start_paused = true)]
async fn too_many_retry_waits_by_the_training_rate() {
    // interface B.2: the "Too many" wait is recomputed from THIS model's
    // 10,000 tok/s rate (NOT LTX's 2,000): tokens 50,000 → 5 s + 5 s buffer.
    use fabstir_llm_node::training::core::submit_proof_with_retry;
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    h.proof
        .script
        .lock()
        .unwrap()
        .push_back(Err("execution reverted: Too many".to_string()));
    let deps = h.deps;
    let proof = h.proof.clone();
    let task = tokio::spawn(async move {
        submit_proof_with_retry(&deps, 42, 50_000, [0u8; 32], "uPROOF").await
    });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(proof.count(), 0, "first attempt errored; none recorded yet");
    // Before the 10 s wait elapses: no retry.
    tokio::time::advance(Duration::from_secs(9)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(proof.count(), 0, "retry must respect tokens/rate + 5 s");
    // Past it: the retry lands — assert BEFORE task.await (awaiting parks
    // the runtime and paused-clock auto-advance would mask a too-long wait).
    tokio::time::advance(Duration::from_secs(2)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        proof.count(),
        1,
        "the retry must land at ~tokens/10_000 + 5 s"
    );
    let submitted = task.await.unwrap();
    assert!(submitted);
}
