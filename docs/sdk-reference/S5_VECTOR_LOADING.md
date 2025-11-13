# S5 Vector Database Loading for Hosts

**Status**: SDK-side complete (Sub-phase 5.1.3), Host-side TBD

**Purpose**: Enable hosts to load pre-existing vector databases from S5 storage instead of requiring clients to upload vectors via WebSocket for every session.

---

## Overview

### Why S5 Vector Loading?

**Problem**: Current `uploadVectors` WebSocket flow requires:
- Clients to send all vectors via WebSocket (slow for large databases)
- 384-dimensional embeddings × 10K vectors = ~15 MB per upload
- Re-uploading same vectors for every new session (wasteful)

**Solution**: Pre-store vectors in S5, reference by path in `session_init`:
- Client uploads vectors to S5 once (via S5VectorStore)
- Host loads vectors from S5 when session starts
- Search executes against S5-loaded vectors
- ~1000x faster for repeated use of same database

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Client (SDK)                            │
│                                                               │
│  1. Create vector database with S5VectorStore                │
│     ├── addVectors() → Encrypted chunks to S5               │
│     ├── manifest.json stores metadata                        │
│     └── Returns manifestPath                                 │
│                                                               │
│  2. Start session with vectorDatabase option                 │
│     await sdk.startSession({                                 │
│       hostUrl: '...',                                        │
│       vectorDatabase: {                                      │
│         manifestPath: 'home/vector-databases/{user}/{db}/...'│
│         userAddress: '0x...'                                 │
│       }                                                      │
│     })                                                       │
│                                                               │
└──────────────────────┬───────────────────────────────────────┘
                       │ WebSocket session_init
                       │ (includes vectorDatabase field)
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                 Host Node (fabstir-llm-node)                 │
│                                                               │
│  3. Receive session_init with vector_database                │
│     {                                                        │
│       type: 'session_init' | 'encrypted_session_init',      │
│       vector_database: {                                     │
│         manifest_path: '...',                                │
│         user_address: '0x...'                                │
│       }                                                      │
│     }                                                        │
│                                                               │
│  4. Load vectors from S5                                     │
│     ├── Download manifest.json from S5                       │
│     ├── Verify owner matches user_address                    │
│     ├── Download vector chunks from S5                       │
│     ├── Decrypt with session encryption key                  │
│     └── Build HNSW/IVF index                                │
│                                                               │
│  5. Handle searchVectors requests                            │
│     ├── Search in HNSW/IVF index                            │
│     └── Return top-K results via WebSocket                   │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

---

## WebSocket Protocol Updates

### 1. session_init Message (Encrypted)

**SDK sends** (after encryption):
```json
{
  "type": "encrypted_session_init",
  "payload": {
    "sessionKey": "...",
    "jobId": "123",
    "modelName": "tiny-vicuna",
    "pricePerToken": 2000,
    "vectorDatabase": {
      "manifestPath": "home/vector-databases/0xABC.../my-docs/manifest.json",
      "userAddress": "0xABC..."
    }
  },
  "chain_id": 84532,
  "session_id": "sess_123",
  "job_id": "123"
}
```

**Note**: `vectorDatabase` is **optional**. If not provided, use existing `uploadVectors` flow.

### 2. session_init Message (Plaintext)

**SDK sends** (plaintext, backward compatible):
```json
{
  "type": "session_init",
  "chain_id": 84532,
  "session_id": "sess_123",
  "jobId": "123",
  "user_address": "0xABC...",
  "vector_database": {
    "manifest_path": "home/vector-databases/0xABC.../my-docs/manifest.json",
    "user_address": "0xABC..."
  }
}
```

### 3. searchVectors Message (Existing)

**SDK sends**:
```json
{
  "type": "searchVectors",
  "session_id": "sess_123",
  "requestId": "req_456",
  "queryVector": [0.1, 0.2, ...],  // 384 dimensions
  "k": 5,
  "threshold": 0.7
}
```

### 4. searchVectorsResponse (Existing)

**Host responds**:
```json
{
  "type": "searchVectorsResponse",
  "requestId": "req_456",
  "results": [
    {
      "id": "vec1",
      "score": 0.95,
      "metadata": {"source": "doc1.pdf", "page": 3}
    },
    {
      "id": "vec2",
      "score": 0.87,
      "metadata": {"source": "doc2.pdf", "page": 1}
    }
  ]
}
```

---

## S5 Storage Structure

### Manifest File

**Path**: `home/vector-databases/{userAddress}/{databaseName}/manifest.json`

