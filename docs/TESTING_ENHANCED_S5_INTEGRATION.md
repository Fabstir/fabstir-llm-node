# Testing Plan: Enhanced S5.js Bridge Integration

## Overview

This document outlines the testing strategy for validating the integration between fabstir-llm-node (Rust) and the Enhanced S5.js bridge service (Node.js) for S5 vector database loading.

**Version**: v8.4.0-s5-vector-loading
**Date**: 2025-11-14
**Status**: Ready for Testing

---

## Architecture Under Test

```
Rust Node (fabstir-llm-node)
    ↓ HTTP requests (localhost:5522)
Enhanced S5.js Bridge Service (Node.js)
    ↓ P2P WebSocket connections
S5 Network (s5.vup.cx portal + P2P peers)
```

**Key Components:**
- `src/storage/enhanced_s5_client.rs` - Rust HTTP client
- `services/s5-bridge/src/server.js` - Node.js HTTP server
- `services/s5-bridge/src/s5_client.js` - S5.js SDK wrapper
- `@julesl23/s5js@beta` - Enhanced S5.js library

---

## Prerequisites

### 1. Environment Setup

```bash
# Install Node.js v20+
node --version  # Should be v20 or higher

# Install bridge dependencies
cd services/s5-bridge
npm install

# Verify @julesl23/s5js@beta is installed
npm list @julesl23/s5js
```

### 2. Configuration

Create `services/s5-bridge/.env`:
```bash
S5_SEED_PHRASE="your twelve word seed phrase here for testing"
S5_PORTAL_URL=https://s5.vup.cx
S5_INITIAL_PEERS=wss://z2DWuPbL5pweybXnEB618pMnV58ECj2VPDNfVGm3tFqBvjF@s5.ninja/s5/p2p
BRIDGE_PORT=5522
BRIDGE_HOST=127.0.0.1
LOG_LEVEL=debug
```

### 3. Test Data Preparation

Create test vector database on S5 network:
```bash
# Use fabstir-sdk or enhanced-s5-js directly to upload test data
# Test manifest structure:
{
  "name": "test-vectors",
  "owner": "0x...",
  "dimensions": 384,
  "vector_count": 100,
  "chunks": [...]
}
```

---

## Phase 1: Bridge Service Unit Tests

### Test 1.1: Bridge Service Startup

**Objective**: Verify bridge service starts and initializes S5.js correctly

**Commands**:
```bash
cd services/s5-bridge
npm start
```

**Expected Output**:
```
[INFO] Enhanced S5.js Bridge starting...
[INFO] S5 client initialized with portal: https://s5.vup.cx
[INFO] Registered with S5 portal
[INFO] Filesystem initialized
[INFO] Bridge server listening on http://127.0.0.1:5522
[INFO] P2P peers connected: 1
```

**Validation**:
- [ ] Service starts without errors
- [ ] S5 portal registration succeeds
- [ ] At least 1 P2P peer connected
- [ ] HTTP server listening on port 5522

**Failure Cases**:
- No seed phrase → Should fail with clear error
- Invalid portal URL → Should timeout/fail gracefully
- No P2P peers → Should warn but continue

---

### Test 1.2: Health Endpoint

**Objective**: Verify health check returns correct status

**Command**:
```bash
curl http://localhost:5522/health
```

**Expected Response**:
```json
{
  "status": "healthy",
  "portal": "https://s5.vup.cx",
  "peers_connected": 1,
  "identity_initialized": true,
  "uptime_seconds": 45
}
```

**Validation**:
- [ ] Status code: 200
- [ ] `status: "healthy"`
- [ ] `peers_connected` > 0
- [ ] `identity_initialized: true`

---

### Test 1.3: File Upload (PUT)

**Objective**: Test uploading a file to S5 network

**Command**:
```bash
echo '{"test": "data"}' > /tmp/test.json

curl -X PUT http://localhost:5522/s5/fs/test-uploads/test.json \
  -H 'Content-Type: application/json' \
  --data-binary @/tmp/test.json
```

