// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! W1 — weight-manifest derivation.
//!
//! The manifest is DERIVED by parsing the pinned graph, never hand-written: a
//! hand-written list can omit a file the graph actually loads, and a manifest
//! that omits a file cannot bind it. The parser therefore fails closed on any
//! weight-bearing input it does not recognise.

use fabstir_llm_node::ltx::template::TemplateStore;
use fabstir_llm_node::ltx::weights::{weight_files, weights_root, WeightEntry};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

fn files_of(id: &str) -> Vec<String> {
    let store = TemplateStore::new(DIR).expect("template store loads");
    let graph = store.graph(id).expect("template exists");
    weight_files(&graph).expect("manifest derives")
}

/// Golden: the exact weight set each pinned template loads. Locked so that a
/// template edit that adds, drops, or swaps a weight file cannot pass silently —
/// it is a provenance change and must be an intentional one.
#[test]
fn t2v_manifest_is_golden() {
    assert_eq!(
        files_of("ltx-t2v-hdr"),
        vec![
            "gemma-3-12b-it-abliterated_lora_rank64_bf16.safetensors",
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-dev-fp8.safetensors",
            "ltx-2.3-spatial-upscaler-x2-1.1.safetensors",
            "ltx_2.3_22b_distilled_1.1_lora_dynamic_fro09_avg_rank_111_bf16.safetensors",
        ]
    );
}

#[test]
fn iclora_manifest_is_golden() {
    assert_eq!(
        files_of("ltx-iclora-hdr"),
        vec![
            "gemma-3-12b-it-abliterated_lora_rank64_bf16.safetensors",
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-distilled-fp8.safetensors",
            "ltx-2.3-22b-ic-lora-union-control-ref0.5.safetensors",
            "moge_2_vitl_normal_fp16.safetensors",
        ]
    );
}

#[test]
fn outpaint_manifest_is_golden() {
    assert_eq!(
        files_of("ltx-outpaint-hdr"),
        vec![
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-dev-fp8.safetensors",
            "ltx-2.3-22b-distilled-lora-384.safetensors",
            "ltx-2.3-22b-ic-lora-outpaint.safetensors",
        ]
    );
}

/// Every template derives a non-empty manifest, and the union across the nine
/// is exactly the 13 weight files the host is required to hold. A template that
/// loads a file outside this set is a deployment change, not a silent one.
#[test]
fn all_nine_templates_derive_and_the_union_is_thirteen() {
    const IDS: [&str; 9] = [
        "ltx-ingredients-hdr",
        "ltx-upscale-hdr",
        "ltx-t2v-hdr",
        "ltx-i2v-hdr",
        "ltx-flf2v-hdr",
        "ltx-iclora-hdr",
        "ltx-outpaint-hdr",
        "ltx-edit-hdr",
        "ltx-restore-hdr",
    ];
    let mut union: Vec<String> = Vec::new();
    for id in IDS {
        let files = files_of(id);
        assert!(!files.is_empty(), "{id} derived an empty manifest");
        // The 22B transformer is the point of the whole exercise: every graph
        // must load one, and it must be one of the two pinned checkpoints.
        assert!(
            files.iter().any(|f| f == "ltx-2.3-22b-dev-fp8.safetensors"
                || f == "ltx-2.3-22b-distilled-fp8.safetensors"),
            "{id} loads no 22B checkpoint"
        );
        for f in files {
            if !union.contains(&f) {
                union.push(f);
            }
        }
    }
    union.sort();
    assert_eq!(union.len(), 13, "union changed: {union:#?}");
}

/// FAIL CLOSED — an unknown loader class carrying a weight file must raise, not
/// be skipped. A parser that silently ignores what it does not understand lets a
/// future template smuggle in an unhashed weight, which is the one thing the
/// manifest exists to prevent.
#[test]
fn unknown_loader_class_fails_closed() {
    let store = TemplateStore::new(DIR).expect("template store loads");
    let mut graph = store.graph("ltx-t2v-hdr").expect("template exists");
    graph.0["9999"] = serde_json::json!({
        "class_type": "SomeFutureLoader",
        "inputs": { "model": "a-sneaky-quant.gguf" }
    });
    let err = weight_files(&graph).expect_err("unknown weight loader must fail closed");
    assert!(
        err.to_string().contains("SomeFutureLoader"),
        "error must name the offending class, got: {err}"
    );
}

