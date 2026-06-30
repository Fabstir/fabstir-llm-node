// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! CSAM-handling — ★ ISOLATED SUBMODULE ★ — access-restricted; no content leaves
//! this boundary. See non-negotiables #2 / #5 / #8 and IMPLEMENTATION §2 (D1).
//!
//! ISOLATION CONTRACT (enforced by module privacy, not just convention):
//! the only surface intended for callers OUTSIDE the submodule is the narrow pair
//! of entry points re-exported below (`moderate_frames` / `moderate_asset_bytes`),
//! which return only a `ModerationResult` (a verdict + a category/rule reason) —
//! never raw matched content, raw NCMEC hashes, or the quarantine store. The
//! internal modules are `pub` so the integration test crate can drive them, but
//! `mod.rs` performs no `pub use` of their content-bearing internals and
//! production callers go through the entry points.

pub mod atrest;
pub mod entry;
pub mod hashlist;
pub mod matcher;
pub mod mock_source;
pub mod ownhash;
pub mod pdq;
pub mod quarantine;
pub mod report;

// The narrow CSAM API surface.
pub use entry::{moderate_asset_bytes, moderate_frames};
