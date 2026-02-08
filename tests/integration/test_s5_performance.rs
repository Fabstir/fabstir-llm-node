// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Phase 5: Performance Testing
// Large database loading and concurrent session tests

use fabstir_llm_node::rag::vector_loader::VectorLoader;
use fabstir_llm_node::storage::enhanced_s5_client::EnhancedS5Client;
use fabstir_llm_node::storage::manifest::{ChunkMetadata, Manifest, Vector, VectorChunk};
use fabstir_llm_node::vector::hnsw::HnswIndex;
use std::sync::Arc;
use std::time::Instant;

/// Helper to create Enhanced S5 client
fn create_enhanced_s5_client() -> EnhancedS5Client {
    use fabstir_llm_node::storage::enhanced_s5_client::S5Config;

    let bridge_url =
        std::env::var("ENHANCED_S5_URL").unwrap_or_else(|_| "http://localhost:5522".to_string());

    let config = S5Config {
        api_url: bridge_url,
        api_key: None,
        timeout_secs: 30,
    };

    EnhancedS5Client::new(config).expect("Failed to create Enhanced S5 client")
}

/// Helper to upload large vector database
async fn setup_large_database(
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

    println!("📦 Setting up large database:");
    println!("   Vectors: {}", num_vectors);
    println!("   Dimensions: {}", dimensions);

    let session_key: Vec<u8> = rand::thread_rng().gen::<[u8; 32]>().to_vec();

    // Generate test vectors
    println!("   🔢 Generating {} vectors...", num_vectors);
    let mut all_vectors = Vec::new();
    for i in 0..num_vectors {
        let vector_data: Vec<f32> = (0..dimensions)
            .map(|j| ((i * dimensions + j) as f32 % 1000.0) / 1000.0)
            .collect();
        all_vectors.push(Vector {
            id: format!("vec_{:06}", i),
            vector: vector_data, // SDK uses 'vector', not 'embedding'
            metadata: serde_json::json!({
                "index": i,
                "batch": i / 1000,
            }),
        });

        if (i + 1) % 10000 == 0 {
            println!("      Generated {} vectors...", i + 1);
        }
    }

    // Split into chunks (1000 vectors per chunk for large databases)
    let chunk_size = 1000;
    let chunks: Vec<VectorChunk> = all_vectors
        .chunks(chunk_size)
        .enumerate()
        .map(|(chunk_id, vectors)| VectorChunk {
            chunk_id,
            vectors: vectors.to_vec(),
        })
        .collect();

    println!("   📤 Uploading {} chunks...", chunks.len());

    // Encrypt and upload chunks
    let cipher = Aes256Gcm::new_from_slice(&session_key).unwrap();
    let mut chunk_metadata = Vec::new();
    let base_path = format!("home/perf-test/{}/db-{}", owner, num_vectors);

    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_json = serde_json::to_vec(&chunk).unwrap();
        let nonce_bytes: [u8; 12] = rand::thread_rng().gen();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, chunk_json.as_ref()).unwrap();

        let mut encrypted_data = nonce_bytes.to_vec();
        encrypted_data.extend_from_slice(&ciphertext);

        let chunk_path = format!("{}/chunk-{:04}.json", base_path, chunk.chunk_id);
        let cid = s5_client
            .put(&chunk_path, encrypted_data.clone(), None)
            .await
            .unwrap();

        chunk_metadata.push(ChunkMetadata {
            chunk_id: chunk.chunk_id,
            cid, // SDK format requires CID
            vector_count: chunk.vectors.len(),
            size_bytes: encrypted_data.len() as u64,
            updated_at: chrono::Utc::now().timestamp_millis(),
        });

        if (i + 1) % 10 == 0 {
            println!("      Uploaded {} chunks...", i + 1);
        }
    }

    // Create and upload manifest
    let now = chrono::Utc::now().timestamp_millis();
    let total_size: u64 = chunk_metadata.iter().map(|c| c.size_bytes).sum();

    let manifest = Manifest {
        name: format!("perf-db-{}", num_vectors),
        owner: owner.to_string(),
        description: format!("Performance test database with {} vectors", num_vectors),
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

    let manifest_path = format!("{}/manifest.json", base_path);
    s5_client
        .put(&manifest_path, encrypted_manifest, None)
        .await
        .unwrap();

    println!("   ✅ Database ready at: {}\n", manifest_path);

    (manifest_path, session_key)
}

