// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TEE test module registry.
//!
//! ⚠️ FALSE-GREEN TRAP: every new `tests/tee/test_X.rs` file MUST be declared
//! here as `mod test_X;`, or `cargo test` silently reports 0 tests and exits
//! success (voiding TDD). Keep this list in sync as test files are added.
mod test_container;
mod test_key_broker;
mod test_keywrap;
mod test_model_source;
mod test_orchestration;
mod test_policy;
mod test_policy_source;
mod test_types;
mod test_verify;
