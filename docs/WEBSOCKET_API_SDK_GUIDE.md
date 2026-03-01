# WebSocket API & SDK Integration Guide

## Overview

This document describes the current state of the fabstir-llm-node WebSocket API implementation and provides guidance for SDK developers working with TypeScript/JavaScript to integrate with the node's capabilities. This covers all work completed from Sub-phase 8.7 through 8.12 in this session.

## Current Implementation Status (Updated March 2026, v8.22.4)

### ✅ Phase 8.18: WebSocket Integration with Main HTTP Server (COMPLETED)
- **WebSocket endpoint now available at `/v1/ws`**
- **Integrated with main Axum HTTP server on port 8080**
- **Streaming inference with proof generation**
- **Direct SDK connection support**

### ✅ Phase 8.7: WebSocket Server Implementation
- **Production WebSocket Server**: Full async server with Axum integration
- **Connection Management**: Handles 1000+ concurrent connections
- **Lifecycle Management**: Proper connection establishment and cleanup
- **Health Monitoring**: Ping/pong keep-alive mechanism
- **Message Framing**: WebSocket protocol handling with tokio

### ✅ Phase 8.8: Protocol Message Types and Handlers
- **Message Types**: Aligned with Fabstir SDK protocol specification
- **Session Handlers**: session_init, session_resume, prompt, response, error
- **Context Loading**: Full conversation history support
- **Recovery Support**: Seamless session recovery after disconnects
- **Streaming Responses**: Token-by-token streaming capability

### ✅ Phase 8.9: Stateless Memory Cache and Inference Integration
- **In-Memory Cache**: Stateless conversation caching (no persistence)
- **LLM Integration**: Connected to llama-cpp-2 for inference
- **Context Window Management**: Automatic truncation to model limits
- **Token Counting**: Accurate token usage tracking
- **Cache Eviction**: Smart policies for memory management
- **Streaming Generation**: Real-time token streaming

### ✅ Phase 8.10: Production Hardening and Monitoring
- **Message Compression**: Gzip/deflate reducing bandwidth >40%
- **Rate Limiting**: Token bucket algorithm (100 req/min default)
- **Authentication**: Job-based verification system
- **Prometheus Metrics**: Performance monitoring structure
- **Health Checks**: Circuit breakers and dependency monitoring
- **Configuration Management**: Hot-reloadable production config

### ✅ Phase 8.12: Security & Monitoring (Latest in This Session)
- **JWT Authentication**: Real JWT tokens using HS256 algorithm
- **Ed25519 Signatures**: Cryptographic message signing and verification
- **Session Management**: Token-based session tracking with expiry
- **Permission System**: Role-based access control (User, Host, Admin)

### ✅ Phase 8.13: Automatic Payment Settlement on Disconnect (v5+, September 2024)
- **Automatic Settlement**: WebSocket disconnect triggers `completeSessionJob()`
- **Payment Distribution**: Host receives 90%, Treasury 10%, User gets refund
- **No User Action Required**: Payments settle even on unexpected disconnects
- **Session Cleanup**: Token trackers and state cleared after settlement
- **Blockchain Integration**: Direct contract calls to JobMarketplace (0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4 - AUDIT-F4 compliant)

### ✅ Phase 8.16: Encrypted WebSocket Image Generation (v8.16.0+)
- **Text-to-Image**: FLUX.2 Klein 4B via SGLang Diffusion sidecar
- **End-to-End Encrypted**: Same XChaCha20-Poly1305 channel as inference
- **Routing**: `"action": "image_generation"` in decrypted JSON payload
- **Safety**: Three-layer content safety pipeline (keyword filter active)
- **Billing**: Megapixel-steps formula with per-session rate limiting
- **Auto-Routing (v8.16.1+)**: `AUTO_IMAGE_ROUTING=true` auto-detects image intent from chat and routes to diffusion sidecar
- **89+ tests passing** across diffusion module

### ✅ Phase 8.17: Thinking/Reasoning Mode + Contract Fixes (v8.17.0–v8.17.6)
- **Thinking Mode (v8.17.0)**: Per-request `thinking` field (`"enabled"`, `"disabled"`, `"low"`, `"medium"`, `"high"`)
  - Harmony template: maps to `Reasoning: none/low/medium/high` in system prompt
  - GLM-4 template: maps to `/think` prefix on last user message when explicitly enabled
  - `DEFAULT_THINKING_MODE` env var for global default (explicit per-request overrides)
- **v8.22.4 BREAKING**: GLM-4 auto `/think` injection **removed** — was causing degenerate meta-reasoning loops on multi-turn conversations. `/think` must now be explicitly requested via the SDK `thinking` field.
- **New JobMarketplace Proxy (v8.17.4)**: `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4`
- **Dispute Window Fix (v8.17.5)**: Dispute window now queried from contract `disputeWindow()` at startup

