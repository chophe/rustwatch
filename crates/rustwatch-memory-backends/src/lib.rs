//! Optional LanceDB vector store and SurrealDB graph store for rustwatch.
//!
//! Build with: `cargo build -p rustwatch-memory-backends`

mod lance;
mod surreal;

pub use lance::LanceMemoryStore;
pub use surreal::SurrealGraphStore;
