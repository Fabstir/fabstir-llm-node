// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Transcoder sidecar integration — client, billing, and rate limiting.

pub mod billing;
pub mod client;
pub mod rate_limiter;
pub mod types;

pub use client::TranscoderClient;
pub use types::*;