### ✅ Phase 8.18: setTokenPricing After Registration (v8.18.0)
- **Two-Step Registration**: `registerNode()` then `setTokenPricing(usdcAddress, price)` — node handles automatically
- **TOKEN_PRICING_USDC env var**: Override USDC per-token price (default: 10,000 = $10/M tokens)

### ✅ Phase 8.19: Stream Cancellation + True Streaming (v8.19.0–v8.19.1)
- **Stream Cancel (v8.19.0)**: `stream_cancel` WebSocket message stops inference mid-stream
- **True Streaming (v8.19.1)**: Tokens stream to client as generated (no batch-then-deliver delay)
- **`stream_end` messages** now include `reason` (complete/cancelled/error) and `tokens_used` fields

### ✅ Phase 8.20: Per-Model Token Pricing (v8.20.0)
- **BREAKING**: `setTokenPricing(address,uint256)` removed — use `setModelTokenPricing(bytes32,address,uint256)`
- Per-model per-token pricing set in a loop for each model x each token (native + USDC)

### ✅ Phase 8.21: Context Usage + Penalties + Sampler Fixes (v8.21.0–v8.21.5)
- **Context Usage (v8.21.0)**: Responses include `usage` object with `prompt_tokens`, `completion_tokens`, `total_tokens`, `context_window_size`
- **Configurable Penalties (v8.21.3)**: `REPEAT_PENALTY`, `FREQUENCY_PENALTY`, `PRESENCE_PENALTY`, `PENALTY_LAST_N` env vars
- **Sampler Chain Persistence (v8.21.5)**: Penalties now correctly persist across tokens

### ✅ Phase 8.22: GLM-4 Fixes (v8.22.0–v8.22.4)
- **System Prompt Fix (v8.22.0)**: Simplified GLM-4 default system prompt (removed RAG instruction that caused hallucinations)
- **`<|endoftext|>` Stop Token (v8.22.3)**: Added EOS stop token matching Ollama's GLM-4 template
- **Sampler Reset (v8.22.1–v8.22.2)**: Sampler penalties reset after `</think>` block closes
- **Disable Auto `/think` (v8.22.4)**: Removed auto `/think` injection for GLM-4 (see above)

### ⚠️ Phase 8.11: Core Functionality (Skipped - To Be Done)
- Real blockchain job verification (currently using mock)
- Full production inference engine connection (partially mocked)

## WebSocket Protocol Implementation

### Connection Endpoint
```
ws://[host-address]:8080/v1/ws
```

**Note**: The WebSocket endpoint has been integrated into the main HTTP server at `/v1/ws` (Sub-phase 8.18 completed).

### Authentication Flow

#### 1. Initial Connection
```typescript
// SDK establishes WebSocket connection
const ws = new WebSocket('ws://host:8080/v1/ws');

// For simple inference (no auth required currently)
const inferenceMessage = {
  type: 'inference',
  request: {
    model: 'tinyllama',  // or any available model
    prompt: 'Your prompt here',
    max_tokens: 100,
    stream: true
  }
};
ws.send(JSON.stringify(inferenceMessage));

// For authenticated sessions (future implementation)
const authMessage = {
  type: 'auth',
  job_id: 12345,  // From blockchain job creation
  token: 'session_token_here'  // Optional if using JWT
};
// ws.send(JSON.stringify(authMessage));
```

#### 2. JWT Token Structure
The node generates JWT tokens with the following claims:
```typescript
interface JwtClaims {
  session_id: string;      // Unique session identifier
  job_id: number;          // Blockchain job ID
  permissions: string[];   // ['Read', 'Write', 'Execute', 'Admin']
  exp: number;            // Expiry timestamp (default: 1 hour)
  iat: number;            // Issued at timestamp
}
```

#### 3. Signature Verification
For high-security sessions, messages can be signed with Ed25519:
```typescript
interface SignedMessage {
  type: 'signed_prompt',
  session_id: string,
  content: string,
  signature: string,  // Hex-encoded Ed25519 signature
  timestamp: number
}
```

### Conversation Management Protocol

#### Session Initialization (New Conversation)
```typescript
const initMessage = {
  type: 'session_init',
  session_id: generateUUID(),
  job_id: 12345,
  model_config: {
    model: 'llama-2-7b',
    max_tokens: 2048,
    temperature: 0.7
  }
};
```

#### Session Resume (After Disconnect)
```typescript
const resumeMessage = {
  type: 'session_resume',
  session_id: 'existing-uuid',
  job_id: 12345,
  conversation_context: [
    { role: 'user', content: 'Previous question...' },
    { role: 'assistant', content: 'Previous response...' }
    // Full conversation history from S5 storage
  ],
  last_message_index: 8
};
```

