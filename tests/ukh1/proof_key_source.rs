// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NP2 wiring: which session key keys a WebSocket request's proof
//! commitments. `ApiServer::proof_session_key_for` is the ONE source both
//! WebSocket inference call sites use: the key this connection stored for
//! this session, or None.

use fabstir_llm_node::api::server::ApiServer;

#[tokio::test]
async fn the_connections_own_session_key_is_the_proof_key() {
    let server = ApiServer::new_for_test();
    let store = server.get_session_key_store();
    store
        .store_key_for("conn-1", "sess-1".into(), [7; 32])
        .await;
    store
        .store_key_for("conn-2", "sess-1".into(), [9; 32])
        .await;
    assert_eq!(
        server.proof_session_key_for("conn-1", Some("sess-1")).await,
        Some([7; 32])
    );
    assert_eq!(
        server.proof_session_key_for("conn-2", Some("sess-1")).await,
        Some([9; 32]),
        "never another connection's key"
    );
    assert_eq!(
        server.proof_session_key_for("conn-3", Some("sess-1")).await,
        None
    );
    assert_eq!(server.proof_session_key_for("conn-1", None).await, None);
}

#[tokio::test]
async fn the_requests_own_session_picks_the_key_not_the_connections() {
    use fabstir_llm_node::api::InferenceRequest;
    let server = ApiServer::new_for_test();
    let store = server.get_session_key_store();
    store
        .store_key_for("conn-1", "sess-A".into(), [1; 32])
        .await;
    store
        .store_key_for("conn-1", "sess-B".into(), [2; 32])
        .await;
    let req: InferenceRequest = serde_json::from_value(serde_json::json!({
        "model": "m", "prompt": "p", "max_tokens": 4, "session_id": "sess-B", "job_id": 42
    }))
    .unwrap();
    assert_eq!(
        server.proof_session_key_for_request("conn-1", &req).await,
        Some([2; 32]),
        "the request names sess-B, so sess-B's key"
    );
    let none: InferenceRequest = serde_json::from_value(serde_json::json!({
        "model": "m", "prompt": "p", "max_tokens": 4
    }))
    .unwrap();
    assert_eq!(
        server.proof_session_key_for_request("conn-1", &none).await,
        None
    );
}

#[test]
fn every_websocket_inference_call_site_uses_the_helper() {
    let src = include_str!("../../src/api/server.rs");
    let prod = &src[..src.find("#[cfg(test)]\nmod tests").unwrap_or(src.len())];
    let calls: Vec<usize> = prod
        .match_indices(".handle_streaming_request(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(calls.len(), 2, "two WebSocket call sites");
    for at in calls {
        // the 1500 bytes before the call and the call's argument list, whitespace removed
        let before: String = prod[at.saturating_sub(1500)..at]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let window: String = prod[at..(at + 4000).min(prod.len())]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let end = window.find(").await").expect("end of the call");
        let args = &window[..end];
        assert!(
            before.contains(
                "letproof_key=server.proof_session_key_for_request(connection_keys.conn_id(),&request).await;"
            ),
            "the proof key must be chosen from the REQUEST's session: {before}"
        );
        assert!(
            args.ends_with("session_adapter.clone(),proof_key,"),
            "the call must pass that key: {args}"
        );
    }
}
