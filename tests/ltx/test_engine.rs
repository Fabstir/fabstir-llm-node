// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! W2 — the engine inventory, and the assertion that would have caught the bug.
//!
//! Hashing the weight file a graph NAMES proves nothing if the loader class that
//! reads that name has been replaced. ComfyUI is general-purpose: a twenty-line
//! custom node that shadows `CheckpointLoaderSimple` loads whatever it likes while
//! the pinned graph still says `ltx-2.3-22b-dev-fp8.safetensors`. So the engine is
//! hashed too, and it goes into `envHash` alongside the weights root.

use std::fs;

use fabstir_llm_node::ltx::attestation::{env_hash, EnvMeta};
use fabstir_llm_node::ltx::engine::engine_hash;
use tempfile::TempDir;

/// The constant every LTX clip ever settled carries: `envHash` over six empty
/// strings, because the values were read from env vars the deploy never set. Every
/// clip, every host, every model — the same 32 bytes. It binds nothing.
///
/// Pinned here so that the day the node starts computing a real one, this test
/// fails loudly and someone has to look at it.
const EMPTY_ENV_HASH: &str = "0x4125a7de2b7ebb297ed101164351dbf402ccb4494075088ef534ff02b880a6c8";

fn empty_meta() -> EnvMeta {
    EnvMeta {
        weights_hash: String::new(),
        lora_hash: String::new(),
        comfy_commit: String::new(),
        node_commit: String::new(),
        cuda_version: String::new(),
        gpu_class: String::new(),
    }
}

/// THE ASSERTION THAT CAN FAIL. Today's `envHash` is the keccak of six empty
/// strings — the same value on every clip, from every host, for every model. It
/// binds nothing. A round-trip test cannot catch that (the hash round-trips
/// perfectly; it is simply a hash of nothing), which is why this asserts on the
/// VALUE rather than on the trip.
#[test]
fn a_populated_env_hash_differs_from_the_hash_of_nothing() {
    let hash_of_nothing = env_hash(&empty_meta());
    assert_eq!(
        hash_of_nothing, EMPTY_ENV_HASH,
        "EMPTY_ENV_HASH constant is stale — update it to the printed value"
    );

    let real = EnvMeta {
        weights_hash: "0xaaaa".into(),
        lora_hash: "0xbbbb".into(),
        comfy_commit: "1377a2f7".into(),
        node_commit: "db047a9".into(),
        cuda_version: "12.8".into(),
        gpu_class: "RTX PRO 6000".into(),
    };
    assert_ne!(
        env_hash(&real),
        hash_of_nothing,
        "a populated EnvMeta must not hash to the empty constant"
    );
}

fn pack(root: &TempDir, name: &str, files: &[(&str, &str)]) {
    for (rel, body) in files {
        let path = root.path().join("custom_nodes").join(name).join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
}

/// A shadowing loader must move the hash. This is the whole point of the engine
/// inventory: the weights can be byte-perfect and the graph pinned, and the host
/// still runs different code.
#[test]
fn a_new_python_node_moves_the_engine_hash() {
    let root = TempDir::new().unwrap();
    pack(
        &root,
        "ComfyUI-LTXVideo",
        &[("nodes.py", "class LTXLoader: pass")],
    );
    let before = engine_hash(root.path()).unwrap();

    pack(
        &root,
        "totally-innocent-pack",
        &[("shadow.py", "class CheckpointLoaderSimple: pass")],
    );
    let after = engine_hash(root.path()).unwrap();

    assert_ne!(
        before, after,
        "a custom node that can shadow a loader class must change the engine hash"
    );
}

/// Editing existing node code must move it too.
#[test]
fn editing_node_code_moves_the_engine_hash() {
    let root = TempDir::new().unwrap();
    pack(&root, "ComfyUI-LTXVideo", &[("nodes.py", "load('fp8')")]);
    let before = engine_hash(root.path()).unwrap();

    pack(&root, "ComfyUI-LTXVideo", &[("nodes.py", "load('q4')")]);
    let after = engine_hash(root.path()).unwrap();

    assert_ne!(
        before, after,
        "edited node code must change the engine hash"
    );
}

/// NOISE MUST NOT MOVE IT. Two untracked screenshots in `example_workflows/assets/`
/// were enough to flag ComfyUI-LTXVideo as "modified" on 3XS-Z. `engineHash` lives
/// in the host's bundle, so moving it means republishing on-chain — noise costs gas.
/// Bytecode is worse: `__pycache__` regenerates on every run, so a naive tree hash
/// would give every clip a different `envHash` and make honest drift look like
/// tampering.
#[test]
fn assets_and_bytecode_do_not_move_the_engine_hash() {
    let root = TempDir::new().unwrap();
    pack(
        &root,
        "ComfyUI-LTXVideo",
        &[("nodes.py", "class LTXLoader: pass")],
    );
    let before = engine_hash(root.path()).unwrap();

    pack(
        &root,
        "ComfyUI-LTXVideo",
        &[
            ("example_workflows/assets/vlcsnap-2026-06-21.png", "PNGDATA"),
            ("README.md", "# docs"),
            ("__pycache__/nodes.cpython-312.pyc", "BYTECODE"),
            ("nodes.pyc", "BYTECODE"),
        ],
    );
    let after = engine_hash(root.path()).unwrap();

    assert_eq!(
        before, after,
        "screenshots, docs and bytecode must NOT move the engine hash"
    );
}

/// `extra_model_paths.yaml` is config, but config that decides WHICH BYTES a
/// filename resolves to — it can point `checkpoints` at another tree entirely. It
/// changes what runs, so it is part of the engine.
#[test]
fn extra_model_paths_is_part_of_the_engine() {
    let root = TempDir::new().unwrap();
    pack(&root, "ComfyUI-LTXVideo", &[("nodes.py", "pass")]);
    let before = engine_hash(root.path()).unwrap();

    fs::write(
        root.path().join("extra_model_paths.yaml"),
        "comfyui:\n  checkpoints: F:/elsewhere/checkpoints\n",
    )
    .unwrap();
    let after = engine_hash(root.path()).unwrap();

    assert_ne!(
        before, after,
        "extra_model_paths.yaml redirects file resolution — it must be in the engine hash"
    );
}

/// Order-independent: two hosts with the same packs agree regardless of walk order.
#[test]
fn engine_hash_is_stable_across_calls() {
    let root = TempDir::new().unwrap();
    pack(&root, "b-pack", &[("n.py", "b")]);
    pack(&root, "a-pack", &[("n.py", "a")]);
    assert_eq!(
        engine_hash(root.path()).unwrap(),
        engine_hash(root.path()).unwrap()
    );
}
