// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D19: a transcode whose client left keeps its result, encrypted at rest,
//! for the owner to fetch on a new connection; the fetch never bills.

use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::websocket::handlers::transcode_result::handle_transcode_result;
use fabstir_llm_node::moderation::types::{ModerationResult, Verdict};
use fabstir_llm_node::transcoder::result_store::{
    result_access, ResultAccess, ResultRecord, ResultSlot, ResultState, ResultStore, INTERRUPTED,
};
use serde_json::{json, Value};
use std::sync::Arc;

const OWNER: &str = "0xAbCdEf0000000000000000000000000000000001";
const OTHER: &str = "0x9999999999999999999999999999999999999999";
const DAY: u64 = 86_400;
const T0: u64 = 1_800_000_000;

fn key() -> [u8; 32] {
    ResultStore::derive_key(&[7u8; 32])
}

fn open(dir: &std::path::Path, now: u64) -> Arc<ResultStore> {
    ResultStore::open(dir, key(), 7 * DAY, 100, now).unwrap()
}

fn complete_payload() -> Value {
    json!({
        "type": "transcode_complete",
        "taskId": "task-1",
        "outputs": [{"id": 1, "cid": "u-secret-output-cid"}],
        "billing": {"units": 1.5, "tokens": 1500, "provenTokens": 1500},
    })
}

fn record(state: ResultState, owner: Option<&str>, stored_at: u64) -> ResultRecord {
    ResultRecord {
        job_id: 1433,
        task_id: "task-1".into(),
        owner: owner.map(|o| o.to_ascii_lowercase()),
        stored_at,
        state,
    }
}

// ── the pure fetch decision ──────────────────────────────────────────────

#[test]
fn only_the_pinned_owner_is_served() {
    let done = record(
        ResultState::Done {
            payload: complete_payload(),
        },
        Some(OWNER),
        T0,
    );
    let never_held = || None;
    assert_eq!(
        result_access(Some(&done), Some(OWNER), T0, 7 * DAY, never_held),
        ResultAccess::Serve(complete_payload())
    );
    // Case does not matter: addresses arrive checksummed or lowercase.
    assert_eq!(
        result_access(
            Some(&done),
            Some(&OWNER.to_ascii_uppercase().replace("0X", "0x")),
            T0,
            7 * DAY,
            never_held
        ),
        ResultAccess::Serve(complete_payload())
    );
    assert_eq!(
        result_access(Some(&done), Some(OTHER), T0, 7 * DAY, never_held),
        ResultAccess::NotOwner
    );
    assert_eq!(
        result_access(Some(&done), None, T0, 7 * DAY, never_held),
        ResultAccess::NotOwner
    );
    let unowned = record(
        ResultState::Done {
            payload: complete_payload(),
        },
        None,
        T0,
    );
    assert_eq!(
        result_access(Some(&unowned), Some(OWNER), T0, 7 * DAY, never_held),
        ResultAccess::NotOwner
    );
}

#[test]
fn a_non_owner_learns_nothing_about_the_state() {
    for state in [
        ResultState::InFlight,
        ResultState::Failed {
            code: "BILLING_FAILED".into(),
            message: "x".into(),
        },
    ] {
        let r = record(state, Some(OWNER), T0);
        assert_eq!(
            result_access(Some(&r), Some(OTHER), T0 + 30 * DAY, 7 * DAY, || None),
            ResultAccess::NotOwner
        );
    }
}

