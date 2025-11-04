// RAG (Retrieval-Augmented Generation) module
// Session-scoped vector storage for semantic search during chat sessions

pub mod session_vector_store;

pub use session_vector_store::{SearchResult, SessionVectorStore, VectorEntry};
