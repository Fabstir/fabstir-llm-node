// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NP1.1: no conversation text leaves the node in plaintext through checkpoint deltas.
//!
//! Before 8.66.0 a session without a recovery key had its prompts and replies uploaded to S5 as
//! plaintext JSON, and the delta CID went on chain with the proof. Now a session without a key
//! buffers nothing and uploads nothing; the proof still goes ahead with an empty delta CID.

use async_trait::async_trait;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::XChaCha20Poly1305;
use fabstir_llm_node::checkpoint::encryption::derive_checkpoint_encryption_key;
use fabstir_llm_node::checkpoint::{
    CheckpointMessage, CheckpointPublisher, EncryptedCheckpointDelta,
};
use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Entry, S5ListResult, StorageError};
use fabstir_llm_node::storage::S5Storage;
use k256::ecdsa::SigningKey;
use rand::rngs::OsRng;
use std::collections::HashMap;
use std::sync::Mutex;

/// Records every put so a test can see exactly what left the node.
struct RecordingS5 {
    inner: MockS5Backend,
    puts: Mutex<Vec<(String, Vec<u8>)>>,
}

impl RecordingS5 {
    fn new() -> Self {
        Self {
            inner: MockS5Backend::new(),
            puts: Mutex::new(Vec::new()),
        }
    }
    fn puts(&self) -> Vec<(String, Vec<u8>)> {
        self.puts.lock().unwrap().clone()
    }
}

#[async_trait]
impl S5Storage for RecordingS5 {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        self.puts
            .lock()
            .unwrap()
            .push((path.to_string(), data.clone()));
        self.inner.put(path, data).await
    }
    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        _meta: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        self.put(path, data).await
    }
    async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        self.inner.get(path).await
    }
    async fn get_metadata(&self, path: &str) -> Result<HashMap<String, String>, StorageError> {
        self.inner.get_metadata(path).await
    }
    async fn get_by_cid(&self, cid: &str) -> Result<Vec<u8>, StorageError> {
        self.inner.get_by_cid(cid).await
    }
    async fn list(&self, path: &str) -> Result<Vec<S5Entry>, StorageError> {
        self.inner.list(path).await
    }
    async fn list_with_options(
        &self,
        path: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<S5ListResult, StorageError> {
        self.inner.list_with_options(path, limit, cursor).await
    }
    async fn delete(&self, path: &str) -> Result<(), StorageError> {
        self.inner.delete(path).await
    }
    async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        self.inner.exists(path).await
    }
    fn clone(&self) -> Box<dyn S5Storage> {
        Box::new(RecordingS5::new())
    }
}

fn host_key() -> [u8; 32] {
    SigningKey::random(&mut OsRng).to_bytes().into()
}

/// A user recovery keypair: (private key, compressed public key as 0x hex).
fn recovery_keypair() -> ([u8; 32], String) {
    let sk = SigningKey::random(&mut OsRng);
    let pk = sk.verifying_key().to_encoded_point(true);
    (
        sk.to_bytes().into(),
        format!("0x{}", hex::encode(pk.as_bytes())),
    )
}

/// Give a session a fresh recovery key; returns the user's private key. Shared with the other
/// checkpoint tests, which publish only for keyed sessions since NP1.1.
pub(super) async fn keyed(publisher: &CheckpointPublisher, session_id: &str) -> [u8; 32] {
    let (user_priv, user_pub) = recovery_keypair();
    publisher
        .set_recovery_public_key(session_id, user_pub)
        .await;
    user_priv
}

/// Decrypt an uploaded delta the way the SDK does, returning the delta JSON.
pub(super) fn decrypt(bytes: &[u8], user_private: &[u8; 32]) -> serde_json::Value {
    serde_json::from_slice(&decrypt_bytes(bytes, user_private)).unwrap()
}

