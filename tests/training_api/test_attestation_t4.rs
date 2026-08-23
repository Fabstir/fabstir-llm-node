// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T4.d: the B.3 slice attestation — field shape, EIP-191 signature
//! recovery to the node address, canonical stored bytes, proofHash over the
//! exact uploaded bytes, and the final-slice extras.

use ethers::core::types::Signature;
use ethers::signers::{LocalWallet, Signer};
use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Storage};
use fabstir_llm_node::training::attestation::{
    build_slice_attestation, canonical_manifest_bytes, sig_digest, upload_slice_attestation,
    SliceAttestationInputs, SliceSigFields,
};
use fabstir_llm_node::training::types::TrainingJob;
use sha2::{Digest, Sha256};

// hardhat #0 — the vectors' key; SYNTHETIC test key, never production.
const KEY_HEX: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

fn fixture_job() -> TrainingJob {
    serde_json::from_value(serde_json::json!({
        "templateId": "train-qlora-synthetic-test-v1",
        "templateHash": "0x" .to_string() + &"11".repeat(32),
        "dataset": {
            "manifestCID": "uAAA",
            "manifestSha256": "0x".to_string() + &"22".repeat(32),
            "declaredTokens": 4_339_200u64,
            "samples": 5000
        },
        "epochs": 2,
        "hyper": { "rank": 16, "alpha": 32, "lr": "0.000200",
                   "seed": "18446744073709551629", "seqLen": 2048 },
        "output": "adapter-v1"
    }))
    .unwrap()
}

fn key_bytes() -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(&hex::decode(KEY_HEX).unwrap());
    key
}

fn inputs(job: &TrainingJob) -> SliceAttestationInputs<'_> {
    SliceAttestationInputs {
        job,
        model_id: format!("0x{}", "aa".repeat(32)),
        template_hash: format!("0x{}", "11".repeat(32)),
        env_hash: format!("0x{}", "33".repeat(32)),
        slice_index: 4,
        step_from: 4000,
        step_to: 5000,
        tokens_delta: 1_000_000,
        cumulative_tokens: 5_000_000,
        checkpoint_manifest_sha256: format!("0x{}", "44".repeat(32)),
        adapter_manifest_sha256: None,
        moderation: None,
        session_id: 931,
        host: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(), // hardhat #0
        timestamp: 1_790_000_000,
    }
}

#[tokio::test]
async fn attestation_shape_signature_and_proof_hash() {
    let job = fixture_job();
    let (value, stored) = build_slice_attestation(&inputs(&job), &key_bytes()).expect("builds");

    // B.3 field shape.
    assert_eq!(value["sliceIndex"], 4);
    assert_eq!(value["tokensDelta"], 1_000_000);
    assert_eq!(value["cumulativeTokens"], 5_000_000);
    assert_eq!(value["sessionId"], "0x3a3"); // 931 as 0x-hex string
    assert!(value.get("adapterManifestSha256").is_none());
    assert!(value.get("moderation").is_none());

    // Stored bytes ARE the canonical serialisation.
    assert_eq!(
        stored,
        canonical_manifest_bytes(&value).into_bytes(),
        "stored attestation bytes must be canonical"
    );

    // T4 round gap 4: the informational fields must BE in the JSON — a
    // drifted timestamp with a faithful digest would break SDK verification.
    assert_eq!(value["timestamp"], 1_790_000_000u64);
    assert_eq!(value["stepFrom"], 4000);
    assert_eq!(value["stepTo"], 5000);

    // The signature recovers over a digest rebuilt ENTIRELY from the JSON's
    // own values (timestamp included) — pinning JSON↔digest consistency.
    let digest = sig_digest(&SliceSigFields {
        model_id: value["modelId"].as_str().unwrap().to_string(),
        template_hash: value["templateHash"].as_str().unwrap().to_string(),
        env_hash: value["envHash"].as_str().unwrap().to_string(),
        input_commitment: value["inputCommitment"].as_str().unwrap().to_string(),
        checkpoint_manifest_sha256: value["checkpointManifestSha256"]
            .as_str()
            .unwrap()
            .to_string(),
        session_id: u64::from_str_radix(
            value["sessionId"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
            16,
        )
        .unwrap(),
        host: value["host"].as_str().unwrap().to_string(),
        timestamp: value["timestamp"].as_u64().unwrap(),
        slice_index: value["sliceIndex"].as_u64().unwrap(),
        tokens_delta: value["tokensDelta"].as_u64().unwrap(),
    })
    .unwrap();
    let sig_hex = value["signature"].as_str().unwrap();
    let signature: Signature = sig_hex.parse().expect("65-byte signature parses");
    let wallet: LocalWallet = KEY_HEX.parse().unwrap();
    let recovered = signature
        .recover(ethers::core::types::RecoveryMessage::Data(digest.to_vec()))
        .expect("recovers");
    assert_eq!(
        recovered,
        wallet.address(),
        "EIP-191 recovery to the node key"
    );

    // proofHash = SHA256(exact uploaded bytes); the CID round-trips.
    let s5 = MockS5Backend::new();
    let (proof_cid, proof_hash) =
        upload_slice_attestation(&s5, "home/training/job_931_slice_4.json", stored.clone())
            .await
            .expect("uploads");
    assert!(!proof_cid.is_empty());
    assert_eq!(proof_hash, <[u8; 32]>::from(Sha256::digest(&stored)));
    let fetched = s5.get("home/training/job_931_slice_4.json").await.unwrap();
    assert_eq!(fetched, stored, "stored bytes fetch back exactly");
}

#[tokio::test]
async fn final_slice_carries_adapter_hash_and_moderation() {
    let job = fixture_job();
    let mut input = inputs(&job);
    input.adapter_manifest_sha256 = Some(format!("0x{}", "55".repeat(32)));
    input.moderation = Some(("cleared".to_string(), "structural-v0".to_string()));
    let (value, stored) = build_slice_attestation(&input, &key_bytes()).unwrap();
    assert_eq!(
        value["adapterManifestSha256"],
        format!("0x{}", "55".repeat(32))
    );
    assert_eq!(value["moderation"]["status"], "cleared");
    assert_eq!(value["moderation"]["policyVersion"], "structural-v0");
    // The extras change the stored bytes (and so the proofHash) — no
    // accidental exclusion from the canonical form.
    let (_, stored_plain) = build_slice_attestation(&inputs(&job), &key_bytes()).unwrap();
    assert_ne!(stored, stored_plain);
}
