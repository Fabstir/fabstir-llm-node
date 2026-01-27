# Fabstir LLM Node - Enhanced Node Management Implementation Plan

## Overview

This implementation plan covers enhanced node identification, dynamic model management, and improved API capabilities for the Fabstir LLM Node. These features enable nodes to have identifiable names, load multiple models dynamically, and provide better discovery mechanisms for clients.

## Phase 1: Enhanced Node Identification

### Overview

Implement comprehensive node identification and discovery mechanisms that allow nodes to have both technical IDs and human-friendly names, with full API exposure of node capabilities.

### Goals

- **Unique Node Identity**: Each node has both UUID and friendly name
- **Node Discovery**: Clients can discover nodes and their capabilities
- **P2P Announcement**: Nodes advertise their identity in the DHT
- **Persistent Identity**: Node identity survives restarts
- **API Exposure**: Full node information available via REST/WebSocket

### Sub-phase 1.1: Core Node Identity Implementation

Implement the foundational node identity system with persistent storage and configuration.

#### Tasks

- [ ] Create `NodeIdentity` struct with id, name, and metadata fields
- [ ] Add `NODE_NAME` environment variable support alongside `NODE_ID`
- [ ] Implement node identity persistence to disk
- [ ] Create identity generation for new nodes (UUID v4)
- [ ] Add identity validation and uniqueness checks
- [ ] Implement identity configuration file support
- [ ] Create node metadata structure (location, capabilities, resources)
- [ ] Add identity migration from old NODE_ID system

**Test Files:**
- `tests/identity/test_node_identity.rs` (max 300 lines)
- `tests/identity/test_identity_persistence.rs` (max 250 lines)
- `tests/identity/test_identity_validation.rs` (max 200 lines)

**Implementation Files:**
- `src/node/identity.rs` (max 400 lines) - Core identity management
- `src/node/metadata.rs` (max 300 lines) - Node metadata structures
- `src/node/persistence.rs` (max 350 lines) - Identity persistence
- Update `src/main.rs` - Initialize node identity

**Configuration Format:**
```toml
[node.identity]
id = "550e8400-e29b-41d4-a716-446655440000"  # Auto-generated if not set
name = "gpu-worker-eu-01"                      # Human-friendly name
location = "eu-west-1"                         # Optional location
tags = ["gpu", "high-memory", "production"]    # Optional tags
```

**Success Criteria:**
- Node generates persistent UUID on first run
- Node name configurable via env var or config file
- Identity survives restarts
- Validation prevents duplicate names in network

### Sub-phase 1.2: Node Information API Endpoints

Create comprehensive API endpoints for node information discovery.

#### Tasks

- [ ] Implement GET `/v1/node/info` endpoint
- [ ] Add node identity to all response headers
- [ ] Create `/v1/node/capabilities` endpoint
- [ ] Implement `/v1/node/resources` for hardware info
- [ ] Add node info to WebSocket connection handshake
- [ ] Create `/v1/node/status` health endpoint with identity
- [ ] Implement node info in Prometheus metrics
- [ ] Add node discovery endpoint `/v1/peers` for P2P network info

**Test Files:**
- `tests/api/test_node_info_endpoints.rs` (max 350 lines)
- `tests/api/test_node_discovery.rs` (max 250 lines)
- `tests/api/test_node_headers.rs` (max 200 lines)

**Implementation Files:**
- `src/api/node_handlers.rs` (max 400 lines) - Node API handlers
- Update `src/api/server.rs` - Add new routes
- `src/api/node_info.rs` (max 300 lines) - Node info structures
- Update `src/api/handlers.rs` - Add node headers

**API Response Example:**
```json
{
  "node": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "gpu-worker-eu-01",
    "version": "1.0.0",
    "uptime_seconds": 3600
  },
  "capabilities": {
    "inference": true,
    "streaming": true,
    "websocket": true,
    "proof_generation": true
  },
  "resources": {
    "cpu_cores": 16,
    "memory_gb": 64,
    "gpu": {
      "available": true,
      "model": "NVIDIA RTX 4090",
      "memory_gb": 24
    }
  },
  "network": {
    "p2p_port": 9001,
    "api_port": 8080,
    "peer_count": 12
  }
}
```

**Success Criteria:**
- All endpoints return node identity
- Headers include X-Node-ID and X-Node-Name
- WebSocket includes node info in handshake
- Metrics include node labels

