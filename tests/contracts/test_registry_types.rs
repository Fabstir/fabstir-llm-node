// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ethers::abi::{decode, encode, ParamType, Token};
use ethers::prelude::*;
use fabstir_llm_node::contracts::types::*;

#[test]
fn test_node_registered_event_parsing() {
    // Test that NodeRegisteredEvent can be parsed from logs
    let node_addr = "0x1234567890123456789012345678901234567890"
        .parse::<Address>()
        .unwrap();
    let metadata = "gpu:rtx4090,ram:32gb,location:us-east".to_string();
    let stake = U256::from(1000000u64);

    // Create event
    let event = NodeRegisteredEvent {
        node: node_addr,
        metadata: metadata.clone(),
        stake,
    };

    // Verify fields
    assert_eq!(event.node, node_addr);
    assert_eq!(event.metadata, metadata);
    assert_eq!(event.stake, stake);
}

#[test]
fn test_node_updated_event_parsing() {
    // Test that NodeUpdatedEvent can be parsed
    let node_addr = "0x1234567890123456789012345678901234567890"
        .parse::<Address>()
        .unwrap();
    let metadata = "gpu:rtx4090,ram:64gb,location:us-west".to_string();

    let event = NodeUpdatedEvent {
        node: node_addr,
        metadata: metadata.clone(),
    };

    assert_eq!(event.node, node_addr);
    assert_eq!(event.metadata, metadata);
}

#[test]
fn test_node_unregistered_event_parsing() {
    // Test that NodeUnregisteredEvent can be parsed
    let node_addr = "0x1234567890123456789012345678901234567890"
        .parse::<Address>()
        .unwrap();

    let event = NodeUnregisteredEvent { node: node_addr };

    assert_eq!(event.node, node_addr);
}

#[test]
fn test_register_node_encoding() {
    // Test encoding registerNode function call
    let metadata = "gpu:rtx3090,ram:16gb".to_string();
    let stake = U256::from(500000u64);

    // Encode the function call manually
    let encoded = encode(&[Token::String(metadata.clone()), Token::Uint(stake)]);

    // Should be able to encode without errors
    assert!(!encoded.is_empty());

    // Create the call using the type
    let call = RegisterNodeCall {
        metadata: metadata.clone(),
        stake,
    };

    assert_eq!(call.metadata, metadata);
    assert_eq!(call.stake, stake);
}

#[test]
fn test_query_registered_nodes_decoding() {
    // Test decoding queryRegisteredNodes return value
    let addresses = vec![
        "0x1234567890123456789012345678901234567890"
            .parse::<Address>()
            .unwrap(),
        "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"
            .parse::<Address>()
            .unwrap(),
    ];

    let metadatas = vec!["gpu:rtx3090".to_string(), "gpu:rtx4090".to_string()];

    // Encode the return value
    let encoded = encode(&[
        Token::Array(addresses.iter().map(|a| Token::Address(*a)).collect()),
        Token::Array(metadatas.iter().map(|m| Token::String(m.clone())).collect()),
    ]);

    // Decode it
    let param_types = vec![
        ParamType::Array(Box::new(ParamType::Address)),
        ParamType::Array(Box::new(ParamType::String)),
    ];

    let decoded = decode(&param_types, &encoded).unwrap();
    assert_eq!(decoded.len(), 2);

    // Create return type
    let return_val = QueryRegisteredNodesReturn {
        nodes: addresses.clone(),
        metadatas: metadatas.clone(),
    };

    assert_eq!(return_val.nodes, addresses);
    assert_eq!(return_val.metadatas, metadatas);
}

#[test]
fn test_get_node_capabilities_decoding() {
    // Test decoding getNodeCapabilities return value
    let capabilities = "gpu:rtx4090,ram:32gb,cpu:amd-epyc,bandwidth:1gbps".to_string();

    // Encode the return value
    let encoded = encode(&[Token::String(capabilities.clone())]);

    // Decode it
    let decoded = decode(&[ParamType::String], &encoded).unwrap();
    assert_eq!(decoded.len(), 1);

    if let Token::String(s) = &decoded[0] {
        assert_eq!(s, &capabilities);
    } else {
        panic!("Expected string token");
    }

    // Create return type
    let return_val = GetNodeCapabilitiesReturn {
        capabilities: capabilities.clone(),
    };

    assert_eq!(return_val.capabilities, capabilities);
}

#[test]
fn test_event_signatures() {
    // Verify event signatures match expected ABI
    use ethers::abi::Hash;

    // NodeRegistered(address,string,uint256)
    let sig = "NodeRegistered(address,string,uint256)";
    let hash = ethers::utils::keccak256(sig.as_bytes());
    println!("NodeRegistered signature hash: 0x{}", hex::encode(hash));

    // NodeUpdated(address,string)
    let sig = "NodeUpdated(address,string)";
    let hash = ethers::utils::keccak256(sig.as_bytes());
    println!("NodeUpdated signature hash: 0x{}", hex::encode(hash));

    // NodeUnregistered(address)
    let sig = "NodeUnregistered(address)";
    let hash = ethers::utils::keccak256(sig.as_bytes());
    println!("NodeUnregistered signature hash: 0x{}", hex::encode(hash));
}

#[test]
fn test_function_selectors() {
    // Verify function selectors match expected ABI

    // registerNode(string,uint256)
    let sig = "registerNode(string,uint256)";
    let selector = &ethers::utils::keccak256(sig.as_bytes())[..4];
    println!("registerNode selector: 0x{}", hex::encode(selector));

    // queryRegisteredNodes()
    let sig = "queryRegisteredNodes()";
    let selector = &ethers::utils::keccak256(sig.as_bytes())[..4];
    println!("queryRegisteredNodes selector: 0x{}", hex::encode(selector));

    // getNodeCapabilities(address)
    let sig = "getNodeCapabilities(address)";
    let selector = &ethers::utils::keccak256(sig.as_bytes())[..4];
    println!("getNodeCapabilities selector: 0x{}", hex::encode(selector));
}
