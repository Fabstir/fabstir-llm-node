# Fabstir LLM Node API Documentation

## Overview

The Fabstir LLM Node provides a RESTful HTTP API and WebSocket interface for interacting with the P2P LLM marketplace. This API enables clients to request inference from available models, monitor node health, stream real-time responses, and verify results with cryptographic proofs (EZKL).

## Base URL

```
http://localhost:8080
```

Default configuration uses `127.0.0.1:8080`. This can be modified in the API configuration.

## Authentication

The API supports optional API key authentication. When enabled, requests must include an API key in the header.

### Headers

```http
X-API-Key: your-api-key-here
```

### Configuration

API authentication is configured through `ApiConfig`:

```rust
ApiConfig {
    require_api_key: true,
    api_keys: vec!["key1", "key2"],
    // ... other settings
}
```

## Rate Limiting

Default rate limit: **60 requests per minute per IP address**

When rate limit is exceeded, the API returns:
- Status Code: `429 Too Many Requests`
- Header: `Retry-After: 60`

## Endpoints

### Health Check

Check the health status of the node.

#### Request

```http
GET /health
```

#### Response

```json
{
  "status": "healthy",
  "issues": null
}
```

Or when issues are present:

```json
{
  "status": "degraded",
  "issues": ["High memory usage", "Model cache full"]
}
```

#### Status Codes

- `200 OK` - Node is operational
- `503 Service Unavailable` - Node is experiencing issues

---

### Version Information

Get detailed version information and feature flags for the node.

#### Request

```http
GET /v1/version
```

#### Response

