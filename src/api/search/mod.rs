// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Web search API endpoint
//!
//! Provides the `/v1/search` HTTP endpoint for web search.

pub mod handler;
pub mod request;
pub mod response;

pub use handler::search_handler;
pub use request::SearchApiRequest;
pub use response::SearchApiResponse;
