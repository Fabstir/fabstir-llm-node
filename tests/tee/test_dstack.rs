// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P2.3 — the thin dstack guest-agent client, against an in-process
//! fake agent on a Unix socket. Proves the transport, the request shape
//! (`report_data` as hex), the reply decoding (hex quote, `tcb_info` as a JSON
//! string inside `/Info`), and the fail-closed paths. The real agent is gate
//! A-4 (simulator, then the CPU CVM); this is what lets A-4 be a confirmation.

use fabstir_llm_node::tee::dstack::{DstackClient, Endpoint};
use fabstir_llm_node::tee::types::{TeeError, REPORT_DATA_LEN};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A fake guest agent: `handler(path, body_json) -> (status, body_json)`.
type Handler = Arc<dyn Fn(&str, serde_json::Value) -> (StatusCode, String) + Send + Sync>;

/// Removes the fake agent's directory (socket included) when the test ends.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn spawn_fake_agent(handler: Handler) -> (PathBuf, Scratch) {
    let dir = std::env::temp_dir().join(format!(
        "tee-dstack-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let scratch = Scratch(dir.clone());
    let sock = dir.join("dstack.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let json: serde_json::Value =
                            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                        let (status, out) = handler(&path, json);
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(status)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(out)))
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    (sock, scratch)
}

fn rand_suffix() -> String {
    use rand::RngCore;
    let mut b = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

fn client(sock: PathBuf) -> DstackClient {
    DstackClient::new(Endpoint::Unix(sock), Duration::from_secs(5))
}

const RD: [u8; REPORT_DATA_LEN] = [0xA5u8; REPORT_DATA_LEN];

#[tokio::test]
async fn get_quote_sends_hex_report_data_and_decodes_the_reply() {
    let (sock, _scratch) = spawn_fake_agent(Arc::new(|path, body| {
        assert_eq!(path, "/GetQuote");
        assert_eq!(body["report_data"].as_str().unwrap(), hex::encode(RD));
        (
            StatusCode::OK,
            serde_json::json!({
                "quote": "0x0400020081000000",
                "event_log": "[{\"imr\":3,\"event_type\":134217729,\"digest\":\"ab\",\"event\":\"compose-hash\",\"event_payload\":\"cd\"}]",
                "report_data": hex::encode(RD),
                "vm_config": "{\"cpu_count\":4,\"memory_size\":8589934592,\"num_gpus\":1}"
            })
            .to_string(),
        )
    }));
    let q = client(sock).get_quote(&RD).await.expect("get_quote");
    assert_eq!(
        q.quote,
        vec![0x04, 0x00, 0x02, 0x00, 0x81, 0x00, 0x00, 0x00]
    );
    assert_eq!(q.report_data, RD.to_vec());
    assert!(q.event_log.contains("compose-hash"));
    assert!(q.vm_config.contains("\"num_gpus\":1"));
}

#[tokio::test]
async fn get_quote_refuses_a_quote_made_for_other_report_data() {
    // An agent that echoes a report_data other than ours produced a quote for
    // someone else's identity/nonce; the client refuses it before it can be
    // packaged as evidence.
    let (sock, _scratch) = spawn_fake_agent(Arc::new(|_, _| {
        (
            StatusCode::OK,
            serde_json::json!({
                "quote": "0400",
                "event_log": "[]",
                "report_data": hex::encode([0x5Au8; 64]),
                "vm_config": ""
            })
            .to_string(),
        )
    }));
    match client(sock).get_quote(&RD).await {
        Err(TeeError::Dstack(msg)) => assert!(msg.contains("report_data"), "{msg}"),
        other => panic!("expected Dstack error, got {other:?}"),
    }
}

#[tokio::test]
async fn get_quote_refuses_an_empty_or_non_hex_quote() {
    let (sock, _scratch) = spawn_fake_agent(Arc::new(|_, _| {
        (
            StatusCode::OK,
            serde_json::json!({"quote": "not-hex", "event_log": "[]"}).to_string(),
        )
    }));
    assert!(matches!(
        client(sock).get_quote(&RD).await,
        Err(TeeError::Dstack(_))
    ));
}

#[tokio::test]
async fn non_200_is_a_dstack_error_with_the_status() {
    let (sock, _scratch) = spawn_fake_agent(Arc::new(|_, _| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "quote generation failed".into(),
        )
    }));
    match client(sock).get_quote(&RD).await {
        Err(TeeError::Dstack(msg)) => assert!(msg.contains("500"), "{msg}"),
        other => panic!("expected Dstack error, got {other:?}"),
    }
}