**Expected Response**:
```json
{
  "success": true,
  "path": "home/test-uploads/test.json",
  "cid": "z59A..."
}
```

**Validation**:
- [ ] Status code: 200
- [ ] Returns valid CID
- [ ] File accessible via S5 portal

---

### Test 1.4: File Download (GET)

**Objective**: Test downloading a file from S5 network

**Command**:
```bash
curl http://localhost:5522/s5/fs/test-uploads/test.json
```

**Expected Response**:
```json
{"test": "data"}
```

**Validation**:
- [ ] Status code: 200
- [ ] Content matches uploaded data
- [ ] Content-Type header correct

---

### Test 1.5: File Not Found (404)

**Objective**: Verify proper error handling for missing files

**Command**:
```bash
curl -w "\nHTTP Status: %{http_code}\n" \
  http://localhost:5522/s5/fs/nonexistent/file.json
```

**Expected Response**:
```json
{
  "error": "File not found",
  "path": "home/nonexistent/file.json"
}
```

**Validation**:
- [ ] Status code: 404
- [ ] Error message present

---

## Phase 2: Rust-to-Bridge Integration Tests

### Test 2.1: Enhanced S5 Client Initialization

**Objective**: Verify Rust can connect to bridge service

**Test File**: `tests/storage/test_enhanced_s5_bridge.rs`

```rust
#[tokio::test]
async fn test_bridge_connection() {
    // Verify bridge is running
    let client = EnhancedS5Backend::new("http://localhost:5522", None).await.unwrap();

    // Test health check
    let health = client.health_check().await;
    assert!(health.is_ok());
}
```

**Commands**:
```bash
# Start bridge first
cd services/s5-bridge && npm start &

# Run Rust test
cargo test --test storage_tests test_bridge_connection -- --nocapture
```

**Validation**:
- [ ] Test passes
- [ ] No connection errors
- [ ] Bridge logs show incoming request

---

### Test 2.2: Manifest Download via Bridge

**Objective**: Test Rust downloading vector DB manifest through bridge

**Test File**: `tests/integration/test_s5_manifest_download.rs`

```rust
#[tokio::test]
async fn test_download_manifest_via_bridge() {
    let s5_client = create_enhanced_s5_client().await;

    let manifest_path = "home/vector-databases/0xTEST/test-db/manifest.json";
    let manifest_bytes = s5_client.get(manifest_path).await.unwrap();

    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).unwrap();

    assert_eq!(manifest.dimensions, 384);
    assert!(manifest.vector_count > 0);
}
```

**Prerequisites**:
- Test manifest uploaded to S5 network
- Path stored in test configuration

**Validation**:
- [ ] Manifest downloads successfully
- [ ] JSON parses correctly
- [ ] Manifest structure valid

---

### Test 2.3: Chunk Download via Bridge

**Objective**: Test downloading vector chunks through bridge

**Test File**: Same as 2.2

```rust
#[tokio::test]
async fn test_download_chunk_via_bridge() {
    let s5_client = create_enhanced_s5_client().await;

    let chunk_path = "home/vector-databases/0xTEST/test-db/chunk-0.json";
    let chunk_bytes = s5_client.get(chunk_path).await.unwrap();

    let chunk: VectorChunk = serde_json::from_slice(&chunk_bytes).unwrap();

    assert_eq!(chunk.chunk_id, 0);
    assert!(!chunk.vectors.is_empty());
}
```

**Validation**:
- [ ] Chunk downloads successfully
- [ ] JSON parses correctly
- [ ] Vector data intact

---

### Test 2.4: Parallel Chunk Downloads

**Objective**: Test concurrent downloads through bridge

**Test File**: `tests/integration/test_parallel_s5_downloads.rs`

```rust
#[tokio::test]
async fn test_parallel_chunk_downloads() {
    let s5_client = Arc::new(create_enhanced_s5_client().await);

    let chunk_paths: Vec<String> = (0..5)
        .map(|i| format!("home/vector-databases/0xTEST/test-db/chunk-{}.json", i))
        .collect();

    let handles: Vec<_> = chunk_paths.iter().map(|path| {
        let client = s5_client.clone();
        let path = path.clone();
        tokio::spawn(async move {
            client.get(&path).await
        })
    }).collect();

    let results = futures::future::join_all(handles).await;

    // All should succeed
    for result in results {
        assert!(result.unwrap().is_ok());
    }
}
```