/// FAIL CLOSED — a NEW weight-bearing input key on a KNOWN loader class must also
/// raise. Reading only the keys we already know about would miss it.
#[test]
fn unknown_input_key_on_known_class_fails_closed() {
    let store = TemplateStore::new(DIR).expect("template store loads");
    let mut graph = store.graph("ltx-t2v-hdr").expect("template exists");
    graph.0["9998"] = serde_json::json!({
        "class_type": "CheckpointLoaderSimple",
        "inputs": {
            "ckpt_name": "ltx-2.3-22b-dev-fp8.safetensors",
            "second_ckpt": "a-sneaky-quant.safetensors"
        }
    });
    let err = weight_files(&graph).expect_err("unknown weight-bearing key must fail closed");
    assert!(
        err.to_string().contains("second_ckpt"),
        "error must name the offending key, got: {err}"
    );
}

/// The user's own inputs are NOT weights. LoadImage/LoadVideo name the buyer's
/// stills and control clip; those are bound by `inputCommitment`, and pulling
/// them into the weight manifest would make the root differ per job.
#[test]
fn input_loaders_are_not_weights() {
    let files = files_of("ltx-iclora-hdr");
    assert!(
        !files
            .iter()
            .any(|f| f.ends_with(".mp4") || f.ends_with(".png")),
        "input media leaked into the weight manifest: {files:?}"
    );
}

/// The root binds name, hash AND size, and is order-independent (entries are
/// canonically sorted) so two hosts holding the same weights agree.
#[test]
fn weights_root_is_order_independent_and_content_bound() {
    const H_A: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    const H_B: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";
    const H_Q4: &str = "0x9999999999999999999999999999999999999999999999999999999999999999";

    let a = WeightEntry::new("b.safetensors", H_B, 2);
    let b = WeightEntry::new("a.safetensors", H_A, 1);
    let root_ab = weights_root(&[a.clone(), b.clone()]).unwrap();
    let root_ba = weights_root(&[b.clone(), a]).unwrap();
    assert_eq!(root_ab, root_ba, "root must not depend on entry order");

    // A different hash under the SAME file name is a different weight set. This
    // is the whole point: swapping FP8 for a Q4 quant behind the same filename
    // must move the root.
    let swapped = WeightEntry::new("b.safetensors", H_Q4, 2);
    let root_swapped = weights_root(&[swapped, b]).unwrap();
    assert_ne!(
        root_ab, root_swapped,
        "a swapped weight file must change the root"
    );
}

/// A malformed hash must not reach the chain, where a garbage root is
/// indistinguishable from an honest one.
#[test]
fn weights_root_rejects_a_malformed_hash() {
    let bad = WeightEntry::new("a.safetensors", "0xdeadbeef", 1);
    assert!(
        weights_root(&[bad]).is_err(),
        "short sha256 must be refused"
    );
}

// ---------------------------------------------------------------------------
// W2 — resolution and hashing against a real directory.
// ---------------------------------------------------------------------------

use fabstir_llm_node::ltx::weights::{resolve_and_hash, WeightCache};
use std::fs;
use tempfile::TempDir;

