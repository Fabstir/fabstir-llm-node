# Host-Side RAG Integration Guide for SDK Developers

## Overview

This guide provides SDK developers with everything needed to integrate **Host-Side RAG (Retrieval-Augmented Generation)** into the Fabstir LLM marketplace. RAG enables document-aware AI responses by allowing users to upload document vectors and search them during chat sessions.

---

## Quick Start

### What is Host-Side RAG?

Host-Side RAG allows users to:
1. Upload document chunks as 384D embeddings
2. Search for relevant chunks during chat
3. Inject context into prompts for better answers
4. All vectors stored in session memory (cleared on disconnect)

### Why Host-Side?

- **Fast**: Native Rust vector search (~100ms for 10K vectors)
- **Simple**: No client-side vector database needed
- **Secure**: Session-isolated, auto-cleanup on disconnect
- **Efficient**: Reuses existing embedding endpoint (POST /v1/embed)

---

## Implementation Status

**Version**: v8.2.0+ (January 2025)
**Status**: ✅ Production-ready (100% complete, 84 tests passing)

### What's Included

✅ Session-scoped vector storage (up to 100K vectors)
✅ WebSocket message types (UploadVectors, SearchVectors)
✅ Message handlers with error handling
✅ 384D embedding support (all-MiniLM-L6-v2 compatible)
✅ Metadata filtering and threshold support
✅ Batch uploads (max 1000 vectors per message)
✅ Automatic session cleanup
✅ Complete SDK examples and documentation

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  CLIENT (SDK)                                           │
├─────────────────────────────────────────────────────────┤
│  1. User uploads PDF                                    │
│  2. Chunk document (~500 tokens per chunk)              │
│  3. Generate embeddings: POST /v1/embed                 │
│  4. Upload vectors via WebSocket                        │
│                                                          │
│  During Chat:                                           │
│  5. Generate query embedding: POST /v1/embed            │
│  6. Search vectors via WebSocket                        │
│  7. Inject context into prompt                          │
│  8. Send augmented prompt to inference                  │
└─────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────┐
│  HOST (fabstir-llm-node)                                │
├─────────────────────────────────────────────────────────┤
│  WebSocket: ws://host:8080/v1/ws                        │
│                                                          │
│  SessionVectorStore (per session):                      │
│  • HashMap<String, Vector>                              │
│  • Cosine similarity search                             │
│  • Metadata filtering                                   │
│  • Auto-cleanup on disconnect                           │
└─────────────────────────────────────────────────────────┘
```

---

## WebSocket Messages

### 1. Upload Vectors

**Purpose**: Store document chunks as vectors in session memory

**Message Format** (camelCase):
```json
{
  "type": "uploadVectors",
  "requestId": "upload-123",
  "vectors": [
    {
      "id": "chunk_0",
      "vector": [0.12, 0.45, ..., 0.89],
      "metadata": {
        "text": "Machine learning is a subset of AI.",
        "page": 1,
        "chunkIndex": 0,
        "source": "ml_guide.pdf"
      }
    }
  ],
  "replace": false
}
```

**Fields**:
- `type`: `"uploadVectors"` (required)
- `requestId`: Optional tracking ID (string)
- `vectors`: Array of vectors (max 1000 per batch)
  - `id`: Unique identifier (string)
  - `vector`: 384-dimensional float array (from POST /v1/embed)
  - `metadata`: JSON object (< 10KB) - **store chunk text here**
- `replace`: Boolean (default: false)
  - `false`: Append to existing vectors
  - `true`: Clear all existing vectors first

**Response**:
```json
{
  "type": "uploadVectorsResult",
  "requestId": "upload-123",
  "uploaded": 1,
  "rejected": 0,
  "errors": []
}
```

**Validation Rules**:
- Max 1000 vectors per batch
- Each vector must be exactly 384 dimensions
- Metadata must be < 10KB per vector
- No NaN or Infinity values allowed

---

### 2. Search Vectors

**Purpose**: Find relevant document chunks using semantic similarity

**Message Format**:
```json
{
  "type": "searchVectors",
  "requestId": "search-456",
  "queryVector": [0.23, 0.56, ..., 0.78],
  "k": 5,
  "threshold": 0.7,
  "metadataFilter": {
    "source": {
      "$eq": "ml_guide.pdf"
    }
  }
}
```

**Fields**:
- `type`: `"searchVectors"` (required)
- `requestId`: Optional tracking ID (string)
- `queryVector`: 384-dimensional float array (question embedding)
- `k`: Number of results to return (max 100)
- `threshold`: Optional minimum similarity score (0.0-1.0)
- `metadataFilter`: Optional JSON query (supports $eq, $in)

**Response**:
```json
{
  "type": "searchVectorsResult",
  "requestId": "search-456",
  "results": [
    {
      "id": "chunk_0",
      "score": 0.95,
      "metadata": {
        "text": "Machine learning is a subset of AI.",
        "page": 1,
        "chunkIndex": 0
      }
    }
  ],
  "totalVectors": 10,
  "searchTimeMs": 2.3
}
```

**Response Fields**:
- `results`: Array sorted by score (descending)
  - `id`: Vector ID
  - `score`: Cosine similarity (higher = more relevant)
  - `metadata`: Original metadata from upload
- `totalVectors`: Total vectors in session
- `searchTimeMs`: Search execution time

---

## SDK Integration Pattern

### TypeScript Example

```typescript
import { WebSocket } from 'ws';

