//! CAM library document types (templates + remotes).
//!
//! Git sync, catalog index, and preset install land in a follow-up library PR.
//! This module keeps only what [`crate::ManufactureSettings`] persists.

use serde::{Deserialize, Serialize};

use crate::{Result, invalid};

pub const DEFAULT_CAM_LIBRARY_REMOTE: &str =
    "";

pub const MAX_LIBRARY_REMOTES: usize = 32;
pub const MAX_TEMPLATES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Post,
    Machine,
    Template,
}

impl LibraryKind {
    pub const ALL: &'static [LibraryKind] =
        &[LibraryKind::Post, LibraryKind::Machine, LibraryKind::Template];

    pub fn label(self) -> &'static str {
        match self {
            LibraryKind::Post => "Post",
            LibraryKind::Machine => "Machine",
            LibraryKind::Template => "Template",
        }
    }

    pub fn folder(self) -> &'static str {
        match self {
            LibraryKind::Post => "posts",
            LibraryKind::Machine => "machines",
            LibraryKind::Template => "templates",
        }
    }
}

/// Where a catalog row lives in the library browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryLocation {
    Document,
    Local,
    Linked,
}

impl LibraryLocation {
    pub fn label(self) -> &'static str {
        match self {
            LibraryLocation::Document => "Document",
            LibraryLocation::Local => "Local",
            LibraryLocation::Linked => "Linked",
        }
    }
}

/// User-configured git remote that can supply posts, machines, and/or templates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryRemote {
    /// Stable id (also used as cache folder name).
    pub id: String,
    pub name: String,
    /// HTTPS, SSH, or file URL / absolute path to a CAM library git repo.
    pub url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Which asset kinds to pull from this remote (empty = all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<LibraryKind>,
    pub enabled: bool,
    /// Last successful sync unix seconds (display only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_error: String,
}

fn default_branch() -> String {
    "main".into()
}

impl Default for LibraryRemote {
    fn default() -> Self {
        Self {
            id: "solidcraft-cam".into(),
            name: "SolidCraft CAM Libraries".into(),
            url: DEFAULT_CAM_LIBRARY_REMOTE.into(),
            branch: default_branch(),
            kinds: Vec::new(),
            enabled: true,
            last_synced_unix: None,
            last_error: String::new(),
        }
    }
}

impl LibraryRemote {
    pub fn validate(&self, path: &str) -> Result<()> {
        if self.id.trim().is_empty() || self.id != self.id.trim() || self.id.len() > 64 {
            return Err(invalid(format!("{path}.id: trimmed non-empty, <= 64 bytes")));
        }
        if self.id.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            return Err(invalid(format!("{path}.id: ascii alphanumeric / - / _ only")));
        }
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(invalid(format!("{path}.name: required, <= 128 bytes")));
        }
        if self.url.trim().is_empty() || self.url.len() > 512 {
            return Err(invalid(format!("{path}.url: required, <= 512 bytes")));
        }
        if self.branch.trim().is_empty() || self.branch.len() > 128 {
            return Err(invalid(format!("{path}.branch: required, <= 128 bytes")));
        }
        Ok(())
    }

    pub fn covers(&self, kind: LibraryKind) -> bool {
        self.kinds.is_empty() || self.kinds.contains(&kind)
    }
}

/// Reusable CAM operation template (Template Library).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CamTemplate {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// e.g. milling_2d, additive, holes, fabrication
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strategies: Vec<String>,
    /// Opaque JSON payload (operation draft(s)); validated as JSON object/array only.
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl Default for CamTemplate {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            category: "milling_2d".into(),
            strategies: Vec::new(),
            payload: serde_json::json!({}),
        }
    }
}

impl CamTemplate {
    pub fn validate(&self, path: &str) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(invalid(format!("{path}.name: required, <= 128 bytes")));
        }
        if self.description.len() > 1_024 {
            return Err(invalid(format!("{path}.description: <= 1024 bytes")));
        }
        if self.category.len() > 64 {
            return Err(invalid(format!("{path}.category: <= 64 bytes")));
        }
        if self.strategies.len() > 32 {
            return Err(invalid(format!("{path}.strategies: max 32")));
        }
        Ok(())
    }
}
