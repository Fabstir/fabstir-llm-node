// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Phase 4: Error Scenario Testing
// Comprehensive error handling tests for S5 vector loading

use fabstir_llm_node::rag::errors::VectorLoadError;
use fabstir_llm_node::rag::vector_loader::VectorLoader;
use fabstir_llm_node::storage::enhanced_s5_client::EnhancedS5Client;
use fabstir_llm_node::storage::manifest::{ChunkMetadata, Manifest};
use std::sync::Arc;

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

/// Helper to upload encrypted manifest
async fn upload_test_manifest(
    s5_client: &EnhancedS5Client,
    manifest: &Manifest,
    session_key: &[u8],
) -> String {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use rand::Rng;

    let manifest_json = serde_json::to_vec(&manifest).unwrap();
    let cipher = Aes256Gcm::new_from_slice(session_key).unwrap();
    let nonce_bytes: [u8; 12] = rand::thread_rng().gen();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, manifest_json.as_ref()).unwrap();

    let mut encrypted_manifest = nonce_bytes.to_vec();
    encrypted_manifest.extend_from_slice(&ciphertext);

    let path = format!("home/test-errors/{}/manifest.json", manifest.owner);
    s5_client
        .put(&path, encrypted_manifest, None)
        .await
        .unwrap();

    path
}

