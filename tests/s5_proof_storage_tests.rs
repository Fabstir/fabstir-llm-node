// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Sub-phase 3.1: Local Testing for S5 Proof Storage (v8.1.2)
//
// Tests for off-chain proof storage with hash+CID submission:
// 1. S5 upload functionality
// 2. Transaction encoding with hash+CID
// 3. Integration test for full checkpoint flow

use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Storage};
use sha2::{Digest, Sha256};
use ethers::abi::{Function, Param, ParamType, Token};
use ethers::types::U256;

// ============================================================================
// Step 1: Test S5 Upload
// ============================================================================

#[tokio::test]
async fn test_s5_upload_proof() {
    // Create mock S5 backend
    let s5_backend = MockS5Backend::new();

    // Create a mock proof (simulating a 221KB STARK proof)
    let job_id = 12345u64;
    let proof_bytes = vec![0xAB; 221_466]; // 221,466 bytes like real proof

    // Test upload
    let proof_path = format!("home/proofs/job_{}_proof.bin", job_id);
    let result = s5_backend.put(&proof_path, proof_bytes.clone()).await;

    assert!(result.is_ok(), "S5 upload should succeed");
    let cid = result.unwrap();

    // Verify CID returned
    assert!(!cid.is_empty(), "CID should not be empty");
    assert!(cid.starts_with("s5://"), "CID should have s5:// prefix");

    println!("✅ S5 Upload Test: CID={}", cid);
    println!("   Proof size: {} bytes ({:.2} KB)", proof_bytes.len(), proof_bytes.len() as f64 / 1024.0);
}

#[tokio::test]
async fn test_s5_proof_retrieval_by_cid() {
    // Create mock S5 backend
    let s5_backend = MockS5Backend::new();

    // Upload a proof
    let job_id = 12345u64;
    let original_proof = vec![0xCD; 221_466];
    let proof_path = format!("home/proofs/job_{}_proof.bin", job_id);

    let cid = s5_backend.put(&proof_path, original_proof.clone()).await.unwrap();

    // Test retrieval by CID
    let result = s5_backend.get_by_cid(&cid).await;

    assert!(result.is_ok(), "Proof retrieval by CID should succeed");
    let retrieved_proof = result.unwrap();

    // Verify retrieved proof matches original
    assert_eq!(retrieved_proof.len(), original_proof.len(), "Proof size should match");
    assert_eq!(retrieved_proof, original_proof, "Proof content should match exactly");

    println!("✅ S5 Retrieval Test: Retrieved {} bytes from CID", retrieved_proof.len());
}

#[tokio::test]
async fn test_s5_upload_deduplication() {
    // Create mock S5 backend
    let s5_backend = MockS5Backend::new();

    // Upload same proof twice
    let proof_bytes = vec![0xEF; 221_466];

    let cid1 = s5_backend.put("home/proofs/job_1_proof.bin", proof_bytes.clone()).await.unwrap();
    let cid2 = s5_backend.put("home/proofs/job_2_proof.bin", proof_bytes.clone()).await.unwrap();

    // Same content should generate same CID (content-addressed)
    assert_eq!(cid1, cid2, "Same proof content should generate same CID");

    println!("✅ S5 Deduplication Test: CID={}", cid1);
}

// ============================================================================
// Step 2: Test Transaction Encoding
// ============================================================================

#[test]
fn test_encode_checkpoint_call_with_hash_and_cid() {
    let job_id = 12345u64;
    let tokens_generated = 150u64;

    // Create a mock proof hash
    let proof_hash: [u8; 32] = [0x12; 32];

    // Create a mock CID
    let proof_cid = "s5://abc123def456...";

    // Encode the call
    let encoded = encode_checkpoint_call(job_id, tokens_generated, proof_hash, proof_cid.to_string());

    // Verify transaction size
    assert!(encoded.len() < 1024, "Transaction should be < 1KB, got {} bytes", encoded.len());
    assert!(encoded.len() > 100, "Transaction should have reasonable size, got {} bytes", encoded.len());

    println!("✅ Transaction Encoding Test:");
    println!("   Transaction size: {} bytes", encoded.len());
    println!("   Size reduction: 221KB → {} bytes ({}x smaller)",
             encoded.len(), 221_466 / encoded.len());

    // Verify function selector (first 4 bytes)
    assert_eq!(encoded.len() >= 4, true, "Should have function selector");

    // Function selector for submitProofOfWork(uint256,uint256,bytes32,string)
    let function_selector = &encoded[0..4];
    println!("   Function selector: 0x{}", hex::encode(function_selector));
}

