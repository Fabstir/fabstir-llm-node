// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4 — GPU END-TO-END proof (`#[ignore]`; run on demand on a GPU host).
//!
//! Drives the **production live-request orchestration** ([`prepare_attested_model`],
//! the same entry the inference load path calls) with NO production edits: a
//! provider signs a policy + encrypts a real GGUF → the node fetches+validates the
//! policy → attests (mock backend) → the KBS releases the DEK → the container is
//! decrypted to **tmpfs** → the decrypted weights are bound to the on-chain model
//! hash → llama.cpp loads them on the **GPU** → real inference runs → the plaintext
//! is securely wiped. Proves the wired encrypted-model pipeline end-to-end on real
//! hardware behind the mock attestation.
//!
//! Run (on the GPU host, e.g. 3XS-Z):
//! ```bash
//! TEE_E2E_GGUF=models/tiny-vicuna-1b.q4_k_m.gguf \
//!   RISC0_SKIP_BUILD=1 cargo test --test tee_e2e -- --ignored --nocapture
//! ```

use async_trait::async_trait;
use fabstir_llm_node::crypto::recover_client_address;
use fabstir_llm_node::inference::{EngineConfig, InferenceRequest, LlmEngine, ModelConfig};
use fabstir_llm_node::tee::container::encrypt_model;
use fabstir_llm_node::tee::mock::{MockAttestationProvider, MockKeyBroker};
use fabstir_llm_node::tee::model_source::{
    is_tmpfs, secure_delete, BlobSource, EncryptedModelLoader,
};
use fabstir_llm_node::tee::orchestration::prepare_attested_model;
use fabstir_llm_node::tee::policy::{
    canonical_policy_bytes, policy_signature_digest, SignedModelPolicy,
};
use fabstir_llm_node::tee::policy_source::{PolicySource, ProviderRegistry};
use fabstir_llm_node::tee::types::{Policy, TeeError, TeeResult};
use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const SKU: &str = "H100";
const MEASUREMENT: [u8; 48] = [0x42u8; 48];
const BLOB_PATH: &str = "m.enc";

/// Serves the single encrypted container (stands in for S5).
struct OneBlob {
    path: String,
    bytes: Vec<u8>,
}
#[async_trait]
impl BlobSource for OneBlob {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>> {
        if path == self.path {
            Ok(self.bytes.clone())
        } else {
            Err(TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                path.to_string(),
            )))
        }
    }
}

/// Mock policy source: serves the one signed policy (HTTP/S5 adapter in production).
struct OnePolicy {
    model_id: [u8; 32],
    signed: SignedModelPolicy,
}
#[async_trait]
impl PolicySource for OnePolicy {
    async fn fetch_policy(&self, model_id: [u8; 32]) -> TeeResult<SignedModelPolicy> {
        if model_id == self.model_id {
            Ok(self.signed.clone())
        } else {
            Err(TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "policy fetch failed",
            )))
        }
    }
}

fn policy(model_id: [u8; 32]) -> Policy {
    Policy {
        policy_version: 1,
        allowed_skus: vec![SKU.to_string()],
        expected_measurement: MEASUREMENT,
        require_cc_on: true,
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 0,
        expiry: u64::MAX - 1,
        model_id,
    }
}

/// Sign `policy` as the provider; return the signed blob + the provider `0x` address.
fn sign(policy: &Policy, encrypted_ref: &str, sk: &SigningKey) -> (SignedModelPolicy, String) {
    let canonical = canonical_policy_bytes(policy).unwrap();
    let digest = policy_signature_digest(&canonical);
    let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash_recoverable(&digest).unwrap();
    let mut sig65 = vec![0u8; 65];
    sig65[..64].copy_from_slice(&sig.to_bytes());
    sig65[64] = recid.to_byte() + 27;
    let provider = recover_client_address(&sig65, &digest).unwrap();
    (
        SignedModelPolicy {
            policy: policy.clone(),
            encrypted_ref: encrypted_ref.to_string(),
            signer: provider.clone(),
            signature: sig65,
        },
        provider,
    )
}

