mod websocket {
    mod test_auth;
    mod test_compression;
    mod test_connection;
    mod test_context_building;
    mod test_context_limits;
    mod test_context_management;
    mod test_e2e_scenarios;
    mod test_error_handling;
    mod test_handler_fallback;
    mod test_health;
    mod test_inference_integration;
    mod test_integration;
    mod test_jwt_security;
    mod test_memory_management;
    mod test_message_types;
    mod test_metrics;
    mod test_performance;
    mod test_prompt_handler;
    mod test_proof_config;
    mod test_proof_responses;
    mod test_proof_types;
    mod test_protocol_messages;
    mod test_rate_limiting;
    mod test_real_basic;
    mod test_real_inference;
    mod test_real_job_verification;
    mod test_response_streaming;
    mod test_server;
    mod test_session_init;
    mod test_session_lifecycle;
    mod test_session_protocol;
    mod test_session_resume;
    mod test_session_state;
    mod test_signature_verification;
    mod test_stateful_handler;
    mod test_transport;
    mod test_encrypted_messages;
    // mod test_chain_messages; // TODO: Implement chain message types first
    // mod test_session_handlers; // TODO: Fix test implementation
    // mod test_connection_chain; // TODO: Implement chain connection types first
    // mod test_real_metrics;  // TODO: Implement PrometheusExporter
    // mod test_system_monitoring; // TODO: Implement SystemMonitor
}
