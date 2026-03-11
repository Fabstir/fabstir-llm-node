// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Tests for the sequential transaction queue (v8.24.0).

use ethers::prelude::*;
use fabstir_llm_node::contracts::tx_queue::{
    is_nonce_error, TransactionQueue, TxQueueConfig, TxRequest, TxResult,
};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

// ---------------------------------------------------------------------------
// Mock transaction sender
// ---------------------------------------------------------------------------

/// Mock that simulates RPC responses for testing the queue processing loop.
#[derive(Clone)]
struct MockTxSender {
    /// Sequence of results to return. Each call pops the first element.
    responses: Arc<Mutex<Vec<Result<H256, String>>>>,
    /// Log of descriptions processed (for FIFO ordering verification).
    log: Arc<Mutex<Vec<String>>>,
}

impl MockTxSender {
    fn new(responses: Vec<Result<H256, String>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn send(&self, description: &str) -> Result<H256, String> {
        self.log.lock().await.push(description.to_string());
        let mut responses = self.responses.lock().await;
        if responses.is_empty() {
            Err("no more mock responses".to_string())
        } else {
            responses.remove(0)
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: spawn a mock processing loop that mimics the real queue behavior
// ---------------------------------------------------------------------------

fn spawn_mock_processor(
    mut rx: mpsc::Receiver<TxRequest>,
    sender: MockTxSender,
    config: TxQueueConfig,
) {
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let mut last_err = String::new();
            let mut result_tx = Some(req.result_tx);

            for attempt in 0..=config.max_retries {
                match sender.send(&req.description).await {
                    Ok(tx_hash) => {
                        let receipt = if req.wait_for_confirmation {
                            Some(Box::new(TransactionReceipt {
                                transaction_hash: tx_hash,
                                status: Some(U64::from(1)),
                                ..Default::default()
                            }))
                        } else {
                            None
                        };
                        if let Some(tx) = result_tx.take() {
                            let _ = tx.send(TxResult::Success { tx_hash, receipt });
                        }
                        break;
                    }
                    Err(e) => {
                        if is_nonce_error(&e) && attempt < config.max_retries {
                            let delay = config.base_retry_delay * 2u32.pow(attempt);
                            tokio::time::sleep(delay).await;
                            last_err = e;
                            continue;
                        }
                        if let Some(tx) = result_tx.take() {
                            let _ = tx.send(TxResult::Failed { error: e });
                        }
                        break;
                    }
                }
            }

            if let Some(tx) = result_tx.take() {
                let _ = tx.send(TxResult::Failed {
                    error: format!("Max retries exhausted: {}", last_err),
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_tx_queue_creation() {
    let queue = TransactionQueue::new(TxQueueConfig::default());
    assert!(queue.sender(84532).is_none());
    assert!(queue.sender(5611).is_none());
}

#[test]
fn test_is_nonce_error_classification() {
    // Should be nonce errors
    assert!(is_nonce_error("nonce too low"));
    assert!(is_nonce_error("replacement transaction underpriced"));
    assert!(is_nonce_error("already known"));
    assert!(is_nonce_error("Error: Nonce Too Low for account"));

    // Should NOT be nonce errors
    assert!(!is_nonce_error("insufficient funds for gas"));
    assert!(!is_nonce_error("execution reverted"));
    assert!(!is_nonce_error("timeout"));
}

#[tokio::test]
async fn test_single_transaction_enqueue_and_result() {
    let config = TxQueueConfig {
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![Ok(H256::from_low_u64_be(42))]);
    spawn_mock_processor(rx, mock, config);

    let (result_tx, result_rx) = oneshot::channel();
    tx.send(TxRequest {
        to: Address::zero(),
        value: U256::zero(),
        data: None,
        description: "test tx".to_string(),
        wait_for_confirmation: false,
        result_tx,
    })
    .await
    .unwrap();

    match result_rx.await.unwrap() {
        TxResult::Success { tx_hash, receipt } => {
            assert_eq!(tx_hash, H256::from_low_u64_be(42));
            assert!(receipt.is_none());
        }
        TxResult::Failed { error } => panic!("Expected success, got: {}", error),
    }
}

#[tokio::test]
async fn test_sequential_ordering_fifo() {
    let config = TxQueueConfig {
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![
        Ok(H256::from_low_u64_be(1)),
        Ok(H256::from_low_u64_be(2)),
        Ok(H256::from_low_u64_be(3)),
    ]);
    let log = mock.log.clone();
    spawn_mock_processor(rx, mock, config);

    let mut receivers = Vec::new();
    for i in 1..=3 {
        let (result_tx, result_rx) = oneshot::channel();
        tx.send(TxRequest {
            to: Address::zero(),
            value: U256::zero(),
            data: None,
            description: format!("tx-{}", i),
            wait_for_confirmation: false,
            result_tx,
        })
        .await
        .unwrap();
        receivers.push(result_rx);
    }

    // All should succeed
    for rx in receivers {
        assert!(matches!(rx.await.unwrap(), TxResult::Success { .. }));
    }

    let processed = log.lock().await;
    assert_eq!(*processed, vec!["tx-1", "tx-2", "tx-3"]);
}

#[tokio::test]
async fn test_fire_and_forget_returns_quickly() {
    let config = TxQueueConfig {
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![Ok(H256::from_low_u64_be(99))]);
    spawn_mock_processor(rx, mock, config);

    let (result_tx, result_rx) = oneshot::channel();
    tx.send(TxRequest {
        to: Address::zero(),
        value: U256::zero(),
        data: None,
        description: "fire-and-forget".to_string(),
        wait_for_confirmation: false,
        result_tx,
    })
    .await
    .unwrap();

    match result_rx.await.unwrap() {
        TxResult::Success { receipt, .. } => assert!(receipt.is_none()),
        TxResult::Failed { error } => panic!("Expected success: {}", error),
    }
}

#[tokio::test]
async fn test_wait_for_confirmation_returns_receipt() {
    let config = TxQueueConfig {
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![Ok(H256::from_low_u64_be(77))]);
    spawn_mock_processor(rx, mock, config);

    let (result_tx, result_rx) = oneshot::channel();
    tx.send(TxRequest {
        to: Address::zero(),
        value: U256::zero(),
        data: None,
        description: "with-confirmation".to_string(),
        wait_for_confirmation: true,
        result_tx,
    })
    .await
    .unwrap();

    match result_rx.await.unwrap() {
        TxResult::Success { tx_hash, receipt } => {
            assert_eq!(tx_hash, H256::from_low_u64_be(77));
            assert!(receipt.is_some());
            assert_eq!(receipt.unwrap().status, Some(U64::from(1)));
        }
        TxResult::Failed { error } => panic!("Expected success: {}", error),
    }
}

#[tokio::test]
async fn test_nonce_error_triggers_retry() {
    let config = TxQueueConfig {
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![
        Err("nonce too low".to_string()),
        Ok(H256::from_low_u64_be(55)),
    ]);
    let log = mock.log.clone();
    spawn_mock_processor(rx, mock, config);

    let (result_tx, result_rx) = oneshot::channel();
    tx.send(TxRequest {
        to: Address::zero(),
        value: U256::zero(),
        data: None,
        description: "retry-tx".to_string(),
        wait_for_confirmation: false,
        result_tx,
    })
    .await
    .unwrap();

    match result_rx.await.unwrap() {
        TxResult::Success { tx_hash, .. } => {
            assert_eq!(tx_hash, H256::from_low_u64_be(55));
        }
        TxResult::Failed { error } => panic!("Expected success after retry: {}", error),
    }

    // Should have been called twice (first fail, then success)
    let processed = log.lock().await;
    assert_eq!(processed.len(), 2);
}

#[tokio::test]
async fn test_max_retries_exhausted_returns_failed() {
    let config = TxQueueConfig {
        max_retries: 2,
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![
        Err("nonce too low".to_string()),
        Err("nonce too low".to_string()),
        Err("nonce too low".to_string()),
    ]);
    spawn_mock_processor(rx, mock, config);

    let (result_tx, result_rx) = oneshot::channel();
    tx.send(TxRequest {
        to: Address::zero(),
        value: U256::zero(),
        data: None,
        description: "exhaust-retries".to_string(),
        wait_for_confirmation: false,
        result_tx,
    })
    .await
    .unwrap();

    match result_rx.await.unwrap() {
        TxResult::Failed { error } => {
            assert!(error.contains("nonce too low") || error.contains("Max retries"));
        }
        TxResult::Success { .. } => panic!("Expected failure after exhausting retries"),
    }
}

#[tokio::test]
async fn test_non_nonce_error_fails_immediately() {
    let config = TxQueueConfig {
        base_retry_delay: std::time::Duration::from_millis(10),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel(64);
    let mock = MockTxSender::new(vec![Err("insufficient funds for gas".to_string())]);
    let log = mock.log.clone();
    spawn_mock_processor(rx, mock, config);

    let (result_tx, result_rx) = oneshot::channel();
    tx.send(TxRequest {
        to: Address::zero(),
        value: U256::zero(),
        data: None,
        description: "no-retry".to_string(),
        wait_for_confirmation: false,
        result_tx,
    })
    .await
    .unwrap();

    match result_rx.await.unwrap() {
        TxResult::Failed { error } => {
            assert!(error.contains("insufficient funds"));
        }
        TxResult::Success { .. } => panic!("Expected immediate failure"),
    }

    // Should have been called exactly once (no retry)
    let processed = log.lock().await;
    assert_eq!(processed.len(), 1);
}

// ---------------------------------------------------------------------------
// Web3Client structural tests (Phase 2)
// ---------------------------------------------------------------------------

#[test]
fn test_web3client_has_enqueue_transaction_method() {
    // Structural: verify Web3Client source contains enqueue_transaction
    let src = include_str!("../src/contracts/client.rs");
    assert!(
        src.contains("pub async fn enqueue_transaction("),
        "Web3Client must have enqueue_transaction method"
    );
    assert!(
        src.contains("tx_queue_sender"),
        "Web3Client must have tx_queue_sender field"
    );
    assert!(
        src.contains("pub fn set_tx_queue_sender("),
        "Web3Client must have set_tx_queue_sender method"
    );
}

#[test]
fn test_web3client_enqueue_fallback_no_queue() {
    // Structural: verify the fallback path exists (direct send when no queue)
    let src = include_str!("../src/contracts/client.rs");
    assert!(
        src.contains("self.send_transaction(to, value, data)"),
        "enqueue_transaction must fall back to send_transaction when no queue"
    );
}

// ---------------------------------------------------------------------------
// CheckpointManager migration structural tests (Phase 3)
// ---------------------------------------------------------------------------

#[test]
fn test_checkpoint_manager_uses_enqueue() {
    let src = include_str!("../src/contracts/checkpoint_manager.rs");
    // No direct send_transaction calls should remain (only enqueue_transaction)
    let direct_calls: Vec<_> = src
        .lines()
        .filter(|l| l.contains(".send_transaction(") && !l.trim_start().starts_with("//"))
        .collect();
    assert!(
        direct_calls.is_empty(),
        "checkpoint_manager.rs should not have direct send_transaction calls, found: {:?}",
        direct_calls
    );
    // enqueue_transaction should appear at least 3 times
    let enqueue_count = src.matches("enqueue_transaction(").count();
    assert!(
        enqueue_count >= 3,
        "Expected at least 3 enqueue_transaction calls, found {}",
        enqueue_count
    );
}

#[test]
fn test_nonce_retry_removed_from_complete_session() {
    let src = include_str!("../src/contracts/checkpoint_manager.rs");
    assert!(
        !src.contains("Retrying with 5 second delay to resolve nonce conflict"),
        "Manual nonce retry in complete_session_job should be removed"
    );
}
