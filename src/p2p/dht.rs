use anyhow::Result;
use libp2p::{
    kad::{GetProvidersOk, GetRecordOk, Event as KademliaEvent, QueryId, QueryResult, RecordKey},
    PeerId,
};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot},
};

use crate::p2p::{DhtEvent, NodeEvent};

pub struct DhtHandler {
    // Pending DHT queries
    get_record_queries: HashMap<QueryId, (oneshot::Sender<Result<Vec<u8>>>, RecordKey)>,
    put_record_queries: HashMap<QueryId, oneshot::Sender<Result<()>>>,
    get_providers_queries: HashMap<QueryId, oneshot::Sender<Result<HashSet<PeerId>>>>,
    start_providing_queries: HashMap<QueryId, oneshot::Sender<Result<()>>>,
    bootstrap_queries: HashMap<QueryId, oneshot::Sender<Result<()>>>,
    
    // State tracking
    bootstrap_in_progress: bool,
    announced_capabilities: HashSet<String>,
    stored_records: HashMap<RecordKey, StoredRecord>,
    published_records: HashMap<RecordKey, PublishedRecord>,
    
    // Configuration
    bootstrap_interval: Duration,
    republish_interval: Duration,
}

#[derive(Clone, Debug)]
struct StoredRecord {
    value: Vec<u8>,
    expiration: Option<Instant>,
}

#[derive(Clone, Debug)]
struct PublishedRecord {
    value: Vec<u8>,
    last_published: Instant,
}

impl DhtHandler {
    pub fn new(bootstrap_interval: Duration, republish_interval: Duration) -> Self {
        Self {
            get_record_queries: HashMap::new(),
            put_record_queries: HashMap::new(),
            get_providers_queries: HashMap::new(),
            start_providing_queries: HashMap::new(),
            bootstrap_queries: HashMap::new(),
            bootstrap_in_progress: false,
            announced_capabilities: HashSet::new(),
            stored_records: HashMap::new(),
            published_records: HashMap::new(),
            bootstrap_interval,
            republish_interval,
        }
    }
    
