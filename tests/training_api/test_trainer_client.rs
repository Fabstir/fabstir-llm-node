// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `TrainerClient` transport matrix against a REAL Unix-socket mock sidecar
//! (CONTRACT §1/§3.1–§3.4/§3.6): success shapes, both envelope classes
//! (4xx pre-attempt and 200-with-envelope), framework 422, and the
//! transport-death class (no socket / dropped connection / hang / garbage) —
//! the §3.7 `SIDECAR_UNAVAILABLE` raw material. The mock serves hyper http1
//! over `tokio::net::UnixListener`, mirroring the sidecar's uvicorn-on-UDS.

use std::path::PathBuf;
use std::time::Duration;

use fabstir_llm_node::training::trainer_client::{SidecarFailure, TrainerClient};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::TokioIo;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Healthy sidecar: happy JSON on every endpoint.
    Normal,
    /// `/v1/count` answers the 400 `DATASET_MALFORMED` envelope.
    CountMalformed,
    /// `/v1/count` answers the 200 `SOURCE_MUTATED` envelope (§3.6's
    /// attempt-produced-no-result class — MUST NOT parse as success).
    CountMutated,
    /// `/v1/scan` answers the 200 `SCAN_FAILURE` envelope.
    ScanFailure,
    /// Framework 422 (FastAPI `{"detail": …}` shape, no §3.6 envelope).
    Framework422,
    /// 200 with an unparseable body (a diverged sidecar).
    Garbage,
    /// Accept the connection, then close it before any response.
    DropConnection,
    /// Accept, read, never respond (client timeout must fire).
    Hang,
}

fn canned(mode: Mode, path: &str) -> (StatusCode, String) {
    let envelope = |status: StatusCode, kind: &str| {
        (
            status,
            format!(r#"{{"error":{{"kind":"{kind}","detail":"synthetic {kind}"}}}}"#),
        )
    };
    match (mode, path) {
        (Mode::CountMalformed, "/v1/count") => envelope(StatusCode::BAD_REQUEST, "DATASET_MALFORMED"),
        (Mode::CountMutated, "/v1/count") => envelope(StatusCode::OK, "SOURCE_MUTATED"),
        (Mode::ScanFailure, "/v1/scan") => envelope(StatusCode::OK, "SCAN_FAILURE"),
        (Mode::Framework422, _) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            r#"{"detail":[{"loc":["body"],"msg":"field required"}]}"#.to_string(),
        ),
        (Mode::Garbage, _) => (StatusCode::OK, "not json at all".to_string()),
        (_, "/v1/health") => (
            StatusCode::OK,
            r#"{"status":"ok","pins":{"templateHash":"0xt","tokenizerSha256":"0xk","stackDigest":"sha256:s"}}"#
                .to_string(),
        ),
        (_, "/v1/status") => (StatusCode::OK, r#"{"slot":"free"}"#.to_string()),
        (_, "/v1/count") => (StatusCode::OK, r#"{"tokens":9,"samples":3}"#.to_string()),
        (_, "/v1/scan") => (
            StatusCode::OK,
            r#"{"verdict":"cleared","policyVersion":"structural-v0"}"#.to_string(),
        ),
        _ => (StatusCode::NOT_FOUND, "{}".to_string()),
    }
}

/// Bind a mock sidecar on a fresh socket under `dir`; serves until dropped.
fn spawn_mock(dir: &std::path::Path, mode: Mode) -> PathBuf {
    let sock = dir.join("trainer.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind mock UDS");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            if mode == Mode::DropConnection {
                drop(stream);
                continue;
            }
            if mode == Mode::Hang {
                tokio::spawn(async move {
                    // Hold the stream open, never answer.
                    let _stream = stream;
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                });
                continue;
            }
            tokio::spawn(async move {
                let service = service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| async move {
                        let (status, body) = canned(mode, req.uri().path());
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(status)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(body)))
                                .unwrap(),
                        )
                    },
                );
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    sock
}

