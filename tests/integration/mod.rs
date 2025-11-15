// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/integration/mod.rs
// Integration test modules

#[cfg(test)]
pub mod mock {
    pub mod test_cache_flow;
    pub mod test_e2e_workflow;
}

#[cfg(test)]
pub mod test_e2e_encryption;

#[cfg(test)]
pub mod test_host_management;

#[cfg(test)]
pub mod test_proof_payment_flow;

#[cfg(test)]
pub mod test_proof_dispute;

#[cfg(test)]
pub mod test_ezkl_end_to_end;

#[cfg(test)]
pub mod test_embed_e2e;

#[cfg(test)]
pub mod test_compatibility;

#[cfg(test)]
pub mod test_rag_e2e;

// Phase 3-5: S5 Vector Loading Integration Tests
#[cfg(test)]
pub mod test_e2e_vector_loading_s5;

#[cfg(test)]
pub mod test_encrypted_session_with_vectors;

#[cfg(test)]
pub mod test_s5_error_scenarios;

#[cfg(test)]
pub mod test_loading_error_messages_s5;

#[cfg(test)]
pub mod test_s5_performance;
