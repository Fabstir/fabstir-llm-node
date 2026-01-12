// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// src/storage/enhanced_s5_client.rs
//!
//! Enhanced S5.js P2P Bridge Client
//!
//! This module provides an HTTP client for connecting to the Enhanced S5.js bridge service.
//!
//! ## Architecture
//!
//! ```text
//! Rust Node (this) → HTTP API → Enhanced S5.js Bridge → P2P Network (WebSocket)
//!                                       ↓
//!                                S5 Portal Gateway (s5.vup.cx)
//!                                       ↓
//!                           Decentralized Storage Network
//! ```
//!
//! ## Bridge Service
//!
//! The bridge service (`services/s5-bridge/`) runs the Enhanced S5.js SDK (`@julesl23/s5js@beta`)
//! and exposes a simple HTTP REST API on `localhost:5522` (configurable via ENHANCED_S5_URL).
//!
//! **The bridge MUST be running before starting the Rust node.**
//!
//! ### Starting the Bridge
//!
//! ```bash
//! # Option 1: Direct start (requires Node.js 20+)
//! cd services/s5-bridge
//! npm install
//! npm start
//!
//! # Option 2: Docker
//! cd services/s5-bridge
//! docker-compose up -d
//!
//! # Option 3: Orchestrated startup (recommended)
//! ./scripts/start-with-s5-bridge.sh
//! ```
//!
//! ## P2P Network
//!
//! The bridge connects to the decentralized S5 network via:
//! - **WebSocket P2P Peers**: Direct peer-to-peer connections (e.g., `wss://s5.ninja/s5/p2p`)
//! - **S5 Portal**: Identity registry and gateway (e.g., `https://s5.vup.cx`)
//! - **Seed Phrase**: User identity recovery (12-word mnemonic)
//!
//! There are **NO centralized servers** - all storage is P2P and decentralized.
//!
//! ## Configuration
//!
//! Set the bridge URL via environment variable:
//! ```bash
//! export ENHANCED_S5_URL=http://localhost:5522
//! ```
//!
//! ## Health Checks
//!
//! Before starting the node, verify the bridge is healthy:
//! ```bash
//! curl http://localhost:5522/health
//! # Expected: {"status":"healthy","connected":true,...}
//! ```
//!
//! ## See Also
//!
//! - Enhanced S5.js SDK: https://github.com/parajbs/s5-network
//! - Bridge service: `services/s5-bridge/README.md`
//! - Deployment guide: `docs/ENHANCED_S5_DEPLOYMENT.md`
//!
//! Phase 4.1.1: Enhanced S5.js with Internal Mock - HTTP Client Implementation
//! Phase 6.1: Enhanced S5.js P2P Bridge Service Integration

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tracing::{error, info, warn};

/// Check if a string is a valid S5 CID in multibase format
/// S5 CIDs are raw 32-byte blake3 hashes encoded with multibase:
/// - base32 with 'b' prefix: 53 characters total (b + 52 chars)
///
/// NOTE: S5 does NOT use IPFS CID format. IPFS CIDs include version/codec/multihash
/// Validate S5 BlobIdentifier CID format
///
/// BlobIdentifier format: 58-70 chars (varies by file size encoding)
/// Structure: 'b' + base32(prefix(2) + multihash(1) + hash(32) + size(1-8))
/// - prefix: [0x5b, 0x82] (2 bytes)
/// - multihash: 0x1e (BLAKE3, 1 byte)
/// - hash: 32 bytes (BLAKE3 hash)
/// - size: 1-8 bytes (little-endian file size, trailing zeros trimmed)
///
/// Total: 36-43 bytes = 58-70 base32 characters with 'b' multibase prefix
///
/// NOTE: The old 53-char raw hash format is DEPRECATED - S5 portals reject it.
fn is_valid_s5_cid(s: &str) -> bool {
    // Must start with 'b' (base32 multibase prefix)
    if !s.starts_with('b') {
        return false;
    }

    // BlobIdentifier: 58-70 chars (varies by file size encoding)
    // 36 bytes minimum (1-byte size) = 58 chars
    // 43 bytes maximum (8-byte size) = 70 chars
    let len = s.len();
    if len < 58 || len > 70 {
        return false;
    }

    // Valid base32 lowercase chars: a-z, 2-7
    s[1..].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
}