#[test]
fn test_transaction_size_within_rpc_limits() {
    // Test with various CID lengths
    let job_id = 12345u64;
    let tokens = 150u64;
    let proof_hash: [u8; 32] = [0xAB; 32];

    // Test with typical CID length (50-100 bytes)
    let short_cid = "s5://".to_string() + &"a".repeat(45); // ~50 bytes
    let encoded_short = encode_checkpoint_call(job_id, tokens, proof_hash, short_cid);

    let long_cid = "s5://".to_string() + &"b".repeat(95); // ~100 bytes
    let encoded_long = encode_checkpoint_call(job_id, tokens, proof_hash, long_cid);

    // Both should be well under 128KB RPC limit
    const RPC_LIMIT: usize = 131_072; // 128KB

    assert!(encoded_short.len() < RPC_LIMIT, "Short CID transaction should fit RPC limit");
    assert!(encoded_long.len() < RPC_LIMIT, "Long CID transaction should fit RPC limit");
    assert!(encoded_long.len() < 1024, "Even long CID should be < 1KB");

    println!("✅ RPC Limit Test:");
    println!("   Short CID tx: {} bytes", encoded_short.len());
    println!("   Long CID tx: {} bytes", encoded_long.len());
    println!("   RPC limit: {} bytes", RPC_LIMIT);
    println!("   Headroom: {}x smaller than limit", RPC_LIMIT / encoded_long.len());
}