#### Sending Prompts (Active Session)
```typescript
// During active session, only send new prompt
const promptMessage = {
  type: 'prompt',
  session_id: 'active-session-uuid',
  content: 'What is machine learning?',
  message_index: 5,  // For ordering verification
  stream: true       // Enable token streaming
};
```

#### Receiving Responses
```typescript
// Non-streaming response
{
  type: 'response',
  session_id: 'active-session-uuid',
  content: 'Machine learning is...',
  tokens_used: 45,
  message_index: 6,
  completion_time_ms: 1234
}

// Streaming response chunks
{
  type: 'stream_chunk',
  session_id: 'active-session-uuid',
  content: 'Machine',  // Partial token
  chunk_index: 0,
  is_final: false
}
```

### Error Handling

```typescript
interface ErrorMessage {
  type: 'error',
  error_code: string,
  message: string,
  details?: any
}

// Common error codes
- 'AUTH_FAILED': Authentication/authorization failure
- 'RATE_LIMIT': Rate limit exceeded
- 'SESSION_EXPIRED': Session token expired
- 'INVALID_JOB': Job verification failed
- 'MODEL_UNAVAILABLE': Requested model not available or host not authorized for model (v8.14.0+)
- 'MODEL_UNAUTHORIZED': Host not authorized to run requested model (see model validation)
- 'CONTEXT_TOO_LARGE': Conversation context exceeds limits
```

### Model Validation (v8.14.0+)

As of v8.14.0, hosts enforce model authorization:

- **Startup**: Host validates MODEL_PATH matches registered model with SHA256 verification
- **Job Claiming**: Hosts only claim jobs for models they're registered for
- **Runtime**: Inference requests verified against loaded model

**SDK Impact**: None - this is transparent server-side security. If you request a model the host doesn't support, the job will remain unclaimed and eventually timeout. For better UX, use pre-flight validation:

```typescript
// Check if host supports model before creating job
const supports = await nodeRegistry.nodeSupportsModel(hostAddress, modelId);
if (!supports) {
  throw new Error('Host does not support this model');
}
```

See `docs/sdk-reference/MODEL-VALIDATION-SDK-COMPATIBILITY.md` for complete SDK integration guide.

## Stateless Memory Cache Architecture

### How It Works

1. **Session Start**: Host allocates memory cache for conversation
2. **During Session**: Host maintains conversation in memory only
3. **On Disconnect**:
   - Automatic payment settlement via `completeSessionJob()` (v5+)
   - Host earnings distributed to HostEarnings contract
   - Unused deposit refunded to user
   - Memory automatically cleared
4. **On Resume**: Full context rebuilt from user-provided history

### SDK Responsibilities

```typescript
class ConversationManager {
  private s5Storage: S5Client;
  private conversationHistory: Message[] = [];
  
  async saveMessage(message: Message) {
    // Always persist to S5 immediately
    this.conversationHistory.push(message);
    await this.s5Storage.save(this.sessionId, this.conversationHistory);
  }
  
  async resumeSession(hostEndpoint: string) {
    // Load full history from S5
    const history = await this.s5Storage.load(this.sessionId);
    
    // Send to new host with full context
    const ws = new WebSocket(hostEndpoint);
    ws.send(JSON.stringify({
      type: 'session_resume',
      session_id: this.sessionId,
      conversation_context: history
    }));
  }
}
```

### Host Memory Management

The host maintains conversation cache with these constraints:
- **Token Limit**: Automatically truncates to last N messages based on model context window
- **Time Limit**: Sessions expire after inactivity (default: 30 minutes)
- **Memory Limit**: Per-session memory cap (default: 10MB)
- **Cleanup**: Automatic garbage collection on disconnect

## Payment Settlement on Disconnect (Critical for SDK Developers)

**Automatic Settlement (v5+, September 2024)**

When a WebSocket connection closes for ANY reason:
- Browser tab closed
- Network disconnection
- Client crash
- Explicit `session_end` message

The node automatically:
1. Submits any pending checkpoints (100+ token batches)
2. Calls `completeSessionJob()` on the blockchain
3. Triggers payment distribution:
   - 90% to HostEarnings contract (0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0)
   - 10% to Treasury
   - Unused deposit back to user

**SDK Implications:**
- Users don't need to explicitly end sessions for payment
- Payments are guaranteed even on unexpected disconnects
- The `session_end` message is optional (for clean shutdown)
- Monitor blockchain events for payment confirmation