**Encrypted with**: AES-GCM using EncryptionManager (client-side encryption key)

**Decrypted Structure**:
```json
{
  "name": "my-docs",
  "owner": "0xABC...",
  "description": "Documentation for Project X",
  "dimensions": 384,
  "vectorCount": 15000,
  "storageSizeBytes": 64000000,
  "created": 1700000000000,
  "lastAccessed": 1700000000000,
  "updated": 1700000000000,
  "chunks": [
    {
      "chunkId": 0,
      "cid": "s5://...",
      "vectorCount": 10000,
      "sizeBytes": 43000000,
      "updatedAt": 1700000000000
    },
    {
      "chunkId": 1,
      "cid": "s5://...",
      "vectorCount": 5000,
      "sizeBytes": 21000000,
      "updatedAt": 1700000000000
    }
  ],
  "chunkCount": 2,
  "folderPaths": ["/docs", "/research"],
  "deleted": false
}
```

### Vector Chunk Files

**Path**: `home/vector-databases/{userAddress}/{databaseName}/chunk-{N}.json`

**Encrypted with**: AES-GCM using EncryptionManager

**Decrypted Structure**:
```json
{
  "chunkId": 0,
  "vectors": [
    {
      "id": "vec1",
      "vector": [0.1, 0.2, ...],  // 384 floats
      "metadata": {
        "source": "doc1.pdf",
        "page": 3,
        "folderPath": "/docs"
      }
    },
    {
      "id": "vec2",
      "vector": [0.3, 0.4, ...],
      "metadata": {
        "source": "doc2.pdf",
        "page": 1,
        "folderPath": "/research"
      }
    }
    // ... up to 10,000 vectors per chunk
  ]
}
```

---

## Host-Side Implementation Guide

### Step 1: Update SessionStore to Store Vector Database Info

**File**: `src/session_store.rs` (or equivalent)

**Add fields to Session struct**:
```rust
pub struct Session {
    pub session_id: String,
    pub job_id: String,
    pub user_address: String,
    pub chain_id: u64,
    // ... existing fields ...

    // NEW: S5 vector database
    pub vector_database: Option<VectorDatabaseInfo>,
}

#[derive(Clone, Debug)]
pub struct VectorDatabaseInfo {
    pub manifest_path: String,
    pub user_address: String,
    pub loaded: bool,  // Track if vectors loaded
}
```

### Step 2: Parse vector_database from session_init

**File**: `src/websocket_handler.rs` (or equivalent)

**Update session_init handler**:
```rust
async fn handle_session_init(
    msg: SessionInitMessage,
    session_store: Arc<SessionStore>
) -> Result<()> {
    // ... existing code ...

    // NEW: Parse vector_database if present
    let vector_database = msg.vector_database.map(|vdb| VectorDatabaseInfo {
        manifest_path: vdb.manifest_path,
        user_address: vdb.user_address,
        loaded: false,
    });

    session_store.create_session(Session {
        session_id: msg.session_id,
        job_id: msg.job_id,
        user_address: msg.user_address,
        chain_id: msg.chain_id,
        vector_database,
        // ... other fields ...
    }).await?;

    // NEW: Load vectors from S5 if provided
    if vector_database.is_some() {
        tokio::spawn(load_vectors_from_s5(
            msg.session_id.clone(),
            session_store.clone()
        ));
    }

    Ok(())
}
```

### Step 3: Implement S5 Vector Loading

**File**: `src/vector_loader.rs` (NEW)

**Dependencies needed**:
- `s5-rust-client` (for S5 downloads)
- `aes-gcm` (for decryption)
- `hnswlib` or similar (for HNSW index)

