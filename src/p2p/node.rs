use anyhow::{anyhow, Result};
use futures::{channel::mpsc, StreamExt};
use libp2p::{
    identity::Keypair,
    kad::RecordKey,
    swarm::SwarmEvent,
    Multiaddr, PeerId, SwarmBuilder,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc as tokio_mpsc, oneshot, Mutex, RwLock},
    time::interval,
    task::JoinHandle,
};

use crate::config::{ConnectionLimits, DhtRoutingTableHealth, NodeConfig, NodeMetrics, PeerInfo};
use crate::p2p::{
    behaviour::NodeBehaviour,
    dht::DhtHandler,
    discovery::{DhtEvent, DiscoveryEvent},
    protocols::{
        InferenceRequest, InferenceResponse, JobClaim, JobResult, ProtocolEvent,
        ProtocolHandler,
    },
};

#[derive(Debug, Clone)]
pub enum NodeEvent {
    NewListenAddr { address: Multiaddr },
    ConnectionEstablished { peer_id: PeerId },
    ConnectionClosed { peer_id: PeerId },
    DiscoveryEvent(DiscoveryEvent),
    DhtEvent(DhtEvent),
    ProtocolEvent(ProtocolEvent),
}

enum Command {
    Connect { peer_id: PeerId, addr: Multiaddr, result_sender: oneshot::Sender<Result<()>> },
    DhtPut { key: RecordKey, value: Vec<u8>, expiration: Option<Duration>, result_sender: oneshot::Sender<Result<()>> },
    DhtGet { key: RecordKey, result_sender: oneshot::Sender<Result<Vec<u8>>> },
    DhtStartProviding { key: RecordKey, result_sender: oneshot::Sender<Result<()>> },
    DhtGetProviders { key: RecordKey, result_sender: oneshot::Sender<Result<HashSet<PeerId>>> },
    GetListeners { result_sender: oneshot::Sender<Vec<Multiaddr>> },
    SendEvent { event: NodeEvent, result_sender: oneshot::Sender<()> },
    Shutdown,
}

pub struct Node {
    peer_id: PeerId,
    config: NodeConfig,
    command_sender: Option<tokio_mpsc::Sender<Command>>,
    event_receiver: Option<tokio_mpsc::Receiver<NodeEvent>>,
    is_running: Arc<RwLock<bool>>,
    start_time: Instant,
    connected_peers: Arc<RwLock<HashSet<PeerId>>>,
    discovered_peers: Arc<RwLock<HashSet<PeerId>>>,
    peer_metadata: Arc<RwLock<HashMap<PeerId, serde_json::Value>>>,
    protocol_handler: Arc<Mutex<ProtocolHandler>>,
    streaming_handlers: Arc<Mutex<HashMap<String, mpsc::Sender<InferenceResponse>>>>,
    rate_limiters: Arc<Mutex<HashMap<PeerId, (Instant, usize)>>>,
    bandwidth_counter: Arc<Mutex<(u64, u64)>>,
    swarm_task: Option<JoinHandle<()>>,
    listeners: Arc<RwLock<Vec<Multiaddr>>>,
}

