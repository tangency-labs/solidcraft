//! Linked components whose bodies come from a design (Insert Design) or a STEP source.
//!
//! The linked design is opened and evaluated on its own, and its solid bodies, as placed in it,
//! join a new component through an Import feature (exact B-rep, kept in this design so it opens
//! without the part file). The link remembers the absolute file in a session and a path relative
//! to the design's folder on disk; Update Links reads it again and replaces the Import feature's
//! bodies, and Break Link keeps the bodies and forgets the file. The bodies' faces are named after the names
//! the design gives them (`Feature::link_faces`), so a joint on a linked face stays on that face
//! when the design changes, as it would inside the design, and reports it when the face is gone.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};
use solidcraft_doc::{DesignLink, Document, FeatureKind, LinkedBodyProperties, UserMaterial};

use super::CommandSpec;
use super::design_link::{absolute, resolve, stored};
use crate::params::{bad, str_, vec3};
use crate::{EngineError, Result, Session};

#[path = "component_extract.rs"]
mod extract;

#[cfg(test)]
#[path = "component_relink_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod relink_tests;

const UPDATE_LINKS_CONTRACT: crate::schema::TypedContract<crate::schema::component_link::UpdateLinksParams> = crate::schema::TypedContract::new();
const BREAK_LINK_CONTRACT: crate::schema::TypedContract<crate::schema::component_link::BreakLinkParams> = crate::schema::TypedContract::new();
const LINKS_CONTRACT: crate::schema::TypedContract<crate::schema::component_link::LinksParams> = crate::schema::TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.insert_design", "Insert Design", insert_design).at("SOLID", "ASSEMBLE").icon("component").params(
        "path: a .sc design → a new component linked to it, holding its solid bodies as placed in that design and a copy of its design properties; \
         name? (default: the design's name); parent?: component id or name (default: the active one); translate?: [x,y,z]; \
         reuse?: bool (default false): insert a free occurrence, reusing an existing link to the same file when available",
    ),
    CommandSpec::new("component.update_links", "Update Links", crate::checked!("component.update_links", update_links))
        .contract(&UPDATE_LINKS_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::component_link::UpdateLinksResult>).icon("update").params(
        "component?: id or name (default: every linked component); components?: a list of ids or names, which goes on past a design it \
         cannot read (that one keeps its bodies, is named in failed and gets a warning) — reads the linked design or STEP files again and replaces \
         their bodies where they changed, as one undo step; → updated, unchanged, failed ([{component, name, error}]), warnings (also \
         joints whose faces are gone or that no longer solve)",
    ),
    CommandSpec::new("component.relink", "Relink Source", relink).icon("update").params(
        "component: id or name of a linked component; path: a saved design (.sc or .json) — replaces the source and linked bodies for all its occurrences, preserving identities and placements; returns path, previous_path, warnings, joint_errors and joint_conflicts",
    ),
    CommandSpec::new("component.break_link", "Break Link", crate::checked!("component.break_link", break_link))
        .contract(&BREAK_LINK_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::component_link::BreakLinkResult>)
        .icon("break")
        .params("component: id or name of a linked component — keeps its bodies as they are and forgets the source file"),
    CommandSpec::new("component.extract", "Extract to File", extract::extract).icon("export").params(
        "component: id or name of a component that is not linked; path: the .sc design to write → its history, subcomponents and their joints \
         move to that design, and the component becomes linked to it (as component.insert_design makes it); returns path, file, feature, bodies, \
         notices (what stays behind), warnings and copies (components that build the same bodies: see component.replace_with_link)",
    ),
    CommandSpec::new("component.copies", "Find Identical Copies", extract::copies).icon("inspect").noundo().params(
        "component: id or name of a linked component → copies: [{component, name}], the components not linked whose own history builds the same bodies",
    ),
    CommandSpec::new("component.replace_with_link", "Replace with Link", extract::replace_with_link).icon("chain").params(
        "component or components: [ids or names] of components that are not linked; with: a linked component, or path: a .sc design \
         (its link here, else a new linked component) → each component's history goes and its placements become placements of the linked \
         component; refused when its bodies are not the same as the link's; returns component, name, replaced and warnings",
    ),
    CommandSpec::new("component.links", "List Links", crate::checked!("component.links", list_links))
        .contract(&LINKS_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::component_link::LinksResult>).icon("chain").noundo().params(
        "→ linked components: component, name, path, file (resolved), found (null when the file is on a network location not allowed, \
         or names a device: it is not looked for), changed (whether the file is no longer what this design last read: Update Links \
         reads it again; null when not found or the link is older than this record), and then host (the network location) and reason",
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

/// Resolve a design's component links against its original file, before opening it from a
/// different location (for example a recovery file) or keeping it in session history.
pub fn resolve_design_links(doc: &mut Document, from: Option<&str>) {
    for link in doc.links.values_mut() {
        link.path = absolute(&resolve(from, &link.path)).to_string_lossy().into_owned();
    }
}

/// The bodies of a linked design: its name, the Import feature that holds them and the names
/// the design gives their faces (`Feature::link_faces`), with the source's design properties.
type Linked = (String, solidcraft_io::StepFeature, Vec<Vec<String>>, solidcraft_doc::Metadata, Vec<SourceProperties>);

#[derive(Clone)]
struct SourceProperties {
    material: String,
    definition: Option<UserMaterial>,
    appearance: Option<solidcraft_doc::appearance::Look>,
    /// Resolved authored overrides, by the face index in this imported body.
    faces: Vec<(usize, solidcraft_doc::appearance::Look)>,
}

/// What a linked file last gave, by the file: digests of its bytes, of the STEP text it gave and
/// of its face names, and that read's warnings and source properties. Update Links evaluates
/// a file again only when its bytes or the linked bodies changed since (#5606). Bounded, oldest first out.
static READS: Mutex<Vec<LinkRead>> = Mutex::new(Vec::new());
const READS_SIZE: usize = 256;

struct LinkRead {
    file: PathBuf,
    bytes: u64,
    step: u64,
    faces: u64,
    warnings: Vec<String>,
    properties: Vec<SourceProperties>,
}

#[cfg(test)]
thread_local! {
    /// Linked designs read and evaluated on this thread, for tests.
    static EVALUATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn digest<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

/// Warnings and source properties of the last read of `file`, when it had these bytes and gave the STEP text
/// `step` and the face names `faces`: the bodies it gives are the ones the link holds.
fn unchanged_read(file: &Path, bytes: u64, step: u64, faces: u64) -> Option<(Vec<String>, Vec<SourceProperties>)> {
    let file = absolute(file);
    let reads = READS.lock().ok()?;
    reads
        .iter()
        .find(|r| r.file == file && r.bytes == bytes && r.step == step && r.faces == faces)
        .map(|r| (r.warnings.clone(), r.properties.clone()))
}

fn remember_read(file: &Path, bytes: u64, feature: &solidcraft_io::StepFeature, faces: &[Vec<String>], properties: &[SourceProperties]) {
    let FeatureKind::Import { step, .. } = &feature.kind else { return };
    let file = absolute(file);
    let Ok(mut reads) = READS.lock() else { return };
    reads.retain(|r| r.file != file);
    if reads.len() >= READS_SIZE {
        reads.remove(0);
    }
    reads.push(LinkRead {
        file,
        bytes,
        step: digest(step.as_bytes()),
        faces: digest(faces),
        warnings: feature.warnings.clone(),
        properties: properties.to_vec(),
    });
}

/// The bytes of the source at `file`, which must not be the design saved at `own`.
fn read_link(file: &Path, own: Option<&str>) -> Result<Vec<u8>> {
    let shown = file.display().to_string();
    if own.is_some_and(|o| absolute(Path::new(o)) == absolute(file)) {
        return Err(EngineError::Other(format!("{shown}: a design can't be linked into itself")));
    }
    // A network location only when its server is allowed, a device never: judged from the text
    // before anything is read (on Windows, reading `\\host\share\x` connects to the host).
    let design = if is_step_source(&shown) { solidcraft_io::limits::Kind::Step } else { solidcraft_io::limits::Kind::Design };
    solidcraft_io::vfs::read_design_path(
        &shown,
        own,
        if is_step_source(&shown) { solidcraft_io::limits::max_step_bytes() } else { solidcraft_io::limits::max_design_bytes() },
    )
    .map_err(|e| {
        let e = solidcraft_io::limits::explain(design, e).to_string();
        EngineError::Other(if e.starts_with(&shown) { e } else { format!("{shown}: {e}") })
    })
}

/// The solid bodies of the design at `file`, evaluated, as an Import feature, with the digest of
/// the bytes read, which the link keeps ([`DesignLink::file_sha256`], #9739).
fn linked_bodies_read(file: &Path, own: Option<&str>) -> Result<(Linked, String)> {
    let bytes = read_link(file, own)?;
    Ok((design_bodies(file, &bytes)?, DesignLink::file_digest(&bytes)))
}

/// Linked files' digests as [`file_sha256`] last worked them out, by file, with the time and
/// size the file had then. Bounded, oldest first out.
static DIGESTS: Mutex<Vec<(PathBuf, (std::time::SystemTime, u64), String)>> = Mutex::new(Vec::new());
const DIGESTS_SIZE: usize = 256;

/// The digest of the design file at `file` now ([`DesignLink::file_digest`]), `None` when it
/// can't be read. `component.links` asks each time a tab is shown, so a file is read again only
/// when its time or size changed since; the browser build keeps no times and reads it each time.
pub(super) fn file_sha256(file: &Path, own: Option<&str>) -> Option<String> {
    let file = absolute(file);
    let stamp = solidcraft_io::vfs::design_path_stamp(&file.to_string_lossy(), own);
    if let Some(stamp) = stamp
        && let Some(known) = DIGESTS.lock().ok()?.iter().find(|(f, s, _)| *f == file && *s == stamp).map(|(_, _, d)| d.clone())
    {
        return Some(known);
    }
    let digest = DesignLink::file_digest(&read_link(&file, own).ok()?);
    if let (Some(stamp), Ok(mut digests)) = (stamp, DIGESTS.lock()) {
        digests.retain(|(f, _, _)| *f != file);
        if digests.len() >= DIGESTS_SIZE {
            digests.remove(0);
        }
        digests.push((file, stamp, digest.clone()));
    }
    Some(digest)
}

/// Only STEP has a source link; other formats accepted by Insert STEP keep their old behaviour.
pub(super) fn is_step_source(path: &str) -> bool {
    Path::new(path).extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("step") || x.eq_ignore_ascii_case("stp"))
}

/// Read either kind of linked source into the same replacement and reference-matching path.
fn source_bodies(file: &Path, bytes: &[u8], link: &DesignLink) -> Result<Linked> {
    let Some(names) = &link.step_names else { return design_bodies(file, bytes) };
    let names = solidcraft_io::StepNames::parse(names).ok_or_else(|| EngineError::Other("invalid linked STEP naming policy".into()))?;
    let shown = file.to_string_lossy();
    let mut feature = solidcraft_io::step_import_feature_named(bytes, &shown, names).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    // Assembly body slots and offsets are retained by the Import feature, like body names.
    // Repeated placements must remain skipped rather than being imported a second time.
    if let FeatureKind::Import { skip, .. } = &mut feature.kind {
        *skip = super::file::import::step_skip(&feature.import);
    }
    remember_read(file, digest(bytes), &feature, &[], &[]);
    Ok((feature.name.clone(), feature, Vec::new(), Default::default(), Vec::new()))
}

/// The solid bodies of `bytes`, the design at `file`, as an Import feature. The model the file
/// keeps is its evaluation when this build saved it for exactly that design, as when it is opened.
fn design_bodies(file: &Path, bytes: &[u8]) -> Result<Linked> {
    #[cfg(test)]
    EVALUATED.with(|n| n.set(n.get() + 1));
    let shown = file.display().to_string();
    let fail = |e: solidcraft_io::IoError| EngineError::Other(format!("{shown}: {e}"));
    let (doc, saved) = if solidcraft_io::is_design_container(bytes) {
        let mut reader = solidcraft_io::DesignContainerReader::new(std::io::Cursor::new(bytes)).map_err(fail)?;
        let saved = reader.geometry();
        (reader.read_document().map_err(fail)?, saved)
    } else {
        (solidcraft_io::read_design(bytes).map_err(fail)?, None)
    };
    let (name, feature, faces, metadata, properties) = session_bodies(&Session::opened_at(doc, saved.as_ref(), Some(&shown)), &shown)?;
    remember_read(file, digest(bytes), &feature, &faces, &properties);
    Ok((name, feature, faces, metadata, properties))
}

/// The solid bodies of `part`, the design saved as `shown`, as an Import feature.
fn session_bodies(part: &Session, shown: &str) -> Result<Linked> {
    let name = part.doc.name.clone();
    if let Some(e) = part.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(EngineError::Other(format!("{shown}: the design has a feature that fails ({e}); fix it before linking it")));
    }
    let world = part.world_state();
    let solids: Vec<String> = world.bodies.iter().filter(|b| !b.body.is_mesh()).map(|b| b.name.clone()).collect();
    if solids.is_empty() {
        return Err(EngineError::Other(format!("{shown}: the design has no solid bodies to link")));
    }
    let step = solidcraft_io::export(&part.doc.painted_for_link(&world), &solids, solidcraft_io::Format::Step, &name)
        .map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    let mut feature = solidcraft_io::step_import_feature(&step, shown).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    if world.bodies.len() > solids.len() {
        feature.warnings.push(format!("{} mesh bodies of {shown} are not linked", world.bodies.len() - solids.len()));
    }
    // The STEP text keeps no face names: carry the design's own, so references into the linked
    // bodies follow them as they do inside the design (#3330).
    let faces = solidcraft_doc::naming::carried_face_names(&world, &feature.import.bodies);
    let properties = feature
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
            Ok(SourceProperties { material, definition, appearance, faces })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((name, feature, faces, part.doc.metadata.clone(), properties))
}

