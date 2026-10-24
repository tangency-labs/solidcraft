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

/// One difference between the open design and a version or another workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DesignChange {
    /// Stable id for a merge choice, such as `param:width` or `feature:4`.
    pub path: String,
    /// The parameter name, or the feature id.
    pub key: String,
    /// The name shown in the list.
    pub name: String,
    pub what: ChangeWhat,
    pub kind: ChangeKind,
}

/// Whether a difference is a parameter, a feature, or the design name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeWhat {
    Parameter,
    Feature,
    Name,
}

/// How the selected design differs from the open one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// The selected design has it and this workspace does not.
    Added,
    /// This workspace has it and the selected design does not.
    Removed,
    /// Only the selected design changed it since the shared version.
    Changed,
    /// Both sides changed it.
    Conflict,
}

/// What [`Document::merge_workspace`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The open workspace had not moved, so it now matches the other one.
    BroughtIn,
    /// Both sides moved, and the changes sat in different places.
    Merged,
    /// The other workspace had nothing new.
    Already,
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

    /// Rename a version or a workspace.
    pub fn rename_node(&mut self, id: &str, name: &str) -> Result<()> {
        let current = self
            .versions
            .versions
            .iter()
            .find(|version| version.id == id)
            .map(|version| version.name.clone())
            .or_else(|| self.versions.workspaces.iter().find(|workspace| workspace.id == id).map(|workspace| workspace.name.clone()));
        let Some(current) = current else { return Err(DocError::Invalid(format!("no version or workspace {id}"))) };
        if current == name.trim() {
            return Ok(());
        }
        let name = checked_name(&self.versions, name)?;
        if let Some(version) = self.versions.versions.iter_mut().find(|version| version.id == id) {
            version.name = name;
            return Ok(());
        }
        if let Some(workspace) = self.versions.workspaces.iter_mut().find(|workspace| workspace.id == id) {
            workspace.name = name;
        }
        Ok(())
    }

    /// Remove a version. Anything that started from it now starts from the version before it.
    pub fn delete_version(&mut self, id: &str) -> Result<()> {
        let parent = self
            .versions
            .versions
            .iter()
            .find(|version| version.id == id)
            .map(|version| version.parent.clone())
            .ok_or_else(|| DocError::Invalid(format!("no version {id}")))?;
        for version in &mut self.versions.versions {
            if version.parent.as_deref() == Some(id) {
                version.parent.clone_from(&parent);
            }
        }
        for workspace in &mut self.versions.workspaces {
            if workspace.from.as_deref() == Some(id) {
                workspace.from.clone_from(&parent);
            }
        }
        self.versions.versions.retain(|version| version.id != id);
        self.drop_unused_blobs();
        Ok(())
    }

    /// Remove a workspace and the versions made on it. Main, and the workspace that is open, stay.
    pub fn delete_workspace(&mut self, id: &str) -> Result<()> {
        if id == "main" {
            return Err(DocError::Invalid("Main stays".into()));
        }
        if id == self.versions.current {
            return Err(DocError::Invalid("open another workspace before deleting this one".into()));
        }
        if self.versions.workspaces.iter().all(|workspace| workspace.id != id) {
            return Err(DocError::Invalid(format!("no workspace {id}")));
        }
        let doomed: Vec<String> = self.versions.versions.iter().filter(|version| version.workspace == id).map(|version| version.id.clone()).collect();
        let parents: Vec<(String, Option<String>)> =
            self.versions.versions.iter().map(|version| (version.id.clone(), version.parent.clone())).collect();
        let survive = |start: &Option<String>| -> Option<String> {
            let mut current = start.clone();
            let mut guard = 0u32;
            while let Some(now) = current.clone() {
                if doomed.iter().all(|id| id != &now) {
                    return Some(now);
                }
                current = parents.iter().find(|(id, _)| id == &now).and_then(|(_, parent)| parent.clone());
                guard += 1;
                if guard > 256 {
                    return None;
                }
            }
            None
        };
        for version in &mut self.versions.versions {
            version.parent = survive(&version.parent);
        }
        for workspace in &mut self.versions.workspaces {
            workspace.from = survive(&workspace.from);
        }
        self.versions.versions.retain(|version| version.workspace != id);
        self.versions.workspaces.retain(|workspace| workspace.id != id);
        self.drop_unused_blobs();
        Ok(())
    }

    /// What differs between the open design and a version.
    pub fn changes_for_version(&self, id: &str) -> Result<Vec<DesignChange>> {
        let hash = self
            .versions
            .versions
            .iter()
            .find(|version| version.id == id)
            .map(|version| version.snapshot.clone())
            .ok_or_else(|| DocError::Invalid(format!("no version {id}")))?;
        let theirs = self.versions.blobs.get(&hash).cloned().ok_or_else(|| DocError::Invalid(format!("no version {id}")))?;
        let ours = self.design_value()?;
        Ok(design_changes(&ours, &ours, &theirs))
    }

    /// What a merge of `id` would change in the open workspace.
    pub fn changes_for_workspace(&self, id: &str) -> Result<Vec<DesignChange>> {
        let (base, ours, theirs) = self.sides(id)?;
        Ok(design_changes(&base, &ours, &theirs))
    }

    /// Bring `id` into the open workspace.
    ///
    /// When only one side moved on since the shared version, that side is kept. When both moved,
    /// features and parameters combine by id and by name. `choices` names each place both sides
    /// changed: `true` takes the other workspace, `false` keeps the open one. A conflict with no
    /// choice is refused, and nothing is written.
    pub fn merge_workspace(&mut self, id: &str, choices: &[(&str, bool)]) -> Result<MergeOutcome> {
        let (base, ours, theirs) = self.sides(id)?;
        if ours == theirs || theirs == base {
            return Ok(MergeOutcome::Already);
        }
        if ours == base {
            self.install_value(theirs)?;
            return Ok(MergeOutcome::BroughtIn);
        }
        let open = ours.clone();
        let (ours, theirs) = resolve_conflicts(&base, ours, theirs, choices)?;
        let merged = merge_values(&base, &ours, &theirs, "")?;
        if merged == open {
            return Ok(MergeOutcome::Already);
        }
        self.install_value(merged)?;
        Ok(MergeOutcome::Merged)
    }

    fn sides(&self, id: &str) -> Result<(Value, Value, Value)> {
        if id == self.versions.current {
            return Err(DocError::Invalid("a workspace cannot be merged into itself".into()));
        }
        let theirs_name = self
            .versions
            .workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .map(|workspace| workspace.name.clone())
            .ok_or_else(|| DocError::Invalid(format!("no workspace {id}")))?;
        let theirs_hash = self
            .versions
            .workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .and_then(|workspace| workspace.snapshot.clone())
            .ok_or_else(|| DocError::Invalid(format!("{theirs_name} has no saved design")))?;
        let theirs = self.versions.blobs.get(&theirs_hash).cloned().ok_or_else(|| DocError::Invalid(format!("{theirs_name} has no saved design")))?;
        let base = match self.shared_version(id) {
            Some(version_id) => {
                let hash = self
                    .versions
                    .versions
                    .iter()
                    .find(|version| version.id == version_id)
                    .map(|version| version.snapshot.clone())
                    .ok_or_else(|| DocError::Invalid("a shared version is missing".into()))?;
                self.versions.blobs.get(&hash).cloned().ok_or_else(|| DocError::Invalid("a shared version is missing".into()))?
            }
            None => return Err(DocError::Invalid(format!("{theirs_name} does not share a version with this workspace"))),
        };
        Ok((base, self.design_value()?, theirs))
    }

    /// The newest version that is on both this workspace's history and `other`'s.
    fn shared_version(&self, other: &str) -> Option<String> {
        let theirs = self.history(other);
        self.history(&self.versions.current).into_iter().find(|id| theirs.iter().any(|theirs_id| theirs_id == id))
    }

    fn history(&self, workspace_id: &str) -> Vec<String> {
        let Some(workspace) = self.versions.workspaces.iter().find(|workspace| workspace.id == workspace_id) else { return Vec::new() };
        let newest = self.versions.versions.iter().rev().find(|version| version.workspace == workspace_id).map(|version| version.id.clone());
        let mut chain = Vec::new();
        let mut id = newest.or_else(|| workspace.from.clone());
        let mut guard = 0u32;
        while let Some(current) = id {
            if chain.iter().any(|seen| seen == &current) || guard > 256 {
                break;
            }
            chain.push(current.clone());
            id = self.versions.versions.iter().find(|version| version.id == current).and_then(|version| version.parent.clone());
            guard += 1;
        }
        if let Some(from) = &workspace.from
            && !chain.iter().any(|seen| seen == from)
        {
            id = Some(from.clone());
            while let Some(current) = id {
                if chain.iter().any(|seen| seen == &current) || guard > 256 {
                    break;
                }
                chain.push(current.clone());
                id = self.versions.versions.iter().find(|version| version.id == current).and_then(|version| version.parent.clone());
                guard += 1;
            }
        }
        chain
    }

    fn drop_unused_blobs(&mut self) {
        let mut used = std::collections::BTreeSet::new();
        for version in &self.versions.versions {
            used.insert(version.snapshot.clone());
        }
        for workspace in &self.versions.workspaces {
            if let Some(snapshot) = &workspace.snapshot {
                used.insert(snapshot.clone());
            }
        }
        self.versions.blobs.retain(|hash, _| used.contains(hash));
    }

    fn design_value(&self) -> Result<Value> {
        let mut copy = self.clone();
        copy.versions = Versions::default();
        serde_json::to_value(&copy).map_err(|error| DocError::Invalid(format!("could not read the design ({error})")))
    }

    fn install_value(&mut self, value: Value) -> Result<()> {
        let body: Document = serde_json::from_value(value).map_err(|error| DocError::Invalid(format!("could not merge ({error})")))?;
        let versions = self.versions.clone();
        *self = body;
        self.versions = versions;
        Ok(())
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

fn design_changes(base: &Value, ours: &Value, theirs: &Value) -> Vec<DesignChange> {
    let mut changes = Vec::new();
    if let Some(kind) = classify(base.get("name"), ours.get("name"), theirs.get("name")) {
        changes.push(DesignChange { path: "name".into(), key: "name".into(), name: "name".into(), what: ChangeWhat::Name, kind });
    }
    collect_keyed(base, ours, theirs, "params", "name", ChangeWhat::Parameter, "param", &mut changes);
    collect_keyed(base, ours, theirs, "features", "id", ChangeWhat::Feature, "feature", &mut changes);
    changes
}

fn collect_keyed(
    base: &Value,
    ours: &Value,
    theirs: &Value,
    field: &str,
    key_field: &str,
    what: ChangeWhat,
    path_prefix: &str,
    changes: &mut Vec<DesignChange>,
) {
    let base_items = keyed(base, field, key_field);
    let ours_items = keyed(ours, field, key_field);
    let theirs_items = keyed(theirs, field, key_field);
    let mut keys: Vec<String> = base_items.keys().chain(ours_items.keys()).chain(theirs_items.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let base_item = base_items.get(&key).copied();
        let ours_item = ours_items.get(&key).copied();
        let theirs_item = theirs_items.get(&key).copied();
        let Some(kind) = classify(base_item, ours_item, theirs_item) else { continue };
        let name = [ours_item, theirs_item, base_item]
            .into_iter()
            .flatten()
            .find_map(|item| item.get("name").and_then(Value::as_str))
            .unwrap_or(key.as_str())
            .to_string();
        changes.push(DesignChange { path: format!("{path_prefix}:{key}"), key, name, what, kind });
    }
}

fn keyed<'a>(value: &'a Value, field: &str, key_field: &str) -> BTreeMap<String, &'a Value> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| field_key(item, key_field).map(|id| (id, item))).collect())
        .unwrap_or_default()
}