**Implementation**:
```rust
use s5_client::S5Client;
use aes_gcm::Aes256Gcm;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Manifest {
    name: String,
    owner: String,
    dimensions: usize,
    vector_count: usize,
    chunks: Vec<ChunkMetadata>,
    // ... other fields from manifest structure above ...
}

#[derive(Deserialize)]
struct ChunkMetadata {
    chunk_id: usize,
    cid: String,
    vector_count: usize,
    // ...
}

#[derive(Deserialize)]
struct VectorChunk {
    chunk_id: usize,
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    id: String,
    vector: Vec<f32>,
    metadata: serde_json::Value,
}

pub async fn load_vectors_from_s5(
    session_id: String,
    session_store: Arc<SessionStore>
) -> Result<()> {
    // 1. Get session and vector database info
    let session = session_store.get_session(&session_id).await?;
    let vdb_info = match &session.vector_database {
        Some(vdb) => vdb,
        None => return Ok(()), // No vector DB to load
    };

    // 2. Initialize S5 client
    let s5_client = S5Client::new("https://s5.cx")?;  // Use appropriate S5 portal

    // 3. Download manifest from S5
    log::info!("Loading manifest from S5: {}", vdb_info.manifest_path);
    let manifest_encrypted = s5_client.download_file(&vdb_info.manifest_path).await?;

    // 4. Decrypt manifest
    // NOTE: You'll need the encryption key from session init (encrypted payload)
    // This is the sessionKey field that was encrypted with host's public key
    let session_key = session.get_encryption_key()?; // Implement this
    let manifest_json = decrypt_aes_gcm(&manifest_encrypted, &session_key)?;
    let manifest: Manifest = serde_json::from_str(&manifest_json)?;

    // 5. Verify owner matches
    if manifest.owner.to_lowercase() != vdb_info.user_address.to_lowercase() {
        return Err(anyhow!("Vector database owner mismatch"));
    }

    log::info!("Manifest verified: {} vectors in {} chunks",
        manifest.vector_count, manifest.chunks.len());

    // 6. Download and decrypt all chunks
    let mut all_vectors = Vec::with_capacity(manifest.vector_count);

    for chunk_meta in &manifest.chunks {
        log::info!("Loading chunk {}/{}", chunk_meta.chunk_id + 1, manifest.chunks.len());

        // Build chunk path
        let chunk_path = vdb_info.manifest_path
            .replace("manifest.json", &format!("chunk-{}.json", chunk_meta.chunk_id));

        // Download chunk
        let chunk_encrypted = s5_client.download_file(&chunk_path).await?;

        // Decrypt chunk
        let chunk_json = decrypt_aes_gcm(&chunk_encrypted, &session_key)?;
        let chunk: VectorChunk = serde_json::from_str(&chunk_json)?;

        all_vectors.extend(chunk.vectors);
    }

    log::info!("Loaded {} vectors from S5", all_vectors.len());

    // 7. Build HNSW index
    let hnsw_index = build_hnsw_index(&all_vectors, manifest.dimensions)?;

    // 8. Store index in session
    session_store.set_vector_index(&session_id, hnsw_index).await?;

    // 9. Mark as loaded
    session_store.mark_vectors_loaded(&session_id).await?;

    log::info!("✅ S5 vector loading complete for session {}", session_id);
    Ok(())
}

fn decrypt_aes_gcm(encrypted: &[u8], key: &[u8]) -> Result<String> {
    // Implement AES-GCM decryption
    // NOTE: Client uses Web Crypto API's AES-GCM
    // Must match exactly (nonce size, tag size, etc.)

    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use aes_gcm::aead::{Aead, Payload};

    // Encrypted format: [nonce (12 bytes) | ciphertext+tag]
    let nonce = Nonce::from_slice(&encrypted[0..12]);
    let ciphertext = &encrypted[12..];

    let cipher = Aes256Gcm::new_from_slice(key)?;
    let plaintext = cipher.decrypt(nonce, Payload {
        msg: ciphertext,
        aad: b"", // No additional data
    })?;

    Ok(String::from_utf8(plaintext)?)
}

fn build_hnsw_index(vectors: &[Vector], dimensions: usize) -> Result<HnswIndex> {
    // Build HNSW index for fast cosine similarity search
    // Use hnswlib-rs or similar library

    use hnswlib::{Hnsw, Metric};

    let mut index = Hnsw::new(
        Metric::Cosine,
        dimensions,
        vectors.len(),
        16,  // M
        200, // ef_construction
    )?;

    for (i, vec) in vectors.iter().enumerate() {
        index.add_point(&vec.vector, i)?;
    }

    Ok(index)
}
```

### Step 4: Update searchVectors Handler

**File**: `src/websocket_handler.rs`

**Update search handler to use S5-loaded index**:
```rust
async fn handle_search_vectors(
    msg: SearchVectorsMessage,
    session_store: Arc<SessionStore>
) -> Result<SearchVectorsResponse> {
    // Get session
    let session = session_store.get_session(&msg.session_id).await?;

    // Check if vectors loaded from S5
    let index = if let Some(vdb) = &session.vector_database {
        if !vdb.loaded {
            return Err(anyhow!("Vectors still loading from S5, try again in a moment"));
        }
        session_store.get_vector_index(&msg.session_id).await?
    } else {
        // Fall back to uploaded vectors (existing flow)
        session_store.get_uploaded_vectors_index(&msg.session_id).await?
    };

    // Perform search
    let results = index.search(&msg.query_vector, msg.k, msg.threshold)?;

    Ok(SearchVectorsResponse {
        request_id: msg.request_id,
        results,
    })
}
```

