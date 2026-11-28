// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! U2: LLAMA_CPU_MOE_LAYERS, LLAMA_USE_MMAP, LLAMA_THREADS(_BATCH),
//! ROPE_SCALING/YARN_ORIG_CTX, LLAMA_TOP_K. Unset = today, proven per setting.

use fabstir_llm_node::inference::llama_settings::{cpu_moe_pattern, LlamaTuning};
use llama_cpp_2::context::params::{LlamaContextParams, RopeScalingType};
use llama_cpp_2::model::params::LlamaModelParams;
use std::collections::HashMap;

fn tuning(pairs: &[(&str, &str)]) -> anyhow::Result<LlamaTuning> {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    LlamaTuning::from_lookup(|k| m.get(k).cloned())
}

/// llama.cpp matches with an unanchored ECMAScript `std::regex_search`; the
/// `regex` crate's `is_match` is the same unanchored search for this syntax.
fn on_cpu(pattern: &str, tensor: &str) -> bool {
    regex::Regex::new(pattern).unwrap().is_match(tensor)
}

#[test]
fn unset_env_is_todays_behaviour() {
    let t = tuning(&[]).unwrap();
    assert_eq!(t, LlamaTuning::default());
    assert_eq!(t.cpu_moe_override(), None, "unset adds no tensor override");
    assert_eq!(
        t.use_mmap_override(),
        None,
        "unset must not call with_use_mmap"
    );
    assert!(t.top_k_sampler().is_none(), "unset builds no top_k sampler");
    // empty values count as unset
    let e = tuning(&[
        ("LLAMA_CPU_MOE_LAYERS", ""),
        ("LLAMA_USE_MMAP", " "),
        ("LLAMA_THREADS", ""),
        ("ROPE_SCALING", ""),
        ("LLAMA_TOP_K", ""),
        ("LLAMA_THREADS_BATCH", ""),
        ("YARN_ORIG_CTX", ""),
    ])
    .unwrap();
    assert_eq!(e, LlamaTuning::default());
}

#[test]
fn moe_pattern_exact_text() {
    assert_eq!(cpu_moe_pattern(0), None);
    assert_eq!(
        cpu_moe_pattern(1).unwrap(),
        r"blk\.(0)\.ffn_(up|down|gate)_(ch|)exps"
    );
    assert_eq!(
        cpu_moe_pattern(3).unwrap(),
        r"blk\.(0|1|2)\.ffn_(up|down|gate)_(ch|)exps"
    );
    assert_eq!(
        tuning(&[("LLAMA_CPU_MOE_LAYERS", "0")])
            .unwrap()
            .cpu_moe_override(),
        None
    );
}

#[test]
fn moe_pattern_matches_exactly_layers_0_to_n_minus_1() {
    let n = 10;
    let p = cpu_moe_pattern(n).unwrap();
    for layer in 0..48u32 {
        for t in [
            "ffn_up_exps",
            "ffn_down_exps",
            "ffn_gate_exps",
            "ffn_up_chexps",
        ] {
            let name = format!("blk.{layer}.{t}.weight");
            assert_eq!(on_cpu(&p, &name), layer < n, "{name} with N={n}");
        }
        // shared experts and attention stay on the GPU
        for t in ["ffn_up_shexp", "ffn_gate_inp", "attn_q", "ffn_down_shexp"] {
            let name = format!("blk.{layer}.{t}.weight");
            assert!(!on_cpu(&p, &name), "{name} must not move");
        }
    }
}

#[test]
fn moe_pattern_blk1_does_not_match_blk10_to_19() {
    let p = cpu_moe_pattern(2).unwrap();
    assert!(on_cpu(&p, "blk.1.ffn_up_exps.weight"));
    for layer in 10..20 {
        let name = format!("blk.{layer}.ffn_up_exps.weight");
        assert!(!on_cpu(&p, &name), "{name} must not match N=2");
    }
    assert!(!on_cpu(&p, "blk.21.ffn_down_exps.weight"));
}

