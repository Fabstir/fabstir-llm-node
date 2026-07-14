// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! W2 — the engine inventory.
//!
//! [`crate::ltx::weights`] binds the weight FILES a pinned graph names. That is
//! worth nothing on its own: the graph pins `class_type` strings, not the code
//! behind them. ComfyUI is a general-purpose UI — 24 custom-node packs on the
//! reference host — and Python registers every pack it finds. A twenty-line node
//! that shadows `CheckpointLoaderSimple` loads whatever it likes while the pinned
//! graph still says `ltx-2.3-22b-dev-fp8.safetensors`, and it is far cheaper than
//! swapping 27 GB on disk.
//!
//! So the engine is hashed too, and folded into `envHash`.
//!
//! Two rules, both learned from the reference host rather than invented:
//!
//! 1. **Hash what can EXECUTE**, not the whole tree: `.py`, `.pyd`, `.so`, `.dll`,
//!    plus `extra_model_paths.yaml` (config, but config that decides which bytes a
//!    file name resolves to). A PNG cannot shadow a class. Two untracked
//!    screenshots in an assets folder were enough to flag a pack as "modified" on
//!    3XS-Z, and `engineHash` lives in the host's on-chain-anchored bundle, so
//!    noise costs gas.
//! 2. **Never hash bytecode.** `__pycache__` regenerates on every run; a naive
//!    tree hash would give every clip a different `envHash` and make honest drift
//!    indistinguishable from tampering.
//!
//! Host-declared, therefore cost-raising rather than absolute — the host owns the
//! box, and only the TEE path makes this non-forgeable. But omitting it leaves the
//! cheapest hole in the design wide open.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ethers::abi::{encode, Token};
use ethers::utils::keccak256;

/// Extensions Python can actually load. A shadowing loader must be one of these.
const EXECUTABLE: [&str; 4] = ["py", "pyd", "so", "dll"];

/// Config that redirects which bytes a file name resolves to — it can point
/// `checkpoints` at an entirely different tree, so it changes what runs.
const RESOLUTION_CONFIG: &str = "extra_model_paths.yaml";

fn is_executable(path: &Path) -> bool {
    // `.pyc` is bytecode, not source: it regenerates and must never move the hash.
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXECUTABLE.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_excluded_dir(name: &str) -> bool {
    name == "__pycache__" || name == ".git"
}

/// Every executable file under `dir`, relative to it, sorted — so the walk order
/// of the filesystem cannot change the hash.
fn executable_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue, // unreadable dir: not fatal, and not silently a match
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !is_excluded_dir(&name) {
                    stack.push(path);
                }
            } else if is_executable(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("0x{}", hex::encode(Sha256::digest(bytes)))
}

/// The hash of one custom-node pack: its executable files, path-sorted, content
/// hashed.
fn pack_hash(pack_dir: &Path) -> Result<String> {
    let mut tokens: Vec<Token> = Vec::new();
    for file in executable_files(pack_dir)? {
        let rel = file
            .strip_prefix(pack_dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/"); // a Windows host and a Linux host must agree
        let bytes = fs::read(&file)
            .with_context(|| format!("reading custom node file {}", file.display()))?;
        tokens.push(Token::String(rel));
        tokens.push(Token::String(sha256_hex(&bytes)));
    }
    Ok(format!("0x{}", hex::encode(keccak256(encode(&tokens)))))
}

/// `engineHash` over the ComfyUI installation at `comfy_root`: every custom-node
/// pack (name + content hash of its executable files), sorted by name, plus
/// `extra_model_paths.yaml` if present.
///
/// Covers ALL packs, not just the ones our classes come from: Python registers
/// every pack, so a shadowing class can be declared anywhere.
pub fn engine_hash(comfy_root: &Path) -> Result<String> {
    let mut packs: Vec<(String, String)> = Vec::new();

    let custom_nodes = comfy_root.join("custom_nodes");
    if custom_nodes.is_dir() {
        for entry in fs::read_dir(&custom_nodes)
            .with_context(|| format!("reading {}", custom_nodes.display()))?
            .flatten()
        {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if is_excluded_dir(&name) {
                continue;
            }
            packs.push((name, pack_hash(&path)?));
        }
    }
    packs.sort();

    let mut tokens: Vec<Token> = Vec::new();
    for (name, hash) in packs {
        tokens.push(Token::String(name));
        tokens.push(Token::String(hash));
    }

    let extra = comfy_root.join(RESOLUTION_CONFIG);
    if extra.is_file() {
        let bytes = fs::read(&extra).with_context(|| format!("reading {}", extra.display()))?;
        tokens.push(Token::String(RESOLUTION_CONFIG.to_string()));
        tokens.push(Token::String(sha256_hex(&bytes)));
    }

    Ok(format!("0x{}", hex::encode(keccak256(encode(&tokens)))))
}