### Sub-phase 1.3: P2P Node Advertisement

Enhance P2P layer to advertise and discover node identities.

#### Tasks

- [ ] Add node identity to DHT records
- [ ] Implement node capability announcement in Kademlia
- [ ] Create custom libp2p protocol for node info exchange
- [ ] Add node name to mDNS advertisements
- [ ] Implement peer node info caching
- [ ] Create node registry synchronization protocol
- [ ] Add node info to peer discovery events
- [ ] Implement node info validation in P2P layer

**Test Files:**
- `tests/p2p/test_node_advertisement.rs` (max 350 lines)
- `tests/p2p/test_node_discovery_with_info.rs` (max 300 lines)
- `tests/p2p/test_node_registry_sync.rs` (max 250 lines)

**Implementation Files:**
- `src/p2p/node_advertisement.rs` (max 400 lines) - DHT advertisement
- `src/p2p/node_info_protocol.rs` (max 350 lines) - Custom protocol
- Update `src/p2p/discovery.rs` - Add node info to discovery
- `src/p2p/node_registry.rs` (max 300 lines) - Peer registry

**Success Criteria:**
- Nodes discoverable by name in DHT
- mDNS includes node names
- Peer list shows node names and capabilities
- Node info synchronized across network

## Phase 2: Dynamic Model Management

### Overview

Enable nodes to load, manage, and serve multiple LLM models dynamically without restart, with full API control over model lifecycle.

### Goals

- **Multi-Model Support**: Nodes can serve multiple models concurrently
- **Dynamic Loading**: Load/unload models via API without restart
- **Model Discovery**: Clients can discover available models
- **Resource Management**: Automatic model eviction based on memory
- **Configuration-Driven**: Models defined in configuration files

### Sub-phase 2.1: Multi-Model Loading Infrastructure

Refactor core engine to support multiple models with dynamic loading.

#### Tasks

- [ ] Refactor LlmEngine to support multiple loaded models
- [ ] Implement model registry with concurrent access
- [ ] Create model loader with async loading support
- [ ] Add model cache with LRU eviction
- [ ] Implement model memory tracking
- [ ] Create model configuration parser
- [ ] Add model preloading on startup
- [ ] Implement model validation before loading

**Test Files:**
- `tests/inference/test_multi_model.rs` (max 400 lines)
- `tests/inference/test_model_loading.rs` (max 300 lines)
- `tests/inference/test_model_eviction.rs` (max 250 lines)

**Implementation Files:**
- Update `src/inference/engine.rs` - Multi-model support
- `src/inference/model_registry.rs` (max 400 lines) - Model registry
- `src/inference/model_loader.rs` (max 350 lines) - Dynamic loader
- `src/inference/model_cache.rs` (max 300 lines) - Cache management

**Configuration Format:**
```yaml
models:
  defaults:
    gpu_layers: 35
    context_size: 2048
    
  available:
    - id: "tiny-vicuna-1b"
      path: "./models/tiny-vicuna-1b.q4_k_m.gguf"
      type: "llama"
      preload: true
      
    - id: "llama2-7b"
      path: "./models/llama2-7b.q4_0.gguf"
      type: "llama2"
      gpu_layers: 40
      preload: false
      
    - id: "codellama-7b"
      path: "./models/codellama-7b.gguf"
      type: "llama"
      tags: ["code", "completion"]
      preload: false
```

**Success Criteria:**
- Engine supports multiple concurrent models
- Models loaded based on configuration
- Memory tracking accurate
- LRU eviction works correctly

### Sub-phase 2.2: Model Management API

Create comprehensive API for model lifecycle management.

#### Tasks

- [ ] Implement POST `/v1/models/load` endpoint
- [ ] Create DELETE `/v1/models/{id}/unload` endpoint
- [ ] Add GET `/v1/models/{id}` for model details
- [ ] Implement PUT `/v1/models/{id}/config` for updates
- [ ] Create `/v1/models/available` for unloaded models
- [ ] Add model download endpoint `/v1/models/download`
- [ ] Implement model warmup endpoint `/v1/models/{id}/warmup`
- [ ] Add batch model operations support

**Test Files:**
- `tests/api/test_model_management.rs` (max 400 lines)
- `tests/api/test_model_lifecycle.rs` (max 300 lines)
- `tests/api/test_model_download.rs` (max 250 lines)

