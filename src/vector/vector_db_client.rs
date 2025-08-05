use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

pub struct VectorDbClient {
    client: Client,
    base_url: String,
}

impl VectorDbClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let _parsed_url = reqwest::Url::parse(base_url)
            .map_err(|e| anyhow!("Invalid URL: {}", e))?;
        
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        
        Ok(Self {
            client,
            base_url: base_url.to_string(),
        })
    }

    pub async fn health_check(&self) -> Result<Value> {
        let url = format!("{}/health", self.base_url);
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Health check failed with status: {}", response.status()));
        }
        
        let result = response.json::<Value>().await?;
        Ok(result)
    }

    pub async fn insert_vector(&self, vector_data: Value) -> Result<Value> {
        let url = format!("{}/vectors", self.base_url);
        let response = self.client
            .post(&url)
            .json(&vector_data)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Insert failed with status: {}", response.status()));
        }
        
        let result = response.json::<Value>().await?;
        Ok(result)
    }

    pub async fn get_vector(&self, vector_id: &str) -> Result<Value> {
        let url = format!("{}/vectors/{}", self.base_url, vector_id);
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(json!({ "error": "Vector not found" }));
            }
            return Err(anyhow!("Get vector failed with status: {}", response.status()));
        }
        
        let result = response.json::<Value>().await?;
        Ok(result)
    }

    pub async fn search_vectors(&self, search_query: Value) -> Result<Value> {
        let url = format!("{}/search", self.base_url);
        let response = self.client
            .post(&url)
            .json(&search_query)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Search failed with status: {}", response.status()));
        }
        
        let result = response.json::<Value>().await?;
        Ok(result)
    }

    pub async fn delete_vector(&self, vector_id: &str) -> Result<Value> {
        let url = format!("{}/vectors/{}", self.base_url, vector_id);
        let response = self.client
            .delete(&url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Delete failed with status: {}", response.status()));
        }
        
        let result = response.json::<Value>().await?;
        Ok(result)
    }

    pub async fn batch_insert(&self, batch_data: Value) -> Result<Value> {
        let url = format!("{}/vectors/batch", self.base_url);
        let response = self.client
            .post(&url)
            .json(&batch_data)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Batch insert failed with status: {}", response.status()));
        }
        
        let result = response.json::<Value>().await?;
        Ok(result)
    }
}