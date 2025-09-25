# Multi-Chain API Examples

This document provides practical examples of using the Fabstir LLM Node API with different blockchain networks.

## Supported Chains

| Chain | Chain ID | Native Token | Network Type |
|-------|----------|--------------|--------------|
| Base Sepolia | 84532 | ETH | Testnet |
| opBNB Testnet | 5611 | BNB | Testnet |

## Base Sepolia Examples

### List Models on Base Sepolia
```bash
curl -X GET "http://localhost:8080/v1/models?chain_id=84532"
```

Response:
```json
{
  "models": [
    {
      "id": "0x1234...abcd",
      "name": "tinyllama-1b.Q4_K_M.gguf",
      "huggingface_repo": "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
      "sha256_hash": "45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6",
      "approval_tier": 1,
      "description": "TinyLlama 1.1B Chat model"
    }
  ],
  "chain_id": 84532,
  "chain_name": "Base Sepolia"
}
```

### Submit Inference Request on Base Sepolia
```bash
curl -X POST "http://localhost:8080/v1/inference" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tinyllama",
    "prompt": "What is Ethereum?",
    "max_tokens": 100,
    "chain_id": 84532,
    "job_id": 123,
    "request_id": "base-req-001"
  }'
```

Response:
```json
{
  "model": "tinyllama",
  "content": "Ethereum is a decentralized blockchain platform...",
  "tokens_used": 85,
  "finish_reason": "complete",
  "request_id": "base-req-001",
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "native_token": "ETH"
}
```

### WebSocket Connection with Base Sepolia
```javascript
const ws = new WebSocket('ws://localhost:8080/v1/ws');

ws.onopen = () => {
  // Authenticate with Base Sepolia job
  ws.send(JSON.stringify({
    type: 'auth',
    job_id: 123,
    chain_id: 84532
  }));

  // Send inference request
  ws.send(JSON.stringify({
    type: 'inference',
    request: {
      model: 'tinyllama',
      prompt: 'Explain Base L2 scaling',
      max_tokens: 150,
      stream: true,
      chain_id: 84532
    }
  }));
};

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  if (data.type === 'stream_chunk') {
    console.log('Chain:', data.chain_name, '- Token:', data.native_token);
    console.log('Content:', data.content);
  }
};
```

## opBNB Testnet Examples

### List Models on opBNB Testnet
```bash
curl -X GET "http://localhost:8080/v1/models?chain_id=5611"
```

Response:
```json
{
  "models": [
    {
      "id": "0x5678...efgh",
      "name": "llama-2-7b.Q4_K_M.gguf",
      "huggingface_repo": "TheBloke/Llama-2-7B-Chat-GGUF",
      "sha256_hash": "abc123...",
      "approval_tier": 1,
      "description": "Llama 2 7B Chat model"
    }
  ],
  "chain_id": 5611,
  "chain_name": "opBNB Testnet"
}
```

### Submit Inference Request on opBNB
```bash
curl -X POST "http://localhost:8080/v1/inference" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama-2-7b",
    "prompt": "What is BNB Smart Chain?",
    "max_tokens": 100,
    "chain_id": 5611,
    "job_id": 456,
    "request_id": "opbnb-req-001"
  }'
```

Response:
```json
{
  "model": "llama-2-7b",
  "content": "BNB Smart Chain is a blockchain network...",
  "tokens_used": 92,
  "finish_reason": "complete",
  "request_id": "opbnb-req-001",
  "chain_id": 5611,
  "chain_name": "opBNB Testnet",
  "native_token": "BNB"
}
```

## Chain Statistics Examples

### Get All Chain Statistics
```bash
curl -X GET "http://localhost:8080/v1/chains/stats"
```

Response:
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

### Get Specific Chain Statistics
```bash
# Base Sepolia stats
curl -X GET "http://localhost:8080/v1/chains/84532/stats"

# opBNB Testnet stats
curl -X GET "http://localhost:8080/v1/chains/5611/stats"
```

## Session Management Examples

### Get Session Information
```bash
curl -X GET "http://localhost:8080/v1/session/123/info"
```

Response:
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

## Streaming with Different Chains

### Base Sepolia Streaming Request
```bash
curl -X POST "http://localhost:8080/v1/inference" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
    "model": "tinyllama",
    "prompt": "Explain Layer 2 scaling",
    "max_tokens": 200,
    "stream": true,
    "chain_id": 84532
  }'
```

Streaming Response:
```
data: {"content": "Layer", "tokens": 1, "finish_reason": null, "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}

data: {"content": " 2", "tokens": 2, "finish_reason": null, "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}

data: {"content": " scaling", "tokens": 3, "finish_reason": null, "chain_id": 84532, "chain_name": "Base Sepolia", "native_token": "ETH"}
```

### opBNB Testnet Streaming Request
```bash
curl -X POST "http://localhost:8080/v1/inference" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
    "model": "llama-2-7b",
    "prompt": "What are the benefits of opBNB?",
    "max_tokens": 150,
    "stream": true,
    "chain_id": 5611
  }'
```

## Error Handling with Chain Context

### Invalid Chain ID
```bash
curl -X GET "http://localhost:8080/v1/models?chain_id=999999"
```

Error Response:
```json
{
  "error_type": "invalid_chain",
  "message": "Unsupported chain ID: 999999",
  "request_id": "req-error-001",
  "chain_id": 999999,
  "details": {
    "supported_chains": [84532, 5611]
  }
}
```

### Chain-Specific Model Not Found
```bash
curl -X POST "http://localhost:8080/v1/inference" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "prompt": "Hello",
    "max_tokens": 10,
    "chain_id": 84532
  }'
```

