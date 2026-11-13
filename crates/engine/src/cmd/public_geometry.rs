//! Public geometry: reference elements of a skeleton component (sketches, construction planes,
//! axes and points) shared as a named group. An element is published under the name its feature
//! has at the time; the feature id says which element it is, so renaming the feature later
//! changes nothing, and deleting it takes it out of the group.

use std::path::Path;

use serde_json::Value;
use solidcraft_doc::construct::{AxisDef, ConstructGeom, GeoRef, PointDef};
use solidcraft_doc::{DesignLink, FeatureKind, GeometryCopy, PlaneRef, PublicGeometry, PublishedItem};

use super::CommandSpec;
use super::component::reply;
use super::design_link::{LinkProblem, LinkState, absolute, link_presence, read_linked_file, resolve, same_file, stored};
use crate::params::{bad, str_};
use crate::schema::public_geometry::{
    CopiesParams, CopiesResult, CopyInfo, ImportParams, ImportResult, Publication, PublicationsParams, PublicationsResult, PublishParams,
    PublishResult, PublishedEntry, UnpublishParams, UnpublishResult, UpdateParams, UpdateResult, UpdatedCopy,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const MAX_ITEMS: usize = 10_000;
const MAX_NAME: usize = 128;

const PUBLISH_CONTRACT: TypedContract<PublishParams> = TypedContract::new();
const UNPUBLISH_CONTRACT: TypedContract<UnpublishParams> = TypedContract::new();
const PUBLICATIONS_CONTRACT: TypedContract<PublicationsParams> = TypedContract::new();
const IMPORT_CONTRACT: TypedContract<ImportParams> = TypedContract::new();
const UPDATE_CONTRACT: TypedContract<UpdateParams> = TypedContract::new();
const COPIES_CONTRACT: TypedContract<CopiesParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("geometry.publish", "Public Geometry", crate::checked!("geometry.publish", publish))
        .icon("construction")
        .contract(&PUBLISH_CONTRACT)
        .output(output_schema_for::<PublishResult>)
        .params("name: the public geometry's name (unique, up to 128 characters); items: [feature ids or names] of sketches and construction planes, axes or points of a skeleton component. Each is published under its feature's name"),
    CommandSpec::new("geometry.unpublish", "Remove Public Geometry", crate::checked!("geometry.unpublish", unpublish))
        .icon("construction")
        .contract(&UNPUBLISH_CONTRACT)
        .output(output_schema_for::<UnpublishResult>)
        .params("name: a public geometry's name; removes the whole group, the features stay"),
    CommandSpec::new("geometry.publications", "Public Geometries", crate::checked!("geometry.publications", publications))
        .icon("construction")
        .noundo()
        .contract(&PUBLICATIONS_CONTRACT)
        .output(output_schema_for::<PublicationsResult>)
        .params("path?: another design file, to list what it shares (default: this design) → publications: [{name, items: [{name, feature, type}]}], one per public geometry"),
    CommandSpec::new("geometry.import", "Copy Geometry from File", crate::checked!("geometry.import", import))
        .icon("construction")
        .contract(&IMPORT_CONTRACT)
        .output(output_schema_for::<ImportResult>)
        .params("path: the design file to copy from; publication: the public geometry to take; name?: the copy's name here. Its sketches, planes, axes and points become features of the active component, with fixed geometry, under their published names"),
    CommandSpec::new("geometry.update", "Update Geometry Copies", crate::checked!("geometry.update", update))
        .icon("construction")
        .contract(&UPDATE_CONTRACT)
        .output(output_schema_for::<UpdateResult>)
        .params("copies?: [copy names] (default: all) → copies: [{name, state, changed, added, lost, reason}]; reads each source file again"),
    CommandSpec::new("geometry.copies", "Geometry Copies", crate::checked!("geometry.copies", copies))
        .icon("construction")
        .noundo()
        .contract(&COPIES_CONTRACT)
        .output(output_schema_for::<CopiesResult>)
        .params("→ copies: [{name, file, publication, component, items, state, reason}]; state is up_to_date, out_of_date, missing, unreadable or not_allowed"),
];