fn client(sock: PathBuf) -> TrainerClient {
    TrainerClient::new(sock, Duration::from_secs(5))
}

#[tokio::test]
async fn health_status_count_scan_happy_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::Normal));
    let pins = c.health().await.expect("health");
    assert_eq!(pins.template_hash, "0xt");
    assert_eq!(pins.stack_digest, "sha256:s");
    assert_eq!(c.status().await.expect("status").slot, "free");
    let count = c
        .count("/staging/job-1/dataset.jsonl")
        .await
        .expect("count");
    assert_eq!((count.tokens, count.samples), (9, 3));
    let scan = c.scan("/staging/job-1/dataset.jsonl").await.expect("scan");
    assert_eq!(scan.verdict, "cleared");
    assert_eq!(scan.policy_version, "structural-v0");
}

#[tokio::test]
async fn count_400_envelope_is_typed_not_transport() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::CountMalformed));
    match c.count("/staging/x").await.unwrap_err() {
        SidecarFailure::Envelope { status, kind, .. } => {
            assert_eq!((status, kind.as_str()), (400, "DATASET_MALFORMED"));
        }
        other => panic!("expected envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn count_200_envelope_must_not_parse_as_success() {
    // §3.6's attempt-failure class rides HTTP 200 — the envelope check must
    // run BEFORE the success parse or SOURCE_MUTATED becomes garbage.
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::CountMutated));
    match c.count("/staging/x").await.unwrap_err() {
        SidecarFailure::Envelope { status, kind, .. } => {
            assert_eq!((status, kind.as_str()), (200, "SOURCE_MUTATED"));
        }
        other => panic!("expected 200 envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn scan_200_failure_envelope_is_typed() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::ScanFailure));
    match c.scan("/staging/x").await.unwrap_err() {
        SidecarFailure::Envelope { kind, .. } => assert_eq!(kind, "SCAN_FAILURE"),
        other => panic!("expected envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn framework_422_is_a_typed_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::Framework422));
    match c.count("/staging/x").await.unwrap_err() {
        SidecarFailure::Envelope { status, kind, .. } => {
            assert_eq!((status, kind.as_str()), (422, "FRAMEWORK_422"));
        }
        other => panic!("expected 422 envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn garbage_success_body_is_transport_class() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::Garbage));
    match c.count("/staging/x").await.unwrap_err() {
        SidecarFailure::Transport(detail) => assert!(detail.contains("unparseable"), "{detail}"),
        other => panic!("expected transport, got {other:?}"),
    }
}

#[tokio::test]
async fn missing_socket_is_transport_class() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path().join("never-bound.sock"));
    match c.health().await.unwrap_err() {
        SidecarFailure::Transport(detail) => assert!(detail.contains("connect"), "{detail}"),
        other => panic!("expected transport, got {other:?}"),
    }
}

#[tokio::test]
async fn dropped_connection_is_transport_class() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(spawn_mock(dir.path(), Mode::DropConnection));
    assert!(matches!(
        c.count("/staging/x").await.unwrap_err(),
        SidecarFailure::Transport(_)
    ));
}

#[tokio::test]
async fn hung_sidecar_hits_the_client_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let sock = spawn_mock(dir.path(), Mode::Hang);
    let c = TrainerClient::new(sock, Duration::from_millis(200));
    let start = std::time::Instant::now();
    // The OUTER guard proves the client cannot hang past its own deadline.
    let result = tokio::time::timeout(Duration::from_secs(5), c.count("/staging/x"))
        .await
        .expect("client must not outlive its own timeout");
    match result.unwrap_err() {
        SidecarFailure::Transport(detail) => assert!(detail.contains("timeout"), "{detail}"),
        other => panic!("expected timeout transport, got {other:?}"),
    }
    assert!(start.elapsed() < Duration::from_secs(2));
}
