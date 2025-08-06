use anyhow::{Result, anyhow};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub model: String,
    pub dimension: usize,
    pub batch_size: usize,
    pub normalize: bool,
}

pub struct EmbeddingGenerator {
    config: EmbeddingConfig,
}

impl EmbeddingGenerator {
    pub async fn new(config: EmbeddingConfig) -> Result<Self> {
        if config.dimension == 0 {
            return Err(anyhow!("Embedding dimension must be greater than 0"));
        }
        if config.batch_size == 0 {
            return Err(anyhow!("Batch size must be greater than 0"));
        }
        
        Ok(Self { config })
    }
    
    pub async fn generate(&self, text: &str) -> Result<Vec<f32>> {
        // Create deterministic pseudo-random embeddings based on text hash
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();
        
        let mut embedding = Vec::with_capacity(self.config.dimension);
        
        // Generate deterministic values using the seed
        let mut current_seed = seed;
        for i in 0..self.config.dimension {
            // Simple linear congruential generator for deterministic pseudo-random numbers
            current_seed = (current_seed.wrapping_mul(1664525).wrapping_add(1013904223)) ^ (i as u64);
            
            // Convert to float in range [-1, 1]
            let value = (current_seed as f64 / u64::MAX as f64) * 2.0 - 1.0;
            embedding.push(value as f32);
        }
        
        // Normalize if required
        if self.config.normalize {
            let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for value in &mut embedding {
                    *value /= norm;
                }
            }
        }
        
        Ok(embedding)
    }
    
    pub async fn generate_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut embeddings = Vec::with_capacity(texts.len());
        
        for text in texts {
            embeddings.push(self.generate(text).await?);
        }
        
        Ok(embeddings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_embedding_generation() {
        let config = EmbeddingConfig {
            model: "test-model".to_string(),
            dimension: 128,
            batch_size: 16,
            normalize: true,
        };
        
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        // Test single generation
        let embedding = generator.generate("test text").await.unwrap();
        assert_eq!(embedding.len(), 128);
        
        // Test deterministic behavior
        let embedding2 = generator.generate("test text").await.unwrap();
        assert_eq!(embedding, embedding2);
        
        // Test different text gives different embedding
        let embedding3 = generator.generate("different text").await.unwrap();
        assert_ne!(embedding, embedding3);
    }
    
    #[tokio::test]
    async fn test_batch_generation() {
        let config = EmbeddingConfig {
            model: "test-model".to_string(),
            dimension: 64,
            batch_size: 4,
            normalize: false,
        };
        
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let texts = vec!["text1", "text2", "text3"];
        let embeddings = generator.generate_batch(&texts).await.unwrap();
        
        assert_eq!(embeddings.len(), 3);
        assert_eq!(embeddings[0].len(), 64);
        assert_eq!(embeddings[1].len(), 64);
        assert_eq!(embeddings[2].len(), 64);
    }
    
    #[tokio::test]
    async fn test_normalization() {
        let config = EmbeddingConfig {
            model: "test-model".to_string(),
            dimension: 100,
            batch_size: 1,
            normalize: true,
        };
        
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        let embedding = generator.generate("normalize test").await.unwrap();
        
        // Check that the embedding is normalized (magnitude ~= 1)
        let magnitude = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.01);
    }
}