/// Convert a raw hash (hex string) to base32 format for internal reference
///
/// DEPRECATED: This produces a 53-char raw hash format, which is NOT a valid
/// BlobIdentifier CID. S5 portals reject this format. For valid CIDs, use the
/// BlobIdentifier format returned by the S5 bridge (58-70 chars with file size).
///
/// This function is kept for internal hash reference purposes only, NOT for
/// portal downloads. The output should NOT be passed to `is_valid_s5_cid()`.
///
/// Result: 53 chars = 'b' + 52 base32 characters (raw 32-byte hash)
#[allow(dead_code)]
fn format_hash_as_cid(hash: &str) -> String {
    // Check if already a BlobIdentifier CID (58-70 chars)
    if is_valid_s5_cid(hash) {
        return hash.to_string();
    }

    // Decode hex hash to bytes
    let hash_bytes = match hex::decode(hash) {
        Ok(bytes) => bytes,
        Err(_) => {
            // Not valid hex, compute blake3 hash of the string (S5 uses blake3)
            blake3::hash(hash.as_bytes()).as_bytes().to_vec()
        }
    };

    // Ensure exactly 32 bytes
    let mut normalized_hash = [0u8; 32];
    let copy_len = std::cmp::min(hash_bytes.len(), 32);
    normalized_hash[..copy_len].copy_from_slice(&hash_bytes[..copy_len]);

    // Base32 encode raw 32-byte hash with 'b' multibase prefix
    // Result: 'b' + 52 lowercase base32 chars = 53 total chars
    // NOTE: This is NOT a valid BlobIdentifier CID (portals reject it)
    let base32_encoded = data_encoding::BASE32_NOPAD.encode(&normalized_hash).to_lowercase();
    format!("b{}", base32_encoded)
}

#[derive(Debug, Clone)]
pub struct S5Config {
    pub api_url: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S5File {
    pub name: String,
    pub size: u64,
    #[serde(rename = "type")]
    pub file_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(rename = "mockStorage")]
    pub mock_storage: bool,
    pub server: String,
    pub version: String,
}

/// Health response from Enhanced S5.js bridge service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeHealthResponse {
    pub status: String,
    pub service: String,
    pub timestamp: String,
    pub initialized: bool,
    pub connected: bool,
    #[serde(rename = "peerCount")]
    pub peer_count: u32,
    pub portal: String,
}

#[derive(Clone, Debug)]
pub struct EnhancedS5Client {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    // Mock storage for testing
    mock_storage: std::sync::Arc<Mutex<HashMap<String, (Vec<u8>, Option<JsonValue>)>>>,
}

