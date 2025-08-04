// tests/models/test_gdpr.rs - Decentralized GDPR compliance tests

use anyhow::Result;
use fabstir_llm_node::models::{
    DecentralizedGdprManager, GdprConfig, SignedRequest,
    ConsentRecord, DeletionBroadcast, PortableDataPackage, 
    RegionalPreference, ZkComplianceProof, EncryptedData,
    P2PGdprNetwork, OnChainConsent,
    AnonymizationProof, AuditProof, ComplianceAttestation,
};
use fabstir_llm_node::models::gdpr::UserControlledAnonymization;
use ed25519_dalek::{SigningKey, VerifyingKey};
use std::collections::HashMap;
use chrono::{Utc, Duration};
use rand::rngs::OsRng;

async fn create_test_manager() -> Result<DecentralizedGdprManager> {
    let config = GdprConfig {
        enable_user_sovereignty: true,
        p2p_network: "libp2p".to_string(),
        consent_blockchain: "base-sepolia".to_string(),
        zk_proof_system: "groth16".to_string(),
        user_controlled_encryption: true,
        no_backdoors: true, // Explicitly no backdoors
        regional_nodes_available: vec!["EU".to_string(), "US".to_string(), "ASIA".to_string()],
    };
    
    DecentralizedGdprManager::new(config).await
}

fn generate_user_keys() -> (SigningKey, VerifyingKey) {
    use rand::RngCore;
    let mut secret_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut secret_bytes);
    let signing_key = SigningKey::from_bytes(&secret_bytes);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

#[tokio::test]
async fn test_user_controlled_data_storage() {
    let manager = create_test_manager().await.unwrap();
    
    // User generates their own keys - never shared
    let (signing_key, verifying_key) = generate_user_keys();
    let user_id = bs58::encode(verifying_key.as_bytes()).into_string();
    
    // User encrypts their own data locally
    let private_data = "My medical condition is diabetes";
    let encrypted = manager.encrypt_for_user(
        private_data.as_bytes(),
        &verifying_key,
    ).await.unwrap();
    
    // Store encrypted data - node cannot decrypt
    let storage_proof = manager.store_encrypted_data(
        &user_id,
        encrypted.clone(),
        RegionalPreference::EU, // Preference, not restriction
    ).await.unwrap();
    
    assert!(!storage_proof.storage_id.is_empty());
    assert!(storage_proof.node_signature.is_valid());
    
    // Only user can decrypt with their private key
    let retrieved = manager.retrieve_encrypted_data(&storage_proof.storage_id).await.unwrap();
    let decrypted = manager.decrypt_with_user_key(
        &retrieved,
        &signing_key,
    ).await.unwrap();
    
    assert_eq!(decrypted, private_data.as_bytes());
    
    // Verify node cannot access data
    assert!(manager.node_can_read_data(&storage_proof.storage_id).await.is_err());
}

#[tokio::test]
async fn test_decentralized_consent_management() {
    let manager = create_test_manager().await.unwrap();
    let (signing_key, verifying_key) = generate_user_keys();
    
    // User creates and signs their own consent
    let consent = ConsentRecord {
        user_pubkey: verifying_key.clone(),
        purposes: vec!["inference".to_string(), "caching".to_string()],
        timestamp: Utc::now(),
        expiry: Some(Utc::now() + Duration::days(365)),
        version: "1.0".to_string(),
    };
    
    // User signs consent with their private key
    let signed_consent = manager.sign_consent(
        consent,
        &signing_key,
    ).await.unwrap();
    
    // Broadcast to blockchain (no central database)
    let tx_hash = manager.broadcast_consent_to_chain(
        signed_consent.clone()
    ).await.unwrap();
    
    assert!(!tx_hash.is_empty());
    
    // Any node can verify consent from blockchain
    let verified = manager.verify_consent_on_chain(
        &verifying_key,
        "inference",
    ).await.unwrap();
    
    assert!(verified.is_valid);
    assert_eq!(verified.signer.as_bytes(), verifying_key.as_bytes());
    
    // No central authority needed for verification
    assert!(!verified.requires_central_validation);
}

