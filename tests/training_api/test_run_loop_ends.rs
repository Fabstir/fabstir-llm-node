// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Split of `test_run_loop.rs` (the 400-line integration-file rule): the
//! failure/end-state rows; helpers live in the sibling module.

use std::time::Duration;

use fabstir_llm_node::training::core::RunEnd;

use super::support::{fixture, line, make_deps, CountBehaviour, ScanBehaviour};
use super::test_run_loop::{drive, drive_with_schedule, finalise_line, good_sessions, slice_line};

#[tokio::test]
async fn stream_death_after_one_settled_slice_is_train_failed() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![slice_line(&h, 0, 0, 0, 5)]; // then the body ENDS
    let (end, _) = drive(&h, script, None).await;
    let RunEnd::Failed {
        code,
        billing,
        last_checkpoint,
        ..
    } = end
    else {
        panic!("expected Failed, got {end:?}");
    };
    assert_eq!(code, "TRAIN_FAILED", "k = 1 → money moved");
    assert_eq!(billing.settled_slices, 1);
    assert!(last_checkpoint.is_some(), "the client keeps the pointer");
    // k ≥ 1 must NOT zero-settle (the proofs settle the session).
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(h.completer.count(), 0);
}

#[tokio::test]
async fn stream_death_with_zero_slices_is_sidecar_unavailable_and_zero_settles() {
    use fabstir_llm_node::training::accept::AttemptClaim;
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![line(0, r#"{"event":"tick","stage":"loading"}"#)]; // then ENDS
    let (end, _) = drive(&h, script, None).await;
    let RunEnd::Failed { code, billing, .. } = end else {
        panic!("expected Failed, got {end:?}");
    };
    assert_eq!(
        code, "SIDECAR_UNAVAILABLE",
        "k = 0 → nothing moved, re-shoppable class"
    );
    assert_eq!(billing.settled_slices, 0);
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.completer.count(), 1, "k = 0 runs the C.3 zero-settle");
    assert_eq!(
        h.deps.attempts.peek(42),
        AttemptClaim::SessionReused,
        "session consumed"
    );
}

#[tokio::test]
async fn cancel_aborts_at_the_next_slice_boundary() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        slice_line(&h, 0, 0, 0, 3),
        slice_line(&h, 1, 500, 3, 6), // NON-final; arrives well after the cancel
        slice_line(&h, 2, 10, 6, 9),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    // The cancel fires at 200 ms — AFTER slice 0 is fully settled, while the
    // loop is PARKED awaiting slice 1 (a NON-final slice, arriving at
    // 500 ms): only the slice-ARRIVAL boundary check can honour the cancel
    // without settling slice 1 (a FINAL slice would be stashed unsettled
    // either way, hiding the check — the first row version proved that).
    let (end, _) = drive_with_schedule(&h, script, Some(200), vec![3, 3, 3]).await;
    let RunEnd::Cancelled {
        billing,
        last_checkpoint,
    } = end
    else {
        panic!("expected Cancelled, got {end:?}");
    };
    assert_eq!(billing.settled_slices, 1, "completed slices settle");
    assert_eq!(billing.billed_tokens, 3, "the aborted slice does not bill");
    assert!(last_checkpoint.is_some());
}

#[tokio::test]
async fn diverged_slice_indexes_fail_closed_without_panicking() {
    // Out-of-bounds and duplicate indexes are handled failures — never a
    // panic in the permit-holding task, never a double-bill (T4 round F2/F6).
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    // Duplicate: index 0 twice.
    let script = vec![
        slice_line(&h, 0, 0, 0, 5),
        slice_line(&h, 0, 10, 0, 5),
        finalise_line(&h, 10),
    ];
    let (end, _) = drive(&h, script, None).await;
    let RunEnd::Failed {
        code,
        detail,
        billing,
        ..
    } = end
    else {
        panic!("expected Failed, got {end:?}");
    };
    assert_eq!(code, "TRAIN_FAILED", "k = 1 landed before the divergence");
    assert!(detail.contains("diverged sidecar"), "{detail}");
    assert_eq!(
        billing.settled_slices, 1,
        "the duplicate must NOT re-settle"
    );

    // Out-of-bounds: index 7 with a 2-slice schedule — must not panic.
    let fx2 = fixture(None).await;
    let h2 = make_deps(
        &fx2,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script2 = vec![{
        let dir = "job-42/slice-7";
        std::fs::create_dir_all(h2.deps.work_root.join(dir)).unwrap();
        line(
            0,
            &format!(
                r#"{{"event":"slice","index":7,"stepFrom":0,"stepTo":9,"dir":"{dir}","files":[]}}"#
            ),
        )
    }];
    let (end2, _) = drive(&h2, script2, None).await;
    let RunEnd::Failed {
        code: code2,
        detail: detail2,
        ..
    } = end2
    else {
        panic!("expected Failed, got {end2:?}");
    };
    assert_eq!(code2, "SIDECAR_UNAVAILABLE", "k = 0");
    assert!(detail2.contains("diverged sidecar"), "{detail2}");
}

#[tokio::test]
async fn a_sequential_slice_after_the_final_one_fails_closed() {
    // Round-2 F4: the guard's SECOND clause (index >= total_slices) is
    // load-bearing on its own — a sequential slice arriving AFTER the final
    // one has index == slices_seen == total, so clause 1 passes it through
    // and `schedule[index]` would PANIC the permit-holding task.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    // One-slice schedule: slice 0 is FINAL (stashed, unsettled), then a
    // sequential slice 1 arrives — out of bounds.
    let script = vec![slice_line(&h, 0, 0, 0, 9), slice_line(&h, 1, 10, 9, 9)];
    let (end, _) = drive_with_schedule(&h, script, None, vec![9]).await;
    let RunEnd::Failed {
        code,
        detail,
        billing,
        ..
    } = end
    else {
        panic!("expected Failed, got {end:?}");
    };
    assert_eq!(
        code, "SIDECAR_UNAVAILABLE",
        "k = 0: the final slice never settled"
    );
    assert!(detail.contains("diverged sidecar"), "{detail}");
    assert_eq!(billing.settled_slices, 0);
}
