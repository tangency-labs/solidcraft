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
use super::design_link::{LinkState, absolute, inside_design_folder, link_presence, link_target, read_linked_file, resolve, same_file, stored};
use crate::params::{bad, str_, vec3};
use crate::schema::component_link::{
    BreakLinkParams, BreakLinkResult, CopiesParams, CopiesResult, DesignInserted, DesignParametersParams, DesignParametersResult, ExtractParams,
    ExtractResult, InsertDesignParams, InsertDesignResult, JointDiagnostic, LinkFailure, LinkInfo, LinkParametersParams, LinkParametersResult,
    LinksParams, LinksResult, RelinkParams, RelinkResult, ReplaceWithLinkParams, ReplaceWithLinkResult, UpdateLinksParams, UpdateLinksResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

#[path = "component_extract.rs"]
mod extract;

#[path = "component_link_parameters.rs"]
mod parameters;
pub(crate) use parameters::{drive_links, follow_command, forget_kept, mark_links_reconciled, reading_links_in, take_drive_report};

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
const LINK_PARAMETERS_CONTRACT: TypedContract<LinkParametersParams> = TypedContract::new();
const DESIGN_PARAMETERS_CONTRACT: TypedContract<DesignParametersParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.insert_design", "Insert Design", crate::checked!("component.insert_design", insert_design)).begins_edit()
        .at("SOLID", "ASSEMBLE")
        .icon("component")
        .contract(&INSERT_DESIGN_CONTRACT)
        .output(output_schema_for::<InsertDesignResult>)
        .params(
        "path: a .sc design → a new component linked to it, holding its solid bodies as placed in that design, its canvases and decals where the design shows them, and a copy of its design properties; \
         document?: the design itself, as its JSON, for a design open in the session that has no file yet: its bodies come in as they are and the link names no file (not with path); \
         configuration?: a row name or id (null or omitted: the saved source state); name? (default: the design's name); parent?: component id or name (default: the active one); \
         translate?: [x,y,z] in mm from the destination parent's origin (default [0,0,0], also with reuse); \
         axis?: [x,y,z], angle?: radians or \"30 deg\", origin?: [x,y,z]; \
         matrix?: [12 or 16 numbers] (row-major 3x4 or 4x4, its translation column numbers or length expressions) or to_matrix?: [12 or 16 numbers]; \
         tolerance?: largest accepted |RᵀR − I| (default 1e-9, at most 1e-3); \
         reuse?: bool (default false): insert a free occurrence, reusing an existing link to the same file, configuration and parameters when available; \
         parameters?: {name: expression} — user parameters of the design to set, each an expression over this design's parameters; by_name?: bool \
         (default false) — also set each of its user parameters named as one of this design's to it; keep?: [names] — ones by_name leaves at the \
         design's own value (see component.link_parameters)",
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
         joints whose faces are gone or that no longer solve)",
    ),
    CommandSpec::new("component.relink", "Relink Source", crate::checked!("component.relink", relink))
        .icon("update")
        .contract(&RELINK_CONTRACT)
        .output(output_schema_for::<RelinkResult>)
        .params(
        "component: id or name of a linked component; path: a saved design (.sc or .json); configuration?: row name or id, null to follow the saved source state (omitted: keep the selection for the same file, otherwise follow the new file) — replaces the source and linked bodies for all its occurrences, preserving identities and placements; names?: \"keep\" (default) leaves the body names as relinking always did (replaced only when the new source has another number of bodies), \"sync\" takes the new source's body names, renames what other features, materials and appearances keep under the old names with them, and gives the linked feature the source design's name when it still has its component's (component and placement names stay yours; only for the whole component, a placement relinked on its own shows a component made from the new source); occurrence?: one of its placements — when it has others, only that placement comes to show the design, as a placement of the component already linked to it or of a new one under the same parent, keeping its position and joints (component is then the one it shows, previous_component the one it showed); returns path, previous_path, warnings, joint_errors and joint_conflicts",
    ),
    CommandSpec::new("component.break_link", "Break Link", crate::checked!("component.break_link", break_link))
        .icon("break")
        .contract(&BREAK_LINK_CONTRACT)
        .output(output_schema_for::<BreakLinkResult>)
        .params("component: id or name of a linked component — keeps its bodies as they are and forgets the source file"),
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
    CommandSpec::new("component.link_parameters", "Link Parameters", crate::checked!("component.link_parameters", parameters::link_parameters))
        .icon("chain")
        .contract(&LINK_PARAMETERS_CONTRACT)
        .output(output_schema_for::<LinkParametersResult>)
        .params(
        "component: id or name of a linked component; set?: {name: expression or null} — sets user parameters of its design, each an \
         expression over this design's parameters (null gives one back to the design); by_name?: bool — also sets each of its user \
         parameters named as one of this design's user parameters to it (set wins); keep?: [names] — replaces the list of its parameters \
         that by_name leaves at their own value ([] keeps none; a name can't be both kept and in set); the linked bodies are worked out \
         again with them; they are worked out again whenever an expression changes value (a value edit never takes in a newer version of \
         the file: Update Links does); update?: bool (default false) — a file changed since this design read it is taken in with these \
         settings, as Update Links does, in the same step (refused without it); without set, by_name or keep, lists them → component, feature, parameters ({name: {expression, \
         value}}: value in mm or degrees), by_name, keep, geometry_changed, warnings",
    ),
    CommandSpec::new("component.design_parameters", "Design Parameters", crate::checked!("component.design_parameters", parameters::design_parameters))
        .icon("chain")
        .noundo()
        .contract(&DESIGN_PARAMETERS_CONTRACT)
        .output(output_schema_for::<DesignParametersResult>)
        .params(
        "path: a .sc design, or component: id or name of a linked component → the design's number user parameters that a link can set \
         (parameters: [{name, expression, unit, value (mm or degrees), display_value, comment?, here?}]: here is the value this design \
         would give it by name, absent when this design has no user parameter of that name and kind; values as the design saved them, \
         before a configuration row); with component, also what the link passes now: by_name, set ({name: expression}), keep, and \
         changed (true: the file changed since this design read it); reads the file, changes nothing",
    ),
    CommandSpec::new("component.links", "List Links", crate::checked!("component.links", list_links))
        .icon("chain")
        .noundo()
        .contract(&LINKS_CONTRACT)
        .output(output_schema_for::<LinksResult>)
        .params(
        "→ linked components: component, name, path, configuration (selected row id, omitted when following the saved source state), file (resolved), found (null when the file is on a network location not allowed, \
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
    doc.feature(link.feature)
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
        .filter(|(comp, link)| link_file_problem(doc, **comp, link, from).is_none())
        .flat_map(|(comp, l)| {
            let meshes = if l.step_names.is_none() { mesh_feature_of(doc, *comp, l) } else { None };
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
    // Bodies made with passed values are not the ones the file gives when it is read as it is.
    if parameters::drives(link) {
        return Some("this design passes it parameter values, which reading the file again does not apply".into());
    }
    let file = resolve(from, &link.path);
    if let Some(problem) = link_presence(&file, from) {
        let what = if link.step_names.is_some() { "STEP file" } else { "design" };
        return Some(format!("its linked {what} cannot be read now: {}", problem.reason));
    }
    let Some(kept) = link.file_sha256.as_ref() else {
        return Some(format!("this design has no record of which {} it read; run Update Links, then save again", file.display()));
    };
    (file_sha256(&file, from).as_ref() != Some(kept))
        .then(|| format!("{} changed since this design read it; run Update Links, then save again", file.display()))
}

/// What a save without linked bodies says of each link that keeps its bodies in the file
/// ([`leave_out_linked_bodies`]).
pub(super) fn kept_linked_bodies(s: &Session) -> Vec<String> {
    let from = s.path.as_deref();
    s.doc
        .links
        .iter()
        .filter(|(comp, link)| !left_out(&s.doc, **comp, link))
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

/// What [`left_out_bodies`] gives for one link: its Import feature as the file gives it now,
/// the face names, whether the file changed since the link read it, and the mesh bodies of a
/// linked design (none for a STEP file).
type LeftOut = (solidcraft_io::StepFeature, Vec<Vec<String>>, bool, Vec<MeshData>);

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
    /// What each linked design gave, by its file and configuration, so that a design many links
    /// lead to is read once: without it the reads grow with the number of ways to reach it. With
    /// [`LinkedRead::Folder`] a design below a link of the outermost design is kept for that link
    /// (the key's last part), whose folder decides what it may read.
    read: std::collections::HashMap<(PathBuf, Option<(u64, Option<u64>)>, Option<PathBuf>), ReadOnce>,
}

thread_local! {
    static READING: std::cell::RefCell<Reading> = std::cell::RefCell::new(Reading {
        opening: Vec::new(),
        which: LinkedRead::All,
        refused: 0,
        outside: 0,
        again: 0,
        read: std::collections::HashMap::new(),
    });
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
                reading.read.clear();
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
                reading.read.clear();
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
    let mut warnings = Vec::new();
    for (comp, link) in links {
        let name = doc.components.iter().find(|c| c.id == comp).map_or_else(|| comp.to_string(), |c| c.name.clone());
        if !may_read(&resolve(from, &link.path)) {
            READING.with(|reading| reading.borrow_mut().outside += 1);
            let what = if link.step_names.is_some() { "STEP file" } else { "design" };
            warnings.push(format!("{name} has no bodies yet: its linked {what} is in another folder, and Update Links reads it"));
            continue;
        }
        let (feature, faces, changed, meshes) = match left_out_bodies(&link, from, step_import_flat(doc, &link), validation) {
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
                        } else {
                            let (next_names, next_inherited, removed) = realign_changed_body_count(
                                "",
                                "",
                                solidcraft_io::StepNames::default(),
                                &f.body_names,
                                &link.inherited,
                                &feature.body_names,
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
            remove_stale_body_properties(doc, link.feature, &removed);
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
    let key = (target, configuration.map(|c| (c.row, c.identity)), allowed_by);
    let known = READING.with(|reading| reading.borrow().read.get(&key).cloned());
    let read = match known {
        Some(read) => read,
        None => {
            let (refused, again) = READING.with(|reading| {
                let reading = reading.borrow();
                (reading.refused, reading.again)
            });
            if again >= READ_AGAIN {
                return refuse(format!(
                    "{}: the linked designs saved without bodies link each other in too many ways to read them all; Update Links reads it once the loop is gone",
                    file.display()
                ));
            }
            let read = read_once(&file, from, configuration, validation);
            // What a design gives after a link below it was refused for its place in this chain
            // is not what another chain gets.
            READING.with(|reading| {
                let mut reading = reading.borrow_mut();
                if reading.refused == refused {
                    reading.read.insert(key, read.clone());
                } else {
                    reading.again += 1;
                }
            });
            read
        }
    };
    let (name, kind, body_names, invisible, warnings, import, faces, digest, meshes) = read.map_err(EngineError::Other)?;
    let changed = link.file_sha256.as_deref().is_some_and(|kept| kept != digest);
    Ok((solidcraft_io::StepFeature { name, kind, body_names, invisible, warnings, import }, faces, changed, meshes))
}

/// [`left_out_bodies`] for a linked STEP file at `file`: read again as Update Links reads it
/// ([`source_bodies`]), with the link's naming rule; `flat` as there.
fn left_out_step(link: &DesignLink, file: &Path, from: Option<&str>, flat: bool, validation: solidcraft_kernel::Validation) -> Result<LeftOut> {
    let bytes = read_link(file, from)?;
    let ((_, feature, ..), _) = source_bodies(file, &bytes, link, flat, validation)?;
    let changed = link.file_sha256.as_deref().is_some_and(|kept| kept != DesignLink::file_digest(&bytes));
    Ok((feature, Vec::new(), changed, Vec::new()))
}

/// [`left_out_bodies`]' read of the design at `file`, for a design opened from `from`.
fn read_once(file: &Path, from: Option<&str>, configuration: Option<SelectedConfiguration>, validation: solidcraft_kernel::Validation) -> ReadOnce {
    let bytes = read_link(file, from).map_err(|e| e.to_string())?;
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    let FeatureKind::Import { step, .. } = &feature.kind else { return };
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
        warnings: feature.warnings.clone(),
        properties: properties.to_vec(),
        metadata: metadata.clone(),
        meshes: meshes.to_vec(),
    });
}

/// The bytes of the source at `file`, which must not be the design saved at `own`.
fn read_link(file: &Path, own: Option<&str>) -> Result<Vec<u8>> {
    let shown = file.display().to_string();
    // A network location only when its server is allowed, a device never: judged from the text
    // before anything is asked of the disk (on Windows, resolving or reading `\\host\share\x`
    // connects to the host), so a refused path skips the self-link check, which resolves it, and
    // `read_linked_file` says why it is refused.
    let reachable = solidcraft_io::vfs::design_path_refusal(&shown, own).is_none();
    if reachable && own.is_some_and(|o| same_file(Path::new(o), file)) {
        return Err(EngineError::Other(format!("{shown}: a design can't be linked into itself")));
    }
    read_linked_file(file, own).map_err(|e| EngineError::Other(e.reason))
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

/// The digest of the design file at `file` now ([`DesignLink::file_digest`]), `None` when it
/// can't be read. `component.links` asks each time a tab is shown, so a file is read again only
/// when its time or size changed since; the browser build keeps no times and reads it each time.
pub(super) fn file_sha256(file: &Path, own: Option<&str>) -> Option<String> {
    let file = absolute(file);
    let generation = {
        let digests = DIGESTS.lock().ok()?;
        let generation = digests.0;
        let stamp = solidcraft_io::vfs::design_path_stamp(&file.to_string_lossy(), own);
        if let Some(stamp) = stamp
            && let Some(known) = digests.1.iter().find(|(f, s, _)| *f == file && *s == stamp).map(|(_, _, d)| d.clone())
        {
            return Some(known);
        }
        (generation, stamp)
    };
    let (generation, stamp) = generation;
    let digest = DesignLink::file_digest(&read_link(&file, own).ok()?);
    if let (Some(stamp), Ok(mut digests)) = (stamp, DIGESTS.lock())
        && digests.0 == generation
    {
        digests.1.retain(|(f, _, _)| *f != file);
        if digests.1.len() >= DIGESTS_SIZE {
            digests.1.remove(0);
        }
        digests.1.push((file, stamp, digest.clone()));
    }
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
            Source::File(file) => read_link(file, own),
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
    #[cfg(test)]
    EVALUATED.with(|n| n.set(n.get() + 1));
    let shown = source.shown();
    let fail = |e: solidcraft_io::IoError| EngineError::Other(format!("{shown}: {e}"));
    let (doc, saved) = if solidcraft_io::is_design_container(bytes) {
        let mut reader = solidcraft_io::DesignContainerReader::new(std::io::Cursor::new(bytes)).map_err(fail)?;
        let saved = reader.geometry();
        (reader.read_document().map_err(fail)?, saved)
    } else {
        (solidcraft_io::read_design(bytes).map_err(fail)?, None)
    };
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
                    (Plane::new(bl, br - bl, tl - bl), solidcraft_kernel::services::affine::fit_aspect(at(bl), at(br), at(tl), aspect))
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
    let mut feature = solidcraft_io::step_import_feature(&step, shown).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
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
/// The cosmetic thread records of linked bodies, by body, as an Import feature keeps them
/// (`link_threads`): none at all when no body has one.
fn linked_threads(sources: &[SourceProperties]) -> Vec<Vec<solidcraft_doc::ThreadInfo>> {
    if sources.iter().any(|source| !source.threads.is_empty()) { sources.iter().map(|source| source.threads.clone()).collect() } else { Vec::new() }
}

fn inherit_properties(s: &mut Session, component: u64, sources: &[SourceProperties]) -> Result<bool> {
    if !s.doc.links.contains_key(&component) {
        return Ok(false);
    }
    let mut prune_candidates = link_inherited_linked_copies(&s.doc);
    let mut changed = consolidate_and_prune_linked_materials(s.doc_mut(), &prune_candidates)?;
    prune_candidates.extend(link_inherited_linked_copies(&s.doc));
    let Some(link) = s.doc.links.get(&component).cloned() else { return Ok(false) };
    let state = s.model.state();
    let names: Vec<String> = state.bodies.iter().filter(|b| b.feature == link.feature).map(|b| b.name.clone()).collect();
    let slots = s.doc.feature(link.feature).map(|f| f.body_name_slots(names.len())).unwrap_or_default();
    let mut inherited = link.inherited.clone();
    inherited.resize(sources.len(), LinkedBodyProperties::default());
    let threads = linked_threads(sources);
    if let Some(feature) = s.doc_mut().feature_mut(link.feature)
        && feature.link_threads != threads
    {
        feature.link_threads = threads;
        changed = true;
    }
    for (name, slot) in names.iter().zip(slots) {
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
    // What the new link passes its design; a reused link must pass the same.
    let drive = parameters::requested(cmd, p)?;
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
    // Values are passed to a design by its file: one held in memory has none to drive.
    if held.is_some() && (parameters::drives(&drive) || !drive.parameters_kept.is_empty()) {
        return Err(bad(cmd, "`parameters`, `by_name` and `keep` need a design saved in a file: give `path`"));
    }
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
                            && parameters::same_drive(link, &drive)
                            && link_target(&resolve(s.path.as_deref(), &link.path)) == target
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
        None => source.bytes(s.path.as_deref())?,
    };
    let read = DesignLink::file_digest(&bytes);
    let (linked, (images, image_warnings), applied, units) = match &source {
        Source::File(file) => {
            let link = DesignLink {
                path: file.to_string_lossy().into_owned(),
                configuration: configuration.map(|c| c.row),
                configuration_identity: configuration.and_then(|c| c.identity),
                file_sha256: Some(read.clone()),
                ..drive.clone()
            };
            parameters::link_bodies_with_images(s, &link, file, &bytes, configuration, true)?
        }
        Source::Session { .. } => {
            let (linked, images) = design_bodies(&source, &bytes, configuration, s.validation())?;
            (linked, images, Default::default(), Default::default())
        }
    };
    let (design_name, mut sf, faces, metadata, properties, meshes) = linked;
    sf.warnings.extend(image_warnings);
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
    let link = DesignLink {
        path: link_path,
        feature: id,
        configuration: configuration.map(|c| c.row),
        configuration_identity: configuration.and_then(|c| c.identity),
        inherited_metadata: metadata,
        file_sha256: Some(read),
        parameter_values: applied,
        parameter_units: units,
        ..drive
    };
    let passed = parameters::listed(&link);
    s.doc_mut().links.insert(comp, link);
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
        parameters: passed,
    };
    reply(cmd, InsertDesignResult { inserted, param_warnings: Vec::new() })
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
    // Linked designs saved without bodies of their own follow the caller's rule as they open.
    let _reading = Opening::enter(s.path.as_deref(), LinkedRead::read(p, cmd)?);
    let links: Vec<(u64, DesignLink)> =
        s.doc.links.iter().filter(|(c, _)| only.as_ref().is_none_or(|o| o.contains(c))).map(|(c, l)| (*c, l.clone())).collect();
    let (mut updated, mut unchanged, mut warnings): (Vec<String>, Vec<String>, Vec<String>) = (Vec::new(), Vec::new(), Vec::new());
    let mut failed: Vec<LinkFailure> = Vec::new();
    let mut read_components = Vec::new();
    let count = links.len();
    for (index, (comp, link)) in links.into_iter().enumerate() {
        let cname = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone());
        let Some(cname) = cname else {
            // The component was deleted: the link goes with it.
            s.doc_mut().links.remove(&comp);
            continue;
        };
        // A window running this off its thread names the design being read again (#5329).
        solidcraft_doc::progress::item(Some((&cname, index, count)));
        if link.path.is_empty() {
            // A design that came from this session has no file to read: it keeps its bodies,
            // and says so.
            warnings.push(format!("{cname} keeps its bodies: its design came from this session and has no file yet"));
            continue;
        }
        let (old, file, bytes) = match read_linked(s, comp, &link) {
            Ok(read) => read,
            Err(e) if go_on => {
                failed.push(LinkFailure { component: comp, name: cname.clone(), error: e.clone() });
                warnings.push(format!("{cname} keeps its bodies: {e}"));
                continue;
            }
            Err(e) => return Err(bad(cmd, format!("{cname}: {e}"))),
        };
        // The link keeps what it read, so opening this design again knows whether the file
        // changed while it was closed (#9739).
        let read = DesignLink::file_digest(&bytes);
        if link.file_sha256.as_deref() != Some(read.as_str())
            && let Some(kept) = s.doc_mut().links.get_mut(&comp)
        {
            kept.file_sha256 = Some(read);
        }
        // A link that has not recorded the properties it inherited (made before they were) is read again.
        let kept_faces = s
            .doc
            .feature(link.feature)
            .filter(|_| !parameters::drives(&link))
            .filter(|_| link.step_names.is_some() || (!link.inherited.is_empty() && link.inherited.iter().all(|p| p.faces.is_some())))
            .map(|f| digest(f.link_faces.as_slice()));
        if let Some((known, properties, metadata)) = kept_faces.and_then(|faces| {
            let meshes = link_meshes(s, comp, link.feature);
            unchanged_read(&file, selected_configuration(&link), digest(bytes.as_slice()), digest(old.0.as_bytes()), faces, meshes)
        }) {
            warnings.extend(known);
            // Another component or session may have evaluated this source first. Its
            // cached geometry says nothing about this link's inherited properties.
            let (took, kept) = inherit_metadata(s, comp, &metadata);
            warnings.extend(kept);
            if inherit_properties(s, comp, &properties)? || took {
                updated.push(cname)
            } else {
                unchanged.push(cname)
            }
            read_components.push(comp);
            continue;
        }
        // A linked STEP file takes no parameters: it is read as it is.
        let bodies = if link.step_names.is_some() {
            source_bodies(&file, &bytes, &link, step_import_flat(&s.doc, &link), s.validation())
                .map(|(read, _)| (read, Default::default(), Default::default()))
        } else {
            parameters::link_bodies(s, &link, &file, &bytes, selected_configuration(&link), false)
        };
        let ((_, sf, faces, metadata, properties, meshes), applied, units) = match bodies {
            Ok(read) => read,
            Err(e) if go_on => {
                // It keeps its bodies, so it has not taken this file in yet.
                if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
                    kept.file_sha256.clone_from(&link.file_sha256);
                }
                failed.push(LinkFailure { component: comp, name: cname.clone(), error: e.to_string() });
                warnings.push(format!("{cname} keeps its bodies: {e}"));
                continue;
            }
            Err(e) => return Err(bad(cmd, format!("{cname}: {e}"))),
        };
        warnings.extend(sf.warnings.iter().cloned());
        if parameters::drives(&link)
            && let Some(kept) = s.doc_mut().links.get_mut(&comp)
        {
            kept.parameter_values = applied;
            kept.parameter_units = units;
        }
        let (took, kept_metadata) = inherit_metadata(s, comp, &metadata);
        warnings.extend(kept_metadata);
        let took = set_link_meshes(s, comp, link.feature, &cname, &file.display().to_string(), meshes)? || took;
        let same = matches!(&sf.kind, FeatureKind::Import { step, records, .. } if *step == old.0 && *records == old.1);
        let kept = s.doc.feature(link.feature).map(|f| &f.link_faces);
        if same && kept == Some(&faces) {
            if inherit_properties(s, comp, &properties)? || took {
                updated.push(cname)
            } else {
                unchanged.push(cname)
            }
            read_components.push(comp);
            continue;
        }
        // The same bodies, their faces named for the first time (a link made before names were
        // kept): nothing a reference finds changes. Names the design gave other faces since do.
        let changed = !same || kept.is_some_and(|k| !k.is_empty()) || took;
        if !same {
            for name in numbered_references(s, comp, link.feature) {
                warnings.push(format!(
                    "{name} refers to faces of {cname} by their place in the linked file, which this update can change: check it, and pick its faces again so it follows them by name"
                ));
            }
        }
        let mut moved = None;
        let mut realigned = None;
        // The old names are read as the new ones were: a STEP link's own naming (source_bodies).
        let naming = link.step_names.as_deref().and_then(solidcraft_io::StepNames::parse).unwrap_or_default();
        let before = s.model.result(link.feature).map(|r| r.state.clone());
        if let Some(f) = s.doc_mut().feature_mut(link.feature) {
            if !same {
                // Bodies keep their names where the design still has as many, also when it lists
                // them in another order (#21371).
                if f.body_names.len() != sf.body_names.len() {
                    if f.body_names.is_empty() {
                        f.body_names = sf.body_names;
                    } else {
                        let (next_names, next_inherited, removed) =
                            realign_changed_body_count(&old.0, &file.display().to_string(), naming, &f.body_names, &link.inherited, &sf.body_names);
                        f.body_names = next_names;
                        realigned = Some((next_inherited, removed));
                    }
                } else if let Some(order) = moved_bodies(&old.0, &file.display().to_string(), naming, &sf.body_names) {
                    f.body_names = reordered(&f.body_names, &order);
                    moved = Some(order);
                }
                f.kind = sf.kind;
            }
            // A link made before the design's face names were kept gets them here.
            f.link_faces = faces;
        }
        // What the link last took from each body moves with it, so a body's own override stays one.
        if let Some((next_inherited, removed)) = realigned {
            if link.step_names.is_none()
                && let Some(kept) = s.doc_mut().links.get_mut(&comp)
            {
                kept.inherited = next_inherited;
            }
            remove_stale_body_properties(s.doc_mut(), link.feature, &removed);
        } else if let Some(order) = moved
            && let Some(kept) = s.doc_mut().links.get_mut(&comp)
            && kept.inherited.len() == order.len()
        {
            kept.inherited = reordered(&kept.inherited, &order);
        }
        s.refresh();
        if !same && let (Some(before), Some(after)) = (before, s.model.result(link.feature).map(|r| r.state.clone())) {
            warnings.extend(s.doc_mut().rebind_linked_faces(link.feature, &before, &after));
            s.refresh();
        }
        let changed = inherit_properties(s, comp, &properties)? || changed;
        if changed {
            updated.push(cname)
        } else {
            unchanged.push(cname)
        }
        read_components.push(comp);
    }
    solidcraft_doc::progress::item(None);
    // None could be read: nothing to take back either.
    if updated.is_empty() && unchanged.is_empty() && !failed.is_empty() {
        return Err(bad(cmd, format!("{}: {}", failed[0].name, failed[0].error)));
    }
    s.refresh();
    if let Some(e) = s.doc.links.values().find_map(|l| s.model.result(l.feature).and_then(|r| r.error.clone())) {
        return Err(EngineError::Other(e));
    }
    // A reference whose face the design no longer has says so here, not only in the joint list.
    if !updated.is_empty() {
        let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        warnings.extend(sol.errors.iter().chain(&sol.conflicts).map(|(_, name, why)| format!("joint {name}: {why}")));
    }
    crate::plugin::record_link_reads(s, &read_components);
    reply(cmd, UpdateLinksResult { updated, unchanged, warnings, failed, param_warnings: Vec::new() })
}

/// `comp`'s linked design as it was last read (its bodies) and as its file is now, read before
/// anything in the design changes.
fn read_linked(s: &Session, comp: u64, link: &DesignLink) -> std::result::Result<(ImportSource, PathBuf, Vec<u8>), String> {
    let Some(FeatureKind::Import { step: old, records, .. }) = s.doc.feature(link.feature).filter(|f| f.component == comp).map(|f| &f.kind) else {
        return Err(format!("its linked bodies (feature {}) are gone; break the link or insert the design again", link.feature));
    };
    link_refused(s.path.as_deref(), &link.path).map_err(|e| e.to_string())?;
    let file = resolve(s.path.as_deref(), &link.path);
    let bytes = read_link(&file, s.path.as_deref()).map_err(|e| e.to_string())?;
    Ok(((old.clone(), records.clone()), file, bytes))
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

    // Pass 2: assign surviving bodies their stored names and baselines; give unmatched new bodies
    // a fresh baseline and a name that cannot collide with any surviving body's stored name.
    let mut reserved = claimed_names.clone();
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
fn remove_stale_body_properties(doc: &mut Document, feature: u64, removed: &[String]) {
    if removed.is_empty() {
        return;
    }
    let stale: std::collections::HashSet<&str> = {
        let still_used: std::collections::HashSet<&str> = doc
            .features
            .iter()
            .filter(|f| f.id != feature)
            .flat_map(|f| f.body_names.iter().map(String::as_str))
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
    if link.step_names.is_some() {
        return Err(bad(cmd, "STEP sources cannot be relinked to a design; break the link and insert the new STEP file"));
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
    let bytes = read_link(&file, s.path.as_deref()).map_err(|e| bad(cmd, e.to_string()))?;
    let configuration = configuration_id(&bytes, selection).map_err(|e| bad(cmd, e.to_string()))?;
    if same_source && p.get("configuration").is_none() && configuration != selected_configuration(&link) {
        return Err(bad(cmd, "the selected configuration was replaced; select a configuration again"));
    }
    if let Some(occurrence) = lone_placement(s, p, comp, cmd)? {
        return relink_placement(s, comp, &link, occurrence, given, &file, configuration);
    }
    // Read/evaluate the candidate before changing anything. Session::execute owns the single
    // undo step and restores both the document and evaluated state on any later failure.
    let ((source_name, sf, faces, metadata, properties, meshes), applied, units) =
        parameters::link_bodies(s, &link, &file, &bytes, configuration, false).map_err(|e| bad(cmd, e.to_string()))?;
    let read = DesignLink::file_digest(&bytes);
    let source_names = sf.body_names.clone();
    let geometry_changed = !s.doc.feature(link.feature).is_some_and(|f| same_import_source(&f.kind, &sf.kind))
        || link_meshes(s, comp, link.feature) != meshes.as_slice();
    let path = stored(s.path.as_deref(), &file);
    let mut warnings = sf.warnings;
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
    if let Some(f) = s.doc_mut().feature_mut(link.feature) {
        // Both modes start as Relink always did; `sync` renames the bodies below.
        if f.body_names.len() != sf.body_names.len() {
            if f.body_names.is_empty() {
                f.body_names = sf.body_names;
            } else {
                let old_step = match &f.kind {
                    FeatureKind::Import { step, .. } => step.as_ref(),
                    _ => "",
                };
                let (next_names, next_inherited, gone) = realign_changed_body_count(
                    old_step,
                    &file.display().to_string(),
                    solidcraft_io::StepNames::default(),
                    &f.body_names,
                    &inherited,
                    &sf.body_names,
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
            f.body_names = reordered(&f.body_names, &order);
            if inherited.len() == order.len() {
                inherited = reordered(&inherited, &order);
            }
        }
        f.kind = sf.kind;
        f.link_faces = faces;
    }
    remove_stale_body_properties(s.doc_mut(), link.feature, &removed);
    let cname = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
    set_link_meshes(s, comp, link.feature, &cname, &file.display().to_string(), meshes)?;
    s.doc_mut().links.insert(
        comp,
        DesignLink {
            path: file.to_string_lossy().into_owned(),
            file_sha256: Some(read),
            configuration: configuration.map(|c| c.row),
            configuration_identity: configuration.and_then(|c| c.identity),
            parameter_values: applied,
            parameter_units: units,
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
    // Report the solver's diagnostics. A resolved joint can still describe the wrong
    // connection after a source edit; the reminder above does not depend on solve errors.
    let (joint_errors, joint_conflicts) = joint_diagnostics(s, &mut warnings);
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
    let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
    for (_, name, why) in sol.errors.iter().chain(&sol.conflicts) {
        warnings.push(format!("joint {name}: {why}"));
    }
    let rows = |rows: &[(u64, String, String)]| {
        rows.iter().map(|(joint, name, why)| JointDiagnostic { joint: *joint, name: name.clone(), why: why.clone() }).collect()
    };
    (rows(&sol.errors), rows(&sol.conflicts))
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

/// One placement of `comp` comes to show the design at `file` instead: as a placement of a
/// component already linked to it, or of a new one under the same parent. The placement keeps
/// its id, position and grounding, so its joints stay; they are moved to the new bodies' faces.
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
    // A pattern or mirror of this placement can stop working: the reply names each feature that does.
    let failing_before: std::collections::BTreeSet<u64> = s.model.results.iter().filter(|r| r.error.is_some()).map(|r| r.id).collect();
    // The placement takes the values its link passed with it: a design that takes them is
    // driven as before, and one that can't is refused (nothing changes).
    let made = insert_design(
        s,
        &json!({
            "path": given,
            "parent": parent,
            "reuse": true,
            "configuration": configuration.map(|c| c.row),
            "parameters": link.parameters,
            "by_name": link.parameters_by_name,
            "keep": link.parameters_kept,
        }),
    )?;
    let linked = made["component"].as_u64().ok_or_else(|| bad(cmd, "the linked component was not created"))?;
    if s.doc.links.get(&linked).and_then(selected_configuration) != configuration {
        return Err(bad(cmd, "the selected configuration changed while relinking; try again"));
    }
    let feature = s.doc.links.get(&linked).map(|l| l.feature).ok_or_else(|| bad(cmd, "the design was not linked"))?;
    let geometry_changed = match (s.doc.feature(link.feature), s.doc.feature(feature)) {
        (Some(was), Some(now)) => !same_import_source(&was.kind, &now.kind) || link_meshes(s, comp, link.feature) != link_meshes(s, linked, feature),
        _ => true,
    };
    let mut warnings: Vec<String> = made["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect();
    if geometry_changed && s.doc.assembly.joints.iter().any(|j| j.a.occurrence == occurrence || j.b.occurrence == occurrence) {
        warnings.push("the linked geometry changed; check this placement's joints, even when the solver reports no error".to_string());
    }
    // Insert Design placed the design once; this placement takes that one's place and name.
    let added = made["occurrence"].as_u64().ok_or_else(|| bad(cmd, "the design was not placed"))?;
    let d = s.doc_mut();
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
    let (joint_errors, joint_conflicts) = joint_diagnostics(s, &mut warnings);
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

fn list_links(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut observations = Vec::new();
    let links: Vec<LinkInfo> = s
        .doc
        .links
        .iter()
        .map(|(c, l)| {
            let file = resolve(s.path.as_deref(), &l.path);
            let name = s.doc.components.iter().find(|x| x.id == *c).map(|x| x.name.clone());
            // A design that came from this session has no file: it is not looked for.
            if l.path.is_empty() {
                return LinkInfo::Looked {
                    component: *c,
                    configuration: l.configuration,
                    parameters: parameters::listed(l),
                    parameters_by_name: l.parameters_by_name,
                    parameters_kept: l.parameters_kept.iter().cloned().collect(),
                    name,
                    path: String::new(),
                    file: String::new(),
                    found: None,
                    changed: None,
                    state: None,
                    reason: None,
                };
            }
            let path = stored(s.path.as_deref(), &file);
            let shown = file.to_string_lossy().into_owned();
            // A refused path is not looked for at all: on Windows asking whether a file on
            // `\\host\share` exists already connects to the host.
            match link_refusal(s.path.as_deref(), &l.path) {
                None => {
                    let found = solidcraft_io::vfs::design_path_is_file(&shown, s.path.as_deref());
                    // Whether the file is no longer what this design last read (#9739): `null`
                    // when it isn't found, or the link keeps no digest (made before they were).
                    let current =
                        l.file_sha256.as_ref().filter(|_| found == Some(true)).and_then(|_| file_sha256(Path::new(&shown), s.path.as_deref()));
                    let changed = current.as_ref().zip(l.file_sha256.as_ref()).map(|(now, kept)| now != kept);
                    // A link opened without its bodies has read nothing of its file yet.
                    let changed = if found == Some(true) && left_out(&s.doc, *c, l) { Some(true) } else { changed };
                    if let (Some(digest), Some(changed)) = (current, changed)
                        && (changed || s.plugin_state.link_changes.get(c).is_some_and(|change| !change.read))
                    {
                        observations.push((*c, shown.clone(), digest, !changed));
                    }
                    // Why a file that can't be reached can't be: `None` when it can be read.
                    let (state, reason) = match link_presence(&file, s.path.as_deref()) {
                        Some(why) => (Some(why.state), Some(why.reason)),
                        None => (None, None),
                    };
                    LinkInfo::Looked {
                        component: *c,
                        configuration: l.configuration,
                        parameters: parameters::listed(l),
                        parameters_by_name: l.parameters_by_name,
                        parameters_kept: l.parameters_kept.iter().cloned().collect(),
                        name,
                        path,
                        file: shown,
                        found,
                        changed,
                        state,
                        reason,
                    }
                }
                Some(solidcraft_io::vfs::Remote::Network(host)) => LinkInfo::Network {
                    component: *c,
                    configuration: l.configuration,
                    name,
                    path,
                    file: shown,
                    found: None,
                    state: LinkState::NotAllowed,
                    reason: format!("on the network location \\\\{host}, which is not allowed"),
                    host,
                },
                Some(solidcraft_io::vfs::Remote::Device) => LinkInfo::Device {
                    component: *c,
                    configuration: l.configuration,
                    name,
                    path,
                    file: shown,
                    found: None,
                    state: LinkState::Unreadable,
                    reason: "names a device, not a file".into(),
                },
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
}