fn models_dir(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (rel, body) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

/// Resolution is by BASENAME across the tree: ComfyUI's class-to-folder mapping is
/// per-host configurable, so hard-coding it here would be a second source of truth.
#[test]
fn resolve_and_hash_finds_files_anywhere_in_the_tree() {
    let dir = models_dir(&[
        ("checkpoints/ltx-2.3-22b-dev-fp8.safetensors", "FP8-BYTES"),
        ("loras/some-lora.safetensors", "LORA-BYTES"),
    ]);
    let mut cache = WeightCache::default();
    let entries = resolve_and_hash(
        dir.path(),
        &[
            "ltx-2.3-22b-dev-fp8.safetensors".to_string(),
            "some-lora.safetensors".to_string(),
        ],
        &mut cache,
    )
    .expect("resolves");

    assert_eq!(entries.len(), 2);
    // SHA-256 of the literal bytes, not of the name.
    assert_eq!(entries[0].size, "FP8-BYTES".len() as u64);
    assert!(entries[0].sha256.starts_with("0x") && entries[0].sha256.len() == 66);
}

/// FAIL CLOSED — a host cannot attest to weights it does not hold.
#[test]
fn a_missing_weight_fails_closed() {
    let dir = models_dir(&[("checkpoints/present.safetensors", "X")]);
    let mut cache = WeightCache::default();
    let err = resolve_and_hash(dir.path(), &["absent.safetensors".to_string()], &mut cache)
        .expect_err("a missing weight must fail closed");
    assert!(err.to_string().contains("absent.safetensors"));
}

/// FAIL CLOSED — which of two same-named files ComfyUI would open is not ours to
/// guess, and guessing is exactly what this module exists to eliminate.
#[test]
fn an_ambiguous_basename_fails_closed() {
    let dir = models_dir(&[
        ("checkpoints/dupe.safetensors", "ONE"),
        ("unet/dupe.safetensors", "TWO"),
    ]);
    let mut cache = WeightCache::default();
    let err = resolve_and_hash(dir.path(), &["dupe.safetensors".to_string()], &mut cache)
        .expect_err("an ambiguous basename must fail closed");
    assert!(err.to_string().contains("AMBIGUOUS"), "got: {err}");
}

/// THE ALARM. Swap the bytes behind the file name — the Q4-for-FP8 substitution
/// this whole phase exists to catch — and the root must move.
#[test]
fn swapping_the_bytes_behind_the_name_moves_the_root() {
    let dir = models_dir(&[("checkpoints/ltx-2.3-22b-dev-fp8.safetensors", "HONEST-FP8")]);
    let want = vec!["ltx-2.3-22b-dev-fp8.safetensors".to_string()];

    let mut cache = WeightCache::default();
    let honest = weights_root(&resolve_and_hash(dir.path(), &want, &mut cache).unwrap()).unwrap();

    // Same file name, same graph, same everything the buyer can see. Different bytes.
    fs::write(
        dir.path()
            .join("checkpoints/ltx-2.3-22b-dev-fp8.safetensors"),
        "SNEAKY-Q4-QUANT",
    )
    .unwrap();

    let mut cache2 = WeightCache::default();
    let cheat = weights_root(&resolve_and_hash(dir.path(), &want, &mut cache2).unwrap()).unwrap();

    assert_ne!(
        honest, cheat,
        "a swapped checkpoint must move weightsRoot — this is the entire point"
    );
}

/// The upscale template loads only files already in the pinned twelve — a new MODE
/// with no new weights, which is what lets it ride bundle v9 without a licence pass.
#[test]
fn upscale_manifest_is_golden() {
    assert_eq!(
        files_of("ltx-upscale-hdr"),
        vec![
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-dev-fp8.safetensors",
            "ltx-2.3-spatial-upscaler-x2-1.1.safetensors",
            "ltx_2.3_22b_distilled_1.1_lora_dynamic_fro09_avg_rank_111_bf16.safetensors",
        ]
    );
}

/// I-phase: the gated Lightricks LoRA is the ONE new weight this mode brings
/// (sha256 515E4E13… in the provenance ledger; licence-gated download per host).
#[test]
fn ingredients_manifest_is_golden() {
    assert_eq!(
        files_of("ltx-ingredients-hdr"),
        vec![
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-dev-fp8.safetensors",
            "ltx-2.3-22b-distilled-lora-384.safetensors",
            "ltx-2.3-22b-ic-lora-ingredients-0.9.safetensors",
        ]
    );
}