#[test]
fn states_and_retention() {
    let none = || None;
    assert_eq!(
        result_access(None, Some(OWNER), T0, 7 * DAY, none),
        ResultAccess::NotFound
    );
    let running = record(ResultState::InFlight, Some(OWNER), T0);
    // A run in flight is pending...
    assert_eq!(
        result_access(Some(&running), Some(OWNER), T0 + 6 * DAY, 7 * DAY, none),
        ResultAccess::Pending
    );
    // ...but no run lasts days: one that old lost its final write, and expires.
    assert_eq!(
        result_access(Some(&running), Some(OWNER), T0 + 7 * DAY, 7 * DAY, none),
        ResultAccess::Expired
    );
    let failed = record(
        ResultState::Failed {
            code: "SESSION_NOT_BILLABLE".into(),
            message: "stopped".into(),
        },
        Some(OWNER),
        T0,
    );
    assert_eq!(
        result_access(Some(&failed), Some(OWNER), T0, 7 * DAY, none),
        ResultAccess::Failed {
            code: "SESSION_NOT_BILLABLE".into(),
            message: "stopped".into()
        }
    );
    let done = record(
        ResultState::Done {
            payload: complete_payload(),
        },
        Some(OWNER),
        T0,
    );
    assert_eq!(
        result_access(Some(&done), Some(OWNER), T0 + 7 * DAY - 1, 7 * DAY, none),
        ResultAccess::Serve(complete_payload())
    );
    assert_eq!(
        result_access(Some(&done), Some(OWNER), T0 + 7 * DAY, 7 * DAY, none),
        ResultAccess::Expired
    );
    assert_eq!(
        result_access(Some(&failed), Some(OWNER), T0 + 7 * DAY, 7 * DAY, none),
        ResultAccess::Expired
    );
}

#[test]
fn a_held_job_is_never_served() {
    let done = record(
        ResultState::Done {
            payload: complete_payload(),
        },
        Some(OWNER),
        T0,
    );
    assert_eq!(
        result_access(Some(&done), Some(OWNER), T0, 7 * DAY, || Some((
            "CONTENT_BLOCKED".into(),
            "blocked".into()
        ))),
        ResultAccess::Held {
            code: "CONTENT_BLOCKED".into(),
            message: "blocked".into()
        }
    );
}

// ── the store ────────────────────────────────────────────────────────────

#[test]
fn a_record_round_trips_and_keeps_its_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), T0);
    store.begin(1433, "task-1", Some(OWNER.into()), T0).unwrap();
    let r = store.get(1433, "task-1").unwrap();
    assert_eq!(r.state, ResultState::InFlight);
    assert_eq!(
        r.owner.as_deref(),
        Some(OWNER.to_ascii_lowercase().as_str())
    );
    store
        .finish(
            1433,
            "task-1",
            Some(OWNER.into()),
            ResultState::Done {
                payload: complete_payload(),
            },
            T0 + 60,
        )
        .unwrap();
    let r = store.get(1433, "task-1").unwrap();
    assert_eq!(
        r.state,
        ResultState::Done {
            payload: complete_payload()
        }
    );
    assert_eq!(r.stored_at, T0 + 60);
    assert!(store.get(1433, "task-2").is_none());
    assert!(store.get(1434, "task-1").is_none());
}

#[test]
fn records_are_encrypted_at_rest() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), T0);
    store.begin(1433, "task-1", Some(OWNER.into()), T0).unwrap();
    store
        .finish(
            1433,
            "task-1",
            Some(OWNER.into()),
            ResultState::Done {
                payload: complete_payload(),
            },
            T0,
        )
        .unwrap();
    let mut files = 0;
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        assert!(!text.contains("u-secret-output-cid"));
        assert!(!text.contains(&OWNER.to_ascii_lowercase()[2..]));
        assert!(!text.contains("transcode_complete"));
        files += 1;
    }
    assert_eq!(files, 1);
}

