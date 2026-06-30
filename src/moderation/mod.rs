// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Host-side content-moderation brain (Track 1 + fail-closed gate).
//!
//! See `docs/development/IMPLEMENTATION-HOST-MODERATION.md`. This module hosts the
//! Track-1 matching engine, the host-reachable fail-closed publish gate, full
//! quarantine, and the NCMEC report path — all behind mockable adapters so the
//! pipeline is green before NCMEC ESP onboarding completes.
//!
//! CSAM-handling logic is cordoned into the isolated [`csam`] submodule (§2 / D1):
//! only a narrow API crosses that boundary — no raw matched content, hashes, or
//! quarantine handles leak out.
//!
//! Module map (files land as their phases are implemented — see IMPLEMENTATION §2):
//!   types · config · gate · verdict_store · ingest · asset · csam/*

pub mod asset;
pub mod config;
pub mod csam;
pub mod gate;
pub mod ingest;
pub mod types;
pub mod verdict_store;
