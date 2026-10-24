//! CAM asset libraries (posts, machines, templates) with local + remote git sources.
//!
//! Browser layout:
//! - Document — assets already in this file
//! - Local — built-in presets shipped with SolidCraft
//! - Linked — user-configured git remotes (central catalog or vendor forks)
//!
//! Remote repos are expected to publish an `index.json` (see [`CatalogIndex`]) listing
//! entries under `posts/`, `machines/`, and `templates/`. Sync clones/fetches into a
//! cache directory; install copies a selected entry into the document.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{MachineProfile, PostProfile, Result, invalid, machines, post};

/// Default central SolidCraft CAM library (posts + machines + templates).
/// Point additional remotes at vendor forks the same way linked remotes attach cloud libraries.
pub const DEFAULT_CAM_LIBRARY_REMOTE: &str = "";

pub const MAX_LIBRARY_REMOTES: usize = 32;
pub const MAX_TEMPLATES: usize = 256;
pub const MAX_CATALOG_ENTRIES: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Post,
    Machine,
    Template,
}

impl LibraryKind {
    pub const ALL: &'static [LibraryKind] = &[LibraryKind::Post, LibraryKind::Machine, LibraryKind::Template];

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
    /// `https://…/repo.git`, `ssh://…`, or `file:///…` / absolute path.
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
            return Err(invalid(format!("{path}.id: trimmed non-empty, ≤ 64 bytes")));
        }
        if self.id.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            return Err(invalid(format!("{path}.id: ascii alphanumeric / - / _ only")));
        }
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(invalid(format!("{path}.name: required, ≤ 128 bytes")));
        }
        if self.url.trim().is_empty() || self.url.len() > 512 {
            return Err(invalid(format!("{path}.url: required, ≤ 512 bytes")));
        }
        if self.branch.trim().is_empty() || self.branch.len() > 128 {
            return Err(invalid(format!("{path}.branch: required, ≤ 128 bytes")));
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
            return Err(invalid(format!("{path}.name: required, ≤ 128 bytes")));
        }
        if self.description.len() > 1_024 {
            return Err(invalid(format!("{path}.description: ≤ 1024 bytes")));
        }
        if self.category.len() > 64 {
            return Err(invalid(format!("{path}.category: ≤ 64 bytes")));
        }
        if self.strategies.len() > 32 {
            return Err(invalid(format!("{path}.strategies: max 32")));
        }
        Ok(())
    }
}

