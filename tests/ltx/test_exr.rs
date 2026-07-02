// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 EXR sidecar tests: frame collection ordering, 256 KiB chunked
//! XChaCha20-Poly1305 round-trip, the keyless `frameHash` vs the key-bearing
//! capability CID, and the keyless Merkle manifest. No S5 backend is touched;
//! the crypto/envelope maths is exercised directly.

use fabstir_llm_node::ltx::exr::{
    build_manifest, capability_cid, collect, decrypt_frame, encrypt_frame, frame_hash, padding_for,
    CHUNK,
};
use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
use fabstir_llm_node::transcoder::merkle::MerkleTree;

// ethers is a dev-dependency; reuse its re-exported `hex` + `keccak256` so the
// test recomputes hashes independently of the module under test.
use ethers::utils::{hex, keccak256};

const TAG: usize = 16;

fn sample_job(frames: u32) -> LtxJob {
    LtxJob {
        template_id: "tmpl-1".to_string(),
        template_hash: "0xabc".to_string(),
        prompt: "a prompt".to_string(),
        seed: "42".to_string(),
        frames,
        fps: 24,
        resolution: Resolution { w: 1920, h: 1080 },
        lora: "".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
    }
}

// --------------------------------------------------------------------------
// collect()
// --------------------------------------------------------------------------

#[test]
fn test_collect_orders_frames() {
    let dir = tempfile::tempdir().unwrap();
    // Numeric runs 2, 10, 1 must sort 1, 2, 10 (NOT lexically: "10" < "2").
    for name in ["f_2_.exr", "f_10_.exr", "f_1_.exr", "notes.txt"] {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    let frames = collect(dir.path()).unwrap();
    let names: Vec<String> = frames
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["f_1_.exr", "f_2_.exr", "f_10_.exr"]);
    // The non-.exr file is excluded.
    assert_eq!(frames.len(), 3);
}

// --------------------------------------------------------------------------
// encrypt_frame chunking
// --------------------------------------------------------------------------

#[test]
fn test_encrypt_chunking_256kib() {
    let key = [7u8; 32];
    // 3 chunks: two full + a short remainder.
    let size = CHUNK * 2 + 100;
    let pt: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

    let ct = encrypt_frame(&pt, &key).unwrap();

    let n = size.div_ceil(CHUNK);
    assert_eq!(n, 3, "expected 3 chunks for 2*CHUNK+100 bytes");

    // The ciphertext is (padded plaintext) + one 16-byte tag per chunk.
    let padding = padding_for(size);
    let tag_bytes = ct.len() - (size + padding);
    assert_eq!(tag_bytes, n * TAG, "one Poly1305 tag per chunk");

    // The two leading chunks are each exactly CHUNK + TAG bytes.
    assert!(ct.len() > 2 * (CHUNK + TAG));

    // Round-trips back to the original plaintext.
    let back = decrypt_frame(&ct, &key, size).unwrap();
    assert_eq!(back, pt);
}

// --------------------------------------------------------------------------
// round-trip
// --------------------------------------------------------------------------

#[test]
fn test_exact_chunk_multiple_rejected() {
    // An exact 256 KiB multiple would yield an undecryptable final chunk; reject it.
    let key = [9u8; 32];
    let pt = vec![0u8; CHUNK];
    assert!(encrypt_frame(&pt, &key).is_err());
    let pt2 = vec![0u8; CHUNK * 2];
    assert!(encrypt_frame(&pt2, &key).is_err());
    // One byte over is fine and round-trips.
    let pt3 = vec![1u8; CHUNK + 1];
    let ct = encrypt_frame(&pt3, &key).unwrap();
    assert_eq!(decrypt_frame(&ct, &key, pt3.len()).unwrap(), pt3);
}

