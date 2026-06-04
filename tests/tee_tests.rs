// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TEE / Confidential-Inference test harness (delegation harness, like `transcoder_tests`).
//!
//! Run with: `cargo test --test tee_tests -- --test-threads=1`
//!
//! Every sub-test file MUST be declared in `tests/tee/mod.rs` as `mod test_X;`,
//! or it is silently never compiled or run (a false GREEN).
mod tee;