**Validation**:
- [ ] All 5 chunks download successfully
- [ ] No connection pool exhaustion
- [ ] Reasonable performance (<5s total)

---

### Test 2.5: Bridge Service Unavailable

**Objective**: Test error handling when bridge is down

**Test File**: `tests/integration/test_s5_error_handling.rs`

```rust
#[tokio::test]
async fn test_bridge_unavailable() {
    // Don't start bridge service for this test
    let client = EnhancedS5Backend::new("http://localhost:5522", None).await;

    // Should fail to connect
    assert!(client.is_err());

    let err = client.unwrap_err();
    assert!(err.to_string().contains("connection refused") ||
            err.to_string().contains("Connection refused"));
}
```

**Validation**:
- [ ] Rust detects connection failure
- [ ] Error message is clear
- [ ] No panic/crash

---

## Phase 3: End-to-End Vector Loading Tests

### Test 3.1: Complete Vector Database Loading Flow

**Objective**: Test full pipeline from session init to vector search

**Test File**: `tests/integration/test_e2e_vector_loading_s5.rs`

```rust
#[tokio::test]
async fn test_complete_vector_loading_flow() {
    // 1. Start bridge service (prerequisite)
    // 2. Create Rust components
    let s5_client = Arc::new(create_enhanced_s5_client().await);
    let vector_loader = VectorLoader::new(s5_client.clone(), 5);

    // 3. Load vectors from S5
    let manifest_path = "home/vector-databases/0xTEST/test-db/manifest.json";
    let user_address = "0xTEST";
    let session_key = [0u8; 32]; // Mock key

    let (progress_tx, mut progress_rx) = mpsc::channel(10);

    let vectors = vector_loader
        .load_vectors_from_s5(manifest_path, user_address, &session_key, Some(progress_tx))
        .await
        .unwrap();

    // 4. Verify progress updates
    let mut manifest_downloaded = false;
    let mut chunks_downloaded = 0;

    while let Ok(progress) = progress_rx.try_recv() {
        match progress {
            LoadProgress::ManifestDownloaded => manifest_downloaded = true,
            LoadProgress::ChunkDownloaded { .. } => chunks_downloaded += 1,
            LoadProgress::Complete { vector_count, .. } => {
                assert_eq!(vector_count, vectors.len());
            }
            _ => {}
        }
    }

    assert!(manifest_downloaded);
    assert!(chunks_downloaded > 0);
    assert!(!vectors.is_empty());

    // 5. Build HNSW index
    let index = HnswIndex::build(vectors, 384).unwrap();

    // 6. Perform search
    let query = vec![0.5f32; 384];
    let results = index.search(&query, 5, 0.5).unwrap();

    assert!(!results.is_empty());
}
```

**Validation**:
- [ ] Manifest downloads successfully
- [ ] All chunks download
- [ ] Progress updates received
- [ ] Vectors load correctly
- [ ] HNSW index builds
- [ ] Search returns results

---

### Test 3.2: Encrypted Session Init with Vector Database

**Objective**: Test encrypted transmission of vector_database info

**Test File**: `tests/integration/test_encrypted_session_with_vectors.rs`