impl Node {
    pub async fn new(config: NodeConfig) -> Result<Self> {
        let keypair = config
            .keypair
            .clone()
            .unwrap_or_else(|| Keypair::generate_ed25519());
        let peer_id = PeerId::from(keypair.public());

        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
            .with_quic()
            .with_behaviour(|key| NodeBehaviour::new(key, &config).expect("Failed to create behaviour"))?
            .with_swarm_config(|cfg| {
                cfg.with_idle_connection_timeout(config.connection_idle_timeout)
            })
            .build();

        // Listen on configured addresses
        let mut initial_listeners = Vec::new();
        for addr in &config.listen_addresses {
            swarm.listen_on(addr.clone())?;
            initial_listeners.push(addr.clone());
        }

        // Add external addresses
        for addr in &config.external_addresses {
            swarm.add_external_address(addr.clone());
        }

        // Bootstrap with configured peers
        for (peer_id, addr) in &config.bootstrap_peers {
            swarm.dial(addr.clone())?;
            swarm.behaviour_mut().kad.add_address(peer_id, addr.clone());
        }

        // Create command channel
        let (command_tx, mut command_rx) = tokio_mpsc::channel::<Command>(100);
        let (event_tx, event_rx) = tokio_mpsc::channel::<NodeEvent>(1000);

        let connected_peers = Arc::new(RwLock::new(HashSet::new()));
        let discovered_peers = Arc::new(RwLock::new(HashSet::new()));
        let is_running = Arc::new(RwLock::new(false));
        let listeners = Arc::new(RwLock::new(initial_listeners));

        // Clone for the swarm task
        let connected_peers_clone = connected_peers.clone();
        let discovered_peers_clone = discovered_peers.clone();
        let is_running_clone = is_running.clone();
        let listeners_clone = listeners.clone();
        let config_clone = config.clone();

        // Spawn swarm event loop
        let swarm_task = tokio::spawn(async move {
            let mut swarm = swarm;
            let mut dht_handler = DhtHandler::new(
                config_clone.dht_bootstrap_interval,
                config_clone.dht_republish_interval,
            );
            
            // Start bootstrap if we have bootstrap peers
            if !config_clone.bootstrap_peers.is_empty() {
                if let Ok(query_id) = swarm.behaviour_mut().kad.bootstrap() {
                    let (tx, _rx) = oneshot::channel();
                    dht_handler.register_bootstrap(query_id, tx);
                    let _ = event_tx.send(NodeEvent::DhtEvent(DhtEvent::BootstrapStarted)).await;
                }
            }
            
            // Set up periodic bootstrap
            let mut bootstrap_interval = if config_clone.dht_bootstrap_interval > Duration::ZERO {
                Some(interval(config_clone.dht_bootstrap_interval))
            } else {
                None
            };
            
            // Set up periodic republish
            let mut republish_interval = if config_clone.dht_republish_interval > Duration::ZERO {
                Some(interval(config_clone.dht_republish_interval))
            } else {
                None
            };
            
            // Set up periodic cleanup (every 60 seconds)
            let mut cleanup_interval = interval(Duration::from_secs(60));
            
            loop {
                tokio::select! {
                    Some(command) = command_rx.recv() => {
                        match command {
                            Command::Connect { peer_id, addr, result_sender } => {
                                let result = swarm.dial(addr.clone()).map(|_| {
                                    swarm.behaviour_mut().kad.add_address(&peer_id, addr);
                                });
                                let _ = result_sender.send(result.map_err(|e| anyhow!(e.to_string())));
                            }
                            Command::DhtPut { key, value, expiration, result_sender } => {
                                let record = libp2p::kad::Record::new(key.clone(), value.clone());
                                match swarm.behaviour_mut().kad.put_record(record, libp2p::kad::Quorum::One) {
                                    Ok(query_id) => {
                                        dht_handler.register_put_record(query_id, result_sender);
                                        dht_handler.store_record(key, value, expiration);
                                    }
                                    Err(e) => {
                                        let _ = result_sender.send(Err(anyhow!(e.to_string())));
                                    }
                                }
                            }
                            Command::DhtGet { key, result_sender } => {
                                // Always query the DHT, but the handler will check expiration
                                let query_id = swarm.behaviour_mut().kad.get_record(key.clone());
                                dht_handler.register_get_record(query_id, key, result_sender);
                            }
                            Command::DhtStartProviding { key, result_sender } => {
                                match swarm.behaviour_mut().kad.start_providing(key) {
                                    Ok(query_id) => {
                                        dht_handler.register_start_providing(query_id, result_sender);
                                    }
                                    Err(e) => {
                                        let _ = result_sender.send(Err(anyhow!(e.to_string())));
                                    }
                                }
                            }
                            Command::DhtGetProviders { key, result_sender } => {
                                let query_id = swarm.behaviour_mut().kad.get_providers(key);
                                dht_handler.register_get_providers(query_id, result_sender);
                            }
                            Command::GetListeners { result_sender } => {
                                let listeners: Vec<_> = swarm.listeners().cloned().collect();
                                let _ = result_sender.send(listeners);
                            }
                            Command::SendEvent { event, result_sender } => {
                                let _ = event_tx.send(event).await;
                                let _ = result_sender.send(());
                            }
                            Command::Shutdown => {
                                break;
                            }
                        }
                    }
                    Some(event) = swarm.next() => {
                        match event {
                            SwarmEvent::NewListenAddr { address, .. } => {
                                listeners_clone.write().await.push(address.clone());
                                let _ = event_tx.send(NodeEvent::NewListenAddr { address }).await;
                            }
                            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                                connected_peers_clone.write().await.insert(peer_id);
                                let _ = event_tx.send(NodeEvent::ConnectionEstablished { peer_id }).await;
                            }
                            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                                connected_peers_clone.write().await.remove(&peer_id);
                                let _ = event_tx.send(NodeEvent::ConnectionClosed { peer_id }).await;
                            }
                            SwarmEvent::Behaviour(event) => {
                                match event {
                                    crate::p2p::behaviour::NodeBehaviourEvent::Kad(kad_event) => {
                                        dht_handler.handle_event(kad_event, &event_tx);
                                    }
                                    crate::p2p::behaviour::NodeBehaviourEvent::Mdns(mdns_event) => {
                                        match mdns_event {
                                            libp2p::mdns::Event::Discovered(peers) => {
                                                for (peer_id, addr) in peers {
                                                    discovered_peers_clone.write().await.insert(peer_id);
                                                    swarm.behaviour_mut().kad.add_address(&peer_id, addr.clone());
                                                    let _ = event_tx.send(NodeEvent::DiscoveryEvent(
                                                        DiscoveryEvent::PeerDiscovered {
                                                            peer_id,
                                                            addresses: vec![addr],
                                                            source: crate::p2p::discovery::DiscoverySource::Mdns,
                                                        }
                                                    )).await;
                                                }
                                            }
                                            libp2p::mdns::Event::Expired(peers) => {
                                                for (peer_id, _) in peers {
                                                    discovered_peers_clone.write().await.remove(&peer_id);
                                                    let _ = event_tx.send(NodeEvent::DiscoveryEvent(
                                                        DiscoveryEvent::PeerExpired { peer_id }
                                                    )).await;
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                    _ = bootstrap_interval.as_mut().unwrap().tick(), if bootstrap_interval.is_some() => {
                        if !dht_handler.is_bootstrap_in_progress() {
                            if let Ok(query_id) = swarm.behaviour_mut().kad.bootstrap() {
                                let (tx, _rx) = oneshot::channel();
                                dht_handler.register_bootstrap(query_id, tx);
                                let _ = event_tx.send(NodeEvent::DhtEvent(DhtEvent::BootstrapStarted)).await;
                            }
                        }
                    }
                    _ = republish_interval.as_mut().unwrap().tick(), if republish_interval.is_some() => {
                        // Clean up expired records
                        dht_handler.cleanup_expired_records();
                        
                        // Republish records that need republishing
                        let records_to_republish = dht_handler.get_records_to_republish();
                        for (key, value) in records_to_republish {
                            let record = libp2p::kad::Record::new(key.clone(), value);
                            if let Ok(_) = swarm.behaviour_mut().kad.put_record(record, libp2p::kad::Quorum::One) {
                                let _ = event_tx.send(NodeEvent::DhtEvent(DhtEvent::RecordRepublished { key })).await;
                            }
                        }
                    }
                    _ = cleanup_interval.tick() => {
                        // Periodic cleanup of expired records
                        dht_handler.cleanup_expired_records();
                    }
                }
            }
            
            *is_running_clone.write().await = false;
        });

        Ok(Self {
            peer_id,
            config: config.clone(),
            command_sender: Some(command_tx),
            event_receiver: Some(event_rx),
            is_running,
            start_time: Instant::now(),
            connected_peers,
            discovered_peers,
            peer_metadata: Arc::new(RwLock::new(HashMap::new())),
            protocol_handler: Arc::new(Mutex::new(ProtocolHandler::new(
                config.protocol_version.clone(),
                config.supported_protocols.clone(),
            ))),
            streaming_handlers: Arc::new(Mutex::new(HashMap::new())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            bandwidth_counter: Arc::new(Mutex::new((0, 0))),
            swarm_task: Some(swarm_task),
            listeners,
        })
    }

    pub async fn start(&mut self) -> tokio_mpsc::Receiver<NodeEvent> {
        *self.is_running.write().await = true;
        self.start_time = Instant::now();

        let event_rx = self.event_receiver.take()
            .expect("start() called multiple times");

        // Start periodic tasks
        if let Some(command_tx) = &self.command_sender {
            self.start_periodic_tasks(command_tx.clone());
        }

        event_rx
    }

    pub async fn shutdown(&mut self) {
        *self.is_running.write().await = false;
        
        if let Some(tx) = self.command_sender.take() {
            let _ = tx.send(Command::Shutdown).await;
        }
        
        if let Some(task) = self.swarm_task.take() {
            let _ = task.await;
        }
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn listeners(&self) -> Vec<Multiaddr> {
        // Use try_read to avoid blocking
        self.listeners.try_read().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn external_addresses(&self) -> Vec<Multiaddr> {
        self.config.external_addresses.clone()
    }

    pub fn bootstrap_peers(&self) -> Vec<(PeerId, Multiaddr)> {
        self.config.bootstrap_peers.clone()
    }

    pub fn capabilities(&self) -> Vec<String> {
        self.config.capabilities.clone()
    }

    pub fn is_running(&self) -> bool {
        self.is_running.try_read().map(|r| *r).unwrap_or(false)
    }

    pub fn metrics(&self) -> NodeMetrics {
        let bandwidth = self.bandwidth_counter.try_lock().map(|b| (b.0, b.1)).unwrap_or((0, 0));
        let connected_peers = self.connected_peers.try_read().map(|p| p.len()).unwrap_or(0);
        NodeMetrics {
            connected_peers,
            bandwidth_in: bandwidth.0,
            bandwidth_out: bandwidth.1,
            uptime: self.start_time.elapsed(),
        }
    }

    pub fn connection_limits(&self) -> ConnectionLimits {
        ConnectionLimits {
            max_connections: self.config.max_connections,
            max_connections_per_peer: self.config.max_connections_per_peer,
            idle_timeout: self.config.connection_idle_timeout,
        }
    }

    pub fn is_auto_reconnect_enabled(&self) -> bool {
        self.config.enable_auto_reconnect
    }

    pub fn reconnect_interval(&self) -> Duration {
        self.config.reconnect_interval
    }

    pub async fn connect(&mut self, peer_id: PeerId, addr: Multiaddr) -> Result<()> {
        if let Some(tx) = &self.command_sender {
            let (result_tx, result_rx) = oneshot::channel();
            tx.send(Command::Connect { peer_id, addr, result_sender: result_tx }).await?;
            result_rx.await?
        } else {
            Err(anyhow!("Node not started"))
        }
    }

    pub fn is_connected(&self, peer_id: PeerId) -> bool {
        self.connected_peers.try_read().map(|p| p.contains(&peer_id)).unwrap_or(false)
    }

    pub async fn discovered_peers(&self) -> HashSet<PeerId> {
        self.discovered_peers.read().await.clone()
    }

    // DHT operations
    pub async fn dht_put(&mut self, key: RecordKey, value: Vec<u8>) -> Result<()> {
        if let Some(tx) = &self.command_sender {
            let (result_tx, result_rx) = oneshot::channel();
            tx.send(Command::DhtPut { key, value, expiration: None, result_sender: result_tx }).await?;
            result_rx.await?
        } else {
            Err(anyhow!("Node not started"))
        }
    }

    pub async fn dht_get(&mut self, key: RecordKey) -> Result<Vec<u8>> {
        if let Some(tx) = &self.command_sender {
            let (result_tx, result_rx) = oneshot::channel();
            tx.send(Command::DhtGet { key, result_sender: result_tx }).await?;
            result_rx.await?
        } else {
            Err(anyhow!("Node not started"))
        }
    }

    pub async fn dht_put_with_expiration(
        &mut self,
        key: RecordKey,
        value: Vec<u8>,
        expiration: Duration,
    ) -> Result<()> {
        if let Some(tx) = &self.command_sender {
            let (result_tx, result_rx) = oneshot::channel();
            tx.send(Command::DhtPut { key, value, expiration: Some(expiration), result_sender: result_tx }).await?;
            result_rx.await?
        } else {
            Err(anyhow!("Node not started"))
        }
    }

    pub async fn dht_start_providing(&mut self, key: RecordKey) -> Result<()> {
        if let Some(tx) = &self.command_sender {
            let (result_tx, result_rx) = oneshot::channel();
            tx.send(Command::DhtStartProviding { key, result_sender: result_tx }).await?;
            result_rx.await?
        } else {
            Err(anyhow!("Node not started"))
        }
    }

    pub async fn dht_get_providers(&mut self, key: RecordKey) -> Result<HashSet<PeerId>> {
        if let Some(tx) = &self.command_sender {
            let (result_tx, result_rx) = oneshot::channel();
            tx.send(Command::DhtGetProviders { key, result_sender: result_tx }).await?;
            result_rx.await?
        } else {
            Err(anyhow!("Node not started"))
        }
    }

    pub async fn dht_get_closest_peers(&mut self, peer_id: PeerId) -> Result<Vec<PeerId>> {
        // For now, return connected peers as a simple implementation
        let peers = self.connected_peers.read().await;
        Ok(peers.iter().take(20).cloned().collect())
    }

    pub fn dht_routing_table_health(&self) -> DhtRoutingTableHealth {
        // In a real implementation, we'd query the swarm's Kademlia behaviour
        // For now, return connected peers count as an approximation
        let num_peers = self.connected_peers.try_read().map(|peers| peers.len()).unwrap_or(0);
        DhtRoutingTableHealth {
            num_peers,
            num_buckets: 20, // Kademlia default
            pending_queries: 0,
        }
    }

    pub async fn announce_capabilities(&mut self) -> Result<()> {
        let capabilities = self.config.capabilities.clone();
        for capability in &capabilities {
            let key = RecordKey::new(&format!("capability:{}", capability).as_bytes());
            self.dht_start_providing(key).await?;
        }
        // Send event for announced capabilities
        if let Some(tx) = &self.command_sender {
            let (result_tx, _) = oneshot::channel();
            let _ = tx.send(Command::SendEvent {
                event: NodeEvent::DhtEvent(DhtEvent::CapabilitiesAnnounced { capabilities }),
                result_sender: result_tx,
            }).await;
        }
        Ok(())
    }

    pub async fn find_nodes_with_capability(&mut self, capability: &str) -> Result<Vec<PeerId>> {
        let key = RecordKey::new(&format!("capability:{}", capability).as_bytes());
        self.dht_get_providers(key).await.map(|set| set.into_iter().collect())
    }

    pub async fn discover_peers_with_capability(&mut self, capability: &str) -> Result<Vec<PeerId>> {
        self.find_nodes_with_capability(capability).await
    }

    pub async fn announce_with_metadata(&mut self) -> Result<()> {
        if let Some(metadata) = &self.config.node_metadata {
            let key = RecordKey::new(&format!("metadata:{}", self.peer_id).as_bytes());
            let value = serde_json::to_vec(metadata)?;
            self.dht_put(key, value).await?;
        }
        Ok(())
    }

    pub async fn get_peer_metadata(&mut self, peer_id: PeerId) -> Result<PeerInfo> {
        let key = RecordKey::new(&format!("metadata:{}", peer_id).as_bytes());
        let value = self.dht_get(key).await?;
        let metadata = serde_json::from_slice(&value)?;
        Ok(PeerInfo { peer_id, metadata })
    }

    pub async fn discover_peers_sorted_by_priority(&mut self) -> Result<Vec<(PeerId, u32)>> {
        // Simplified implementation
        Ok(vec![])
    }

    // Rendezvous operations
    pub async fn register_rendezvous(&mut self, _namespace: &str) -> Result<()> {
        // TODO: Implement
        Ok(())
    }

    pub async fn discover_rendezvous(&mut self, _namespace: &str) -> Result<()> {
        if let Some(rate_limit) = self.config.discovery_rate_limit {
            let now = Instant::now();
            let mut rate_limiters = self.rate_limiters.lock().await;
            if let Some((last_time, _)) = rate_limiters.get(&self.peer_id) {
                if now.duration_since(*last_time) < rate_limit {
                    return Err(anyhow!("Rate limited"));
                }
            }
            rate_limiters.insert(self.peer_id, (now, 1));
        }
        
        // TODO: Implement actual rendezvous discovery
        Ok(())
    }

    // Protocol operations
    pub async fn send_inference_request(
        &mut self,
        _peer_id: PeerId,
        _request: InferenceRequest,
    ) -> Result<()> {
        // TODO: Implement
        Ok(())
    }

    pub async fn send_inference_request_with_timeout(
        &mut self,
        peer_id: PeerId,
        request: InferenceRequest,
        _timeout: Duration,
    ) -> Result<()> {
        self.send_inference_request(peer_id, request).await
    }

    pub async fn send_streaming_inference_request(
        &mut self,
        peer_id: PeerId,
        request: InferenceRequest,
    ) -> Result<mpsc::Receiver<InferenceResponse>> {
        let (tx, rx) = mpsc::channel(100);
        self.streaming_handlers
            .lock()
            .await
            .insert(request.request_id.clone(), tx);
        
        self.send_inference_request(peer_id, request).await?;
        Ok(rx)
    }

    pub async fn send_inference_response(
        &mut self,
        _peer_id: PeerId,
        _response: InferenceResponse,
    ) -> Result<()> {
        // TODO: Implement
        Ok(())
    }

    pub async fn send_streaming_response(
        &mut self,
        peer_id: PeerId,
        response: InferenceResponse,
    ) -> Result<()> {
        self.send_inference_response(peer_id, response).await
    }

    pub async fn send_job_claim(&mut self, _peer_id: PeerId, _claim: JobClaim) -> Result<()> {
        // TODO: Implement
        Ok(())
    }

    pub async fn send_job_result(&mut self, _peer_id: PeerId, _result: JobResult) -> Result<()> {
        // TODO: Implement
        Ok(())
    }

    // Helper methods
    fn start_periodic_tasks(&self, tx: tokio_mpsc::Sender<Command>) {
        // DHT bootstrap
        if self.config.dht_bootstrap_interval > Duration::ZERO {
            let interval_duration = self.config.dht_bootstrap_interval;
            let _event_tx = tx.clone();
            tokio::spawn(async move {
                let mut interval = interval(interval_duration);
                loop {
                    interval.tick().await;
                    // Would send bootstrap event
                }
            });
        }
    }
}