#[test]
fn moe_override_reaches_the_pinned_params() {
    let t = tuning(&[("LLAMA_CPU_MOE_LAYERS", "4")]).unwrap();
    let pat = t.cpu_moe_override().unwrap();
    let params = t.model_params(
        LlamaModelParams::default().with_n_gpu_layers(99),
        Some(pat.as_c_str()),
    );
    assert_eq!(
        params.n_gpu_layers(),
        99,
        "gpu layers applied before pinning"
    );
    assert_eq!(
        params.tensor_buft_override_patterns(),
        vec![r"blk\.(0|1|2|3)\.ffn_(up|down|gate)_(ch|)exps".to_string()]
    );
    drop(params);
    drop(pat);

    let none = tuning(&[]).unwrap();
    let params = none.model_params(
        LlamaModelParams::default(),
        none.cpu_moe_override().as_deref(),
    );
    assert!(params.tensor_buft_override_patterns().is_empty());
}

#[test]
fn use_mmap_only_false_changes_anything() {
    let f = tuning(&[("LLAMA_USE_MMAP", "false")]).unwrap();
    assert_eq!(f.use_mmap_override(), Some(false));
    let params = f.model_params(LlamaModelParams::default(), None);
    assert!(
        !params.use_mmap(),
        "LLAMA_USE_MMAP=false must reach the params"
    );
    let tr = tuning(&[("LLAMA_USE_MMAP", "TRUE")]).unwrap();
    assert_eq!(
        tr.use_mmap_override(),
        None,
        "true keeps llama.cpp's default"
    );
    assert!(
        tuning(&[("LLAMA_USE_MMAP", "0")]).is_err(),
        "only true/false accepted"
    );
}

#[test]
fn threads_applied_only_when_set() {
    let base = LlamaContextParams::default();
    let (d_t, d_b) = (base.n_threads(), base.n_threads_batch());
    let same = tuning(&[])
        .unwrap()
        .apply_to_context(LlamaContextParams::default());
    assert_eq!((same.n_threads(), same.n_threads_batch()), (d_t, d_b));
    let t = tuning(&[("LLAMA_THREADS", "12"), ("LLAMA_THREADS_BATCH", "16")]).unwrap();
    let p = t.apply_to_context(LlamaContextParams::default());
    assert_eq!(p.n_threads(), 12);
    assert_eq!(p.n_threads_batch(), 16);
    assert!(tuning(&[("LLAMA_THREADS", "0")]).is_err());
    assert!(tuning(&[("LLAMA_THREADS", "many")]).is_err());
}

#[test]
fn yarn_off_unless_set() {
    let base = LlamaContextParams::default();
    let same = tuning(&[])
        .unwrap()
        .apply_to_context(LlamaContextParams::default());
    assert_eq!(same.rope_scaling_type(), base.rope_scaling_type());
    assert_eq!(same.yarn_orig_ctx(), base.yarn_orig_ctx());
    assert_ne!(base.rope_scaling_type(), RopeScalingType::Yarn);

    let t = tuning(&[("ROPE_SCALING", "yarn"), ("YARN_ORIG_CTX", "262144")]).unwrap();
    let p = t.apply_to_context(LlamaContextParams::default());
    assert_eq!(p.rope_scaling_type(), RopeScalingType::Yarn);
    assert_eq!(p.yarn_orig_ctx(), 262144);
    assert!(tuning(&[("ROPE_SCALING", "linear")]).is_err());
}

#[test]
fn top_k_sampler_built_only_when_set() {
    assert!(tuning(&[]).unwrap().top_k_sampler().is_none());
    let t = tuning(&[("LLAMA_TOP_K", "20")]).unwrap();
    assert_eq!(t.top_k, Some(20));
    assert!(
        t.top_k_sampler().is_some(),
        "LLAMA_TOP_K=20 must build a sampler"
    );
    assert!(tuning(&[("LLAMA_TOP_K", "0")]).is_err());
}

#[test]
fn engine_places_top_k_before_top_p_and_ignores_request_top_k() {
    let src = include_str!("../../src/inference/engine.rs");
    let prod = &src[..src.find("#[cfg(test)]").unwrap()];
    let k = prod
        .find("self.tuning.top_k_sampler()")
        .expect("top_k wired");
    let p = prod.find("LlamaSampler::top_p(").expect("top_p");
    assert!(k < p, "top_k before top_p");
    assert!(
        !prod.contains("LlamaSampler::top_k(request"),
        "request.top_k unused"
    );
    assert!(prod.contains("self.tuning.apply_to_context(ctx_params)"));
    assert!(prod.contains("self.tuning.model_params("));
}
