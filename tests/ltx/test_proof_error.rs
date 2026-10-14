// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: `ProofSubmit` returns a TYPED error, classified once (inside
//! `CheckpointManager::submit_ltx_proof`, through `classify_submit_outcome`):
//! afterwards no caller reads error text.

use ethers::types::{TransactionReceipt, H256, U64};
use fabstir_llm_node::ltx::submit::{
    classify_submit_outcome, proof_receipt, ProofReceipt, ProofSubmitError,
};

fn receipt(status: u64, block: Option<u64>) -> TransactionReceipt {
    TransactionReceipt {
        status: Some(U64::from(status)),
        block_number: block.map(U64::from),
        ..Default::default()
    }
}

#[test]
fn a_too_many_estimate_failure_is_rate_limited() {
    // the queue's Failed text when the gas estimate reverts on the rate limit
    let got = classify_submit_outcome(Err(
        "(code: 3, message: execution reverted: Too many, data: None)".to_string(),
    ));
    assert_eq!(got, Err(ProofSubmitError::RateLimited));
}

#[test]
fn any_other_send_failure_is_other() {
    let got = classify_submit_outcome(Err("Nonce fetch failed: timeout".to_string()));
    assert!(matches!(got, Err(ProofSubmitError::Other(_))), "{got:?}");
}

#[test]
fn a_mined_status_zero_receipt_is_reverted_at_its_block() {
    let hash = H256::repeat_byte(7);
    let got = classify_submit_outcome(Ok((hash, Some(receipt(0, Some(1234))))));
    assert_eq!(got, Err(ProofSubmitError::Reverted { block: 1234 }));
}

#[test]
fn no_receipt_within_the_wait_is_unconfirmed_with_its_hash() {
    let hash = H256::repeat_byte(9);
    let got = classify_submit_outcome(Ok((hash, None)));
    assert_eq!(got, Err(ProofSubmitError::Unconfirmed(hash)));
}

#[test]
fn a_status_one_receipt_landed() {
    let hash = H256::repeat_byte(1);
    assert_eq!(
        classify_submit_outcome(Ok((hash, Some(receipt(1, Some(5)))))),
        Ok(hash)
    );
}

#[test]
fn a_receipt_maps_to_its_status_and_block() {
    assert_eq!(
        proof_receipt(&receipt(1, Some(10))),
        Some(ProofReceipt {
            success: true,
            block: 10
        })
    );
    assert_eq!(
        proof_receipt(&receipt(0, Some(11))),
        Some(ProofReceipt {
            success: false,
            block: 11
        })
    );
    // not mined into a block yet: no receipt to judge
    assert_eq!(proof_receipt(&receipt(1, None)), None);
}