**Implementation Files:**
- `src/api/model_handlers.rs` (max 500 lines) - Model API handlers
- Update `src/api/server.rs` - Add model routes
- `src/api/model_types.rs` (max 300 lines) - Request/response types
- `src/api/model_validation.rs` (max 250 lines) - Validation logic

**API Examples:**
```bash
# Load a model
POST /v1/models/load
{
  "model_id": "llama2-7b",
  "config": {
    "gpu_layers": 40,
    "context_size": 4096
  }
}

# Get model info
GET /v1/models/llama2-7b
{
  "id": "llama2-7b",
  "status": "loaded",
  "memory_usage_mb": 4096,
  "active_sessions": 3,
  "total_tokens_generated": 150000,
  "capabilities": ["chat", "completion"],
  "performance": {
    "avg_tokens_per_second": 45.2,
    "avg_latency_ms": 120
  }
}
```

**Success Criteria:**
- All CRUD operations work for models
- Model loading non-blocking
- Proper error handling for invalid models
- Resource limits enforced

### Sub-phase 2.3: Request Routing to Models

Implement intelligent request routing to appropriate models.

#### Tasks

- [ ] Modify inference handler to route to specific models
- [ ] Implement model validation in request processing
- [ ] Add fallback model selection logic
- [ ] Create model affinity for sessions
- [ ] Implement load balancing across model instances
- [ ] Add request queuing per model
- [ ] Create model-specific rate limiting
- [ ] Implement priority-based model access

**Test Files:**
- `tests/inference/test_request_routing.rs` (max 350 lines)
- `tests/inference/test_model_fallback.rs` (max 250 lines)
- `tests/inference/test_model_affinity.rs` (max 250 lines)

**Implementation Files:**
- `src/inference/request_router.rs` (max 400 lines) - Routing logic
- Update `src/api/handlers.rs` - Use model routing
- `src/inference/model_selector.rs` (max 300 lines) - Selection logic
- `src/inference/request_queue.rs` (max 350 lines) - Per-model queues

**Success Criteria:**
- Requests routed to correct models
- Fallback works when model unavailable
- Session affinity maintained
- Load balanced across models

### Sub-phase 2.4: Model Performance Monitoring

Add comprehensive monitoring and metrics for multi-model operations.

#### Tasks

- [ ] Implement per-model performance metrics
- [ ] Add model usage statistics tracking
- [ ] Create model health checks
- [ ] Implement performance-based model scoring
- [ ] Add model cost tracking
- [ ] Create model performance dashboards
- [ ] Implement alerting for model issues
- [ ] Add A/B testing support for models

**Test Files:**
- `tests/monitoring/test_model_metrics.rs` (max 300 lines)
- `tests/monitoring/test_model_health.rs` (max 250 lines)
- `tests/monitoring/test_performance_scoring.rs` (max 250 lines)

**Implementation Files:**
- `src/monitoring/model_metrics.rs` (max 400 lines) - Metrics collection
- `src/monitoring/model_health.rs` (max 300 lines) - Health monitoring
- `src/monitoring/model_scoring.rs` (max 350 lines) - Performance scoring
- Update `src/monitoring/metrics.rs` - Add model metrics

**Metrics Example:**
```prometheus
# Model-specific metrics
llm_model_requests_total{model="llama2-7b"} 1500
llm_model_tokens_generated_total{model="llama2-7b"} 450000
llm_model_latency_seconds{model="llama2-7b",quantile="0.99"} 0.250
llm_model_memory_bytes{model="llama2-7b"} 4294967296
llm_model_load_duration_seconds{model="llama2-7b"} 12.5
```

**Success Criteria:**
- All models have metrics
- Performance tracked accurately
- Health checks work
- Dashboards show per-model stats

## Phase 3: Advanced Model Features

### Overview

Implement advanced features for model management including hot-swapping, versioning, and specialized model types.

### Sub-phase 3.1: Model Versioning and Hot-Swapping

Enable zero-downtime model updates and version management.

#### Tasks

- [ ] Implement model versioning system
- [ ] Create atomic model swapping
- [ ] Add version-specific request routing
- [ ] Implement gradual rollout support
- [ ] Create rollback mechanism
- [ ] Add version comparison tools
- [ ] Implement model migration for active sessions
- [ ] Create version deprecation warnings

**Test Files:**
- `tests/models/test_versioning.rs` (max 350 lines)
- `tests/models/test_hot_swap.rs` (max 300 lines)
- `tests/models/test_rollback.rs` (max 250 lines)

