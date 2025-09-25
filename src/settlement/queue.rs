pub use super::types::{SettlementRequest, SettlementStatus};
use super::types::SettlementError;
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Ordering;
use tokio::sync::RwLock;
use std::sync::Arc;

// Wrapper for priority queue ordering
#[derive(Clone)]
struct PriorityRequest {
    request: SettlementRequest,
}

impl Eq for PriorityRequest {}

impl PartialEq for PriorityRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request.session_id == other.request.session_id
    }
}

impl Ord for PriorityRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first (BinaryHeap is a max-heap)
        // So we want higher priority values to be "greater"
        self.request.priority.cmp(&other.request.priority)
            .then_with(|| other.request.retry_count.cmp(&self.request.retry_count))
    }
}

impl PartialOrd for PriorityRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct SettlementQueue {
    // Main storage by session ID
    requests: Arc<RwLock<HashMap<u64, SettlementRequest>>>,
    // Priority queue for pending requests
    pending_queue: Arc<RwLock<BinaryHeap<PriorityRequest>>>,
    // Chain-specific indexes
    by_chain: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
}

impl SettlementQueue {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            pending_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            by_chain: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add(&mut self, request: SettlementRequest) {
        let session_id = request.session_id;
        let chain_id = request.chain_id;

        // Add to main storage
        self.requests.write().await.insert(session_id, request.clone());

        // Add to chain index
        self.by_chain.write().await
            .entry(chain_id)
            .or_insert_with(Vec::new)
            .push(session_id);

        // Add to priority queue if pending
        if request.status == SettlementStatus::Pending {
            self.pending_queue.write().await.push(PriorityRequest { request });
        }
    }

    pub async fn get(&self, session_id: u64) -> Option<SettlementRequest> {
        self.requests.read().await.get(&session_id).cloned()
    }

    pub async fn get_next(&mut self) -> Option<SettlementRequest> {
        // Get highest priority pending request
        while let Some(priority_req) = self.pending_queue.write().await.pop() {
            let session_id = priority_req.request.session_id;

            // Check if still in pending status
            if let Some(request) = self.requests.read().await.get(&session_id) {
                if request.status == SettlementStatus::Pending {
                    return Some(request.clone());
                }
            }
        }
        None
    }

    pub async fn get_by_chain(&self, chain_id: u64) -> Vec<SettlementRequest> {
        let mut results = Vec::new();

        if let Some(session_ids) = self.by_chain.read().await.get(&chain_id) {
            let requests = self.requests.read().await;
            for session_id in session_ids {
                if let Some(request) = requests.get(session_id) {
                    results.push(request.clone());
                }
            }
        }

        results
    }

    pub async fn update_status(&mut self, session_id: u64, status: SettlementStatus) {
        if let Some(request) = self.requests.write().await.get_mut(&session_id) {
            request.status = status.clone();

            // Re-add to queue if changed to pending
            if status == SettlementStatus::Pending {
                self.pending_queue.write().await.push(PriorityRequest {
                    request: request.clone()
                });
            }
        }
    }

    pub async fn increment_retry(&mut self, session_id: u64) {
        if let Some(request) = self.requests.write().await.get_mut(&session_id) {
            request.retry_count += 1;
        }
    }

    pub async fn reset_for_retry(&mut self, session_id: u64) {
        if let Some(request) = self.requests.write().await.get_mut(&session_id) {
            request.status = SettlementStatus::Pending;
            // Re-add to pending queue
            self.pending_queue.write().await.push(PriorityRequest {
                request: request.clone()
            });
        }
    }

    pub async fn remove(&mut self, session_id: u64) -> Option<SettlementRequest> {
        // Remove from main storage
        let removed = self.requests.write().await.remove(&session_id);

        if let Some(ref request) = removed {
            // Remove from chain index
            if let Some(chain_sessions) = self.by_chain.write().await.get_mut(&request.chain_id) {
                chain_sessions.retain(|&id| id != session_id);
            }
        }

        removed
    }

    pub async fn size(&self) -> usize {
        self.requests.read().await.len()
    }

    pub async fn pending_count(&self) -> usize {
        self.pending_queue.read().await.len()
    }

    pub async fn clear(&mut self) {
        self.requests.write().await.clear();
        self.pending_queue.write().await.clear();
        self.by_chain.write().await.clear();
    }
}

impl Default for SettlementQueue {
    fn default() -> Self {
        Self::new()
    }
}