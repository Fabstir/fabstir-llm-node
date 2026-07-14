// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! W1 — weight-manifest derivation for a pinned graph.
//!
//! The pinned template proves *which graph* ran. It does not prove *which
//! weights* ran: the graph names its checkpoints and LoRAs by FILE NAME, and a
//! host is free to put different bytes behind that name. Billing is
//! `frames * w * h`, not compute time, so a host that swaps `-dev-fp8` for a Q4
//! quant renders far faster, bills identically, and delivers a worse clip — and
//! every attestation still verifies. The weight manifest is what closes that.
//!
//! The manifest is DERIVED here by parsing the graph, never hand-written: a
//! hand-written list can omit a file the graph actually loads, and an omitted
//! file cannot be bound. Consequently the parser FAILS CLOSED — any input that
//! names a weight file through a class or key this module does not know is an
//! error, not a skip. A parser that silently ignores what it does not understand
//! is exactly the hole the manifest exists to close.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ethers::abi::{encode, Token};
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};

use crate::ltx::template::Graph;

/// Suffixes that make a string input a *weight file*. The buyer's own inputs
/// (`.png`, `.mp4`) are deliberately absent: those are bound by
/// `inputCommitment`, and folding them in here would make the root differ per
/// job instead of per host installation.
const WEIGHT_EXTENSIONS: [&str; 6] = [".safetensors", ".gguf", ".ckpt", ".sft", ".pt", ".pth"];

/// The loader classes the seven pinned templates use, and the input keys through
/// which each names a weight file. Adding a template that loads weights any other
/// way is a deliberate act, and must land here alongside it.
const KNOWN_LOADERS: &[(&str, &[&str])] = &[
    ("CheckpointLoaderSimple", &["ckpt_name"]),
    ("LTXAVTextEncoderLoader", &["ckpt_name", "text_encoder"]),
    ("LTXICLoRALoaderModelOnly", &["lora_name"]),
    ("LTXVAudioVAELoader", &["ckpt_name"]),
    ("LatentUpscaleModelLoader", &["model_name"]),
    ("LoadMoGeModel", &["model_name"]),
    ("LoraLoader", &["lora_name"]),
    ("LoraLoaderModelOnly", &["lora_name"]),
];

/// One weight file, as the host holds it: the name the graph loads it by, the
/// SHA-256 of its bytes, and its size. Size is not redundant — it makes a
/// truncated or partially-downloaded file a mismatch rather than a puzzle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeightEntry {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

impl WeightEntry {
    pub fn new(name: impl Into<String>, sha256: impl Into<String>, size: u64) -> Self {
        Self {
            name: name.into(),
            sha256: sha256.into(),
            size,
        }
    }
}

fn is_weight_file(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    WEIGHT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

fn known_keys(class_type: &str) -> Option<&'static [&'static str]> {
    KNOWN_LOADERS
        .iter()
        .find(|(class, _)| *class == class_type)
        .map(|(_, keys)| *keys)
}

/// Every weight file the pinned graph loads: sorted, de-duplicated (one file may
/// be loaded by several nodes — the LTX AV checkpoint carries the transformer,
/// the audio VAE and the text-encoder base, so three loaders name it).
///
/// Fails closed on any weight-bearing input from an unknown class or key.
pub fn weight_files(graph: &Graph) -> Result<Vec<String>> {
    let nodes = graph
        .0
        .as_object()
        .ok_or_else(|| anyhow!("graph is not a JSON object"))?;

    let mut files: Vec<String> = Vec::new();
    for (node_id, node) in nodes {
        let class_type = node
            .get("class_type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let Some(inputs) = node.get("inputs").and_then(|v| v.as_object()) else {
            continue;
        };

        for (key, value) in inputs {
            let Some(text) = value.as_str() else { continue };
            if !is_weight_file(text) {
                continue;
            }
            // This input names a weight file. It is bound, or the parse fails.
            let keys = known_keys(class_type).ok_or_else(|| {
                anyhow!(
                    "node {node_id}: unknown weight loader class {class_type:?} names weight file \
                     {text:?} — add it to KNOWN_LOADERS (fail-closed: an unbound weight would let \
                     a host swap the model without moving the weightsRoot)"
                )
            })?;
            if !keys.contains(&key.as_str()) {
                return Err(anyhow!(
                    "node {node_id}: class {class_type:?} names weight file {text:?} through \
                     unknown input key {key:?} — add it to KNOWN_LOADERS (fail-closed)"
                ));
            }
            if !files.iter().any(|f| f == text) {
                files.push(text.to_string());
            }
        }
    }

    files.sort();
    Ok(files)
}

