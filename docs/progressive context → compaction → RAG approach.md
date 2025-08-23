## 📝 Updates for fabstir-llm-node/IMPLEMENTATION.md

```markdown
### Sub-phase 4.3.3: Progressive Context Support (MVP to RAG)

#### Overview
Three-stage implementation: Context passing (MVP) → Compaction support → Full RAG integration

#### Stage 1: Context Passing (MVP - Implement Now)

##### Chunk 1: Add Context Support to Job Processing

###### Implementation Tasks
- [ ] Add `conversation_context` field to JobRequest
- [ ] Update request validation to accept context
- [ ] Format context with prompt for LLM
- [ ] Implement context size limits
- [ ] Add context truncation if too large

###### Code Changes
```rust
// src/models/job.rs
#[derive(Deserialize, Serialize, Debug)]
pub struct Message {
    pub role: String,  // "user", "assistant", "system"
    pub content: String,
}

#[derive(Deserialize, Serialize)]
pub struct JobRequest {
    pub id: String,
    pub prompt: String,
    pub parameters: LLMParameters,
    
    // MVP: Include conversation context
    #[serde(default)]
    pub conversation_context: Vec<Message>,
    
    // Future: RAG support (ignored in MVP)
    #[serde(default)]
    pub enable_rag: bool,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_config: Option<SessionConfig>,
}

// src/handlers/inference.rs
async fn process_job(req: JobRequest) -> Result<JobResponse> {
    // Build contextual prompt
    let full_prompt = build_prompt_with_context(
        &req.conversation_context,
        &req.prompt
    );
    
    // Send to LLM
    let response = llm_service.infer(&full_prompt, &req.parameters).await?;
    
    Ok(JobResponse {
        job_id: req.id,
        response,
        metadata: ResponseMetadata {
            context_tokens: count_tokens(&req.conversation_context),
            model: llm_service.model_name(),
            timestamp: Utc::now(),
        },
    })
}

fn build_prompt_with_context(context: &[Message], prompt: &str) -> String {
    let mut full_prompt = String::new();
    
    // Add conversation history
    for msg in context.iter().take(10) { // Limit to last 10 messages
        full_prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
    }
    
    // Add current prompt
    full_prompt.push_str(&format!("User: {}\nAssistant:", prompt));
    
    full_prompt
}
```

###### Success Criteria
- [ ] Context preserved across requests
- [ ] Token counting implemented
- [ ] Context truncation working
- [ ] No vectordb dependency
- [ ] API backward compatible

#### Stage 2: Compaction Support (Phase 2)

##### Chunk 2: Batch Embedding Generation

###### Implementation Tasks
- [ ] Add batch embedding endpoint
- [ ] Implement conversation summarization
- [ ] Create compaction metadata structure
- [ ] Add progress tracking for long operations
- [ ] Cache embeddings temporarily

###### New Endpoints
```rust
// POST /api/compact/embeddings
async fn generate_batch_embeddings(
    messages: Vec<Message>
) -> Result<BatchEmbeddingResponse> {
    // Future: Use vectordb for embedding generation
    // MVP: Return placeholder
    Ok(BatchEmbeddingResponse {
        embeddings: vec![],
        model: "sentence-transformers/all-MiniLM-L6-v2",
        dimensions: 384,
    })
}

// POST /api/compact/summarize
async fn summarize_conversation(
    messages: Vec<Message>
) -> Result<SummaryResponse> {
    let prompt = format_summarization_prompt(messages);
    let summary = llm_service.infer(&prompt, &summary_params).await?;
    
    Ok(SummaryResponse {
        summary,
        token_count: count_tokens(messages),
        message_count: messages.len(),
    })
}
```

#### Stage 3: RAG Preparation (Phase 3 - Future)

##### Chunk 3: Session Management Structure

###### Design Tasks (No Implementation Yet)
- [ ] Define SessionManager trait
- [ ] Create session configuration types
- [ ] Plan delegation token validation
- [ ] Design session lifecycle hooks
- [ ] Document RAG integration points

###### Interface Design
```rust
// src/sessions/mod.rs (Future implementation)
#[async_trait]
pub trait SessionManager {
    /// Create RAG session with user's embeddings
    async fn create_session(
        &self,
        user_delegation: DelegationToken,
        conversation_id: String,
    ) -> Result<SessionId>;
    
    /// Process prompt with RAG context
    async fn process_with_rag(
        &self,
        session_id: SessionId,
        prompt: String,
    ) -> Result<RagResponse>;
    
    /// Clean up expired sessions
    async fn cleanup_expired(&self) -> Result<usize>;
}

// For MVP: Returns NotImplemented
pub struct FutureSessionManager;

impl SessionManager for FutureSessionManager {
    async fn create_session(&self, _: DelegationToken, _: String) -> Result<SessionId> {
        Err(Error::NotImplemented("RAG sessions coming in Phase 3"))
    }
}
```

#### Configuration Changes

##### Environment Variables
```bash
# MVP (no vectordb needed)
LLM_MODEL=tiny-vicuna
MAX_CONTEXT_SIZE=4096
ENABLE_RAG=false

# Phase 2 (compaction)
ENABLE_COMPACTION=true
SUMMARY_MODEL=tiny-vicuna

# Phase 3 (future)
ENABLE_RAG=true
VECTORDB_URL=http://localhost:8080
SESSION_TIMEOUT=1800
```

#### Testing Requirements

##### MVP Tests
- [ ] Test context formatting
- [ ] Test context size limits
- [ ] Test without vectordb
- [ ] Verify token counting
- [ ] Load test with context

##### Phase 2 Tests
- [ ] Test batch embedding generation
- [ ] Test summarization quality
- [ ] Test compaction endpoint
- [ ] Measure compaction performance

##### Phase 3 Tests (Future)
- [ ] Test session creation
- [ ] Test RAG context retrieval
- [ ] Test delegation validation
- [ ] Test session cleanup
```