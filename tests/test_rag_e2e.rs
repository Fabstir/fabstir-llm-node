// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// End-to-end test for RAG functionality

use anyhow::Result;
use fabstir_llm_node::embeddings::{EmbeddingModelConfig, EmbeddingModelManager};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Debug, Serialize, Deserialize)]
struct UploadVectorsRequest {
    #[serde(rename = "type")]
    msg_type: String,
    session_id: String,
    vectors: Vec<VectorUpload>,
    replace: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct VectorUpload {
    id: String,
    vector: Vec<f32>,
    metadata: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchVectorsRequest {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "queryVector")]
    query_vector: Vec<f32>,
    k: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct UploadVectorsResponse {
    uploaded: usize,
    rejected: usize,
}

#[derive(Debug, Deserialize)]
struct SearchVectorsResponse {
    results: Vec<VectorSearchResult>,
    #[serde(rename = "searchTimeMs")]
    search_time_ms: f64,
    #[serde(rename = "totalVectors")]
    total_vectors: usize,
}

#[derive(Debug, Deserialize)]
struct VectorSearchResult {
    id: String,
    score: f32,
    metadata: serde_json::Value,
}

// Chunk text into smaller pieces for embedding
fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let sentences: Vec<&str> = text.split(". ").collect();
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for sentence in sentences {
        if current_chunk.len() + sentence.len() > chunk_size && !current_chunk.is_empty() {
            chunks.push(current_chunk.trim().to_string());
            // Keep last sentence for overlap
            current_chunk = if overlap > 0 {
                sentence.to_string() + ". "
            } else {
                String::new()
            };
        } else {
            current_chunk.push_str(sentence);
            current_chunk.push_str(". ");
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk.trim().to_string());
    }

    chunks
}