**Implementation Files:**
- `src/models/versioning.rs` (max 400 lines) - Version management
- `src/models/hot_swap.rs` (max 350 lines) - Hot-swapping logic
- `src/models/migration.rs` (max 300 lines) - Session migration
- Update `src/inference/engine.rs` - Support versioning

**Success Criteria:**
- Models can be updated without downtime
- Active sessions migrate smoothly
- Rollback works within 5 seconds
- Version routing accurate

### Sub-phase 3.2: Specialized Model Types

Support different model types with specific capabilities.

#### Tasks

- [ ] Implement embedding model support
- [ ] Add code completion model type
- [ ] Create chat-specific model handling
- [ ] Implement multi-modal model support
- [ ] Add fine-tuned model management
- [ ] Create model adapter system
- [ ] Implement model chaining support
- [ ] Add model ensemble capabilities

**Test Files:**
- `tests/models/test_embeddings.rs` (max 300 lines)
- `tests/models/test_specialized_types.rs` (max 350 lines)
- `tests/models/test_model_chaining.rs` (max 250 lines)

**Implementation Files:**
- `src/models/embeddings.rs` (max 400 lines) - Embedding support
- `src/models/specialized.rs` (max 450 lines) - Specialized types
- `src/models/adapters.rs` (max 350 lines) - Model adapters
- `src/models/ensemble.rs` (max 400 lines) - Ensemble support

**Success Criteria:**
- Different model types work correctly
- Embeddings generation functional
- Model chaining produces correct results
- Adapters allow model flexibility

### Sub-phase 3.3: Model Marketplace Integration

Connect nodes to model marketplaces for dynamic model acquisition.

#### Tasks

- [ ] Create model marketplace client
- [ ] Implement model discovery from marketplace
- [ ] Add automated model downloading
- [ ] Create model verification and signing
- [ ] Implement model licensing checks
- [ ] Add model payment integration
- [ ] Create model recommendation system
- [ ] Implement model update notifications

**Test Files:**
- `tests/marketplace/test_model_discovery.rs` (max 350 lines)
- `tests/marketplace/test_model_download.rs` (max 300 lines)
- `tests/marketplace/test_licensing.rs` (max 250 lines)

**Implementation Files:**
- `src/marketplace/client.rs` (max 500 lines) - Marketplace client
- `src/marketplace/discovery.rs` (max 350 lines) - Model discovery
- `src/marketplace/licensing.rs` (max 300 lines) - License management
- `src/marketplace/verification.rs` (max 350 lines) - Model verification

**Success Criteria:**
- Models discoverable from marketplace
- Automated download works
- Licensing enforced
- Verification prevents tampering

## Phase 4: Client SDK Enhancements

### Overview

Enhance client SDKs to leverage node identification and multi-model capabilities.

### Sub-phase 4.1: Node Discovery Client

Implement client-side node discovery and selection.

#### Tasks

- [ ] Create node discovery client library
- [ ] Implement node capability matching
- [ ] Add intelligent node selection
- [ ] Create node health monitoring client
- [ ] Implement failover mechanisms
- [ ] Add node preference system
- [ ] Create node pool management
- [ ] Implement sticky sessions with named nodes

**Test Files:**
- `tests/client/test_node_discovery_client.rs` (max 350 lines)
- `tests/client/test_node_selection.rs` (max 300 lines)
- `tests/client/test_failover.rs` (max 250 lines)

**Implementation Files:**
- `src/client/node_discovery.rs` (max 400 lines) - Discovery client
- `src/client/node_selector.rs` (max 350 lines) - Selection logic
- `src/client/node_pool.rs` (max 300 lines) - Pool management
- `src/client/failover.rs` (max 350 lines) - Failover logic

**Success Criteria:**
- Clients discover nodes by name
- Capability matching works
- Failover happens within 1 second
- Sticky sessions maintained

### Sub-phase 4.2: Model-Aware Client

Create client SDK features for multi-model interactions.

#### Tasks

- [ ] Implement model discovery in client
- [ ] Add model capability checking
- [ ] Create model-specific request builders
- [ ] Implement model performance tracking client-side
- [ ] Add model cost estimation
- [ ] Create model recommendation client
- [ ] Implement model fallback chains
- [ ] Add model comparison utilities

