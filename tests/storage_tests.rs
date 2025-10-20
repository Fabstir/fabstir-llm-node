// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Include all storage test modules
mod storage {
    mod test_cbor_compat;
    mod test_model_storage;
    mod test_result_cache;
    mod test_s5_client;

    #[path = "mock/test_enhanced_s5_api.rs"]
    mod test_enhanced_s5_api;
}
