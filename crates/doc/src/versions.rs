//! Named versions and branches stored in the design file.
//!
//! The open workspace is the rest of the document. Every version, and every workspace that is
//! not open, keeps a snapshot of that same document with this graph left out. Identical
//! snapshots are stored once.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{DocError, Document, Result};

/// How many versions one design may keep.
const MAX_VERSIONS: usize = 256;
/// How many workspaces (Main included) one design may keep.
const MAX_WORKSPACES: usize = 32;
/// The longest name a version or a workspace may have.
const MAX_NAME: usize = 80;

/// One frozen design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub name: String,
    /// The version this one follows. The first version on Main has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The workspace it was taken from.
    pub workspace: String,
    /// Key into [`Versions::blobs`].
    pub snapshot: String,
}

/// An editable line of the design. The open one has no snapshot: the document is it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// The version this workspace was branched from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The design as it was when the user left this workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

/// Where a node sits in the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphKind {
    Version,
    Workspace,
}

/// One node of the graph, laid out in columns from the oldest version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphPoint {
    pub id: String,
    pub kind: GraphKind,
    pub name: String,
    pub column: u32,
    pub row: u32,
    pub current: bool,
    pub parent: Option<String>,
}

/// The version graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Versions {
    /// The workspace the rest of the document is.
    #[serde(default = "main_id")]
    pub current: String,
    #[serde(default = "main_workspace")]
    pub workspaces: Vec<Workspace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<Version>,
    /// Content-addressed copies of the design, without this graph.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blobs: BTreeMap<String, Value>,
    /// The next `vN` / `wN` id.
    #[serde(default = "next_starts")]
    pub next: u64,
}

fn main_id() -> String {
    "main".into()
}

fn next_starts() -> u64 {
    1
}

fn main_workspace() -> Vec<Workspace> {
    vec![Workspace { id: main_id(), name: "Main".into(), from: None, snapshot: None }]
}

impl Default for Versions {
    fn default() -> Self {
        Self { current: main_id(), workspaces: main_workspace(), versions: Vec::new(), blobs: BTreeMap::new(), next: 1 }
    }
}

impl Versions {
    /// A design that has never been versioned. Not saved.
    pub fn is_empty(&self) -> bool {
        self.current == "main"
            && self.versions.is_empty()
            && self.blobs.is_empty()
            && self.workspaces.len() == 1
            && self.workspaces[0].id == "main"
            && self.workspaces[0].name == "Main"
            && self.workspaces[0].from.is_none()
            && self.workspaces[0].snapshot.is_none()
    }

    /// The graph, oldest versions to the left and workspace tips on their own column.
    pub fn layout(&self) -> Vec<GraphPoint> {
        let mut guard = Vec::new();
        let mut points = Vec::new();
        let row_of = |id: &str| self.workspaces.iter().position(|workspace| workspace.id == id).unwrap_or(0) as u32;
        for version in &self.versions {
            points.push(GraphPoint {
                id: version.id.clone(),
                kind: GraphKind::Version,
                name: version.name.clone(),
                column: self.version_column(&version.id, &mut guard),
                row: row_of(&version.workspace),
                current: false,
                parent: version.parent.clone(),
            });
        }
        for workspace in &self.workspaces {
            points.push(GraphPoint {
                id: workspace.id.clone(),
                kind: GraphKind::Workspace,
                name: workspace.name.clone(),
                column: self.workspace_column(workspace, &mut guard),
                row: row_of(&workspace.id),
                current: workspace.id == self.current,
                parent: workspace.from.clone(),
            });
        }
        points
    }

    fn version_column(&self, id: &str, guard: &mut Vec<String>) -> u32 {
        if guard.iter().any(|seen| seen == id) {
            return 0;
        }
        guard.push(id.to_string());
        let column = self
            .versions
            .iter()
            .find(|version| version.id == id)
            .and_then(|version| version.parent.as_deref())
            .map(|parent| self.version_column(parent, guard).saturating_add(1))
            .unwrap_or(0);
        guard.pop();
        column
    }

