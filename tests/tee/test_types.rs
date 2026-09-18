// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 1.1 — core TEE types: serde roundtrip (task 1.1.4).

use fabstir_llm_node::tee::types::{
    version_at_least, CcMode, Claims, CvmPolicy, Evidence, GpuPolicy, Policy, WrappedKey,
};
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

fn a_policy() -> Policy {
    Policy {
        schema_version: 2,
        policy_version: 1,
        model_id: [5u8; 32],
        not_before: 1_000,
        expiry: 2_000,
        cvm: CvmPolicy {
            mrtd: hex::encode([7u8; 48]),
            rtmr0: "00".repeat(48),
            rtmr1: "00".repeat(48),
            rtmr2: "00".repeat(48),
            os_image_hash: "00".repeat(32),
            compose_hash: "00".repeat(32),
            app_id: None,
            key_provider: None,
            require_td_debug_off: true,
            allowed_tcb_status: vec!["UpToDate".to_string()],
            allowed_advisory_ids: vec![],
        },
        gpu: GpuPolicy {
            allowed_hwmodels: vec!["H100".to_string(), "H200".to_string()],
            require_cc_mode: Some(CcMode::On),
            require_secure_boot: true,
            require_debug_disabled: true,
            min_driver_version: None,
            min_vbios_version: None,
        },
    }
}

#[test]
fn version_floor_compares_components_as_hex_case_insensitively() {
    // Phase 5 P3 converge: VBIOS components are hex (`96.00.9f.00.01`); a
    // decimal-only parse fell back to string order, which put `ff` above `100`
    // and made `A0` != `a0`.
    for (have, floor, ok) in [
        ("96.00.9f.00.01", "96.00.a0", false),
        ("96.00.A0", "96.00.9f", true),
        ("96.00.A0", "96.00.a0", true), // equal, whatever the case
        ("96.00.a0", "96.00.A0", true),
        ("96.00.ff", "96.00.100", false), // 0xff < 0x100
        ("96.00.100", "96.00.ff", true),
        // Decimal driver strings keep their order under the hex rule.
        ("580.95.05", "580.95", true),
        ("580.95", "580.95.05", false),
        ("581.0", "580.95.05", true),
        ("580.100", "580.95", true),
        ("580.95.05", "580.95.05", true),
        // Empty components are zero, like missing ones (round 46).
        ("580.95.05.", "580.95.05", true),
        ("580..95", "580.0.95", true),
        ("580.95.05", "580.95.05.", true),
        ("580.95.04.", "580.95.05", false),
        // Non-hex on either side refuses; never a lexical fallback (round 55).
        ("unknown", "580.95", false),
        ("r580.95.05", "580.95", false),
        ("N/A", "1", false),
        ("580.95.05", "unknown", false),
        ("96.00.9f.00.01", "96.00.zz", false),
    ] {
        assert_eq!(version_at_least(have, floor), ok, "{have} >= {floor}");
    }
}

#[test]
fn policy_validation_refuses_malformed_version_floors() {
    // Round 65: a floor `version_at_least` could never satisfy (non-hex, empty
    // component, empty string) is refused when the policy is loaded, not
    // discovered as a failed release on the GPU day.
    for (driver, vbios, ok) in [
        (Some("580.95.05"), Some("96.00.9f.00.01"), true),
        (None, None, true),
        (Some("r580.95"), None, false),
        (Some("580..95"), None, false),
        (Some(""), None, false),
        (Some("580.95."), None, false),
        (None, Some("96.00.zz"), false),
        (None, Some("N/A"), false),
    ] {
        let mut p = a_policy();
        p.gpu.min_driver_version = driver.map(str::to_string);
        p.gpu.min_vbios_version = vbios.map(str::to_string);
        assert_eq!(
            p.validate().is_ok(),
            ok,
            "driver {driver:?} vbios {vbios:?}"
        );
    }
}

#[test]
fn policy_validation_reports_a_multibyte_value_without_panicking() {
    // The refusal message truncates the offending value for the log; slicing it
    // by BYTE could land inside a multibyte char and panic (a hostile policy
    // must be refused, never crash the node).
    let mut p = a_policy();
    p.cvm.mrtd = "\u{20ac}".repeat(30); // 90 bytes, 30 chars; byte 20 is mid-char
    let err = p.validate().expect_err("non-hex mrtd is refused");
    let msg = err.to_string();
    assert!(msg.contains("cvm.mrtd"), "{msg}");
    assert!(msg.contains(&"\u{20ac}".repeat(20)), "{msg}");
    assert!(
        !msg.contains(&"\u{20ac}".repeat(21)),
        "truncated at 20 chars: {msg}"
    );
    // And the good form still validates.
    a_policy().validate().expect("well-formed policy");
}

#[test]
fn test_types_roundtrip_serde() {
    // Evidence — `image_measurement: [u8; 48]` exercises `#[serde(with = "BigArray")]`.
    assert_roundtrip(&Evidence {
        gpu_report: vec![1, 2, 3, 4, 5],
        cpu_quote: vec![0u8; 64], // mock: bytes 0..64 carry report_data
        // Phase 5 (P2.1): the dstack event log and VM config ride along as
        // opaque UTF-8 JSON; real values are ~KB, these just prove the wire.
        event_log: br#"[{"imr":3,"event_type":134217729,"digest":"ab","event":"compose-hash","event_payload":"cd"}]"#.to_vec(),
        vm_config: br#"{"spec_version":1,"cpu_count":4,"memory_size":8589934592}"#.to_vec(),
        image_measurement: [7u8; 48],
        pk_att: vec![9u8; 33], // compressed secp256k1 pubkey
        nonce: [3u8; 32],
    });

    // Policy v2 — nested cvm/gpu objects, hex strings, Options, both serializers.
    assert_roundtrip(&Policy {
        schema_version: 2,
        policy_version: 1,
        model_id: [5u8; 32],
        not_before: 1_000,
        expiry: 2_000,
        cvm: CvmPolicy {
            mrtd: hex::encode([7u8; 48]),
            rtmr0: "00".repeat(48),
            rtmr1: "00".repeat(48),
            rtmr2: "00".repeat(48),
            os_image_hash: "00".repeat(32),
            compose_hash: "00".repeat(32),
            app_id: None,
            key_provider: None,
            require_td_debug_off: true,
            allowed_tcb_status: vec!["UpToDate".to_string()],
            allowed_advisory_ids: vec![],
        },
        gpu: GpuPolicy {
            allowed_hwmodels: vec!["H100".to_string(), "H200".to_string()],
            require_cc_mode: Some(CcMode::On),
            require_secure_boot: true,
            require_debug_disabled: true,
            min_driver_version: None,
            min_vbios_version: None,
        },
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