**Monitoring Settlement in SDK:**
```javascript
// Listen for SessionCompleted event on blockchain
const filter = jobMarketplace.filters.SessionCompleted(jobId);
jobMarketplace.on(filter, (jobId, host, tokensUsed, event) => {
  console.log(`Session ${jobId} automatically settled: ${tokensUsed} tokens`);
  console.log(`Transaction hash: ${event.transactionHash}`);
});
```

**Requirements:**
- Node must have `HOST_PRIVATE_KEY` configured
- Node version v5-payment-settlement or later
- JobMarketplace: 0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4 (v8.13.0+ AUDIT-F4 remediated)
- Host must have called `setTokenPricing(USDC, price)` for ERC20 sessions to work (v8.18.0+, automatic on registration)

## Compression Support

The WebSocket server supports message compression:

```typescript
// SDK: Enable per-message deflate
const ws = new WebSocket('ws://host:8080/ws/session', {
  perMessageDeflate: {
    zlibDeflateOptions: {
      level: zlib.Z_BEST_COMPRESSION
    },
    threshold: 1024  // Only compress messages > 1KB
  }
});
```

## Rate Limiting

Current implementation:
- **Default Limit**: 100 requests per minute
- **Burst Capacity**: 200 tokens
- **Per Session**: Each session_id has independent limits

```typescript
// SDK: Handle rate limit errors
ws.on('message', (data) => {
  const msg = JSON.parse(data);
  if (msg.type === 'error' && msg.error_code === 'RATE_LIMIT') {
    // Implement exponential backoff
    const retryAfter = msg.details?.retry_after_ms || 1000;
    setTimeout(() => retryPrompt(), retryAfter);
  }
});
```

## Health Monitoring

The node exposes health endpoints:

```typescript
// Check node health
GET http://host:8080/health

Response:
{
  "status": "healthy",  // or "degraded", "unhealthy"
  "websocket_connections": 15,
  "active_sessions": 12,
  "memory_usage_mb": 234,
  "uptime_seconds": 3600
}

// Check readiness for new connections
GET http://host:8080/ready

Response: 
{
  "ready": true,
  "accepting_connections": true,
  "model_loaded": true,
  "circuit_breaker_status": "closed"
}
```

## Security Best Practices for SDK

### 1. Token Management
```typescript
class TokenManager {
  private token?: string;
  private expiresAt?: number;
  
  async getValidToken(): Promise<string> {
    if (!this.token || Date.now() >= this.expiresAt) {
      // Refresh token before expiry
      this.token = await this.refreshToken();
    }
    return this.token;
  }
}
```

### 2. Message Signing (Optional High Security)
```typescript
import { sign } from 'tweetnacl';

class SecureMessaging {
  private keypair: Uint8Array;
  
  signMessage(content: string): string {
    const message = new TextEncoder().encode(content);
    const signature = sign.detached(message, this.keypair);
    return Buffer.from(signature).toString('hex');
  }
}
```

### 3. Connection Recovery
```typescript
class ResilientWebSocket {
  private ws?: WebSocket;
  private reconnectAttempts = 0;
  
  connect() {
    this.ws = new WebSocket(this.endpoint);
    
    this.ws.on('close', () => {
      // Exponential backoff reconnection
      const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
      setTimeout(() => this.connect(), delay);
      this.reconnectAttempts++;
    });
    
    this.ws.on('open', () => {
      this.reconnectAttempts = 0;
      // Resume session with full context
      this.resumeSession();
    });
  }
}
```

## Performance Optimization Tips

### 1. Context Management
```typescript
// Trim conversation to essential context
function optimizeContext(messages: Message[], maxTokens = 2048): Message[] {
  let tokenCount = 0;
  const optimized = [];
  
  // Keep most recent messages within token budget
  for (let i = messages.length - 1; i >= 0; i--) {
    const msgTokens = estimateTokens(messages[i].content);
    if (tokenCount + msgTokens > maxTokens) break;
    optimized.unshift(messages[i]);
    tokenCount += msgTokens;
  }
  
  return optimized;
}
```

### 2. Batching Requests
```typescript
// Send multiple prompts efficiently
const batchMessage = {
  type: 'batch_prompt',
  session_id: 'uuid',
  prompts: [
    { content: 'Question 1', id: 'q1' },
    { content: 'Question 2', id: 'q2' }
  ]
};
```

### 3. Streaming Responses
```typescript
// Handle streaming for better UX
let fullResponse = '';
ws.on('message', (data) => {
  const msg = JSON.parse(data);
  if (msg.type === 'stream_chunk') {
    fullResponse += msg.content;
    updateUI(fullResponse);  // Progressive rendering
    
    if (msg.is_final) {
      saveToS5(fullResponse);
    }
  }
});
```

## Migration from HTTP to WebSocket

