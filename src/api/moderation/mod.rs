// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Moderation HTTP API (B8): `POST /v1/moderate/asset` (+ `/v1/moderate/review`,
//! Phase 6.3). DTOs + handlers; the moderation logic lives in `crate::moderation`.

pub mod frames;
pub mod handler;
pub mod request;
pub mod response;
pub mod review;

pub use frames::{
    moderate_frames_handler, moderate_frames_inner, ModerateFramesRequest, ModerateFramesResponse,
};
pub use handler::{
    moderate_asset_handler, moderate_asset_inner, moderate_asset_inner_preserving, MAX_ASSET_BYTES,
};
pub use request::ModerateAssetRequest;
pub use response::ModerateAssetResponse;
pub use review::{
    moderate_review_handler, moderate_review_inner, resolve_role, ModerateReviewRequest,
    ModerateReviewResponse, AUTHORISED_REVIEWER_TOKEN,
};
