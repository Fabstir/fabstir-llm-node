use fabstir_llm_node::storage::{
    CborCompat, CborEncoder, CborDecoder, CborError,
    S5Metadata, DirV1Entry, DirV1, CompressionType
};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestStruct {
        name: String,
        value: i32,
        data: Vec<u8>,
        metadata: HashMap<String, String>,
    }
    
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct NFTMetadata {
        name: String,
        description: String,
        image: String,
        animation_url: Option<String>,
        attributes: Vec<Attribute>,
        properties: HashMap<String, serde_json::Value>,
    }
    
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Attribute {
        trait_type: String,
        value: String,
        display_type: Option<String>,
    }

    #[tokio::test]
    async fn test_basic_cbor_encode_decode() {
        let encoder = CborEncoder::new();
        let decoder = CborDecoder::new();
        
        let test_data = TestStruct {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
            metadata: HashMap::from([
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
            ]),
        };
        
        // Encode
        let encoded = encoder.encode(&test_data).unwrap();
        assert!(!encoded.is_empty());
        
        // Decode
        let decoded: TestStruct = decoder.decode(&encoded).unwrap();
        assert_eq!(decoded, test_data);
    }

    #[tokio::test]
    async fn test_deterministic_encoding() {
        let encoder = CborEncoder::new();
        
        let data = HashMap::from([
            ("z".to_string(), "last".to_string()),
            ("a".to_string(), "first".to_string()),
            ("m".to_string(), "middle".to_string()),
        ]);
        
        // Encode multiple times
        let encoded1 = encoder.encode(&data).unwrap();
        let encoded2 = encoder.encode(&data).unwrap();
        let encoded3 = encoder.encode(&data).unwrap();
        
        // Should be identical (deterministic)
        assert_eq!(encoded1, encoded2);
        assert_eq!(encoded2, encoded3);
    }

    #[tokio::test]
    async fn test_s5_metadata_encoding() {
        let compat = CborCompat::new();
        
        let metadata = S5Metadata {
            content_type: "application/json".to_string(),
            size: 1024,
            created_at: 1234567890,
            modified_at: 1234567890,
            attributes: HashMap::from([
                ("author".to_string(), "test-user".to_string()),
                ("version".to_string(), "1.0.0".to_string()),
            ]),
        };
        
        let encoded = compat.encode_s5_metadata(&metadata).unwrap();
        let decoded = compat.decode_s5_metadata(&encoded).unwrap();
        
        assert_eq!(decoded.content_type, metadata.content_type);
        assert_eq!(decoded.size, metadata.size);
        assert_eq!(decoded.attributes, metadata.attributes);
    }

    #[tokio::test]
    async fn test_dirv1_structure() {
        let compat = CborCompat::new();
        
        // Create directory structure
        let mut entries = HashMap::new();
        
        entries.insert("file1.txt".to_string(), DirV1Entry {
            cid: "bafy123".to_string(),
            size: 100,
            entry_type: "file".to_string(),
            metadata: HashMap::new(),
        });
        
        entries.insert("file2.json".to_string(), DirV1Entry {
            cid: "bafy456".to_string(),
            size: 200,
            entry_type: "file".to_string(),
            metadata: HashMap::from([
                ("content-type".to_string(), "application/json".to_string()),
            ]),
        });
        
        entries.insert("subdir".to_string(), DirV1Entry {
            cid: "bafy789".to_string(),
            size: 0,
            entry_type: "directory".to_string(),
            metadata: HashMap::new(),
        });
        
        let dir = DirV1 {
            version: 1,
            entries,
            metadata: HashMap::from([
                ("created".to_string(), "2024-01-01".to_string()),
            ]),
        };
        
        let encoded = compat.encode_dirv1(&dir).unwrap();
        let decoded = compat.decode_dirv1(&encoded).unwrap();
        
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.entries.len(), 3);
        assert_eq!(decoded.entries.get("file1.txt").unwrap().size, 100);
        assert_eq!(decoded.metadata.get("created"), Some(&"2024-01-01".to_string()));
    }

    #[tokio::test]
    async fn test_compression_compatibility() {
        let compat = CborCompat::new();
        
        // Create large repetitive data
        let large_data: Vec<u8> = (0..10000)
            .map(|i| (i % 10) as u8)
            .collect();
        
        let test_struct = TestStruct {
            name: "compression test".to_string(),
            value: 999,
            data: large_data,
            metadata: HashMap::new(),
        };
        
        // Encode with compression
        let encoded = compat.encode_with_compression(&test_struct, CompressionType::Zstd).unwrap();
        let uncompressed = compat.encode(&test_struct).unwrap();
        
        // Compressed should be smaller
        assert!(encoded.len() < uncompressed.len());
        assert!(encoded.len() < 1000); // Original is 10000+ bytes
        
        // Decode
        let decoded: TestStruct = compat.decode_compressed(&encoded, CompressionType::Zstd).unwrap();
        assert_eq!(decoded.name, test_struct.name);
        assert_eq!(decoded.data.len(), test_struct.data.len());
    }

    #[tokio::test]
    async fn test_nft_metadata_compatibility() {
        let compat = CborCompat::new();
        
        let nft = NFTMetadata {
            name: "Fabstir LLM Node #1".to_string(),
            description: "Certified LLM compute provider".to_string(),
            image: "s5://bafy123/image.png".to_string(),
            animation_url: Some("s5://bafy456/animation.mp4".to_string()),
            attributes: vec![
                Attribute {
                    trait_type: "GPU Model".to_string(),
                    value: "RTX 4090".to_string(),
                    display_type: None,
                },
                Attribute {
                    trait_type: "Compute Power".to_string(),
                    value: "82.6 TFLOPS".to_string(),
                    display_type: Some("number".to_string()),
                },
            ],
            properties: HashMap::from([
                ("verified".to_string(), serde_json::json!(true)),
                ("uptime".to_string(), serde_json::json!(99.9)),
                ("models".to_string(), serde_json::json!(["llama", "mistral"])),
            ]),
        };
        
        let encoded = compat.encode(&nft).unwrap();
        let decoded: NFTMetadata = compat.decode(&encoded).unwrap();
        
        assert_eq!(decoded.name, nft.name);
        assert_eq!(decoded.attributes.len(), 2);
        assert_eq!(decoded.properties.get("verified"), Some(&serde_json::json!(true)));
    }

    #[tokio::test]
    async fn test_float_edge_cases() {
        let compat = CborCompat::new();
        
        #[derive(Debug, Serialize, Deserialize)]
        struct FloatTest {
            normal: f64,
            infinity: f64,
            neg_infinity: f64,
            nan: f64,
            tiny: f64,
            huge: f64,
        }
        
        let test = FloatTest {
            normal: 3.14159,
            infinity: f64::INFINITY,
            neg_infinity: f64::NEG_INFINITY,
            nan: f64::NAN,
            tiny: f64::MIN_POSITIVE,
            huge: f64::MAX,
        };
        
        let encoded = compat.encode(&test).unwrap();
        let decoded: FloatTest = compat.decode(&encoded).unwrap();
        
        assert_eq!(decoded.normal, test.normal);
        assert!(decoded.infinity.is_infinite() && decoded.infinity.is_sign_positive());
        assert!(decoded.neg_infinity.is_infinite() && decoded.neg_infinity.is_sign_negative());
        assert!(decoded.nan.is_nan());
        assert_eq!(decoded.tiny, test.tiny);
        assert_eq!(decoded.huge, test.huge);
    }

    #[tokio::test]
    async fn test_batch_encoding() {
        let compat = CborCompat::new();
        
        let items = vec![
            TestStruct {
                name: "item1".to_string(),
                value: 1,
                data: vec![1, 2, 3],
                metadata: HashMap::new(),
            },
            TestStruct {
                name: "item2".to_string(),
                value: 2,
                data: vec![4, 5, 6],
                metadata: HashMap::new(),
            },
            TestStruct {
                name: "item3".to_string(),
                value: 3,
                data: vec![7, 8, 9],
                metadata: HashMap::new(),
            },
        ];
        
        // Batch encode
        let encoded_batch = compat.encode_batch(&items).unwrap();
        assert_eq!(encoded_batch.len(), 3);
        
        // Batch decode
        let decoded_batch: Vec<TestStruct> = compat.decode_batch(&encoded_batch).unwrap();
        assert_eq!(decoded_batch, items);
    }

    #[tokio::test]
    async fn test_empty_values() {
        let compat = CborCompat::new();
        
        // Empty string
        let empty_string = "";
        let encoded = compat.encode(&empty_string).unwrap();
        let decoded: String = compat.decode(&encoded).unwrap();
        assert_eq!(decoded, empty_string);
        
        // Empty vec
        let empty_vec: Vec<u8> = vec![];
        let encoded = compat.encode(&empty_vec).unwrap();
        let decoded: Vec<u8> = compat.decode(&encoded).unwrap();
        assert_eq!(decoded, empty_vec);
        
        // Empty map
        let empty_map: HashMap<String, String> = HashMap::new();
        let encoded = compat.encode(&empty_map).unwrap();
        let decoded: HashMap<String, String> = compat.decode(&encoded).unwrap();
        assert_eq!(decoded, empty_map);
    }

    #[tokio::test]
    async fn test_large_embedding_vector() {
        let compat = CborCompat::new();
        
        // Simulate 768-dimensional embedding vector
        let embedding: Vec<f32> = (0..768)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();
        
        let encoded = compat.encode(&embedding).unwrap();
        let decoded: Vec<f32> = compat.decode(&encoded).unwrap();
        
        assert_eq!(decoded.len(), 768);
        for (i, (a, b)) in embedding.iter().zip(decoded.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "Mismatch at index {}: {} vs {}", i, a, b);
        }
    }

    #[tokio::test]
    async fn test_error_handling() {
        let decoder = CborDecoder::new();
        
        // Invalid CBOR
        let invalid_data = vec![0xFF, 0xFF, 0xFF];
        let result: Result<TestStruct, _> = decoder.decode(&invalid_data);
        assert!(result.is_err());
        
        // Wrong type
        let string_data = CborEncoder::new().encode(&"just a string").unwrap();
        let result: Result<TestStruct, _> = decoder.decode(&string_data);
        assert!(result.is_err());
        
        // Empty data
        let result: Result<TestStruct, _> = decoder.decode(&[]);
        assert!(result.is_err());
    }
}