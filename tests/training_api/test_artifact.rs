// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T4.c: the D.1 splitter (pinned by the shiftedRemainder vector), sharded
//! encrypt+upload round-trips, and the canonical artifact-manifest-v1 build.

use fabstir_llm_node::ltx::input_image::parse_capability_cid;
use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Storage};
use fabstir_llm_node::training::artifact::{
    shard_sizes, upload_artifact_manifest, upload_file_sharded,
};
use fabstir_llm_node::training::staging::SHARD_PLAINTEXT_MAX_BYTES;

const MAX: u64 = SHARD_PLAINTEXT_MAX_BYTES;
const CHUNK: u64 = 262_144;

#[test]
fn splitter_small_single_shard() {
    assert_eq!(shard_sizes(1).unwrap(), vec![1]);
    assert_eq!(shard_sizes(MAX).unwrap(), vec![MAX]);
}

#[test]
fn splitter_cuts_at_exactly_max_with_remainder_last() {
    assert_eq!(shard_sizes(MAX + 5).unwrap(), vec![MAX, 5]);
    assert_eq!(shard_sizes(3 * MAX).unwrap(), vec![MAX, MAX, MAX]);
}

#[test]
fn splitter_shifted_remainder_pins_the_vector() {
    // The manifests.json shiftedRemainder case: 25,686,016 = MAX + 524,288
    // (= 2 × 262,144) → [MAX, 524_287, 1].
    assert_eq!(
        shard_sizes(25_686_016).unwrap(),
        vec![MAX, 524_287, 1],
        "the exact-multiple remainder must shift one byte"
    );
    // The minimal multiple too.
    assert_eq!(shard_sizes(CHUNK).unwrap(), vec![CHUNK - 1, 1]);
}

#[test]
fn splitter_properties_hold_over_a_sweep() {
    for total in [
        1u64,
        CHUNK - 1,
        CHUNK,
        CHUNK + 1,
        2 * CHUNK,
        MAX - 1,
        MAX,
        MAX + 1,
        MAX + CHUNK,
        2 * MAX + 3 * CHUNK,
        5 * MAX + 7,
    ] {
        let sizes = shard_sizes(total).unwrap();
        assert_eq!(sizes.iter().sum::<u64>(), total, "sum for {total}");
        for (i, s) in sizes.iter().enumerate() {
            assert!(*s > 0 && *s <= MAX, "shard {i} of {total}: {s}");
            assert!(
                s % CHUNK != 0,
                "shard {i} of {total} is an exact multiple: {s}"
            );
        }
    }
    assert!(shard_sizes(0).is_err(), "zero bytes must refuse");
}

#[tokio::test]
async fn sharded_upload_round_trips_and_manifest_is_canonical() {
    use sha2::{Digest, Sha256};
    let s5 = MockS5Backend::new();
    // Big enough to force the shifted-remainder branch: MAX + 2×CHUNK.
    let total = (MAX + 2 * CHUNK) as usize;
    let bytes: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
    let entry = upload_file_sharded(&s5, "home/training/job_42", "optimizer.bin", &bytes)
        .await
        .expect("uploads");
    assert_eq!(entry.name, "optimizer.bin");
    assert_eq!(entry.size_bytes, total as u64);
    assert_eq!(
        entry.sha256,
        format!("0x{}", hex::encode(Sha256::digest(&bytes)))
    );
    assert_eq!(
        entry
            .shards
            .iter()
            .map(|s| s.size_bytes)
            .collect::<Vec<_>>(),
        vec![MAX, 2 * CHUNK - 1, 1]
    );
    // Every shard's capability CID must decrypt (via the envelope's own key)
    // back to the exact plaintext slice, and its sha256 claim must hold.
    let mut offset = 0usize;
    for (i, shard) in entry.shards.iter().enumerate() {
        let envelope = parse_capability_cid(&shard.cid).expect("capability parses");
        assert_eq!(envelope.plaintext_len as u64, shard.size_bytes, "shard {i}");
        let expected = &bytes[offset..offset + shard.size_bytes as usize];
        assert_eq!(
            shard.sha256,
            format!("0x{}", hex::encode(Sha256::digest(expected))),
            "shard {i} sha"
        );
        // The ciphertext blob is retrievable by its ct-hash CID and decrypts.
        let ct = s5
            .get_by_cid(&shard.cid)
            .await
            .or(s5
                .get(&format!("home/training/job_42/optimizer.bin.shard{i}"))
                .await)
            .expect("ciphertext stored");
        let pt =
            fabstir_llm_node::ltx::exr::decrypt_frame(&ct, &envelope.key, envelope.plaintext_len)
                .expect("decrypts");
        assert_eq!(pt, expected, "shard {i} round-trips");
        offset += shard.size_bytes as usize;
    }

    // The manifest: canonical bytes, D.3 shape, encrypted upload.
    let manifest_ref = upload_artifact_manifest(
        &s5,
        "home/training/job_42",
        "checkpoint",
        Some(4),
        std::slice::from_ref(&entry),
    )
    .await
    .expect("manifest uploads");
    let envelope = parse_capability_cid(&manifest_ref.manifest_cid).expect("manifest capability");
    let ct = s5
        .get(&"home/training/job_42/manifest.checkpoint.4".to_string())
        .await
        .expect("manifest ciphertext stored");
    let pt = fabstir_llm_node::ltx::exr::decrypt_frame(&ct, &envelope.key, envelope.plaintext_len)
        .expect("manifest decrypts");
    assert_eq!(
        manifest_ref.manifest_sha256,
        format!("0x{}", hex::encode(Sha256::digest(&pt))),
        "manifestSha256 = SHA256(exact stored canonical bytes)"
    );
    let value: serde_json::Value = serde_json::from_slice(&pt).unwrap();
    assert_eq!(value["schema"], "artifact-manifest-v1");
    assert_eq!(value["kind"], "checkpoint");
    assert_eq!(value["sliceIndex"], 4);
    assert_eq!(value["files"][0]["name"], "optimizer.bin");
    assert_eq!(value["files"][0]["shards"].as_array().unwrap().len(), 3);
    // Canonical = recursively key-sorted compact: byte-equality check.
    let canonical = fabstir_llm_node::training::attestation::canonical_manifest_bytes(&value);
    assert_eq!(pt, canonical.into_bytes(), "stored bytes ARE canonical");
    // Fresh keys per artifact: two uploads of the SAME bytes differ.
    let entry2 = upload_file_sharded(&s5, "home/training/job_43", "optimizer.bin", &bytes)
        .await
        .unwrap();
    assert_ne!(
        entry.shards[0].cid, entry2.shards[0].cid,
        "per-artifact fresh key => different capability"
    );
}
