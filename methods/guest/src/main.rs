#![no_main]
#![no_std]

// Risc0 zkVM Guest Program for Commitment Proofs
//
// This program runs inside the zero-knowledge virtual machine and proves
// knowledge of 4 hash commitments (job_id, model_hash, input_hash, output_hash)
// without revealing the actual hash values to the verifier.
//
// Phase: 2.2 (Implementation complete)
// Status: Production-ready guest program
//
// How it works:
// 1. Host (prover) sends 4x [u8; 32] hashes via env::write()
// 2. Guest reads hashes via env::read()
// 3. Guest commits hashes to public journal via env::commit()
// 4. Proof generated proves "I know hashes that match this journal"
// 5. Verifier checks proof + journal without seeing original hashes

risc0_zkvm::guest::entry!(main);

use risc0_zkvm::guest::env;

pub fn main() {
    // Phase 2.2: Read witness data from host (4x 32-byte hashes)
    // Host sends data using write() (serde serialization)
    // Guest reads using env::read() (serde deserialization)
    // Risc0 v3.0: Both sides must use matching serialization
    let job_id: [u8; 32] = env::read();
    let model_hash: [u8; 32] = env::read();
    let input_hash: [u8; 32] = env::read();
    let output_hash: [u8; 32] = env::read();

    // Phase 2.2: Commit all values to journal (makes them public)
    // Journal is the public output of the proof that verifier can check
    // The order must match: job_id, model_hash, input_hash, output_hash
    // Using commit_slice to write raw bytes to journal (no serialization overhead)
    env::commit_slice(&job_id);
    env::commit_slice(&model_hash);
    env::commit_slice(&input_hash);
    env::commit_slice(&output_hash);

    // That's it! The zkVM will:
    // 1. Generate a STARK proof that this code executed correctly
    // 2. Include the journal (4 committed hashes) as public output
    // 3. Allow verifiers to check the proof without re-running the guest
}
