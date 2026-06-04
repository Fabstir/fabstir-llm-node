// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 2.1 — encrypted-container header encode/decode tests (RED→GREEN).
//!
//! The header is a fixed-size binary record prefixing the chunked-AEAD body.
//! These tests pin the wire layout (round-trip) and the fail-closed parse path
//! (truncated buffer / wrong magic / unsupported version).

use fabstir_llm_node::tee::container::{
    chunk_count, decrypt_model, encrypt_model, ContainerHeader, AEAD_TAG_LEN, CONTAINER_MAGIC,
    CONTAINER_VERSION, HEADER_LEN,
};
use fabstir_llm_node::tee::types::TeeError;

fn sample_header() -> ContainerHeader {
    ContainerHeader {
        magic: CONTAINER_MAGIC,
        version: CONTAINER_VERSION,
        model_id: [7u8; 32],
        chunk_size: 8 * 1024 * 1024,
        num_chunks: 42,
        nonce_base: [3u8; 16],
        policy_hash: [9u8; 32],
    }
}

#[test]
fn header_roundtrip_preserves_all_fields() {
    let h = sample_header();
    let bytes = h.encode();
    assert_eq!(
        bytes.len(),
        HEADER_LEN,
        "encoded header must be exactly HEADER_LEN bytes"
    );
    let decoded = ContainerHeader::decode(&bytes).expect("decode of a valid header must succeed");
    assert_eq!(decoded, h, "decode∘encode must be the identity");
}

#[test]
fn decode_rejects_bad_magic() {
    let mut bytes = sample_header().encode();
    bytes[0] ^= 0xFF; // corrupt the magic
    let err = ContainerHeader::decode(&bytes).expect_err("bad magic must fail closed");
    assert!(
        matches!(err, TeeError::Crypto(_)),
        "bad magic must return TeeError::Crypto, got {err:?}"
    );
}

#[test]
fn decode_rejects_unsupported_version() {
    let mut h = sample_header();
    h.version = CONTAINER_VERSION + 1;
    let bytes = h.encode();
    let err = ContainerHeader::decode(&bytes).expect_err("unsupported version must fail closed");
    assert!(
        matches!(err, TeeError::Crypto(_)),
        "unsupported version must return TeeError::Crypto, got {err:?}"
    );
}

#[test]
fn decode_rejects_truncated_buffer() {
    let bytes = sample_header().encode();
    let err = ContainerHeader::decode(&bytes[..HEADER_LEN - 1])
        .expect_err("a buffer shorter than HEADER_LEN must fail closed");
    assert!(
        matches!(err, TeeError::Crypto(_)),
        "truncated header must return TeeError::Crypto, got {err:?}"
    );
}

#[test]
fn decode_rejects_empty_buffer() {
    let err = ContainerHeader::decode(&[]).expect_err("empty buffer must fail closed");
    assert!(
        matches!(err, TeeError::Crypto(_)),
        "empty buffer must return TeeError::Crypto, got {err:?}"
    );
}

#[test]
fn decode_ignores_trailing_body_bytes() {
    // Real containers are `[header][chunk_0][chunk_1]…`, so decode must parse
    // exactly HEADER_LEN bytes and leave the trailing AEAD body for the caller.
    let h = sample_header();
    let mut buf = h.encode();
    buf.extend_from_slice(&[0xAB; 4096]); // simulated ciphertext body
    let decoded = ContainerHeader::decode(&buf).expect("a header followed by a body must decode");
    assert_eq!(
        decoded, h,
        "trailing body bytes must not affect the parsed header"
    );
}

// ---- Sub-phase 2.2: encrypt path -------------------------------------------

#[test]
fn chunk_count_is_ceil_of_len_over_chunk_size() {
    assert_eq!(
        chunk_count(0, 100).unwrap(),
        0,
        "empty plaintext = 0 chunks"
    );
    assert_eq!(chunk_count(1, 100).unwrap(), 1);
    assert_eq!(
        chunk_count(100, 100).unwrap(),
        1,
        "exact multiple = no extra chunk"
    );
    assert_eq!(chunk_count(101, 100).unwrap(), 2);
    assert_eq!(chunk_count(250, 100).unwrap(), 3);
}

#[test]
fn chunk_count_rejects_zero_chunk_size() {
    let err = chunk_count(100, 0).expect_err("zero chunk_size must fail closed");
    assert!(
        matches!(err, TeeError::Crypto(_)),
        "zero chunk_size must return TeeError::Crypto, got {err:?}"
    );
}

