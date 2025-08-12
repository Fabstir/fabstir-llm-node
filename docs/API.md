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