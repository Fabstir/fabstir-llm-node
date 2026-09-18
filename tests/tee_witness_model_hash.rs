// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3 converge round 35 — the proof witness's `model_hash` on the
//! attested path is the on-chain model id, not `sha256(MODEL_PATH)`. Own test
//! binary: the attested id is a process-wide OnceLock (first set wins), so the
//! two halves must run in this order in an otherwise untouched process.

use fabstir_llm_node::contracts::checkpoint_manager::witness_model_hash;
use fabstir_llm_node::tee::{attested_model_id, mark_attested_model_id};
use sha2::{Digest, Sha256};

#[test]
fn witness_model_hash_prefers_the_attested_model_id() {
    // Plain path: the pre-existing placeholder, a hash of the path STRING.
    assert_eq!(attested_model_id(), None);
    let model_path =
        std::env::var("MODEL_PATH").unwrap_or_else(|_| "./models/default.gguf".to_string());
    let mut placeholder = [0u8; 32];
    placeholder.copy_from_slice(&Sha256::digest(model_path.as_bytes()));
    assert_eq!(witness_model_hash(), placeholder);

    // Attested path: the id the policy and container were bound to.
    let id = [0x5Au8; 32];
    mark_attested_model_id(id);
    assert_eq!(attested_model_id(), Some(id));
    assert_eq!(
        witness_model_hash(),
        id,
        "the witness carries the real identity"
    );
    // First set wins: a second id cannot re-label a running node.
    mark_attested_model_id([0x11u8; 32]);
    assert_eq!(witness_model_hash(), id);
}
