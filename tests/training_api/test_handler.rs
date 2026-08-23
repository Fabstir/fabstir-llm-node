// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T3.2: the thin `train` handler — ack/error envelope shapes (fixed AAD
//! `encrypted_train_response`), the A.1 serde surface, TD14 GPU exclusion,
//! and the disabled-host path. Session-level logic itself is `core.rs`'s
//! (matrixed in test_pipeline*); these rows pin the HANDLER's seams.

use std::sync::Arc;

use fabstir_llm_node::api::websocket::handlers::training::handle_encrypted_train;
use fabstir_llm_node::crypto::decrypt_with_aead;
use serde_json::{json, Value};

use super::support::{
    fixture, make_deps, model_id, passing_snapshot, CountBehaviour, MockSessions, ScanBehaviour,
    NOW,
};

const SESSION_KEY: [u8; 32] = [9u8; 32];

fn good_sessions() -> MockSessions {
    MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    }
}

fn decrypt_envelope(envelope: &Value) -> Value {
    assert_eq!(envelope["type"], "encrypted_response", "{envelope}");
    let payload = &envelope["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    assert_eq!(
        aad, b"encrypted_train_response",
        "the fixed per-handler AAD"
    );
    let pt = decrypt_with_aead(&ct, &nonce, &aad, &SESSION_KEY).expect("decrypts");
    serde_json::from_slice(&pt).unwrap()
}

fn train_action(fx_job: &fabstir_llm_node::training::types::TrainingJob) -> Value {
    let mut v = serde_json::to_value(fx_job).unwrap();
    v["action"] = json!("train");
    v
}

#[tokio::test]
async fn happy_ack_carries_verified_billing_and_schedule_and_holds_the_permit() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(h.deps);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let mut action = train_action(&fx.job);
    action["requestId"] = json!("req-7");

    let (envelope, task) = handle_encrypted_train(
        Some(deps.clone()),
        semaphore.clone(),
        3, // allowListVersion
        &action,
        &SESSION_KEY,
        "ws-session-1",
        Some(500),
        Some(&json!(11)),
        NOW,
    )
    .await;
    let inner = decrypt_envelope(&envelope);
    assert_eq!(inner["type"], "train_accepted");
    assert_eq!(inner["status"], "processing");
    assert_eq!(inner["sessionId"], 500);
    assert_eq!(inner["allowListVersion"], 3);
    assert_eq!(inner["billing"]["unit"], "training-token");
    assert_eq!(inner["billing"]["tokens"], 9);
    assert_eq!(inner["billing"]["pricePerToken"], "904");
    assert_eq!(inner["schedule"]["sliceTokens"], 1_000_000);
    assert_eq!(inner["schedule"]["slices"], 1);
    assert_eq!(inner["requestId"], "req-7");
    assert_eq!(envelope["id"], 11, "message id echoed on the envelope");

    let task = task.expect("acceptance returns the task");
    assert_eq!(task.job_id, 500);
    assert_eq!(task.accepted.schedule, vec![9]);
    // TD14: the task HOLDS the only GPU permit.
    assert!(semaphore.clone().try_acquire_owned().is_err());
    drop(task);
    assert!(
        semaphore.try_acquire_owned().is_ok(),
        "permit released with the task"
    );
}

#[tokio::test]
async fn missing_job_id_and_disabled_host_reject_without_consuming() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(h.deps);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let action = train_action(&fx.job);

    let (envelope, task) = handle_encrypted_train(
        Some(deps.clone()),
        semaphore.clone(),
        1,
        &action,
        &SESSION_KEY,
        "ws",
        None, // no chain job id
        None,
        NOW,
    )
    .await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&envelope);
    assert_eq!(inner["type"], "train_error");
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");

    let (envelope2, task2) = handle_encrypted_train(
        None, // training disabled
        semaphore.clone(),
        1,
        &action,
        &SESSION_KEY,
        "ws",
        Some(501),
        None,
        NOW,
    )
    .await;
    assert!(task2.is_none());
    let inner2 = decrypt_envelope(&envelope2);
    assert_eq!(inner2["error"]["code"], "SIDECAR_UNAVAILABLE");
    // Neither path consumed the permit.
    assert!(semaphore.try_acquire_owned().is_ok());
}

#[tokio::test]
async fn null_numeric_member_fails_the_a1_wire_rule_at_the_handler() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(h.deps);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let mut action = train_action(&fx.job);
    action["dataset"]["declaredTokens"] = Value::Null;

    let (envelope, task) = handle_encrypted_train(
        Some(deps),
        semaphore,
        1,
        &action,
        &SESSION_KEY,
        "ws",
        Some(502),
        None,
        NOW,
    )
    .await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&envelope);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
    assert!(
        inner["error"]["message"]
            .as_str()
            .unwrap()
            .contains("validation"),
        "{inner}"
    );
}