```json
{
  "version": "8.4.1",
  "build": "v8.4.1-s5-integration-tests-2025-11-15",
  "date": "2025-11-15",
  "features": [
    "multi-chain",
    "base-sepolia",
    "opbnb-testnet",
    "chain-aware-sessions",
    "auto-settlement",
    "websocket-compression",
    "rate-limiting",
    "job-auth",
    "dual-pricing",
    "native-stable-pricing",
    "end-to-end-encryption",
    "ecdh-key-exchange",
    "xchacha20-poly1305",
    "encrypted-sessions",
    "session-key-management",
    "ecdsa-authentication",
    "perfect-forward-secrecy",
    "replay-protection",
    "gpu-stark-proofs",
    "risc0-zkvm",
    "cuda-acceleration",
    "zero-knowledge-proofs",
    "s5-proof-storage",
    "off-chain-proofs",
    "proof-hash-cid",
    "host-side-rag",
    "session-vector-storage",
    "384d-embeddings",
    "cosine-similarity-search",
    "chat-templates",
    "model-specific-formatting",
    "s5-vector-loading",
    "encrypted-vector-database-paths"
  ],
  "chains": [84532, 5611],
  "breaking_changes": [
    "S5 Integration Testing Complete (v8.4.1)",
    "All 19 S5 vector loading tests passing (100%)",
    "Enhanced S5.js bridge integration verified"
  ]
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `version` | String | Semantic version number (e.g., "8.4.1") |
| `build` | String | Full build string with feature tag and date |
| `date` | String | Build date (YYYY-MM-DD) |
| `features` | Array<String> | List of supported features |
| `chains` | Array<Integer> | Supported blockchain network IDs |
| `breaking_changes` | Array<String> | Notable changes from previous version |

#### Status Codes

- `200 OK` - Successfully retrieved version info

---

### Node Status

Get current node status, capabilities, and configuration.

#### Request

```http
GET /status
```

#### Response

```json
{
  "status": "active",
  "node_id": "12D3KooWRj7G...",
  "peer_id": "QmYxRc...",
  "uptime_seconds": 86400,
  "active_sessions": 5,
  "total_jobs_completed": 342,
  "capabilities": {
    "inference": true,
    "encryption": true,
    "rag": true,
    "s5_vector_loading": true,
    "proof_generation": "risc0"
  },
  "models_loaded": ["tinyllama", "tiny-vicuna"],
  "chain_id": 84532,
  "version": "8.4.1"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `status` | String | Node status: "active", "busy", "maintenance" |
| `node_id` | String | Libp2p node identifier |
| `peer_id` | String | P2P peer identifier |
| `uptime_seconds` | Integer | Seconds since node started |
| `active_sessions` | Integer | Current active WebSocket sessions |
| `total_jobs_completed` | Integer | Lifetime completed jobs |
| `capabilities` | Object | Node feature support flags |
| `capabilities.inference` | Boolean | LLM inference support |
| `capabilities.encryption` | Boolean | End-to-end encryption available |
| `capabilities.rag` | Boolean | RAG vector storage support |
| `capabilities.s5_vector_loading` | Boolean | S5 vector database loading |
| `capabilities.proof_generation` | String | Proof type: "risc0", "simple", "none" |
| `models_loaded` | Array<String> | Currently loaded models |
| `chain_id` | Integer | Active blockchain network ID |
| `version` | String | Node version |

#### Status Codes

- `200 OK` - Successfully retrieved status
- `503 Service Unavailable` - Node is offline or unreachable

---

### List Supported Chains

Get all blockchain networks supported by this node.

#### Request

```http
GET /chains
```

#### Response

```json
{
  "chains": [
    {
      "chain_id": 84532,
      "chain_name": "Base Sepolia",
      "native_token": "ETH",
      "rpc_url": "https://sepolia.base.org",
      "contracts": {
        "node_registry": "0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6",
        "job_marketplace": "0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E",
        "payment_escrow": "0x...",
        "host_earnings": "0x..."
      },
      "active": true
    },
    {
      "chain_id": 5611,
      "chain_name": "opBNB Testnet",
      "native_token": "BNB",
      "rpc_url": "https://opbnb-testnet-rpc.bnbchain.org",
      "contracts": {
        "node_registry": "0x...",
        "job_marketplace": "0x...",
        "payment_escrow": "0x...",
        "host_earnings": "0x..."
      },
      "active": false
    }
  ],
  "default_chain_id": 84532
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `chains` | Array | List of supported blockchain networks |
| `chains[].chain_id` | Integer | Network chain ID |
| `chains[].chain_name` | String | Human-readable network name |
| `chains[].native_token` | String | Native token symbol (ETH, BNB, etc.) |
| `chains[].rpc_url` | String | RPC endpoint URL |
| `chains[].contracts` | Object | Deployed contract addresses |
| `chains[].active` | Boolean | Whether chain is currently active |
| `default_chain_id` | Integer | Default chain when not specified |

#### Status Codes

- `200 OK` - Successfully retrieved chains

---

### Get Chain Configuration

Get detailed configuration for a specific blockchain network.

#### Request

```http
GET /chain/{chain_id}
```

#### Path Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `chain_id` | Integer | Network chain ID (84532, 5611) |

#### Response

```json
{
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "native_token": "ETH",
  "rpc_url": "https://sepolia.base.org",
  "contracts": {
    "node_registry": "0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6",
    "job_marketplace": "0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E",
    "payment_escrow": "0x...",
    "host_earnings": "0x..."
  },
  "pricing": {
    "native_min": "2272727273",
    "native_max": "22727272727273",
    "stable_min": "10",
    "stable_max": "100000",
    "decimals_native": 18,
    "decimals_stable": 6
  },
  "active": true,
  "block_time_ms": 2000,
  "confirmation_blocks": 1
}
```

#### Status Codes

- `200 OK` - Successfully retrieved chain config
- `404 Not Found` - Chain ID not supported

---

### List Available Models

Retrieve a list of models available on this node. With the new model governance system, only approved models from the ModelRegistry are available. Models can be queried for specific blockchain networks.

#### Request

```http
GET /v1/models?chain_id={chain_id}
```

#### Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `chain_id` | Integer | No | 84532 | Blockchain network ID (84532 for Base Sepolia, 5611 for opBNB Testnet) |

#### Response

```json
{
  "models": [
    {
      "id": "0x1234...abcd",
      "name": "tiny-vicuna-1b.q4_k_m.gguf",
      "huggingface_repo": "CohereForAI/TinyVicuna-1B-32k-GGUF",
      "sha256_hash": "329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f",
      "approval_tier": 1,
      "description": "TinyVicuna 1B model with 32k context, Q4_K_M quantization"
    },
    {
      "id": "0x5678...efgh",
      "name": "tinyllama-1b.Q4_K_M.gguf",
      "huggingface_repo": "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
      "sha256_hash": "45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6",
      "approval_tier": 1,
      "description": "TinyLlama 1.1B Chat model, Q4_K_M quantization"
    }
  ],
  "chain_id": 84532,
  "chain_name": "Base Sepolia"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `models` | Array | List of available models approved by ModelRegistry |
| `models[].id` | String | Keccak256 hash of repo/filename (model ID) |
| `models[].name` | String | Model filename (GGUF format) |
| `models[].huggingface_repo` | String | HuggingFace repository path |
| `models[].sha256_hash` | String | SHA256 hash for integrity verification |
| `models[].approval_tier` | Integer | Approval level (1=trusted, 2=community) |
| `models[].description` | String? | Optional model description |
| `chain_id` | Integer | Chain ID for which models are available |
| `chain_name` | String | Human-readable chain name |

#### Status Codes

- `200 OK` - Successfully retrieved models
- `500 Internal Server Error` - Failed to retrieve models

---

### Inference Request

Submit a text generation request to a specific model.

#### Request

```http
POST /v1/inference
Content-Type: application/json
```

```json
{
  "model": "llama-2-7b",
  "prompt": "Explain quantum computing in simple terms",
  "max_tokens": 500,
  "temperature": 0.7,
  "stream": false,
  "request_id": "req-12345",
  "chain_id": 84532,
  "job_id": 123
```

#### Request Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `model` | String | Yes | - | Model ID to use for inference |
| `prompt` | String | Yes | - | Input text prompt |
| `max_tokens` | Integer | Yes | - | Maximum tokens to generate |
| `temperature` | Float | No | 0.7 | Sampling temperature (0.0-2.0) |
| `stream` | Boolean | No | false | Enable streaming response |
| `request_id` | String | No | Auto-generated | Client-provided request ID for tracking |
| `job_id` | Integer | No | - | Blockchain job ID for payment |
| `session_id` | String | No | - | Session identifier |
| `chain_id` | Integer | No | 84532 | Blockchain network ID (84532 for Base Sepolia, 5611 for opBNB Testnet) |

#### Non-Streaming Response

```json
{
  "model": "llama-2-7b",
  "content": "Quantum computing is a revolutionary approach to computation that harnesses quantum mechanical phenomena...",
  "tokens_used": 245,
  "finish_reason": "complete",
  "request_id": "req-12345",
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "native_token": "ETH"
```

#### Streaming Response (SSE)

When `stream: true`, the response is sent as Server-Sent Events:

```http
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive

data: {"content": "Quantum", "tokens": 1, "finish_reason": null, "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}

data: {"content": " computing", "tokens": 2, "finish_reason": null, "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}

data: {"content": " is", "tokens": 3, "finish_reason": null, "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}

data: {"content": "", "tokens": 245, "finish_reason": "complete", "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `model` | String | Model used for generation |
| `content` | String | Generated text content |
| `tokens_used` | Integer | Number of tokens generated (non-streaming) |
| `tokens` | Integer | Number of tokens in chunk (streaming) |
| `finish_reason` | String | Reason for completion: "complete", "max_tokens", "stop_sequence" |
| `request_id` | String | Request identifier for tracking |
| `chain_id` | Integer | Blockchain network ID used for this request |
| `chain_name` | String | Human-readable chain name |
| `native_token` | String | Native token symbol (ETH or BNB) |

#### Status Codes

- `200 OK` - Successful inference
- `400 Bad Request` - Invalid request parameters
- `404 Not Found` - Model not found
- `429 Too Many Requests` - Rate limit exceeded
- `500 Internal Server Error` - Inference failed
- `503 Service Unavailable` - Node or model unavailable

---

### Chat Templates (v8.3.13+)

**Status**: Production Ready
**Feature**: Model-specific prompt formatting with automatic template detection

The node automatically formats prompts using model-specific chat templates to ensure optimal inference quality. Templates are detected based on model metadata and applied transparently.

#### Supported Templates

| Template | Models | Format |
|----------|--------|--------|
| **Harmony** | Harmony models | `<\|im_start\|>system\n{system}<\|im_end\|>\n<\|im_start\|>user\n{user}<\|im_end\|>\n<\|im_start\|>assistant\n` |
| **Llama** | Llama 2/3 models | `[INST] {system}\n\n{user} [/INST]` |
| **ChatML** | GPT-style models | `<\|im_start\|>user\n{user}<\|im_end\|>\n<\|im_start\|>assistant\n` |
| **Vicuna** | Vicuna models | `USER: {user}\nASSISTANT:` |
| **Plain** | Fallback | No special formatting |

#### Automatic Template Detection

The node detects templates based on model name patterns:

```javascript
// Model name → Template mapping
{
  "harmony": "Harmony template",
  "llama": "Llama template",
  "vicuna": "Vicuna template",
  "chatml": "ChatML template",
  "default": "Plain template"
}
```

#### Example: Harmony Template

**Input Prompt**:
```json
{
  "model": "harmony-1b",
  "prompt": "What is machine learning?",
  "system_prompt": "You are a helpful AI assistant."
}
```

**Formatted for Model** (automatic):
```
<|im_start|>system
You are a helpful AI assistant.<|im_end|>
<|im_start|>user
What is machine learning?<|im_end|>
<|im_start|>assistant
```

#### Multi-Turn Conversations

Templates handle conversation history automatically:

```json
{
  "model": "harmony-1b",
  "conversation": [
    {"role": "system", "content": "You are a helpful AI assistant."},
    {"role": "user", "content": "What is AI?"},
    {"role": "assistant", "content": "AI is artificial intelligence..."},
    {"role": "user", "content": "Tell me more."}
  ]
}
```

**Formatted** (automatic):
```
<|im_start|>system
You are a helpful AI assistant.<|im_end|>
<|im_start|>user
What is AI?<|im_end|>
<|im_start|>assistant
AI is artificial intelligence...<|im_end|>
<|im_start|>user
Tell me more.<|im_end|>
<|im_start|>assistant
```

#### Configuration

Templates are configured per model in the node's model registry:

```rust
// Automatic detection based on model metadata
let template = detect_template(&model_name);
let formatted_prompt = template.apply(prompt, conversation_history);
```

**No client-side formatting required** - the node handles all template formatting internally based on the model being used.

---

### Metrics

Retrieve node performance and usage metrics.

#### Request

```http
GET /metrics
```

#### Response

```json
{
  "node_id": "12D3KooWExample",
  "uptime_seconds": 86400,
  "total_requests": 15234,
  "active_connections": 5,
  "models_loaded": 2,
  "gpu_utilization": 0.75,
  "memory_usage_gb": 12.5,
  "inference_queue_size": 3,
  "average_response_time_ms": 250,
  "total_tokens_generated": 5234123
}
```

#### Status Codes

- `200 OK` - Successfully retrieved metrics
- `500 Internal Server Error` - Failed to retrieve metrics

---

### List Supported Chains

Get list of all supported blockchain networks.

#### Request

```http
GET /v1/chains
```

#### Response

```json
{
  "chains": [
    {
      "chain_id": 84532,
      "name": "Base Sepolia",
      "native_token": "ETH",
      "rpc_url": "https://sepolia.base.org",
      "contracts": {
        "job_marketplace": "0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E",
        "node_registry": "0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6",
        "proof_system": "0x2ACcc60893872A499700908889B38C5420CBcFD1",
        "host_earnings": "0x908962e8c6CE72610021586f85ebDE09aAc97776",
        "model_registry": "0x92b2De840bB2171203011A6dBA928d855cA8183E",
        "usdc_token": "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
      }
    },
    {
      "chain_id": 5611,
      "name": "opBNB Testnet",
      "native_token": "BNB",
      "rpc_url": "https://opbnb-testnet.binance.org",
      "contracts": {
        "job_marketplace": "0x...",
        "node_registry": "0x...",
        "proof_system": "0x...",
        "host_earnings": "0x...",
        "model_registry": "0x...",
        "usdc_token": "0x..."
      }
    }
  ],
  "default_chain": 84532
}
```

#### Status Codes

- `200 OK` - Successfully retrieved chains

---

### Get Chain Statistics

Retrieve aggregated statistics for all chains.

#### Request

```http
GET /v1/chains/stats
```

#### Response

```json
{
  "chains": [
    {
      "chain_id": 84532,
      "chain_name": "Base Sepolia",
      "total_sessions": 150,
      "active_sessions": 5,
      "total_tokens_processed": 45000,
      "total_settlements": 120,
      "failed_settlements": 2,
      "average_settlement_time_ms": 1500,
      "last_activity": "2024-01-15T12:30:00Z"
    },
    {
      "chain_id": 5611,
      "chain_name": "opBNB Testnet",
      "total_sessions": 80,
      "active_sessions": 2,
      "total_tokens_processed": 25000,
      "total_settlements": 70,
      "failed_settlements": 1,
      "average_settlement_time_ms": 1200,
      "last_activity": "2024-01-15T12:25:00Z"
    }
  ],
  "total": {
    "total_sessions": 230,
    "active_sessions": 7,
    "total_tokens_processed": 70000
  }
}
```

#### Status Codes

- `200 OK` - Successfully retrieved statistics

---

### Get Specific Chain Statistics

Retrieve statistics for a specific chain.

#### Request

```http
GET /v1/chains/{chain_id}/stats
```

#### Path Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `chain_id` | Integer | Yes | Chain ID (84532 or 5611) |

#### Response

```json
{
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "total_sessions": 150,
  "active_sessions": 5,
  "total_tokens_processed": 45000,
  "total_settlements": 120,
  "failed_settlements": 2,
  "average_settlement_time_ms": 1500,
  "last_activity": "2024-01-15T12:30:00Z"
}
```

#### Status Codes

- `200 OK` - Successfully retrieved statistics
- `404 Not Found` - Chain not found

---

### Get Session Information

Retrieve information about a specific session.

#### Request

```http
GET /v1/session/{session_id}/info
```

#### Path Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `session_id` | Integer | Yes | Session ID |

#### Response

```json
{
  "session_id": 123,
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "native_token": "ETH",
  "status": "active",
  "tokens_used": 500
}
```

#### Status Codes

- `200 OK` - Successfully retrieved session info
- `404 Not Found` - Session not found

---

### Generate Embeddings

Generate 384-dimensional text embeddings using host-side ONNX models. This endpoint provides **zero-cost embeddings** as an alternative to expensive external APIs (OpenAI, Cohere), enabling efficient vector database operations and semantic search.

#### Request

```http
POST /v1/embed
Content-Type: application/json
```

```json
{
  "texts": [
    "Machine learning is a subset of artificial intelligence",
    "Deep learning uses neural networks with multiple layers"
  ],
  "model": "all-MiniLM-L6-v2",
  "chainId": 84532
}
```

#### Request Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `texts` | Array<String> | Yes | - | Array of texts to embed (1-96 texts) |
| `model` | String | No | "all-MiniLM-L6-v2" | Embedding model to use (or "default") |
| `chainId` | Integer | No | 84532 | Blockchain network ID (84532 for Base Sepolia, 5611 for opBNB Testnet) |

#### Request Validation

- **Text Count**: 1-96 texts per request
- **Text Length**: 1-8192 characters per text
- **No Empty/Whitespace-Only**: All texts must contain non-whitespace characters
- **Supported Chains**: 84532 (Base Sepolia) or 5611 (opBNB Testnet)

#### Response

```json
{
  "embeddings": [
    {
      "embedding": [0.123, -0.456, 0.789, ...],
      "text": "Machine learning is a subset of artificial intelligence",
      "tokenCount": 12
    },
    {
      "embedding": [0.234, -0.567, 0.891, ...],
      "text": "Deep learning uses neural networks with multiple layers",
      "tokenCount": 10
    }
  ],
  "model": "all-MiniLM-L6-v2",
  "provider": "host",
  "totalTokens": 22,
  "cost": 0.0,
  "chainId": 84532,
  "chainName": "Base Sepolia",
  "nativeToken": "ETH"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `embeddings` | Array | Array of embedding results, one per input text |
| `embeddings[].embedding` | Array<Float> | 384-dimensional embedding vector |
| `embeddings[].text` | String | Original input text |
| `embeddings[].tokenCount` | Integer | Number of tokens in this text (excluding padding) |
| `model` | String | Actual model name used for generation |
| `provider` | String | Always "host" for node-generated embeddings |
| `totalTokens` | Integer | Sum of all token counts |
| `cost` | Float | Always 0.0 (zero-cost embeddings) |
| `chainId` | Integer | Chain ID used for this request |
| `chainName` | String | Human-readable chain name |
| `nativeToken` | String | Native token symbol (ETH or BNB) |

#### Embedding Dimensions

All embeddings are **exactly 384 dimensions** to match the vector database requirements. This is validated at runtime:

- Model outputs are verified to be 384-dimensional
- Dimension mismatches return HTTP 500 with error details
- Compatible with Fabstir Vector DB (384D indexes)

#### Model Selection

Request "default" model or specify by name:

```json
{
  "texts": ["Example text"],
  "model": "default"  // Uses default model
}
```

```json
{
  "texts": ["Example text"],
  "model": "all-MiniLM-L6-v2"  // Specific model
}
```

Available models can be discovered via `GET /v1/models?type=embedding`.

#### Status Codes

- `200 OK` - Successfully generated embeddings
- `400 Bad Request` - Invalid request parameters (see error codes below)
- `404 Not Found` - Model not found or not loaded
- `500 Internal Server Error` - Embedding generation failed or dimension mismatch
- `503 Service Unavailable` - Embedding service not available (model manager not initialized)

#### Error Codes

| Code | HTTP Status | Description | Example |
|------|-------------|-------------|---------|
| `VALIDATION_ERROR` | 400 | Invalid request parameters | Empty texts array, text too long |
| `INVALID_CHAIN_ID` | 400 | Unsupported chain ID | Using chain_id other than 84532 or 5611 |
| `MODEL_NOT_FOUND` | 404 | Requested model not available | Typo in model name |
| `DIMENSION_MISMATCH` | 500 | Model outputs wrong dimensions | Model not configured for 384D |
| `SERVICE_UNAVAILABLE` | 503 | Embedding manager not loaded | Node started without embedding support |
| `INFERENCE_FAILED` | 500 | ONNX inference error | Model file corrupted |

#### Error Response Format

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Validation error: Text length exceeds maximum of 8192 characters (got 10000)",
    "field": "texts",
    "details": "Text at index 0 is too long"
  }
}
```

#### Performance Characteristics

**Single Text Embedding:**
- **CPU**: <100ms per embedding (typical: 76ms)
- **GPU** (optional): <50ms per embedding
- **Memory**: ~90MB model size (all-MiniLM-L6-v2)

**Batch Embedding:**
- **Batch 10**: <500ms total (CPU)
- **Batch 96** (max): <3s total (CPU)
- **Throughput**: ~500 embeddings/second (CPU)

**Model Specs:**
- **Model**: all-MiniLM-L6-v2 (sentence-transformers)
- **Size**: 90MB ONNX format
- **Dimensions**: 384 (fixed)
- **Max Input**: 128 tokens (truncated if longer)
- **Runtime**: ONNX Runtime (CPU optimized)

---

### List Embedding Models

Discover available embedding models on the node. This extends the existing `/v1/models` endpoint with a `type` query parameter.

#### Request

```http
GET /v1/models?type=embedding&chain_id=84532
```

#### Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `type` | String | No | "inference" | Model type: "embedding" or "inference" |
| `chain_id` | Integer | No | 84532 | Chain ID (84532 or 5611) |

#### Response

```json
{
  "models": [
    {
      "name": "all-MiniLM-L6-v2",
      "dimensions": 384,
      "available": true,
      "is_default": true
    }
  ],
  "chain_id": 84532,
  "chain_name": "Base Sepolia"
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `models` | Array | List of available embedding models |
| `models[].name` | String | Model identifier (e.g., "all-MiniLM-L6-v2") |
| `models[].dimensions` | Integer | Embedding vector dimensions (always 384) |
| `models[].available` | Boolean | Whether model is currently loaded |
| `models[].is_default` | Boolean | Whether this is the default model |
| `chain_id` | Integer | Chain ID for this response |
| `chain_name` | String | Human-readable chain name |

#### Default Behavior

Without `type` parameter, endpoint returns inference models (backward compatible):

```bash
# Returns inference models (Llama, TinyVicuna, etc.)
curl "http://localhost:8080/v1/models"

# Returns embedding models
curl "http://localhost:8080/v1/models?type=embedding"

# Explicit inference models
curl "http://localhost:8080/v1/models?type=inference"
```

#### Empty Models Array

If no embedding models are loaded, the endpoint returns an empty array (not an error):

```json
{
  "models": [],
  "chain_id": 84532,
  "chain_name": "Base Sepolia"
}
```

#### Status Codes

- `200 OK` - Successfully retrieved models (even if empty array)
- `400 Bad Request` - Invalid query parameters

---

## WebSocket API (Production Ready - Phases 8.7-8.12)

For real-time bidirectional communication and conversation management, connect via WebSocket. The WebSocket API has been completely rebuilt with production features including stateless memory caching, compression, rate limiting, JWT authentication, Ed25519 signatures, and **automatic payment settlement on disconnect (v5+)**.

### Connection Endpoints

#### Main WebSocket Endpoint (Active)
```javascript
ws://localhost:8080/v1/ws
```

This endpoint is integrated with the main HTTP server and supports streaming inference with proof generation.

### End-to-End Encryption (Recommended)

**Status**: Production Ready (January 2025)
**Security**: 111 tests passing, no vulnerabilities found

The WebSocket API supports end-to-end encryption using ECDH + XChaCha20-Poly1305 AEAD. Encryption provides:
- ✅ **Confidentiality**: Messages encrypted with 256-bit keys
- ✅ **Authenticity**: ECDSA signatures verify client identity
- ✅ **Integrity**: Poly1305 MAC detects all tampering
- ✅ **Perfect Forward Secrecy**: Ephemeral keys per session

**See**: `docs/ENCRYPTION_SECURITY.md` for comprehensive security guide
**See**: `docs/sdk-reference/NODE_ENCRYPTION_GUIDE.md` for SDK integration

#### Encrypted Session Initialization

```json
{
  "type": "encrypted_session_init",
  "session_id": "uuid-v4",
  "job_id": 12345,
  "chain_id": 84532,
  "payload": {
    "ephPubHex": "0x02...",      // Client ephemeral public key (33 bytes compressed)
    "ciphertextHex": "0x...",    // Encrypted session data
    "signatureHex": "0x...",     // ECDSA signature (65 bytes)
    "nonceHex": "0x...",         // XChaCha20 nonce (24 bytes)
    "aadHex": "0x..."            // Additional authenticated data
  }
}
```

**Encrypted Session Data** (decrypted by node):
```json
{
  "session_key": "0x...",        // 32-byte session key (hex)
  "job_id": "12345",             // Job ID as string
  "model_name": "tinyllama",     // Model to use
  "price_per_token": "0.0001",   // Pricing agreement
  "timestamp": 1737000000        // Unix timestamp
}
```

**Response**:
```json
{
  "type": "session_init_ack",
  "session_id": "uuid-v4",
  "job_id": 12345,
  "chain_id": 84532,
  "status": "success"
}
```

#### Encrypted Message (Prompt)

```json
{
  "type": "encrypted_message",
  "session_id": "uuid-v4",
  "id": "msg-123",                // Message ID for correlation
  "payload": {
    "ciphertextHex": "0x...",    // Encrypted prompt
    "nonceHex": "0x...",         // Unique 24-byte nonce
    "aadHex": "0x..."            // AAD with message index (e.g., "message_0")
  }
}
```

#### Encrypted Response Streaming

**Streaming Chunks**:
```json
{
  "type": "encrypted_chunk",
  "session_id": "uuid-v4",
  "id": "msg-123",
  "tokens": 5,                    // Token count
  "payload": {
    "ciphertextHex": "0x...",    // Encrypted response chunk
    "nonceHex": "0x...",         // Unique nonce per chunk
    "aadHex": "0x...",           // AAD with chunk index (e.g., "chunk_0")
    "index": 0                    // Chunk sequence number
  }
}
```

**Final Message**:
```json
{
  "type": "encrypted_response",
  "session_id": "uuid-v4",
  "id": "msg-123",
  "payload": {
    "ciphertextHex": "0x...",    // Encrypted finish_reason
    "nonceHex": "0x...",         // Unique nonce
    "aadHex": "0x..."            // AAD for final message
  }
}
```

#### Encryption Error Codes

| Code | Description | Action |
|------|-------------|--------|
| `ENCRYPTION_NOT_SUPPORTED` | Node lacks HOST_PRIVATE_KEY | Configure key or use plaintext |
| `DECRYPTION_FAILED` | Invalid ciphertext/nonce/AAD | Check encryption parameters |
| `INVALID_SIGNATURE` | Signature verification failed | Verify client private key |
| `SESSION_KEY_NOT_FOUND` | Session not initialized | Send encrypted_session_init first |
| `INVALID_NONCE_SIZE` | Nonce not 24 bytes | Use XChaCha20 nonce size |
| `INVALID_HEX_ENCODING` | Malformed hex string | Verify hex encoding |
| `MISSING_PAYLOAD` | Payload object missing | Include payload in message |

#### Cryptographic Primitives

- **Key Exchange**: ECDH on secp256k1 (Ethereum curve)
- **Symmetric Encryption**: XChaCha20-Poly1305 AEAD
- **Key Derivation**: HKDF-SHA256
- **Signatures**: ECDSA secp256k1 with address recovery
- **Nonce Generation**: CSPRNG (24 bytes)

#### Security Properties

- ✅ **256-bit security**: XChaCha20 with 32-byte keys
- ✅ **Perfect Forward Secrecy**: Ephemeral keys per session
- ✅ **Replay Protection**: AAD with message/chunk indices
- ✅ **Tamper Detection**: Poly1305 authentication tags
- ✅ **Non-Repudiation**: ECDSA signatures
- ✅ **Session Isolation**: Unique keys per session

#### Client Example (TypeScript)

See `docs/sdk-reference/NODE_ENCRYPTION_GUIDE.md` for complete implementation.

```typescript
import { secp256k1 } from '@noble/curves/secp256k1';
import { xchacha20poly1305 } from '@noble/ciphers/chacha';
import { hkdf } from '@noble/hashes/hkdf';
import { sha256 } from '@noble/hashes/sha256';

// 1. Generate ephemeral keypair
const clientEphemeral = secp256k1.utils.randomPrivateKey();
const clientEphemeralPub = secp256k1.getPublicKey(clientEphemeral, true);

// 2. ECDH with node public key
const sharedSecret = secp256k1.getSharedSecret(clientEphemeral, nodePublicKey);

// 3. Derive session key
const sessionKey = hkdf(sha256, sharedSecret, undefined, undefined, 32);

// 4. Encrypt session data
const nonce = crypto.getRandomValues(new Uint8Array(24));
const cipher = xchacha20poly1305(sessionKey, nonce);
const ciphertext = cipher.encrypt(
  new TextEncoder().encode(JSON.stringify(sessionData)),
  aad
);

// 5. Sign and send
const signature = await wallet.signMessage(sha256(ciphertext));
ws.send(JSON.stringify({
  type: 'encrypted_session_init',
  session_id: sessionId,
  payload: {
    ephPubHex: bytesToHex(clientEphemeralPub),
    ciphertextHex: bytesToHex(ciphertext),
    signatureHex: signature,
    nonceHex: bytesToHex(nonce),
    aadHex: bytesToHex(aad)
  }
}));
```

#### Backward Compatibility

- **Plaintext Support**: Node accepts both encrypted and plaintext messages
- **Automatic Detection**: Message type determines protocol
- **Deprecation Warnings**: Plaintext messages log warnings
- **Default Mode**: SDK Phase 6.2+ uses encryption by default

### RAG (Retrieval-Augmented Generation)

**Status**: Production Ready (v8.3.0+, Enhanced in v8.4.0+)
**Feature**: Host-Side RAG with session-scoped vector storage and S5 vector loading

The WebSocket API supports RAG functionality for document-based context retrieval. Vectors are stored in session memory and automatically cleared on disconnect.

**See**: `docs/RAG_SDK_INTEGRATION.md` for comprehensive integration guide

#### Upload Vectors

Upload document embeddings to session storage for semantic search.

**Request**:
```json
{
  "type": "uploadVectors",
  "sessionId": "session-123",
  "requestId": "upload-456",
  "vectors": [
    {
      "id": "chunk-0",
      "vector": [0.12, 0.45, ..., 0.89],
      "metadata": {
        "text": "Machine learning is a subset of AI.",
        "page": 1,
        "source": "ml_guide.pdf"
      }
    }
  ],
  "replace": false
}
```

**Response**:
```json
{
  "type": "uploadVectorsResponse",
  "requestId": "upload-456",
  "uploaded": 1,
  "rejected": 0,
  "errors": []
}
```

#### Search Vectors

Search uploaded vectors using semantic similarity.

**Request**:
```json
{
  "type": "searchVectors",
  "sessionId": "session-123",
  "requestId": "search-789",
  "queryVector": [0.23, 0.56, ..., 0.78],
  "k": 5,
  "threshold": 0.7
}
```

**Response**:
```json
{
  "type": "searchVectorsResponse",
  "requestId": "search-789",
  "results": [
    {
      "id": "chunk-0",
      "score": 0.95,
      "metadata": {
        "text": "Machine learning is a subset of AI.",
        "page": 1
      }
    }
  ],
  "totalVectors": 10,
  "searchTimeMs": 2.3
}
```

**Requirements**:
- Embeddings must be 384-dimensional (from `POST /v1/embed`)
- Maximum 100,000 vectors per session
- Vectors cleared automatically on disconnect
- Session must be created before upload

### S5 Vector Database Loading (v8.4.0+)

**Status**: Production Ready (v8.4.1+)
**Feature**: Load pre-built vector databases from S5 decentralized storage

The WebSocket API supports loading complete vector databases from S5 storage, enabling efficient distribution of large document collections without manual upload. Supports both plaintext and encrypted CID paths.

**See**: `docs/sdk-reference/S5_VECTOR_LOADING.md` for SDK integration guide

#### Load Vector Database

Load a complete vector database from S5 storage using a CID (Content Identifier).

**Request**:
```json
{
  "type": "LoadVectorDatabase",
  "sessionId": "session-123",
  "requestId": "load-001",
  "cid": "z4QmX7Kd9...",
  "metadata": {
    "source": "legal_docs_corpus",
    "version": "v1.2.0",
    "documentCount": 5000
  }
}
```

**Request with Encrypted Path** (v8.4.0+):
```json
{
  "type": "LoadVectorDatabase",
  "sessionId": "session-123",
  "requestId": "load-001",
  "vector_database": "enc:0x4a7b2c...",  // Encrypted CID
  "metadata": {
    "source": "private_corpus",
    "encrypted": true
  }
}
```

**Success Response**:
```json
{
  "type": "VectorDatabaseLoaded",
  "requestId": "load-001",
  "sessionId": "session-123",
  "vectorsLoaded": 5000,
  "loadTimeMs": 234,
  "metadata": {
    "source": "legal_docs_corpus",
    "version": "v1.2.0"
  }
}
```

**Progress Updates** (during load):
```json
{
  "type": "VectorLoadProgress",
  "requestId": "load-001",
  "sessionId": "session-123",
  "stage": "downloading",  // downloading | parsing | indexing
  "progress": 45,          // Percentage (0-100)
  "message": "Downloading vectors from S5..."
}
```

**Error Response**:
```json
{
  "type": "VectorDatabaseError",
  "requestId": "load-001",
  "sessionId": "session-123",
  "error": "S5_FETCH_FAILED",
  "message": "Failed to fetch CID z4QmX7Kd9... from S5 network",
  "details": {
    "cid": "z4QmX7Kd9...",
    "retries": 3
  }
}
```

#### Error Codes

| Code | Description | Recovery Action |
|------|-------------|-----------------|
| `S5_FETCH_FAILED` | Failed to download from S5 | Verify CID exists and S5 network is accessible |
| `INVALID_CID_FORMAT` | Malformed CID | Use valid base58btc CID format (z...) |
| `PARSE_ERROR` | Invalid vector data format | Verify vectors match expected schema |
| `DIMENSION_MISMATCH` | Vectors not 384D | Regenerate with correct embedding model |
| `SESSION_NOT_FOUND` | Invalid session ID | Create session before loading |
| `DECRYPTION_FAILED` | Failed to decrypt vector_database | Verify session has correct decryption key |
| `VECTOR_LIMIT_EXCEEDED` | Too many vectors (>100K) | Split into smaller databases |

#### Vector Database Format

S5 vector databases must be JSON arrays with this schema:

```json
[
  {
    "id": "chunk-0",
    "vector": [0.12, 0.45, ..., 0.89],  // 384 dimensions
    "metadata": {
      "text": "Document content...",
      "page": 1,
      "source": "document.pdf"
    }
  }
]
```

**Requirements**:
- Each vector must be exactly 384 dimensions
- `id` field must be unique within the database
- `metadata` is optional but recommended
- Maximum 100,000 vectors per database
- S5 CID must be in base58btc format (starts with 'z')

**Integration with Enhanced S5.js**:
- Uses HTTP bridge API at `ENHANCED_S5_URL` (default: http://localhost:5522)
- Falls back to native S5 network if bridge unavailable
- Automatic retry with exponential backoff
- Supports encrypted `vector_database` paths from job parameters

### Simple Inference (No Auth Required)

For basic inference without job management:

```json
{
  "type": "inference",
  "request": {
    "model": "tinyllama",
    "prompt": "What is machine learning?",
    "max_tokens": 100,
    "stream": true,
    "chain_id": 84532
  }
}
```

Response format:
```json
{
  "type": "stream_chunk",
  "content": "Machine learning is",
  "tokens": 3,
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "native_token": "ETH",
  "proof": {
    "proof_type": "EZKL",
    "proof_data": "0xEF...",
    "model_hash": "sha256:...",
    "timestamp": 1737000000
  }
}
```

### Authentication (For Job-Based Sessions)

The WebSocket server supports authentication via job ID and JWT tokens:

#### Initial Authentication
```json
{
  "type": "auth",
  "job_id": 12345,
  "chain_id": 84532,
  "token": "jwt_token_here"  // Optional JWT token
}
```

#### JWT Token Structure
```typescript
interface JwtClaims {
  session_id: string;
  job_id: number;
  permissions: string[];  // ['Read', 'Write', 'Execute', 'Admin']
  exp: number;           // Expiry timestamp
  iat: number;           // Issued at timestamp
}
```

### Message Format (Enhanced Protocol with Proof Support)

#### ConversationMessage Structure (Updated)
```typescript
interface ConversationMessage {
  role: "user" | "assistant" | "system";
  content: string;
  timestamp?: number;      // Optional Unix timestamp
  tokens?: number;         // Token count for this message
  proof?: ProofData;       // Cryptographic proof (NEW - Sub-phase 8.14)
}

interface ProofData {
  hash: string;            // SHA256 hash of proof
  proof_type: string;      // "ezkl", "risc0", "simple"
  model_hash: string;      // Hash of model used
  input_hash: string;      // Hash of input prompt
  output_hash: string;     // Hash of generated output
  timestamp: number;       // Millisecond timestamp
}
```

#### Session Initialization
```json
{
  "type": "session_init",
  "session_id": "uuid-v4",
  "job_id": 12345,
  "chain_id": 84532,
  "model_config": {
    "model": "llama-2-7b",
    "max_tokens": 2048,
    "temperature": 0.7
  },
  "conversation_context": []  // Array of ConversationMessage with optional proofs
}
```

#### Session Resume (After Disconnect)
```json
{
  "type": "session_resume",
  "session_id": "existing-uuid",
  "job_id": 12345,
  "conversation_context": [
    {"role": "user", "content": "Previous question"},
    {"role": "assistant", "content": "Previous response"}
  ],
  "last_message_index": 8
}
```

#### Prompt (During Active Session)
```json
{
  "type": "prompt",
  "session_id": "active-uuid",
  "content": "What is machine learning?",
  "message_index": 5,
  "stream": true
}
```

#### Response (Non-Streaming with Proof)
```json
{
  "type": "response",
  "session_id": "active-uuid",
  "content": "Machine learning is...",
  "tokens_used": 45,
  "message_index": 6,
  "completion_time_ms": 1234,
  "proof": {
    "hash": "2ea94da64d1556f5047376f7fb680c2dba5417831b6badab0b7f6e688b9d7318",
    "proof_type": "simple",
    "model_hash": "abc123...",
    "input_hash": "def456...",
    "output_hash": "ghi789...",
    "timestamp": 1757356139000
  }
}
```

#### Streaming Response with Final Proof
```json
// Intermediate chunks
{
  "type": "stream_chunk",
  "session_id": "active-uuid",
  "content": "Machine",
  "chunk_index": 0,
  "is_final": false,
  "proof": null
}

// Final chunk with proof
{
  "type": "stream_chunk",
  "session_id": "active-uuid",
  "content": "",
  "chunk_index": 42,
  "is_final": true,
  "tokens_used": 150,
  "proof": {
    "hash": "3fa85f64d1556f5047376f7fb680c2dba5417831b6badab0b7f6e688b9d7319",
    "proof_type": "simple",
    "model_hash": "abc123...",
    "input_hash": "def456...",
    "output_hash": "xyz890...",
    "timestamp": 1757356140500
  }
}

### Production Features

#### EZKL Proof Generation (Enhanced - Sub-phases 8.14-8.15)
- **Cryptographic Proofs**: Verify inference without re-running
- **Payment Security**: Funds released only after verification
- **Multiple Proof Types**: EZKL, Risc0, Simple
- **Hash Verification**: Model, input, and output integrity
- **Concurrent Generation**: Efficient batch processing
- **ProofManager with LRU Cache**: Intelligent caching with eviction
- **ProofConfig**: Environment-based configuration
- **Streaming Integration**: Proofs in final stream tokens

#### ProofManager Configuration
The ProofManager handles proof generation with intelligent caching:

```typescript
// Environment Variables for Proof Configuration
ENABLE_PROOF_GENERATION=true     // Enable/disable proof generation
PROOF_TYPE=Simple                // Proof type: EZKL, Risc0, Simple
PROOF_MODEL_PATH=./models/model.gguf  // Model path for proof
PROOF_CACHE_SIZE=100             // Maximum cached proofs (LRU eviction)
PROOF_BATCH_SIZE=10              // Batch size for concurrent generation
```

**LRU Cache Features:**
- Maintains insertion order with VecDeque
- Automatic eviction of oldest entries when cache is full
- O(1) lookup performance with HashMap
- Millisecond timestamp precision for cache freshness

#### Message Compression
The WebSocket server supports per-message deflate compression:
- **Gzip**: For maximum compression
- **Deflate**: For lower latency
- **Threshold**: Messages > 1KB are automatically compressed
- **Bandwidth Savings**: >40% reduction on average

#### Rate Limiting
- **Default**: 100 requests per minute per session
- **Burst Capacity**: 200 tokens
- **Token Bucket Algorithm**: Smooth traffic shaping
- **Per-Session**: Each session_id has independent limits

#### Stateless Memory Cache
The server maintains conversation context in memory during active sessions:
- **No Persistence**: All data cleared on disconnect
- **Automatic Truncation**: Based on model context window
- **Token Limits**: Enforced per model configuration
- **Session Timeout**: 30 minutes of inactivity
- **Memory Cap**: 10MB per session

#### Health Monitoring
- **Circuit Breakers**: Automatic failure detection
- **Health Checks**: `/health` and `/ready` endpoints
- **Metrics**: Prometheus-compatible (structure ready)
- **Connection Pooling**: Efficient resource management

#### Security Features
- **JWT Authentication**: HS256 algorithm
- **Ed25519 Signatures**: Optional message signing
- **Session Tokens**: Time-limited with refresh
- **Permission System**: Role-based access control
- **EZKL Proof Generation**: Cryptographic verification of inference (NEW)

#### Connection Maintenance
- Ping interval: 30 seconds
- Pong timeout: 10 seconds
- Automatic reconnection recommended on disconnect
- Session recovery with full context rebuild

### Message Types (Extended)

| Type | Direction | Description |
|------|-----------|-------------|
| `auth` | Client → Server | Authentication with job_id/token |
| `session_init` | Client → Server | Start new conversation session |
| `session_resume` | Client → Server | Resume session with full context |
| `prompt` | Client → Server | Send user prompt (minimal data) |
| `response` | Server → Client | Non-streaming complete response |
| `stream_chunk` | Server → Client | Streaming response token |
| `session_end` | Client → Server | Clean session termination (optional - disconnect auto-settles v5+) |
| `error` | Server → Client | Error message with code |
| `token_refresh` | Server → Client | New JWT token |
| `rate_limit` | Server → Client | Rate limit warning |
| `ping` | Bidirectional | Keep-alive |
| `pong` | Bidirectional | Keep-alive response |

### Error Codes (WebSocket)

| Code | Description | Recovery Action |
|------|-------------|----------------|
| `AUTH_FAILED` | Authentication/authorization failure | Refresh token or re-authenticate |
| `RATE_LIMIT` | Rate limit exceeded | Exponential backoff |
| `SESSION_EXPIRED` | Session token expired | Create new session |
| `INVALID_JOB` | Job verification failed | Check blockchain job status |
| `MODEL_UNAVAILABLE` | Requested model not loaded | Try alternative model |
| `CONTEXT_TOO_LARGE` | Conversation exceeds limits | Truncate context |
| `CIRCUIT_OPEN` | Service temporarily unavailable | Wait and retry |

---

## Zero-Knowledge Proof Generation (v8.1.0+)

**Status**: Production Ready (Risc0 zkVM v3.0 with CUDA acceleration)

The node generates cryptographic proofs for all inference results to ensure payment security and enable trustless verification. Proofs are generated using **GPU-accelerated STARK proofs** and stored off-chain in the S5 decentralized network.

### Proof System Architecture (v8.1.2+)

**Production Configuration**:
- **Proof Engine**: Risc0 zkVM v3.0 (enabled with `--features real-ezkl` build flag)
- **Acceleration**: CUDA GPU acceleration (0.2-2.3s per proof)
- **Proof Size**: ~221KB per proof
- **Storage**: Off-chain in S5 network (737x size reduction on-chain)
- **On-Chain Submission**: Only hash + CID (hash: 32 bytes, CID: ~50 bytes)
- **Contract Function**: `submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID)`
- **Contract Address**: `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E` (Base Sepolia)

**🚨 CRITICAL**: Nodes built without `--features real-ezkl` generate **mock proofs** (126 bytes) which are NOT valid for production use!

### Proof Types

The system supports multiple proof types:

| Type | Status | Size | Use Case |
|------|--------|------|----------|
| **Risc0** | ✅ Production (Default) | ~221KB | GPU-accelerated STARK proofs with S5 storage |
| Simple | ⚠️ Development Only | ~126 bytes | Testing and development (mock proofs) |
| EZKL | 🚧 Deprecated | N/A | Legacy support only |

**Production Deployment**:
```bash
# ✅ CORRECT - Real Risc0 STARK proofs
cargo build --release --features real-ezkl -j 4

# ❌ WRONG - Mock proofs (not production-ready!)
cargo build --release
```

### Proof-Enhanced Response

When proof generation is enabled, responses include cryptographic verification:

```json
{
  "type": "response",
  "session_id": "active-uuid",
  "content": "Machine learning is...",
  "tokens_used": 45,
  "message_index": 6,
  "completion_time_ms": 1234,
  "proof": {
    "job_id": 123,
    "model_hash": "sha256:abc123...",
    "input_hash": "sha256:def456...",
    "output_hash": "sha256:ghi789...",
    "proof_type": "risc0",
    "proof_hash": "0x7f3a...",     // SHA256 hash of proof
    "proof_cid": "z5bmX...",       // S5 CID where proof is stored
    "proof_size": 221184,          // Proof size in bytes (~221KB)
    "timestamp": 1737000000,       // Unix timestamp
    "tokens_claimed": 45
  }
}
```

**Note**: The full proof (~221KB) is stored in S5 decentralized storage. Only the `proof_hash` and `proof_cid` are submitted on-chain, achieving 737x size reduction.

### Automatic Checkpoint Submission (v8.1.0+)

**Trigger**: Every 50 tokens generated during inference

The node automatically generates and submits checkpoints to verify work progress:

1. **Token Tracking**: Node counts tokens in both streaming and non-streaming inference
2. **Checkpoint Trigger**: Every 50 tokens, checkpoint submission is triggered
3. **Proof Generation**: Risc0 zkVM generates ~221KB STARK proof (0.2-2.3s with CUDA)
4. **S5 Upload**: Proof uploaded to S5 decentralized network, receives CID
5. **Hash Calculation**: SHA256 hash computed for proof integrity
6. **On-Chain Submission**: `submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID)`
7. **Gas Optimization**: Only ~82 bytes submitted on-chain (hash: 32 bytes, CID: ~50 bytes)

**Log Messages to Watch**:
```
📊 Tracking: job_id=123, tokens=25, total=25
🚨 TRIGGERING checkpoint: job_id=123, tokens_claimed=50
🔐 Generating real Risc0 STARK proof for job 123
✅ Checkpoint submitted: job_id=123, hash=0x7f3a..., cid=z5bmX...
```

**Contract Integration**:
```solidity
// JobMarketplace contract function (v8.1.2+)
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,      // SHA256 hash of proof
    string memory proofCID  // S5 CID for off-chain retrieval
) external;
```

**S5 Proof Retrieval**:
```javascript
// Retrieve full proof from S5 for verification
const proofCID = "z5bmX7Kd9...";
const s5Url = `https://s5.vup.cx/s5/blob/${proofCID}`;
const proofBytes = await fetch(s5Url).then(r => r.arrayBuffer());

