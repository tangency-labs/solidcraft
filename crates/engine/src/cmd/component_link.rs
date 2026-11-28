//! Linked components whose bodies come from a design (Insert Design) or a STEP source.
//!
//! The linked design is opened and evaluated on its own, and its solid bodies, as placed in it,
//! join a new component through an Import feature (exact B-rep, kept in this design so it opens
//! without the part file). The link remembers the absolute file in a session and a path relative
//! to the design's folder on disk; Update Links reads it again and replaces the Import feature's
//! bodies, and Break Link keeps the bodies and forgets the file. The bodies' faces are named after the names
//! the design gives them (`Feature::link_faces`), so a joint on a linked face stays on that face
//! when the design changes, as it would inside the design, and reports it when the face is gone.

use solidcraft_geom::prelude::*;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};
use solidcraft_doc::config::RowKey;
use solidcraft_doc::{DesignLink, Document, FeatureKind, LinkedBodyProperties, MeshData, UserMaterial};

use super::CommandSpec;
use super::design_link::{
    LinkState, absolute, inside_design_folder, link_presence, link_target, read_linked_file, real_file, resolve, same_file, stored,
};
use crate::params::{bad, str_, vec3};
use crate::schema::component_link::{
    BreakLinkParams, BreakLinkResult, CopiesParams, CopiesResult, DesignInserted, ExtractParams, ExtractResult, InsertDesignParams,
    InsertDesignResult, JointDeviation, JointDiagnostic, LinkFailure, LinkInfo, LinkSource, LinksParams, LinksResult, MovedOccurrence, RelinkParams,
    RelinkResult, ReplaceWithLinkParams, ReplaceWithLinkResult, UpdateLinksParams, UpdateLinksResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

#[path = "component_extract.rs"]
mod extract;

#[cfg(test)]
#[path = "component_relink_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod relink_tests;

const INSERT_DESIGN_CONTRACT: TypedContract<InsertDesignParams> = TypedContract::new();
const UPDATE_LINKS_CONTRACT: TypedContract<UpdateLinksParams> = TypedContract::new();
const RELINK_CONTRACT: TypedContract<RelinkParams> = TypedContract::new();
const BREAK_LINK_CONTRACT: TypedContract<BreakLinkParams> = TypedContract::new();
const EXTRACT_CONTRACT: TypedContract<ExtractParams> = TypedContract::new();
const COPIES_CONTRACT: TypedContract<CopiesParams> = TypedContract::new();
const REPLACE_WITH_LINK_CONTRACT: TypedContract<ReplaceWithLinkParams> = TypedContract::new();
const LINKS_CONTRACT: TypedContract<LinksParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.insert_design", "Insert Design", crate::checked!("component.insert_design", insert_design)).begins_edit()
        .at("SOLID", "ASSEMBLE")
        .icon("component")
        .contract(&INSERT_DESIGN_CONTRACT)
        .output(output_schema_for::<InsertDesignResult>)
        .params(
        "path: a .sc design, or a file an installed importer plugin reads (format?: its plugin.entry, default the importer that claims the extension; options?: its declared options) → a new component linked to it, holding its solid bodies as placed in that design, its canvases and decals where the design shows them, and a copy of its design properties; \
         document?: the design itself, as its JSON, for a design open in the session that has no file yet: its bodies come in as they are and the link names no file (not with path); \
         configuration?: a row name or id (null or omitted: the saved source state); name? (default: the design's name); parent?: component id or name (default: the active one); \
         translate?: [x,y,z] in mm from the destination parent's origin (default [0,0,0], also with reuse); \
         axis?: [x,y,z], angle?: radians or \"30 deg\", origin?: [x,y,z]; \
         matrix?: [12 or 16 numbers] (row-major 3x4 or 4x4, its translation column numbers or length expressions) or to_matrix?: [12 or 16 numbers]; \
         tolerance?: largest accepted |RᵀR − I| (default 1e-9, at most 1e-3); \
         reuse?: bool (default false): insert a free occurrence, reusing an existing link to the same file and configuration when available",
    ),
    CommandSpec::new("component.update_links", "Update Links", crate::checked!("component.update_links", update_links))
        .icon("update")
        .contract(&UPDATE_LINKS_CONTRACT)
        .output(output_schema_for::<UpdateLinksResult>)
        .params(
        "component?: id or name (default: every linked component); components?: a list of ids or names, which goes on past a design it \
         cannot read (that one keeps its bodies, is named in failed and gets a warning); linked_bodies?: all|folder (a linked design that \
         was itself saved without the bodies of its linked designs: which of those files are read for them; default all; folder: only \
         files in this design's folder or the linked design's own, and a linked design reaching further is not read) — reads the linked design or STEP files again and replaces \
         their bodies where they changed, and their part properties (part number, revision…) where the design changed them and this \
         design did not, as one undo step; → updated, unchanged, failed ([{component, name, error}]), warnings (also \
         features that newly fail and joints whose faces are gone or that no longer solve); a file an importer plugin read is read again by that plugin when its bytes changed, not when they did not, and not at all, with no plugin run, while it is outside the design's folder and its folder is not allowed (Preferences, General); a plugin that is gone, turned off or not the one named keeps the bodies and is named in failed; a KiCad board opened from a .kicad_pcb is read again by SolidCraft with the options it was opened with, replacing the component's bodies, under the same folder rule",
    ),
    CommandSpec::new("component.relink", "Relink Source", crate::checked!("component.relink", relink))
        .icon("update")
        .contract(&RELINK_CONTRACT)
        .output(output_schema_for::<RelinkResult>)
        .params(
        "component: id or name of a linked component; path: a saved design (.sc or .json); configuration?: row name or id, null to follow the saved source state (omitted: keep the selection for the same file, otherwise follow the new file) — replaces the source and linked bodies for all its occurrences, preserving identities and placements; names?: \"keep\" (default) leaves the body names as relinking always did (replaced only when the new source has another number of bodies), \"sync\" takes the new source's body names, renames what other features, materials and appearances keep under the old names with them, and gives the linked feature the source design's name when it still has its component's (component and placement names stay yours; only for the whole component, a placement relinked on its own shows a component made from the new source); occurrence?: one of its placements — when it has others, only that placement comes to show the design, as a placement of the component already linked to it or of a new one under the same parent, keeping its position and joints (component is then the one it shows, previous_component the one it showed); returns path, previous_path, warnings, joint_errors, joint_conflicts, joint_deviations and moved_occurrences",
    ),
    CommandSpec::new("component.break_link", "Break Link", crate::checked!("component.break_link", break_link))
        .icon("break")
        .contract(&BREAK_LINK_CONTRACT)
        .output(output_schema_for::<BreakLinkResult>)
        .params("component: id or name of a linked component — keeps its bodies as they are and forgets the source file, and the importer that read it"),
    CommandSpec::new("component.extract", "Extract to File", crate::checked!("component.extract", extract::extract))
        .icon("export")
        .contract(&EXTRACT_CONTRACT)
        .output(output_schema_for::<ExtractResult>)
        .params(
        "component: id or name of a component that is not linked; path: the .sc design to write → its history, subcomponents and their joints \
         move to that design, and the component becomes linked to it (as component.insert_design makes it); returns path, file, feature, bodies, \
         notices (what stays behind), warnings and copies (components that build the same bodies: see component.replace_with_link)",
    ),
    CommandSpec::new("component.copies", "Find Identical Copies", crate::checked!("component.copies", extract::copies))
        .icon("inspect")
        .noundo()
        .contract(&COPIES_CONTRACT)
        .output(output_schema_for::<CopiesResult>)
        .params(
        "component: id or name of a linked component → copies: [{component, name}], the components not linked whose own history builds the same bodies",
    ),
    CommandSpec::new("component.replace_with_link", "Replace with Link", crate::checked!("component.replace_with_link", extract::replace_with_link))
        .icon("chain")
        .contract(&REPLACE_WITH_LINK_CONTRACT)
        .output(output_schema_for::<ReplaceWithLinkResult>)
        .params(
        "component or components: [ids or names] of components that are not linked; with: a linked component, or path: a .sc design \
         (its link here, else a new linked component) → each component's history goes and its placements become placements of the linked \
         component; refused when its bodies are not the same as the link's; returns component, name, replaced and warnings",
    ),
    CommandSpec::new("component.links", "List Links", crate::checked!("component.links", list_links))
        .icon("chain")
        .noundo()
        .contract(&LINKS_CONTRACT)
        .output(output_schema_for::<LinksResult>)
        .params(
        "→ linked components: component, name, path, importer? ({format, plugin?, version?}: `step`, `kicad` (a board SolidCraft read, with no plugin) or the importer plugin that read the file; for a plugin also available (it is installed and on), installed_version (when it differs or it can't be used) and needs (which plugin and version the link needs and why it can't be used)), configuration (selected row id, omitted when following the saved source state), file (resolved), found (null when the file is on a network location not allowed, \
         or names a device: it is not looked for), changed (whether the file is no longer what this design last read: Update Links \
         reads it again; null when not found or the link is older than this record), then host (the network location); state (missing, \
         unreadable, not_allowed: a network location not allowed or outside the folders this agent may use) and reason when the file \
         can't be reached",
    ),
];

/// Why the file a stored link names may not be reached, from its text alone (`None`: it may): a
/// device, or a network location not allowed (`vfs::design_path_refusal`). Both the link as
/// stored and the file it resolves to are judged, before anything asks the disk or the network.
fn link_refusal(design: Option<&str>, link: &str) -> Option<solidcraft_io::vfs::Remote> {
    let file = resolve(design, link);
    solidcraft_io::vfs::design_path_refusal(link, design).or_else(|| solidcraft_io::vfs::design_path_refusal(&file.to_string_lossy(), design))
}

/// The error for a link [`link_refusal`] refuses.
fn link_refused(design: Option<&str>, link: &str) -> Result<()> {
    solidcraft_io::vfs::check_design_path(link, design).map_err(|e| EngineError::Other(e.to_string()))?;
    let file = resolve(design, link);
    solidcraft_io::vfs::check_design_path(&file.to_string_lossy(), design).map_err(|e| EngineError::Other(e.to_string()))
}

/// Compare an old link with an allowed replacement without probing a refused old source.
/// Repairing a blocked network link to a local file must not contact the old server.
fn same_link_source(s: &Session, link: &DesignLink, file: &Path) -> bool {
    let old = resolve(s.path.as_deref(), &link.path);
    absolute(&old) == file || (link_refused(s.path.as_deref(), &link.path).is_ok() && same_file(&old, file))
}

/// Resolve a design's links (its component links and its drawings' sources) against its original
/// file, before opening it from a different location (for example a recovery file) or keeping it
/// in session history.
pub fn resolve_design_links(doc: &mut Document, from: Option<&str>) {
    super::design_link::located(doc, from);
}

/// Whether `link`, of the component `comp`, has no bodies in `doc`: its Import feature holds
/// neither STEP text nor records, as a file saved without linked bodies keeps it. That holds
/// for a linked design and for a linked STEP file (Insert STEP) alike.
fn left_out(doc: &Document, comp: u64, link: &DesignLink) -> bool {
    link.importer.is_none()
        && doc
            .feature(link.feature)
            .filter(|f| f.component == comp)
            .is_some_and(|f| matches!(&f.kind, FeatureKind::Import { step, records: None, .. } if step.is_empty()))
}

/// The Mesh Import feature of the linked component `comp` in `doc` that holds the mesh bodies of
/// the design `link` names, beside its Import feature ([`set_link_meshes`] makes it). Only a
/// feature that names a design file of the link's file name counts: a mesh someone inserted in
/// the component (an STL, a 3MF) names its own file, and the design would not give it back, so
/// a save without linked bodies keeps it. Update Links leaves the feature's file name as it is
/// when the meshes it reads are the same, so after a relink to a file of another name the
/// meshes stay in the file too, which is the safe way to be wrong.
fn mesh_feature_of(doc: &Document, comp: u64, link: &DesignLink) -> Option<u64> {
    let name = Path::new(&link.path).file_name()?;
    doc.features
        .iter()
        .find(|f| {
            f.component == comp
                && f.id != link.feature
                && matches!(&f.kind, FeatureKind::MeshImport { file, .. }
                    if solidcraft_io::is_design_path(file) && Path::new(file).file_name() == Some(name))
        })
        .map(|f| f.id)
}

/// Whether the linked STEP import of `link` skips no placement, as a flattened import
/// (`flatten: true`) never does: it builds every body where the file places it, with no
/// components below it to show a later placement of a part. The stored skip list says so, and
/// a save without linked bodies keeps it (only the STEP text is left out). A nested import whose
/// file placed no part twice skips nothing either, and is read the same way: should its file now
/// place a part twice, each placement is built where the file puts it, as no component shows it.
fn step_import_flat(doc: &Document, link: &DesignLink) -> bool {
    doc.feature(link.feature).is_some_and(|f| matches!(&f.kind, FeatureKind::Import { skip, .. } if skip.is_empty()))
}

/// `design`, the copy of `doc` a save writes, without the bodies of its linked designs and
/// linked STEP files: each link's Import feature loses its STEP text and its records, the mesh
/// bodies a linked design brings (its Mesh Import feature beside the link) are left out too, and
/// the copy says so ([`Document::linked_bodies_left_out`]), so opening it reads them from the
/// linked files again ([`read_left_out_bodies`]). A link whose file cannot be reached now, or
/// changed since the link read it, keeps its bodies: they may be the only copy
/// ([`link_file_problem`]). `doc` is the design in the session, saved at `from`, whose link
/// paths are whole.
pub(super) fn leave_out_linked_bodies(design: &mut std::borrow::Cow<'_, Document>, doc: &Document, from: Option<&str>) {
    let features: Vec<u64> = doc
        .links
        .iter()
        .filter(|(comp, link)| link.importer.is_none() && link_file_problem(doc, **comp, link, from).is_none())
        .flat_map(|(comp, l)| {
            let meshes = if l.is_design() { mesh_feature_of(doc, *comp, l) } else { None };
            std::iter::once(l.feature).chain(meshes)
        })
        .collect();
    let design = design.to_mut();
    design.linked_bodies_left_out = true;
    for id in features {
        match design.feature_mut(id).map(|f| &mut f.kind) {
            Some(FeatureKind::Import { step, records, .. }) => {
                *step = "".into();
                *records = None;
            }
            Some(FeatureKind::MeshImport { meshes, .. }) => meshes.clear(),
            _ => {}
        }
    }
}

/// Why a save without linked bodies keeps the bodies of `link`, of the component `comp`, in the
/// file all the same: its design file cannot be reached now, or is no longer the file the link
/// read them from, so the file would not give these bodies back. A link older than the record of
/// what it read cannot tell, and keeps them until Update Links makes that record. `None` when it
/// is that file, and for a link whose bodies are gone.
fn link_file_problem(doc: &Document, comp: u64, link: &DesignLink, from: Option<&str>) -> Option<String> {
    let records = match doc.feature(link.feature).filter(|f| f.component == comp).map(|f| &f.kind) {
        Some(FeatureKind::Import { records, .. }) => records.is_some(),
        _ => return None,
    };
    // A STEP file read into records (`reader: cadmpeg`) is read again by SolidCraft's own
    // reader, which gives STEP text, not those records.
    if records && link.step_names.is_some() {
        return Some("its STEP file was read into B-rep records, which reading the file again does not give back".into());
    }
    let file = resolve(from, &link.path);
    if let Some(problem) = link_presence(&file, from) {
        let what = if link.step_names.is_some() { "STEP file" } else { "design" };
        return Some(format!("its linked {what} cannot be read now: {}", problem.reason));
    }
    let Some(kept) = link.file_sha256.as_ref() else {
        return Some(format!("this design has no record of which {} it read; run Update Links, then save again", file.display()));
    };
    (file_sha256(&file, from, link.importer.is_some()).as_ref() != Some(kept))
        .then(|| format!("{} changed since this design read it; run Update Links, then save again", file.display()))
}

/// What a save without linked bodies says of each link that keeps its bodies in the file
/// ([`leave_out_linked_bodies`]).
pub(super) fn kept_linked_bodies(s: &Session) -> Vec<String> {
    let from = s.path.as_deref();
    s.doc
        .links
        .iter()
        .filter(|(comp, link)| link.importer.is_none() && !left_out(&s.doc, **comp, link))
        .filter_map(|(comp, link)| {
            let why = link_file_problem(&s.doc, *comp, link, from)?;
            let name = s.doc.components.iter().find(|c| c.id == *comp).map_or_else(|| comp.to_string(), |c| c.name.clone());
            Some(format!("{name} keeps its bodies in the file: {why}"))
        })
        .collect()
}

/// What one linked design gave when it was read for a link saved without bodies: the parts of
/// its Import feature, its face names, its file's digest and its mesh bodies, or why it could
/// not be read.
type ReadOnce = std::result::Result<
    (
        String,
        FeatureKind,
        Vec<String>,
        Vec<bool>,
        Vec<String>,
        std::sync::Arc<solidcraft_kernel::StepImport>,
        Vec<Vec<String>>,
        String,
        Vec<MeshData>,
    ),
    String,
>;

/// What one linked STEP file gave when it was read for a link saved without bodies: the parts of
/// its Import feature and its file's digest, or why it could not be read.
type StepReadOnce =
    std::result::Result<(String, FeatureKind, Vec<String>, Vec<bool>, Vec<String>, std::sync::Arc<solidcraft_kernel::StepImport>, String), String>;

/// What [`left_out_bodies`] gives for one link: its Import feature as the file gives it now,
/// the face names, whether the file changed since the link read it, and the mesh bodies of a
/// linked design (none for a STEP file).
type LeftOut = (solidcraft_io::StepFeature, Vec<Vec<String>>, bool, Vec<MeshData>);

/// Cache key for a linked design read during one open ([`Reading::read`]): target path,
/// configuration selection, allowed folder root, and current nesting depth (`opening.len()`) so a
/// shallow read is never reused at a depth where nested links would exceed [`OPENING_DEPTH`].
type ReadKey = (PathBuf, Option<(u64, Option<u64>)>, Option<PathBuf>, usize);

/// Cache key for a linked STEP file read during one open: `(absolute_path, flat, step_names)`,
/// so a STEP file linked into many components is read, parsed and hashed once per open.
type StepReadKey = (PathBuf, bool, String);

/// Single-flight, deadlock-free memoization map shared across parallel branches of one open.
///
/// Acyclic diamond dependencies (e.g. `top -> {left, right} -> shared`) wait on the in-flight
/// reader and share its result so the shared part is evaluated exactly once. Both cache waits
/// (`waiting_on`) and nested worker join edges (`join_edges`: parent thread waiting on spawned
/// `map_on_threads` helpers) are tracked under the same lock so any cyclic wait chain—direct or
/// across nested worker pools—is detected before blocking and bypassed cleanly.
struct SharedReads {
    state: Mutex<SharedReadsState>,
    done: std::sync::Condvar,
}

#[derive(Default)]
struct SharedReadsState {
    ready: std::collections::HashMap<ReadKey, ReadOnce>,
    step_ready: std::collections::HashMap<StepReadKey, StepReadOnce>,
    /// Key -> `(owner_thread, owner_pool_depth_when_claimed)`.
    in_flight: std::collections::HashMap<ReadKey, (std::thread::ThreadId, usize)>,
    /// Waiter -> `(owner_thread, owner_claim_depth)`.
    waiting_on: std::collections::HashMap<std::thread::ThreadId, (std::thread::ThreadId, usize)>,
    /// Directed wait edges `parent -> (active child worker, parent pool depth)` while `parent` joins
    /// nested work. A parent claim acquired at depth `d` can only block behind children spawned at
    /// depth `s > d`; claims made inside the parent's own `map_on_threads` job have `d == s` and
    /// release before `parent` joins those siblings.
    join_edges: std::collections::HashMap<std::thread::ThreadId, Vec<(std::thread::ThreadId, usize)>>,
    /// Nesting depth of active `map_on_threads` pools currently executing on each thread.
    pool_depth: std::collections::HashMap<std::thread::ThreadId, usize>,
}

struct PoolDepthGuard {
    shared: SharedReadCache,
    tid: std::thread::ThreadId,
}

impl Drop for PoolDepthGuard {
    fn drop(&mut self) {
        let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(depth) = st.pool_depth.get_mut(&self.tid) {
            *depth = depth.saturating_sub(1);
            if *depth == 0 {
                st.pool_depth.remove(&self.tid);
            }
        }
    }
}

enum ReadAcquire {
    Ready(Box<ReadOnce>),
    Claimed(ReadClaim),
    Bypass,
}

struct ReadClaim {
    shared: SharedReadCache,
    key: Option<ReadKey>,
}

impl ReadClaim {
    fn finish(mut self, value: Option<ReadOnce>) {
        if let Some(key) = self.key.take() {
            let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            st.in_flight.remove(&key);
            if let Some(val) = value {
                st.ready.insert(key, val);
            }
            self.shared.done.notify_all();
        }
    }
}

impl Drop for ReadClaim {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            st.in_flight.remove(&key);
            self.shared.done.notify_all();
        }
    }
}

impl SharedReads {
    fn new() -> SharedReadCache {
        std::sync::Arc::new(SharedReads { state: Mutex::new(SharedReadsState::default()), done: std::sync::Condvar::new() })
    }

    fn get_ready(&self, key: &ReadKey) -> Option<ReadOnce> {
        self.state.lock().ok().and_then(|st| st.ready.get(key).cloned())
    }

    fn get_step_ready(&self, key: &StepReadKey) -> Option<StepReadOnce> {
        self.state.lock().ok().and_then(|st| st.step_ready.get(key).cloned())
    }

    fn insert_step_ready(&self, key: StepReadKey, val: StepReadOnce) {
        if let Ok(mut st) = self.state.lock() {
            st.step_ready.insert(key, val);
        }
    }

    fn remove_ready(&self, key: &ReadKey) {
        if let Ok(mut st) = self.state.lock() {
            st.ready.remove(key);
        }
    }

    fn enter_pool(self: &SharedReadCache, parent: std::thread::ThreadId) -> (usize, PoolDepthGuard) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let entry = st.pool_depth.entry(parent).or_insert(0);
        *entry += 1;
        (*entry, PoolDepthGuard { shared: self.clone(), tid: parent })
    }

    fn register_join(&self, parent: std::thread::ThreadId, child: std::thread::ThreadId, pool_depth: usize) {
        if parent != child {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.join_edges.entry(parent).or_default().push((child, pool_depth));
        }
    }

    fn unregister_join(&self, parent: std::thread::ThreadId, child: std::thread::ThreadId, pool_depth: usize) {
        if parent != child {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(children) = st.join_edges.get_mut(&parent) {
                if let Some(pos) = children.iter().position(|&e| e == (child, pool_depth)) {
                    children.swap_remove(pos);
                }
                if children.is_empty() {
                    st.join_edges.remove(&parent);
                }
            }
        }
    }

    /// Whether `me` is reachable from `(owner, owner_claim_depth)` in the combined wait-for graph
    /// (`waiting_on` U depth-filtered `join_edges`), meaning waiting on `owner` would form a
    /// deadlock cycle.
    fn would_cycle(st: &SharedReadsState, owner: std::thread::ThreadId, owner_claim_depth: usize, me: std::thread::ThreadId) -> bool {
        let mut stack = vec![(owner, owner_claim_depth)];
        let mut min_seen: std::collections::HashMap<std::thread::ThreadId, usize> = std::collections::HashMap::new();
        while let Some((cur, min_depth)) = stack.pop() {
            if cur == me {
                return true;
            }
            if min_seen.get(&cur).is_some_and(|&prev| prev <= min_depth) {
                continue;
            }
            min_seen.insert(cur, min_depth);
            if let Some(&(next, next_depth)) = st.waiting_on.get(&cur) {
                stack.push((next, next_depth));
            }
            if let Some(children) = st.join_edges.get(&cur) {
                for &(child, spawn_depth) in children {
                    if spawn_depth > min_depth {
                        stack.push((child, 0));
                    }
                }
            }
        }
        false
    }

    fn acquire(self: &SharedReadCache, key: &ReadKey) -> ReadAcquire {
        let me = std::thread::current().id();
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(val) = st.ready.get(key) {
                return ReadAcquire::Ready(Box::new(val.clone()));
            }
            if let Some(&(owner, owner_depth)) = st.in_flight.get(key) {
                if Self::would_cycle(&st, owner, owner_depth, me) {
                    return ReadAcquire::Bypass;
                }
                st.waiting_on.insert(me, (owner, owner_depth));
                st = self.done.wait(st).unwrap_or_else(|e| e.into_inner());
                st.waiting_on.remove(&me);
                continue;
            }
            let my_depth = st.pool_depth.get(&me).copied().unwrap_or(0);
            st.in_flight.insert(key.clone(), (me, my_depth));
            return ReadAcquire::Claimed(ReadClaim { shared: self.clone(), key: Some(key.clone()) });
        }
    }
}

type SharedReadCache = std::sync::Arc<SharedReads>;

/// Maximum worker threads used when evaluating independent linked files concurrently.
const MAX_LINK_WORKERS: usize = 16;

fn link_cores() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(MAX_LINK_WORKERS)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// The reads of linked designs saved without bodies that one open, or one Update Links, makes
/// on this thread.
struct Reading {
    /// The design files being opened, outermost first (`None`: a design not saved yet). A design
    /// saved without linked bodies reads its linked designs as it opens, and each of those may do
    /// the same, so a chain of links that returns to one of these files would never end.
    opening: Vec<Option<PathBuf>>,
    /// Which files the outermost caller lets these reads take ([`LinkedRead`]); every design
    /// opened below it follows the same rule.
    which: LinkedRead,
    /// How many links were not read because of where they are in `opening`: one that returns to
    /// a file being opened, or one nested too deep. What a design gives after such a refusal
    /// holds for this chain of links only.
    refused: usize,
    /// How many links were not read because they are outside the folders `which` allows. Those
    /// folders are the same for every chain below one link of the outermost design
    /// ([`may_read`]), so what a design gives after such a refusal is kept for that link.
    outside: usize,
    /// How many reads were not kept in `read` because of a refusal below them. Each may be made
    /// again by another chain of links, so past [`READ_AGAIN`] no further design is read.
    again: usize,
    /// Worker thread budget for parallelizing links at or below this level, preventing nested
    /// thread explosion across sub-assembly hierarchies.
    budget: usize,
    /// What each linked design gave, by its file and configuration, so that a design many links
    /// lead to is read once: without it the reads grow with the number of ways to reach it. With
    /// [`LinkedRead::Folder`] a design below a link of the outermost design is kept for that link
    /// (the key's last part), whose folder decides what it may read.
    read: Option<SharedReadCache>,
}

impl Reading {
    fn empty() -> Self {
        Reading { opening: Vec::new(), which: LinkedRead::All, refused: 0, outside: 0, again: 0, budget: 1, read: None }
    }
}

thread_local! {
    static READING: std::cell::RefCell<Reading> = std::cell::RefCell::new(Reading::empty());
}

fn reading_read_get(key: &ReadKey) -> Option<ReadOnce> {
    READING.with(|reading| reading.borrow().read.clone())?.get_ready(key)
}

fn reading_read_acquire(key: &ReadKey) -> ReadAcquire {
    match READING.with(|reading| reading.borrow().read.clone()) {
        Some(cache) => cache.acquire(key),
        None => ReadAcquire::Bypass,
    }
}

fn reading_step_get(key: &StepReadKey) -> Option<StepReadOnce> {
    READING.with(|reading| reading.borrow().read.clone())?.get_step_ready(key)
}

fn reading_step_insert(key: StepReadKey, val: StepReadOnce) {
    if let Some(cache) = READING.with(|reading| reading.borrow().read.clone()) {
        cache.insert_step_ready(key, val);
    }
}

/// Temporarily installs a branch [`Reading`] snapshot on a worker thread (preserving `opening`,
/// `which`, shared memoization cache, and bounded `budget`), records the parent-to-child join
/// wait edge in [`SharedReads`] for deadlock detection, and restores the thread's prior
/// [`Reading`] state on drop — essential because `map_on_threads` also executes work on the
/// calling thread.
struct BranchReading {
    saved: Option<Reading>,
    join_edge: Option<(SharedReadCache, std::thread::ThreadId, std::thread::ThreadId, usize)>,
}

impl BranchReading {
    fn enter(
        opening: Vec<Option<PathBuf>>,
        which: LinkedRead,
        budget: usize,
        read: Option<SharedReadCache>,
        parent_tid: std::thread::ThreadId,
        pool_depth: usize,
    ) -> Self {
        let me = std::thread::current().id();
        let join_edge = read.as_ref().filter(|_| me != parent_tid && pool_depth > 0).map(|cache| {
            cache.register_join(parent_tid, me, pool_depth);
            (cache.clone(), parent_tid, me, pool_depth)
        });
        let branch = Reading { opening, which, refused: 0, outside: 0, again: 0, budget, read };
        let saved = READING.with(|r| r.replace(branch));
        BranchReading { saved: Some(saved), join_edge }
    }

    fn finish(mut self) -> (usize, usize, usize) {
        let stats = READING.with(|r| {
            let b = r.borrow();
            (b.refused, b.outside, b.again)
        });
        if let Some((cache, parent, child, pool_depth)) = self.join_edge.take() {
            cache.unregister_join(parent, child, pool_depth);
        }
        if let Some(saved) = self.saved.take() {
            READING.with(|r| *r.borrow_mut() = saved);
        }
        stats
    }
}

impl Drop for BranchReading {
    fn drop(&mut self) {
        if let Some((cache, parent, child, pool_depth)) = self.join_edge.take() {
            cache.unregister_join(parent, child, pool_depth);
        }
        if let Some(saved) = self.saved.take() {
            READING.with(|r| *r.borrow_mut() = saved);
        }
    }
}

#[cfg(test)]
fn measure_eval_counts<R>(f: impl FnOnce() -> R) -> (R, usize, usize, usize) {
    let saved_e = EVALUATED.with(|n| n.replace(0));
    let saved_r = REUSED.with(|n| n.replace(0));
    let saved_s = STEP_READS.with(|n| n.replace(0));
    struct Restore(usize, usize, usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            EVALUATED.with(|n| n.set(self.0));
            REUSED.with(|n| n.set(self.1));
            STEP_READS.with(|n| n.set(self.2));
        }
    }
    let _restore = Restore(saved_e, saved_r, saved_s);
    let out = f();
    let e = EVALUATED.with(std::cell::Cell::get);
    let r = REUSED.with(std::cell::Cell::get);
    let s = STEP_READS.with(std::cell::Cell::get);
    (out, e, r, s)
}

#[cfg(test)]
fn add_eval_counts(evaluated: usize, reused: usize, step_reads: usize) {
    EVALUATED.with(|n| n.set(n.get() + evaluated));
    REUSED.with(|n| n.set(n.get() + reused));
    STEP_READS.with(|n| n.set(n.get() + step_reads));
}

/// How many designs deep [`read_left_out_bodies`] follows links saved without bodies.
const OPENING_DEPTH: usize = 32;

/// How many reads one open makes that it cannot keep for the next chain of links
/// ([`Reading::again`]): those with a link below them that returns to a design being opened, or
/// is nested too deep. Designs that link each other in a loop, each by several links, would
/// otherwise be read once for every way round it. A design without such a link is not counted.
const READ_AGAIN: usize = 64;

/// One entry of [`Reading::opening`], taken off when it goes out of scope.
pub(super) struct Opening;

impl Opening {
    /// `file` is being opened, or has its links read again. The outermost entry sets which files
    /// the reads below it may take; `which` of an entry inside another is not used.
    pub(super) fn enter(file: Option<&str>, which: LinkedRead) -> Opening {
        READING.with(|reading| {
            let mut reading = reading.borrow_mut();
            if reading.opening.is_empty() {
                reading.which = which;
                reading.refused = 0;
                reading.outside = 0;
                reading.again = 0;
                reading.budget = link_cores();
                reading.read = Some(SharedReads::new());
            }
            reading.opening.push(file.map(|file| absolute(Path::new(file))));
        });
        Opening
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        READING.with(|reading| {
            let mut reading = reading.borrow_mut();
            reading.opening.pop();
            if reading.opening.is_empty() {
                reading.read = None;
                reading.budget = 1;
            }
        });
    }
}

/// Whether the reads under way may take `file` for a link saved without bodies. With
/// [`LinkedRead::Folder`] it is in the folder of the outermost design, or in the folder of the
/// linked design that design reads: the caller chose to read that one, and a design below it
/// reaches no further than the two.
fn may_read(file: &Path) -> bool {
    READING.with(|reading| {
        let reading = reading.borrow();
        reading.which == LinkedRead::All
            || reading.opening.iter().take(2).flatten().any(|design| inside_design_folder(Some(&design.to_string_lossy()), file))
    })
}

/// Which linked design files a design saved without linked bodies reads as it opens
/// ([`read_left_out_bodies`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkedRead {
    /// Every link's file the caller may read.
    All,
    /// Only files in the design's own folder or below it. A design received from someone else
    /// names what files it likes, and one reaching out of its folder would copy another design
    /// of the user's into it unasked: the app asks before it reads those. A linked design that
    /// was itself saved without linked bodies reads its own links from that folder or its own
    /// ([`may_read`]).
    Folder,
}

impl LinkedRead {
    /// `linked_bodies` of `cmd` (`doc.open`, `component.update_links`): `all` (the default) or
    /// `folder`.
    pub(super) fn read(p: &Value, cmd: &str) -> Result<LinkedRead> {
        match p.get("linked_bodies") {
            None | Some(Value::Null) => Ok(LinkedRead::All),
            Some(Value::String(word)) if word == "all" => Ok(LinkedRead::All),
            Some(Value::String(word)) if word == "folder" => Ok(LinkedRead::Folder),
            Some(_) => Err(bad(cmd, "`linked_bodies` must be all or folder")),
        }
    }
}

/// Read the bodies of `doc`'s links that its file was saved without ([`left_out`]) from their
/// linked design files, before `doc`, opened from `from`, is evaluated. Each link takes the
/// bodies and face names its file gives now, as Update Links takes them. Returns what to tell
/// the user: a link whose file is not read (`which`, or the rule of the open this one is part
/// of) or cannot be read keeps no bodies, and its feature says so when it is evaluated; a file that changed since the design was saved is
/// named, because Update Links still has its materials and properties to take in. `doc`'s link
/// paths are whole.
pub(super) fn read_left_out_bodies(
    doc: &mut Document,
    from: Option<&str>,
    validation: solidcraft_kernel::Validation,
    which: LinkedRead,
) -> Vec<String> {
    let links: Vec<(u64, DesignLink)> = doc.links.iter().filter(|(comp, link)| left_out(doc, **comp, link)).map(|(c, l)| (*c, l.clone())).collect();
    if links.is_empty() {
        return Vec::new();
    }
    let _opening = Opening::enter(from, which);
    let mut prefetched_steps: std::collections::HashMap<usize, StepReadOnce> = std::collections::HashMap::new();
    let mut prefetched_designs: std::collections::HashMap<usize, ReadOnce> = std::collections::HashMap::new();
    let (budget, parent_opening, parent_which, parent_refused, parent_again, shared_read) = READING.with(|reading| {
        let r = reading.borrow();
        (r.budget, r.opening.clone(), r.which, r.refused, r.again, r.read.clone())
    });
    if links.len() > 1 && budget > 1 && parent_refused == 0 && parent_again == 0 && !solidcraft_doc::progress::cancelled() {
        enum LeftOutJob {
            Step { link_indices: Vec<usize>, key: StepReadKey, file: PathBuf, flat: bool, step_names: String },
            Design { link_indices: Vec<usize>, key: ReadKey, file: PathBuf, configuration: Option<SelectedConfiguration> },
        }
        enum LeftOutJobOut {
            Cancelled,
            Step {
                link_indices: Vec<usize>,
                key: StepReadKey,
                read: StepReadOnce,
                #[cfg(test)]
                evals: usize,
                #[cfg(test)]
                reuses: usize,
                #[cfg(test)]
                step_reads: usize,
            },
            Design {
                link_indices: Vec<usize>,
                key: ReadKey,
                read: ReadOnce,
                refused: usize,
                outside: usize,
                again: usize,
                #[cfg(test)]
                evals: usize,
                #[cfg(test)]
                reuses: usize,
                #[cfg(test)]
                step_reads: usize,
            },
        }
        let allowed_by = (parent_which == LinkedRead::Folder).then(|| parent_opening.get(1).cloned().flatten()).flatten();
        let depth = parent_opening.len();
        let mut jobs: Vec<LeftOutJob> = Vec::new();
        let mut step_job_by_key: std::collections::HashMap<StepReadKey, usize> = std::collections::HashMap::new();
        let mut design_job_by_key: std::collections::HashMap<ReadKey, usize> = std::collections::HashMap::new();
        for (link_idx, (_comp, link)) in links.iter().enumerate() {
            let file = resolve(from, &link.path);
            if !may_read(&file) || link_refused(from, &link.path).is_err() {
                continue;
            }
            if let Some(step_names) = &link.step_names {
                let flat = step_import_flat(doc, link);
                let key: StepReadKey = (absolute(&file), flat, step_names.clone());
                if reading_step_get(&key).is_some() {
                    continue;
                }
                if let Some(&job_idx) = step_job_by_key.get(&key) {
                    if let LeftOutJob::Step { link_indices, .. } = &mut jobs[job_idx] {
                        link_indices.push(link_idx);
                    }
                } else {
                    let job_idx = jobs.len();
                    step_job_by_key.insert(key.clone(), job_idx);
                    jobs.push(LeftOutJob::Step { link_indices: vec![link_idx], key, file, flat, step_names: step_names.clone() });
                }
                continue;
            }
            let target = absolute(&file);
            let is_cycle = parent_opening.iter().flatten().any(|open| *open == target || same_file(open, &target));
            if is_cycle || depth >= OPENING_DEPTH {
                continue;
            }
            let configuration = selected_configuration(link);
            let key: ReadKey = (target, configuration.map(|c| (c.row, c.identity)), allowed_by.clone(), depth);
            if reading_read_get(&key).is_some() {
                continue;
            }
            if let Some(&job_idx) = design_job_by_key.get(&key) {
                if let LeftOutJob::Design { link_indices, .. } = &mut jobs[job_idx] {
                    link_indices.push(link_idx);
                }
            } else {
                let job_idx = jobs.len();
                design_job_by_key.insert(key.clone(), job_idx);
                jobs.push(LeftOutJob::Design { link_indices: vec![link_idx], key, file, configuration });
            }
        }
        if jobs.len() > 1 {
            let workers = budget.min(jobs.len());
            let child_budget = (budget / workers).max(1);
            let progress = solidcraft_doc::progress::current();
            let parent_tid = std::thread::current().id();
            let allowed_hosts = solidcraft_io::vfs::current_allowed_hosts();
            // Workers start unconfined: carry the caller's file scope so linked reads stay inside it.
            let scope = solidcraft_io::vfs::current_scope();
            let (pool_depth, _pool_guard) = shared_read.as_ref().map_or((0, None), |c| {
                let (d, g) = c.enter_pool(parent_tid);
                (d, Some(g))
            });
            let done = solidcraft_kernel::map_on_threads(&jobs, workers, |job| -> LeftOutJobOut {
                solidcraft_io::vfs::scoped(scope.clone(), || {
                    solidcraft_io::vfs::allowing_hosts(&allowed_hosts, || {
                        solidcraft_doc::progress::watch_nested(progress.clone(), || {
                            if solidcraft_doc::progress::cancelled() || solidcraft_kernel::watch::cancelled() {
                                return LeftOutJobOut::Cancelled;
                            }
                            match job {
                                LeftOutJob::Step { link_indices, key, file, flat, step_names } => {
                                    let run = || read_step_once(file, from, step_names, *flat, validation);
                                    #[cfg(test)]
                                    let (read, evals, reuses, step_reads) = measure_eval_counts(run);
                                    #[cfg(not(test))]
                                    let read = run();
                                    LeftOutJobOut::Step {
                                        link_indices: link_indices.clone(),
                                        key: key.clone(),
                                        read,
                                        #[cfg(test)]
                                        evals,
                                        #[cfg(test)]
                                        reuses,
                                        #[cfg(test)]
                                        step_reads,
                                    }
                                }
                                LeftOutJob::Design { link_indices, key, file, configuration } => {
                                    let branch = BranchReading::enter(
                                        parent_opening.clone(),
                                        parent_which,
                                        child_budget,
                                        shared_read.clone(),
                                        parent_tid,
                                        pool_depth,
                                    );
                                    let (read, evals, reuses, step_reads) = match reading_read_acquire(key) {
                                        ReadAcquire::Ready(read) => (*read, 0usize, 0usize, 0usize),
                                        ReadAcquire::Claimed(claim) => {
                                            let run = || read_once(file, from, *configuration, validation);
                                            #[cfg(test)]
                                            let (read, evals, reuses, step_reads) = measure_eval_counts(run);
                                            #[cfg(not(test))]
                                            let (read, evals, reuses, step_reads) = (run(), 0usize, 0usize, 0usize);
                                            let refused_here = READING.with(|r| r.borrow().refused);
                                            claim.finish((refused_here == 0).then(|| read.clone()));
                                            (read, evals, reuses, step_reads)
                                        }
                                        ReadAcquire::Bypass => {
                                            let run = || read_once(file, from, *configuration, validation);
                                            #[cfg(test)]
                                            let (read, evals, reuses, step_reads) = measure_eval_counts(run);
                                            #[cfg(not(test))]
                                            let (read, evals, reuses, step_reads) = (run(), 0usize, 0usize, 0usize);
                                            (read, evals, reuses, step_reads)
                                        }
                                    };
                                    #[cfg(not(test))]
                                    let _ = (evals, reuses, step_reads);
                                    let (refused, outside, again) = branch.finish();
                                    LeftOutJobOut::Design {
                                        link_indices: link_indices.clone(),
                                        key: key.clone(),
                                        read,
                                        refused,
                                        outside,
                                        again,
                                        #[cfg(test)]
                                        evals,
                                        #[cfg(test)]
                                        reuses,
                                        #[cfg(test)]
                                        step_reads,
                                    }
                                }
                            }
                        })
                    })
                })
            });
            // Merge branch outcomes in deterministic link order.
            for out in done.into_iter().flatten() {
                match out {
                    LeftOutJobOut::Cancelled => {}
                    LeftOutJobOut::Step {
                        link_indices,
                        key,
                        read,
                        #[cfg(test)]
                        evals,
                        #[cfg(test)]
                        reuses,
                        #[cfg(test)]
                        step_reads,
                    } => {
                        #[cfg(test)]
                        add_eval_counts(evals, reuses, step_reads);
                        reading_step_insert(key, read.clone());
                        for idx in link_indices {
                            prefetched_steps.insert(idx, read.clone());
                        }
                    }
                    LeftOutJobOut::Design {
                        link_indices,
                        key,
                        read,
                        refused,
                        outside,
                        again,
                        #[cfg(test)]
                        evals,
                        #[cfg(test)]
                        reuses,
                        #[cfg(test)]
                        step_reads,
                    } => {
                        #[cfg(test)]
                        add_eval_counts(evals, reuses, step_reads);
                        let within_limit = READING.with(|reading| {
                            let mut r = reading.borrow_mut();
                            if r.again >= READ_AGAIN {
                                // Evict speculative worker entries past the sequential retry
                                // cutoff so fallback calls enforce `READ_AGAIN` identically.
                                if let Some(cache) = &r.read {
                                    cache.remove_ready(&key);
                                }
                                return false;
                            }
                            r.refused += refused;
                            r.outside += outside;
                            r.again += again;
                            if refused != 0 {
                                r.again += 1;
                            }
                            true
                        });
                        if within_limit {
                            for idx in link_indices {
                                prefetched_designs.insert(idx, read.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    let mut warnings = Vec::new();
    for (link_idx, (comp, link)) in links.into_iter().enumerate() {
        if solidcraft_doc::progress::cancelled() {
            break;
        }
        let name = doc.components.iter().find(|c| c.id == comp).map_or_else(|| comp.to_string(), |c| c.name.clone());
        if !may_read(&resolve(from, &link.path)) {
            READING.with(|reading| reading.borrow_mut().outside += 1);
            let what = if link.step_names.is_some() { "STEP file" } else { "design" };
            warnings.push(format!("{name} has no bodies yet: its linked {what} is in another folder, and Update Links reads it"));
            continue;
        }
        let read_result = if let Some(step_res) = prefetched_steps.remove(&link_idx) {
            step_res.map_err(EngineError::Other).map(|(n, kind, body_names, invisible, w, import, digest)| {
                let changed = link.file_sha256.as_deref().is_some_and(|kept| kept != digest);
                (solidcraft_io::StepFeature { name: n, kind, body_names, invisible, warnings: w, import }, Vec::new(), changed, Vec::new())
            })
        } else if let Some(design_res) = prefetched_designs.remove(&link_idx) {
            design_res.map_err(EngineError::Other).map(|(n, kind, body_names, invisible, w, import, faces, digest, meshes)| {
                let changed = link.file_sha256.as_deref().is_some_and(|kept| kept != digest);
                (solidcraft_io::StepFeature { name: n, kind, body_names, invisible, warnings: w, import }, faces, changed, meshes)
            })
        } else {
            left_out_bodies(&link, from, step_import_flat(doc, &link), validation)
        };
        let (feature, faces, changed, meshes) = match read_result {
            Ok(read) => read,
            Err(e) => {
                warnings.push(format!("{name} has no bodies: {e}"));
                continue;
            }
        };
        let step_file = link.step_names.is_some();
        if changed {
            // What an unchanged file's read warns of was said when the link took it in.
            warnings.extend(feature.warnings.iter().cloned());
            warnings.push(if step_file {
                format!("{name}: its linked STEP file changed since this design was saved; its bodies are the new ones, and Update Links records that this design read it")
            } else {
                format!(
                    "{name}: its linked design changed since this design was saved; its bodies are the new ones, and Update Links takes in its materials and properties"
                )
            });
        }
        let mut realigned = None;
        let other_bodies: std::collections::HashSet<String> = doc
            .features
            .iter()
            .filter(|other| other.id != link.feature)
            .flat_map(|other| other.body_names.iter().filter(|n| !n.is_empty()).cloned())
            .collect();
        let mut unpinned_inherited = if !step_file { realign_unpinned_inherited(doc, &link.inherited, &feature.body_names) } else { None };
        if let Some(f) = doc.feature_mut(link.feature) {
            match (&mut f.kind, feature.kind) {
                // An unchanged STEP file gives back the text it was saved without; the tree,
                // the skipped placements and the body names stay as the design saved them.
                (FeatureKind::Import { step, .. }, FeatureKind::Import { step: read, .. }) if step_file && !changed => *step = read,
                (_, kind) => {
                    // Bodies keep their names where the file still has as many.
                    if f.body_names.len() != feature.body_names.len() {
                        if f.body_names.is_empty() {
                            f.body_names = feature.body_names;
                            if let Some(next_inherited) = unpinned_inherited.take() {
                                realigned = Some((next_inherited, Vec::new()));
                            }
                        } else {
                            let (next_names, next_inherited, removed) = realign_changed_body_count(
                                "",
                                "",
                                solidcraft_io::StepNames::default(),
                                &f.body_names,
                                &link.inherited,
                                &feature.body_names,
                                &other_bodies,
                            );
                            f.body_names = next_names;
                            realigned = Some((next_inherited, removed));
                        }
                    }
                    f.kind = kind;
                    f.link_faces = faces;
                }
            }
        }
        if let Some((next_inherited, removed)) = realigned {
            if !step_file && let Some(kept) = doc.links.get_mut(&comp) {
                kept.inherited = next_inherited;
            }
            remove_stale_body_properties(doc, link.feature, &removed, &other_bodies);
        }
        // The mesh bodies a linked design brings, left out with its solids. A design that has
        // mesh bodies only since this one was saved has no Mesh Import feature here: the link
        // is reported as changed, and Update Links brings them in. One that has none left loses
        // the feature, as Update Links deletes it ([`set_link_meshes`]): left empty, Update Links
        // would find nothing changed and keep it.
        if !step_file && let Some(id) = mesh_feature_of(doc, comp, &link) {
            match doc.feature_mut(id).map(|f| &mut f.kind) {
                Some(FeatureKind::MeshImport { meshes: held, .. }) if held.is_empty() && !meshes.is_empty() => *held = meshes,
                Some(FeatureKind::MeshImport { meshes: held, .. }) if held.is_empty() => {
                    if let Err(e) = doc.delete_feature(id) {
                        warnings.push(format!("{name}: its design has no mesh bodies now, and its empty Mesh Import feature stays: {e}"));
                    }
                }
                _ => {}
            }
        }
    }
    warnings
}

/// The Import feature and face names the file of `link` gives now, and whether that file
/// changed since the link last read it, for a design opened from `from`; `flat` as in [`source_bodies`].
fn left_out_bodies(link: &DesignLink, from: Option<&str>, flat: bool, validation: solidcraft_kernel::Validation) -> Result<LeftOut> {
    link_refused(from, &link.path)?;
    let file = resolve(from, &link.path);
    if link.step_names.is_some() {
        // A STEP file links nothing, so it needs none of the guards against loops below.
        return left_out_step(link, &file, from, flat, validation);
    }
    let target = absolute(&file);
    let (depth, again) = READING.with(|reading| {
        let reading = reading.borrow();
        (reading.opening.len(), reading.opening.iter().flatten().any(|open| *open == target || same_file(open, &target)))
    });
    let refuse = |why: String| {
        READING.with(|reading| reading.borrow_mut().refused += 1);
        Err(EngineError::Other(why))
    };
    if again {
        return refuse(format!("{}: it links back to a design that links it, so its bodies cannot be read", file.display()));
    }
    if depth >= OPENING_DEPTH {
        return refuse(format!("{}: linked designs saved without bodies are nested more than {OPENING_DEPTH} deep", file.display()));
    }
    let configuration = selected_configuration(link);
    let allowed_by = READING.with(|reading| {
        let reading = reading.borrow();
        (reading.which == LinkedRead::Folder).then(|| reading.opening.get(1).cloned().flatten()).flatten()
    });
    let key = (target, configuration.map(|c| (c.row, c.identity)), allowed_by, depth);
    let read = match reading_read_acquire(&key) {
        ReadAcquire::Ready(read) => *read,
        acquired => {
            let (refused, again) = READING.with(|reading| {
                let reading = reading.borrow();
                (reading.refused, reading.again)
            });
            if again >= READ_AGAIN {
                drop(acquired);
                return refuse(format!(
                    "{}: the linked designs saved without bodies link each other in too many ways to read them all; Update Links reads it once the loop is gone",
                    file.display()
                ));
            }
            let read = read_once(&file, from, configuration, validation);
            // What a design gives after a link below it was refused for its place in this chain
            // is not what another chain gets.
            let cacheable = READING.with(|reading| {
                let mut reading = reading.borrow_mut();
                if reading.refused == refused {
                    true
                } else {
                    reading.again += 1;
                    false
                }
            });
            if let ReadAcquire::Claimed(claim) = acquired {
                claim.finish(cacheable.then(|| read.clone()));
            }
            read
        }
    };
    let (name, kind, body_names, invisible, warnings, import, faces, digest, meshes) = read.map_err(EngineError::Other)?;
    let changed = link.file_sha256.as_deref().is_some_and(|kept| kept != digest);
    Ok((solidcraft_io::StepFeature { name, kind, body_names, invisible, warnings, import }, faces, changed, meshes))
}

/// [`left_out_step`]'s single read of a linked STEP file at `file`, for a design opened from `from`.
fn read_step_once(file: &Path, from: Option<&str>, step_names: &str, flat: bool, validation: solidcraft_kernel::Validation) -> StepReadOnce {
    let bytes = read_link(file, from, false).map_err(|e| e.to_string())?;
    let probe = DesignLink { step_names: Some(step_names.to_string()), ..Default::default() };
    let ((_, feature, ..), _) = source_bodies(file, &bytes, &probe, flat, validation).map_err(|e| e.to_string())?;
    let solidcraft_io::StepFeature { name, kind, body_names, invisible, warnings, import } = feature;
    Ok((name, kind, body_names, invisible, warnings, import, DesignLink::file_digest(&bytes)))
}

/// [`left_out_bodies`] for a linked STEP file at `file`: read again as Update Links reads it
/// ([`source_bodies`]), with the link's naming rule; `flat` as there.
fn left_out_step(link: &DesignLink, file: &Path, from: Option<&str>, flat: bool, validation: solidcraft_kernel::Validation) -> Result<LeftOut> {
    let step_names = link.step_names.as_deref().unwrap_or_default();
    let key = (absolute(file), flat, step_names.to_string());
    let read = match reading_step_get(&key) {
        Some(read) => read,
        None => {
            let read = read_step_once(file, from, step_names, flat, validation);
            reading_step_insert(key, read.clone());
            read
        }
    };
    let (name, kind, body_names, invisible, warnings, import, digest) = read.map_err(EngineError::Other)?;
    let changed = link.file_sha256.as_deref().is_some_and(|kept| kept != digest);
    Ok((solidcraft_io::StepFeature { name, kind, body_names, invisible, warnings, import }, Vec::new(), changed, Vec::new()))
}

/// [`left_out_bodies`]' read of the design at `file`, for a design opened from `from`.
fn read_once(file: &Path, from: Option<&str>, configuration: Option<SelectedConfiguration>, validation: solidcraft_kernel::Validation) -> ReadOnce {
    let bytes = read_link(file, from, false).map_err(|e| e.to_string())?;
    let ((_, feature, faces, _, _, meshes), _) =
        design_bodies_reusing(&Source::File(file.to_path_buf()), &bytes, configuration, validation, true).map_err(|e| e.to_string())?;
    let solidcraft_io::StepFeature { name, kind, body_names, invisible, warnings, import } = feature;
    Ok((name, kind, body_names, invisible, warnings, import, faces, DesignLink::file_digest(&bytes), meshes))
}

/// The bodies of a linked design: its name, the Import feature that holds its solids and the
/// names the design gives their faces (`Feature::link_faces`), with the source's design
/// properties, and its mesh bodies as placed in it.
type Linked = (String, solidcraft_io::StepFeature, Vec<Vec<String>>, solidcraft_doc::Metadata, Vec<SourceProperties>, Vec<MeshData>);

/// A linked design's canvases and decals, each on a plane in the design's own coordinates (the
/// frame its linked bodies come in), with what could not be carried as it is.
type LinkedImages = (Vec<solidcraft_doc::canvas::Canvas>, Vec<String>);

#[derive(Clone)]
struct SourceProperties {
    material: String,
    definition: Option<UserMaterial>,
    appearance: Option<solidcraft_doc::appearance::Look>,
    /// Resolved authored overrides, by the face index in this imported body.
    faces: Vec<(usize, solidcraft_doc::appearance::Look)>,
    threads: Vec<solidcraft_doc::ThreadInfo>,
}

/// What a linked file last gave, by the file: digests of its bytes, of the STEP text it gave and
/// of its face names, and that read's warnings and source properties. Update Links evaluates
/// a file again only when its bytes or the linked bodies changed since (#5606). Bounded, oldest first out.
static READS: Mutex<Vec<LinkRead>> = Mutex::new(Vec::new());
const READS_SIZE: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SelectedConfiguration {
    row: u64,
    identity: Option<u64>,
}

fn selected_configuration(link: &DesignLink) -> Option<SelectedConfiguration> {
    link.configuration.map(|row| SelectedConfiguration { row, identity: link.configuration_identity })
}

struct LinkRead {
    file: PathBuf,
    configuration: Option<SelectedConfiguration>,
    bytes: u64,
    step: u64,
    faces: u64,
    warnings: Vec<String>,
    properties: Vec<SourceProperties>,
    metadata: solidcraft_doc::Metadata,
    /// The mesh bodies it gave. They share their arrays with the link that took them, so the
    /// check that the link still holds them is cheap.
    meshes: Vec<MeshData>,
}

#[cfg(test)]
thread_local! {
    /// Linked designs read and evaluated on this thread, for tests.
    static EVALUATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Of those, the ones whose evaluation came from the local store, for tests.
    static REUSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Linked STEP files parsed on this thread, for tests.
    static STEP_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Uncached file SHA-256 digests computed on this thread, for tests.
    static FILE_HASH_COMPUTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// `Session::refresh()` calls made inside `update_links` on this thread, for tests.
    static UPDATE_LINKS_REFRESHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Run `f` over `items` in parallel when there is more than one item, propagating this thread's
/// `vfs::ThreadContext` (path scope and one-action allowed network hosts) and merging test
/// counters from helper threads back into the calling thread.
fn map_link_work<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    if items.len() <= 1 {
        return items.iter().map(|item| Some(f(item))).collect();
    }
    let caller = std::thread::current().id();
    let vfs_ctx = solidcraft_io::vfs::ThreadContext::current();
    #[cfg(test)]
    {
        let raw = solidcraft_kernel::map_on_threads(items, MAX_LINK_WORKERS, |item| {
            if std::thread::current().id() == caller {
                (f(item), 0usize, 0usize, 0usize, 0usize)
            } else {
                vfs_ctx.run(|| {
                    let eval_before = EVALUATED.with(std::cell::Cell::get);
                    let reused_before = REUSED.with(std::cell::Cell::get);
                    let step_before = STEP_READS.with(std::cell::Cell::get);
                    let hash_before = FILE_HASH_COMPUTED.with(std::cell::Cell::get);
                    let out = f(item);
                    let eval_delta = EVALUATED.with(std::cell::Cell::get).saturating_sub(eval_before);
                    let reused_delta = REUSED.with(std::cell::Cell::get).saturating_sub(reused_before);
                    let step_delta = STEP_READS.with(std::cell::Cell::get).saturating_sub(step_before);
                    let hash_delta = FILE_HASH_COMPUTED.with(std::cell::Cell::get).saturating_sub(hash_before);
                    (out, eval_delta, reused_delta, step_delta, hash_delta)
                })
            }
        });
        let (mut eval_add, mut reused_add, mut step_add, mut hash_add) = (0usize, 0usize, 0usize, 0usize);
        let out = raw
            .into_iter()
            .map(|entry| {
                entry.map(|(value, e, r, s, h)| {
                    eval_add = eval_add.saturating_add(e);
                    reused_add = reused_add.saturating_add(r);
                    step_add = step_add.saturating_add(s);
                    hash_add = hash_add.saturating_add(h);
                    value
                })
            })
            .collect();
        if eval_add > 0 {
            EVALUATED.with(|n| n.set(n.get().saturating_add(eval_add)));
        }
        if reused_add > 0 {
            REUSED.with(|n| n.set(n.get().saturating_add(reused_add)));
        }
        if step_add > 0 {
            STEP_READS.with(|n| n.set(n.get().saturating_add(step_add)));
        }
        if hash_add > 0 {
            FILE_HASH_COMPUTED.with(|n| n.set(n.get().saturating_add(hash_add)));
        }
        out
    }
    #[cfg(not(test))]
    {
        solidcraft_kernel::map_on_threads(items, MAX_LINK_WORKERS, |item| {
            if std::thread::current().id() == caller { f(item) } else { vfs_ctx.run(|| f(item)) }
        })
    }
}

fn digest<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

/// Warnings, source properties and design properties of the last read of `file`, when it had these
/// bytes and gave the STEP text `step`, the face names `faces` and the mesh bodies `meshes`: the
/// bodies it gives are the ones the link holds.
fn unchanged_read(
    file: &Path,
    configuration: Option<SelectedConfiguration>,
    bytes: u64,
    step: u64,
    faces: u64,
    meshes: &[MeshData],
) -> Option<(Vec<String>, Vec<SourceProperties>, solidcraft_doc::Metadata)> {
    let file = absolute(file);
    let reads = READS.lock().ok()?;
    reads
        .iter()
        .find(|r| r.file == file && r.configuration == configuration && r.bytes == bytes && r.step == step && r.faces == faces && r.meshes == meshes)
        .map(|r| (r.warnings.clone(), r.properties.clone(), r.metadata.clone()))
}

fn remember_read_parts(
    file: &Path,
    configuration: Option<SelectedConfiguration>,
    bytes: u64,
    kind: &FeatureKind,
    warnings: &[String],
    faces: &[Vec<String>],
    properties: &[SourceProperties],
    metadata: &solidcraft_doc::Metadata,
    meshes: &[MeshData],
) {
    let FeatureKind::Import { step, .. } = kind else { return };
    let file = absolute(file);
    let Ok(mut reads) = READS.lock() else { return };
    reads.retain(|r| r.file != file || r.configuration != configuration);
    if reads.len() >= READS_SIZE {
        reads.remove(0);
    }
    reads.push(LinkRead {
        file,
        configuration,
        bytes,
        step: digest(step.as_bytes()),
        faces: digest(faces),
        warnings: warnings.to_vec(),
        properties: properties.to_vec(),
        metadata: metadata.clone(),
        meshes: meshes.to_vec(),
    });
}

fn remember_read(
    file: &Path,
    configuration: Option<SelectedConfiguration>,
    bytes: u64,
    feature: &solidcraft_io::StepFeature,
    faces: &[Vec<String>],
    properties: &[SourceProperties],
    metadata: &solidcraft_doc::Metadata,
    meshes: &[MeshData],
) {
    remember_read_parts(file, configuration, bytes, &feature.kind, &feature.warnings, faces, properties, metadata, meshes);
}

/// The bytes of the source at `file`, which must not be the design saved at `own`.
/// `any`: an importer plugin's file, which may have any name.
fn read_link(file: &Path, own: Option<&str>, any: bool) -> Result<Vec<u8>> {
    let shown = file.display().to_string();
    // A network location only when its server is allowed, a device never: judged from the text
    // before anything is asked of the disk (on Windows, resolving or reading `\\host\share\x`
    // connects to the host), so a refused path skips the self-link check, which resolves it, and
    // `read_linked_file` says why it is refused.
    let reachable = solidcraft_io::vfs::design_path_refusal(&shown, own).is_none();
    if reachable && own.is_some_and(|o| same_file(Path::new(o), file)) {
        return Err(EngineError::Other(format!("{shown}: a design can't be linked into itself")));
    }
    read_linked_file(file, own, any).map_err(|e| EngineError::Other(e.reason))
}

/// Read a configuration selector before any I/O. An explicit null follows the saved state.
fn configuration_key<'a>(p: &'a Value, default: Option<u64>, cmd: &str) -> Result<Option<RowKey<'a>>> {
    match p.get("configuration") {
        None => Ok(default.map(RowKey::Id)),
        Some(Value::Null) => Ok(None),
        Some(value) => RowKey::read(value).map(Some).ok_or_else(|| bad(cmd, "`configuration` must be a row name, id, or null")),
    }
}

/// Resolve a name once. Links keep the row's stable id through source row renames.
fn configuration_id(bytes: &[u8], key: Option<RowKey<'_>>) -> Result<Option<SelectedConfiguration>> {
    let Some(key) = key else { return Ok(None) };
    let doc = solidcraft_io::read_design(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
    doc.configs
        .row(key)
        .map(|row| Some(SelectedConfiguration { row: row.id, identity: doc.configs.row_identity(row.id) }))
        .ok_or_else(|| EngineError::Other(format!("configuration `{key}` does not exist in the source design")))
}

/// Linked files' digests as [`file_sha256`] last worked them out, by file, with the time and
/// size the file had then, paired with a generation bumped on every save invalidation so an
/// overlapping read cannot repopulate a stale entry. Bounded, oldest first out.
static DIGESTS: Mutex<(u64, Vec<(PathBuf, (std::time::SystemTime, u64), String)>)> = Mutex::new((0, Vec::new()));
const DIGESTS_SIZE: usize = 256;

enum DigestLookup {
    Hit(String),
    Miss { generation: u64, stamp: Option<(std::time::SystemTime, u64)> },
}

fn lookup_cached_sha256(file: &Path, own: Option<&str>) -> Option<DigestLookup> {
    let digests = DIGESTS.lock().ok()?;
    let generation = digests.0;
    let stamp = solidcraft_io::vfs::design_path_stamp(&file.to_string_lossy(), own);
    if let Some(stamp) = stamp
        && let Some(known) = digests.1.iter().find(|(f, s, _)| *f == file && *s == stamp).map(|(_, _, d)| d.clone())
    {
        return Some(DigestLookup::Hit(known));
    }
    Some(DigestLookup::Miss { generation, stamp })
}

fn store_cached_sha256(file: PathBuf, generation: u64, stamp: Option<(std::time::SystemTime, u64)>, digest: &str) {
    if let (Some(stamp), Ok(mut digests)) = (stamp, DIGESTS.lock())
        && digests.0 == generation
    {
        digests.1.retain(|(f, _, _)| *f != file);
        if digests.1.len() >= DIGESTS_SIZE {
            digests.1.remove(0);
        }
        digests.1.push((file, stamp, digest.to_string()));
    }
}

fn compute_file_sha256(file: &Path, own: Option<&str>, any: bool) -> Option<String> {
    #[cfg(test)]
    FILE_HASH_COMPUTED.with(|n| n.set(n.get() + 1));
    Some(DesignLink::file_digest(&read_link(file, own, any).ok()?))
}

/// The digest of the design file at `file` now ([`DesignLink::file_digest`]), `None` when it
/// can't be read. `component.links` asks each time a tab is shown, so a file is read again only
/// when its time or size changed since; the browser build keeps no times and reads it each time.
pub(super) fn file_sha256(file: &Path, own: Option<&str>, any: bool) -> Option<String> {
    let file = absolute(file);
    let (generation, stamp) = match lookup_cached_sha256(&file, own)? {
        DigestLookup::Hit(known) => return Some(known),
        DigestLookup::Miss { generation, stamp } => (generation, stamp),
    };
    let digest = compute_file_sha256(&file, own, any)?;
    store_cached_sha256(file, generation, stamp, &digest);
    Some(digest)
}

/// Drop cached file digests after SolidCraft writes a file, invalidating any path aliases and
/// preventing an overlapping [`file_sha256`] read from publishing a stale digest under the same
/// timestamp tick and size.
pub(super) fn forget_file_digests() {
    if let Ok(mut digests) = DIGESTS.lock() {
        digests.0 = digests.0.wrapping_add(1);
        digests.1.clear();
    }
}

/// Only STEP has a source link; other formats accepted by Insert STEP keep their old behaviour.
pub(super) fn is_step_source(path: &str) -> bool {
    Path::new(path).extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("step") || x.eq_ignore_ascii_case("stp"))
}

/// Read either kind of linked source into the same replacement and reference-matching path.
/// `flat`: the link's STEP import was flattened ([`step_import_flat`]), so it skips no placement.
fn source_bodies(
    file: &Path,
    bytes: &[u8],
    link: &DesignLink,
    flat: bool,
    validation: solidcraft_kernel::Validation,
) -> Result<(Linked, LinkedImages)> {
    let Some(names) = &link.step_names else {
        return design_bodies(&Source::File(file.to_path_buf()), bytes, selected_configuration(link), validation);
    };
    #[cfg(test)]
    STEP_READS.with(|n| n.set(n.get() + 1));
    let names = solidcraft_io::StepNames::parse(names).ok_or_else(|| EngineError::Other("invalid linked STEP naming policy".into()))?;
    let shown = file.to_string_lossy();
    let mut feature = solidcraft_io::step_import_feature_named(bytes, &shown, names).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    // Assembly body slots and offsets are retained by the Import feature, like body names.
    // Repeated placements must remain skipped rather than being imported a second time, where
    // components below the link's show them; a flattened import has none, and builds them all.
    if let FeatureKind::Import { skip, .. } = &mut feature.kind {
        *skip = if flat { Vec::new() } else { super::file::import::step_skip(&feature.import) };
    }
    remember_read(file, None, digest(bytes), &feature, &[], &[], &Default::default(), &[]);
    Ok(((feature.name.clone(), feature, Vec::new(), Default::default(), Vec::new(), Vec::new()), (Vec::new(), Vec::new())))
}

/// Read a link's source again by what read it: the importer plugin it names (which writes the design
/// the bodies come from), else the built-in reader. The link keeps the version of the plugin that
/// read it last. `bytes` are the source file's own, whose digest decides when it was read last.
fn linked_bodies(s: &mut Session, comp: u64, file: &Path, bytes: &[u8], link: &DesignLink) -> Result<(Linked, LinkedImages)> {
    let Some(importer) = &link.importer else { return source_bodies(file, bytes, link, step_import_flat(&s.doc, link), s.validation()) };
    if let Some(why) = importer_file_refusal(s, file) {
        return Err(EngineError::Other(why));
    }
    let slot = linked_importer(s, importer)?;
    let options = if importer.options.is_null() { json!({}) } else { importer.options.clone() };
    // Another version of the plugin that can't read the file says so beside its own words.
    let other_version = |s: &Session, e: EngineError| {
        let now = crate::plugin::importer_status(s, &slot.plugin_id, &slot.id).installed_version;
        match (&importer.version, now) {
            (Some(was), Some(now)) if *was != now => {
                EngineError::Other(format!("{e} (this link was read with version {was} of the plugin; version {now} is installed)"))
            }
            _ => e,
        }
    };
    let made = crate::plugin::import_design_bytes(s, &slot, &file.to_string_lossy(), &options).map_err(|e| other_version(s, e))?;
    let (mut read, images) = design_bodies(&Source::File(file.to_path_buf()), &made.design, None, s.validation())?;
    // What `design_bodies` remembers is keyed by the bytes it read: for this link, the file's.
    remember_read(file, None, digest(bytes), &read.1, &read.2, &read.4, &read.3, &read.5);
    read.1.warnings.extend(made.notes);
    if let Some(kept) = s.doc_mut().links.get_mut(&comp).and_then(|l| l.importer.as_mut()) {
        kept.version = Some(made.version);
    }
    Ok((read, images))
}

/// The installed, enabled importer a link names, to read its file again: one of the plugin the
/// record names. A record that names no plugin is not one this build reads, and runs nothing.
fn linked_importer(s: &Session, importer: &solidcraft_doc::LinkImporter) -> Result<crate::plugin::Importer> {
    let Some(plugin) = importer.plugin.as_deref() else {
        return Err(EngineError::Other(format!(
            "this link's record names no importer plugin for `{}`, so nothing can read its file again; break the link and insert the file again; the component keeps its last bodies",
            importer.format
        )));
    };
    let status = crate::plugin::importer_status(s, plugin, &importer.format);
    let needs = importer_needs(importer, &status);
    crate::plugin::importers(s).into_iter().find(|i| i.id == importer.format && i.plugin_id == plugin && needs.is_none()).ok_or_else(|| {
        let what = needs.unwrap_or_else(|| format!("the importer `{}` is not offered by plugin `{plugin}`", importer.format));
        EngineError::Other(format!("{what}; the component keeps its last bodies"))
    })
}

/// The caller named `file` for a link: Update Links may read it again in this session, wherever it is.
pub(super) fn allow_linked_file(s: &mut Session, file: &Path) {
    s.plugin_state.linked_files.insert(link_target(&absolute(file)));
}

/// Read the KiCad board a link names again, when its file changed. Whether the component's
/// bodies were replaced (`true`) or the board is the one the link read (`false`). Nothing
/// changes when the board can't be read.
fn update_kicad_link(
    s: &mut Session,
    comp: u64,
    link: &DesignLink,
    importer: &solidcraft_doc::LinkImporter,
) -> std::result::Result<(bool, Vec<String>), String> {
    link_refused(s.path.as_deref(), &link.path).map_err(|e| e.to_string())?;
    let file = resolve(s.path.as_deref(), &link.path);
    // The board the import reads: a `.kicad_pro` stands for its same-stem `.kicad_pcb`. That file,
    // not the one the link names, is judged (its place and every link in its path) and read, once,
    // and the import builds from those bytes.
    let board = solidcraft_io::kicad_board_path(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let shown = board.to_string_lossy().into_owned();
    solidcraft_io::vfs::check_design_path(&shown, s.path.as_deref()).map_err(|e| e.to_string())?;
    if let Some(why) = importer_file_refusal(s, &file).or_else(|| importer_file_refusal(s, &board)) {
        return Err(why);
    }
    let bytes = read_link(&board, s.path.as_deref(), true).map_err(|e| e.to_string())?;
    let read = DesignLink::file_digest(&bytes);
    if link.file_sha256.as_deref() == Some(read.as_str()) {
        return Ok((false, Vec::new()));
    }
    let done = super::file::replace_kicad(s, comp, &shown, bytes, &importer.options).map_err(|e| e.to_string())?;
    if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
        kept.file_sha256 = Some(done.digest);
    }
    Ok((true, done.warnings))
}

/// Why an importer plugin may not read `file` for a link now, judged from its place alone: it is
/// outside this design's own folder and its folder is not one the user allowed (Preferences,
/// General), and the caller did not name it to Insert Design in this session. A design received
/// from someone else names what files it likes, and none of them is read, nor any plugin run on
/// it, until its folder is allowed. `None`: it may be read.
fn importer_file_refusal(s: &Session, file: &Path) -> Option<String> {
    let target = link_target(file);
    // The file the read opens, every link in its path followed, must be in a consented place too.
    let real = real_file(file).unwrap_or_else(|| target.clone());
    let consented = |p: &Path| {
        inside_design_folder(s.path.as_deref(), p)
            || s.plugin_state.link_folders.iter().any(|folder| p.starts_with(real_file(folder).unwrap_or_else(|| absolute(folder))))
    };
    let allowed = s.plugin_state.linked_files.contains(&target) || (consented(&target) && consented(&real));
    (!allowed).then(|| {
        format!(
            "{}: this file is outside the design's folder and its folder is not allowed yet; allow it (Preferences, General, linked file folders) and SolidCraft reads it with the importer",
            file.display()
        )
    })
}

/// Where the design a link takes comes from: a file on disk, or a design the caller holds in
/// memory (one open in this session that has no file yet, #16829). A design in memory has no
/// file to read, to refuse or to link to: the link that holds it names no file.
enum Source {
    /// A saved design, by path.
    File(PathBuf),
    /// A design held in memory: its name, and its JSON as the caller holds it.
    Session { name: String, text: String },
}

impl Source {
    /// The name the messages about this design use.
    fn shown(&self) -> String {
        match self {
            Source::File(file) => file.display().to_string(),
            Source::Session { name, .. } => name.clone(),
        }
    }
    /// The file the design is read from and remembered by (`None`: a design in memory, which no
    /// file holds and no read remembers).
    fn file(&self) -> Option<&Path> {
        match self {
            Source::File(file) => Some(file),
            Source::Session { .. } => None,
        }
    }
    /// The bytes of the design: the file's, or the JSON the caller holds, under the design size
    /// limit as a file is.
    fn bytes(&self, own: Option<&str>) -> Result<Vec<u8>> {
        match self {
            Source::File(file) => read_link(file, own, false),
            Source::Session { text, .. } => {
                let bytes = text.as_bytes().to_vec();
                let limit = solidcraft_io::limits::max_design_bytes();
                if bytes.len() > limit {
                    return Err(bad("component.insert_design", format!("the design is larger than the design size limit ({limit} bytes)")));
                }
                Ok(bytes)
            }
        }
    }
}

/// The solid bodies of `bytes`, the design `source` names, as an Import feature, and its canvases
/// and decals ([`design_images`]). The model the file keeps is its evaluation when this build saved
/// it for exactly that design, as when it is opened.
fn design_bodies(
    source: &Source,
    bytes: &[u8],
    configuration: Option<SelectedConfiguration>,
    validation: solidcraft_kernel::Validation,
) -> Result<(Linked, LinkedImages)> {
    design_bodies_reusing(source, bytes, configuration, validation, false)
}

/// The local store's key for the evaluation of `bytes`, the linked design at `file`
/// ([`design_bodies_reusing`]), and the store. The file's whole path is part of the key: a design
/// may read files beside it (a font), so the same bytes elsewhere are another evaluation.
fn linked_evaluation(file: &Path, bytes: &[u8]) -> Option<(solidcraft_io::ImportCache, String)> {
    let cache = super::file::import_cache().filter(|_| solidcraft_geom::env::var_os("NO_MODEL_CACHE").is_none())?;
    let shown = file.display().to_string();
    let key = solidcraft_io::ImportCache::key(bytes, &shown, &json!({"linked_design": absolute(file).to_string_lossy()}));
    Some((cache, key))
}

fn parse_design_bytes(source: &Source, bytes: &[u8]) -> Result<(Document, Option<solidcraft_doc::SavedModel>)> {
    let shown = source.shown();
    let fail = |e: solidcraft_io::IoError| EngineError::Other(format!("{shown}: {e}"));
    if solidcraft_io::is_design_container(bytes) {
        let mut reader = solidcraft_io::DesignContainerReader::new(std::io::Cursor::new(bytes)).map_err(fail)?;
        let saved = reader.geometry();
        Ok((reader.read_document().map_err(fail)?, saved))
    } else {
        Ok((solidcraft_io::read_design(bytes).map_err(fail)?, None))
    }
}

fn evaluate_parsed_design(
    source: &Source,
    bytes: &[u8],
    doc: Document,
    saved: Option<solidcraft_doc::SavedModel>,
    configuration: Option<SelectedConfiguration>,
    validation: solidcraft_kernel::Validation,
    reuse: bool,
) -> Result<(Linked, LinkedImages)> {
    #[cfg(test)]
    EVALUATED.with(|n| n.set(n.get() + 1));
    let shown = source.shown();
    if let Some(selected) = configuration
        && doc.configs.row_identity(selected.row) != selected.identity
    {
        return Err(EngineError::Other(format!("{shown}: configuration `{}` was replaced; select a configuration again", selected.row)));
    }
    let file = source.file();
    let store = if reuse && saved.is_none() && configuration.is_none() && doc.links.is_empty() {
        file.and_then(|f| linked_evaluation(f, bytes))
    } else {
        None
    };
    let kept = store.as_ref().and_then(|(cache, key)| cache.read(key));
    let fresh = kept.is_none();
    #[cfg(test)]
    REUSED.with(|n| n.set(n.get() + usize::from(!fresh)));
    let (doc, saved) = match kept {
        Some((doc, model, _)) => (doc, Some(model)),
        None => (doc, saved),
    };
    // A design in memory has no file: its own links stay as they are, as they do in the session
    // that holds it.
    let path = file.map(|f| f.display().to_string());
    let outside = READING.with(|reading| reading.borrow().outside);
    let mut part = Session::opened_at_validating(doc, saved.as_ref(), path.as_deref(), validation);
    if READING.with(|reading| reading.borrow().outside) != outside {
        // Part of the design would pass for the whole of it.
        return Err(EngineError::Other(format!(
            "{shown}: it was saved without the bodies of its own linked designs, and some of those are in another folder, which is not read unasked; Update Links reads them"
        )));
    }
    if fresh
        && let Some((cache, key)) = store
        && part.model.results.iter().all(|r| r.error.is_none())
        && let Some((model, pending)) = super::file::saved_geometry_now(&part, &part.doc)
    {
        super::file::write_import_cache(&cache, key, &part.doc, model, pending, &json!({}));
    }
    if let Some(selected) = configuration {
        // Use the same rule, rebuild and style validation as an interactive activation.
        // Its scratch evaluation cannot use the saved geometry of another row.
        part.execute("config.activate", &json!({"row": selected.row})).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    }
    let (name, feature, faces, metadata, properties, meshes) = session_bodies(&part, &shown)?;
    if let Some(file) = source.file() {
        remember_read(file, configuration, digest(bytes), &feature, &faces, &properties, &metadata, &meshes);
    }
    let images = design_images(&part, &shown);
    Ok(((name, feature, faces, metadata, properties, meshes), images))
}

/// [`design_bodies`], with `reuse` taking the design's evaluation from the local store of reused
/// imports when it holds one for these bytes, and keeping it there otherwise
/// (`solidcraft_io::ImportCache`, with its trust rules: only this build's evaluation of exactly
/// this design is adopted). A design saved without linked bodies reads every link as it opens,
/// and a plain JSON part keeps no model of its own, so each open would evaluate each part's whole
/// timeline again. Only a design whose model depends on its bytes alone is kept: one with links
/// of its own, read in a chosen configuration, or held in memory, is evaluated each time.
fn design_bodies_reusing(
    source: &Source,
    bytes: &[u8],
    configuration: Option<SelectedConfiguration>,
    validation: solidcraft_kernel::Validation,
    reuse: bool,
) -> Result<(Linked, LinkedImages)> {
    let (doc, saved) = match parse_design_bytes(source, bytes) {
        Ok(parsed) => parsed,
        Err(e) => {
            #[cfg(test)]
            EVALUATED.with(|n| n.set(n.get() + 1));
            return Err(e);
        }
    };
    evaluate_parsed_design(source, bytes, doc, saved, configuration, validation, reuse)
}

/// The canvases and decals of `part`, the design saved as `shown`, each where the design shows
/// it, on a fixed plane in the design's own coordinates: the frame its linked bodies come in, so
/// added to the linked component they sit on the bodies as in the design and move with each
/// placement. One that follows sketch points comes in where they put it now and no longer
/// follows them; one those points stretch keeps its image's own shape, and is named in the
/// warnings. Hidden ones come in hidden.
fn design_images(part: &Session, shown: &str) -> LinkedImages {
    use solidcore_record::{Plane, Vec2};
    let (vals, _) = part.doc.param_values();
    let state = part.model.state();
    let (mut images, mut warnings) = (Vec::new(), Vec::new());
    for c in &part.doc.canvases {
        let mut image = c.clone();
        image.anchor = None;
        image.component = 0;
        match c.anchor.as_ref().and_then(|a| part.doc.canvas_anchor_corners(c, a, &state).ok()) {
            Some(k) => {
                // The frame unmirrored: a mirrored image's bottom-left corner is its bottom-right.
                let (bl, br, tl) = if c.flip { (k[1], k[0], k[2]) } else { (k[0], k[1], k[3]) };
                let [w, h] = c.pixels;
                let aspect = if w == 0 { 1.0 } else { f64::from(h) / f64::from(w) };
                let at = |p: solidcore_record::Vec3| [p.x, p.y, p.z];
                let (Some(plane), Some((width, stretched))) =
                    (Plane::new(bl, br - bl, tl - bl), solidcraft_kernel::affine::fit_aspect(at(bl), at(br), at(tl), aspect))
                else {
                    warnings.push(format!("{shown}: image `{}` has no area where its sketch puts it: left out", c.name));
                    continue;
                };
                image.plane = solidcraft_doc::PlaneRef::Custom { plane };
                image.width = width;
                image.angle = 0.0;
                let height = image.height();
                image.center = Vec2::new(image.width / 2.0, height / 2.0);
                if stretched {
                    warnings.push(format!("{shown}: image `{}` is stretched by its sketch there; it comes in with its image's own shape", c.name));
                }
            }
            None => match part.doc.canvas_plane(&vals, c, &state) {
                Ok(plane) => image.plane = solidcraft_doc::PlaneRef::Custom { plane },
                Err(e) => {
                    warnings.push(format!("{shown}: image `{}`: {e}: left out", c.name));
                    continue;
                }
            },
        }
        images.push(image);
    }
    (images, warnings)
}

/// The name to read the STEP text of a design's bodies under: `shown`, unless its extension names a
/// format with its own reader (FreeCAD, Rhino, IGES). The text is STEP whatever the design came
/// from: a design an importer plugin wrote from `part.FCStd` is linked as `part.FCStd`, and routing
/// that STEP by the name sent it to the FreeCAD reader, which refused it.
fn step_label(shown: &str) -> String {
    if solidcraft_io::is_native_cad_path(shown) || solidcraft_io::is_iges_path(shown) {
        Path::new(shown).with_extension("step").to_string_lossy().into_owned()
    } else {
        shown.to_string()
    }
}

/// The solid bodies of `part`, the design saved as `shown`, as an Import feature, and its mesh
/// bodies, which a Mesh Import feature beside it holds (#20050).
fn session_bodies(part: &Session, shown: &str) -> Result<Linked> {
    let name = part.doc.name.clone();
    if let Some(e) = part.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(EngineError::Other(format!("{shown}: the design has a feature that fails ({e}); fix it before linking it")));
    }
    let world = part.world_state()?;
    let solids: Vec<String> = world.bodies.iter().filter(|b| !b.body.is_mesh()).map(|b| b.name.clone()).collect();
    if solids.is_empty() {
        return Err(EngineError::Other(format!("{shown}: the design has no solid bodies to link")));
    }
    let step = solidcraft_io::export(&part.doc.painted_for_link(&world), &solids, solidcraft_io::Format::Step, &name)
        .map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    let mut feature = solidcraft_io::step_import_feature(&step, &step_label(shown)).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    let meshes: Vec<MeshData> = world.bodies.iter().filter(|b| b.body.is_mesh()).map(|b| super::file::mesh_data(&part.doc, b, &b.mesh())).collect();
    // The STEP text keeps no face names: carry the design's own, so references into the linked
    // bodies follow them as they do inside the design (#3330).
    let faces = solidcraft_doc::naming::carried_face_names(&world, &feature.import.bodies);
    let mut sources = Vec::new();
    let mut properties = feature
        .import
        .bodies
        .iter()
        .zip(&faces)
        .map(|(read, carried)| {
            // STEP can split a source body into several lumps. Match its carried source
            // face names rather than assuming source and imported body orders agree.
            let source = world
                .bodies
                .iter()
                .find(|b| b.name == read.name)
                .or_else(|| {
                    world
                        .bodies
                        .iter()
                        .filter(|b| !b.body.is_mesh())
                        .filter_map(|b| {
                            let names = solidcraft_doc::naming::face_names(b);
                            let count = carried.iter().filter(|n| !n.is_empty() && names.contains(n)).count();
                            (count > 0).then_some((count, b))
                        })
                        .max_by_key(|(count, _)| *count)
                        .map(|(_, b)| b)
                })
                .ok_or_else(|| EngineError::Other(format!("{shown}: cannot match linked body {} to its source properties", read.name)))?;
            sources.push(source);
            let key = source.definition_name();
            let material = part.doc.body_material(key);
            let definition = if solidcraft_doc::material_density(&material).is_none() { part.doc.user_material(&material).cloned() } else { None };
            let appearance = part
                .doc
                .appearances
                .bodies
                .get(key)
                .cloned()
                .or_else(|| part.doc.appearances.components.get(&part.doc.model_body_component(source)).cloned());
            let names = solidcraft_doc::naming::face_names(source);
            let authored: std::collections::BTreeMap<_, _> = part
                .doc
                .authored_face_colors(source)
                .into_iter()
                .filter_map(|(index, look)| names.get(index).map(|name| (name.clone(), look)))
                .collect();
            let faces = carried.iter().enumerate().filter_map(|(index, name)| authored.get(name).map(|look| (index, look.clone()))).collect();
            Ok(SourceProperties { material, definition, appearance, faces, threads: Vec::new() })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut counts = std::collections::HashMap::new();
    for source in &sources {
        counts.entry(source.name.as_str()).and_modify(|n: &mut usize| *n = n.saturating_add(1)).or_insert(1);
    }
    let mut threads: std::collections::HashMap<&str, Vec<solidcraft_doc::ThreadInfo>> = std::collections::HashMap::new();
    for thread in &world.threads {
        threads.entry(&thread.body).or_default().push(thread.clone());
    }
    for (source, properties) in sources.iter().zip(&mut properties) {
        let Some(records) = threads.remove(source.name.as_str()) else { continue };
        if counts.get(source.name.as_str()) == Some(&1) {
            properties.threads = records;
        } else {
            // A record names its source body, not a face. Assigning it to every imported
            // lump would invent threads on unrelated bodies. Keep the solids and report it.
            feature
                .warnings
                .push(format!("cosmetic threads on {} are not linked because the source body was split into several bodies", source.name));
        }
    }
    Ok((name, feature, faces, part.doc.metadata.clone(), properties, meshes))
}

fn material_fingerprint(doc: &Document, material: &str) -> Result<Option<String>> {
    if solidcraft_doc::material_density(material).is_some() {
        return Ok(None);
    }
    doc.user_material(material).map(LinkedBodyProperties::fingerprint).transpose().map_err(|e| EngineError::Other(e.to_string()))
}

/// `name` without the ` (linked N)` suffixes inheriting a linked material may have added.
fn linked_stem(name: &str) -> &str {
    let mut stem = name;
    while let Some(rest) = stem.strip_suffix(')')
        && let Some((head, n)) = rest.rsplit_once(" (linked ")
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        stem = head;
    }
    stem
}

fn is_linked_copy(name: &str) -> bool {
    linked_stem(name) != name
}

fn same_material_properties(a: &UserMaterial, b: &UserMaterial) -> bool {
    let mut candidate = a.clone();
    candidate.name.clone_from(&b.name);
    &candidate == b
}

fn referenced_materials(doc: &Document) -> std::collections::HashSet<String> {
    let mut referenced: std::collections::HashSet<String> = doc.materials.values().map(|m| m.to_ascii_lowercase()).collect();
    for link in doc.links.values() {
        for snap in &link.inherited {
            if !snap.material.is_empty() {
                referenced.insert(snap.material.to_ascii_lowercase());
            }
        }
    }
    let material_cols: Vec<usize> =
        doc.configs.columns.iter().enumerate().filter_map(|(i, c)| matches!(c, solidcraft_doc::config::Column::Material(_)).then_some(i)).collect();
    for row in &doc.configs.rows {
        for &ci in &material_cols {
            if let Some(cell) = row.cells.get(ci) {
                referenced.insert(cell.trim().to_ascii_lowercase());
            }
        }
    }
    referenced
}

fn link_inherited_linked_copies(doc: &Document) -> std::collections::HashSet<String> {
    doc.links
        .values()
        .flat_map(|l| &l.inherited)
        .filter(|snap| is_linked_copy(&snap.material))
        .map(|snap| snap.material.to_ascii_lowercase())
        .collect()
}

/// Remove unreferenced auto-generated `(linked N)` materials whose provenance is established via
/// `candidates` (lowercased names previously held by a link's `inherited` snapshots). User-created
/// materials — even if named with a `(linked N)` suffix — are never removed.
pub(super) fn prune_unused_linked_materials(doc: &mut Document, candidates: &std::collections::HashSet<String>) -> bool {
    if candidates.is_empty() {
        return false;
    }
    let referenced = referenced_materials(doc);
    let before = doc.user_materials.len();
    doc.user_materials.retain(|m| {
        let lower = m.name.to_ascii_lowercase();
        !(is_linked_copy(&m.name) && candidates.contains(&lower) && !referenced.contains(&lower))
    });
    doc.user_materials.len() != before
}

/// Merge identical link-proven `(linked N)` material copies sharing the exact same stem onto a
/// single canonical `(linked N)` entry, remap body assignments / link snapshots / material
/// configuration cells, and prune unreferenced link-proven `(linked N)` copies (#21651).
///
/// Only materials with established link provenance (referenced by `doc.links[*].inherited`, or
/// unreferenced exact stem+property duplicates of such a link-proven copy) are touched; user-created
/// materials — including authored names containing `(linked N)` — and ambiguous truncated stems are
/// left intact.
fn consolidate_and_prune_linked_materials(doc: &mut Document, prune_candidates: &std::collections::HashSet<String>) -> Result<bool> {
    let proven = link_inherited_linked_copies(doc);
    let referenced = referenced_materials(doc);
    // First pick one canonical link-proven `(linked N)` entry per (exact linked_stem, properties).
    let mut canonicals: Vec<usize> = Vec::new();
    for (i, m) in doc.user_materials.iter().enumerate() {
        let lower = m.name.to_ascii_lowercase();
        if !is_linked_copy(&m.name) || !proven.contains(&lower) {
            continue;
        }
        let stem = linked_stem(&m.name);
        let already = canonicals.iter().any(|&j| {
            let c = &doc.user_materials[j];
            linked_stem(&c.name).eq_ignore_ascii_case(stem) && same_material_properties(c, m)
        });
        if !already {
            canonicals.push(i);
        }
    }
    let mut removed = vec![false; doc.user_materials.len()];
    let mut remap: std::collections::HashMap<String, (String, Option<String>, Option<String>)> = std::collections::HashMap::new();
    for (i, m) in doc.user_materials.iter().enumerate() {
        if !is_linked_copy(&m.name) || canonicals.contains(&i) {
            continue;
        }
        let lower = m.name.to_ascii_lowercase();
        if !proven.contains(&lower) && referenced.contains(&lower) {
            continue;
        }
        let stem = linked_stem(&m.name);
        let Some(&canon_idx) = canonicals.iter().find(|&&j| {
            let c = &doc.user_materials[j];
            linked_stem(&c.name).eq_ignore_ascii_case(stem) && same_material_properties(c, m)
        }) else {
            continue;
        };
        let canon = &doc.user_materials[canon_idx];
        let dup_fp = LinkedBodyProperties::fingerprint(m).map_err(|e| EngineError::Other(e.to_string()))?;
        let canon_fp = LinkedBodyProperties::fingerprint(canon).map_err(|e| EngineError::Other(e.to_string()))?;
        remap.insert(lower, (canon.name.clone(), Some(dup_fp), Some(canon_fp)));
        removed[i] = true;
    }
    if !remap.is_empty() {
        for mat in doc.materials.values_mut() {
            if let Some((canon, ..)) = remap.get(&mat.to_ascii_lowercase()) {
                mat.clone_from(canon);
            }
        }
        for link in doc.links.values_mut() {
            for snap in &mut link.inherited {
                if let Some((canon, dup_fp, canon_fp)) = remap.get(&snap.material.to_ascii_lowercase()) {
                    snap.material.clone_from(canon);
                    if snap.material_fingerprint == *dup_fp {
                        snap.material_fingerprint.clone_from(canon_fp);
                    }
                }
            }
        }
        let material_cols: Vec<usize> = doc
            .configs
            .columns
            .iter()
            .enumerate()
            .filter_map(|(i, c)| matches!(c, solidcraft_doc::config::Column::Material(_)).then_some(i))
            .collect();
        for row in &mut doc.configs.rows {
            for &ci in &material_cols {
                if let Some(cell) = row.cells.get_mut(ci)
                    && let Some((canon, ..)) = remap.get(&cell.trim().to_ascii_lowercase())
                {
                    cell.clone_from(canon);
                }
            }
        }
        let mut idx = 0usize;
        doc.user_materials.retain(|_| {
            let keep = !removed[idx];
            idx += 1;
            keep
        });
    }
    let pruned = prune_unused_linked_materials(doc, prune_candidates);
    Ok(!remap.is_empty() || pruned)
}

/// Copy a design-owned definition without changing a same-named assembly material.
fn inherit_material(
    doc: &mut Document,
    source: &SourceProperties,
    previous: Option<&str>,
    prune_candidates: &std::collections::HashSet<String>,
) -> Result<String> {
    let Some(definition) = &source.definition else { return Ok(source.material.clone()) };
    let stem = linked_stem(&definition.name);
    let short: String = stem.chars().take(64).collect();
    // Equal density/optics do not make differently named materials interchangeable. Reuse the exact
    // source name first, then a matching `(linked N)` copy of the same stem, and finally any
    // previously assigned copy so updates converge rather than preserving duplicates (#21651).
    let exact = |m: &UserMaterial| m.name.eq_ignore_ascii_case(&definition.name);
    let copy = |m: &UserMaterial| {
        let ms = linked_stem(&m.name);
        ms.eq_ignore_ascii_case(stem) || (is_linked_copy(&m.name) && ms.eq_ignore_ascii_case(&short))
    };
    let prev = |m: &UserMaterial| previous.is_some_and(|p| m.name.eq_ignore_ascii_case(p));
    for same_name in [&exact as &dyn Fn(&UserMaterial) -> bool, &copy, &prev] {
        for existing in doc.user_materials.iter().filter(|m| same_name(m)) {
            if same_material_properties(definition, existing) {
                return Ok(existing.name.clone());
            }
        }
    }
    if doc.user_materials.len() >= solidcraft_doc::MAX_USER_MATERIALS {
        prune_unused_linked_materials(doc, prune_candidates);
    }
    if doc.user_materials.len() >= solidcraft_doc::MAX_USER_MATERIALS {
        return Err(EngineError::Other(format!(
            "cannot inherit linked material: this design already has {} custom materials",
            solidcraft_doc::MAX_USER_MATERIALS
        )));
    }
    let mut copy = definition.clone();
    if doc.material_name(&copy.name).is_some() {
        let base: String = copy.name.chars().take(64).collect();
        let name = (1..=solidcraft_doc::MAX_USER_MATERIALS + 1)
            .map(|i| format!("{base} (linked {i})"))
            .find(|name| doc.material_name(name).is_none())
            .ok_or_else(|| EngineError::Other("cannot name the linked material".into()))?;
        copy.name = name;
    }
    let name = copy.name.clone();
    doc.user_materials.push(copy);
    Ok(name)
}

/// Replace only unchanged inherited face assignments. A locally edited, added or removed
/// override remains local, independently for each face. Resolve names on the evaluated Import,
/// not source feature IDs or face order from the source's model.
fn inherit_faces(
    s: &mut Session,
    name: &str,
    source: &SourceProperties,
    previous: Option<&LinkedBodyProperties>,
) -> (Vec<solidcraft_doc::appearance::FaceLook>, bool) {
    use solidcraft_doc::appearance::FaceLook;
    let previous = previous.and_then(|p| p.faces.as_deref()).unwrap_or_default();
    if source.faces.is_empty() && previous.is_empty() {
        return (Vec::new(), false);
    }
    let state = s.model.state();
    let Some(body) = state.body(name) else { return (Vec::new(), false) };
    let names = solidcraft_doc::naming::face_names(body);
    // A stable named assignment needs no new geometric point. Keep its position in
    // the appearance list and avoid tessellating on unchanged cached reads.
    if previous.len() == source.faces.len()
        && previous.iter().all(|old| old.body == name && s.doc.appearances.faces.contains(old))
        && source.faces.iter().all(|(index, look)| {
            names.get(*index).is_some_and(|name| previous.iter().any(|old| old.name.as_ref() == Some(name) && old.look == *look))
        })
    {
        return (previous.to_vec(), false);
    }
    let before = s.doc.appearances.faces.clone();
    let mut local = before.clone();
    let mut inherited = Vec::new();
    let mut replaced = Vec::new();
    let mut blocked = std::collections::BTreeSet::new();
    for old in previous {
        let mut old = old.clone();
        old.body = name.into();
        if let Some(index) = local.iter().position(|face| *face == old) {
            local.remove(index);
            replaced.push(old);
        } else {
            if let Some(name) = &old.name {
                blocked.extend(solidcraft_doc::naming::faces_with_stem(body, name));
            }
            // Remember an explicit local removal too, including after save/reopen.
            inherited.push(old);
        }
    }
    s.doc_mut().appearances.faces = local;
    blocked.extend(s.doc.authored_face_colors(body).into_iter().map(|(index, _)| index));
    let spots = if source.faces.is_empty() { Vec::new() } else { solidcraft_doc::naming::face_spots(body) };
    let mut assignments = Vec::new();
    for (index, look) in &source.faces {
        if blocked.contains(index) {
            // An appended local override can coexist with the old inherited entry.
            // Keep that baseline as a tombstone if the local override is later removed.
            if let Some(old) = names.get(*index).and_then(|name| previous.iter().find(|old| old.name.as_ref() == Some(name))) {
                let mut old = old.clone();
                old.body = name.into();
                if !inherited.contains(&old) {
                    inherited.push(old);
                }
            }
            continue;
        }
        let Some(face_name) = names.get(*index).filter(|name| !name.is_empty()) else { continue };
        let Some(spot) = spots.get(*index).and_then(|spot| *spot) else { continue };
        let face = FaceLook {
            body: name.into(),
            point: spot.point,
            look: look.clone(),
            name: Some(face_name.clone()),
            strict_name: false,
            certificate: None,
        };
        assignments.push(face.clone());
        inherited.push(face);
    }
    // Replace inherited entries in place; leave all local and other bodies' entries
    // in their original order, including multiple overrides on the same face.
    let mut after = Vec::new();
    for old in &before {
        if replaced.contains(old) {
            if let Some(index) = assignments.iter().position(|face| face.body == old.body && face.name == old.name) {
                after.push(assignments.remove(index));
            }
        } else {
            after.push(old.clone());
        }
    }
    after.extend(assignments);
    let changed = after != before;
    s.doc_mut().appearances.faces = after;
    (inherited, changed)
}

/// Apply properties after evaluation has resolved body-name collisions. Last inherited
/// values are kept independently for material and appearance, so overriding one does not
/// stop updates of the other. Session::execute makes this part of the command's undo step.
fn inherit_properties(s: &mut Session, component: u64, sources: &[SourceProperties]) -> Result<bool> {
    if !s.doc.links.contains_key(&component) {
        return Ok(false);
    }
    let mut prune_candidates = link_inherited_linked_copies(&s.doc);
    let mut changed = consolidate_and_prune_linked_materials(s.doc_mut(), &prune_candidates)?;
    prune_candidates.extend(link_inherited_linked_copies(&s.doc));
    let Some(link) = s.doc.links.get(&component).cloned() else { return Ok(false) };
    let body_slots: Vec<(String, usize)> = {
        let state = s.model.state();
        let alive: std::collections::HashSet<&str> = state.bodies.iter().filter(|b| b.feature == link.feature).map(|b| b.name.as_str()).collect();
        if let Some(r) = s.model.result(link.feature).filter(|r| r.error.is_none() && !r.skipped && !r.stale) {
            let made: Vec<String> = r.state.bodies.iter().filter(|b| b.feature == link.feature).map(|b| b.name.clone()).collect();
            let slots = s.doc.feature(link.feature).map(|f| f.body_name_slots(made.len())).unwrap_or_default();
            made.into_iter().zip(slots).filter(|(name, _)| alive.contains(name.as_str())).collect()
        } else {
            let names: Vec<String> = state.bodies.iter().filter(|b| b.feature == link.feature).map(|b| b.name.clone()).collect();
            let slots = s.doc.feature(link.feature).map(|f| f.body_name_slots(names.len())).unwrap_or_default();
            names.into_iter().zip(slots).collect()
        }
    };
    let mut inherited = link.inherited.clone();
    inherited.resize(sources.len(), LinkedBodyProperties::default());
    let threads = if sources.iter().any(|source| !source.threads.is_empty()) {
        sources.iter().map(|source| source.threads.clone()).collect()
    } else {
        Vec::new()
    };
    if let Some(feature) = s.doc_mut().feature_mut(link.feature)
        && feature.link_threads != threads
    {
        feature.link_threads = threads;
        changed = true;
    }
    for (name, slot) in &body_slots {
        let slot = *slot;
        let Some(source) = sources.get(slot) else { continue };
        let old = link.inherited.get(slot);
        let old_material = old.filter(|old| !old.material.is_empty());
        let current_definition = material_fingerprint(&s.doc, &s.doc.body_material(name))?;
        let may_change_material = match old_material {
            Some(old) => {
                s.doc.materials.get(name).map_or(solidcraft_doc::DEFAULT_MATERIAL, String::as_str) == old.material
                    && current_definition == old.material_fingerprint
            }
            None => !s.doc.materials.contains_key(name),
        };
        let mut next = old.cloned().unwrap_or_default();
        if may_change_material {
            let material = inherit_material(s.doc_mut(), source, old_material.map(|old| old.material.as_str()), &prune_candidates)?;
            changed |= s.doc.materials.get(name).map_or(solidcraft_doc::DEFAULT_MATERIAL, String::as_str) != material;
            s.doc_mut().materials.insert(name.clone(), material.clone());
            next.material_fingerprint = material_fingerprint(&s.doc, &material)?;
            next.material = material;
        }
        if s.doc.appearances.bodies.get(name) == old.and_then(|old| old.appearance.as_ref()) {
            changed |= s.doc.appearances.bodies.get(name) != source.appearance.as_ref();
            match &source.appearance {
                Some(look) => {
                    s.doc_mut().appearances.bodies.insert(name.clone(), look.clone());
                }
                None => {
                    s.doc_mut().appearances.bodies.remove(name);
                }
            }
            next.appearance.clone_from(&source.appearance);
        }
        let (faces, face_changed) = inherit_faces(s, name, source, old);
        next.faces = Some(faces);
        changed |= face_changed;
        if let Some(snapshot) = inherited.get_mut(slot) {
            *snapshot = next;
        }
    }
    if let Some(link) = s.doc_mut().links.get_mut(&component) {
        link.inherited = inherited;
    }
    changed |= consolidate_and_prune_linked_materials(s.doc_mut(), &prune_candidates)?;
    Ok(changed)
}

/// Take the linked design's properties (`source`, its `metadata`, which Insert Design seeds a new
/// component with) into `component`. A property that still has the value the link last took
/// follows the design; one changed in this design stays, and the returned warnings name it when
/// the design's value has moved. A link that took none (made before it kept them) adds only the
/// properties the component lacks. Session::execute makes this part of the command's undo step.
fn inherit_metadata(s: &mut Session, component: u64, source: &solidcraft_doc::Metadata) -> (bool, Vec<String>) {
    let Some(taken) = s.doc.links.get(&component).map(|l| l.inherited_metadata.clone()) else { return (false, Vec::new()) };
    let Some(c) = s.doc.components.iter().find(|c| c.id == component) else { return (false, Vec::new()) };
    let (name, before) = (c.name.clone(), c.metadata.clone());
    let mut after = before.clone();
    let mut warnings = Vec::new();
    let keys: std::collections::BTreeSet<&String> = source.0.keys().chain(taken.0.keys()).collect();
    for key in keys {
        let (theirs, was) = (source.0.get(key), taken.0.get(key));
        let here = after.0.get(key);
        if here == was {
            match theirs {
                Some(value) => after.0.insert(key.clone(), value.clone()),
                None => after.0.remove(key),
            };
        } else if here != theirs && was != theirs {
            let shown = |v: Option<&Value>| v.map_or_else(|| "none".to_string(), Value::to_string);
            warnings.push(format!(
                "{name} keeps its part property `{key}` ({}), set in this design; the linked design now has {}",
                shown(here),
                shown(theirs)
            ));
        }
    }
    let changed = after != before;
    let d = s.doc_mut();
    if changed && let Some(c) = d.components.iter_mut().find(|c| c.id == component) {
        c.metadata = after;
    }
    if let Some(link) = d.links.get_mut(&component) {
        link.inherited_metadata = source.clone();
    }
    (changed, warnings)
}

fn component_key(s: &Session, p: &Value, key: &str, cmd: &str) -> Result<u64> {
    let k = match p.get(key) {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, format!("`{key}` must be a component id or name"))),
    };
    s.doc.resolve_component(&k).map_err(|error| bad(cmd, error.to_string()))?.ok_or_else(|| bad(cmd, format!("no component `{k}`")))
}

/// A command's reply as JSON: they build their replies from the types their output schemas
/// describe, so the two can't drift apart.
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// Placement transform for `component.insert_design`: accepts `to_matrix` or `matrix` (row-major
/// 3x4 or 4x4, with length expressions in the translation column), or `translate`, `axis`, `angle`,
/// and `origin`.
fn placement_transform(s: &Session, p: &Value, cmd: &str) -> Result<(solidcraft_doc::Mat, Option<solidcraft_doc::ExpressedMove>)> {
    let matrix = p.get("matrix");
    let to_matrix = p.get("to_matrix");
    if matrix.is_some() && to_matrix.is_some() {
        return Err(bad(cmd, "give either to_matrix or matrix, not both"));
    }
    if let Some(key) = ["to_matrix", "matrix"].into_iter().find(|k| p.get(*k).is_some()) {
        if let Some(k) = ["translate", "axis", "angle", "origin"].into_iter().find(|k| p.get(*k).is_some()) {
            return Err(bad(cmd, format!("give either {key} or {k}, not both")));
        }
        let m = matrix.or(to_matrix).ok_or_else(|| bad(cmd, "matrix"))?;
        let (numbers, kept) =
            super::component::matrix::with_exprs(m, |e| s.doc.eval(e, solidcraft_doc::expr::Kind::Length).map_err(|err| err.to_string()), cmd)?;
        let tolerance = super::component::matrix::tolerance(p, cmd)?;
        let transform = super::component::matrix::rigid(&numbers, tolerance, cmd)?;
        if !solidcraft_doc::is_placement(&transform) {
            return Err(bad(cmd, "placement transform exceeds supported numeric range"));
        }
        let expressed = if kept.iter().any(Option::is_some) {
            Some(solidcraft_doc::ExpressedMove {
                translate: kept,
                at: [transform[3][0], transform[3][1], transform[3][2]],
                frame: solidcraft_doc::IDENTITY,
                world: false,
                origin: Default::default(),
                origin_at: [0.0; 3],
                rotation: solidcraft_doc::IDENTITY,
                grounded_ok: false,
            })
        } else {
            None
        };
        return Ok((transform, expressed));
    }

    let translate = match p.get("translate") {
        None => solidcore_record::Vec3::ZERO,
        Some(v) => vec3(v).filter(|t| t.is_finite() && t.len() < 1e9).ok_or_else(|| bad(cmd, "`translate` must be [x, y, z] in mm"))?,
    };
    let axis = super::component::move_vector(p, "axis", solidcore_record::Vec3::Z, false, cmd)?;
    let angle = match p.get("angle") {
        Some(Value::String(e)) => s.doc.eval(e, solidcraft_doc::expr::Kind::Angle).map_err(|e| bad(cmd, e.to_string()))?,
        None => 0.0,
        Some(value) => value.as_f64().ok_or_else(|| bad(cmd, "angle must be a number or angle expression"))?,
    };
    let origin = super::component::move_vector(p, "origin", solidcore_record::Vec3::ZERO, true, cmd)?;
    if !(translate.is_finite() && axis.is_finite() && angle.is_finite() && origin.is_finite()) {
        return Err(bad(cmd, "non-finite placement"));
    }
    if angle != 0.0 && axis.normalized().is_none() {
        return Err(bad(cmd, "rotation axis must have a finite nonzero length"));
    }
    let transform = solidcraft_doc::rigid(translate, origin, axis, angle);
    if !solidcraft_doc::is_placement(&transform) {
        return Err(bad(cmd, "placement transform exceeds supported numeric range"));
    }
    Ok((transform, None))
}

fn insert_design(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.insert_design";
    let selection = configuration_key(p, None, cmd)?;
    // The design to link: a file, or the design the caller holds in memory (one open in this
    // session that has no file yet, #16829).
    let (given, held) = match (p.get("path"), p.get("document")) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `path` or `document`, not both")),
        (Some(path), None) => {
            let given = path.as_str().filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
            (Some(given), None)
        }
        (None, Some(document)) => {
            let text = document.as_str().filter(|x| !x.trim().is_empty()).ok_or_else(|| bad(cmd, "`document` must be the design's JSON"))?;
            (None, Some(text.to_string()))
        }
        (None, None) => return Err(bad(cmd, "`path` or `document` is required")),
    };
    let importer = match given {
        Some(given) => insert_importer(s, p, given)?,
        None => None,
    };
    if held.is_some() && p.get("format").is_some_and(|f| !f.is_null()) {
        return Err(bad(cmd, "`format` applies to a file an importer plugin reads, not to `document`"));
    }
    if importer.is_some() && selection.is_some() {
        return Err(bad(cmd, "`configuration` selects a row of a design; a file an importer reads has none"));
    }
    let options = match p.get("options") {
        None | Some(Value::Null) => json!({}),
        Some(o) if o.is_object() && importer.is_some() => o.clone(),
        Some(_) if importer.is_none() => return Err(bad(cmd, "`options` applies to a file an importer plugin reads")),
        Some(_) => return Err(bad(cmd, "`options` must be an object")),
    };
    let (transform, expressed) = placement_transform(s, p, cmd)?;
    let source = match (given, held) {
        (Some(given), None) => {
            // A network or device path is judged from its text, as given, before anything reads it.
            link_refused(s.path.as_deref(), given).map_err(|e| bad(cmd, e.to_string()))?;
            let file = absolute(Path::new(given));
            if s.path.as_deref().is_some_and(|own| same_file(Path::new(own), &file)) {
                return Err(bad(cmd, "a design can't be linked into itself"));
            }
            Source::File(file)
        }
        (None, Some(text)) => {
            let name = str_(p, "name").filter(|n| !n.trim().is_empty()).unwrap_or("the design").to_string();
            Source::Session { name, text }
        }
        // One of the two is always there: the pair comes from the match above.
        (_, _) => return Err(bad(cmd, "`path` or `document` is required")),
    };
    // Named selections must be resolved against the source before deciding which definition
    // to reuse. Keep the same bytes for evaluation, so the selection and bodies agree.
    let selected_bytes = match selection {
        Some(_) => Some(source.bytes(s.path.as_deref())?),
        None => None,
    };
    let configuration = match &selected_bytes {
        Some(bytes) => configuration_id(bytes, selection).map_err(|e| bad(cmd, e.to_string()))?,
        None => None,
    };
    if p.get("reuse").and_then(Value::as_bool).unwrap_or(false) {
        // A design in memory has no file to be found by: every link to a file is another design.
        let existing = match &source {
            Source::File(file) => {
                let target = link_target(file);
                s.doc
                    .links
                    .iter()
                    .find(|(_, link)| {
                        selected_configuration(link) == configuration
                            && link_target(&resolve(s.path.as_deref(), &link.path)) == target
                            && link.importer.as_ref().map(|i| (&i.format, linked_options(&i.options)))
                                == importer.as_ref().map(|i| (&i.id, linked_options(&options)))
                    })
                    .map(|(id, _)| *id)
            }
            Source::Session { .. } => None,
        };
        if let Some(component) = existing {
            let parent = if p.get("parent").is_some() { component_key(s, p, "parent", cmd)? } else { s.active_component };
            // Reuse the definition, not another placement's captured or pending move.
            // Insert Design places from the parent origin whether the link is new or reused.
            let occurrence = s.doc_mut().add_occurrence(component, parent, transform)?;
            if let Some(expressed) = expressed
                && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occurrence)
            {
                o.expressed_moves.push(expressed);
            }
            return reply(cmd, InsertDesignResult { inserted: DesignInserted::Reused { occurrence, component }, param_warnings: Vec::new() });
        }
    }
    let bytes = match selected_bytes {
        Some(bytes) => bytes,
        None => match (&importer, source.file()) {
            (Some(_), Some(file)) => read_link(file, s.path.as_deref(), true)?,
            _ => source.bytes(s.path.as_deref())?,
        },
    };
    // An importer's file is read by the plugin, which writes the design the bodies come from; the
    // link keeps what the file's own bytes were, so a changed file is told from an unchanged one.
    let (record, produced) = match &importer {
        Some(importer) => {
            let file = source.file().ok_or_else(|| bad(cmd, "an importer reads a file, not a `document`"))?;
            // The caller named this file: Update Links may read it again in this session.
            s.plugin_state.linked_files.insert(link_target(file));
            let made = crate::plugin::import_design_bytes(s, importer, &file.to_string_lossy(), &options)?;
            // The plugin read the file itself: what it read is what the link's digest says.
            let again = read_link(file, s.path.as_deref(), true)?;
            if DesignLink::file_digest(&again) != DesignLink::file_digest(&bytes) {
                return Err(bad(cmd, format!("{}: the file changed while it was being read; insert it again", file.display())));
            }
            let record = solidcraft_doc::LinkImporter {
                format: importer.id.clone(),
                plugin: Some(importer.plugin_id.clone()),
                version: Some(made.version),
                options: linked_options(&options).clone(),
            };
            (Some(record), Some((made.design, made.notes)))
        }
        None => (None, None),
    };
    let design_bytes = produced.as_ref().map_or(bytes.as_slice(), |(design, _)| design.as_slice());
    let ((design_name, mut sf, faces, metadata, properties, meshes), (images, image_warnings)) =
        design_bodies(&source, design_bytes, configuration, s.validation())?;
    if let Some((_, notes)) = produced {
        // What `design_bodies` remembers is keyed by the bytes it read: for this link, the file's.
        if let Some(file) = source.file() {
            remember_read(file, None, digest(bytes.as_slice()), &sf, &faces, &properties, &metadata, &meshes);
        }
        sf.warnings.extend(notes);
    }
    sf.warnings.extend(image_warnings);
    let read = DesignLink::file_digest(&bytes);
    let name = str_(p, "name").filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or(design_name);
    let mut create = json!({"name": name, "activate": false});
    // Clipboard insertion uses reuse even for the first paste, which must stay movable.
    if p.get("reuse").and_then(Value::as_bool).unwrap_or(false) {
        create["grounded"] = json!(false);
    }
    if let Some(parent) = p.get("parent") {
        create["parent"] = parent.clone();
    }
    let made = super::component::new_component(s, &create)?;
    let comp = made["component"].as_u64().ok_or_else(|| bad(cmd, "the component was not created"))?;
    let occ = made["occurrence"].as_u64();
    // Seed only a new definition: reused links keep any locally edited part properties.
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == comp) {
        c.metadata = metadata.clone();
    }
    let id = s.doc_mut().add_feature(sf.kind, Some(&name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = comp;
        f.body_names = sf.body_names;
        f.link_faces = faces;
    }
    set_link_meshes(s, comp, id, &name, &source.shown(), meshes)?;
    let mesh_feature = link_mesh_feature(s, comp, id).map(|(f, _)| f);
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| Some(o.id) == occ) {
        o.transform = transform;
        if let Some(expressed) = expressed {
            o.expressed_moves.push(expressed);
        }
    }
    // A design in memory has no file: the link names none, and keeps the bodies it brought in.
    let (path, link_path) = match &source {
        Source::File(file) => (stored(s.path.as_deref(), file), file.to_string_lossy().into_owned()),
        Source::Session { .. } => (String::new(), String::new()),
    };
    s.doc_mut().links.insert(
        comp,
        DesignLink {
            path: link_path,
            feature: id,
            configuration: configuration.map(|c| c.row),
            configuration_identity: configuration.and_then(|c| c.identity),
            inherited: Vec::new(),
            inherited_metadata: metadata,
            file_sha256: Some(read),
            step_names: None,
            importer: record,
        },
    );
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    inherit_properties(s, comp, &properties)?;
    add_design_images(s, comp, &name, images)?;
    s.refresh();
    let st = s.model.state();
    let bodies: Vec<String> = st.bodies.iter().filter(|b| b.feature == id || Some(b.feature) == mesh_feature).map(|b| b.name.clone()).collect();
    let inserted = DesignInserted::New {
        component: comp,
        occurrence: occ,
        name,
        feature: id,
        path,
        created_bodies: bodies.clone(),
        bodies,
        warnings: sf.warnings,
    };
    reply(cmd, InsertDesignResult { inserted, param_warnings: Vec::new() })
}

/// The importer plugin that reads the file `component.insert_design` is given: the one `format`
/// names, or else the one that claims the file's extension when it is not a design's. `None`: it
/// is read as a design.
pub(super) fn insert_importer(s: &Session, p: &Value, given: &str) -> Result<Option<crate::plugin::Importer>> {
    let format = str_(p, "format");
    let design =
        Path::new(given).extension().and_then(|x| x.to_str()).is_some_and(|x| ["sc", "solidcraft", "json"].iter().any(|d| x.eq_ignore_ascii_case(d)));
    if format.is_none() && design {
        return Ok(None);
    }
    crate::plugin::importer_for(s, given, format)
}

/// The options a link keeps: none (null) when an importer was given none.
fn linked_options(options: &Value) -> &Value {
    if options.as_object().is_some_and(|o| o.is_empty()) { &Value::Null } else { options }
}

/// The linked design's canvases and decals ([`design_images`]) added to its new component
/// `comp`, so its images come into the assembly on its bodies. Each gets an id of this design and
/// keeps its name, or takes `<name> (<component>)` when this design already has an image so named.
fn add_design_images(s: &mut Session, comp: u64, component: &str, images: Vec<solidcraft_doc::canvas::Canvas>) -> Result<()> {
    for mut image in images {
        let doc = s.doc_mut();
        image.id = doc.allocate_id()?;
        image.component = comp;
        let taken = |n: &str| doc.canvases.iter().any(|c| c.name == n);
        if taken(&image.name) {
            let base = format!("{} ({component})", image.name);
            image.name = (1..=doc.canvases.len().saturating_add(1))
                .map(|n| if n == 1 { base.clone() } else { format!("{base} {n}") })
                .find(|n| !taken(n))
                .unwrap_or(base);
        }
        doc.canvases.push(image);
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct UpdateSourceKey {
    canonical: PathBuf,
    configuration: Option<SelectedConfiguration>,
    step_names: Option<String>,
    flat: bool,
}

#[derive(Clone)]
struct EvaluatedLink {
    kind: FeatureKind,
    body_names: Vec<String>,
    warnings: Vec<String>,
    /// Held alive until `s.refresh()` so timeline evaluation reuses the parsed STEP import.
    import: std::sync::Arc<solidcraft_kernel::StepImport>,
    faces: Vec<Vec<String>>,
    metadata: solidcraft_doc::Metadata,
    properties: Vec<SourceProperties>,
    meshes: Vec<MeshData>,
}

impl EvaluatedLink {
    fn from_linked(linked: Linked) -> Self {
        let (_, sf, faces, metadata, properties, meshes) = linked;
        Self { kind: sf.kind, body_names: sf.body_names, warnings: sf.warnings, import: sf.import, faces, metadata, properties, meshes }
    }
}

enum TargetLink {
    SessionOnly {
        cname: String,
    },
    Failed {
        comp: u64,
        cname: String,
        error: String,
    },
    Kicad {
        comp: u64,
        cname: String,
        link: DesignLink,
        importer: solidcraft_doc::LinkImporter,
    },
    Importer {
        comp: u64,
        cname: String,
        link: DesignLink,
        old: ImportSource,
        file: PathBuf,
        before: Option<solidcraft_doc::ModelState>,
    },
    Native {
        comp: u64,
        cname: String,
        link: DesignLink,
        old: ImportSource,
        file: PathBuf,
        canonical: PathBuf,
        flat: bool,
        before: Option<solidcraft_doc::ModelState>,
    },
}

enum PostRefreshEntry {
    Done { cname: String, updated: bool },
    Inherit { comp: u64, cname: String, properties: Vec<SourceProperties>, changed: bool },
}

struct EvalJob {
    key: UpdateSourceKey,
    display_name: String,
    slot: usize,
    file: PathBuf,
    bytes: std::sync::Arc<Vec<u8>>,
    configuration: Option<SelectedConfiguration>,
    step_names: Option<String>,
    flat: bool,
}

struct ApplyContext<'a> {
    s: &'a mut Session,
    link_warnings: &'a mut [Vec<String>],
    kept_imports: &'a mut Vec<std::sync::Arc<solidcraft_kernel::StepImport>>,
    rebind_queue: &'a mut Vec<(usize, u64, Option<solidcraft_doc::ModelState>)>,
    tie_queue: &'a mut Vec<(usize, String, Vec<TieNumberedJointSide>)>,
    realigned_features: &'a mut std::collections::HashSet<u64>,
    post_entries: &'a mut Vec<PostRefreshEntry>,
    read_components: &'a mut Vec<u64>,
    needs_first_refresh: &'a mut bool,
}

fn link_threads_for(sources: &[SourceProperties]) -> Vec<Vec<solidcraft_doc::ThreadInfo>> {
    if sources.iter().any(|source| !source.threads.is_empty()) { sources.iter().map(|source| source.threads.clone()).collect() } else { Vec::new() }
}

fn apply_link_threads(s: &mut Session, feature: u64, sources: &[SourceProperties]) -> bool {
    let threads = link_threads_for(sources);
    if s.doc.feature(feature).is_some_and(|f| f.link_threads != threads) {
        if let Some(f) = s.doc_mut().feature_mut(feature) {
            f.link_threads = threads;
        }
        true
    } else {
        false
    }
}

fn eval_job_sequential(job: &EvalJob, validation: solidcraft_kernel::Validation) -> std::result::Result<EvaluatedLink, String> {
    let probe = DesignLink {
        configuration: job.configuration.map(|c| c.row),
        configuration_identity: job.configuration.and_then(|c| c.identity),
        step_names: job.step_names.clone(),
        ..Default::default()
    };
    source_bodies(&job.file, &job.bytes, &probe, job.flat, validation)
        .map(|(linked, _)| EvaluatedLink::from_linked(linked))
        .map_err(|e| e.to_string())
}

fn apply_evaluated_link(
    ctx: &mut ApplyContext<'_>,
    slot: usize,
    comp: u64,
    cname: String,
    link: &DesignLink,
    old: &ImportSource,
    file: &Path,
    before: Option<solidcraft_doc::ModelState>,
    eval: EvaluatedLink,
) -> Result<()> {
    ctx.kept_imports.push(eval.import);
    ctx.link_warnings[slot].extend(eval.warnings);
    let (took, kept_metadata) = inherit_metadata(ctx.s, comp, &eval.metadata);
    ctx.link_warnings[slot].extend(kept_metadata);
    let mesh_changed = set_link_meshes(ctx.s, comp, link.feature, &cname, &file.display().to_string(), eval.meshes)?;
    if mesh_changed {
        *ctx.needs_first_refresh = true;
    }
    let threads_changed = apply_link_threads(ctx.s, link.feature, &eval.properties);
    if threads_changed {
        *ctx.needs_first_refresh = true;
    }
    let took = mesh_changed || threads_changed || took;
    let same = matches!(&eval.kind, FeatureKind::Import { step, records, .. } if *step == old.0 && *records == old.1);
    let kept = ctx.s.doc.feature(link.feature).map(|f| &f.link_faces);
    if same && kept == Some(&eval.faces) {
        ctx.post_entries.push(PostRefreshEntry::Inherit { comp, cname, properties: eval.properties, changed: took });
        ctx.read_components.push(comp);
        return Ok(());
    }
    // The same bodies, their faces named for the first time (a link made before names were
    // kept): nothing a reference finds changes. Names the design gave other faces since do.
    let changed = !same || kept.is_some_and(|k| !k.is_empty()) || took;
    // Joints on faces told apart only by a positional tie number, and where they were, so a
    // shift this update gives them is said instead of passing unnoticed.
    let tie_before = if same { Vec::new() } else { snapshot_tie_numbered_joints(ctx.s, comp, link.feature) };
    if !same {
        for name in numbered_references(ctx.s, comp, link.feature) {
            ctx.link_warnings[slot].push(format!(
                "{name} refers to faces of {cname} by their place in the linked file, which this update can change: check it, and pick its faces again so it follows them by name"
            ));
        }
        for name in tie_numbered_origin_references(ctx.s, comp, link.feature) {
            ctx.link_warnings[slot].push(format!(
                "{name} refers to a positional tie-numbered face of {cname}, which can shift when sibling faces are added or removed: check it, and pick its face again"
            ));
        }
    }
    let mut moved = None;
    let mut realigned = None;
    // The old names are read as the new ones were: a STEP link's own naming (source_bodies).
    let naming = link.step_names.as_deref().and_then(solidcraft_io::StepNames::parse).unwrap_or_default();
    let (old_names, other_bodies) = feature_body_context_excluding(ctx.s, link.feature, ctx.realigned_features);
    let mut unpinned_inherited = if old_names.is_empty() && link.step_names.is_none() {
        realign_unpinned_inherited(&ctx.s.doc, &link.inherited, &eval.body_names)
    } else {
        None
    };
    let mut count_changed = false;
    if let Some(f) = ctx.s.doc_mut().feature_mut(link.feature) {
        if !same {
            // Bodies keep their names where the design still has as many, also when it lists
            // them in another order (#21371).
            if old_names.len() != eval.body_names.len() {
                count_changed = true;
                if old_names.is_empty() {
                    f.body_names = eval.body_names;
                    if let Some(next_inherited) = unpinned_inherited.take() {
                        realigned = Some((next_inherited, Vec::new()));
                    }
                } else {
                    let (next_names, next_inherited, removed) = realign_changed_body_count(
                        &old.0,
                        &file.display().to_string(),
                        naming,
                        &old_names,
                        &link.inherited,
                        &eval.body_names,
                        &other_bodies,
                    );
                    f.body_names = next_names;
                    realigned = Some((next_inherited, removed));
                }
            } else if let Some(order) = moved_bodies(&old.0, &file.display().to_string(), naming, &eval.body_names) {
                f.body_names = reordered(&old_names, &order);
                moved = Some(order);
            } else if f.body_names.is_empty() && !old_names.is_empty() {
                f.body_names = old_names;
            }
            f.kind = eval.kind;
        }
        // A link made before the design's face names were kept gets them here.
        f.link_faces = eval.faces;
    }
    if count_changed {
        ctx.realigned_features.insert(link.feature);
    }
    // What the link last took from each body moves with it, so a body's own override stays one.
    if let Some((next_inherited, removed)) = realigned {
        if link.step_names.is_none()
            && let Some(kept) = ctx.s.doc_mut().links.get_mut(&comp)
        {
            kept.inherited = next_inherited;
        }
        remove_stale_body_properties(ctx.s.doc_mut(), link.feature, &removed, &other_bodies);
    } else if let Some(order) = moved
        && let Some(kept) = ctx.s.doc_mut().links.get_mut(&comp)
        && !kept.inherited.is_empty()
    {
        kept.inherited = reordered(&kept.inherited, &order);
    }
    *ctx.needs_first_refresh = true;
    if !same {
        ctx.rebind_queue.push((slot, link.feature, before));
    }
    if !tie_before.is_empty() {
        ctx.tie_queue.push((slot, cname.clone(), tie_before));
    }
    ctx.post_entries.push(PostRefreshEntry::Inherit { comp, cname, properties: eval.properties, changed });
    ctx.read_components.push(comp);
    Ok(())
}

fn update_links(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.update_links";
    let only = match (p.get("component"), p.get("components")) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `component` or `components`, not both")),
        (Some(_), None) => Some(vec![component_key(s, p, "component", cmd)?]),
        (None, Some(Value::Array(list))) if !list.is_empty() && list.len() <= 10_000 => {
            Some(list.iter().map(|c| component_key(s, &json!({"component": c}), "component", cmd)).collect::<Result<Vec<_>>>()?)
        }
        (None, Some(_)) => return Err(bad(cmd, "`components` must be a list of component ids or names")),
        (None, None) => None,
    };
    if let Some(c) = only.iter().flatten().find(|c| !s.doc.links.contains_key(c)) {
        let name = s.doc.components.iter().find(|x| x.id == *c).map_or_else(|| c.to_string(), |x| x.name.clone());
        return Err(bad(cmd, format!("{name}: that component is not linked to a design")));
    }
    // A batch goes on past a source it cannot read: that link keeps its bodies and is named in
    // `failed`, so the others still come in as one step (#9743).
    let go_on = p.get("component").is_none();
    let failing_before: std::collections::BTreeMap<u64, String> = s.model.results.iter().filter_map(|r| Some((r.id, r.error.clone()?))).collect();
    // Linked designs saved without bodies of their own follow the caller's rule as they open.
    let _reading = Opening::enter(s.path.as_deref(), LinkedRead::read(p, cmd)?);
    let links: Vec<(u64, DesignLink)> =
        s.doc.links.iter().filter(|(c, _)| only.as_ref().is_none_or(|o| o.contains(c))).map(|(c, l)| (*c, l.clone())).collect();
    let own = s.path.clone();
    let validation = s.validation();
    let doc_before_update = s.doc.clone();

    // Phase 1: classify target links and snapshot `before` states upfront before any mutation.
    let mut targets: Vec<TargetLink> = Vec::with_capacity(links.len());
    for (comp, link) in links {
        if solidcraft_doc::progress::cancelled() {
            break;
        }
        let cname = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone());
        let Some(cname) = cname else {
            // The component was deleted: the link goes with it.
            s.doc_mut().links.remove(&comp);
            continue;
        };
        if link.path.is_empty() {
            targets.push(TargetLink::SessionOnly { cname });
            continue;
        }
        // A KiCad board is read by SolidCraft itself, with the options it was opened with.
        if let Some(importer) = link.importer.as_ref().filter(|i| i.is_kicad()).cloned() {
            targets.push(TargetLink::Kicad { comp, cname, link, importer });
            continue;
        }
        let Some(FeatureKind::Import { step: old_step, records: old_records, .. }) =
            s.doc.feature(link.feature).filter(|f| f.component == comp).map(|f| &f.kind)
        else {
            let error = format!("its linked bodies (feature {}) are gone; break the link or insert the design again", link.feature);
            if !go_on {
                return Err(bad(cmd, format!("{cname}: {error}")));
            }
            targets.push(TargetLink::Failed { comp, cname, error });
            continue;
        };
        if let Err(e) = link_refused(own.as_deref(), &link.path) {
            let error = e.to_string();
            if !go_on {
                return Err(bad(cmd, format!("{cname}: {error}")));
            }
            targets.push(TargetLink::Failed { comp, cname, error });
            continue;
        }
        let old = (old_step.clone(), old_records.clone());
        let file = resolve(own.as_deref(), &link.path);
        let before = s.model.result(link.feature).map(|r| (*r.state).clone());
        if link.importer.is_some() {
            if let Some(error) = importer_file_refusal(s, &file) {
                if !go_on {
                    return Err(bad(cmd, format!("{cname}: {error}")));
                }
                targets.push(TargetLink::Failed { comp, cname, error });
                continue;
            }
            targets.push(TargetLink::Importer { comp, cname, link, old, file, before });
        } else {
            // Safe: `link_refused` already authorized this path before touching disk.
            let canonical = real_file(&file).unwrap_or_else(|| absolute(&file));
            let flat = step_import_flat(&s.doc, &link);
            targets.push(TargetLink::Native { comp, cname, link, old, file, canonical, flat, before });
        }
    }

    // Phase 2A: deduplicate and read/hash unique non-importer files in parallel.
    let mut file_jobs: Vec<((PathBuf, Option<std::ffi::OsString>), PathBuf)> = Vec::new();
    let mut seen_files = std::collections::HashSet::new();
    for target in &targets {
        if let TargetLink::Native { file, canonical, .. } = target {
            let key = (canonical.clone(), file.extension().map(std::ffi::OsStr::to_os_string));
            if seen_files.insert(key.clone()) {
                file_jobs.push((key, file.clone()));
            }
        }
    }
    let read_results = map_link_work(&file_jobs, |(_, file)| {
        let bytes = read_link(file, own.as_deref(), false).map_err(|e| e.to_string())?;
        let sha256 = DesignLink::file_digest(&bytes);
        let bytes_hash = digest(bytes.as_slice());
        Ok::<_, String>((std::sync::Arc::new(bytes), sha256, bytes_hash))
    });
    type FileReadResult = std::result::Result<(std::sync::Arc<Vec<u8>>, String, u64), String>;
    let mut file_reads: std::collections::HashMap<(PathBuf, Option<std::ffi::OsString>), FileReadResult> =
        std::collections::HashMap::with_capacity(file_jobs.len());
    for ((key, _), result) in file_jobs.into_iter().zip(read_results) {
        let entry = result.unwrap_or_else(|| Err("linked file read failed".to_string()));
        file_reads.insert(key, entry);
    }

    // Phase 2B: check per-link `unchanged_read` cache and deduplicate unique sources that need evaluation.
    let mut native_cached: Vec<Option<(Vec<String>, Vec<SourceProperties>, solidcraft_doc::Metadata)>> = vec![None; targets.len()];
    let mut eval_jobs: Vec<EvalJob> = Vec::new();
    let mut seen_sources = std::collections::HashSet::new();
    for (slot, target) in targets.iter().enumerate() {
        let TargetLink::Native { comp, cname, link, old, file, canonical, flat, .. } = target else { continue };
        let read_key = (canonical.clone(), file.extension().map(std::ffi::OsStr::to_os_string));
        let Some(Ok((bytes, _, bytes_hash))) = file_reads.get(&read_key) else { continue };
        // A link that has not recorded the properties it inherited (made before they were) is read again.
        let kept_faces = s
            .doc
            .feature(link.feature)
            .filter(|_| link.step_names.is_some() || (!link.inherited.is_empty() && link.inherited.iter().all(|p| p.faces.is_some())))
            .map(|f| digest(f.link_faces.as_slice()));
        if let Some(known) = kept_faces.and_then(|faces| {
            let meshes = link_meshes(s, *comp, link.feature);
            unchanged_read(file, selected_configuration(link), *bytes_hash, digest(old.0.as_bytes()), faces, meshes)
        }) {
            native_cached[slot] = Some(known);
            continue;
        }
        let configuration = selected_configuration(link);
        let source_key = UpdateSourceKey { canonical: canonical.clone(), configuration, step_names: link.step_names.clone(), flat: *flat };
        if seen_sources.insert(source_key.clone()) {
            eval_jobs.push(EvalJob {
                key: source_key,
                display_name: cname.clone(),
                slot,
                file: file.clone(),
                bytes: bytes.clone(),
                configuration,
                step_names: link.step_names.clone(),
                flat: *flat,
            });
        }
    }

    // Phase 2C: evaluate unique changed/uncached sources in parallel (including nested left-out
    // sub-assemblies via `BranchReading` and `SharedReads`).
    let count = targets.len();
    let mut evaluated: std::collections::HashMap<UpdateSourceKey, std::result::Result<(PathBuf, EvaluatedLink), String>> =
        std::collections::HashMap::with_capacity(eval_jobs.len());
    let cores = link_cores();
    if eval_jobs.len() > 1 && cores > 1 && !solidcraft_doc::progress::cancelled() {
        struct EvalJobOut {
            result: Option<std::result::Result<EvaluatedLink, String>>,
            #[cfg(test)]
            evals: usize,
            #[cfg(test)]
            reuses: usize,
            #[cfg(test)]
            step_reads: usize,
        }
        let (parent_opening, parent_which, shared_read) = READING.with(|r| {
            let r = r.borrow();
            (r.opening.clone(), r.which, r.read.clone())
        });
        let workers = cores.min(eval_jobs.len());
        let child_budget = (cores / workers.max(1)).max(1);
        let progress = solidcraft_doc::progress::current();
        let parent_tid = std::thread::current().id();
        let allowed_hosts = solidcraft_io::vfs::current_allowed_hosts();
        // Workers start unconfined: carry the caller's file scope so linked reads stay inside it.
        let scope = solidcraft_io::vfs::current_scope();
        let (pool_depth, _pool_guard) = shared_read.as_ref().map_or((0, None), |c| {
            let (d, g) = c.enter_pool(parent_tid);
            (d, Some(g))
        });
        let par_results = solidcraft_kernel::map_on_threads(&eval_jobs, workers, |job| -> EvalJobOut {
            solidcraft_io::vfs::scoped(scope.clone(), || {
                solidcraft_io::vfs::allowing_hosts(&allowed_hosts, || {
                    solidcraft_doc::progress::watch_nested(progress.clone(), || {
                        if solidcraft_doc::progress::cancelled() || solidcraft_kernel::watch::cancelled() {
                            return EvalJobOut {
                                result: None,
                                #[cfg(test)]
                                evals: 0,
                                #[cfg(test)]
                                reuses: 0,
                                #[cfg(test)]
                                step_reads: 0,
                            };
                        }
                        // A window running this off its thread names the design being read again (#5329).
                        solidcraft_doc::progress::item(Some((&job.display_name, job.slot, count)));
                        let branch =
                            BranchReading::enter(parent_opening.clone(), parent_which, child_budget, shared_read.clone(), parent_tid, pool_depth);
                        let run = || eval_job_sequential(job, validation);
                        #[cfg(test)]
                        let (result, evals, reuses, step_reads) = measure_eval_counts(run);
                        #[cfg(not(test))]
                        let result = run();
                        let _ = branch.finish();
                        EvalJobOut {
                            result: Some(result),
                            #[cfg(test)]
                            evals,
                            #[cfg(test)]
                            reuses,
                            #[cfg(test)]
                            step_reads,
                        }
                    })
                })
            })
        });
        for (job, outcome) in eval_jobs.into_iter().zip(par_results) {
            let Some(out) = outcome else {
                if !solidcraft_doc::progress::cancelled() {
                    solidcraft_doc::progress::item(Some((&job.display_name, job.slot, count)));
                    let res = eval_job_sequential(&job, validation);
                    evaluated.insert(job.key, res.map(|eval| (job.file, eval)));
                }
                continue;
            };
            #[cfg(test)]
            add_eval_counts(out.evals, out.reuses, out.step_reads);
            if let Some(res) = out.result {
                evaluated.insert(job.key, res.map(|eval| (job.file, eval)));
            }
        }
    } else {
        for job in eval_jobs {
            if solidcraft_doc::progress::cancelled() {
                break;
            }
            // A window running this off its thread names the design being read again (#5329).
            solidcraft_doc::progress::item(Some((&job.display_name, job.slot, count)));
            let res = eval_job_sequential(&job, validation).map(|eval| (job.file, eval));
            evaluated.insert(job.key, res);
        }
    }

    // Phase 3: apply all mutations in exact deterministic order without refreshing inside the loop.
    let mut link_warnings: Vec<Vec<String>> = vec![Vec::new(); count];
    let mut failed: Vec<LinkFailure> = Vec::new();
    let mut read_components: Vec<u64> = Vec::new();
    let mut kept_imports: Vec<std::sync::Arc<solidcraft_kernel::StepImport>> = Vec::new();
    let mut rebind_queue: Vec<(usize, u64, Option<solidcraft_doc::ModelState>)> = Vec::new();
    let mut tie_queue: Vec<(usize, String, Vec<TieNumberedJointSide>)> = Vec::new();
    let mut realigned_features: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut post_entries: Vec<PostRefreshEntry> = Vec::new();
    let mut needs_first_refresh = false;

    for (slot, target) in targets.into_iter().enumerate() {
        if solidcraft_doc::progress::cancelled() {
            break;
        }
        match target {
            TargetLink::SessionOnly { cname } => {
                solidcraft_doc::progress::item(Some((&cname, slot, count)));
                // A design that came from this session has no file to read: it keeps its bodies,
                // and says so.
                link_warnings[slot].push(format!("{cname} keeps its bodies: its design came from this session and has no file yet"));
            }
            TargetLink::Failed { comp, cname, error } => {
                solidcraft_doc::progress::item(Some((&cname, slot, count)));
                failed.push(LinkFailure { component: comp, name: cname.clone(), error: error.clone() });
                link_warnings[slot].push(format!("{cname} keeps its bodies: {error}"));
            }
            TargetLink::Kicad { comp, cname, link, importer } => {
                solidcraft_doc::progress::item(Some((&cname, slot, count)));
                match update_kicad_link(s, comp, &link, &importer) {
                    Ok((true, notes)) => {
                        link_warnings[slot].extend(notes);
                        post_entries.push(PostRefreshEntry::Done { cname, updated: true });
                        read_components.push(comp);
                    }
                    Ok((false, _)) => {
                        post_entries.push(PostRefreshEntry::Done { cname, updated: false });
                        read_components.push(comp);
                    }
                    Err(e) if go_on => {
                        failed.push(LinkFailure { component: comp, name: cname.clone(), error: e.clone() });
                        link_warnings[slot].push(format!("{cname} keeps its bodies: {e}"));
                    }
                    Err(e) => return Err(bad(cmd, format!("{cname}: {e}"))),
                }
            }
            TargetLink::Importer { comp, cname, link, old, file, before } => {
                solidcraft_doc::progress::item(Some((&cname, slot, count)));
                let bytes = match read_link(&file, s.path.as_deref(), true) {
                    Ok(bytes) => bytes,
                    Err(e) if go_on => {
                        let error = e.to_string();
                        failed.push(LinkFailure { component: comp, name: cname.clone(), error: error.clone() });
                        link_warnings[slot].push(format!("{cname} keeps its bodies: {error}"));
                        continue;
                    }
                    Err(e) => return Err(bad(cmd, format!("{cname}: {e}"))),
                };
                let read = DesignLink::file_digest(&bytes);
                let same_bytes = link.file_sha256.as_deref() == Some(read.as_str());
                // A file an importer read that is the file it read is not read again: no plugin runs, so a
                // plugin that is gone or turned off has nothing to fail at.
                if same_bytes {
                    post_entries.push(PostRefreshEntry::Done { cname, updated: false });
                    read_components.push(comp);
                    continue;
                }
                let kept_faces = s
                    .doc
                    .feature(link.feature)
                    .filter(|_| link.step_names.is_some() || (!link.inherited.is_empty() && link.inherited.iter().all(|p| p.faces.is_some())))
                    .map(|f| digest(f.link_faces.as_slice()));
                if let Some((known, properties, metadata)) = kept_faces.and_then(|faces| {
                    let meshes = link_meshes(s, comp, link.feature);
                    unchanged_read(&file, selected_configuration(&link), digest(bytes.as_slice()), digest(old.0.as_bytes()), faces, meshes)
                }) {
                    if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
                        kept.file_sha256 = Some(read);
                    }
                    link_warnings[slot].extend(known);
                    let (took, kept) = inherit_metadata(s, comp, &metadata);
                    link_warnings[slot].extend(kept);
                    let threads_changed = apply_link_threads(s, link.feature, &properties);
                    if threads_changed {
                        needs_first_refresh = true;
                    }
                    post_entries.push(PostRefreshEntry::Inherit { comp, cname, properties, changed: took || threads_changed });
                    read_components.push(comp);
                    continue;
                }
                let linked = match linked_bodies(s, comp, &file, &bytes, &link) {
                    Ok((read_linked, _)) => read_linked,
                    Err(e) if go_on => {
                        let error = e.to_string();
                        failed.push(LinkFailure { component: comp, name: cname.clone(), error: error.clone() });
                        link_warnings[slot].push(format!("{cname} keeps its bodies: {error}"));
                        continue;
                    }
                    Err(e) => return Err(bad(cmd, format!("{cname}: {e}"))),
                };
                if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
                    kept.file_sha256 = Some(read);
                }
                let mut ctx = ApplyContext {
                    s,
                    link_warnings: &mut link_warnings,
                    kept_imports: &mut kept_imports,
                    rebind_queue: &mut rebind_queue,
                    tie_queue: &mut tie_queue,
                    realigned_features: &mut realigned_features,
                    post_entries: &mut post_entries,
                    read_components: &mut read_components,
                    needs_first_refresh: &mut needs_first_refresh,
                };
                apply_evaluated_link(&mut ctx, slot, comp, cname, &link, &old, &file, before, EvaluatedLink::from_linked(linked))?;
            }
            TargetLink::Native { comp, cname, link, old, file, canonical, flat, before } => {
                solidcraft_doc::progress::item(Some((&cname, slot, count)));
                let read_key = (canonical.clone(), file.extension().map(std::ffi::OsStr::to_os_string));
                let (sha256, bytes_hash) = match file_reads.get(&read_key) {
                    Some(Ok((_, sha256, bytes_hash))) => (sha256.clone(), *bytes_hash),
                    Some(Err(e)) if go_on => {
                        failed.push(LinkFailure { component: comp, name: cname.clone(), error: e.clone() });
                        link_warnings[slot].push(format!("{cname} keeps its bodies: {e}"));
                        continue;
                    }
                    Some(Err(e)) => return Err(bad(cmd, format!("{cname}: {e}"))),
                    None => {
                        let error = "linked file read failed".to_string();
                        if go_on {
                            failed.push(LinkFailure { component: comp, name: cname.clone(), error: error.clone() });
                            link_warnings[slot].push(format!("{cname} keeps its bodies: {error}"));
                            continue;
                        }
                        return Err(bad(cmd, format!("{cname}: {error}")));
                    }
                };
                let same_bytes = link.file_sha256.as_deref() == Some(sha256.as_str());
                if let Some((known, properties, metadata)) = native_cached[slot].take() {
                    if !same_bytes && let Some(kept) = s.doc_mut().links.get_mut(&comp) {
                        kept.file_sha256 = Some(sha256);
                    }
                    link_warnings[slot].extend(known);
                    // Another component or session may have evaluated this source first. Its
                    // cached geometry says nothing about this link's inherited properties.
                    let (took, kept) = inherit_metadata(s, comp, &metadata);
                    link_warnings[slot].extend(kept);
                    let threads_changed = apply_link_threads(s, link.feature, &properties);
                    if threads_changed {
                        needs_first_refresh = true;
                    }
                    post_entries.push(PostRefreshEntry::Inherit { comp, cname, properties, changed: took || threads_changed });
                    read_components.push(comp);
                    continue;
                }
                let configuration = selected_configuration(&link);
                let source_key = UpdateSourceKey { canonical, configuration, step_names: link.step_names.clone(), flat };
                let (eval_file, eval) = match evaluated.get(&source_key) {
                    Some(Ok((eval_file, eval))) => (eval_file, eval.clone()),
                    Some(Err(e)) if go_on => {
                        failed.push(LinkFailure { component: comp, name: cname.clone(), error: e.clone() });
                        link_warnings[slot].push(format!("{cname} keeps its bodies: {e}"));
                        continue;
                    }
                    Some(Err(e)) => return Err(bad(cmd, format!("{cname}: {e}"))),
                    None => {
                        let error = "linked source evaluation failed".to_string();
                        if go_on {
                            failed.push(LinkFailure { component: comp, name: cname.clone(), error: error.clone() });
                            link_warnings[slot].push(format!("{cname} keeps its bodies: {error}"));
                            continue;
                        }
                        return Err(bad(cmd, format!("{cname}: {error}")));
                    }
                };
                if absolute(&file) != absolute(eval_file) {
                    remember_read_parts(
                        &file,
                        configuration,
                        bytes_hash,
                        &eval.kind,
                        &eval.warnings,
                        &eval.faces,
                        &eval.properties,
                        &eval.metadata,
                        &eval.meshes,
                    );
                }
                if !same_bytes && let Some(kept) = s.doc_mut().links.get_mut(&comp) {
                    kept.file_sha256 = Some(sha256);
                }
                let mut ctx = ApplyContext {
                    s,
                    link_warnings: &mut link_warnings,
                    kept_imports: &mut kept_imports,
                    rebind_queue: &mut rebind_queue,
                    tie_queue: &mut tie_queue,
                    realigned_features: &mut realigned_features,
                    post_entries: &mut post_entries,
                    read_components: &mut read_components,
                    needs_first_refresh: &mut needs_first_refresh,
                };
                apply_evaluated_link(&mut ctx, slot, comp, cname, &link, &old, &file, before, eval)?;
            }
        }
    }
    solidcraft_doc::progress::item(None);
    // None could be read: nothing to take back either.
    if post_entries.is_empty() && !failed.is_empty() {
        return Err(bad(cmd, format!("{}: {}", failed[0].name, failed[0].error)));
    }

    // Phase 4: ONE refresh for all mutated links, then rebind faces for every changed link.
    if needs_first_refresh {
        #[cfg(test)]
        UPDATE_LINKS_REFRESHES.with(|n| n.set(n.get() + 1));
        s.refresh();
    }
    drop(kept_imports);

    let mut rebind_modified_doc = false;
    if !rebind_queue.is_empty() {
        let mut doc = (*s.doc).clone();
        for (slot, feature, before) in rebind_queue {
            if let (Some(before), Some(after)) = (before, s.model.result(feature).map(|r| r.state.clone())) {
                link_warnings[slot].extend(doc.rebind_linked_faces(feature, &before, &after));
            }
        }
        if doc != *s.doc {
            *s.doc_mut() = doc;
            rebind_modified_doc = true;
        }
    }

    let doc_after_rebind = s.doc.clone();
    let (mut updated, mut unchanged): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for entry in post_entries {
        match entry {
            PostRefreshEntry::Done { cname, updated: is_updated } => {
                if is_updated {
                    updated.push(cname);
                } else {
                    unchanged.push(cname);
                }
            }
            PostRefreshEntry::Inherit { comp, cname, properties, changed } => {
                let changed = inherit_properties(s, comp, &properties)? || changed;
                if changed {
                    updated.push(cname);
                } else {
                    unchanged.push(cname);
                }
            }
        }
    }

    // Phase 5: at most ONE final refresh, only if any face rebind modified the document.
    if rebind_modified_doc {
        #[cfg(test)]
        UPDATE_LINKS_REFRESHES.with(|n| n.set(n.get() + 1));
        s.refresh();
    } else if !needs_first_refresh && !std::sync::Arc::ptr_eq(&doc_before_update, &s.doc) && *s.doc == *doc_before_update {
        // Restore the original Arc when helper probes (`s.doc_mut()`) made no actual changes so
        // `Session::execute` does not record a spurious undo step or extra evaluation.
        s.doc = doc_before_update;
    } else if !std::sync::Arc::ptr_eq(&doc_after_rebind, &s.doc) || (!needs_first_refresh && !std::sync::Arc::ptr_eq(&doc_before_update, &s.doc)) {
        s.revision += 1;
    }

    if let Some(e) = s.doc.links.values().find_map(|l| s.model.result(l.feature).and_then(|r| r.error.clone())) {
        return Err(EngineError::Other(e));
    }
    for (slot, cname, tie_before) in tie_queue {
        link_warnings[slot].extend(tie_numbered_joint_warnings(s, &cname, &tie_before));
    }
    let mut warnings: Vec<String> = link_warnings.into_iter().flatten().collect();
    // A reference whose face or edge the design no longer has says so here, both for timeline
    // features that newly fail and for joints.
    if !updated.is_empty() {
        for r in &s.model.results {
            if let Some(why) = r.error.as_ref()
                && failing_before.get(&r.id) != Some(why)
            {
                warnings.push(if why.starts_with(&format!("{}: ", r.name)) { why.clone() } else { format!("{}: {why}", r.name) });
            }
        }
        let sol = solidcraft_doc::joints::solve_holding_with_analysis(
            &s.doc,
            &s.model.state(),
            &std::collections::BTreeMap::new(),
            solidcraft_doc::joints::Analysis::Placement,
        );
        warnings.extend(sol.errors.iter().chain(&sol.conflicts).map(|(_, name, why)| format!("joint {name}: {why}")));
    }
    crate::plugin::record_link_reads(s, &read_components);
    reply(cmd, UpdateLinksResult { updated, unchanged, warnings, failed, param_warnings: Vec::new() })
}

/// What an Import feature's bodies are made from: its STEP text and its records.
type ImportSource = (std::sync::Arc<str>, Option<std::sync::Arc<solidcraft_doc::kernel::ImportRecords>>);

/// Whether two Import features make their bodies from the same source.
fn same_import_source(a: &FeatureKind, b: &FeatureKind) -> bool {
    match (a, b) {
        (FeatureKind::Import { step, records, .. }, FeatureKind::Import { step: other, records: other_records, .. }) => {
            step == other && records == other_records
        }
        _ => false,
    }
}

/// The Mesh Import feature in the linked component `comp` that holds its design's mesh bodies,
/// beside the link's Import feature `link`, with those meshes.
fn link_mesh_feature(s: &Session, comp: u64, link: u64) -> Option<(u64, &[MeshData])> {
    s.doc.features.iter().find_map(|f| match &f.kind {
        FeatureKind::MeshImport { meshes, .. } if f.component == comp && f.id != link => Some((f.id, meshes.as_slice())),
        _ => None,
    })
}

/// The mesh bodies the linked component `comp` holds from its design (none for a link made
/// before they were linked, or a design that has none).
fn link_meshes(s: &Session, comp: u64, link: u64) -> &[MeshData] {
    link_mesh_feature(s, comp, link).map_or(&[], |(_, meshes)| meshes)
}

/// Give the linked component `comp` its design's mesh bodies `meshes`: its Mesh Import feature
/// takes them, made when the design first has some and deleted when it has none left. `name`
/// names a new feature and `file` is the design it shows. Whether the meshes changed.
fn set_link_meshes(s: &mut Session, comp: u64, link: u64, name: &str, file: &str, meshes: Vec<MeshData>) -> Result<bool> {
    let held = link_mesh_feature(s, comp, link).map(|(id, held)| (id, held == meshes.as_slice()));
    match held {
        Some((_, true)) => Ok(false),
        None if meshes.is_empty() => Ok(false),
        Some((id, false)) if meshes.is_empty() => {
            s.doc_mut().delete_feature(id)?;
            Ok(true)
        }
        Some((id, false)) => {
            if let Some(f) = s.doc_mut().feature_mut(id) {
                f.kind = FeatureKind::MeshImport { file: file.to_string(), meshes };
            }
            Ok(true)
        }
        None => {
            let id = s.doc_mut().add_feature(FeatureKind::MeshImport { file: file.to_string(), meshes }, Some(&format!("{name} meshes")))?;
            if let Some(f) = s.doc_mut().feature_mut(id) {
                f.component = comp;
            }
            Ok(true)
        }
    }
}

/// The joints and joint origins on occurrences of `comp` that name its linked faces by their
/// place in the file (`F<feature>:face:<k>`, made before the design's own names were kept), which
/// an update can give to another face.
fn numbered_references(s: &Session, comp: u64, feature: u64) -> Vec<String> {
    let numbered = format!("F{feature}:face:");
    let on = |o: &solidcraft_doc::joints::JointOrigin| {
        s.doc.occurrences.iter().any(|x| x.id == o.part() && s.doc.component_within(x.component, comp))
            && o.snap.face_names().iter().any(|n| n.trim_start_matches(solidcraft_doc::naming::LEGACY_MARK).starts_with(&numbered))
    };
    let joints = s.doc.assembly.joints.iter().filter(|j| on(&j.a) || on(&j.b)).map(|j| format!("joint {}", j.name));
    let origins = s.doc.assembly.origins.iter().filter(|o| on(&o.origin)).map(|o| format!("joint origin {}", o.name));
    joints.chain(origins).collect()
}

/// A joint side on a linked component referencing a positional tie-numbered linked face
/// (`F<feature>:link:<digest>#<k>`), captured before `component.update_links` replaces the bodies.
struct TieNumberedJointSide {
    joint_id: u64,
    joint_name: String,
    is_a: bool,
    face_name: String,
    before: Option<solidcraft_doc::Mat>,
}

/// Snapshot every joint side on occurrences of `comp` that references a positional tie-numbered
/// linked face of `feature`, together with its resolved origin frame before the update.
fn snapshot_tie_numbered_joints(s: &Session, comp: u64, feature: u64) -> Vec<TieNumberedJointSide> {
    let st = s.model.state();
    let tie_face = |o: &solidcraft_doc::joints::JointOrigin| -> Option<String> {
        if !s.doc.occurrences.iter().any(|x| x.id == o.part() && s.doc.component_within(x.component, comp)) {
            return None;
        }
        o.snap.face_names().into_iter().find(|n| solidcraft_doc::naming::has_tie_numbered_link_face(n.as_str(), feature)).cloned()
    };
    let mut out = Vec::new();
    for j in &s.doc.assembly.joints {
        for (is_a, side) in [(true, &j.a), (false, &j.b)] {
            if let Some(face_name) = tie_face(side) {
                let before = solidcraft_doc::joints::resolve_origin(&s.doc, &st, side).ok();
                out.push(TieNumberedJointSide { joint_id: j.id, joint_name: j.name.clone(), is_a, face_name, before });
            }
        }
    }
    out
}

/// Named joint origins on occurrences of `comp` that reference a positional tie-numbered linked
/// face of `feature`.
fn tie_numbered_origin_references(s: &Session, comp: u64, feature: u64) -> Vec<String> {
    s.doc
        .assembly
        .origins
        .iter()
        .filter(|o| {
            s.doc.occurrences.iter().any(|x| x.id == o.origin.part() && s.doc.component_within(x.component, comp))
                && o.origin.snap.face_names().iter().any(|n| solidcraft_doc::naming::has_tie_numbered_link_face(n.as_str(), feature))
        })
        .map(|o| format!("joint origin {}", o.name))
        .collect()
}

/// Warnings for joints that still resolve through a positional tie-numbered linked face after
/// `component.update_links` (missing faces are reported as joint errors instead). Reports the
/// exact shift distance and/or tilt angle whenever the resolved origin moved or tilted.
fn tie_numbered_joint_warnings(s: &Session, cname: &str, sides: &[TieNumberedJointSide]) -> Vec<String> {
    let st = s.model.state();
    let mut out = Vec::new();
    for entry in sides {
        let Some(j) = s.doc.assembly.joints.iter().find(|j| j.id == entry.joint_id) else { continue };
        let side = if entry.is_a { &j.a } else { &j.b };
        let Ok(after) = solidcraft_doc::joints::resolve_origin(&s.doc, &st, side) else { continue };
        let moved = entry.before.and_then(|before| {
            let (dx, dy, dz) = (after[3][0] - before[3][0], after[3][1] - before[3][1], after[3][2] - before[3][2]);
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            let dot = before[2][0] * after[2][0] + before[2][1] * after[2][1] + before[2][2] * after[2][2];
            let shifted = dist > 1e-3;
            let tilted = dot < 1.0 - 1e-4;
            (shifted || tilted).then(|| (shifted.then_some(dist), tilted.then(|| dot.clamp(-1.0, 1.0).acos().to_degrees())))
        });
        match moved {
            Some((Some(dist), Some(deg))) => out.push(format!(
                "joint {} refers to a positional tie-numbered face of {cname} (`{}`) that shifted by {dist:.2} mm and tilted by {deg:.1}° in this update: check it, and pick its face again",
                entry.joint_name, entry.face_name
            )),
            Some((Some(dist), None)) => out.push(format!(
                "joint {} refers to a positional tie-numbered face of {cname} (`{}`) that shifted by {dist:.2} mm in this update: check it, and pick its face again",
                entry.joint_name, entry.face_name
            )),
            Some((None, Some(deg))) => out.push(format!(
                "joint {} refers to a positional tie-numbered face of {cname} (`{}`) that tilted by {deg:.1}° in this update: check it, and pick its face again",
                entry.joint_name, entry.face_name
            )),
            _ => out.push(format!(
                "joint {} refers to a positional tie-numbered face of {cname} (`{}`), which can shift when sibling faces are added or removed: check it, and pick its face again",
                entry.joint_name, entry.face_name
            )),
        }
    }
    out
}

/// Where each body of a link's design was when the link last read it, for a design that kept
/// its number of bodies but lists them in another order (#21371): `order[j]` is the place the
/// design's body `j` had then. `step` is the STEP the link keeps from that read, which names the
/// bodies as the design did when read with `names` (the link's own naming for a STEP link, the
/// default for a design); `now` are the names the design gives its bodies now. A body the
/// design still has is found by its name; the bodies it has new take the places of the ones it no
/// longer has, in order. `None` when nothing moved, and when the old STEP can't be read or its
/// names, or the new ones, can't tell the bodies apart: the bodies then keep their places.
fn moved_bodies(step: &str, file: &str, names: solidcraft_io::StepNames, now: &[String]) -> Option<Vec<usize>> {
    if step.is_empty() {
        return None;
    }
    // A second read of the STEP text: the link keeps no other record of the old names.
    let was = solidcraft_io::step_import_feature_named(step.as_bytes(), file, names).ok()?.body_names;
    let places: std::collections::HashMap<&str, usize> = was.iter().enumerate().map(|(k, n)| (n.as_str(), k)).collect();
    let distinct = now.iter().collect::<std::collections::HashSet<_>>().len() == now.len();
    if was.len() != now.len() || places.len() != was.len() || !distinct {
        return None;
    }
    let found: Vec<Option<usize>> = now.iter().map(|n| places.get(n.as_str()).copied()).collect();
    let taken: std::collections::HashSet<usize> = found.iter().flatten().copied().collect();
    let mut freed = (0..was.len()).filter(|k| !taken.contains(k));
    let order: Vec<usize> = found.iter().map(|k| k.or_else(|| freed.next())).collect::<Option<_>>()?;
    order.iter().enumerate().any(|(j, k)| j != *k).then_some(order)
}

/// `items` (one per body, in the order a link last read them) in the design's new `order`.
fn reordered<T: Clone + Default>(items: &[T], order: &[usize]) -> Vec<T> {
    order.iter().map(|k| items.get(*k).cloned().unwrap_or_default()).collect()
}

/// Map each non-empty name that appears exactly once in `names` to its slot index.
fn unique_name_slots(names: &[String]) -> std::collections::HashMap<&str, usize> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for name in names {
        if !name.is_empty() {
            *counts.entry(name.as_str()).or_insert(0) += 1;
        }
    }
    names.iter().enumerate().filter(|(_, name)| counts.get(name.as_str()) == Some(&1)).map(|(k, name)| (name.as_str(), k)).collect()
}

/// Effective per-slot body names produced by `feature` in the current evaluated model (falling
/// back to stored `f.body_names` for skipped or unevaluated slots), plus all body names currently
/// owned by other features.
fn feature_body_context(s: &Session, feature: u64) -> (Vec<String>, std::collections::HashSet<String>) {
    feature_body_context_excluding(s, feature, &std::collections::HashSet::new())
}

/// [`feature_body_context`], skipping stale `s.model` bodies for features already realigned in the
/// current batch whose updated `f.body_names` in `s.doc` already hold their effective names.
fn feature_body_context_excluding(
    s: &Session,
    feature: u64,
    stale_model_features: &std::collections::HashSet<u64>,
) -> (Vec<String>, std::collections::HashSet<String>) {
    let mut effective = s.doc.feature(feature).map(|f| f.body_names.clone()).unwrap_or_default();
    if let Some(f) = s.doc.feature(feature)
        && let Some(r) = s.model.result(feature).filter(|r| r.error.is_none() && !r.skipped && !r.stale)
    {
        let made: Vec<String> = r.state.bodies.iter().filter(|b| b.feature == feature).map(|b| b.name.clone()).collect();
        let slots = f.body_name_slots(made.len());
        if let Some(last) = slots.last().copied()
            && effective.len() <= last
        {
            effective.resize(last + 1, String::new());
        }
        for (slot, name) in slots.into_iter().zip(made) {
            if let Some(entry) = effective.get_mut(slot) {
                *entry = name;
            }
        }
    }
    let mut other_bodies: std::collections::HashSet<String> = s
        .model
        .state()
        .bodies
        .iter()
        .filter(|b| b.feature != feature && !stale_model_features.contains(&b.feature))
        .map(|b| b.name.clone())
        .collect();
    for f in &s.doc.features {
        if f.id != feature {
            other_bodies.extend(f.body_names.iter().filter(|n| !n.is_empty()).cloned());
        }
    }
    (effective, other_bodies)
}

/// When a link has no stored or evaluated body names yet (e.g. a file saved without linked bodies
/// and empty `body_names`), keep its existing `inherited` baseline when the body count still
/// matches; if the count changed, keep baselines only for slots whose names already exist in
/// `doc.materials` and give brand-new body names a fresh baseline so they inherit from the source.
fn realign_unpinned_inherited(doc: &Document, old_inherited: &[LinkedBodyProperties], new_names: &[String]) -> Option<Vec<LinkedBodyProperties>> {
    if old_inherited.is_empty() || old_inherited.len() == new_names.len() {
        return None;
    }
    Some(
        new_names
            .iter()
            .enumerate()
            .map(
                |(i, name)| {
                    if doc.materials.contains_key(name) { old_inherited.get(i).cloned().unwrap_or_default() } else { LinkedBodyProperties::default() }
                },
            )
            .collect(),
    )
}

/// Match each body of a linked design whose body count changed to its prior slot, if any.
/// Surviving bodies keep their assembly name and inherited baseline (preserving local overrides);
/// new bodies get a fresh baseline (`LinkedBodyProperties::default()`) so [`inherit_properties`]
/// takes the source's material and appearance instead of mistaking them for local overrides.
/// Returns `(next_body_names, next_inherited, removed_body_names)`.
fn realign_changed_body_count(
    old_step: &str,
    file: &str,
    naming: solidcraft_io::StepNames,
    old_names: &[String],
    old_inherited: &[LinkedBodyProperties],
    new_names: &[String],
    other_bodies: &std::collections::HashSet<String>,
) -> (Vec<String>, Vec<LinkedBodyProperties>, Vec<String>) {
    let old_source: Option<Vec<String>> = (!old_step.is_empty())
        .then(|| solidcraft_io::step_import_feature_named(old_step.as_bytes(), file, naming).ok().map(|f| f.body_names))
        .flatten()
        .filter(|was| was.len() == old_names.len());
    let places = old_source.as_deref().map_or_else(|| unique_name_slots(old_names), unique_name_slots);
    let mut new_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for name in new_names {
        *new_counts.entry(name.as_str()).or_insert(0) += 1;
    }

    let mut claimed = std::collections::HashSet::new();
    let mut claimed_names = std::collections::HashSet::new();
    let mut matches: Vec<Option<(usize, String)>> = Vec::with_capacity(new_names.len());

    // Pass 1: resolve every surviving body first so its stored assembly name is reserved before
    // any unmatched new body is named.
    for name in new_names {
        let matched =
            (new_counts.get(name.as_str()) == Some(&1)).then(|| places.get(name.as_str()).copied()).flatten().filter(|k| claimed.insert(*k));
        if let Some(k) = matched {
            let kept_name = old_names.get(k).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| name.clone());
            claimed_names.insert(kept_name.clone());
            matches.push(Some((k, kept_name)));
        } else {
            matches.push(None);
        }
    }

    // Pass 2: assign surviving bodies their effective names and baselines; give unmatched new
    // bodies a fresh baseline and a name that cannot collide with any surviving body or another
    // feature's body.
    let mut reserved = claimed_names.clone();
    reserved.extend(other_bodies.iter().cloned());
    let mut next_names = Vec::with_capacity(new_names.len());
    let mut next_inherited = Vec::with_capacity(new_names.len());
    for (name, matched) in new_names.iter().zip(matches) {
        if let Some((k, kept_name)) = matched {
            next_names.push(kept_name);
            next_inherited.push(old_inherited.get(k).cloned().unwrap_or_default());
        } else {
            let assigned = if !name.is_empty() && reserved.contains(name.as_str()) {
                (2usize..).map(|n| format!("{name} ({n})")).find(|candidate| !reserved.contains(candidate.as_str())).unwrap_or_else(|| name.clone())
            } else {
                name.clone()
            };
            if !assigned.is_empty() {
                reserved.insert(assigned.clone());
            }
            next_names.push(assigned);
            next_inherited.push(LinkedBodyProperties::default());
        }
    }

    // Only prune unmatched old names when we have the old STEP source names to prove which source
    // bodies actually disappeared; without them (e.g. a design saved without linked bodies), a
    // surviving body stored under a renamed/disambiguated assembly name would appear unmatched and
    // must keep its local overrides.
    let removed = if old_source.is_some() {
        old_names
            .iter()
            .enumerate()
            .filter(|(k, name)| !claimed.contains(k) && !name.is_empty() && !claimed_names.contains(name.as_str()))
            .map(|(_, name)| name.clone())
            .collect()
    } else {
        Vec::new()
    };
    (next_names, next_inherited, removed)
}

/// Remove per-body material, appearance and metadata entries for bodies that a linked feature no
/// longer builds, leaving any name still owned by another feature or `body_components` alone.
fn remove_stale_body_properties(doc: &mut Document, feature: u64, removed: &[String], other_bodies: &std::collections::HashSet<String>) {
    if removed.is_empty() {
        return;
    }
    let stale: std::collections::HashSet<&str> = {
        let still_used: std::collections::HashSet<&str> = doc
            .features
            .iter()
            .filter(|f| f.id != feature)
            .flat_map(|f| f.body_names.iter().map(String::as_str))
            .chain(other_bodies.iter().map(String::as_str))
            .chain(doc.body_components.keys().map(String::as_str))
            .collect();
        removed.iter().map(String::as_str).filter(|name| !still_used.contains(name)).collect()
    };
    if stale.is_empty() {
        return;
    }
    doc.materials.retain(|name, _| !stale.contains(name.as_str()));
    doc.appearances.bodies.retain(|name, _| !stale.contains(name.as_str()));
    doc.appearances.faces.retain(|face| !stale.contains(face.body.as_str()));
    doc.collision_shapes.retain(|name, _| !stale.contains(name.as_str()));
    doc.print_faces.retain(|name, _| !stale.contains(name.as_str()));
    doc.plastic.assigned.retain(|name, _| !stale.contains(name.as_str()));
}

/// `component.relink`: a linked component shows the design at `path` instead, in all its
/// placements, or with `occurrence` in that placement only.
fn relink(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.relink";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc.links.get(&comp).cloned().ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    if !link.is_design() {
        return Err(bad(
            cmd,
            "this is the link of an imported file (STEP or an importer plugin's), not of a design: break the link and insert the new file",
        ));
    }
    if !matches!(s.doc.feature(link.feature), Some(f) if f.component == comp && matches!(f.kind, FeatureKind::Import { .. })) {
        return Err(bad(cmd, "the linked bodies are gone; break the link or insert the design again"));
    }
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let sync = match p.get("names") {
        None => false,
        Some(Value::String(n)) if n == "keep" => false,
        Some(Value::String(n)) if n == "sync" => true,
        Some(_) => return Err(bad(cmd, "`names` must be \"keep\" or \"sync\"")),
    };
    let file = absolute(Path::new(given));
    // Any design Insert Design takes: a .sc container (or a legacy one) or a plain JSON design.
    if !solidcraft_io::is_design_path(given) {
        return Err(bad(cmd, "choose a saved design (.sc or .json)"));
    }
    // A network or device path is judged from its text before the disk is asked anything.
    link_refused(s.path.as_deref(), given).map_err(|e| bad(cmd, e.to_string()))?;
    // The same path, or on native hosts an alias of this assembly (a symlink or a path with
    // different case): the one self-link rule Insert Design and Update Links use too.
    if s.path.as_deref().is_some_and(|own| same_file(Path::new(own), &file)) {
        return Err(bad(cmd, "a design can't be linked into itself"));
    }
    let same_source = same_link_source(s, &link, &file);
    let selection = configuration_key(p, if same_source { link.configuration } else { None }, cmd)?;
    let bytes = read_link(&file, s.path.as_deref(), false).map_err(|e| bad(cmd, e.to_string()))?;
    let configuration = configuration_id(&bytes, selection).map_err(|e| bad(cmd, e.to_string()))?;
    if same_source && p.get("configuration").is_none() && configuration != selected_configuration(&link) {
        return Err(bad(cmd, "the selected configuration was replaced; select a configuration again"));
    }
    if let Some(occurrence) = lone_placement(s, p, comp, cmd)? {
        return relink_placement(s, comp, &link, occurrence, given, &file, configuration);
    }
    // Read/evaluate the candidate before changing anything. Session::execute owns the single
    // undo step and restores both the document and evaluated state on any later failure.
    let ((source_name, sf, faces, metadata, properties, meshes), _) =
        design_bodies(&Source::File(file.clone()), &bytes, configuration, s.validation()).map_err(|e| bad(cmd, e.to_string()))?;
    let read = DesignLink::file_digest(&bytes);
    let source_names = sf.body_names.clone();
    let geometry_changed = !s.doc.feature(link.feature).is_some_and(|f| same_import_source(&f.kind, &sf.kind))
        || link_meshes(s, comp, link.feature) != meshes.as_slice();
    let path = stored(s.path.as_deref(), &file);
    let mut warnings = sf.warnings;
    let relink_joints_before = snapshot_relink_joints(s, comp, None);
    let poses_before = snapshot_occurrence_poses(s);
    if geometry_changed
        && s.doc
            .assembly
            .joints
            .iter()
            .any(|j| s.doc.occurrences.iter().any(|o| o.component == comp && (j.a.occurrence == o.id || j.b.occurrence == o.id)))
    {
        warnings.push("the linked geometry changed; check this component's joints, even when the solver reports no error".to_string());
    }
    let mut inherited = link.inherited.clone();
    let mut removed = Vec::new();
    let (old_names, other_bodies) = feature_body_context(s, link.feature);
    let mut unpinned_inherited = if old_names.is_empty() { realign_unpinned_inherited(&s.doc, &inherited, &sf.body_names) } else { None };
    if let Some(f) = s.doc_mut().feature_mut(link.feature) {
        // Both modes start as Relink always did; `sync` renames the bodies below.
        if old_names.len() != sf.body_names.len() {
            if old_names.is_empty() {
                f.body_names = sf.body_names;
                if let Some(next_inherited) = unpinned_inherited.take() {
                    inherited = next_inherited;
                }
            } else {
                let old_step = match &f.kind {
                    FeatureKind::Import { step, .. } => step.as_ref(),
                    _ => "",
                };
                let (next_names, next_inherited, gone) = realign_changed_body_count(
                    old_step,
                    &file.display().to_string(),
                    solidcraft_io::StepNames::default(),
                    &old_names,
                    &inherited,
                    &sf.body_names,
                    &other_bodies,
                );
                f.body_names = next_names;
                inherited = next_inherited;
                removed = gone;
            }
        } else if let FeatureKind::Import { step, .. } = &f.kind
            && !same_import_source(&f.kind, &sf.kind)
            && let Some(order) = moved_bodies(step, &file.display().to_string(), solidcraft_io::StepNames::default(), &sf.body_names)
        {
            // The names, and what the link last took from each body, stay with their bodies (#21371).
            f.body_names = reordered(&old_names, &order);
            if !inherited.is_empty() {
                inherited = reordered(&inherited, &order);
            }
        } else if f.body_names.is_empty() && !old_names.is_empty() {
            f.body_names = old_names;
        }
        f.kind = sf.kind;
        f.link_faces = faces;
    }
    remove_stale_body_properties(s.doc_mut(), link.feature, &removed, &other_bodies);
    let cname = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
    set_link_meshes(s, comp, link.feature, &cname, &file.display().to_string(), meshes)?;
    s.doc_mut().links.insert(
        comp,
        DesignLink {
            path: file.to_string_lossy().into_owned(),
            file_sha256: Some(read),
            configuration: configuration.map(|c| c.row),
            configuration_identity: configuration.and_then(|c| c.identity),
            inherited,
            ..link.clone()
        },
    );
    s.refresh();
    if let Some(error) = s.model.result(link.feature).and_then(|r| r.error.clone()) {
        return Err(bad(cmd, error));
    }
    if sync {
        sync_names(s, comp, link.feature, &source_name, &source_names, &mut warnings);
    }
    inherit_properties(s, comp, &properties)?;
    let (_, kept) = inherit_metadata(s, comp, &metadata);
    warnings.extend(kept);
    s.refresh();
    // Report the solver's diagnostics and any per-joint frame deviations or occurrence pose
    // displacements caused by the relink. A resolved joint can still bind to the wrong face
    // when a new source reuses the same carried face name; the deviation check catches that.
    let (mut joint_errors, joint_conflicts) = joint_diagnostics(s, &mut warnings);
    let joint_deviations = relink_joint_deviations(s, &relink_joints_before, &mut warnings, &mut joint_errors);
    let moved_occurrences = relink_moved_occurrences(s, &poses_before, &mut warnings);
    // A link to a design that came from this session had no file: it names none.
    let previous_path = if link.path.is_empty() { String::new() } else { stored(s.path.as_deref(), &resolve(s.path.as_deref(), &link.path)) };
    reply(
        cmd,
        RelinkResult {
            component: comp,
            previous_component: None,
            occurrence: None,
            feature: link.feature,
            path,
            previous_path,
            geometry_changed,
            warnings,
            joint_errors,
            joint_conflicts,
            joint_deviations,
            moved_occurrences,
            param_warnings: Vec::new(),
        },
    )
}

/// `names: "sync"`: the linked bodies, by position, take the names the new source gives them,
/// through the rename `body.rename` does, so the materials, appearances and later features that
/// name a body follow it. A body that cannot take its new name (another body has it) keeps its
/// name and is reported. The linked feature takes the source design's name when it still has the
/// component's (the pair Insert Design makes); the component keeps its name.
fn sync_names(s: &mut Session, comp: u64, feature: u64, source_name: &str, source: &[String], warnings: &mut Vec<String>) {
    // The feature's own build result names every body it made, in build order, which is what the
    // stored name slots follow; a later feature may have removed some of them from the model.
    let made: Vec<String> =
        s.model.result(feature).map(|r| r.state.bodies.iter().filter(|b| b.feature == feature).map(|b| b.name.clone()).collect()).unwrap_or_default();
    let slots = s.doc.feature(feature).map(|f| f.body_name_slots(made.len())).unwrap_or_default();
    let alive: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == feature).map(|b| b.name.clone()).collect();
    let survivors = made.iter().zip(slots).filter(|(name, _)| alive.contains(name));
    for (old, slot) in survivors.map(|(n, slot)| (n.clone(), slot)).collect::<Vec<_>>() {
        let old = &old;
        let Some(target) = source.get(slot).map(|n| n.trim()).filter(|n| !n.is_empty()) else { continue };
        if old == target {
            continue;
        }
        let before = s.doc.clone();
        match super::browser::rename_one(s, "component.relink", old, target) {
            Ok(_) => s.refresh(),
            Err(e) => {
                s.doc = before;
                s.refresh();
                warnings.push(format!("body {old} keeps its name instead of {target}: {e}"));
            }
        }
    }
    let name = source_name.trim();
    let own = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone());
    let taken = s.doc.features.iter().any(|f| f.name == name && f.id != feature);
    if !name.is_empty()
        && !taken
        && let Some(f) = s.doc_mut().feature_mut(feature).filter(|f| Some(&f.name) == own.as_ref())
    {
        f.name = name.to_string();
    }
}

/// The solver's joint errors and conflicts after a relink: added to `warnings`, and as rows.
fn joint_diagnostics(s: &Session, warnings: &mut Vec<String>) -> (Vec<JointDiagnostic>, Vec<JointDiagnostic>) {
    let sol = solidcraft_doc::joints::solve_holding_with_analysis(
        &s.doc,
        &s.model.state(),
        &std::collections::BTreeMap::new(),
        solidcraft_doc::joints::Analysis::Placement,
    );
    for (_, name, why) in sol.errors.iter().chain(&sol.conflicts) {
        warnings.push(format!("joint {name}: {why}"));
    }
    let rows = |rows: &[(u64, String, String)]| {
        rows.iter().map(|(joint, name, why)| JointDiagnostic { joint: *joint, name: name.clone(), why: why.clone() }).collect()
    };
    (rows(&sol.errors), rows(&sol.conflicts))
}

/// Thresholds for reporting joint frame deviations and occurrence pose shifts after `component.relink`.
const RELINK_SHIFT_TOL_MM: f64 = 1e-3;
const RELINK_ANGLE_TOL_DEG: f64 = 0.1;
/// Normal/axis rotation (degrees) at or above which a re-resolved face is flagged as incompatible
/// (e.g., a perpendicular or opposite face carrying the same carried link face name).
const RELINK_INCOMPATIBLE_ANGLE_DEG: f64 = 45.0;

/// A resolved joint side on a linked component (or single placement), captured before `component.relink`.
struct RelinkJointSnapshot {
    joint_id: u64,
    joint_name: String,
    is_a: bool,
    before: solidcraft_doc::Mat,
}

/// Snapshot every unsuppressed joint side whose snap resolves on `comp` (or on `occurrence` alone
/// when relinking one placement of several).
fn snapshot_relink_joints(s: &Session, comp: u64, occurrence: Option<u64>) -> Vec<RelinkJointSnapshot> {
    let st = s.model.state();
    let matches_side = |side: &solidcraft_doc::joints::JointOrigin| match occurrence {
        Some(occ) => side.part() == occ || side.occurrences().any(|id| id == occ),
        None => s.doc.occurrences.iter().any(|x| x.id == side.part() && s.doc.component_within(x.component, comp)),
    };
    let mut out = Vec::new();
    for j in &s.doc.assembly.joints {
        if j.suppressed {
            continue;
        }
        for (is_a, side) in [(true, &j.a), (false, &j.b)] {
            if matches_side(side)
                && let Ok(before) = solidcraft_doc::joints::resolve_origin(&s.doc, &st, side)
            {
                out.push(RelinkJointSnapshot { joint_id: j.id, joint_name: j.name.clone(), is_a, before });
            }
        }
    }
    out
}

/// Snapshot every occurrence's world transform before `component.relink`.
fn snapshot_occurrence_poses(s: &Session) -> Vec<(u64, String, solidcraft_doc::Mat)> {
    s.doc.occurrences.iter().map(|o| (o.id, o.name.clone(), solidcraft_doc::joints::occurrence_world(&s.doc, o.id))).collect()
}

/// How far two resolved origin frames differ: (origin shift mm, Z-axis angle deg).
fn frame_deviation(before: &solidcraft_doc::Mat, after: &solidcraft_doc::Mat) -> (f64, f64) {
    solidcraft_kernel::frame_z_deviation(before, after)
}

/// How far two rigid world transforms differ: (translation mm, rotation deg).
fn pose_delta(a: &solidcraft_doc::Mat, b: &solidcraft_doc::Mat) -> (f64, f64) {
    solidcraft_kernel::rigid_pose_delta(a, b)
}

/// Per-joint frame deviations after `component.relink`: records every snapshotted joint side that
/// still resolves but whose origin shifted or normal/axis rotated, adds explicit warnings, and
/// flags sharply rotated (incompatible) face re-bindings in `joint_errors`.
fn relink_joint_deviations(
    s: &Session,
    before: &[RelinkJointSnapshot],
    warnings: &mut Vec<String>,
    joint_errors: &mut Vec<JointDiagnostic>,
) -> Vec<JointDeviation> {
    let st = s.model.state();
    let mut out = Vec::new();
    for entry in before {
        let Some(j) = s.doc.assembly.joints.iter().find(|j| j.id == entry.joint_id) else { continue };
        let side = if entry.is_a { &j.a } else { &j.b };
        let Ok(after) = solidcraft_doc::joints::resolve_origin(&s.doc, &st, side) else { continue };
        let (origin_shift_mm, normal_angle_deg) = frame_deviation(&entry.before, &after);
        let shifted = origin_shift_mm > RELINK_SHIFT_TOL_MM;
        let tilted = normal_angle_deg > RELINK_ANGLE_TOL_DEG;
        if !shifted && !tilted {
            continue;
        }
        let incompatible = normal_angle_deg >= RELINK_INCOMPATIBLE_ANGLE_DEG;
        let side_label = if entry.is_a { "a" } else { "b" };
        let face = side.snap.face_names().into_iter().next().cloned();
        let face_note = face.as_deref().map_or_else(String::new, |f| format!(", face `{f}`"));
        if incompatible {
            let detail = format!(
                "side {side_label}{face_note} resolved to an incompatible face (origin shifted by {origin_shift_mm:.2} mm, normal rotated by {normal_angle_deg:.1}°); check and re-pick its face"
            );
            warnings.push(format!("joint {}: {detail}", entry.joint_name));
            if !joint_errors.iter().any(|e| e.joint == entry.joint_id) {
                joint_errors.push(JointDiagnostic { joint: entry.joint_id, name: entry.joint_name.clone(), why: detail });
            }
        } else if shifted && tilted {
            warnings.push(format!(
                "joint {} (side {side_label}{face_note}): resolved face shifted by {origin_shift_mm:.2} mm and tilted by {normal_angle_deg:.1}° after relink",
                entry.joint_name
            ));
        } else if shifted {
            warnings.push(format!(
                "joint {} (side {side_label}{face_note}): resolved face shifted by {origin_shift_mm:.2} mm after relink",
                entry.joint_name
            ));
        } else {
            warnings.push(format!(
                "joint {} (side {side_label}{face_note}): resolved face tilted by {normal_angle_deg:.1}° after relink",
                entry.joint_name
            ));
        }
        out.push(JointDeviation {
            joint: entry.joint_id,
            name: entry.joint_name.clone(),
            side: side_label.to_string(),
            face,
            origin_shift_mm,
            normal_angle_deg,
            incompatible,
        });
    }
    out
}

/// Occurrences whose solved world pose shifted or rotated after `component.relink`.
fn relink_moved_occurrences(s: &Session, before: &[(u64, String, solidcraft_doc::Mat)], warnings: &mut Vec<String>) -> Vec<MovedOccurrence> {
    let mut out = Vec::new();
    for (id, old_name, before_world) in before {
        let Some(occ) = s.doc.occurrences.iter().find(|o| o.id == *id) else { continue };
        let after_world = solidcraft_doc::joints::occurrence_world(&s.doc, *id);
        let (translation_mm, rotation_deg) = pose_delta(before_world, &after_world);
        let moved = translation_mm > RELINK_SHIFT_TOL_MM;
        let rotated = rotation_deg > RELINK_ANGLE_TOL_DEG;
        if !moved && !rotated {
            continue;
        }
        let name = if occ.name.is_empty() { old_name.clone() } else { occ.name.clone() };
        if moved && rotated {
            warnings.push(format!("occurrence {name} moved by {translation_mm:.2} mm and rotated by {rotation_deg:.1}° after relink"));
        } else if moved {
            warnings.push(format!("occurrence {name} moved by {translation_mm:.2} mm after relink"));
        } else {
            warnings.push(format!("occurrence {name} rotated by {rotation_deg:.1}° after relink"));
        }
        out.push(MovedOccurrence { occurrence: *id, name, translation_mm, rotation_deg });
    }
    out
}

/// The placement `occurrence` of `comp` to relink on its own: `None` when none is given, or when
/// it is the only placement, which a relink of the whole component covers.
fn lone_placement(s: &Session, p: &Value, comp: u64, cmd: &str) -> Result<Option<u64>> {
    let Some(given) = p.get("occurrence") else { return Ok(None) };
    let occurrence = given.as_u64().ok_or_else(|| bad(cmd, "`occurrence` must be a placement id"))?;
    if !s.doc.occurrences.iter().any(|o| o.id == occurrence && o.component == comp) {
        return Err(bad(cmd, format!("placement {occurrence} doesn't show that component")));
    }
    Ok((s.doc.occurrences.iter().filter(|o| o.component == comp).count() > 1).then_some(occurrence))
}

/// Built bodies of a linked component's `Import` or `MeshImport` feature as
/// `(assembly_name, source_name)` pairs in build order.
fn linked_body_entries(s: &Session, feature_id: u64) -> Vec<(String, String)> {
    let Some(f) = s.doc.feature(feature_id) else {
        return Vec::new();
    };
    let skip: std::collections::HashSet<u64> = match &f.kind {
        FeatureKind::Import { skip, .. } => skip.iter().copied().collect(),
        _ => std::collections::HashSet::new(),
    };
    let built: Vec<(usize, String)> = if let Some(r) = s.model.result(feature_id).filter(|r| r.error.is_none() && !r.skipped && !r.stale) {
        let made: Vec<String> = r.state.bodies.iter().filter(|b| b.feature == feature_id).map(|b| b.name.clone()).collect();
        f.body_name_slots(made.len()).into_iter().zip(made).collect()
    } else {
        let made: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == feature_id).map(|b| b.name.clone()).collect();
        if !made.is_empty() {
            f.body_name_slots(made.len()).into_iter().zip(made).collect()
        } else {
            f.body_names
                .iter()
                .enumerate()
                .filter(|(k, n)| !skip.contains(&(*k as u64)) && !n.trim().is_empty())
                .map(|(k, n)| (k, n.clone()))
                .collect()
        }
    };
    let source_names: Vec<String> = match &f.kind {
        FeatureKind::Import { step, records, .. } => {
            let imp = match records {
                Some(r) => solidcraft_kernel::records_import_shared(r).ok(),
                None if !step.is_empty() => solidcraft_kernel::step_import_shared(step).ok(),
                None => None,
            };
            imp.map(|imp| imp.bodies.iter().map(|b| b.name.clone()).collect()).unwrap_or_default()
        }
        FeatureKind::MeshImport { meshes, .. } => meshes.iter().map(|m| m.name.clone()).collect(),
        _ => Vec::new(),
    };
    built
        .into_iter()
        .map(|(slot, assembly)| {
            let source = source_names.get(slot).filter(|n| !n.trim().is_empty()).cloned().unwrap_or_else(|| assembly.clone());
            (assembly, source)
        })
        .collect()
}

/// Match bodies built by the old linked feature (`old`: `(assembly_name, source_name)`) to bodies
/// built by the replacement linked feature (`new`: `(assembly_name, source_name)`) for retargeting
/// downstream body patterns and mirrors of a relinked placement (#21378).
///
/// Only unambiguous 1-to-1 mappings are emitted:
/// 1. Unique exact source-name matches (so disambiguated assembly names like `Block` -> `Block (2)`
///    still pair cleanly).
/// 2. Unique exact assembly-name matches against the replacement's remaining source names (for
///    bodies renamed locally in the assembly).
/// 3. When exactly one old body and one new body remain unclaimed (including the common 1 -> 1
///    single-body swap where the new design names its sole body differently), pair them.
fn match_relink_bodies(old: &[(String, String)], new: &[(String, String)]) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    if old.is_empty() || new.is_empty() {
        return map;
    }
    let mut claimed_old = vec![false; old.len()];
    let mut claimed_new = vec![false; new.len()];

    // Pass 1: unique exact source-name match.
    let old_sources: Vec<String> = old.iter().map(|(_, src)| src.clone()).collect();
    let new_sources: Vec<String> = new.iter().map(|(_, src)| src.clone()).collect();
    let old_src_slots = unique_name_slots(&old_sources);
    let new_src_slots = unique_name_slots(&new_sources);
    for (name, &oi) in &old_src_slots {
        if let Some(&ni) = new_src_slots.get(name) {
            claimed_old[oi] = true;
            claimed_new[ni] = true;
            map.insert(old[oi].0.clone(), new[ni].0.clone());
        }
    }

    // Pass 2: unique match between unclaimed old assembly names and unclaimed new source names.
    let rem_old_asm: Vec<String> = old.iter().enumerate().map(|(i, (asm, _))| if claimed_old[i] { String::new() } else { asm.clone() }).collect();
    let rem_new_src: Vec<String> = new.iter().enumerate().map(|(i, (_, src))| if claimed_new[i] { String::new() } else { src.clone() }).collect();
    let old_asm_slots = unique_name_slots(&rem_old_asm);
    let rem_new_src_slots = unique_name_slots(&rem_new_src);
    for (name, &oi) in &old_asm_slots {
        if let Some(&ni) = rem_new_src_slots.get(name) {
            claimed_old[oi] = true;
            claimed_new[ni] = true;
            map.insert(old[oi].0.clone(), new[ni].0.clone());
        }
    }

    // Pass 3: sole remaining old body <-> sole remaining new body.
    let unclaimed_old: Vec<usize> = claimed_old.iter().enumerate().filter_map(|(i, c)| (!*c).then_some(i)).collect();
    let unclaimed_new: Vec<usize> = claimed_new.iter().enumerate().filter_map(|(i, c)| (!*c).then_some(i)).collect();
    if let ([oi], [ni]) = (unclaimed_old.as_slice(), unclaimed_new.as_slice()) {
        map.insert(old[*oi].0.clone(), new[*ni].0.clone());
    }

    map
}

/// The deepest timeline folder spanning the insertion slot between `slot - 1` and `slot`.
fn timeline_group_at_slot(doc: &Document, slot: usize) -> Option<u64> {
    let before = doc.features.get(slot.checked_sub(1)?)?.id;
    let after = doc.features.get(slot)?.id;
    let direct = |feature| doc.timeline_groups.iter().find(|group| group.features.contains(&feature)).map(|group| group.id);
    let parents: std::collections::BTreeMap<u64, Option<u64>> = doc.timeline_groups.iter().map(|group| (group.id, group.parent)).collect();
    let mut before_ancestors = std::collections::BTreeSet::new();
    let mut current = direct(before);
    let mut steps = 0usize;
    while let Some(id) = current {
        if !before_ancestors.insert(id) || steps >= doc.timeline_groups.len() {
            return None;
        }
        current = parents.get(&id).copied().flatten();
        steps = steps.saturating_add(1);
    }
    current = direct(after);
    steps = 0;
    while let Some(id) = current {
        if before_ancestors.contains(&id) {
            return Some(id);
        }
        if steps >= doc.timeline_groups.len() {
            return None;
        }
        current = parents.get(&id).copied().flatten();
        steps = steps.saturating_add(1);
    }
    None
}

/// Move `moving_ids` earlier in `doc.features` to `raw_target` while keeping `doc.marker` and
/// every `doc.timeline_groups` folder contiguous and ordered by timeline index.
fn move_features_to(doc: &mut Document, moving_ids: &[u64], raw_target: usize) {
    if moving_ids.is_empty() {
        return;
    }
    let moving_set: std::collections::BTreeSet<u64> = moving_ids.iter().copied().collect();
    let before_target = doc.features.iter().take(raw_target).filter(|f| moving_set.contains(&f.id)).count();
    let target = raw_target.saturating_sub(before_target);
    let crossed_marker =
        doc.marker.map_or(0, |m| doc.features.iter().enumerate().filter(|(i, f)| moving_set.contains(&f.id) && *i >= m && target < m).count());
    let mut moving = Vec::with_capacity(moving_ids.len());
    doc.features.retain(|f| {
        if moving_set.contains(&f.id) {
            moving.push(f.clone());
            false
        } else {
            true
        }
    });
    if moving.is_empty() {
        return;
    }
    for group in &mut doc.timeline_groups {
        group.features.retain(|id| !moving_set.contains(id));
    }
    let insert_at = target.min(doc.features.len());
    let spanning_group = timeline_group_at_slot(doc, insert_at);
    let moved_ids: Vec<u64> = moving.iter().map(|f| f.id).collect();
    doc.features.splice(insert_at..insert_at, moving);
    if let Some(m) = &mut doc.marker {
        *m = m.saturating_add(crossed_marker).min(doc.features.len());
    }
    if let Some(group_id) = spanning_group
        && let Some(group) = doc.timeline_groups.iter_mut().find(|g| g.id == group_id)
    {
        group.features.extend(moved_ids);
    }
    if !doc.timeline_groups.is_empty() {
        let positions: std::collections::BTreeMap<u64, usize> = doc.features.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
        for group in &mut doc.timeline_groups {
            group.features.sort_by_key(|id| positions.get(id).copied().unwrap_or(usize::MAX));
        }
    }
}

/// The earliest timeline index of a body pattern or mirror referencing a body of `occurrence`
/// that `body_map` can retarget.
fn earliest_retargeted_consumer(doc: &Document, occurrence: u64, body_map: &std::collections::BTreeMap<String, String>) -> Option<usize> {
    if body_map.is_empty() {
        return None;
    }
    doc.features.iter().enumerate().find_map(|(i, f)| match &f.kind {
        FeatureKind::Pattern { bodies, placements, .. } | FeatureKind::Mirror { bodies, placements, .. }
            if bodies
                .iter()
                .zip(placements.iter())
                .any(|(body, path)| path.last() == Some(&occurrence) && body_map.contains_key(body.0.as_str())) =>
        {
            Some(i)
        }
        _ => None,
    })
}

/// Retarget body patterns and mirrors referencing `occurrence`'s old body names to the
/// replacement linked component's matched body names (#21378).
fn retarget_occurrence_copies(doc: &mut Document, occurrence: u64, body_map: &std::collections::BTreeMap<String, String>) {
    if body_map.is_empty() {
        return;
    }
    for f in &mut doc.features {
        match &mut f.kind {
            FeatureKind::Pattern { bodies, placements, .. } | FeatureKind::Mirror { bodies, placements, .. } => {
                for (body, path) in bodies.iter_mut().zip(placements.iter()) {
                    if path.last() == Some(&occurrence)
                        && let Some(mapped) = body_map.get(body.0.as_str())
                    {
                        body.0.clone_from(mapped);
                    }
                }
            }
            _ => {}
        }
    }
}

/// One placement of `comp` comes to show the design at `file` instead: as a placement of a
/// component already linked to it, or of a new one under the same parent. The placement keeps
/// its id, position and grounding, so its joints stay (moved to the new bodies' faces), and
/// downstream body patterns and mirrors referencing its bodies are retargeted when the new
/// design has an unambiguous 1-to-1 body match (#21378).
fn relink_placement(
    s: &mut Session,
    comp: u64,
    link: &DesignLink,
    occurrence: u64,
    given: &str,
    file: &Path,
    configuration: Option<SelectedConfiguration>,
) -> Result<Value> {
    let cmd = "component.relink";
    if same_link_source(s, link, file) && selected_configuration(link) == configuration {
        return Err(bad(cmd, "that placement already shows this design"));
    }
    let parent = s.doc.occurrences.iter().find(|o| o.id == occurrence).map_or(0, |o| o.parent);
    let old = s.model.state();
    let relink_joints_before = snapshot_relink_joints(s, comp, Some(occurrence));
    let poses_before = snapshot_occurrence_poses(s);
    // A pattern or mirror of this placement can still fail when the replacement has no unambiguous
    // matching body: the reply names each feature that does.
    let failing_before: std::collections::BTreeSet<u64> = s.model.results.iter().filter(|r| r.error.is_some()).map(|r| r.id).collect();
    let warned_before: std::collections::BTreeSet<u64> = s.model.results.iter().filter(|r| r.warning.is_some()).map(|r| r.id).collect();
    let old_mesh_feature = link_mesh_feature(s, comp, link.feature).map(|(id, _)| id);
    let old_solid_bodies = linked_body_entries(s, link.feature);
    let old_mesh_bodies = old_mesh_feature.map(|id| linked_body_entries(s, id)).unwrap_or_default();
    let made = insert_design(s, &json!({"path": given, "parent": parent, "reuse": true, "configuration": configuration.map(|c| c.row)}))?;
    let linked = made["component"].as_u64().ok_or_else(|| bad(cmd, "the linked component was not created"))?;
    if s.doc.links.get(&linked).and_then(selected_configuration) != configuration {
        return Err(bad(cmd, "the selected configuration changed while relinking; try again"));
    }
    let feature = s.doc.links.get(&linked).map(|l| l.feature).ok_or_else(|| bad(cmd, "the design was not linked"))?;
    let new_mesh_feature = link_mesh_feature(s, linked, feature).map(|(id, _)| id);
    let geometry_changed = match (s.doc.feature(link.feature), s.doc.feature(feature)) {
        (Some(was), Some(now)) => !same_import_source(&was.kind, &now.kind) || link_meshes(s, comp, link.feature) != link_meshes(s, linked, feature),
        _ => true,
    };
    let mut warnings: Vec<String> = made["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect();
    if geometry_changed && s.doc.assembly.joints.iter().any(|j| j.a.occurrence == occurrence || j.b.occurrence == occurrence) {
        warnings.push("the linked geometry changed; check this placement's joints, even when the solver reports no error".to_string());
    }
    // Pin evaluated body names before repositioning the replacement's Import feature or
    // retargeting patterns/mirrors so moving an import earlier never renumbers existing bodies
    // or shifts inherited materials/appearances.
    s.pin_timeline_body_names();
    let new_solid_bodies = linked_body_entries(s, feature);
    let new_mesh_bodies = new_mesh_feature.map(|id| linked_body_entries(s, id)).unwrap_or_default();
    let mut body_map = match_relink_bodies(&old_solid_bodies, &new_solid_bodies);
    for (old_name, new_name) in match_relink_bodies(&old_mesh_bodies, &new_mesh_bodies) {
        body_map.entry(old_name).or_insert(new_name);
    }
    let earliest_consumer = earliest_retargeted_consumer(&s.doc, occurrence, &body_map);
    // Insert Design placed the design once; this placement takes that one's place and name.
    let added = made["occurrence"].as_u64().ok_or_else(|| bad(cmd, "the design was not placed"))?;
    let d = s.doc_mut();
    if let Some(first) = earliest_consumer {
        let mut moving_ids = Vec::new();
        if d.feature_index(feature).is_some_and(|i| i >= first) {
            moving_ids.push(feature);
        }
        if let Some(mesh_id) = new_mesh_feature
            && d.feature_index(mesh_id).is_some_and(|i| i >= first)
        {
            moving_ids.push(mesh_id);
        }
        if !moving_ids.is_empty() {
            let after_old = match (d.feature_index(link.feature), old_mesh_feature.and_then(|id| d.feature_index(id))) {
                (Some(a), Some(b)) if b == a + 1 => b + 1,
                (Some(a), _) => a + 1,
                (None, _) => first,
            };
            move_features_to(d, &moving_ids, after_old.min(first));
        }
    }
    retarget_occurrence_copies(d, occurrence, &body_map);
    let name = d.occurrences.iter().find(|o| o.id == added).map(|o| o.name.clone()).unwrap_or_default();
    d.occurrences.retain(|o| o.id != added);
    if let Some(o) = d.occurrences.iter_mut().find(|o| o.id == occurrence) {
        o.component = linked;
        o.name = name;
    }
    if s.active_occurrence_path.as_ref().is_some_and(|path| path.contains(&occurrence)) {
        s.active_component = 0;
        s.active_occurrence_path = None;
    }
    s.refresh();
    extract::rename_joint_faces(s, &old, feature, &std::collections::BTreeSet::from([occurrence]));
    s.refresh();
    warnings.extend(
        s.model
            .results
            .iter()
            .filter(|r| !failing_before.contains(&r.id))
            .filter_map(|r| r.error.as_ref().map(|e| format!("`{}` worked before this relink and fails now: {e}", r.name))),
    );
    warnings.extend(
        s.model
            .results
            .iter()
            .filter(|r| {
                r.id != feature
                    && Some(r.id) != new_mesh_feature
                    && r.error.is_none()
                    && !failing_before.contains(&r.id)
                    && !warned_before.contains(&r.id)
            })
            .filter_map(|r| r.warning.as_ref().map(|w| format!("`{}` worked before this relink and now warns: {w}", r.name))),
    );
    let (mut joint_errors, joint_conflicts) = joint_diagnostics(s, &mut warnings);
    let joint_deviations = relink_joint_deviations(s, &relink_joints_before, &mut warnings, &mut joint_errors);
    let moved_occurrences = relink_moved_occurrences(s, &poses_before, &mut warnings);
    reply(
        cmd,
        RelinkResult {
            component: linked,
            previous_component: Some(comp),
            occurrence: Some(occurrence),
            feature,
            path: stored(s.path.as_deref(), file),
            previous_path: stored(s.path.as_deref(), &resolve(s.path.as_deref(), &link.path)),
            geometry_changed,
            warnings,
            joint_errors,
            joint_conflicts,
            joint_deviations,
            moved_occurrences,
            param_warnings: Vec::new(),
        },
    )
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.break_link";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc_mut().links.remove(&comp).ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    let path = stored(s.path.as_deref(), &resolve(s.path.as_deref(), &link.path));
    reply(cmd, BreakLinkResult { component: comp, path, param_warnings: Vec::new() })
}

/// What reads `link`'s file: the built-in STEP reader, an importer, or (for a design) nothing
/// to name.
fn link_source(s: &Session, link: &DesignLink) -> Option<LinkSource> {
    let plain =
        |format: &str| LinkSource { format: format.into(), plugin: None, version: None, available: None, installed_version: None, needs: None };
    match (&link.importer, &link.step_names) {
        (Some(i), _) if i.is_kicad() => Some(plain(&i.format)),
        (Some(i), _) => {
            let Some(plugin) = &i.plugin else {
                return Some(LinkSource {
                    available: Some(false),
                    needs: Some("this link's record names no importer plugin, so nothing can read its file again".into()),
                    ..plain(&i.format)
                });
            };
            let status = crate::plugin::importer_status(s, plugin, &i.format);
            let needs = importer_needs(i, &status);
            let differs = status.installed_version.as_ref().is_some_and(|v| Some(v) != i.version.as_ref());
            Some(LinkSource {
                format: i.format.clone(),
                plugin: i.plugin.clone(),
                version: i.version.clone(),
                available: Some(status.problem.is_none()),
                installed_version: status.installed_version.filter(|_| differs || needs.is_some()),
                needs,
            })
        }
        (None, Some(_)) => Some(plain("step")),
        (None, None) => None,
    }
}

/// What a link needs when the importer plugin it names can't read its file: which plugin and
/// version, and why it can't. `None` when it can.
fn importer_needs(importer: &solidcraft_doc::LinkImporter, status: &crate::plugin::ImporterStatus) -> Option<String> {
    let problem = status.problem?;
    let plugin = importer.plugin.as_deref().unwrap_or(&importer.format);
    let version = importer.version.as_deref().map_or_else(String::new, |v| format!(" version {v}"));
    let installed = match &status.installed_version {
        Some(v) if Some(v) != importer.version.as_ref() => format!(" (version {v} is installed)"),
        _ => String::new(),
    };
    Some(format!("importer plugin `{plugin}`{version} is needed to read this file again, but it is {problem}{installed}"))
}

enum ListEntry {
    Empty {
        c: u64,
        name: Option<String>,
        configuration: Option<u64>,
    },
    ImporterNotAllowed {
        c: u64,
        name: Option<String>,
        configuration: Option<u64>,
        importer: Option<LinkSource>,
        path: String,
        shown: String,
        reason: String,
    },
    Network {
        c: u64,
        name: Option<String>,
        configuration: Option<u64>,
        importer: Option<LinkSource>,
        path: String,
        shown: String,
        host: String,
    },
    Device {
        c: u64,
        name: Option<String>,
        configuration: Option<u64>,
        importer: Option<LinkSource>,
        path: String,
        shown: String,
    },
    LookUp {
        c: u64,
        name: Option<String>,
        configuration: Option<u64>,
        importer: Option<LinkSource>,
        path: String,
        shown: String,
        abs: PathBuf,
        any: bool,
        kept_sha256: Option<String>,
        is_left_out: bool,
    },
}

fn list_links(s: &mut Session, _p: &Value) -> Result<Value> {
    let own = s.path.as_deref();
    let comp_names: std::collections::HashMap<u64, String> = s.doc.components.iter().map(|c| (c.id, c.name.clone())).collect();
    let mut entries: Vec<ListEntry> = Vec::with_capacity(s.doc.links.len());
    let mut unique_shown: Vec<String> = Vec::new();
    let mut seen_shown = std::collections::HashSet::new();

    for (&c, l) in &s.doc.links {
        let name = comp_names.get(&c).cloned();
        // A design that came from this session has no file: it is not looked for.
        if l.path.is_empty() {
            entries.push(ListEntry::Empty { c, name, configuration: l.configuration });
            continue;
        }
        let file = resolve(own, &l.path);
        let path = stored(own, &file);
        let shown = file.to_string_lossy().into_owned();
        let importer = link_source(s, l);
        // A refused path is not looked for at all: on Windows asking whether a file on
        // `\\host\share` exists already connects to the host.
        match link_refusal(own, &l.path) {
            None => {
                if l.importer.is_some()
                    && let Some(reason) = importer_file_refusal(s, &file)
                {
                    // Not read, not even hashed: an importer's file outside the design's folder
                    // waits for its folder to be allowed.
                    entries.push(ListEntry::ImporterNotAllowed { c, name, configuration: l.configuration, importer, path, shown, reason });
                    continue;
                }
                if seen_shown.insert(shown.clone()) {
                    unique_shown.push(shown.clone());
                }
                entries.push(ListEntry::LookUp {
                    c,
                    name,
                    configuration: l.configuration,
                    importer,
                    path,
                    abs: absolute(&file),
                    shown,
                    any: l.importer.is_some(),
                    kept_sha256: l.file_sha256.clone(),
                    is_left_out: left_out(&s.doc, c, l),
                });
            }
            Some(solidcraft_io::vfs::Remote::Network(host)) => {
                entries.push(ListEntry::Network { c, name, configuration: l.configuration, importer, path, shown, host });
            }
            Some(solidcraft_io::vfs::Remote::Device) => {
                entries.push(ListEntry::Device { c, name, configuration: l.configuration, importer, path, shown });
            }
        }
    }

    // Deduplicate presence/is_file checks per unique resolved path.
    let mut statuses: std::collections::HashMap<String, (Option<bool>, Option<(LinkState, String)>)> =
        std::collections::HashMap::with_capacity(unique_shown.len());
    for shown in unique_shown {
        let found = solidcraft_io::vfs::design_path_is_file(&shown, own);
        let presence = link_presence(Path::new(&shown), own).map(|why| (why.state, why.reason));
        statuses.insert(shown, (found, presence));
    }

    // Deduplicate SHA-256 checks per unique resolved `(absolute_path, any)` and hash uncached files in parallel.
    let mut seen_digests = std::collections::HashSet::new();
    let mut digests_map: std::collections::HashMap<(PathBuf, bool), Option<String>> = std::collections::HashMap::new();
    let mut uncached: Vec<(PathBuf, bool, u64, Option<(std::time::SystemTime, u64)>)> = Vec::new();
    for entry in &entries {
        let ListEntry::LookUp { shown, abs, any, kept_sha256, .. } = entry else { continue };
        if kept_sha256.is_none() || statuses.get(shown).is_none_or(|(found, _)| *found != Some(true)) {
            continue;
        }
        if !seen_digests.insert((abs.clone(), *any)) {
            continue;
        }
        match lookup_cached_sha256(abs, own) {
            Some(DigestLookup::Hit(known)) => {
                digests_map.insert((abs.clone(), *any), Some(known));
            }
            Some(DigestLookup::Miss { generation, stamp }) => {
                uncached.push((abs.clone(), *any, generation, stamp));
            }
            None => {
                digests_map.insert((abs.clone(), *any), None);
            }
        }
    }
    if !uncached.is_empty() {
        let computed = map_link_work(&uncached, |(abs, any, _, _)| compute_file_sha256(abs, own, *any));
        for ((abs, any, generation, stamp), digest) in uncached.into_iter().zip(computed) {
            let digest = digest.flatten();
            if let Some(d) = &digest {
                store_cached_sha256(abs.clone(), generation, stamp, d);
            }
            digests_map.insert((abs, any), digest);
        }
    }

    let mut observations = Vec::new();
    let links: Vec<LinkInfo> = entries
        .into_iter()
        .map(|entry| match entry {
            ListEntry::Empty { c, name, configuration } => LinkInfo::Looked {
                component: c,
                configuration,
                importer: None,
                name,
                path: String::new(),
                file: String::new(),
                found: None,
                changed: None,
                state: None,
                reason: None,
            },
            ListEntry::ImporterNotAllowed { c, name, configuration, importer, path, shown, reason } => LinkInfo::Looked {
                component: c,
                configuration,
                importer,
                name,
                path,
                file: shown,
                found: None,
                changed: None,
                state: Some(LinkState::NotAllowed),
                reason: Some(reason),
            },
            ListEntry::Network { c, name, configuration, importer, path, shown, host } => LinkInfo::Network {
                component: c,
                configuration,
                importer,
                name,
                path,
                file: shown,
                found: None,
                state: LinkState::NotAllowed,
                reason: format!("on the network location \\\\{host}, which is not allowed"),
                host,
            },
            ListEntry::Device { c, name, configuration, importer, path, shown } => LinkInfo::Device {
                component: c,
                configuration,
                importer,
                name,
                path,
                file: shown,
                found: None,
                state: LinkState::Unreadable,
                reason: "names a device, not a file".into(),
            },
            ListEntry::LookUp { c, name, configuration, importer, path, shown, abs, any, kept_sha256, is_left_out } => {
                let (found, presence) = statuses.get(&shown).cloned().unwrap_or((None, None));
                // Whether the file is no longer what this design last read (#9739): `null`
                // when it isn't found, or the link keeps no digest (made before they were).
                let current = kept_sha256.as_ref().filter(|_| found == Some(true)).and_then(|_| digests_map.get(&(abs, any)).cloned().flatten());
                let changed = current.as_ref().zip(kept_sha256.as_ref()).map(|(now, kept)| now != kept);
                // A link opened without its bodies has read nothing of its file yet.
                let changed = if found == Some(true) && is_left_out { Some(true) } else { changed };
                if let (Some(digest), Some(changed)) = (current, changed)
                    && (changed || s.plugin_state.link_changes.get(&c).is_some_and(|change| !change.read))
                {
                    observations.push((c, shown.clone(), digest, !changed));
                }
                // Why a file that can't be reached can't be: `None` when it can be read.
                let (state, reason) = match presence {
                    Some((state, reason)) => (Some(state), Some(reason)),
                    None => (None, None),
                };
                LinkInfo::Looked { component: c, configuration, importer, name, path, file: shown, found, changed, state, reason }
            }
        })
        .collect();
    for (component, path, digest, read) in observations {
        crate::plugin::record_link_change(s, component, path, digest, read);
    }
    reply("component.links", LinksResult { links, param_warnings: Vec::new() })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn save_box(path: &Path, height: f64) {
        let mut part = Session::default();
        run(&mut part, "solid.box", json!({"length": 10, "width": 20, "height": height, "body_name": "Plate"}));
        run(&mut part, "file.save", json!({"path": path}));
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    }

    #[cfg(unix)]
    #[test]
    fn save_as_through_directory_alias_keeps_links_after_reopening_and_moving() {
        for format in ["json", "container"] {
            let dir = std::env::temp_dir().join(format!("solidcraft-link-directory-alias-{format}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let dir = dir.canonicalize().unwrap();
            let project = dir.join("projects/example");
            std::fs::create_dir_all(project.join("out")).unwrap();
            let alias = dir.join("output");
            std::os::unix::fs::symlink(project.join("out"), &alias).unwrap();
            let source_alias = dir.join("source");
            std::os::unix::fs::symlink(&project, &source_alias).unwrap();
            let part = project.join("part.sc");
            let assembly = project.join("assembly.sc");
            save_box(&part, 30.0);
            let mut s = Session::default();
            run(&mut s, "component.insert_design", json!({"path": part}));
            run(&mut s, "file.save", json!({"path": assembly}));
            run(&mut s, "doc.open", json!({"path": assembly}));
            let saved = alias.join("assembly.sc");
            run(&mut s, "file.save_as", json!({"path": saved, "format": format}));
            let disk = solidcraft_io::read_design(&std::fs::read(&saved).unwrap()).unwrap();
            assert_eq!(disk.links.len(), 1);
            let link = &disk.links.values().next().unwrap().path;
            assert_eq!(link, "../part.sc");
            assert!(saved.parent().unwrap().join(link).is_file());
            for reopened in [saved, project.join("out/assembly.sc")] {
                run(&mut s, "doc.open", json!({"path": reopened}));
                assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["found"], true);
                let mut reused = s.scratch();
                run(&mut reused, "component.insert_design", json!({"path": source_alias.join("part.sc"), "reuse": true}));
                assert_eq!(reused.doc.links.len(), 1);
            }
            let moved = dir.join("moved");
            std::fs::rename(&project, &moved).unwrap();
            save_box(&moved.join("part.sc"), 40.0);
            run(&mut s, "doc.open", json!({"path": moved.join("out/assembly.sc")}));
            assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["part"]));
            assert!((volume(&mut s) - 8000.0).abs() < 1e-6);
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// #6860: Save As changes the file's folder, not the targets kept in the edit history.
    #[test]
    fn save_as_keeps_link_targets_through_undo_redo_and_checkpoints() {
        for on_copy in [false, true] {
            let dir = std::env::temp_dir().join(format!("solidcraft-link-save-as-{}-{on_copy}", std::process::id()));
            std::fs::create_dir_all(dir.join("parts")).unwrap();
            std::fs::create_dir_all(dir.join("asm/sub/deeper")).unwrap();
            let part = dir.join("parts/plate.solidcraft");
            let case = dir.join("asm/case.solidcraft");
            save_box(&part, 30.0);
            let mut s = Session::default();
            run(&mut s, "component.insert_design", json!({"path": part}));
            run(&mut s, "file.save", json!({"path": case}));
            // Opening starts from the relative paths actually written to disk.
            run(&mut s, "doc.open", json!({"path": case}));
            run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [60, 0, 0]}));
            let checkpoint = s.checkpoint();
            run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [80, 0, 0]}));
            run(&mut s, "edit.undo", json!({}));
            let before = s.doc.clone();
            let undo_before = s.undo.last().unwrap().doc.clone();
            let missing = dir.join("missing/case.solidcraft");
            assert!(s.execute("file.save_as", &json!({"path": missing})).is_err());
            assert_eq!(s.path.as_deref(), case.to_str());
            for folder in ["asm/sub", "asm/sub/deeper"] {
                let path = dir.join(folder).join("case.solidcraft");
                if on_copy {
                    let mut copy = s.scratch();
                    run(&mut copy, "file.save_as", json!({"path": path}));
                    s.adopt_save(&copy);
                } else {
                    run(&mut s, "file.save_as", json!({"path": path}));
                }
                assert!(!s.is_dirty());
                for command in ["edit.redo", "edit.undo", "edit.undo", "edit.redo"] {
                    run(&mut s, command, json!({}));
                    let links = run(&mut s, "component.links", json!({}));
                    assert_eq!(links["links"][0]["found"], true, "{on_copy}, {folder}, {command}: {links}");
                    assert!(same_file(Path::new(links["links"][0]["file"].as_str().unwrap()), &part));
                }
            }
            assert!(std::sync::Arc::ptr_eq(&s.doc, &before), "saving does not rewrite the live document");
            assert!(std::sync::Arc::ptr_eq(&s.undo.last().unwrap().doc, &undo_before), "saving does not copy the edit history");
            s.restore_checkpoint(checkpoint).unwrap();
            assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["found"], true);
            let path = s.path.clone().unwrap();
            for format in ["json", "container"] {
                run(&mut s, "file.save", json!({"format": format}));
                let disk = solidcraft_io::read_design(&std::fs::read(&path).unwrap()).unwrap();
                assert_eq!(disk.links.values().next().unwrap().path, "../../../parts/plate.solidcraft");
                run(&mut s, "doc.open", json!({"path": path}));
                assert!(!s.is_dirty());
                assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["found"], true);
            }
            save_box(&part, 45.0);
            assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["plate"]));
            assert!((volume(&mut s) - 9125.0).abs() < 1e-6);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn recovered_relative_links_keep_their_targets_after_save_as() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-recovery-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::create_dir_all(dir.join("asm/sub")).unwrap();
        let part = dir.join("parts/plate.solidcraft");
        save_box(&part, 30.0);
        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": part}));
        run(&mut s, "file.save", json!({"path": dir.join("asm/case.solidcraft")}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [60, 0, 0]}));
        let recovery = dir.join("recovery");
        let mut autosaver = crate::recovery::Autosaver::new(&recovery, std::time::Duration::ZERO).unwrap();
        assert!(autosaver.save(&s).unwrap());
        drop(autosaver);
        let entry = crate::recovery::orphans(&recovery).pop().unwrap();
        // Older autosaves hold paths relative to the design, not the recovery folder.
        std::fs::write(recovery.join(format!("{}.solidcraft", entry.id)), solidcraft_io::write_design(&s.design()).unwrap()).unwrap();
        let mut recovered = Session::default();
        run(&mut recovered, "doc.recover", json!({"dir": recovery, "id": entry.id}));
        assert_eq!(run(&mut recovered, "component.links", json!({}))["links"][0]["found"], true);
        run(&mut recovered, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [80, 0, 0]}));
        run(&mut recovered, "file.save_as", json!({"path": dir.join("asm/sub/case.solidcraft")}));
        run(&mut recovered, "edit.undo", json!({}));
        assert_eq!(run(&mut recovered, "component.links", json!({}))["links"][0]["found"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_assembly_reopens_with_its_link_updates_it_and_breaks_it() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-reopen-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::create_dir_all(dir.join("asm")).unwrap();
        let part = dir.join("parts").join("plate.solidcraft");
        let case = dir.join("asm").join("case.solidcraft");
        save_box(&part, 30.0);
        // Given an assembly holding the part, saved in a folder beside the part's.
        let mut a = Session::default();
        let inserted = run(&mut a, "component.insert_design", json!({"path": part}));
        assert_eq!(inserted["name"], "plate");
        run(&mut a, "file.save", json!({"path": case}));
        assert!((volume(&mut a) - 6000.0).abs() < 1e-6);
        // When it is opened again, the link is kept relative to it and finds the part.
        let mut b = Session::default();
        run(&mut b, "doc.open", json!({"path": case}));
        let links = run(&mut b, "component.links", json!({}));
        assert_eq!(links["links"][0]["path"], "../parts/plate.solidcraft", "{links}");
        assert_eq!(links["links"][0]["found"], true, "{links}");
        assert!((volume(&mut b) - 6000.0).abs() < 1e-6);
        // An unchanged part leaves the assembly as it is; an edited one comes in on update.
        assert_eq!(run(&mut b, "component.update_links", json!({}))["unchanged"], json!(["plate"]));
        save_box(&part, 45.0);
        assert_eq!(run(&mut b, "component.update_links", json!({}))["updated"], json!(["plate"]));
        assert!((volume(&mut b) - 9000.0).abs() < 1e-6, "{}", volume(&mut b));
        // A part file that has gone is reported, and the assembly keeps the bodies it had.
        std::fs::remove_file(&part).unwrap();
        let err = b.execute("component.update_links", &json!({})).unwrap_err().to_string();
        assert!(err.contains("plate"), "{err}");
        let links = run(&mut b, "component.links", json!({}));
        assert_eq!((&links["links"][0]["found"], &links["links"][0]["state"]), (&json!(false), &json!("missing")), "{links}");
        assert!((volume(&mut b) - 9000.0).abs() < 1e-6);
        // Break Link keeps the bodies and forgets the file.
        run(&mut b, "component.break_link", json!({"component": "plate"}));
        assert_eq!(run(&mut b, "component.links", json!({}))["links"], json!([]));
        assert!((volume(&mut b) - 9000.0).abs() < 1e-6);
        assert!(b.execute("component.update_links", &json!({"component": "plate"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_links_reads_again_only_the_designs_that_changed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-reads-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (plate, block) = (dir.join("plate.solidcraft"), dir.join("block.solidcraft"));
        save_box(&plate, 30.0);
        save_box(&block, 10.0);
        let mut a = Session::default();
        run(&mut a, "component.insert_design", json!({"path": plate}));
        run(&mut a, "component.insert_design", json!({"path": block, "translate": [50, 0, 0]}));
        let evaluated = || EVALUATED.with(std::cell::Cell::get);
        // Unchanged files are not evaluated again.
        let before = evaluated();
        let r = run(&mut a, "component.update_links", json!({}));
        assert_eq!((r["updated"].clone(), r["unchanged"].clone()), (json!([]), json!(["plate", "block"])), "{r}");
        assert_eq!(evaluated(), before);
        // An edited file is, and only that one.
        save_box(&plate, 45.0);
        let r = run(&mut a, "component.update_links", json!({}));
        assert_eq!((r["updated"].clone(), r["unchanged"].clone()), (json!(["plate"]), json!(["block"])), "{r}");
        assert_eq!(evaluated(), before + 1);
        assert!((volume(&mut a) - 11000.0).abs() < 1e-6, "{}", volume(&mut a));
        // Undone, the link holds the old bodies again: the same file brings the new ones back.
        run(&mut a, "edit.undo", json!({}));
        assert!((volume(&mut a) - 8000.0).abs() < 1e-6, "{}", volume(&mut a));
        assert_eq!(run(&mut a, "component.update_links", json!({}))["updated"], json!(["plate"]));
        assert!((volume(&mut a) - 11000.0).abs() < 1e-6, "{}", volume(&mut a));
        // A file saved again without its model is evaluated, and gives the same bodies.
        let mut part = Session::default();
        run(&mut part, "doc.open", json!({"path": block}));
        run(&mut part, "file.save", json!({"cache": false}));
        let before = evaluated();
        assert_eq!(run(&mut a, "component.update_links", json!({}))["unchanged"], json!(["plate", "block"]));
        assert_eq!(evaluated(), before + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #9739: a link keeps what it read of its file, so an assembly opened again says which
    /// linked designs changed while it was closed; Update Links takes the change and the record.
    #[test]
    fn a_link_says_its_file_changed_while_the_design_was_closed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-changed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (part, assembly) = (dir.join("p.sc"), dir.join("a.sc"));
        save_box(&part, 10.0);
        let mut a = Session::default();
        run(&mut a, "component.insert_design", json!({"path": part}));
        run(&mut a, "file.save", json!({"path": assembly}));
        let changed = |s: &mut Session| run(s, "component.links", json!({}))["links"][0]["changed"].clone();
        assert_eq!(changed(&mut a), json!(false));
        // The part is edited while the assembly is closed.
        save_box(&part, 30.0);
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": assembly}));
        assert_eq!(changed(&mut opened), json!(true));
        assert!((volume(&mut opened) - 2000.0).abs() < 1e-6, "{}", volume(&mut opened));
        assert_eq!(run(&mut opened, "component.update_links", json!({}))["updated"], json!(["p"]));
        assert_eq!(changed(&mut opened), json!(false));
        assert!((volume(&mut opened) - 6000.0).abs() < 1e-6, "{}", volume(&mut opened));
        // Saved, it opens up to date.
        run(&mut opened, "file.save", json!({}));
        let mut again = Session::default();
        run(&mut again, "doc.open", json!({"path": assembly}));
        assert_eq!(changed(&mut again), json!(false));
        // A link made before the record says nothing; Update Links gives it one.
        again.doc_mut().links.values_mut().for_each(|l| l.file_sha256 = None);
        assert_eq!(changed(&mut again), Value::Null);
        assert_eq!(run(&mut again, "component.update_links", json!({}))["unchanged"], json!(["p"]));
        assert_eq!(changed(&mut again), json!(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #9743: a list of links is read in one step that goes on past a design it can't read.
    #[test]
    fn update_links_given_a_list_reads_them_in_one_step_past_a_design_it_cannot_read() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (plate, block) = (dir.join("plate.solidcraft"), dir.join("block.solidcraft"));
        save_box(&plate, 30.0);
        save_box(&block, 10.0);
        let mut a = Session::default();
        let first = run(&mut a, "component.insert_design", json!({"path": plate}))["component"].clone();
        let second = run(&mut a, "component.insert_design", json!({"path": plate, "translate": [50, 0, 0]}))["component"].clone();
        let third = run(&mut a, "component.insert_design", json!({"path": block, "translate": [100, 0, 0]}))["component"].clone();
        assert!((volume(&mut a) - 14000.0).abs() < 1e-6, "{}", volume(&mut a));
        // Two links to the same changed file come in together, and one Undo takes both back.
        save_box(&plate, 45.0);
        let r = run(&mut a, "component.update_links", json!({"components": [first, second]}));
        assert_eq!((r["updated"].as_array().map(Vec::len), r["failed"].clone()), (Some(2), json!([])), "{r}");
        assert!((volume(&mut a) - 20000.0).abs() < 1e-6, "{}", volume(&mut a));
        run(&mut a, "edit.undo", json!({}));
        assert!((volume(&mut a) - 14000.0).abs() < 1e-6, "{}", volume(&mut a));
        // A design it can't read keeps its bodies and is named; the others still come in.
        std::fs::write(&block, b"not a design").unwrap();
        let r = run(&mut a, "component.update_links", json!({"components": [first, third]}));
        assert_eq!(r["updated"].as_array().map(Vec::len), Some(1), "{r}");
        assert_eq!((r["failed"][0]["component"].clone(), r["failed"][0]["name"].clone()), (third.clone(), json!("block")), "{r}");
        assert!(r["warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.starts_with("block")))), "{r}");
        assert!((volume(&mut a) - 17000.0).abs() < 1e-6, "{}", volume(&mut a));
        // Not taken in, it still says its file changed (#9739).
        let links = run(&mut a, "component.links", json!({}));
        let block_link = links["links"].as_array().and_then(|l| l.iter().find(|l| l["component"] == third)).cloned();
        assert_eq!(block_link.map(|l| l["changed"].clone()), Some(json!(true)), "{links}");
        // None it can read: an error, and nothing to undo.
        let err = a.execute("component.update_links", &json!({"components": [third]})).unwrap_err().to_string();
        assert!(err.contains("block"), "{err}");
        // Malformed lists are refused.
        for bad in
            [json!({"components": []}), json!({"components": 3}), json!({"components": [first], "component": first}), json!({"components": ["nope"]})]
        {
            assert!(a.execute("component.update_links", &bad).is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inserting_a_missing_file_or_bad_parameters_changes_nothing() {
        let mut s = Session::default();
        let missing = std::env::temp_dir().join(format!("solidcraft-no-such-part-{}.solidcraft", std::process::id()));
        assert!(s.execute("component.insert_design", &json!({"path": missing})).is_err());
        assert!(s.execute("component.insert_design", &json!({})).is_err());
        assert!(s.execute("component.insert_design", &json!({"path": "x.solidcraft", "translate": [1, "a", 2]})).is_err());
        assert!(s.doc.components.is_empty() && s.doc.links.is_empty() && s.doc.features.is_empty());
    }

    /// #3330's part: a plate 60 × 40 × 5 with a Ø 6 hole at x = `hx` (10 mm), saved at `path`. The
    /// session stays open to edit the part as a user would.
    fn plate(path: &Path) -> Session {
        let mut part = Session::default();
        run(&mut part, "parameters.add", json!({"name": "hx", "expression": "10 mm"}));
        run(&mut part, "solid.box", json!({"length": 60, "width": 40, "height": 5, "corner": [-30, -20, 0], "body_name": "Plate"}));
        run(&mut part, "solid.cylinder", json!({"diameter": 6, "height": 7, "base": ["hx", 0, -1], "operation": "cut", "name": "Hole"}));
        run(&mut part, "file.save", json!({"path": path}));
        part
    }

    /// Another Ø 6 hole through the plate at `x`.
    fn hole(part: &mut Session, x: f64, name: &str) {
        run(part, "solid.cylinder", json!({"diameter": 6, "height": 7, "base": [x, 0, -1], "operation": "cut", "name": name}));
    }

    /// #3330's assembly: the plate linked in and grounded, and a pin rigidly jointed to the top edge
    /// of its hole. `older`: the link keeps no face names, as one made before them.
    fn pinned(path: &Path, older: bool) -> Session {
        let mut s = Session::default();
        let feature = run(&mut s, "component.insert_design", json!({"path": path, "name": "Plate"}))["feature"].as_u64().unwrap();
        if older {
            s.doc_mut().feature_mut(feature).unwrap().link_faces.clear();
            s.refresh();
        }
        run(&mut s, "occurrence.ground", json!({"occurrence": "Plate:1", "grounded": true}));
        run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20, "base": [0, 50, 0], "operation": "new", "body_name": "PinBody"}));
        run(&mut s, "component.create", json!({"name": "Pin", "parent": "root", "activate": false}));
        run(&mut s, "component.move_bodies", json!({"bodies": ["PinBody"], "component": "Pin"}));
        let joint = json!({"type": "rigid", "name": "PinInHole", "flip": true,
            "a": {"occurrence": "Plate:1", "circle": [13, 0, 5]}, "b": {"occurrence": "Pin:1", "circle": [3, 50, 0]}});
        run(&mut s, "joint.create", joint);
        s
    }

    /// Asserts the pin's joint sits on the plate's top at x (world) and solves without errors.
    fn assert_joint_at(s: &mut Session, x: f64, case: &str) {
        let list = run(s, "joint.list", json!({}));
        assert_eq!(list["errors"], json!([]), "{case}: {list}");
        let o = &list["joints"][0]["frame_a"]["origin"];
        let at = [0, 1, 2].map(|i| o[i].as_f64().unwrap_or(f64::NAN));
        assert!((at[0] - x).abs() < 1e-6 && at[1].abs() < 1e-6 && (at[2] - 5.0).abs() < 1e-6, "{case}: the joint is at {at:?}, not x = {x}");
    }

    /// #3330: a joint on a linked part's hole stays on that hole when Update Links brings in an
    /// edit of the part, as it does inside one design, also when the edit adds a hole of the same
    /// size that takes the first one's place in the file.
    #[test]
    fn joints_on_a_linked_part_follow_its_faces_through_update_links() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let hx = |part: &mut Session, x: &str| {
            run(part, "parameters.change", json!({"name": "hx", "expression": x}));
        };
        let edits: [(&str, &dyn Fn(&mut Session), f64); 3] = [
            ("the hole moved to x = 20", &|p: &mut Session| hx(p, "20 mm"), 20.0),
            (
                "a hole at x = -10 added before it in the timeline, and the hole moved to x = 20",
                &|p: &mut Session| {
                    run(p, "timeline.rollback", json!({"position": 1}));
                    hole(p, -10.0, "Hole0");
                    run(p, "timeline.rollback", json!({}));
                    hx(p, "20 mm");
                },
                20.0,
            ),
            (
                "the hole moved to x = 5, and a hole added at x = 15",
                &|p: &mut Session| {
                    hx(p, "5 mm");
                    hole(p, 15.0, "Hole2");
                },
                5.0,
            ),
        ];
        for (case, edit, x) in edits {
            let mut part = plate(&path);
            let mut s = pinned(&path, false);
            assert_joint_at(&mut s, 10.0, case);
            edit(&mut part);
            run(&mut part, "file.save", json!({}));
            let update = run(&mut s, "component.update_links", json!({}));
            assert_eq!(update["updated"], json!(["Plate"]), "{case}: {update}");
            assert_eq!(update["warnings"], json!([]), "{case}: {update}");
            assert_joint_at(&mut s, x, case);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #22047: an unmoved planar face on a linked part (qualified by its neighbouring faces,
    /// `F<id>:link:<digest>#[…]`) keeps resolving its planar mate joint and leaves the mated
    /// occurrence in place with zero drift when an adjacent corner cut shortens and drops off its
    /// boundary.
    #[test]
    fn mate_joint_on_a_linked_planar_face_survives_adjacent_cut_shortening() {
        use solidcraft_doc::joints::{Snap, occurrence_world};

        let dir = std::env::temp_dir().join(format!("solidcraft-link-neighbour-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("side_plate.solidcraft");

        // Linked part: a block whose +x face (x = 100) is split by a lower slot into two pieces so
        // the upper mating piece carries a neighbour qualifier including its two pin holes and an
        // adjacent top corner cut.
        let mut part = Session::default();
        run(&mut part, "parameters.add", json!({"name": "cut_len", "expression": "52 mm"}));
        run(&mut part, "solid.box", json!({"length": 100, "width": 40, "height": 60, "corner": [0, 0, 0], "body_name": "SidePlate"}));
        run(&mut part, "solid.hole", json!({"position": [100, 12, 35], "direction": [-1, 0, 0], "diameter": 6, "depth": 20, "name": "PinHoleA"}));
        run(&mut part, "solid.hole", json!({"position": [100, 28, 35], "direction": [-1, 0, 0], "diameter": 6, "depth": 20, "name": "PinHoleB"}));
        run(
            &mut part,
            "solid.box",
            json!({"length": "cut_len", "width": 44, "height": 14, "corner": [50, -2, 50], "operation": "cut", "name": "TopCut"}),
        );
        run(
            &mut part,
            "solid.box",
            json!({"length": 20, "width": 44, "height": 10, "corner": [85, -2, 10], "operation": "cut", "name": "LowerSlot"}),
        );
        run(&mut part, "file.save", json!({"path": path}));

        // Assembly: link and ground SidePlate, build a Bracket with matching pin holes, and constrain
        // it with a planar mate on the +x face plus two cylindrical pin joints.
        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": path, "name": "SidePlate"}));
        run(&mut s, "occurrence.ground", json!({"occurrence": "SidePlate:1", "grounded": true}));
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 40, "height": 30, "corner": [100, 0, 20], "operation": "new", "body_name": "BracketBody"}),
        );
        run(&mut s, "solid.hole", json!({"position": [100, 12, 35], "direction": [1, 0, 0], "diameter": 6, "depth": 10, "name": "BracketHoleA"}));
        run(&mut s, "solid.hole", json!({"position": [100, 28, 35], "direction": [1, 0, 0], "diameter": 6, "depth": 10, "name": "BracketHoleB"}));
        run(&mut s, "component.create", json!({"name": "Bracket", "parent": "root", "activate": false}));
        run(&mut s, "component.move_bodies", json!({"bodies": ["BracketBody"], "component": "Bracket"}));

        run(
            &mut s,
            "joint.create",
            json!({
                "type": "planar",
                "name": "SideMate",
                "a": {"occurrence": "SidePlate:1", "face": [100, 20, 35]},
                "b": {"occurrence": "Bracket:1", "face": [100, 20, 35]},
            }),
        );
        run(
            &mut s,
            "joint.create",
            json!({
                "type": "cylindrical",
                "name": "PinA",
                "a": {"occurrence": "SidePlate:1", "circle": [100, 15, 35]},
                "b": {"occurrence": "Bracket:1", "circle": [100, 15, 35]},
            }),
        );
        run(
            &mut s,
            "joint.create",
            json!({
                "type": "cylindrical",
                "name": "PinB",
                "a": {"occurrence": "SidePlate:1", "circle": [100, 31, 35]},
                "b": {"occurrence": "Bracket:1", "circle": [100, 31, 35]},
            }),
        );

        let mate_face = match &s.doc.assembly.joints.iter().find(|j| j.name == "SideMate").unwrap().a.snap {
            Snap::FaceCenter { face: Some(name), .. } => name.clone(),
            other => panic!("expected named face on SideMate, got {other:?}"),
        };
        assert!(mate_face.contains(":link:") && mate_face.contains("#["), "expected linked neighbour-qualified face name, got {mate_face}");

        let list_before = run(&mut s, "joint.list", json!({}));
        assert_eq!(list_before["errors"], json!([]), "before update: {list_before}");
        assert_eq!(list_before["conflicts"], json!([]), "before update: {list_before}");
        let bracket_occ = s.doc.occurrences.iter().find(|o| o.name == "Bracket:1").unwrap().id;
        let world_before = occurrence_world(&s.doc, bracket_occ);

        // Shorten TopCut in the linked part so it stops at x = 80 and no longer borders x = 100.
        run(&mut part, "parameters.change", json!({"name": "cut_len", "expression": "30 mm"}));
        run(&mut part, "file.save", json!({}));

        let update = run(&mut s, "component.update_links", json!({}));
        assert_eq!(update["updated"], json!(["SidePlate"]), "{update}");
        assert_eq!(update["warnings"], json!([]), "{update}");

        let list_after = run(&mut s, "joint.list", json!({}));
        assert_eq!(list_after["errors"], json!([]), "after update: {list_after}");
        assert_eq!(list_after["conflicts"], json!([]), "after update: {list_after}");
        let mate_after = list_after["joints"].as_array().unwrap().iter().find(|j| j["name"] == "SideMate").unwrap();
        assert!(!mate_after["frame_a"].is_null(), "SideMate lost frame_a: {mate_after}");
        let frame_a_x = mate_after["frame_a"]["origin"][0].as_f64().unwrap();
        assert!((frame_a_x - 100.0).abs() < 1e-6, "SideMate frame_a moved off x = 100: {mate_after}");

        let world_after = occurrence_world(&s.doc, bracket_occ);
        for (a, b) in world_before.iter().flatten().zip(world_after.iter().flatten()) {
            assert!((a - b).abs() < 1e-6, "Bracket drifted after update_links: before={world_before:?}, after={world_after:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The plate linked in, with a sketch on its top face (away from the hole) extruded 3 mm.
    /// Returns the sketch's and the extrude's feature ids.
    fn sketched_on_plate(s: &mut Session, path: &Path) -> (u64, u64) {
        run(s, "component.insert_design", json!({"path": path, "name": "Plate"}));
        let sketch = run(s, "sketch.create", json!({"plane": {"face": [-20, 10, 5]}, "name": "OnPlate"}))["sketch"].as_u64().unwrap();
        run(s, "sketch.rectangle.two_point", json!({"p0": [-25, 5], "p1": [-15, 15]}));
        run(s, "sketch.finish", json!({}));
        let extrude = run(s, "solid.extrude", json!({"sketch": "OnPlate", "distance": 3}))["feature"].as_u64().unwrap();
        (sketch, extrude)
    }

    /// The error of feature `id`, if it failed.
    fn feature_error(s: &Session, id: u64) -> Option<String> {
        s.model.result(id).and_then(|r| r.error.clone())
    }

    /// A sketch on a linked part's face stays on that face when Update Links brings in an edit
    /// that leaves the face as it was, as joints do (#3330): the face is found by the name the
    /// part gives it, not by the imported file's digest, which any edit of the part changes.
    #[test]
    fn a_sketch_on_a_linked_face_follows_it_through_update_links() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-sketch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let edits: [(&str, &dyn Fn(&mut Session)); 3] = [
            ("a body added far from the plate", &|p: &mut Session| {
                run(p, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0], "body_name": "Spacer"}));
            }),
            ("the hole moved to x = 20", &|p: &mut Session| {
                run(p, "parameters.change", json!({"name": "hx", "expression": "20 mm"}));
            }),
            ("a second hole added", &|p: &mut Session| hole(p, 15.0, "Hole2")),
        ];
        for (case, edit) in edits {
            let mut part = plate(&path);
            let mut s = Session::default();
            let (sketch, extrude) = sketched_on_plate(&mut s, &path);
            edit(&mut part);
            run(&mut part, "file.save", json!({}));
            let update = run(&mut s, "component.update_links", json!({}));
            assert_eq!(update["updated"], json!(["Plate"]), "{case}: {update}");
            assert_eq!(feature_error(&s, sketch), None, "{case}: the sketch lost its face");
            assert_eq!(feature_error(&s, extrude), None, "{case}: the extrude failed");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// When the face a sketch is on is gone from the linked part, the sketch still fails: it is
    /// not moved to another face.
    #[test]
    fn a_sketch_whose_linked_face_is_gone_still_fails() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-sketch-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let mut part = plate(&path);
        let mut s = Session::default();
        let (sketch, _) = sketched_on_plate(&mut s, &path);
        run(&mut part, "timeline.delete", json!({"feature": "Hole"}));
        let plate_box = part.doc.features.first().map(|f| f.id).unwrap();
        run(&mut part, "timeline.delete", json!({"feature": plate_box}));
        run(&mut part, "solid.box", json!({"length": 60, "width": 40, "height": 8, "corner": [-30, -20, 0], "body_name": "Plate"}));
        run(&mut part, "file.save", json!({}));
        run(&mut s, "component.update_links", json!({}));
        assert!(feature_error(&s, sketch).is_some(), "a sketch on a face the part no longer has must fail");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #3330: when the part no longer has the face a joint is on, Update Links and the joint list
    /// say so instead of moving the joint to another hole of the same size.
    #[test]
    fn a_joint_whose_linked_face_is_gone_is_reported_not_moved() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let mut part = plate(&path);
        let mut s = pinned(&path, false);
        hole(&mut part, -10.0, "Hole0");
        run(&mut part, "timeline.delete", json!({"feature": "Hole"}));
        run(&mut part, "file.save", json!({}));
        let update = run(&mut s, "component.update_links", json!({}));
        let warnings = update["warnings"].to_string();
        assert!(warnings.contains("joint PinInHole") && warnings.contains("no longer exists"), "{update}");
        let list = run(&mut s, "joint.list", json!({}));
        assert_eq!(list["errors"][0]["name"], "PinInHole", "{list}");
        assert!(list["joints"][0]["frame_a"].is_null(), "{list}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_links_keep_resolving_joint_faces_and_report_a_removed_hole() {
        let dir = std::env::temp_dir().join(format!("solidcraft-step-link-joints-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut part = plate(&dir.join("plate.sc"));
        let path = dir.join("plate.step");
        run(&mut part, "file.export", json!({"path": path}));
        let mut s = Session::default();
        run(&mut s, "file.insert_step", json!({"path": path, "name": "Plate"}));
        run(&mut s, "occurrence.ground", json!({"occurrence": "Plate:1", "grounded": true}));
        run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20, "base": [0, 50, 0], "operation": "new", "body_name": "PinBody"}));
        run(&mut s, "component.create", json!({"name": "Pin", "parent": "root", "activate": false}));
        run(&mut s, "component.move_bodies", json!({"bodies": ["PinBody"], "component": "Pin"}));
        run(
            &mut s,
            "joint.create",
            json!({"type": "rigid", "name": "PinInHole", "flip": true,
            "a": {"occurrence": "Plate:1", "circle": [13, 0, 5]}, "b": {"occurrence": "Pin:1", "circle": [3, 50, 0]}}),
        );
        let joint = s.doc.assembly.joints.iter().find(|j| j.name == "PinInHole").unwrap().id;
        assert_joint_at(&mut s, 10.0, "inserted STEP");
        run(&mut part, "parameters.change", json!({"name": "hx", "expression": "20 mm"}));
        run(&mut part, "file.export", json!({"path": path}));
        let reply = run(&mut s, "component.update_links", json!({}));
        assert!(reply["warnings"].to_string().contains("PinInHole"), "positional STEP faces need a reminder: {reply}");
        assert_joint_at(&mut s, 20.0, "updated STEP");
        assert!(s.doc.assembly.joints.iter().any(|j| j.id == joint));
        run(&mut part, "timeline.delete", json!({"feature": "Hole"}));
        run(&mut part, "file.export", json!({"path": path}));
        let reply = run(&mut s, "component.update_links", json!({}));
        let list = run(&mut s, "joint.list", json!({}));
        assert!(reply["warnings"].to_string().contains("PinInHole"), "{reply}");
        assert_eq!(list["errors"][0]["name"], "PinInHole", "{list}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Two arms jointed top face to bottom face, both occurrences of one linked arm design: when
    /// the arm is made thicker, its holes moved or it is made longer, Update Links brings the
    /// change in and every joint type re-solves onto the moved faces, edges and corners, as it
    /// does inside one design (`jointed_arms_stay_together_as_the_arm_changes`).
    #[test]
    fn jointed_linked_arms_stay_together_through_update_links() {
        use crate::cmd::joints::face_tests::{ARM_EDITS, ARM_FORMS, arm_pairs, arm_part, arm_size, arms_apart, joint_arms};
        let dir = std::env::temp_dir().join(format!("solidcraft-link-arms-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("arm.solidcraft");
        let mut apart = Vec::new();
        for form in ARM_FORMS {
            let mut part = Session::default();
            arm_part(&mut part);
            run(&mut part, "file.save", json!({"path": path}));
            let mut s = Session::default();
            let first = run(&mut s, "component.insert_design", json!({"path": path, "name": "Arm"}))["occurrence"].as_u64().unwrap();
            let pairs = arm_pairs(&mut s, first);
            joint_arms(&mut s, form, &pairs);
            apart.extend(arms_apart(&mut s, "as made", form, &pairs, arm_size(&[])));
            for (i, (name, value)) in ARM_EDITS.into_iter().enumerate() {
                run(&mut part, "parameters.change", json!({"name": name, "expression": format!("{value} mm")}));
                run(&mut part, "file.save", json!({}));
                let update = run(&mut s, "component.update_links", json!({}));
                assert_eq!(update["updated"], json!(["Arm"]), "{form} {name}: {update}");
                assert_eq!(update["warnings"], json!([]), "{form} {name}: {update}");
                apart.extend(arms_apart(&mut s, &format!("{name} = {value}"), form, &pairs, arm_size(&ARM_EDITS[..=i])));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(apart.is_empty(), "{} joints apart:\n{}", apart.len(), apart.join("\n"));
    }

    /// A joint made on a link that kept no face names (an older design) names the faces by their
    /// place in the file: Update Links says it can't vouch for it, and keeps the names from then on.
    #[test]
    fn update_links_warns_about_joints_on_faces_named_by_their_place() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-numbered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let mut part = plate(&path);
        let mut s = pinned(&path, true);
        let feature = s.doc.links.values().next().unwrap().feature;
        assert!(s.doc.feature(feature).unwrap().link_faces.is_empty());
        // An unchanged part keeps the bodies; the names come in quietly.
        assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"], json!(["Plate"]));
        assert!(!s.doc.feature(feature).unwrap().link_faces.is_empty());
        run(&mut part, "parameters.change", json!({"name": "hx", "expression": "20 mm"}));
        run(&mut part, "file.save", json!({}));
        let update = run(&mut s, "component.update_links", json!({}));
        assert!(update["warnings"].to_string().contains("joint PinInHole refers to faces of Plate by their place"), "{update}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A design someone sends may name a linked file on another computer (`\\host\share\…`) or a
    /// device. On Windows, reading it or only asking whether it exists connects to that computer
    /// and may send it the user's sign-in. Update Links and List Links judge the stored path from
    /// its text and touch nothing: List Links reports it unknown with a reason, Update Links
    /// refuses with the server's name, and nothing in the design changes.
    #[test]
    fn links_to_network_locations_or_devices_touch_nothing_until_allowed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-network-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        let part = dir.join("parts").join("plate.solidcraft");
        save_box(&part, 30.0);
        let case = dir.join("case.solidcraft");
        let mut a = Session::default();
        let comp = run(&mut a, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        run(&mut a, "file.save", json!({"path": case}));
        let feature = a.doc.links[&comp].feature;
        // On Unix these names are plain files beside the design, so a check that was skipped would
        // find them: the refusal below comes from the text alone.
        let crafted = [r"\\evil.example\share\plate.solidcraft", "//evil.example/share/plate.solidcraft", r"\\?\UNC\evil.example\share\p.solidcraft"];
        if cfg!(unix) {
            std::fs::copy(&part, dir.join(crafted[0])).unwrap();
        }
        let devices = [r"\\.\pipe\plate.solidcraft", r"\\.\COM1", "parts/NUL.solidcraft", "parts/com1.solidcraft", r"parts\CON.part.solidcraft"];
        std::fs::copy(&part, dir.join("parts").join("NUL.solidcraft")).unwrap();
        let before = volume(&mut a);
        for link in crafted.iter().chain(&devices) {
            a.doc_mut().links.insert(comp, DesignLink { path: link.to_string(), feature, ..Default::default() });
            let calls = solidcraft_io::vfs::file_calls();
            let listed = run(&mut a, "component.links", json!({}));
            let entry = &listed["links"][0];
            assert!(entry["found"].is_null() && entry["reason"].is_string(), "{link}: {listed}");
            let err = a.execute("component.update_links", &json!({})).unwrap_err().to_string();
            assert_eq!(solidcraft_io::vfs::file_calls(), calls, "{link}: a file call was made");
            if devices.contains(link) {
                assert!(entry["host"].is_null() && err.contains("names a device"), "{link}: {listed} {err}");
            } else {
                assert_eq!(entry["host"], "evil.example", "{link}: {listed}");
                assert!(err.contains(r"network location \\evil.example"), "{link}: {err}");
            }
            assert_eq!(volume(&mut a), before, "{link}: the design changed");
        }
        // Allowed (Allow once, a preference, …), the same link is read as any other.
        #[cfg(unix)]
        {
            a.doc_mut().links.insert(comp, DesignLink { path: crafted[0].to_string(), feature, ..Default::default() });
            solidcraft_io::vfs::allowing_hosts(&["EVIL.example".to_string()], || {
                let listed = run(&mut a, "component.links", json!({}));
                assert_eq!(listed["links"][0]["found"], true, "{listed}");
                assert_eq!(run(&mut a, "component.update_links", json!({}))["unchanged"], json!(["plate"]));
            });
            assert!(run(&mut a, "component.links", json!({}))["links"][0]["found"].is_null(), "allowed for that action only");
        }
        // A local link, relative or absolute, is found and read as before.
        for local in ["parts/plate.solidcraft".to_string(), part.to_string_lossy().to_string()] {
            a.doc_mut().links.insert(comp, DesignLink { path: local.clone(), feature, ..Default::default() });
            assert_eq!(run(&mut a, "component.links", json!({}))["links"][0]["found"], true, "{local}");
            assert_eq!(run(&mut a, "component.update_links", json!({}))["unchanged"], json!(["plate"]), "{local}");
        }
        // Changing a link to a network path is refused before normalization or file access.
        for command in ["component.relink", "component.replace_with_link"] {
            for path in crafted {
                let calls = solidcraft_io::vfs::file_calls();
                let err = a.execute(command, &json!({"component": comp, "path": path})).unwrap_err().to_string();
                assert!(err.contains("network location"), "{command} {path}: {err}");
                assert_eq!(solidcraft_io::vfs::file_calls(), calls);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every place that reaches a file through a link path goes through the design-path rule
    /// (`vfs::check_design_path` / `read_design_path` / `design_path_is_file` / `link_refusal`):
    /// no plain file call on a link's path in the link commands, a drawing's source path judged by the
    /// rule before `same_file` canonicalises it, and the font file a design's text names is checked
    /// before it is read.
    #[test]
    fn every_link_path_goes_through_the_design_path_rule() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let code = |file: &str| {
            // Windows Git checkouts can use CRLF. Locate the test module in either
            // representation, rather than mistaking its fixture file calls for product code.
            let text = std::fs::read_to_string(root.join(file)).unwrap().replace("\r\n", "\n");
            // The code before its test module.
            text.split("\n#[cfg(test)]\n#[allow(clippy::unwrap_used").next().unwrap_or("").to_string()
        };
        for file in ["src/cmd/component_link.rs", "src/cmd/component_extract.rs"] {
            let text = code(file);
            for raw in [
                "vfs::read(",
                "vfs::read_to_string(",
                "vfs::is_file(",
                "vfs::exists(",
                "vfs::len(",
                "std::fs::read",
                "std::fs::metadata",
                ".is_file()",
                ".exists()",
                ".metadata()",
            ] {
                assert!(!text.contains(raw), "{file}: `{raw}` on a link's path must go through vfs::check_design_path");
            }
            // Relink canonicalises the path it is given, after the rule.
            if let Some(at) = text.find("canonicalize") {
                let rule = text[..at].rfind("link_refused(s.path.as_deref(), given)").unwrap_or(usize::MAX);
                assert!(rule < at, "{file}: canonicalize before check_design_path");
            }
        }
        // A drawing's source path: the rule judges its text before `same_file` canonicalises it (a network touch).
        let drawing = code("src/cmd/drawing.rs");
        let source = &drawing[drawing.find("fn linked_source(").unwrap()..];
        let rule = source.find("check_design_path(given").expect("linked_source goes through the design-path rule");
        assert!(rule < source.find("same_file(").unwrap(), "src/cmd/drawing.rs: same_file before check_design_path");
        let fonts = std::fs::read_to_string(root.join("../sketch/src/fonts.rs")).unwrap();
        let (check, read) = (fonts.find("FILE_CHECK.get()").unwrap(), fonts.find("read_font(&path)").unwrap());
        assert!(check < read, "a font file named by a design is checked before it is read");
        let io = std::fs::read_to_string(root.join("../io/src/vfs.rs")).unwrap();
        assert!(io.contains("set_font_file_check(|p| check_design_path("), "the font check is the design-path rule");
    }

    /// A one-face sheet at z = 0.15 bounded by spline edges lying on the plane z = 0, written
    /// without pcurves: its edges are 0.15 mm off their face and read with tolerances of 0.45 mm.
    /// An importer plugin's design carries its geometry as an Import feature labelled with the file it
    /// came from (`part.FCStd`) holding STEP. Linking it must read that STEP as STEP, as File > Open
    /// does, not hand it to the FreeCAD reader because of the label.
    #[test]
    fn a_design_an_importer_wrote_from_a_freecad_file_links_its_step_geometry() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-importer-fcstd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut part = Session::default();
        run(&mut part, "solid.box", json!({"length": 10, "width": 10, "height": 5, "body_name": "Block"}));
        let step_file = dir.join("block.step");
        run(&mut part, "file.export", json!({"path": step_file}));
        let step = std::fs::read(&step_file).unwrap();
        let mut kind = solidcraft_io::step_import_feature(&step, "block.step").unwrap().kind;
        if let FeatureKind::Import { file, .. } = &mut kind {
            *file = "part.FCStd".into();
        }
        let mut doc = Document::default();
        doc.add_feature(kind, None).unwrap();
        let bytes = solidcraft_io::write_design(&doc).unwrap();
        for name in ["part.FCStd", "part.fcstd", "part.3dm"] {
            let source = Source::File(dir.join(name));
            let ((_, feature, ..), _) = design_bodies(&source, &bytes, None, Default::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(feature.body_names.len(), 1, "{name}");
        }
        assert_eq!(step_label("/a/part.FCStd"), "/a/part.step");
        assert_eq!(step_label("/a/part.sc"), "/a/part.sc");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn loose_sheet_step() -> String {
        let mut t = String::from(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','hand','');\n\
             FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n\
             #10=PLANE('',#13);\n#11=CARTESIAN_POINT('',(0.,0.,0.));\n#12=DIRECTION('',(0.,0.,1.));\n\
             #14=DIRECTION('',(1.,0.,0.));\n#13=AXIS2_PLACEMENT_3D('',#11,#12,#14);\n",
        );
        let corners = [(0, 0), (30, 0), (30, 30), (0, 30)];
        for (k, &(x, y)) in corners.iter().enumerate() {
            let (nx, ny) = corners[(k + 1) % 4];
            t += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,0.15));\n#{}=VERTEX_POINT('',#{});\n", 200 + k, 210 + k, 200 + k);
            for m in 0..4 {
                let f = m as f64 / 3.0;
                let (px, py) = (x as f64 + (nx - x) as f64 * f, y as f64 + (ny - y) as f64 * f);
                t += &format!("#{}=CARTESIAN_POINT('',({px:?},{py:?},0.15));\n", 300 + k * 4 + m);
            }
            let points: Vec<String> = (0..4).map(|m| format!("#{}", 300 + k * 4 + m)).collect();
            t += &format!("#{}=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.);\n", 400 + k, points.join(","));
            t += &format!(
                "#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
                500 + k,
                210 + k,
                210 + (k + 1) % 4,
                400 + k,
                510 + k,
                500 + k
            );
        }
        t += "#520=EDGE_LOOP('',(#510,#511,#512,#513));\n#521=FACE_OUTER_BOUND('',#520,.T.);\n#522=ADVANCED_FACE('',(#521),#10,.T.);\n\
              #530=OPEN_SHELL('',(#522));\n#531=SHELL_BASED_SURFACE_MODEL('Sheet',(#530));\n\
              #532=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#531),#540);\n\
              #540=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#541))GLOBAL_UNIT_ASSIGNED_CONTEXT((#542,#543))REPRESENTATION_CONTEXT('',''));\n\
              #541=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#542,'','');\n#542=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n\
              #543=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\nENDSEC;\nEND-ISO-10303-21;\n";
        t
    }

    /// A 30 × 25 × 20 block with a blind Ø10 bore running diagonally in plan at z = 6, and an
    /// obround slot (8 mm half-length, 4 wide) cut 16 mm towards the bore from a plane beside it
    /// whose axes are tilted 45° about the bore's axis: closed, 15 faces.
    fn bored_and_slotted_block(s: &mut Session) {
        let k = std::f64::consts::FRAC_1_SQRT_2;
        run(s, "solid.box", json!({"length": 30, "width": 25, "height": 20, "name": "Block", "body_name": "Block"}));
        let base = [15.0 - 40.0 * k, 12.5 + 40.0 * k, 6.0];
        run(s, "solid.cylinder", json!({"diameter": 10, "height": 41, "base": base, "axis": [k, -k, 0.0], "operation": "cut", "name": "Bore"}));
        let point = |id: &str, x: f64, y: f64| json!({"fixed": true, "id": id, "pos": [x, y]});
        let plane =
            json!({"type": "custom", "plane": {"origin": [15.0 + 12.0 * k, 12.5 + 8.0 * k, 7.5], "x_dir": [0.5, -0.5, -k], "y_dir": [0.5, -0.5, k]}});
        let feature = json!({
            "id": 100, "name": "SlotSketch", "type": "sketch", "suppressed": false, "plane": plane,
            "sketch": {
                "constraints": [], "counters": {"a": 2, "l": 2, "p": 5},
                "points": [point("origin", 0.0, 0.0), point("p1", -8.0, -2.0), point("p2", 8.0, -2.0), point("p3", 8.0, 2.0), point("a1.center", 8.0, 0.0), point("p5", -8.0, 2.0), point("a2.center", -8.0, 0.0)],
                "curves": [
                    {"a": 1, "b": 2, "construction": false, "id": "l1", "type": "line"},
                    {"a": 2, "b": 3, "c": 4, "construction": false, "id": "a1", "type": "arc"},
                    {"a": 3, "b": 5, "construction": false, "id": "l2", "type": "line"},
                    {"a": 5, "b": 1, "c": 6, "construction": false, "id": "a2", "type": "arc"}
                ]
            }
        });
        run(s, "timeline.add", json!({"feature": feature}));
        let cut = json!({
            "id": 101, "name": "Slot", "type": "extrude", "suppressed": false, "sketch": 100, "operation": "cut", "profiles": {"type": "all"},
            "extent": {"convention": "total_distance", "direction": "positive", "distance": "16 mm"}
        });
        run(s, "timeline.add", json!({"feature": cut}));
    }

    /// Each body of a design measured: (name, kernel faces, closed, volume).
    fn bodies(s: &mut Session) -> Vec<(String, u64, bool, Option<f64>)> {
        let m = run(s, "inspect.measure", json!({}));
        m["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| (b["name"].as_str().unwrap().to_string(), b["kernel"]["faces"].as_u64().unwrap(), b["closed"] == true, b["volume_mm3"].as_f64()))
            .collect()
    }

    /// Insert Design brings a design's bodies in through STEP, and a STEP file stated one
    /// accuracy for all its bodies: the loosest. A closed block beside a sheet whose edges carry
    /// 0.45 mm tolerances was read back at 0.45 mm, and at that tolerance a face of its bore
    /// failed, so the block came back open (14 faces, no volume). Each body is now written with
    /// its own accuracy, and the block comes back as the design holds it.
    #[test]
    fn insert_design_reads_each_body_at_its_own_accuracy_beside_a_loose_sheet() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-accuracy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (sheet, part) = (dir.join("sheet.step"), dir.join("part.solidcraft"));
        std::fs::write(&sheet, loose_sheet_step()).unwrap();
        let mut own = Session::default();
        bored_and_slotted_block(&mut own);
        run(&mut own, "file.insert_step", json!({"path": sheet}));
        run(&mut own, "file.save", json!({"path": part}));
        let held = bodies(&mut own);
        let block = held.iter().find(|b| b.0 == "Block").cloned().unwrap();
        assert!(block.2 && block.1 == 15, "the block the design holds: {held:?}");
        assert!(held.iter().any(|b| b.0 == "Sheet" && !b.2), "the sheet is open: {held:?}");
        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": part}));
        let linked = bodies(&mut s);
        let closed: Vec<_> = linked.iter().filter(|b| b.2).collect();
        assert_eq!(closed.len(), 1, "{linked:?}");
        assert_eq!(closed[0].1, block.1, "{linked:?}");
        let (a, b) = (closed[0].3.unwrap(), block.3.unwrap());
        assert!((a - b).abs() < 1e-6 * b, "volume {a} vs {b}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An ASCII STL of a closed tetrahedron, 10 mm along each axis.
    pub(super) fn tetra_stl() -> &'static str {
        "solid t\n\
         facet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 0 10 0\nvertex 10 0 0\nendloop\nendfacet\n\
         facet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 0 10\nendloop\nendfacet\n\
         facet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 0 0 10\nvertex 0 10 0\nendloop\nendfacet\n\
         facet normal 0 0 0\nouter loop\nvertex 10 0 0\nvertex 0 10 0\nvertex 0 0 10\nendloop\nendfacet\n\
         endsolid t\n"
    }

    /// #20050: Insert Design brings a design's mesh bodies (a KiCad board's parts) with its
    /// solids, in the linked component, and Update Links follows them when the design gains or
    /// loses them, also for a link that holds none yet with the file unchanged.
    #[test]
    fn a_linked_design_brings_its_mesh_bodies_and_update_links_follows_them() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-meshes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (part, stl) = (dir.join("board.solidcraft"), dir.join("Chip.stl"));
        std::fs::write(&stl, tetra_stl()).unwrap();
        let meshes = |s: &Session| -> Vec<String> { s.model.state().bodies.iter().filter(|b| b.body.is_mesh()).map(|b| b.name.clone()).collect() };
        let save_board = |with_mesh: bool| {
            let mut board = Session::default();
            run(&mut board, "solid.box", json!({"length": 10, "width": 20, "height": 2, "body_name": "Board"}));
            if with_mesh {
                run(&mut board, "file.insert_mesh", json!({"path": stl}));
            }
            run(&mut board, "file.save", json!({"path": part}));
            meshes(&board)
        };
        let chip = save_board(true);
        assert_eq!(chip.len(), 1, "{chip:?}");
        // Inserted, the design's mesh body comes in with its solid, in the linked component.
        let mut a = Session::default();
        let inserted = run(&mut a, "component.insert_design", json!({"path": part}));
        assert_eq!(meshes(&a), chip, "{inserted}");
        assert!(inserted["bodies"].as_array().unwrap().contains(&json!(chip[0])), "{inserted}");
        assert!(!inserted["warnings"].to_string().contains("not linked"), "{inserted}");
        let comp = inserted["component"].as_u64().unwrap();
        let st = a.model.state();
        assert!(st.bodies.iter().all(|b| a.doc.model_body_component(b) == comp), "every body is in the linked component");
        assert_eq!(run(&mut a, "component.update_links", json!({}))["unchanged"], json!(["board"]));
        // The design loses its mesh body, then gets it back: Update Links follows both.
        save_board(false);
        assert_eq!(run(&mut a, "component.update_links", json!({}))["updated"], json!(["board"]));
        assert!(meshes(&a).is_empty(), "{:?}", meshes(&a));
        save_board(true);
        assert_eq!(run(&mut a, "component.update_links", json!({}))["updated"], json!(["board"]));
        assert_eq!(meshes(&a), chip);
        // A link that holds no meshes yet (made before they were linked) gets them from the
        // unchanged file: what was read before is no answer for it.
        let held = link_mesh_feature(&a, comp, a.doc.links[&comp].feature).map(|(id, _)| id).unwrap();
        a.doc_mut().delete_feature(held).unwrap();
        a.refresh();
        assert!(meshes(&a).is_empty());
        assert_eq!(run(&mut a, "component.update_links", json!({}))["updated"], json!(["board"]));
        assert_eq!(meshes(&a), chip);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A joint referencing a positional tie-numbered linked face (`F<id>:link:<hash>#<k>`) must
    /// warn with the shift distance when `component.update_links` moves that face, never silently
    /// sliding onto another location.
    #[test]
    fn tie_numbered_linked_face_shift_emits_warning_with_distance() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-tie-warn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let mut part = plate(&path);
        let mut s = pinned(&path, false);
        let (&comp, link) = s.doc.links.iter().next().unwrap();
        let feature = link.feature;

        // Simulate a positional tie-numbered carried face (`#2`) on the linked plate's bore wall.
        let mut carried = s.doc.feature(feature).unwrap().link_faces.clone();
        for body in &mut carried {
            for face in body {
                if face.contains(":side") && !face.contains('#') {
                    face.push_str("#2");
                }
            }
        }
        s.doc_mut().feature_mut(feature).unwrap().link_faces = carried;
        s.refresh();
        let tie_name = {
            let st = s.model.state();
            let b = st.bodies.iter().find(|b| b.feature == feature).unwrap();
            solidcraft_doc::naming::face_names(b)
                .iter()
                .find(|n| solidcraft_doc::naming::is_tie_numbered_link_face(n.as_str(), feature))
                .cloned()
                .unwrap()
        };
        if let solidcraft_doc::joints::Snap::CircleCenter { face, .. } = &mut s.doc_mut().assembly.joints[0].a.snap {
            *face = Some(tie_name);
        }
        s.refresh();

        let before = snapshot_tie_numbered_joints(&s, comp, feature);
        assert_eq!(before.len(), 1);
        assert!(before[0].before.is_some());

        // Move the hole by 30 mm (`hx: 10 mm -> -20 mm`) while keeping the same tie-numbered name.
        run(&mut part, "parameters.change", json!({"name": "hx", "expression": "-20 mm"}));
        run(&mut part, "file.save", json!({"path": path}));
        let update = run(&mut s, "component.update_links", json!({}));
        assert_eq!(update["updated"], json!(["Plate"]), "{update}");
        // Re-apply the tie-numbered carried name on the updated import and check the shift warning.
        let mut after_carried = s.doc.feature(feature).unwrap().link_faces.clone();
        for body in &mut after_carried {
            for face in body {
                if face.contains(":side") && !face.contains('#') {
                    face.push_str("#2");
                }
            }
        }
        s.doc_mut().feature_mut(feature).unwrap().link_faces = after_carried;
        s.refresh();
        let warnings = tie_numbered_joint_warnings(&s, "Plate", &before);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("joint PinInHole") && warnings[0].contains("shifted by 30.00 mm"), "{warnings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A part holding one pin, a cylinder beside the Z axis, saved as `path`.
    fn save_pin(path: &Path) {
        let mut part = Session::default();
        run(&mut part, "solid.cylinder", json!({"base": [20, 0, 0], "radius": 2, "height": 10, "body_name": "Pin"}));
        run(&mut part, "file.save", json!({"path": path}));
    }

    /// Every body the world shows (each placement's), by name, with its volume.
    fn shown_bodies(s: &Session) -> std::collections::BTreeMap<String, f64> {
        let world = s.world_state().unwrap();
        let shown: std::collections::BTreeMap<String, f64> = world.bodies.iter().map(|b| (b.name.clone(), b.volume().unwrap_or(f64::NAN))).collect();
        assert_eq!(shown.len(), world.bodies.len(), "one body per name: {shown:?}");
        shown
    }

    /// Every feature evaluates, and no pattern leaves a body out (which it says in a warning).
    fn assert_every_copy_made(s: &Session, when: &str) {
        for f in &s.doc.features {
            let r = s.model.result(f.id).unwrap_or_else(|| panic!("{when}: `{}` has no result", f.name));
            assert!(r.error.is_none(), "{when}: `{}` failed: {:?}", f.name, r.error);
            if matches!(f.kind, FeatureKind::Pattern { .. }) {
                assert!(r.warning.is_none(), "{when}: `{}` warns: {:?}", f.name, r.warning);
            }
        }
    }

    /// The bodies `shown` holds sum to `count` pins of `pin` each.
    fn assert_pins(shown: &std::collections::BTreeMap<String, f64>, count: usize, pin: f64, when: &str) {
        assert_eq!(shown.len(), count, "{when}: {shown:?}");
        let total: f64 = shown.values().sum();
        let want = count as f64 * pin;
        assert!((total - want).abs() < 1e-5 * want, "{when}: volume {total}, want {count} x {pin}: {shown:?}");
    }

    /// Five pins about Z, then those five twice: the issue's ten pins.
    fn chained_patterns() -> [Value; 2] {
        [json!({"axis": "Z", "count": 5, "angle": "360 deg"}), json!({"axis": "Z", "count": 2, "angle": "36 deg"})]
    }

    /// The pin part linked into a new assembly with `reuse` (placed once, at the origin), then
    /// patterned there by each circular pattern of `patterns` in turn, each copying every body the
    /// world shows: the session, the linked component, the part's file, its folder and one pin's
    /// volume.
    fn patterned_pin(case: &str, patterns: &[Value]) -> (Session, u64, PathBuf, PathBuf, f64) {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-reuse-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("pin.solidcraft");
        save_pin(&part);
        let mut s = Session::default();
        let inserted = run(&mut s, "component.insert_design", json!({"path": part, "reuse": true}));
        let component = inserted["component"].as_u64().unwrap();
        let pin = shown_bodies(&s).get("Pin").copied().unwrap_or_else(|| panic!("no pin: {inserted}"));
        assert!(pin > 0.0, "{pin}");
        for pattern in patterns {
            let mut p = pattern.clone();
            p["bodies"] = json!(shown_bodies(&s).into_keys().collect::<Vec<_>>());
            run(&mut s, "solid.pattern.circular", p);
        }
        assert_every_copy_made(&s, case);
        (s, component, part, dir, pin)
    }

    /// `component.insert_design` of the pin part again with `reuse`, `x` mm along X: the new
    /// placement, of the component the part is already linked to.
    fn place_pin_again(s: &mut Session, part: &Path, component: u64, x: f64) -> u64 {
        let p = json!({"path": part, "reuse": true, "parent": 0, "matrix": [1, 0, 0, x, 0, 1, 0, 0, 0, 0, 1, 0]});
        let placed = run(s, "component.insert_design", p);
        assert_eq!(placed["component"].as_u64(), Some(component), "the link is reused: {placed}");
        placed["occurrence"].as_u64().unwrap()
    }

    /// A pattern of a linked part's body keeps its copies when the part is placed again with
    /// `reuse`, and the new placement shows the part's own body only.
    #[test]
    fn reusing_a_linked_part_keeps_its_pattern_copies() {
        let (mut s, component, part, dir, pin) = patterned_pin("single", &chained_patterns()[..1]);
        let before = shown_bodies(&s);
        assert_pins(&before, 5, pin, "patterned");
        place_pin_again(&mut s, &part, component, 100.0);
        let after = shown_bodies(&s);
        assert_pins(&after, 6, pin, "placed again");
        assert!(before.keys().all(|name| after.contains_key(name)), "{before:?} -> {after:?}");
        assert_every_copy_made(&s, "placed again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The issue: a linked pin patterned five times about Z, then the five pins twice, ten pins.
    /// Placing the part again with `reuse` moves the first pattern's copies out of the part (else
    /// every placement would show them), and the second pattern, which picked them at the part's
    /// placement, no longer found them there: it left four pins out, and their copies, keeping 7
    /// bodies of 11. Now every pin stays, with no error or warning, and the new placement shows
    /// the one pin.
    #[test]
    fn reusing_a_linked_part_keeps_chained_pattern_copies() {
        let (mut s, component, part, dir, pin) = patterned_pin("chained", &chained_patterns());
        let before = shown_bodies(&s);
        assert_pins(&before, 10, pin, "patterned");
        place_pin_again(&mut s, &part, component, 100.0);
        let after = shown_bodies(&s);
        assert_pins(&after, 11, pin, "placed again");
        assert!(before.keys().all(|name| after.contains_key(name)), "{before:?} -> {after:?}");
        assert_every_copy_made(&s, "placed again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Later changes keep the chained copies too: a third placement, moving it, and undoing back
    /// to the part placed once, which shows the ten pins it showed before.
    #[test]
    fn chained_pattern_copies_stay_through_further_placements_and_undo() {
        let (mut s, component, part, dir, pin) = patterned_pin("further", &chained_patterns());
        let once = shown_bodies(&s);
        assert_pins(&once, 10, pin, "patterned");
        place_pin_again(&mut s, &part, component, 100.0);
        let third = place_pin_again(&mut s, &part, component, -100.0);
        assert_pins(&shown_bodies(&s), 12, pin, "placed a third time");
        assert_every_copy_made(&s, "placed a third time");
        run(&mut s, "occurrence.move", json!({"occurrence": third, "translate": [0, 50, 0]}));
        assert_pins(&shown_bodies(&s), 12, pin, "moved");
        assert_every_copy_made(&s, "moved");
        for _ in 0..10 {
            if s.doc.occurrences.len() == 1 {
                break;
            }
            run(&mut s, "edit.undo", json!({}));
        }
        assert_eq!(s.doc.occurrences.len(), 1, "undone back to one placement");
        let undone = shown_bodies(&s);
        assert_pins(&undone, 10, pin, "undone");
        assert!(once.keys().eq(undone.keys()), "{once:?} -> {undone:?}");
        assert_every_copy_made(&s, "undone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deleting or updating an early linked component must not re-evaluate later independent
    /// linked components, either in-session or on the first edit after opening a saved assembly.
    #[test]
    fn deleting_or_updating_early_linked_component_keeps_later_links_cached() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let save_part = |path: &Path, body_name: &str, height: f64| {
            let mut part = Session::default();
            run(&mut part, "solid.box", json!({"length": 10, "width": 20, "height": height, "body_name": body_name}));
            run(&mut part, "file.save", json!({"path": path}));
        };
        let (p_a, p_b, p_c, p_d, asm) =
            (dir.join("PartA.sc"), dir.join("PartB.sc"), dir.join("PartC.sc"), dir.join("PartD.sc"), dir.join("Assembly.sc"));
        save_part(&p_a, "BlockA", 10.0);
        save_part(&p_b, "BlockB", 10.0);
        save_part(&p_c, "BlockC", 10.0);
        save_part(&p_d, "BlockD", 10.0);

        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": p_a}));
        let comp_b = run(&mut s, "component.insert_design", json!({"path": p_b}))["component"].as_u64().unwrap();
        run(&mut s, "component.insert_design", json!({"path": p_c}));
        run(&mut s, "component.insert_design", json!({"path": p_d}));
        run(&mut s, "file.save", json!({"path": asm}));

        // First edit after opening: deleting PartB recomputes zero later linked components.
        let mut opened_del = Session::default();
        run(&mut opened_del, "doc.open", json!({"path": asm}));
        let before = opened_del.model.recomputed;
        run(&mut opened_del, "component.delete", json!({"component": comp_b}));
        assert_eq!(opened_del.model.recomputed - before, 0, "opened delete must not recompute later linked imports");
        assert!((volume(&mut opened_del) - 6000.0).abs() < 1e-6, "{}", volume(&mut opened_del));

        // In-session delete of PartB also recomputes zero later linked components.
        let before = s.model.recomputed;
        run(&mut s, "component.delete", json!({"component": comp_b}));
        assert_eq!(s.model.recomputed - before, 0, "in-session delete must not recompute later linked imports");
        assert!((volume(&mut s) - 6000.0).abs() < 1e-6, "{}", volume(&mut s));

        // Save the 3-part assembly, update PartA on disk, and verify Update Links recomputes only
        // PartA (never PartC or PartD) both in-session and right after opening.
        run(&mut s, "file.save", json!({"path": asm}));
        save_part(&p_a, "BlockA", 25.0);

        let mut opened_upd = Session::default();
        run(&mut opened_upd, "doc.open", json!({"path": asm}));
        let before = opened_upd.model.recomputed;
        assert_eq!(run(&mut opened_upd, "component.update_links", json!({}))["updated"], json!(["PartA"]));
        assert_eq!(opened_upd.model.recomputed - before, 1, "opened update_links must recompute only PartA");
        assert!((volume(&mut opened_upd) - 9000.0).abs() < 1e-6, "{}", volume(&mut opened_upd));

        let before = s.model.recomputed;
        assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["PartA"]));
        assert_eq!(s.model.recomputed - before, 1, "in-session update_links must recompute only PartA");
        assert!((volume(&mut s) - 9000.0).abs() < 1e-6, "{}", volume(&mut s));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn save_material_boxes(path: &Path, items: &[(&str, f64, &str)]) {
        let mut part = Session::default();
        for &(name, x, material) in items {
            run(&mut part, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "body_name": name}));
            if material != solidcraft_doc::DEFAULT_MATERIAL {
                run(&mut part, "material.assign", json!({"bodies": [name], "material": material}));
            }
        }
        run(&mut part, "file.save", json!({"path": path}));
    }

    fn component_materials(s: &Session, component: u64) -> std::collections::BTreeMap<String, String> {
        let state = s.model.state();
        state.bodies.iter().filter(|b| b.component == component).map(|b| (b.name.clone(), s.doc.body_material(&b.name))).collect()
    }

    #[test]
    fn update_links_and_relink_inherit_materials_after_body_count_and_name_rewrite() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-rewrite-mat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("part.sc");

        // Update Links: grow from 3 bodies to 5 with new names at early slots while keeping local overrides.
        save_material_boxes(&part, &[("Plate", 0.0, "Aluminum"), ("Block", 30.0, "Steel"), ("Pin", 60.0, "Brass")]);
        let mut s = Session::default();
        let comp = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Copper"}));
        run(&mut s, "material.assign", json!({"bodies": ["Pin"], "material": "Default"}));

        save_material_boxes(
            &part,
            &[
                ("Bracket", 0.0, "Titanium"),
                ("Block", 30.0, "Oak"),
                ("Spacer", 60.0, "Glass"),
                ("Pin", 90.0, "Aluminum"),
                ("Sleeve", 120.0, "ABS Plastic"),
            ],
        );
        let updated = run(&mut s, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        assert_eq!(
            component_materials(&s, comp),
            std::collections::BTreeMap::from([
                ("Block".into(), "Copper".into()),
                ("Bracket".into(), "Titanium".into()),
                ("Pin".into(), "Default".into()),
                ("Sleeve".into(), "ABS Plastic".into()),
                ("Spacer".into(), "Glass".into()),
            ])
        );
        assert!(!s.doc.materials.contains_key("Plate"), "removed body material entry must be cleaned up");

        // Shrink to 2 brand-new bodies: both inherit their materials instead of staying Default.
        save_material_boxes(&part, &[("Flange", 0.0, "Steel"), ("Collar", 30.0, "Brass")]);
        let updated = run(&mut s, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        assert_eq!(
            component_materials(&s, comp),
            std::collections::BTreeMap::from([("Collar".into(), "Brass".into()), ("Flange".into(), "Steel".into())])
        );

        // Relink to the rewritten file with different body count and new names.
        save_material_boxes(&part, &[("Plate", 0.0, "Aluminum"), ("Block", 30.0, "Steel"), ("Pin", 60.0, "Brass")]);
        let mut relinked = Session::default();
        let comp = run(&mut relinked, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        run(&mut relinked, "material.assign", json!({"bodies": ["Block"], "material": "Copper"}));
        save_material_boxes(&part, &[("Bracket", 0.0, "Titanium"), ("Block", 30.0, "Oak"), ("Spacer", 60.0, "Glass"), ("Sleeve", 90.0, "Steel")]);
        run(&mut relinked, "component.relink", json!({"component": comp, "path": part}));
        assert_eq!(
            component_materials(&relinked, comp),
            std::collections::BTreeMap::from([
                ("Block".into(), "Copper".into()),
                ("Bracket".into(), "Titanium".into()),
                ("Sleeve".into(), "Steel".into()),
                ("Spacer".into(), "Glass".into()),
            ])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_without_linked_bodies_then_update_links_inherits_new_body_materials() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-leftout-mat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (part, assembly) = (dir.join("part.sc"), dir.join("assembly.sc"));

        save_material_boxes(&part, &[("Plate", 0.0, "Aluminum"), ("Block", 30.0, "Steel")]);
        let mut s = Session::default();
        let comp = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Copper"}));
        run(&mut s, "file.save", json!({"path": assembly, "linked_bodies": false}));

        save_material_boxes(&part, &[("Bracket", 0.0, "Titanium"), ("Block", 30.0, "Oak"), ("Spacer", 60.0, "Brass")]);
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": assembly}));
        assert_eq!(run(&mut opened, "component.links", json!({}))["links"][0]["changed"], json!(true));

        let updated = run(&mut opened, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        assert_eq!(
            component_materials(&opened, comp),
            std::collections::BTreeMap::from([
                ("Block".into(), "Copper".into()),
                ("Bracket".into(), "Titanium".into()),
                ("Spacer".into(), "Brass".into()),
            ])
        );
        assert_eq!(run(&mut opened, "component.links", json!({}))["links"][0]["changed"], json!(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn material_only_change_from_default_to_custom_material_updates_linked_component() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-mat-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("part.sc");

        for assign_default_locally in [false, true] {
            save_material_boxes(&part, &[("Block", 0.0, "Default")]);
            let mut s = Session::default();
            let comp = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
            if assign_default_locally {
                // `material.assign` with Default removes the map key; it is still Default, not a divergence.
                run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Default"}));
                assert!(!s.doc.materials.contains_key("Block"));
            }
            assert_eq!(s.doc.body_material("Block"), "Default");
            assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["changed"], json!(false));

            let mut edited = Session::default();
            run(&mut edited, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
            edited.doc_mut().user_materials.push(UserMaterial { name: "CustomMat".into(), density: 3.4, appearance: None, physical_card: None });
            run(&mut edited, "material.assign", json!({"bodies": ["Block"], "material": "CustomMat"}));
            run(&mut edited, "file.save", json!({"path": part}));

            assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["changed"], json!(true));
            let updated = run(&mut s, "component.update_links", json!({}));
            assert_eq!(updated["updated"], json!(["part"]), "assign_default_locally={assign_default_locally}: {updated}");
            assert_eq!(s.doc.body_material("Block"), "CustomMat");
            let props = run(&mut s, "component.properties", json!({"component": comp}));
            assert_eq!(props["bodies"][0]["material"], "CustomMat", "{props}");
            assert!((props["total"]["mass_g"].as_f64().unwrap() - 3.4).abs() < 1e-6, "{props}");
            assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["changed"], json!(false));
            assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"], json!(["part"]));
        }

        // Same-length built-in material swap immediately after `component.links` must invalidate DIGESTS.
        save_material_boxes(&part, &[("Block", 0.0, "Steel")]);
        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": part}));
        assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["changed"], json!(false));
        save_material_boxes(&part, &[("Block", 0.0, "Brass")]);
        assert_eq!(run(&mut s, "component.links", json!({}))["links"][0]["changed"], json!(true));
        let updated = run(&mut s, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        assert_eq!(s.doc.body_material("Block"), "Brass");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn component_delete_clears_body_materials_so_reinsert_inherits_updated_material() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-delete-reinsert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("part.sc");

        save_material_boxes(&part, &[("Block", 0.0, "Default")]);
        let mut s = Session::default();
        let first = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        assert_eq!(s.doc.materials.get("Block").map(String::as_str), Some("Default"));
        s.doc_mut().appearances.bodies.insert("Block".into(), solidcraft_doc::appearance::Look::custom([10, 20, 30], 1.0));

        // Rewrite the child with a custom material and a new appearance.
        let mut updated = Session::default();
        run(&mut updated, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        updated.doc_mut().user_materials.push(UserMaterial { name: "CustomMat".into(), density: 3.4, appearance: None, physical_card: None });
        run(&mut updated, "material.assign", json!({"bodies": ["Block"], "material": "CustomMat"}));
        updated.doc_mut().appearances.bodies.insert("Block".into(), solidcraft_doc::appearance::Look::custom([20, 40, 60], 1.0));
        run(&mut updated, "file.save", json!({"path": part}));

        // Delete the linked component, verify cleanup and undo/redo, then re-insert in the same session.
        run(&mut s, "component.delete", json!({"component": first}));
        assert!(!s.doc.materials.contains_key("Block"));
        assert!(!s.doc.appearances.bodies.contains_key("Block"));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.materials.get("Block").map(String::as_str), Some("Default"));
        assert!(s.doc.appearances.bodies.contains_key("Block"));
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.doc.materials.contains_key("Block"));
        assert!(!s.doc.appearances.bodies.contains_key("Block"));

        let second = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        assert_eq!(s.doc.body_material("Block"), "CustomMat");
        let props = run(&mut s, "component.properties", json!({"component": second}));
        assert_eq!(props["bodies"][0]["material"], "CustomMat", "{props}");
        assert!((props["total"]["mass_g"].as_f64().unwrap() - 3.4).abs() < 1e-6, "{props}");
        let state = s.model.state();
        let body = state.bodies.iter().find(|b| b.component == second).unwrap();
        assert_eq!(s.doc.model_body_look(body).unwrap().color, [20, 40, 60]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn realign_changed_body_count_preserves_survivor_name_when_new_body_collides() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-survivor-collision-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("part.sc");

        save_material_boxes(&part, &[("Plate", 0.0, "Steel")]);
        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": part}));
        run(&mut s, "body.rename", json!({"body": "Plate", "name": "Cover"}));
        run(&mut s, "material.assign", json!({"bodies": ["Cover"], "material": "Copper"}));

        // Child now adds a brand-new body literally named "Cover" (colliding with the survivor's
        // stored assembly name) ahead of surviving "Plate".
        save_material_boxes(&part, &[("Cover", 0.0, "Brass"), ("Plate", 30.0, "Steel")]);
        let updated = run(&mut s, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        // The surviving body keeps its stored assembly name and local Copper override; the new
        // body is disambiguated and inherits Brass.
        assert_eq!(s.doc.body_material("Cover"), "Copper");
        assert_eq!(s.doc.body_material("Cover (2)"), "Brass");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn component_delete_keeps_material_for_surviving_feature_body() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Shared"}));
        run(&mut s, "material.assign", json!({"bodies": ["Shared"], "material": "Brass"}));
        let comp = run(&mut s, "component.create", json!({"name": "Extra", "activate": false}))["component"].as_u64().unwrap();
        // Legacy assignment pointing a surviving root feature's body at the deleted component.
        s.doc_mut().body_components.insert("Shared".into(), comp);
        run(&mut s, "component.delete", json!({"component": comp}));
        assert_eq!(s.doc.body_material("Shared"), "Brass");
    }

    fn save_step_box(path: &Path, height: f64, corner: [f64; 3]) {
        let mut part = Session::default();
        run(&mut part, "solid.box", json!({"length": 10, "width": 10, "height": height, "corner": corner, "body_name": "StepBox"}));
        run(&mut part, "file.export", json!({"path": path}));
    }

    #[test]
    fn update_links_batches_refreshes_and_deduplicates_duplicate_sources() {
        for with_sketch in [false, true] {
            let dir = std::env::temp_dir().join(format!("solidcraft-link-batch-par-{with_sketch}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let plate_path = dir.join("plate.sc");
            let block_path = dir.join("block.sc");
            let step_path = dir.join("bracket.step");

            let mut plate_part = plate(&plate_path);
            save_material_boxes(&block_path, &[("Block", 0.0, "Steel")]);
            save_step_box(&step_path, 10.0, [0.0, 200.0, 0.0]);

            let build_assembly = || -> (Session, Vec<u64>, Option<(u64, u64)>) {
                let mut s = Session::default();
                let mut comps = vec![
                    run(&mut s, "component.insert_design", json!({"path": plate_path, "name": "PlateA"}))["component"].as_u64().unwrap(),
                    run(&mut s, "component.insert_design", json!({"path": plate_path, "name": "PlateB", "translate": [100, 0, 0]}))["component"]
                        .as_u64()
                        .unwrap(),
                    run(&mut s, "component.insert_design", json!({"path": plate_path, "name": "PlateC", "translate": [200, 0, 0]}))["component"]
                        .as_u64()
                        .unwrap(),
                ];
                run(&mut s, "occurrence.ground", json!({"occurrence": "PlateA:1", "grounded": true}));
                run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20, "base": [0, 50, 0], "operation": "new", "body_name": "PinBody"}));
                run(&mut s, "component.create", json!({"name": "Pin", "parent": "root", "activate": false}));
                run(&mut s, "component.move_bodies", json!({"bodies": ["PinBody"], "component": "Pin"}));
                run(
                    &mut s,
                    "joint.create",
                    json!({
                        "type": "rigid",
                        "name": "PinInHole",
                        "flip": true,
                        "a": {"occurrence": "PlateA:1", "circle": [13, 0, 5]},
                        "b": {"occurrence": "Pin:1", "circle": [3, 50, 0]},
                    }),
                );
                let attached = if with_sketch {
                    let sketch =
                        run(&mut s, "sketch.create", json!({"plane": {"face": [-20, 10, 5]}, "name": "OnPlate"}))["sketch"].as_u64().unwrap();
                    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-25, 5], "p1": [-15, 15]}));
                    run(&mut s, "sketch.finish", json!({}));
                    let extrude = run(&mut s, "solid.extrude", json!({"sketch": "OnPlate", "distance": 3}))["feature"].as_u64().unwrap();
                    Some((sketch, extrude))
                } else {
                    None
                };
                comps.push(
                    run(&mut s, "component.insert_design", json!({"path": block_path, "name": "BlockA", "translate": [0, 100, 0]}))["component"]
                        .as_u64()
                        .unwrap(),
                );
                comps.push(
                    run(&mut s, "component.insert_design", json!({"path": block_path, "name": "BlockB", "translate": [100, 100, 0]}))["component"]
                        .as_u64()
                        .unwrap(),
                );
                comps.push(run(&mut s, "file.insert_step", json!({"path": step_path, "name": "StepA"}))["component"].as_u64().unwrap());
                comps.push(run(&mut s, "file.insert_step", json!({"path": step_path, "name": "StepB"}))["component"].as_u64().unwrap());
                (s, comps, attached)
            };

            let (mut batched, comps, attached) = build_assembly();
            let (mut reference, ref_comps, _) = build_assembly();

            // Mutate all 3 distinct sources on disk (linked into 7 components total).
            run(&mut plate_part, "parameters.change", json!({"name": "hx", "expression": "20 mm"}));
            run(&mut plate_part, "file.save", json!({}));
            save_material_boxes(&block_path, &[("Block", 0.0, "Brass"), ("Cap", 25.0, "Aluminum")]);
            save_step_box(&step_path, 25.0, [0.0, 200.0, 0.0]);

            // Update batched assembly in one command and verify refresh count & deduplication.
            EVALUATED.with(|n| n.replace(0));
            STEP_READS.with(|n| n.replace(0));
            UPDATE_LINKS_REFRESHES.with(|n| n.replace(0));
            let batch_reply = run(&mut batched, "component.update_links", json!({}));
            let refresh_runs = UPDATE_LINKS_REFRESHES.with(std::cell::Cell::get);
            assert_eq!(EVALUATED.with(std::cell::Cell::get), 2, "2 unique designs across 5 links (with_sketch={with_sketch})");
            assert_eq!(STEP_READS.with(std::cell::Cell::get), 1, "1 unique STEP file across 2 links (with_sketch={with_sketch})");
            if with_sketch {
                assert!((1..=2).contains(&refresh_runs), "expected 1-2 refreshes with face rebind, got {refresh_runs}");
            } else {
                assert_eq!(refresh_runs, 1, "expected exactly 1 refresh without face rebind, got {refresh_runs}");
            }

            // Update reference assembly sequentially one component at a time and compare bit-for-bit.
            let mut ref_updated = Vec::new();
            let mut ref_warnings = Vec::new();
            for comp in &ref_comps {
                let r = run(&mut reference, "component.update_links", json!({"component": comp}));
                ref_updated.extend(r["updated"].as_array().unwrap().iter().cloned());
                ref_warnings.extend(r["warnings"].as_array().unwrap().iter().cloned());
            }

            assert_eq!(batch_reply["updated"], Value::Array(ref_updated), "with_sketch={with_sketch}: {batch_reply}");
            assert_eq!(batch_reply["unchanged"], json!([]), "with_sketch={with_sketch}: {batch_reply}");
            assert_eq!(batch_reply["failed"], json!([]), "with_sketch={with_sketch}: {batch_reply}");
            assert_eq!(batch_reply["warnings"], Value::Array(ref_warnings), "with_sketch={with_sketch}: {batch_reply}");
            assert_joint_at(&mut batched, 20.0, "batched updated joint");
            assert_joint_at(&mut reference, 20.0, "reference updated joint");
            if let Some((sketch, extrude)) = attached {
                assert_eq!(feature_error(&batched, sketch), None, "rebound sketch must succeed");
                assert_eq!(feature_error(&batched, extrude), None, "extrude on rebound sketch must succeed");
            }

            let batched_bodies = bodies(&mut batched);
            let ref_bodies = bodies(&mut reference);
            assert_eq!(batched_bodies.len(), ref_bodies.len());
            for (b, r) in batched_bodies.iter().zip(&ref_bodies) {
                assert_eq!((&b.0, b.1, b.2), (&r.0, r.1, r.2), "body mismatch: {b:?} vs {r:?}");
                match (b.3, r.3) {
                    (Some(bv), Some(rv)) => assert!((bv - rv).abs() < 1e-6, "volume mismatch for {}: {bv} vs {rv}", b.0),
                    (None, None) => {}
                    other => panic!("volume presence mismatch for {}: {other:?}", b.0),
                }
            }
            for (&bc, &rc) in comps.iter().zip(&ref_comps) {
                assert_eq!(component_materials(&batched, bc), component_materials(&reference, rc));
            }

            // Re-running with no further changes performs zero evaluations, zero STEP parses, and zero refreshes.
            EVALUATED.with(|n| n.replace(0));
            STEP_READS.with(|n| n.replace(0));
            UPDATE_LINKS_REFRESHES.with(|n| n.replace(0));
            let evals_before = batched.evaluation_count();
            let noop = run(&mut batched, "component.update_links", json!({}));
            assert_eq!(noop["updated"], json!([]), "{noop}");
            assert_eq!(noop["unchanged"].as_array().map(Vec::len), Some(7), "{noop}");
            assert_eq!(EVALUATED.with(std::cell::Cell::get), 0);
            assert_eq!(STEP_READS.with(std::cell::Cell::get), 0);
            assert_eq!(UPDATE_LINKS_REFRESHES.with(std::cell::Cell::get), 0);
            assert_eq!(batched.evaluation_count(), evals_before);

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn read_left_out_bodies_deduplicates_duplicate_step_and_design_links() {
        let dir = std::env::temp_dir().join(format!("solidcraft-left-out-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part_a = dir.join("part_a.json");
        let part_b = dir.join("part_b.json");
        let step = dir.join("gear.step");
        let asm = dir.join("assembly.json");

        save_box(&part_a, 30.0);
        save_box(&part_b, 15.0);
        save_step_box(&step, 10.0, [200.0, 0.0, 0.0]);

        let mut s = Session::default();
        for (idx, x) in [0, 50, 100].into_iter().enumerate() {
            run(&mut s, "component.insert_design", json!({"path": part_a, "name": format!("PartA{idx}"), "translate": [x, 0, 0]}));
        }
        for (idx, x) in [0, 50].into_iter().enumerate() {
            run(&mut s, "component.insert_design", json!({"path": part_b, "name": format!("PartB{idx}"), "translate": [x, 60, 0]}));
        }
        for idx in 0..3 {
            run(&mut s, "file.insert_step", json!({"path": step, "name": format!("Gear{idx}")}));
        }
        run(&mut s, "file.save", json!({"path": asm, "linked_bodies": false}));

        EVALUATED.with(|n| n.replace(0));
        STEP_READS.with(|n| n.replace(0));
        let mut opened = Session::default();
        let reply = run(&mut opened, "doc.open", json!({"path": asm}));
        assert_eq!(reply["errors"], 0, "{reply}");
        assert_eq!(EVALUATED.with(std::cell::Cell::get), 2, "5 design links share 2 unique files");
        assert_eq!(STEP_READS.with(std::cell::Cell::get), 1, "3 STEP links share 1 unique file");
        assert_eq!(opened.model.state().bodies.len(), 8);
        // 3 * (10*20*30) + 2 * (10*20*15) + 3 * (10*10*10) = 18_000 + 6_000 + 3_000 = 27_000.
        assert!((volume(&mut opened) - 27_000.0).abs() < 1e-5, "{}", volume(&mut opened));

        // Modify 1 shared design and the shared STEP file while closed: each is still read once.
        save_box(&part_a, 40.0);
        save_step_box(&step, 20.0, [200.0, 0.0, 0.0]);
        EVALUATED.with(|n| n.replace(0));
        STEP_READS.with(|n| n.replace(0));
        let mut reopened = Session::default();
        let reply = run(&mut reopened, "doc.open", json!({"path": asm}));
        assert_eq!(reply["errors"], 0, "{reply}");
        assert_eq!(EVALUATED.with(std::cell::Cell::get), 2);
        assert_eq!(STEP_READS.with(std::cell::Cell::get), 1);
        // 3 * 8_000 + 2 * 3_000 + 3 * 2_000 = 36_000.
        assert!((volume(&mut reopened) - 36_000.0).abs() < 1e-5, "{}", volume(&mut reopened));
        let changed_warnings = reply["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w.as_str().is_some_and(|text| text.contains("changed since this design was saved")))
            .count();
        assert_eq!(changed_warnings, 6, "{reply}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_links_deduplicates_and_matches_exact_status_for_shared_and_distinct_files() {
        let dir = std::env::temp_dir().join(format!("solidcraft-list-links-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part_a = dir.join("part_a.sc");
        let part_b = dir.join("part_b.sc");
        let step = dir.join("gear.step");
        let gone = dir.join("gone.sc");

        save_box(&part_a, 10.0);
        save_box(&part_b, 20.0);
        save_step_box(&step, 10.0, [200.0, 0.0, 0.0]);
        save_box(&gone, 15.0);

        let mut s = Session::default();
        for name in ["A1", "A2", "A3"] {
            run(&mut s, "component.insert_design", json!({"path": part_a, "name": name}));
        }
        for name in ["B1", "B2"] {
            run(&mut s, "component.insert_design", json!({"path": part_b, "name": name}));
        }
        for name in ["S1", "S2"] {
            run(&mut s, "file.insert_step", json!({"path": step, "name": name}));
        }
        for name in ["G1", "G2"] {
            run(&mut s, "component.insert_design", json!({"path": gone, "name": name}));
        }

        // Modify part_a and gear.step, remove gone.sc, and clear digest cache so hashing actually runs.
        save_box(&part_a, 45.0);
        save_step_box(&step, 20.0, [200.0, 0.0, 0.0]);
        std::fs::remove_file(&gone).unwrap();
        forget_file_digests();

        FILE_HASH_COMPUTED.with(|n| n.replace(0));
        let listed = run(&mut s, "component.links", json!({}));
        assert_eq!(FILE_HASH_COMPUTED.with(std::cell::Cell::get), 3, "only 3 unique existing files should be hashed: {listed}");
        let rows = listed["links"].as_array().unwrap();
        assert_eq!(rows.len(), 9, "{listed}");
        let expected = [
            ("A1", json!(true), json!(true), Value::Null),
            ("A2", json!(true), json!(true), Value::Null),
            ("A3", json!(true), json!(true), Value::Null),
            ("B1", json!(true), json!(false), Value::Null),
            ("B2", json!(true), json!(false), Value::Null),
            ("S1", json!(true), json!(true), Value::Null),
            ("S2", json!(true), json!(true), Value::Null),
            ("G1", json!(false), Value::Null, json!("missing")),
            ("G2", json!(false), Value::Null, json!("missing")),
        ];
        for (row, (name, found, changed, state)) in rows.iter().zip(expected) {
            assert_eq!(
                (row["name"].as_str(), &row["found"], &row["changed"], &row["state"]),
                (Some(name), &found, &changed, &state),
                "link row mismatch for {name}: {row}"
            );
        }

        // Second listing with warm cache performs zero uncached hashes and matches identically.
        FILE_HASH_COMPUTED.with(|n| n.replace(0));
        let cached = run(&mut s, "component.links", json!({}));
        assert_eq!(FILE_HASH_COMPUTED.with(std::cell::Cell::get), 0);
        assert_eq!(cached, listed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parallel_left_out_open_and_update_links_are_deterministic_and_deduplicate_shared_parts() {
        let dir = std::env::temp_dir().join(format!("solidcraft-par-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let shared = dir.join("shared.sc");
        save_box(&shared, 10.0);

        // Create 4 distinct sub-assemblies saved without linked bodies, each linking `shared.sc`
        // plus its own unique box, forming a 4-way diamond dependency tree.
        let mut subs = Vec::new();
        for k in 0..4 {
            let sub_path = dir.join(format!("sub_{k}.sc"));
            let mut sub = Session::default();
            run(&mut sub, "solid.box", json!({"length": 10 + k * 2, "width": 10, "height": 10, "body_name": format!("SubBox{k}")}));
            run(&mut sub, "component.insert_design", json!({"path": shared, "name": "Shared", "translate": [30, 0, 0]}));
            run(&mut sub, "file.save", json!({"path": sub_path, "linked_bodies": false}));
            subs.push(sub_path);
        }

        // Top-level assembly linking all 4 sub-assemblies (plus a duplicate link to sub_0), saved
        // with `linked_bodies: false`.
        let top_path = dir.join("top.sc");
        let mut top = Session::default();
        for (k, sub) in subs.iter().enumerate() {
            run(&mut top, "component.insert_design", json!({"path": sub, "name": format!("Sub{k}"), "translate": [0, k * 60, 0]}));
        }
        run(&mut top, "component.insert_design", json!({"path": &subs[0], "name": "Sub0Dup", "translate": [0, 300, 0]}));
        run(&mut top, "file.save", json!({"path": top_path, "linked_bodies": false}));

        // Opening the top assembly in parallel must evaluate each distinct sub-assembly once and
        // the shared diamond part once (4 subs + 1 shared = 5), matching an inline reference build.
        let reset_evals = || EVALUATED.with(|n| n.replace(0));
        reset_evals();
        let mut opened = Session::default();
        let open_reply = run(&mut opened, "doc.open", json!({"path": top_path}));
        assert_eq!(open_reply["errors"], 0, "{open_reply}");
        assert_eq!(reset_evals(), 5, "4 distinct sub-assemblies + 1 shared diamond part must be evaluated exactly once");

        let snapshot = |s: &Session| -> Vec<(String, u64, u64, Vec<String>)> {
            s.model
                .state()
                .bodies
                .iter()
                .map(|b| {
                    let m = solidcraft_kernel::measure(&b.body).unwrap();
                    (b.name.clone(), m.volume.to_bits(), m.area.to_bits(), (*solidcraft_doc::naming::face_names(b)).clone())
                })
                .collect()
        };
        assert_eq!(snapshot(&opened), snapshot(&top), "parallel left-out open must match reference bit-for-bit");

        // Now modify 3 of the 4 leaf part designs in a direct multi-link assembly and verify
        // parallel `component.update_links` updates them deterministically, deduplicates shared
        // links, and reports failures in exact link order.
        let mut leaves = Vec::new();
        for k in 0..4 {
            let p = dir.join(format!("leaf_{k}.sc"));
            save_box(&p, 10.0 + k as f64 * 5.0);
            leaves.push(p);
        }
        let mut multi = Session::default();
        for (k, leaf) in leaves.iter().enumerate() {
            run(&mut multi, "component.insert_design", json!({"path": leaf, "name": format!("Leaf{k}"), "translate": [k * 50, 0, 0]}));
        }
        // Second component pointing at leaf_0 (must be deduplicated during update_links).
        run(&mut multi, "component.insert_design", json!({"path": &leaves[0], "name": "Leaf0Copy", "translate": [250, 0, 0]}));

        // Edit leaf_0, leaf_1, and leaf_2; leave leaf_3 unchanged.
        save_box(&leaves[0], 20.0);
        save_box(&leaves[1], 25.0);
        save_box(&leaves[2], 30.0);

        reset_evals();
        let upd = run(&mut multi, "component.update_links", json!({}));
        assert_eq!(upd["updated"], json!(["Leaf0", "Leaf1", "Leaf2", "Leaf0Copy"]), "{upd}");
        assert_eq!(upd["unchanged"], json!(["Leaf3"]), "{upd}");
        assert_eq!(reset_evals(), 3, "3 distinct edited leaves must be evaluated once each (Leaf0Copy deduplicated)");

        // Expected total volume: 20*20*10 + 25*20*10 + 30*20*10 + 25*20*10 + 20*20*10 (wait: save_box is L x 20 x 10).
        let mut seq = Session::default();
        for (k, leaf) in leaves.iter().enumerate() {
            run(&mut seq, "component.insert_design", json!({"path": leaf, "name": format!("Leaf{k}"), "translate": [k * 50, 0, 0]}));
        }
        run(&mut seq, "component.insert_design", json!({"path": &leaves[0], "name": "Leaf0Copy", "translate": [250, 0, 0]}));
        assert!((volume(&mut multi) - volume(&mut seq)).abs() < 1e-6, "multi={} seq={}", volume(&mut multi), volume(&mut seq));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two sub-assemblies in `dir/subs`, saved without linked bodies, each linking its own part
    /// in `dir/leaves` (`length` sizes the sub-assembly's own box).
    fn save_scoped_subs(dir: &Path, length: f64) -> Vec<std::path::PathBuf> {
        std::fs::create_dir_all(dir.join("subs")).unwrap();
        std::fs::create_dir_all(dir.join("leaves")).unwrap();
        (0..2)
            .map(|k| {
                let leaf = dir.join(format!("leaves/leaf_{k}.sc"));
                if !leaf.exists() {
                    save_box(&leaf, 10.0);
                }
                let mut sub = Session::default();
                run(&mut sub, "solid.box", json!({"length": length, "width": 10, "height": 10, "body_name": format!("SubBox{k}")}));
                run(&mut sub, "component.insert_design", json!({"path": leaf, "name": "Leaf", "translate": [30, 0, 0]}));
                let path = dir.join(format!("subs/sub_{k}.sc"));
                run(&mut sub, "file.save", json!({"path": path, "linked_bodies": false}));
                path
            })
            .collect()
    }

    #[test]
    fn opening_without_linked_bodies_in_a_scope_reads_no_nested_link_outside_it() {
        let dir = std::env::temp_dir().join(format!("solidcraft-par-links-scope-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let subs = save_scoped_subs(&dir, 10.0);
        let mut top = Session::default();
        for (k, sub) in subs.iter().enumerate() {
            run(&mut top, "component.insert_design", json!({"path": sub, "name": format!("Sub{k}"), "translate": [0, k * 60, 0]}));
        }
        let top_path = dir.join("top.sc");
        run(&mut top, "file.save", json!({"path": top_path, "linked_bodies": false}));
        let bytes = std::fs::read(&top_path).unwrap();
        let path = top_path.to_string_lossy().into_owned();
        let open = || {
            let doc = solidcraft_io::read_design(&bytes).unwrap();
            Session::opened_at_validating(doc, None, Some(&path), Session::default().validation())
        };
        // The two sub-assemblies are read on worker threads, each reading its part there.
        assert_eq!(open().model.state().bodies.len(), 4);
        let scope = solidcraft_io::vfs::PathScope::new([dir.join("subs")]);
        let names: Vec<String> = solidcraft_io::vfs::scoped(Some(scope), open).model.state().bodies.iter().map(|b| b.name.clone()).collect();
        assert!(!names.iter().any(|n| n.starts_with("Plate")), "the parts outside the scope must not be read: {names:?}");
        // An empty scope refuses both sub-assemblies.
        assert_eq!(solidcraft_io::vfs::scoped(Some(Default::default()), open).model.state().bodies.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_links_in_a_scope_reads_no_nested_link_outside_it() {
        let dir = std::env::temp_dir().join(format!("solidcraft-par-links-scope-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let subs = save_scoped_subs(&dir, 10.0);
        let assembly = || {
            let mut s = Session::default();
            for (k, sub) in subs.iter().enumerate() {
                run(&mut s, "component.insert_design", json!({"path": sub, "name": format!("Sub{k}"), "translate": [0, k * 60, 0]}));
            }
            s
        };
        let (mut free, mut confined) = (assembly(), assembly());
        assert_eq!(free.model.state().bodies.len(), 4);
        save_scoped_subs(&dir, 20.0);
        // Both changed sub-assemblies are read again on worker threads, each reading its part.
        assert_eq!(run(&mut free, "component.update_links", json!({}))["updated"], json!(["Sub0", "Sub1"]));
        assert_eq!(free.model.state().bodies.len(), 4);
        let scope = solidcraft_io::vfs::PathScope::new([dir.join("subs")]);
        let refused = solidcraft_io::vfs::scoped(Some(scope), || confined.execute("component.update_links", &json!({})));
        let err = refused.expect_err("the parts outside the scope must not be read").to_string();
        assert!(err.contains("leaf_0.sc"), "{err}");
        assert_eq!(confined.model.state().bodies.len(), 4, "a refused update keeps what the design had");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn relink_nested_subassembly_preserves_materials_across_two_levels() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-nested-relink-mat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let p1 = dir.join("p1.sc");
        let p2 = dir.join("p2.sc");
        let sa1 = dir.join("sa1.sc");
        let sa1_v2 = dir.join("sa1_v2.sc");
        let sa0 = dir.join("sa0.sc");
        let sa0_v2 = dir.join("sa0_v2.sc");

        save_material_boxes(&p1, &[("InnerPart", 0.0, "Aluminum")]);
        save_material_boxes(&p2, &[("InnerA", 0.0, "Steel"), ("InnerB", 20.0, "Copper")]);

        let mut sa1_doc = Session::default();
        let p1_comp = run(&mut sa1_doc, "component.insert_design", json!({"path": p1}))["component"].as_u64().unwrap();
        run(&mut sa1_doc, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "SubFrame"}));
        run(&mut sa1_doc, "material.assign", json!({"bodies": ["SubFrame"], "material": "Brass"}));
        run(&mut sa1_doc, "file.save", json!({"path": sa1}));

        run(&mut sa1_doc, "component.relink", json!({"component": p1_comp, "path": p2}));
        run(&mut sa1_doc, "file.save_as", json!({"path": sa1_v2}));
        assert!(
            sa1_doc.model.state().bodies.iter().all(|b| sa1_doc.doc.body_material(&b.name) != solidcraft_doc::DEFAULT_MATERIAL),
            "sa1_v2 must have no Default materials"
        );

        let mut sa0_doc = Session::default();
        let sa1_comp = run(&mut sa0_doc, "component.insert_design", json!({"path": sa1}))["component"].as_u64().unwrap();
        run(&mut sa0_doc, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "body_name": "Housing"}));
        run(&mut sa0_doc, "material.assign", json!({"bodies": ["Housing"], "material": "Titanium"}));
        run(&mut sa0_doc, "file.save", json!({"path": sa0}));

        run(&mut sa0_doc, "component.relink", json!({"component": sa1_comp, "path": sa1_v2}));
        run(&mut sa0_doc, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [80, 0, 0], "body_name": "AddedPart"}));
        run(&mut sa0_doc, "material.assign", json!({"bodies": ["AddedPart"], "material": "Oak"}));
        run(&mut sa0_doc, "file.save_as", json!({"path": sa0_v2}));
        assert!(
            sa0_doc.model.state().bodies.iter().all(|b| sa0_doc.doc.body_material(&b.name) != solidcraft_doc::DEFAULT_MATERIAL),
            "sa0_v2 must have no Default materials"
        );

        let mut top = Session::default();
        let sa0_comp = run(&mut top, "component.insert_design", json!({"path": sa0}))["component"].as_u64().unwrap();
        // Local override on a surviving body must survive the nested relink.
        run(&mut top, "material.assign", json!({"bodies": ["Housing"], "material": "Glass"}));

        run(&mut top, "component.relink", json!({"component": sa0_comp, "path": sa0_v2}));
        let expected = std::collections::BTreeMap::from([
            ("AddedPart".into(), "Oak".into()),
            ("Housing".into(), "Glass".into()),
            ("InnerA".into(), "Steel".into()),
            ("InnerB".into(), "Copper".into()),
            ("SubFrame".into(), "Brass".into()),
        ]);
        assert_eq!(component_materials(&top, sa0_comp), expected);
        assert!(!top.doc.materials.contains_key("InnerPart"), "removed grandchild body material must be pruned");

        let props = run(&mut top, "component.properties", json!({"component": sa0_comp}));
        let expected_mass: f64 = expected.values().map(|m| solidcraft_doc::material_density(m).unwrap()).sum();
        assert!(
            props["bodies"].as_array().unwrap().iter().all(|b| b["material"] != solidcraft_doc::DEFAULT_MATERIAL),
            "top assembly must have zero Default materials after nested relink: {props}"
        );
        assert!((props["total"]["mass_g"].as_f64().unwrap() - expected_mass).abs() < 1e-6, "mass mismatch: {props}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn relink_nested_subassembly_preserves_materials_with_disambiguated_and_duplicate_names() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-nested-disambig-mat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("v1")).unwrap();
        std::fs::create_dir_all(dir.join("v2")).unwrap();

        // Case 1: 2-level nested relink where a surviving body was disambiguated in Top ("Plate (2)")
        // because Top already owns "Plate", and the new grandchild introduces a body named "Plate (2)"
        // ahead of surviving "Plate".
        let p1 = dir.join("p1.sc");
        let p2 = dir.join("p2.sc");
        let sa1 = dir.join("sa1.sc");
        let sa1_v2 = dir.join("sa1_v2.sc");
        let sa0 = dir.join("sa0.sc");
        let sa0_v2 = dir.join("sa0_v2.sc");

        save_material_boxes(&p1, &[("Plate", 0.0, "Aluminum")]);
        save_material_boxes(&p2, &[("Plate (2)", 0.0, "Copper"), ("Plate", 20.0, "Aluminum")]);

        let mut sa1_doc = Session::default();
        let p1_comp = run(&mut sa1_doc, "component.insert_design", json!({"path": p1}))["component"].as_u64().unwrap();
        run(&mut sa1_doc, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "Base"}));
        run(&mut sa1_doc, "material.assign", json!({"bodies": ["Base"], "material": "Steel"}));
        run(&mut sa1_doc, "file.save", json!({"path": sa1}));

        run(&mut sa1_doc, "component.relink", json!({"component": p1_comp, "path": p2}));
        run(&mut sa1_doc, "file.save_as", json!({"path": sa1_v2}));

        let mut sa0_doc = Session::default();
        let sa1_comp = run(&mut sa0_doc, "component.insert_design", json!({"path": sa1}))["component"].as_u64().unwrap();
        run(&mut sa0_doc, "file.save", json!({"path": sa0}));

        run(&mut sa0_doc, "component.relink", json!({"component": sa1_comp, "path": sa1_v2}));
        run(&mut sa0_doc, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "body_name": "Extra"}));
        run(&mut sa0_doc, "material.assign", json!({"bodies": ["Extra"], "material": "Titanium"}));
        run(&mut sa0_doc, "file.save_as", json!({"path": sa0_v2}));

        let mut top = Session::default();
        run(&mut top, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-40, 0, 0], "body_name": "Plate"}));
        run(&mut top, "material.assign", json!({"bodies": ["Plate"], "material": "Brass"}));
        let sa0_comp = run(&mut top, "component.insert_design", json!({"path": sa0}))["component"].as_u64().unwrap();
        assert_eq!(
            component_materials(&top, sa0_comp),
            std::collections::BTreeMap::from([("Base".into(), "Steel".into()), ("Plate (2)".into(), "Aluminum".into())])
        );

        run(&mut top, "component.relink", json!({"component": sa0_comp, "path": sa0_v2}));
        assert_eq!(top.doc.body_material("Plate"), "Brass");
        let mats = component_materials(&top, sa0_comp);
        assert_eq!(
            mats,
            std::collections::BTreeMap::from([
                ("Base".into(), "Steel".into()),
                ("Extra".into(), "Titanium".into()),
                ("Plate (2)".into(), "Aluminum".into()),
                ("Plate (2) (2)".into(), "Copper".into()),
            ])
        );
        let props = run(&mut top, "component.properties", json!({"component": sa0_comp}));
        let expected_mass: f64 = mats.values().map(|m| solidcraft_doc::material_density(m).unwrap()).sum();
        assert!((props["total"]["mass_g"].as_f64().unwrap() - expected_mass).abs() < 1e-6, "{props}");

        // Case 2: duplicate generic body names ("Body1", "Body2" -> "dup", "dup (2)") relinked to a
        // new revision of the same design name with 3 generic bodies in a new material.
        let dup_v1 = dir.join("v1/dup.sc");
        let dup_v2 = dir.join("v2/dup.sc");
        let mut d1 = Session::default();
        run(&mut d1, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
        run(&mut d1, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        run(&mut d1, "material.assign", json!({"bodies": ["Body1", "Body2"], "material": "Aluminum"}));
        run(&mut d1, "file.save", json!({"path": dup_v1}));

        let mut d2 = Session::default();
        run(&mut d2, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
        run(&mut d2, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        run(&mut d2, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
        run(&mut d2, "material.assign", json!({"bodies": ["Body1", "Body2", "Body3"], "material": "Steel"}));
        run(&mut d2, "file.save", json!({"path": dup_v2}));

        let mut asm = Session::default();
        let dup_comp = run(&mut asm, "component.insert_design", json!({"path": dup_v1}))["component"].as_u64().unwrap();
        assert_eq!(
            component_materials(&asm, dup_comp),
            std::collections::BTreeMap::from([("dup".into(), "Aluminum".into()), ("dup (2)".into(), "Aluminum".into())])
        );
        run(&mut asm, "component.relink", json!({"component": dup_comp, "path": dup_v2}));
        assert_eq!(
            component_materials(&asm, dup_comp),
            std::collections::BTreeMap::from([
                ("dup".into(), "Steel".into()),
                ("dup (2)".into(), "Steel".into()),
                ("dup (3)".into(), "Steel".into()),
            ])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_stored_body_names_keep_inherited_baseline_on_open_update_and_relink() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-empty-names-baseline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();

        let part = dir.join("outside/part.sc");
        let part_v2 = dir.join("outside/part_v2.sc");
        let assembly = dir.join("inside/assembly.json");

        save_material_boxes(&part, &[("Plate", 0.0, "Aluminum"), ("Block", 30.0, "Steel")]);
        save_material_boxes(&part_v2, &[("Plate", 0.0, "Titanium"), ("Block", 30.0, "Brass")]);

        let mut s = Session::default();
        let comp = run(&mut s, "component.insert_design", json!({"path": part}))["component"].as_u64().unwrap();
        let feat = s.doc.links[&comp].feature;
        run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Copper"}));
        run(&mut s, "file.save", json!({"path": assembly, "linked_bodies": false}));

        // Simulate a saved document whose link kept its inherited baseline but has empty stored `body_names`.
        let mut disk = solidcraft_io::read_design(&std::fs::read(&assembly).unwrap()).unwrap();
        disk.feature_mut(feat).unwrap().body_names.clear();
        std::fs::write(&assembly, solidcraft_io::write_design(&disk).unwrap()).unwrap();

        // Update source material Aluminum -> Steel while Block is locally overridden to Copper.
        save_material_boxes(&part, &[("Plate", 0.0, "Steel"), ("Block", 30.0, "Oak")]);

        // 1. Opening with `linked_bodies: "all"` runs `read_left_out_bodies` with empty stored `body_names`.
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": assembly}));
        let updated = run(&mut opened, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        assert_eq!(
            component_materials(&opened, comp),
            std::collections::BTreeMap::from([("Block".into(), "Copper".into()), ("Plate".into(), "Steel".into())])
        );

        // 2. Opening with `linked_bodies: "folder"` leaves the outside link unevaluated with empty `body_names`;
        //    `update_links` still preserves the baseline so Plate follows Aluminum -> Steel and Block keeps Copper.
        let mut deferred_update = Session::default();
        run(&mut deferred_update, "doc.open", json!({"path": assembly, "linked_bodies": "folder"}));
        let updated = run(&mut deferred_update, "component.update_links", json!({}));
        assert_eq!(updated["updated"], json!(["part"]), "{updated}");
        assert_eq!(
            component_materials(&deferred_update, comp),
            std::collections::BTreeMap::from([("Block".into(), "Copper".into()), ("Plate".into(), "Steel".into())])
        );

        // 3. Relinking from the same unevaluated empty-`body_names` state also preserves the baseline.
        let mut deferred_relink = Session::default();
        run(&mut deferred_relink, "doc.open", json!({"path": assembly, "linked_bodies": "folder"}));
        run(&mut deferred_relink, "component.relink", json!({"component": comp, "path": part_v2}));
        assert_eq!(
            component_materials(&deferred_relink, comp),
            std::collections::BTreeMap::from([("Block".into(), "Copper".into()), ("Plate".into(), "Titanium".into())])
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// when an updated linked design removes the edge a chamfer in the assembly sits on,
    /// `component.update_links {}` still updates every link at that level, marks the chamfer
    /// failed with a reselect message, reports it in `warnings`, rebuilds the rest of the
    /// assembly, and lets `timeline.edit` reselect the chamfer on the updated body.
    #[test]
    fn update_links_updates_level_when_dependent_chamfer_loses_reference() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-broken-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (part_a, part_c, assembly) = (dir.join("part_a.sc"), dir.join("part_c.sc"), dir.join("assembly.sc"));

        let mut a = Session::default();
        run(&mut a, "parameters.add", json!({"name": "step_d", "expression": "12 mm"}));
        run(&mut a, "solid.cylinder", json!({"diameter": 20, "height": 20, "body_name": "SteppedPin"}));
        run(
            &mut a,
            "solid.cylinder",
            json!({"diameter": "step_d", "height": 10, "base": [0, 0, 20], "operation": "join", "targets": ["SteppedPin"]}),
        );
        run(&mut a, "file.save", json!({"path": &part_a}));

        save_box(&part_c, 10.0);

        let mut b = Session::default();
        run(&mut b, "component.insert_design", json!({"path": &part_a, "name": "Pin"}));
        run(&mut b, "component.insert_design", json!({"path": &part_c, "name": "Spacer", "translate": [40, 0, 0]}));
        let chamfer =
            run(&mut b, "solid.chamfer", json!({"edges": [[6, 0, 20]], "distance": 1, "body": "SteppedPin", "name": "Chamfer3"}))["feature"]
                .as_u64()
                .unwrap();
        let bracket =
            run(&mut b, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [80, 0, 0], "body_name": "Bracket"}))["feature"]
                .as_u64()
                .unwrap();
        let hole = run(
            &mut b,
            "solid.cylinder",
            json!({"diameter": 4, "height": 12, "base": [85, 5, -1], "operation": "cut", "targets": ["Bracket"], "name": "MountHole"}),
        )["feature"]
            .as_u64()
            .unwrap();
        run(&mut b, "file.save", json!({"path": &assembly}));
        assert_eq!(feature_error(&b, chamfer), None);
        assert_eq!(feature_error(&b, bracket), None);
        assert_eq!(feature_error(&b, hole), None);

        // Make the step flush with the Ø20 cylinder so the step edge Chamfer3 picked disappears,
        // and resize the second linked design at the same level.
        run(&mut a, "parameters.change", json!({"name": "step_d", "expression": "20 mm"}));
        run(&mut a, "file.save", json!({}));
        save_box(&part_c, 20.0);

        let update = run(&mut b, "component.update_links", json!({}));
        let updated: Vec<&str> = update["updated"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(updated.contains(&"Pin") && updated.contains(&"Spacer"), "both same-level links must update: {update}");
        assert_eq!(update["failed"], json!([]), "{update}");

        let links = run(&mut b, "component.links", json!({}));
        for entry in links["links"].as_array().unwrap() {
            assert_eq!(entry["changed"], json!(false), "{links}");
        }

        let err = feature_error(&b, chamfer).expect("Chamfer3 must be marked failed when its step edge is gone");
        assert!(
            solidcraft_doc::reselect_parts(&err).is_some() || (err.contains("no longer exists") && err.contains("reselect")),
            "expected clear reselect error on Chamfer3, got: {err}"
        );
        let warnings: Vec<&str> = update["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(
            warnings.iter().any(|w| w.contains("Chamfer3") && (w.contains("reselect") || w.contains("no longer exists"))),
            "update_links warnings must report Chamfer3's lost reference: {update}"
        );
        assert_eq!(feature_error(&b, bracket), None, "other features in B must still rebuild");
        assert_eq!(feature_error(&b, hole), None, "downstream features after Chamfer3 must still rebuild");

        let expected_without_chamfer = std::f64::consts::PI * 100.0 * 30.0 + (10.0 * 20.0 * 20.0) + (1000.0 - std::f64::consts::PI * 4.0 * 10.0);
        let actual = volume(&mut b);
        assert!((actual - expected_without_chamfer).abs() < 1.0, "expected updated bodies volume {expected_without_chamfer}, got {actual}");

        // Reselecting Chamfer3 onto a valid edge of the updated cylinder clears the error and rebuilds it.
        run(&mut b, "timeline.edit", json!({"feature": chamfer, "set": {"edges": [[10, 0, 30]]}}));
        assert_eq!(feature_error(&b, chamfer), None, "reselecting Chamfer3 must clear its error");
        let after_reselect = volume(&mut b);
        assert!(after_reselect < expected_without_chamfer - 1.0, "Chamfer3 must cut material after reselect: {after_reselect}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
