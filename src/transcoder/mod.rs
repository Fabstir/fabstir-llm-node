// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Transcoder sidecar integration — client, billing, and rate limiting.

pub mod billing;
pub mod checkpoint;
pub mod client;
pub mod gop;
pub mod job_validation;
pub mod merkle;
pub mod proof;
pub mod quality;
pub mod rate_limiter;
pub mod types;

pub use client::TranscoderClient;
pub use types::*;
