// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Phase 3.1: Complete Vector Database Loading Flow
// End-to-end test for S5 vector loading with real Enhanced S5.js bridge

use fabstir_llm_node::rag::vector_loader::{LoadProgress, VectorLoader};
use fabstir_llm_node::storage::enhanced_s5_client::EnhancedS5Client;
use fabstir_llm_node::storage::manifest::{ChunkMetadata, Manifest, Vector, VectorChunk};
use fabstir_llm_node::vector::hnsw::HnswIndex;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Helper to create Enhanced S5 client connected to bridge
fn create_enhanced_s5_client() -> EnhancedS5Client {
    use fabstir_llm_node::storage::enhanced_s5_client::S5Config;

    let bridge_url = std::env::var("ENHANCED_S5_URL")
        .unwrap_or_else(|_| "http://localhost:5522".to_string());

    let config = S5Config {
        api_url: bridge_url,
        api_key: None,
        timeout_secs: 30,
    };

    EnhancedS5Client::new(config)
        .expect("Failed to create Enhanced S5 client")
}

/// Helper to create test manifest and upload to S5
async fn setup_test_vector_database(
    s5_client: &EnhancedS5Client,
    owner: &str,
    num_vectors: usize,
    dimensions: usize,
) -> (String, Vec<u8>) {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use rand::Rng;

    // Generate session key
    let session_key: Vec<u8> = rand::thread_rng().gen::<[u8; 32]>().to_vec();

    // Create test vectors
    let mut all_vectors = Vec::new();
    for i in 0..num_vectors {
        let vector_data: Vec<f32> = (0..dimensions).map(|j| ((i + j) as f32) / 100.0).collect();
        all_vectors.push(Vector {
            id: format!("vec_{}", i),
            vector: vector_data,  // SDK uses 'vector', not 'embedding'
            metadata: serde_json::json!({
                "index": i,
                "test": true,
            }),
        });
    }

    // Split into chunks (100 vectors per chunk)
    let chunk_size = 100;
    let chunks: Vec<VectorChunk> = all_vectors
        .chunks(chunk_size)
        .enumerate()
        .map(|(chunk_id, vectors)| VectorChunk {
            chunk_id,
            vectors: vectors.to_vec(),
        })
        .collect();

    // Encrypt and upload chunks
    let cipher = Aes256Gcm::new_from_slice(&session_key).unwrap();
    let mut chunk_metadata = Vec::new();

    let base_path = format!("home/test-vectors/{}/test-db-{}", owner, num_vectors);

    for chunk in &chunks {
        let chunk_json = serde_json::to_vec(&chunk).unwrap();

        // Encrypt with random nonce
        let nonce_bytes: [u8; 12] = rand::thread_rng().gen();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, chunk_json.as_ref()).unwrap();

        // Prepend nonce (Web Crypto API format)
        let mut encrypted_data = nonce_bytes.to_vec();
        encrypted_data.extend_from_slice(&ciphertext);

        // Upload to S5
        let chunk_path = format!("{}/chunk-{}.json", base_path, chunk.chunk_id);
        let cid = s5_client.put(&chunk_path, encrypted_data.clone(), None).await.unwrap();

        chunk_metadata.push(ChunkMetadata {
            chunk_id: chunk.chunk_id,
            cid,  // SDK format requires CID
            vector_count: chunk.vectors.len(),
            size_bytes: encrypted_data.len() as u64,
            updated_at: chrono::Utc::now().timestamp_millis(),
        });
    }

    let now = chrono::Utc::now().timestamp_millis();
    let total_size: u64 = chunk_metadata.iter().map(|c| c.size_bytes).sum();

    // Create and encrypt manifest
    let manifest = Manifest {
        name: format!("test-db-{}", num_vectors),
        owner: owner.to_string(),
        description: format!("Test database with {} vectors", num_vectors),
        dimensions,
        vector_count: num_vectors,
        storage_size_bytes: total_size,
        created: now,
        last_accessed: now,
        updated: now,
        chunks: chunk_metadata,
        chunk_count: chunks.len(),
        folder_paths: vec![],
        deleted: false,
    };

    let manifest_json = serde_json::to_vec(&manifest).unwrap();
    let nonce_bytes: [u8; 12] = rand::thread_rng().gen();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, manifest_json.as_ref()).unwrap();

    let mut encrypted_manifest = nonce_bytes.to_vec();
    encrypted_manifest.extend_from_slice(&ciphertext);

    // Upload manifest
    let manifest_path = format!("{}/manifest.json", base_path);
    s5_client.put(&manifest_path, encrypted_manifest, None).await.unwrap();

    println!("✅ Test database uploaded to S5: {}", manifest_path);
    println!("   Vectors: {}, Chunks: {}", num_vectors, chunks.len());

    (manifest_path, session_key)
}

