use crate::api::websocket::messages::WebSocketMessage;
use anyhow::{anyhow, Result};
use flate2::write::{GzEncoder, DeflateEncoder};
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::io::Write;

/// Configuration for WebSocket message compression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    pub enabled: bool,
    pub threshold_bytes: usize,
    pub compression_type: String,
    pub level: u32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_bytes: 1024,
            compression_type: "gzip".to_string(),
            level: 6,
        }
    }
}

/// Handles compression and decompression of WebSocket messages
pub struct MessageCompressor {
    config: CompressionConfig,
}

impl MessageCompressor {
    /// Create a new message compressor with config
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }
    
    /// Create compressor from HTTP headers
    pub fn from_headers(headers: &[(&str, &str)]) -> Self {
        let mut config = CompressionConfig::default();
        
        for (key, value) in headers {
            if key.to_lowercase() == "accept-encoding" {
                if value.contains("gzip") {
                    config.compression_type = "gzip".to_string();
                    break;
                } else if value.contains("deflate") {
                    config.compression_type = "deflate".to_string();
                }
            }
        }
        
        Self::new(config)
    }
    
    /// Check if compression is supported
    pub fn supports_compression(&self) -> bool {
        self.config.enabled
    }
    
    /// Get the compression type
    pub fn compression_type(&self) -> &str {
        &self.config.compression_type
    }
    
    /// Compress a WebSocket message
    pub async fn compress(&self, message: &WebSocketMessage) -> Result<Vec<u8>> {
        // Serialize message to JSON
        let json = serde_json::to_vec(message)?;
        
        // Check if compression is disabled
        if !self.config.enabled {
            return Ok(json);
        }
        
        // Check if message is below threshold
        if json.len() < self.config.threshold_bytes {
            return Ok(json);
        }
        
        // Compress based on type
        match self.config.compression_type.as_str() {
            "gzip" => self.compress_gzip(&json),
            "deflate" => self.compress_deflate(&json),
            _ => {
                // Invalid compression type, fallback to uncompressed
                Ok(json)
            }
        }
    }
    
    /// Compress using gzip
    fn compress_gzip(&self, data: &[u8]) -> Result<Vec<u8>> {
        let compression = match self.config.level {
            0 => Compression::none(),
            1..=3 => Compression::fast(),
            4..=6 => Compression::default(),
            7..=9 => Compression::best(),
            _ => Compression::default(),
        };
        
        let mut encoder = GzEncoder::new(Vec::new(), compression);
        encoder.write_all(data)?;
        encoder.finish().map_err(|e| anyhow!("Gzip compression failed: {}", e))
    }
    
    /// Compress using deflate
    fn compress_deflate(&self, data: &[u8]) -> Result<Vec<u8>> {
        let compression = match self.config.level {
            0 => Compression::none(),
            1..=3 => Compression::fast(),
            4..=6 => Compression::default(),
            7..=9 => Compression::best(),
            _ => Compression::default(),
        };
        
        let mut encoder = DeflateEncoder::new(Vec::new(), compression);
        encoder.write_all(data)?;
        encoder.finish().map_err(|e| anyhow!("Deflate compression failed: {}", e))
    }
}

/// Statistics about compression performance
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    pub total_messages: usize,
    pub compressed_messages: usize,
    pub total_bytes_original: usize,
    pub total_bytes_compressed: usize,
    pub average_compression_ratio: f64,
}

impl CompressionStats {
    /// Calculate compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.total_bytes_original == 0 {
            return 0.0;
        }
        1.0 - (self.total_bytes_compressed as f64 / self.total_bytes_original as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_basic_compression() {
        let config = CompressionConfig {
            enabled: true,
            threshold_bytes: 10,
            compression_type: "gzip".to_string(),
            level: 6,
        };
        
        let compressor = MessageCompressor::new(config);
        let message = WebSocketMessage::Prompt {
            session_id: "test".to_string(),
            content: "Hello World".to_string(),
            message_index: 1,
        };
        
        let compressed = compressor.compress(&message).await.unwrap();
        assert!(compressed.len() > 0);
    }
}