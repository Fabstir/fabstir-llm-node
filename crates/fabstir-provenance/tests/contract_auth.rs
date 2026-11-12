// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Request authentication vector (AP-SIGNER-CONTRACT section 1, reproduced from spike AP0.2).
//! Mutations: canonical line order or framing, EIP-191 prefix, window, v handling, high-s acceptance.
use fabstir_provenance::contract::*;
use k256::ecdsa::SigningKey;

const BODY: &[u8] = br#"{"reservation_id":"rsv_demo","slot":0}"#;
const TS: u64 = 1_794_000_000;
const NONCE: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
const ADDR: &str = "1a642f0e3c3af545e7acbd38b07251b3990914f1";

fn test_key() -> SigningKey {
    SigningKey::from_bytes((&[1u8; 32]).into()).unwrap()
}

#[test]
fn auth_vector_reproduces_bit_exactly() {
    let canon = canonical_bytes(
        "post",
        "/v1/sign",
        TS,
        "00112233445566778899AABBCCDDEEFF",
        BODY,
    );
    assert_eq!(
        String::from_utf8(canon.clone()).unwrap(),
        "platformlessai-provenance/1\nPOST\n/v1/sign\nts:1794000000\nnonce:00112233445566778899aabbccddeeff\nbody-sha256:1ef3a4fb5605c7fb84f1b225f5e1ae89ed951905fcb4a8eb7bf88f200d4255d9"
    );
    let d = keccak(&canon);
    assert_eq!(
        hex::encode(d),
        "a73bc137cd1a2483c0a92e353aff2c6cd4b587be0f90226a7bd8082d3b901a42"
    );
    assert_eq!(
        hex::encode(eip191_hash(&d)),
        "271306493d06c47b020cf1a7f4baee6b76443eee8cda96cb7b0103579f1d8c81"
    );
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    assert_eq!(h.signature, "0x285f2f193233697b21bab0aded79e4d642dd6ad4b2bf8e7d086330c7d67662b06e2312ee3037a6b0e718329872fdfff5e33b5c93e5b4d398626505df137c788a1c");
    assert_eq!(hex::encode(address_of(test_key().verifying_key())), ADDR);
}

#[test]
fn recovers_the_signing_host() {
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    let a = recover_caller(
        TS + 5,
        "POST",
        "/v1/sign",
        &h.timestamp,
        &h.nonce,
        &h.signature,
        BODY,
    )
    .unwrap();
    assert_eq!(hex::encode(a), ADDR);
}

#[test]
fn tampering_changes_the_recovered_address() {
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    for (m, t, b) in [
        ("POST", "/v1/sign", &b"{}"[..]),
        ("POST", "/v1/reserve", BODY),
        ("GET", "/v1/sign", BODY),
    ] {
        let a = recover_caller(TS, m, t, &h.timestamp, &h.nonce, &h.signature, b);
        assert_ne!(a.map(hex::encode).as_deref(), Ok(ADDR), "{m} {t}");
    }
}

#[test]
fn stale_requests_are_refused_both_ways() {
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    assert_eq!(
        recover_caller(
            TS + WINDOW_SECS + 1,
            "POST",
            "/v1/sign",
            &h.timestamp,
            &h.nonce,
            &h.signature,
            BODY
        ),
        Err(AuthError::Stale)
    );
    assert_eq!(
        recover_caller(
            TS - WINDOW_SECS - 1,
            "POST",
            "/v1/sign",
            &h.timestamp,
            &h.nonce,
            &h.signature,
            BODY
        ),
        Err(AuthError::Stale)
    );
    assert!(recover_caller(
        TS + WINDOW_SECS,
        "POST",
        "/v1/sign",
        &h.timestamp,
        &h.nonce,
        &h.signature,
        BODY
    )
    .is_ok());
}

#[test]
fn malformed_headers_are_refused() {
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    let r = |ts: &str, n: &str, s: &str| recover_caller(TS, "POST", "/v1/sign", ts, n, s, BODY);
    assert!(matches!(
        r("x", &h.nonce, &h.signature),
        Err(AuthError::Malformed(_))
    ));
    assert!(matches!(
        r(&h.timestamp, "0011", &h.signature),
        Err(AuthError::Malformed(_))
    ));
    assert!(matches!(
        r(&h.timestamp, &h.nonce.to_uppercase(), &h.signature),
        Err(AuthError::Malformed(_))
    ));
    assert!(matches!(
        r(&h.timestamp, &h.nonce, &h.signature[2..]),
        Err(AuthError::Malformed(_))
    ));
    assert!(matches!(
        r(&h.timestamp, &h.nonce, &h.signature[..130]),
        Err(AuthError::Malformed(_))
    ));
}

#[test]
fn high_s_signature_is_refused() {
    // flip s to n - s and v: the same signer, a malleable second encoding
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    let mut b = hex::decode(&h.signature[2..]).unwrap();
    let sig = k256::ecdsa::Signature::from_slice(&b[..64]).unwrap();
    let n_minus_s = -*sig.s();
    let high = k256::ecdsa::Signature::from_scalars(*sig.r(), n_minus_s).unwrap();
    b[..64].copy_from_slice(&high.to_bytes());
    b[64] ^= 1;
    let s = format!("0x{}", hex::encode(b));
    assert!(recover_caller(TS, "POST", "/v1/sign", &h.timestamp, &h.nonce, &s, BODY).is_err());
}

#[test]
fn only_v_27_or_28_is_accepted() {
    let h = sign_request(&test_key(), "POST", "/v1/sign", TS, NONCE, BODY);
    let mut b = hex::decode(&h.signature[2..]).unwrap();
    b[64] -= 27; // the same signature with v = 0 or 1
    let s = format!("0x{}", hex::encode(b));
    assert!(recover_caller(TS, "POST", "/v1/sign", &h.timestamp, &h.nonce, &s, BODY).is_err());
}