#[tokio::test]
async fn info_inlines_the_tcb_info_json_string() {
    // The real agent returns `tcb_info` as a JSON *string* inside the JSON
    // response (the official SDKs special-case it the same way).
    let tcb = serde_json::json!({
        "mrtd": "b24d", "rtmr0": "3fc6", "rtmr1": "07e6", "rtmr2": "df67", "rtmr3": "aaaa",
        "os_image_hash": "bd36", "compose_hash": "cc01", "device_id": "dev",
        "app_compose": "{}", "event_log": [
            {"imr": 3, "event_type": 134217729, "digest": "ab", "event": "compose-hash", "event_payload": "cc01"}
        ]
    })
    .to_string();
    let (sock, _scratch) = spawn_fake_agent(Arc::new(move |path, _| {
        assert_eq!(path, "/Info");
        (
            StatusCode::OK,
            serde_json::json!({
                "app_id": "app1", "instance_id": "inst1", "app_cert": "", "tcb_info": tcb,
                "app_name": "llm-node", "device_id": "dev", "mr_aggregated": "",
                "os_image_hash": "bd36", "key_provider_info": "{}", "compose_hash": "cc01",
                "vm_config": "{\"cpu_count\":4}"
            })
            .to_string(),
        )
    }));
    let info = client(sock).info().await.expect("info");
    assert_eq!(info.app_id, "app1");
    assert_eq!(info.compose_hash, "cc01");
    assert_eq!(info.tcb_info.mrtd, "b24d");
    assert_eq!(info.tcb_info.rtmr3, "aaaa");
    assert_eq!(info.tcb_info.event_log.len(), 1);
    assert_eq!(info.tcb_info.event_log[0].event, "compose-hash");
}

#[tokio::test]
async fn http_endpoint_with_a_base_path_posts_under_that_prefix() {
    // The simulator is reached over TCP; a base path in the env value must be
    // prepended to the RPC path, as the official SDKs do.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let svc = service_fn(|req: Request<Incoming>| async move {
                    let path = req.uri().path().to_string();
                    let out = if path == "/prpc/GetQuote" {
                        serde_json::json!({"quote": "0400", "event_log": "[]"}).to_string()
                    } else {
                        format!("wrong path {path}")
                    };
                    let status = if path == "/prpc/GetQuote" {
                        StatusCode::OK
                    } else {
                        StatusCode::NOT_FOUND
                    };
                    Ok::<_, std::convert::Infallible>(
                        Response::builder()
                            .status(status)
                            .body(Full::new(Bytes::from(out)))
                            .unwrap(),
                    )
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    let endpoint = DstackClient::parse_endpoint(&format!("http://127.0.0.1:{port}/prpc")).unwrap();
    let c = DstackClient::new(endpoint, Duration::from_secs(5));
    let q = c.get_quote(&RD).await.expect("prefixed GetQuote");
    assert_eq!(q.quote, vec![0x04, 0x00]);
    // And without the prefix the same server answers 404, proving the prefix mattered.
    let bare = DstackClient::parse_endpoint(&format!("http://127.0.0.1:{port}")).unwrap();
    assert!(matches!(
        DstackClient::new(bare, Duration::from_secs(5)).get_quote(&RD).await,
        Err(TeeError::Dstack(m)) if m.contains("404")
    ));
}

#[tokio::test]
async fn missing_socket_is_a_dstack_error_not_a_panic() {
    let c = client(PathBuf::from("/nonexistent/dstack.sock"));
    assert!(matches!(c.get_quote(&RD).await, Err(TeeError::Dstack(_))));
    assert!(matches!(c.info().await, Err(TeeError::Dstack(_))));
}

#[test]
fn endpoint_parsing_mirrors_the_official_sdk() {
    assert_eq!(
        DstackClient::parse_endpoint("/tmp/sim/dstack.sock").unwrap(),
        Endpoint::Unix(PathBuf::from("/tmp/sim/dstack.sock"))
    );
    assert_eq!(
        DstackClient::parse_endpoint("http://127.0.0.1:8090/").unwrap(),
        Endpoint::Http {
            host: "127.0.0.1".into(),
            port: 8090,
            base_path: String::new()
        }
    );
    // A path prefix is KEPT as a base path (the SDKs treat the value as a base
    // URL); an IPv6 literal is unbracketed (rounds 5 and 7).
    assert_eq!(
        DstackClient::parse_endpoint("http://127.0.0.1:8090/prpc/").unwrap(),
        Endpoint::Http {
            host: "127.0.0.1".into(),
            port: 8090,
            base_path: "/prpc".into()
        }
    );
    assert_eq!(
        DstackClient::parse_endpoint("http://[::1]:8090").unwrap(),
        Endpoint::Http {
            host: "::1".into(),
            port: 8090,
            base_path: String::new()
        }
    );
    assert!(matches!(
        DstackClient::parse_endpoint("https://example.com:443"),
        Err(TeeError::Dstack(_))
    ));
    assert!(matches!(
        DstackClient::parse_endpoint("http://nohost"),
        Err(TeeError::Dstack(_))
    ));
}
