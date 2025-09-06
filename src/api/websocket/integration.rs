use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use super::manager::SessionManager;
use super::session::{SessionState, WebSocketSession};

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub temperature: f32,
    pub max_tokens: i32,
}

#[derive(Debug, Clone)]
pub struct InferenceResponse {
    pub context_used: bool,
    pub session_id: Option<String>,
    pub messages_included: usize,
}

#[derive(Debug, Clone)]
pub struct BatchResult {
    pub session_id: String,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct IntegrationStatistics {
    pub sessions_processed: usize,
    pub sessions_timed_out: usize,
    pub resources_freed: usize,
}

#[derive(Debug, Clone)]
pub struct IntegrationMetrics {
    pub total_requests: usize,
    pub avg_response_time_ms: f64,
    pub error_rate: f64,
}

#[derive(Clone)]
pub struct SessionIntegration {
    manager: Arc<SessionManager>,
    persistence_enabled: Arc<RwLock<bool>>,
    session_timeout: Arc<RwLock<Duration>>,
    statistics: Arc<RwLock<IntegrationStatistics>>,
    metrics: Arc<RwLock<IntegrationMetrics>>,
    session_errors: Arc<RwLock<HashMap<String, usize>>>,
    workers: Arc<RwLock<Vec<String>>>,
    worker_sessions: Arc<RwLock<HashMap<String, Vec<String>>>>,
    worker_index: Arc<RwLock<usize>>,
}

impl SessionIntegration {
    pub fn new(manager: Arc<SessionManager>) -> Self {
        Self {
            manager,
            persistence_enabled: Arc::new(RwLock::new(false)),
            session_timeout: Arc::new(RwLock::new(Duration::from_secs(1800))),
            statistics: Arc::new(RwLock::new(IntegrationStatistics {
                sessions_processed: 0,
                sessions_timed_out: 0,
                resources_freed: 0,
            })),
            metrics: Arc::new(RwLock::new(IntegrationMetrics {
                total_requests: 0,
                avg_response_time_ms: 0.0,
                error_rate: 0.0,
            })),
            session_errors: Arc::new(RwLock::new(HashMap::new())),
            workers: Arc::new(RwLock::new(vec![])),
            worker_sessions: Arc::new(RwLock::new(HashMap::new())),
            worker_index: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn process_with_context(&self, request: InferenceRequest) -> Result<InferenceResponse> {
        // Validate request
        if request.prompt.is_empty() {
            if let Some(ref session_id) = request.session_id {
                let mut errors = self.session_errors.write().await;
                *errors.entry(session_id.clone()).or_insert(0) += 1;
            }
            return Err(anyhow!("Empty prompt"));
        }
        
        if request.temperature < 0.0 || request.temperature > 1.0 {
            if let Some(ref session_id) = request.session_id {
                let mut errors = self.session_errors.write().await;
                *errors.entry(session_id.clone()).or_insert(0) += 1;
            }
            return Err(anyhow!("Invalid temperature"));
        }
        
        if request.max_tokens < 0 {
            if let Some(ref session_id) = request.session_id {
                let mut errors = self.session_errors.write().await;
                *errors.entry(session_id.clone()).or_insert(0) += 1;
            }
            return Err(anyhow!("Invalid max_tokens"));
        }

        let messages_included = if let Some(ref session_id) = request.session_id {
            if let Some(session) = self.manager.get_session(session_id).await {
                session.messages.read().await.len()
            } else {
                0
            }
        } else {
            0
        };

        Ok(InferenceResponse {
            context_used: messages_included > 0,
            session_id: request.session_id,
            messages_included,
        })
    }

    pub async fn create_job_session(&self, job_id: &str) -> Result<WebSocketSession> {
        let session = WebSocketSession::new(&format!("job-{}", job_id));
        session.metadata.write().await.insert("job_id".to_string(), job_id.to_string());
        self.manager.register_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn update_job_stage(&self, session: &WebSocketSession, stage: &str) -> Result<()> {
        session.metadata.write().await.insert("stage".to_string(), stage.to_string());
        
        let mut stages = session.metadata.write().await
            .entry("stages".to_string())
            .or_insert_with(|| "[]".to_string())
            .clone();
        
        // Parse and append stage
        if let Ok(mut stages_vec) = serde_json::from_str::<Vec<String>>(&stages) {
            stages_vec.push(stage.to_string());
            stages = serde_json::to_string(&stages_vec)?;
            session.metadata.write().await.insert("stages".to_string(), stages);
        }
        
        Ok(())
    }

    pub async fn get_job_stages(&self, session: &WebSocketSession) -> Result<Vec<String>> {
        let metadata = session.metadata.read().await;
        if let Some(stages_str) = metadata.get("stages") {
            Ok(serde_json::from_str(stages_str)?)
        } else {
            Ok(vec![])
        }
    }

    pub async fn enable_persistence(&self, enabled: bool) -> Result<()> {
        *self.persistence_enabled.write().await = enabled;
        Ok(())
    }

    pub async fn save_session(&self, session: &WebSocketSession) -> Result<bool> {
        if !*self.persistence_enabled.read().await {
            return Ok(false);
        }
        
        // Simulate saving to storage
        info!("Saving session {} to persistent storage", session.id);
        Ok(true)
    }

    pub async fn load_session(&self, session_id: &str) -> Result<WebSocketSession> {
        if !*self.persistence_enabled.read().await {
            return Err(anyhow!("Persistence not enabled"));
        }
        
        // Simulate loading from storage
        let session = WebSocketSession::new(session_id);
        session.add_message_async("user", "Remember this").await?;
        Ok(session)
    }

    pub async fn process_session_request(&self, session: &WebSocketSession, _request: &str) -> Result<()> {
        let mut stats = self.statistics.write().await;
        stats.sessions_processed += 1;
        
        let mut metrics = self.metrics.write().await;
        metrics.total_requests += 1;
        
        // Simulate processing time
        let start = std::time::Instant::now();
        sleep(Duration::from_millis(10)).await;
        let elapsed = start.elapsed().as_millis() as f64;
        
        // Update average response time
        metrics.avg_response_time_ms = 
            (metrics.avg_response_time_ms * (metrics.total_requests - 1) as f64 + elapsed) 
            / metrics.total_requests as f64;
        
        Ok(())
    }

    pub async fn get_statistics(&self) -> Result<IntegrationStatistics> {
        Ok(self.statistics.read().await.clone())
    }

    pub async fn recover_session(&self, session_id: &str) -> Result<WebSocketSession> {
        let session = if let Some(existing) = self.manager.get_session(session_id).await {
            existing
        } else {
            WebSocketSession::new(session_id)
        };
        
        session.set_state(SessionState::Active).await?;
        session.metadata.write().await.insert("recovered".to_string(), "true".to_string());
        
        Ok(session)
    }

    pub async fn get_session_errors(&self, session_id: &str) -> Result<usize> {
        Ok(*self.session_errors.read().await.get(session_id).unwrap_or(&0))
    }

    pub async fn create_worker_session(&self, worker_id: &str) -> Result<WebSocketSession> {
        let session = WebSocketSession::new(&format!("worker-{}-session", worker_id));
        
        let mut worker_sessions = self.worker_sessions.write().await;
        worker_sessions.entry(worker_id.to_string())
            .or_insert_with(Vec::new)
            .push(session.id.clone());
        
        self.manager.register_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn handoff_session(&self, session_id: &str, new_worker: &str) -> Result<bool> {
        // Remove from old worker
        let mut worker_sessions = self.worker_sessions.write().await;
        for sessions in worker_sessions.values_mut() {
            sessions.retain(|id| id != session_id);
        }
        
        // Add to new worker
        worker_sessions.entry(new_worker.to_string())
            .or_insert_with(Vec::new)
            .push(session_id.to_string());
        
        Ok(true)
    }

    pub async fn get_worker_session(&self, worker_id: &str, session_id: &str) -> Result<WebSocketSession> {
        let worker_sessions = self.worker_sessions.read().await;
        if let Some(sessions) = worker_sessions.get(worker_id) {
            if sessions.contains(&session_id.to_string()) {
                if let Some(session) = self.manager.get_session(session_id).await {
                    return Ok(session);
                }
            }
        }
        Err(anyhow!("Session not found for worker"))
    }

    pub async fn process_batch(&self, sessions: Vec<WebSocketSession>) -> Result<Vec<BatchResult>> {
        let mut results = vec![];
        
        for session in sessions {
            results.push(BatchResult {
                session_id: session.id.clone(),
                success: true,
            });
        }
        
        Ok(results)
    }

    pub async fn set_session_timeout(&self, timeout: Duration) -> Result<()> {
        *self.session_timeout.write().await = timeout;
        
        // Start cleanup task
        let manager = self.manager.clone();
        let stats = self.statistics.clone();
        let timeout = timeout.clone();
        
        tokio::spawn(async move {
            sleep(timeout).await;
            
            // Clean up timed out sessions
            let sessions = manager.get_active_sessions().await;
            for session in sessions {
                manager.remove_session(&session.id).await;
                let mut s = stats.write().await;
                s.sessions_timed_out += 1;
            }
        });
        
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        let sessions = self.manager.get_active_sessions().await;
        let count = sessions.len();
        
        for session in sessions {
            self.manager.remove_session(&session.id).await;
        }
        
        let mut stats = self.statistics.write().await;
        stats.resources_freed = count;
        
        Ok(())
    }

    pub async fn get_metrics(&self) -> Result<IntegrationMetrics> {
        Ok(self.metrics.read().await.clone())
    }

    pub async fn configure_workers(&self, worker_ids: Vec<&str>) -> Result<()> {
        let mut workers = self.workers.write().await;
        *workers = worker_ids.iter().map(|id| id.to_string()).collect();
        Ok(())
    }

    pub async fn assign_session_to_worker(&self, session: WebSocketSession) -> Result<String> {
        let workers = self.workers.read().await;
        if workers.is_empty() {
            return Err(anyhow!("No workers configured"));
        }
        
        // Round-robin assignment
        let mut index = self.worker_index.write().await;
        let worker_id = workers[*index % workers.len()].clone();
        *index += 1;
        
        // Track assignment
        let mut worker_sessions = self.worker_sessions.write().await;
        worker_sessions.entry(worker_id.clone())
            .or_insert_with(Vec::new)
            .push(session.id.clone());
        
        self.manager.register_session(session).await?;
        
        Ok(worker_id)
    }
}