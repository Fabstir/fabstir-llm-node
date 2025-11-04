# RAG SDK Integration Guide

## Overview

This guide shows SDK developers how to integrate **RAG (Retrieval-Augmented Generation)** functionality with fabstir-llm-node. RAG enables applications to provide document-specific context to LLMs, improving answer quality and reducing hallucinations.

### What is RAG?

RAG (Retrieval-Augmented Generation) is a technique that combines:
1. **Retrieval**: Finding relevant information from a knowledge base
2. **Augmentation**: Injecting that information into the LLM prompt
3. **Generation**: Producing an answer based on the provided context

### Why Host-Side RAG?

- **Performance**: Native Rust vector search (< 100ms for 10K vectors)
- **Privacy**: Vectors stay in session memory, cleared on disconnect
- **Simplicity**: No client-side vector database needed
- **Efficiency**: Reuses existing embedding infrastructure (POST /v1/embed)
- **Cost**: No additional services required

---

## Prerequisites

Before integrating RAG, ensure you have:

1. **Embedding Endpoint**: Access to `POST /v1/embed` for generating 384D embeddings
2. **WebSocket Connection**: Established connection to `ws://host:8080/v1/ws`
3. **Document Chunking**: Strategy to split documents into ~500 token chunks
4. **Tokenizer**: Library like tiktoken (Python/JS) or tokenizers (Rust) for chunk sizing

---

## Complete Workflow

```
┌─────────────────────────────────────────────────────────────┐
│  PHASE 1: SESSION START (One-time per session)             │
└─────────────────────────────────────────────────────────────┘

1. User uploads PDF/document to SDK
2. SDK chunks document (500 tokens per chunk recommended)
3. SDK generates embeddings: POST /v1/embed for each chunk
4. SDK uploads vectors: Send UploadVectors WebSocket message
5. Host stores vectors in session memory (SessionVectorStore)

┌─────────────────────────────────────────────────────────────┐
│  PHASE 2: CHAT INTERACTION (Per user question)             │
└─────────────────────────────────────────────────────────────┘

1. User asks question
2. SDK generates query embedding: POST /v1/embed
3. SDK searches vectors: Send SearchVectors WebSocket message
4. Host returns top-k relevant chunks (sorted by similarity)
5. SDK extracts text from chunk metadata
6. SDK injects context into prompt: "Context: ... Question: ..."
7. SDK sends augmented prompt to inference endpoint
8. Host generates answer using provided context

┌─────────────────────────────────────────────────────────────┐
│  PHASE 3: SESSION END                                       │
└─────────────────────────────────────────────────────────────┘

1. User disconnects WebSocket
2. Host automatically clears all session vectors from memory
3. No persistence - vectors exist only during session
```

---

## Message Formats

### 1. UploadVectors Message

Uploads document vectors to the host's session storage.

**WebSocket Message** (camelCase for JavaScript compatibility):

```json
{
  "type": "uploadVectors",
  "requestId": "upload-123",
  "vectors": [
    {
      "id": "doc_0",
      "vector": [0.12, 0.45, ..., 0.89],
      "metadata": {
        "text": "Machine learning is a subset of AI.",
        "page": 1,
        "chunk_index": 0,
        "source": "ml_guide.pdf"
      }
    },
    {
      "id": "doc_1",
      "vector": [0.34, 0.67, ..., 0.23],
      "metadata": {
        "text": "Neural networks process information in layers.",
        "page": 1,
        "chunk_index": 1,
        "source": "ml_guide.pdf"
      }
    }
  ],
  "replace": false
}
```

**Fields**:
- `type`: `"uploadVectors"` (required)
- `requestId`: String, optional tracking ID
- `vectors`: Array of vector objects (max 1000 per batch)
  - `id`: Unique identifier for this vector
  - `vector`: Array of 384 floats (from POST /v1/embed)
  - `metadata`: JSON object (< 10KB) - store chunk text here