#[tokio::test]
async fn test_p2p_right_to_erasure() {
    let manager = create_test_manager().await.unwrap();
    let (signing_key, verifying_key) = generate_user_keys();
    let user_id = bs58::encode(verifying_key.as_bytes()).into_string();
    
    // Store some encrypted data across multiple nodes
    for i in 0..3 {
        let data = format!("Data piece {}", i);
        let encrypted = manager.encrypt_for_user(data.as_bytes(), &verifying_key).await.unwrap();
        manager.store_encrypted_data(&user_id, encrypted, RegionalPreference::Any).await.unwrap();
    }
    
    // User creates deletion request signed with their key
    let deletion_request = DeletionBroadcast {
        user_pubkey: verifying_key.clone(),
        delete_all_data: true,
        timestamp: Utc::now(),
        nonce: rand::random::<u64>(),
    };
    
    let signed_request = manager.sign_deletion_request(
        deletion_request,
        &signing_key,
    ).await.unwrap();
    
    // Broadcast deletion request to P2P network
    let deletion_proof = manager.broadcast_deletion_to_p2p(
        signed_request
    ).await.unwrap();
    
    // Nodes independently verify signature and delete
    assert!(deletion_proof.nodes_responded.len() >= 3);
    for node_proof in &deletion_proof.nodes_responded {
        assert!(node_proof.signature_verified);
        assert!(node_proof.data_deleted);
        assert!(!node_proof.data_retained); // No backdoor retention
    }
    
    // Generate ZK proof of deletion for compliance
    let zk_proof = manager.generate_deletion_proof(
        &deletion_proof
    ).await.unwrap();
    
    assert!(zk_proof.is_valid());
    assert!(zk_proof.proves_complete_deletion());
}

#[tokio::test]
async fn test_user_controlled_data_portability() {
    let manager = create_test_manager().await.unwrap();
    let (signing_key, verifying_key) = generate_user_keys();
    let user_id = bs58::encode(verifying_key.as_bytes()).into_string();
    
    // Store various types of user data
    let inference_data = "AI response to my query";
    let preference_data = r#"{"model": "llama", "temperature": 0.7}"#;
    
    for data in [inference_data, preference_data] {
        let encrypted = manager.encrypt_for_user(data.as_bytes(), &verifying_key).await.unwrap();
        manager.store_encrypted_data(&user_id, encrypted, RegionalPreference::Any).await.unwrap();
    }
    
    // User requests their data with signed request
    let export_request = SignedRequest::new(
        "EXPORT_ALL_MY_DATA",
        &signing_key,
    );
    
    // Nodes return encrypted data that only user can decrypt
    let portable_package = manager.collect_user_data_p2p(
        &verifying_key,
        export_request,
    ).await.unwrap();
    
    assert!(portable_package.total_size_bytes > 0);
    assert_eq!(portable_package.format, "encrypted_json");
    
    // User decrypts their own data locally
    let decrypted_export = manager.decrypt_portable_package(
        portable_package,
        &signing_key,
    ).await.unwrap();
    
    // Verify it's in machine-readable format
    let parsed: serde_json::Value = serde_json::from_slice(&decrypted_export).unwrap();
    assert!(parsed["inferences"].is_array());
    assert!(parsed["preferences"].is_object());
    
    // No central authority accessed the data
    assert!(parsed["_metadata"]["decentralized_export"].as_bool().unwrap());
}

#[tokio::test]
async fn test_regional_preference_routing() {
    let manager = create_test_manager().await.unwrap();
    let (_signing_key, _verifying_key) = generate_user_keys();
    
    // User sets regional preference (not a hard restriction)
    let preference = RegionalPreference::PreferEU {
        allow_fallback: true,
        acceptable_regions: vec!["EU".to_string(), "UK".to_string()],
    };
    
    // Find nodes that match preference
    let available_nodes = manager.discover_regional_nodes(
        preference.clone()
    ).await.unwrap();
    
    // User selects from available nodes
    assert!(!available_nodes.is_empty());
    let selected_node = available_nodes.first().unwrap();
    
    // Verify node's regional attestation
    let attestation = manager.verify_node_location(
        &selected_node.node_id,
        &selected_node.location_proof,
    ).await.unwrap();
    
    assert!(attestation.is_valid);
    assert!(["EU", "UK"].contains(&attestation.region.as_str()));
    
    // User can still choose non-EU node if they want
    let us_nodes = manager.discover_regional_nodes(
        RegionalPreference::US
    ).await.unwrap();
    
    assert!(!us_nodes.is_empty()); // User has choice
}

