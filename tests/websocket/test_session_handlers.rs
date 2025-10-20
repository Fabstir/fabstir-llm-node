// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::websocket::{
    handlers::session_init::SessionInitHandler,
    messages::{ChainInfo, SessionInitMessage, SessionInitResponse},
};
use fabstir_llm_node::contracts::client::ContractClient;
use std::sync::Arc;
use tokio::sync::RwLock;
use ethers::types::{Address, U256};

#[tokio::test]
async fn test_init_handler_with_chain() {
    // Test that session initialization includes chain validation
    let handler = SessionInitHandler::new();

    // Test with valid chain (Base Sepolia)
    let result = handler
        .handle_session_init_with_chain(
            "test-session-base",
            123,
            vec![],
            Some(84532), // Base Sepolia
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.chain_info.is_some());

    let chain_info = response.chain_info.unwrap();
    assert_eq!(chain_info.chain_id, 84532);
    assert_eq!(chain_info.chain_name, "Base Sepolia");
    assert_eq!(chain_info.native_token, "ETH");

    // Test with another valid chain (opBNB Testnet)
    let result = handler
        .handle_session_init_with_chain(
            "test-session-opbnb",
            456,
            vec![],
            Some(5611), // opBNB Testnet
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.chain_info.is_some());

    let chain_info = response.chain_info.unwrap();
    assert_eq!(chain_info.chain_id, 5611);
    assert_eq!(chain_info.chain_name, "opBNB Testnet");
    assert_eq!(chain_info.native_token, "BNB");
}

#[tokio::test]
async fn test_job_verification_on_chain() {
    // Mock job verification on specific chain
    struct MockChainJobVerifier {
        supported_chains: Vec<u64>,
        jobs: std::collections::HashMap<(u64, u64), JobInfo>, // (chain_id, job_id) -> JobInfo
    }

    #[derive(Clone, Debug)]
    struct JobInfo {
        job_id: u64,
        chain_id: u64,
        user: String,
        host: String,
        deposit: U256,
        status: String,
    }

    impl MockChainJobVerifier {
        fn new() -> Self {
            let mut jobs = std::collections::HashMap::new();

            // Add test job on Base Sepolia
            jobs.insert(
                (84532, 100),
                JobInfo {
                    job_id: 100,
                    chain_id: 84532,
                    user: "0xuser1".to_string(),
                    host: "0xhost1".to_string(),
                    deposit: U256::from(1000),
                    status: "active".to_string(),
                },
            );

            // Add test job on opBNB Testnet
            jobs.insert(
                (5611, 200),
                JobInfo {
                    job_id: 200,
                    chain_id: 5611,
                    user: "0xuser2".to_string(),
                    host: "0xhost2".to_string(),
                    deposit: U256::from(2000),
                    status: "active".to_string(),
                },
            );

            Self {
                supported_chains: vec![84532, 5611],
                jobs,
            }
        }

        fn verify_job(&self, chain_id: u64, job_id: u64) -> Result<JobInfo, String> {
            if !self.supported_chains.contains(&chain_id) {
                return Err(format!("Chain {} not supported", chain_id));
            }

            self.jobs
                .get(&(chain_id, job_id))
                .cloned()
                .ok_or_else(|| format!("Job {} not found on chain {}", job_id, chain_id))
        }
    }

    let verifier = MockChainJobVerifier::new();

    // Test: Job exists on correct chain
    let result = verifier.verify_job(84532, 100);
    assert!(result.is_ok());
    let job = result.unwrap();
    assert_eq!(job.chain_id, 84532);
    assert_eq!(job.deposit, U256::from(1000));

    // Test: Job doesn't exist on wrong chain
    let result = verifier.verify_job(5611, 100); // Job 100 is on Base, not opBNB
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));

    // Test: Job on unsupported chain
    let result = verifier.verify_job(1, 100); // Ethereum mainnet not supported
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not supported"));
}

#[tokio::test]
async fn test_streaming_with_chain_context() {
    // Test that streaming responses include chain context
    use fabstir_llm_node::api::websocket::messages::StreamToken;

    struct MockStreamHandler {
        chain_info: Option<ChainInfo>,
    }

    impl MockStreamHandler {
        fn new(chain_id: Option<u64>) -> Self {
            let chain_info = chain_id.map(|id| match id {
                84532 => ChainInfo {
                    chain_id: 84532,
                    chain_name: "Base Sepolia".to_string(),
                    native_token: "ETH".to_string(),
                    rpc_url: "https://sepolia.base.org".to_string(),
                },
                5611 => ChainInfo {
                    chain_id: 5611,
                    chain_name: "opBNB Testnet".to_string(),
                    native_token: "BNB".to_string(),
                    rpc_url: "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
                },
                _ => ChainInfo {
                    chain_id: id,
                    chain_name: "Unknown".to_string(),
                    native_token: "UNKNOWN".to_string(),
                    rpc_url: String::new(),
                },
            });

            Self { chain_info }
        }

        fn create_stream_response(&self, content: String, is_final: bool) -> StreamResponse {
            StreamResponse {
                content,
                is_final,
                chain_info: self.chain_info.clone(),
                tokens_used: 10,
                message_index: 1,
            }
        }
    }

    #[derive(Debug, Clone)]
    struct StreamResponse {
        content: String,
        is_final: bool,
        chain_info: Option<ChainInfo>,
        tokens_used: u32,
        message_index: u32,
    }

    // Test streaming with Base Sepolia context
    let handler = MockStreamHandler::new(Some(84532));
    let response = handler.create_stream_response("Hello".to_string(), false);
    assert!(response.chain_info.is_some());
    assert_eq!(response.chain_info.unwrap().chain_id, 84532);

    // Test streaming with opBNB context
    let handler = MockStreamHandler::new(Some(5611));
    let response = handler.create_stream_response("World".to_string(), true);
    assert!(response.chain_info.is_some());
    assert_eq!(response.chain_info.unwrap().native_token, "BNB");

    // Test streaming without chain context (legacy)
    let handler = MockStreamHandler::new(None);
    let response = handler.create_stream_response("Test".to_string(), false);
    assert!(response.chain_info.is_none());
}