- `replace`: Boolean
  - `false`: Append to existing vectors (default)
  - `true`: Clear all existing vectors first

**Response**:

```json
{
  "type": "uploadVectorsResult",
  "requestId": "upload-123",
  "uploaded": 2,
  "rejected": 0,
  "errors": []
}
```

**Response Fields**:
- `uploaded`: Number of vectors successfully stored
- `rejected`: Number of vectors that failed validation
- `errors`: Array of error messages for rejected vectors

---

### 2. SearchVectors Message

Searches for relevant chunks using semantic similarity.

**WebSocket Message**:

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
- `requestId`: String, optional tracking ID
- `queryVector`: Array of 384 floats (question embedding)
- `k`: Number of top results to return (max 100)
- `threshold`: Optional minimum similarity score (0.0-1.0)
  - Results with score < threshold are filtered out
- `metadataFilter`: Optional JSON query object
  - Supports `$eq` (equals) and `$in` (one of) operators

**Response**:

```json
{
  "type": "searchVectorsResult",
  "requestId": "search-456",
  "results": [
    {
      "id": "doc_0",
      "score": 0.95,
      "metadata": {
        "text": "Machine learning is a subset of AI.",
        "page": 1,
        "chunk_index": 0
      }
    },
    {
      "id": "doc_1",
      "score": 0.87,
      "metadata": {
        "text": "Neural networks process information in layers.",
        "page": 1,
        "chunk_index": 1
      }
    }
  ],
  "totalVectors": 2,
  "searchTimeMs": 2.3
}
```

**Response Fields**:
- `results`: Array of search results (sorted by score descending)
  - `id`: Vector ID
  - `score`: Cosine similarity score (higher = more relevant)
  - `metadata`: Original metadata from upload
- `totalVectors`: Total number of vectors in session
- `searchTimeMs`: Search execution time in milliseconds

---

## TypeScript/JavaScript Integration

### Complete Example

```typescript
import { WebSocket } from 'ws'; // Node.js
// Or use native WebSocket in browser

class RAGClient {
  private ws: WebSocket;
  private sessionVectors = new Map<string, number[]>();

  constructor(hostUrl: string) {
    this.ws = new WebSocket(`${hostUrl}/v1/ws`);

    this.ws.on('message', (data) => {
      const message = JSON.parse(data.toString());
      this.handleMessage(message);
    });
  }

  // Step 1: Generate embedding for text
  async generateEmbedding(text: string): Promise<number[]> {
    const response = await fetch('http://host:8080/v1/embed', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ input: text })
    });

    const data = await response.json();
    return data.embedding; // 384D array
  }

  // Step 2: Upload document vectors
  async uploadDocument(chunks: string[]): Promise<void> {
    const vectors = [];

    for (let i = 0; i < chunks.length; i++) {
      const embedding = await this.generateEmbedding(chunks[i]);

      vectors.push({
        id: `doc_${i}`,
        vector: embedding,
        metadata: {
          text: chunks[i],
          chunk_index: i,
          timestamp: Date.now()
        }
      });
    }

    // Send in batches of 1000 (max batch size)
    for (let i = 0; i < vectors.length; i += 1000) {
      const batch = vectors.slice(i, i + 1000);

      const uploadMessage = {
        type: 'uploadVectors',
        requestId: `upload-${Date.now()}`,
        vectors: batch,
        replace: i === 0 // Only clear on first batch
      };

      this.ws.send(JSON.stringify(uploadMessage));

      // Wait for response before sending next batch
      await this.waitForUploadResponse();
    }
  }

  // Step 3: Search for relevant chunks
  async search(question: string, topK: number = 5): Promise<SearchResult[]> {
    // Generate query embedding
    const queryEmbedding = await this.generateEmbedding(question);

    // Send search message
    const searchMessage = {
      type: 'searchVectors',
      requestId: `search-${Date.now()}`,
      queryVector: queryEmbedding,
      k: topK,
      threshold: 0.7, // Only return results with >70% similarity
      metadataFilter: null
    };

    this.ws.send(JSON.stringify(searchMessage));

    // Wait for and return results
    return this.waitForSearchResponse();
  }

  // Step 4: Inject context and generate answer
  async answerQuestion(question: string): Promise<string> {
    // Search for relevant chunks
    const results = await this.search(question, 5);

    // Build context from top results
    const context = results
      .map(r => r.metadata.text)
      .join('\n\n');

    // Create augmented prompt
    const prompt = `Use the following context to answer the question.