#[tokio::test]
async fn gpu_busy_is_capacity_and_consumes_the_session_per_c3() {
    // Realigned: the TD14 permit is taken AFTER acceptance; a busy GPU is a
    // consuming CAPACITY reject (sessions are host-bound — an unsettled
    // capacity reject would strand the deposit until timeout).
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(h.deps);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let held = semaphore.clone().try_acquire_owned().unwrap(); // LTX holds the GPU
    let action = train_action(&fx.job);

    let (envelope, task) = handle_encrypted_train(
        Some(deps.clone()),
        semaphore.clone(),
        1,
        &action,
        &SESSION_KEY,
        "ws",
        Some(503),
        None,
        NOW,
    )
    .await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&envelope);
    assert_eq!(inner["error"]["code"], "CAPACITY");
    // Round-9 F-R9-3: the round-8 discriminator landed here unpinned — setting
    // this back to None left the whole suite green. A GPU-busy reject is
    // funded, consumed and zero-completed, so an SDK reading a reasonless
    // CAPACITY as retry-safe would retry a session the node has completed.
    assert_eq!(inner["error"]["detail"]["reason"], "slotBusy");
    // The session is consumed + its zero-settle scheduled.
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(h.completer.calls.lock().unwrap().as_slice(), &[503]);
    drop(held);
    let (envelope2, task2) = handle_encrypted_train(
        Some(deps),
        semaphore,
        1,
        &action,
        &SESSION_KEY,
        "ws",
        Some(503),
        None,
        NOW + 60,
    )
    .await;
    assert!(
        task2.is_none(),
        "the consumed session must not accept again"
    );
    let inner2 = decrypt_envelope(&envelope2);
    assert_eq!(inner2["error"]["detail"]["reason"], "sessionReused");
}

#[tokio::test]
async fn a3_reject_reason_flows_to_the_error_detail() {
    let fx = fixture(None).await;
    let mut sessions = good_sessions();
    if let Ok(snap) = &mut sessions.snapshot {
        snap.deposit = ethers::types::U256::zero();
    }
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let (envelope, task) = handle_encrypted_train(
        Some(Arc::new(h.deps)),
        Arc::new(tokio::sync::Semaphore::new(1)),
        1,
        &train_action(&fx.job),
        &SESSION_KEY,
        "ws",
        Some(504),
        None,
        NOW,
    )
    .await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&envelope);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
    assert_eq!(inner["error"]["detail"]["reason"], "sessionParams");
}

/// T5.3 round-9 F-R9-1 (LIVE): the A.1 serde surface echoed the entire
/// offending client string back to the client.
///
/// `serde` renders `Unexpected::Str` with NO truncation, so any wire member of
/// the wrong JSON type came straight back. Measured at 200,066 bytes from a
/// 200 KB input, and the socket sets no `max_message_size`, so the ceiling was
/// tungstenite's 64 MiB default; the node then multiplied it through the
/// format, the json, the encrypt and two hex encodes.
///
/// This is the FIRST gate a malformed `train` hits: before both chain reads,
/// before the attempt claim, before any funding is verified. It is the
/// shortest path in the whole surface to the amplifier `redact.rs` exists to
/// stop, and it survived three rounds of sweeping for exactly this class.
#[tokio::test]
async fn a_malformed_train_job_does_not_echo_the_whole_offending_string() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(h.deps);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let mut action = train_action(&fx.job);
    // A wire member of the wrong JSON type, 200 KB of it.
    action["epochs"] = json!("A".repeat(200_000));

    let (envelope, task) = handle_encrypted_train(
        Some(deps.clone()),
        semaphore.clone(),
        3,
        &action,
        &SESSION_KEY,
        "ws-session-1",
        Some(500),
        Some(&json!(11)),
        NOW,
    )
    .await;
    assert!(task.is_none(), "a malformed job must not spawn work");
    let inner = decrypt_envelope(&envelope);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED", "{inner}");
    let message = inner["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.len() < 1024,
        "the reject echoed {} bytes back to the client; it must be bounded",
        message.len()
    );
    // Bounding must not destroy diagnosability. serde puts the diagnosis at
    // the TAIL ("expected u32") and omits the field name entirely, so a
    // head-only truncation would keep 96 bytes of the attacker's padding and
    // throw away the only actionable part.
    assert!(
        message.contains("expected u32"),
        "the bound kept the noise and dropped the diagnosis: {message}"
    );
}
