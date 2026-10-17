//! Policy schema 3 (V3.2) at route level (D7, D8, D10, D17d): `challenge`
//! validates the three documents but needs no candidate; it refuses before any
//! nonce when the bundle is missing, invalid or rolled back; the prefilter's
//! self-explaining refusal costs zero egress; the removed-profile hint needs no
//! archive IO on the refusal path; a broken clock is a fault; the released
//! record names the profile, the digests and the signers. Each test names the
//! mutation that turns it red.

use super::policy_fixture::{
    bundle, recording_policy, recording_profile, sign, write_bundle, NOW, PROFILE_ID,
};
use super::test_gpu::NrasOpts;
use super::test_routes::{evidence, node_client, spawn_broker, spawn_broker_with, Broker};
use fabstir_llm_node::tee::host_profile::{AuthorityBundle, HostProfile};
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::keywrap::generate_ephemeral_keypair;
use fabstir_llm_node::tee::types::TeeError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn serve(b: &Broker, bundle: &AuthorityBundle) {
    write_bundle(&b.state.cfg.authority_dir, bundle);
}

/// The recording profile under another id, with its mrtd changed: a host
/// shape that does NOT match the recording's evidence.
fn elsewhere(id: &str) -> HostProfile {
    let mut p = recording_profile();
    p.id = id.into();
    p.cvm.mrtd = "00".repeat(48);
    p
}

async fn challenge(b: &Broker) -> Result<[u8; 32], TeeError> {
    let (_, pk) = generate_ephemeral_keypair();
    node_client(b).challenge(b.model_id, &pk).await
}

/// challenge + request_key with the recording's evidence; the refusal text.
async fn request(b: &Broker) -> Result<(), String> {
    let client = node_client(b);
    let (_, pk) = generate_ephemeral_keypair();
    let nonce = client
        .challenge(b.model_id, &pk)
        .await
        .map_err(|e| e.to_string())?;
    client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn challenge_validates_documents_but_needs_no_candidate() {
    // A set whose only profile is of a class the policy does not allow: zero
    // candidates, yet a nonce is issued so the node reaches request_key and its
    // refusal is captured (D7, D18). Mutation: require a candidate at challenge.
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let mut other = recording_profile();
    other.class = "other-class".into();
    serve(&b, &bundle(1, 2, vec![other]));
    challenge(&b)
        .await
        .expect("a nonce for a host with no candidate");
    assert_eq!(b.state.nonces.outstanding(), 1);
}

#[tokio::test]
async fn challenge_refuses_before_a_nonce_when_bundle_missing_invalid_or_hwm_rolled_back() {
    // Mutation: load the bundle at step 3 only → each case issues a nonce.
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let file = b
        .state
        .authority
        .path_for(&recording_policy(b.model_id, 1).host_authority.root);
    // Missing → no_provider.
    std::fs::remove_file(&file).unwrap();
    let e = challenge(&b).await.unwrap_err().to_string();
    assert!(
        e.contains("no key/policy") && e.contains("no authority bundle"),
        "{e}"
    );
    assert_eq!(b.state.nonces.outstanding(), 0);
    // Invalid (a set signature that does not recover) → verification.
    let mut bad = bundle(1, 2, vec![recording_profile()]);
    bad.set.signatures[0][64] = 9;
    serve(&b, &bad);
    let e = challenge(&b).await.unwrap_err().to_string();
    assert!(e.contains("does not recover"), "{e}");
    assert_eq!(b.state.nonces.outstanding(), 0);
    // Epoch 3 loads (one nonce); epoch 2 re-served is a rollback → refused.
    serve(&b, &bundle(1, 3, vec![recording_profile()]));
    challenge(&b).await.unwrap();
    serve(&b, &bundle(1, 2, vec![recording_profile()]));
    let e = challenge(&b).await.unwrap_err().to_string();
    assert!(e.contains("epoch 2 is below the high-water mark 3"), "{e}");
    assert_eq!(b.state.nonces.outstanding(), 1, "only the epoch-3 nonce");
}

#[tokio::test]
async fn prefilter_refuses_before_egress_when_no_candidate_matches() {
    // Candidates exist but none matches the evidence. Mutation: skip the
    // prefilter's identity rows → the request reaches NRAS (hits > 0) before
    // step 7 refuses.
    let b = spawn_broker(&[], NrasOpts::default()).await;
    serve(&b, &bundle(1, 2, vec![elsewhere("a"), elsewhere("b")]));
    let e = request(&b).await.unwrap_err();
    assert!(
        e.contains("no profile matches: closest is profile a"),
        "{e}"
    );
    assert!(e.contains("mrtd: "), "{e}");
    assert_eq!(b.nras.fake.hit_count(), 0, "refused before any egress");
}

#[tokio::test]
async fn boot_with_zero_candidates_gets_hints_from_the_prefilter_with_no_egress() {
    // The paid booking's boot 1 (D18): the policy requires pins, epoch 1 holds
    // the predicted profile UNPINNED. The refusal must say why, from the
    // prefilter, before NRAS. Mutation: compute the hints in step 7 only →
    // a bare count here.
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let mut p = recording_policy(b.model_id, 2);
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (true, true);
    let (signed, _) = sign(&p, &b.signer);
    std::fs::write(
        b.state.policy.path_for(&b.model_id),
        serde_json::to_vec(&signed).unwrap(),
    )
    .unwrap();
    let mut unpinned = recording_profile();
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    serve(&b, &bundle(1, 2, vec![unpinned]));
    let e = request(&b).await.unwrap_err();
    assert!(
        e.contains("candidates=0 of 1 (expired=0, class_filtered=0, unpinned=1)"),
        "{e}"
    );
    assert!(
        e.contains(&format!(
            "matches {PROFILE_ID} but the policy requires a pinned app id"
        )),
        "{e}"
    );
    assert_eq!(b.nras.fake.hit_count(), 0);
}

#[tokio::test]
async fn removed_profile_ledger_is_rebuilt_on_accept_and_refusal_path_does_no_io() {
    // Epoch 2 drops the recording profile. After the accept the archive dir
    // is DELETED; the refusal still names the removed profile (the cached
    // ledger, D8). Mutation: scan the archive on the refusal path → no hint.
    let b = spawn_broker(&[], NrasOpts::default()).await;
    challenge(&b).await.unwrap(); // accepts epoch 1 (archived)
    serve(&b, &bundle(1, 2, vec![elsewhere("other")]));
    challenge(&b).await.unwrap(); // accepts epoch 2: ledger rebuilt here
    std::fs::remove_dir_all(b.state.cfg.archive_dir()).unwrap();
    let e = request(&b).await.unwrap_err();
    assert!(
        e.contains(&format!(
            "matches {PROFILE_ID}, present in the archived bundle 0000000000000000000"
        )) && e.contains("but absent from epoch 2"),
        "{e}"
    );
    assert_eq!(b.nras.fake.hit_count(), 0, "the prefilter's hint (B3)");
    assert!(
        !b.state.cfg.archive_dir().exists(),
        "the refusal path wrote nothing to the archive"
    );
}

#[tokio::test]
async fn request_is_a_fault_when_the_clock_is_broken() {
    // `now_unix` returns u64::MAX on a broken clock. Mutation: map it to 0
    // instead of refusing → the request is judged at 1970 (a verification
    // refusal about windows), not a fault.
    let clock = Arc::new(AtomicU64::new(NOW));
    let c2 = clock.clone();
    let b = spawn_broker_with(&[], NrasOpts::default(), move || c2.load(Ordering::SeqCst)).await;
    let client = node_client(&b);
    let (_, pk) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    clock.store(u64::MAX, Ordering::SeqCst);
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("broker error unavailable") && e.contains("system clock unavailable"),
        "{e}"
    );
    let e = challenge(&b).await.unwrap_err().to_string();
    assert!(e.contains("system clock unavailable"), "{e}");
}