fn material_fingerprint(doc: &Document, material: &str) -> Result<Option<String>> {
    if solidcraft_doc::material_density(material).is_some() {
        return Ok(None);
    }
    doc.user_material(material).map(LinkedBodyProperties::fingerprint).transpose().map_err(|e| EngineError::Other(e.to_string()))
}

/// Copy a design-owned definition without changing a same-named assembly material.
fn inherit_material(doc: &mut Document, source: &SourceProperties, previous: Option<&str>) -> Result<String> {
    let Some(definition) = &source.definition else { return Ok(source.material.clone()) };
    for existing in &doc.user_materials {
        // Equal density/optics do not make differently named materials interchangeable.
        // A previous disambiguated copy can be reused on subsequent source updates.
        if !existing.name.eq_ignore_ascii_case(&definition.name) && previous != Some(existing.name.as_str()) {
            continue;
        }
        let mut candidate = definition.clone();
        candidate.name.clone_from(&existing.name);
        if &candidate == existing {
            return Ok(existing.name.clone());
        }
    }
    if doc.user_materials.len() >= solidcraft_doc::MAX_USER_MATERIALS {
        return Err(EngineError::Other("cannot inherit linked material: this design already has 1000 custom materials".into()));
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
    let Some(link) = s.doc.links.get(&component).cloned() else { return Ok(false) };
    let state = s.model.state();
    let names: Vec<String> = state.bodies.iter().filter(|b| b.feature == link.feature).map(|b| b.name.clone()).collect();
    let slots = s.doc.feature(link.feature).map(|f| f.body_name_slots(names.len())).unwrap_or_default();
    let mut inherited = link.inherited.clone();
    inherited.resize(sources.len(), LinkedBodyProperties::default());
    let mut changed = false;
    for (name, slot) in names.iter().zip(slots) {
        let Some(source) = sources.get(slot) else { continue };
        let old = link.inherited.get(slot);
        let current_definition = material_fingerprint(&s.doc, &s.doc.body_material(name))?;
        let may_change_material = match old {
            Some(old) => s.doc.materials.get(name) == Some(&old.material) && current_definition == old.material_fingerprint,
            None => !s.doc.materials.contains_key(name),
        };
        let mut next = old.cloned().unwrap_or_default();
        if may_change_material {
            let material = inherit_material(s.doc_mut(), source, old.map(|old| old.material.as_str()))?;
            changed |= s.doc.materials.get(name) != Some(&material);
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
    Ok(changed)
}

fn component_key(s: &Session, p: &Value, key: &str, cmd: &str) -> Result<u64> {
    let k = match p.get(key) {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, format!("`{key}` must be a component id or name"))),
    };
    s.doc.find_component(&k).ok_or_else(|| bad(cmd, format!("no component `{k}`")))
}

fn insert_design(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.insert_design";
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let translate = match p.get("translate") {
        None => solidcraft_geom::Vec3::ZERO,
        Some(v) => vec3(v).filter(|t| t.is_finite() && t.len() < 1e9).ok_or_else(|| bad(cmd, "`translate` must be [x, y, z] in mm"))?,
    };
    // A network or device path is judged from its text, as given, before anything reads it.
    link_refused(s.path.as_deref(), given).map_err(|e| bad(cmd, e.to_string()))?;
    let file = absolute(Path::new(given));
    if s.path.as_deref().is_some_and(|own| absolute(Path::new(own)) == file) {
        return Err(bad(cmd, "a design can't be linked into itself"));
    }
    if p.get("reuse").and_then(Value::as_bool).unwrap_or(false) {
        let existing = s.doc.links.iter().find(|(_, link)| absolute(&resolve(s.path.as_deref(), &link.path)) == file).map(|(id, _)| *id);
        if let Some(component) = existing {
            let parent = p.get("parent").cloned().unwrap_or_else(|| json!(s.active_component));
            return super::component::copy_occurrence(s, &json!({"component": component, "parent": parent, "translate": translate}));
        }
    }
    let ((design_name, sf, faces, metadata, properties), read) = linked_bodies_read(&file, s.path.as_deref())?;
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
        c.metadata = metadata;
    }
    let id = s.doc_mut().add_feature(sf.kind, Some(&name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = comp;
        f.body_names = sf.body_names;
        f.link_faces = faces;
    }
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| Some(o.id) == occ) {
        o.transform[3] = [translate.x, translate.y, translate.z, 1.0];
    }
    let path = stored(s.path.as_deref(), &file);
    s.doc_mut().links.insert(
        comp,
        DesignLink { path: file.to_string_lossy().into_owned(), feature: id, inherited: Vec::new(), file_sha256: Some(read), step_names: None },
    );
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    inherit_properties(s, comp, &properties)?;
    s.refresh();
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.as_str()).collect();
    Ok(
        json!({"component": comp, "occurrence": occ, "name": name, "feature": id, "path": path, "bodies": bodies, "created_bodies": bodies, "warnings": sf.warnings}),
    )
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
    let links: Vec<(u64, DesignLink)> =
        s.doc.links.iter().filter(|(c, _)| only.as_ref().is_none_or(|o| o.contains(c))).map(|(c, l)| (*c, l.clone())).collect();
    let (mut updated, mut unchanged, mut warnings, mut failed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
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
        let (old, file, bytes) = match read_linked(s, comp, &link) {
            Ok(read) => read,
            Err(e) if go_on => {
                failed.push(json!({"component": comp, "name": cname, "error": e}));
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
            .filter(|_| link.step_names.is_some() || (!link.inherited.is_empty() && link.inherited.iter().all(|p| p.faces.is_some())))
            .map(|f| digest(f.link_faces.as_slice()));
        if let Some((known, properties)) = kept_faces.and_then(|faces| unchanged_read(&file, digest(bytes.as_slice()), digest(old.as_bytes()), faces))
        {
            warnings.extend(known);
            // Another component or session may have evaluated this source first. Its
            // cached geometry says nothing about this link's inherited properties.
            if inherit_properties(s, comp, &properties)? {
                updated.push(cname)
            } else {
                unchanged.push(cname)
            }
            continue;
        }
        let (_, sf, faces, _, properties) = match source_bodies(&file, &bytes, &link) {
            Ok(read) => read,
            Err(e) if go_on => {
                // It keeps its bodies, so it has not taken this file in yet.
                if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
                    kept.file_sha256.clone_from(&link.file_sha256);
                }
                failed.push(json!({"component": comp, "name": cname, "error": e.to_string()}));
                warnings.push(format!("{cname} keeps its bodies: {e}"));
                continue;
            }
            Err(e) => return Err(bad(cmd, format!("{cname}: {e}"))),
        };
        warnings.extend(sf.warnings.iter().cloned());
        let same = matches!(&sf.kind, FeatureKind::Import { step, .. } if *step == old);
        let kept = s.doc.feature(link.feature).map(|f| &f.link_faces);
        if same && kept == Some(&faces) {
            if inherit_properties(s, comp, &properties)? {
                updated.push(cname)
            } else {
                unchanged.push(cname)
            }
            continue;
        }
        // The same bodies, their faces named for the first time (a link made before names were
        // kept): nothing a reference finds changes. Names the design gave other faces since do.
        let changed = !same || kept.is_some_and(|k| !k.is_empty());
        if !same {
            for name in numbered_references(s, comp, link.feature) {
                warnings.push(format!(
                    "{name} refers to faces of {cname} by their place in the linked file, which this update can change: check it, and pick its faces again so it follows them by name"
                ));
            }
        }
        let before = s.model.result(link.feature).map(|r| r.state.clone());
        if let Some(f) = s.doc_mut().feature_mut(link.feature) {
            if !same {
                // Bodies keep their names where the design still has as many.
                if f.body_names.len() != sf.body_names.len() {
                    f.body_names = sf.body_names;
                }
                f.kind = sf.kind;
            }
            // A link made before the design's face names were kept gets them here.
            f.link_faces = faces;
        }
        s.refresh();
        if !same && let (Some(before), Some(after)) = (before, s.model.result(link.feature).map(|r| r.state.clone())) {
            warnings.extend(s.doc_mut().rebind_linked_faces(link.feature, &before, &after));
            s.refresh();
        }
        let changed = inherit_properties(s, comp, &properties)? || changed;
        if changed { updated.push(cname) } else { unchanged.push(cname) }
    }
    solidcraft_doc::progress::item(None);
    // None could be read: nothing to take back either.
    if updated.is_empty() && unchanged.is_empty() && !failed.is_empty() {
        let (name, why) = (failed[0]["name"].as_str().unwrap_or_default(), failed[0]["error"].as_str().unwrap_or_default());
        return Err(bad(cmd, format!("{name}: {why}")));
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
    Ok(json!({"updated": updated, "unchanged": unchanged, "warnings": warnings, "failed": failed}))
}

/// `comp`'s linked design as it was last read (its bodies) and as its file is now, read before
/// anything in the design changes.
fn read_linked(s: &Session, comp: u64, link: &DesignLink) -> std::result::Result<(std::sync::Arc<str>, PathBuf, Vec<u8>), String> {
    let Some(FeatureKind::Import { step: old, .. }) = s.doc.feature(link.feature).filter(|f| f.component == comp).map(|f| &f.kind) else {
        return Err(format!("its linked bodies (feature {}) are gone; break the link or insert the design again", link.feature));
    };
    link_refused(s.path.as_deref(), &link.path).map_err(|e| e.to_string())?;
    let file = resolve(s.path.as_deref(), &link.path);
    let bytes = read_link(&file, s.path.as_deref()).map_err(|e| e.to_string())?;
    Ok((old.clone(), file, bytes))
}

/// The joints and joint origins on occurrences of `comp` that name its linked faces by their
/// place in the file (`F<feature>:face:<k>`, made before the design's own names were kept), which
/// an update can give to another face.
fn numbered_references(s: &Session, comp: u64, feature: u64) -> Vec<String> {
    let numbered = format!("F{feature}:face:");
    let on = |o: &solidcraft_doc::joints::JointOrigin| {
        s.doc.occurrences.iter().any(|x| x.id == o.occurrence && s.doc.component_within(x.component, comp))
            && o.snap.face_names().iter().any(|n| n.trim_start_matches(solidcraft_doc::naming::LEGACY_MARK).starts_with(&numbered))
    };
    let joints = s.doc.assembly.joints.iter().filter(|j| on(&j.a) || on(&j.b)).map(|j| format!("joint {}", j.name));
    let origins = s.doc.assembly.origins.iter().filter(|o| on(&o.origin)).map(|o| format!("joint origin {}", o.name));
    joints.chain(origins).collect()
}

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
    let file = absolute(Path::new(given));
    // Any design Insert Design takes: a .sc container (or a legacy one) or a plain JSON design.
    if !solidcraft_io::is_design_path(given) {
        return Err(bad(cmd, "choose a saved design (.sc or .json)"));
    }
    // A network or device path is judged from its text before the disk is asked anything.
    link_refused(s.path.as_deref(), given).map_err(|e| bad(cmd, e.to_string()))?;
    // The ordinary lexical check in linked_bodies also works in the browser VFS. On native
    // hosts, reject an alias of this assembly too (a symlink or a path with different case).
    if let (Ok(candidate), Some(Ok(own))) = (file.canonicalize(), s.path.as_deref().map(std::fs::canonicalize))
        && candidate == own
    {
        return Err(bad(cmd, "a design can't be linked into itself"));
    }
    // Read/evaluate the candidate before changing anything. Session::execute owns the single
    // undo step and restores both the document and evaluated state on any later failure.
    let ((_, sf, faces, _, properties), read) = linked_bodies_read(&file, s.path.as_deref()).map_err(|e| bad(cmd, e.to_string()))?;
    let geometry_changed = match (s.doc.feature(link.feature).map(|f| &f.kind), &sf.kind) {
        (Some(FeatureKind::Import { step: old, .. }), FeatureKind::Import { step, .. }) => old != step,
        _ => true,
    };
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
    if let Some(f) = s.doc_mut().feature_mut(link.feature) {
        if f.body_names.len() != sf.body_names.len() {
            f.body_names = sf.body_names;
        }
        f.kind = sf.kind;
        f.link_faces = faces;
    }
    s.doc_mut().links.insert(comp, DesignLink { path: file.to_string_lossy().into_owned(), file_sha256: Some(read), ..link.clone() });
    s.refresh();
    if let Some(error) = s.model.result(link.feature).and_then(|r| r.error.clone()) {
        return Err(bad(cmd, error));
    }
    inherit_properties(s, comp, &properties)?;
    s.refresh();
    // Report the solver's diagnostics. A resolved joint can still describe the wrong
    // connection after a source edit; the reminder above does not depend on solve errors.
    let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
    let diagnostics =
        |rows: &[(u64, String, String)]| rows.iter().map(|(id, name, why)| json!({"joint": id, "name": name, "why": why})).collect::<Vec<_>>();
    for (_, name, why) in sol.errors.iter().chain(&sol.conflicts) {
        warnings.push(format!("joint {name}: {why}"));
    }
    let previous_path = stored(s.path.as_deref(), &resolve(s.path.as_deref(), &link.path));
    Ok(json!({"component": comp, "feature": link.feature, "path": path, "previous_path": previous_path, "geometry_changed": geometry_changed,
        "warnings": warnings, "joint_errors": diagnostics(&sol.errors), "joint_conflicts": diagnostics(&sol.conflicts)}))
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.break_link";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc_mut().links.remove(&comp).ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    Ok(json!({"component": comp, "path": stored(s.path.as_deref(), &resolve(s.path.as_deref(), &link.path))}))
}