/// One row in a library browser (document / local / linked).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub id: String,
    pub kind: LibraryKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vendor: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub location: LibraryLocation,
    /// Remote id when `location == Linked`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_id: String,
    /// Relative path inside the remote (or builtin key).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// Manifest published at the root of a CAM library git repo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CatalogIndex {
    pub schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub entries: Vec<CatalogIndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogIndexEntry {
    pub id: String,
    pub kind: LibraryKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vendor: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Path relative to repo root (e.g. `posts/Haas/ngc.post.json`).
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// Built-in local catalog (shipped presets + sample templates).
pub fn builtin_catalog() -> Vec<CatalogEntry> {
    let mut out = Vec::new();
    for (id, p) in post::preset_posts() {
        out.push(CatalogEntry {
            id: format!("local/post/{id}"),
            kind: LibraryKind::Post,
            name: p.name.clone(),
            vendor: "SolidCraft".into(),
            description: format!("{:?} dialect post", p.dialect),
            location: LibraryLocation::Local,
            source_id: String::new(),
            path: id.into(),
            capabilities: vec!["milling".into(), "turning".into(), "fabrication".into()],
        });
    }
    for (id, m) in machines::preset_machines() {
        let mut caps = vec!["milling".into()];
        if m.probe.present {
            caps.push("inspection".into());
        }
        out.push(CatalogEntry {
            id: format!("local/machine/{id}"),
            kind: LibraryKind::Machine,
            name: m.name.clone(),
            vendor: m.oem.clone(),
            description: format!("{} · {}", m.model, m.integration.kind_label()),
            location: LibraryLocation::Local,
            source_id: String::new(),
            path: id.into(),
            capabilities: caps,
        });
    }
    for t in sample_templates() {
        out.push(CatalogEntry {
            id: format!("local/template/{}", slug(&t.name)),
            kind: LibraryKind::Template,
            name: t.name.clone(),
            vendor: "SolidCraft".into(),
            description: t.description.clone(),
            location: LibraryLocation::Local,
            source_id: String::new(),
            path: slug(&t.name),
            capabilities: {
                let mut c = vec![t.category.clone()];
                c.extend(t.strategies.iter().cloned());
                c
            },
        });
    }
    out
}

fn slug(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().trim_matches('-').to_string()
}

/// Sample templates for Template Library categories.
pub fn sample_templates() -> Vec<CamTemplate> {
    vec![
        CamTemplate {
            name: "Drill & Countersink Hole".into(),
            description: "Contains two operations".into(),
            category: "holes".into(),
            strategies: vec!["drill".into(), "chamfer".into()],
            payload: serde_json::json!({"ops": ["drill", "chamfer"]}),
        },
        CamTemplate {
            name: "Drill & Tap Countersink Hole".into(),
            description: "Contains three operations".into(),
            category: "holes".into(),
            strategies: vec!["drill".into(), "tap".into(), "chamfer".into()],
            payload: serde_json::json!({"ops": ["drill", "tap", "chamfer"]}),
        },
        CamTemplate {
            name: "FFF - Additive Arrange".into(),
            description: "Additive arrangement preset".into(),
            category: "additive".into(),
            strategies: vec!["arrange".into()],
            payload: serde_json::json!({"kind": "fff_arrange"}),
        },
        CamTemplate {
            name: "SLA - Automatic Orientation".into(),
            description: "Additive orientation preset".into(),
            category: "additive".into(),
            strategies: vec!["orientation".into()],
            payload: serde_json::json!({"kind": "sla_orient"}),
        },
        CamTemplate {
            name: "2D Profile — Sheet Cut".into(),
            description: "Fabrication waterjet/laser/plasma profile".into(),
            category: "fabrication".into(),
            strategies: vec!["2d_profile".into()],
            payload: serde_json::json!({"kind": "fab_2d_profile"}),
        },
    ]
}

/// Default remotes suggested for a fresh install (user can disable / replace).
pub fn default_remotes() -> Vec<LibraryRemote> {
    vec![LibraryRemote::default()]
}

/// Cache root for cloned library remotes (`$SOLIDCRAFT_CAM_LIBRARY_CACHE` or platform default).
pub fn cache_root() -> PathBuf {
    if let Ok(p) = std::env::var("SOLIDCRAFT_CAM_LIBRARY_CACHE") {
        return PathBuf::from(p);
    }
    dirs_cache().join("cam-libraries")
}

fn dirs_cache() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("SolidCraft");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("solidcraft");
    }
    PathBuf::from(".solidcraft-cache")
}

pub fn remote_cache_dir(remote: &LibraryRemote) -> PathBuf {
    cache_root().join(&remote.id)
}

/// Parse `index.json` from a synced remote checkout.
pub fn read_index(repo_dir: &Path) -> Result<CatalogIndex> {
    let path = repo_dir.join("index.json");
    let bytes = fs::read(&path).map_err(|e| invalid(format!("read {}: {e}", path.display())))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid("index.json too large (max 8 MiB)"));
    }
    let index: CatalogIndex = serde_json::from_slice(&bytes).map_err(|e| invalid(format!("index.json: {e}")))?;
    if index.schema_version == 0 || index.schema_version > 1 {
        return Err(invalid(format!("unsupported catalog schema_version {}", index.schema_version)));
    }
    if index.entries.len() > MAX_CATALOG_ENTRIES {
        return Err(invalid(format!("too many catalog entries (max {MAX_CATALOG_ENTRIES})")));
    }
    Ok(index)
}