fn classify(base: Option<&Value>, ours: Option<&Value>, theirs: Option<&Value>) -> Option<ChangeKind> {
    match (base, ours, theirs) {
        (_, Some(ours), Some(theirs)) if ours == theirs => None,
        (Some(base), Some(ours), Some(theirs)) if ours == base && theirs != base => Some(ChangeKind::Changed),
        (Some(base), Some(_), Some(theirs)) if theirs == base => None,
        (Some(_), Some(_), Some(_)) => Some(ChangeKind::Conflict),
        (None, Some(ours), Some(theirs)) if ours != theirs => Some(ChangeKind::Conflict),
        (None, None, Some(_)) => Some(ChangeKind::Added),
        (None, Some(_), None) => None,
        (Some(base), Some(ours), None) if ours == base => Some(ChangeKind::Removed),
        (Some(_), Some(_), None) => Some(ChangeKind::Conflict),
        (Some(base), None, Some(theirs)) if theirs == base => None,
        (Some(_), None, Some(_)) => Some(ChangeKind::Conflict),
        _ => None,
    }
}

fn resolve_conflicts(base: &Value, mut ours: Value, mut theirs: Value, choices: &[(&str, bool)]) -> Result<(Value, Value)> {
    for change in design_changes(base, &ours, &theirs) {
        if change.kind != ChangeKind::Conflict {
            continue;
        }
        let choice = choices.iter().find(|(path, _)| *path == change.path);
        let Some((_, take_theirs)) = choice else {
            return Err(DocError::Invalid(format!("both workspaces changed {}", change.name)));
        };
        if *take_theirs {
            copy_item(&mut ours, &theirs, &change);
        } else {
            copy_item(&mut theirs, &ours, &change);
        }
    }
    Ok((ours, theirs))
}

