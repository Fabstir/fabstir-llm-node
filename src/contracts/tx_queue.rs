// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Sequential per-chain transaction queue for nonce collision prevention.
//!
//! Ensures only one transaction is in-flight per chain at a time,
//! eliminating "replacement transaction underpriced" errors when
//! multiple checkpoint/settlement calls overlap.

use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

/// A transaction request to be submitted via the queue.
pub struct TxRequest {
    pub to: Address,
    pub value: U256,
    pub data: Option<Bytes>,
    pub description: String,
    pub wait_for_confirmation: bool,
    pub result_tx: oneshot::Sender<TxResult>,
}

/// Result of a queued transaction submission.
#[derive(Debug)]
pub enum TxResult {
    Success {
        tx_hash: H256,
        receipt: Option<Box<TransactionReceipt>>,
    },
    Failed {
        error: String,
    },
}

/// Configuration for the transaction queue.
#[derive(Debug, Clone)]
pub struct TxQueueConfig {
    pub channel_capacity: usize,
    pub max_retries: u32,
    pub base_retry_delay: Duration,
    pub confirmation_timeout: Duration,
}

impl Default for TxQueueConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 64,
            max_retries: 3,
            base_retry_delay: Duration::from_secs(2),
            confirmation_timeout: Duration::from_secs(60),
        }
    }
}

/// Returns true if the error message indicates a nonce collision.
pub fn is_nonce_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("nonce too low")
        || lower.contains("replacement transaction underpriced")
        || lower.contains("already known")
}

/// Per-chain FIFO transaction queue.
pub struct TransactionQueue {
    config: TxQueueConfig,
    senders: HashMap<u64, mpsc::Sender<TxRequest>>,
}

impl TransactionQueue {
    pub fn new(config: TxQueueConfig) -> Self {
        Self {
            config,
            senders: HashMap::new(),
        }
    }

    /// Returns the sender for a given chain, if the chain has been started.
    pub fn sender(&self, chain_id: u64) -> Option<mpsc::Sender<TxRequest>> {
        self.senders.get(&chain_id).cloned()
    }

    /// Start a processing loop for the given chain. Returns the sender for enqueuing requests.
    pub fn start_chain(
        &mut self,
        chain_id: u64,
        signer: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
        provider: Arc<Provider<Http>>,
    ) -> mpsc::Sender<TxRequest> {
        let (tx, rx) = mpsc::channel(self.config.channel_capacity);
        let config = self.config.clone();
        let address = signer.address();

        tokio::spawn(process_chain_queue(
            chain_id, rx, signer, provider, address, config,
        ));

        self.senders.insert(chain_id, tx.clone());
        tx
    }
}

/// Core processing loop: drains the queue one request at a time per chain.
async fn process_chain_queue(
    chain_id: u64,
    mut rx: mpsc::Receiver<TxRequest>,
    signer: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    provider: Arc<Provider<Http>>,
    address: Address,
    config: TxQueueConfig,
) {
    info!(
        "🔗 [TxQueue] Processing loop started for chain {}",
        chain_id
    );

    while let Some(req) = rx.recv().await {
        info!(
            "🔗 [TxQueue] Submitting: {} (chain {})",
            req.description, chain_id
        );
        let mut result_tx = Some(req.result_tx);
        let mut last_err = String::new();

        for attempt in 0..=config.max_retries {
            // Fetch fresh nonce from RPC
            let nonce = match provider
                .get_transaction_count(address, Some(BlockNumber::Pending.into()))
                .await
            {
                Ok(n) => n,
                Err(e) => {
                    error!(
                        "🔗 [TxQueue] Failed to get nonce for chain {}: {}",
                        chain_id, e
                    );
                    if let Some(tx) = result_tx.take() {
                        let _ = tx.send(TxResult::Failed {
                            error: format!("Nonce fetch failed: {e}"),
                        });
                    }
                    break;
                }
            };

            let mut tx_request = TransactionRequest::new()
                .to(req.to)
                .value(req.value)
                .nonce(nonce);

            if let Some(ref data) = req.data {
                tx_request = tx_request.data(data.clone());
            }

            match signer.send_transaction(tx_request, None).await {
                Ok(pending_tx) => {
                    let tx_hash = pending_tx.tx_hash();
                    info!(
                        "🔗 [TxQueue] Sent tx {:?} (nonce={}, chain {})",
                        tx_hash, nonce, chain_id
                    );

                    let receipt = if req.wait_for_confirmation {
                        wait_for_receipt(&provider, tx_hash, config.confirmation_timeout)
                            .await
                            .map(Box::new)
                    } else {
                        None
                    };

                    if let Some(tx) = result_tx.take() {
                        let _ = tx.send(TxResult::Success { tx_hash, receipt });
                    }
                    break;
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    if is_nonce_error(&err_msg) && attempt < config.max_retries {
                        let delay = config.base_retry_delay * 2u32.pow(attempt);
                        warn!(
                            "🔗 [TxQueue] Nonce error on chain {} (attempt {}/{}): {}. Retrying in {:?}",
                            chain_id,
                            attempt + 1,
                            config.max_retries,
                            err_msg,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        last_err = err_msg;
                        continue;
                    }
                    error!(
                        "🔗 [TxQueue] Transaction failed on chain {}: {}",
                        chain_id, err_msg
                    );
                    if let Some(tx) = result_tx.take() {
                        let _ = tx.send(TxResult::Failed { error: err_msg });
                    }
                    break;
                }
            }
        }

        // If all retries exhausted without sending a response
        if let Some(tx) = result_tx.take() {
            let _ = tx.send(TxResult::Failed {
                error: format!("Max retries exhausted: {last_err}"),
            });
        }
    }

    info!(
        "🔗 [TxQueue] Processing loop ended for chain {} (channel closed)",
        chain_id
    );
}

/// Poll for a transaction receipt up to timeout.
async fn wait_for_receipt(
    provider: &Provider<Http>,
    tx_hash: H256,
    timeout: Duration,
) -> Option<TransactionReceipt> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match provider.get_transaction_receipt(tx_hash).await {
            Ok(Some(receipt)) => return Some(receipt),
            Ok(None) => {}
            Err(e) => {
                warn!("🔗 [TxQueue] Receipt poll error: {}", e);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            warn!("🔗 [TxQueue] Timeout waiting for receipt of {:?}", tx_hash);
            return None;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
