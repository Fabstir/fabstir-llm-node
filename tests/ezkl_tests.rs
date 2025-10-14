// tests/ezkl_tests.rs - Include all EZKL test modules

mod ezkl {
    mod test_ezkl_availability;  // Phase 2.1: Availability tests
    mod test_commitment_circuit;  // Phase 2.2: Circuit design tests
    mod test_witness_generation;  // Phase 2.2: Witness builder tests
    mod test_circuit_constraints; // Phase 2.2: Constraint tests
    mod test_key_management;      // Phase 2.3: Key generation tests
    mod test_integration;
    mod test_proof_generation;
    mod test_proof_validation;    // Phase 3.3: Proof validation tests
    mod test_proof_caching;       // Phase 4.2: Proof caching tests
    mod test_performance;         // Phase 4.3: Performance optimization tests
    mod test_tamper_detection;
    mod test_verification;
    mod test_verification_performance;
    mod test_error_recovery;
}