/// Convert a remote index into browser catalog rows.
pub fn catalog_from_index(remote: &LibraryRemote, index: &CatalogIndex) -> Vec<CatalogEntry> {
    index
        .entries
        .iter()
        .filter(|e| remote.covers(e.kind))
        .map(|e| CatalogEntry {
            id: format!("linked/{}/{}", remote.id, e.id),
            kind: e.kind,
            name: e.name.clone(),
            vendor: e.vendor.clone(),
            description: e.description.clone(),
            location: LibraryLocation::Linked,
            source_id: remote.id.clone(),
            path: e.path.clone(),
            capabilities: e.capabilities.clone(),
        })
        .collect()
}

/// Resolve the on-disk checkout for a remote (local path in-place, else cache clone).
pub fn remote_checkout_dir(remote: &LibraryRemote) -> PathBuf {
    if let Some(local) = local_path_from_url(&remote.url) { local } else { remote_cache_dir(remote) }
}

/// Clone or fetch a remote into the cache. Returns path to checkout.
pub fn sync_remote(remote: &LibraryRemote) -> std::result::Result<PathBuf, String> {
    if let Some(local) = local_path_from_url(&remote.url) {
        // file:// or absolute path — read in place (no copy; keeps tests free of env mutation).
        if !local.join("index.json").is_file() {
            return Err(format!("local library missing index.json at {}", local.display()));
        }
        return Ok(local);
    }
    let dest = remote_cache_dir(remote);
    fs::create_dir_all(cache_root()).map_err(|e| e.to_string())?;
    if dest.join(".git").is_dir() {
        let status = Command::new("git")
            .args(["-C", dest.to_str().unwrap_or("."), "fetch", "--depth", "1", "origin", &remote.branch])
            .status()
            .map_err(|e| format!("git fetch: {e}"))?;
        if !status.success() {
            return Err(format!("git fetch failed ({status})"));
        }
        let status = Command::new("git")
            .args(["-C", dest.to_str().unwrap_or("."), "checkout", "-B", &remote.branch, &format!("origin/{}", remote.branch)])
            .status()
            .map_err(|e| format!("git checkout: {e}"))?;
        if !status.success() {
            return Err(format!("git checkout failed ({status})"));
        }
    } else {
        if dest.exists() {
            let _ = fs::remove_dir_all(&dest);
        }
        let status = Command::new("git")
            .args(["clone", "--depth", "1", "--branch", &remote.branch, &remote.url, dest.to_str().unwrap_or(".")])
            .status()
            .map_err(|e| format!("git clone: {e} (is git on PATH?)"))?;
        if !status.success() {
            return Err(format!("git clone failed ({status})"));
        }
    }
    if !dest.join("index.json").is_file() {
        return Err("remote checkout has no index.json".into());
    }
    Ok(dest)
}

fn local_path_from_url(url: &str) -> Option<PathBuf> {
    let u = url.trim();
    if let Some(rest) = u.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    let p = Path::new(u);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }
    None
}

