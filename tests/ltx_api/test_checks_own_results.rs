// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks, node 8.65.4: an input that is byte-identical to a result THIS host delivered after its own
//! checks (recorded by the generation task just before `ltx_complete`) is not sent to the classifier again, and
//! counts as a passed input. Enforce only; every miss is a check as before. Each test names its mutation.

use super::checks_chain::*;
use super::checks_svc::*;
use super::ltx_task_support::S5_ENV_LOCK;
use super::relight_stub::mp4_clip;
use super::test_checks_fixed_graph::{
    assert_skipped, go, go_on, go_with, preview, source, store50, Run, CHAIN, HDR, ILLEGAL,
};
use fabstir_llm_node::ltx::checks::alerts::LogAlerts;
use fabstir_llm_node::ltx::checks::cleared::Cleared;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::ContentChecks;
use fabstir_llm_node::ltx::exr::decrypt_frame;
use fabstir_llm_node::ltx::input_image::parse_capability_cid;
use serde_json::json;
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// The first artefact the client received (the marked mp4), read back out of the in-memory S5 by its capability.
async fn delivered(run: &Run) -> Vec<u8> {
    let env = parse_capability_cid(run.ran.terminal["frames"][0].as_str().unwrap()).unwrap();
    let s5 = run.cm.get_s5_storage();
    for dir in s5.list("home/ltx").await.unwrap() {
        for f in s5.list(&format!("home/ltx/{}", dir.name)).await.unwrap() {
            let ct = s5
                .get(&format!("home/ltx/{}/{}", dir.name, f.name))
                .await
                .unwrap();
            if blake3::hash(&ct).as_bytes() == &env.ct_hash {
                return decrypt_frame(&ct, &env.key, env.plaintext_len).unwrap();
            }
        }
    }
    panic!("the delivered frame is not in storage");
}

#[tokio::test]
async fn a_cutout_part_one_is_delivered_recorded_and_part_two_takes_it_without_a_check() {
    // The record not written, the lookup missing, or the count not credited -> the matte is judged -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let one = go(Mode::Enforce, HDR, "", &[source()], &[0], None, 731).await;
    assert_eq!(
        one.ran.terminal["type"], "ltx_complete",
        "{:?}",
        one.ran.terminal
    );
    let matte = delivered(&one).await;
    assert_eq!(
        one.rig.checks.cleared_lookup(&sha(&matte)),
        Some(Cleared::FixedGraph)
    );
    assert_eq!(one.rig.checks.cleared_len(), 1);
    let slice = mp4_clip(147);
    let two = go_on(
        one.rig,
        CHAIN,
        "",
        &[slice, matte.clone()],
        &[0],
        None,
        732,
        &json!({"frames": 41}),
    )
    .await;
    assert_eq!(
        two.ran.terminal["type"], "ltx_complete",
        "{:?}",
        two.ran.terminal
    );
    let seen: Vec<String> = two
        .rig
        .svc
        .seen("mp4")
        .into_iter()
        .map(|f| f.sha256)
        .collect();
    assert!(
        !seen.contains(&sha(&matte)),
        "the previous part's matte was sent to the classifier"
    );
    assert!(logs
        .text()
        .contains("stage=c3 verdict=CLEARED_OWN_RESULT how=fixed-graph"));
    assert!(logs
        .text()
        .contains("template=ltx-alpha-chain-hdr reason=\"empty prompt, inputs checked\" inputs=2"));
    assert!(two.rig.refusals().is_empty());
}

#[tokio::test]
async fn the_same_matte_with_one_byte_changed_is_checked() {
    // A lookup on anything but the exact bytes -> the changed file is skipped -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let one = go(Mode::Enforce, HDR, "", &[source()], &[0], None, 733).await;
    let mut matte = delivered(&one).await;
    let last = matte.len() - 1;
    matte[last] ^= 1;
    let two = go_on(
        one.rig,
        CHAIN,
        "",
        &[mp4_clip(147), matte.clone()],
        &[0],
        None,
        734,
        &json!({"frames": 41}),
    )
    .await;
    assert_eq!(two.ran.code(), "CONTENT_REFUSED", "{:?}", two.ran.terminal);
    assert!(two
        .rig
        .svc
        .seen("mp4")
        .iter()
        .any(|f| f.sha256 == sha(&matte)));
    assert_eq!(two.rig.refusals()[0]["stage"], json!("c3"));
    assert!(!logs.text().contains("CLEARED_OWN_RESULT"));
}

