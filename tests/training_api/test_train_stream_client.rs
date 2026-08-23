// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T4.a: the client side of the §3.5 held NDJSON train stream — event
//! parsing, pre-stream envelopes, the in-band terminal, the died-stream
//! case (§3.7's raw material for the k-split), and the 120 s silence rule.

use std::time::Duration;

use fabstir_llm_node::training::trainer_client::{
    SidecarFailure, TrainStreamEvent, TrainWireRequest, TrainerClient,
};

use super::support::{line, spawn_train_sidecar, ScriptLine, TrainBehaviour};

const SILENCE: Duration = Duration::from_millis(400);

fn wire_body() -> TrainWireRequest {
    serde_json::from_value(serde_json::json!({
        "jobId": 42,
        "datasetPath": "/staging/job-42/dataset.jsonl",
        "declaredTokens": 9,
        "epochs": 1,
        "hyper": { "rank": 16, "alpha": 32, "lr": "0.000200", "seed": "13", "seqLen": 2048 }
    }))
    .map(|v: serde_json::Value| TrainWireRequest {
        job_id: v["jobId"].as_u64().unwrap(),
        dataset_path: v["datasetPath"].as_str().unwrap().to_string(),
        declared_tokens: v["declaredTokens"].as_u64().unwrap(),
        epochs: v["epochs"].as_u64().unwrap() as u32,
        hyper: serde_json::from_value(v["hyper"].clone()).unwrap(),
    })
    .unwrap()
}

