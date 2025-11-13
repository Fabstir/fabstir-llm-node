// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/crypto_tests.rs - Include all crypto test modules

mod crypto {
    mod test_aes_gcm;
    mod test_coverage;
    mod test_ecdh;
    mod test_encryption;
    mod test_errors;
    mod test_private_key;
    mod test_session_init;
    mod test_session_keys;
    mod test_session_lifecycle;
    mod test_signature;
}
