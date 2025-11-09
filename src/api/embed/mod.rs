// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Embedding API Module (Sub-phase 1.2 - Module Structure)
//!
//! This module provides the POST /v1/embed endpoint for generating
//! 384-dimensional embeddings using ONNX Runtime and all-MiniLM-L6-v2.

pub mod handler;
pub mod request;
pub mod response;

pub use handler::embed_handler;
pub use request::EmbedRequest;
pub use response::{EmbedResponse, EmbeddingResult};
