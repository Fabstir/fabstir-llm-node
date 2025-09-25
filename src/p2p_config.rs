use libp2p::{identity::Keypair, Multiaddr, PeerId};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub keypair: Option<Keypair>,
    pub listen_addresses: Vec<Multiaddr>,
    pub external_addresses: Vec<Multiaddr>,
    pub bootstrap_peers: Vec<(PeerId, Multiaddr)>,
    pub max_connections: usize,
    pub max_connections_per_peer: usize,
    pub connection_idle_timeout: Duration,
    pub capabilities: Vec<String>,
    pub enable_auto_reconnect: bool,
    pub reconnect_interval: Duration,
    pub protocol_version: String,
    pub supported_protocols: Vec<String>,
    pub max_requests_per_minute: usize,
    pub enable_mdns: bool,
    pub mdns_service_name: Option<String>,
    pub enable_rendezvous_server: bool,
    pub enable_rendezvous_client: bool,
    pub rendezvous_servers: Vec<(PeerId, Multiaddr)>,
    pub node_metadata: Option<serde_json::Value>,
    pub discovery_rate_limit: Option<Duration>,
    pub peer_expiration_time: Duration,
    pub dht_bootstrap_interval: Duration,
    pub dht_republish_interval: Duration,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            keypair: None,
            listen_addresses: vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
            ],
            external_addresses: vec![],
            bootstrap_peers: vec![],
            max_connections: 200,
            max_connections_per_peer: 5,
            connection_idle_timeout: Duration::from_secs(120),
            capabilities: vec![],
            enable_auto_reconnect: false,
            reconnect_interval: Duration::from_secs(30),
            protocol_version: "1.0.0".to_string(),
            supported_protocols: vec![
                "/fabstir/inference/1.0.0".to_string(),
                "/fabstir/job/1.0.0".to_string(),
            ],
            max_requests_per_minute: 100,
            enable_mdns: true,
            mdns_service_name: None,
            enable_rendezvous_server: false,
            enable_rendezvous_client: false,
            rendezvous_servers: vec![],
            node_metadata: None,
            discovery_rate_limit: None,
            peer_expiration_time: Duration::from_secs(300),
            dht_bootstrap_interval: Duration::from_secs(300),
            dht_republish_interval: Duration::from_secs(3600),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionLimits {
    pub max_connections: usize,
    pub max_connections_per_peer: usize,
    pub idle_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct NodeMetrics {
    pub connected_peers: usize,
    pub bandwidth_in: u64,
    pub bandwidth_out: u64,
    pub uptime: Duration,
}

#[derive(Clone, Debug)]
pub struct DhtRoutingTableHealth {
    pub num_peers: usize,
    pub num_buckets: usize,
    pub pending_queries: usize,
}

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub metadata: serde_json::Value,
}
