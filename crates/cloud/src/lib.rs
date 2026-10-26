//! Portable repository snapshots. No CAD, UI, credentials, filesystem or network dependencies.
//! See docs/cloud-storage-format.md for the compatibility and publication contract.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod catalog;
mod configuration;
pub mod conflicts;
pub mod credentials;
mod download;
#[cfg(feature = "github")]
pub mod github;
pub mod jobs;
pub mod management;
mod outbox;
mod project;
mod publication;
mod settings;
pub mod setup;
mod snapshot;
mod thumbnail;
pub use catalog::{CachedEntry, LocalRevision, StoredDesign, manifest_digest, stored_designs};
pub use configuration::{Configuration, MAX_SETTINGS_BYTES};
pub use download::DownloadCache;
pub use outbox::{PendingSave, StagedSave};
pub use project::{Destination, PROJECT_MARKER, Project, check_path};
pub use publication::{Publication, Published, revision};
pub use settings::{Authentication, Connection, GitRemote, HostSetup, Settings, SshIdentity, github_setup, host_setup};
pub use snapshot::{Asset, CHUNK_BYTES, Chunk, Codec, FORMAT, MAX_ASSET_BYTES, MAX_MANIFEST_BYTES, Manifest, Snapshot, digest};

pub use thumbnail::{MAX_THUMBNAIL_BYTES, Thumbnail, attach_thumbnail};

/// Errors are classified so every host can offer the same recovery action.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Upgrade SolidCraft to open or save this design: {0}")]
    UpgradeRequired(String),
    #[error("Invalid cloud design: {0}")]
    Invalid(String),
    #[error("Cloud storage: {0}")]
    Storage(String),
    #[error("The design changed elsewhere; keep both versions before saving")]
    Conflict,
}

pub type Result<T> = std::result::Result<T, Error>;
