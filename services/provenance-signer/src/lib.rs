// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! provenance-signer: the Platformless AI C2PA claim signer and manifest/fingerprint repository
//! (`docs/development/IMPLEMENTATION-AI-PROVENANCE.md` D5, D10, D11(A), D12; contract
//! `AP-SIGNER-CONTRACT.md`).

pub mod app;
pub mod canary;
pub mod claim_signer;
pub mod config;
pub mod keystore;
pub mod notify;
pub mod reconcile;
pub mod repo;
pub mod service;
pub mod staging_chain;
pub mod store;
#[cfg(feature = "testkit")]
pub mod testkit;
pub mod tsa;
