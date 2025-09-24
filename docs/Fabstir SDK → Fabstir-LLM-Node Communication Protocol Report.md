Fabstir SDK → Fabstir-LLM-Node Communication Protocol Report

  Overview

  This report details how the Fabstir SDK (TypeScript) communicates with fabstir-llm-node (Rust) for LLM inference, maintaining user
  data ownership while leveraging stateless host infrastructure.

  Core Architecture Principles

  1. User Data Ownership: Users store all conversation data in their decentralized S5 storage
  2. Stateless Hosts: Hosts maintain conversation only in memory during active sessions
  3. Resilience: Full recovery capability if host crashes mid-session
  4. Privacy: Hosts never persist user conversations

  Communication Protocols

  1. WebSocket Protocol (Primary - For Sessions)

  Session Initialization

  // User connects to discovered host
  WebSocket URL: ws://host-address:8080/ws/session

  // Initial handshake message
  {
    "type": "session_init",
    "session_id": "user-generated-uuid",
    "job_id": 12345,  // From blockchain job creation
    "conversation_context": [  // Previous messages if resuming
      {"role": "user", "content": "What is AI?"},
      {"role": "assistant", "content": "AI is..."}
    ]
  }

  During Active Session

  // User sends only new prompt (host has context in memory)
  {
    "type": "prompt",
    "session_id": "user-generated-uuid",
    "content": "Tell me more about machine learning",
    "message_index": 5  // For ordering/verification
  }

  // Host responds with inference result
  {
    "type": "response",
    "session_id": "user-generated-uuid",
    "content": "Machine learning is a subset of AI...",
    "tokens_used": 45,
    "message_index": 6
  }

  Session Termination and Payment Settlement (v5+)

  // When WebSocket disconnects (any reason)
  // Node automatically triggers payment settlement
  {
    "type": "session_end",  // Optional - disconnect auto-settles
    "session_id": "user-generated-uuid"
  }

  // Node automatically on disconnect:
  // 1. Submits pending checkpoints (100+ tokens)
  // 2. Calls completeSessionJob() on blockchain
  // 3. Distributes payments:
  //    - 90% to HostEarnings contract
  //    - 10% to Treasury
  //    - Refunds unused deposit to user

  Session Recovery (After Host Crash)

  // User reconnects to new host with full history
  {
    "type": "session_resume",
    "session_id": "same-uuid",
    "job_id": 12345,
    "conversation_context": [  // Full history from S5
      {"role": "user", "content": "What is AI?"},
      {"role": "assistant", "content": "AI is..."},
      {"role": "user", "content": "Tell me more about ML"},
      {"role": "assistant", "content": "Machine learning..."},
      // ... all previous messages
    ],
    "last_message_index": 8
  }

  2. HTTP/1.1 Protocol (Fallback - Stateless Queries)

  Single Inference Request

  POST /v1/inference HTTP/1.1
  Content-Type: application/json

  {
    "model": "llama-2-7b",
    "prompt": "What is the capital of France?",
    "conversation_context": [  // Optional context for stateless request
      {"role": "user", "content": "Let's talk about European cities"},
      {"role": "assistant", "content": "Sure, I'd be happy to discuss European cities"}
    ],
    "session_id": "optional-tracking-id",
    "max_tokens": 100,
    "temperature": 0.7
  }

  Data Flow Architecture

  ┌─────────────────┐                    ┌──────────────────┐
  │   User (SDK)    │                    │  Host (Rust Node)│
  │                 │                    │                  │
  │ ┌─────────────┐ │   WebSocket       │  ┌────────────┐  │
  │ │ S5 Storage  │ │◄──────────────────►│  │  Memory    │  │
  │ │ (Permanent) │ │                    │  │  (Session) │  │
  │ └─────────────┘ │                    │  └────────────┘  │
  │                 │                    │                  │
  │ Stores:         │                    │  Caches:         │
  │ • Full history  │   Send: prompt     │  • Current conv  │
  │ • All sessions  │   ───────────────► │  • During session│
  │ • Encrypted     │                    │  • Cleared after │
  │                 │   Recv: response   │                  │
  │                 │ ◄─────────────────  │                  │
  └─────────────────┘                    └──────────────────┘

  Sequence Flows

  Normal Session Flow

  1. User discovers host via P2P/registry
  2. User creates blockchain job (payment escrow in USDC)
  3. User connects WebSocket to host
  4. User sends session_init with any prior context
  5. Host creates in-memory conversation cache
  6. For each prompt:
     a. User sends prompt only
     b. Host adds to memory cache
     c. Host sends cache + prompt to LLM
     d. Host returns response
     e. User stores prompt + response in S5
     f. Host adds response to memory cache
  7. Session ends:
     - Host automatically settles payment on disconnect (v5+)
     - Host clears memory
     - User has full history in S5

  Recovery Flow (Host Crash or Connection Loss)

  1. WebSocket connection drops
  2. Previous host automatically settles payment (v5+):
     - Submits any pending checkpoints
     - Calls completeSessionJob() on blockchain
     - Payments distributed (90% host, 10% treasury, refund to user)
  3. User discovers new host
  4. User loads full conversation from S5
  5. User connects to new host (new job/payment if needed)
  6. User sends session_resume with full context
  7. New host rebuilds memory cache
  8. Session continues normally

  Message Format Specifications

  WebSocket Message Envelope

  interface WSMessage {
    type: 'session_init' | 'session_resume' | 'prompt' | 'response' | 'error' | 'session_end';
    session_id: string;
    timestamp: number;
    payload: any;  // Type-specific content
  }

  Conversation Context Format

  interface Message {
    role: 'user' | 'assistant' | 'system';
    content: string;
    timestamp?: number;
    tokens?: number;  // For assistant messages
  }

  interface ConversationContext {
    messages: Message[];
    total_tokens: number;
    session_metadata?: {
      model: string;
      created_at: number;
      checkpoint_count: number;
    };
  }

  Host Memory Management

  The host (fabstir-llm-node) should:

  1. On session_init/resume: Build conversation cache from provided context
  2. On each prompt: Append to cache, maintain last N messages based on token limit
  3. On response generation: Add to cache for context continuity
  4. On session_end/disconnect:
     - Automatically trigger payment settlement (v5+)
     - Call completeSessionJob() on blockchain
     - Distribute payments (90% host, 10% treasury)
     - Clear all session data from memory
  5. Never persist: No disk storage of conversation data

  SDK Responsibilities

  The SDK (TypeScript) must:

  1. Generate session IDs: Unique UUID per session
  2. Store every message: Immediately persist to S5 after send/receive
  3. Track message ordering: Use indices for verification
  4. Handle reconnection: Automatic recovery with full context
  5. Manage token budget: Track usage against job limits

  Security Considerations

  1. WebSocket Authentication: Include job_id for payment verification
  2. Message Signing: Optional ECDSA signatures for high-value sessions
  3. Encryption: S5 handles encryption at rest, TLS for transport
  4. Rate Limiting: Host enforces based on payment/tokens
  5. Payment Protection: Automatic settlement on disconnect ensures hosts get paid (v5+)

  Performance Optimizations

  1. Context Truncation: Host limits to last 10-20 messages (configurable)
  2. Compression: Optional gzip for large conversation_context
  3. Streaming: Support for token-by-token response streaming
  4. Batching: Multiple prompts in single WebSocket frame when applicable

● This report clarifies the communication protocol where:
  - Users own and persist all data in S5 storage
  - Hosts only cache in memory during active sessions
  - Full recovery is possible since users have complete history
  - WebSocket maintains efficiency by sending only new prompts during active sessions
  - HTTP provides fallback for simple stateless queries
  - Payment settlement is automatic on disconnect (v5+), ensuring hosts are always compensated

  The key insight is that hosts act as stateless compute providers with temporary memory cache and guaranteed payment settlement,
  while users maintain full data sovereignty through decentralized storage. The automatic payment mechanism (v5+) ensures hosts
  receive compensation even if connections drop unexpectedly, with 90% going to the host via HostEarnings contract, 10% to
  treasury, and unused deposits refunded to users.