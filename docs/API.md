# Fabstir LLM Node API Documentation

## Overview

The Fabstir LLM Node provides a RESTful HTTP API and WebSocket interface for interacting with the P2P LLM marketplace. This API enables clients to request inference from available models, monitor node health, and stream real-time responses.

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

### List Available Models

Retrieve a list of models available on this node.

#### Request

```http
GET /v1/models
```

#### Response

```json
{
  "models": [
    {
      "id": "llama-2-7b",
      "name": "Llama 2 7B",
      "description": "Meta's Llama 2 model with 7 billion parameters"
    },
    {
      "id": "vicuna-13b",
      "name": "Vicuna 13B",
      "description": "Fine-tuned LLaMA model for conversation"
    }
  ]
}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `models` | Array | List of available models |
| `models[].id` | String | Unique model identifier |
| `models[].name` | String | Human-readable model name |
| `models[].description` | String? | Optional model description |

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
  "request_id": "req-12345"
}
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

#### Non-Streaming Response

```json
{
  "model": "llama-2-7b",
  "content": "Quantum computing is a revolutionary approach to computation that harnesses quantum mechanical phenomena...",
  "tokens_used": 245,
  "finish_reason": "complete",
  "request_id": "req-12345"
}
```

#### Streaming Response (SSE)

When `stream: true`, the response is sent as Server-Sent Events:

```http
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive

data: {"content": "Quantum", "tokens_used": 1, "finish_reason": null}

data: {"content": " computing", "tokens_used": 2, "finish_reason": null}

data: {"content": " is", "tokens_used": 3, "finish_reason": null}

data: {"content": "", "tokens_used": 245, "finish_reason": "complete"}
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `model` | String | Model used for generation |
| `content` | String | Generated text content |
| `tokens_used` | Integer | Number of tokens generated |
| `finish_reason` | String | Reason for completion: "complete", "max_tokens", "stop_sequence" |
| `request_id` | String | Request identifier for tracking |

#### Status Codes

- `200 OK` - Successful inference
- `400 Bad Request` - Invalid request parameters
- `404 Not Found` - Model not found
- `429 Too Many Requests` - Rate limit exceeded
- `500 Internal Server Error` - Inference failed
- `503 Service Unavailable` - Node or model unavailable

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

## WebSocket API

For real-time bidirectional communication, connect via WebSocket.

### Connection

```javascript
ws://localhost:8080/v1/ws
```

### Message Format

#### Client Request

```json
{
  "type": "inference_request",
  "payload": {
    "model": "llama-2-7b",
    "prompt": "Write a haiku about programming",
    "max_tokens": 50,
    "temperature": 0.9
  }
}
```

#### Server Response

```json
{
  "type": "inference_response",
  "payload": {
    "content": "Code flows like water",
    "tokens_used": 5,
    "finish_reason": null
  }
}
```

#### Connection Maintenance

- Ping interval: 30 seconds
- Pong timeout: 10 seconds
- Automatic reconnection recommended on disconnect

### Message Types

| Type | Direction | Description |
|------|-----------|-------------|
| `inference_request` | Client → Server | Request inference |
| `inference_response` | Server → Client | Streaming response chunk |
| `error` | Server → Client | Error message |
| `ping` | Bidirectional | Keep-alive |
| `pong` | Bidirectional | Keep-alive response |

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
```

### WebSocket Client (JavaScript)

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

- **v1** (Current) - Initial API release with core inference capabilities

---

## Support

For issues, feature requests, or questions about the API:

1. Check this documentation for common solutions
2. Review the [GitHub Issues](https://github.com/fabstir/fabstir-llm-node/issues)
3. Contact the development team through official channels

---

## License

This API is part of the Fabstir LLM Node project. See the project LICENSE file for details.