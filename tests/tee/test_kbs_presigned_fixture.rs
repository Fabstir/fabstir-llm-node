// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The `preserve_order` proof, node side (this crate runs WITHOUT the `kbs`
//! feature; D23): `tests/kbs/fixtures/v3-policy-signed.json` and
//! `authority-bundle.json` were signed by the broker tool built WITH the
//! feature (dcap-qvl's `std` turns on `serde_json/preserve_order` for every
//! such build). The node's own `canonical_bytes` (BTreeMap keys) must recover
//! the same signers for all three documents, or the two sides'
//! canonicalisation has diverged. Mutation (in the kbs build): replace
//! `sort_json_keys` by identity → the signatures fail.

use fabstir_llm_node::tee::host_profile::{parse_bundle, verify_manifest, verify_set};
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use std::path::PathBuf;

const PROVIDER: &str = "0x18abb9ca9428d272c8796158b1f5ac321855a5bd";
const POLICY_HASH: &str = "46977c3b366ad8e1a32d9870a9d39d1a2dffe5820475b84eea469a3debfe7f9f";
const ROOT: &str = "0xd84fbf720e8a90fabf5fc1ca07f20be85b508420";
const SIGNER: &str = "0x76c3ad3e393994992d754584e9fc7d6006a2c3fb";

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/kbs/fixtures")
        .join(name);
    std::fs::read(path).expect("fixture present")
}

#[test]
fn tool_signed_fixture_verifies_on_the_node_path() {
    let signed: SignedModelPolicy =
        serde_json::from_slice(&fixture("v3-policy-signed.json")).expect("signed policy");
    assert_eq!(signed.signer, PROVIDER);
    signed
        .verify_signer(PROVIDER)
        .expect("the node path recovers the tool's provider");
    assert_eq!(hex::encode(signed.policy_hash().unwrap()), POLICY_HASH);
    signed.policy.validate().unwrap();
    assert!(signed
        .verify_signer("0x0000000000000000000000000000000000000001")
        .is_err());

    let b = parse_bundle(&fixture("authority-bundle.json"), "fixture").expect("bundle");
    verify_manifest(&b.manifest, ROOT).expect("the node path recovers the root");
    assert_eq!(b.manifest.manifest.root, signed.policy.host_authority.root);
    let counted = verify_set(&b.set, &b.manifest.manifest).expect("the set verifies");
    assert_eq!(counted.signers, [SIGNER]);
    assert!(counted.ignored.is_empty());
}