---

## Testing Checklist

### Unit Tests
- [ ] Manifest download and decryption
- [ ] Chunk download and decryption
- [ ] Owner verification
- [ ] HNSW index building
- [ ] Search with S5-loaded vectors

### Integration Tests
- [ ] End-to-end: SDK uploads to S5 → Host loads → Search works
- [ ] Multiple sessions sharing same S5 database
- [ ] Concurrent loading (multiple sessions starting)
- [ ] Error handling: Invalid manifest path, corrupt data, network failures

### Performance Tests
- [ ] Loading 10K vectors: < 5 seconds
- [ ] Loading 100K vectors: < 30 seconds
- [ ] Search latency: < 100ms with 100K vectors
- [ ] Memory usage: Reasonable for large databases

---

## Error Handling

### Common Errors

**1. Manifest Not Found**
```json
{
  "type": "error",
  "code": "MANIFEST_NOT_FOUND",
  "message": "Vector database manifest not found at path: home/vector-databases/..."
}
```

**2. Owner Mismatch**
```json
{
  "type": "error",
  "code": "OWNER_MISMATCH",
  "message": "Vector database owner does not match user_address"
}
```

**3. Decryption Failed**
```json
{
  "type": "error",
  "code": "DECRYPTION_FAILED",
  "message": "Failed to decrypt vector database: Invalid encryption key or corrupted data"
}
```

**4. Loading In Progress**
```json
{
  "type": "error",
  "code": "VECTORS_LOADING",
  "message": "Vectors still loading from S5, try search again in a moment"
}
```

---

## Backward Compatibility

**IMPORTANT**: S5 vector loading is **optional**. Existing `uploadVectors` flow must continue to work.

**Fallback Strategy**:
1. If `vector_database` provided in session_init → Load from S5
2. If no `vector_database` → Use existing uploadVectors flow
3. Support both in same session (S5 + uploaded vectors)

---

## Performance Considerations

### S5 Download Optimization
- **Parallel chunk downloads**: Download all chunks in parallel (not sequential)
- **Connection pooling**: Reuse HTTP connections for S5 portal
- **Resume support**: Handle partial downloads gracefully

### Memory Management
- **Stream processing**: Don't load all vectors into memory at once
- **Index persistence**: Cache HNSW index to disk for reuse
- **Lazy loading**: Load chunks on-demand if database is huge (1M+ vectors)

### Caching Strategy
- **Manifest caching**: Cache manifests for frequently used databases
- **Index caching**: Persist HNSW index between sessions (keyed by manifest_path)
- **TTL**: Invalidate cache after 24 hours or on manifest update

---

## Security Considerations

### Owner Verification
**CRITICAL**: Always verify `manifest.owner === vector_database.user_address`

This prevents users from loading another user's private vector databases.

### Encryption Key Management
- **Session key**: Encrypted with host's public key in encrypted_session_init
- **Storage**: Never persist decrypted vectors to disk without re-encryption
- **Memory**: Clear decrypted data when session ends

### Rate Limiting
- **S5 downloads**: Limit concurrent downloads per host (prevent DoS)
- **Index building**: Queue index builds to prevent CPU exhaustion
- **Search requests**: Rate limit per session

---

## Migration Path

### Phase 1: SDK-side Complete ✅
- ExtendedSessionConfig updated with vectorDatabase field
- sendEncryptedInit includes vectorDatabase in payload
- sendPlaintextInit includes vector_database field

### Phase 2: Host-side Implementation [TBD]
- Update SessionStore to parse vector_database
- Implement S5 vector loading
- Update searchVectors to use S5-loaded index
- Add error handling and logging

### Phase 3: Testing & Validation [TBD]
- Unit tests for S5 loading
- Integration tests with SDK
- Performance benchmarks
- Production deployment

---

## References

- **SDK Implementation**: `packages/sdk-core/src/managers/SessionManager.ts`
- **S5VectorStore**: `packages/sdk-core/src/storage/S5VectorStore.ts`
- **Implementation Plan**: `docs/IMPLEMENTATION_S5_VECTOR_STORE.md`
- **WebSocket Protocol**: `docs/node-reference/WEBSOCKET_API_SDK_GUIDE.md`

---

**Last Updated**: 2025-11-13