#[tokio::test]
#[ignore] // Run manually
async fn test_manifest_not_found() {
    println!("\n🧪 Phase 4.1: Manifest Not Found");
    println!("=================================\n");

    let s5_client = create_enhanced_s5_client();
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    let session_key = [0u8; 32];

    println!("🔍 Attempting to load from non-existent manifest...");
    let result = vector_loader
        .load_vectors_from_s5(
            "home/vector-databases/nonexistent/manifest.json",
            "0xTEST",
            &session_key,
            None,
        )
        .await;

    println!("📋 Result: {:?}\n", result);

    assert!(result.is_err(), "Should return error for missing manifest");

    match result.unwrap_err() {
        VectorLoadError::ManifestNotFound(path) => {
            println!("✅ Correct error type: ManifestNotFound");
            println!("   Path: {}", path);
            assert!(path.contains("nonexistent"));
        }
        VectorLoadError::ManifestDownloadFailed { path, .. } => {
            println!("✅ Correct error type: ManifestDownloadFailed");
            println!("   Path: {}", path);
            assert!(path.contains("nonexistent"));
        }
        other => {
            panic!(
                "Expected ManifestNotFound or ManifestDownloadFailed, got: {:?}",
                other
            );
        }
    }

    println!("\n🎉 Phase 4.1 Test PASSED\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_owner_mismatch() {
    println!("\n🧪 Phase 4.2: Owner Mismatch");
    println!("============================\n");

    let s5_client = create_enhanced_s5_client();
    let session_key = [0u8; 32];

    // Create manifest with owner: 0xALICE
    let now = chrono::Utc::now().timestamp_millis();
    let manifest = Manifest {
        name: "test-owner-mismatch".to_string(),
        owner: "0xALICE".to_string(),
        description: "Test owner mismatch".to_string(),
        dimensions: 384,
        vector_count: 0,
        storage_size_bytes: 0,
        created: now,
        last_accessed: now,
        updated: now,
        chunks: vec![],
        chunk_count: 0,
        folder_paths: vec![],
        deleted: false,
    };

    println!("📤 Uploading manifest with owner: {}", manifest.owner);
    let manifest_path = upload_test_manifest(&s5_client, &manifest, &session_key).await;
    println!("✅ Manifest uploaded to: {}\n", manifest_path);

    // Try to load as different owner: 0xBOB
    println!("🔐 Attempting to load as wrong owner (0xBOB)...");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    let result = vector_loader
        .load_vectors_from_s5(
            &manifest_path,
            "0xBOB", // Wrong owner!
            &session_key,
            None,
        )
        .await;

    println!("📋 Result: {:?}\n", result);

    assert!(result.is_err(), "Should reject mismatched owner");

    match result.unwrap_err() {
        VectorLoadError::OwnerMismatch { expected, actual } => {
            println!("✅ Correct error type: OwnerMismatch");
            println!("   Expected: {}", expected);
            println!("   Actual: {}", actual);
            assert_eq!(expected, "0xBOB");
            assert_eq!(actual, "0xALICE");
        }
        other => {
            panic!("Expected OwnerMismatch, got: {:?}", other);
        }
    }

    println!("\n🎉 Phase 4.2 Test PASSED - Ownership validation works!\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_corrupted_manifest() {
    println!("\n🧪 Phase 4.4: Corrupted Manifest");
    println!("=================================\n");

    let s5_client = create_enhanced_s5_client();
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    let session_key = [0u8; 32];

    // Upload corrupted (invalid JSON) data
    println!("📤 Uploading corrupted manifest...");
    let corrupted_data = b"{invalid json";
    let manifest_path = "home/test-errors/corrupted/manifest.json";
    s5_client
        .put(manifest_path, corrupted_data.to_vec(), None)
        .await
        .unwrap();
    println!("✅ Corrupted data uploaded\n");

    // Try to load it
    println!("🔍 Attempting to load corrupted manifest...");
    let result = vector_loader
        .load_vectors_from_s5(manifest_path, "0xTEST", &session_key, None)
        .await;

    println!("📋 Result: {:?}\n", result);

    assert!(result.is_err(), "Should reject corrupted manifest");

    match result.unwrap_err() {
        VectorLoadError::DecryptionFailed(_) | VectorLoadError::ManifestParseError(_) => {
            println!("✅ Correct error type (Decryption or ManifestParseError)");
        }
        other => {
            panic!(
                "Expected DecryptionFailed or ManifestParseError, got: {:?}",
                other
            );
        }
    }

    println!("\n🎉 Phase 4.4 Test PASSED\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_invalid_session_key() {
    println!("\n🧪 Phase 4: Invalid Session Key (Decryption Failure)");
    println!("====================================================\n");

    let s5_client = create_enhanced_s5_client();
    let correct_key = [0x42u8; 32];
    let wrong_key = [0xFFu8; 32];

    // Upload manifest with correct key
    let now = chrono::Utc::now().timestamp_millis();
    let manifest = Manifest {
        name: "test-wrong-key".to_string(),
        owner: "0xTEST".to_string(),
        description: "Test wrong key".to_string(),
        dimensions: 384,
        vector_count: 0,
        storage_size_bytes: 0,
        created: now,
        last_accessed: now,
        updated: now,
        chunks: vec![],
        chunk_count: 0,
        folder_paths: vec![],
        deleted: false,
    };

    println!("📤 Uploading manifest encrypted with key A...");
    let manifest_path = upload_test_manifest(&s5_client, &manifest, &correct_key).await;
    println!("✅ Manifest uploaded\n");

    // Try to decrypt with wrong key
    println!("🔑 Attempting to decrypt with wrong key B...");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    let result = vector_loader
        .load_vectors_from_s5(&manifest_path, "0xTEST", &wrong_key, None)
        .await;

    println!("📋 Result: {:?}\n", result);

    assert!(result.is_err(), "Should fail with wrong decryption key");

    match result.unwrap_err() {
        VectorLoadError::DecryptionFailed(_) => {
            println!("✅ Correct error type: DecryptionFailed");
        }
        other => {
            panic!("Expected DecryptionFailed, got: {:?}", other);
        }
    }

    println!("\n🎉 Decryption failure test PASSED\n");
}

#[tokio::test]
#[ignore] // Run manually
async fn test_dimension_mismatch() {
    println!("\n🧪 Phase 4: Dimension Mismatch Detection");
    println!("=========================================\n");

    let s5_client = create_enhanced_s5_client();
    let session_key = [0x42u8; 32];

    // Create manifest claiming 384 dimensions
    let now = chrono::Utc::now().timestamp_millis();
    let manifest = Manifest {
        name: "test-dimension-mismatch".to_string(),
        owner: "0xTEST".to_string(),
        description: "Test dimension mismatch".to_string(),
        dimensions: 384,
        vector_count: 1,
        storage_size_bytes: 1000,
        created: now,
        last_accessed: now,
        updated: now,
        chunks: vec![ChunkMetadata {
            chunk_id: 0,
            cid: "z5mock123".to_string(),
            vector_count: 1,
            size_bytes: 1000,
            updated_at: now,
        }],
        chunk_count: 1,
        folder_paths: vec![],
        deleted: false,
    };

    println!("📤 Uploading manifest (claiming 384 dimensions)...");
    let manifest_path = upload_test_manifest(&s5_client, &manifest, &session_key).await;

    // Upload chunk with WRONG dimensions (128 instead of 384)
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use fabstir_llm_node::storage::manifest::{Vector, VectorChunk};
    use rand::Rng;

    let wrong_dimension_vector = Vector {
        id: "vec_0".to_string(),
        vector: vec![0.5f32; 128], // Wrong! Should be 384
        metadata: serde_json::json!({}),
    };

    let chunk = VectorChunk {
        chunk_id: 0,
        vectors: vec![wrong_dimension_vector],
    };

    let chunk_json = serde_json::to_vec(&chunk).unwrap();
    let cipher = Aes256Gcm::new_from_slice(&session_key).unwrap();
    let nonce_bytes: [u8; 12] = rand::thread_rng().gen();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, chunk_json.as_ref()).unwrap();

    let mut encrypted_chunk = nonce_bytes.to_vec();
    encrypted_chunk.extend_from_slice(&ciphertext);

    let chunk_path = manifest_path.replace("manifest.json", "chunk-0.json");
    s5_client
        .put(&chunk_path, encrypted_chunk, None)
        .await
        .unwrap();
    println!("✅ Chunk uploaded (with 128 dimensions)\n");

    // Try to load - should detect dimension mismatch
    println!("🔍 Attempting to load (should detect mismatch)...");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    let result = vector_loader
        .load_vectors_from_s5(&manifest_path, "0xTEST", &session_key, None)
        .await;

    println!("📋 Result: {:?}\n", result);

    assert!(result.is_err(), "Should detect dimension mismatch");

    match result.unwrap_err() {
        VectorLoadError::DimensionMismatch {
            expected, actual, ..
        } => {
            println!("✅ Correct error type: DimensionMismatch");
            println!("   Expected: {}", expected);
            println!("   Actual: {}", actual);
            assert_eq!(expected, 384);
            assert_eq!(actual, 128);
        }
        other => {
            panic!("Expected DimensionMismatch, got: {:?}", other);
        }
    }

    println!("\n🎉 Dimension mismatch detection PASSED\n");
}

#[tokio::test]
#[ignore] // Manual only - requires stopping/starting bridge
async fn test_bridge_service_unavailable() {
    println!("\n🧪 Phase 4.3: Bridge Service Unavailable");
    println!("=========================================\n");

    println!("⚠️  This test requires manually stopping the bridge service");
    println!("   Run: pkill -f 's5-bridge'\n");

    // Create client (this succeeds - client creation doesn't connect)
    println!("🔌 Creating client...");
    use fabstir_llm_node::storage::enhanced_s5_client::S5Config;

    let config = S5Config {
        api_url: "http://localhost:5522".to_string(),
        api_key: None,
        timeout_secs: 5, // Short timeout for faster test
    };

    let client = EnhancedS5Client::new(config)
        .expect("Client creation should succeed even when bridge is down");

    // Now try to make an actual request - this should fail
    println!("📡 Attempting health check on stopped bridge...");
    let result = client.health_check().await;

    println!("📋 Result: {:?}\n", result);

    assert!(
        result.is_err(),
        "Health check should fail when bridge is unavailable"
    );

    let err = result.unwrap_err();
    let err_msg = err.to_string().to_lowercase();

    assert!(
        err_msg.contains("connection") || err_msg.contains("refused") || err_msg.contains("tcp"),
        "Error should mention connection issue: {}",
        err_msg
    );

    println!("✅ Correct error: {}", err);
    println!("\n🎉 Bridge unavailable test PASSED\n");
    println!("⚠️  Remember to restart bridge: cd services/s5-bridge && npm start\n");
}

#[tokio::test]
#[ignore] // TODO: VectorLoadError doesn't have EmptyDatabase variant - that's only in VectorLoadingError (WebSocket layer)
async fn test_empty_database() {
    println!("\n🧪 Phase 4: Empty Vector Database");
    println!("==================================\n");

    let s5_client = create_enhanced_s5_client();
    let session_key = [0x42u8; 32];

    // Create manifest with 0 vectors
    let now = chrono::Utc::now().timestamp_millis();
    let manifest = Manifest {
        name: "test-empty".to_string(),
        owner: "0xTEST".to_string(),
        description: "Test empty database".to_string(),
        dimensions: 384,
        vector_count: 0,
        storage_size_bytes: 0,
        created: now,
        last_accessed: now,
        updated: now,
        chunks: vec![],
        chunk_count: 0,
        folder_paths: vec![],
        deleted: false,
    };

    println!("📤 Uploading empty manifest (0 vectors)...");
    let manifest_path = upload_test_manifest(&s5_client, &manifest, &session_key).await;
    println!("✅ Empty manifest uploaded\n");

    // Try to load
    println!("🔍 Attempting to load empty database...");
    let vector_loader = VectorLoader::new(Box::new(s5_client.clone()), 5);
    let result = vector_loader
        .load_vectors_from_s5(&manifest_path, "0xTEST", &session_key, None)
        .await;

    println!("📋 Result: {:?}\n", result);

    // TODO: VectorLoadError at RAG layer doesn't validate for empty databases
    // Empty database validation happens at WebSocket layer (VectorLoadingError::EmptyDatabase)
    // This test should either:
    // 1. Accept Ok(vec![]) as valid result
    // 2. Be moved to WebSocket layer tests
    // 3. VectorLoader should add validation for vector_count == 0

    // For now, expect success with empty vector
    assert!(
        result.is_ok(),
        "Empty database should load successfully (validation happens at WebSocket layer)"
    );
    let vectors = result.unwrap();
    assert_eq!(vectors.len(), 0, "Should return empty vector list");

    println!("\n🎉 Empty database test PASSED\n");
}