If the answer is not in the context, say "I don't know based on the provided context."

Context:
${context}

Question: ${question}

Answer:`;

    // Send to inference
    const inferenceMessage = {
      type: 'inference',
      prompt: prompt,
      max_tokens: 200,
      stream: true
    };

    this.ws.send(JSON.stringify(inferenceMessage));

    // Handle streaming response...
    return this.waitForInferenceResponse();
  }

  // Message handler
  private handleMessage(message: any) {
    switch (message.type) {
      case 'uploadVectorsResult':
        console.log(`✅ Uploaded: ${message.uploaded}, Rejected: ${message.rejected}`);
        if (message.errors.length > 0) {
          console.error('Upload errors:', message.errors);
        }
        break;

      case 'searchVectorsResult':
        console.log(`🔍 Found ${message.results.length} results in ${message.searchTimeMs}ms`);
        break;

      case 'error':
        console.error('❌ Error:', message.error);
        break;
    }
  }

  // Helper: Wait for upload response
  private waitForUploadResponse(): Promise<void> {
    return new Promise((resolve, reject) => {
      const handler = (data: any) => {
        const msg = JSON.parse(data.toString());
        if (msg.type === 'uploadVectorsResult') {
          this.ws.off('message', handler);
          if (msg.rejected > 0) {
            reject(new Error(`Upload failed: ${msg.errors.join(', ')}`));
          } else {
            resolve();
          }
        }
      };
      this.ws.on('message', handler);
    });
  }

  // Helper: Wait for search response
  private waitForSearchResponse(): Promise<SearchResult[]> {
    return new Promise((resolve) => {
      const handler = (data: any) => {
        const msg = JSON.parse(data.toString());
        if (msg.type === 'searchVectorsResult') {
          this.ws.off('message', handler);
          resolve(msg.results);
        }
      };
      this.ws.on('message', handler);
    });
  }

  // Helper: Wait for inference response
  private waitForInferenceResponse(): Promise<string> {
    return new Promise((resolve) => {
      let answer = '';
      const handler = (data: any) => {
        const msg = JSON.parse(data.toString());
        if (msg.type === 'streamToken') {
          answer += msg.token;
        } else if (msg.type === 'streamEnd') {
          this.ws.off('message', handler);
          resolve(answer);
        }
      };
      this.ws.on('message', handler);
    });
  }
}

// Usage Example
interface SearchResult {
  id: string;
  score: number;
  metadata: {
    text: string;
    chunk_index: number;
    [key: string]: any;
  };
}

async function main() {
  const client = new RAGClient('ws://localhost:8080');

  // 1. Upload document
  const document = `
    Machine learning is a subset of artificial intelligence.
    Neural networks are inspired by biological neurons.
    Deep learning uses multiple layers to process data.
  `;

  const chunks = document.split('.').map(s => s.trim()).filter(s => s);
  await client.uploadDocument(chunks);

  // 2. Ask questions
  const answer = await client.answerQuestion('What is machine learning?');
  console.log('Answer:', answer);
}

main().catch(console.error);
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

#### 2. Invalid Vector Dimensions

```json
{
  "type": "uploadVectorsResult",
  "uploaded": 0,
  "rejected": 1,
  "errors": ["Vector doc_0: Invalid dimensions: expected 384, got 256"]
}
```

**Solution**: Verify embeddings are 384-dimensional from POST /v1/embed.

---

#### 3. Batch Size Exceeded

```json
{
  "type": "error",
  "error": "Upload batch size too large: 1500 vectors (max: 1000)"
}
```

**Solution**: Split uploads into batches of ≤ 1000 vectors.

```typescript
// Split into batches
const BATCH_SIZE = 1000;
for (let i = 0; i < vectors.length; i += BATCH_SIZE) {
  const batch = vectors.slice(i, i + BATCH_SIZE);
  await uploadBatch(batch);
}
```

---

#### 4. NaN or Infinity Values

```json
{
  "type": "uploadVectorsResult",
  "uploaded": 0,
  "rejected": 1,
  "errors": ["Invalid vector values: contains NaN or Infinity"]
}
```

**Solution**: Validate embeddings before uploading.

```typescript
function isValidVector(vector: number[]): boolean {
  return vector.every(v => !isNaN(v) && isFinite(v));
}

