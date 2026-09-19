// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The `preserve_order` proof, node side (this crate runs WITHOUT the `kbs`
//! feature): `tests/kbs/fixtures/test-policy-signed.json` was signed by the
//! broker tool built WITH the feature (dcap-qvl's `std` turns on
//! `serde_json/preserve_order` for every such build). The node's own
//! `canonical_policy_bytes` (BTreeMap keys) must recover the same signer, or the
//! two sides' canonicalisation has diverged. Mutation: replace `sort_json_keys`
//! by identity on this side → the signature fails.

use fabstir_llm_node::tee::policy::SignedModelPolicy;
use std::path::PathBuf;

const SIGNER: &str = "0xcc259c75aa6dd43c127d5c0b6a32594640f2e2b6";
const POLICY_HASH: &str = "6cb0a505e4f4e63d66f3a1146f63a699611c77834e680e5de56882b52974cd2f";

fn fixture() -> SignedModelPolicy {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/kbs/fixtures/test-policy-signed.json");
    serde_json::from_slice(&std::fs::read(path).expect("fixture present")).expect("signed policy")
}

#[test]
fn tool_signed_fixture_verifies_on_the_node_path() {
    let signed = fixture();
    assert_eq!(signed.signer, SIGNER);
    signed
        .verify_signer(SIGNER)
        .expect("the node path recovers the tool's signer");
    assert_eq!(hex::encode(signed.policy_hash().unwrap()), POLICY_HASH);
    signed.policy.validate().unwrap();
    assert!(signed
        .verify_signer("0x0000000000000000000000000000000000000001")
        .is_err());
}