#[test]
fn chunk_count_rejects_overflow_at_2_pow_32() {
    // chunk_size = 1 ⇒ num_chunks == plaintext_len. 2^32 bytes ⇒ 2^32 chunks,
    // which would overflow the 4-byte nonce counter → must fail closed.
    let two_pow_32 = 1u64 << 32;
    let err = chunk_count(two_pow_32, 1).expect_err("2^32 chunks must be rejected");
    assert!(
        matches!(err, TeeError::ContainerTooLarge),
        "2^32 chunks must return ContainerTooLarge, got {err:?}"
    );
    // 2^32 - 1 chunks is the largest count that still fits a u32.
    assert_eq!(chunk_count(two_pow_32 - 1, 1).unwrap(), u32::MAX);
}

#[test]
fn encrypt_model_writes_header_and_one_aead_chunk_per_slice() {
    let dek = [0x11u8; 32];
    let model_id = [0x22u8; 32];
    let policy_hash = [0x33u8; 32];
    let chunk_size = 1024u32;
    // 3 full chunks + a 7-byte tail ⇒ ceil = 4 chunks.
    let plaintext = vec![0xAAu8; 1024 * 3 + 7];

    let container =
        encrypt_model(&plaintext, &dek, model_id, policy_hash, chunk_size).expect("encrypt");
    let header = ContainerHeader::decode(&container).expect("header decodes");

    assert_eq!(header.magic, CONTAINER_MAGIC);
    assert_eq!(header.version, CONTAINER_VERSION);
    assert_eq!(header.model_id, model_id);
    assert_eq!(header.policy_hash, policy_hash);
    assert_eq!(header.chunk_size, chunk_size);
    assert_eq!(header.num_chunks, 4, "ceil(3079/1024) == 4");
    // Each chunk's ciphertext = its plaintext bytes + a 16-byte Poly1305 tag.
    let expected_len = HEADER_LEN + plaintext.len() + header.num_chunks as usize * AEAD_TAG_LEN;
    assert_eq!(
        container.len(),
        expected_len,
        "body = plaintext + one tag per chunk"
    );
    assert_ne!(
        header.nonce_base, [0u8; 16],
        "nonce_base must be CSPRNG-random"
    );
}

#[test]
fn encrypt_model_uses_a_fresh_random_nonce_base_each_call() {
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let pt = vec![0u8; 4096];
    let a = encrypt_model(&pt, &dek, model_id, policy_hash, 1024).unwrap();
    let b = encrypt_model(&pt, &dek, model_id, policy_hash, 1024).unwrap();
    let ha = ContainerHeader::decode(&a).unwrap();
    let hb = ContainerHeader::decode(&b).unwrap();
    assert_ne!(
        ha.nonce_base, hb.nonce_base,
        "a fresh nonce_base per call prevents (key, nonce) reuse across containers"
    );
}