// Verify hash matches on-chain submission
const calculatedHash = sha256(proofBytes);
assert(calculatedHash === onChainProofHash);
```

### Proof Verification

Clients can verify proofs by retrieving them from S5 and checking the hash:

```javascript
// Example verification (client-side)
async function verifyProof(jobId, proofHash, proofCID) {
  // 1. Retrieve proof from S5
  const s5Url = `https://s5.vup.cx/s5/blob/${proofCID}`;
  const proofBytes = await fetch(s5Url).then(r => r.arrayBuffer());

  // 2. Verify hash matches on-chain submission
  const calculatedHash = sha256(proofBytes);
  if (calculatedHash !== proofHash) {
    throw new Error('Proof hash mismatch - tampered data');
  }

  // 3. Verify proof with Risc0 zkVM verifier
  const isValid = await risc0Verifier.verify(proofBytes);
  if (!isValid) {
    throw new Error('Invalid Risc0 STARK proof');
  }

  return true;
}

// Monitor checkpoint submissions
const filter = jobMarketplace.filters.ProofOfWorkSubmitted(jobId);
const events = await jobMarketplace.queryFilter(filter);
console.log(`Checkpoints submitted: ${events.length}`);
```

### Payment Security Flow

1. **Job Request**: Client submits job with payment escrow
2. **Inference**: Node processes request
3. **Proof Generation**: Node creates cryptographic proof
4. **Result Delivery**: Node sends result with proof
5. **Verification**: Client/Contract verifies proof
6. **Payment Release**: Funds released only after verification

### Automatic Payment Settlement (v5+, September 2024)

**WebSocket Disconnect Triggers Settlement**

Starting with v5, payment settlement is automatic when WebSocket disconnects:

1. **Connection Closes**: Any disconnect reason (user action, network, timeout)
2. **Node Action**: Automatically calls `completeSessionJob()`
3. **Blockchain Transaction**: Submits settlement to JobMarketplace contract
4. **Payment Distribution**:
   - Host: 90% sent to HostEarnings contract (0x908962e8c6CE72610021586f85ebDE09aAc97776)
   - Treasury: 10% fee (0xbeaBB2a5AEd358aA0bd442dFFd793411519Bdc11)
   - User: Unused deposit refunded

**No User Action Required**: Sessions settle automatically, ensuring hosts always get paid for completed work.

**Monitoring Settlement**:
```javascript
// SDK developers should monitor blockchain events
// Transaction will emit SessionCompleted event
const filter = jobMarketplace.filters.SessionCompleted(jobId);
jobMarketplace.on(filter, (jobId, host, tokensUsed, event) => {
  console.log(`Session ${jobId} settled: ${tokensUsed} tokens`);
});
```

**Requirements**:
- Node must have `HOST_PRIVATE_KEY` configured
- Node version v5-payment-settlement or later
- JobMarketplace: 0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E (v8.1.2+)

### Proof Configuration

Configure proof generation in node settings:

```toml
[proof]
enabled = true
type = "EZKL"
max_proof_size = 10000
model_path = "./models/tinyllama-1.1b.gguf"
settings_path = "./ezkl/settings.json"
```

### Benefits

- **Payment Security**: Funds only released for verified work
- **Interruption Recovery**: Prove partial completion
- **Dispute Prevention**: Cryptographic evidence
- **Trust Minimization**: No blind trust required
- **Audit Trail**: Verifiable computation history

---

## Error Handling

### Error Response Format

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Invalid temperature value",
    "field": "temperature",
    "details": "Temperature must be between 0.0 and 2.0"
  }
}
```

