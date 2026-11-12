// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared AI-provenance definitions for the Fabstir node and `provenance-signer`
//! (`docs/development/IMPLEMENTATION-AI-PROVENANCE.md`, milestone AP1).
//!
//! - [`class`]: the per-template class table (D7).
//! - [`defs`]: the deterministic C2PA definitions and the signer's D11(A) gate (D6, D11).
//! - [`fingerprint`]: the MPEG-7 signature set format and its digest (D3).
//! - [`frames_index`], [`exr_record`]: EXR jobs (D9, D9a).
//! - [`flow`]: the node-side Builder flow (placeholder, embed, hash, patch).
//! - [`contract`]: the node <-> signer wire contract and request authentication.
//! - [`matcher`]: the fingerprint matching rule (AP0.1b), used by the repository's index API.

pub mod class;
pub mod contract;
pub mod defs;
pub mod exr_header;
pub mod exr_record;
pub mod fingerprint;
pub mod leak_guard;
pub mod flow;
pub mod frames_index;
pub mod matcher;

/// Re-exported so the node uses exactly the pinned c2pa without its own dependency line.
pub use c2pa;
pub use class::{Class, Model, OutputRole, TemplateClass};
pub use defs::Params;

/// This crate's version: node and signer must report the same one (`/v1/limits`).
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
