// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared tracker helpers for the A.1 panic-safety tests
//! (`test_ltx_panic.rs`) and the single-exit cleanup tests
//! (`test_ltx_cleanup.rs`).

use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::ltx::billing::COMPLETING_LATCH_SECS;
use std::time::Duration;

/// Exact pending count. Deliberately NOT `unwrap_or(0)`: that would make
/// "forfeited" and "tracker entry vanished" indistinguishable, so every
/// `== 0` assertion in these suites would also pass if an entry were ever
/// removed.
pub async fn pending_count(server: &ApiServer, job_id: u64) -> u32 {
    server
        .ltx_tracker()
        .get_job_info(job_id)
        .await
        .expect("tracker entry must still exist")
        .pending_count
}

/// Mark one clip's proof pending, asserting the atomic accept gate admitted it.
pub async fn mark_pending(server: &ApiServer, job_id: u64) {
    assert!(
        server
            .ltx_tracker()
            .mark_proof_pending(job_id, Duration::from_secs(COMPLETING_LATCH_SECS))
            .await,
        "accept gate admits the clip"
    );
}