    pub fn handle_event(
        &mut self,
        event: KademliaEvent,
        event_tx: &mpsc::Sender<NodeEvent>,
    ) {
        match event {
            KademliaEvent::OutboundQueryProgressed { id, result, .. } => {
                match result {
                    QueryResult::GetRecord(Ok(GetRecordOk::FoundRecord(record))) => {
                        if let Some((tx, key)) = self.get_record_queries.remove(&id) {
                            // Check if the record is expired in our local storage
                            if let Some(stored) = self.stored_records.get(&key) {
                                if let Some(expiration) = stored.expiration {
                                    if Instant::now() > expiration {
                                        // Record is expired, return error
                                        let _ = tx.send(Err(anyhow::anyhow!("Record expired")));
                                        return;
                                    }
                                }
                            }
                            
                            let value = record.record.value.clone();
                            let _ = tx.send(Ok(value.clone()));
                            let _ = event_tx.try_send(NodeEvent::DhtEvent(DhtEvent::RecordFound {
                                key: record.record.key.to_vec(),
                                value,
                            }));
                        }
                    }
                    QueryResult::GetRecord(Err(_)) => {
                        if let Some((tx, _)) = self.get_record_queries.remove(&id) {
                            let _ = tx.send(Err(anyhow::anyhow!("Record not found")));
                        }
                    }
                    QueryResult::PutRecord(Ok(_)) => {
                        if let Some(tx) = self.put_record_queries.remove(&id) {
                            let _ = tx.send(Ok(()));
                        }
                    }
                    QueryResult::PutRecord(Err(_)) => {
                        if let Some(tx) = self.put_record_queries.remove(&id) {
                            let _ = tx.send(Err(anyhow::anyhow!("Failed to store record")));
                        }
                    }
                    QueryResult::GetProviders(Ok(GetProvidersOk::FoundProviders { providers, .. })) => {
                        if let Some(tx) = self.get_providers_queries.remove(&id) {
                            let _ = tx.send(Ok(providers.clone()));
                            let _ = event_tx.try_send(NodeEvent::DhtEvent(DhtEvent::ProvidersFound {
                                key: vec![], // Would need to track this
                                providers,
                            }));
                        }
                    }
                    QueryResult::GetProviders(Ok(GetProvidersOk::FinishedWithNoAdditionalRecord { .. })) => {
                        if let Some(tx) = self.get_providers_queries.remove(&id) {
                            let _ = tx.send(Ok(HashSet::new()));
                        }
                    }
                    QueryResult::GetProviders(Err(_)) => {
                        if let Some(tx) = self.get_providers_queries.remove(&id) {
                            let _ = tx.send(Err(anyhow::anyhow!("Failed to get providers")));
                        }
                    }
                    QueryResult::StartProviding(Ok(_)) => {
                        if let Some(tx) = self.start_providing_queries.remove(&id) {
                            let _ = tx.send(Ok(()));
                        }
                    }
                    QueryResult::StartProviding(Err(_)) => {
                        if let Some(tx) = self.start_providing_queries.remove(&id) {
                            let _ = tx.send(Err(anyhow::anyhow!("Failed to start providing")));
                        }
                    }
                    QueryResult::Bootstrap(Ok(result)) => {
                        if let Some(tx) = self.bootstrap_queries.remove(&id) {
                            let _ = tx.send(Ok(()));
                            let _ = event_tx.try_send(NodeEvent::DhtEvent(DhtEvent::BootstrapCompleted {
                                num_peers: result.num_remaining as usize,
                            }));
                        }
                        self.bootstrap_in_progress = false;
                    }
                    QueryResult::Bootstrap(Err(_)) => {
                        if let Some(tx) = self.bootstrap_queries.remove(&id) {
                            let _ = tx.send(Err(anyhow::anyhow!("Bootstrap failed")));
                        }
                        self.bootstrap_in_progress = false;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    
    pub fn register_get_record(&mut self, query_id: QueryId, key: RecordKey, sender: oneshot::Sender<Result<Vec<u8>>>) {
        self.get_record_queries.insert(query_id, (sender, key));
    }
    
    pub fn register_put_record(&mut self, query_id: QueryId, sender: oneshot::Sender<Result<()>>) {
        self.put_record_queries.insert(query_id, sender);
    }
    
    pub fn register_get_providers(&mut self, query_id: QueryId, sender: oneshot::Sender<Result<HashSet<PeerId>>>) {
        self.get_providers_queries.insert(query_id, sender);
    }
    
    pub fn register_start_providing(&mut self, query_id: QueryId, sender: oneshot::Sender<Result<()>>) {
        self.start_providing_queries.insert(query_id, sender);
    }
    
    pub fn register_bootstrap(&mut self, query_id: QueryId, sender: oneshot::Sender<Result<()>>) {
        self.bootstrap_queries.insert(query_id, sender);
        self.bootstrap_in_progress = true;
    }
    
    pub fn is_bootstrap_in_progress(&self) -> bool {
        self.bootstrap_in_progress
    }
    
    pub fn store_record(&mut self, key: RecordKey, value: Vec<u8>, expiration: Option<Duration>) {
        let record = StoredRecord {
            value: value.clone(),
            expiration: expiration.map(|d| Instant::now() + d),
        };
        self.stored_records.insert(key.clone(), record);
        
        // Also track as published record for republishing
        self.published_records.insert(key, PublishedRecord {
            value,
            last_published: Instant::now(),
        });
    }
    
    pub fn get_stored_record(&self, key: &RecordKey) -> Option<Vec<u8>> {
        self.stored_records.get(key).and_then(|record| {
            // Check if record has expired
            if let Some(expiration) = record.expiration {
                if Instant::now() > expiration {
                    return None;
                }
            }
            Some(record.value.clone())
        })
    }
    
    pub fn get_records_to_republish(&mut self) -> Vec<(RecordKey, Vec<u8>)> {
        let now = Instant::now();
        let mut records_to_republish = Vec::new();
        
        for (key, record) in &mut self.published_records {
            if now.duration_since(record.last_published) >= self.republish_interval {
                record.last_published = now;
                records_to_republish.push((key.clone(), record.value.clone()));
            }
        }
        
        records_to_republish
    }
    
    pub fn cleanup_expired_records(&mut self) {
        let now = Instant::now();
        self.stored_records.retain(|_, record| {
            if let Some(expiration) = record.expiration {
                expiration > now
            } else {
                true
            }
        });
    }
    
    pub fn add_announced_capability(&mut self, capability: String) {
        self.announced_capabilities.insert(capability);
    }
}