#[tokio::test]
async fn test_zero_knowledge_compliance_proofs() {
    let manager = create_test_manager().await.unwrap();
    
    // Node generates ZK proof of GDPR compliance
    let compliance_proof = manager.generate_compliance_proof(
        ZkComplianceProof {
            proves: vec![
                "data_encrypted_at_rest".to_string(),
                "no_plain_text_storage".to_string(),
                "deletion_capability".to_string(),
                "audit_log_integrity".to_string(),
            ],
            timestamp: Utc::now(),
            prover_node_id: "node123".to_string(),
        }
    ).await.unwrap();
    
    // Regulator can verify without seeing data
    let verification = manager.verify_compliance_proof(
        &compliance_proof
    ).await.unwrap();
    
    assert!(verification.all_claims_valid);
    assert!(verification.proof_timestamp <= Utc::now());
    assert!(!verification.reveals_user_data); // Privacy preserved
    
    // Proof can be published publicly
    assert!(compliance_proof.is_publicly_verifiable());
}

#[tokio::test]
async fn test_consent_withdrawal_via_smart_contract() {
    let manager = create_test_manager().await.unwrap();
    let (signing_key, verifying_key) = generate_user_keys();
    
    // User grants consent on-chain
    let consent = OnChainConsent {
        user_pubkey: verifying_key.clone(),
        purposes: vec!["inference".to_string(), "analytics".to_string(), "model_training".to_string()],
        smart_contract_address: "0x1234...".to_string(),
        block_number: 12345,
    };
    
    let _consent_tx = manager.grant_consent_on_chain(
        consent,
        &signing_key,
    ).await.unwrap();
    
    // User withdraws specific consent via smart contract
    let withdrawal = SignedRequest::new(
        "WITHDRAW_CONSENT:analytics,model_training",
        &signing_key,
    );
    
    let withdrawal_tx = manager.withdraw_consent_on_chain(
        withdrawal,
        &verifying_key,
    ).await.unwrap();
    
    assert!(!withdrawal_tx.tx_hash.is_empty());
    
    // Verify remaining consent
    let active_consent = manager.get_active_consent_from_chain(
        &verifying_key
    ).await.unwrap();
    
    assert_eq!(active_consent.purposes, vec!["inference"]);
    assert!(!active_consent.purposes.contains(&"analytics".to_string()));
    
    // All nodes see updated consent immediately
    assert!(active_consent.is_globally_synchronized);
}

#[tokio::test]
async fn test_privacy_preserving_audit() {
    let manager = create_test_manager().await.unwrap();
    let (signing_key, verifying_key) = generate_user_keys();
    
    // Perform actions that need auditing
    let actions = vec![
        ("data_access", Utc::now() - Duration::hours(2)),
        ("consent_granted", Utc::now() - Duration::hours(1)),
        ("data_exported", Utc::now()),
    ];
    
    // Generate privacy-preserving audit entries
    let mut audit_proofs = vec![];
    for (action, timestamp) in actions {
        let proof = manager.generate_audit_proof(
            AuditProof {
                action: action.to_string(),
                timestamp,
                user_pubkey_hash: blake3::hash(verifying_key.as_bytes()),
                node_id: "node456".to_string(),
                // No PII in audit
                details: HashMap::new(),
            }
        ).await.unwrap();
        
        audit_proofs.push(proof);
    }
    
    // Regulator can verify audit trail without identifying users
    let audit_verification = manager.verify_audit_trail(
        audit_proofs
    ).await.unwrap();
    
    assert!(audit_verification.all_valid);
    assert_eq!(audit_verification.total_actions, 3);
    assert!(!audit_verification.contains_pii);
    
    // Generate aggregate compliance report
    let report = audit_verification.generate_aggregate_report();
    assert!(report.contains("Total GDPR actions: 3"));
    assert!(!report.contains(&format!("{:?}", verifying_key))); // No user identification
}

#[tokio::test]
async fn test_homomorphic_analytics_on_encrypted_data() {
    let manager = create_test_manager().await.unwrap();
    
    // Multiple users with encrypted data
    let users: Vec<_> = (0..10)
        .map(|_| generate_user_keys())
        .collect();
    
    // Each user stores encrypted usage data
    for (_signing_key, verifying_key) in &users {
        let usage_data = rand::random::<u32>() % 1000; // Token count
        let encrypted = manager.homomorphic_encrypt(
            usage_data,
            verifying_key,
        ).await.unwrap();
        
        manager.store_encrypted_metric(
            verifying_key,
            "tokens_used",
            encrypted,
        ).await.unwrap();
    }
    
    // Compute aggregate statistics on encrypted data
    let total_usage = manager.compute_encrypted_sum(
        "tokens_used"
    ).await.unwrap();
    
    // Result is computed without decrypting individual data
    assert!(total_usage.result > 0);
    assert!(total_usage.computed_on_encrypted_data);
    assert_eq!(total_usage.users_included, 10);
    
    // No individual user data was exposed
    assert!(total_usage.privacy_preserved);
}