### Common Error Codes

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `VALIDATION_ERROR` | 400 | Invalid request parameters |
| `MODEL_NOT_FOUND` | 404 | Requested model not available |
| `RATE_LIMIT_EXCEEDED` | 429 | Too many requests |
| `INSUFFICIENT_RESOURCES` | 503 | Node lacks resources |
| `INFERENCE_FAILED` | 500 | Model inference error |
| `CONNECTION_ERROR` | 503 | P2P network issue |
| `TIMEOUT` | 504 | Request timeout |
| `UNAUTHORIZED` | 401 | Invalid or missing API key |

---

## Configuration

### API Server Configuration

The API server can be configured with the following options:

```rust
ApiConfig {
    // Network
    listen_addr: "127.0.0.1:8080",
    max_connections: 1000,
    max_connections_per_ip: 10,
    
    // Timeouts
    request_timeout: Duration::from_secs(30),
    connection_idle_timeout: Duration::from_secs(60),
    shutdown_timeout: Duration::from_secs(30),
    
    // Security
    require_api_key: false,
    api_keys: vec![],
    cors_allowed_origins: vec!["*"],
    
    // Rate Limiting
    rate_limit_per_minute: 60,
    
    // Features
    enable_websocket: true,
    enable_http2: false,
    enable_auto_retry: false,
    max_retries: 3,
    
    // Circuit Breaker
    enable_circuit_breaker: false,
    circuit_breaker_threshold: 5,
    circuit_breaker_timeout: Duration::from_secs(30),
    
    // WebSocket
    websocket_ping_interval: Duration::from_secs(30),
    websocket_pong_timeout: Duration::from_secs(10),
    
    // Performance
    max_concurrent_streams: 100,
    connection_retry_count: 3,
    connection_retry_backoff: Duration::from_millis(100),
    
    // Health Checks
    enable_connection_health_checks: false,
    health_check_interval: Duration::from_secs(10),
    
    // Debugging
    enable_error_details: false,
}
```