#[tokio::test]
#[ignore = "needs a GPU + a real GGUF; set TEE_E2E_GGUF and run with --ignored"]
async fn encrypted_model_decrypts_attested_and_loads_on_gpu() {
    let gguf = std::env::var("TEE_E2E_GGUF")
        .expect("set TEE_E2E_GGUF=/path/to/model.gguf (e.g. models/tiny-vicuna-1b.q4_k_m.gguf)");
    let plaintext = std::fs::read(&gguf).expect("read source GGUF");
    println!("[e2e] source GGUF {gguf} ({} bytes)", plaintext.len());

    let (model_id, dek) = ([0xABu8; 32], [0xCDu8; 32]);

    // 1. Provider side (offline tooling): sign the policy, then encrypt the weights
    //    bound to that policy's hash (8 MiB chunks).
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let (signed, provider_addr) = sign(&policy(model_id), BLOB_PATH, &sk);
    let policy_hash = signed.policy_hash().expect("policy_hash");
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 8 * 1024 * 1024)
        .expect("encrypt_model");
    println!("[e2e] encrypted container {} bytes", container.len());

    // The on-chain-approved model hash (ModelInfo.sha256_hash) is the SHA-256 of
    // the plaintext GGUF; the orchestration binds the decrypted weights to it (4.3.2).
    let expected_hash = format!("{:x}", Sha256::digest(&plaintext));

    // 2. Node side: the SAME production orchestration a live request triggers —
    //    fetch+validate policy → attest (mock) → KBS DEK → decrypt to tmpfs → hash-bind.
    let decrypt_dir = format!("/dev/shm/tee-e2e-{}", std::process::id());
    let loader = EncryptedModelLoader::new(&decrypt_dir).with_tee_enabled(true);
    let s5 = OneBlob {
        path: BLOB_PATH.to_string(),
        bytes: container,
    };
    let policy_src = OnePolicy { model_id, signed };
    let providers = ProviderRegistry::new().with_provider(model_id, provider_addr);
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, policy(model_id)))]));
    let attestation = MockAttestationProvider::new(SKU, MEASUREMENT, true);

    let prepared = prepare_attested_model(
        &loader,
        &policy_src,
        &providers,
        &s5,
        &kbs,
        &attestation,
        model_id,
        Some(&expected_hash),
    )
    .await
    .expect("prepare_attested_model (live-request orchestration)");
    println!("[e2e] decrypted to {}", prepared.path.display());

    // 3. The plaintext lives ONLY in tmpfs (RAM) and round-trips byte-exact.
    assert!(
        is_tmpfs(&prepared.path),
        "decrypted weights MUST be on tmpfs (RAM): {:?}",
        prepared.path
    );
    assert_eq!(
        std::fs::read(&prepared.path).expect("read decrypted"),
        plaintext,
        "decrypted GGUF must byte-match the original"
    );

    // 4. llama.cpp loads the DECRYPTED model on the GPU and runs real inference.
    //    `encrypted: true` engages the verify→load TOCTOU warning (Phase-4 risk).
    let mut engine = LlmEngine::new(EngineConfig::default())
        .await
        .expect("LlmEngine::new");
    let cfg = ModelConfig {
        model_path: prepared.path.clone(),
        model_type: "llama".to_string(),
        context_size: 512,
        gpu_layers: 99, // offload all layers to the GPU
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
        chat_template: None,
        encrypted: true,
    };
    let mid = engine
        .load_model(cfg)
        .await
        .expect("load the decrypted model on the GPU");
    println!("[e2e] model loaded on GPU, id={mid}");

    let req: InferenceRequest = serde_json::from_value(serde_json::json!({
        "model_id": mid,
        "prompt": "The capital of France is",
        "max_tokens": 16,
        "temperature": 0.7,
        "top_p": 0.95,
        "top_k": 40,
        "min_p": 0.05,
        "seed": 42,
        "stop_sequences": [],
        "stream": false
    }))
    .expect("build InferenceRequest");
    let out = engine.run_inference(req).await.expect("run_inference");
    println!("[e2e] inference output: {:?}", out.text);
    assert!(!out.text.trim().is_empty(), "inference must produce output");
    assert!(out.tokens_generated > 0, "must generate at least one token");

    // 5. Tear down: unload (release the mmap/VRAM), then evict → secure-wipe tmpfs.
    engine.unload_model(&mid).await.ok();
    let decrypted = prepared.path.clone();
    loader.release(&prepared.model_id, &prepared.policy_hash);
    loader.evict_unreferenced();
    let _ = secure_delete(&decrypted);
    assert!(
        !decrypted.exists(),
        "decrypted plaintext must be securely deleted from tmpfs"
    );
    let _ = std::fs::remove_dir_all(&decrypt_dir);
    println!(
        "[e2e] ✅ sign+encrypt → fetch-policy → attest → DEK → decrypt(tmpfs) → \
         hash-bind → GPU load → infer → secure_delete OK"
    );
}
