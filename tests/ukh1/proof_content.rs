// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! U2b / NP2 (plan D20): the proof journal's input and output fields are
//! `HMAC-SHA256(HKDF-SHA256(session key, info "fabstir-proof-content-v1:" + job id), content)`,
//! never a plain SHA-256 of user content.

use fabstir_llm_node::contracts::checkpoint_manager::ContentCommitments;
use fabstir_llm_node::crypto::proof_content::{proof_content_commitment, ProofContentKey};
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

/// Test vectors handed to the SDK developer
/// (temp/ukh1/builder-a/np2-test-vectors.md). Computed here so they cannot
/// drift: change the derivation and this goes red.
const VECTORS: &[(&str, u64, &str, &str)] = &[
    (
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        1,
        "Hello",
        "1f011fc5b51fc39a9e8434c5b156b1b0ceef0812b5ebfce415c55914a464105c",
    ),
    (
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        2,
        "Hello",
        "5d2ac5d5af3029ec3f5925e6f76a97c0c9b8b63d0a70c4d663c284c11195a80c",
    ),
    (
        "c0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffee0001",
        1540,
        "<|im_start|>user\nWhat is a gaffer?<|im_end|>\n",
        "51bcca6d2a22db466acf0b38b805c10cda96ad84bee607c5599fbe8cc7115ef5",
    ),
];

fn key(hex32: &str) -> [u8; 32] {
    hex::decode(hex32).unwrap().try_into().unwrap()
}

#[test]
fn sdk_test_vectors_hold() {
    for (sk, job, content, expected) in VECTORS {
        let got = hex::encode(proof_content_commitment(&key(sk), *job, content.as_bytes()));
        println!("VECTOR session_key={sk} job_id={job} content={content:?} commitment={got}");
        assert_eq!(&got, expected, "vector for job {job}");
    }
}

#[test]
fn derivation_is_hkdf_then_hmac_with_the_stated_info() {
    use hkdf::Hkdf;
    use hmac::{Hmac, Mac};
    let sk = key(VECTORS[0].0);
    let hk = Hkdf::<Sha256>::new(None, &sk);
    let mut k = [0u8; 32];
    hk.expand(b"fabstir-proof-content-v1:77", &mut k).unwrap();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&k).unwrap();
    mac.update(b"prompt");
    let want: [u8; 32] = mac.finalize().into_bytes().into();
    assert_eq!(proof_content_commitment(&sk, 77, b"prompt"), want);
}

#[test]
fn commitment_is_not_plain_sha256() {
    let sk = key(VECTORS[0].0);
    assert_ne!(proof_content_commitment(&sk, 1, b"Hello"), sha(b"Hello"));
}

#[test]
fn two_jobs_get_different_keys() {
    let sk = key(VECTORS[0].0);
    assert_ne!(
        proof_content_commitment(&sk, 1, b"same"),
        proof_content_commitment(&sk, 2, b"same"),
        "same key for two job ids"
    );
    assert_ne!(
        ProofContentKey::from_session_key(&sk, 1).commit(b"x"),
        ProofContentKey::from_session_key(&sk, 2).commit(b"x")
    );
    assert_eq!(
        format!("{:?}", ProofContentKey::from_session_key(&sk, 1)),
        "ProofContentKey(<redacted>)"
    );
}

fn manager() -> ContentCommitments {
    ContentCommitments::default()
}

#[tokio::test]
async fn journal_fields_are_keyed_with_the_session_key() {
    let cm = manager();
    let sk = key(VECTORS[0].0);
    cm.set_prompt_content(9, "the prompt", Some(&sk)).await;
    cm.append_response(9, "the reply").await;
    let (p, r) = cm.get_content_hashes(9).await.expect("partial hashes");
    assert_ne!(
        p,
        sha(b"the prompt"),
        "journal input must not be SHA-256(prompt)"
    );
    assert_ne!(
        r,
        sha(b"the reply"),
        "journal output must not be SHA-256(response)"
    );
    assert_eq!(p, proof_content_commitment(&sk, 9, b"the prompt"));
    assert_eq!(r, proof_content_commitment(&sk, 9, b"the reply"));
    let fin = cm.finalize_response_hash(9).await;
    assert_eq!(fin, proof_content_commitment(&sk, 9, b"the reply"));
    assert_eq!(cm.get_content_hashes(9).await.unwrap(), (p, fin));
}

