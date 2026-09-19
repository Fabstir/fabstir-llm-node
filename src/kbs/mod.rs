// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! `fabstir-kbs`: the Phase 5 key broker. Design: `docs/development/DESIGN-PHASE5-KBS.md`
//! (converged 2026-09-18). One binary (`src/bin/kbs.rs`) behind the `kbs` feature; the
//! request path reuses the node's `tee::{types, policy, policy_source, keywrap, container,
//! kbs_http}` and decides on broker-local types (design D16).

pub mod capture;
pub mod config;
pub mod cpu;
pub mod egress;
pub mod error;
pub mod eventlog;
pub mod gpu;
pub mod keyring;
pub mod memo;
pub mod nonce;
pub mod nras_claims;
pub mod policy_file;
pub mod routes;
pub mod tools;
pub mod verify;

pub use error::{KbsError, Kind};
