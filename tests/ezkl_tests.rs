// tests/ezkl_tests.rs - Include all EZKL test modules

mod ezkl {
    mod test_ezkl_availability;  // Phase 2.1: Availability tests
    mod test_integration;
    mod test_proof_generation;
    mod test_tamper_detection;
    mod test_verification;
    mod test_verification_performance;
    mod test_error_recovery;
}
