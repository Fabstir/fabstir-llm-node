// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use reqwest::{Client, Error as ReqwestError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::{wrappers::ReceiverStream, Stream};

pub type VectorId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VectorBackend {
    Mock,
    Real { api_url: String },
}

#[derive(Debug, Clone)]
pub struct VectorDBConfig {
    pub backend: VectorBackend,
    pub api_key: Option<String>,
    pub timeout_ms: u64,
    pub max_retries: u32,
}

impl Default for VectorDBConfig {
    fn default() -> Self {
        Self {
            backend: VectorBackend::Mock,
            api_key: None,
            timeout_ms: 5000,
            max_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    pub id: String,
    pub vector: Vec<f32>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchOptions {
    pub k: usize,
    pub search_recent: bool,
    pub search_historical: bool,
    pub hnsw_ef: Option<u32>,
    pub ivf_n_probe: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub include_metadata: bool,
    pub score_threshold: Option<f32>,
    pub filter: Option<HashMap<String, FilterValue>>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            k: 10,
            search_recent: true,
            search_historical: true,
            hnsw_ef: None,
            ivf_n_probe: None,
            timeout_ms: None,
            include_metadata: true,
            score_threshold: None,
            filter: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FilterValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Array(Vec<String>),
    Range { min: Option<f64>, max: Option<f64> },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    In,
    NotIn,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub distance: f32,
    pub score: f32,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsertResult {
    pub id: String,
    pub index: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchInsertResult {
    pub successful: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub total_vectors: i64,
    pub indices: HashMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStats {
    pub total_vectors: i64,
    pub recent_vectors: i64,
    pub historical_vectors: i64,
    pub indices_count: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateEvent {
    pub event_type: String,
    pub vector_id: String,
    pub timestamp: u64,
}

#[derive(Error, Debug)]
pub enum VectorError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] ReqwestError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Vector not found: {0}")]
    NotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Backend error: {0}")]
    Backend(String),
    #[error("Timeout")]
    Timeout,
}

// Mock backend implementation
struct MockBackend {
    vectors: Arc<RwLock<HashMap<String, VectorEntry>>>,
    stats: Arc<RwLock<VectorStats>>,
}

impl MockBackend {
    fn new() -> Self {
        Self {
            vectors: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(VectorStats {
                total_vectors: 0,
                recent_vectors: 0,
                historical_vectors: 0,
                indices_count: 1,
                total_size_bytes: 0,
            })),
        }
    }

    async fn insert_vector(&self, vector: VectorEntry) -> Result<InsertResult, VectorError> {
        let mut vectors = self.vectors.write().await;
        let mut stats = self.stats.write().await;

        let vector_size = vector.vector.len() * 4 + vector.metadata.len() * 50; // Rough estimate

        vectors.insert(vector.id.clone(), vector.clone());
        stats.total_vectors += 1;
        stats.recent_vectors += 1;
        stats.total_size_bytes += vector_size as u64;

        Ok(InsertResult {
            id: vector.id,
            index: "mock".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        })
    }

    async fn get_vector(&self, id: &str) -> Result<VectorEntry, VectorError> {
        let vectors = self.vectors.read().await;
        vectors
            .get(id)
            .cloned()
            .ok_or_else(|| VectorError::NotFound(id.to_string()))
    }

    async fn search(&self, query: Vec<f32>, k: usize) -> Result<Vec<SearchResult>, VectorError> {
        let vectors = self.vectors.read().await;
        let mut results = Vec::new();

        for (id, entry) in vectors.iter() {
            // Simple cosine similarity calculation
            let similarity = cosine_similarity(&query, &entry.vector);
            let distance = 1.0 - similarity;

            results.push(SearchResult {
                id: id.clone(),
                distance,
                score: similarity,
                metadata: entry.metadata.clone(),
            });
        }

        // Sort by distance (ascending)
        results.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
        results.truncate(k);

        Ok(results)
    }

    async fn delete_vector(&self, id: &str) -> Result<(), VectorError> {
        let mut vectors = self.vectors.write().await;
        let mut stats = self.stats.write().await;

        if let Some(vector) = vectors.remove(id) {
            let vector_size = vector.vector.len() * 4 + vector.metadata.len() * 50;
            stats.total_vectors -= 1;
            stats.recent_vectors = stats.recent_vectors.saturating_sub(1);
            stats.total_size_bytes = stats.total_size_bytes.saturating_sub(vector_size as u64);
        }

        Ok(())
    }

    async fn vector_exists(&self, id: &str) -> Result<bool, VectorError> {
        let vectors = self.vectors.read().await;
        Ok(vectors.contains_key(id))
    }

    async fn get_stats(&self) -> Result<VectorStats, VectorError> {
        let stats = self.stats.read().await;
        Ok((*stats).clone())
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

#[derive(Clone)]
pub struct VectorDBClient {
    config: VectorDBConfig,
    http_client: Client,
    mock_backend: Option<Arc<MockBackend>>,
}

impl VectorDBClient {
    pub async fn new(config: VectorDBConfig) -> Result<Self, VectorError> {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()?;

        let mock_backend = match config.backend {
            VectorBackend::Mock => Some(Arc::new(MockBackend::new())),
            _ => None,
        };

        Ok(Self {
            config,
            http_client,
            mock_backend,
        })
    }

    pub async fn health(&self) -> Result<HealthStatus, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => {
                let stats = self.mock_backend.as_ref().unwrap().get_stats().await?;
                Ok(HealthStatus {
                    status: "ok".to_string(),
                    version: "1.0.0-mock".to_string(),
                    total_vectors: stats.total_vectors,
                    indices: HashMap::from([("mock".to_string(), stats.total_vectors)]),
                })
            }
            VectorBackend::Real { api_url } => {
                let url = format!("{}/health", api_url);
                let response = self.http_client.get(&url).send().await?;
                let health: HealthStatus = response.json().await?;
                Ok(health)
            }
        }
    }

    pub async fn insert_vector(&self, vector: VectorEntry) -> Result<InsertResult, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => {
                self.mock_backend
                    .as_ref()
                    .unwrap()
                    .insert_vector(vector)
                    .await
            }
            VectorBackend::Real { api_url } => {
                let url = format!("{}/vectors", api_url);
                let mut request = self.http_client.post(&url);

                if let Some(api_key) = &self.config.api_key {
                    request = request.header("Authorization", format!("Bearer {}", api_key));
                }

                let response = request.json(&vector).send().await?;
                let result: InsertResult = response.json().await?;
                Ok(result)
            }
        }
    }

    pub async fn batch_insert(
        &self,
        vectors: Vec<VectorEntry>,
    ) -> Result<BatchInsertResult, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => {
                let mut successful = 0;
                let mut failed = 0;
                let mut errors = Vec::new();

                for vector in vectors {
                    match self
                        .mock_backend
                        .as_ref()
                        .unwrap()
                        .insert_vector(vector)
                        .await
                    {
                        Ok(_) => successful += 1,
                        Err(e) => {
                            failed += 1;
                            errors.push(e.to_string());
                        }
                    }
                }

                Ok(BatchInsertResult {
                    successful,
                    failed,
                    errors,
                })
            }
            VectorBackend::Real { api_url } => {
                let url = format!("{}/vectors/batch", api_url);
                let mut request = self.http_client.post(&url);

                if let Some(api_key) = &self.config.api_key {
                    request = request.header("Authorization", format!("Bearer {}", api_key));
                }

                let response = request.json(&vectors).send().await?;
                let result: BatchInsertResult = response.json().await?;
                Ok(result)
            }
        }
    }

