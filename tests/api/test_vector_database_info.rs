// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for VectorDatabaseInfo message type (Sub-phase 1.1)

use fabstir_llm_node::api::websocket::message_types::VectorDatabaseInfo;
use serde_json::json;

#[cfg(test)]
mod vector_database_info_tests {
    use super::*;

    /// Test 1: VectorDatabaseInfo serialization to JSON
    #[test]
    fn test_vector_database_info_serialization() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let json = serde_json::to_value(&vdb_info).expect("Failed to serialize");

        // Check camelCase serialization
        assert_eq!(
            json["manifestPath"],
            "home/vector-databases/0xABC.../my-docs/manifest.json"
        );
        assert_eq!(
            json["userAddress"],
            "0xABCDEF1234567890ABCDEF1234567890ABCDEF12"
        );
    }

    /// Test 2: VectorDatabaseInfo deserialization from JSON (SDK format is camelCase)
    #[test]
    fn test_vector_database_info_deserialization() {
        let json = json!({
            "manifestPath": "home/vector-databases/0x123.../docs/manifest.json",
            "userAddress": "0x1234567890ABCDEF1234567890ABCDEF12345678"
        });

        let vdb_info: VectorDatabaseInfo = serde_json::from_value(json)
            .expect("Failed to deserialize");

        assert_eq!(
            vdb_info.manifest_path,
            "home/vector-databases/0x123.../docs/manifest.json"
        );
        assert_eq!(
            vdb_info.user_address,
            "0x1234567890ABCDEF1234567890ABCDEF12345678"
        );
    }

    /// Test 3: VectorDatabaseInfo with camelCase JSON (SDK compatibility)
    #[test]
    fn test_vector_database_info_camel_case() {
        let json = json!({
            "manifestPath": "home/vector-databases/0xABC.../data/manifest.json",
            "userAddress": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12"
        });

        let vdb_info: VectorDatabaseInfo = serde_json::from_value(json)
            .expect("Failed to deserialize camelCase");

        assert_eq!(
            vdb_info.manifest_path,
            "home/vector-databases/0xABC.../data/manifest.json"
        );
        assert_eq!(
            vdb_info.user_address,
            "0xABCDEF1234567890ABCDEF1234567890ABCDEF12"
        );
    }

    /// Test 4: VectorDatabaseInfo validation - valid manifest_path
    #[test]
    fn test_vector_database_info_valid_manifest_path() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        assert!(vdb_info.validate().is_ok());
    }

    /// Test 5: VectorDatabaseInfo validation - invalid manifest_path (not ending with manifest.json)
    #[test]
    fn test_vector_database_info_invalid_manifest_path_extension() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/data.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let result = vdb_info.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("manifest.json"));
    }

    /// Test 6: VectorDatabaseInfo validation - invalid user_address (lowercase, but we accept it)
    #[test]
    fn test_vector_database_info_lowercase_user_address() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xabc.../my-docs/manifest.json".to_string(),
            user_address: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
        };

        // We accept lowercase addresses (checksumming is optional)
        assert!(vdb_info.validate().is_ok());
    }

    /// Test 7: VectorDatabaseInfo validation - invalid user_address (too short)
    #[test]
    fn test_vector_database_info_invalid_user_address_length() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABC".to_string(),
        };

        let result = vdb_info.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("42 characters"));
    }

    /// Test 8: VectorDatabaseInfo validation - missing 0x prefix
    #[test]
    fn test_vector_database_info_no_hex_prefix() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "ABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let result = vdb_info.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("0x"));
    }

    /// Test 9: VectorDatabaseInfo with empty fields
    #[test]
    fn test_vector_database_info_empty_fields() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "".to_string(),
            user_address: "".to_string(),
        };

        let result = vdb_info.validate();
        assert!(result.is_err());
    }

    /// Test 10: VectorDatabaseInfo roundtrip (serialize then deserialize)
    #[test]
    fn test_vector_database_info_roundtrip() {
        let original = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let json = serde_json::to_value(&original).expect("Failed to serialize");
        let deserialized: VectorDatabaseInfo = serde_json::from_value(json)
            .expect("Failed to deserialize");

        assert_eq!(original.manifest_path, deserialized.manifest_path);
        assert_eq!(original.user_address, deserialized.user_address);
    }
}

#[cfg(test)]
mod session_init_with_vector_database_tests {
    use super::*;

    /// Test 11: SessionInit message with vector_database field
    #[test]
    fn test_session_init_with_vector_database() {
        use fabstir_llm_node::api::websocket::messages::SessionInitMessage;

        let json = json!({
            "job_id": 12345,
            "chain_id": 84532,
            "user_address": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "host_address": "0x1234567890ABCDEF1234567890ABCDEF12345678",
            "model_id": "tiny-vicuna",
            "timestamp": 1700000000,
            "vector_database": {
                "manifestPath": "home/vector-databases/0xABC.../my-docs/manifest.json",
                "userAddress": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12"
            }
        });

        let msg: SessionInitMessage = serde_json::from_value(json)
            .expect("Failed to deserialize");

        assert_eq!(msg.job_id, 12345);
        assert!(msg.vector_database.is_some());

        let vdb = msg.vector_database.unwrap();
        assert_eq!(
            vdb.manifest_path,
            "home/vector-databases/0xABC.../my-docs/manifest.json"
        );
    }

    /// Test 12: SessionInit message without vector_database (backward compatibility)
    #[test]
    fn test_session_init_without_vector_database() {
        use fabstir_llm_node::api::websocket::messages::SessionInitMessage;

        let json = json!({
            "job_id": 12345,
            "chain_id": 84532,
            "user_address": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "host_address": "0x1234567890ABCDEF1234567890ABCDEF12345678",
            "model_id": "tiny-vicuna",
            "timestamp": 1700000000
        });

        let msg: SessionInitMessage = serde_json::from_value(json)
            .expect("Failed to deserialize");

        assert_eq!(msg.job_id, 12345);
        assert!(msg.vector_database.is_none());
    }

    /// Test 13: SessionInit with vector_database null (should be None)
    #[test]
    fn test_session_init_with_null_vector_database() {
        use fabstir_llm_node::api::websocket::messages::SessionInitMessage;

        let json = json!({
            "job_id": 12345,
            "chain_id": 84532,
            "user_address": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "host_address": "0x1234567890ABCDEF1234567890ABCDEF12345678",
            "model_id": "tiny-vicuna",
            "timestamp": 1700000000,
            "vector_database": null
        });

        let msg: SessionInitMessage = serde_json::from_value(json)
            .expect("Failed to deserialize");

        assert!(msg.vector_database.is_none());
    }
}

#[cfg(test)]
mod encrypted_session_init_with_vector_database_tests {
    use super::*;

    /// Test 14: Encrypted session init payload decryption with vector_database
    /// This test will be activated once encryption payload is updated
    #[test]
    #[ignore] // Enable once encryption payload is updated
    fn test_encrypted_session_init_with_vector_database() {
        /*
        // After decryption, the inner payload should contain vector_database
        let decrypted_payload = json!({
            "sessionKey": "base64_session_key_here",
            "jobId": 12345,
            "modelName": "tiny-vicuna",
            "pricePerToken": 2000,
            "vectorDatabase": {
                "manifestPath": "home/vector-databases/0xABC.../my-docs/manifest.json",
                "userAddress": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12"
            }
        });

        // Test parsing the decrypted payload
        // Will need to add struct for this in implementation
        */
    }
}