/// Decrypt an uploaded delta, returning the plaintext JSON bytes exactly as encrypted.
pub(super) fn decrypt_bytes(bytes: &[u8], user_private: &[u8; 32]) -> Vec<u8> {
    let enc: EncryptedCheckpointDelta = serde_json::from_slice(bytes).expect("encrypted delta");
    assert!(enc.encrypted);
    let eph = hex::decode(&enc.ephemeral_public_key[2..]).unwrap();
    let key = derive_checkpoint_encryption_key(user_private, &eph).unwrap();
    let nonce = hex::decode(&enc.nonce).unwrap();
    let ct = hex::decode(&enc.ciphertext).unwrap();
    XChaCha20Poly1305::new_from_slice(&key)
        .unwrap()
        .decrypt(chacha20poly1305::XNonce::from_slice(&nonce), ct.as_slice())
        .expect("decrypts with the user's key")
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test]
async fn np1_1_no_key_uploads_nothing_and_returns_an_empty_cid() {
    let s5 = RecordingS5::new();
    let publisher = CheckpointPublisher::new("0xhost".to_string());
    let prompt = "my secret plaintext prompt";
    publisher
        .buffer_message(
            "s-nokey",
            CheckpointMessage::new_user(prompt.to_string(), 1),
        )
        .await;
    publisher
        .buffer_message(
            "s-nokey",
            CheckpointMessage::new_assistant("a secret reply".to_string(), 2, false),
        )
        .await;
    publisher
        .update_streaming_response("s-nokey", "partial secret")
        .await;

    // Ok is what lets the caller go on to submit the proof.
    let cid = publisher
        .publish_checkpoint("s-nokey", [7u8; 32], 0, 1000, &host_key(), &s5)
        .await
        .expect("no key must not block the proof");

    assert_eq!(cid, "", "no delta, so the on-chain deltaCID is empty");
    assert!(
        s5.puts().is_empty(),
        "nothing may be uploaded: {:?}",
        s5.puts().iter().map(|p| &p.0).collect::<Vec<_>>()
    );
    let state = publisher.get_session_state("s-nokey").await;
    assert!(
        state.map_or(true, |s| s.message_buffer.is_empty()),
        "nothing buffered without a key"
    );
}

#[tokio::test]
async fn np1_1_with_key_the_delta_is_encrypted_and_no_plaintext_is_uploaded() {
    let s5 = RecordingS5::new();
    let publisher = CheckpointPublisher::new("0xhost".to_string());
    let (user_priv, user_pub) = recovery_keypair();
    let prompt = "what is the launch codeword zebra-42";
    publisher.set_recovery_public_key("s-key", user_pub).await;
    publisher
        .buffer_message("s-key", CheckpointMessage::new_user(prompt.to_string(), 1))
        .await;

    let cid = publisher
        .publish_checkpoint("s-key", [8u8; 32], 0, 500, &host_key(), &s5)
        .await
        .unwrap();

    assert!(!cid.is_empty());
    let puts = s5.puts();
    assert!(!puts.is_empty());
    for (path, bytes) in &puts {
        assert!(
            !contains(bytes, prompt),
            "plaintext prompt found in upload to {path}"
        );
    }
    let (_, delta) = puts
        .iter()
        .find(|(p, _)| p.contains("delta_"))
        .expect("a delta");
    let json = decrypt(delta, &user_priv);
    assert_eq!(json["messages"][0]["content"], prompt);
}

#[tokio::test]
async fn np1_1_a_key_set_mid_session_covers_only_later_messages() {
    let s5 = RecordingS5::new();
    let publisher = CheckpointPublisher::new("0xhost".to_string());
    let (user_priv, user_pub) = recovery_keypair();
    let early = "early message before any key";
    let late = "late message after the key";
    publisher
        .buffer_message("s-mid", CheckpointMessage::new_user(early.to_string(), 1))
        .await;
    // A streamed reply chunk before the key is not kept either.
    publisher
        .update_streaming_response("s-mid", "early streamed reply")
        .await;
    publisher.set_recovery_public_key("s-mid", user_pub).await;
    publisher
        .buffer_message("s-mid", CheckpointMessage::new_user(late.to_string(), 2))
        .await;

    publisher
        .publish_checkpoint("s-mid", [9u8; 32], 0, 500, &host_key(), &s5)
        .await
        .unwrap();

    let puts = s5.puts();
    for (path, bytes) in &puts {
        assert!(
            !contains(bytes, early),
            "early message leaked in plaintext to {path}"
        );
    }
    let (_, delta) = puts
        .iter()
        .find(|(p, _)| p.contains("delta_"))
        .expect("a delta");
    let json = decrypt(delta, &user_priv);
    let msgs = json["messages"].as_array().unwrap();
    assert_eq!(
        msgs.len(),
        1,
        "only the message tracked after the key: {msgs:?}"
    );
    assert_eq!(msgs[0]["content"], late);
}