If currently using HTTP endpoints, here's the migration path:

### Old HTTP Approach
```typescript
// Stateless, full context every request
const response = await fetch('http://host:8080/v1/inference', {
  method: 'POST',
  body: JSON.stringify({
    prompt: 'Question',
    conversation_context: entireHistory  // Inefficient
  })
});
```

### New WebSocket Approach
```typescript
// Stateful during session, efficient
class WebSocketClient {
  private ws: WebSocket;
  private sessionActive = false;
  
  async initialize(jobId: number) {
    this.ws = new WebSocket('ws://host:8080/ws/session');
    
    await this.waitForOpen();
    
    // Send init once
    this.ws.send(JSON.stringify({
      type: 'session_init',
      job_id: jobId,
      session_id: generateUUID()
    }));
    
    this.sessionActive = true;
  }
  
  async sendPrompt(content: string) {
    // Only send new prompt, not entire history
    this.ws.send(JSON.stringify({
      type: 'prompt',
      content: content
    }));
  }
}
```

## Testing Your SDK Integration

### 1. Unit Tests
```typescript
describe('WebSocket Integration', () => {
  it('should handle session initialization', async () => {
    const client = new WebSocketClient();
    await client.connect('ws://localhost:8080/ws/session');
    
    const response = await client.initialize(12345);
    expect(response.type).toBe('session_ready');
  });
  
  it('should recover from disconnection', async () => {
    const client = new WebSocketClient();
    await client.connect();
    
    // Simulate disconnect
    client.ws.close();
    
    // Should auto-reconnect and resume
    await sleep(2000);
    expect(client.isConnected()).toBe(true);
  });
});
```

### 2. Integration Tests
```typescript
describe('End-to-End Conversation', () => {
  it('should maintain context across prompts', async () => {
    const client = new WebSocketClient();
    await client.initialize(12345);
    
    // First prompt
    const response1 = await client.sendPrompt('What is AI?');
    expect(response1.content).toContain('artificial intelligence');
    
    // Follow-up using context
    const response2 = await client.sendPrompt('Tell me more');
    expect(response2.content).toContain('AI');  // Should reference previous
  });
});
```

## Known Limitations and Production Status

### Currently Using Mocks (Phase 8.11 Pending)
1. **Job Verification**: Mock verifier accepts specific test job IDs (12345, etc.)
2. **Blockchain Integration**: Not yet verifying on-chain job status

### Production Ready Features (Phases 8.7-8.12 Complete)
1. **WebSocket Server**: ✅ Fully functional with 1000+ concurrent connections
2. **Protocol Handlers**: ✅ All message types implemented per SDK spec
3. **Memory Cache**: ✅ Stateless session caching with auto-eviction
4. **LLM Integration**: ✅ Connected to llama-cpp-2 for real inference
5. **Context Management**: ✅ Automatic window sizing and token counting
6. **Streaming**: ✅ Token-by-token response streaming
7. **Compression**: ✅ Gzip/deflate reducing bandwidth >40%
8. **Rate Limiting**: ✅ Token bucket with configurable limits
9. **JWT Auth**: ✅ Real cryptographic tokens with HS256
10. **Ed25519 Signatures**: ✅ Cryptographic signing and verification
11. **Health Monitoring**: ✅ Circuit breakers and health checks
12. **Metrics Structure**: ✅ Prometheus-compatible (export pending)

## Implementation Timeline Summary

### What Was Built in This Session (Phases 8.7-8.12)

1. **Phase 8.7**: Built production WebSocket server from scratch
2. **Phase 8.8**: Implemented all protocol message types per SDK specification  
3. **Phase 8.9**: Added stateless memory cache and LLM integration
4. **Phase 8.10**: Hardened for production with compression, rate limiting, auth
5. **Phase 8.12**: Added JWT and Ed25519 cryptographic security

### Next Steps for SDK Development

#### Immediate Priorities (SDK can implement now)
1. **Implement WebSocket client** with reconnection logic
2. **Add S5 storage integration** for conversation persistence
3. **Build conversation manager** for stateless context handling
4. **Create JWT token handling** with refresh mechanism
5. **Add streaming response handler** for real-time UX
6. **Implement message compression** support (gzip/deflate)
7. **Add rate limit handling** with exponential backoff

#### After Phase 8.11 Completion
1. **Blockchain job verification** will be connected
2. **Payment verification** through smart contracts
3. **Full production deployment** readiness

## Support and Resources

- **Node API Docs**: `/workspace/docs/node-reference/API.md`
- **Contract Integration**: `/workspace/docs/compute-contracts-reference/`
- **Test Accounts**: See `.env.test.local` for Base Sepolia test accounts
- **WebSocket Tests**: `/workspace/tests/websocket/` for reference implementations
- **Model Validation**: `/workspace/docs/sdk-reference/MODEL-VALIDATION-SDK-COMPATIBILITY.md` for SDK compatibility guide

