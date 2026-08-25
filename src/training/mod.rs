// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Training Jobs M0 (LoRA fine-tune): frozen-contract wire types, the pinned
//! slice schedule, and the fixed-field commitment/digest encoders.
//!
//! Contract: `docs/sdk-reference/DESIGN-TRAINING-M0-INTERFACE.md` (FROZEN
//! 2026-08-22; its Status line is the version authority). Build sheet:
//! `docs/development/IMPLEMENTATION-TRAINING-M0.md`.
//! Everything here is exercised against `tests/training/vectors/*.json`, the
//! cross-side truth both the SDK and this node reproduce byte-for-byte.

pub mod redact;
pub mod accept;
pub mod advert;
pub mod artifact;
pub mod attestation;
pub mod chain;
pub mod core;
pub mod schedule;
pub mod serve;
pub mod staging;
pub mod submit;
pub mod tracker;
pub mod trainer_client;
pub mod types;
