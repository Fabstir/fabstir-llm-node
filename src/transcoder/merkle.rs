// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Keccak256-based Merkle tree for GOP proof aggregation.

use serde_json::json;
use tiny_keccak::{Hasher, Keccak};

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

/// A Merkle tree built from GOP proof hashes.
pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
}

impl MerkleTree {
    pub fn new() -> Self {
        Self { leaves: Vec::new() }
    }

    pub fn add_leaf(&mut self, hash: [u8; 32]) {
        self.leaves.push(hash);
    }

    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// Compute the Merkle root from leaves, bottom-up.
    pub fn root(&self) -> [u8; 32] {
        if self.leaves.is_empty() {
            return [0u8; 32];
        }
        let mut layer: Vec<[u8; 32]> = self.leaves.clone();
        while layer.len() > 1 {
            // Pad odd count by duplicating last node
            if layer.len() % 2 != 0 {
                layer.push(*layer.last().unwrap());
            }
            layer = layer
                .chunks(2)
                .map(|pair| {
                    let mut combined = [0u8; 64];
                    combined[..32].copy_from_slice(&pair[0]);
                    combined[32..].copy_from_slice(&pair[1]);
                    keccak256(&combined)
                })
                .collect();
        }
        layer[0]
    }

    /// Return sibling hashes from leaf to root (Merkle path).
    pub fn proof_for_leaf(&self, index: usize) -> Vec<[u8; 32]> {
        if index >= self.leaves.len() {
            return Vec::new();
        }
        let mut proof = Vec::new();
        let mut layer: Vec<[u8; 32]> = self.leaves.clone();
        let mut idx = index;
        while layer.len() > 1 {
            if layer.len() % 2 != 0 {
                layer.push(*layer.last().unwrap());
            }
            let sibling = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            proof.push(layer[sibling]);
            // Build next layer
            layer = layer
                .chunks(2)
                .map(|pair| {
                    let mut combined = [0u8; 64];
                    combined[..32].copy_from_slice(&pair[0]);
                    combined[32..].copy_from_slice(&pair[1]);
                    keccak256(&combined)
                })
                .collect();
            idx /= 2;
        }
        proof
    }

    /// Serialize tree as JSON bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let leaves_hex: Vec<String> = self.leaves.iter().map(hex::encode).collect();
        let root_hex = hex::encode(self.root());
        let val = json!({ "leaves": leaves_hex, "root": root_hex });
        serde_json::to_vec(&val).unwrap_or_default()
    }
}

/// Verify a Merkle proof for a given leaf.
pub fn verify_proof(leaf: [u8; 32], proof: &[[u8; 32]], index: usize, root: [u8; 32]) -> bool {
    let mut hash = leaf;
    let mut idx = index;
    for sibling in proof {
        let mut combined = [0u8; 64];
        if idx % 2 == 0 {
            combined[..32].copy_from_slice(&hash);
            combined[32..].copy_from_slice(sibling);
        } else {
            combined[..32].copy_from_slice(sibling);
            combined[32..].copy_from_slice(&hash);
        }
        hash = keccak256(&combined);
        idx /= 2;
    }
    hash == root
}
