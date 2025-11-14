// RAG (Retrieval-Augmented Generation) module
// Session-scoped vector storage for semantic search during chat sessions

pub mod session_vector_store;
pub mod vector_loader;

pub use session_vector_store::{SearchResult, SessionVectorStore, VectorEntry};
pub use vector_loader::{LoadProgress, VectorLoader};