#[test]
fn test_roundtrip_decrypt() {
    let key = [42u8; 32];

    // Small (single sub-256KiB chunk).
    let small = b"hello HDR exr frame".to_vec();
    let ct_small = encrypt_frame(&small, &key).unwrap();
    assert_eq!(decrypt_frame(&ct_small, &key, small.len()).unwrap(), small);

    // Larger than one chunk.
    let big: Vec<u8> = (0..(CHUNK + 12_345))
        .map(|i| (i.wrapping_mul(31) % 256) as u8)
        .collect();
    let ct_big = encrypt_frame(&big, &key).unwrap();
    assert_eq!(decrypt_frame(&ct_big, &key, big.len()).unwrap(), big);
}

// --------------------------------------------------------------------------
// keyless frame hash vs key-bearing capability CID
// --------------------------------------------------------------------------

#[test]
fn test_framehash_keyless() {
    let pt = b"identical plaintext frame bytes".to_vec();
    let key_a = [1u8; 32];
    let key_b = [2u8; 32];

    let ct_a = encrypt_frame(&pt, &key_a).unwrap();
    let ct_b = encrypt_frame(&pt, &key_b).unwrap();
    // Same plaintext, different keys -> different ciphertext.
    assert_ne!(ct_a, ct_b);

    let fh_a = frame_hash(&ct_a);
    let fh_b = frame_hash(&ct_b);
    // Different ciphertext -> different frame hash.
    assert_ne!(fh_a, fh_b);

    // frame_hash is PURELY keccak256(ciphertext) — recompute independently
    // (note: frame_hash takes no key argument).
    let expect_a = format!("0x{}", hex::encode(keccak256(&ct_a)));
    let expect_b = format!("0x{}", hex::encode(keccak256(&ct_b)));
    assert_eq!(fh_a, expect_a);
    assert_eq!(fh_b, expect_b);

    // The capability CID embeds the key, so it differs per key even though the
    // plaintext is identical; it is the `u`-prefixed key-bearing form.
    let pad = padding_for(pt.len()) as u32;
    let cap_a = capability_cid(&pt, &ct_a, &key_a, pad);
    let cap_b = capability_cid(&pt, &ct_b, &key_b, pad);
    assert_ne!(cap_a, cap_b);
    assert!(cap_a.starts_with('u'));
    assert!(cap_b.starts_with('u'));
    // And a capability CID is NOT a frame hash.
    assert_ne!(cap_a, fh_a);
}

// --------------------------------------------------------------------------
// manifest Merkle root
// --------------------------------------------------------------------------

#[test]
fn test_manifest_merkle_root() {
    let job = sample_job(3);
    // Deterministic, known frame hashes from distinct ciphertext blobs.
    let hashes: Vec<String> = (0..3u8).map(|i| frame_hash(&[i; 100])).collect();

    let m = build_manifest(&hashes, &job).unwrap();

    // Recompute the Merkle root over the RAW 32-byte decoded leaves.
    let mut tree = MerkleTree::new();
    for h in &hashes {
        let raw = hex::decode(h.trim_start_matches("0x")).unwrap();
        let mut leaf = [0u8; 32];
        leaf.copy_from_slice(&raw);
        tree.add_leaf(leaf);
    }
    let expected_root = format!("0x{}", hex::encode(tree.root()));

    assert_eq!(m.merkle_root, expected_root);
    assert_eq!(m.frame_count, 3);
    assert_eq!(m.fps, job.fps);
    assert_eq!(m.resolution, job.resolution);
    assert_eq!(m.frame_hashes, hashes);

    // Deterministic across calls.
    let m2 = build_manifest(&hashes, &job).unwrap();
    assert_eq!(m, m2);
}

// --------------------------------------------------------------------------
// manifest labels / keyless invariant
// --------------------------------------------------------------------------

