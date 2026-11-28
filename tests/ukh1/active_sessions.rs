// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! UKH1: `GET /v1/sessions/active` exists only when ACTIVE_SESSIONS_TOKEN is
//! set, needs `Authorization: Bearer <token>`, and returns only
//! `{"active_sessions": N}` (N = the session key store's count).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fabstir_llm_node::api::server::{active_sessions_authorised, ApiServer};
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "s3cret-token-for-uk-host-stop";

async fn get_active(server: Arc<ApiServer>, auth: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut req = Request::get("/v1/sessions/active");
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let resp = ApiServer::create_router(server)
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1 << 16)
        .await
        .unwrap()
        .to_vec();
    (status, body)
}

fn server_with_token(token: Option<&str>) -> Arc<ApiServer> {
    let mut s = ApiServer::new_for_test();
    s.set_active_sessions_token_for_test(token.map(String::from));
    Arc::new(s)
}

#[tokio::test]
async fn route_is_404_when_no_token_is_configured() {
    for t in [None, Some(""), Some("   ")] {
        let server = server_with_token(t);
        let (status, _) = get_active(server.clone(), Some(&format!("Bearer {TOKEN}"))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "token {t:?}");
        let (status, _) = get_active(server, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "token {t:?}, no header");
    }
}

#[tokio::test]
async fn wrong_or_missing_token_is_401() {
    let server = server_with_token(Some(TOKEN));
    for auth in [
        None,
        Some("Bearer wrong"),
        Some(TOKEN),
        Some("Basic czNjcmV0"),
        Some("Bearer s3cret-token-for-uk-host-sto"),
        Some("Bearer s3cret-token-for-uk-host-stopX"),
    ] {
        let (status, body) = get_active(server.clone(), auth).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{auth:?}");
        assert!(!String::from_utf8_lossy(&body).contains("active_sessions"));
    }
}

#[tokio::test]
async fn right_token_returns_the_count_and_nothing_else() {
    let server = server_with_token(Some(TOKEN));
    let auth = format!("Bearer {TOKEN}");
    let (status, body) = get_active(server.clone(), Some(&auth)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v, serde_json::json!({ "active_sessions": 0 }));

    let store = server.get_session_key_store();
    store
        .store_key_for("conn-a", "sess-1".into(), [1; 32])
        .await;
    store
        .store_key_for("conn-b", "sess-2".into(), [2; 32])
        .await;
    let (_, body) = get_active(server.clone(), Some(&auth)).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v, serde_json::json!({ "active_sessions": 2 }));
    let text = v.to_string();
    assert!(!text.contains("sess-") && !text.contains("conn-"));
    store.remove_connection("conn-a").await;
    let (_, body) = get_active(server, Some(&auth)).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["active_sessions"], 1);
}

#[test]
fn authorisation_rule() {
    use axum::http::StatusCode as S;
    assert_eq!(
        active_sessions_authorised(None, Some("Bearer x")),
        Err(S::NOT_FOUND)
    );
    assert_eq!(
        active_sessions_authorised(Some("t"), None),
        Err(S::UNAUTHORIZED)
    );
    assert_eq!(
        active_sessions_authorised(Some("t"), Some("Bearer u")),
        Err(S::UNAUTHORIZED)
    );
    assert_eq!(
        active_sessions_authorised(Some("t"), Some("Bearer t")),
        Ok(())
    );
    // RFC 7235: the scheme is case-insensitive; the token is not
    assert_eq!(
        active_sessions_authorised(Some("t"), Some("bearer t")),
        Ok(())
    );
    assert_eq!(
        active_sessions_authorised(Some("t"), Some("BEARER t")),
        Ok(())
    );
    assert_eq!(
        active_sessions_authorised(Some("t"), Some("Bearer T")),
        Err(S::UNAUTHORIZED)
    );
    assert_eq!(
        active_sessions_authorised(Some("t"), Some("Bearert")),
        Err(S::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn env_token_is_trimmed_once_so_a_crlf_file_still_matches() {
    use fabstir_llm_node::api::server::active_sessions_token_from_env;
    assert_eq!(
        active_sessions_token_from_env(Some("tok\r".into())),
        Some("tok".into())
    );
    assert_eq!(
        active_sessions_token_from_env(Some("  tok \n".into())),
        Some("tok".into())
    );
    assert_eq!(active_sessions_token_from_env(Some(" \r\n".into())), None);
    assert_eq!(active_sessions_token_from_env(None), None);
    // end to end through the router: a CRLF env value still opens the route
    let server = server_with_token(Some("crlf-token\r"));
    let (status, _) = get_active(server.clone(), Some("Bearer crlf-token")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get_active(server, Some("bearer crlf-token")).await;
    assert_eq!(status, StatusCode::OK);
}
