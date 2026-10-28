// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D15: completion keyed to the job. A finished, billed transcode on a
//! connection that never hangs up (session 1433) is completed after a grace
//! period; work ending with no live connection completes at once.

use fabstir_llm_node::api::job_work::{
    finish_action, Completer, ConnectionJob, FinishAction, JobState, JobWork, WorkRefusal,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const GRACE: Duration = Duration::from_millis(80);

fn recording() -> (Completer, Arc<Mutex<Vec<u64>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sink = calls.clone();
    let completer: Completer = Arc::new(move |job_id| sink.lock().unwrap().push(job_id));
    (completer, calls)
}

fn completions(calls: &Arc<Mutex<Vec<u64>>>) -> Vec<u64> {
    calls.lock().unwrap().clone()
}

#[test]
fn the_finish_rule() {
    assert_eq!(
        finish_action(1, 0, false, true),
        None,
        "work still in flight"
    );
    assert_eq!(
        finish_action(0, 0, true, false),
        Some(FinishAction::CompleteNow),
        "nobody left: complete, billed or not"
    );
    assert_eq!(
        finish_action(0, 1, false, true),
        Some(FinishAction::CompleteAfterGrace)
    );
    assert_eq!(
        finish_action(0, 1, true, true),
        None,
        "never under a live chat/LTX/training connection"
    );
    assert_eq!(
        finish_action(0, 1, false, false),
        None,
        "an unbilled transcode on a live connection waits for the disconnect"
    );
}

#[tokio::test]
async fn the_1433_shape_completes_after_the_grace_period() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(1433, "conn");
    let guard = work.begin(1433).expect("accepted");
    guard.mark_billed();
    drop(guard);
    assert!(
        completions(&calls).is_empty(),
        "not before the grace period"
    );
    tokio::time::sleep(GRACE * 3).await;
    assert_eq!(completions(&calls), vec![1433]);
    assert!(work.is_closing(1433));
}

#[tokio::test]
async fn new_work_inside_the_grace_period_cancels_the_completion() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(7, "conn");
    let first = work.begin(7).unwrap();
    first.mark_billed();
    drop(first);
    let second = work
        .begin(7)
        .expect("still accepting during the grace period");
    tokio::time::sleep(GRACE * 3).await;
    assert!(
        completions(&calls).is_empty(),
        "the first timer must not complete under the second unit"
    );
    drop(second);
    assert!(
        completions(&calls).is_empty(),
        "unbilled second unit: no timer"
    );
}

#[tokio::test]
async fn a_job_that_carried_other_work_is_never_completed_by_the_timer() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(8, "conn");
    work.note_other_work(8);
    let guard = work.begin(8).unwrap();
    guard.mark_billed();
    drop(guard);
    tokio::time::sleep(GRACE * 3).await;
    assert!(completions(&calls).is_empty());
}

#[tokio::test]
async fn work_ending_with_no_live_connection_completes_at_once() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(9, "conn");
    let guard = work.begin(9).unwrap();
    let state = work.connection_closed(9, "conn");
    assert_eq!(
        state,
        JobState {
            work_in_flight: 1,
            other_connections: 0
        }
    );
    drop(guard); // held, failed or unbilled: still settled, never stranded
    assert_eq!(completions(&calls), vec![9]);
    assert_eq!(work.begin(9).err(), Some(WorkRefusal::Closing));
}

#[tokio::test]
async fn a_closing_job_refuses_new_work() {
    let (completer, _calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(10, "conn");
    let guard = work.begin(10).unwrap();
    guard.mark_billed();
    drop(guard);
    tokio::time::sleep(GRACE * 3).await;
    assert_eq!(work.begin(10).err(), Some(WorkRefusal::Closing));
}

#[tokio::test]
async fn another_live_connection_keeps_the_job_open() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(11, "chat");
    work.connection_opened(11, "transcode");
    let guard = work.begin(11).unwrap();
    let state = work.connection_closed(11, "transcode");
    assert_eq!(state.other_connections, 1);
    drop(guard);
    assert!(
        completions(&calls).is_empty(),
        "the chat socket still carries the job"
    );
}

#[tokio::test]
async fn a_panicking_task_still_releases_its_unit() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    let guard = work.begin(12).unwrap();
    let task = tokio::spawn(async move {
        let _guard = guard;
        panic!("transcode task died");
    });
    assert!(task.await.is_err());
    assert_eq!(completions(&calls), vec![12], "no connection: completed");
}

#[tokio::test]
async fn a_later_failed_unit_does_not_erase_the_billed_state() {
    // Review finding: an unbilled second unit cancelled the billed first one's
    // timer and nothing re-armed it, stranding the 1433 shape again.
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(13, "conn");
    let first = work.begin(13).unwrap();
    first.mark_billed();
    drop(first);
    drop(work.begin(13).unwrap()); // e.g. a retry that failed before starting
    tokio::time::sleep(GRACE * 3).await;
    assert_eq!(completions(&calls), vec![13]);
}

#[tokio::test]
async fn the_timer_measures_from_the_last_unit_not_the_first() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(14, "conn");
    let first = work.begin(14).unwrap();
    first.mark_billed();
    drop(first);
    tokio::time::sleep(GRACE * 7 / 10).await;
    drop(work.begin(14).unwrap());
    tokio::time::sleep(GRACE / 2).await;
    assert!(
        completions(&calls).is_empty(),
        "the first unit's timer must not fire after a later unit"
    );
    tokio::time::sleep(GRACE * 2).await;
    assert_eq!(completions(&calls), vec![14]);
}

#[tokio::test]
async fn chat_arriving_during_the_grace_period_cancels_the_completion() {
    // Review finding (HIGH): the timer completed the session under a user who
    // started chatting after the transcode finished.
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(15, "conn");
    let guard = work.begin(15).unwrap();
    guard.mark_billed();
    drop(guard);
    tokio::time::sleep(GRACE / 3).await;
    work.note_other_work(15);
    tokio::time::sleep(GRACE * 3).await;
    assert!(completions(&calls).is_empty());
}

#[tokio::test]
async fn a_dropped_registration_deregisters_the_connection() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    work.connection_opened(16, "chat");
    let mut registration = ConnectionJob::new(work.clone(), "transcode".into());
    registration.set(Some(16));
    let guard = work.begin(16).unwrap();
    drop(registration); // e.g. the handler panicked
    assert_eq!(work.connection_closed(16, "chat").other_connections, 0);
    drop(guard);
    assert_eq!(
        completions(&calls),
        vec![16],
        "nothing left carrying the job"
    );
}

#[tokio::test]
async fn moving_a_connection_to_another_job_reports_the_old_one() {
    let (completer, calls) = recording();
    let work = JobWork::new(completer, GRACE);
    let mut registration = ConnectionJob::new(work.clone(), "conn".into());
    assert_eq!(registration.set(Some(17)), None);
    let (old, left) = registration.set(Some(18)).expect("left job 17");
    assert_eq!(old, 17);
    assert_eq!(
        left,
        JobState {
            work_in_flight: 0,
            other_connections: 0
        }
    );
    assert!(work.complete_if_unowned(17));
    assert!(!work.complete_if_unowned(17), "only once");
    assert!(!work.complete_if_unowned(18), "job 18 is still carried");
    assert_eq!(completions(&calls), vec![17]);
}
