// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 3 (M1a) input-image path: parse the `0xae` capability envelope, derive
//! the S5 blob-download CID, fetch the ciphertext over the portal with a blake3
//! integrity gate, decrypt, and hash — all without a live portal (a local axum
//! server mocks `GET /s5/blob/:cid`).

use fabstir_llm_node::ltx::exr::{capability_cid, encrypt_frame, padding_for};
use fabstir_llm_node::ltx::input_image::{
    blob_download_cid, download_blob, fetch_image_hash, parse_capability_cid,
};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/ltx/capability-fixture.json"
);

/// Serve the given bytes for ANY path (the node builds `/s5/blob/{cid}`; the mock
/// is route-agnostic so it stays independent of axum's path-param syntax).
async fn spawn_blob_server(body: Vec<u8>) -> String {
    use axum::Router;
    let app = Router::new().fallback(move || {
        let b = body.clone();
        async move { b }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

/// Build a real capability CID from a plaintext, returning `(cid, plaintext,
/// ciphertext, key, padding)` so tests can assert against the exact inputs.
fn make_cid(plaintext: &[u8]) -> (String, Vec<u8>, Vec<u8>, [u8; 32], u32) {
    let key = [0x24u8; 32];
    let ciphertext = encrypt_frame(plaintext, &key).unwrap();
    let padding = padding_for(plaintext.len()) as u32;
    let cid = capability_cid(plaintext, &ciphertext, &key, padding);
    (cid, plaintext.to_vec(), ciphertext, key, padding)
}

#[test]
fn test_parse_capability_cid_roundtrips() {
    let plaintext: Vec<u8> = (0..5000u32).map(|i| (i % 256) as u8).collect();
    let (cid, _pt, ciphertext, key, padding) = make_cid(&plaintext);

    let env = parse_capability_cid(&cid).unwrap();
    assert_eq!(&env.ct_hash, blake3::hash(&ciphertext).as_bytes());
    assert_eq!(env.key, key);
    assert_eq!(env.padding, padding);
    assert_eq!(env.plaintext_len, plaintext.len());
    assert_eq!(&env.pt_hash, blake3::hash(&plaintext).as_bytes());
}

#[test]
fn test_parse_capability_cid_rejects_malformed() {
    let plaintext = b"hello world".to_vec();
    let (cid, ..) = make_cid(&plaintext);

    // Missing the 'u' multibase prefix.
    assert!(parse_capability_cid(&cid[1..]).is_err());
    // Not valid base64url.
    assert!(parse_capability_cid("u!!!not-base64!!!").is_err());
    // Truncated envelope.
    assert!(parse_capability_cid("uYWJj").is_err());
    // Wrong CID-type byte ([0] flipped from 0xae). Rebuild by mangling one byte.
    let mut bytes =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &cid[1..])
            .unwrap();
    bytes[0] = 0x00;
    let bad = format!(
        "u{}",
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
    );
    assert!(parse_capability_cid(&bad).is_err());
}

#[test]
fn test_blob_download_cid_shape() {
    let ct_hash = [0x7bu8; 32];
    let cid = blob_download_cid(&ct_hash);
    assert!(cid.starts_with('z'), "base58btc multibase prefix");
    let decoded = bs58::decode(&cid[1..]).into_vec().unwrap();
    // [0x5b, 0x82, 0x1f] ++ ct_hash(32) ++ 0x00
    assert_eq!(decoded.len(), 36);
    assert_eq!(&decoded[0..3], &[0x5b, 0x82, 0x1f]);
    assert_eq!(&decoded[3..35], &ct_hash);
    assert_eq!(decoded[35], 0x00);
}

#[tokio::test]
async fn test_download_blob_verifies_and_returns() {
    let ciphertext: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 256) as u8).collect();
    let ct_hash: [u8; 32] = *blake3::hash(&ciphertext).as_bytes();
    let url = spawn_blob_server(ciphertext.clone()).await;
    let got = download_blob(&url, &ct_hash).await.unwrap();
    assert_eq!(got, ciphertext);
}

#[tokio::test]
async fn test_download_blob_integrity_mismatch_errors() {
    let served: Vec<u8> = vec![1, 2, 3, 4, 5];
    // Ask for a DIFFERENT blob's hash than what the server returns.
    let wrong_hash: [u8; 32] = *blake3::hash(b"something else").as_bytes();
    let url = spawn_blob_server(served).await;
    assert!(
        download_blob(&url, &wrong_hash).await.is_err(),
        "blake3(body) != ct_hash must hard-fail before any decrypt"
    );
}

#[tokio::test]
async fn test_fetch_image_hash_end_to_end() {
    // The M0 capability fixture: known plaintext (a byte ramp), its ciphertext,
    // and the capability CID. fetch_image_hash must decrypt to the plaintext and
    // return keccak256(plaintext).
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(FIXTURE).unwrap()).unwrap();
    let cid = fixture["capabilityCid"].as_str().unwrap().to_string();
    let ciphertext = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        fixture["ciphertextBase64"].as_str().unwrap(),
    )
    .unwrap();
    let plaintext = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        fixture["plaintextBase64"].as_str().unwrap(),
    )
    .unwrap();

    let url = spawn_blob_server(ciphertext).await;
    let (image_hash, decrypted) = fetch_image_hash(&url, &cid).await.unwrap();
    assert_eq!(decrypted, plaintext, "decrypts to the fixture plaintext");
    assert_eq!(
        image_hash,
        ethers::utils::keccak256(&plaintext),
        "imageHash = keccak256(plaintext)"
    );
}
