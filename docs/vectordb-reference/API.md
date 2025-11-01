# Fabstir AI Vector Database API Documentation

**Version:** v0.2.0 (CRUD Operations Release)

## Overview

Fabstir AI Vector Database is a high-performance, decentralized vector database built on S5 storage with hybrid HNSW/IVF indexing, chunked storage, and production-ready CRUD operations for AI applications. It provides scalable vector similarity search with encryption by default, optimized for video metadata search and decentralized storage.

### Key Features

- **Chunked Storage**: Scalable partitioning with lazy loading (10K vectors/chunk default)
- **Encrypted by Default**: ChaCha20-Poly1305 encryption at rest (<5% overhead)
- **High Performance**: 58ms warm search latency, 64 MB memory for 100K vectors
- **Decentralized Storage**: Built on S5 network for immutable, content-addressed storage
- **Hybrid Indexing**: Combines HNSW (Hierarchical Navigable Small World) for recent data and IVF (Inverted File) for historical data
- **Multiple Interfaces**: REST API + Node.js native bindings (napi-rs) + WASM
- **Memory Efficient**: 10x memory reduction vs v0.1.0 via chunked storage with LRU cache
- **Production Ready**: Tested at scale, supports 1M+ vectors
- **Time-based Partitioning**: Automatic migration between indices based on data age
- **CBOR Serialization**: Compatible with S5 storage format
- **CRUD Operations (v0.2.0)**: Delete, update, metadata filtering, schema validation, vacuum

### Architecture Overview

```
┌─────────────────────────────────────────────┐
│         Client Interfaces (v0.1.1)          │
│  ┌──────────────┐  ┌──────────────┐  ┌────┐ │
│  │ Node.js      │  │ REST API     │  │WASM│ │
│  │ (napi-rs)    │  │ (Port 7533)  │  │    │ │
│  └──────┬───────┘  └──────┬───────┘  └─┬──┘ │
└─────────┼──────────────────┼────────────┼────┘
          │                  │            │
          └──────────┬───────┴────────────┘
                     │
          ┌──────────▼──────────┐
          │   Hybrid Index      │
          │  ┌─────────────┐    │
          │  │ HNSW Index  │    │
          │  │ (Recent)    │    │
          │  └─────────────┘    │
          │  ┌─────────────┐    │
          │  │ IVF Index   │    │
          │  │ (Historical)│    │
          │  └─────────────┘    │
          └──────────┬──────────┘
                     │
          ┌──────────▼──────────┐
          │ Chunked Storage     │
          │  ┌─────────────┐    │
          │  │ 10K/chunk   │    │
          │  │ Lazy Load   │    │
          │  └──────┬──────┘    │
          │         │           │
          │  ┌──────▼──────┐    │
          │  │ Encryption  │    │
          │  │ (ChaCha20)  │    │
          │  └──────┬──────┘    │
          │         │           │
          │  ┌──────▼──────┐    │
          │  │Enhanced S5  │    │
          │  │  (CBOR)     │    │
          │  └─────────────┘    │
          └─────────────────────┘
                     │
          ┌──────────▼──────────┐
          │   S5 Network        │
          │ (Decentralized)     │
          └─────────────────────┘
```

**v0.2.0 Architecture Highlights**:

- **Chunked Storage**: Vectors partitioned into 10K chunks with lazy loading
- **LRU Cache**: 150 MB default cache for hot chunks
- **Encryption**: ChaCha20-Poly1305 at rest, <5% overhead
- **Multiple Clients**: Node.js native bindings (recommended), REST API, WASM
- **CRUD Operations**: Soft deletion, metadata updates, filtering, schema validation, vacuum
- **Manifest v3**: Persistence format with deletion tracking and schema support

## Client Interfaces

Fabstir Vector DB provides multiple interfaces for different use cases:

### 1. Node.js Native Bindings (Recommended for SDK Integration)

**Best for:** Node.js/TypeScript applications, SDK integration, production deployments

```javascript
const { VectorDbSession } = require('@fabstir/vector-db-native');

const session = await VectorDbSession.create({
  s5Portal: 'http://localhost:5522',
  userSeedPhrase: 'your-seed-phrase',
  sessionId: 'user-123',
  encryptAtRest: true,    // Enabled by default
  chunkSize: 10000,       // 10K vectors/chunk
  cacheSizeMb: 150,       // Cache 10 chunks
});

// Add vectors with native object metadata
await session.addVectors([{
  id: 'doc1',
  vector: [...],  // 384-dim
  metadata: { text: 'Hello world', userId: 'user123' }
}]);

// Save to S5 (encrypted, chunked)
const cid = await session.saveToS5();

// Load from S5 (lazy loading)
await session.loadUserVectors(cid, { lazyLoad: true });

// Search (58ms warm cache)
const results = await session.search(queryVector, 5);
console.log(results[0].metadata.text);  // Direct property access

await session.destroy();  // CRITICAL: Clean up memory
```

**Documentation:** [v0.2.0 Quick Start Guide](./SDK_V0.2.0_QUICK_START.md) | [SDK Developer README](./SDK_DEVELOPER_README.md)

#### v0.2.0 CRUD Operations Example

```javascript
const { VectorDbSession } = require('@fabstir/vector-db-native');

async function crudExample() {
  const session = await VectorDbSession.create({
    s5Portal: 'http://localhost:5522',
    userSeedPhrase: 'your-seed-phrase',
    sessionId: 'user-123',
  });

  try {
    // 1. Set schema for metadata validation
    await session.setSchema({
      type: 'object',
      properties: {
        title: { type: 'string', required: true },
        category: { type: 'string', required: true },
        price: { type: 'number', required: false }
      }
    });

    // 2. Add vectors
    await session.addVectors([
      { id: 'doc-1', vector: [...], metadata: { title: 'Product A', category: 'electronics', price: 99.99 } }
    ]);

    // 3. Search with metadata filter (MongoDB-style)
    const results = await session.search(queryVector, 5, {
      filter: {
        $and: [
          { category: { $eq: 'electronics' } },
          { price: { $lte: 150 } }
        ]
      }
    });

    // 4. Update metadata (no re-indexing)
    await session.updateMetadata('doc-1', {
      title: 'Product A - Updated',
      price: 89.99
    });

    // 5. Delete by metadata
    const deleteResult = await session.deleteByMetadata({
      category: 'books'
    });
    console.log(`Deleted ${deleteResult.deletedCount} vectors`);

    // 6. Vacuum (optional, before save)
    const vacStats = await session.vacuum();
    console.log(`Removed ${vacStats.totalRemoved} deleted vectors`);

    // 7. Save to S5
    const cid = await session.saveToS5();

  } finally {
    await session.destroy();
  }
}
```