    /// A workspace tip sits just after the last version made on it, or just after the version
    /// it was branched from.
    fn workspace_column(&self, workspace: &Workspace, guard: &mut Vec<String>) -> u32 {
        let on_it =
            self.versions.iter().filter(|version| version.workspace == workspace.id).map(|version| self.version_column(&version.id, guard)).max();
        if let Some(column) = on_it {
            return column.saturating_add(1);
        }
        workspace.from.as_deref().map(|id| self.version_column(id, guard).saturating_add(1)).unwrap_or(0)
    }

    /// Refuse a graph that cannot be switched.
    pub fn validate(&self) -> Result<()> {
        let bad = |message: &str| Err(DocError::Invalid(format!("damaged design: {message}")));
        if self.workspaces.is_empty() || self.workspaces.len() > MAX_WORKSPACES || self.versions.len() > MAX_VERSIONS {
            return bad("the version graph is empty or too large");
        }
        if self.workspaces.iter().filter(|workspace| workspace.id == self.current).count() != 1 {
            return bad("the open workspace is missing");
        }
        let mut ids = BTreeMap::<&str, ()>::new();
        for workspace in &self.workspaces {
            if !ids.insert(workspace.id.as_str(), ()).is_none() || workspace.name.trim().is_empty() {
                return bad("a workspace id is repeated or unnamed");
            }
            if workspace.from.as_deref().is_some_and(|id| self.versions.iter().all(|version| version.id != id)) {
                return bad("a workspace starts from a missing version");
            }
            match &workspace.snapshot {
                None if workspace.id == self.current => {}
                Some(hash) if workspace.id != self.current && self.blobs.contains_key(hash) => {}
                _ => return bad("a workspace snapshot does not match the open workspace"),
            }
        }
        for version in &self.versions {
            if !ids.insert(version.id.as_str(), ()).is_none() || version.name.trim().is_empty() {
                return bad("a version id is repeated or unnamed");
            }
            if !self.blobs.contains_key(&version.snapshot) {
                return bad("a version snapshot is missing");
            }
            if self.workspaces.iter().all(|workspace| workspace.id != version.workspace) {
                return bad("a version names a missing workspace");
            }
            if version.parent.as_deref().is_some_and(|id| self.versions.iter().all(|other| other.id != id)) {
                return bad("a version follows a missing version");
            }
        }
        Ok(())
    }
}

impl Document {
    /// Freeze the open workspace under `name`.
    ///
    /// `link_version` is asked for each linked design's path and may name the version that file
    /// was on. The answer is stored in the snapshot, not in the open design.
    pub fn create_version(&mut self, name: &str, mut link_version: impl FnMut(&str) -> Option<String>) -> Result<String> {
        let name = checked_name(&self.versions, name)?;
        if self.versions.versions.len() >= MAX_VERSIONS {
            return Err(DocError::Invalid(format!("a design keeps at most {MAX_VERSIONS} versions")));
        }
        let workspace = self.versions.current.clone();
        let parent = self
            .versions
            .versions
            .iter()
            .rev()
            .find(|version| version.workspace == workspace)
            .map(|version| version.id.clone())
            .or_else(|| self.versions.workspaces.iter().find(|item| item.id == workspace).and_then(|item| item.from.clone()));
        let snapshot = self.store_blob(true, &mut link_version)?;
        let id = self.allocate_version_id();
        self.versions.versions.push(Version { id: id.clone(), name, parent, workspace, snapshot });
        Ok(id)
    }

