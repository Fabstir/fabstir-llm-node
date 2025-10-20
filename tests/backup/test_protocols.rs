// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::p2p::{
    Node, NodeConfig, NodeEvent, ProtocolEvent, 
    InferenceRequest, InferenceResponse, JobClaim, JobResult
};
use libp2p::PeerId;
use std::time::Duration;
use tokio::time::timeout;
use futures::StreamExt;

#[tokio::test]
async fn test_inference_request_protocol() {
    // Create provider and client nodes
    let mut provider = create_node().await;
    let mut client = create_node().await;
    
    let provider_peer_id = provider.peer_id();
    let provider_addr = provider.listeners()[0].clone();
    
    let mut provider_events = provider.start().await;
    let mut client_events = client.start().await;
    
    // Connect client to provider
    client.connect(provider_peer_id, provider_addr).await.unwrap();
    
    // Send inference request
    let request = InferenceRequest {
        request_id: "test-123".to_string(),
        model: "llama-7b".to_string(),
        prompt: "Hello, world!".to_string(),
        max_tokens: 100,
        temperature: 0.7,
        stream: false,
    };
    
    client
        .send_inference_request(provider_peer_id, request.clone())
        .await
        .expect("Failed to send request");
    
    // Provider should receive request
    let received_request = timeout(Duration::from_secs(2), async {
        loop {
            match provider_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::InferenceRequestReceived {
                    peer_id,
                    request: req,
                })) => {
                    if peer_id == client.peer_id() {
                        return Ok(req);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for request")
    .expect("Request reception failed");
    
    assert_eq!(received_request.request_id, request.request_id);
    assert_eq!(received_request.model, request.model);
    assert_eq!(received_request.prompt, request.prompt);
}

#[tokio::test]
async fn test_inference_response_protocol() {
    let mut provider = create_node().await;
    let mut client = create_node().await;
    
    let provider_peer_id = provider.peer_id();
    let client_peer_id = client.peer_id();
    
    let mut provider_events = provider.start().await;
    let mut client_events = client.start().await;
    
    // Connect
    connect_nodes(&mut client, &mut provider).await;
    
    // Send request
    let request = InferenceRequest {
        request_id: "test-456".to_string(),
        model: "llama-7b".to_string(),
        prompt: "Once upon a time".to_string(),
        max_tokens: 50,
        temperature: 0.8,
        stream: false,
    };
    
    client.send_inference_request(provider_peer_id, request.clone()).await.unwrap();
    
    // Provider receives and responds
    tokio::spawn(async move {
        loop {
            match provider_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::InferenceRequestReceived {
                    peer_id,
                    request,
                })) => {
                    // Send response
                    let response = InferenceResponse {
                        request_id: request.request_id,
                        content: "there was a magical kingdom".to_string(),
                        tokens_used: 6,
                        model_used: request.model,
                        finish_reason: "stop".to_string(),
                    };
                    
                    provider
                        .send_inference_response(peer_id, response)
                        .await
                        .expect("Failed to send response");
                }
                _ => {}
            }
        }
    });
    
    // Client receives response
    let response = timeout(Duration::from_secs(2), async {
        loop {
            match client_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::InferenceResponseReceived {
                    peer_id,
                    response,
                })) => {
                    if peer_id == provider_peer_id {
                        return Ok(response);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for response")
    .expect("Response reception failed");
    
    assert_eq!(response.request_id, "test-456");
    assert_eq!(response.content, "there was a magical kingdom");
    assert_eq!(response.tokens_used, 6);
}

#[tokio::test]
async fn test_streaming_inference_protocol() {
    let mut provider = create_node().await;
    let mut client = create_node().await;
    
    let provider_peer_id = provider.peer_id();
    let mut provider_events = provider.start().await;
    
    connect_nodes(&mut client, &mut provider).await;
    
    // Start provider response handler
    tokio::spawn(async move {
        loop {
            match provider_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::InferenceRequestReceived {
                    peer_id,
                    request,
                })) => {
                    if request.stream {
                        // Send streaming response
                        let chunks = vec!["Once ", "upon ", "a ", "time..."];
                        
                        for (i, chunk) in chunks.iter().enumerate() {
                            let response = InferenceResponse {
                                request_id: request.request_id.clone(),
                                content: chunk.to_string(),
                                tokens_used: 1,
                                model_used: request.model.clone(),
                                finish_reason: if i == chunks.len() - 1 { "stop" } else { "length" }.to_string(),
                            };
                            
                            provider
                                .send_streaming_response(peer_id, response)
                                .await
                                .expect("Failed to send chunk");
                            
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                _ => {}
            }
        }
    });
    
    // Send streaming request
    let request = InferenceRequest {
        request_id: "stream-123".to_string(),
        model: "llama-7b".to_string(),
        prompt: "Tell me a story".to_string(),
        max_tokens: 100,
        temperature: 0.7,
        stream: true,
    };
    
    let mut stream = client
        .send_streaming_inference_request(provider_peer_id, request)
        .await
        .expect("Failed to send streaming request");
    
    // Collect stream chunks
    let mut collected = String::new();
    let mut chunk_count = 0;
    
    while let Some(chunk) = stream.next().await {
        collected.push_str(&chunk.content);
        chunk_count += 1;
    }
    
    assert_eq!(collected, "Once upon a time...");
    assert_eq!(chunk_count, 4);
}

#[tokio::test]
async fn test_job_claim_protocol() {
    let mut host = create_node().await;
    let mut marketplace = create_node().await;
    
    let host_peer_id = host.peer_id();
    let marketplace_peer_id = marketplace.peer_id();
    
    let mut host_events = host.start().await;
    let mut marketplace_events = marketplace.start().await;
    
    connect_nodes(&mut host, &mut marketplace).await;
    
    // Host claims a job
    let claim = JobClaim {
        job_id: 42,
        host_address: "0x1234567890abcdef".to_string(),
        model_commitment: vec![1, 2, 3, 4],
        estimated_completion: Duration::from_secs(300),
    };
    
    host.send_job_claim(marketplace_peer_id, claim.clone())
        .await
        .expect("Failed to send claim");
    
    // Marketplace receives claim
    let received_claim = timeout(Duration::from_secs(2), async {
        loop {
            match marketplace_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::JobClaimReceived {
                    peer_id,
                    claim,
                })) => {
                    if peer_id == host_peer_id {
                        return Ok(claim);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for claim")
    .expect("Claim reception failed");
    
    assert_eq!(received_claim.job_id, 42);
    assert_eq!(received_claim.host_address, "0x1234567890abcdef");
}

#[tokio::test]
async fn test_job_result_protocol() {
    let mut host = create_node().await;
    let mut client = create_node().await;
    
    let host_peer_id = host.peer_id();
    let mut host_events = host.start().await;
    let mut client_events = client.start().await;
    
    connect_nodes(&mut host, &mut client).await;
    
    // Host submits job result
    let result = JobResult {
        job_id: 42,
        output_hash: vec![5, 6, 7, 8],
        proof_data: vec![9, 10, 11, 12],
        tokens_used: 150,
        computation_time: Duration::from_secs(10),
    };
    
    host.send_job_result(client.peer_id(), result.clone())
        .await
        .expect("Failed to send result");
    
    // Client receives result
    let received_result = timeout(Duration::from_secs(2), async {
        loop {
            match client_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::JobResultReceived {
                    peer_id,
                    result,
                })) => {
                    if peer_id == host_peer_id {
                        return Ok(result);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for result")
    .expect("Result reception failed");
    
    assert_eq!(received_result.job_id, 42);
    assert_eq!(received_result.tokens_used, 150);
}

#[tokio::test]
async fn test_protocol_versioning() {
    // Create nodes with different protocol versions
    let config_v1 = NodeConfig {
        protocol_version: "1.0.0".to_string(),
        ..Default::default()
    };
    
    let config_v2 = NodeConfig {
        protocol_version: "2.0.0".to_string(),
        ..Default::default()
    };
    
    let mut node_v1 = Node::new(config_v1).await.expect("Failed to create v1 node");
    let mut node_v2 = Node::new(config_v2).await.expect("Failed to create v2 node");
    
    let mut events_v1 = node_v1.start().await;
    let _events_v2 = node_v2.start().await;
    
    // Try to connect incompatible versions
    let result = node_v1.connect(node_v2.peer_id(), node_v2.listeners()[0].clone()).await;
    
    // Should receive protocol mismatch event
    let event = timeout(Duration::from_secs(2), async {
        loop {
            match events_v1.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::ProtocolMismatch {
                    peer_id,
                    our_version,
                    their_version,
                })) => {
                    return Ok((peer_id, our_version, their_version));
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for mismatch")
    .expect("Mismatch detection failed");
    
    assert_eq!(event.0, node_v2.peer_id());
    assert_eq!(event.1, "1.0.0");
    assert_eq!(event.2, "2.0.0");
}

#[tokio::test]
async fn test_protocol_negotiation() {
    let config = NodeConfig {
        supported_protocols: vec![
            "/fabstir/inference/1.0.0".to_string(),
            "/fabstir/inference/1.1.0".to_string(),
            "/fabstir/job/1.0.0".to_string(),
        ],
        ..Default::default()
    };
    
    let mut node1 = Node::new(config.clone()).await.expect("Failed to create node1");
    let mut node2 = Node::new(config).await.expect("Failed to create node2");
    
    let mut events1 = node1.start().await;
    let _events2 = node2.start().await;
    
    connect_nodes(&mut node1, &mut node2).await;
    
    // Should negotiate common protocols
    let negotiated = timeout(Duration::from_secs(2), async {
        loop {
            match events1.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::ProtocolsNegotiated {
                    peer_id,
                    protocols,
                })) => {
                    if peer_id == node2.peer_id() {
                        return Ok(protocols);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for negotiation")
    .expect("Protocol negotiation failed");
    
    assert_eq!(negotiated.len(), 3);
    assert!(negotiated.contains(&"/fabstir/inference/1.1.0".to_string()));
}

#[tokio::test]
async fn test_request_timeout() {
    let mut provider = create_node().await;
    let mut client = create_node().await;
    
    let provider_peer_id = provider.peer_id();
    let _provider_events = provider.start().await;
    let mut client_events = client.start().await;
    
    connect_nodes(&mut client, &mut provider).await;
    
    // Send request (provider won't respond)
    let request = InferenceRequest {
        request_id: "timeout-test".to_string(),
        model: "llama-7b".to_string(),
        prompt: "This will timeout".to_string(),
        max_tokens: 50,
        temperature: 0.7,
        stream: false,
    };
    
    client
        .send_inference_request_with_timeout(provider_peer_id, request, Duration::from_secs(1))
        .await
        .expect("Failed to send request");
    
    // Should receive timeout event
    let event = timeout(Duration::from_secs(2), async {
        loop {
            match client_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::RequestTimeout {
                    peer_id,
                    request_id,
                })) => {
                    return Ok((peer_id, request_id));
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for timeout event")
    .expect("Timeout detection failed");
    
    assert_eq!(event.0, provider_peer_id);
    assert_eq!(event.1, "timeout-test");
}

#[tokio::test]
async fn test_protocol_rate_limiting() {
    let config = NodeConfig {
        max_requests_per_minute: 10,
        ..Default::default()
    };
    
    let mut provider = Node::new(config).await.expect("Failed to create provider");
    let mut client = create_node().await;
    
    let provider_peer_id = provider.peer_id();
    let mut provider_events = provider.start().await;
    
    connect_nodes(&mut client, &mut provider).await;
    
    // Send multiple requests quickly
    for i in 0..15 {
        let request = InferenceRequest {
            request_id: format!("rate-limit-{}", i),
            model: "llama-7b".to_string(),
            prompt: "Test".to_string(),
            max_tokens: 10,
            temperature: 0.7,
            stream: false,
        };
        
        let _ = client.send_inference_request(provider_peer_id, request).await;
    }
    
    // Should receive rate limit event after 10 requests
    let event = timeout(Duration::from_secs(2), async {
        loop {
            match provider_events.recv().await {
                Some(NodeEvent::ProtocolEvent(ProtocolEvent::RateLimitExceeded {
                    peer_id,
                    requests_made,
                    limit,
                })) => {
                    return Ok((peer_id, requests_made, limit));
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for rate limit")
    .expect("Rate limit detection failed");
    
    assert_eq!(event.0, client.peer_id());
    assert!(event.1 > 10);
    assert_eq!(event.2, 10);
}

// Helper functions

async fn create_node() -> Node {
    let config = NodeConfig::default();
    Node::new(config).await.expect("Failed to create node")
}

async fn connect_nodes(node1: &mut Node, node2: &mut Node) {
    let peer_id = node2.peer_id();
    let addr = node2.listeners()[0].clone();
    node1.connect(peer_id, addr).await.expect("Failed to connect");
    tokio::time::sleep(Duration::from_millis(200)).await;
}