    pub async fn get_vector(&self, id: &str) -> Result<VectorEntry, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => self.mock_backend.as_ref().unwrap().get_vector(id).await,
            VectorBackend::Real { api_url } => {
                let url = format!("{}/vectors/{}", api_url, id);
                let mut request = self.http_client.get(&url);

                if let Some(api_key) = &self.config.api_key {
                    request = request.header("Authorization", format!("Bearer {}", api_key));
                }

                let response = request.send().await?;
                if response.status() == 404 {
                    return Err(VectorError::NotFound(id.to_string()));
                }

                let vector: VectorEntry = response.json().await?;
                Ok(vector)
            }
        }
    }

    pub async fn delete_vector(&self, id: &str) -> Result<(), VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => self.mock_backend.as_ref().unwrap().delete_vector(id).await,
            VectorBackend::Real { api_url } => {
                let url = format!("{}/vectors/{}", api_url, id);
                let mut request = self.http_client.delete(&url);

                if let Some(api_key) = &self.config.api_key {
                    request = request.header("Authorization", format!("Bearer {}", api_key));
                }

                let _response = request.send().await?;
                Ok(())
            }
        }
    }

    pub async fn vector_exists(&self, id: &str) -> Result<bool, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => self.mock_backend.as_ref().unwrap().vector_exists(id).await,
            VectorBackend::Real { .. } => match self.get_vector(id).await {
                Ok(_) => Ok(true),
                Err(VectorError::NotFound(_)) => Ok(false),
                Err(e) => Err(e),
            },
        }
    }

    pub async fn search(
        &self,
        query_vector: Vec<f32>,
        k: usize,
    ) -> Result<Vec<SearchResult>, VectorError> {
        let options = SearchOptions {
            k,
            ..Default::default()
        };
        self.search_with_options(query_vector, options).await
    }

    pub async fn search_with_options(
        &self,
        query_vector: Vec<f32>,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => {
                let mut results = self
                    .mock_backend
                    .as_ref()
                    .unwrap()
                    .search(query_vector, options.k)
                    .await?;

                // Apply filters
                if let Some(filter) = &options.filter {
                    results.retain(|result| {
                        filter.iter().all(|(key, filter_value)| {
                            match (result.metadata.get(key), filter_value) {
                                (Some(value), FilterValue::String(filter_str)) => {
                                    value == filter_str
                                }
                                (Some(value), FilterValue::Array(filter_array)) => {
                                    // For array filters, check if any array element matches
                                    if let Ok(vec_tags) = serde_json::from_str::<Vec<String>>(value)
                                    {
                                        filter_array
                                            .iter()
                                            .any(|filter_tag| vec_tags.contains(filter_tag))
                                    } else {
                                        false
                                    }
                                }
                                (Some(value), FilterValue::Range { min, max }) => {
                                    if let Ok(num_value) = value.parse::<f64>() {
                                        let min_check =
                                            min.map_or(true, |min_val| num_value >= min_val);
                                        let max_check =
                                            max.map_or(true, |max_val| num_value <= max_val);
                                        min_check && max_check
                                    } else {
                                        false
                                    }
                                }
                                _ => false,
                            }
                        })
                    });
                }

                // Apply score threshold
                if let Some(threshold) = options.score_threshold {
                    results.retain(|result| result.score >= threshold);
                }

                Ok(results)
            }
            VectorBackend::Real { api_url } => {
                let url = format!("{}/search", api_url);
                let mut request = self.http_client.post(&url);

                if let Some(api_key) = &self.config.api_key {
                    request = request.header("Authorization", format!("Bearer {}", api_key));
                }

                #[derive(Serialize)]
                struct SearchRequest {
                    vector: Vec<f32>,
                    #[serde(flatten)]
                    options: SearchOptions,
                }

                let search_request = SearchRequest {
                    vector: query_vector,
                    options,
                };

                let response = request.json(&search_request).send().await?;
                let results: Vec<SearchResult> = response.json().await?;
                Ok(results)
            }
        }
    }

    pub async fn get_stats(&self) -> Result<VectorStats, VectorError> {
        match &self.config.backend {
            VectorBackend::Mock => self.mock_backend.as_ref().unwrap().get_stats().await,
            VectorBackend::Real { api_url } => {
                let url = format!("{}/stats", api_url);
                let mut request = self.http_client.get(&url);

                if let Some(api_key) = &self.config.api_key {
                    request = request.header("Authorization", format!("Bearer {}", api_key));
                }

                let response = request.send().await?;
                let stats: VectorStats = response.json().await?;
                Ok(stats)
            }
        }
    }

    pub async fn subscribe_updates(
        &self,
    ) -> Result<impl Stream<Item = Result<UpdateEvent, VectorError>>, VectorError> {
        let (tx, rx) = mpsc::channel(100);

        // For mock backend, simulate some updates
        match &self.config.backend {
            VectorBackend::Mock => {
                tokio::spawn(async move {
                    // Just send a test event after a delay for mock purposes
                    // In real implementation, this would connect to a WebSocket or SSE stream
                    let _ = tx
                        .send(Ok(UpdateEvent {
                            event_type: "vector_added".to_string(),
                            vector_id: "mock_vector".to_string(),
                            timestamp: chrono::Utc::now().timestamp() as u64,
                        }))
                        .await;
                });
            }
            VectorBackend::Real { .. } => {
                // Real implementation would connect to WebSocket/SSE stream
                // For now, just return empty stream
            }
        }

        Ok(ReceiverStream::new(rx))
    }
}
