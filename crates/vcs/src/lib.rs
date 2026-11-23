//! Git-based version control for SolidCraft design histories.
//!
//! Each design lives in a folder. `init` creates a `.git` repository there and writes a
//! `.gitignore` that excludes the binary design container while keeping the parametric JSON
//! sidecar. `commit` serialises the design's parametric data to `<stem>.model.json`, stages it,
//! and writes a commit. `log` and `status` read the repository without modifying it.
//!
//! All public functions are gated to non-wasm targets; web builds have no writable filesystem.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
mod imp;

#[cfg(not(target_arch = "wasm32"))]
pub use imp::{CommitInfo, StatusInfo, commit, init, log, status};

pub use error::VcsError;

mod error {
    #[derive(Debug, thiserror::Error)]
    pub enum VcsError {
        #[error("the design has not been saved to a file yet (save it first, then run vcs.init)")]
        NoPath,
        #[error("the design's folder has no git repository (run vcs.init first)")]
        NotARepo,
        #[error("git: {0}")]
        Git(String),
        #[error("{0}")]
        Io(#[from] std::io::Error),
    }
}
