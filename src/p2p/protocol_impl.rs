use anyhow::Result;
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::{
    request_response::{self, Codec},
    StreamProtocol,
};
use futures::io::{AsyncRead, AsyncWrite};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

use super::{InferenceRequest, InferenceResponse, JobClaim, JobResult};

// Type alias for response channel
pub type ResponseChannel = request_response::ResponseChannel<FabstirResponse>;

// Protocol constants
pub const INFERENCE_PROTOCOL: &str = "/fabstir/inference/1.0.0";
pub const JOB_PROTOCOL: &str = "/fabstir/job/1.0.0";

// Codec for encoding/decoding messages
#[derive(Debug, Clone, Default)]
pub struct FabstirCodec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FabstirRequest {
    Inference(InferenceRequest),
    JobClaim(JobClaim),
    JobResult(JobResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FabstirResponse {
    Inference(InferenceResponse),
    JobClaimAck { job_id: u64, accepted: bool },
    JobResultAck { job_id: u64, accepted: bool },
}

#[async_trait]
impl Codec for FabstirCodec {
    type Protocol = StreamProtocol;
    type Request = FabstirRequest;
    type Response = FabstirResponse;

    async fn read_request<T>(
        &mut self,
        _protocol: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<FabstirRequest>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        
        // Limit message size to 1MB
        if len > 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Message too large"));
        }
        
        // Read the message
        buf.resize(len, 0);
        io.read_exact(&mut buf).await?;
        
        let request = serde_json::from_slice(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(request)
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<FabstirResponse>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        
        // Limit message size to 1MB
        if len > 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Message too large"));
        }
        
        // Read the message
        buf.resize(len, 0);
        io.read_exact(&mut buf).await?;
        
        let response = serde_json::from_slice(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(response)
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &StreamProtocol,
        io: &mut T,
        request: FabstirRequest,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&request)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        // Write length prefix
        let len = data.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        
        // Write the message
        io.write_all(&data).await?;
        io.flush().await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &StreamProtocol,
        io: &mut T,
        response: FabstirResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&response)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        // Write length prefix
        let len = data.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        
        // Write the message
        io.write_all(&data).await?;
        io.flush().await?;
        Ok(())
    }
}

// Request tracking for timeouts
pub struct RequestTracker {
    pending_requests: HashMap<String, (Instant, oneshot::Sender<Result<InferenceResponse>>)>,
    timeout_duration: Duration,
}

impl RequestTracker {
    pub fn new(timeout_duration: Duration) -> Self {
        Self {
            pending_requests: HashMap::new(),
            timeout_duration,
        }
    }

    pub fn track_request(
        &mut self,
        request_id: String,
    ) -> oneshot::Receiver<Result<InferenceResponse>> {
        let (tx, rx) = oneshot::channel();
        self.pending_requests
            .insert(request_id, (Instant::now(), tx));
        rx
    }

    pub fn complete_request(&mut self, request_id: &str, response: InferenceResponse) {
        if let Some((_, tx)) = self.pending_requests.remove(request_id) {
            let _ = tx.send(Ok(response));
        }
    }

    pub fn check_timeouts(&mut self) -> Vec<String> {
        let now = Instant::now();
        let mut timed_out = Vec::new();

        let mut to_remove = Vec::new();
        for (request_id, (start_time, _)) in self.pending_requests.iter() {
            if now.duration_since(*start_time) > self.timeout_duration {
                to_remove.push(request_id.clone());
            }
        }
        
        for request_id in &to_remove {
            if let Some((_, tx)) = self.pending_requests.remove(request_id) {
                let _ = tx.send(Err(anyhow::anyhow!("Request timed out")));
                timed_out.push(request_id.clone());
            }
        }

        timed_out
    }
}

// Rate limiter
pub struct RateLimiter {
    peer_requests: HashMap<libp2p::PeerId, Vec<Instant>>,
    max_requests_per_minute: usize,
}

impl RateLimiter {
    pub fn new(max_requests_per_minute: usize) -> Self {
        Self {
            peer_requests: HashMap::new(),
            max_requests_per_minute,
        }
    }

    pub fn check_rate_limit(&mut self, peer_id: &libp2p::PeerId) -> Result<()> {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        let requests = self.peer_requests.entry(*peer_id).or_insert_with(Vec::new);
        
        // Remove old requests
        requests.retain(|&time| time > one_minute_ago);
        
        if requests.len() >= self.max_requests_per_minute {
            return Err(anyhow::anyhow!(
                "Rate limit exceeded: {} requests in the last minute",
                requests.len()
            ));
        }

        requests.push(now);
        Ok(())
    }

    pub fn get_request_count(&self, peer_id: &libp2p::PeerId) -> usize {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        self.peer_requests
            .get(peer_id)
            .map(|requests| {
                requests
                    .iter()
                    .filter(|&&time| time > one_minute_ago)
                    .count()
            })
            .unwrap_or(0)
    }
}

// Streaming handler
pub struct StreamingHandler {
    active_streams: HashMap<String, mpsc::Sender<InferenceResponse>>,
}

impl StreamingHandler {
    pub fn new() -> Self {
        Self {
            active_streams: HashMap::new(),
        }
    }

    pub fn create_stream(&mut self, request_id: String) -> mpsc::Receiver<InferenceResponse> {
        let (tx, rx) = mpsc::channel(100);
        self.active_streams.insert(request_id, tx);
        rx
    }

    pub async fn send_chunk(&mut self, response: InferenceResponse) -> Result<()> {
        if let Some(tx) = self.active_streams.get_mut(&response.request_id) {
            tx.send(response.clone()).await.map_err(|_| {
                anyhow::anyhow!("Failed to send streaming response")
            })?;

            // Close stream if this is the final chunk
            if response.finish_reason == "stop" {
                self.active_streams.remove(&response.request_id);
            }
        }
        Ok(())
    }

    pub fn close_stream(&mut self, request_id: &str) {
        self.active_streams.remove(request_id);
    }
}