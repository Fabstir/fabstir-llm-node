use anyhow::Result;
use fabstir_llm_node::{
    config::NodeConfig,
    p2p::{Node, NodeEvent},
};
use std::{env, time::Duration};
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    // Simple logging setup
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }

    println!("🚀 Starting Fabstir LLM Node...\n");

    // Parse environment variables for configuration
    let p2p_port = env::var("P2P_PORT").unwrap_or_else(|_| "9000".to_string());
    let api_port = env::var("API_PORT").unwrap_or_else(|_| "8080".to_string());

    // Configure P2P node
    println!("📡 Configuring P2P networking...");
    let node_config = NodeConfig {
        listen_addresses: vec![
            format!("/ip4/0.0.0.0/tcp/{}", p2p_port).parse()?,
            format!("/ip4/0.0.0.0/tcp/{}", p2p_port.parse::<u16>()? + 1).parse()?,
            format!("/ip4/0.0.0.0/udp/{}/quic-v1", p2p_port.parse::<u16>()? + 2).parse()?,
        ],
        capabilities: vec![
            "llama-7b".to_string(),
            "vicuna-7b".to_string(),
            "inference".to_string(),
        ],
        enable_mdns: true,
        enable_auto_reconnect: true,
        ..Default::default()
    };

    // Create and start P2P node
    let mut p2p_node = Node::new(node_config).await?;
    let peer_id = p2p_node.peer_id();
    println!("✅ P2P node created with ID: {}", peer_id);

    let mut event_receiver = p2p_node.start().await;
    println!("✅ P2P node started");

    // Wait for listeners to be established
    tokio::time::sleep(Duration::from_millis(500)).await;
    let listeners = p2p_node.listeners();
    for addr in &listeners {
        println!("   Listening on: {}", addr);
    }

    // Print node information
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("🎉 Fabstir LLM Node is running!");
    println!("{}", separator);
    println!("Peer ID:        {}", peer_id);
    println!("P2P Ports:      {}-{}", p2p_port, p2p_port.parse::<u16>()? + 2);
    println!("API Port:       {} (not yet implemented)", api_port);
    println!("\nP2P Addresses:");
    for addr in &listeners {
        println!("  {}", addr);
    }
    println!("\nNote: Full inference and API capabilities are being integrated.");
    println!("Currently running P2P networking layer only.");
    println!("\nPress Ctrl+C to shutdown...");
    println!("{}\n", separator);

    // Handle P2P events in background
    let event_handle = tokio::spawn(async move {
        while let Some(event) = event_receiver.recv().await {
            match event {
                NodeEvent::ConnectionEstablished { peer_id } => {
                    println!("📌 New peer connected: {}", peer_id);
                }
                NodeEvent::ConnectionClosed { peer_id } => {
                    println!("📤 Peer disconnected: {}", peer_id);
                }
                NodeEvent::DiscoveryEvent(e) => {
                    println!("🔍 Discovery: {:?}", e);
                }
                _ => {}
            }
        }
    });

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    
    println!("\n⏹️  Shutting down...");
    
    // Cleanup
    p2p_node.shutdown().await;
    event_handle.abort();
    
    println!("👋 Goodbye!");
    Ok(())
}