    /// Copy the open workspace as it is now into a new workspace and open that. The workspace
    /// that was open is kept. A version is saved first when this moment is not already one, so
    /// the two lines meet on the graph.
    pub fn branch_from_here(&mut self, name: &str, mut link_version: impl FnMut(&str) -> Option<String>) -> Result<String> {
        let name = checked_name(&self.versions, name)?;
        if self.versions.workspaces.len() >= MAX_WORKSPACES {
            return Err(DocError::Invalid(format!("a design keeps at most {MAX_WORKSPACES} workspaces")));
        }
        let snapshot = self.store_blob(true, &mut link_version)?;
        let current = self.versions.current.clone();
        let from =
            if let Some(version) = self.versions.versions.iter().rev().find(|version| version.workspace == current && version.snapshot == snapshot) {
                version.id.clone()
            } else {
                if self.versions.versions.len() >= MAX_VERSIONS {
                    return Err(DocError::Invalid(format!("a design keeps at most {MAX_VERSIONS} versions")));
                }
                let version_name = unique_name(&self.versions, "Point", &name);
                let parent =
                    self.versions.versions.iter().rev().find(|version| version.workspace == current).map(|version| version.id.clone()).or_else(
                        || self.versions.workspaces.iter().find(|workspace| workspace.id == current).and_then(|workspace| workspace.from.clone()),
                    );
                let id = self.allocate_version_id();
                self.versions.versions.push(Version { id: id.clone(), name: version_name, parent, workspace: current, snapshot: snapshot.clone() });
                id
            };
        self.open_branch(name, &from, &snapshot)
    }

    /// Start a workspace from a version and open it. The workspace that was open is kept.
    pub fn branch(&mut self, name: &str, from_version: &str) -> Result<String> {
        let name = checked_name(&self.versions, name)?;
        if self.versions.workspaces.len() >= MAX_WORKSPACES {
            return Err(DocError::Invalid(format!("a design keeps at most {MAX_WORKSPACES} workspaces")));
        }
        let snapshot = self
            .versions
            .versions
            .iter()
            .find(|version| version.id == from_version)
            .map(|version| version.snapshot.clone())
            .ok_or_else(|| DocError::Invalid(format!("no version {from_version}")))?;
        self.open_branch(name, from_version, &snapshot)
    }

    fn open_branch(&mut self, name: String, from_version: &str, snapshot: &str) -> Result<String> {
        self.park_open_workspace()?;
        self.install_snapshot(snapshot)?;
        let id = self.allocate_workspace_id();
        self.versions.workspaces.push(Workspace { id: id.clone(), name, from: Some(from_version.to_string()), snapshot: None });
        self.versions.current = id.clone();
        Ok(id)
    }

    /// Open another workspace. The one that was open is kept as a snapshot.
    pub fn switch_workspace(&mut self, id: &str) -> Result<()> {
        if id == self.versions.current {
            return Ok(());
        }
        if self.versions.workspaces.iter().all(|workspace| workspace.id != id) {
            return Err(DocError::Invalid(format!("no workspace {id}")));
        }
        self.park_open_workspace()?;
        let snapshot = self
            .versions
            .workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .and_then(|workspace| workspace.snapshot.clone())
            .ok_or_else(|| DocError::Invalid(format!("workspace {id} has no saved design")))?;
        self.install_snapshot(&snapshot)?;
        if let Some(workspace) = self.versions.workspaces.iter_mut().find(|workspace| workspace.id == id) {
            workspace.snapshot = None;
        }
        self.versions.current = id.to_string();
        Ok(())
    }

    /// Replace the open workspace with a version. The version stays in the graph.
    pub fn restore_version(&mut self, id: &str) -> Result<()> {
        let snapshot = self
            .versions
            .versions
            .iter()
            .find(|version| version.id == id)
            .map(|version| version.snapshot.clone())
            .ok_or_else(|| DocError::Invalid(format!("no version {id}")))?;
        self.install_snapshot(&snapshot)
    }