#[cfg(unix)]
#[test]
fn the_directory_and_records_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("results");
    let store = open(&sub, T0);
    store.begin(1433, "task-1", Some(OWNER.into()), T0).unwrap();
    let mode = std::fs::metadata(&sub).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    for entry in std::fs::read_dir(&sub).unwrap() {
        let mode = entry.unwrap().metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn another_host_key_reads_nothing() {
    let dir = tempfile::tempdir().unwrap();
    open(dir.path(), T0)
        .begin(1433, "task-1", Some(OWNER.into()), T0)
        .unwrap();
    assert_ne!(
        ResultStore::derive_key(&[7u8; 32]),
        ResultStore::derive_key(&[8u8; 32])
    );
    let other = ResultStore::open(
        dir.path(),
        ResultStore::derive_key(&[8u8; 32]),
        7 * DAY,
        100,
        T0,
    )
    .unwrap();
    assert!(other.get(1433, "task-1").is_none());
}

#[test]
fn a_record_copied_onto_another_name_does_not_decrypt() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), T0);
    store.begin(1433, "victim", Some(OTHER.into()), T0).unwrap();
    store.begin(1433, "mine", Some(OWNER.into()), T0).unwrap();
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 2);
    // Swap the two files' bytes: neither may be read under the other's name.
    let a = std::fs::read(&files[0]).unwrap();
    let b = std::fs::read(&files[1]).unwrap();
    std::fs::write(&files[0], &b).unwrap();
    std::fs::write(&files[1], &a).unwrap();
    assert!(store.get(1433, "victim").is_none());
    assert!(store.get(1433, "mine").is_none());
}

#[test]
fn a_restart_marks_runs_in_flight_interrupted_and_reports_their_jobs() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = open(dir.path(), T0);
        store.begin(1433, "task-1", Some(OWNER.into()), T0).unwrap();
        store.begin(1500, "task-9", Some(OWNER.into()), T0).unwrap();
        store
            .finish(
                1501,
                "done",
                Some(OWNER.into()),
                ResultState::Done {
                    payload: complete_payload(),
                },
                T0,
            )
            .unwrap();
    }
    let store = open(dir.path(), T0 + 100);
    assert_eq!(store.interrupted_jobs(), vec![1433, 1500]);
    match store.get(1433, "task-1").unwrap().state {
        ResultState::Failed { code, .. } => assert_eq!(code, INTERRUPTED),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(matches!(
        store.get(1501, "done").unwrap().state,
        ResultState::Done { .. }
    ));
}

#[test]
fn records_expire_at_open_and_on_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), T0);
    store
        .finish(
            1,
            "old",
            Some(OWNER.into()),
            ResultState::Done {
                payload: complete_payload(),
            },
            T0,
        )
        .unwrap();
    store
        .finish(
            2,
            "newer",
            Some(OWNER.into()),
            ResultState::Done {
                payload: complete_payload(),
            },
            T0 + 3 * DAY,
        )
        .unwrap();
    store
        .begin(3, "running", Some(OWNER.into()), T0 + 3 * DAY)
        .unwrap();
    store.begin(4, "lost", Some(OWNER.into()), T0).unwrap();
    assert_eq!(store.sweep(T0 + 7 * DAY), 2);
    assert!(store.get(1, "old").is_none());
    assert!(store.get(2, "newer").is_some());
    assert!(store.get(3, "running").is_some());
    assert!(store.get(4, "lost").is_none(), "a week-old run is gone");
    drop(store);
    // At open, after the second record's retention.
    let store = open(dir.path(), T0 + 10 * DAY);
    assert!(store.get(2, "newer").is_none());
}

#[test]
fn the_cap_evicts_the_oldest_finished_and_refuses_when_all_are_running() {
    let dir = tempfile::tempdir().unwrap();
    let store = ResultStore::open(dir.path(), key(), 7 * DAY, 3, T0).unwrap();
    for (i, at) in [(1u64, T0 + 10), (2, T0 + 5)] {
        store
            .finish(
                i,
                "t",
                Some(OWNER.into()),
                ResultState::Done {
                    payload: complete_payload(),
                },
                at,
            )
            .unwrap();
    }
    store.begin(3, "t", Some(OWNER.into()), T0).unwrap();
    // Full: the oldest finished (job 2) makes room.
    store.begin(4, "t", Some(OWNER.into()), T0).unwrap();
    assert!(store.get(2, "t").is_none());
    assert!(store.get(1, "t").is_some());
    store.begin(5, "t", Some(OWNER.into()), T0).unwrap();
    assert!(store.get(1, "t").is_none());
    assert_eq!(store.len(), 3);
    // Three runs in flight and nothing finished to evict: refused.
    assert!(store.begin(6, "t", Some(OWNER.into()), T0).is_err());
    assert_eq!(store.len(), 3);
}

