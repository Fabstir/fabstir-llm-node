// Include all vector test modules
mod vector {
    mod test_client;
    mod test_embeddings;
    mod test_semantic_cache;
    mod test_storage;
    
    // Mock tests for Vector DB API
    mod mock {
        mod test_vector_db_api;
    }
}