// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Host-side content-moderation test harness (delegation harness, like `tee_tests`).
//!
//! Run with: `cargo test --test moderation_tests -- --test-threads=1`
//!
//! Every sub-test file MUST be declared in `tests/moderation/mod.rs` as `mod test_X;`,
//! or it is silently never compiled or run (a false GREEN).
mod moderation;
