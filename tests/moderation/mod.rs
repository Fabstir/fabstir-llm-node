// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Moderation test module registry.
//!
//! ⚠️ FALSE-GREEN TRAP: every new `tests/moderation/test_X.rs` file MUST be
//! declared here as `mod test_X;`, or `cargo test` silently reports 0 tests and
//! exits success (voiding TDD). Keep this list in sync as test files are added.

mod test_acceptance;
mod test_asset;
mod test_atrest;
mod test_config;
mod test_e2e;
mod test_frames_state;
mod test_gate;
mod test_hashlist;
mod test_ingest;
mod test_matcher_exact;
mod test_matcher_pdq;
mod test_metrics;
mod test_moderate_asset;
mod test_moderate_frames;
mod test_moderate_review;
mod test_ownhash;
mod test_pdq;
mod test_preserve;
mod test_quarantine;
mod test_report;
mod test_transcode_gate;
mod test_types;
mod test_verdict_store;
