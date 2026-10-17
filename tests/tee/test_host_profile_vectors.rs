// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Policy schema 3 (V3.1): the node's canonical bytes and EIP-191 digest
//! against an INDEPENDENT implementation (ethers 6.17), so the signing path
//! cannot be self-consistent and wrong. Split from `test_host_profile.rs`
//! (400-line cap).

use super::profile_fixture::a_manifest;
use fabstir_llm_node::tee::host_profile::{verify_manifest, SignedAuthorityManifest};

#[test]
fn canonical_bytes_and_eip191_match_an_independent_wallet() {
    // ethers 6.17 (an independent implementation) computed these: the
    // well-known `hashMessage("hello")`, and a wallet `signMessage` over the
    // canonical manifest below with the throw-away key 0x11…11. A wrong EIP-191
    // prefix, a non-compact or unsorted canonical form, or a recovery that
    // disagrees with real wallets (D2: hardware-wallet signatures) turns this red.
    use fabstir_llm_node::tee::host_profile::{canonical_bytes, eip191_digest};
    assert_eq!(
        hex::encode(eip191_digest(b"hello")),
        "50b2c43fd39106bafbba0da34fc430e1f91e3c96ea2acee2bc34119f92b37750"
    );
    let root = "0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a";
    let mut m = a_manifest(root, &[format!("0x{}", "2".repeat(40))], 1);
    m.expiry = 4_102_444_800;
    let body = format!(
        r#"{{"doc":"fabstir-host-authority-manifest","expiry":4102444800,"manifest_version":1,"not_before":0,"root":"{root}","schema_version":1,"signers":["0x{}"],"threshold":1}}"#,
        "2".repeat(40)
    );
    assert_eq!(
        String::from_utf8(canonical_bytes(&m).unwrap()).unwrap(),
        body
    );
    assert_eq!(
        hex::encode(eip191_digest(body.as_bytes())),
        "a644ea74033dbf7f2ed0f589abaa571f08201ccb0238dc83d62c61a1a7aef72c"
    );
    let sig = hex::decode("60ddc1226c9743edf7de7b8f92d53d232ae704b3d5a64a93dd1a4240d42ee34943631b3e61e84e1faa69ffaaea04ef38cc9b11cfc5135899873889461bde43a41c").unwrap();
    verify_manifest(
        &SignedAuthorityManifest {
            manifest: m,
            signature: sig,
        },
        root,
    )
    .expect("a wallet's personal_sign over the canonical bytes verifies");
}