fn list_links(s: &mut Session, _p: &Value) -> Result<Value> {
    let links: Vec<Value> = s
        .doc
        .links
        .iter()
        .map(|(c, l)| {
            let file = resolve(s.path.as_deref(), &l.path);
            let name = s.doc.components.iter().find(|x| x.id == *c).map(|x| x.name.clone());
            let path = stored(s.path.as_deref(), &file);
            let file = file.to_string_lossy();
            // A refused path is not looked for at all: on Windows asking whether a file on
            // `\\host\share` exists already connects to the host.
            match link_refusal(s.path.as_deref(), &l.path) {
                None => {
                    let found = solidcraft_io::vfs::design_path_is_file(&file, s.path.as_deref());
                    // Whether the file is no longer what this design last read (#9739): `null`
                    // when it isn't found, or the link keeps no digest (made before they were).
                    let changed = l
                        .file_sha256
                        .as_ref()
                        .filter(|_| found == Some(true))
                        .and_then(|kept| file_sha256(Path::new(file.as_ref()), s.path.as_deref()).map(|now| now != *kept));
                    json!({"component": c, "name": name, "path": path, "file": file, "found": found, "changed": changed})
                }
                Some(solidcraft_io::vfs::Remote::Network(host)) => json!({"component": c, "name": name, "path": path, "file": file,
                    "found": null, "host": host, "reason": format!("on the network location \\\\{host}, which is not allowed")}),
                Some(solidcraft_io::vfs::Remote::Device) => json!({"component": c, "name": name, "path": path, "file": file,
                    "found": null, "reason": "names a device, not a file"}),
            }
        })
        .collect();
    Ok(json!({"links": links}))
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
                    assert_eq!(absolute(Path::new(links["links"][0]["file"].as_str().unwrap())), absolute(&part));
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
        std::fs::write(recovery.join(format!("{}.solidcraft", entry.id)), solidcraft_io::write_design(&s.design())).unwrap();
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
        // Relink to a network path (programmatic: no prompt) is refused before the disk is asked.
        let calls = solidcraft_io::vfs::file_calls();
        let err = a.execute("component.relink", &json!({"component": comp, "path": r"\\evil.example\share\x.solidcraft"})).unwrap_err().to_string();
        assert!(err.contains("network location"), "{err}");
        assert_eq!(solidcraft_io::vfs::file_calls(), calls);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every place that reaches a file through a link path goes through the design-path rule
    /// (`vfs::check_design_path` / `read_design_path` / `design_path_is_file` / `link_refusal`):
    /// no plain file call on a link's path in the link commands, and the font file a design's text
    /// names is checked before it is read.
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
        let fonts = std::fs::read_to_string(root.join("../sketch/src/fonts.rs")).unwrap();
        let (check, read) = (fonts.find("FILE_CHECK.get()").unwrap(), fonts.find("read_font(&path)").unwrap());
        assert!(check < read, "a font file named by a design is checked before it is read");
        let io = std::fs::read_to_string(root.join("../io/src/vfs.rs")).unwrap();
        assert!(io.contains("set_font_file_check(|p| check_design_path("), "the font check is the design-path rule");
    }
}
