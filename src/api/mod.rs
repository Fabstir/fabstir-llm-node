// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
pub mod errors;
pub mod handlers;
pub mod http_server;
pub mod pool;
pub mod response_formatter;
pub mod server;
pub mod streaming;
pub mod token_tracker;
pub mod websocket;

pub use errors::{ApiError, ErrorResponse};
pub use handlers::{
    ChainInfo, ChainStatistics, ChainStatsResponse, ChainsResponse, HealthResponse,
    InferenceRequest, InferenceResponse, ModelInfo, ModelsResponse, SessionInfo,
    SessionInfoResponse, SessionStatus, TotalStatistics,
};
pub use pool::{ConnectionPool, ConnectionStats, PoolConfig};
pub use server::{ApiConfig, ApiServer};
pub use streaming::StreamingResponse;