#[tokio::test]
#[ignore] // Run manually with: cargo test --test integration_tests test_complete_vector_loading_flow -- --ignored --nocapture
async fn test_complete_vector_loading_flow() {
    println!("\n🧪 Phase 3.1: Complete Vector Database Loading Flow");
    println!("================================================\n");

    // 1. Setup: Create Enhanced S5 client
    println!("📡 Step 1: Connecting to S5 bridge...");
    let s5_client = create_enhanced_s5_client();
    println!("✅ Connected to bridge\n");

    // 2. Setup: Upload test vector database to S5
    println!("📤 Step 2: Uploading test vector database to S5...");
    let owner = "0xTEST_E2E";
    let num_vectors = 250; // 3 chunks
    let dimensions = 384;

    let (manifest_path, session_key) = setup_test_vector_database(
        &s5_client,
        owner,
        num_vectors,
        dimensions,
    ).await;
    println!("✅ Database uploaded\n");

    // 3. Create vector loader
    println!("🔧 Step 3: Creating vector loader...");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    println!("✅ Vector loader ready\n");

    // 4. Load vectors from S5 with progress tracking
    println!("📥 Step 4: Loading vectors from S5...");
    let (progress_tx, mut progress_rx) = mpsc::channel(10);

    let loader_handle = {
        // Create new loader for the spawned task
        let loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
        let manifest_path = manifest_path.clone();
        let owner = owner.to_string();
        let session_key = session_key.clone();

        tokio::spawn(async move {
            loader
                .load_vectors_from_s5(&manifest_path, &owner, &session_key, Some(progress_tx))
                .await
        })
    };

    // 5. Monitor progress updates
    println!("📊 Step 5: Monitoring loading progress...");
    let mut manifest_downloaded = false;
    let mut chunks_downloaded = 0;
    let mut total_chunks = 0;

    while let Some(progress) = progress_rx.recv().await {
        match progress {
            LoadProgress::ManifestDownloaded => {
                manifest_downloaded = true;
                println!("   ✓ Manifest downloaded");
            }
            LoadProgress::ChunkDownloaded { chunk_id, total } => {
                chunks_downloaded += 1;
                total_chunks = total;
                println!("   ✓ Chunk {}/{} downloaded", chunk_id + 1, total);
            }
            LoadProgress::IndexBuilding => {
                println!("   ⚙️  Building HNSW index...");
            }
            LoadProgress::Complete { vector_count, duration_ms } => {
                println!("   ✅ Loading complete: {} vectors in {}ms", vector_count, duration_ms);
            }
        }
    }

    // Wait for loader to finish
    let vectors = loader_handle.await.unwrap().unwrap();

    // 6. Verify progress tracking
    assert!(manifest_downloaded, "Manifest should be downloaded");
    assert!(chunks_downloaded > 0, "At least one chunk should be downloaded");
    assert_eq!(chunks_downloaded, total_chunks, "All chunks should be downloaded");
    assert_eq!(vectors.len(), num_vectors, "Should load all vectors");
    println!("\n✅ Progress tracking verified\n");

    // 7. Build HNSW index
    println!("🏗️  Step 6: Building HNSW index...");
    let index = HnswIndex::build(vectors, dimensions).unwrap();
    println!("✅ HNSW index built\n");

    // 8. Perform search
    println!("🔍 Step 7: Performing vector search...");
    let query: Vec<f32> = (0..dimensions).map(|i| (i as f32) / 100.0).collect();
    let results = index.search(&query, 5, 0.0).unwrap();

    assert!(!results.is_empty(), "Search should return results");
    println!("✅ Found {} similar vectors", results.len());
    println!("   Top result: id={}, score={:.4}", results[0].id, results[0].score);

    println!("\n🎉 Phase 3.1 Test PASSED\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_cache_hit_performance() {
    println!("\n🧪 Phase 3.1b: Cache Hit Performance");
    println!("====================================\n");

    let s5_client = create_enhanced_s5_client();
    let owner = "0xTEST_CACHE";
    let num_vectors = 100;
    let dimensions = 384;

    // Upload test database
    println!("📤 Uploading test database...");
    let (manifest_path, session_key) = setup_test_vector_database(
        &s5_client,
        owner,
        num_vectors,
        dimensions,
    ).await;

    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);

    // First load (cache miss)
    println!("\n⏱️  First load (cache miss)...");
    let start = std::time::Instant::now();
    let vectors1 = vector_loader
        .load_vectors_from_s5(&manifest_path, owner, &session_key, None)
        .await
        .unwrap();
    let first_duration = start.elapsed();
    println!("✅ Loaded {} vectors in {:?}", vectors1.len(), first_duration);

    // Note: Current implementation doesn't have index caching yet
    // This test documents expected behavior for future cache implementation

    println!("\n🎉 Cache test completed\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_concurrent_session_loading() {
    println!("\n🧪 Phase 3.1c: Concurrent Session Loading");
    println!("========================================\n");

    let s5_client = create_enhanced_s5_client();
    let owner = "0xTEST_CONCURRENT";
    let num_vectors = 100;
    let dimensions = 384;

    // Upload test database
    println!("📤 Uploading test database...");
    let (manifest_path, session_key) = setup_test_vector_database(
        &s5_client,
        owner,
        num_vectors,
        dimensions,
    ).await;

    println!("\n🚀 Launching 3 concurrent loading sessions...");

    let handles: Vec<_> = (0..3)
        .map(|i| {
            let s5_client = s5_client.clone();
            let manifest_path = manifest_path.clone();
            let owner = owner.to_string();
            let session_key = session_key.clone();

            tokio::spawn(async move {
                println!("   Session {} starting...", i);
                let loader = VectorLoader::new(Box::new(s5_client), 5);
                let result = loader
                    .load_vectors_from_s5(&manifest_path, &owner, &session_key, None)
                    .await;
                println!("   Session {} completed", i);
                result
            })
        })
        .collect();

    // Wait for all sessions to complete
    let results = futures::future::join_all(handles).await;

    // Verify all succeeded
    for (i, result) in results.iter().enumerate() {
        let vectors = result.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(vectors.len(), num_vectors, "Session {} should load all vectors", i);
    }

    println!("\n✅ All 3 sessions loaded successfully");
    println!("🎉 Concurrent loading test PASSED\n");
}