## Appendix: Message Type Reference

### Client → Server Messages
- `auth`: Initial authentication
- `session_init`: Start new conversation
- `session_resume`: Resume with context
- `prompt`: Send user prompt
- `batch_prompt`: Multiple prompts
- `session_end`: Clean termination
- `encrypted_message` (action: `image_generation`): Generate image over encrypted channel (v8.16.0+)

### Server → Client Messages
- `session_ready`: Initialization complete
- `response`: Non-streaming answer
- `stream_chunk`: Streaming token
- `error`: Error notification
- `token_refresh`: New JWT token
- `rate_limit`: Rate limit warning
- `encrypted_response` (type: `image_generation_result`): Encrypted generated image (v8.16.0+)
- `encrypted_response` (type: `image_generation_error`): Encrypted generation error (v8.16.0+)

### Error Codes
- `AUTH_FAILED`: Authentication failure
- `RATE_LIMIT`: Too many requests
- `SESSION_EXPIRED`: Token expired
- `INVALID_JOB`: Job verification failed
- `MODEL_UNAVAILABLE`: Model not loaded
- `MODEL_UNAUTHORIZED`: Host not authorized for requested model (v8.14.0+)
- `CONTEXT_TOO_LARGE`: Exceeds token limit
- `CIRCUIT_OPEN`: Service temporarily unavailable
- `RATE_LIMIT_EXCEEDED`: Image generation rate limit hit (v8.16.0+)
- `PROMPT_BLOCKED`: Image prompt blocked by safety filter (v8.16.0+)
- `DIFFUSION_SERVICE_UNAVAILABLE`: No diffusion sidecar configured (v8.16.0+)
- `VALIDATION_FAILED`: Invalid image generation parameters (v8.16.0+)
- `IMAGE_GENERATION_FAILED`: Sidecar generation error (v8.16.0+)

## Thinking/Reasoning Mode (v8.17.0+)

The `thinking` field controls model reasoning behavior per-request. Include it in the encrypted message payload alongside `prompt`:

```json
{
  "prompt": "Explain quantum computing",
  "thinking": "high"
}
```

**Valid values**: `"enabled"`, `"disabled"`, `"low"`, `"medium"`, `"high"`

**Per-template behavior**:

| Template | `"enabled"` | `"disabled"` | `"low"` | `"medium"` | `"high"` | Absent |
|----------|------------|-------------|---------|-----------|---------|--------|
| Harmony (GPT-OSS) | `Reasoning: medium` | `Reasoning: none` | `Reasoning: low` | `Reasoning: medium` | `Reasoning: high` | Default `medium` |
| GLM-4 | `/think` prefix | No injection | `/think` | `/think` | `/think` | **No prefix** (v8.22.4) |
| Others | No-op | No-op | No-op | No-op | No-op | No-op |

**Global default**: Set `DEFAULT_THINKING_MODE` env var on the host to apply a default when clients omit the field. Explicit per-request values always override the env var. An empty string `""` is treated as unset.

**User override protection**: If a system message in the conversation context already contains `Reasoning:`, the injection is skipped to respect the user's explicit directive.

**v8.22.4 Breaking Change**: GLM-4 auto `/think` injection has been removed. Previously (v8.17.3–v8.22.3), GLM-4 auto-injected `/think` when no thinking mode was specified. This caused degenerate meta-reasoning loops on multi-turn conversations. To enable thinking on GLM-4, the SDK must now explicitly set `"thinking": "enabled"` in the request.

---

## Image Generation over Encrypted WebSocket (v8.16.0+)

Image generation uses the same encrypted WebSocket channel as inference. The SDK sends an `encrypted_message` containing `"action": "image_generation"` in the decrypted JSON payload. The node routes this to the image generation handler instead of the inference pipeline.