#[test]
fn a_slot_dropped_without_an_outcome_records_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), T0);
    store.begin(1433, "task-1", Some(OWNER.into()), T0).unwrap();
    drop(ResultSlot::new(
        store.clone(),
        1433,
        "task-1".into(),
        Some(OWNER.into()),
    ));
    match store.get(1433, "task-1").unwrap().state {
        ResultState::Failed { code, .. } => assert_eq!(code, INTERRUPTED),
        other => panic!("expected Failed, got {other:?}"),
    }
    // An outcome written once is not overwritten by the drop.
    let mut slot = ResultSlot::new(store.clone(), 1433, "task-2".into(), Some(OWNER.into()));
    slot.done(complete_payload());
    drop(slot);
    let r = store.get(1433, "task-2").unwrap();
    assert_eq!(
        r.state,
        ResultState::Done {
            payload: complete_payload()
        }
    );
    assert_eq!(
        r.owner.as_deref(),
        Some(OWNER.to_ascii_lowercase().as_str())
    );
}

#[test]
fn an_unwritable_directory_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, b"x").unwrap();
    assert!(ResultStore::open(&file, key(), 7 * DAY, 100, T0).is_err());
}

// ── the fetch, end to end through the handler ────────────────────────────

const FETCH_KEY: [u8; 32] = [0x42; 32];

fn decrypt(envelope: &Value) -> Value {
    assert_eq!(envelope["type"], "encrypted_response", "{envelope}");
    let p = &envelope["payload"];
    let ct = hex::decode(p["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(p["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(p["aadHex"].as_str().unwrap()).unwrap();
    let plain = fabstir_llm_node::crypto::decrypt_with_aead(&ct, &nonce, &aad, &FETCH_KEY).unwrap();
    serde_json::from_slice(&plain).unwrap()
}

fn server_with_done_result(dir: &std::path::Path) -> ApiServer {
    let server = ApiServer::new_for_test();
    let store = ResultStore::open(dir, key(), 7 * DAY, 100, 0).unwrap();
    store.begin(1433, "task-1", Some(OWNER.into()), 0).unwrap();
    let mut slot = ResultSlot::new(store.clone(), 1433, "task-1".into(), Some(OWNER.into()));
    slot.done(complete_payload());
    drop(slot);
    server.set_transcode_result_store(Some(store));
    server
}

fn fetch(server: &ApiServer, job: Option<u64>, signer: Option<&str>, task: &str) -> Value {
    decrypt(&handle_transcode_result(
        server,
        &json!({"action": "transcode_result", "taskId": task}),
        &FETCH_KEY,
        "1433",
        job,
        signer,
        Some(&json!("msg-7")),
    ))
}

#[test]
fn the_owner_fetches_the_stored_completion_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_done_result(dir.path());
    let envelope = handle_transcode_result(
        &server,
        &json!({"action": "transcode_result", "taskId": "task-1"}),
        &FETCH_KEY,
        "1433",
        Some(1433),
        Some(&OWNER.to_ascii_lowercase()),
        Some(&json!("msg-7")),
    );
    assert_eq!(envelope["id"], "msg-7");
    assert_eq!(decrypt(&envelope), complete_payload());
}

#[test]
fn a_non_owner_is_refused_and_sees_no_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_done_result(dir.path());
    let answer = fetch(&server, Some(1433), Some(OTHER), "task-1");
    assert_eq!(answer["type"], "transcode_error");
    assert_eq!(answer["taskId"], "task-1");
    assert_eq!(answer["error"]["code"], "NOT_OWNER");
    assert!(!answer.to_string().contains("u-secret-output-cid"));
    // No signer at all (a connection with no encrypted init) is not the owner.
    assert_eq!(
        fetch(&server, Some(1433), None, "task-1")["error"]["code"],
        "NOT_OWNER"
    );
}

