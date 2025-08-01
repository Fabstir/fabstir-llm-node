use anyhow::Result;
use ethers::types::{Address, H256, U256, Filter, Log};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentEvent {
    pub event_type: PaymentEventType,
    pub job_id: H256,
    pub amount: U256,
    pub token: Address,
    pub from: Address,
    pub to: Address,
    pub block_number: u64,
    pub transaction_hash: H256,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaymentEventType {
    PaymentReceived,
    PaymentFailed,
    PaymentRefunded,
    PaymentDisputed,
}

#[derive(Debug, Clone)]
pub struct PaymentFilter {
    pub node_address: Option<Address>,
    pub event_types: Vec<PaymentEventType>,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub min_amount: Option<U256>,
}

impl Default for PaymentFilter {
    fn default() -> Self {
        Self {
            node_address: None,
            event_types: vec![
                PaymentEventType::PaymentReceived,
                PaymentEventType::PaymentFailed,
                PaymentEventType::PaymentRefunded,
                PaymentEventType::PaymentDisputed,
            ],
            from_block: None,
            to_block: None,
            min_amount: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentStats {
    pub total_received: U256,
    pub total_failed: U256,
    pub total_refunded: U256,
    pub payment_count: u64,
    pub success_rate: f64,
    pub average_payment: U256,
}

pub struct PaymentTracker {
    contract_client: Arc<dyn ContractClient>,
    payment_events: Arc<RwLock<Vec<PaymentEvent>>>,
    confirmation_blocks: u64,
    node_address: Address,
}

#[async_trait::async_trait]
pub trait ContractClient: Send + Sync {
    async fn get_payment_events(
        &self,
        filter: Filter,
    ) -> Result<Vec<Log>>;
    
    async fn parse_payment_event(
        &self,
        log: &Log,
    ) -> Result<PaymentEvent>;
    
    async fn get_current_block(&self) -> Result<u64>;
    
    async fn subscribe_to_events(
        &self,
        filter: Filter,
    ) -> Result<Box<dyn futures::Stream<Item = Log> + Send + Unpin>>;
}

impl PaymentTracker {
    pub fn new(
        contract_client: Arc<dyn ContractClient>,
        node_address: Address,
        confirmation_blocks: u64,
    ) -> Self {
        Self {
            contract_client,
            payment_events: Arc::new(RwLock::new(Vec::new())),
            confirmation_blocks,
            node_address,
        }
    }
    
    pub async fn track_payments(&self, filter: PaymentFilter) -> Result<Vec<PaymentEvent>> {
        let current_block = self.contract_client.get_current_block().await?;
        
        let eth_filter = Filter::new()
            .from_block(filter.from_block.unwrap_or(0))
            .to_block(filter.to_block.unwrap_or(current_block));
        
        let logs = self.contract_client.get_payment_events(eth_filter).await?;
        let mut events = Vec::new();
        
        for log in logs {
            if let Ok(event) = self.contract_client.parse_payment_event(&log).await {
                if self.matches_filter(&event, &filter) {
                    events.push(event);
                }
            }
        }
        
        // Store events
        self.payment_events.write().await.extend(events.clone());
        
        Ok(events)
    }
    
    pub async fn get_payment_for_job(&self, job_id: H256) -> Result<Option<PaymentEvent>> {
        let events = self.payment_events.read().await;
        Ok(events.iter()
            .find(|e| e.job_id == job_id && e.event_type == PaymentEventType::PaymentReceived)
            .cloned())
    }
    
    pub async fn get_confirmed_payments(&self) -> Result<Vec<PaymentEvent>> {
        let current_block = self.contract_client.get_current_block().await?;
        let events = self.payment_events.read().await;
        
        Ok(events.iter()
            .filter(|e| {
                e.event_type == PaymentEventType::PaymentReceived &&
                current_block >= e.block_number + self.confirmation_blocks
            })
            .cloned()
            .collect())
    }
    
    pub async fn get_payment_stats(&self) -> Result<PaymentStats> {
        let events = self.payment_events.read().await;
        
        let total_received = events.iter()
            .filter(|e| e.event_type == PaymentEventType::PaymentReceived)
            .map(|e| e.amount)
            .fold(U256::zero(), |acc, amt| acc + amt);
        
        let total_failed = events.iter()
            .filter(|e| e.event_type == PaymentEventType::PaymentFailed)
            .map(|e| e.amount)
            .fold(U256::zero(), |acc, amt| acc + amt);
        
        let total_refunded = events.iter()
            .filter(|e| e.event_type == PaymentEventType::PaymentRefunded)
            .map(|e| e.amount)
            .fold(U256::zero(), |acc, amt| acc + amt);
        
        let payment_count = events.iter()
            .filter(|e| e.event_type == PaymentEventType::PaymentReceived)
            .count() as u64;
        
        let total_attempts = events.iter()
            .filter(|e| {
                e.event_type == PaymentEventType::PaymentReceived ||
                e.event_type == PaymentEventType::PaymentFailed
            })
            .count() as f64;
        
        let success_rate = if total_attempts > 0.0 {
            payment_count as f64 / total_attempts
        } else {
            0.0
        };
        
        let average_payment = if payment_count > 0 {
            total_received / U256::from(payment_count)
        } else {
            U256::zero()
        };
        
        Ok(PaymentStats {
            total_received,
            total_failed,
            total_refunded,
            payment_count,
            success_rate,
            average_payment,
        })
    }
    
    pub async fn start_monitoring(&self) -> Result<tokio::sync::mpsc::Receiver<PaymentEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let filter = Filter::new();
        let mut stream = self.contract_client.subscribe_to_events(filter).await?;
        
        let events = self.payment_events.clone();
        let client = self.contract_client.clone();
        let node_address = self.node_address;
        
        tokio::spawn(async move {
            use futures::StreamExt;
            
            while let Some(log) = stream.next().await {
                if let Ok(event) = client.parse_payment_event(&log).await {
                    if event.to == node_address {
                        events.write().await.push(event.clone());
                        let _ = tx.send(event).await;
                    }
                }
            }
        });
        
        Ok(rx)
    }
    
    fn matches_filter(&self, event: &PaymentEvent, filter: &PaymentFilter) -> bool {
        if let Some(addr) = filter.node_address {
            if event.to != addr {
                return false;
            }
        }
        
        if !filter.event_types.contains(&event.event_type) {
            return false;
        }
        
        if let Some(min) = filter.min_amount {
            if event.amount < min {
                return false;
            }
        }
        
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use futures::task::{Context, Poll};
    
    struct MockContractClient {
        events: Vec<PaymentEvent>,
        current_block: u64,
    }
    
    impl MockContractClient {
        fn new() -> Self {
            Self {
                events: vec![],
                current_block: 100,
            }
        }
        
        fn add_event(&mut self, event: PaymentEvent) {
            self.events.push(event);
        }
    }
    
    #[async_trait::async_trait]
    impl ContractClient for MockContractClient {
        async fn get_payment_events(&self, _filter: Filter) -> Result<Vec<Log>> {
            Ok(vec![])
        }
        
        async fn parse_payment_event(&self, _log: &Log) -> Result<PaymentEvent> {
            if !self.events.is_empty() {
                Ok(self.events[0].clone())
            } else {
                anyhow::bail!("No events")
            }
        }
        
        async fn get_current_block(&self) -> Result<u64> {
            Ok(self.current_block)
        }
        
        async fn subscribe_to_events(
            &self,
            _filter: Filter,
        ) -> Result<Box<dyn futures::Stream<Item = Log> + Send + Unpin>> {
            struct EmptyStream;
            
            impl futures::Stream for EmptyStream {
                type Item = Log;
                
                fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                    Poll::Ready(None)
                }
            }
            
            Ok(Box::new(EmptyStream))
        }
    }
    
    #[tokio::test]
    async fn test_payment_tracker_creation() {
        let client = Arc::new(MockContractClient::new());
        let node_address = Address::random();
        let tracker = PaymentTracker::new(client, node_address, 12);
        
        assert_eq!(tracker.confirmation_blocks, 12);
        assert_eq!(tracker.node_address, node_address);
    }
}