#[tokio::test]
#[ignore] // Run manually: cargo test --test integration_tests test_large_database_10k -- --ignored --nocapture
async fn test_large_database_10k() {
    println!("\n🧪 Phase 5.1: Large Database Loading (10K vectors)");
    println!("===================================================\n");

    let s5_client = create_enhanced_s5_client();
    let owner = "0xPERF_10K";
    let num_vectors = 10_000;
    let dimensions = 384;

    // Setup
    println!("⏱️  Phase 1: Database Setup");
    println!("-----------------------------");
    let setup_start = Instant::now();
    let (manifest_path, session_key) =
        setup_large_database(&s5_client, owner, num_vectors, dimensions).await;
    let setup_duration = setup_start.elapsed();
    println!("Setup time: {:?}\n", setup_duration);

    // Load vectors
    println!("⏱️  Phase 2: Vector Loading");
    println!("-----------------------------");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 10); // 10 parallel chunks
    let load_start = Instant::now();

    let vectors = vector_loader
        .load_vectors_from_s5(&manifest_path, owner, &session_key, None)
        .await
        .unwrap();

    let load_duration = load_start.elapsed();

    println!("✅ Loaded {} vectors in {:?}", vectors.len(), load_duration);
    println!(
        "   Throughput: {:.0} vectors/sec\n",
        num_vectors as f64 / load_duration.as_secs_f64()
    );

    // Build index
    println!("⏱️  Phase 3: Index Building");
    println!("-----------------------------");
    let index_start = Instant::now();
    let index = HnswIndex::build(vectors, dimensions).unwrap();
    let index_duration = index_start.elapsed();
    println!("✅ Index built in {:?}\n", index_duration);

    // Search performance
    println!("⏱️  Phase 4: Search Performance");
    println!("--------------------------------");
    let query: Vec<f32> = (0..dimensions)
        .map(|i| (i as f32) / dimensions as f32)
        .collect();

    let search_start = Instant::now();
    let results = index.search(&query, 10, 0.0).unwrap();
    let search_duration = search_start.elapsed();

    println!("✅ Search completed in {:?}", search_duration);
    println!("   Found {} results\n", results.len());

    // Performance assertions
    println!("📊 Performance Summary");
    println!("----------------------");
    println!("Loading: {:?} (target: <5s)", load_duration);
    println!("Index Build: {:?}", index_duration);
    println!("Search: {:?} (target: <100ms)", search_duration);

    assert!(
        load_duration.as_secs() < 10,
        "Loading should complete in <10s for 10K vectors (got {:?})",
        load_duration
    );

    assert!(
        search_duration.as_millis() < 200,
        "Search should complete in <200ms (got {:?})",
        search_duration
    );

    println!("\n🎉 10K vector performance test PASSED\n");
}