### Smart Contract Addresses

**Single Source of Truth**: `.env.contracts` file

All contract addresses are defined in the `.env.contracts` file at the repository root. This file serves as the single source of truth for all deployments.

#### Base Sepolia (Chain ID: 84532)

**Current Deployment** (v8.1.2+):

| Contract | Address | Version | Features |
|----------|---------|---------|----------|
| **NodeRegistry** | `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` | v7.0.29+ | Dual pricing support (native + stable) |
| **JobMarketplace** | `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E` | v8.1.2+ | S5 off-chain proof storage (hash + CID) |
| **PaymentEscrow** | `PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS` | Current | Payment escrow with earnings tracking |
| **HostEarnings** | `HOST_EARNINGS_ADDRESS` | Current | Host earnings accumulator (90% share) |
| **ProofSystem** | `PROOF_SYSTEM_ADDRESS` | Current | Proof verification system |
| **ReputationSystem** | `REPUTATION_SYSTEM_ADDRESS` | Current | Node reputation tracking |

**Deprecated Addresses** (Do not use):
- Old NodeRegistry: `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7` (pre-v7.0.29, no dual pricing)
- Old JobMarketplace (full proof): `0x462050a4a551c4292586D9c1DE23e3158a9bF3B3` (pre-v8.1.2)
- Old JobMarketplace (pre-S5): `0xe169A4B57700080725f9553E3Cc69885fea13629` (pre-v8.1.2)

#### opBNB Testnet (Chain ID: 5611)

**Status**: Supported but currently inactive

Contract addresses for opBNB Testnet are defined in `.env.contracts` but the chain is not actively used in production deployments.

#### Contract Features by Version

**NodeRegistry (v7.0.29+)**:
- Dual pricing: `minPricePerTokenNative` and `minPricePerTokenStable`
- Pricing validation against contract constants
- Native: MIN=2,272,727,273 wei, MAX=22,727,272,727,273 wei
- Stable: MIN=10, MAX=100,000 (6 decimals)

**JobMarketplace (v8.1.2+)**:
- Off-chain proof storage in S5 network
- `submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID)` function
- 737x size reduction on-chain (221KB → ~82 bytes)
- Proof retrieval from S5 using CID

#### Environment Configuration

```bash
# From .env.contracts
NODE_REGISTRY_FAB_ADDRESS=0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6
JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS=0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E
PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS=<from .env.contracts>
HOST_EARNINGS_ADDRESS=<from .env.contracts>

# Test accounts (from .env.test.local)
TEST_USER_1_ADDRESS=<from .env.test.local>
TEST_USER_1_PRIVATE_KEY=<from .env.test.local>
TEST_HOST_1_ADDRESS=<from .env.test.local>
TEST_HOST_1_PRIVATE_KEY=<from .env.test.local>
```

#### Contract ABIs

Contract ABIs are located in:
- `contracts/` - Full contract ABIs
- `docs/compute-contracts-reference/client-abis/` - Client-optimized ABIs
  - `JobMarketplaceWithModels-CLIENT-ABI.json`
  - `NodeRegistryWithModels-CLIENT-ABI.json`

#### Documentation References

- **Host Registration**: `docs/compute-contracts-reference/HOST_REGISTRATION_GUIDE.md`
- **JobMarketplace**: `docs/compute-contracts-reference/JobMarketplace.md`
- **S5 Integration**: `docs/compute-contracts-reference/S5_NODE_INTEGRATION_GUIDE.md`
- **Dual Pricing**: `docs/NODE_DUAL_PRICING_UPDATE_v7.0.29.md`

---

## Client Examples

### cURL

#### Basic Inference Request

```bash
curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama-2-7b",
    "prompt": "What is the capital of France?",
    "max_tokens": 50,
    "temperature": 0.5
  }'
```

#### Streaming Request

```bash
curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
    "model": "llama-2-7b",
    "prompt": "Tell me a story",
    "max_tokens": 200,
    "stream": true
  }'
```

#### Generate Embeddings

```bash
# Single text embedding
curl -X POST http://localhost:8080/v1/embed \
  -H "Content-Type: application/json" \
  -d '{
    "texts": ["What is machine learning?"]
  }'

# Batch embedding with explicit model
curl -X POST http://localhost:8080/v1/embed \
  -H "Content-Type: application/json" \
  -d '{
    "texts": [
      "Machine learning is a subset of AI",
      "Deep learning uses neural networks",
      "Neural networks mimic biological neurons"
    ],
    "model": "all-MiniLM-L6-v2",
    "chainId": 84532
  }'

# List available embedding models
curl "http://localhost:8080/v1/models?type=embedding"
```

### Python

```python
import requests
import json

# Non-streaming request
def inference_request(prompt, model="llama-2-7b"):
    url = "http://localhost:8080/v1/inference"
    payload = {
        "model": model,
        "prompt": prompt,
        "max_tokens": 100,
        "temperature": 0.7
    }
    
    response = requests.post(url, json=payload)
    if response.status_code == 200:
        return response.json()
    else:
        raise Exception(f"Error: {response.status_code} - {response.text}")

# Streaming request
def streaming_inference(prompt, model="llama-2-7b"):
    url = "http://localhost:8080/v1/inference"
    payload = {
        "model": model,
        "prompt": prompt,
        "max_tokens": 100,
        "stream": True
    }
    
    with requests.post(url, json=payload, stream=True) as response:
        for line in response.iter_lines():
            if line:
                if line.startswith(b'data: '):
                    data = json.loads(line[6:])
                    print(data['content'], end='', flush=True)
                    if data.get('finish_reason'):
                        break

# Generate embeddings
def generate_embeddings(texts, model="all-MiniLM-L6-v2", chain_id=84532):
    """Generate embeddings for multiple texts"""
    url = "http://localhost:8080/v1/embed"
    payload = {
        "texts": texts,
        "model": model,
        "chainId": chain_id
    }

    response = requests.post(url, json=payload)
    if response.status_code == 200:
        return response.json()
    else:
        raise Exception(f"Embedding error: {response.status_code} - {response.text}")

# List embedding models
def list_embedding_models(chain_id=84532):
    """Get available embedding models"""
    url = f"http://localhost:8080/v1/models?type=embedding&chain_id={chain_id}"
    response = requests.get(url)
    return response.json()

# Example usage
texts = [
    "Machine learning is a subset of AI",
    "Deep learning uses neural networks",
    "Transformers revolutionized NLP"
]

result = generate_embeddings(texts)
print(f"Generated {len(result['embeddings'])} embeddings")
print(f"Total tokens: {result['totalTokens']}")
print(f"Cost: ${result['cost']} (zero-cost!)")
print(f"Dimensions: {len(result['embeddings'][0]['embedding'])}")

# List available models
models = list_embedding_models()
print(f"Available models: {[m['name'] for m in models['models']]}")
```

### JavaScript/TypeScript

```javascript
// Non-streaming request
async function inferenceRequest(prompt, model = 'llama-2-7b') {
  const response = await fetch('http://localhost:8080/v1/inference', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      model,
      prompt,
      max_tokens: 100,
      temperature: 0.7,
    }),
  });

  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`);
  }

  return await response.json();
}

// Streaming request using EventSource
function streamingInference(prompt, model = 'llama-2-7b') {
  const eventSource = new EventSource(
    `http://localhost:8080/v1/inference?` + 
    new URLSearchParams({
      model,
      prompt,
      max_tokens: '100',
      stream: 'true',
    })
  );

  eventSource.onmessage = (event) => {
    const data = JSON.parse(event.data);
    process.stdout.write(data.content);
    
    if (data.finish_reason) {
      eventSource.close();
    }
  };

  eventSource.onerror = (error) => {
    console.error('EventSource error:', error);
    eventSource.close();
  };
}

// Generate embeddings
async function generateEmbeddings(texts, model = 'all-MiniLM-L6-v2', chainId = 84532) {
  const response = await fetch('http://localhost:8080/v1/embed', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      texts,
      model,
      chainId,
    }),
  });

  if (!response.ok) {
    throw new Error(`Embedding error! status: ${response.status}`);
  }

  return await response.json();
}

// List embedding models
async function listEmbeddingModels(chainId = 84532) {
  const response = await fetch(
    `http://localhost:8080/v1/models?type=embedding&chain_id=${chainId}`
  );
  return await response.json();
}

// Example usage with TypeScript types
interface EmbeddingResult {
  embedding: number[];  // 384-dimensional vector
  text: string;
  tokenCount: number;
}

interface EmbedResponse {
  embeddings: EmbeddingResult[];
  model: string;
  provider: string;
  totalTokens: number;
  cost: number;
  chainId: number;
  chainName: string;
  nativeToken: string;
}

async function example() {
  const texts = [
    "Machine learning is a subset of AI",
    "Deep learning uses neural networks",
    "Transformers revolutionized NLP"
  ];

  try {
    const result: EmbedResponse = await generateEmbeddings(texts);
    console.log(`Generated ${result.embeddings.length} embeddings`);
    console.log(`Total tokens: ${result.totalTokens}`);
    console.log(`Cost: $${result.cost} (zero-cost!)`);
    console.log(`Dimensions: ${result.embeddings[0].embedding.length}`);

    // Use embeddings for similarity search, RAG, etc.
    const embeddings = result.embeddings.map(e => e.embedding);

    // List available models
    const models = await listEmbeddingModels();
    console.log(`Available models: ${models.models.map(m => m.name).join(', ')}`);
  } catch (error) {
    console.error('Error generating embeddings:', error);
  }
}
```

### WebSocket Client (JavaScript - Updated for Session Management)

```javascript
// Modern session-based WebSocket client
class FabstirWebSocketClient {
  constructor(endpoint = 'ws://localhost:8080/ws/session') {
    this.endpoint = endpoint;
    this.ws = null;
    this.sessionId = null;
    this.conversationHistory = [];
  }

  async connect(jobId) {
    this.ws = new WebSocket(this.endpoint);
    
    return new Promise((resolve, reject) => {
      this.ws.onopen = () => {
        // Authenticate
        this.ws.send(JSON.stringify({
          type: 'auth',
          job_id: jobId
        }));
        
        // Initialize session
        this.sessionId = crypto.randomUUID();
        this.ws.send(JSON.stringify({
          type: 'session_init',
          session_id: this.sessionId,
          job_id: jobId,
          model_config: {
            model: 'llama-2-7b',
            max_tokens: 2048,
            temperature: 0.7
          }
        }));
        
        resolve();
      };
      
      this.ws.onerror = reject;
    });
  }

  async sendPrompt(content) {
    const messageIndex = this.conversationHistory.length;
    
    this.ws.send(JSON.stringify({
      type: 'prompt',
      session_id: this.sessionId,
      content: content,
      message_index: messageIndex,
      stream: true
    }));
    
    // Store in history
    this.conversationHistory.push({
      role: 'user',
      content: content
    });
  }

  async resumeSession(jobId) {
    // Reconnect with full context
    await this.connect(jobId);
    
    this.ws.send(JSON.stringify({
      type: 'session_resume',
      session_id: this.sessionId,
      job_id: jobId,
      conversation_context: this.conversationHistory,
      last_message_index: this.conversationHistory.length - 1
    }));
  }

  handleMessage(callback) {
    this.ws.onmessage = (event) => {
      const message = JSON.parse(event.data);
      
      if (message.type === 'response' || message.type === 'stream_chunk') {
        callback(message);
        
        // Store complete responses with proof
        if (message.type === 'response' || message.is_final) {
          this.conversationHistory.push({
            role: 'assistant',
            content: message.content,
            proof: message.proof  // Store cryptographic proof
          });
          
          // Verify proof if present
          if (message.proof) {
            this.verifyProof(message.proof);
          }
        }
      }
    };
  }
  
  async verifyProof(proof) {
    // Basic proof verification
    console.log(`Proof received: Type=${proof.proof_type}, Hash=${proof.hash.substring(0, 16)}...`);
    
    // Verify hashes match expected values
    // In production, this would involve cryptographic verification
    if (proof.proof_type === 'ezkl' || proof.proof_type === 'risc0') {
      console.log('Cryptographic proof verified');
    }
    
    return true;
  }

  disconnect() {
    // Note: Disconnect triggers automatic payment settlement (v5+)
    // No need to explicitly call completeSessionJob
    if (this.ws) {
      this.ws.close();
      console.log('WebSocket disconnected - payment settlement triggered automatically');
    }
  }
}

// Usage example
const client = new FabstirWebSocketClient();
await client.connect(12345);

client.handleMessage((message) => {
  if (message.type === 'stream_chunk') {
    process.stdout.write(message.content);
  }
});

await client.sendPrompt('Explain quantum computing');
```

### Legacy WebSocket Client (Deprecated)

```javascript
const ws = new WebSocket('ws://localhost:8080/v1/ws');