if (!isValidVector(embedding)) {
  console.error('Invalid embedding detected!');
}
```

---

#### 5. Metadata Too Large

```json
{
  "type": "uploadVectorsResult",
  "uploaded": 0,
  "rejected": 1,
  "errors": ["Metadata too large: 15000 bytes (max: 10240 bytes)"]
}
```

**Solution**: Keep metadata < 10KB per vector. Store large content externally.

```typescript
// ❌ BAD: Storing entire document in metadata
metadata: { fullDocument: "Very long text..." }

// ✅ GOOD: Store only essential info
metadata: {
  text: "Chunk text (< 500 tokens)",
  page: 1,
  chunk_index: 0
}
```

---

## Performance Tips

### 1. Batch Uploads

Upload vectors in batches of 500-1000 for optimal performance:

```typescript
const OPTIMAL_BATCH_SIZE = 500;
// Upload in smaller batches for better throughput
```

### 2. Cache Embeddings

Generate embeddings once and cache them:

```typescript
const embeddingCache = new Map<string, number[]>();

async function getEmbedding(text: string): Promise<number[]> {
  if (embeddingCache.has(text)) {
    return embeddingCache.get(text)!;
  }

  const embedding = await generateEmbedding(text);
  embeddingCache.set(text, embedding);
  return embedding;
}
```

### 3. Parallel Embedding Generation

Generate embeddings in parallel for faster uploads:

```typescript
const embeddings = await Promise.all(
  chunks.map(chunk => generateEmbedding(chunk))
);
```

### 4. Use Thresholds

Filter low-quality results with `threshold` parameter:

```typescript
// Only return highly relevant results (>80% similarity)
const searchMessage = {
  type: 'searchVectors',
  queryVector: embedding,
  k: 10,
  threshold: 0.8  // Higher threshold = stricter matching
};
```

### 5. Optimize Chunk Size

- **Too small** (< 200 tokens): Loss of context
- **Too large** (> 1000 tokens): Reduced precision
- **Recommended**: 400-600 tokens per chunk

---

## Security Considerations

### Session Isolation

Vectors are isolated per session:
- Each WebSocket session has its own vector store
- Vectors from one session are never accessible to another
- Automatic cleanup on disconnect prevents data leaks

### Memory Limits

Default limits (configurable by host):
- **Max vectors per session**: 100,000
- **Max batch size**: 1,000 vectors
- **Max metadata size**: 10KB per vector

### Data Privacy

- **No persistence**: Vectors exist only during WebSocket session
- **Automatic cleanup**: All vectors cleared on disconnect
- **In-memory only**: No disk storage or logging

---

## Best Practices

### 1. Document Chunking Strategy

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

### 2. Metadata Structure

Include essential information for context reconstruction:

```json
{
  "text": "The actual chunk text",
  "page": 5,
  "chunk_index": 12,
  "source": "document.pdf",
  "section": "Introduction",
  "timestamp": 1234567890
}
```

### 3. Context Injection Template

Use a consistent template for context injection:

```typescript
const prompt = `You are a helpful assistant. Answer the question based on the provided context.

Context:
${context}

Question: ${question}

Instructions:
- Use only information from the context
- If the answer is not in the context, say "I don't know based on the provided context"
- Be concise and accurate

Answer:`;
```

### 4. Error Resilience

Implement retry logic for failed uploads:

```typescript
async function uploadWithRetry(vectors: any[], maxRetries: number = 3) {
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      await uploadVectors(vectors);
      return; // Success
    } catch (error) {
      if (attempt === maxRetries) throw error;
      await sleep(1000 * attempt); // Exponential backoff
    }
  }
}
```

---

## API Reference

### Endpoints

- **POST /v1/embed**: Generate 384D embeddings
  - Request: `{ "input": "text to embed" }`
  - Response: `{ "embedding": [f32; 384] }`

- **WS /v1/ws**: WebSocket endpoint for all interactions
  - Messages: UploadVectors, SearchVectors, Inference

### Message Types

| Type | Direction | Purpose |
|------|-----------|---------|
| `uploadVectors` | Client → Host | Upload document vectors |
| `uploadVectorsResult` | Host → Client | Upload confirmation |
| `searchVectors` | Client → Host | Search for relevant chunks |
| `searchVectorsResult` | Host → Client | Search results |
| `error` | Host → Client | Error notification |

---

## Complete Example: PDF RAG Pipeline

```typescript
import * as pdf from 'pdf-parse';
import { RAGClient } from './rag-client';