#[tokio::test]
async fn test_federated_deletion_verification() {
    let manager = create_test_manager().await.unwrap();
    let (signing_key, verifying_key) = generate_user_keys();
    
    // User data spread across multiple nodes
    let storage_nodes = vec!["node_eu", "node_us", "node_asia"];
    let mut _storage_proofs = vec![];
    
    for node in &storage_nodes {
        let proof = manager.store_to_specific_node(
            &verifying_key,
            b"distributed data",
            node,
        ).await.unwrap();
        _storage_proofs.push(proof);
    }
    
    // User broadcasts deletion request
    let deletion = SignedRequest::new(
        "DELETE_ALL",
        &signing_key,
    );
    
    let deletion_result = manager.federated_delete(
        &verifying_key,
        deletion,
        storage_nodes.clone(),
    ).await.unwrap();
    
    // Each node provides cryptographic proof of deletion
    assert_eq!(deletion_result.deletion_proofs.len(), 3);
    
    for proof in &deletion_result.deletion_proofs {
        assert!(proof.verify_deletion());
        assert!(proof.timestamp > Utc::now() - Duration::minutes(1));
        assert!(!proof.data_retained);
    }
    
    // Combined proof for compliance
    let combined_proof = deletion_result.generate_combined_proof();
    assert!(combined_proof.proves_complete_deletion_across_network());
}

#[tokio::test]
async fn test_user_controlled_anonymization() {
    let manager = create_test_manager().await.unwrap();
    let (_signing_key, verifying_key) = generate_user_keys();
    
    // User's data with PII
    let original = "My name is Alice and I live in Berlin, email: alice@example.com";
    
    // User specifies anonymization preferences
    let anon_prefs = UserControlledAnonymization {
        remove_names: true,
        remove_locations: true,
        remove_emails: true,
        preserve_context: true,
        user_defined_rules: vec![
            ("Berlin".to_string(), "[CITY]".to_string()),
            ("Alice".to_string(), "[NAME]".to_string()),
        ],
    };
    
    // Anonymization happens client-side with user control
    let anonymized = manager.anonymize_with_user_rules(
        original,
        anon_prefs,
        &verifying_key,
    ).await.unwrap();
    
    assert_eq!(anonymized.text, "My name is [NAME] and I live in [CITY], email: [EMAIL]");
    assert!(anonymized.is_reversible_by_user); // User can reverse if needed
    assert!(!anonymized.is_reversible_by_node); // Node cannot reverse
    
    // Generate proof of anonymization for compliance
    let anon_proof = anonymized.generate_anonymization_proof();
    assert!(anon_proof.verifies_pii_removed());
}

#[tokio::test]
async fn test_decentralized_compliance_attestation() {
    let manager = create_test_manager().await.unwrap();
    
    // Nodes collectively attest to compliance
    let participating_nodes = vec!["node1", "node2", "node3"];
    let mut attestations = vec![];
    
    for node_id in participating_nodes {
        let attestation = manager.generate_node_attestation(
            node_id,
            ComplianceAttestation {
                gdpr_compliant: true,
                encryption_at_rest: true,
                no_backdoors: true,
                user_data_sovereignty: true,
                audit_capability: true,
                deletion_capability: true,
            }
        ).await.unwrap();
        
        attestations.push(attestation);
    }
    
    // Aggregate attestations into network-wide proof
    let network_compliance = manager.aggregate_attestations(
        attestations
    ).await.unwrap();
    
    assert!(network_compliance.threshold_met); // Enough nodes attested
    assert!(network_compliance.is_verifiable);
    assert!(!network_compliance.requires_trusted_third_party);
    
    // Can be verified by anyone (regulators, users, auditors)
    let public_verification = manager.verify_network_compliance(
        &network_compliance
    ).await.unwrap();
    
    assert!(public_verification.all_claims_verified);
    assert_eq!(public_verification.compliance_score, 100.0);
}