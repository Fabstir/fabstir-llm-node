// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for transcoding checkpoint submission.

use fabstir_llm_node::transcoder::checkpoint::{
    billing_units_to_tokens, build_checkpoint, TranscodeCheckpoint,
};

#[test]
fn test_billing_units_to_tokens() {
    assert_eq!(billing_units_to_tokens(60.0), 60000);
    assert_eq!(billing_units_to_tokens(22.5), 22500);
    assert_eq!(billing_units_to_tokens(0.001), 1);
}

#[test]
fn test_billing_units_to_tokens_ceiling() {
    assert_eq!(billing_units_to_tokens(22.501), 22501);
    assert_eq!(billing_units_to_tokens(0.0001), 1);
}

#[test]
fn test_build_checkpoint() {
    let proof_hash = [0xABu8; 32];
    let cp = build_checkpoint(10, 5.5, proof_hash, "bafytest123");
    assert_eq!(cp.gop_index, 10);
    assert_eq!(cp.billing_tokens, 5500);
    assert_eq!(cp.proof_hash, proof_hash);
    assert_eq!(cp.proof_cid, "bafytest123");
}

#[test]
fn test_checkpoint_proof_submitted_message_format() {
    let cp = build_checkpoint(5, 12.0, [1u8; 32], "bafycid");
    let msg = serde_json::json!({
        "type": "transcode_proof_submitted",
        "gopIndex": cp.gop_index,
        "billingTokens": cp.billing_tokens,
        "proofHash": hex::encode(cp.proof_hash),
        "proofCid": cp.proof_cid
    });
    assert_eq!(msg["type"], "transcode_proof_submitted");
    assert_eq!(msg["gopIndex"], 5);
    assert_eq!(msg["billingTokens"], 12000);
    assert!(msg["proofHash"].as_str().unwrap().len() == 64);
    assert_eq!(msg["proofCid"], "bafycid");
}