interface VectorUpload {
  id: string;
  vector: number[];
  metadata: Record<string, any>;
}

interface SearchResult {
  id: string;
  score: number;
  metadata: Record<string, any>;
}

class RAGClient {
  private ws: WebSocket;
  private pendingRequests = new Map<string, (data: any) => void>();

  constructor(hostUrl: string) {
    this.ws = new WebSocket(`${hostUrl}/v1/ws`);
    this.ws.on('message', (data) => this.handleMessage(JSON.parse(data.toString())));
  }

  // Generate embedding using host's endpoint
  async generateEmbedding(text: string): Promise<number[]> {
    const response = await fetch('http://host:8080/v1/embed', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ input: text })
    });
    const data = await response.json();
    return data.embedding; // 384D array
  }

  // Upload document chunks
  async uploadDocument(chunks: string[]): Promise<void> {
    const vectors: VectorUpload[] = [];

    // Generate embeddings for all chunks
    for (let i = 0; i < chunks.length; i++) {
      const embedding = await this.generateEmbedding(chunks[i]);
      vectors.push({
        id: `chunk_${i}`,
        vector: embedding,
        metadata: {
          text: chunks[i],
          chunkIndex: i,
          timestamp: Date.now()
        }
      });
    }

    // Upload in batches of 1000
    for (let i = 0; i < vectors.length; i += 1000) {
      const batch = vectors.slice(i, i + 1000);
      await this.uploadBatch(batch, i === 0);
    }
  }

  // Upload a batch of vectors
  private async uploadBatch(vectors: VectorUpload[], replace: boolean): Promise<void> {
    const requestId = `upload-${Date.now()}`;

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(requestId, (response) => {
        if (response.rejected > 0) {
          reject(new Error(`Upload failed: ${response.errors.join(', ')}`));
        } else {
          resolve();
        }
      });

      this.ws.send(JSON.stringify({
        type: 'uploadVectors',
        requestId,
        vectors,
        replace
      }));
    });
  }

  // Search for relevant chunks
  async search(question: string, topK: number = 5): Promise<SearchResult[]> {
    const queryEmbedding = await this.generateEmbedding(question);
    const requestId = `search-${Date.now()}`;

    return new Promise((resolve) => {
      this.pendingRequests.set(requestId, (response) => {
        resolve(response.results);
      });

      this.ws.send(JSON.stringify({
        type: 'searchVectors',
        requestId,
        queryVector: queryEmbedding,
        k: topK,
        threshold: 0.7,
        metadataFilter: null
      }));
    });
  }

  // Answer question using RAG
  async answerQuestion(question: string): Promise<string> {
    // 1. Search for relevant chunks
    const results = await this.search(question, 5);

    // 2. Build context from top results
    const context = results
      .map(r => r.metadata.text)
      .join('\n\n');

    // 3. Create augmented prompt
    const prompt = `Use the following context to answer the question.
If the answer is not in the context, say "I don't know based on the provided context."

Context:
${context}

Question: ${question}

Answer:`;

    // 4. Send to inference (implement based on your SDK)
    return this.sendToInference(prompt);
  }

  // Handle incoming messages
  private handleMessage(message: any) {
    const handler = this.pendingRequests.get(message.requestId);
    if (handler) {
      handler(message);
      this.pendingRequests.delete(message.requestId);
    }

    // Handle other message types
    switch (message.type) {
      case 'uploadVectorsResult':
        console.log(`✅ Uploaded: ${message.uploaded}, Rejected: ${message.rejected}`);
        break;
      case 'searchVectorsResult':
        console.log(`🔍 Found ${message.results.length} results in ${message.searchTimeMs}ms`);
        break;
      case 'error':
        console.error('❌ Error:', message.error);
        break;
    }
  }

  private sendToInference(prompt: string): Promise<string> {
    // Implement based on your SDK's inference method
    throw new Error('Implement inference call');
  }
}