fn copy_item(dest: &mut Value, source: &Value, change: &DesignChange) {
    if change.what == ChangeWhat::Name {
        match source.get("name").cloned() {
            Some(name) => {
                if let Some(object) = dest.as_object_mut() {
                    object.insert("name".into(), name);
                }
            }
            None => {
                if let Some(object) = dest.as_object_mut() {
                    object.remove("name");
                }
            }
        }
        return;
    }
    let (field, key_field) = match change.what {
        ChangeWhat::Parameter => ("params", "name"),
        ChangeWhat::Feature => ("features", "id"),
        ChangeWhat::Name => return,
    };
    let item = source
        .get(field)
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| field_key(item, key_field).as_deref() == Some(change.key.as_str())).cloned());
    let Some(object) = dest.as_object_mut() else { return };
    let Some(items) = object.entry(field).or_insert_with(|| Value::Array(Vec::new())).as_array_mut() else { return };
    if let Some(pos) = items.iter().position(|item| field_key(item, key_field).as_deref() == Some(change.key.as_str())) {
        match item {
            Some(item) => items[pos] = item,
            None => {
                items.remove(pos);
            }
        }
    } else if let Some(item) = item {
        items.push(item);
    }
}

fn merge_values(base: &Value, ours: &Value, theirs: &Value, path: &str) -> Result<Value> {
    if ours == theirs {
        return Ok(ours.clone());
    }
    if ours == base {
        return Ok(theirs.clone());
    }
    if theirs == base {
        return Ok(ours.clone());
    }
    match (base, ours, theirs) {
        (Value::Object(base), Value::Object(ours), Value::Object(theirs)) => {
            let mut keys: Vec<&String> = base.keys().chain(ours.keys()).chain(theirs.keys()).collect();
            keys.sort();
            keys.dedup();
            let mut merged = serde_json::Map::new();
            for key in keys {
                let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match (base.get(key), ours.get(key), theirs.get(key)) {
                    (_, Some(ours), Some(theirs)) => {
                        merged.insert(key.clone(), merge_values(base.get(key).unwrap_or(&Value::Null), ours, theirs, &child)?);
                    }
                    (Some(base), Some(ours), None) => {
                        if ours != base {
                            return Err(DocError::Invalid(format!("both workspaces changed {child}")));
                        }
                    }
                    (Some(base), None, Some(theirs)) => {
                        if theirs != base {
                            return Err(DocError::Invalid(format!("both workspaces changed {child}")));
                        }
                    }
                    (None, Some(ours), None) => {
                        merged.insert(key.clone(), ours.clone());
                    }
                    (None, None, Some(theirs)) => {
                        merged.insert(key.clone(), theirs.clone());
                    }
                    _ => {}
                }
            }
            Ok(Value::Object(merged))
        }
        (Value::Array(base), Value::Array(ours), Value::Array(theirs)) => match array_key(base, ours, theirs) {
            Some(key) => merge_keyed_array(base, ours, theirs, key, path),
            None => Err(DocError::Invalid(format!("both workspaces changed {path}"))),
        },
        _ => Err(DocError::Invalid(format!("both workspaces changed {}", if path.is_empty() { "the design" } else { path }))),
    }
}