#[tokio::test]
async fn an_ordinary_result_that_passed_c5_is_recorded_and_clears_when_it_comes_back() {
    // `how` not C5Pass for a passing result, or only the exemptions recorded -> the lookup misses -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let spare = json!({"sparePreview": true});
    let one = go_with(
        Mode::Enforce,
        "ltx-edit-hdr",
        "make it blue",
        &[source()],
        &[0],
        None,
        735,
        &spare,
    )
    .await;
    assert_eq!(
        one.ran.terminal["type"], "ltx_complete",
        "{:?}",
        one.ran.terminal
    );
    let result = delivered(&one).await;
    assert_eq!(
        one.rig.checks.cleared_lookup(&sha(&result)),
        Some(Cleared::C5Pass)
    );
    // back as the input of an alpha job (41 frames: the stub's clip is short), spared by nobody: not sent to the
    // classifier, counted as checked, so the matte is not judged either
    let two = go_on(
        one.rig,
        HDR,
        "",
        &[result.clone()],
        &[],
        None,
        736,
        &json!({"frames": 41}),
    )
    .await;
    assert_eq!(
        two.ran.terminal["type"], "ltx_complete",
        "{:?}",
        two.ran.terminal
    );
    let seen: Vec<String> = two
        .rig
        .svc
        .seen("mp4")
        .into_iter()
        .map(|f| f.sha256)
        .collect();
    assert!(
        !seen.contains(&sha(&result)),
        "the earlier result was sent to the classifier"
    );
    assert!(logs
        .text()
        .contains("stage=c3 verdict=CLEARED_OWN_RESULT how=c5-pass"));
}

#[tokio::test]
async fn a_refused_result_is_never_recorded() {
    // Recording at the check instead of at delivery -> one entry -> red
    let _env = S5_ENV_LOCK.lock().await;
    let r = go(
        Mode::Enforce,
        "ltx-edit-hdr",
        "make it blue",
        &[source()],
        &[0],
        None,
        737,
    )
    .await;
    assert_eq!(r.ran.code(), "CONTENT_REFUSED", "{:?}", r.ran.terminal);
    assert_eq!(r.rig.checks.cleared_len(), 0);
    assert!(!r.rig.dir("evidence").join("cleared-results.json").exists());
}

#[tokio::test]
async fn in_shadow_and_off_nothing_is_recorded_and_nothing_is_skipped() {
    // Recording or looking up outside enforce -> an entry, or a skipped check -> red
    let _env = S5_ENV_LOCK.lock().await;
    let spare = json!({"sparePreview": true});
    for (mode, jid) in [(Mode::Shadow, 738), (Mode::Off, 739)] {
        let r = go_with(
            mode,
            "ltx-edit-hdr",
            "make it blue",
            &[source()],
            &[0],
            None,
            jid,
            &spare,
        )
        .await;
        assert_eq!(
            r.ran.terminal["type"], "ltx_complete",
            "{mode:?}: {:?}",
            r.ran.terminal
        );
        assert_eq!(r.rig.checks.cleared_len(), 0, "{mode:?}");
        assert!(!r.rig.dir("evidence").join("cleared-results.json").exists());
    }
    // an entry written by an enforcing node is not honoured by a shadow one reading the same file
    let (logs, _c) = Logs::capture();
    let enforce = rig(Mode::Enforce).await;
    let clip = mp4_clip(12);
    enforce
        .checks
        .record_cleared(&sha(&clip), Some(Cleared::C5Pass))
        .await;
    let shadow = rig_with(Mode::Shadow, true, |cfg| {
        cfg.cleared_file = enforce.checks.config().cleared_file.clone();
    })
    .await;
    shadow.svc.set_media("mp4", 0, ILLEGAL);
    let mut job = shadow.checks.job("s", Some(1), None);
    let _ = shadow.checks.check_clip(&mut job, &clip).await;
    assert!(!logs.text().contains("CLEARED_OWN_RESULT"));
    assert!(shadow
        .svc
        .seen("mp4")
        .iter()
        .any(|f| f.sha256 == sha(&clip)));
}

