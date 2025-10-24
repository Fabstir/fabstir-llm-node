// ---
// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// ---
// Build script for Fabstir LLM Node
//
// Phase 1.1: Risc0 zkVM Guest Program Compilation
//
// This build script compiles the Risc0 guest program (methods/guest/) into
// a RISC-V ELF binary when the `real-ezkl` feature is enabled.
//
// Generated constants (in OUT_DIR/methods.rs):
// - COMMITMENT_GUEST_ELF: The compiled guest program binary
// - COMMITMENT_GUEST_ID: Deterministic hash of the guest program (Image ID)
//
// These constants are used by the prover and verifier in:
// - src/crypto/ezkl/prover.rs
// - src/crypto/ezkl/verifier.rs
//
// Guest method directories are specified in Cargo.toml under [package.metadata.risc0]

fn main() {
    // Only compile guest program when real-ezkl feature is enabled
    #[cfg(feature = "real-ezkl")]
    {
        // Compile all guest programs specified in Cargo.toml metadata
        // This generates methods.rs in OUT_DIR with:
        // - COMMITMENT_GUEST_ELF (the executable binary)
        // - COMMITMENT_GUEST_ID (the image ID for verification)
        risc0_build::embed_methods();

        println!("cargo:rerun-if-changed=methods/guest/src");
        println!("cargo:rerun-if-changed=methods/guest/Cargo.toml");

        println!("cargo:warning=✅ Risc0 guest program will be compiled (Phase 1.2 pending)");
    }

    // Without real-ezkl feature, build script does nothing
    #[cfg(not(feature = "real-ezkl"))]
    {
        println!(
            "cargo:warning=⏭️  Skipping Risc0 guest compilation (real-ezkl feature not enabled)"
        );
    }
}
