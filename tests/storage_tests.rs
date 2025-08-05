// Include all storage test modules
mod storage {
    mod test_cbor_compat;
    mod test_s5_client;
    mod test_model_storage;
    mod test_result_cache;
    
    #[path = "mock/test_enhanced_s5_api.rs"]
    mod test_enhanced_s5_api;
}