ws.onopen = () => {
  console.log('Connected to Fabstir LLM Node');
  
  // Send inference request
  ws.send(JSON.stringify({
    type: 'inference_request',
    payload: {
      model: 'llama-2-7b',
      prompt: 'Explain blockchain in one sentence',
      max_tokens: 50,
      temperature: 0.7,
    }
  }));
};

ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  
  if (message.type === 'inference_response') {
    process.stdout.write(message.payload.content);
    
    if (message.payload.finish_reason) {
      console.log('\nInference complete');
      ws.close();
    }
  } else if (message.type === 'error') {
    console.error('Error:', message.payload);
    ws.close();
  }
};

ws.onerror = (error) => {
  console.error('WebSocket error:', error);
};

ws.onclose = () => {
  console.log('Disconnected from Fabstir LLM Node');
};
```

---

## P2P Discovery API

The Fabstir LLM Node provides P2P discovery endpoints for finding and connecting to other nodes in the decentralized network. This enables clients to discover nodes offering specific models and capabilities.

### Discovery Mechanisms

The node supports three discovery mechanisms:

1. **mDNS (Local Discovery)** - Automatically discovers peers on the same local network
2. **Kademlia DHT (Global Discovery)** - Distributed hash table for global peer discovery
3. **Bootstrap Nodes** - Initial nodes for joining the P2P network

---

### List Discovered Peers

Get a list of all discovered peers in the network.

#### Request

```http
GET /v1/peers
```

#### Response

```json
{
  "peers": [
    {
      "peer_id": "12D3KooWLRPJbXzZ7yYcQvcVn8RaKNrLQxmFrKxKVXqP4tz9HeBq",
      "addresses": [
        "/ip4/192.168.1.100/tcp/9000",
        "/ip4/192.168.1.100/udp/9001/quic"
      ],
      "capabilities": ["llama-2-7b", "mistral-7b"],
      "discovered_via": "mdns",
      "last_seen": 1708123456789,
      "connection_status": "connected"
    },
    {
      "peer_id": "12D3KooWBXzZ8yYcQvcVn9RbKNsLQxnFsKyKVXqQ5uz8HeCs",
      "addresses": [
        "/ip4/89.45.23.100/tcp/9000"
      ],
      "capabilities": ["vicuna-13b"],
      "discovered_via": "dht",
      "last_seen": 1708123450000,
      "connection_status": "disconnected"
    }
  ],
  "total_peers": 2,
  "connected_peers": 1
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `peers` | Array | List of discovered peers |
| `peers[].peer_id` | String | Unique peer identifier |
| `peers[].addresses` | Array | Network addresses for the peer |
| `peers[].capabilities` | Array | Models/services offered by the peer |
| `peers[].discovered_via` | String | Discovery mechanism: "mdns", "dht", "bootstrap" |
| `peers[].last_seen` | Integer | Unix timestamp of last contact |
| `peers[].connection_status` | String | "connected", "disconnected", "connecting" |
| `total_peers` | Integer | Total number of discovered peers |
| `connected_peers` | Integer | Number of currently connected peers |

---

### Get Peer Information

Retrieve detailed information about a specific peer.

#### Request

```http
GET /v1/peers/{peer_id}
```

#### Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `peer_id` | String | Yes | The peer ID to query |

#### Response

```json
{
  "peer_id": "12D3KooWLRPJbXzZ7yYcQvcVn8RaKNrLQxmFrKxKVXqP4tz9HeBq",
  "addresses": [
    "/ip4/192.168.1.100/tcp/9000",
    "/ip4/192.168.1.100/udp/9001/quic"
  ],
  "capabilities": ["llama-2-7b", "mistral-7b"],
  "metadata": {
    "node_version": "0.1.0",
    "gpu_info": "NVIDIA RTX 4090",
    "max_batch_size": 8,
    "pricing": {
      "per_token": "0.0001",
      "currency": "ETH"
    }
  },
  "connection_info": {
    "status": "connected",
    "latency_ms": 45,
    "bandwidth_mbps": 100,
    "established_at": 1708123400000
  },
  "reputation": {
    "total_jobs": 1523,
    "success_rate": 0.998,
    "average_response_time_ms": 250
  }
}
```

---

### Trigger Peer Discovery

Manually trigger peer discovery process.

#### Request

```http
POST /v1/peers/discover
```

```json
{
  "method": "all",
  "timeout_ms": 5000
}
```

#### Request Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `method` | String | No | "all" | Discovery method: "all", "mdns", "dht", "bootstrap" |
| `timeout_ms` | Integer | No | 5000 | Maximum time to wait for discovery |

#### Response

```json
{
  "discovered_count": 5,
  "new_peers": 2,
  "discovery_results": {
    "mdns": {
      "discovered": 2,
      "duration_ms": 120
    },
    "dht": {
      "discovered": 3,
      "duration_ms": 890
    }
  }
}
```

---

### List Node Capabilities

Get the capabilities (models and services) advertised by this node.

#### Request

```http
GET /v1/capabilities
```

#### Response

```json
{
  "capabilities": [
    {
      "type": "model",
      "id": "llama-2-7b",
      "name": "Llama 2 7B",
      "version": "1.0.0",
      "enabled": true,
      "performance": {
        "tokens_per_second": 50,
        "max_context_length": 4096
      }
    },
    {
      "type": "model",
      "id": "mistral-7b",
      "name": "Mistral 7B",
      "version": "0.1.0",
      "enabled": true,
      "performance": {
        "tokens_per_second": 60,
        "max_context_length": 8192
      }
    }
  ],
  "announced_to_network": true,
  "last_announcement": 1708123456789
}
```

---

### Announce Capabilities

Announce this node's capabilities to the P2P network.

#### Request

```http
POST /v1/capabilities/announce
```

```json
{
  "capabilities": ["llama-2-7b", "mistral-7b"],
  "metadata": {
    "gpu_info": "NVIDIA RTX 4090",
    "max_batch_size": 8,
    "pricing": {
      "per_token": "0.0001",
      "currency": "ETH"
    }
  },
  "ttl_seconds": 3600
}
```

#### Request Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `capabilities` | Array | No | Current | Models/services to announce |
| `metadata` | Object | No | {} | Additional node metadata |
| `ttl_seconds` | Integer | No | 3600 | Time-to-live for the announcement |

#### Response

```json
{
  "announced": true,
  "capabilities_count": 2,
  "propagation_estimate_ms": 1500,
  "next_refresh": 1708127056789
}
```

---

### Search for Capabilities

Find nodes in the network offering specific capabilities.

#### Request

```http
POST /v1/capabilities/search
```

```json
{
  "capability": "llama-2-7b",
  "max_results": 10,
  "filters": {
    "min_success_rate": 0.95,
    "max_latency_ms": 100,
    "required_gpu": "RTX"
  }
}
```

#### Request Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `capability` | String | Yes | The capability/model to search for |
| `max_results` | Integer | No | Maximum number of results (default: 10) |
| `filters` | Object | No | Optional filters for peer selection |
| `filters.min_success_rate` | Float | No | Minimum job success rate (0-1) |
| `filters.max_latency_ms` | Integer | No | Maximum network latency |
| `filters.required_gpu` | String | No | GPU type requirement |

#### Response

```json
{
  "results": [
    {
      "peer_id": "12D3KooWLRPJbXzZ7yYcQvcVn8RaKNrLQxmFrKxKVXqP4tz9HeBq",
      "addresses": ["/ip4/192.168.1.100/tcp/9000"],
      "capability_info": {
        "id": "llama-2-7b",
        "version": "1.0.0",
        "performance": {
          "tokens_per_second": 50,
          "success_rate": 0.998
        }
      },
      "network_info": {
        "latency_ms": 45,
        "connection_status": "connected"
      },
      "score": 0.95
    }
  ],
  "total_found": 3,
  "search_duration_ms": 750
}
```

---

### Network Topology

Get the current P2P network topology from this node's perspective.

#### Request

```http
GET /v1/network/topology
```

#### Response

```json
{
  "local_peer_id": "12D3KooWLocal...",
  "bootstrap_nodes": [
    {
      "peer_id": "12D3KooWBootstrap1...",
      "status": "connected"
    }
  ],
  "routing_table": {
    "size": 20,
    "buckets": 4
  },
  "network_stats": {
    "total_discovered": 45,
    "currently_connected": 8,
    "average_latency_ms": 65,
    "bandwidth_usage": {
      "upload_mbps": 10.5,
      "download_mbps": 25.3
    }
  },
  "discovery_status": {
    "mdns": {
      "enabled": true,
      "last_discovery": 1708123456789,
      "peers_found": 3
    },
    "dht": {
      "enabled": true,
      "bootstrap_status": "completed",
      "last_refresh": 1708123000000,
      "peers_found": 42
    }
  }
}
```

---

### P2P Configuration

Configure P2P discovery settings at runtime.

#### Request

```http
PUT /v1/network/config
```

```json
{
  "enable_mdns": true,
  "enable_dht": true,
  "max_peers": 50,
  "discovery_interval_seconds": 60,
  "capability_refresh_interval_seconds": 300
}
```

#### Response

```json
{
  "updated": true,
  "config": {
    "enable_mdns": true,
    "enable_dht": true,
    "max_peers": 50,
    "discovery_interval_seconds": 60,
    "capability_refresh_interval_seconds": 300
  },
  "restart_required": false
}
```

---

### P2P Configuration Options

When starting the node, P2P discovery can be configured with:

```rust
NodeConfig {
    // Discovery settings
    enable_mdns: true,                    // Enable local network discovery
    enable_dht: true,                     // Enable DHT participation
    enable_rendezvous: false,             // Enable rendezvous protocol
    
    // Bootstrap configuration
    bootstrap_peers: vec![                // Initial peers for network join
        (peer_id, "/ip4/89.45.23.100/tcp/9000"),
    ],
    bootstrap_interval: Duration::from_secs(300),
    
    // DHT settings
    dht_replication_factor: 20,          // Number of replicas in DHT
    dht_republish_interval: Duration::from_secs(3600),
    dht_record_ttl: Duration::from_secs(7200),
    
    // Capability announcement
    capabilities: vec![                   // Models this node provides
        "llama-2-7b".to_string(),
        "mistral-7b".to_string(),
    ],
    capability_ttl: Duration::from_secs(3600),
    
    // Network limits
    max_peers: 50,                       // Maximum connected peers
    max_discovered_peers: 200,           // Maximum discovered peers to track
    
    // Discovery intervals
    peer_discovery_interval: Duration::from_secs(60),
    capability_announcement_interval: Duration::from_secs(300),
}
```

---

## Best Practices

### 1. Connection Management

- Implement connection pooling for multiple requests
- Reuse WebSocket connections for multiple inference requests
- Handle connection timeouts and implement retry logic

### 2. Error Handling

- Always check response status codes
- Implement exponential backoff for retries
- Parse error responses for detailed information

### 3. Streaming

- Use streaming for long-form content generation
- Implement proper stream parsing and buffering
- Handle partial responses and connection interruptions

### 4. Rate Limiting

- Respect rate limits to avoid 429 errors
- Implement client-side rate limiting
- Use the `Retry-After` header when rate limited

### 5. Model Selection

- Query `/v1/models` to verify model availability
- Cache model list with appropriate TTL
- Handle model unavailability gracefully

---

## Troubleshooting

### Common Issues

#### Connection Refused

```
Error: connect ECONNREFUSED 127.0.0.1:8080
```

**Solution**: Ensure the Fabstir LLM Node is running and listening on the correct port.

#### Model Not Found

```json
{
  "error": {
    "code": "MODEL_NOT_FOUND",
    "message": "Model 'gpt-4' not found on this node"
  }
}
```

**Solution**: Use `/v1/models` to list available models.

#### Rate Limit Exceeded

```json
{
  "error": {
    "code": "RATE_LIMIT_EXCEEDED",
    "message": "Rate limit exceeded. Please retry after 60 seconds"
  }
}
```

**Solution**: Implement rate limiting on the client side or wait for the specified retry period.

#### Timeout Errors

```json
{
  "error": {
    "code": "TIMEOUT",
    "message": "Request timeout after 30 seconds"
  }
}
```

**Solution**: 
- Reduce `max_tokens` for faster responses
- Increase client timeout settings
- Use streaming for long generations

### P2P Discovery Issues

#### Node Not Discoverable

```
Error: Node is not being discovered by peers
```

**Solution**:
- Ensure P2P ports (default 9000-9001) are open in firewall
- Check that mDNS is enabled for local discovery: `enable_mdns: true`
- Verify bootstrap nodes are reachable and configured correctly
- Announce capabilities explicitly: `POST /v1/capabilities/announce`
- Check NAT configuration - may need port forwarding for external peers

#### Bootstrap Failure

```json
{
  "error": {
    "code": "BOOTSTRAP_FAILED",
    "message": "Failed to connect to bootstrap nodes"
  }
}
```

**Solution**:
- Verify bootstrap node addresses are correct
- Check network connectivity to bootstrap nodes
- Try alternative bootstrap nodes
- Start with mDNS discovery on local network first
- Ensure DHT is enabled: `enable_dht: true`

#### No Peers Found

```json
{
  "peers": [],
  "total_peers": 0
}
```

**Solution**:
- Wait 30-60 seconds after node startup for discovery
- Manually trigger discovery: `POST /v1/peers/discover`
- Check if running behind restrictive NAT/firewall
- Verify at least one discovery mechanism is enabled
- Join a known bootstrap node or use a relay server temporarily

#### Capability Search Returns Empty

```json
{
  "results": [],
  "total_found": 0
}
```

**Solution**:
- Ensure nodes are announcing their capabilities
- Wait for DHT propagation (can take 1-2 minutes)
- Check capability ID matches exactly (case-sensitive)
- Remove overly restrictive filters in search query
- Verify network has nodes offering the requested capability

---

## P2P Client Examples

### Python - Discovering Peers

```python
import requests
import json
import time

class P2PDiscoveryClient:
    def __init__(self, base_url="http://localhost:8080"):
        self.base_url = base_url
    
    def discover_peers(self, method="all", timeout_ms=5000):
        """Trigger peer discovery"""
        response = requests.post(
            f"{self.base_url}/v1/peers/discover",
            json={"method": method, "timeout_ms": timeout_ms}
        )
        return response.json()
    
    def get_peers(self):
        """Get list of discovered peers"""
        response = requests.get(f"{self.base_url}/v1/peers")
        return response.json()
    
    def find_nodes_with_model(self, model_id, max_results=10):
        """Find nodes offering a specific model"""
        response = requests.post(
            f"{self.base_url}/v1/capabilities/search",
            json={
                "capability": model_id,
                "max_results": max_results,
                "filters": {
                    "min_success_rate": 0.9
                }
            }
        )
        return response.json()
    
    def announce_capabilities(self, capabilities):
        """Announce this node's capabilities"""
        response = requests.post(
            f"{self.base_url}/v1/capabilities/announce",
            json={"capabilities": capabilities}
        )
        return response.json()

# Example usage
client = P2PDiscoveryClient()

# Discover peers on the network
print("Discovering peers...")
discovery_result = client.discover_peers()
print(f"Found {discovery_result['discovered_count']} peers")

# Wait for discovery to complete
time.sleep(2)

# Get list of peers
peers = client.get_peers()
print(f"Connected to {peers['connected_peers']} of {peers['total_peers']} peers")

# Find nodes with Llama 2 7B model
llama_nodes = client.find_nodes_with_model("llama-2-7b")
if llama_nodes['results']:
    best_node = llama_nodes['results'][0]
    print(f"Best Llama 2 node: {best_node['peer_id'][:16]}...")
    print(f"  Latency: {best_node['network_info']['latency_ms']}ms")
    print(f"  Success rate: {best_node['capability_info']['performance']['success_rate']}")
```

### JavaScript/TypeScript - P2P Network Monitor

```typescript
interface Peer {
  peer_id: string;
  addresses: string[];
  capabilities: string[];
  connection_status: string;
}

interface NetworkTopology {
  local_peer_id: string;
  network_stats: {
    total_discovered: number;
    currently_connected: number;
    average_latency_ms: number;
  };
}

class P2PNetworkMonitor {
  private baseUrl: string;
  private pollInterval: number;

  constructor(baseUrl = 'http://localhost:8080', pollInterval = 30000) {
    this.baseUrl = baseUrl;
    this.pollInterval = pollInterval;
  }

  async getNetworkTopology(): Promise<NetworkTopology> {
    const response = await fetch(`${this.baseUrl}/v1/network/topology`);
    return response.json();
  }

  async getPeers(): Promise<{ peers: Peer[] }> {
    const response = await fetch(`${this.baseUrl}/v1/peers`);
    return response.json();
  }

  async monitorNetwork(callback: (topology: NetworkTopology) => void) {
    // Initial fetch
    const topology = await this.getNetworkTopology();
    callback(topology);

    // Poll for updates
    setInterval(async () => {
      try {
        const topology = await this.getNetworkTopology();
        callback(topology);
      } catch (error) {
        console.error('Failed to fetch network topology:', error);
      }
    }, this.pollInterval);
  }

  async findBestNodeForModel(
    modelId: string, 
    maxLatency = 100
  ): Promise<any> {
    const response = await fetch(`${this.baseUrl}/v1/capabilities/search`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        capability: modelId,
        max_results: 5,
        filters: {
          max_latency_ms: maxLatency,
          min_success_rate: 0.95
        }
      })
    });
    
    const result = await response.json();
    return result.results?.[0] || null;
  }
}

// Example usage
const monitor = new P2PNetworkMonitor();

// Monitor network topology
monitor.monitorNetwork((topology) => {
  console.log(`Network Status:`);
  console.log(`  Discovered peers: ${topology.network_stats.total_discovered}`);
  console.log(`  Connected peers: ${topology.network_stats.currently_connected}`);
  console.log(`  Average latency: ${topology.network_stats.average_latency_ms}ms`);
});

// Find best node for inference
async function selectNodeForInference(model: string) {
  const bestNode = await monitor.findBestNodeForModel(model);
  if (bestNode) {
    console.log(`Selected node ${bestNode.peer_id} for ${model}`);
    // Use this node for inference requests
    return bestNode;
  } else {
    console.log(`No suitable nodes found for ${model}`);
    return null;
  }
}
```

### Go - Capability Announcement

```go
package main

import (
    "bytes"
    "encoding/json"
    "fmt"
    "net/http"
    "time"
)

type CapabilityAnnouncement struct {
    Capabilities []string               `json:"capabilities"`
    Metadata     map[string]interface{} `json:"metadata"`
    TTLSeconds   int                   `json:"ttl_seconds"`
}

type P2PClient struct {
    BaseURL string
    Client  *http.Client
}

func NewP2PClient(baseURL string) *P2PClient {
    return &P2PClient{
        BaseURL: baseURL,
        Client: &http.Client{
            Timeout: 10 * time.Second,
        },
    }
}

func (c *P2PClient) AnnounceCapabilities(
    capabilities []string, 
    gpuInfo string,
) error {
    announcement := CapabilityAnnouncement{
        Capabilities: capabilities,
        Metadata: map[string]interface{}{
            "gpu_info": gpuInfo,
            "node_version": "0.1.0",
            "max_batch_size": 8,
        },
        TTLSeconds: 3600,
    }

    jsonData, err := json.Marshal(announcement)
    if err != nil {
        return err
    }

    resp, err := c.Client.Post(
        c.BaseURL+"/v1/capabilities/announce",
        "application/json",
        bytes.NewBuffer(jsonData),
    )
    if err != nil {
        return err
    }
    defer resp.Body.Close()

    if resp.StatusCode != http.StatusOK {
        return fmt.Errorf("announcement failed with status: %d", resp.StatusCode)
    }

    return nil
}

func (c *P2PClient) DiscoverPeers() (map[string]interface{}, error) {
    resp, err := c.Client.Get(c.BaseURL + "/v1/peers")
    if err != nil {
        return nil, err
    }
    defer resp.Body.Close()

    var result map[string]interface{}
    if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
        return nil, err
    }

    return result, nil
}

func main() {
    client := NewP2PClient("http://localhost:8080")

    // Announce capabilities
    capabilities := []string{"llama-2-7b", "mistral-7b"}
    err := client.AnnounceCapabilities(capabilities, "NVIDIA RTX 4090")
    if err != nil {
        fmt.Printf("Failed to announce: %v\n", err)
    } else {
        fmt.Println("Capabilities announced successfully")
    }

    // Discover peers
    peers, err := client.DiscoverPeers()
    if err != nil {
        fmt.Printf("Failed to discover peers: %v\n", err)
    } else {
        fmt.Printf("Discovered %v peers\n", peers["total_peers"])
    }
}
```

### Rust - Complete P2P Integration

```rust
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize)]
struct CapabilitySearch {
    capability: String,
    max_results: u32,
    filters: SearchFilters,
}

#[derive(Serialize)]
struct SearchFilters {
    min_success_rate: f32,
    max_latency_ms: u32,
}

#[derive(Deserialize)]
struct SearchResult {
    results: Vec<NodeResult>,
    total_found: u32,
}

#[derive(Deserialize)]
struct NodeResult {
    peer_id: String,
    capability_info: CapabilityInfo,
    network_info: NetworkInfo,
    score: f32,
}

#[derive(Deserialize)]
struct CapabilityInfo {
    id: String,
    performance: Performance,
}

#[derive(Deserialize)]
struct Performance {
    tokens_per_second: u32,
    success_rate: f32,
}

#[derive(Deserialize)]
struct NetworkInfo {
    latency_ms: u32,
    connection_status: String,
}

pub struct P2PDiscoveryClient {
    base_url: String,
    client: reqwest::Client,
}

impl P2PDiscoveryClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn find_best_node(
        &self, 
        model: &str,
    ) -> Result<Option<String>, reqwest::Error> {
        let search = CapabilitySearch {
            capability: model.to_string(),
            max_results: 5,
            filters: SearchFilters {
                min_success_rate: 0.95,
                max_latency_ms: 100,
            },
        };

        let response = self.client
            .post(format!("{}/v1/capabilities/search", self.base_url))
            .json(&search)
            .send()
            .await?
            .json::<SearchResult>()
            .await?;

        Ok(response.results.first().map(|n| n.peer_id.clone()))
    }

    pub async fn trigger_discovery(&self) -> Result<(), reqwest::Error> {
        let mut params = HashMap::new();
        params.insert("method", "all");
        params.insert("timeout_ms", "5000");

        self.client
            .post(format!("{}/v1/peers/discover", self.base_url))
            .json(&params)
            .send()
            .await?;

        Ok(())
    }
}

// Example usage
#[tokio::main]
async fn main() {
    let client = P2PDiscoveryClient::new("http://localhost:8080");
    
    // Trigger discovery
    client.trigger_discovery().await.unwrap();
    
    // Find best node for Llama 2
    match client.find_best_node("llama-2-7b").await {
        Ok(Some(peer_id)) => {
            println!("Best node for Llama 2: {}", peer_id);
            // Use this peer_id for inference requests
        }
        Ok(None) => println!("No suitable nodes found"),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

---

## API Versioning

The API uses URL path versioning. Current version: **v1**

Future versions will maintain backward compatibility where possible. Breaking changes will be introduced in new major versions (e.g., `/v2/`).

### Version History

- **v8.4.1** (Current) - S5 Vector Loading Production Ready (November 2025)
  - S5 Integration Testing Complete: All 19 tests passing (100%)
  - Enhanced S5.js bridge integration with real HTTP API
  - `LoadVectorDatabase`, `VectorDatabaseLoaded`, `VectorLoadProgress`, `VectorDatabaseError` message types
  - Support for encrypted `vector_database` paths in job parameters
  - Bridge unavailability testing and fallback handling verified
  - Production-ready S5 vector database loading from decentralized storage
  - Documentation updates across all SDK and node reference docs

- **v8.4.0** - S5 Vector Loading Feature Complete (November 2025)
  - Complete implementation of S5 vector database loading
  - Integration with Enhanced S5.js bridge (HTTP API)
  - Support for encrypted vector database paths
  - 384-dimensional vector validation and indexing
  - Progress tracking during vector load operations
  - Comprehensive error handling for S5 fetch failures
  - Session-scoped vector storage with automatic cleanup

- **v8.3.13** - Chat Templates and Harmony Support (November 2025)
  - Model-specific chat template formatting (Harmony, Llama, ChatML, Vicuna)
  - Automatic template detection based on model metadata
  - Multi-turn conversation formatting
  - Harmony chat template with `<|im_start|>` tokens
  - Improved inference quality through proper prompt formatting

- **v8.3.6** - Host-Side RAG Enhancements (November 2025)
  - Session-scoped vector storage optimizations
  - Improved cosine similarity search performance (<50ms for 10K vectors)
  - Enhanced vector upload/search WebSocket messages
  - Better memory management for large vector collections

- **v8.3.0** - Host-Side RAG Introduction (October 2025)
  - Session-scoped vector storage for document retrieval
  - 384-dimensional embeddings via `/v1/embed` endpoint
  - `uploadVectors` and `searchVectors` WebSocket messages
  - Cosine similarity search with configurable thresholds
  - Automatic cleanup on session disconnect
  - Support for up to 100,000 vectors per session

- **v8.2.0** - Production Hardening (October 2025)
  - Enhanced error handling and logging
  - Improved WebSocket stability and reconnection
  - Better metrics and monitoring
  - Performance optimizations for high-load scenarios

- **v8.1.2** - S5 Off-Chain Proof Storage (September 2025)
  - Off-chain proof storage in S5 decentralized network
  - On-chain submission of hash + CID only (737x size reduction)
  - `submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID)` contract function
  - New JobMarketplace contract: `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E`
  - ~82 bytes on-chain per checkpoint (hash: 32B, CID: ~50B)
  - Full ~221KB proofs retrievable from S5 for verification

- **v8.1.0** - Risc0 zkVM Integration (September 2025)
  - GPU-accelerated STARK proof generation via Risc0 zkVM v3.0
  - CUDA support for fast proof generation (0.2-2.3s per proof)
  - Automatic checkpoint submission every 50 tokens
  - SHA256 hash calculation for proof integrity
  - Proof size: ~221KB (real Risc0) vs 126 bytes (mock)
  - **Critical**: Requires `--features real-ezkl` build flag for production

- **v8.0.0** - End-to-End Encryption (August 2025)
  - ECDH key exchange on secp256k1 (Ethereum-compatible)
  - XChaCha20-Poly1305 AEAD for symmetric encryption
  - HKDF-SHA256 for key derivation
  - ECDSA signature recovery for client authentication
  - Perfect forward secrecy via ephemeral keys
  - Session key management with TTL-based expiration
  - 111 comprehensive tests (87 unit, 14 integration, 10 security)
  - `encrypted_session_init`, `encrypted_message`, `encrypted_chunk` message types

- **v7.0.29** - Dual Pricing System (July 2025)
  - Separate pricing for native tokens (ETH/BNB) and stablecoins (USDC)
  - `minPricePerTokenNative` and `minPricePerTokenStable` in NodeRegistry
  - Geometric mean pricing as default (MIN × MAX)^0.5
  - Pricing validation against contract constants
  - New contract: `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` (NodeRegistry with dual pricing)

- **v1.5** - Automatic Payment Settlement on WebSocket Disconnect (September 2024)
  - WebSocket disconnect triggers `completeSessionJob()` automatically
  - Ensures payment distribution even on unexpected disconnects
  - Host earnings (90%) automatically sent to HostEarnings contract
  - Treasury fee (10%) and user refund handled automatically
  - Requires HOST_PRIVATE_KEY configuration

- **v1.3** - Model Governance and Registry Integration
  - Integration with ModelRegistry smart contract for approved models
  - Model validation with SHA256 hash verification
  - Node registration with validated model IDs
  - Support for NodeRegistryWithModels contract

- **v1.2** - Enhanced proof integration in WebSocket messages
  - ProofData structure in ConversationMessage and StreamToken
  - ProofManager with LRU cache eviction
  - Streaming integration with proofs in final tokens

- **v1.1** - Initial proof generation support
  - Cryptographic verification of inference results
  - Support for multiple proof types
  - Hash-based verification of model, input, and output

- **v1.0** - Initial API release
  - WebSocket production features
  - Session management with stateless memory cache
  - JWT authentication and Ed25519 signatures
  - Message compression and rate limiting

---

## Troubleshooting

### Common Issues and Solutions

#### Chain-Related Issues

**Problem: "Unsupported chain ID" error**
- **Cause**: Using an invalid chain ID in the request
- **Solution**: Use only supported chain IDs: 84532 (Base Sepolia) or 5611 (opBNB Testnet)
- **Example Fix**:
  ```bash
  # Correct
  curl "http://localhost:8080/v1/models?chain_id=84532"

  # Incorrect
  curl "http://localhost:8080/v1/models?chain_id=1"  # Mainnet not supported
  ```

**Problem: Model not available on specific chain**
- **Cause**: Model is not registered on the requested chain's ModelRegistry
- **Solution**: Check available models for the chain first
- **Example**:
  ```bash
  # First check available models
  curl "http://localhost:8080/v1/models?chain_id=84532"
  # Then use only listed models in inference requests
  ```

**Problem: Settlement fails on wrong chain**
- **Cause**: Job was created on a different chain than the one being used
- **Solution**: Ensure job_id and chain_id match the original job creation
- **Verification**:
  ```bash
  curl "http://localhost:8080/v1/session/{session_id}/info"
  # Check the chain_id in response matches your job's chain
  ```

#### Connection Issues

**Problem: WebSocket connection drops immediately**
- **Cause**: Invalid authentication or missing chain_id
- **Solution**: Include both job_id and chain_id in auth message
- **Example**:
  ```json
  {
    "type": "auth",
    "job_id": 123,
    "chain_id": 84532
  }
  ```

**Problem: "Connection refused" error**
- **Cause**: Node not running or incorrect port
- **Solution**: Check node is running and using correct port (default 8080)
- **Verification**:
  ```bash
  # Check if node is listening
  netstat -an | grep 8080

  # Test health endpoint
  curl "http://localhost:8080/health"
  ```

#### Inference Issues

**Problem: Inference request hangs or times out**
- **Cause**: Model not loaded or GPU memory issues
- **Solution**:
  1. Check model is downloaded: `ls models/`
  2. Check GPU memory: `nvidia-smi`
  3. Restart node with lower batch size
- **Debug Commands**:
  ```bash
  # Check logs for model loading issues
  RUST_LOG=debug cargo run

  # Monitor GPU usage
  watch -n 1 nvidia-smi
  ```

**Problem: "Model not found" despite being listed**
- **Cause**: Model file missing or corrupted
- **Solution**: Re-download model and verify hash
- **Steps**:
  ```bash
  # Download model
  ./scripts/phase_4_2_2/download_test_model.sh

  # Verify model hash matches registry
  sha256sum models/tinyllama-1b.Q4_K_M.gguf
  ```

#### Payment and Settlement Issues

**Problem: Payment not received after job completion**
- **Cause**: Settlement transaction failed on blockchain
- **Solution**: Check chain statistics and retry settlement
- **Debug**:
  ```bash
  # Check chain stats for failed settlements
  curl "http://localhost:8080/v1/chains/{chain_id}/stats"

  # Check session status
  curl "http://localhost:8080/v1/session/{session_id}/info"
  ```

**Problem: "Insufficient gas" error in logs**
- **Cause**: Node wallet has insufficient native tokens (ETH/BNB)
- **Solution**: Fund the node wallet address
- **Check Balance**:
  ```bash
  # Base Sepolia
  cast balance {node_address} --rpc-url https://sepolia.base.org

  # opBNB Testnet
  cast balance {node_address} --rpc-url https://opbnb-testnet.binance.org
  ```

#### Rate Limiting Issues

**Problem: "Rate limit exceeded" (429 error)**
- **Cause**: Too many requests in short time
- **Solution**: Implement exponential backoff
- **Example Python Retry Logic**:
  ```python
  import time
  import random

  def make_request_with_retry(url, data, max_retries=3):
      for attempt in range(max_retries):
          response = requests.post(url, json=data)
          if response.status_code == 429:
              # Exponential backoff with jitter
              wait = (2 ** attempt) + random.uniform(0, 1)
              time.sleep(wait)
          else:
              return response
      raise Exception("Max retries exceeded")
  ```

#### Performance Issues

**Problem: Slow inference response times**
- **Cause**: Model too large for available GPU memory
- **Solution**: Use smaller quantized models or adjust batch size
- **Model Sizes**:
  - TinyLlama Q4_K_M: ~700MB VRAM
  - Llama-2-7B Q4_K_M: ~4GB VRAM
  - Llama-2-13B Q4_K_M: ~8GB VRAM

**Problem: High memory usage**
- **Cause**: Model cache holding too many models
- **Solution**: Configure cache size in environment
- **Configuration**:
  ```bash
  # Limit model cache to 2 models
  export MODEL_CACHE_SIZE=2
  cargo run --release
  ```

#### Embedding Issues

**Problem: "Embedding service not available" (503 error)**
- **Cause**: Embedding model manager not initialized
- **Solution**: Download embedding models and verify model path
- **Steps**:
  ```bash
  # Download embedding model
  ./scripts/download_embedding_model.sh

  # Verify model files exist
  ls -lh models/all-MiniLM-L6-v2-onnx/

  # Should see:
  # - model.onnx (~90MB)
  # - tokenizer.json (~500KB)

  # Restart node with embedding support
  cargo run --release
  ```

**Problem: "Model not found" when requesting embedding model**
- **Cause**: Model name typo or model not loaded
- **Solution**: List available models and use exact name
- **Debug**:
  ```bash
  # List available embedding models
  curl "http://localhost:8080/v1/models?type=embedding"

  # Use exact name from response
  curl -X POST http://localhost:8080/v1/embed \
    -H "Content-Type: application/json" \
    -d '{"texts": ["test"], "model": "all-MiniLM-L6-v2"}'
  ```

**Problem: "Dimension mismatch" error (500 error)**
- **Cause**: Model outputs wrong dimensions (not 384)
- **Solution**: Re-download model files or use different model
- **Verification**:
  ```bash
  # Re-download model with correct dimensions
  ./scripts/download_embedding_model.sh

  # Check model config in logs
  RUST_LOG=debug cargo run 2>&1 | grep "dimension"
  ```

**Problem: Embedding request times out**
- **Cause**: Too many texts in batch or texts too long
- **Solution**: Reduce batch size or text length
- **Limits**:
  - Maximum texts per request: 96
  - Maximum text length: 8192 characters
  - Recommended batch size: 10-20 for optimal performance
- **Example**:
  ```bash
  # Split large batch into smaller chunks
  # Bad: 200 texts at once
  # Good: 10 batches of 20 texts each
  ```

**Problem: "Text too long" validation error (400 error)**
- **Cause**: Individual text exceeds 8192 character limit
- **Solution**: Truncate or split long texts
- **Example**:
  ```python
  def chunk_text(text, max_length=8000):
      """Split long text into chunks"""
      return [text[i:i+max_length] for i in range(0, len(text), max_length)]

  # Use chunked texts
  long_text = "very long document..."
  chunks = chunk_text(long_text)
  embeddings = generate_embeddings(chunks)
  ```

**Problem: "Empty texts" validation error (400 error)**
- **Cause**: Sending empty array or whitespace-only strings
- **Solution**: Filter empty/whitespace strings before sending
- **Example**:
  ```python
  # Filter out empty and whitespace-only strings
  texts = [t.strip() for t in raw_texts if t and t.strip()]
  if texts:
      embeddings = generate_embeddings(texts)
  ```

**Problem: Slow embedding generation**
- **Cause**: CPU-only inference or large batch size
- **Solution**: Optimize batch size or consider GPU acceleration (optional)
- **Performance Tips**:
  - Optimal batch size: 10-20 texts per request
  - CPU performance: ~76ms per embedding (~13 embeddings/second)
  - Memory usage: ~90MB for model
  - Use concurrent requests for higher throughput
- **Example Parallel Processing**:
  ```python
  import asyncio
  import aiohttp

  async def embed_batch(session, texts):
      async with session.post(
          'http://localhost:8080/v1/embed',
          json={'texts': texts}
      ) as response:
          return await response.json()

  async def embed_large_dataset(all_texts, batch_size=20):
      async with aiohttp.ClientSession() as session:
          tasks = []
          for i in range(0, len(all_texts), batch_size):
              batch = all_texts[i:i+batch_size]
              tasks.append(embed_batch(session, batch))
          return await asyncio.gather(*tasks)
  ```

**Problem: Model download fails**
- **Cause**: Network issues or incorrect model repository
- **Solution**: Verify network connection and use pinned model version
- **Manual Download**:
  ```bash
  # Download from HuggingFace (if script fails)
  cd models/
  mkdir -p all-MiniLM-L6-v2-onnx
  cd all-MiniLM-L6-v2-onnx

  # Download model file
  wget https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/refs%2Fpr%2F21/onnx/model.onnx

  # Download tokenizer
  wget https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/raw/refs%2Fpr%2F21/tokenizer.json

  # Verify file sizes
  ls -lh
  # model.onnx should be ~90MB
  # tokenizer.json should be ~500KB
  ```

### Model Download Instructions

The embedding endpoint requires ONNX models to be downloaded before use.

#### Automatic Download (Recommended)

```bash
# Download all required embedding models
./scripts/download_embedding_model.sh

