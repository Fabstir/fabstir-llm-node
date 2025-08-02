use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use chrono::{DateTime, Utc};
use thiserror::Error;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationMethod {
    GroundTruth,
    ConsistencyCheck,
    FormatValidation,
    SemanticSimilarity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    pub sampling_rate: f64,
    pub verification_methods: Vec<VerificationMethod>,
    pub accuracy_threshold: f64,
    pub consistency_threshold: f64,
    pub batch_size: usize,
    pub async_verification: bool,
    pub store_results: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub job_id: String,
    pub is_accurate: bool,
    pub accuracy_score: f64,
    pub confidence: f64,
    pub method_used: VerificationMethod,
    pub timestamp: DateTime<Utc>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyMetrics {
    pub total_verifications: u64,
    pub accurate_count: u64,
    pub accuracy_rate: f64,
    pub average_confidence: f64,
    pub time_period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScore {
    pub overall_score: f64,
    pub accuracy_component: f64,
    pub consistency_component: f64,
    pub format_component: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SamplingStrategy {
    Random,
    Systematic,
    Stratified,
    ModelBased,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyAlert {
    pub timestamp: DateTime<Utc>,
    pub current_accuracy: f64,
    pub threshold: f64,
    pub model: Option<String>,
    pub recent_failures: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub name: String,
    pub pattern: String,
    pub required_fields: Vec<String>,
    pub format_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyCheck {
    pub is_consistent: bool,
    pub consistency_score: f64,
    pub canonical_answer: Option<String>,
    pub response_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatValidationResult {
    pub is_valid: bool,
    pub rule_name: String,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatistics {
    pub total_queued: u64,
    pub completed: u64,
    pub pending: u64,
    pub failed: u64,
}

#[derive(Debug, Error)]
pub enum AccuracyError {
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    #[error("Invalid sample rate: {0}")]
    InvalidSampleRate(f64),
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Rule not found: {0}")]
    RuleNotFound(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct AccuracyVerifier {
    config: VerificationConfig,
    verification_results: Arc<Mutex<Vec<VerificationResult>>>,
    model_results: Arc<Mutex<HashMap<String, Vec<VerificationResult>>>>,
    validation_rules: Arc<Mutex<HashMap<String, ValidationRule>>>,
    alert_sender: broadcast::Sender<AccuracyAlert>,
    queue_stats: Arc<Mutex<QueueStatistics>>,
    sample_counter: Arc<AtomicU64>,
}

impl AccuracyVerifier {
    pub fn new(config: VerificationConfig) -> Self {
        let (alert_sender, _) = broadcast::channel(100);

        Self {
            config,
            verification_results: Arc::new(Mutex::new(Vec::new())),
            model_results: Arc::new(Mutex::new(HashMap::new())),
            validation_rules: Arc::new(Mutex::new(HashMap::new())),
            alert_sender,
            queue_stats: Arc::new(Mutex::new(QueueStatistics {
                total_queued: 0,
                completed: 0,
                pending: 0,
                failed: 0,
            })),
            sample_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn verify_response(
        &self,
        job_id: &str,
        prompt: &str,
        response: &str,
        ground_truth: Option<&str>,
    ) -> Result<VerificationResult, AccuracyError> {
        let method = if ground_truth.is_some() {
            VerificationMethod::GroundTruth
        } else {
            VerificationMethod::SemanticSimilarity
        };

        let (is_accurate, accuracy_score, confidence) = match method {
            VerificationMethod::GroundTruth => {
                if let Some(truth) = ground_truth {
                    let result = self.verify_against_ground_truth(response, truth).await;
                    (result.is_accurate, result.accuracy_score, result.confidence)
                } else {
                    (false, 0.0, 0.0)
                }
            }
            _ => {
                // Simulate verification for other methods
                let score = 0.9;
                (score > 0.8, score, 0.9)
            }
        };

        let result = VerificationResult {
            job_id: job_id.to_string(),
            is_accurate,
            accuracy_score,
            confidence,
            method_used: method,
            timestamp: Utc::now(),
            details: None,
        };

        // Store result
        if self.config.store_results {
            let mut results = self.verification_results.lock().await;
            results.push(result.clone());
        }

        Ok(result)
    }

    pub async fn should_sample_request(&self, job_id: &str) -> bool {
        let counter = self.sample_counter.fetch_add(1, Ordering::Relaxed);
        let hash = self.simple_hash(job_id);
        (hash % 1000) < (self.config.sampling_rate * 1000.0) as u64
    }

    pub async fn verify_against_ground_truth(
        &self,
        response: &str,
        ground_truth: &str,
    ) -> VerificationResult {
        // Simple similarity check
        let response_lower = response.to_lowercase();
        let truth_lower = ground_truth.to_lowercase();
        
        let is_accurate = response_lower.contains(&truth_lower) || 
                         truth_lower.contains(&response_lower);
        
        let accuracy_score = if is_accurate { 0.95 } else { 0.2 };
        let confidence = if is_accurate { 0.95 } else { 0.3 };

        VerificationResult {
            job_id: "".to_string(),
            is_accurate,
            accuracy_score,
            confidence,
            method_used: VerificationMethod::GroundTruth,
            timestamp: Utc::now(),
            details: None,
        }
    }

    pub async fn check_consistency(
        &self,
        prompt: &str,
        responses: &[&str],
    ) -> Result<ConsistencyCheck, AccuracyError> {
        if responses.is_empty() {
            return Ok(ConsistencyCheck {
                is_consistent: false,
                consistency_score: 0.0,
                canonical_answer: None,
                response_count: 0,
            });
        }

        // Simple consistency check - look for common elements
        let mut answer_frequencies = HashMap::new();
        
        for response in responses {
            // Extract potential answers (simple heuristic)
            if let Some(answer) = self.extract_answer(response) {
                *answer_frequencies.entry(answer).or_insert(0) += 1;
            }
        }

        let most_common = answer_frequencies
            .iter()
            .max_by_key(|(_, &count)| count)
            .map(|(answer, &count)| (answer.clone(), count));

        let (canonical_answer, max_count) = most_common.unwrap_or(("".to_string(), 0));
        let consistency_score = max_count as f64 / responses.len() as f64;
        let is_consistent = consistency_score >= self.config.consistency_threshold;

        Ok(ConsistencyCheck {
            is_consistent,
            consistency_score,
            canonical_answer: if canonical_answer.is_empty() { None } else { Some(canonical_answer) },
            response_count: responses.len(),
        })
    }

    pub async fn add_validation_rule(&self, rule_id: &str, rule: ValidationRule) {
        let mut rules = self.validation_rules.lock().await;
        rules.insert(rule_id.to_string(), rule);
    }

    pub async fn validate_format(&self, rule_id: &str, response: &str) -> Result<FormatValidationResult, AccuracyError> {
        let rules = self.validation_rules.lock().await;
        let rule = rules.get(rule_id)
            .ok_or_else(|| AccuracyError::RuleNotFound(rule_id.to_string()))?;

        let mut violations = Vec::new();
        let mut is_valid = true;

        // Check pattern
        if rule.format_type == "json" {
            if !response.trim().starts_with('{') || !response.trim().ends_with('}') {
                violations.push("Invalid JSON format".to_string());
                is_valid = false;
            } else {
                // Try to parse as JSON
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(response) {
                    // Check required fields
                    for field in &rule.required_fields {
                        if !parsed.get(field).is_some() {
                            violations.push(format!("Missing required field: {}", field));
                            is_valid = false;
                        }
                    }
                } else {
                    violations.push("Invalid JSON syntax".to_string());
                    is_valid = false;
                }
            }
        }

        Ok(FormatValidationResult {
            is_valid,
            rule_name: rule.name.clone(),
            violations,
        })
    }

    pub async fn calculate_semantic_similarity(
        &self,
        reference: &str,
        response: &str,
    ) -> Result<f64, AccuracyError> {
        // Simple word-based similarity calculation
        let ref_lower = reference.to_lowercase();
        let resp_lower = response.to_lowercase();
        let ref_words: Vec<&str> = ref_lower.split_whitespace().collect();
        let resp_words: Vec<&str> = resp_lower.split_whitespace().collect();

        if ref_words.is_empty() || resp_words.is_empty() {
            return Ok(0.0);
        }

        let mut common_words = 0;
        for word in &ref_words {
            if resp_words.contains(word) {
                common_words += 1;
            }
        }

        let similarity = (2.0 * common_words as f64) / (ref_words.len() + resp_words.len()) as f64;
        Ok(similarity)
    }

    pub async fn record_verification_result(
        &self,
        job_id: &str,
        is_accurate: bool,
        score: f64,
    ) -> Result<(), AccuracyError> {
        let result = VerificationResult {
            job_id: job_id.to_string(),
            is_accurate,
            accuracy_score: score,
            confidence: if is_accurate { 0.9 } else { 0.3 },
            method_used: VerificationMethod::GroundTruth,
            timestamp: Utc::now(),
            details: None,
        };

        let mut results = self.verification_results.lock().await;
        results.push(result);

        // Check for alerts
        self.check_accuracy_alerts().await;

        Ok(())
    }

    pub async fn get_accuracy_metrics(&self, time_period: &str) -> AccuracyMetrics {
        let results = self.verification_results.lock().await;
        
        let total_verifications = results.len() as u64;
        let accurate_count = results.iter().filter(|r| r.is_accurate).count() as u64;
        let accuracy_rate = if total_verifications > 0 {
            accurate_count as f64 / total_verifications as f64
        } else {
            0.0
        };

        let average_confidence = if total_verifications > 0 {
            results.iter().map(|r| r.confidence).sum::<f64>() / total_verifications as f64
        } else {
            0.0
        };

        AccuracyMetrics {
            total_verifications,
            accurate_count,
            accuracy_rate,
            average_confidence,
            time_period: time_period.to_string(),
        }
    }

    pub async fn subscribe_to_alerts(&self) -> broadcast::Receiver<AccuracyAlert> {
        self.alert_sender.subscribe()
    }

    pub async fn record_model_verification(
        &self,
        model: &str,
        job_id: &str,
        is_accurate: bool,
        score: f64,
    ) -> Result<(), AccuracyError> {
        let result = VerificationResult {
            job_id: job_id.to_string(),
            is_accurate,
            accuracy_score: score,
            confidence: 0.9,
            method_used: VerificationMethod::GroundTruth,
            timestamp: Utc::now(),
            details: None,
        };

        let mut model_results = self.model_results.lock().await;
        model_results.entry(model.to_string()).or_insert_with(Vec::new).push(result);

        Ok(())
    }

    pub async fn get_model_accuracy(&self, model: &str) -> AccuracyMetrics {
        let model_results = self.model_results.lock().await;
        let results = model_results.get(model).cloned().unwrap_or_default();

        let total_verifications = results.len() as u64;
        let accurate_count = results.iter().filter(|r| r.is_accurate).count() as u64;
        let accuracy_rate = if total_verifications > 0 {
            accurate_count as f64 / total_verifications as f64
        } else {
            0.0
        };

        let average_confidence = if total_verifications > 0 {
            results.iter().map(|r| r.confidence).sum::<f64>() / total_verifications as f64
        } else {
            0.0
        };

        AccuracyMetrics {
            total_verifications,
            accurate_count,
            accuracy_rate,
            average_confidence,
            time_period: "all".to_string(),
        }
    }

    pub async fn queue_verification(
        &self,
        job_id: &str,
        prompt: &str,
        response: &str,
        ground_truth: Option<&str>,
    ) -> Result<(), AccuracyError> {
        // Update queue stats
        {
            let mut stats = self.queue_stats.lock().await;
            stats.total_queued += 1;
            stats.pending += 1;
        }

        // Perform verification (in real implementation, this would be async)
        let result = self.verify_response(job_id, prompt, response, ground_truth).await;

        // Update queue stats
        {
            let mut stats = self.queue_stats.lock().await;
            stats.pending -= 1;
            if result.is_ok() {
                stats.completed += 1;
            } else {
                stats.failed += 1;
            }
        }

        result.map(|_| ())
    }

    pub async fn get_queue_statistics(&self) -> QueueStatistics {
        self.queue_stats.lock().await.clone()
    }

    pub async fn get_verification_history(&self, limit: usize) -> Vec<VerificationResult> {
        let results = self.verification_results.lock().await;
        let mut sorted_results = results.clone();
        sorted_results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        sorted_results.into_iter().take(limit).collect()
    }

    pub async fn export_verification_data(&self, format: &str) -> Result<String, AccuracyError> {
        let results = self.verification_results.lock().await;

        match format {
            "csv" => {
                let mut csv = String::from("job_id,accuracy,score,confidence,timestamp\n");
                for result in results.iter() {
                    csv.push_str(&format!(
                        "{},{},{},{},{}\n",
                        result.job_id,
                        result.is_accurate,
                        result.accuracy_score,
                        result.confidence,
                        result.timestamp.format("%Y-%m-%d %H:%M:%S")
                    ));
                }
                Ok(csv)
            }
            "json" => {
                Ok(serde_json::to_string_pretty(&*results)?)
            }
            _ => Err(AccuracyError::VerificationFailed("Unsupported format".to_string()))
        }
    }

    async fn check_accuracy_alerts(&self) {
        let results = self.verification_results.lock().await;
        
        if results.len() >= 10 {
            let recent_results: Vec<_> = results.iter().rev().take(10).collect();
            let accurate_count = recent_results.iter().filter(|r| r.is_accurate).count();
            let accuracy_rate = accurate_count as f64 / recent_results.len() as f64;

            if accuracy_rate < self.config.accuracy_threshold {
                let alert = AccuracyAlert {
                    timestamp: Utc::now(),
                    current_accuracy: accuracy_rate,
                    threshold: self.config.accuracy_threshold,
                    model: None,
                    recent_failures: (recent_results.len() - accurate_count) as u64,
                };

                let _ = self.alert_sender.send(alert);
            }
        }
    }

    fn extract_answer(&self, response: &str) -> Option<String> {
        // Simple heuristic to extract potential answers
        let words: Vec<&str> = response.split_whitespace().collect();
        
        // Look for numbers
        for word in &words {
            if let Ok(_num) = word.parse::<i32>() {
                return Some(word.to_string());
            }
        }

        // Look for short answers at the end
        if let Some(last_word) = words.last() {
            if last_word.len() <= 10 {
                return Some(last_word.to_string());
            }
        }

        None
    }

    fn simple_hash(&self, input: &str) -> u64 {
        // Simple hash function for sampling
        let mut hash = 0u64;
        for byte in input.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        hash
    }
}