#[tokio::test]
async fn the_record_honours_the_exact_bytes_survives_a_restart_expires_and_tolerates_damage() {
    // Any of: a changed byte matches, the file ignored at load, no expiry, a damaged file crashing -> red
    let (logs, _c) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let file = rig.checks.config().cleared_file.clone().unwrap();
    let clip = mp4_clip(20);
    let other = mp4_clip(21);
    rig.checks
        .record_cleared(&sha(&clip), Some(Cleared::Passes))
        .await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    rig.checks
        .record_cleared_at(&sha(&other), Some(Cleared::C5Pass), now - 8 * 24 * 3600)
        .await;
    // honoured now, the expired one is not
    let mut job = rig.checks.job("s", Some(1), None);
    rig.checks.check_clip(&mut job, &clip).await.unwrap();
    assert!(logs
        .text()
        .contains("verdict=CLEARED_OWN_RESULT how=passes"));
    assert!(rig.checks.cleared_lookup(&sha(&other)).is_none(), "expired");
    assert!(
        rig.checks.cleared_lookup(&sha(&mp4_clip(22))).is_none(),
        "unknown"
    );
    // a restart: a new ContentChecks over the same file still honours it
    let again = ContentChecks::new(rig.checks.config().clone(), std::sync::Arc::new(LogAlerts));
    assert_eq!(again.cleared_lookup(&sha(&clip)), Some(Cleared::Passes));
    assert!(again.cleared_lookup(&sha(&other)).is_none());
    // a damaged or missing file is an empty record, not a crash
    for junk in [
        &b"{ not json"[..],
        b"",
        b"{\"entries\": [{\"sha256\": 5}]}",
        b"\xff\xfe",
    ] {
        std::fs::write(&file, junk).unwrap();
        let damaged =
            ContentChecks::new(rig.checks.config().clone(), std::sync::Arc::new(LogAlerts));
        assert_eq!(damaged.cleared_len(), 0);
    }
    std::fs::remove_file(&file).unwrap();
    assert_eq!(
        ContentChecks::new(rig.checks.config().clone(), std::sync::Arc::new(LogAlerts))
            .cleared_len(),
        0
    );
    // bounded: the cap drops the oldest
    for i in 0..(fabstir_llm_node::ltx::checks::cleared::MAX_ENTRIES + 5) {
        rig.checks
            .record_cleared_at(&format!("{i:064x}"), Some(Cleared::C5Pass), now + i as u64)
            .await;
    }
    assert!(rig.checks.cleared_len() <= fabstir_llm_node::ltx::checks::cleared::MAX_ENTRIES);
}
#[tokio::test]
async fn a_job_claiming_the_alpha_id_with_another_hash_is_refused_as_before() {
    // `verify` bypassed for the alpha ids -> accepted -> red
    let _env = S5_ENV_LOCK.lock().await;
    let r = go(
        Mode::Enforce,
        HDR,
        "",
        &[source()],
        &[0],
        Some("0xdead"),
        709,
    )
    .await;
    assert!(!r.ran.accepted);
    assert_eq!(r.ran.code(), "VALIDATION_FAILED", "{:?}", r.ran.terminal);
    assert_eq!(r.renderer.count("prompt"), 0);
    // and the hash of ANOTHER pinned template is no better
    let other = store50().template_hash("ltx-edit-hdr").unwrap().to_string();
    let r = go(Mode::Enforce, HDR, "", &[source()], &[0], Some(&other), 710).await;
    assert_eq!(r.ran.code(), "VALIDATION_FAILED");
}

#[tokio::test]
async fn with_checks_off_or_shadow_this_rule_does_not_apply() {
    // The `Mode::Enforce` guard removed -> the shadow run logs the skip and never sends the matte -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let r = go(Mode::Off, HDR, "", &[source()], &[0], None, 711).await;
    assert_eq!(
        r.ran.terminal["type"], "ltx_complete",
        "{:?}",
        r.ran.terminal
    );
    assert!(!logs.text().contains("SKIPPED_FIXED_GRAPH"));
    assert_eq!(
        r.rig.svc.count("moderate mp4"),
        1,
        "off: only the probe, no check code runs"
    );
    let (logs, _c) = Logs::capture();
    let r = go(Mode::Shadow, HDR, "", &[source()], &[0], None, 712).await;
    assert!(!logs.text().contains("SKIPPED_FIXED_GRAPH"));
    let seen: Vec<String> = r
        .rig
        .svc
        .seen("mp4")
        .into_iter()
        .map(|f| f.sha256)
        .collect();
    assert!(
        seen.contains(&sha(&preview())),
        "shadow still judges the matte"
    );
}
