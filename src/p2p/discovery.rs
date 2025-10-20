// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use libp2p::{kad::RecordKey, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerDiscovered {
        peer_id: PeerId,
        addresses: Vec<Multiaddr>,
        source: DiscoverySource,
    },
    PeerExpired {
        peer_id: PeerId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoverySource {
    Mdns,
    Dht,
    Rendezvous,
    Bootstrap,
}

#[derive(Debug, Clone)]
pub enum DhtEvent {
    BootstrapStarted,
    BootstrapCompleted {
        num_peers: usize,
    },
    RecordStored {
        key: Vec<u8>,
    },
    RecordFound {
        key: Vec<u8>,
        value: Vec<u8>,
    },
    ProvidersFound {
        key: Vec<u8>,
        providers: HashSet<PeerId>,
    },
    CapabilitiesAnnounced {
        capabilities: Vec<String>,
    },
    RecordRepublished {
        key: RecordKey,
    },
}
