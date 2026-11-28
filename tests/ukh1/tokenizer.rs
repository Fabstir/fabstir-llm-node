// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! U1: 0.1.154's `str_to_token` always parsed special tokens; the 0.1.161
//! call must too, or `<|im_start|>` becomes plain text (fluent garbage).
//! Vocab-only load of the Qwen3.8 Flash Next vocabulary; no weights. Shard 1
//! alone cannot be loaded even vocab-only (llama.cpp's split loader opens all
//! four shards first), so the test uses a single-file copy of shard 1's
//! metadata with the split.* keys removed and zero tensors, made by
//! `temp/ukh1/builder-a/make_vocab_only_gguf.py` (same 64 metadata pairs,
//! same vocabulary).

use fabstir_llm_node::inference::engine::{render_token_like_0_1_154, tokenize_like_0_1_154};
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use std::path::PathBuf;
use std::sync::OnceLock;

const VOCAB: &str = "temp/ukh1/builder-a/qwen38-flash-next-vocab-only.gguf";

fn chatml_vocab() -> &'static LlamaModel {
    static MODEL: OnceLock<(LlamaBackend, LlamaModel)> = OnceLock::new();
    &MODEL
        .get_or_init(|| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(VOCAB);
            let backend = LlamaBackend::init().expect("backend");
            let params = LlamaModelParams::default().with_vocab_only(true);
            let model = LlamaModel::load_from_file(&backend, &path, &params)
                .expect("vocab-only load of the ChatML shard");
            (backend, model)
        })
        .1
}

#[test]
#[ignore = "needs temp/ukh1/builder-a/qwen38-flash-next-vocab-only.gguf (not in CI)"]
fn im_start_tokenizes_to_one_special_token() {
    let model = chatml_vocab();
    let toks = tokenize_like_0_1_154(model, "<|im_start|>", false);
    assert_eq!(
        toks.len(),
        1,
        "<|im_start|> must be ONE special token, got {toks:?}"
    );
    // and it renders back as the marker (Special::Tokenize semantics)
    let bytes = render_token_like_0_1_154(model, toks[0]).expect("renders");
    assert_eq!(bytes, b"<|im_start|>");
}

#[test]
#[ignore = "needs temp/ukh1/builder-a/qwen38-flash-next-vocab-only.gguf (not in CI)"]
fn chatml_prompt_keeps_markers_as_single_tokens() {
    let model = chatml_vocab();
    let prompt = "<|im_start|>user\nHi<|im_end|>\n<|im_start|>assistant\n";
    let toks = tokenize_like_0_1_154(model, prompt, false);
    let start = tokenize_like_0_1_154(model, "<|im_start|>", false)[0];
    let end = tokenize_like_0_1_154(model, "<|im_end|>", false)[0];
    assert_eq!(toks.iter().filter(|t| **t == start).count(), 2);
    assert_eq!(toks.iter().filter(|t| **t == end).count(), 1);
    // plain text tokenises to more than one token for the same characters
    let plain = model.vocab().tokenize(b"<|im_start|>", false, false);
    assert!(
        plain.len() > 1,
        "parse_special=false splits the marker: {plain:?}"
    );
}