```rust
#[tokio::test]
async fn test_encrypted_session_init_with_vector_database() {
    // 1. Create encrypted payload with vector_database field
    let session_data = serde_json::json!({
        "jobId": "test-job-s5-vectors",
        "modelName": "llama-3",
        "sessionKey": "0x1234...abcdef",
        "pricePerToken": 2000,
        "vectorDatabase": {
            "manifestPath": "home/vector-databases/0xTEST/test-db/manifest.json",
            "userAddress": "0xTEST"
        }
    });

    // 2. Encrypt with ECDH + XChaCha20-Poly1305
    let (eph_priv, eph_pub) = generate_keypair();
    let shared_key = derive_shared_key(&node_pub, &eph_priv).unwrap();
    let ciphertext = encrypt_with_aead(&session_data_bytes, &nonce, &aad, &shared_key).unwrap();

    // 3. Create payload
    let payload = EncryptedSessionPayload {
        eph_pub: eph_pub.serialize().to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature,
        aad: aad.to_vec(),
    };

    // 4. Decrypt on node side
    let session_init = decrypt_session_init(&payload, &node_priv).unwrap();

    // 5. Verify vector_database field
    assert!(session_init.vector_database.is_some());
    let vdb = session_init.vector_database.unwrap();
    assert_eq!(vdb.manifest_path, "home/vector-databases/0xTEST/test-db/manifest.json");
    assert_eq!(vdb.user_address, "0xTEST");
}
```

**Validation**:
- [ ] Encryption succeeds
- [ ] Decryption succeeds
- [ ] vector_database field intact
- [ ] All fields correct

---

### Test 3.3: WebSocket Session with S5 Vector Loading

**Objective**: Test complete WebSocket flow with S5 vectors

**Manual Test** (requires running node):

```bash
# 1. Start bridge
cd services/s5-bridge && npm start

# 2. Start Rust node
ENHANCED_S5_URL=http://localhost:5522 cargo run --release

# 3. Connect WebSocket client
node scripts/test-websocket-with-vectors.js
```

**Test Script** (`scripts/test-websocket-with-vectors.js`):
```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8080/v1/ws');

ws.on('open', () => {
  // Send session_init with vector_database
  const initMessage = {
    type: 'session_init',
    session_id: 'test-' + Date.now(),
    job_id: 12345,
    model_config: {
      model: 'tinyllama',
      max_tokens: 100
    },
    vector_database: {
      manifest_path: 'home/vector-databases/0xTEST/test-db/manifest.json',
      user_address: '0xTEST'
    }
  };

  ws.send(JSON.stringify(initMessage));
});

ws.on('message', (data) => {
  const msg = JSON.parse(data);
  console.log('Received:', msg.type);

  if (msg.type === 'vector_loading_status') {
    console.log(`Vector loading: ${msg.status} - ${msg.message}`);

    if (msg.status === 'ready') {
      console.log(`✅ Loaded ${msg.vectors_loaded} vectors`);

      // Now can send search queries
      const searchMsg = {
        type: 'searchVectors',
        session_id: initMessage.session_id,
        query_vector: new Array(384).fill(0.5),
        k: 5,
        threshold: 0.5
      };
      ws.send(JSON.stringify(searchMsg));
    }
  }

  if (msg.type === 'vector_search_result') {
    console.log(`Found ${msg.results.length} similar vectors`);
    ws.close();
  }
});
```

**Expected Output**:
```
Received: session_ready
Received: vector_loading_status
Vector loading: loading - Loading vector database...
Received: vector_loading_status
Vector loading: ready - Vector database loaded successfully
✅ Loaded 100 vectors
Received: vector_search_result
Found 5 similar vectors
```

**Validation**:
- [ ] Session initializes
- [ ] Vector loading starts
- [ ] Progress updates received
- [ ] Loading completes successfully
- [ ] Search returns results

---

## Phase 4: Error Scenario Testing

### Test 4.1: Manifest Not Found

**Objective**: Test handling of non-existent manifest

```rust
#[tokio::test]
async fn test_manifest_not_found() {
    let s5_client = Arc::new(create_enhanced_s5_client().await);
    let vector_loader = VectorLoader::new(s5_client, 5);

    let result = vector_loader
        .load_vectors_from_s5(
            "home/vector-databases/nonexistent/manifest.json",
            "0xTEST",
            &[0u8; 32],
            None
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.error_code(), "MANIFEST_NOT_FOUND");
}
```

**Validation**:
- [ ] Returns proper error
- [ ] Error code correct
- [ ] User-friendly message

---

### Test 4.2: Owner Mismatch

**Objective**: Test rejection of mismatched owner