    fn park_open_workspace(&mut self) -> Result<()> {
        let snapshot = self.store_blob(false, &mut |_| None)?;
        let current = self.versions.current.clone();
        let workspace = self
            .versions
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == current)
            .ok_or_else(|| DocError::Invalid("the open workspace is missing".into()))?;
        workspace.snapshot = Some(snapshot);
        Ok(())
    }

    fn install_snapshot(&mut self, hash: &str) -> Result<()> {
        let blob = self.versions.blobs.get(hash).cloned().ok_or_else(|| DocError::Invalid("a version snapshot is missing".into()))?;
        let body: Document =
            serde_json::from_value(blob).map_err(|error| DocError::Invalid(format!("a version snapshot could not be read ({error})")))?;
        let versions = self.versions.clone();
        *self = body;
        self.versions = versions;
        Ok(())
    }

    /// Hash the design, without the graph, and keep the blob. When `probe_links` is set, linked
    /// designs record the version `link_version` reports, in the blob only.
    fn store_blob(&mut self, probe_links: bool, link_version: &mut impl FnMut(&str) -> Option<String>) -> Result<String> {
        let mut copy = self.clone();
        copy.versions = Versions::default();
        if probe_links {
            for link in copy.links.values_mut() {
                if let Some(version) = link_version(&link.path) {
                    link.version = Some(version);
                }
            }
        }
        let value = serde_json::to_value(&copy).map_err(|error| DocError::Invalid(format!("could not store a version ({error})")))?;
        let bytes = serde_json::to_vec(&value).map_err(|error| DocError::Invalid(format!("could not store a version ({error})")))?;
        let hash = hex(Sha256::digest(&bytes).as_slice());
        self.versions.blobs.entry(hash.clone()).or_insert(value);
        Ok(hash)
    }

    fn allocate_version_id(&mut self) -> String {
        let id = format!("v{}", self.versions.next);
        self.versions.next = self.versions.next.saturating_add(1);
        id
    }

    fn allocate_workspace_id(&mut self) -> String {
        let id = format!("w{}", self.versions.next);
        self.versions.next = self.versions.next.saturating_add(1);
        id
    }
}

fn unique_name(versions: &Versions, base: &str, also: &str) -> String {
    let taken = |candidate: &str| {
        candidate == also
            || versions.versions.iter().any(|version| version.name == candidate)
            || versions.workspaces.iter().any(|workspace| workspace.name == candidate)
    };
    if !taken(base) {
        return base.to_string();
    }
    let mut number = 2u32;
    loop {
        let candidate = format!("{base} {number}");
        if !taken(&candidate) {
            return candidate;
        }
        number += 1;
    }
}