**Test Files:**
- `tests/client/test_model_discovery.rs` (max 300 lines)
- `tests/client/test_model_requests.rs` (max 350 lines)
- `tests/client/test_model_fallback.rs` (max 250 lines)

**Implementation Files:**
- `src/client/model_discovery.rs` (max 350 lines) - Model discovery
- `src/client/model_client.rs` (max 400 lines) - Model interactions
- `src/client/model_selector.rs` (max 300 lines) - Model selection
- `src/client/cost_estimator.rs` (max 250 lines) - Cost estimation

**Success Criteria:**
- Clients discover available models
- Model-specific features work
- Fallback chains execute correctly
- Cost estimation accurate

## Implementation Priority

1. **Phase 1.1** - Core Node Identity (Foundation for everything)
2. **Phase 1.2** - Node Information API (Immediate value)
3. **Phase 2.1** - Multi-Model Infrastructure (Core feature)
4. **Phase 2.2** - Model Management API (User-facing feature)
5. **Phase 2.3** - Request Routing (Complete multi-model)
6. **Phase 1.3** - P2P Advertisement (Network enhancement)
7. **Phase 2.4** - Model Monitoring (Operations)
8. **Phase 3.1** - Hot-Swapping (Advanced ops)
9. **Phase 4.1** - Client Discovery (SDK enhancement)
10. **Phase 4.2** - Model-Aware Client (SDK enhancement)
11. **Phase 3.2** - Specialized Models (Future expansion)
12. **Phase 3.3** - Marketplace (Future expansion)

## Environment Variables

```bash
# Node Identity
NODE_ID=550e8400-e29b-41d4-a716-446655440000  # Auto-generated if not provided
NODE_NAME=gpu-worker-eu-01                     # Human-friendly name
NODE_LOCATION=eu-west-1                        # Optional location
NODE_TAGS=gpu,high-memory,production          # Comma-separated tags

# Model Configuration
MODELS_CONFIG_PATH=./models.yaml              # Path to models config
MODELS_DIR=./models                           # Directory containing models
MODEL_CACHE_SIZE_GB=32                        # Max memory for model cache
MODEL_PRELOAD=tiny-vicuna-1b,llama2-7b       # Models to preload on startup
MODEL_DEFAULT=tiny-vicuna-1b                  # Default model if not specified

# Model Management
ENABLE_MODEL_API=true                         # Enable model management API
MODEL_DOWNLOAD_DIR=/tmp/models               # Temp directory for downloads
MODEL_MAX_CONCURRENT=3                        # Max concurrent loaded models
MODEL_EVICTION_POLICY=lru                     # Eviction policy (lru, lfu, fifo)

# Performance
MODEL_WARMUP_TOKENS=50                        # Tokens for model warmup
MODEL_LOAD_TIMEOUT_SECONDS=60                # Timeout for model loading
MODEL_INFERENCE_TIMEOUT_SECONDS=120          # Timeout for inference requests
```

## Success Metrics

1. **Node Identification**
   - 100% of nodes have persistent identity
   - Node discovery time < 100ms
   - Zero identity collisions in network

2. **Model Management**
   - Model loading time < 30 seconds
   - Model switching latency < 500ms
   - Support for 3+ concurrent models
   - Memory usage within 95% of available

3. **API Performance**
   - Model API response time < 50ms
   - Node info API response time < 10ms
   - Model routing overhead < 5ms

4. **Client Experience**
   - Node discovery from client < 1 second
   - Model discovery from client < 500ms
   - Failover time < 2 seconds

## Testing Strategy

1. **Unit Tests**: Each module thoroughly tested
2. **Integration Tests**: Multi-model scenarios
3. **Load Tests**: Multiple models under load
4. **Network Tests**: P2P with named nodes
5. **Client Tests**: SDK functionality
6. **Performance Tests**: Model loading/switching benchmarks

## Migration Path

1. **Phase 1**: Add node identity without breaking changes
2. **Phase 2**: Enable multi-model with single model default
3. **Phase 3**: Advanced features as opt-in
4. **Phase 4**: Client SDK updates with backward compatibility

## Documentation Requirements

1. **API Documentation**: OpenAPI spec for all new endpoints
2. **Configuration Guide**: Complete models.yaml documentation
3. **Migration Guide**: From single to multi-model setup
4. **Client SDK Guide**: Using node and model discovery
5. **Operations Guide**: Managing multi-model nodes