// Usage
async function main() {
  const client = new RAGClient('ws://localhost:8080');

  // Upload PDF chunks
  const chunks = [
    'Machine learning is a subset of AI.',
    'Neural networks are inspired by biological neurons.',
    'Deep learning uses multiple layers.'
  ];
  await client.uploadDocument(chunks);

  // Ask question
  const answer = await client.answerQuestion('What is machine learning?');
  console.log('Answer:', answer);
}
```

---

## Best Practices

### 1. Document Chunking

**Recommended**: 400-600 tokens per chunk

```python
# Python example using tiktoken
import tiktoken

def chunk_document(text: str, chunk_size: int = 500) -> list[str]:
    encoding = tiktoken.get_encoding("cl100k_base")
    tokens = encoding.encode(text)

    chunks = []
    for i in range(0, len(tokens), chunk_size):
        chunk_tokens = tokens[i:i + chunk_size]
        chunk_text = encoding.decode(chunk_tokens)
        chunks.append(chunk_text)

    return chunks
```

### 2. Metadata Strategy

**Include essential information only** (< 10KB per vector):

```json
{
  "text": "The actual chunk text (400-600 tokens)",
  "page": 5,
  "chunkIndex": 12,
  "source": "document.pdf",
  "section": "Introduction"
}
```

**Don't include**:
- Full document text
- Large binary data
- Redundant information

### 3. Context Injection Template

```typescript
const prompt = `You are a helpful assistant. Answer based on the provided context.

Context:
${context}

Question: ${question}

Instructions:
- Use only information from the context
- If the answer is not in the context, say "I don't know based on the provided context"
- Be concise and accurate

Answer:`;
```

### 4. Batch Processing

```typescript
// Upload in batches of 500-1000 for optimal performance
const BATCH_SIZE = 500;

for (let i = 0; i < vectors.length; i += BATCH_SIZE) {
  const batch = vectors.slice(i, i + BATCH_SIZE);
  await uploadBatch(batch);

  // Optional: Add small delay between batches
  await sleep(100);
}
```

### 5. Error Handling

```typescript
async function uploadWithRetry(vectors: VectorUpload[], maxRetries = 3) {
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      await uploadBatch(vectors);
      return; // Success
    } catch (error) {
      if (attempt === maxRetries) throw error;

      console.warn(`Upload failed (attempt ${attempt}/${maxRetries}), retrying...`);
      await sleep(1000 * attempt); // Exponential backoff
    }
  }
}
```

---

## Error Handling

### Common Errors

#### 1. RAG Not Enabled
```json
{
  "type": "error",
  "error": "RAG not enabled for this session"
}
```
**Solution**: Ensure RAG is enabled during session initialization.

---

#### 2. Invalid Dimensions
```json
{
  "type": "uploadVectorsResult",
  "uploaded": 0,
  "rejected": 1,
  "errors": ["Vector chunk_0: Invalid dimensions: expected 384, got 256"]
}
```
**Solution**: Verify embeddings from POST /v1/embed are 384-dimensional.

---

#### 3. Batch Size Exceeded
```json
{
  "type": "error",
  "error": "Upload batch size too large: 1500 vectors (max: 1000)"
}
```
**Solution**: Split into batches ≤ 1000 vectors.

---

#### 4. NaN/Infinity Values
```json
{
  "type": "uploadVectorsResult",
  "errors": ["Invalid vector values: contains NaN or Infinity"]
}
```
**Solution**: Validate embeddings:
```typescript
function isValidVector(vector: number[]): boolean {
  return vector.every(v => !isNaN(v) && isFinite(v));
}
```

---

#### 5. Metadata Too Large
```json
{
  "type": "uploadVectorsResult",
  "errors": ["Metadata too large: 15000 bytes (max: 10240 bytes)"]
}
```
**Solution**: Keep metadata < 10KB per vector.

---

## Performance Guidelines

### Expected Performance

| Operation | Dataset Size | Expected Time |
|-----------|-------------|---------------|
| Upload | 1K vectors | < 10ms |
| Upload | 10K vectors | ~40ms |
| Search | 1K vectors | < 5ms |
| Search | 10K vectors | ~100ms |
| Search | 100K vectors | < 500ms |

### Optimization Tips

1. **Parallel Embedding Generation**:
```typescript
const embeddings = await Promise.all(
  chunks.map(chunk => generateEmbedding(chunk))
);
```

2. **Cache Embeddings**:
```typescript
const cache = new Map<string, number[]>();

