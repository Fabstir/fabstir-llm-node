// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! LTX VRAM admission control — direct reuse of the transcoder's capacity
//! primitives.
//!
//! `CachedSidecarStatus` is a generic 2s-TTL cache over an `{active, queued, max}`
//! `SidecarStatus` with no transcoder-specific fields, so it is re-exported
//! verbatim rather than forked. This mirrors `transcoder::capacity` exactly: that
//! module is *only* `CachedSidecarStatus`; the `has_sidecar_capacity()` method
//! lives on `ApiServer`, and the LTX equivalent is wired there in Phase 8 against
//! `MAX_CONCURRENT_GENERATIONS`.
//!
//! NOTE: `CachedSidecarStatus::{get_or_fetch, has_capacity}` take a
//! `&TranscoderClient` (its only transcoder-coupled seam). The Phase-8 server
//! wiring binds the cache to the ComfyUI sidecar's status fetch.

pub use crate::transcoder::capacity::CachedSidecarStatus;
pub use crate::transcoder::types::SidecarStatus;