#[tokio::test]
async fn test_chain_switch_request() {
    // Test handling chain switch requests during session
    struct SessionWithChain {
        session_id: String,
        current_chain: u64,
        job_id: u64,
    }

    impl SessionWithChain {
        fn new(session_id: String, chain_id: u64, job_id: u64) -> Self {
            Self {
                session_id,
                current_chain: chain_id,
                job_id,
            }
        }

        fn switch_chain(&mut self, new_chain: u64) -> Result<(), String> {
            // In reality, chain switching mid-session should be rejected
            // as jobs are chain-specific
            if self.current_chain != new_chain {
                return Err(format!(
                    "Cannot switch from chain {} to {} mid-session. Please end current session first.",
                    self.current_chain, new_chain
                ));
            }
            Ok(())
        }

        fn get_chain_info(&self) -> ChainInfo {
            match self.current_chain {
                84532 => ChainInfo {
                    chain_id: 84532,
                    chain_name: "Base Sepolia".to_string(),
                    native_token: "ETH".to_string(),
                    rpc_url: "https://sepolia.base.org".to_string(),
                },
                5611 => ChainInfo {
                    chain_id: 5611,
                    chain_name: "opBNB Testnet".to_string(),
                    native_token: "BNB".to_string(),
                    rpc_url: "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
                },
                _ => panic!("Unsupported chain"),
            }
        }
    }

    // Create session on Base Sepolia
    let mut session = SessionWithChain::new("session-1".to_string(), 84532, 100);
    assert_eq!(session.current_chain, 84532);

    // Attempt to switch to opBNB (should fail)
    let result = session.switch_chain(5611);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Cannot switch"));

    // Verify session still on original chain
    assert_eq!(session.current_chain, 84532);
    let chain_info = session.get_chain_info();
    assert_eq!(chain_info.native_token, "ETH");
}

#[tokio::test]
async fn test_cross_chain_session_rejection() {
    // Test that cross-chain session requests are properly rejected
    struct ChainAwareSessionManager {
        sessions: std::collections::HashMap<String, SessionData>,
    }

    #[derive(Clone)]
    struct SessionData {
        session_id: String,
        chain_id: u64,
        job_id: u64,
        is_active: bool,
    }

    impl ChainAwareSessionManager {
        fn new() -> Self {
            Self {
                sessions: std::collections::HashMap::new(),
            }
        }

        fn create_session(
            &mut self,
            session_id: String,
            chain_id: u64,
            job_id: u64,
        ) -> Result<(), String> {
            // Check if job already has active session on different chain
            for session in self.sessions.values() {
                if session.job_id == job_id && session.chain_id != chain_id && session.is_active {
                    return Err(format!(
                        "Job {} already has active session on chain {}. Cannot create on chain {}",
                        job_id, session.chain_id, chain_id
                    ));
                }
            }

            self.sessions.insert(
                session_id.clone(),
                SessionData {
                    session_id,
                    chain_id,
                    job_id,
                    is_active: true,
                },
            );
            Ok(())
        }

        fn end_session(&mut self, session_id: &str) -> Result<(), String> {
            self.sessions
                .get_mut(session_id)
                .map(|s| s.is_active = false)
                .ok_or_else(|| "Session not found".to_string())
        }
    }

    let mut manager = ChainAwareSessionManager::new();

    // Create session for job 100 on Base Sepolia
    let result = manager.create_session("session-1".to_string(), 84532, 100);
    assert!(result.is_ok());

    // Try to create another session for same job on different chain (should fail)
    let result = manager.create_session("session-2".to_string(), 5611, 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already has active session"));

    // End first session
    manager.end_session("session-1").unwrap();

    // Now creating session on different chain should work
    let result = manager.create_session("session-3".to_string(), 5611, 100);
    assert!(result.is_ok());
}

#[cfg(test)]
mod chain_context_tests {
    use super::*;

    #[test]
    fn test_chain_info_serialization() {
        let chain_info = ChainInfo {
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
            rpc_url: "https://sepolia.base.org".to_string(),
        };

        let json = serde_json::to_string(&chain_info).unwrap();
        assert!(json.contains("\"chain_id\":84532"));
        assert!(json.contains("\"native_token\":\"ETH\""));

        let deserialized: ChainInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.chain_id, chain_info.chain_id);
    }

    #[test]
    fn test_session_chain_context() {
        struct SessionContext {
            chain_id: u64,
            native_token_symbol: String,
            gas_price_multiplier: f64,
        }

        impl SessionContext {
            fn new(chain_id: u64) -> Self {
                let (symbol, multiplier) = match chain_id {
                    84532 => ("ETH".to_string(), 1.1), // Base Sepolia
                    5611 => ("BNB".to_string(), 1.2),  // opBNB
                    _ => ("UNKNOWN".to_string(), 1.0),
                };

                Self {
                    chain_id,
                    native_token_symbol: symbol,
                    gas_price_multiplier: multiplier,
                }
            }
        }

        let ctx = SessionContext::new(84532);
        assert_eq!(ctx.native_token_symbol, "ETH");
        assert_eq!(ctx.gas_price_multiplier, 1.1);

        let ctx = SessionContext::new(5611);
        assert_eq!(ctx.native_token_symbol, "BNB");
        assert_eq!(ctx.gas_price_multiplier, 1.2);
    }
}