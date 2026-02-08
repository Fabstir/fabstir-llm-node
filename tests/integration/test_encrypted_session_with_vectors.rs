// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Phase 3.2: Encrypted Session Init with Vector Database
// Test encrypted transmission of vector_database info via WebSocket

use fabstir_llm_node::crypto::{decrypt_with_aead, derive_shared_key, encrypt_with_aead};
use k256::{ecdh::EphemeralSecret, elliptic_curve::sec1::ToEncodedPoint, PublicKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct VectorDatabaseInfo {
    #[serde(rename = "manifestPath")]
    pub manifest_path: String,
    #[serde(rename = "userAddress")]
    pub user_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestSessionInitData {
    #[serde(rename = "jobId")]
    pub job_id: String,
    #[serde(rename = "modelName")]
    pub model_name: String,
    #[serde(rename = "sessionKey")]
    pub session_key: String,
    #[serde(rename = "pricePerToken")]
    pub price_per_token: u64,
    #[serde(rename = "vectorDatabase", skip_serializing_if = "Option::is_none")]
    pub vector_database: Option<VectorDatabaseInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedSessionPayload {
    #[serde(rename = "ephPub")]
    pub eph_pub: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub signature: Vec<u8>,
    pub aad: Vec<u8>,
}

#[tokio::test]
async fn test_encrypted_session_init_with_vector_database() {
    println!("\n🧪 Phase 3.2: Encrypted Session Init with Vector Database");
    println!("=========================================================\n");

    // 1. Create session data with vector_database field
    println!("📝 Step 1: Creating session init payload with vector_database...");
    let session_data = TestSessionInitData {
        job_id: "test-job-s5-vectors".to_string(),
        model_name: "llama-3".to_string(),
        session_key: "0x1234567890abcdef".to_string(),
        price_per_token: 2000,
        vector_database: Some(VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xTEST/test-db/manifest.json".to_string(),
            user_address: "0xTEST".to_string(),
        }),
    };

    let session_json = serde_json::to_string(&session_data).unwrap();
    println!("✅ Session data created with vector_database field\n");

    // 2. Generate ephemeral keypair for ECDH
    println!("🔐 Step 2: Generating ECDH keypair...");
    let eph_secret = EphemeralSecret::random(&mut OsRng);
    let eph_public = PublicKey::from(&eph_secret);
    let eph_public_bytes = eph_public.to_encoded_point(true).as_bytes().to_vec();
    println!("✅ Ephemeral keypair generated\n");

    // 3. Generate node keypair (simulating host node)
    println!("🏠 Step 3: Simulating host node keypair...");
    let node_secret_bytes = [0x42u8; 32]; // Mock node private key
    println!("✅ Node keypair ready\n");

    // 4. Derive shared secret (ECDH)
    println!("🤝 Step 4: Deriving shared secret via ECDH...");
    let shared_key = derive_shared_key(&eph_public_bytes, &node_secret_bytes).unwrap();
    println!("✅ Shared secret derived ({} bytes)\n", shared_key.len());

    // 5. Encrypt session data with XChaCha20-Poly1305
    println!("🔒 Step 5: Encrypting session data...");
    let nonce: [u8; 24] = [0x01; 24]; // XChaCha20 uses 24-byte nonce
    let aad = b"session_init"; // Additional authenticated data

    let ciphertext = encrypt_with_aead(session_json.as_bytes(), &nonce, aad, &shared_key).unwrap();
    println!(
        "✅ Session data encrypted ({} bytes → {} bytes)\n",
        session_json.len(),
        ciphertext.len()
    );

    // 6. Create encrypted payload
    println!("📦 Step 6: Creating encrypted payload...");
    let payload = EncryptedSessionPayload {
        eph_pub: eph_public.to_encoded_point(true).as_bytes().to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: vec![0u8; 65], // Mock signature
        aad: aad.to_vec(),
    };
    println!("✅ Payload created\n");

    // 7. Simulate decryption on node side
    println!("🔓 Step 7: Decrypting on node side...");

    // Node derives shared secret using ephemeral public key (already in bytes)
    let shared_key_node = derive_shared_key(&payload.eph_pub, &node_secret_bytes).unwrap();

    // Decrypt
    let nonce: [u8; 24] = payload.nonce.as_slice().try_into().unwrap();
    let decrypted_bytes =
        decrypt_with_aead(&payload.ciphertext, &nonce, &payload.aad, &shared_key_node).unwrap();

    let decrypted_json = String::from_utf8(decrypted_bytes).unwrap();
    println!("✅ Decrypted successfully\n");

    // 8. Verify vector_database field intact
    println!("✔️  Step 8: Verifying vector_database field...");
    let session_init: TestSessionInitData = serde_json::from_str(&decrypted_json).unwrap();

    assert!(
        session_init.vector_database.is_some(),
        "vector_database should be present"
    );

    let vdb = session_init.vector_database.unwrap();
    assert_eq!(
        vdb.manifest_path,
        "home/vector-databases/0xTEST/test-db/manifest.json"
    );
    assert_eq!(vdb.user_address, "0xTEST");

    println!("✅ Vector database info verified:");
    println!("   Manifest: {}", vdb.manifest_path);
    println!("   Owner: {}", vdb.user_address);

    // 9. Verify all other fields intact
    println!("\n✔️  Step 9: Verifying other fields...");
    assert_eq!(session_init.job_id, "test-job-s5-vectors");
    assert_eq!(session_init.model_name, "llama-3");
    assert_eq!(session_init.price_per_token, 2000);
    println!("✅ All fields verified\n");

    println!("🎉 Phase 3.2 Test PASSED - Encrypted vector_database transmission works!\n");
}

#[tokio::test]
async fn test_session_init_without_vector_database() {
    println!("\n🧪 Phase 3.2b: Session Init WITHOUT vector_database (backward compatibility)");
    println!("===============================================================================\n");

    // Create session data WITHOUT vector_database field
    let session_data = TestSessionInitData {
        job_id: "test-job-no-vectors".to_string(),
        model_name: "llama-3".to_string(),
        session_key: "0xabcdef".to_string(),
        price_per_token: 1000,
        vector_database: None,
    };

    let session_json = serde_json::to_string(&session_data).unwrap();

    // Verify JSON doesn't include vectorDatabase field (skip_serializing_if)
    let json_value: serde_json::Value = serde_json::from_str(&session_json).unwrap();
    assert!(
        json_value.get("vectorDatabase").is_none(),
        "vectorDatabase should be omitted when None"
    );

    println!("✅ Backward compatibility verified:");
    println!("   vectorDatabase field correctly omitted when None");

    // Verify we can still deserialize
    let session_init: TestSessionInitData = serde_json::from_str(&session_json).unwrap();
    assert!(session_init.vector_database.is_none());

    println!("   Deserialization works without vectorDatabase field");
    println!("\n🎉 Backward compatibility test PASSED\n");
}

#[tokio::test]
async fn test_encrypted_session_with_large_manifest_path() {
    println!("\n🧪 Phase 3.2c: Encrypted Session with Long Manifest Path");
    println!("========================================================\n");

    // Test with very long manifest path (edge case)
    let long_path = format!(
        "home/vector-databases/{}/my-very-long-database-name-with-lots-of-characters/manifest.json",
        "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7"
    );

    let session_data = TestSessionInitData {
        job_id: "test-long-path".to_string(),
        model_name: "llama-3".to_string(),
        session_key: "0x123".to_string(),
        price_per_token: 2000,
        vector_database: Some(VectorDatabaseInfo {
            manifest_path: long_path.clone(),
            user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
        }),
    };

    let session_json = serde_json::to_string(&session_data).unwrap();

    // Encrypt
    let eph_secret = EphemeralSecret::random(&mut OsRng);
    let eph_public = PublicKey::from(&eph_secret);
    let eph_public_bytes = eph_public.to_encoded_point(true).as_bytes().to_vec();

    let node_secret_bytes = [0x42u8; 32];

    let shared_key = derive_shared_key(&eph_public_bytes, &node_secret_bytes).unwrap();
    let nonce: [u8; 24] = [0x01; 24];
    let aad = b"session_init";

    let ciphertext = encrypt_with_aead(session_json.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Decrypt and verify
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &shared_key).unwrap();
    let decrypted_json = String::from_utf8(decrypted).unwrap();
    let session_init: TestSessionInitData = serde_json::from_str(&decrypted_json).unwrap();

    let vdb = session_init.vector_database.unwrap();
    assert_eq!(vdb.manifest_path, long_path);

    println!("✅ Long manifest path handled correctly:");
    println!("   Path length: {} characters", long_path.len());
    println!("   Encryption/decryption successful");
    println!("\n🎉 Long path test PASSED\n");
}

#[tokio::test]
async fn test_json_serialization_formats() {
    println!("\n🧪 Phase 3.2d: JSON Serialization Format Compatibility");
    println!("======================================================\n");

    // Test camelCase serialization (for SDK compatibility)
    let session_data = TestSessionInitData {
        job_id: "test123".to_string(),
        model_name: "llama-3".to_string(),
        session_key: "0xabc".to_string(),
        price_per_token: 3000,
        vector_database: Some(VectorDatabaseInfo {
            manifest_path: "home/test/manifest.json".to_string(),
            user_address: "0xTEST".to_string(),
        }),
    };

    let json = serde_json::to_string_pretty(&session_data).unwrap();
    println!("📄 Serialized JSON:\n{}\n", json);

    // Verify camelCase field names
    let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        json_value.get("jobId").is_some(),
        "Should use camelCase: jobId"
    );
    assert!(
        json_value.get("modelName").is_some(),
        "Should use camelCase: modelName"
    );
    assert!(
        json_value.get("sessionKey").is_some(),
        "Should use camelCase: sessionKey"
    );
    assert!(
        json_value.get("pricePerToken").is_some(),
        "Should use camelCase: pricePerToken"
    );
    assert!(
        json_value.get("vectorDatabase").is_some(),
        "Should use camelCase: vectorDatabase"
    );

    // Verify nested object
    let vdb = json_value.get("vectorDatabase").unwrap();
    assert!(
        vdb.get("manifestPath").is_some(),
        "Should use camelCase: manifestPath"
    );
    assert!(
        vdb.get("userAddress").is_some(),
        "Should use camelCase: userAddress"
    );

    println!("✅ JSON serialization format verified:");
    println!("   ✓ All fields use camelCase");
    println!("   ✓ Compatible with SDK expectations");
    println!("\n🎉 Serialization format test PASSED\n");
}