#[test]
fn not_found_pending_and_missing_task() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_done_result(dir.path());
    let store = server.transcode_result_store().unwrap();
    assert_eq!(
        fetch(&server, Some(1434), Some(OWNER), "task-1")["error"]["code"],
        "RESULT_NOT_FOUND"
    );
    assert_eq!(
        fetch(&server, None, Some(OWNER), "task-1")["error"]["code"],
        "RESULT_NOT_FOUND"
    );
    assert_eq!(
        fetch(&server, Some(1433), Some(OWNER), "")["error"]["code"],
        "VALIDATION_FAILED"
    );
    store
        .begin(
            1433,
            "running",
            Some(OWNER.into()),
            fabstir_llm_node::transcoder::result_store::unix_now(),
        )
        .unwrap();
    let pending = fetch(&server, Some(1433), Some(OWNER), "running");
    assert_eq!(pending["type"], "transcode_result_pending");
    assert_eq!(pending["taskId"], "running");
    store
        .finish(
            1433,
            "failed",
            Some(OWNER.into()),
            ResultState::Failed {
                code: "BILLING_FAILED".into(),
                message: "the final proof did not land".into(),
            },
            fabstir_llm_node::transcoder::result_store::unix_now(),
        )
        .unwrap();
    let failed = fetch(&server, Some(1433), Some(OWNER), "failed");
    assert_eq!(failed["error"]["code"], "BILLING_FAILED");
}

#[test]
fn an_expired_result_answers_expired_once_then_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let server = ApiServer::new_for_test();
    let store = ResultStore::open(dir.path(), key(), 7 * DAY, 100, 0).unwrap();
    // Finished eight days ago (the store's clock for the fetch is the wall clock).
    let eight_days_ago = fabstir_llm_node::transcoder::result_store::unix_now() - 8 * DAY;
    store
        .finish(
            1433,
            "task-1",
            Some(OWNER.into()),
            ResultState::Done {
                payload: complete_payload(),
            },
            eight_days_ago,
        )
        .unwrap();
    server.set_transcode_result_store(Some(store));
    assert_eq!(
        fetch(&server, Some(1433), Some(OWNER), "task-1")["error"]["code"],
        "RESULT_EXPIRED"
    );
    assert_eq!(
        fetch(&server, Some(1433), Some(OWNER), "task-1")["error"]["code"],
        "RESULT_NOT_FOUND"
    );
}

#[test]
fn a_held_job_is_not_served_at_fetch_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = server_with_done_result(dir.path());
    server.set_moderation_enforce_for_test(true);
    server.moderation_store().set(
        1433,
        ModerationResult {
            verdict: Verdict::Blocked,
            reason: Some("blocked after completion".into()),
            report_id: None,
        },
    );
    let answer = fetch(&server, Some(1433), Some(OWNER), "task-1");
    assert_eq!(answer["type"], "transcode_error");
    assert_eq!(answer["error"]["code"], "CONTENT_BLOCKED");
    assert!(!answer.to_string().contains("u-secret-output-cid"));
    // Cleared: served.
    server
        .moderation_store()
        .set(1433, ModerationResult::cleared());
    assert_eq!(
        fetch(&server, Some(1433), Some(OWNER), "task-1"),
        complete_payload()
    );
}

#[tokio::test]
async fn a_fetch_never_bills_or_takes_work() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_with_done_result(dir.path());
    for _ in 0..3 {
        fetch(&server, Some(1433), Some(OWNER), "task-1");
    }
    assert!(server
        .transcoding_tracker()
        .get_job_info(1433)
        .await
        .is_none());
    let left = server.job_work().connection_closed(1433, "probe");
    assert_eq!(left.work_in_flight, 0);
    assert!(!server.job_work().is_closing(1433));
}

#[test]
fn no_store_answers_unavailable() {
    let server = ApiServer::new_for_test();
    assert_eq!(
        fetch(&server, Some(1433), Some(OWNER), "task-1")["error"]["code"],
        "RESULT_STORE_UNAVAILABLE"
    );
}