```rust
#[tokio::test]
async fn test_owner_mismatch() {
    let s5_client = Arc::new(create_enhanced_s5_client().await);
    let vector_loader = VectorLoader::new(s5_client, 5);

    // Manifest has owner: 0xALICE
    // But we claim to be: 0xBOB
    let result = vector_loader
        .load_vectors_from_s5(
            "home/vector-databases/0xALICE/test-db/manifest.json",
            "0xBOB",  // Wrong owner
            &[0u8; 32],
            None
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.error_code(), "OWNER_MISMATCH");
}
```

**Validation**:
- [ ] Ownership validation works
- [ ] Proper error returned
- [ ] Security check passed

---

### Test 4.3: Bridge Service Restart

**Objective**: Test recovery when bridge restarts mid-operation

**Manual Test**:
```bash
# 1. Start bridge
cd services/s5-bridge && npm start &
BRIDGE_PID=$!

# 2. Start long-running operation
cargo test --test integration_tests test_large_database_load &
TEST_PID=$!

# 3. Kill bridge after 2 seconds
sleep 2 && kill $BRIDGE_PID

# 4. Restart bridge
npm start &

# 5. Check if test recovers or fails gracefully
wait $TEST_PID
```

**Expected Behavior**:
- Operation fails with connection error
- Error message indicates bridge unavailable
- No data corruption
- Retry mechanism works (if implemented)

---

### Test 4.4: Corrupted Manifest

**Objective**: Test handling of invalid manifest data

```rust
#[tokio::test]
async fn test_corrupted_manifest() {
    // Upload corrupted manifest to S5
    let corrupted_json = b"{invalid json";

    // Try to load it
    let result = vector_loader.load_vectors_from_s5(...).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.error_code(), "INVALID_MANIFEST");
}
```

**Validation**:
- [ ] JSON parsing error caught
- [ ] Proper error code
- [ ] No panic

---

## Phase 5: Performance Testing

### Test 5.1: Large Database Loading

**Objective**: Measure performance with 100K vectors

**Test Setup**:
- Upload 100K vectors (100 chunks × 1000 vectors each)
- Total size: ~150MB encrypted

**Test**:
```rust
#[tokio::test]
#[ignore] // Run separately
async fn test_large_database_performance() {
    let start = Instant::now();

    let vectors = vector_loader
        .load_vectors_from_s5(large_manifest_path, ...)
        .await
        .unwrap();

    let duration = start.elapsed();

    assert_eq!(vectors.len(), 100_000);
    assert!(duration.as_secs() < 60, "Loading took too long: {:?}", duration);

    println!("Loaded 100K vectors in {:?}", duration);
}
```

**Target Performance**:
- [ ] 100K vectors load in <60s
- [ ] Memory usage <500MB
- [ ] No connection timeouts

---

### Test 5.2: Concurrent Session Loading

**Objective**: Test multiple sessions loading simultaneously

```rust
#[tokio::test]
async fn test_concurrent_session_loading() {
    let handles: Vec<_> = (0..5).map(|i| {
        tokio::spawn(async move {
            let loader = create_vector_loader().await;
            loader.load_vectors_from_s5(...).await
        })
    }).collect();

    let results = futures::future::join_all(handles).await;

    // All should succeed
    for result in results {
        assert!(result.unwrap().is_ok());
    }
}
```

**Validation**:
- [ ] All 5 sessions load successfully
- [ ] No connection pool issues
- [ ] Reasonable total time

---

## Phase 6: Production Readiness Checklist

### 6.1: Bridge Service Health

**Checklist**:
- [ ] Bridge starts automatically on system boot
- [ ] Bridge has systemd service file (or equivalent)
- [ ] Bridge restarts on crash (systemd Restart=always)
- [ ] Health endpoint monitored by external service
- [ ] Logs collected by centralized logging system
- [ ] Metrics exposed for Prometheus/Grafana

---

### 6.2: Error Recovery

**Checklist**:
- [ ] Bridge recovers from S5 network disconnection
- [ ] Bridge recovers from portal unavailability
- [ ] Rust node detects bridge failures
- [ ] Rust node has retry logic with exponential backoff
- [ ] Failed operations logged with context
- [ ] Alerts triggered on repeated failures