#[test]
fn encrypt_model_rejects_zero_chunk_size() {
    let err = encrypt_model(b"data", &[0u8; 32], [0u8; 32], [0u8; 32], 0)
        .expect_err("zero chunk_size must fail closed");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn encrypt_model_empty_plaintext_is_header_only() {
    let container = encrypt_model(&[], &[1u8; 32], [2u8; 32], [3u8; 32], 256).expect("encrypt");
    let header = ContainerHeader::decode(&container).expect("header decodes");
    assert_eq!(header.num_chunks, 0);
    assert_eq!(
        container.len(),
        HEADER_LEN,
        "no body chunks for empty plaintext"
    );
}

// ---- Sub-phase 2.3: decrypt-stream path ------------------------------------

fn enc(
    plaintext: &[u8],
    dek: &[u8; 32],
    model_id: [u8; 32],
    policy_hash: [u8; 32],
    chunk_size: u32,
) -> Vec<u8> {
    encrypt_model(plaintext, dek, model_id, policy_hash, chunk_size).expect("encrypt")
}

#[test]
fn decrypt_model_roundtrips_to_original_bytes() {
    let (dek, model_id, policy_hash) = ([9u8; 32], [8u8; 32], [7u8; 32]);
    // Cover empty, sub-chunk, exact-multiple, and ragged-tail lengths.
    for len in [0usize, 1, 100, 1024, 1024 * 3, 1024 * 3 + 7, 5000] {
        let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let container = enc(&plaintext, &dek, model_id, policy_hash, 1024);
        let mut out = Vec::new();
        decrypt_model(&container, &dek, &model_id, &policy_hash, &mut out)
            .unwrap_or_else(|e| panic!("decrypt len={len} failed: {e:?}"));
        assert_eq!(out, plaintext, "round-trip mismatch at len={len}");
    }
}

#[test]
fn decrypt_model_wrong_key_fails() {
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let container = enc(b"top secret weights", &dek, model_id, policy_hash, 8);
    let mut out = Vec::new();
    let err = decrypt_model(&container, &[0xFFu8; 32], &model_id, &policy_hash, &mut out)
        .expect_err("wrong DEK must fail the AEAD tag");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn decrypt_model_tampered_chunk_fails() {
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let mut container = enc(b"top secret weights", &dek, model_id, policy_hash, 8);
    container[HEADER_LEN + 2] ^= 0x01; // flip a byte inside the first chunk's ciphertext
    let mut out = Vec::new();
    let err = decrypt_model(&container, &dek, &model_id, &policy_hash, &mut out)
        .expect_err("a tampered chunk must fail the AEAD tag");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn decrypt_model_model_id_mismatch_fails_closed() {
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let container = enc(b"weights", &dek, model_id, policy_hash, 8);
    let mut out = Vec::new();
    let err = decrypt_model(&container, &dek, &[0xAAu8; 32], &policy_hash, &mut out)
        .expect_err("model_id mismatch must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
    assert!(out.is_empty(), "no plaintext may be emitted on a mismatch");
}

#[test]
fn decrypt_model_policy_hash_mismatch_fails_closed() {
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let container = enc(b"weights", &dek, model_id, policy_hash, 8);
    let mut out = Vec::new();
    let err = decrypt_model(&container, &dek, &model_id, &[0xBBu8; 32], &mut out)
        .expect_err("policy_hash mismatch must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
    assert!(out.is_empty(), "no plaintext may be emitted on a mismatch");
}

// ---- Header tamper-evidence (the full header is bound into every chunk's AAD) ---
// These pin the fix for the silent-truncation break: rewriting any structural
// header field, or dropping a chunk + decrementing num_chunks, must fail closed.

/// Re-attach a mutated header to the original body (the truncation toolkit).
fn forge(container: &[u8], mutate: impl FnOnce(&mut ContainerHeader), body: &[u8]) -> Vec<u8> {
    let mut header = ContainerHeader::decode(container).expect("decode");
    mutate(&mut header);
    let mut forged = header.encode();
    forged.extend_from_slice(body);
    forged
}

#[test]
fn decrypt_model_rejects_dropped_last_chunk_with_rewritten_count() {
    // THE attack: encrypt 4 chunks, then drop the final sealed chunk AND claim 3.
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let plaintext = vec![0x5Au8; 8 * 3 + 1]; // chunk_size 8 ⇒ 4 chunks
    let container = enc(&plaintext, &dek, model_id, policy_hash, 8);
    assert_eq!(ContainerHeader::decode(&container).unwrap().num_chunks, 4);

    let full_ct = 8 + AEAD_TAG_LEN;
    let kept_body = &container[HEADER_LEN..HEADER_LEN + 3 * full_ct]; // drop chunk 3's bytes
    let forged = forge(&container, |h| h.num_chunks = 3, kept_body);

    let mut out = Vec::new();
    let err = decrypt_model(&forged, &dek, &model_id, &policy_hash, &mut out)
        .expect_err("dropping the last chunk + rewriting num_chunks must fail closed");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn decrypt_model_rejects_mutated_header_fields() {
    let (dek, model_id, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let container = enc(&vec![7u8; 100], &dek, model_id, policy_hash, 32);
    let body = container[HEADER_LEN..].to_vec();

    // Each structural mutation must break the per-chunk AAD (or the nonce).
    let mutators: Vec<(&str, fn(&mut ContainerHeader))> = vec![
        ("num_chunks", |h| h.num_chunks += 1),
        ("chunk_size", |h| h.chunk_size += 1),
        ("nonce_base", |h| h.nonce_base[0] ^= 0xFF),
    ];
    for (name, m) in mutators {
        let forged = forge(&container, m, &body);
        let mut out = Vec::new();
        let result = decrypt_model(&forged, &dek, &model_id, &policy_hash, &mut out);
        assert!(
            matches!(result, Err(TeeError::Crypto(_))),
            "mutating {name} must fail closed with TeeError::Crypto, got {result:?}"
        );
    }
}
