// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Include all vector test modules
mod vector {
    mod test_client;
    mod test_embeddings;
    mod test_hnsw_index;
    mod test_index_cache;
    mod test_semantic_cache;
    mod test_storage;

    // Mock tests for Vector DB API
    mod mock {
        mod test_vector_db_api;
    }
}