impl EnhancedS5Client {
    pub fn new(config: S5Config) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self {
            client,
            base_url: config.api_url,
            api_key: config.api_key,
            mock_storage: std::sync::Arc::new(Mutex::new(HashMap::new())),
        })
    }

    // Legacy constructor for backward compatibility
    pub fn new_legacy(base_url: String) -> Result<Self> {
        Self::new(S5Config {
            api_url: base_url,
            api_key: None,
            timeout_secs: 30,
        })
    }

    pub async fn health_check(&self) -> Result<HealthResponse> {
        let url = format!("{}/health", self.base_url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Health check failed with status: {}",
                response.status()
            ));
        }

        let health: HealthResponse = response.json().await?;
        Ok(health)
    }

    /// Check Enhanced S5.js bridge service health
    pub async fn bridge_health_check(&self) -> Result<BridgeHealthResponse> {
        let url = format!("{}/health", self.base_url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Bridge health check failed with status: {}",
                response.status()
            ));
        }

        let health: BridgeHealthResponse = response.json().await?;
        Ok(health)
    }

    /// Upload a file to S5 and return the CID
    /// The S5 bridge returns the CID in the response body as JSON: {"cid": "bafybei..."}
    pub async fn put_file(&self, path: &str, content: Vec<u8>) -> Result<String> {
        let content_size = content.len();
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };

        info!(
            "📤 [S5-HTTP] PUT request: url='{}', path='{}', size={} bytes",
            url, path, content_size
        );

        let start_time = std::time::Instant::now();

        let response = self.client
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .body(content)
            .send()
            .await?;

        let status = response.status();
        let duration_ms = start_time.elapsed().as_millis();

        info!(
            "📤 [S5-HTTP] Response received: status={}, duration={}ms",
            status, duration_ms
        );

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(
                "📤 [S5-HTTP] ❌ PUT FAILED: status={}, path='{}', error='{}'",
                status, path, error_text
            );
            return Err(anyhow!("Failed to PUT file: {} - {}", status, error_text));
        }

        // Parse the response to get the CID
        // S5 bridge returns JSON with CID from Advanced API: {"success": true, "path": "...", "cid": "baaa..."}
        let response_text = response.text().await.unwrap_or_default();
        info!("📤 [S5-HTTP] Raw response: '{}'", response_text);

        // Try to parse as JSON to extract CID and debug info
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_text) {
            // Log debug info if present
            if let Some(debug_info) = json.get("debug") {
                info!(
                    "📤 [S5-HTTP] Bridge debug: requestId={}, uploadDurationMs={}, portalAccount={}",
                    debug_info.get("requestId").and_then(|v| v.as_str()).unwrap_or("?"),
                    debug_info.get("uploadDurationMs").and_then(|v| v.as_u64()).unwrap_or(0),
                    debug_info.get("portalAccount").and_then(|v| v.as_str()).unwrap_or("?")
                );
            }

            // Check for networkUploaded flag
            let network_uploaded = json.get("networkUploaded").and_then(|v| v.as_bool()).unwrap_or(false);

            // Check for CID field from S5 Advanced API (formatCID output)
            if let Some(cid) = json.get("cid").and_then(|v| v.as_str()) {
                info!(
                    "📤 [S5-HTTP] ✅ PUT SUCCESS: path='{}', cid='{}', cid_len={}, networkUploaded={}",
                    path, cid, cid.len(), network_uploaded
                );
                return Ok(cid.to_string());
            }
        }

        // No CID in response - fail explicitly
        error!(
            "📤 [S5-HTTP] ❌ No CID in response: path='{}', response='{}'",
            path, response_text
        );
        Err(anyhow!("S5 bridge did not return CID in response: '{}'. Bridge must use S5 Advanced API (FS5Advanced.pathToCID + formatCID).", response_text))
    }

    pub async fn get_file(&self, path: &str) -> Result<Vec<u8>> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };

        info!("GET file from: {}", url);

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Err(anyhow!("File not found: {}", path));
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Failed to GET file: {} - {}", status, error_text));
        }

        let content = response.bytes().await?;
        Ok(content.to_vec())
    }

    pub async fn list_directory(&self, path: &str) -> Result<Vec<S5File>> {
        // Ensure path ends with / for directory listing
        let formatted_path = if path.starts_with("/s5/fs") {
            if !path.ends_with('/') {
                format!("{}/", path)
            } else {
                path.to_string()
            }
        } else {
            let clean_path = path.trim_start_matches('/').trim_end_matches('/');
            format!("/s5/fs/{}/", clean_path)
        };

        let url = format!("{}{}", self.base_url, formatted_path);

        info!("LIST directory: {}", url);

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            // Directory doesn't exist, return empty list
            return Ok(Vec::new());
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to list directory: {} - {}",
                status,
                error_text
            ));
        }

        let files: Vec<S5File> = response.json().await?;
        Ok(files)
    }

    pub async fn delete_file(&self, path: &str) -> Result<()> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };

        info!("DELETE file: {}", url);

        let response = self.client.delete(&url).send().await?;

        // Delete should be idempotent - 404 is okay
        if response.status() == 404 {
            warn!("File not found for deletion (idempotent): {}", path);
            return Ok(());
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to DELETE file: {} - {}",
                status,
                error_text
            ));
        }

        Ok(())
    }

    pub async fn exists(&self, path: &str) -> Result<bool> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };

        let response = self.client.head(&url).send().await?;

        Ok(response.status().is_success())
    }

    // New methods for E2E workflow tests
    pub async fn put(
        &self,
        path: &str,
        data: Vec<u8>,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        // Upload to S5 bridge via HTTP and get the real CID
        let cid = self.put_file(path, data.clone()).await?;

        // Also store in mock storage for compatibility (for local testing)
        let mut storage = self.mock_storage.lock().unwrap();
        storage.insert(path.to_string(), (data, metadata));

        info!("Uploaded data to S5 at path: {} with CID: {}", path, cid);
        Ok(cid)
    }

    pub async fn get(&self, path: &str) -> Result<(Vec<u8>, Option<JsonValue>)> {
        // Try to fetch from S5 bridge via HTTP
        match self.get_file(path).await {
            Ok(data) => Ok((data, None)),
            Err(_) => {
                // Fall back to mock storage for compatibility
                let storage = self.mock_storage.lock().unwrap();
                if let Some(entry) = storage.get(path) {
                    Ok(entry.clone())
                } else {
                    Err(anyhow!("File not found at path: {}", path))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let config = S5Config {
            api_url: "http://localhost:5524".to_string(),
            api_key: None,
            timeout_secs: 30,
        };
        let client = EnhancedS5Client::new(config);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_legacy_client_creation() {
        let client = EnhancedS5Client::new_legacy("http://localhost:5524".to_string());
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_path_formatting() {
        let client = EnhancedS5Client::new_legacy("http://localhost:5524".to_string()).unwrap();

        // Test various path formats are handled correctly
        let test_paths = vec![
            "/s5/fs/test/file.txt",
            "test/file.txt",
            "/test/file.txt",
            "s5/fs/test/file.txt",
        ];

        for path in test_paths {
            // Just ensure no panic occurs
            let _ = client.exists(path).await;
        }
    }

    #[test]
    fn test_format_hash_as_cid() {
        // Test with typical S5 bridge hash (32 hex chars = 16 bytes)
        // NOTE: This is DEPRECATED - produces raw hash, NOT BlobIdentifier
        let hash = "9779e1b4109298dbb9d948b9348e99b9";
        let cid = format_hash_as_cid(hash);

        println!("Input hash: {}", hash);
        println!("Output CID: {}", cid);
        println!("CID length: {}", cid.len());

        // Must start with 'b' (base32 multibase prefix)
        assert!(
            cid.starts_with('b'),
            "CID must start with 'b' prefix, got: {}",
            cid
        );

        // Must be exactly 53 characters (raw hash format, NOT BlobIdentifier)
        assert_eq!(
            cid.len(),
            53,
            "Raw hash must be 53 chars (b + 52 base32), got {}: {}",
            cid.len(),
            cid
        );

        // NOTE: 53-char is NOT a valid BlobIdentifier CID (is_valid_s5_cid returns false)
        // This is expected - format_hash_as_cid produces deprecated raw hash format

        // Must NOT be raw hex
        assert!(
            !cid.chars().all(|c| c.is_ascii_hexdigit()),
            "CID must not be raw hex: {}",
            cid
        );

        // Must NOT be IPFS format
        assert!(
            !cid.starts_with("bafkrei"),
            "CID must NOT be IPFS format (bafkrei), got: {}",
            cid
        );
        assert!(
            !cid.starts_with("bafybei"),
            "CID must NOT be IPFS format (bafybei), got: {}",
            cid
        );
    }

    #[test]
    fn test_format_hash_as_cid_full_hash() {
        // Test with full 64 char hex hash (32 bytes)
        // NOTE: This is DEPRECATED - produces raw hash, NOT BlobIdentifier
        let hash = "9779e1b4109298dbb9d948b9348e99b9f5b821ec99e02c80a1b2c3d4e5f6a7b8";
        let cid = format_hash_as_cid(hash);

        println!("Input hash: {}", hash);
        println!("Output CID: {}", cid);
        println!("CID length: {}", cid.len());

        assert!(cid.starts_with('b'), "CID must start with 'b' prefix");
        assert_eq!(cid.len(), 53, "Raw hash must be 53 chars");

        // NOTE: 53-char is NOT a valid BlobIdentifier CID (is_valid_s5_cid returns false)
        // This is expected - format_hash_as_cid produces deprecated raw hash format

        // Must NOT be IPFS format
        assert!(
            !cid.starts_with("bafkrei"),
            "CID must NOT be IPFS format"
        );
        assert!(
            !cid.starts_with("bafybei"),
            "CID must NOT be IPFS format"
        );
    }

    #[test]
    fn test_valid_blob_identifier_passthrough() {
        // Already valid BlobIdentifier CID (58-70 chars) should pass through unchanged
        let valid_cid = "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxy";
        assert_eq!(valid_cid.len(), 58, "Test CID should be 58 chars");
        assert!(is_valid_s5_cid(valid_cid), "Should be valid BlobIdentifier");
        let result = format_hash_as_cid(valid_cid);
        assert_eq!(
            result, valid_cid,
            "Valid BlobIdentifier CID should pass through unchanged"
        );
    }

    #[test]
    fn test_is_valid_s5_cid_blob_identifier_format() {
        // BlobIdentifier format: 58-70 chars (varies by file size encoding)
        // Structure: 'b' + base32(prefix(2) + multihash(1) + hash(32) + size(1-8))
        // Minimum: 36 bytes = 58 chars; Maximum: ~43 bytes = 70 chars

        // Valid BlobIdentifier CID - 58 chars (minimum size, 1-byte file size)
        let valid_58_char = "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxy";
        assert_eq!(valid_58_char.len(), 58);
        assert!(
            is_valid_s5_cid(valid_58_char),
            "58-char BlobIdentifier CID should be valid"
        );

        // Valid BlobIdentifier CID - 59 chars (typical small files)
        let valid_59_char = "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxyz";
        assert_eq!(valid_59_char.len(), 59);
        assert!(
            is_valid_s5_cid(valid_59_char),
            "59-char BlobIdentifier CID should be valid"
        );

        // Valid BlobIdentifier CID - 62 chars (medium files)
        let valid_62_char = "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxyz234";
        assert_eq!(valid_62_char.len(), 62);
        assert!(
            is_valid_s5_cid(valid_62_char),
            "62-char BlobIdentifier CID should be valid"
        );

        // Valid BlobIdentifier CID - 70 chars (maximum size, 8-byte file size)
        let valid_70_char =
            "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxyz234567abcde";
        assert_eq!(valid_70_char.len(), 70);
        assert!(
            is_valid_s5_cid(valid_70_char),
            "70-char BlobIdentifier CID should be valid"
        );
    }

    #[test]
    fn test_reject_old_53_char_raw_hash() {
        // 53-char raw hash format is DEPRECATED - portals reject it
        // Only accept BlobIdentifier format (58-70 chars)
        let old_53_char_cid = "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst";
        assert_eq!(old_53_char_cid.len(), 53);
        assert!(
            !is_valid_s5_cid(old_53_char_cid),
            "53-char raw hash CID should be REJECTED (portals don't accept it)"
        );
    }

    #[test]
    fn test_is_valid_s5_cid() {
        // Invalid - IPFS format CIDs (wrong structure, contain 8/9/0/1)
        // Note: These are 59 chars which is in valid range, but contain invalid base32 chars
        assert!(
            !is_valid_s5_cid("bafybeig123abc456def789ghi012jkl345mno678pqr901stu234vwx5"),
            "IPFS-style CID (bafybei...) should be invalid - contains 8,9,0,1"
        );
        assert!(
            !is_valid_s5_cid("bafkreig123abc456def789ghi012jkl345mno678pqr901stu234vwx5"),
            "IPFS-style CID (bafkrei...) should be invalid - contains 8,9,0,1"
        );

        // Invalid - raw hex hashes (wrong chars and wrong length)
        assert!(
            !is_valid_s5_cid("9779e1b4109298dbb9d948b9348e99b9"),
            "Hex hash should be invalid (wrong prefix)"
        );
        assert!(
            !is_valid_s5_cid("f5b821ec99e02c80"),
            "Short hex should be invalid"
        );
        assert!(
            !is_valid_s5_cid("1234567890abcdef"),
            "Hex digits only should be invalid"
        );

        // Invalid - too short (under 58)
        assert!(!is_valid_s5_cid("babcdef"), "7 chars should be invalid");
        assert!(
            !is_valid_s5_cid("babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst"),
            "53-char raw hash should be invalid (deprecated)"
        );
        assert!(
            !is_valid_s5_cid("babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwx"),
            "57 chars should be invalid (too short)"
        );

        // Invalid - too long (over 70)
        assert!(
            !is_valid_s5_cid(
                "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxyz234567abcdef"
            ),
            "71 chars should be invalid (too long)"
        );

        // Invalid - wrong prefix (must start with 'b')
        assert!(
            !is_valid_s5_cid("zabcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuvwxy"),
            "Wrong prefix 'z' should be invalid"
        );

        // Invalid - uppercase letters (base32 must be lowercase)
        assert!(
            !is_valid_s5_cid("bABCDEabcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst"),
            "Uppercase letters should be invalid"
        );

        // Invalid - invalid base32 characters (8, 9, 0, 1)
        assert!(
            !is_valid_s5_cid("b89010abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst"),
            "Invalid base32 chars (8,9,0,1) should be invalid"
        );
    }
}