/// Resolve every file in `files` under `models_dir` and hash it.
///
/// **Resolution is by BASENAME, deliberately.** ComfyUI's class→folder mapping
/// (`checkpoints/`, `loras/`, `text_encoders/`, …) is configurable per host via
/// `extra_model_paths.yaml`; encoding it here would be a second source of truth
/// that drifts out of agreement with the thing it describes. Walk, index, and
/// fail closed on:
///
/// - a **missing** file — the host cannot attest to weights it does not hold;
/// - an **ambiguous** basename (the same name in two directories) — guessing which
///   one ComfyUI would open is exactly the kind of assumption this module exists
///   to eliminate.
///
/// Hashing is cached by `(path, size, mtime)`: 78 GB on the reference host, ~80
/// seconds once. A file whose bytes moved re-hashes, and that re-hash is not a
/// cost — it is the alarm.
pub fn resolve_and_hash(
    models_dir: &Path,
    files: &[String],
    cache: &mut WeightCache,
) -> Result<Vec<WeightEntry>> {
    let index = index_by_basename(models_dir)?;

    let mut entries = Vec::with_capacity(files.len());
    for name in files {
        let found = index.get(name.as_str()).ok_or_else(|| {
            anyhow!(
                "weight {name:?} not found under {} — the host cannot attest to weights it does \
                 not hold (fail-closed)",
                models_dir.display()
            )
        })?;
        if found.len() > 1 {
            let paths: Vec<String> = found.iter().map(|p| p.display().to_string()).collect();
            return Err(anyhow!(
                "weight {name:?} is AMBIGUOUS under {} — {} copies: {paths:?}. Which one ComfyUI \
                 opens is not ours to guess (fail-closed)",
                models_dir.display(),
                found.len()
            ));
        }
        entries.push(cache.entry_for(name, &found[0])?);
    }
    Ok(entries)
}

fn index_by_basename(dir: &Path) -> Result<HashMap<String, Vec<PathBuf>>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("reading {}", current.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                index
                    .entry(name.to_string())
                    .or_default()
                    .push(path.clone());
            }
        }
    }
    Ok(index)
}

/// SHA-256 cache keyed by `(path, size, mtime)`, persisted so that a container
/// restart does not re-read 78 GB. A changed file misses the cache and re-hashes,
/// which is the behaviour we want: that is the event worth noticing.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WeightCache {
    entries: HashMap<String, CachedHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedHash {
    size: u64,
    mtime: i64,
    sha256: String,
}

impl WeightCache {
    /// Load from `path`, or start empty. A corrupt cache is not fatal: it is an
    /// optimisation, and the honest fallback is to re-hash.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing weight cache {}", path.display()))?;
        Ok(())
    }

    fn entry_for(&mut self, name: &str, path: &Path) -> Result<WeightEntry> {
        let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let key = path.display().to_string();
        if let Some(hit) = self.entries.get(&key) {
            if hit.size == size && hit.mtime == mtime {
                return Ok(WeightEntry::new(name, hit.sha256.clone(), size));
            }
        }

        let sha256 = sha256_file(path)?;
        self.entries.insert(
            key,
            CachedHash {
                size,
                mtime,
                sha256: sha256.clone(),
            },
        );
        Ok(WeightEntry::new(name, sha256, size))
    }
}

/// Streamed, so a 27 GB checkpoint does not land in RAM.
fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let file =
        std::fs::File::open(path).with_context(|| format!("opening weight {}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)
        .with_context(|| format!("hashing weight {}", path.display()))?;
    Ok(format!("0x{}", hex::encode(hasher.finalize())))
}

/// `weightsRoot = keccak256(abi.encode([(name, sha256, size), …]))` over the
/// canonically sorted entries — the one bytes32 that goes on-chain as the model's
/// `sha256Hash` and into the bundle.
///
/// abi.encode is length-prefixed, so no two field-boundary splittings collide:
/// any change to a name, a hash or a size moves the root. Order-independent by
/// construction, so two honest hosts holding the same weights agree.
pub fn weights_root(entries: &[WeightEntry]) -> Result<String> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut tokens: Vec<Token> = Vec::with_capacity(sorted.len() * 3);
    for e in &sorted {
        // Reject a malformed hash here rather than let a garbage root reach the
        // chain, where it would be indistinguishable from an honest one.
        let raw = e.sha256.strip_prefix("0x").unwrap_or(&e.sha256);
        let bytes = hex::decode(raw)
            .map_err(|err| anyhow!("weight {:?}: invalid sha256 {:?}: {err}", e.name, e.sha256))?;
        if bytes.len() != 32 {
            return Err(anyhow!(
                "weight {:?}: sha256 must be 32 bytes, got {}",
                e.name,
                bytes.len()
            ));
        }
        tokens.push(Token::String(e.name.clone()));
        tokens.push(Token::FixedBytes(bytes));
        tokens.push(Token::Uint(e.size.into()));
    }

    Ok(format!("0x{}", hex::encode(keccak256(encode(&tokens)))))
}
