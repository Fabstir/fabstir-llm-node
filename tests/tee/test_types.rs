// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 1.1 — core TEE types: serde roundtrip (task 1.1.4).

use fabstir_llm_node::tee::types::{Claims, Evidence, Policy, WrappedKey};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;

/// Round-trip a value through both serializers the TEE pipeline uses — bincode
/// (canonical container bytes) and serde_json (signed-policy path) — and assert
/// both reproduce the original. This also exercises `#[serde(with = "BigArray")]`
/// on the `[u8; 48]` measurement fields under both binary and JSON encodings,
/// which encode fixed arrays differently.
fn assert_roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let bin: T = bincode::deserialize(&bincode::serialize(value).unwrap()).unwrap();
    assert_eq!(*value, bin, "bincode roundtrip mismatch");
    let json: T = serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap();
    assert_eq!(*value, json, "serde_json roundtrip mismatch");
}

#[test]
fn test_types_roundtrip_serde() {
    // Evidence — `image_measurement: [u8; 48]` exercises `#[serde(with = "BigArray")]`.
    assert_roundtrip(&Evidence {
        gpu_report: vec![1, 2, 3, 4, 5],
        cpu_quote: vec![0u8; 64], // mock: bytes 0..64 carry report_data
        image_measurement: [7u8; 48],
        pk_att: vec![9u8; 33], // compressed secp256k1 pubkey
        nonce: [3u8; 32],
    });

    // Policy — `expected_measurement: [u8; 48]` also exercises BigArray.
    assert_roundtrip(&Policy {
        policy_version: 1,
        allowed_skus: vec!["H100".to_string(), "H200".to_string()],
        expected_measurement: [7u8; 48],
        require_cc_on: true,
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 1_000,
        expiry: 2_000,
        model_id: [5u8; 32],
    });

    assert_roundtrip(&Claims {
        verified_at: 1_234,
        gpu_report_hash: [2u8; 32],
        measurement_verified: true,
    });

    assert_roundtrip(&WrappedKey {
        eph_pub: vec![4u8; 33],
        nonce: [6u8; 24],
        ciphertext: vec![8u8; 48],
    });
}
