//! Contracts and result types for `cmd/vcs.rs`: git-based version control for design histories.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Initialize a git repository in the design's folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct VcsInitParams {
    /// User name for git commits (falls back to "SolidCraft VCS").
    #[serde(default)]
    pub author_name: Option<String>,
    /// Email address for git commits (falls back to "vcs@solidcraft.local").
    #[serde(default)]
    pub author_email: Option<String>,
}

/// What `vcs.init` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VcsInitResult {
    /// Absolute path of the git repository root.
    pub repo: String,
    /// True when this call created a new repository; false when one already existed.
    pub created: bool,
}

/// Commit the current design to the history.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct VcsCommitParams {
    /// Commit message describing this design state.
    pub message: String,
    /// User name for this commit (overrides the repository default).
    #[serde(default)]
    pub author_name: Option<String>,
    /// Email address for this commit (overrides the repository default).
    #[serde(default)]
    pub author_email: Option<String>,
}

/// What `vcs.commit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VcsCommitResult {
    /// The full SHA-1 hex id of the new commit.
    pub commit: String,
    /// The commit message.
    pub message: String,
}

/// Query the current history status of the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct VcsStatusParams {}

/// What `vcs.status` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VcsStatusResult {
    /// Whether a git repository exists for this design's folder.
    pub initialized: bool,
    /// Whether the design has changed since the last commit.
    pub changed: bool,
    /// The current branch name, when a repository is initialized and HEAD names a branch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The id of the most recent commit on HEAD, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

/// List the design's commit history.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct VcsLogParams {
    /// Maximum commits to return (default 20, capped at 1000).
    #[serde(default)]
    pub limit: Option<u64>,
}

/// A single entry in the design's commit history.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VcsLogEntry {
    /// Full SHA-1 hex id of the commit.
    pub commit: String,
    /// Commit message.
    pub message: String,
    /// Author name.
    pub author: String,
    /// Unix timestamp of the author date.
    pub time: i64,
}

/// What `vcs.log` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VcsLogResult {
    /// Commits on HEAD, newest first.
    pub commits: Vec<VcsLogEntry>,
}