/// Load a post JSON from a synced remote path.
pub fn load_post_file(repo: &Path, rel: &str) -> Result<PostProfile> {
    let path = repo.join(rel);
    let bytes = fs::read(&path).map_err(|e| invalid(format!("post {}: {e}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("post JSON: {e}")))
}

/// Load a machine JSON from a synced remote path.
pub fn load_machine_file(repo: &Path, rel: &str) -> Result<MachineProfile> {
    let path = repo.join(rel);
    let bytes = fs::read(&path).map_err(|e| invalid(format!("machine {}: {e}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("machine JSON: {e}")))
}

/// Load a template JSON from a synced remote path.
pub fn load_template_file(repo: &Path, rel: &str) -> Result<CamTemplate> {
    let path = repo.join(rel);
    let bytes = fs::read(&path).map_err(|e| invalid(format!("template {}: {e}", path.display())))?;
    let t: CamTemplate = serde_json::from_slice(&bytes).map_err(|e| invalid(format!("template JSON: {e}")))?;
    t.validate("template")?;
    Ok(t)
}

/// Build document-scoped catalog rows from in-document assets.
pub fn document_catalog(
    posts: &BTreeMap<String, PostProfile>,
    machines: &BTreeMap<String, MachineProfile>,
    templates: &BTreeMap<String, CamTemplate>,
) -> Vec<CatalogEntry> {
    let mut out = Vec::new();
    for (id, p) in posts {
        out.push(CatalogEntry {
            id: format!("document/post/{id}"),
            kind: LibraryKind::Post,
            name: p.name.clone(),
            vendor: String::new(),
            description: "In this document".into(),
            location: LibraryLocation::Document,
            source_id: String::new(),
            path: id.clone(),
            capabilities: Vec::new(),
        });
    }
    for (id, m) in machines {
        out.push(CatalogEntry {
            id: format!("document/machine/{id}"),
            kind: LibraryKind::Machine,
            name: m.name.clone(),
            vendor: m.oem.clone(),
            description: m.model.clone(),
            location: LibraryLocation::Document,
            source_id: String::new(),
            path: id.clone(),
            capabilities: Vec::new(),
        });
    }
    for (id, t) in templates {
        out.push(CatalogEntry {
            id: format!("document/template/{id}"),
            kind: LibraryKind::Template,
            name: t.name.clone(),
            vendor: String::new(),
            description: t.description.clone(),
            location: LibraryLocation::Document,
            source_id: String::new(),
            path: id.clone(),
            capabilities: vec![t.category.clone()],
        });
    }
    out
}

/// Minimal example `index.json` for authoring a central repo.
pub fn example_index() -> CatalogIndex {
    CatalogIndex {
        schema_version: 1,
        name: "SolidCraft CAM Libraries".into(),
        description: "Central posts, machines, and templates".into(),
        entries: vec![
            CatalogIndexEntry {
                id: "post-grbl-laser".into(),
                kind: LibraryKind::Post,
                name: "GRBL Laser".into(),
                vendor: "Generic".into(),
                description: "Laser/plasma GRBL dialect".into(),
                path: "posts/Generic/grbl-laser.post.json".into(),
                capabilities: vec!["fabrication".into(), "cutting".into()],
            },
            CatalogIndexEntry {
                id: "machine-generic-fff".into(),
                kind: LibraryKind::Machine,
                name: "Generic FFF".into(),
                vendor: "Autodesk-compatible".into(),
                description: "Additive FFF machine shell".into(),
                path: "machines/Additive/generic-fff.machine.json".into(),
                capabilities: vec!["additive".into()],
            },
        ],
    }
}

trait IntegrationKindLabel {
    fn kind_label(&self) -> String;
}

impl IntegrationKindLabel for crate::MachineIntegration {
    fn kind_label(&self) -> String {
        format!("{:?}", self.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_has_posts_machines_templates() {
        let c = builtin_catalog();
        assert!(c.iter().any(|e| e.kind == LibraryKind::Post));
        assert!(c.iter().any(|e| e.kind == LibraryKind::Machine));
        assert!(c.iter().any(|e| e.kind == LibraryKind::Template));
    }

    #[test]
    fn example_index_roundtrips() {
        let idx = example_index();
        let json = serde_json::to_string_pretty(&idx).unwrap();
        let back: CatalogIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(idx, back);
    }

    #[test]
    fn sync_local_file_repo() {
        let tmp = std::env::temp_dir().join(format!("sc-cam-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("posts")).unwrap();
        let idx = example_index();
        fs::write(tmp.join("index.json"), serde_json::to_vec_pretty(&idx).unwrap()).unwrap();
        // Override cache so we don't pollute user dirs
        let remote = LibraryRemote {
            id: "test-local".into(),
            name: "Test".into(),
            url: tmp.to_string_lossy().into(),
            branch: "main".into(),
            kinds: Vec::new(),
            enabled: true,
            last_synced_unix: None,
            last_error: String::new(),
        };
        let dest = sync_remote(&remote).unwrap();
        assert_eq!(dest, tmp);
        let loaded = read_index(&dest).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }
}