#[tokio::test]
async fn released_decision_and_verified_json_carry_profile_digests_and_signers() {
    // D17d. Mutation: omit any field from the released line / decision.txt /
    // verified.json's `profile`.
    let b = spawn_broker(&[], NrasOpts::default()).await;
    // Distinct counters, so a field written from the other is caught.
    serve(&b, &bundle(2, 3, vec![recording_profile()]));
    request(&b).await.expect("released");
    let verified = b.state.cfg.capture_dir().join("verified");
    let dir = std::fs::read_dir(&verified)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let decision = std::fs::read_to_string(dir.join("decision.txt")).unwrap();
    let auth = b
        .state
        .authority
        .load(&recording_policy(b.model_id, 1), NOW)
        .unwrap();
    let (sd, md) = (
        hex::encode(auth.set_digest),
        hex::encode(auth.manifest_digest),
    );
    let signer = &auth.counted.signers[0];
    for field in [
        format!("profile={PROFILE_ID}"),
        "class=test-class".into(),
        "epoch=3".into(),
        "manifest=2".into(),
        format!("set_digest={}", &sd[..16]),
        format!("manifest_digest={}", &md[..16]),
        format!("signers={signer}"),
    ] {
        assert!(decision.contains(&field), "missing {field:?} in {decision}");
    }
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("verified.json")).unwrap()).unwrap();
    let p = &v["profile"];
    assert_eq!(p["headline"], PROFILE_ID, "{v}");
    assert_eq!(p["matched"], serde_json::json!([PROFILE_ID]), "{v}");
    assert_eq!(p["class"], "test-class", "{v}");
    assert_eq!(
        (p["epoch"].as_u64(), p["manifest_version"].as_u64()),
        (Some(3), Some(2)),
        "{v}"
    );
    assert_eq!(
        (p["set_digest"].as_str(), p["manifest_digest"].as_str()),
        (Some(&*sd), Some(&*md))
    );
    assert_eq!(p["signers"], serde_json::json!([signer]), "{v}");
}
