// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! UKH1 U1 CUDA smoke: load a real GGUF with llama-cpp-2 0.1.161 on the GPU and
//! read a reply through the production engine. Both tests need the GPU and a
//! model file, so they are #[ignore]; run one at a time:
//!
//!   timeout 600 cargo test -j 4 --test ukh1_cuda_smoke -- --ignored --test-threads=1 tiny_vicuna
//!   timeout 900 cargo test -j 4 --test ukh1_cuda_smoke -- --ignored --test-threads=1 gpt_oss_20b
//!
//! tiny-vicuna (llama2 vocab, 0.67 GB) fits beside other GPU work; gpt-oss-20b
//! (harmony, 12 GB) needs about 13 GB of free VRAM.

use fabstir_llm_node::inference::{
    ChatTemplate, EngineConfig, InferenceRequest, LlmEngine, ModelConfig,
};
use std::path::PathBuf;

async fn reply(model: &str, template: ChatTemplate, gpu_layers: usize, question: &str) -> String {
    std::env::set_var("MODEL_CHAT_TEMPLATE", template.as_str());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("models")
        .join(model);
    assert!(path.exists(), "{} missing", path.display());
    let mut engine = LlmEngine::new(EngineConfig {
        models_directory: path.parent().unwrap().to_path_buf(),
        max_context_length: 2048,
        gpu_layers,
        batch_size: 512,
        ..EngineConfig::default()
    })
    .await
    .unwrap();
    let model_id = engine
        .load_model(ModelConfig {
            model_path: path,
            model_type: "gguf".into(),
            context_size: 2048,
            gpu_layers,
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
            chat_template: Some(template.clone()),
            encrypted: false,
        })
        .await
        .expect("model loads with 0.1.161");
    let prompt = template.format_messages(&[("user".to_string(), question.to_string())]);
    let result = engine
        .run_inference(InferenceRequest {
            model_id,
            prompt,
            max_tokens: 48,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 40,
            repeat_penalty: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            min_p: 0.0,
            seed: Some(1),
            stop_sequences: vec![],
            stream: false,
            cancel_flag: None,
            token_sender: None,
            result_sender: None,
        })
        .await
        .expect("inference");
    println!(
        "[{model}] {} tokens, finish={}: {:?}",
        result.tokens_generated, result.finish_reason, result.text
    );
    result.text
}

#[tokio::test]
#[ignore = "needs the GPU and models/tiny-vicuna-1b.q4_k_m.gguf"]
async fn tiny_vicuna_answers() {
    let text = reply(
        "tiny-vicuna-1b.q4_k_m.gguf",
        ChatTemplate::Vicuna,
        99,
        "What is the capital of France? Answer in one word.",
    )
    .await;
    assert!(text.to_lowercase().contains("paris"), "reply: {text:?}");
}

#[tokio::test]
#[ignore = "needs ~13 GB free VRAM and models/openai_gpt-oss-20b-MXFP4.gguf"]
async fn gpt_oss_20b_answers() {
    let text = reply(
        "openai_gpt-oss-20b-MXFP4.gguf",
        ChatTemplate::Harmony,
        99,
        "What is the capital of France? Answer in one word.",
    )
    .await;
    assert!(text.to_lowercase().contains("paris"), "reply: {text:?}");
    assert!(
        !text.contains("<|start|>"),
        "harmony markers must not leak: {text:?}"
    );
}
