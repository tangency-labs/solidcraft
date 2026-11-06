//! SolidBOM's development service: project storage first (task 2), the HTTP API next (task 3).
//!
//! This is a separate workspace from the CAD application. It is not built for wasm and its
//! dependencies are not shipped in the desktop program.

pub mod store;

pub use store::{Store, StoreError};

/// A store operation's result.
pub type Result<T> = std::result::Result<T, StoreError>;
