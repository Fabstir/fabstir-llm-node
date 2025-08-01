use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey, Signature};
use rand::{rngs::OsRng, Rng};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferenceResult {
    pub job_id: String,
    pub model_id: String,
    pub prompt: String,
    pub response: String,
    pub tokens_generated: u32,
    pub inference_time_ms: u64,
    pub timestamp: DateTime<Utc>,
    pub node_id: String,
    pub metadata: ResultMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ResultMetadata {
    pub temperature: f32,
    pub max_tokens: u32,
    pub top_p: f32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackagedResult {
    pub result: InferenceResult,
    pub signature: Vec<u8>,
    pub encoding: String,
    pub version: String,
}

#[derive(Clone)]
pub struct ResultPackager {
    node_id: String,
    signing_key: SigningKey,
}

impl ResultPackager {
    pub fn new(node_id: String) -> Self {
        // Generate a random signing key
        let mut secret_bytes = [0u8; 32];
        OsRng.fill(&mut secret_bytes);
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        
        Self {
            node_id,
            signing_key,
        }
    }
    
    pub fn package_result(&self, result: InferenceResult) -> Result<PackagedResult> {
        // Serialize result to CBOR deterministically
        let cbor_data = self.encode_cbor(&result)?;
        
        // Sign the serialized data
        let signature = self.signing_key.sign(&cbor_data);
        
        Ok(PackagedResult {
            result,
            signature: signature.to_bytes().to_vec(),
            encoding: "cbor".to_string(),
            version: "1.0".to_string(),
        })
    }
    
    pub fn verify_package(&self, package: &PackagedResult) -> Result<bool> {
        // Re-encode the result to get the original data
        let cbor_data = self.encode_cbor(&package.result)?;
        
        // Get the verifying key from our signing key
        let verifying_key: VerifyingKey = self.signing_key.verifying_key();
        
        // Convert signature bytes back to Signature
        let signature_bytes: [u8; 64] = package.signature
            .as_slice()
            .try_into()
            .context("Invalid signature length")?;
        let signature = Signature::from_bytes(&signature_bytes);
        
        // Verify the signature
        match verifying_key.verify_strict(&cbor_data, &signature) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    pub fn encode_cbor(&self, result: &InferenceResult) -> Result<Vec<u8>> {
        // Use ciborium for deterministic CBOR encoding
        let mut buffer = Vec::new();
        ciborium::into_writer(result, &mut buffer)
            .context("Failed to encode result to CBOR")?;
        Ok(buffer)
    }
    
    pub fn decode_cbor(&self, data: &[u8]) -> Result<InferenceResult> {
        ciborium::from_reader(data)
            .context("Failed to decode CBOR data")
    }
}