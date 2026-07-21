// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! FC1.6 — vault-session client authorisation.
//!
//! When a platform vault is the on-chain depositor (fiat-credit sessions),
//! possession of a sessionId must not be enough to consume the render: the
//! connecting client has to be the depositor itself OR a client the credits
//! backend explicitly authorised. The backend signs
//! `keccak256("FC1-SESSION-AUTH:<sessionId decimal>:<client address lowercase>")`
//! with a dedicated auth key (NOT the vault funds key) and the helper POSTs
//! that signature to `/v1/session-auth` before its WebSocket submit; the
//! encrypted-session-init path then checks the recovered client address
//! against this table. Crypto-native sessions (depositor is not a configured
//! vault) are untouched by construction, and with no `FIAT_VAULT_ADDRESSES`
//! configured the whole gate is skipped.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tiny_keccak::{Hasher, Keccak};

use super::server::ApiServer;

/// jobId → the client address the backend authorised (lowercase).
pub type SessionAuthStore = Mutex<HashMap<u64, String>>;

pub const SESSION_AUTH_SCHEME: &str = "fc1-session-auth-v1";

/// The signed message, locked cross-repo with the website backend
/// (`src/lib/fiat-vault.ts::sessionAuthDigest`): no EIP-191 prefix — the raw
/// keccak digest is what the backend key signs.
pub fn session_auth_digest(session_id: u64, client_address: &str) -> [u8; 32] {
    let message = format!(
        "FC1-SESSION-AUTH:{}:{}",
        session_id,
        client_address.to_lowercase()
    );
    let mut keccak = Keccak::v256();
    let mut digest = [0u8; 32];
    keccak.update(message.as_bytes());
    keccak.finalize(&mut digest);
    digest
}

/// Recover the signer of an authorisation over (sessionId, clientAddress).
pub fn verify_session_auth(
    session_id: u64,
    client_address: &str,
    signature: &[u8],
) -> anyhow::Result<String> {
    let digest = session_auth_digest(session_id, client_address);
    crate::crypto::recover_client_address(signature, &digest)
}

