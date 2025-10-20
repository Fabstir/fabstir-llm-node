// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/risc0_tests.rs - Include all Risc0 zkVM test modules

mod risc0 {
    mod test_guest_behavior;      // Phase 2.1: Guest program behavior tests
    mod test_proof_generation;    // Phase 3.1: Proof generation tests
    mod test_verification;        // Phase 4.1: Proof verification tests
}
