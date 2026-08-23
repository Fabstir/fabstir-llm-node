// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T4.f: `TrainTask::execute` — wire-frame sequence (stage fidelity through
//! the dataset legs, pointer-before-proof, train_complete/CANCELLED shapes),
//! the liveness heartbeat, and the end-of-run completion + attempt-finish
//! discipline (Completed arms NO cooldown).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use fabstir_llm_node::api::websocket::handlers::training::TrainTask;
use fabstir_llm_node::crypto::decrypt_with_aead;
use fabstir_llm_node::training::accept::{AttemptClaim, AttemptRegistry};
use fabstir_llm_node::training::core::AcceptedSession;
use serde_json::Value;

use super::support::{
    addr, fixture, line, make_deps, model_id, passing_snapshot, CountBehaviour, Harness,
    MockCompleter, MockSessions, ScanBehaviour, ScriptLine, NOW,
};

const SESSION_KEY: [u8; 32] = [9u8; 32];

pub(crate) fn decrypt(envelope: &Value) -> Value {
    let payload = &envelope["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    serde_json::from_slice(&decrypt_with_aead(&ct, &nonce, &aad, &SESSION_KEY).unwrap()).unwrap()
}

pub(crate) fn good_sessions() -> MockSessions {
    MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 0, // completion wait = settle_buffer only (zeroed below)
    }
}

pub(crate) fn slice_line(h: &Harness, index: u64, delay: u64) -> ScriptLine {
    let dir = format!("job-42/slice-{index}");
    let file_dir = h.deps.work_root.join(&dir);
    std::fs::create_dir_all(&file_dir).unwrap();
    std::fs::write(file_dir.join("adapter_model.safetensors"), vec![7u8; 2048]).unwrap();
    line(
        delay,
        &format!(
            r#"{{"event":"slice","index":{index},"stepFrom":0,"stepTo":9,"dir":"{dir}","files":[{{"name":"adapter_model.safetensors","relPath":"{dir}/adapter_model.safetensors","sizeBytes":2048}}]}}"#
        ),
    )
}

pub(crate) fn finalise_line(h: &Harness, delay: u64) -> ScriptLine {
    let dir = "job-42/adapter";
    let file_dir = h.deps.work_root.join(dir);
    std::fs::create_dir_all(&file_dir).unwrap();
    std::fs::write(file_dir.join("adapter_model.safetensors"), vec![8u8; 2048]).unwrap();
    line(
        delay,
        &format!(
            r#"{{"event":"finalise","adapter":{{"dir":"{dir}","files":[{{"name":"adapter_model.safetensors","relPath":"{dir}/adapter_model.safetensors","sizeBytes":2048}}]}}}}"#
        ),
    )
}

