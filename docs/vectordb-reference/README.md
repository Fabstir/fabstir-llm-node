# Fabstir AI Vector Database

A high-performance, decentralized vector database built on S5 storage with hybrid HNSW/IVF indexing, chunked storage, and production-ready CRUD operations for AI applications.

## Features

- 🚀 **High Performance**: 58ms warm search latency (tested with 100K vectors)
- 📦 **Chunked Storage**: Scalable partitioning with lazy loading (10K vectors/chunk)
- 🔒 **Encrypted by Default**: ChaCha20-Poly1305 encryption at rest (<5% overhead)
- 🌐 **Decentralized Storage**: Built on S5 network via Enhanced S5.js
- 🔍 **Hybrid Indexing**: HNSW for recent data, IVF for historical data
- 💾 **Memory Efficient**: 64 MB for 100K vectors with lazy loading (10x reduction)
- 📈 **Scales to 1M+ vectors**: Tested at production scale
- 🔧 **Multiple Interfaces**: REST API (port 7533) + Node.js native bindings
- 🐳 **Docker Ready**: Production and development containers
- ✨ **CRUD Operations (v0.2.0)**: Delete, update, filter, schema validation, vacuum

## Performance (v0.2.0)

**100K Vectors (384-dim, all-MiniLM-L6-v2):**

- **Load Time**: 685ms (6x faster than v0.1.0)
- **Memory Usage**: 64 MB (10x reduction vs v0.1.0)
- **Search Latency**: 58ms warm cache, ~1000ms cold cache
- **Encryption Overhead**: <5% (ChaCha20-Poly1305)

**Key Improvements:**

- Chunked storage with lazy loading
- LRU cache for hot chunks (default: 150 MB)
- Encryption enabled by default
- Scales to 1M+ vectors

See [Performance Tuning Guide](docs/PERFORMANCE_TUNING.md) for optimization strategies.

## Quick Start

### Node.js Native Bindings (Recommended for SDK Integration)

```javascript
const { VectorDbSession } = require('@fabstir/vector-db-native');

async function example() {
  // 1. Create session with chunked storage
  const session = await VectorDbSession.create({
    s5Portal: 'http://localhost:5522',
    userSeedPhrase: 'your-seed-phrase',
    sessionId: 'user-123',
    encryptAtRest: true,    // Enabled by default
    chunkSize: 10000,       // 10K vectors/chunk
    cacheSizeMb: 150,       // Cache 10 chunks
  });

  try {
    // 2. Add vectors
    await session.addVectors([{
      id: 'doc1',
      vector: [...], // 384-dim embedding
      metadata: { text: 'Hello world' }
    }]);

    // 3. Save to S5 (encrypted, chunked)
    const cid = await session.saveToS5();
    console.log(`Saved: ${cid}`);

    // 4. Load from S5 (lazy loading)
    await session.loadUserVectors(cid, { lazyLoad: true });

    // 5. Search (warm cache: ~58ms)
    const results = await session.search(queryVector, 5);
    console.log(results[0].metadata.text);  // Native object access
  } finally {
    await session.destroy();  // CRITICAL: Clean up memory
  }
}
```

### CRUD Operations (v0.2.0 - NEW!)

```javascript
async function crudExample() {
  const session = await VectorDbSession.create({
    s5Portal: 'http://localhost:5522',
    userSeedPhrase: 'your-seed-phrase',
    sessionId: 'user-123',
  });

  try {
    // 1. Set schema (optional metadata validation)
    await session.setSchema({
      type: 'object',
      properties: {
        title: { type: 'string', required: true },
        category: { type: 'string', required: true },
        price: { type: 'number', required: false }
      }
    });

    // 2. Add vectors with validated metadata
    await session.addVectors([
      { id: 'doc-1', vector: [...], metadata: { title: 'Product A', category: 'electronics', price: 99.99 } },
      { id: 'doc-2', vector: [...], metadata: { title: 'Product B', category: 'books', price: 14.99 } }
    ]);

    // 3. Search with metadata filter
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
      category: 'electronics',
      price: 89.99
    });

    // 5. Delete by metadata filter
    const deleteResult = await session.deleteByMetadata({
      category: 'books'
    });
    console.log(`Deleted ${deleteResult.deletedCount} vectors`);

    // 6. Vacuum deleted vectors (optional, before save)
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
- `deleteVector(id)` - Delete single vector
- `deleteByMetadata(filter)` - Bulk delete by metadata criteria
- `updateMetadata(id, metadata)` - Update metadata without re-indexing
- `search(query, k, { filter })` - Search with MongoDB-style metadata filters
- `setSchema(schema)` - Optional metadata validation
- `vacuum()` - Physical cleanup of deleted vectors

See [v0.2.0 Quick Start Guide](docs/SDK_V0.2.0_QUICK_START.md) for complete CRUD documentation.

### REST API Production Deployment (After Reboot)

```bash
# 1. Start Enhanced S5.js
cd ~/dev/Fabstir/partners/S5/GitHub/s5.js/
./start-real-s5.sh &