# This downloads:
# - all-MiniLM-L6-v2 ONNX model (~90MB)
# - Tokenizer files (~500KB)
# To: models/all-MiniLM-L6-v2-onnx/
```

#### Manual Download

If the automatic script fails, download manually:

```bash
cd models/
mkdir -p all-MiniLM-L6-v2-onnx
cd all-MiniLM-L6-v2-onnx

# Download model (ONNX format, 384 dimensions)
curl -L -o model.onnx \
  "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/refs%2Fpr%2F21/onnx/model.onnx"

# Download tokenizer
curl -L -o tokenizer.json \
  "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/raw/refs%2Fpr%2F21/tokenizer.json"

# Verify downloads
ls -lh
# Should see:
#   model.onnx      ~90MB
#   tokenizer.json  ~500KB
```

#### Verify Model Files

```bash
# Check file integrity
sha256sum models/all-MiniLM-L6-v2-onnx/model.onnx
sha256sum models/all-MiniLM-L6-v2-onnx/tokenizer.json

# Test embedding generation
curl -X POST http://localhost:8080/v1/embed \
  -H "Content-Type: application/json" \
  -d '{"texts": ["test embedding"]}'

# Should return 384-dimensional embedding
```

#### Alternative Models (Future)

Currently, only all-MiniLM-L6-v2 is supported. To add more models:

1. Ensure model outputs exactly 384 dimensions
2. Download ONNX format model and tokenizer
3. Add model config to embedding manager initialization
4. Restart node to load new model

**Supported Formats**:
- ONNX models only (for CPU/GPU compatibility)
- Sentence-transformer architecture
- Output dimension: 384 (required)

### Debug Commands

#### Check Node Health
```bash
# Basic health check
curl "http://localhost:8080/health"