Error Response:
```json
{
  "error_type": "model_not_found",
  "message": "Model 'gpt-4' not available on Base Sepolia",
  "request_id": "req-error-002",
  "chain_id": 84532,
  "details": {
    "chain_name": "Base Sepolia",
    "available_models": ["tinyllama", "llama-2-7b"]
  }
}
```

## Python Client Example

```python
import requests
import json

class FabstirLLMClient:
    def __init__(self, base_url="http://localhost:8080"):
        self.base_url = base_url

    def list_chains(self):
        """Get all supported chains"""
        response = requests.get(f"{self.base_url}/v1/chains")
        return response.json()

    def list_models(self, chain_id=84532):
        """List models for a specific chain"""
        response = requests.get(
            f"{self.base_url}/v1/models",
            params={"chain_id": chain_id}
        )
        return response.json()

    def inference(self, prompt, model="tinyllama", chain_id=84532, max_tokens=100):
        """Submit inference request on specific chain"""
        payload = {
            "model": model,
            "prompt": prompt,
            "max_tokens": max_tokens,
            "chain_id": chain_id,
            "request_id": f"py-req-{chain_id}-001"
        }

        response = requests.post(
            f"{self.base_url}/v1/inference",
            json=payload
        )
        return response.json()

    def get_chain_stats(self, chain_id=None):
        """Get statistics for specific chain or all chains"""
        if chain_id:
            url = f"{self.base_url}/v1/chains/{chain_id}/stats"
        else:
            url = f"{self.base_url}/v1/chains/stats"

        response = requests.get(url)
        return response.json()

# Usage example
client = FabstirLLMClient()

# List all chains
chains = client.list_chains()
print(f"Supported chains: {chains}")

# Get models on Base Sepolia
models = client.list_models(chain_id=84532)
print(f"Base Sepolia models: {models}")

# Submit inference on Base Sepolia
result = client.inference(
    prompt="What is Ethereum?",
    chain_id=84532,
    max_tokens=50
)
print(f"Base Sepolia response: {result}")

# Submit inference on opBNB Testnet
result = client.inference(
    prompt="What is BNB?",
    model="llama-2-7b",
    chain_id=5611,
    max_tokens=50
)
print(f"opBNB response: {result}")

# Get chain statistics
stats = client.get_chain_stats()
print(f"All chain stats: {stats}")
```

## JavaScript/TypeScript Client Example

```typescript
interface ChainConfig {
  chainId: number;
  name: string;
  nativeToken: string;
}

interface InferenceRequest {
  model: string;
  prompt: string;
  maxTokens: number;
  chainId?: number;
  jobId?: number;
  stream?: boolean;
}

class FabstirLLMClient {
  private baseUrl: string;
  private chains: Map<number, ChainConfig> = new Map([
    [84532, { chainId: 84532, name: 'Base Sepolia', nativeToken: 'ETH' }],
    [5611, { chainId: 5611, name: 'opBNB Testnet', nativeToken: 'BNB' }]
  ]);

  constructor(baseUrl = 'http://localhost:8080') {
    this.baseUrl = baseUrl;
  }

  async listModels(chainId = 84532) {
    const response = await fetch(`${this.baseUrl}/v1/models?chain_id=${chainId}`);
    return response.json();
  }

  async inference(request: InferenceRequest) {
    const chainId = request.chainId || 84532;
    const chain = this.chains.get(chainId);

    if (!chain) {
      throw new Error(`Unsupported chain ID: ${chainId}`);
    }

    const response = await fetch(`${this.baseUrl}/v1/inference`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': request.stream ? 'text/event-stream' : 'application/json'
      },
      body: JSON.stringify({
        model: request.model,
        prompt: request.prompt,
        max_tokens: request.maxTokens,
        chain_id: chainId,
        job_id: request.jobId,
        stream: request.stream || false
      })
    });

    if (request.stream) {
      return this.handleStreamResponse(response, chain);
    }

    return response.json();
  }

  private async handleStreamResponse(response: Response, chain: ChainConfig) {
    const reader = response.body?.getReader();
    const decoder = new TextDecoder();

    if (!reader) throw new Error('No response body');

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      const chunk = decoder.decode(value);
      const lines = chunk.split('\n');

      for (const line of lines) {
        if (line.startsWith('data: ')) {
          const data = JSON.parse(line.slice(6));
          console.log(`[${chain.name}] ${data.content}`);
        }
      }
    }
  }

  async getChainStats(chainId?: number) {
    const url = chainId
      ? `${this.baseUrl}/v1/chains/${chainId}/stats`
      : `${this.baseUrl}/v1/chains/stats`;

    const response = await fetch(url);
    return response.json();
  }
}

// Usage
const client = new FabstirLLMClient();

// Base Sepolia inference
client.inference({
  model: 'tinyllama',
  prompt: 'Explain Ethereum',
  maxTokens: 100,
  chainId: 84532
}).then(result => {
  console.log('Base Sepolia result:', result);
});

// opBNB Testnet inference
client.inference({
  model: 'llama-2-7b',
  prompt: 'Explain BNB Chain',
  maxTokens: 100,
  chainId: 5611
}).then(result => {
  console.log('opBNB result:', result);
});

// Stream response on Base Sepolia
client.inference({
  model: 'tinyllama',
  prompt: 'What is Layer 2 scaling?',
  maxTokens: 200,
  chainId: 84532,
  stream: true
});
```

## Notes

- Default chain is Base Sepolia (84532) when `chain_id` is not specified
- Each chain has its own set of contract addresses for job management
- Native tokens differ by chain: ETH for Base Sepolia, BNB for opBNB Testnet
- Model availability may vary by chain based on registry approval
- Session tracking is chain-specific to ensure proper settlement