// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//
// RAG Integration Example for SDK Developers
//
// This example demonstrates how to integrate RAG (Retrieval-Augmented Generation)
// functionality with fabstir-llm-node. It shows the complete workflow:
//
// 1. Generate embeddings for document chunks
// 2. Upload vectors to the host's session storage
// 3. Search for relevant chunks during chat
// 4. Inject context into prompts for better answers
//
// Run with: cargo run --example rag_integration

use anyhow::Result;
use serde_json::json;

/// Simulates a PDF document chunked into smaller pieces
fn chunk_document(text: &str) -> Vec<String> {
    // In real SDK: Use tiktoken or similar to chunk by tokens (~500 tokens per chunk)
    // For demo: Split by sentences
    text.split('.')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Simulates calling POST /v1/embed to generate embeddings
/// In real SDK: Make HTTP request to the host's embedding endpoint
fn generate_embedding(text: &str) -> Vec<f32> {
    println!("  📊 Generating embedding for: {:.50}...", text);

    // Real SDK would call: POST http://host:8080/v1/embed
    // Body: { "input": text }
    // Response: { "embedding": [f32; 384] }

    // For demo: Generate mock 384D vector based on text hash
    let hash = text.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
    let base = (hash % 100) as f32 / 100.0;
    vec![base; 384]
}

/// Example: Upload document vectors to host session
fn example_upload_vectors() -> Result<()> {
    println!("\n🔵 Example 1: Upload Document Vectors\n");

    // Step 1: Load and chunk document
    let document = "Machine learning is a subset of artificial intelligence. \
                    Neural networks are inspired by biological neurons. \
                    Deep learning uses multiple layers of neural networks. \
                    Supervised learning requires labeled training data. \
                    Unsupervised learning finds patterns without labels.";

    println!("📄 Document: {:.80}...", document);
    let chunks = chunk_document(document);
    println!("✂️  Split into {} chunks", chunks.len());

    // Step 2: Generate embeddings for each chunk
    let mut vectors = Vec::new();
    for (idx, chunk) in chunks.iter().enumerate() {
        let embedding = generate_embedding(chunk);
        vectors.push(json!({
            "id": format!("doc_{}", idx),
            "vector": embedding,
            "metadata": {
                "text": chunk,
                "page": 1,
                "chunk_index": idx,
                "category": "ml"
            }
        }));
    }

    // Step 3: Create UploadVectors WebSocket message
    let upload_message = json!({
        "type": "uploadVectors",  // Message type
        "requestId": "upload-123",  // Optional: for tracking
        "vectors": vectors,
        "replace": false  // false = append, true = clear existing
    });

    println!("\n📤 WebSocket Message to Send:");
    println!("{}", serde_json::to_string_pretty(&upload_message)?);

    // Step 4: Send via WebSocket (pseudo-code)
    println!("\n💬 SDK would send:");
    println!("   ws.send(JSON.stringify(uploadMessage))");

    // Step 5: Receive response
    let mock_response = json!({
        "type": "uploadVectorsResult",
        "requestId": "upload-123",
        "uploaded": 5,
        "rejected": 0,
        "errors": []
    });

    println!("\n📥 Expected Response:");
    println!("{}", serde_json::to_string_pretty(&mock_response)?);

    Ok(())
}

/// Example: Search for relevant chunks during chat
fn example_search_vectors() -> Result<()> {
    println!("\n🔵 Example 2: Search for Relevant Chunks\n");

    // Step 1: User asks a question
    let user_question = "What is neural networks?";
    println!("❓ User Question: {}", user_question);

    // Step 2: Generate query embedding
    let query_embedding = generate_embedding(user_question);
    println!("  📊 Query embedding generated (384D)");

    // Step 3: Create SearchVectors WebSocket message
    let search_message = json!({
        "type": "searchVectors",
        "requestId": "search-456",
        "queryVector": query_embedding,
        "k": 3,  // Return top-3 most relevant chunks
        "threshold": 0.7,  // Optional: minimum similarity score
        "metadataFilter": null  // Optional: filter by metadata
    });

    println!("\n📤 WebSocket Message to Send:");
    println!("{}", serde_json::to_string_pretty(&search_message)?);

    // Step 4: Receive search results
    let mock_response = json!({
        "type": "searchVectorsResult",
        "requestId": "search-456",
        "results": [
            {
                "id": "doc_1",
                "score": 0.95,
                "metadata": {
                    "text": "Neural networks are inspired by biological neurons",
                    "page": 1,
                    "chunk_index": 1
                }
            },
            {
                "id": "doc_2",
                "score": 0.87,
                "metadata": {
                    "text": "Deep learning uses multiple layers of neural networks",
                    "page": 1,
                    "chunk_index": 2
                }
            },
            {
                "id": "doc_0",
                "score": 0.75,
                "metadata": {
                    "text": "Machine learning is a subset of artificial intelligence",
                    "page": 1,
                    "chunk_index": 0
                }
            }
        ],
        "totalVectors": 5,
        "searchTimeMs": 2.3
    });

    println!("\n📥 Search Results:");
    println!("{}", serde_json::to_string_pretty(&mock_response)?);

    Ok(())
}

/// Example: Inject context into prompt
fn example_context_injection() -> Result<()> {
    println!("\n🔵 Example 3: Context Injection\n");

    // Step 1: Search results from previous example
    let search_results = vec![
        "Neural networks are inspired by biological neurons",
        "Deep learning uses multiple layers of neural networks",
        "Machine learning is a subset of artificial intelligence",
    ];

    let user_question = "What is neural networks?";

    // Step 2: Build context-augmented prompt
    let context = search_results.join("\n\n");

    let augmented_prompt = format!(
        "Use the following context to answer the question. \
         If the answer is not in the context, say 'I don't know based on the provided context.'\n\n\
         Context:\n{}\n\n\
         Question: {}\n\n\
         Answer:",
        context, user_question
    );

    println!("📝 Original Question:");
    println!("   {}", user_question);

    println!("\n📚 Retrieved Context:");
    for (i, chunk) in search_results.iter().enumerate() {
        println!("   {}. {}", i + 1, chunk);
    }

    println!("\n🎯 Augmented Prompt:");
    println!("{}", augmented_prompt);

    // Step 3: Send to inference
    let inference_message = json!({
        "type": "inference",
        "prompt": augmented_prompt,
        "max_tokens": 200,
        "stream": true
    });

    println!("\n📤 Inference Request:");
    println!("{}", serde_json::to_string_pretty(&inference_message)?);

    println!("\n💡 Expected Answer:");
    println!("   Neural networks are computational models inspired by biological neurons.");
    println!("   They use multiple layers (as in deep learning) to process information.");

    Ok(())
}

/// Example: Complete RAG workflow
fn example_complete_workflow() -> Result<()> {
    println!("\n🔵 Example 4: Complete RAG Workflow\n");

    println!("📋 Complete Workflow Steps:");
    println!("\n1️⃣  SESSION START");
    println!("   • User uploads PDF to SDK");
    println!("   • SDK chunks document (~500 tokens per chunk)");
    println!("   • SDK calls POST /v1/embed for each chunk");
    println!("   • SDK sends UploadVectors WebSocket message");
    println!("   • Host stores vectors in session memory");

    println!("\n2️⃣  CHAT INTERACTION");
    println!("   • User asks question");
    println!("   • SDK calls POST /v1/embed for question");
    println!("   • SDK sends SearchVectors WebSocket message");
    println!("   • Host returns top-k relevant chunks");
    println!("   • SDK injects chunks into prompt as context");
    println!("   • SDK sends augmented prompt to inference");
    println!("   • Host generates answer using context");

    println!("\n3️⃣  SESSION END");
    println!("   • User disconnects from WebSocket");
    println!("   • Host automatically clears all vectors from memory");
    println!("   • No persistence - vectors are session-scoped");

    Ok(())
}

/// Example: Error handling
fn example_error_handling() -> Result<()> {
    println!("\n🔵 Example 5: Error Handling\n");

    println!("⚠️  Common Errors and Solutions:\n");

    println!("1. RAG_NOT_ENABLED");
    println!("   Error: {{\"error\": \"RAG not enabled for this session\"}}");
    println!("   Solution: Ensure RAG is enabled before uploading vectors");
    println!();

    println!("2. INVALID_DIMENSIONS");
    println!("   Error: {{\"error\": \"Vector doc_1: Invalid dimensions: expected 384, got 256\"}}");
    println!("   Solution: Ensure all embeddings are 384-dimensional");
    println!("   Verify: POST /v1/embed returns 384D vectors");
    println!();

    println!("3. BATCH_SIZE_EXCEEDED");
    println!("   Error: {{\"error\": \"Upload batch size too large: 1500 vectors (max: 1000)\"}}");
    println!("   Solution: Split large uploads into batches of <= 1000 vectors");
    println!();

    println!("4. NAN_OR_INFINITY");
    println!("   Error: {{\"error\": \"Invalid vector values: contains NaN or Infinity\"}}");
    println!("   Solution: Validate embeddings before uploading");
    println!("   Check: embedding.every(v => !isNaN(v) && isFinite(v))");
    println!();

    println!("5. METADATA_TOO_LARGE");
    println!("   Error: {{\"error\": \"Metadata too large: 15000 bytes (max: 10240 bytes)\"}}");
    println!("   Solution: Reduce metadata size (< 10KB per vector)");
    println!("   Store large content externally, keep only references");

    Ok(())
}

fn main() -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║  RAG Integration Example for fabstir-llm-node             ║");
    println!("║  Demonstrates how SDK developers can integrate RAG        ║");
    println!("╚════════════════════════════════════════════════════════════╝");

    // Run all examples
    example_upload_vectors()?;
    example_search_vectors()?;
    example_context_injection()?;
    example_complete_workflow()?;
    example_error_handling()?;

    println!("\n✅ All examples completed!");
    println!("\n📚 For detailed documentation, see:");
    println!("   docs/RAG_SDK_INTEGRATION.md");
    println!("\n🔗 API Reference:");
    println!("   - POST /v1/embed - Generate embeddings");
    println!("   - WS /v1/ws - WebSocket endpoint");
    println!("   - Message types: UploadVectors, SearchVectors");

    Ok(())
}
