//! Design §7 / D8: the file policy source, the model-id check, the keyring floor,
//! `no_provider` for a missing file, and the signer/window checks it inherits.

use super::policy_fixture::{recording_policy, sign, signer, test_model_id};
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::kbs::policy_file::{load_policy, FilePolicySource};
use fabstir_llm_node::tee::policy_source::ProviderRegistry;
use std::path::Path;

fn write(dir: &Path, model_id: [u8; 32], json: &[u8]) {
    std::fs::write(dir.join(format!("{}.json", hex::encode(model_id))), json).unwrap();
}

#[tokio::test]
async fn loads_a_valid_policy_and_refuses_the_floor_below_it() {
    let dir = tempfile::tempdir().unwrap();
    let id = test_model_id(1);
    let sk = signer();
    let (signed, provider) = sign(&recording_policy(id, 3), &sk);
    write(dir.path(), id, &serde_json::to_vec(&signed).unwrap());
    let src = FilePolicySource::new(dir.path());
    let reg = ProviderRegistry::new().with_provider(id, provider);
    let got = load_policy(&src, &reg, id, 3).await.unwrap();
    assert_eq!(got.policy.policy_version, 3);
    let e = load_policy(&src, &reg, id, 4).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(e.detail.contains("below keyring floor"), "{e}");
}

#[tokio::test]
async fn a_policy_for_model_b_under_a_file_named_for_a_is_refused_on_model_id() {
    // Same signer for A and B: with different signers the signer row would mask the mutation.
    let dir = tempfile::tempdir().unwrap();
    let a = test_model_id(1);
    let b = test_model_id(2);
    let sk = signer();
    let (signed_b, provider) = sign(&recording_policy(b, 1), &sk);
    write(dir.path(), a, &serde_json::to_vec(&signed_b).unwrap());
    let src = FilePolicySource::new(dir.path());
    let reg = ProviderRegistry::new()
        .with_provider(a, provider.clone())
        .with_provider(b, provider);
    let e = load_policy(&src, &reg, a, 1).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(e.detail.contains("model id"), "{e}");
}

#[tokio::test]
async fn missing_file_is_no_provider_and_wrong_signer_or_window_is_verification() {
    let dir = tempfile::tempdir().unwrap();
    let id = test_model_id(1);
    let src = FilePolicySource::new(dir.path());
    let sk = signer();
    let (signed, provider) = sign(&recording_policy(id, 1), &sk);
    let reg = ProviderRegistry::new().with_provider(id, provider);
    let e = load_policy(&src, &reg, id, 1).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::NoProvider, 404));

    // wrong signer: registry binds another address
    write(dir.path(), id, &serde_json::to_vec(&signed).unwrap());
    let other =
        ProviderRegistry::new().with_provider(id, "0x0000000000000000000000000000000000000001");
    let e = load_policy(&src, &other, id, 1).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification);

    // expired window
    let mut p = recording_policy(id, 1);
    p.expiry = p.not_before + 1;
    let (signed_expired, provider) = sign(&p, &sk);
    write(
        dir.path(),
        id,
        &serde_json::to_vec(&signed_expired).unwrap(),
    );
    let reg = ProviderRegistry::new().with_provider(id, provider);
    let e = load_policy(&src, &reg, id, 1).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification);

    // a file that does not parse (a half-copied swap) is a broker fault, never a permanent
    // verification refusal for the node
    write(dir.path(), id, b"not json");
    let e = load_policy(&src, &reg, id, 1).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
}

#[tokio::test]
async fn an_unreadable_policy_dir_is_a_broker_fault() {
    let dir = tempfile::tempdir().unwrap();
    let id = test_model_id(1);
    let path = dir.path().join(format!("{}.json", hex::encode(id)));
    std::fs::create_dir(&path).unwrap(); // a directory where the file should be
    let src = FilePolicySource::new(dir.path());
    let reg =
        ProviderRegistry::new().with_provider(id, "0x0000000000000000000000000000000000000001");
    let e = load_policy(&src, &reg, id, 1).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
}

#[test]
fn path_is_lowercase_hex() {
    let src = FilePolicySource::new("/x");
    assert_eq!(
        src.path_for(&[0xAB; 32]).to_string_lossy(),
        format!("/x/{}.json", "ab".repeat(32))
    );
}

#[test]
fn tool_signed_fixture_verifies_under_the_kbs_feature() {
    // The other half of the preserve_order proof lives in tests/tee (feature
    // off): both documents were signed by the tool built WITH this feature.
    use fabstir_llm_node::tee::host_profile::{
        parse_bundle, parse_signed_document, verify_manifest, verify_set,
    };
    use fabstir_llm_node::tee::types::{POLICY_DOC, POLICY_SCHEMA_VERSION};
    let signed: fabstir_llm_node::tee::policy::SignedModelPolicy = parse_signed_document(
        &super::fixtures::fixture("v3-policy-signed.json"),
        "fixture",
        "/policy",
        POLICY_DOC,
        POLICY_SCHEMA_VERSION,
    )
    .unwrap();
    signed
        .verify_signer("0x18abb9ca9428d272c8796158b1f5ac321855a5bd")
        .unwrap();
    assert_eq!(
        hex::encode(signed.policy_hash().unwrap()),
        "46977c3b366ad8e1a32d9870a9d39d1a2dffe5820475b84eea469a3debfe7f9f"
    );
    signed.policy.validate().unwrap();
    let b = parse_bundle(
        &super::fixtures::fixture("authority-bundle.json"),
        "fixture",
    )
    .unwrap();
    verify_manifest(&b.manifest, "0xd84fbf720e8a90fabf5fc1ca07f20be85b508420").unwrap();
    let counted = verify_set(&b.set, &b.manifest.manifest).unwrap();
    assert_eq!(
        counted.signers,
        ["0x76c3ad3e393994992d754584e9fc7d6006a2c3fb"]
    );
}
