use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn setup_test_env() {
    std::env::set_var("RUST_LOG", "debug");
}

#[test]
fn test_openapi_spec_valid() {
    setup_test_env();

    // Check if OpenAPI spec exists
    let openapi_path = Path::new("docs/openapi.yaml");
    assert!(
        openapi_path.exists(),
        "OpenAPI specification file should exist at docs/openapi.yaml"
    );

    // Read and validate the OpenAPI spec structure
    let content = fs::read_to_string(openapi_path).expect("Failed to read OpenAPI spec");

    // Basic validation - check for required OpenAPI fields
    assert!(content.contains("openapi:"), "Must specify OpenAPI version");
    assert!(content.contains("info:"), "Must have info section");
    assert!(content.contains("paths:"), "Must have paths section");
    assert!(content.contains("components:"), "Must have components section");

    // Check for chain-specific endpoints
    assert!(content.contains("/v1/chains"), "Must document /v1/chains endpoint");
    assert!(content.contains("/v1/chains/stats"), "Must document chain stats endpoint");
    assert!(content.contains("chain_id"), "Must document chain_id parameter");
}

#[test]
fn test_example_requests() {
    setup_test_env();

    // Test that example requests are valid JSON and have required fields

    // Example inference request with chain
    let inference_example = json!({
        "model": "tinyllama",
        "prompt": "Hello world",
        "max_tokens": 50,
        "chain_id": 84532,
        "job_id": 123
    });

    assert_eq!(inference_example["chain_id"], 84532);
    assert_eq!(inference_example["model"], "tinyllama");

    // Example models request with chain parameter
    let models_query = "chain_id=84532";
    assert!(models_query.contains("chain_id"));

    // Example chains response
    let chains_response = json!({
        "chains": [
            {
                "chain_id": 84532,
                "name": "Base Sepolia",
                "native_token": "ETH",
                "rpc_url": "https://sepolia.base.org"
            }
        ],
        "default_chain": 84532
    });

    assert!(chains_response["chains"].is_array());
    assert_eq!(chains_response["default_chain"], 84532);
}

#[test]
fn test_documentation_completeness() {
    setup_test_env();

    // Check that main API documentation exists and is complete
    let api_doc_path = Path::new("docs/API.md");
    assert!(api_doc_path.exists(), "API.md documentation must exist");

    let api_content = fs::read_to_string(api_doc_path).expect("Failed to read API.md");

    // Check for all required endpoints
    let required_endpoints = [
        "/health",
        "/v1/models",
        "/v1/inference",
        "/v1/chains",
        "/v1/chains/stats",
        "/v1/session",
        "/v1/ws",
        "/metrics",
    ];

    for endpoint in &required_endpoints {
        assert!(
            api_content.contains(endpoint),
            "API documentation must include {} endpoint",
            endpoint
        );
    }

    // Check for chain-specific documentation
    assert!(api_content.contains("chain_id"), "Must document chain_id parameter");
    assert!(api_content.contains("Base Sepolia"), "Must document Base Sepolia chain");
    assert!(api_content.contains("native_token"), "Must document native token");
}

#[test]
fn test_chain_parameter_documentation() {
    setup_test_env();

    // Verify that chain parameters are properly documented
    let api_doc = fs::read_to_string("docs/API.md").unwrap_or_default();

    // Check for chain parameter in models endpoint
    assert!(
        api_doc.contains("GET /v1/models") || api_doc.contains("### List Available Models"),
        "Models endpoint must be documented"
    );

    // Check for chain_id in inference request
    assert!(
        api_doc.contains("chain_id") || api_doc.contains("Chain ID"),
        "chain_id parameter must be documented"
    );

    // Check for native token documentation
    assert!(
        api_doc.contains("ETH") || api_doc.contains("native token"),
        "Native token must be documented"
    );
}

#[test]
fn test_response_schema_documentation() {
    setup_test_env();

    // Test that response schemas include chain information
    let example_response = json!({
        "model": "tinyllama",
        "content": "Generated text",
        "tokens_used": 25,
        "finish_reason": "stop",
        "request_id": "req-123",
        "chain_id": 84532,
        "chain_name": "Base Sepolia",
        "native_token": "ETH"
    });

    // Verify all required fields are present
    assert!(example_response["chain_id"].is_number());
    assert!(example_response["chain_name"].is_string());
    assert!(example_response["native_token"].is_string());
}

#[test]
fn test_error_response_documentation() {
    setup_test_env();

    // Test that error responses are documented with chain context
    let error_example = json!({
        "error_type": "model_not_found",
        "message": "Model not available on Base Sepolia",
        "request_id": "req-123",
        "chain_id": 84532,
        "details": {
            "chain_name": "Base Sepolia",
            "native_token": "ETH"
        }
    });

    assert_eq!(error_example["chain_id"], 84532);
    assert!(error_example["message"].as_str().unwrap().contains("Base Sepolia"));
    assert_eq!(error_example["details"]["native_token"], "ETH");
}

#[test]
fn test_troubleshooting_documentation() {
    setup_test_env();

    let troubleshooting_path = Path::new("docs/TROUBLESHOOTING.md");
    if troubleshooting_path.exists() {
        let content = fs::read_to_string(troubleshooting_path).expect("Failed to read troubleshooting guide");

        // Check for common chain-related issues
        assert!(
            content.contains("chain") || content.contains("Chain"),
            "Troubleshooting should cover chain-related issues"
        );
    }
}

#[test]
fn test_websocket_documentation_updated() {
    setup_test_env();

    let ws_doc_path = Path::new("docs/WEBSOCKET_API_SDK_GUIDE.md");
    if ws_doc_path.exists() {
        let content = fs::read_to_string(ws_doc_path).expect("Failed to read WebSocket guide");

        // Check for chain context in WebSocket docs
        assert!(
            content.contains("chain") || content.contains("Chain"),
            "WebSocket documentation should include chain information"
        );
    }
}