/// A feature id or name as given in `items`.
fn item_key(v: &Value, cmd: &str) -> Result<String> {
    match v {
        Value::String(t) if !t.trim().is_empty() && t.len() <= 4096 => Ok(t.trim().to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(bad(cmd, "an item must be a feature id or name")),
    }
}

/// The publishable feature `item` names: its id and name. Refuses what cannot be published and
/// what already is.
fn publishable(s: &Session, cmd: &str, item: &str) -> Result<(u64, String)> {
    // An id names one feature; a name may be shared between skeletons, so the active
    // component's wins and anything still ambiguous is refused.
    let mut found: Vec<_> = s.doc.features.iter().filter(|f| f.id.to_string() == item || f.name.eq_ignore_ascii_case(item)).collect();
    if found.len() > 1 && found.iter().any(|f| f.id.to_string() == item) {
        found.retain(|f| f.id.to_string() == item);
    }
    if found.len() > 1 {
        let in_active: Vec<_> = found.iter().copied().filter(|f| f.component == s.active_component).collect();
        if in_active.len() == 1 {
            found = in_active;
        }
    }
    let f = match found.as_slice() {
        [] => return Err(bad(cmd, format!("no feature `{item}`"))),
        [f] => *f,
        _ => return Err(bad(cmd, format!("`{item}` is ambiguous: give its id"))),
    };
    if !s.doc.is_skeleton(f.component) {
        return Err(bad(cmd, format!("`{}` is not in a skeleton component: only skeleton elements can be published", f.name)));
    }
    if !f.kind.is_reference() {
        return Err(bad(cmd, format!("`{}` is not a sketch or a construction plane, axis or point", f.name)));
    }
    if let Some(g) = s.doc.public_geometry.iter().find(|g| g.items.iter().any(|i| i.feature == f.id)) {
        return Err(bad(cmd, format!("`{}` is already published in `{}`", f.name, g.name)));
    }
    Ok((f.id, f.name.clone()))
}

fn entries(s: &Session, items: &[PublishedItem]) -> Vec<PublishedEntry> {
    let kind = |id| s.doc.feature(id).map_or("", |f| f.kind.base_name()).to_string();
    items.iter().map(|i| PublishedEntry { name: i.name.clone(), feature: i.feature, kind: kind(i.feature) }).collect()
}

fn publish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "geometry.publish";
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` is required"))?;
    if name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(bad(cmd, format!("`name` must be plain text of at most {MAX_NAME} characters")));
    }
    if s.doc.public_geometry.iter().any(|g| g.name.eq_ignore_ascii_case(name)) {
        return Err(bad(cmd, format!("a public geometry `{name}` already exists")));
    }
    let raw = p.get("items").and_then(Value::as_array).filter(|a| !a.is_empty() && a.len() <= MAX_ITEMS);
    let raw = raw.ok_or_else(|| bad(cmd, "`items` must list the features to publish"))?;
    let mut items: Vec<PublishedItem> = Vec::with_capacity(raw.len());
    for v in raw {
        let (feature, own) = publishable(s, cmd, &item_key(v, cmd)?)?;
        if items.iter().any(|e| e.feature == feature) {
            return Err(bad(cmd, format!("`{own}` is listed twice")));
        }
        if items.iter().chain(s.doc.public_geometry.iter().flat_map(|g| g.items.iter())).any(|e| e.name.eq_ignore_ascii_case(&own)) {
            return Err(bad(cmd, format!("the name `{own}` is already published")));
        }
        items.push(PublishedItem { name: own, feature });
    }
    if s.doc.public_geometry.iter().map(|g| g.items.len()).sum::<usize>() + items.len() > MAX_ITEMS {
        return Err(bad(cmd, "too many published elements"));
    }
    let out = PublishResult { name: name.to_string(), items: entries(s, &items), param_warnings: Vec::new() };
    s.doc_mut().public_geometry.push(PublicGeometry { name: name.to_string(), items });
    reply(out)
}

fn unpublish(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").map(str::trim).unwrap_or_default();
    let at = s.doc.public_geometry.iter().position(|g| g.name.eq_ignore_ascii_case(name));
    let at = at.ok_or_else(|| bad("geometry.unpublish", format!("no public geometry `{name}`")))?;
    let gone = s.doc_mut().public_geometry.remove(at);
    reply(UnpublishResult { name: gone.name, removed: gone.items.len(), param_warnings: Vec::new() })
}

fn publications(s: &mut Session, p: &Value) -> Result<Value> {
    let list = |s: &Session| s.doc.public_geometry.iter().map(|g| Publication { name: g.name.clone(), items: entries(s, &g.items) }).collect();
    let publications = match str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096) {
        Some(given) => list(&open(s, &absolute(Path::new(given))).map_err(|e| bad("geometry.publications", e.reason))?.0),
        None => list(s),
    };
    reply(PublicationsResult { publications, param_warnings: Vec::new() })
}

/// The construction feature that holds `geom` as fixed geometry.
fn fixed(geom: &ConstructGeom) -> FeatureKind {
    match geom {
        ConstructGeom::Plane(plane) => FeatureKind::ConstructionPlane { plane: PlaneRef::Custom { plane: *plane } },
        ConstructGeom::Axis { origin, dir } => {
            FeatureKind::ConstructionAxis { def: AxisDef::TwoPoints { a: GeoRef::Point { p: *origin }, b: GeoRef::Point { p: *origin + *dir } } }
        }
        ConstructGeom::Point(p) => FeatureKind::ConstructionPoint { def: PointDef::At { point: GeoRef::Point { p: *p } } },
    }
}

/// The sketch feature that holds a solved sketch where it came to: its points and curves held
/// there, and nothing that reads the design it comes from (dimensions driven by that design's
/// parameters, projections of its bodies). Ids are kept, so what was built on a copy finds its
/// curves again after an update.
fn fixed_sketch(solved: &solidcraft_doc::SolvedSketch) -> FeatureKind {
    let mut sketch = solved.sketch.clone();
    sketch.constraints.clear();
    sketch.patterns.clear();
    sketch.links.clear();
    sketch.offsets.clear();
    sketch.freeform_offsets.clear();
    sketch.equations.clear();
    for p in &mut sketch.points {
        (p.fixed, p.link, p.exprs) = (true, None, None);
    }
    for c in &mut sketch.curves {
        (c.fixed, c.link) = (true, None);
    }
    FeatureKind::Sketch { plane: PlaneRef::Custom { plane: solved.plane }, sketch }
}

/// What a public geometry of a source design gives: the fixed feature of each element by
/// published name, and the names of the elements the source design could not build.
struct Taken {
    geometry: Vec<(String, FeatureKind)>,
    skipped: Vec<String>,
    digest: String,
    /// The public geometry's name as the source design spells it.
    publication: String,
}

fn problem(state: LinkState, reason: String) -> LinkProblem {
    LinkProblem { state, reason }
}

fn unreadable(file: &Path, e: String) -> LinkProblem {
    let shown = file.display().to_string();
    problem(LinkState::Unreadable, if e.starts_with(&shown) { e } else { format!("{shown}: {e}") })
}

/// Read the design at `file`: the design, and its file's digest.
fn open(s: &Session, file: &Path) -> std::result::Result<(Session, String), LinkProblem> {
    let own = s.path.as_deref();
    if own.is_some_and(|o| same_file(Path::new(o), file)) {
        return Err(unreadable(file, "a design can't copy geometry from itself".into()));
    }
    let bytes = read_linked_file(file, own)?;
    let source = super::component_link::linked_session(&file.display().to_string(), &bytes, s.validation());
    Ok((source.map_err(|e| unreadable(file, e.to_string()))?, DesignLink::file_digest(&bytes)))
}

/// Read the design at `file` and take its public geometry `publication`.
fn take(s: &Session, file: &Path, publication: &str) -> std::result::Result<Taken, LinkProblem> {
    let (source, digest) = open(s, file)?;
    let group = source.doc.public_geometry.iter().find(|g| g.name.eq_ignore_ascii_case(publication));
    let group = group.ok_or_else(|| unreadable(file, format!("no public geometry `{publication}`")))?;
    let world = source.world_state().map_err(|e| unreadable(file, e.to_string()))?;
    let (mut geometry, mut skipped) = (Vec::new(), Vec::new());
    for item in &group.items {
        let construct = world.construct.iter().find(|c| c.feature == item.feature).map(|c| fixed(&c.geom));
        match construct.or_else(|| world.sketches.iter().find(|k| k.feature == item.feature).map(|k| fixed_sketch(k))) {
            Some(kind) => geometry.push((item.name.clone(), kind)),
            None => skipped.push(item.name.clone()),
        }
    }
    Ok(Taken { geometry, skipped, digest, publication: group.name.clone() })
}

/// Add the feature `kind` to `component` under the published name `item`, or under
/// `item (copy)` where a feature here already has that name (most designs have a `Sketch1`).
fn add(s: &mut Session, cmd: &str, component: u64, copy: &str, item: &str, kind: &FeatureKind) -> Result<u64> {
    let free = |n: &str| !s.doc.features.iter().any(|f| f.name.eq_ignore_ascii_case(n));
    let name = if free(item) { item.to_string() } else { format!("{item} ({copy})") };
    if !free(&name) {
        return Err(bad(cmd, format!("a feature named `{name}` already exists here: rename it")));
    }
    if matches!(kind, FeatureKind::ConstructionPlane { .. }) {
        super::features::check_plane_name(&name, cmd)?;
    }
    let id = s.doc_mut().add_feature(kind.clone(), Some(&name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = component;
    }
    Ok(id)
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "geometry.import";
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let publication = str_(p, "publication").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`publication` is required"))?;
    if s.doc.geometry_copies.len() >= MAX_ITEMS {
        return Err(bad(cmd, "too many geometry copies"));
    }
    let file = absolute(Path::new(given));
    let taken = take(s, &file, publication).map_err(|e| bad(cmd, e.reason))?;
    let publication = taken.publication.as_str();
    let taken_name = |n: &str| s.doc.geometry_copies.iter().any(|c| c.name.eq_ignore_ascii_case(n));
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.chars().count() > MAX_NAME || n.chars().any(char::is_control) => {
            return Err(bad(cmd, format!("`name` must be plain text of at most {MAX_NAME} characters")));
        }
        Some(n) if taken_name(n) => return Err(bad(cmd, format!("a geometry copy `{n}` already exists"))),
        Some(n) => n.to_string(),
        None => (1..=MAX_ITEMS)
            .map(|k| if k == 1 { publication.to_string() } else { format!("{publication} ({k})") })
            .find(|n| !taken_name(n))
            .ok_or_else(|| bad(cmd, "too many geometry copies"))?,
    };
    if taken.geometry.is_empty() {
        return Err(bad(cmd, format!("`{publication}` shares nothing the source design could build: nothing to copy")));
    }
    let component = s.active_component;
    let mut items = Vec::with_capacity(taken.geometry.len());
    for (item, kind) in &taken.geometry {
        items.push(PublishedItem { name: item.clone(), feature: add(s, cmd, component, &name, item, kind)? });
    }
    let out = ImportResult {
        name: name.clone(),
        file: file.display().to_string(),
        publication: publication.to_string(),
        items: entries(s, &items),
        skipped: taken.skipped,
        param_warnings: Vec::new(),
    };
    let copy = GeometryCopy {
        name,
        file: stored(s.path.as_deref(), &file),
        publication: publication.to_string(),
        file_sha256: Some(taken.digest),
        component,
        items,
    };
    s.doc_mut().geometry_copies.push(copy);
    reply(out)
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "geometry.update";
    let asked: Option<Vec<&str>> = match p.get("copies") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) if a.len() <= MAX_ITEMS => {
            Some(a.iter().map(|v| v.as_str().map(str::trim)).collect::<Option<Vec<_>>>().ok_or_else(|| bad(cmd, "`copies` must list copy names"))?)
        }
        _ => return Err(bad(cmd, "`copies` must list copy names")),
    };
    if let Some(unknown) = asked.iter().flatten().find(|n| !s.doc.geometry_copies.iter().any(|c| c.name.eq_ignore_ascii_case(n))) {
        return Err(bad(cmd, format!("no geometry copy `{unknown}`")));
    }
    let wanted = |name: &str| asked.as_ref().is_none_or(|a| a.iter().any(|n| n.eq_ignore_ascii_case(name)));
    let picked: Vec<GeometryCopy> = s.doc.geometry_copies.iter().filter(|c| wanted(&c.name)).cloned().collect();
    let mut report = Vec::with_capacity(picked.len());
    for mut copy in picked {
        let file = resolve(s.path.as_deref(), &copy.file);
        let (mut changed, mut added) = (Vec::new(), Vec::new());
        let taken = match take(s, &file, &copy.publication) {
            Ok(taken) => taken,
            Err(e) => {
                let (state, reason) = (e.state.word().to_string(), Some(e.reason));
                report.push(UpdatedCopy { name: copy.name, state, changed, added, lost: Vec::new(), reason });
                continue;
            }
        };
        for (item, kind) in &taken.geometry {
            let held = copy.items.iter().find(|i| i.name.eq_ignore_ascii_case(item)).map(|i| i.feature);
            match held.filter(|id| s.doc.feature(*id).is_some()) {
                Some(id) => {
                    if s.doc.feature(id).is_some_and(|f| f.kind != *kind) {
                        if let Some(f) = s.doc_mut().feature_mut(id) {
                            f.kind = kind.clone();
                        }
                        changed.push(item.clone());
                    }
                }
                None => {
                    let feature = add(s, cmd, copy.component, &copy.name, item, kind)?;
                    copy.items.retain(|i| !i.name.eq_ignore_ascii_case(item));
                    copy.items.push(PublishedItem { name: item.clone(), feature });
                    added.push(item.clone());
                }
            }
        }
        let offered = |name: &str| taken.geometry.iter().map(|(n, _)| n).chain(&taken.skipped).any(|n| n.eq_ignore_ascii_case(name));
        let lost: Vec<String> = copy.items.iter().filter(|i| !offered(&i.name)).map(|i| i.name.clone()).collect();
        copy.file_sha256 = Some(taken.digest);
        let state = if changed.is_empty() && added.is_empty() { "unchanged" } else { "updated" }.to_string();
        let name = copy.name.clone();
        if let Some(kept) = s.doc_mut().geometry_copies.iter_mut().find(|c| c.name == copy.name) {
            *kept = copy;
        }
        report.push(UpdatedCopy { name, state, changed, added, lost, reason: None });
    }
    reply(UpdateResult { copies: report, param_warnings: Vec::new() })
}

fn copies(s: &mut Session, _: &Value) -> Result<Value> {
    let own = s.path.as_deref();
    let copies = s
        .doc
        .geometry_copies
        .iter()
        .map(|c| {
            let file = resolve(own, &c.file);
            let (state, reason) = match link_presence(&file, own) {
                Some(e) => (e.state, Some(e.reason)),
                None if super::component_link::file_sha256(&file, own) == c.file_sha256 => (LinkState::UpToDate, None),
                None => (LinkState::OutOfDate, None),
            };
            CopyInfo {
                name: c.name.clone(),
                file: file.display().to_string(),
                publication: c.publication.clone(),
                component: c.component,
                items: entries(s, &c.items),
                state: state.word().to_string(),
                reason,
            }
        })
        .collect();
    reply(CopiesResult { copies, param_warnings: Vec::new() })
}

#[cfg(test)]
#[path = "public_geometry_tests.rs"]
mod tests;