#[test]
fn test_encode_parameters_correctly() {
    let job_id = 99999u64;
    let tokens = 250u64;
    let proof_hash: [u8; 32] = [0xFF; 32];
    let proof_cid = "s5://test123".to_string();

    let encoded = encode_checkpoint_call(job_id, tokens, proof_hash, proof_cid.clone());

    // Verify we can decode the parameters
    let function = Function {
        name: "submitProofOfWork".to_string(),
        inputs: vec![
            Param {
                name: "jobId".to_string(),
                kind: ParamType::Uint(256),
                internal_type: None,
            },
            Param {
                name: "tokensClaimed".to_string(),
                kind: ParamType::Uint(256),
                internal_type: None,
            },
            Param {
                name: "proofHash".to_string(),
                kind: ParamType::FixedBytes(32),
                internal_type: None,
            },
            Param {
                name: "proofCID".to_string(),
                kind: ParamType::String,
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    // Decode should succeed
    let decoded = function.decode_input(&encoded[4..]); // Skip function selector
    assert!(decoded.is_ok(), "Should be able to decode parameters");

    let tokens_decoded = decoded.unwrap();
    assert_eq!(tokens_decoded.len(), 4, "Should have 4 parameters");

    println!("✅ Parameter Encoding Test: Successfully encoded and verified 4 parameters");
}

// ============================================================================
// Step 3: Integration Test
// ============================================================================

#[tokio::test]
async fn test_full_checkpoint_flow_with_mock_s5() {
    // Simulate the complete flow:
    // 1. Generate proof (mock)
    // 2. Calculate hash
    // 3. Upload to S5
    // 4. Encode transaction with hash+CID
    // 5. Verify transaction size

    let job_id = 12345u64;
    let tokens_generated = 150u64;

    // Step 1: Generate mock proof (221KB like real STARK proof)
    let proof_bytes = generate_mock_proof(job_id, tokens_generated);
    assert_eq!(proof_bytes.len(), 221_466, "Mock proof should be same size as real proof");

    // Step 2: Calculate SHA256 hash
    let mut hasher = Sha256::new();
    hasher.update(&proof_bytes);
    let proof_hash = hasher.finalize();
    let proof_hash_bytes: [u8; 32] = proof_hash.into();

    println!("📊 Proof hash: 0x{}", hex::encode(&proof_hash_bytes));

    // Step 3: Upload to S5
    let s5_backend = MockS5Backend::new();
    let proof_path = format!("home/proofs/job_{}_proof.bin", job_id);
    let proof_cid = s5_backend.put(&proof_path, proof_bytes.clone()).await.unwrap();

    println!("📦 Proof CID: {}", proof_cid);
    assert!(!proof_cid.is_empty(), "CID should be generated");

    // Step 4: Encode transaction with hash+CID
    let encoded_tx = encode_checkpoint_call(job_id, tokens_generated, proof_hash_bytes, proof_cid.clone());

    println!("📦 Transaction size: {} bytes (was {}KB proof - {}x reduction!)",
             encoded_tx.len(),
             proof_bytes.len() / 1024,
             proof_bytes.len() / encoded_tx.len());

    // Step 5: Verify transaction fits RPC limits
    const RPC_LIMIT: usize = 131_072; // 128KB
    assert!(encoded_tx.len() < RPC_LIMIT, "Transaction must fit RPC limit");
    assert!(encoded_tx.len() < 1024, "Transaction should be < 1KB");

    // Step 6: Verify proof can be retrieved and verified
    let retrieved_proof = s5_backend.get_by_cid(&proof_cid).await.unwrap();
    assert_eq!(retrieved_proof, proof_bytes, "Retrieved proof should match original");

    // Verify hash of retrieved proof matches
    let mut hasher2 = Sha256::new();
    hasher2.update(&retrieved_proof);
    let retrieved_hash = hasher2.finalize();
    let retrieved_hash_bytes: [u8; 32] = retrieved_hash.into();

    assert_eq!(retrieved_hash_bytes, proof_hash_bytes, "Hash should match after retrieval");

    println!("✅ Integration Test PASSED:");
    println!("   ✓ Proof generation: {} bytes", proof_bytes.len());
    println!("   ✓ Hash calculation: 32 bytes");
    println!("   ✓ S5 upload: CID generated");
    println!("   ✓ Transaction encoding: {} bytes", encoded_tx.len());
    println!("   ✓ Proof retrieval: verified");
    println!("   ✓ Hash verification: matched");
    println!("   ✓ Size reduction: {}x (221KB → {}B)",
             proof_bytes.len() / encoded_tx.len(), encoded_tx.len());
}

#[tokio::test]
async fn test_multiple_checkpoints_different_cids() {
    // Verify that different proofs generate different CIDs
    let s5_backend = MockS5Backend::new();

    let mut cids = Vec::new();

    for job_id in 1..=5 {
        let proof = generate_mock_proof(job_id, 100);
        let cid = s5_backend.put(
            &format!("home/proofs/job_{}_proof.bin", job_id),
            proof
        ).await.unwrap();

        cids.push(cid.clone());
        println!("Job {}: CID = {}", job_id, cid);
    }

    // Verify all CIDs are different (different job IDs create different proofs)
    for i in 0..cids.len() {
        for j in (i+1)..cids.len() {
            assert_ne!(cids[i], cids[j], "Different proofs should have different CIDs");
        }
    }

    println!("✅ Multiple Checkpoint Test: All {} CIDs are unique", cids.len());
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Generate a mock proof of the same size as real STARK proof
fn generate_mock_proof(job_id: u64, tokens: u64) -> Vec<u8> {
    // Create deterministic but unique proof based on job_id and tokens
    let seed = format!("job_{}:tokens_{}", job_id, tokens);
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let hash = hasher.finalize();

    // Expand to 221,466 bytes (real proof size)
    let mut proof = Vec::with_capacity(221_466);
    for i in 0u32..6921 {  // 221,466 / 32 = 6920.6875
        hasher = Sha256::new();
        hasher.update(&hash);
        hasher.update(&i.to_le_bytes());
        proof.extend_from_slice(&hasher.finalize());
    }
    proof.truncate(221_466);
    proof
}

/// Encode checkpoint call with hash + CID (v8.1.2 signature)
fn encode_checkpoint_call(
    job_id: u64,
    tokens_generated: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
) -> Vec<u8> {
    let function = Function {
        name: "submitProofOfWork".to_string(),
        inputs: vec![
            Param {
                name: "jobId".to_string(),
                kind: ParamType::Uint(256),
                internal_type: None,
            },
            Param {
                name: "tokensClaimed".to_string(),
                kind: ParamType::Uint(256),
                internal_type: None,
            },
            Param {
                name: "proofHash".to_string(),
                kind: ParamType::FixedBytes(32),
                internal_type: None,
            },
            Param {
                name: "proofCID".to_string(),
                kind: ParamType::String,
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    let tokens = vec![
        Token::Uint(U256::from(job_id)),
        Token::Uint(U256::from(tokens_generated)),
        Token::FixedBytes(proof_hash.to_vec()),
        Token::String(proof_cid),
    ];

    function.encode_input(&tokens).expect("Failed to encode submitProofOfWork call")
}

// ============================================================================
// Additional Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_s5_upload_error_handling() {
    let s5_backend = MockS5Backend::new();

    // Test invalid path (should fail validation)
    let result = s5_backend.put("/invalid/path", vec![1, 2, 3]).await;
    assert!(result.is_err(), "Invalid path should fail");

    // Test path traversal attempt
    let result = s5_backend.put("home/../etc/passwd", vec![1, 2, 3]).await;
    assert!(result.is_err(), "Path traversal should be rejected");

    println!("✅ Error Handling Test: Invalid paths correctly rejected");
}

#[tokio::test]
async fn test_quota_limit() {
    let s5_backend = MockS5Backend::new();

    // Set a quota limit
    s5_backend.set_quota_limit(500_000).await; // 500KB limit

    // First upload should succeed (221KB < 500KB)
    let proof1 = vec![0xAA; 221_466];
    let result1 = s5_backend.put("home/proofs/job_1.bin", proof1).await;
    assert!(result1.is_ok(), "First upload should succeed");

    // Second upload should succeed (221KB + 221KB < 500KB)
    let proof2 = vec![0xBB; 221_466];
    let result2 = s5_backend.put("home/proofs/job_2.bin", proof2).await;
    assert!(result2.is_ok(), "Second upload should succeed");

    // Third upload should fail (exceeds quota)
    let proof3 = vec![0xCC; 221_466];
    let result3 = s5_backend.put("home/proofs/job_3.bin", proof3).await;
    assert!(result3.is_err(), "Third upload should fail (quota exceeded)");

    println!("✅ Quota Limit Test: Quota enforcement working correctly");
}
