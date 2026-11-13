//! cleanplate core: every algorithm on in-memory planes. No file, process or OS access (wasm32-ready).
pub mod filter;
pub mod geom;
pub mod plane;

pub use plane::Plane;
