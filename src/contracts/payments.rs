// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use super::client::Web3Client;
use super::types::*;

#[derive(Debug, Clone)]
pub struct PaymentConfig {
    pub escrow_address: Address,
    pub supported_tokens: Vec<TokenInfo>,
    pub min_payment_amount: U256,
    pub payment_timeout: Duration,
    pub platform_fee_percentage: u16, // Basis points (250 = 2.5%)
}

impl Default for PaymentConfig {
    fn default() -> Self {
        // Load from environment variable - REQUIRED, NO FALLBACK
        let escrow_address = std::env::var("PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS")
            .expect(
                "❌ FATAL: PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS environment variable MUST be set",
            )
            .parse()
            .expect("❌ FATAL: Invalid PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS format");

        // Get USDC token address from environment (Base Sepolia USDC)
        let usdc_address = std::env::var("USDC_TOKEN")
            .expect("USDC_TOKEN environment variable is required")
            .parse::<Address>()
            .expect("Invalid USDC_TOKEN address format");

        Self {
            escrow_address,
            supported_tokens: vec![
                TokenInfo {
                    symbol: "USDC".to_string(),
                    address: usdc_address,
                    decimals: 6,
                },
                TokenInfo {
                    symbol: "ETH".to_string(),
                    address: Address::zero(),
                    decimals: 18,
                },
            ],
            min_payment_amount: U256::from(1_000_000),
            payment_timeout: Duration::from_secs(3600),
            platform_fee_percentage: 250,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,
    pub address: Address,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentEvent {
    PaymentReleased {
        job_id: U256,
        recipient: Address,
        amount: U256,
    },
    DisputeRaised {
        job_id: U256,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositInfo {
    pub job_id: U256,
    pub amount: U256,
    pub token_symbol: String,
    pub status: PaymentStatus,
    pub client: Address,
}

pub struct PaymentVerifier {
    config: PaymentConfig,
    web3_client: Arc<Web3Client>,
    escrow: PaymentEscrow<Provider<Http>>,
    event_sender: Arc<RwLock<Option<mpsc::Sender<PaymentEvent>>>>,
    token_contracts: Arc<RwLock<HashMap<String, Address>>>,
}

impl PaymentVerifier {
    pub async fn new(config: PaymentConfig, web3_client: Arc<Web3Client>) -> Result<Self> {
        let escrow = PaymentEscrow::new(config.escrow_address, web3_client.provider.clone());

        let mut token_contracts = HashMap::new();
        for token in &config.supported_tokens {
            token_contracts.insert(token.symbol.clone(), token.address);
        }

        Ok(Self {
            config,
            web3_client,
            escrow,
            event_sender: Arc::new(RwLock::new(None)),
            token_contracts: Arc::new(RwLock::new(token_contracts)),
        })
    }

    pub fn is_token_supported(&self, symbol: &str) -> bool {
        self.config
            .supported_tokens
            .iter()
            .any(|t| t.symbol == symbol)
    }

    pub async fn verify_escrow_deposit(&self, job_id: U256) -> Result<DepositInfo> {
        let deposit = self.escrow.get_deposit(job_id).call().await?;

        let token_symbol = self.get_token_symbol(deposit.2).await?;

        Ok(DepositInfo {
            job_id,
            amount: deposit.1,
            token_symbol,
            status: PaymentStatus::from(deposit.3),
            client: deposit.0,
        })
    }

    pub async fn start_monitoring(&mut self) -> mpsc::Receiver<PaymentEvent> {
        let (tx, rx) = mpsc::channel(100);
        *self.event_sender.write().await = Some(tx.clone());

        let verifier = self.clone_for_monitoring();
        tokio::spawn(async move {
            verifier.monitoring_loop().await;
        });

        rx
    }

    pub async fn get_token_balance(&self, symbol: &str, address: Address) -> Result<U256> {
        let token_contracts = self.token_contracts.read().await;
        let token_address = token_contracts
            .get(symbol)
            .ok_or_else(|| anyhow!("Token {} not supported", symbol))?;

        if token_address.is_zero() {
            // ETH balance
            self.web3_client
                .provider
                .get_balance(address, None)
                .await
                .map_err(Into::into)
        } else {
            // ERC20 balance
            let token = IERC20::new(*token_address, self.web3_client.provider.clone());
            token.balance_of(address).call().await.map_err(Into::into)
        }
    }

    pub async fn check_token_approval(
        &self,
        symbol: &str,
        owner: Address,
        amount: U256,
    ) -> Result<bool> {
        let token_contracts = self.token_contracts.read().await;
        let token_address = token_contracts
            .get(symbol)
            .ok_or_else(|| anyhow!("Token {} not supported", symbol))?;

        if token_address.is_zero() {
            // ETH doesn't need approval
            Ok(true)
        } else {
            let token = IERC20::new(*token_address, self.web3_client.provider.clone());
            let allowance = token
                .allowance(owner, self.config.escrow_address)
                .call()
                .await?;
            Ok(allowance >= amount)
        }
    }

    pub fn escrow_address(&self) -> Address {
        self.config.escrow_address
    }

    pub async fn get_payment_status(&self, job_id: U256) -> Result<PaymentStatus> {
        let deposit = self.escrow.get_deposit(job_id).call().await?;
        Ok(PaymentStatus::from(deposit.3))
    }

    pub fn calculate_payment_split(&self, total_amount: U256) -> (U256, U256) {
        let platform_fee =
            total_amount * U256::from(self.config.platform_fee_percentage) / U256::from(10000);
        let host_amount = total_amount - platform_fee;
        (host_amount, platform_fee)
    }

    pub async fn check_payment_timeout(&self, _job_id: U256) -> Result<bool> {
        // In a real implementation, would check job creation timestamp
        // For now, return false
        Ok(false)
    }

    pub async fn can_request_refund(&self, job_id: U256) -> Result<bool> {
        let status = self.get_payment_status(job_id).await?;
        let timed_out = self.check_payment_timeout(job_id).await?;

        Ok(status == PaymentStatus::Locked && timed_out)
    }

    pub async fn process_batch_payments(
        &self,
        job_ids: &[U256],
    ) -> Result<Vec<(U256, Result<H256>)>> {
        let mut results = Vec::new();

        for job_id in job_ids {
            // In a real implementation, would batch process via multicall
            let result = Ok(H256::random());
            results.push((*job_id, result));
        }

        Ok(results)
    }

    pub async fn get_payment_history(
        &self,
        address: Address,
        _days: u64,
    ) -> Result<Vec<PaymentInfo>> {
        // In a real implementation, would query historical events
        Ok(vec![PaymentInfo {
            job_id: U256::from(1),
            amount: U256::from(100_000_000),
            token_symbol: "USDC".to_string(),
            status: PaymentStatus::Released,
            client: address,
        }])
    }

    pub async fn estimate_claim_gas(&self, _job_id: U256) -> Result<U256> {
        // Estimate gas for claiming payment
        Ok(U256::from(150_000))
    }

    pub async fn get_current_gas_price(&self) -> Result<U256> {
        self.web3_client.get_gas_price().await
    }

    async fn get_token_symbol(&self, token_address: Address) -> Result<String> {
        for token in &self.config.supported_tokens {
            if token.address == token_address {
                return Ok(token.symbol.clone());
            }
        }
        Err(anyhow!("Unknown token address"))
    }

    async fn monitoring_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            interval.tick().await;

            // In a real implementation, would monitor for payment events
            // For now, just sleep
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    fn clone_for_monitoring(&self) -> Self {
        Self {
            config: self.config.clone(),
            web3_client: self.web3_client.clone(),
            escrow: self.escrow.clone(),
            event_sender: self.event_sender.clone(),
            token_contracts: self.token_contracts.clone(),
        }
    }
}

// ERC20 interface for token interactions
abigen!(
    IERC20,
    r#"[
        {
            "inputs": [{"internalType": "address", "name": "account", "type": "address"}],
            "name": "balanceOf",
            "outputs": [{"internalType": "uint256", "name": "", "type": "uint256"}],
            "stateMutability": "view",
            "type": "function"
        },
        {
            "inputs": [
                {"internalType": "address", "name": "owner", "type": "address"},
                {"internalType": "address", "name": "spender", "type": "address"}
            ],
            "name": "allowance",
            "outputs": [{"internalType": "uint256", "name": "", "type": "uint256"}],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
);