**Auto-Routing (v8.16.1+)**: When `AUTO_IMAGE_ROUTING=true` is set on the host, the node can also detect image generation intent from normal chat messages (e.g., "generate an image of a sunset") and auto-route them to the diffusion sidecar — no `"action"` field required. This is opt-in (default OFF). SDK-side intent detection with explicit `"action": "image_generation"` remains the recommended approach. See the [SDK Image Generation Integration Guide](./sdk-reference/SDK_IMAGE_GENERATION_INTEGRATION.md#auto-routing-v8161) for details.

### Prerequisites

- Active encrypted session (completed `encrypted_session_init` handshake)
- Host must have `DIFFUSION_ENDPOINT` configured (check `/v1/version` for `image-generation` feature flag)

### Sending an Image Generation Request

```typescript
// 1. Build inner payload with "action" routing key
const innerPayload = {
  action: "image_generation",  // Required: routes to image gen handler
  prompt: "A cat astronaut floating in space",
  size: "1024x1024",          // Optional (default: "1024x1024")
  steps: 4,                   // Optional (default: 4)
  seed: 42,                   // Optional (random if omitted)
  negativePrompt: "blurry",   // Optional
  guidanceScale: 3.5,         // Optional (default: 3.5)
  safetyLevel: "strict",      // Optional (default: "strict")
};

// 2. Encrypt with session key
const plaintext = new TextEncoder().encode(JSON.stringify(innerPayload));
const nonce = crypto.getRandomValues(new Uint8Array(24));
const aad = new TextEncoder().encode(`message_${messageIndex}`);
const ciphertext = xchacha20poly1305(sessionKey, nonce).encrypt(plaintext, aad);

// 3. Send as encrypted_message
ws.send(JSON.stringify({
  type: "encrypted_message",
  session_id: sessionId,
  id: `img-${Date.now()}`,
  payload: {
    ciphertextHex: bytesToHex(ciphertext),
    nonceHex: bytesToHex(nonce),
    aadHex: bytesToHex(aad),
  }
}));
```

### Receiving the Response

The node responds with an `encrypted_response`. The AAD for image generation responses is `encrypted_image_response` (different from inference responses).

```typescript
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.type === "encrypted_response") {
    const ct = hexToBytes(msg.payload.ciphertextHex);
    const nonce = hexToBytes(msg.payload.nonceHex);
    const aad = hexToBytes(msg.payload.aadHex);  // "encrypted_image_response"
    const decrypted = xchacha20poly1305(sessionKey, nonce).decrypt(ct, aad);
    const inner = JSON.parse(new TextDecoder().decode(decrypted));

    if (inner.type === "image_generation_result") {
      // Success: inner.image is base64 PNG
      const img = new Image();
      img.src = `data:image/png;base64,${inner.image}`;
      console.log(`Seed: ${inner.seed}, Time: ${inner.processingTimeMs}ms`);
      console.log(`Cost: ${inner.billing.generationUnits} units`);
    } else if (inner.type === "image_generation_error") {
      // Error: inner.error.code and inner.error.message
      console.error(`Error: ${inner.error.code} - ${inner.error.message}`);
    }
  }
};
```

### Error Codes

| Code | Cause | SDK Action |
|------|-------|------------|
| `RATE_LIMIT_EXCEEDED` | > 5 requests/min per session | Show cooldown message |
| `PROMPT_BLOCKED` | Safety filter blocked the prompt | Show safety warning |
| `DIFFUSION_SERVICE_UNAVAILABLE` | No sidecar configured | Hide image gen UI |
| `VALIDATION_FAILED` | Invalid size, empty prompt, etc. | Show validation error |
| `IMAGE_GENERATION_FAILED` | Sidecar error (timeout, OOM) | Retry once, then show error |

### Key Differences from Inference Messages

| Property | Inference | Image Generation |
|----------|-----------|-----------------|
| Routing | Default (no action field) | `"action": "image_generation"` (or auto-detected if `AUTO_IMAGE_ROUTING=true`) |
| Response AAD | Varies | `encrypted_image_response` (fixed) |
| Response type | `stream_chunk` / `response` | `image_generation_result` |
| Rate limit | Connection-level | 5/min per session (sliding window) |

For the full SDK integration guide with TypeScript interfaces, billing formula, safety levels, and UI patterns, see [SDK Image Generation Integration Guide](./sdk-reference/SDK_IMAGE_GENERATION_INTEGRATION.md).

## Host Registration & Token Pricing (v8.18.0+)

As of v8.18.0, host registration is now a two-step process on-chain:

1. `registerNode(metadata, apiUrl, modelIds, minPriceNative, minPriceStable)` — registers the host
2. `setTokenPricing(usdcAddress, price)` — sets per-token ERC20 pricing (required for USDC sessions)

The node handles step 2 automatically after step 1. If `setTokenPricing` fails, the node logs an error but does NOT fail registration. The host can retry via:

```bash
# Manual retry for already-registered hosts
cast send 0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22 \
  "setTokenPricing(address,uint256)" \
  0x036CbD53842c5426634e7929541eC2318f3dCF7e 10000 \
  --private-key $HOST_PRIVATE_KEY \
  --rpc-url "https://sepolia.base.org" --legacy
```

**SDK Impact**: If a host hasn't called `setTokenPricing`, ERC20 (USDC) job creation will fail with `"No token pricing"` revert from the contract. Native ETH sessions are unaffected.

**Environment Variable**: Set `TOKEN_PRICING_USDC` to override the default USDC price (10,000 = $10/M tokens). Must be in range [1, 100,000,000].

---

## Current Working Implementation (February 2026)

### Simple WebSocket Connection Example

The WebSocket endpoint is now live at `/v1/ws`. Here's a working example:

```javascript
// Minimal working example for SDK developers
const WebSocket = require('ws');

async function testInference() {
  const ws = new WebSocket('ws://localhost:8080/v1/ws');
  
  ws.on('open', () => {
    console.log('Connected to WebSocket');
    
    // Send inference request
    const request = {
      type: 'inference',
      request: {
        model: 'tinyllama',
        prompt: 'What is the capital of France?',
        max_tokens: 50,
        stream: true
      }
    };
    
    ws.send(JSON.stringify(request));
  });
  
  ws.on('message', (data) => {
    const msg = JSON.parse(data);
    
    if (msg.type === 'stream_chunk') {
      process.stdout.write(msg.content);  // Print tokens as they arrive
      console.log('\nProof:', msg.proof);  // Proof data included
    } else if (msg.type === 'stream_end') {
      console.log('\nStreaming complete');
      console.log('Final proof:', msg.proof);
      ws.close();
    } else if (msg.type === 'error') {
      console.error('Error:', msg.error);
      ws.close();
    }
  });
  
  ws.on('error', (err) => {
    console.error('WebSocket error:', err);
  });
}

testInference();
```

### Stream Cancellation (v8.19.0+)

To stop an active streaming inference mid-generation, send a `stream_cancel` message:

```json
{
  "type": "stream_cancel",
  "session_id": "your-session-id",
  "reason": "user_cancelled"
}
```

The node will:
1. Stop the LLM generation loop between tokens (zero-latency `AtomicBool` flag)
2. Stop delivering any pre-generated tokens
3. Send `stream_end` with `reason: "cancelled"` and accurate `tokens_used`
4. Free GPU/KV cache resources
5. Reset the cancel flag so the session accepts new prompts immediately

**Example with cancellation:**
```javascript
// Start streaming
ws.send(JSON.stringify({ type: 'inference', request: { prompt: '...', stream: true } }));

// After receiving some chunks, cancel
ws.send(JSON.stringify({ type: 'stream_cancel', session_id: sessionId }));

// Listen for the cancelled stream_end
ws.on('message', (data) => {
  const msg = JSON.parse(data);
  if (msg.type === 'stream_end') {
    console.log(msg.reason);      // "cancelled" or "complete"
    console.log(msg.tokens_used);  // tokens generated before cancel
  }
});
```

**Edge cases:**
- `stream_cancel` with no active stream → safe no-op (silently ignored)
- Multiple `stream_cancel` messages → idempotent (second is a no-op)
- `stream_cancel` is **always plaintext**, even during encrypted sessions
- New prompt after cancel → works immediately, no special handling needed

### Response Format with Proofs

All responses now include cryptographic proof data:

```json
{
  "type": "stream_chunk",
  "content": "Paris",
  "tokens": 1,
  "proof": {
    "proof_type": "EZKL",
    "proof_data": "0xEF...",  // Mock EZKL proof (SHA256 based)
    "model_hash": "sha256:abc123...",
    "timestamp": 1737000000,
    "input_hash": "0x123...",
    "output_hash": "0x456...",
    "parameters": {
      "temperature": 0.7,
      "max_tokens": 50
    }
  }
}
```

### Host Discovery via Smart Contract

Hosts register their API URLs in the NodeRegistry contract:

```javascript
// Get host's WebSocket URL from contract
const registry = new ethers.Contract(registryAddress, ABI, provider);
const apiUrl = await registry.getNodeApiUrl(hostAddress);
// Returns: "http://host.example.com:8080"

// Convert to WebSocket URL
const wsUrl = apiUrl.replace('http://', 'ws://') + '/v1/ws';
// Result: "ws://host.example.com:8080/v1/ws"
```

### Known Limitations

1. **Authentication**: Job-based auth not yet wired (use without auth for now)
2. **Session Management**: Stateless only, no conversation persistence
3. **Proof Verification**: Using mock EZKL proofs (SHA256-based)
4. **Rate Limiting**: Not enforced on WebSocket endpoint yet
5. **Model Validation**: Hosts may reject jobs for unsupported models (v8.14.0+)

### Troubleshooting

**Connection Refused**: Ensure node is running on port 8080
```bash
cargo run --release
```

**No Response**: Check if model is loaded
```bash
curl http://localhost:8080/v1/models
```

**Invalid Message Format**: Use exact JSON structure shown above

**Proof Verification Fails**: Mock proofs won't verify on-chain yet (use for testing only)