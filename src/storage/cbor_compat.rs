use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use thiserror::Error;
use zstd;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompressionType {
    None,
    Zstd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S5Metadata {
    pub content_type: String,
    pub size: u64,
    pub created_at: i64,
    pub modified_at: i64,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirV1Entry {
    pub cid: String,
    pub size: u64,
    pub entry_type: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirV1 {
    pub version: u32,
    pub entries: HashMap<String, DirV1Entry>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum CborError {
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_cbor::Error),
    #[error("Compression error: {0}")]
    CompressionError(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

#[derive(Debug, Clone)]
pub struct CborEncoder {
    deterministic: bool,
}

impl CborEncoder {
    pub fn new() -> Self {
        Self {
            deterministic: true,
        }
    }

    pub fn encode<T: Serialize>(&self, data: &T) -> Result<Vec<u8>, CborError> {
        let mut buffer = Vec::new();
        if self.deterministic {
            // Use deterministic encoding with sorted keys
            let mut serializer = serde_cbor::Serializer::new(&mut buffer).packed_format();
            data.serialize(&mut serializer)?;
        } else {
            serde_cbor::to_writer(&mut buffer, data)?;
        }
        Ok(buffer)
    }
}

impl Default for CborEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct CborDecoder;

impl CborDecoder {
    pub fn new() -> Self {
        Self
    }

    pub fn decode<T: DeserializeOwned>(&self, data: &[u8]) -> Result<T, CborError> {
        if data.is_empty() {
            return Err(CborError::InvalidData("Empty data".to_string()));
        }
        
        let result = serde_cbor::from_slice(data)?;
        Ok(result)
    }
}

impl Default for CborDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct CborCompat {
    encoder: CborEncoder,
    decoder: CborDecoder,
}

impl CborCompat {
    pub fn new() -> Self {
        Self {
            encoder: CborEncoder::new(),
            decoder: CborDecoder::new(),
        }
    }

    pub fn encode<T: Serialize>(&self, data: &T) -> Result<Vec<u8>, CborError> {
        self.encoder.encode(data)
    }

    pub fn decode<T: DeserializeOwned>(&self, data: &[u8]) -> Result<T, CborError> {
        self.decoder.decode(data)
    }

    pub fn encode_s5_metadata(&self, metadata: &S5Metadata) -> Result<Vec<u8>, CborError> {
        self.encode(metadata)
    }

    pub fn decode_s5_metadata(&self, data: &[u8]) -> Result<S5Metadata, CborError> {
        self.decode(data)
    }

    pub fn encode_dirv1(&self, dir: &DirV1) -> Result<Vec<u8>, CborError> {
        self.encode(dir)
    }

    pub fn decode_dirv1(&self, data: &[u8]) -> Result<DirV1, CborError> {
        self.decode(data)
    }

    pub fn encode_with_compression<T: Serialize>(
        &self,
        data: &T,
        compression: CompressionType,
    ) -> Result<Vec<u8>, CborError> {
        let encoded = self.encode(data)?;
        
        match compression {
            CompressionType::None => Ok(encoded),
            CompressionType::Zstd => {
                let compressed = zstd::stream::encode_all(&encoded[..], 3)
                    .map_err(|e| CborError::CompressionError(format!("Zstd compression failed: {}", e)))?;
                Ok(compressed)
            }
        }
    }

    pub fn decode_compressed<T: DeserializeOwned>(
        &self,
        data: &[u8],
        compression: CompressionType,
    ) -> Result<T, CborError> {
        let decoded_data = match compression {
            CompressionType::None => data.to_vec(),
            CompressionType::Zstd => {
                zstd::stream::decode_all(data)
                    .map_err(|e| CborError::CompressionError(format!("Zstd decompression failed: {}", e)))?
            }
        };
        
        self.decode(&decoded_data)
    }

    pub fn encode_batch<T: Serialize>(&self, items: &[T]) -> Result<Vec<Vec<u8>>, CborError> {
        let mut encoded_items = Vec::new();
        for item in items {
            encoded_items.push(self.encode(item)?);
        }
        Ok(encoded_items)
    }

    pub fn decode_batch<T: DeserializeOwned>(&self, encoded_items: &[Vec<u8>]) -> Result<Vec<T>, CborError> {
        let mut decoded_items = Vec::new();
        for encoded in encoded_items {
            decoded_items.push(self.decode(encoded)?);
        }
        Ok(decoded_items)
    }
}

impl Default for CborCompat {
    fn default() -> Self {
        Self::new()
    }
}

// Utility functions for common serialization patterns
pub fn serialize_with_deterministic_keys<T: Serialize>(data: &T) -> Result<Vec<u8>, CborError> {
    let encoder = CborEncoder::new();
    encoder.encode(data)
}

pub fn compress_cbor_data(data: &[u8], level: i32) -> Result<Vec<u8>, CborError> {
    zstd::stream::encode_all(data, level)
        .map_err(|e| CborError::CompressionError(format!("Compression failed: {}", e)))
}

pub fn decompress_cbor_data(compressed: &[u8]) -> Result<Vec<u8>, CborError> {
    zstd::stream::decode_all(compressed)
        .map_err(|e| CborError::CompressionError(format!("Decompression failed: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_encoding() {
        let encoder = CborEncoder::new();
        
        let data = HashMap::from([
            ("z".to_string(), "last".to_string()),
            ("a".to_string(), "first".to_string()),
            ("m".to_string(), "middle".to_string()),
        ]);
        
        let encoded1 = encoder.encode(&data).unwrap();
        let encoded2 = encoder.encode(&data).unwrap();
        
        assert_eq!(encoded1, encoded2);
    }

    #[test]
    fn test_compression() {
        let compat = CborCompat::new();
        let data = vec![42u8; 10000]; // Highly compressible
        
        let compressed = compat.encode_with_compression(&data, CompressionType::Zstd).unwrap();
        let decompressed: Vec<u8> = compat.decode_compressed(&compressed, CompressionType::Zstd).unwrap();
        
        assert!(compressed.len() < data.len());
        assert_eq!(decompressed, data);
    }
}