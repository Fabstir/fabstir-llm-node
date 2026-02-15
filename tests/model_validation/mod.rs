// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Model Validation Tests (TDD - February 4, 2026)
//!
//! Test suite for model authorization enforcement.
//!
//! This implements the security fix described in IMPLEMENTATION-MODEL-VALIDATION.md:
//! - Error types and Display trait
//! - Dynamic model map from contract
//! - Contract queries with caching
//! - Startup validation with SHA256 verification
//!
//! **TDD Approach**: Tests written BEFORE implementation.

mod test_contract_queries;
mod test_dynamic_model_map;
mod test_error_types;
mod test_job_claim;
mod test_main_integration;
mod test_model_id_extraction;
mod test_startup_validation;