# 2. Start Vector DB
cd ~/dev/Fabstir/fabstir-vectordb/
docker-compose -f docker-compose.dev.yml up -d vector-db-prod

# 3. Verify
curl http://localhost:5522/health         # Enhanced S5.js
curl http://localhost:7533/api/v1/health  # Vector DB
```

### Development Environment

```bash
# Start dev container with mounted workspace
docker-compose -f docker-compose.dev.yml up -d vector-db-dev

# Enter container for development
docker exec -it fabstir-vectordb-dev bash
cd /workspace  # Your code is mounted here
```

## Configuration

### Critical Settings

- **Timeout**: 30 seconds for S5 operations (src/storage/enhanced_s5_storage.rs line 51)
  - Must be `unwrap_or(30000)` not `unwrap_or(5000)`
- **Vector Dimensions**: 384 (for all-MiniLM-L6-v2)
- **Production Port**: 7533
- **Dev Ports**: 7530-7532

### Environment Variables

```bash
# S5 Configuration
S5_MODE=real                               # Use real S5 network
S5_PORTAL_URL=http://host.docker.internal:5522  # Enhanced S5.js endpoint
VECTOR_DIMENSION=384                       # Vector dimensions

# Server Configuration
VECTOR_DB_PORT=7533                        # Server port
VECTOR_DB_HOST=0.0.0.0                    # Bind to all interfaces
```

## Architecture

```
Vector DB (port 7533)
    ↓ [HTTP PUT with 30s timeout]
Enhanced S5.js (port 5522)
    ↓ [S5 protocol]
Real S5 Network (s5.vup.cx)
    ↓ [Distributed storage]
Permanent decentralized storage
```

## API Examples

### Insert Vector

```bash
curl -X POST http://localhost:7533/api/v1/vectors \
  -H "Content-Type: application/json" \
  -d '{
    "id": "test-001",
    "vector": [0.1, 0.2, 0.3],
    "metadata": {"type": "test"}
  }'
```

### Search Vectors

```bash
curl -X POST http://localhost:7533/api/v1/search \
  -H "Content-Type: application/json" \
  -d '{
    "vector": [0.1, 0.2, 0.3],
    "k": 5
  }'
```

## Docker Containers

### Production Container (Lightweight ~100MB)

- **Image**: fabstir-vectordb-prod:latest
- **Port**: 7533
- **File**: Dockerfile.production

### Development Container

- **Image**: fabstir-vectordb-fabstir-ai-vector-db
- **Workspace**: /workspace (mounted from host)
- **File**: docker-compose.dev.yml

## Troubleshooting

### Timeout Errors

```bash
# Check timeout setting
grep "unwrap_or" src/storage/enhanced_s5_storage.rs
# Should show: unwrap_or(30000)
```

### Port Conflicts

```bash
# Kill process on port
fuser -k 7533/tcp

# Check what's using port
lsof -i :7533
```

### Container Not Connecting to S5

```bash
# Test from inside container
docker exec vector-db-prod curl http://host.docker.internal:5522/health
```

## Development

### Building from Source

```bash
# Build release binary
cargo build --release --bin server

# Build Docker image
docker build -f Dockerfile.production -t fabstir-vectordb-prod:latest .
```

### Running Tests

```bash
# Unit tests
cargo test

# Integration tests with real S5
STORAGE_MODE=real cargo test --ignored
```

## Lessons Learned

1. **Timeout Critical**: Real S5 operations take 5-10 seconds, not milliseconds
2. **Container Networking**: Use `host.docker.internal` for container→host communication
3. **Port Configuration**: Production uses 7533, dev uses 7530-7532

## Related Projects

- [Enhanced S5.js](../partners/S5/GitHub/s5.js/) - Storage backend
- [Fabstir LLM Marketplace](../fabstir-llm-marketplace/) - Main application

## License

MIT

## Support

For issues or questions, please open an issue on GitHub or check the [deployment guide](docs/DEPLOYMENT.md).