#[tokio::test]
async fn test_rag_whale_document() -> Result<()> {
    println!("\n🎬 Starting RAG End-to-End Test: The Whale");
    println!("==========================================\n");

    // Step 1: Initialize embedding model
    println!("📚 Step 1: Loading embedding model...");
    let embedding_config = EmbeddingModelConfig {
        name: "all-MiniLM-L6-v2".to_string(),
        model_path: "./models/all-MiniLM-L6-v2-onnx/model.onnx".to_string(),
        tokenizer_path: "./models/all-MiniLM-L6-v2-onnx/tokenizer.json".to_string(),
        dimensions: 384,
    };

    let manager = Arc::new(EmbeddingModelManager::new(vec![embedding_config]).await?);
    println!("✅ Embedding model loaded: all-MiniLM-L6-v2 (384D)\n");

    // Step 2: Read and chunk the document
    println!("📖 Step 2: Reading 'The Whale' document...");
    let document = std::fs::read_to_string("/workspace/tmp/The Whale v2.txt")?;
    println!("   Document size: {} characters", document.len());

    let chunks = chunk_text(&document, 500, 50);
    println!("   Created {} chunks\n", chunks.len());

    // Step 3: Generate embeddings for all chunks
    println!(
        "🧮 Step 3: Generating embeddings for {} chunks...",
        chunks.len()
    );
    let mut vectors = Vec::new();

    // Get the embedding model
    let model = manager.get_model(Some("all-MiniLM-L6-v2")).await?;

    for (i, chunk) in chunks.iter().enumerate() {
        let embedding = model.embed(chunk).await?;
        println!(
            "   ✓ Chunk {}: {} chars → {} dimensions",
            i + 1,
            chunk.len(),
            embedding.len()
        );

        vectors.push(VectorUpload {
            id: format!("whale-chunk-{}", i),
            vector: embedding,
            metadata: json!({
                "text": chunk,
                "source": "The Whale v2.txt",
                "chunk_index": i,
            }),
        });
    }
    println!("✅ Generated {} embeddings\n", vectors.len());

    // Step 4: Connect to WebSocket
    println!("🔌 Step 4: Connecting to WebSocket...");
    let ws_url = "ws://localhost:8083/v1/ws";
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut write, mut read) = ws_stream.split();
    println!("✅ Connected to {}\n", ws_url);

    // Step 5: Wait for welcome message
    println!("👋 Step 5: Waiting for welcome message...");
    if let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            let welcome: serde_json::Value = serde_json::from_str(&text)?;
            println!("   Received: {:?}\n", welcome);
        }
    }

    // Step 6: Initialize session
    println!("🎯 Step 6: Initializing session...");
    let session_id = "test-whale-rag-session";
    let session_init = json!({
        "type": "session_init",
        "session_id": session_id,
        "job_id": 99999,
        "chain_id": 84532
    });
    write.send(Message::Text(session_init.to_string())).await?;

    // Wait for session_init_ack
    if let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            let ack: serde_json::Value = serde_json::from_str(&text)?;
            println!("   Session initialized: {:?}\n", ack);
        }
    }

    // Step 7: Upload vectors
    println!("📤 Step 7: Uploading {} vectors...", vectors.len());
    let upload_request = UploadVectorsRequest {
        msg_type: "uploadVectors".to_string(),
        session_id: session_id.to_string(),
        vectors,
        replace: false,
    };

    let upload_json = serde_json::to_string(&upload_request)?;
    write.send(Message::Text(upload_json)).await?;

    // Wait for upload response
    if let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            let response: UploadVectorsResponse = serde_json::from_str(&text)?;
            println!("✅ Upload complete:");
            println!("   - Uploaded: {}", response.uploaded);
            println!("   - Rejected: {}\n", response.rejected);
        }
    }

    // Step 8: Generate query embedding
    println!("🔍 Step 8: Generating query embedding...");
    let query_text = "lonely fat man";
    println!("   Query: \"{}\"", query_text);

    let query_embedding = model.embed(query_text).await?;
    println!("   Query embedding: {} dimensions\n", query_embedding.len());

    // Step 9: Search vectors
    println!("🎯 Step 9: Searching for relevant chunks...");
    let search_request = SearchVectorsRequest {
        msg_type: "searchVectors".to_string(),
        session_id: session_id.to_string(),
        query_vector: query_embedding,
        k: 3,
        threshold: None, // No threshold - return top-k regardless of score
    };

    let search_json = serde_json::to_string(&search_request)?;
    write.send(Message::Text(search_json)).await?;

    // Wait for search response
    if let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            println!("📥 Raw response: {}\n", text);
            let response: SearchVectorsResponse = serde_json::from_str(&text)?;
            println!("✅ Search complete in {:.2}ms", response.search_time_ms);
            println!("   Found {} results:\n", response.results.len());

            for (i, result) in response.results.iter().enumerate() {
                println!("   📄 Result {} (score: {:.4}):", i + 1, result.score);
                println!("      ID: {}", result.id);
                if let Some(text) = result.metadata.get("text") {
                    let text_str = text.as_str().unwrap_or("");
                    let preview = if text_str.len() > 200 {
                        format!("{}...", &text_str[..200])
                    } else {
                        text_str.to_string()
                    };
                    println!("      Text: {}", preview);
                }
                println!();
            }

            // Verify results are returned (session persistence works!)
            assert!(
                !response.results.is_empty(),
                "Should find at least one result"
            );
            assert_eq!(
                response.results.len(),
                3,
                "Should return exactly 3 results (k=3)"
            );

            println!("🎬 Verification:");
            println!(
                "   ✓ Found {} results from {} total vectors",
                response.results.len(),
                response.total_vectors
            );
            println!("   ✓ Session persistence working - vectors uploaded in one message, retrieved in next!");
            println!("   ✓ Search completed in {:.2}ms", response.search_time_ms);

            // Display top results with scores
            for (i, result) in response.results.iter().enumerate() {
                println!("\n   Result {} (score: {:.4}):", i + 1, result.score);
                if let Some(text) = result.metadata.get("text").and_then(|v| v.as_str()) {
                    let preview = if text.len() > 100 {
                        format!("{}...", &text[..100])
                    } else {
                        text.to_string()
                    };
                    println!("      {}", preview);
                }
            }

            println!("\n🎉 SUCCESS! RAG session persistence is working!");
            println!("   ✅ Vectors uploaded via uploadVectors message");
            println!("   ✅ Session persisted across messages");
            println!("   ✅ Vectors retrieved via searchVectors message");
            println!("   ✅ v8.3.4 fix confirmed - session store prevents vector loss!");
        }
    }

    println!("\n==========================================");
    println!("🎬 RAG End-to-End Test Complete!");

    Ok(())
}
