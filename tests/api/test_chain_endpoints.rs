use fabstir_llm_node::api::{
    ChainInfo, ChainsResponse, ChainStatistics, ChainStatsResponse,
    SessionInfo, SessionStatus, SessionInfoResponse, TotalStatistics,
};
use fabstir_llm_node::blockchain::{ChainRegistry, ChainConfig};
use serde_json::json;
use std::sync::Arc;

fn setup_test_env() {
    // Simple test setup without env_logger
    std::env::set_var("RUST_LOG", "debug");
}

#[test]
fn test_chain_config_creation() {
    setup_test_env();

    // Test Base Sepolia configuration
    let base_config = ChainConfig::base_sepolia();
    assert_eq!(base_config.chain_id, 84532);
    assert_eq!(base_config.name, "Base Sepolia");
    assert_eq!(base_config.native_token.symbol, "ETH");
    assert_eq!(base_config.native_token.decimals, 18);
    assert!(base_config.is_deployed());

    // Test opBNB Testnet configuration
    let opbnb_config = ChainConfig::opbnb_testnet();
    assert_eq!(opbnb_config.chain_id, 5611);
    assert_eq!(opbnb_config.name, "opBNB Testnet");
    assert_eq!(opbnb_config.native_token.symbol, "BNB");
    assert_eq!(opbnb_config.native_token.decimals, 18);
}

#[test]
fn test_chain_registry() {
    setup_test_env();

    let registry = ChainRegistry::new();

    // Test Base Sepolia is available
    let base = registry.get_chain(84532);
    assert!(base.is_some());
    assert_eq!(base.unwrap().name, "Base Sepolia");

    // Test default chain
    assert_eq!(registry.get_default_chain_id(), 84532);
    assert_eq!(registry.get_default_chain().name, "Base Sepolia");

    // Test all chains
    let all_chains = registry.get_all_chains();
    assert!(all_chains.len() >= 1); // At least Base Sepolia
}

#[test]
fn test_session_info_structure() {
    setup_test_env();

    let session = SessionInfo {
        job_id: 123,
        chain_id: Some(84532),
        user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
        start_time: chrono::Utc::now(),
        tokens_used: 1000,
        status: SessionStatus::Active,
    };

    assert_eq!(session.job_id, 123);
    assert_eq!(session.chain_id, Some(84532));
    assert_eq!(session.status, SessionStatus::Active);
}

#[test]
fn test_chain_info_serialization() {
    setup_test_env();

    let chain_info = ChainInfo {
        chain_id: 84532,
        name: "Base Sepolia".to_string(),
        native_token: "ETH".to_string(),
        rpc_url: "https://sepolia.base.org".to_string(),
        contracts: fabstir_llm_node::blockchain::ContractAddresses {
            job_marketplace: "0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944".to_string(),
            node_registry: "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218".to_string(),
            proof_system: "0x2ACcc60893872A499700908889B38C5420CBcFD1".to_string(),
            host_earnings: "0x908962e8c6CE72610021586f85ebDE09aAc97776".to_string(),
            model_registry: "0x92b2De840bB2171203011A6dBA928d855cA8183E".to_string(),
            usdc_token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".to_string(),
        },
    };

    // Test serialization
    let json_str = serde_json::to_string(&chain_info).unwrap();
    assert!(json_str.contains("84532"));
    assert!(json_str.contains("Base Sepolia"));
    assert!(json_str.contains("ETH"));
}

#[test]
fn test_chains_response_structure() {
    setup_test_env();

    let chains = vec![
        ChainInfo {
            chain_id: 84532,
            name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
            rpc_url: "https://sepolia.base.org".to_string(),
            contracts: fabstir_llm_node::blockchain::ContractAddresses {
                job_marketplace: "0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944".to_string(),
                node_registry: "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218".to_string(),
                proof_system: "0x2ACcc60893872A499700908889B38C5420CBcFD1".to_string(),
                host_earnings: "0x908962e8c6CE72610021586f85ebDE09aAc97776".to_string(),
                model_registry: "0x92b2De840bB2171203011A6dBA928d855cA8183E".to_string(),
                usdc_token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".to_string(),
            },
        },
    ];

    let response = ChainsResponse {
        chains,
        default_chain: 84532,
    };

    assert_eq!(response.chains.len(), 1);
    assert_eq!(response.default_chain, 84532);
}

#[test]
fn test_chain_statistics() {
    setup_test_env();

    let stats = ChainStatistics {
        chain_id: 84532,
        chain_name: "Base Sepolia".to_string(),
        total_sessions: 150,
        active_sessions: 5,
        total_tokens_processed: 1_500_000,
        total_settlements: 145,
        failed_settlements: 2,
        average_settlement_time_ms: 3500,
        last_activity: chrono::Utc::now(),
    };

    assert_eq!(stats.chain_id, 84532);
    assert_eq!(stats.total_sessions, 150);
    assert_eq!(stats.active_sessions, 5);
    assert_eq!(stats.total_tokens_processed, 1_500_000);

    // Test stats response with aggregation
    let total = TotalStatistics {
        total_sessions: 150,
        active_sessions: 5,
        total_tokens_processed: 1_500_000,
    };

    let stats_response = ChainStatsResponse {
        chains: vec![stats],
        total,
    };

    assert_eq!(stats_response.chains.len(), 1);
    assert_eq!(stats_response.total.total_sessions, 150);
}

#[test]
fn test_session_info_response() {
    setup_test_env();

    let response = SessionInfoResponse {
        session_id: 123,
        chain_id: 84532,
        chain_name: "Base Sepolia".to_string(),
        native_token: "ETH".to_string(),
        status: "active".to_string(),
        tokens_used: 450,
    };

    assert_eq!(response.session_id, 123);
    assert_eq!(response.chain_id, 84532);
    assert_eq!(response.chain_name, "Base Sepolia");
    assert_eq!(response.native_token, "ETH");
    assert_eq!(response.status, "active");
    assert_eq!(response.tokens_used, 450);
}

#[test]
fn test_inference_request_with_chain() {
    setup_test_env();

    let request_json = json!({
        "job_id": 123,
        "chain_id": 84532,
        "model": "tinyllama",
        "prompt": "Hello world",
        "max_tokens": 50
    });

    // Verify the JSON structure
    assert_eq!(request_json["chain_id"], 84532);
    assert_eq!(request_json["job_id"], 123);
}

#[test]
fn test_models_response_with_chain() {
    setup_test_env();

    use fabstir_llm_node::api::{ModelsResponse, ModelInfo};

    let response = ModelsResponse {
        models: vec![
            ModelInfo {
                id: "model1".to_string(),
                name: "TinyLlama".to_string(),
                description: Some("Small model".to_string()),
            }
        ],
        chain_id: Some(84532),
        chain_name: Some("Base Sepolia".to_string()),
    };

    assert_eq!(response.models.len(), 1);
    assert_eq!(response.chain_id, Some(84532));
    assert_eq!(response.chain_name, Some("Base Sepolia".to_string()));
}