#[test]
fn test_manifest_labels_colour() {
    let job = sample_job(2);
    let hashes = vec![frame_hash(b"frame-a"), frame_hash(b"frame-b")];
    let m = build_manifest(&hashes, &job).unwrap();

    assert_eq!(m.colour_encoding, "linear-HDR-from-LogC3");

    // The manifest must carry NO key-bearing capability CID. Serialise it and
    // assert no string value is a `u`-prefixed capability CID (those are long
    // base64url blobs; frame hashes are short `0x`-hex).
    let json = serde_json::to_string(&m).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        !has_capability_cid(&v),
        "manifest must not hold a u-prefixed capability CID: {json}"
    );

    // Every frame hash is the keyless 0x-hex form, not a capability CID.
    for h in &m.frame_hashes {
        assert!(h.starts_with("0x"));
        assert!(!h.starts_with('u'));
    }
}

/// True if any JSON string value looks like a `u`-prefixed S5 capability CID.
fn has_capability_cid(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(s) => s.starts_with('u') && s.len() > 40,
        serde_json::Value::Array(a) => a.iter().any(has_capability_cid),
        serde_json::Value::Object(o) => o.values().any(has_capability_cid),
        _ => false,
    }
}

/// Emit `tests/ltx/capability-fixture.json` — ONE real node-encrypted frame for the
/// SDK to verify its `downloadDecryptedByCID` byte-for-byte (not mocked). A
/// multi-chunk frame so the SDK exercises the `0xae` envelope parse AND the chunk
/// loop, per-chunk `le(i,24)` nonce, per-chunk Poly1305 tag, padding, and the
/// truncate-to-size. Deterministic + fixed key, so it regenerates identically.
#[test]
fn emit_capability_fixture() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;

    let key: [u8; 32] = std::array::from_fn(|i| i as u8); // fixed test key 0x00..0x1f
    let size = CHUNK + 40_000; // 1 full 256 KiB chunk + a partial -> multi-chunk
    let plaintext: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

    let ciphertext = encrypt_frame(&plaintext, &key).unwrap();
    let padding = padding_for(plaintext.len());
    let cap = capability_cid(&plaintext, &ciphertext, &key, padding as u32);
    let ct_hash = blake3::hash(&ciphertext); // the blob hash embedded in the envelope
    let pt_hash = blake3::hash(&plaintext);

    // What the SDK does: parse the envelope -> (key, blobHash, size), fetch the blob
    // by blobHash, decrypt with (key, size). `decrypt_frame` is the node mirror; assert
    // it reproduces the plaintext so the fixture is self-proving.
    assert_eq!(
        decrypt_frame(&ciphertext, &key, plaintext.len()).unwrap(),
        plaintext
    );

    let fixture = serde_json::json!({
        "_note": "Capability-CID interop fixture (node->SDK). Generated by tests/ltx/test_exr.rs::emit_capability_fixture; do not hand-edit. SDK test: multibase-decode the u-CID, parse the 0xae envelope to (key, blobHash, size), decrypt ciphertextBase64 with (key,size), assert it equals plaintextBase64 byte-for-byte; and keccak256(ciphertext) === frameHashKeccak256OfCiphertext.",
        "encoding": "plaintext/ciphertext are standard base64; key/hashes are 0x-hex; sizes are integers",
        "chunkSize": CHUNK,
        "tagSize": TAG,
        "chunkCount": size.div_ceil(CHUNK),
        "capabilityCid": cap,
        "envelope": {
            "layout": "u + base64url( 0xae, 0xa6, 0x12(=18), 0x1f, blake3(ciphertext)[32], key[32], paddingLE[4], 0x26, 0x1f, blake3(plaintext)[32], sizeLE-trimmed )",
            "key": format!("0x{}", hex::encode(key)),
            "blobHashBlake3OfCiphertext": format!("0x{}", hex::encode(ct_hash.as_bytes())),
            "ptHashBlake3OfPlaintext": format!("0x{}", hex::encode(pt_hash.as_bytes())),
            "size": plaintext.len(),
            "padding": padding,
        },
        "frameHashKeccak256OfCiphertext": frame_hash(&ciphertext),
        "plaintextBase64": b64.encode(&plaintext),
        "ciphertextBase64": b64.encode(&ciphertext),
    });

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/ltx/capability-fixture.json"
    );
    std::fs::write(path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
    assert!(std::path::Path::new(path).exists());
}
