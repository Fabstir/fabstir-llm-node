// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! UKH1 (8.67.0): crate bump tokenizer mapping (U1), llama settings (U2),
//! keyed proof content commitments (U2b) and the region start check (U3).
//! Own target because `api_tests` does not compile on this tree.

mod ukh1 {
    pub mod active_sessions;
    pub mod llama_settings;
    pub mod proof_content;
    pub mod proof_key_source;
    pub mod region;
    pub mod tokenizer;
}