# Detailed metrics
curl "http://localhost:8080/metrics"

# Chain-specific stats
curl "http://localhost:8080/v1/chains/stats"
```

#### Test Chain Connectivity
```bash
# Test Base Sepolia RPC
cast client --rpc-url https://sepolia.base.org

# Test opBNB Testnet RPC
cast client --rpc-url https://opbnb-testnet.binance.org

# Check contract deployment (v8.1.2+ hash+CID contract)
cast code 0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E --rpc-url https://sepolia.base.org
```

#### Monitor Logs
```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Filter for specific modules
RUST_LOG=fabstir_llm_node::api=debug cargo run

# Monitor chain-specific logs
cargo run 2>&1 | grep -E "chain_id|Base Sepolia|opBNB"
```

### Environment Variables

Ensure these are properly set:

```bash
# Required for multi-chain support
CHAIN_ID=84532                    # Default chain (Base Sepolia)
RPC_URL=https://sepolia.base.org  # Default RPC endpoint

# Contract addresses (from .env.contracts)
JOB_MARKETPLACE_ADDRESS=0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E
NODE_REGISTRY_ADDRESS=0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6

# Node wallet (must have gas on both chains)
HOST_PRIVATE_KEY=0x...

# API configuration
API_PORT=8080
P2P_PORT=9000

# Model path
MODEL_PATH=./models/tinyllama-1b.Q4_K_M.gguf
```

### Getting Help

If issues persist after trying these solutions:

1. Check logs with `RUST_LOG=debug` for detailed error messages
2. Verify all contract addresses match deployment
3. Ensure node wallet has sufficient gas on target chain
4. Review chain-specific examples in `docs/CHAIN_EXAMPLES.md`
5. File an issue with chain_id, error message, and debug logs

---

## Support

For issues, feature requests, or questions about the API:

1. Check this documentation for common solutions
2. Review the [GitHub Issues](https://github.com/fabstir/fabstir-llm-node/issues)
3. Contact the development team through official channels

---

## License

This API is part of the Fabstir LLM Node project. See the project LICENSE file for details.