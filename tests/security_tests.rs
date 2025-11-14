// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Security test runner
//!
//! Runs comprehensive security tests for cryptographic implementation
//! and embedding API security

#[cfg(test)]
mod security {
    mod test_embed_security;
    mod test_s5_security;
}