#[tokio::test]
async fn no_session_key_is_still_keyed_and_per_job() {
    let cm = manager();
    cm.set_prompt_content(1, "same prompt", None).await;
    cm.set_prompt_content(2, "same prompt", None).await;
    cm.append_response(1, "same reply").await;
    cm.append_response(2, "same reply").await;
    let (p1, r1) = cm.get_content_hashes(1).await.unwrap();
    let (p2, r2) = cm.get_content_hashes(2).await.unwrap();
    assert_ne!(
        p1,
        sha(b"same prompt"),
        "no-key path must not publish SHA-256(prompt)"
    );
    assert_ne!(
        r1,
        sha(b"same reply"),
        "no-key path must not publish SHA-256(response)"
    );
    assert_ne!(p1, p2, "random keys differ per job");
    assert_ne!(r1, r2);
    // stable within a job: a second prompt on job 1 uses the same random key
    cm.set_prompt_content(1, "same prompt", None).await;
    assert_eq!(cm.get_content_hashes(1).await.unwrap().0, p1);
    // a response finalised before any prompt is keyed too
    cm.append_response(3, "orphan").await;
    assert_ne!(cm.finalize_response_hash(3).await, sha(b"orphan"));
}

#[test]
fn server_never_hashes_the_prompt_plainly() {
    let src = include_str!("../../src/api/server.rs");
    assert!(
        !src.contains("Sha256::digest(full_prompt"),
        "plain prompt hash in server.rs"
    );
    assert!(
        !src.contains("set_prompt_hash("),
        "unkeyed setter still called"
    );
    assert_eq!(
        src.matches("cm.set_prompt_content(jid, &full_prompt,")
            .count(),
        2
    );
    let cm = include_str!("../../src/contracts/checkpoint_manager.rs");
    let prod = &cm[..cm.find("#[cfg(test)]\nmod tests").unwrap()];
    assert!(
        !prod.contains("Sha256::digest(entry.response_buffer"),
        "plain response hash"
    );
}

#[tokio::test]
async fn rekeying_a_job_recomputes_every_commitment_under_the_current_key() {
    let cm = manager();
    let a = key(VECTORS[0].0);
    let b = key(VECTORS[2].0);
    cm.set_prompt_content(5, "first prompt", Some(&a)).await;
    cm.append_response(5, "first reply").await;
    let fin_a = cm.finalize_response_hash(5).await;
    assert_eq!(fin_a, proof_content_commitment(&a, 5, b"first reply"));
    // the same job re-keyed (a second connection's session key)
    cm.set_prompt_content(5, "second prompt", Some(&b)).await;
    let (p, r) = cm.get_content_hashes(5).await.unwrap();
    assert_eq!(p, proof_content_commitment(&b, 5, b"second prompt"));
    assert_eq!(
        r,
        proof_content_commitment(&b, 5, b"first reply"),
        "a finalised response under the old key must not survive a re-key"
    );
    assert_eq!(cm.finalize_response_hash(5).await, r);
}

#[tokio::test]
async fn debug_never_shows_content_or_key() {
    let cm = manager();
    let sk = key(VECTORS[0].0);
    cm.set_prompt_content(8, "PROMPT-CANARY-77", Some(&sk))
        .await;
    cm.append_response(8, "REPLY-CANARY-88").await;
    let _ = cm.finalize_response_hash(8).await;
    let dbg = format!("{cm:?}");
    assert!(!dbg.contains("CANARY"), "{dbg}");
    let k = ProofContentKey::from_session_key(&sk, 8);
    let kd = format!("{k:?}");
    assert_eq!(kd, "ProofContentKey(<redacted>)");
    let entry_dbg = cm.debug_entry_for_test(8).await;
    assert!(!entry_dbg.contains("CANARY"), "{entry_dbg}");
    assert!(entry_dbg.contains("response_len"), "{entry_dbg}");
}

#[tokio::test]
async fn results_proof_generator_does_not_publish_plain_hashes() {
    use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
    use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
    let result = InferenceResult {
        job_id: "j1".into(),
        model_id: "m".into(),
        prompt: "secret prompt".into(),
        response: "secret reply".into(),
        tokens_generated: 2,
        inference_time_ms: 1,
        timestamp: chrono::Utc::now(),
        node_id: "n".into(),
        metadata: ResultMetadata::default(),
    };
    let generator = ProofGenerator::new(
        ProofGenerationConfig {
            proof_type: ProofType::Simple,
            model_path: "./models/m.gguf".into(),
            settings_path: None,
            max_proof_size: 10_000,
        },
        "n".into(),
    );
    let proof = generator.generate_proof(&result).await.unwrap();
    assert_ne!(proof.input_hash, hex::encode(sha(b"secret prompt")));
    assert_ne!(proof.output_hash, hex::encode(sha(b"secret reply")));
    assert!(generator.verify_proof(&proof, &result).await.unwrap());
    let mut other = result.clone();
    other.response = "another reply".into();
    assert!(!generator.verify_proof(&proof, &other).await.unwrap());
}