pub(crate) fn execute_job() -> fabstir_llm_node::training::types::TrainingJob {
    serde_json::from_value(serde_json::json!({
        "templateId": "train-qlora-synthetic-test-v1",
        "templateHash": format!("0x{}", "ab".repeat(32)),
        "dataset": {
            "manifestCID": "uWILL-BE-REPLACED",
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

/// The scripted sidecar for execute: scan/count/health/status canned + the
/// scripted train stream.
pub(crate) fn spawn_full(
    dir: &std::path::Path,
    script: Vec<ScriptLine>,
    train_busy: bool,
) -> std::path::PathBuf {
    use futures::StreamExt;
    use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
    use hyper::body::{Bytes, Frame};
    use hyper::service::service_fn;
    use hyper::{Response, StatusCode};
    use hyper_util::rt::TokioIo;

    let sock = dir.join("trainer.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind mock UDS");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let script = script.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let script = script.clone();
                    async move {
                        let boxed = |status: StatusCode, body: String| {
                            Response::builder()
                                .status(status)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(body)).boxed())
                                .unwrap()
                        };
                        let response: Response<BoxBody<Bytes, std::convert::Infallible>> =
                            match req.uri().path() {
                                "/v1/scan" => boxed(
                                    StatusCode::OK,
                                    r#"{"verdict":"cleared","policyVersion":"structural-v0"}"#
                                        .to_string(),
                                ),
                                "/v1/count" => {
                                    boxed(StatusCode::OK, r#"{"tokens":9,"samples":3}"#.to_string())
                                }
                                "/v1/train" if train_busy => boxed(
                                    StatusCode::CONFLICT,
                                    r#"{"error":{"kind":"SLOT_BUSY","detail":"synthetic"}}"#
                                        .to_string(),
                                ),
                                "/v1/train" => {
                                    let body_stream = futures::stream::iter(script.into_iter())
                                        .then(|s| async move {
                                            tokio::time::sleep(Duration::from_millis(s.delay_ms))
                                                .await;
                                            Ok::<_, std::convert::Infallible>(Frame::data(
                                                Bytes::from(s.line),
                                            ))
                                        });
                                    Response::builder()
                                        .status(StatusCode::OK)
                                        .header("content-type", "application/x-ndjson")
                                        .body(BodyExt::boxed(StreamBody::new(body_stream)))
                                        .unwrap()
                                }
                                _ => boxed(StatusCode::NOT_FOUND, "{}".to_string()),
                            };
                        Ok::<_, std::convert::Infallible>(response)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    sock
}

pub(crate) struct ExecuteOutcome {
    pub(crate) frames: Vec<Value>,
    pub(crate) completer: Arc<MockCompleter>,
    pub(crate) attempts: Arc<AttemptRegistry>,
    pub(crate) staging_root: std::path::PathBuf,
    pub(crate) work_root: std::path::PathBuf,
}

/// Consume the harness, run execute over the scripted sidecar, return the
/// decrypted frames + the seam handles for assertions.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_execute(
    h: Harness,
    manifest_cid: String,
    manifest_sha: String,
    script: Vec<ScriptLine>,
    schedule: Vec<u64>,
    heartbeat: Duration,
    cancel_after_ms: Option<u64>,
    claim_first: bool,
) -> ExecuteOutcome {
    let snapshot = passing_snapshot();
    run_execute_with_snapshot(
        h,
        manifest_cid,
        manifest_sha,
        script,
        schedule,
        heartbeat,
        cancel_after_ms,
        claim_first,
        snapshot,
        false,
    )
    .await
}

/// As `run_execute`, but with an explicit session snapshot — the completion
/// CREATION floor reads `accepted.snapshot.start_time` against the REAL
/// clock, so a row pinning that floor must supply a real-clock-derived
/// start time (round-2 F1: the constant fixture made the floor unreachable
/// and its pin vacuous).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_execute_with_snapshot(
    mut h: Harness,
    manifest_cid: String,
    manifest_sha: String,
    script: Vec<ScriptLine>,
    schedule: Vec<u64>,
    heartbeat: Duration,
    cancel_after_ms: Option<u64>,
    claim_first: bool,
    snapshot: fabstir_llm_node::training::accept::SessionSnapshot,
    train_busy: bool,
) -> ExecuteOutcome {
    let dir = tempfile::tempdir().unwrap();
    let sock = spawn_full(dir.path(), script, train_busy);
    std::mem::forget(dir);
    std::mem::forget(h.staging_dir);
    std::mem::forget(h.work_dir);
    h.deps.trainer = Arc::new(
        fabstir_llm_node::training::trainer_client::TrainerClient::new(
            sock,
            Duration::from_secs(5),
        ),
    );
    h.deps.settle_buffer_secs = 0; // completion wait 0 in Band A
    let completer = h.completer.clone();
    let attempts = h.deps.attempts.clone();
    let staging_root = h.deps.staging_root.clone();
    let work_root = h.deps.work_root.clone();
    // Point the job's manifest at the fixture's real one (prepare fetches it).
    let mut job = execute_job();

    let depositor = snapshot.depositor;
    let deps = Arc::new(h.deps);
    if claim_first {
        assert_eq!(
            deps.attempts.try_begin(42, depositor, NOW, 60),
            AttemptClaim::Ok
        );
    }
    job.dataset.manifest_cid = manifest_cid;
    job.dataset.manifest_sha256 = manifest_sha;

    let training_tokens: u64 = schedule.iter().sum();
    let accepted = AcceptedSession {
        snapshot,
        training_tokens,
        schedule,
    };
    let task = TrainTask {
        job_id: 42,
        job,
        accepted,
        permit: Arc::new(tokio::sync::Semaphore::new(1))
            .try_acquire_owned()
            .unwrap(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(256);
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(after) = cancel_after_ms {
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(after)).await;
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }
    let collector = tokio::spawn(async move {
        let mut frames = Vec::new();
        while let Some(frame) = rx.recv().await {
            frames.push(decrypt(&frame));
        }
        frames
    });
    task.execute(
        deps,
        SESSION_KEY,
        "ws-1".to_string(),
        None,
        tx,
        cancel,
        heartbeat,
        Duration::from_secs(2),
        NOW,
    )
    .await;
    ExecuteOutcome {
        frames: collector.await.unwrap(),
        completer,
        attempts,
        staging_root,
        work_root,
    }
}

#[tokio::test]
async fn happy_execute_emits_the_full_frame_sequence_and_completes() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        slice_line(&h, 0, 10),
        slice_line(&h, 1, 10),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    let out = run_execute(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![5, 4],
        Duration::from_secs(60),
        None,
        true,
    )
    .await;
    let stages: Vec<String> = out
        .frames
        .iter()
        .filter(|f| f["type"] == "train_progress")
        .filter_map(|f| f["stage"].as_str().map(str::to_string))
        .collect();
    // Stage fidelity through the dataset legs (interface protocol item 2).
    for required in ["staging", "scanning", "counting", "uploading", "finalising"] {
        assert!(
            stages.iter().any(|s| s == required),
            "missing stage {required}: {stages:?}"
        );
    }
    // Pointer-before-proof: the uploading frame for slice 0 precedes its
    // slice-settled frame.
    let uploading_pos = out
        .frames
        .iter()
        .position(|f| f["stage"] == "uploading")
        .expect("uploading frame");
    let slice_pos = out
        .frames
        .iter()
        .position(|f| f["slice"]["index"] == 0)
        .expect("slice frame");
    assert!(
        uploading_pos < slice_pos,
        "checkpoint pointer before its proof"
    );
    // The protocol's checkpoint shape carries sizeBytes (round-1 F4).
    assert_eq!(
        out.frames[uploading_pos]["checkpoint"]["sizeBytes"], 2048,
        "{:?}",
        out.frames[uploading_pos]
    );
    // The terminal frame.
    let complete = out
        .frames
        .iter()
        .find(|f| f["type"] == "train_complete")
        .expect("train_complete");
    assert_eq!(complete["billing"]["unit"], "training-token");
    assert_eq!(complete["billing"]["tokens"], 9);
    assert_eq!(complete["billing"]["pricePerToken"], "904");
    assert_eq!(complete["proofCIDs"].as_array().unwrap().len(), 2);
    assert_eq!(complete["moderation"]["status"], "cleared");
    assert!(complete.get("warnings").is_none());
    // TD15 on Complete: NEITHER root keeps job plaintext (round-1 F5).
    assert!(
        !out.staging_root.join("job-42").exists(),
        "staged plaintext must go"
    );
    assert!(!out.work_root.join("job-42").exists());
    // End-of-run completion ran exactly once…
    assert_eq!(out.completer.count(), 1);
    // …and the attempt finished COMPLETED: consumed forever, NO cooldown.
    assert_eq!(out.attempts.peek(42), AttemptClaim::SessionReused);
    assert_eq!(
        out.attempts.try_begin(43, addr(0xD1), NOW + 1, 60),
        AttemptClaim::Ok,
        "a successful run must not arm the depositor's cooldown"
    );
}

#[tokio::test]
async fn heartbeat_keeps_frames_flowing_through_a_silent_stretch() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        slice_line(&h, 0, 10),
        slice_line(&h, 1, 700), // a 700 ms silent training stretch
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    let out = run_execute(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![5, 4],
        Duration::from_millis(100), // shrunk ≤60 s heartbeat
        None,
        false,
    )
    .await;
    let progress_count = out
        .frames
        .iter()
        .filter(|f| f["type"] == "train_progress")
        .count();
    // Event-driven frames alone: ~9 (3 stage legs + ticks/slices/uploading/
    // finalising); the 700 ms gap at 100 ms cadence must add ≥ 4 heartbeats.
    assert!(
        progress_count >= 13,
        "heartbeat must tick through the silence: {progress_count} progress frames"
    );
}