---

### 6.3: Security

**Checklist**:
- [ ] Bridge only listens on localhost (not 0.0.0.0)
- [ ] Seed phrase stored securely (env var, not hardcoded)
- [ ] Seed phrase backed up securely
- [ ] Bridge has rate limiting (if needed)
- [ ] No sensitive data in logs
- [ ] HTTPS used for S5 portal communication

---

### 6.4: Monitoring

**Checklist**:
- [ ] Bridge uptime tracked
- [ ] P2P peer connection count monitored
- [ ] Download success/failure rates tracked
- [ ] Latency metrics collected
- [ ] Error rate alerts configured
- [ ] Disk space monitoring (if caching)

---

## Test Execution Schedule

### Day 1: Setup and Unit Tests
- Set up bridge service
- Run Phase 1 tests (bridge unit tests)
- Fix any bridge service issues

### Day 2: Integration Tests
- Run Phase 2 tests (Rust-to-bridge)
- Debug connection issues
- Verify data transfer works

### Day 3: End-to-End Tests
- Run Phase 3 tests (full pipeline)
- Test with real vector databases
- Validate WebSocket integration

### Day 4: Error Testing
- Run Phase 4 tests (error scenarios)
- Ensure graceful failure handling
- Document error patterns

### Day 5: Performance and Readiness
- Run Phase 5 tests (performance)
- Complete Phase 6 checklist
- Final production readiness review

---

## Test Results Template

For each test, record:

```markdown
### Test X.Y: [Test Name]

**Date**: YYYY-MM-DD
**Tester**: [Name]
**Status**: ✅ PASS / ❌ FAIL / ⚠️ PARTIAL

**Results**:
- Metric 1: [value]
- Metric 2: [value]

**Issues Found**:
- Issue 1: [description]
- Issue 2: [description]

**Notes**:
[Additional observations]
```

---

## Success Criteria

The Enhanced S5.js bridge integration is considered ready for production when:

1. ✅ All Phase 1 tests pass (bridge unit tests)
2. ✅ All Phase 2 tests pass (Rust integration)
3. ✅ All Phase 3 tests pass (end-to-end)
4. ✅ 80%+ of Phase 4 tests pass (error handling)
5. ✅ Performance targets met in Phase 5
6. ✅ All Phase 6 checklist items completed
7. ✅ No critical bugs outstanding
8. ✅ Documentation complete

---

## Troubleshooting Guide

### Issue: Bridge won't start

**Symptoms**: `npm start` fails
**Checks**:
- Node.js version >= 20
- `npm install` completed successfully
- `.env` file exists with S5_SEED_PHRASE
- Port 5522 not already in use

---

### Issue: P2P peers not connecting

**Symptoms**: `peers_connected: 0` in health check
**Checks**:
- S5_INITIAL_PEERS correct in .env
- Firewall allows WebSocket connections
- S5 portal reachable (curl https://s5.vup.cx)
- Seed phrase valid

---

### Issue: Rust can't connect to bridge

**Symptoms**: "connection refused" errors
**Checks**:
- Bridge actually running (`ps aux | grep node`)
- Bridge listening on correct port (`curl http://localhost:5522/health`)
- ENHANCED_S5_URL set correctly
- No firewall blocking localhost connections

---

### Issue: Files not found on S5

**Symptoms**: 404 errors from bridge
**Checks**:
- Files actually uploaded to S5 network
- Correct path format (home/...)
- Sufficient time for S5 propagation (30s)
- Same identity used for upload and download

---

## References

- **Implementation Plan**: `/workspace/docs/IMPLEMENTATION_S5_VECTOR_LOADING.md`
- **Bridge Documentation**: `/workspace/services/s5-bridge/README.md`
- **Deployment Guide**: `/workspace/docs/ENHANCED_S5_DEPLOYMENT.md`
- **Enhanced S5.js SDK**: `@julesl23/s5js@beta`

---

**Document Status**: Draft - Ready for execution
**Next Update**: After Phase 1 testing complete