/// The pure accept/reject decision, kept parameter-only so tests need no
/// chain, no server, no mocking (same pattern as the existing crypto tests).
///
/// - depositor not a configured vault → ACCEPT (crypto-native, untouched);
/// - vault-paid → accept only the depositor itself or the backend-authorised
///   client for this session. All comparisons case-insensitive.
pub fn authorise_session_client(
    depositor: &str,
    client_address: &str,
    vault_addresses: &[String],
    authorised_client: Option<&str>,
) -> bool {
    let vault_paid = vault_addresses
        .iter()
        .any(|vault| vault.eq_ignore_ascii_case(depositor));
    if !vault_paid {
        return true;
    }
    client_address.eq_ignore_ascii_case(depositor)
        || authorised_client.is_some_and(|authorised| authorised.eq_ignore_ascii_case(client_address))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAuthRequest {
    pub session_id: String,
    pub client_address: String,
    pub scheme: String,
    pub signature: String,
}

/// POST /v1/session-auth — the helper hands over the backend's authorisation
/// before its WS submit. Self-authenticating: only a signature that recovers
/// to the configured backend auth address is accepted, so the route needs no
/// bearer of its own. 404 when the feature is unconfigured (pre-FC1.6 shape,
/// which the helper tolerates).
pub async fn session_auth_handler(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<SessionAuthRequest>,
) -> impl IntoResponse {
    let Some(expected) = server.fiat_backend_auth_address() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session-auth is not enabled on this node"})),
        );
    };
    if request.scheme != SESSION_AUTH_SCHEME {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("unknown scheme {} (expected {})", request.scheme, SESSION_AUTH_SCHEME)})),
        );
    }
    let Ok(session_id) = request.session_id.parse::<u64>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "sessionId must be a decimal string"})),
        );
    };
    let sig_hex = request.signature.trim_start_matches("0x");
    let Ok(signature) = hex::decode(sig_hex) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "signature must be hex"})),
        );
    };
    match verify_session_auth(session_id, &request.client_address, &signature) {
        Ok(recovered) if recovered.eq_ignore_ascii_case(expected) => {
            if let Ok(mut store) = server.session_auth_store().lock() {
                store.insert(session_id, request.client_address.to_lowercase());
            }
            (StatusCode::OK, Json(json!({"ok": true})))
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "authorisation signature is not from the configured backend auth key"})),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;

    // Mirrors the website's locked vector (test/fiat-vault.test.ts): key
    // 0x11…11 → address 0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A.
    const AUTH_KEY: [u8; 32] = [0x11; 32];
    const AUTH_ADDRESS: &str = "0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a";
    const CLIENT: &str = "0x1234567890abcdef1234567890abcdef12345678";
    const VAULT: &str = "0x8ba1f109551bd432803012645ac136ddd64dba72";

    fn sign_auth(session_id: u64, client: &str) -> Vec<u8> {
        let digest = session_auth_digest(session_id, client);
        let key = SigningKey::from_bytes(&AUTH_KEY.into()).unwrap();
        let (signature, recovery_id) = key.sign_prehash_recoverable(&digest).unwrap();
        let mut bytes = signature.to_bytes().to_vec();
        bytes.push(recovery_id.to_byte());
        bytes
    }

    #[test]
    fn digest_matches_the_website_locked_vector() {
        // keccak256("FC1-SESSION-AUTH:818:0x1234567890abcdef1234567890abcdef12345678")
        let digest = session_auth_digest(818, "0x1234567890abcDEF1234567890abcdef12345678");
        assert_eq!(
            hex::encode(digest),
            "6cba97aea9365ee8f302b9878b72d6b55935bc1e922ed37d9e3da4cdad2f6aee"
        );
    }

    #[test]
    fn verify_recovers_the_backend_auth_address() {
        let signature = sign_auth(818, CLIENT);
        let recovered = verify_session_auth(818, CLIENT, &signature).unwrap();
        assert!(recovered.eq_ignore_ascii_case(AUTH_ADDRESS));
    }

    #[test]
    fn verify_binds_the_exact_session_and_client() {
        let signature = sign_auth(818, CLIENT);
        // Same signature presented for a different session or client recovers
        // a DIFFERENT (wrong) address — never the backend's.
        let other_session = verify_session_auth(819, CLIENT, &signature);
        assert!(!matches!(other_session, Ok(ref a) if a.eq_ignore_ascii_case(AUTH_ADDRESS)));
        let other_client =
            verify_session_auth(818, "0x9999999999999999999999999999999999999999", &signature);
        assert!(!matches!(other_client, Ok(ref a) if a.eq_ignore_ascii_case(AUTH_ADDRESS)));
    }

    #[test]
    fn crypto_native_sessions_are_always_served() {
        let vaults = vec![VAULT.to_string()];
        // Depositor is a burner, not the vault: any client is accepted,
        // authorisation or not — provably unaffected.
        assert!(authorise_session_client(CLIENT, CLIENT, &vaults, None));
        assert!(authorise_session_client(
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            CLIENT,
            &vaults,
            None
        ));
        // And with the feature unconfigured (empty vault list) everything passes.
        assert!(authorise_session_client(VAULT, CLIENT, &[], None));
    }

    #[test]
    fn vault_sessions_serve_only_the_depositor_or_the_authorised_client() {
        let vaults = vec![VAULT.to_string()];
        // The vault itself connecting (depositor == client) is fine.
        assert!(authorise_session_client(VAULT, VAULT, &vaults, None));
        // The blessed client, case-insensitively.
        assert!(authorise_session_client(
            VAULT,
            &CLIENT.to_uppercase().replace("0X", "0x"),
            &vaults,
            Some(CLIENT)
        ));
        // An unrelated client with no authorisation: rejected.
        assert!(!authorise_session_client(VAULT, CLIENT, &vaults, None));
        // An authorisation for a DIFFERENT client does not transfer.
        assert!(!authorise_session_client(
            VAULT,
            "0x9999999999999999999999999999999999999999",
            &vaults,
            Some(CLIENT)
        ));
    }

    #[test]
    fn vault_address_comparison_is_case_insensitive() {
        let vaults = vec![VAULT.to_uppercase().replace("0X", "0x")];
        assert!(!authorise_session_client(VAULT, CLIENT, &vaults, None));
        assert!(authorise_session_client(VAULT, CLIENT, &vaults, Some(CLIENT)));
    }
}