fn checked_name(versions: &Versions, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(|c| c.is_control()) {
        return Err(DocError::Invalid(format!("a version name is empty, has a control character, or is longer than {MAX_NAME} characters")));
    }
    let taken = versions.versions.iter().any(|version| version.name == name) || versions.workspaces.iter().any(|workspace| workspace.name == name);
    if taken {
        return Err(DocError::Invalid(format!("a version or workspace is already named {name}")));
    }
    Ok(name.to_string())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

/// The version a linked design file is on: its newest version on the open workspace, or
/// `ws:<workspace>` when that file has no versions yet. `None` when the file cannot be read.
pub fn linked_file_version(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let Some(versions) = value.get("versions") else {
        return Some("ws:main".into());
    };
    let current = versions.get("current").and_then(Value::as_str).unwrap_or("main");
    let latest = versions
        .get("versions")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().rev().find(|node| node.get("workspace").and_then(Value::as_str) == Some(current)));
    latest.and_then(|node| node.get("id")).and_then(Value::as_str).map(str::to_string).or_else(|| Some(format!("ws:{current}")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::DesignLink;

    fn design() -> Document {
        let mut doc = Document::new("Robot");
        doc.links.insert(1, DesignLink { path: "motor.solidcraft".into(), feature: 4, version: None });
        doc
    }

    #[test]
    fn a_new_design_is_main_and_saves_no_graph() {
        let doc = Document::new("Robot");
        assert!(doc.versions.is_empty());
        let saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
        assert!(saved.get("versions").is_none(), "an unversioned design does not save a graph");
        doc.versions.validate().unwrap();
    }

    #[test]
    fn a_version_freezes_the_design_and_restore_brings_it_back() {
        let mut doc = design();
        let first = doc.create_version("Start", |_| None).unwrap();
        doc.name = "Later".into();
        let second = doc.create_version("Later", |_| None).unwrap();
        assert_eq!(doc.versions.versions.iter().find(|version| version.id == second).unwrap().parent.as_deref(), Some(first.as_str()));
        doc.restore_version(&first).unwrap();
        assert_eq!(doc.name, "Robot");
        assert_eq!(doc.versions.versions.len(), 2, "restore keeps the graph");
        assert!(doc.links[&1].version.is_none(), "the open design's links are not rewritten");
    }

    #[test]
    fn the_same_design_is_stored_once() {
        let mut doc = Document::new("Robot");
        doc.create_version("One", |_| None).unwrap();
        doc.create_version("Two", |_| None).unwrap();
        assert_eq!(doc.versions.blobs.len(), 1);
    }

    #[test]
    fn main_can_be_branched_without_a_version_first() {
        let mut doc = Document::new("Robot");
        doc.name = "Now".into();
        let branch = doc.branch_from_here("Side", |_| None).unwrap();
        assert_eq!(doc.versions.current, branch);
        assert_eq!(doc.name, "Now");
        assert_eq!(doc.versions.versions.len(), 1, "the fork is a saved point on Main");
        assert_eq!(doc.versions.versions[0].name, "Point");
        doc.switch_workspace("main").unwrap();
        assert_eq!(doc.name, "Now");
        let points = doc.versions.layout();
        let side = points.iter().find(|point| point.name == "Side").unwrap();
        let main = points.iter().find(|point| point.id == "main").unwrap();
        assert_ne!(side.row, main.row);
        assert!(side.parent.is_some());
    }

    #[test]
    fn a_branch_keeps_both_workspaces() {
        let mut doc = design();
        let first = doc.create_version("Start", |_| None).unwrap();
        doc.name = "Main later".into();
        doc.create_version("Later", |_| None).unwrap();
        let branch = doc.branch("Side", &first).unwrap();
        assert_eq!(doc.name, "Robot");
        assert_eq!(doc.versions.current, branch);
        doc.name = "On the side".into();
        doc.switch_workspace("main").unwrap();
        assert_eq!(doc.name, "Main later");
        doc.switch_workspace(&branch).unwrap();
        assert_eq!(doc.name, "On the side");
    }

    #[test]
    fn a_linked_file_version_is_recorded_only_in_the_snapshot() {
        let mut doc = design();
        doc.create_version("Start", |path| (path == "motor.solidcraft").then(|| "v9".into())).unwrap();
        let blob = doc.versions.blobs.values().next().unwrap();
        assert_eq!(blob["links"]["1"]["version"], "v9");
        assert!(doc.links[&1].version.is_none());
    }

    #[test]
    fn the_graph_round_trips_and_lays_a_branch_beside_main() {
        let mut doc = design();
        let first = doc.create_version("Start", |_| None).unwrap();
        doc.branch("Side", &first).unwrap();
        let opened = Document::from_json(&doc.to_json()).unwrap();
        assert_eq!(opened.versions.versions.len(), 1);
        assert_eq!(opened.versions.workspaces.len(), 2);
        opened.versions.validate().unwrap();
        let points = opened.versions.layout();
        let start = points.iter().find(|point| point.id == first).unwrap();
        let side = points.iter().find(|point| point.name == "Side").unwrap();
        let main = points.iter().find(|point| point.id == "main").unwrap();
        assert!(side.column > start.column);
        assert_eq!(side.parent.as_deref(), Some(first.as_str()));
        assert!(main.column > start.column);
        assert_ne!(side.row, main.row, "a branch has its own line");
        assert_eq!(start.row, main.row, "a version stays on the workspace that made it");
        assert!(side.current);
        assert!(!main.current);
    }

    #[test]
    fn names_and_missing_versions_are_refused() {
        let mut doc = Document::new("Robot");
        assert!(doc.create_version("  ", |_| None).is_err());
        doc.create_version("Start", |_| None).unwrap();
        assert!(doc.create_version("Start", |_| None).is_err());
        assert!(doc.branch("Side", "v9").is_err());
        assert!(doc.restore_version("nope").is_err());
    }

    #[test]
    fn a_file_without_versions_reports_its_workspace() {
        assert_eq!(linked_file_version(br#"{"name":"Motor"}"#), Some("ws:main".into()));
        assert_eq!(linked_file_version(br#"{"versions":{"current":"main"}}"#), Some("ws:main".into()));
        let marked = br#"{"versions":{"current":"main","versions":[{"id":"v3","workspace":"main"}]}}"#;
        assert_eq!(linked_file_version(marked), Some("v3".into()));
        assert_eq!(linked_file_version(b"not json"), None);
    }
}
