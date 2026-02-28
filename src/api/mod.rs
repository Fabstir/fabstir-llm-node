// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
pub mod describe_image;
pub mod embed;
pub mod errors;
pub mod generate_image;
pub mod handlers;
pub mod http_server;
pub mod ocr;
pub mod pool;
pub mod response_formatter;
pub mod search;
pub mod server;
pub mod streaming;
pub mod token_tracker;
pub mod websocket;

pub use describe_image::{describe_image_handler, DescribeImageRequest, DescribeImageResponse};
pub use embed::{embed_handler, EmbedRequest, EmbedResponse, EmbeddingResult};
pub use errors::{ApiError, ErrorResponse};
pub use generate_image::{generate_image_handler, GenerateImageRequest, GenerateImageResponse};
pub use handlers::{
    ChainInfo, ChainStatistics, ChainStatsResponse, ChainsResponse, HealthResponse,
    InferenceRequest, InferenceResponse, ModelInfo, ModelsResponse, SessionInfo,
    SessionInfoResponse, SessionStatus, TotalStatistics, UsageInfo,
};
pub use ocr::{ocr_handler, OcrRequest, OcrResponse};
pub use pool::{ConnectionPool, ConnectionStats, PoolConfig};
pub use search::{search_handler, SearchApiRequest, SearchApiResponse};
pub use server::{ApiConfig, ApiServer};
pub use streaming::StreamingResponse;
