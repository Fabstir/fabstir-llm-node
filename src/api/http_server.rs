// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//
// AppState for API handlers
//
// Note: The main HTTP server and router are in server.rs.
// This module only provides AppState which is shared by handler modules.

use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use super::{ApiServer, ChainStatistics, SessionInfo};
use crate::blockchain::ChainRegistry;

/// Shared application state for HTTP handlers
#[derive(Clone)]
pub struct AppState {
    pub api_server: Arc<ApiServer>,
    pub chain_registry: Arc<ChainRegistry>,
    pub sessions: Arc<RwLock<HashMap<u64, SessionInfo>>>,
    pub chain_stats: Arc<RwLock<HashMap<u64, ChainStatistics>>>,
    pub embedding_model_manager: Arc<RwLock<Option<Arc<crate::embeddings::EmbeddingModelManager>>>>,
    pub vision_model_manager: Arc<RwLock<Option<Arc<crate::vision::VisionModelManager>>>>,
    pub search_service: Arc<RwLock<Option<Arc<crate::search::SearchService>>>>,
    pub diffusion_client: Arc<RwLock<Option<Arc<crate::diffusion::DiffusionClient>>>>,
}

impl AppState {
    /// Create a minimal AppState for testing
    pub fn new_for_test() -> Self {
        AppState {
            api_server: Arc::new(ApiServer::new_for_test()),
            chain_registry: Arc::new(ChainRegistry::new()),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            chain_stats: Arc::new(RwLock::new(HashMap::new())),
            embedding_model_manager: Arc::new(RwLock::new(None)),
            vision_model_manager: Arc::new(RwLock::new(None)),
            search_service: Arc::new(RwLock::new(None)),
            diffusion_client: Arc::new(RwLock::new(None)),
        }
    }
}
