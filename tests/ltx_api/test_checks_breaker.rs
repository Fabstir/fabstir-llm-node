// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks, the breaker's two residual rules. A service that fails real items while passing the tiny probe
//! clip is caught by counting failures in a row across jobs; and a dead socket alerts when the breaker opens, not
//! once per check. Each test names the mutation that turns it red.

use super::checks_svc::*;
use super::relight_stub::mp4_clip;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{CHECK_UNAVAILABLE, MSG_BREAKER};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn broken_on_real_clips() -> Reply {
    let e = json!({"error": {"kind": "PIPELINE_FAILURE", "detail": "CUDA out of memory"}});
    Reply::Raw(200, e)
}

async fn clip(rig: &Rig, n: u64) -> Option<&'static str> {
    let mut job = rig.checks.job(&format!("s-{n}"), Some(n), None);
    rig.checks
        .check_clip(&mut job, &mp4_clip(121))
        .await
        .err()
        .map(|r| r.code)
}

#[tokio::test]
async fn three_failures_in_a_row_open_the_breaker_though_the_probe_passes() {
    // no count -> the breaker never opens -> red; no reset on a verdict, or input faults counted -> it opens two
    // checks early -> red; no waiting period -> the passing probe closes it at once -> red
    let rig = rig_with(Mode::Enforce, true, |c| {
        c.breaker_hold = Duration::from_millis(1500)
    })
    .await;
    // real clips fail, the node's own probe clip passes: the service looks healthy to every probe
    rig.svc.spare_probe.store(true, Ordering::SeqCst);
    rig.svc.set_media("mp4", 0, broken_on_real_clips());
    for n in [1, 2] {
        assert_eq!(clip(&rig, n).await, Some(CHECK_UNAVAILABLE));
    }
    assert!(!rig.checks.breaker_open(), "two in a row: not yet");
    // a real verdict in between (a pass here; a block is a verdict too) starts the count again
    *rig.svc.media_rule.lock().unwrap() = None;
    assert_eq!(clip(&rig, 3).await, None);
    rig.svc.set_media("mp4", 0, broken_on_real_clips());
    for n in [4, 5] {
        assert_eq!(clip(&rig, n).await, Some(CHECK_UNAVAILABLE));
    }
    assert!(!rig.checks.breaker_open(), "the pass reset the count");
    // files only their own job is to blame for never count
    let unsupported = json!({"error": {"kind": "UNSUPPORTED_INPUT", "detail": "undecodable"}});
    rig.svc.set_media("mp4", 0, Reply::Raw(422, unsupported));
    for n in [6, 7, 8] {
        assert_eq!(clip(&rig, n).await, Some("VALIDATION_FAILED"));
    }
    assert!(!rig.checks.breaker_open(), "input faults are not counted");
    assert_eq!(rig.alert_count("breaker-open"), 0);
    // the third service failure in a row
    rig.svc.set_media("mp4", 0, broken_on_real_clips());
    let probes = rig.svc.count("probe-text");
    assert_eq!(clip(&rig, 9).await, Some(CHECK_UNAVAILABLE));
    assert!(rig.checks.breaker_open(), "three in a row");
    assert_eq!(rig.alert_count("breaker-open"), 1);
    let reason = rig.alerts.all();
    let reason = &reason.iter().find(|(k, _)| k == "breaker-open").unwrap().1;
    assert!(reason.contains("3 checks in a row"), "{reason}");
    assert_eq!(
        rig.checks.pre_accept(None, "s-next").unwrap_err().message,
        MSG_BREAKER
    );
    assert_eq!(
        rig.svc.count("probe-text"),
        probes,
        "the third failure does not ask the probe"
    );
    // the probe passes straight away, and the breaker stays open for the waiting period all the same
    assert!(
        !rig.checks.probe_now().await,
        "held open whatever the probe says"
    );
    assert!(rig.svc.count("probe-text") > probes, "the probe did run");
    assert!(rig.checks.breaker_open());
    tokio::time::sleep(Duration::from_millis(1700)).await;
    assert!(
        rig.checks.probe_now().await,
        "after the wait a passing probe closes it"
    );
    assert!(rig.checks.pre_accept(None, "s-next").is_ok());
    // and the count starts from nothing: two more failures do not reopen it
    for n in [10, 11] {
        assert_eq!(clip(&rig, n).await, Some(CHECK_UNAVAILABLE));
    }
    assert!(!rig.checks.breaker_open());
}

#[tokio::test]
async fn a_dead_socket_alerts_when_the_breaker_opens_not_on_every_check() {
    // the alert sent whenever the socket is unreachable -> one per check -> red
    let rig = rig(Mode::Shadow).await;
    rig.svc.stop();
    // shadow refuses nothing, so jobs keep arriving while the service is down
    for n in 1..=4 {
        assert_eq!(clip(&rig, n).await, None, "shadow lets the job through");
    }
    assert!(rig.checks.breaker_open());
    assert_eq!(
        (
            rig.alert_count("breaker-open"),
            rig.alert_count("check-unavailable"),
            rig.alert_count("check-unavailable-Unreachable"),
        ),
        (1, 1, 1),
        "once for the opening, then the rate-limited kind (once in five minutes)"
    );
}
