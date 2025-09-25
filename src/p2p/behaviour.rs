use libp2p::{
    identify, kad, mdns, rendezvous, request_response, swarm::NetworkBehaviour, StreamProtocol,
};
use std::time::Duration;

use super::protocol_impl::FabstirCodec;
use crate::p2p_config::NodeConfig;

#[derive(NetworkBehaviour)]
pub struct NodeBehaviour {
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub mdns: mdns::tokio::Behaviour,
    pub identify: identify::Behaviour,
    pub rendezvous: rendezvous::client::Behaviour,
    pub request_response: request_response::Behaviour<FabstirCodec>,
}

impl NodeBehaviour {
    pub fn new(
        keypair: &libp2p::identity::Keypair,
        config: &NodeConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let peer_id = keypair.public().to_peer_id();

        // Configure Kademlia
        let mut kad_config = kad::Config::new(libp2p::kad::PROTOCOL_NAME);
        kad_config.set_query_timeout(Duration::from_secs(60));
        kad_config.set_replication_factor(20.try_into().unwrap());

        let store = kad::store::MemoryStore::new(peer_id);
        let mut kad = kad::Behaviour::with_config(peer_id, store, kad_config);

        // Set Kademlia mode based on whether we have bootstrap peers
        if config.bootstrap_peers.is_empty() {
            kad.set_mode(Some(kad::Mode::Server));
        } else {
            kad.set_mode(Some(kad::Mode::Client));
        }

        // Add bootstrap peers to Kademlia
        for (peer_id, addr) in &config.bootstrap_peers {
            kad.add_address(peer_id, addr.clone());
        }

        // Configure mDNS
        let mdns_config = mdns::Config {
            ttl: Duration::from_secs(120),
            query_interval: Duration::from_secs(5),
            enable_ipv6: false,
        };
        let mdns = mdns::tokio::Behaviour::new(mdns_config, peer_id)?;

        // Configure Identify
        let identify_config =
            identify::Config::new("/fabstir/id/1.0.0".to_string(), keypair.public())
                .with_agent_version(format!("fabstir-llm-node/{}", config.protocol_version));

        let identify = identify::Behaviour::new(identify_config);

        // Configure Rendezvous
        let rendezvous = rendezvous::client::Behaviour::new(keypair.clone());

        // Configure Request-Response
        let protocols = vec![
            (
                StreamProtocol::new("/fabstir/inference/1.0.0"),
                request_response::ProtocolSupport::Full,
            ),
            (
                StreamProtocol::new("/fabstir/job/1.0.0"),
                request_response::ProtocolSupport::Full,
            ),
        ];

        let request_response_config = request_response::Config::default()
            .with_request_timeout(Duration::from_secs(60))
            .with_max_concurrent_streams(100);

        let request_response = request_response::Behaviour::with_codec(
            FabstirCodec::default(),
            protocols,
            request_response_config,
        );

        Ok(Self {
            kad,
            mdns,
            identify,
            rendezvous,
            request_response,
        })
    }
}