fn array_key(base: &[Value], ours: &[Value], theirs: &[Value]) -> Option<&'static str> {
    let sides = [base, ours, theirs];
    let usable = |field: &str| sides.iter().all(|side| side.is_empty() || side.iter().all(|item| field_key(item, field).is_some()));
    let present = |field: &str| sides.iter().any(|side| !side.is_empty() && side.iter().all(|item| field_key(item, field).is_some()));
    if present("id") && usable("id") {
        Some("id")
    } else if present("name") && usable("name") {
        Some("name")
    } else {
        None
    }
}

fn field_key(value: &Value, field: &str) -> Option<String> {
    match value.get(field)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn index_by<'a>(items: &'a [Value], key: &str) -> BTreeMap<String, &'a Value> {
    items.iter().filter_map(|item| field_key(item, key).map(|id| (id, item))).collect()
}

fn merge_keyed_array(base: &[Value], ours: &[Value], theirs: &[Value], key: &str, path: &str) -> Result<Value> {
    let base = index_by(base, key);
    let ours_index = index_by(ours, key);
    let theirs_index = index_by(theirs, key);
    let mut order: Vec<String> = ours.iter().filter_map(|item| field_key(item, key)).collect();
    for id in theirs.iter().filter_map(|item| field_key(item, key)) {
        if !order.iter().any(|seen| seen == &id) {
            order.push(id);
        }
    }
    let mut merged = Vec::new();
    for id in order {
        let child = format!("{path} {key} {id}");
        match (base.get(&id), ours_index.get(&id), theirs_index.get(&id)) {
            (_, Some(ours), Some(theirs)) => merged.push(merge_values(base.get(&id).copied().unwrap_or(&Value::Null), ours, theirs, &child)?),
            (Some(base), Some(ours), None) => {
                if **ours != **base {
                    return Err(DocError::Invalid(format!("both workspaces changed {child}")));
                }
            }
            (Some(base), None, Some(theirs)) => {
                if **theirs != **base {
                    return Err(DocError::Invalid(format!("both workspaces changed {child}")));
                }
            }
            (None, Some(ours), None) => merged.push((*ours).clone()),
            (None, None, Some(theirs)) => merged.push((*theirs).clone()),
            (None, None, None) | (Some(_), None, None) => {}
        }
    }
    Ok(Value::Array(merged))
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
    }

    #[test]
    fn rename_delete_and_merge() {
        use crate::Param;
        let mut doc = Document::new("Robot");
        doc.params.push(Param { name: "width".into(), expr: "1".into(), unit: "mm".into(), comment: String::new(), model: false });
        let side = doc.branch_from_here("Side", |_| None).unwrap();
        doc.rename_node(&side, "Trial").unwrap();
        assert!(doc.versions.workspaces.iter().any(|workspace| workspace.name == "Trial"));
        assert!(doc.rename_node("main", "Trial").is_err(), "a name already in use is refused");
        doc.params[0].expr = "2".into();
        doc.switch_workspace("main").unwrap();
        doc.params.push(Param { name: "height".into(), expr: "3".into(), unit: "mm".into(), comment: String::new(), model: false });
        assert_eq!(doc.merge_workspace(&side, &[]).unwrap(), MergeOutcome::Merged);
        assert_eq!(doc.params.iter().find(|param| param.name == "width").unwrap().expr, "2");
        assert!(doc.params.iter().any(|param| param.name == "height"));
        doc.params[0].expr = "9".into();
        doc.switch_workspace(&side).unwrap();
        doc.params[0].expr = "8".into();
        doc.switch_workspace("main").unwrap();
        let changes = doc.changes_for_workspace(&side).unwrap();
        assert!(changes.iter().any(|change| change.path == "param:width" && change.kind == ChangeKind::Conflict));
        assert!(doc.merge_workspace(&side, &[]).is_err(), "the same parameter changed on both sides");
        assert_eq!(doc.params[0].expr, "9", "a refused merge writes nothing");
        assert_eq!(doc.merge_workspace(&side, &[("param:width", true)]).unwrap(), MergeOutcome::Merged);
        assert_eq!(doc.params.iter().find(|param| param.name == "width").unwrap().expr, "8");
        assert!(doc.params.iter().any(|param| param.name == "height"));
        let point = doc.versions.versions[0].id.clone();
        doc.delete_version(&point).unwrap();
        assert!(doc.versions.versions.iter().all(|version| version.id != point));
        doc.switch_workspace(&side).unwrap();
        assert!(doc.delete_workspace(&side).is_err());
        doc.switch_workspace("main").unwrap();
        doc.delete_workspace(&side).unwrap();
        assert_eq!(doc.versions.workspaces.len(), 1);
        assert!(doc.delete_workspace("main").is_err());
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