async function getCachedEmbedding(text: string): Promise<number[]> {
  if (cache.has(text)) return cache.get(text)!;

  const embedding = await generateEmbedding(text);
  cache.set(text, embedding);
  return embedding;
}
```

3. **Use Thresholds**:
```typescript
// Only return highly relevant results
const results = await search(question, 10, { threshold: 0.8 });
```

4. **Filter by Metadata**:
```typescript
// Narrow search scope
const results = await search(question, 10, {
  metadataFilter: { source: { $eq: 'specific_doc.pdf' } }
});
```

---

## Testing Checklist

### Unit Tests
- [ ] Embedding generation returns 384D arrays
- [ ] Chunk size validation (400-600 tokens)
- [ ] Metadata size < 10KB per vector
- [ ] Vector validation (no NaN/Infinity)

### Integration Tests
- [ ] Upload 100 chunks successfully
- [ ] Search returns relevant results
- [ ] Context injection works correctly
- [ ] Session cleanup removes vectors
- [ ] Concurrent sessions don't interfere

### Performance Tests
- [ ] Upload 1K vectors < 100ms
- [ ] Search 10K vectors < 200ms
- [ ] Multiple searches don't degrade performance

---

## Security Considerations

### Session Isolation
- Each WebSocket session has its own vector store
- Vectors from one session are never accessible to another
- Automatic cleanup on disconnect

### Memory Limits
- **Max vectors per session**: 100,000 (configurable)
- **Max batch size**: 1,000 vectors
- **Max metadata size**: 10KB per vector

### Data Privacy
- **No persistence**: Vectors exist only during WebSocket session
- **Automatic cleanup**: All vectors cleared on disconnect
- **In-memory only**: No disk storage or logging

---

## API Reference

### Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/v1/embed` | POST | Generate 384D embeddings |
| `/v1/ws` | WebSocket | Upload/search vectors |

### Message Types

| Type | Direction | Purpose |
|------|-----------|---------|
| `uploadVectors` | Client → Host | Upload document vectors |
| `uploadVectorsResult` | Host → Client | Upload confirmation |
| `searchVectors` | Client → Host | Search for chunks |
| `searchVectorsResult` | Host → Client | Search results |
| `error` | Host → Client | Error notification |

---

## Troubleshooting

### Search Returns No Results

**Possible Causes**:
1. No vectors uploaded yet
2. Threshold too high
3. Embedding mismatch

**Solutions**:
```typescript
// Check total vectors
const response = await search('test', 1);
console.log('Total vectors:', response.totalVectors);

// Lower threshold
const results = await search(question, 5, { threshold: 0.5 });

// Verify embeddings
const embedding = await generateEmbedding('test');
console.log('Embedding dimensions:', embedding.length); // Should be 384
```

### Slow Search Performance

**Expected**: < 100ms for 10K vectors

**Solutions**:
- Reduce vectors per session
- Lower `k` value
- Use metadata filters

---

## Additional Resources

- **Complete Examples**: `/workspace/examples/rag_integration.rs`
- **Full API Documentation**: `/workspace/docs/RAG_SDK_INTEGRATION.md`
- **WebSocket Guide**: `/workspace/docs/WEBSOCKET_API_SDK_GUIDE.md`
- **Implementation Details**: `/workspace/docs/IMPLEMENTATION_HOST_SIDE_RAG.md`

---

## Support

For issues or questions:
1. Check error messages in WebSocket responses
2. Review this guide's troubleshooting section
3. Check example code in `examples/rag_integration.rs`
4. Report issues at: https://github.com/anthropics/fabstir-llm-node/issues

---

## Version History

**v8.2.0** (January 2025) - Initial Release
- ✅ Session-scoped vector storage
- ✅ WebSocket message types
- ✅ Message handlers
- ✅ 84 passing tests
- ✅ Complete SDK documentation
- ✅ Production-ready

**Status**: Production-ready, 100% complete
