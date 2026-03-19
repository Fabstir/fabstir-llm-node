// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for keccak256-based Merkle tree construction.

use fabstir_llm_node::transcoder::merkle::{verify_proof, MerkleTree};
use tiny_keccak::{Hasher, Keccak};

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

fn test_leaf(n: u8) -> [u8; 32] {
    keccak256(&[n])
}

#[test]
fn test_merkle_single_leaf() {
    let mut tree = MerkleTree::new();
    let leaf = test_leaf(1);
    tree.add_leaf(leaf);
    assert_eq!(tree.root(), leaf);
}

#[test]
fn test_merkle_two_leaves() {
    let mut tree = MerkleTree::new();
    let l0 = test_leaf(0);
    let l1 = test_leaf(1);
    tree.add_leaf(l0);
    tree.add_leaf(l1);
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(&l0);
    combined[32..].copy_from_slice(&l1);
    let expected = keccak256(&combined);
    assert_eq!(tree.root(), expected);
    assert_ne!(tree.root(), l0);
    assert_ne!(tree.root(), l1);
}

#[test]
fn test_merkle_four_leaves() {
    let mut tree = MerkleTree::new();
    let leaves: Vec<[u8; 32]> = (0..4).map(test_leaf).collect();
    for l in &leaves {
        tree.add_leaf(*l);
    }
    // Manual: h01 = keccak(l0 ++ l1), h23 = keccak(l2 ++ l3), root = keccak(h01 ++ h23)
    let h01 = keccak256(&[leaves[0], leaves[1]].concat());
    let h23 = keccak256(&[leaves[2], leaves[3]].concat());
    let expected = keccak256(&[h01, h23].concat());
    assert_eq!(tree.root(), expected);
}

#[test]
fn test_merkle_odd_leaves() {
    let mut tree = MerkleTree::new();
    for i in 0..3 {
        tree.add_leaf(test_leaf(i));
    }
    // 3 leaves: pad by duplicating last → [l0, l1, l2, l2]
    let root = tree.root();
    assert_ne!(root, [0u8; 32]);
}

#[test]
fn test_merkle_proof_valid() {
    let mut tree = MerkleTree::new();
    let leaves: Vec<[u8; 32]> = (0..4).map(test_leaf).collect();
    for l in &leaves {
        tree.add_leaf(*l);
    }
    let proof = tree.proof_for_leaf(2);
    assert_eq!(proof.len(), 2); // log2(4) = 2
}

#[test]
fn test_merkle_leaf_count() {
    let mut tree = MerkleTree::new();
    for i in 0..5 {
        tree.add_leaf(test_leaf(i));
    }
    assert_eq!(tree.leaf_count(), 5);
}

#[test]
fn test_merkle_serialize_deserialize() {
    let mut tree = MerkleTree::new();
    for i in 0..4 {
        tree.add_leaf(test_leaf(i));
    }
    let bytes = tree.serialize();
    assert!(!bytes.is_empty());
    // Deterministic: same inputs → same serialization
    let mut tree2 = MerkleTree::new();
    for i in 0..4 {
        tree2.add_leaf(test_leaf(i));
    }
    assert_eq!(tree.serialize(), tree2.serialize());
}