async function processAndUploadPDF(
  pdfBuffer: Buffer,
  client: RAGClient
): Promise<void> {
  // 1. Extract text from PDF
  const data = await pdf(pdfBuffer);
  const text = data.text;

  // 2. Chunk by tokens (~500 per chunk)
  const chunks = chunkByTokens(text, 500);

  // 3. Upload to host
  await client.uploadDocument(chunks);

  console.log(`✅ Uploaded ${chunks.length} chunks from PDF`);
}

async function chatWithPDF(
  client: RAGClient,
  question: string
): Promise<string> {
  // Search and answer using RAG
  const answer = await client.answerQuestion(question);
  return answer;
}

// Usage
const client = new RAGClient('ws://localhost:8080');
const pdfBuffer = await fs.readFile('document.pdf');

await processAndUploadPDF(pdfBuffer, client);
const answer = await chatWithPDF(client, 'What are the main findings?');
console.log('Answer:', answer);
```

---

## Troubleshooting

### Search Returns No Results

**Possible causes**:
1. No vectors uploaded yet
2. Threshold too high
3. Query embedding mismatched with document embeddings

**Solutions**:
```typescript
// Check total vectors
const response = await client.search('test', 1);
console.log('Total vectors:', response.totalVectors);

// Lower threshold
const results = await client.search(question, 5, { threshold: 0.5 });

// Verify embedding endpoint
const testEmb = await client.generateEmbedding('test');
console.log('Embedding dimensions:', testEmb.length); // Should be 384
```

### Slow Search Performance

**Expected performance**:
- 1K vectors: < 10ms
- 10K vectors: < 100ms
- 100K vectors: < 500ms

**Solutions**:
- Reduce number of vectors per session
- Lower `k` value (fewer results = faster)
- Use metadata filters to narrow search space

---

## Additional Resources

- **Example Code**: `examples/rag_integration.rs`
- **API Documentation**: `docs/API.md`
- **WebSocket Guide**: `docs/WEBSOCKET_API_SDK_GUIDE.md`
- **Implementation Plan**: `docs/IMPLEMENTATION_HOST_SIDE_RAG.md`

---

## Support

For issues or questions:
1. Check error messages in WebSocket responses
2. Review this guide's error handling section
3. Check example code in `examples/rag_integration.rs`
4. Report issues at: https://github.com/anthropics/fabstir-llm-node/issues