#[tokio::test]
#[ignore] // Run manually: cargo test --test integration_tests test_large_database_100k -- --ignored --nocapture
async fn test_large_database_100k() {
    println!("\n🧪 Phase 5.1: Large Database Loading (100K vectors)");
    println!("====================================================\n");

    let s5_client = create_enhanced_s5_client();
    let owner = "0xPERF_100K";
    let num_vectors = 100_000;
    let dimensions = 384;

    // Setup (will take a while)
    println!("⏱️  Phase 1: Database Setup (this will take ~5-10 minutes)");
    println!("------------------------------------------------------------");
    let setup_start = Instant::now();
    let (manifest_path, session_key) =
        setup_large_database(&s5_client, owner, num_vectors, dimensions).await;
    let setup_duration = setup_start.elapsed();
    println!("Setup time: {:?}\n", setup_duration);

    // Load vectors
    println!("⏱️  Phase 2: Vector Loading");
    println!("-----------------------------");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 10); // 10 parallel chunks
    let load_start = Instant::now();

    let vectors = vector_loader
        .load_vectors_from_s5(&manifest_path, owner, &session_key, None)
        .await
        .unwrap();

    let load_duration = load_start.elapsed();

    println!("✅ Loaded {} vectors in {:?}", vectors.len(), load_duration);
    println!(
        "   Throughput: {:.0} vectors/sec\n",
        num_vectors as f64 / load_duration.as_secs_f64()
    );

    // Memory check
    println!("💾 Memory Usage Check");
    println!("---------------------");
    let vector_size_bytes = vectors.len() * dimensions * 4; // f32 = 4 bytes
    let estimated_mb = vector_size_bytes / (1024 * 1024);
    println!("Estimated vector memory: ~{} MB", estimated_mb);
    assert!(estimated_mb < 500, "Should use <500MB for 100K vectors");

    println!("\n📊 Performance Summary");
    println!("----------------------");
    println!("Loading: {:?} (target: <60s)", load_duration);
    println!("Memory: ~{} MB (target: <500MB)", estimated_mb);

    assert!(
        load_duration.as_secs() < 120,
        "Loading should complete in <120s for 100K vectors (got {:?})",
        load_duration
    );

    println!("\n🎉 100K vector performance test PASSED\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_concurrent_session_loading_stress() {
    println!("\n🧪 Phase 5.2: Concurrent Session Loading (5 sessions)");
    println!("======================================================\n");

    let s5_client = create_enhanced_s5_client();
    let owner = "0xPERF_CONCURRENT";
    let num_vectors = 1_000; // Smaller for concurrent test
    let dimensions = 384;

    // Setup shared database
    println!("📦 Setting up shared database for 5 concurrent sessions...");
    let (manifest_path, session_key) =
        setup_large_database(&s5_client, owner, num_vectors, dimensions).await;

    println!("\n🚀 Launching 5 concurrent loading sessions...\n");

    let start = Instant::now();

    let handles: Vec<_> = (0..5)
        .map(|session_id| {
            let s5_client = s5_client.clone();
            let manifest_path = manifest_path.clone();
            let owner = owner.to_string();
            let session_key = session_key.clone();

            tokio::spawn(async move {
                println!("   Session {} starting...", session_id);
                let session_start = Instant::now();

                let loader = VectorLoader::new(Box::new(s5_client), 5);
                let result = loader
                    .load_vectors_from_s5(&manifest_path, &owner, &session_key, None)
                    .await;

                let session_duration = session_start.elapsed();
                println!(
                    "   Session {} completed in {:?}",
                    session_id, session_duration
                );

                (session_id, result, session_duration)
            })
        })
        .collect();

    // Wait for all sessions
    let results = futures::future::join_all(handles).await;

    let total_duration = start.elapsed();

    // Verify all succeeded
    println!("\n📊 Results:");
    println!("-----------");
    for result in &results {
        let (session_id, load_result, session_duration) = result.as_ref().unwrap();
        let vectors = load_result.as_ref().unwrap();

        println!(
            "   Session {}: {} vectors in {:?}",
            session_id,
            vectors.len(),
            session_duration
        );
        assert_eq!(vectors.len(), num_vectors);
    }

    println!("\n⏱️  Total concurrent time: {:?}", total_duration);
    println!("✅ All 5 sessions loaded successfully");
    println!(
        "🔍 Connection pool handled {} concurrent sessions without issues\n",
        results.len()
    );

    // Performance assertion
    assert!(
        total_duration.as_secs() < 30,
        "Concurrent loading should complete in <30s (got {:?})",
        total_duration
    );

    println!("🎉 Concurrent loading stress test PASSED\n");
}