fn happy_script() -> Vec<ScriptLine> {
    vec![
        line(0, r#"{"event":"tick","stage":"loading"}"#),
        line(10, r#"{"event":"tick","stage":"training"}"#),
        line(
            10,
            r#"{"event":"slice","index":0,"stepFrom":0,"stepTo":9,"dir":"job-42/slice-0","files":[{"name":"adapter_model.safetensors","relPath":"job-42/slice-0/adapter_model.safetensors","sizeBytes":8192}]}"#,
        ),
        line(
            10,
            r#"{"event":"finalise","adapter":{"dir":"job-42/adapter","files":[{"name":"adapter_model.safetensors","relPath":"job-42/adapter/adapter_model.safetensors","sizeBytes":16384},{"name":"adapter.gguf","relPath":"job-42/adapter/adapter.gguf","sizeBytes":128}]}}"#,
        ),
        line(10, r#"{"event":"done"}"#),
    ]
}

async fn open(
    behaviour: TrainBehaviour,
) -> Result<fabstir_llm_node::training::trainer_client::TrainStream, SidecarFailure> {
    let dir = tempfile::tempdir().unwrap();
    let sock = spawn_train_sidecar(dir.path(), behaviour);
    // Keep the tempdir alive for the test's duration by leaking it (the
    // socket path must outlive the stream).
    std::mem::forget(dir);
    let client = TrainerClient::new(sock, Duration::from_secs(5));
    client.train(&wire_body(), SILENCE).await
}

#[tokio::test]
async fn happy_stream_parses_every_event_then_ends_cleanly() {
    let mut stream = open(TrainBehaviour::Script(happy_script()))
        .await
        .expect("opens");
    let mut events = Vec::new();
    while let Some(event) = stream.next_event().await.expect("stream stays healthy") {
        events.push(event);
    }
    assert!(matches!(events[0], TrainStreamEvent::Tick { ref stage, .. } if stage == "loading"));
    let slice = events
        .iter()
        .find_map(|e| match e {
            TrainStreamEvent::Slice {
                index,
                step_to,
                dir,
                files,
                ..
            } => Some((*index, *step_to, dir.clone(), files.len())),
            _ => None,
        })
        .expect("slice event parsed");
    assert_eq!(slice, (0, 9, "job-42/slice-0".to_string(), 1));
    let finalise = events
        .iter()
        .find_map(|e| match e {
            TrainStreamEvent::Finalise {
                files, warnings, ..
            } => Some((files.len(), warnings.len())),
            _ => None,
        })
        .expect("finalise event parsed");
    assert_eq!(finalise, (2, 0));
    assert!(matches!(events.last(), Some(TrainStreamEvent::Done)));
    // Post-done reads keep answering None, never an error.
    assert!(matches!(stream.next_event().await, Ok(None)));
}

#[tokio::test]
async fn pre_stream_slot_busy_is_a_409_envelope() {
    match open(TrainBehaviour::Busy409).await.unwrap_err() {
        SidecarFailure::Envelope { status, kind, .. } => {
            assert_eq!((status, kind.as_str()), (409, "SLOT_BUSY"));
        }
        other => panic!("expected envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn pre_stream_bounds_is_a_400_envelope() {
    match open(TrainBehaviour::Bounds400).await.unwrap_err() {
        SidecarFailure::Envelope { status, kind, .. } => {
            assert_eq!((status, kind.as_str()), (400, "TEMPLATE_BOUNDS"));
        }
        other => panic!("expected envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn in_band_terminal_line_surfaces_as_a_200_envelope_after_prior_events() {
    let script = vec![
        line(0, r#"{"event":"tick","stage":"loading"}"#),
        line(
            10,
            r#"{"event":"slice","index":0,"stepFrom":0,"stepTo":9,"dir":"job-42/slice-0","files":[]}"#,
        ),
        line(
            10,
            r#"{"error":{"kind":"TRAIN_FAILURE","detail":"synthetic explosion"}}"#,
        ),
    ];
    let mut stream = open(TrainBehaviour::Script(script)).await.expect("opens");
    assert!(matches!(
        stream.next_event().await,
        Ok(Some(TrainStreamEvent::Tick { .. }))
    ));
    assert!(matches!(
        stream.next_event().await,
        Ok(Some(TrainStreamEvent::Slice { index: 0, .. }))
    ));
    match stream.next_event().await.unwrap_err() {
        SidecarFailure::Envelope {
            status,
            kind,
            detail,
        } => {
            assert_eq!((status, kind.as_str()), (200, "TRAIN_FAILURE"));
            assert!(detail.contains("synthetic"), "{detail}");
        }
        other => panic!("expected terminal envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn stream_death_without_terminal_is_transport() {
    // The body ends after slice 0 with neither `done` nor a terminal line —
    // the §3.7 died-stream case (k = 1 here; the CALLER holds k).
    let script = vec![
        line(0, r#"{"event":"tick","stage":"loading"}"#),
        line(
            10,
            r#"{"event":"slice","index":0,"stepFrom":0,"stepTo":9,"dir":"job-42/slice-0","files":[]}"#,
        ),
    ];
    let mut stream = open(TrainBehaviour::Script(script)).await.expect("opens");
    assert!(matches!(stream.next_event().await, Ok(Some(_))));
    assert!(matches!(stream.next_event().await, Ok(Some(_))));
    match stream.next_event().await.unwrap_err() {
        SidecarFailure::Transport(detail) => {
            assert!(detail.contains("without a terminal"), "{detail}")
        }
        other => panic!("expected transport, got {other:?}"),
    }
}

#[tokio::test]
async fn silence_beyond_the_watchdog_is_transport() {
    // 600 ms gap vs a 400 ms watchdog: the read must fail with the silence
    // rule, not hang.
    let script = vec![
        line(0, r#"{"event":"tick","stage":"loading"}"#),
        line(600, r#"{"event":"done"}"#),
    ];
    let mut stream = open(TrainBehaviour::Script(script)).await.expect("opens");
    assert!(matches!(stream.next_event().await, Ok(Some(_))));
    match stream.next_event().await.unwrap_err() {
        SidecarFailure::Transport(detail) => assert!(detail.contains("silence"), "{detail}"),
        other => panic!("expected silence transport, got {other:?}"),
    }
}

#[tokio::test]
async fn garbage_line_is_transport() {
    let script = vec![line(0, "not json at all")];
    let mut stream = open(TrainBehaviour::Script(script)).await.expect("opens");
    assert!(matches!(
        stream.next_event().await,
        Err(SidecarFailure::Transport(_))
    ));
}

#[test]
fn wire_request_serialises_to_the_exact_contract_shape() {
    // The sidecar is extra-forbid: any field drift 422s live. Pin the bytes.
    let value = serde_json::to_value(wire_body()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "jobId": 42,
            "datasetPath": "/staging/job-42/dataset.jsonl",
            "declaredTokens": 9,
            "epochs": 1,
            "hyper": { "rank": 16, "alpha": 32, "lr": "0.000200", "seed": "13", "seqLen": 2048 }
        })
    );
}
