// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use fabstir_llm_node::{
    api::{ApiConfig, ApiServer},
    contracts::{checkpoint_manager::CheckpointManager, Web3Client, Web3Config},
    inference::{EngineConfig, LlmEngine, ModelConfig},
    p2p::{Node, NodeEvent},
    p2p_config::NodeConfig,
};
use std::{env, path::PathBuf, sync::Arc, time::Duration};
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber for logging
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::init();

    println!("🚀 Starting Fabstir LLM Node...\n");

    // Parse environment variables for configuration
    let p2p_port = env::var("P2P_PORT").unwrap_or_else(|_| "9000".to_string());
    let api_port = env::var("API_PORT").unwrap_or_else(|_| "8080".to_string());
    let model_path = env::var("MODEL_PATH")
        .unwrap_or_else(|_| "./models/tiny-vicuna-1b.q4_k_m.gguf".to_string());
    let gpu_layers = env::var("GPU_LAYERS")
        .unwrap_or_else(|_| "35".to_string())
        .parse::<usize>()
        .unwrap_or(35); // Default to GPU acceleration

    // Configure and initialize inference engine
    println!("🧠 Initializing LLM inference engine...");

    // Read batch size from environment variable
    let batch_size = env::var("LLAMA_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(2048);

    // Read max context length from environment variable
    let max_context_length = env::var("MAX_CONTEXT_LENGTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8192);

    let engine_config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length,
        gpu_layers,
        thread_count: 8,
        batch_size,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 4,
        model_eviction_policy: "lru".to_string(),
    };

    let mut llm_engine = LlmEngine::new(engine_config).await?;
    println!("✅ Inference engine initialized");

    // Load the real GGUF model
    let model_path_buf = PathBuf::from(&model_path);
    let mut model_id = String::new();

    if model_path_buf.exists() {
        println!("📦 Loading model: {}", model_path);
        let model_config = ModelConfig {
            model_path: model_path_buf,
            model_type: "llama".to_string(),
            context_size: max_context_length,
            gpu_layers,
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
            chat_template: None, // Use model's default chat template
        };

        match llm_engine.load_model(model_config).await {
            Ok(id) => {
                model_id = id.clone();
                println!("✅ Model loaded successfully (ID: {})", id);
                println!("   GPU layers: {}", gpu_layers);
                println!("   Context size: {} tokens", max_context_length);
                println!("   Batch size: {} tokens", batch_size);
            }
            Err(e) => {
                eprintln!("❌ Failed to load model: {}", e);
                eprintln!("   The node will start but inference won't work.");
            }
        }
    } else {
        eprintln!("⚠️  Model file not found at: {}", model_path);
        eprintln!("   Please ensure the GGUF model file exists.");
        return Err(anyhow::anyhow!("Model file not found"));
    }

    // Configure P2P node
    println!("\n📡 Configuring P2P networking...");
    let node_config = NodeConfig {
        listen_addresses: vec![
            format!("/ip4/0.0.0.0/tcp/{}", p2p_port).parse()?,
            format!("/ip4/0.0.0.0/tcp/{}", p2p_port.parse::<u16>()? + 1).parse()?,
            format!("/ip4/0.0.0.0/udp/{}/quic-v1", p2p_port.parse::<u16>()? + 2).parse()?,
        ],
        capabilities: vec![
            "llama".to_string(),
            "vicuna".to_string(),
            "tiny-vicuna".to_string(),
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

    // Configure and start API server
    println!("\n🌐 Starting API server...");
    let api_config = ApiConfig {
        listen_addr: format!("0.0.0.0:{}", api_port),
        enable_websocket: true,
        cors_allowed_origins: vec!["*".to_string()],
        ..Default::default()
    };

    // Create API server and pass the loaded model ID
    let api_server = ApiServer::new(api_config).await?;
    api_server.set_engine(Arc::new(llm_engine)).await;
    api_server
        .set_default_model_id(if model_id.is_empty() {
            "tiny-vicuna".to_string()
        } else {
            model_id
        })
        .await;

    // Initialize Embedding Model Manager for /v1/embed endpoint
    println!("🧠 Initializing embedding model manager...");

    // Create default embedding model config for all-MiniLM-L6-v2
    let embedding_configs = vec![
        fabstir_llm_node::embeddings::EmbeddingModelConfig {
            name: "all-MiniLM-L6-v2".to_string(),
            model_path: "./models/all-MiniLM-L6-v2-onnx/model.onnx".to_string(),
            tokenizer_path: "./models/all-MiniLM-L6-v2-onnx/tokenizer.json".to_string(),
            dimensions: 384,
        },
    ];

    match fabstir_llm_node::embeddings::EmbeddingModelManager::new(embedding_configs).await {
        Ok(manager) => {
            let manager = Arc::new(manager);
            api_server.set_embedding_model_manager(manager.clone()).await;
            println!("✅ Embedding model manager initialized");

            // List available models
            let models = manager.list_models();
            if !models.is_empty() {
                println!("   Available embedding models:");
                for model in models {
                    println!("     - {} ({}D)", model.name, model.dimensions);
                }
            }
        }
        Err(e) => {
            println!("⚠️  Failed to initialize embedding model manager: {}", e);
            println!("   /v1/embed endpoint will return 503 Service Unavailable");
            println!("   This is optional - node will continue without embeddings");
        }
    }

    // Initialize Vision Model Manager for /v1/ocr and /v1/describe-image endpoints
    println!("👁️  Initializing vision model manager...");

    let ocr_model_path = env::var("OCR_MODEL_PATH")
        .unwrap_or_else(|_| "./models/paddleocr-onnx".to_string());
    let florence_model_path = env::var("FLORENCE_MODEL_PATH")
        .unwrap_or_else(|_| "./models/florence-2-onnx".to_string());

    let vision_config = fabstir_llm_node::vision::VisionModelConfig {
        ocr_model_dir: Some(ocr_model_path),
        florence_model_dir: Some(florence_model_path),
    };

    match fabstir_llm_node::vision::VisionModelManager::new(vision_config).await {
        Ok(manager) => {
            let manager = Arc::new(manager);
            api_server.set_vision_model_manager(manager.clone()).await;
            println!("✅ Vision model manager initialized");

            // List available vision models
            let models = manager.list_models();
            if !models.is_empty() {
                println!("   Available vision models:");
                for model in models {
                    let status = if model.available { "✓" } else { "✗" };
                    println!("     {} {} ({})", status, model.name, model.model_type);
                }
            } else {
                println!("   No vision models loaded");
                println!("   /v1/ocr and /v1/describe-image will return 503");
            }
        }
        Err(e) => {
            println!("⚠️  Failed to initialize vision model manager: {}", e);
            println!("   /v1/ocr and /v1/describe-image endpoints will return 503");
            println!("   This is optional - node will continue without vision models");
        }
    }

    // Initialize Web3 and CheckpointManager if HOST_PRIVATE_KEY is available
    if let Ok(host_private_key) = env::var("HOST_PRIVATE_KEY") {
        println!("🔗 Initializing Web3 client for checkpoint submission...");

        // Load RPC URL from env or use default
        let rpc_url = env::var("RPC_URL").unwrap_or_else(|_| {
            "https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR".to_string()
        });

        let web3_config = Web3Config {
            rpc_url,
            chain_id: 84532, // Base Sepolia
            private_key: Some(host_private_key),
            ..Default::default()
        };

        match Web3Client::new(web3_config).await {
            Ok(web3_client) => {
                let web3_client = Arc::new(web3_client);
                match CheckpointManager::new(web3_client).await {
                    Ok(checkpoint_manager) => {
                        api_server
                            .set_checkpoint_manager(Arc::new(checkpoint_manager))
                            .await;
                        println!("✅ Checkpoint manager initialized - payments enabled!");
                    }
                    Err(e) => {
                        println!("⚠️  Failed to initialize checkpoint manager: {}", e);
                        println!("   Node will run but automatic checkpoint submission disabled");
                    }
                }
            }
            Err(e) => {
                println!("⚠️  Failed to initialize Web3 client: {}", e);
                println!("   Node will run but automatic checkpoint submission disabled");
            }
        }
    } else {
        println!("ℹ️  HOST_PRIVATE_KEY not set - checkpoint submission disabled");
        println!("   To enable payments, set HOST_PRIVATE_KEY environment variable");
    }

    // The API server is already running in the background (started in new())
    // We don't need to call run() or spawn a task

    println!("✅ API server started on http://0.0.0.0:{}", api_port);

    // Print node information
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("🎉 Fabstir LLM Node is running with REAL inference!");
    println!("{}", separator);
    println!("Peer ID:        {}", peer_id);
    println!(
        "P2P Ports:      {}-{}",
        p2p_port,
        p2p_port.parse::<u16>()? + 2
    );
    println!("API Port:       {}", api_port);
    println!(
        "Model:          {}",
        model_path.split('/').last().unwrap_or("unknown")
    );
    println!("GPU Layers:     {}", gpu_layers);
    println!("\nAPI Endpoints:");
    println!("  Health:       http://localhost:{}/health", api_port);
    println!("  Models:       http://localhost:{}/v1/models", api_port);
    println!(
        "  Inference:    POST http://localhost:{}/v1/inference",
        api_port
    );
    println!("  Embed:        POST http://localhost:{}/v1/embed", api_port);
    println!("  OCR:          POST http://localhost:{}/v1/ocr", api_port);
    println!(
        "  Describe:     POST http://localhost:{}/v1/describe-image",
        api_port
    );
    println!("  WebSocket:    ws://localhost:{}/v1/ws", api_port);
    println!("\nTest with curl:");
    println!(
        "  curl -X POST http://localhost:{}/v1/inference \\",
        api_port
    );
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{");
    println!("      \"model\": \"tiny-vicuna\",");
    println!("      \"prompt\": \"What is the capital of France?\",");
    println!("      \"max_tokens\": 50,");
    println!("      \"temperature\": 0.7");
    println!("    }}'");
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
