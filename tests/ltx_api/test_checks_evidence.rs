// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks fixes F7, F8 and F13 at the checker: an item that should have been sealed is never deleted when
//! sealing fails; an unknown `BLOCK_*` word refuses and alerts; result-stage refusals are counted node-wide; and a
//! client's session id cannot shape a log line, an alert or a record. Each test names the mutation that turns it
//! red.

use super::checks_svc::*;
use super::relight_stub::mp4_clip;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{safe_id, CONTENT_REFUSED, MSG_REFUSED, RESULT_REFUSALS_ALERT};
use fabstir_llm_node::ltx::provenance::SELF_TEST_MP4;
use std::os::unix::fs::PermissionsExt;

const ILLEGAL: Reply = Reply::Verdict("BLOCK_ILLEGAL", Some("csam_suspected"));

fn set_mode(path: &std::path::Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A rig whose evidence directory cannot take a new file (so sealing fails). `unsealed_ok` leaves the fallback
/// folder in place and writable.
async fn rig_with_failing_seal(unsealed_ok: bool) -> Rig {
    let rig = rig(Mode::Enforce).await;
    let evidence = rig.dir("evidence");
    if unsealed_ok {
        // the fallback folder exists already; without it, it cannot be created in a read-only parent
        std::fs::create_dir(evidence.join("unsealed")).unwrap();
    }
    std::fs::write(evidence.join("refusals.jsonl"), b"").unwrap();
    set_mode(&evidence, 0o500);
    rig
}

/// Give the temporary folder back its permissions so it can be removed.
fn restore(rig: &Rig) {
    set_mode(&rig.dir("evidence"), 0o700);
}

#[tokio::test]
async fn a_failed_seal_keeps_the_item_instead_of_deleting_the_only_copy() {
    // fix F8. Dropping the staged copy when sealing fails -> no file in unsealed/ -> red
    let rig = rig_with_failing_seal(true).await;
    rig.svc
        .spare_probe
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.svc.set_media("mp4", 0, ILLEGAL);
    let clip = mp4_clip(121);
    let mut job = rig.checks.job("s-keep", Some(301), None);
    let r = rig.checks.check_clip(&mut job, &clip).await.unwrap_err();
    assert_eq!(
        (r.code, r.message.as_str()),
        (CONTENT_REFUSED, MSG_REFUSED),
        "the job is refused all the same"
    );
    let kept: Vec<_> = std::fs::read_dir(rig.dir("evidence").join("unsealed"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1, "the item is kept, in the owner-only folder");
    assert_eq!(std::fs::read(&kept[0]).unwrap(), clip);
    assert_eq!(
        std::fs::metadata(&kept[0]).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(kept[0].extension().is_some_and(|e| e == "mp4"));
    assert!(
        rig.files_in("staging").is_empty(),
        "and only then does the staging copy go"
    );
    let alerts = rig.alerts.all();
    assert!(
        alerts.iter().any(|(k, d)| k == "seal-failed"
            && d.contains("job=301")
            && d.contains("kept=unsealed")),
        "{alerts:?}"
    );
    assert_eq!(rig.alert_count("sealed"), 0);
    let rec = rig.refusals();
    assert_eq!(
        (&rec[0]["sealed"], &rec[0]["unsealed"]),
        (&serde_json::json!(false), &serde_json::json!("unsealed"))
    );
    restore(&rig);
}

#[tokio::test]
async fn with_nowhere_to_write_the_staged_copy_is_left_in_place() {
    // fix F8. Deleting the staged copy when even the fallback cannot be written -> staging is empty -> red
    let rig = rig_with_failing_seal(false).await;
    rig.svc
        .spare_probe
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.svc.set_media("mp4", 0, ILLEGAL);
    let clip = mp4_clip(121);
    let mut job = rig.checks.job("s-keep", Some(302), None);
    let r = rig.checks.check_clip(&mut job, &clip).await.unwrap_err();
    assert_eq!(r.code, CONTENT_REFUSED);
    let staged = rig.files_in("staging");
    assert_eq!(staged.len(), 1, "the only copy stays where it is");
    assert_eq!(
        std::fs::read(rig.dir("staging").join(&staged[0])).unwrap(),
        clip
    );
    assert!(rig
        .alerts
        .all()
        .iter()
        .any(|(k, d)| k == "seal-failed" && d.contains("kept=staging")));
    restore(&rig);
}

#[tokio::test]
async fn an_unknown_block_word_refuses_and_alerts() {
    // fix F13. No alert on a BLOCK word outside the wire addendum -> a service blocking everything goes
    // unnoticed -> red; an unknown BLOCK word treated as a service fault -> CHECK_UNAVAILABLE -> red
    let rig = rig(Mode::Enforce).await;
    rig.svc
        .spare_probe
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.svc
        .set_media("mp4", 0, Reply::Verdict("BLOCK_EVERYTHING_V3", None));
    let mut job = rig.checks.job("s-word", Some(303), None);
    let r = rig
        .checks
        .check_clip(&mut job, &mp4_clip(121))
        .await
        .unwrap_err();
    assert_eq!((r.code, r.message.as_str()), (CONTENT_REFUSED, MSG_REFUSED));
    let alerts = rig.alerts.all();
    assert!(
        alerts.iter().any(|(k, d)| k == "unknown-verdict"
            && d.contains("verdict=BLOCK_EVERYTHING_V")
            && d.contains("stage=c3")),
        "{alerts:?}"
    );
    assert!(!rig.checks.breaker_open());
    // a known word does not alert
    rig.svc.set_media(
        "mp4",
        0,
        Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit")),
    );
    rig.checks
        .check_clip(&mut job, &mp4_clip(121))
        .await
        .unwrap_err();
    assert_eq!(rig.alert_count("unknown-verdict"), 1);
    // a text BLOCK_ILLEGAL whose stats are damaged is still the strongest signal: refused AND sealed
    let damaged =
        serde_json::json!({"verdict": "BLOCK_ILLEGAL", "category": "csam_suspected", "stats": {}});
    rig.svc.set_text(Reply::Raw(200, damaged));
    let r = rig
        .checks
        .check_prompt(&mut job, "any words", false)
        .await
        .unwrap_err();
    assert_eq!(r.code, CONTENT_REFUSED);
    assert_eq!(rig.alert_count("sealed"), 1);
}

#[tokio::test]
async fn result_refusals_are_counted_across_addresses_and_alert_at_five_an_hour() {
    // fix F7. Counting per address only -> five addresses, one refusal each, raise nothing -> red
    let rig = rig(Mode::Enforce).await;
    rig.svc.set_media(
        "mp4",
        1, // call 0 was the start-up probe
        Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit")),
    );
    for n in 1..=RESULT_REFUSALS_ALERT {
        // a fresh address every time: the per-address cool-down never sees a second refusal
        let address = format!("0x{n:040x}");
        assert!(rig.checks.pre_accept(Some(&address), "s").is_ok());
        let mut job = rig
            .checks
            .job(&format!("s-{n}"), Some(n as u64), Some(&address));
        let r = rig
            .checks
            .check_result(&mut job, "LTX_00001.mp4", SELF_TEST_MP4)
            .await
            .unwrap_err();
        assert_eq!(r.code, CONTENT_REFUSED);
        assert_eq!(
            rig.alert_count("result-refusals"),
            (n == RESULT_REFUSALS_ALERT) as usize,
            "after {n} refusal(s)"
        );
        assert!(!rig.checks.cooldown().paused(&address));
    }
    assert_eq!(rig.alert_count("repeated-refusals"), 0);
    assert!(
        rig.checks.pre_accept(Some("0xnew"), "s").is_ok(),
        "nobody else is throttled"
    );
    // an input-stage refusal is not a render given away: it does not move this count
    let mut job = rig.checks.job("s-clip", Some(99), Some("0xclip"));
    rig.checks
        .check_clip(&mut job, &mp4_clip(121))
        .await
        .unwrap_err();
    assert_eq!(rig.alert_count("result-refusals"), 1);
}

#[tokio::test]
async fn a_session_id_cannot_shape_logs_alerts_or_records() {
    // fix F7. The raw session id used anywhere -> the injected text appears -> red
    let (logs, _capture) = Logs::capture();
    let evil = "s1 verdict=PASS\n{\"sealed\":true} lighthouse/../../x";
    assert_eq!(
        safe_id(evil),
        "s1_verdict_PASS___sealed__true__lighthouse_______x"
    );
    assert_eq!(safe_id(&"a".repeat(200)).len(), 64);
    assert_eq!(safe_id("job-12_AB"), "job-12_AB");
    let rig = rig(Mode::Enforce).await;
    rig.svc
        .spare_probe
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.svc.set_media("mp4", 0, ILLEGAL);
    let mut job = rig.checks.job(evil, Some(304), None);
    rig.checks
        .check_clip(&mut job, &mp4_clip(121))
        .await
        .unwrap_err();
    let rec = rig.refusals();
    assert_eq!(rec[0]["session"], safe_id(evil));
    let alert_text: String = rig
        .alerts
        .all()
        .iter()
        .map(|(k, d)| format!("{k} {d}\n"))
        .collect();
    for (place, text) in [("log", logs.text()), ("alerts", alert_text)] {
        assert!(text.contains(&safe_id(evil)), "{place} names the session");
        assert!(
            !text.contains("{\"sealed\"") && !text.contains("verdict=PASS\n"),
            "{place} carries the raw session id"
        );
    }
    // with no client address the cool-down keys on the SANITISED session id
    for _ in 0..2 {
        rig.checks
            .check_clip(&mut job, &mp4_clip(121))
            .await
            .unwrap_err();
    }
    assert!(rig.checks.pre_accept(None, evil).is_err(), "paused");
    assert!(rig.checks.pre_accept(None, "another-session").is_ok());
}