**New v0.2.0 Methods:**
- `deleteVector(id: string)` - Delete single vector
- `deleteByMetadata(filter: MetadataFilter)` - Bulk delete by metadata
- `updateMetadata(id: string, metadata: any)` - Update metadata without re-indexing
- `search(query, k, { filter })` - Search with MongoDB-style filters
- `setSchema(schema)` - Optional metadata validation
- `vacuum()` - Physical cleanup of deleted vectors

See [v0.2.0 Quick Start Guide](./SDK_V0.2.0_QUICK_START.md) for complete CRUD documentation.

### 2. REST API (Language-Agnostic)

**Best for:** Microservices, language-agnostic access, HTTP-based integrations

```bash
# Production endpoint
curl -X POST http://localhost:7533/api/v1/search \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, ...], "k": 5}'
```

**Port:** 7533 (production), 7530-7532 (development)
**Documentation:** See [REST API Endpoints](#rest-api-endpoints) below

### 3. WASM (Browser/Portable)

**Best for:** Browser applications, portable deployments, edge computing

```javascript
import init, { VectorDB } from "@fabstir/vector-db-wasm";

await init();
const db = new VectorDB();
db.add_vector(id, vector, metadata);
const results = db.search(query_vector, k);
```

### Interface Comparison

| Feature               | Node.js Native  | REST API        | WASM               |
| --------------------- | --------------- | --------------- | ------------------ |
| **Performance**       | ⭐⭐⭐⭐⭐ Best | ⭐⭐⭐ Good     | ⭐⭐⭐⭐ Very Good |
| **Memory Efficiency** | ⭐⭐⭐⭐⭐      | ⭐⭐⭐⭐        | ⭐⭐⭐             |
| **S5 Persistence**    | ✅ Full support | ✅ Full support | ⚠️ Limited         |
| **Chunked Storage**   | ✅ Native       | ✅ Native       | ⚠️ Limited         |
| **Native Metadata**   | ✅ Objects      | ❌ JSON strings | ❌ JSON strings    |
| **Use Case**          | SDK integration | Microservices   | Browser apps       |
| **Setup Complexity**  | Low             | Medium          | Low                |

## Installation & Setup

### Prerequisites

- **For REST API Server:**
  - Docker and Docker Compose (recommended)
  - OR: Rust 1.70+ (for native installation)
  - Enhanced S5.js running on port 5522
- **For Node.js Bindings:**
  - Node.js 18+ and npm
  - Rust toolchain (for building from source)
- **S5 Network Access:**
  - Enhanced S5.js portal (default: http://localhost:5522)
  - OR: Direct S5 network access (https://s5.vup.cx)

### Installation Steps

#### Option 1: Node.js Native Bindings (Recommended for SDK)

```bash
# Install from npm
npm install @fabstir/vector-db-native

# Or build from source
cd bindings/node
npm install
npm run build
```

**Usage:** See [Client Interfaces](#client-interfaces) section above.

#### Option 2: REST API Server with Docker

```bash
# Clone the repository
git clone https://github.com/fabstir/fabstir-ai-vector-db.git
cd fabstir-ai-vector-db

# Start production container
docker-compose -f docker-compose.dev.yml up -d vector-db-prod

# REST API will be available at:
# http://localhost:7533/api/v1
```

#### Option 3: Build REST API from Source

```bash
# Clone the repository
git clone https://github.com/fabstir/fabstir-ai-vector-db.git
cd fabstir-ai-vector-db

# Build the Rust backend
cargo build --release --bin server

# Run the server
VECTOR_DB_PORT=7533 cargo run --release --bin server
```

**Note:** For production deployment, you must have Enhanced S5.js running on port 5522. See [Enhanced S5.js setup](../partners/S5/GitHub/s5.js/).

### Configuration Options

#### REST API Server Configuration

Environment variables can be set in `.env` file or passed directly:

```bash
# S5 Storage Configuration
S5_MODE=real                              # Storage mode: "real" or "mock" (default: real)
S5_PORTAL_URL=http://localhost:5522       # Enhanced S5.js URL (required)
S5_SEED_PHRASE=your-seed-phrase-here      # S5 seed phrase (12 or 24 words)
S5_SEED_PHRASE_FILE=/path/to/seed.txt     # Optional: Path to seed phrase file
S5_API_KEY=your-api-key                   # Optional: S5 API key
S5_CONNECTION_TIMEOUT=30000               # Connection timeout in ms (default: 30000)
S5_RETRY_ATTEMPTS=3                       # Number of retry attempts (default: 3)

# Vector Database Configuration
VECTOR_DIMENSION=384                      # Vector dimensions (default: 384 for all-MiniLM-L6-v2)
MAX_VECTORS_PER_INDEX=1000000             # Maximum vectors per index

# Chunked Storage Configuration (v0.1.1)
CHUNK_SIZE=10000                          # Vectors per chunk (default: 10000)
CACHE_SIZE_MB=150                         # Chunk cache size in MB (default: 150)
LAZY_LOAD=true                            # Enable lazy loading (default: true)
ENCRYPT_AT_REST=true                      # Enable encryption (default: true)

# Index Configuration
HNSW_M=16                                 # HNSW connectivity parameter
HNSW_EF_CONSTRUCTION=200                  # HNSW construction quality
IVF_N_CLUSTERS=256                        # IVF number of clusters
IVF_N_PROBE=16                            # IVF clusters to search

# Server Configuration
VECTOR_DB_HOST=0.0.0.0                    # Server host (default: 0.0.0.0)
VECTOR_DB_PORT=7533                       # REST API port (production: 7533, dev: 7530-7532)
VECTOR_DB_MAX_REQUEST_SIZE=10485760       # Max request size (10MB)
VECTOR_DB_TIMEOUT_SECS=30                 # Request timeout
VECTOR_DB_CORS_ORIGINS=http://localhost:3000  # CORS origins
```

#### Node.js Bindings Configuration (v0.1.1)

For Node.js native bindings, configuration is passed programmatically:

```javascript
const config = {
  s5Portal: "http://localhost:5522", // Enhanced S5.js endpoint
  userSeedPhrase: "your-seed-phrase", // 12 or 24 words
  sessionId: "session-123", // Unique session ID

  // Optional: Chunked storage tuning
  chunkSize: 10000, // 10K vectors/chunk (default)
  cacheSizeMb: 150, // 150 MB cache (default)
  encryptAtRest: true, // ChaCha20-Poly1305 (default)

  // Optional: Advanced settings
  memoryBudgetMb: 512, // Memory limit (default: 512)
  debug: false, // Debug logging (default: false)
};

const session = await VectorDbSession.create(config);
```

**Tuning Guidelines:**

- **Memory-constrained:** `chunkSize: 5000`, `cacheSizeMb: 75`
- **Performance-focused:** `chunkSize: 20000`, `cacheSizeMb: 300`
- **Large datasets (1M+):** `chunkSize: 20000`, `cacheSizeMb: 300`

See [Performance Tuning Guide](./PERFORMANCE_TUNING.md) for detailed optimization strategies.

### Docker Setup

The project includes Docker configuration for production and development:

```yaml
# docker-compose.dev.yml (excerpt)
version: "3.8"

services:
  vector-db-prod:
    build:
      context: .
      dockerfile: Dockerfile.production
    ports:
      - "7533:7533" # Production REST API
    environment:
      - S5_PORTAL_URL=http://host.docker.internal:5522
      - VECTOR_DIMENSION=${VECTOR_DIMENSION:-384}
      - CHUNK_SIZE=${CHUNK_SIZE:-10000}
      - CACHE_SIZE_MB=${CACHE_SIZE_MB:-150}
      - ENCRYPT_AT_REST=${ENCRYPT_AT_REST:-true}
    extra_hosts:
      - "host.docker.internal:host-gateway"
```

**Important:** Use `host.docker.internal:5522` for S5 portal URL when running in Docker to access Enhanced S5.js on the host.

## Core API Reference

### REST API Endpoints

**Base URL (Production):** `http://localhost:7533/api/v1`
**Development Ports:** 7530-7532

#### Health Check

```http
GET /health
```

Response:

```json
{
  "status": "healthy",
  "version": "0.1.0",
  "storage": {
    "mode": "mock",
    "connected": true,
    "base_url": "http://localhost:5524"
  },
  "indices": {
    "hnsw": {
      "healthy": true,
      "vector_count": 1234
    },
    "ivf": {
      "healthy": true,
      "vector_count": 5678
    }
  }
}
```

The `storage` object provides information about the S5 storage backend:

- `mode`: Either "mock" or "real" depending on configuration
- `connected`: Whether the storage backend is accessible
- `base_url`: Present for mock mode, shows the mock server URL
- `portal_url`: Present for real mode, shows the S5 portal URL

#### Vector Operations

##### Insert Vector

```http
POST /vectors
Content-Type: application/json

{
  "id": "vec_12345",
  "vector": [0.1, 0.2, 0.3, ...],
  "metadata": {
    "video_id": "video_abc",
    "title": "Example Video",
    "tags": ["ai", "tutorial"]
  }
}
```

Response:

```json
{
  "id": "vec_12345",
  "index": "recent",
  "timestamp": "2025-01-15T10:30:00Z"
}
```

##### Batch Insert

```http
POST /vectors/batch
Content-Type: application/json

{
  "vectors": [
    {
      "id": "vec_001",
      "vector": [0.1, 0.2, ...],
      "metadata": {...}
    },
    {
      "id": "vec_002",
      "vector": [0.3, 0.4, ...],
      "metadata": {...}
    }
  ]
}
```

Response:

```json
{
  "successful": 2,
  "failed": 0,
  "errors": []
}
```

##### Get Vector

```http
GET /vectors/{id}
```

Response:

```json
{
  "id": "vec_12345",
  "vector": [0.1, 0.2, 0.3, ...],
  "metadata": {
    "video_id": "video_abc",
    "title": "Example Video"
  },
  "index": "recent",
  "timestamp": "2025-01-15T10:30:00Z"
}
```

##### Delete Vector

```http
DELETE /vectors/{id}
```

Response: `204 No Content`

#### Search Operations

##### Vector Search

```http
POST /search
Content-Type: application/json

{
  "vector": [0.1, 0.2, 0.3, ...],
  "k": 10,
  "filter": {
    "tags": ["ai", "tutorial"]
  },
  "options": {
    "search_recent": true,
    "search_historical": true,
    "hnsw_ef": 50,
    "ivf_n_probe": 16,
    "timeout_ms": 5000,
    "include_metadata": true,
    "score_threshold": 0.8
  }
}
```

Response:

```json
{
  "results": [
    {
      "id": "vec_12345",
      "distance": 0.15,
      "score": 0.92,
      "metadata": {
        "video_id": "video_abc",
        "title": "Example Video"
      }
    }
  ],
  "search_time_ms": 23.5,
  "indices_searched": 2,
  "partial_results": false
}
```

#### Admin Operations

##### Get Statistics

```http
GET /admin/statistics
```

Response:

```json
{
  "total_vectors": 10000,
  "recent_vectors": 2000,
  "historical_vectors": 8000,
  "memory_usage": {
    "total_bytes": 104857600,
    "hnsw_bytes": 41943040,
    "ivf_bytes": 62914560
  }
}
```

##### Trigger Migration

```http
POST /admin/migrate
```

Response:

```json
{
  "vectors_migrated": 500,
  "duration_ms": 1234.5
}
```

##### Rebalance Clusters

```http
POST /admin/rebalance
```

Response:

```json
{
  "clusters_modified": 32,
  "vectors_moved": 1500
}
```

##### Create Backup

```http
POST /admin/backup
Content-Type: application/json

{
  "backup_path": "/backups/vector-db-backup.tar.gz",
  "compress": true
}
```

Response:

```json
{
  "backup_size": 52428800,
  "vectors_backed_up": 10000,
  "compression_ratio": 0.65
}
```

#### Streaming Operations

##### Server-Sent Events (SSE)

```http
GET /stream/updates
```

Event stream format:

```
event: vector_added
data: {"id": "vec_12345", "index": "recent"}

event: migration_started
data: {"vectors_to_migrate": 500}

event: migration_completed
data: {"vectors_migrated": 500, "duration_ms": 1234}
```

##### WebSocket Connection

```http
GET /ws
Upgrade: websocket
```

## Future Features (Planned)

The following features are planned for future releases:

### MCP Server Integration (Planned)

**Status:** 🚧 Not yet implemented

The MCP (Model Context Protocol) server will enable privacy-preserving integration with LLMs while maintaining full user control over data. MCP is a **local protocol** that runs on your machine, ensuring data never leaves your control.

**Key Privacy Features:**

- ✅ **Local Protocol**: Runs entirely on your machine, not a cloud service
- ✅ **Decentralized Storage**: All vector data remains in S5 decentralized storage
- ✅ **User Sovereignty**: Full control via seed phrases, no third-party access
- ✅ **No Data Centralization**: Vectors never touch centralized servers
- ✅ **Open Standard**: Works with any MCP-compatible LLM (local models, etc.)

**Planned Capabilities:**

- Direct vector search tool for MCP-compatible LLMs
- Vector insertion/management via local MCP protocol
- Real-time context retrieval for AI applications
- Seamless integration with MCP clients (Claude Desktop, Continue, etc.)
- Support for local/open-source LLMs via MCP

**Privacy Architecture:**

```
User's Machine (Full Control)
├─ MCP Client (local LLM, etc.)
│  └─ Local MCP Protocol
│     └─ Fabstir Vector DB (localhost)
│        └─ S5 Decentralized Storage
│           └─ User's encrypted data (seed phrase controlled)
```

**Why MCP for Platformless AI:**

- Maintains decentralization principles
- No centralized intermediaries
- User controls all data access
- Works with any LLM that supports MCP
- Privacy-first by design

**Tracking:** See GitHub issues for progress updates.

## Data Models

### Vector Format

```typescript
interface Vector {
  id: string; // Unique identifier (e.g., "vec_12345")
  vector: number[]; // Float32 array of embeddings
  metadata?: object; // Optional metadata
}
```

### VectorId

Vectors are identified using a 32-byte BLAKE3 hash:

```rust
pub struct VectorId([u8; 32]);

impl VectorId {
    pub fn new() -> Self;                    // Generate random ID
    pub fn from_string(s: &str) -> Self;     // Hash from string
    pub fn hash_hex(&self) -> String;        // Hex representation
    pub fn to_string(&self) -> String;       // Human-readable format
}
```

### Embedding

```rust
pub struct Embedding {
    data: Vec<f32>,
}

impl Embedding {
    pub fn dimension(&self) -> usize;
    pub fn magnitude(&self) -> f32;
    pub fn normalize(&self) -> Self;
    pub fn cosine_similarity(&self, other: &Self) -> f32;
    pub fn euclidean_distance(&self, other: &Self) -> f32;
}
```

### Video Metadata

```typescript
interface VideoMetadata {
  video_id: string;
  title: string;
  description?: string;
  tags: string[];
  duration_seconds: number;
  upload_timestamp: string; // ISO 8601
  model_name: string; // Embedding model used
  extra: Record<string, any>;
}
```

### Search Result

```typescript
interface SearchResult {
  id: string;
  distance: number; // Euclidean distance
  score: number; // Normalized similarity score (0-1)
  metadata?: object;
}
```

### Index Configuration

#### HNSW Configuration

```rust
pub struct HNSWConfig {
    pub max_connections: usize,          // M parameter (default: 16)
    pub max_connections_layer_0: usize,  // M*2 for layer 0 (default: 32)
    pub ef_construction: usize,          // Construction quality (default: 200)
    pub seed: Option<u64>,              // Random seed
}
```

#### IVF Configuration

```rust
pub struct IVFConfig {
    pub n_clusters: usize,      // Number of clusters (default: 256)
    pub n_probe: usize,         // Clusters to search (default: 16)
    pub train_size: usize,      // Training set size (default: 10000)
    pub max_iterations: usize,  // K-means iterations (default: 25)
    pub seed: Option<u64>,      // Random seed
}
```

#### Hybrid Configuration

```rust
pub struct HybridConfig {
    pub recent_threshold: Duration,      // Age threshold (default: 7 days)
    pub hnsw_config: HNSWConfig,
    pub ivf_config: IVFConfig,
    pub migration_batch_size: usize,     // Batch size for migration (default: 100)
    pub auto_migrate: bool,              // Auto-migrate old vectors (default: true)
}
```

## S5 Storage Integration

### How Vectors are Stored in S5

The database uses S5's decentralized storage with the following structure:

1. **Content Addressing**: Each vector is stored with a CID (Content Identifier)
2. **CBOR Serialization**: All data is serialized using CBOR format for S5 compatibility
3. **Compression**: Optional zstd compression for large vectors
4. **Immutability**: Once stored, vectors cannot be modified (only new versions can be added)

### Storage Layout

```
/vectors/
  ├── recent/           # HNSW index data
  │   ├── nodes/        # Vector nodes
  │   └── graph/        # HNSW graph connections
  ├── historical/       # IVF index data
  │   ├── centroids/    # Cluster centroids
  │   └── inverted/     # Inverted lists
  └── metadata/         # Vector metadata
```

### S5 Client Configuration

```rust
pub struct S5Config {
    pub node_url: String,           // S5 portal URL
    pub api_key: Option<String>,    // Optional API key
    pub enable_compression: bool,   // Enable compression
    pub cache_size: usize,         // Local cache size
}
```

### Seed Phrase Management

The S5 seed phrase can be:

1. Set via `S5_SEED_PHRASE` environment variable
2. Loaded from a file via `S5_SEED_PHRASE_FILE` environment variable
3. Generated automatically and stored securely
4. Retrieved via admin API (with proper authentication)

#### Using Environment Variable

```bash
# Set custom seed phrase (must be 12 or 24 words)
export S5_SEED_PHRASE="your twelve word mnemonic seed phrase here for s5 storage access"
```

#### Using Seed Phrase File (Recommended)

```bash
# Create seed phrase file with proper permissions
echo "your twelve word seed phrase goes here like this example phrase" > ~/.s5-seed
chmod 600 ~/.s5-seed  # Restrict access to owner only

# Use file for configuration
export S5_SEED_PHRASE_FILE=~/.s5-seed
```

**Security Notes**:

- Seed phrases must contain exactly 12 or 24 words
- On Unix systems, seed phrase files with world-readable permissions will trigger a warning
- The file method takes precedence over the environment variable if both are set
- Seed phrases are never logged or exposed in API responses

### Portal Configuration

```bash
# Use default S5 portal
export S5_PORTAL_URL=https://s5.vup.cx

# Or use custom portal
export S5_PORTAL_URL=http://localhost:5524
```

## HAMT Sharding

### Automatic Activation

HAMT (Hash Array Mapped Trie) sharding automatically activates when:

- Vector count exceeds `HAMT_ACTIVATION_THRESHOLD` (default: 1000)
- Provides O(log n) lookup performance
- Maintains compatibility with S5's HAMT implementation

### Performance Characteristics

| Vector Count | Without HAMT | With HAMT |
| ------------ | ------------ | --------- |
| < 1,000      | O(1)         | O(1)      |
| 10,000       | O(n)         | O(log n)  |
| 100,000      | O(n)         | O(log n)  |
| 1,000,000    | O(n)         | O(log n)  |

### Configuration Options

```bash
# Activation threshold
HAMT_ACTIVATION_THRESHOLD=1000

# Branching factor (affects tree depth)
HAMT_BRANCHING_FACTOR=32

# Maximum depth
HAMT_MAX_DEPTH=10
```

### HAMT Structure

```
Root Node
├─ Branch[0-31]
│  ├─ Branch[0-31]
│  │  └─ Leaf → Vector Data
│  └─ Leaf → Vector Data
└─ Branch[32-63]
   └─ Leaf → Vector Data
```

## Examples

### Basic Usage Examples

#### 1. Initialize Client (TypeScript)

```typescript
import { VectorDBClient } from "fabstir-ai-vector-db";

const client = new VectorDBClient({
  apiUrl: "http://localhost:7530",
  apiKey: process.env.VECTOR_DB_API_KEY,
});

// Check health
const health = await client.health();
console.log("Database status:", health.status);
```

#### 2. Insert Vectors

```typescript
// Single vector insertion
const vector = await generateEmbedding("AI tutorial video");
const result = await client.insertVector({
  id: "vec_001",
  vector: vector,
  metadata: {
    video_id: "video_123",
    title: "Introduction to AI",
    tags: ["ai", "tutorial", "beginner"],
  },
});

// Batch insertion
const vectors = await Promise.all([
  generateEmbedding("Machine learning basics"),
  generateEmbedding("Deep learning fundamentals"),
  generateEmbedding("Neural networks explained"),
]);

const batchResult = await client.batchInsert({
  vectors: vectors.map((vec, i) => ({
    id: `vec_${i + 100}`,
    vector: vec,
    metadata: {
      video_id: `video_${i + 100}`,
      title: `Lesson ${i + 1}`,
    },
  })),
});
```

#### 3. Search Vectors

```typescript
// Basic search
const queryVector = await generateEmbedding("How do neural networks work?");
const results = await client.search({
  vector: queryVector,
  k: 5,
});

// Advanced search with options
const advancedResults = await client.search({
  vector: queryVector,
  k: 10,
  filter: {
    tags: ["neural-networks", "deep-learning"],
  },
  options: {
    search_recent: true,
    search_historical: true,
    hnsw_ef: 100, // Higher = better quality, slower
    ivf_n_probe: 32, // More clusters = better quality, slower
    timeout_ms: 5000,
    include_metadata: true,
    score_threshold: 0.75,
  },
});
```

### Advanced Queries

#### 1. Time-based Search

```typescript
// Search only recent vectors (last 24 hours)
const recentResults = await client.search({
  vector: queryVector,
  k: 10,
  options: {
    search_recent: true,
    search_historical: false,
    recent_threshold_override: 24 * 60 * 60 * 1000, // 24 hours in ms
  },
});
```

#### 2. Filtered Search

```typescript
// Complex metadata filtering
const filteredResults = await client.search({
  vector: queryVector,
  k: 20,
  filter: {
    $and: [
      { tags: { $in: ["ai", "ml", "deep-learning"] } },
      { duration_seconds: { $gte: 300, $lte: 1800 } },
      { model_name: "text-embedding-ada-002" },
    ],
  },
});
```

#### 3. Streaming Updates

```typescript
// Subscribe to real-time updates
const eventSource = new EventSource("http://localhost:7530/stream/updates");

eventSource.addEventListener("vector_added", (event) => {
  const data = JSON.parse(event.data);
  console.log("New vector added:", data.id);
});

eventSource.addEventListener("migration_completed", (event) => {
  const data = JSON.parse(event.data);
  console.log(`Migration completed: ${data.vectors_migrated} vectors`);
});
```

### Integration with AI Applications

#### 1. OpenAI Integration

```typescript
import OpenAI from "openai";
import { VectorDBClient } from "fabstir-ai-vector-db";

const openai = new OpenAI();
const vectorDB = new VectorDBClient();

async function indexVideo(videoTranscript: string, metadata: any) {
  // Generate embedding
  const response = await openai.embeddings.create({
    model: "text-embedding-ada-002",
    input: videoTranscript,
  });

  const embedding = response.data[0].embedding;

  // Store in vector database
  await vectorDB.insertVector({
    id: `vec_${metadata.video_id}`,
    vector: embedding,
    metadata: {
      ...metadata,
      model_name: "text-embedding-ada-002",
      indexed_at: new Date().toISOString(),
    },
  });
}

async function semanticSearch(query: string) {
  // Generate query embedding
  const response = await openai.embeddings.create({
    model: "text-embedding-ada-002",
    input: query,
  });

  const queryEmbedding = response.data[0].embedding;

  // Search similar videos
  return await vectorDB.search({
    vector: queryEmbedding,
    k: 10,
    options: {
      include_metadata: true,
      score_threshold: 0.8,
    },
  });
}
```

#### 2. LangChain Integration

```typescript
import { VectorDBStore } from "fabstir-ai-vector-db/langchain";
import { OpenAIEmbeddings } from "langchain/embeddings/openai";

// Create vector store
const vectorStore = new VectorDBStore({
  embeddings: new OpenAIEmbeddings(),
  apiUrl: "http://localhost:7530",
});

// Add documents
await vectorStore.addDocuments([
  { pageContent: "AI fundamentals", metadata: { topic: "ai" } },
  { pageContent: "Machine learning basics", metadata: { topic: "ml" } },
]);

// Similarity search
const results = await vectorStore.similaritySearch("What is AI?", 5);
```

### Using with Different Embedding Models

#### 1. Cohere Embeddings

```typescript
import cohere from "cohere-ai";

cohere.init(process.env.COHERE_API_KEY);

async function generateCohereEmbedding(text: string) {
  const response = await cohere.embed({
    texts: [text],
    model: "embed-english-v3.0",
  });

  return response.body.embeddings[0];
}
```

#### 2. Sentence Transformers (Python)

```python
from sentence_transformers import SentenceTransformer
import requests

model = SentenceTransformer('all-MiniLM-L6-v2')

def index_video(video_text, metadata):
    # Generate embedding
    embedding = model.encode(video_text).tolist()

    # Store in vector database
    response = requests.post(
        'http://localhost:7530/vectors',
        json={
            'id': f"vec_{metadata['video_id']}",
            'vector': embedding,
            'metadata': metadata
        }
    )
    return response.json()
```

#### 3. Custom Embeddings

```typescript
// Using your own embedding model
async function customEmbedding(text: string): Promise<number[]> {
  // Your custom embedding logic
  const response = await fetch("http://your-model-api/embed", {
    method: "POST",
    body: JSON.stringify({ text }),
    headers: { "Content-Type": "application/json" },
  });

  const data = await response.json();
  return data.embedding;
}

// Use with vector database
const embedding = await customEmbedding("Video content");
await vectorDB.insertVector({
  id: "custom_vec_001",
  vector: embedding,
  metadata: {
    model_name: "custom-model-v1",
    dimension: embedding.length,
  },
});
```

## Performance & Scalability

### v0.1.1 Benchmarks (Phase 6 - Actual Results)

**Test Environment:** Node.js native bindings, all-MiniLM-L6-v2 embeddings (384-dim)

#### 100K Vectors - Production Ready ✅

| Metric                  | v0.1.0  | v0.1.1      | Improvement       |
| ----------------------- | ------- | ----------- | ----------------- |
| **Load Time**           | ~4000ms | **685ms**   | **6x faster**     |
| **Memory Usage**        | ~640 MB | **64 MB**   | **10x reduction** |
| **Search (warm)**       | ~60ms   | **58ms**    | Similar           |
| **Search (cold)**       | ~1200ms | **~1000ms** | ~17% faster       |
| **Encryption Overhead** | N/A     | **<5%**     | ChaCha20-Poly1305 |

#### Key Improvements (v0.1.1)

- ✅ **Chunked Storage**: 10K vectors/chunk with lazy loading
- ✅ **LRU Cache**: 150 MB default, configurable
- ✅ **Encryption**: Enabled by default, minimal overhead
- ✅ **Memory Efficiency**: 10x reduction via lazy loading
- ✅ **Scalability**: Tested to 1M+ vectors

### Scaling Considerations

#### 1. Memory Usage (v0.1.1 Chunked Storage)

**With Lazy Loading (Default):**

- **100K vectors**: ~64 MB (10 chunks × 6.4 MB/chunk cached)
- **500K vectors**: ~128 MB (cache limited to ~10 chunks)
- **1M+ vectors**: ~150-200 MB (LRU cache maintains constant memory)

**Formula:**

```
Memory ≈ cacheSizeMb + (active_chunks × ~6-15 MB)
```

**Tuning:**

- Memory-constrained: `cacheSizeMb: 75` → ~100 MB total
- Performance-focused: `cacheSizeMb: 300` → ~350 MB total

#### 2. Storage Requirements (384-dim vectors)

**v0.1.1 with Chunked Storage + Encryption:**

- **Raw**: 384 × 4 bytes = 1.5 KB per vector
- **Chunked + Encrypted**: ~1.6 KB per vector (<5% overhead)
- **Indexes (HNSW)**: Additional ~500 bytes per vector

**Examples:**

- 100K vectors: ~200 MB (storage)
- 1M vectors: ~2 GB (storage)
- 10M vectors: ~20 GB (storage)

#### 3. Query Performance Optimization

```typescript
// Optimize for speed (lower quality)
const fastSearch = {
  hnsw_ef: 50, // Lower ef = faster
  ivf_n_probe: 8, // Fewer probes = faster
  timeout_ms: 1000,
};

// Optimize for quality (slower)
const qualitySearch = {
  hnsw_ef: 200, // Higher ef = better quality
  ivf_n_probe: 32, // More probes = better quality
  timeout_ms: 5000,
};

// Balanced approach
const balancedSearch = {
  hnsw_ef: 100,
  ivf_n_probe: 16,
  timeout_ms: 3000,
};
```

### Best Practices

#### 1. Batch Operations

```typescript
// Good: Batch insert
await client.batchInsert({
  vectors: largeArrayOfVectors, // Insert 1000 at once
});

// Bad: Individual inserts
for (const vector of largeArrayOfVectors) {
  await client.insertVector(vector); // Slow!
}
```

#### 2. Connection Pooling

```typescript
// Reuse client instances
const client = new VectorDBClient({
  apiUrl: "http://localhost:7530",
  maxConnections: 10,
  keepAlive: true,
});

// Use the same client instance across your application
export default client;
```

#### 3. Index Tuning

```bash
# For write-heavy workloads
HNSW_EF_CONSTRUCTION=100  # Lower for faster inserts
IVF_N_CLUSTERS=128        # Fewer clusters

# For read-heavy workloads
HNSW_EF_CONSTRUCTION=400  # Higher for better search quality
IVF_N_CLUSTERS=512        # More clusters for better distribution
```

#### 4. Migration Strategy

```typescript
// Configure automatic migration
const config = {
  recent_threshold: 7 * 24 * 60 * 60 * 1000, // 7 days
  migration_batch_size: 1000,
  auto_migrate: true,
};

// Or manually trigger migration during off-peak
await client.admin.migrate();
```

## Error Handling

### Common Errors and Solutions

#### 1. Configuration Errors

```typescript
// Error: S5_PORTAL_URL required for real mode
// Solution: Set the portal URL when using real mode
export S5_MODE=real
export S5_PORTAL_URL=https://s5.vup.cx

// Error: Invalid URL format for S5_PORTAL_URL: must start with http:// or https://
// Solution: Ensure URLs have proper protocol
export S5_PORTAL_URL=https://s5.vup.cx  // ✓ Correct
export S5_PORTAL_URL=s5.vup.cx          // ✗ Wrong

// Error: Invalid seed phrase: expected 12 or 24 words, got 10
// Solution: Use a valid BIP39 seed phrase with correct word count
export S5_SEED_PHRASE="twelve words go here for proper seed phrase format example test"
```

#### 2. Dimension Mismatch

```typescript
// Error: DimensionMismatch: expected 1536, got 384
// Solution: Ensure all vectors have the same dimension
const EXPECTED_DIMENSION = 1536;

function validateVector(vector: number[]): void {
  if (vector.length !== EXPECTED_DIMENSION) {
    throw new Error(`Vector must have ${EXPECTED_DIMENSION} dimensions`);
  }
}
```

#### 3. Duplicate Vector ID

```typescript
// Error: DuplicateVector: Vector with ID vec_123 already exists
// Solution: Use unique IDs or check existence first
try {
  await client.insertVector({ id, vector, metadata });
} catch (error) {
  if (error.code === "DUPLICATE_VECTOR") {
    // Update existing vector instead
    await client.updateVector({ id, vector, metadata });
  }
}
```

#### 4. Index Not Initialized

```typescript
// Error: NotInitialized: Index not trained
// Solution: Initialize the index with training data
const trainingVectors = await loadTrainingData();
await client.admin.initializeIndex(trainingVectors);
```

#### 5. S5 Connection Issues

```typescript
// Error: NetworkError: Failed to connect to S5 portal
// Solution: Implement retry logic
async function retryOperation<T>(
  operation: () => Promise<T>,
  maxRetries = 3
): Promise<T> {
  for (let i = 0; i < maxRetries; i++) {
    try {
      return await operation();
    } catch (error) {
      if (i === maxRetries - 1) throw error;
      await new Promise((resolve) =>
        setTimeout(resolve, 1000 * Math.pow(2, i))
      );
    }
  }
  throw new Error("Max retries exceeded");
}
```

### Debugging Tips

#### 1. Enable Debug Logging

```bash
# Set environment variable
export RUST_LOG=vector_db=debug,tower_http=debug

# Or in .env file
RUST_LOG=vector_db=debug,tower_http=debug
```

#### 2. Monitor Performance

```typescript
// Use the statistics endpoint
const stats = await client.admin.getStatistics();
console.log("Vector distribution:", {
  recent: stats.recent_vectors,
  historical: stats.historical_vectors,
  memoryUsage: stats.memory_usage,
});
```

#### 3. Health Checks

```typescript
// Regular health monitoring
setInterval(async () => {
  try {
    const health = await client.health();
    if (health.status !== "healthy") {
      console.error("Database unhealthy:", health);
      // Trigger alerts
    }
  } catch (error) {
    console.error("Health check failed:", error);
  }
}, 30000); // Every 30 seconds
```

#### 4. Trace Requests

```typescript
// Add request ID for tracing
const requestId = generateRequestId();
const result = await client.search({
  vector: queryVector,
  k: 10,
  headers: {
    "X-Request-ID": requestId,
  },
});

// Check server logs for this request ID
```

## API Rate Limits and Quotas

### Default Limits

- **Request Rate**: 1000 requests/minute per IP
- **Batch Size**: Maximum 1000 vectors per batch
- **Request Size**: 10MB per request
- **Query Timeout**: 30 seconds
- **Connection Limit**: 100 concurrent connections

## Implementation Status

### Fully Implemented Endpoints

- ✅ `GET /health` - Health check
- ✅ `POST /vectors` - Insert single vector
- ✅ `POST /vectors/batch` - Batch insert vectors
- ✅ `GET /vectors/{id}` - Get vector by ID
- ✅ `DELETE /vectors/{id}` - Delete vector
- ✅ `POST /search` - Vector similarity search

### Partially Implemented Endpoints

These endpoints have placeholder implementations that return default/empty responses:

- ⚠️ `GET /admin/statistics` - Returns zeros (TODO: implement actual statistics)
- ⚠️ `POST /admin/migrate` - Returns zeros (TODO: implement migration logic)
- ⚠️ `POST /admin/rebalance` - Returns zeros (TODO: implement rebalancing)
- ⚠️ `POST /admin/backup` - Returns zeros (TODO: implement backup functionality)
- ⚠️ `GET /stream/updates` - Returns empty stream (TODO: implement SSE events)
- ⚠️ `GET /ws` - Returns status code only (TODO: implement WebSocket handler)

### Configuring Limits

```bash
# Environment variables
RATE_LIMIT_PER_MINUTE=1000
MAX_BATCH_SIZE=1000
MAX_REQUEST_SIZE=10485760
QUERY_TIMEOUT_MS=30000
MAX_CONNECTIONS=100
```

### Handling Rate Limits

```typescript
// Implement exponential backoff
async function handleRateLimit<T>(operation: () => Promise<T>): Promise<T> {
  const maxRetries = 5;
  let delay = 1000; // Start with 1 second

  for (let i = 0; i < maxRetries; i++) {
    try {
      return await operation();
    } catch (error) {
      if (error.status === 429) {
        // Rate limited
        await new Promise((resolve) => setTimeout(resolve, delay));
        delay *= 2; // Exponential backoff
        continue;
      }
      throw error;
    }
  }
  throw new Error("Rate limit exceeded after retries");
}
```

## Security Considerations

### Authentication

```typescript
// API Key authentication
const client = new VectorDBClient({
  apiUrl: "http://localhost:7530",
  apiKey: process.env.VECTOR_DB_API_KEY,
});

// Bearer token in headers
const response = await fetch("http://localhost:7530/vectors", {
  headers: {
    Authorization: `Bearer ${apiKey}`,
    "Content-Type": "application/json",
  },
});
```

### Encryption

- **Transport**: Use HTTPS in production
- **Storage**: Vectors are encrypted at rest in S5
- **Keys**: Store API keys and seed phrases securely

### Access Control

```typescript
// Example middleware for role-based access
app.use("/admin/*", requireRole("admin"));
app.use("/vectors/delete", requireRole("editor"));
app.use("/search", requireRole("reader"));
```

## Troubleshooting Guide

### Common Issues

1. **Slow Search Performance**

   - Check index statistics
   - Increase `hnsw_ef` for recent index
   - Increase `ivf_n_probe` for historical index
   - Consider adding more RAM

2. **High Memory Usage**

   - Enable compression
   - Reduce `HNSW_M` parameter
   - Implement pagination for large results
   - Monitor with `admin/statistics`

3. **Failed Migrations**

   - Check disk space
   - Verify S5 connectivity
   - Review migration logs
   - Manually trigger with smaller batches

4. **Connection Timeouts**
   - Increase timeout values
   - Check network connectivity
   - Verify S5 portal status
   - Implement retry logic

### Support Resources

- GitHub Issues: https://github.com/fabstir/fabstir-ai-vector-db/issues
- Documentation: https://docs.fabstir.ai/vector-db
- Community Discord: https://discord.gg/fabstir
- Email Support: support@fabstir.ai

---

## Version History

### v0.2.0 - CRUD Operations Release (2025-01-31)

**Major Features:**

- ✅ **Deletion Operations**: Soft deletion with `deleteVector()` and `deleteByMetadata()`
- ✅ **Update Operations**: In-place metadata updates with `updateMetadata()`
- ✅ **Metadata Filtering**: MongoDB-style query syntax for search
- ✅ **Schema Validation**: Optional metadata type enforcement
- ✅ **Vacuum API**: Manual cleanup of soft-deleted vectors
- ✅ **Manifest v3**: Persistence format with deletion tracking and schema support

**API Changes:**

- Added `deleteVector(id: string)` - Delete single vector
- Added `deleteByMetadata(filter: MetadataFilter)` - Bulk delete by metadata
- Added `updateMetadata(id: string, metadata: any)` - Update metadata without re-indexing
- Enhanced `search()` with `filter` option for metadata filtering
- Added `setSchema(schema: MetadataSchema | null)` - Optional metadata validation
- Added `vacuum()` - Physical cleanup of deleted vectors
- Enhanced `getStats()` with deletion counts

**New Types:**

- `DeleteResult` - Result from delete operations
- `MetadataFilter` - MongoDB-style filter language
- `VacuumStats` - Statistics from vacuum operation
- `MetadataSchema` - Schema definition for validation

**Performance:**

- Delete: O(1) flag update (in-memory)
- Update metadata: O(1) map update (in-memory)
- Vacuum: <1ms for 10 deletions, <100ms for 1000 deletions
- Search with filter: +10-20ms overhead (depends on selectivity)

**Testing:**

- 100+ tests passing across all phases
- Real S5 integration validated
- Backward compatible with v0.1.1

**Documentation:**

- Added [v0.2.0 Quick Start Guide](./SDK_V0.2.0_QUICK_START.md)
- Added [SDK Developer README](./SDK_DEVELOPER_README.md)
- Updated TypeScript definitions in `bindings/node/index.d.ts`
- Updated README.md and API.md with CRUD examples

### v0.1.1 - Chunked Storage Release (2025-01-28)

**Major Features:**

- ✅ **Chunked Storage**: Scalable partitioning with 10K vectors/chunk default
- ✅ **Lazy Loading**: On-demand chunk loading with LRU cache (150 MB default)
- ✅ **Encryption by Default**: ChaCha20-Poly1305 at rest (<5% overhead)
- ✅ **Node.js Native Bindings**: Production-ready napi-rs bindings (v0.1.0)
- ✅ **Memory Optimization**: 10x reduction (64 MB for 100K vectors vs 640 MB in v0.1.0)
- ✅ **Performance**: 6x faster load times (685ms vs ~4000ms)

**API Changes:**

- Added `chunkSize`, `cacheSizeMb`, `encryptAtRest` to `VectorDbConfig`
- Added `lazyLoad` option to `loadUserVectors()`
- Updated default S5 timeout to 30 seconds for real S5 operations
- Changed production port from 7530 to 7533

**Performance (Phase 6 Tested - 100K vectors):**

- Load: 685ms
- Memory: 64 MB
- Search (warm): 58ms
- Search (cold): ~1000ms
- Encryption overhead: <5%

**Documentation:**

- Added [Performance Tuning Guide](./PERFORMANCE_TUNING.md)
- Updated [Vector DB Integration Guide](./sdk-reference/VECTOR_DB_INTEGRATION.md)
- Refreshed README with v0.1.1 features

### v0.1.0 - Initial Production Release (2025-01-15)

**Features:**

- Basic HNSW/IVF hybrid indexing
- S5 storage integration
- REST API (port 7530)
- Node.js bindings (early version)
- Basic search and insert operations

**Known Limitations:**

- High memory usage (640 MB for 100K vectors)
- Slow load times (~4 seconds for 100K vectors)
- No encryption at rest
- No chunked storage

### Pre-v0.1.0 - Development Phases

**Phase 1-5: Core Infrastructure**

- HNSW index implementation
- IVF index implementation
- Hybrid routing logic
- S5 storage adapter
- Basic REST API

**Phase 6: Testing & Optimization**

- 100K vector testing
- Performance benchmarking
- Memory profiling

**Phase 7: Documentation**

- API documentation
- Integration guides
- Performance tuning guides

**Phase 8: Enhanced S5.js Integration**

- Mock and real S5 modes
- Configuration management
- Seed phrase handling
- Health monitoring

**Phase 9: Node.js Native Bindings**

- napi-rs implementation
- S5 persistence
- Native metadata support
- Production testing

---

## Migration Guide

### Upgrading from v0.1.0 to v0.1.1

**Node.js Bindings:**

```javascript
// v0.1.0 - No chunked storage
const session = await VectorDbSession.create({
  s5Portal: "http://localhost:5522",
  userSeedPhrase: "seed",
  sessionId: "id",
});

// v0.1.1 - Add chunked storage config
const session = await VectorDbSession.create({
  s5Portal: "http://localhost:5522",
  userSeedPhrase: "seed",
  sessionId: "id",
  chunkSize: 10000, // NEW: Chunked storage
  cacheSizeMb: 150, // NEW: Cache configuration
  encryptAtRest: true, // NEW: Encryption (default)
});

// Load with lazy loading
await session.loadUserVectors(cid, { lazyLoad: true }); // NEW: lazyLoad option
```

**REST API:**

- Update base URL: `http://localhost:7530/api/v1` → `http://localhost:7533/api/v1`
- S5 timeout increased: 5000ms → 30000ms (no code changes needed)

**Data Compatibility:**

- ✅ CIDs from v0.1.0 are **compatible** with v0.1.1
- v0.1.1 saves in chunked format (not backward compatible)
- Recommend re-saving data to benefit from chunked storage

---

**Last Updated:** 2025-01-31 | **API Version:** v0.2.0
