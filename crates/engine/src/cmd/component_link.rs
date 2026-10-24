//! Linked designs: a component whose bodies come from another design file (Insert Design).
//!
//! The linked design is opened and evaluated on its own, and its solid bodies, as placed in it,
//! join a new component through an Import feature (exact B-rep, kept in this design so it opens
//! without the part file). The link remembers the file, relative to this design's folder once it
//! is saved; Update Links reads the file again and replaces the Import feature's bodies, and
//! Break Link keeps the bodies and forgets the file. The bodies' faces are named after the names
//! the design gives them (`Feature::link_faces`), so a joint on a linked face stays on that face
//! when the design changes, as it would inside the design, and reports it when the face is gone.

use std::path::{Component as PathPart, Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_doc::{DesignLink, Document, FeatureKind};

use super::CommandSpec;
use crate::params::{bad, str_, vec3};
use crate::{EngineError, Result, Session};

#[path = "component_extract.rs"]
mod extract;

#[cfg(test)]
#[path = "component_relink_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod relink_tests;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.insert_design", "Insert Design", insert_design).at("SOLID", "ASSEMBLE").icon("component").params(
        "path: a .solidcraft design → a new component linked to it, holding its solid bodies as placed in that design; \
         name? (default: the design's name); parent?: component id or name (default: the active one); translate?: [x,y,z]; \
         reuse?: bool (default false): insert a free occurrence, reusing an existing link to the same file when available",
    ),
    CommandSpec::new("component.update_links", "Update Links", update_links).icon("update").params(
        "component?: id or name (default: every linked component) — reads the linked designs again and replaces their bodies where they changed; \
         → updated, unchanged, warnings (also joints whose faces are gone or that no longer solve)",
    ),
    CommandSpec::new("component.relink", "Relink Source", relink).icon("update").params(
        "component: id or name of a linked component; path: a saved .solidcraft design — replaces the source and linked bodies for all its occurrences, preserving identities and placements; returns path, previous_path, warnings, joint_errors and joint_conflicts",
    ),
    CommandSpec::new("component.break_link", "Break Link", break_link)
        .icon("break")
        .params("component: id or name of a linked component — keeps its bodies as they are and forgets the design file"),
    CommandSpec::new("component.extract", "Extract to File", extract::extract).icon("export").params(
        "component: id or name of a component that is not linked; path: the .solidcraft design to write → its history, subcomponents and their joints \
         move to that design, and the component becomes linked to it (as component.insert_design makes it); returns path, file, feature, bodies, \
         notices (what stays behind), warnings and copies (components that build the same bodies: see component.replace_with_link)",
    ),
    CommandSpec::new("component.copies", "Find Identical Copies", extract::copies).icon("inspect").noundo().params(
        "component: id or name of a linked component → copies: [{component, name}], the components not linked whose own history builds the same bodies",
    ),
    CommandSpec::new("component.replace_with_link", "Replace with Link", extract::replace_with_link).icon("chain").params(
        "component or components: [ids or names] of components that are not linked; with: a linked component, or path: a .solidcraft design \
         (its link here, else a new linked component) → each component's history goes and its placements become placements of the linked \
         component; refused when its bodies are not the same as the link's; returns component, name, replaced and warnings",
    ),
    CommandSpec::new("component.links", "List Links", list_links)
        .icon("chain")
        .noundo()
        .params("→ linked components: component, name, path, file (resolved), found"),
];

/// The folder relative link paths are kept against: the design's own, once it has a file.
fn base_dir(path: Option<&str>) -> Option<PathBuf> {
    path.and_then(|p| absolute(Path::new(p)).parent().map(Path::to_path_buf))
}

/// A path as given, made absolute when the platform can, with `.` and `..` worked out.
fn absolute(p: &Path) -> PathBuf {
    let full = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in full.components() {
        match c {
            PathPart::CurDir => {}
            PathPart::ParentDir if matches!(out.components().next_back(), Some(PathPart::Normal(_))) => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// `target` relative to the folder `base` (both absolute), when they share a root.
fn relative(base: &Path, target: &Path) -> Option<PathBuf> {
    let (b, t): (Vec<_>, Vec<_>) = (base.components().collect(), target.components().collect());
    let root = |c: &[PathPart]| c.iter().take_while(|x| matches!(x, PathPart::Prefix(_) | PathPart::RootDir)).count();
    if root(&b) == 0 || b.get(..root(&b)) != t.get(..root(&b)) {
        return None;
    }
    let common = b.iter().zip(&t).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in t.iter().skip(common) {
        out.push(c.as_os_str());
    }
    Some(out)
}

/// How a link stores `file` for a design saved at `design` (`None`: not saved yet).
fn stored(design: Option<&str>, file: &Path) -> String {
    let file = absolute(file);
    match base_dir(design).and_then(|d| relative(&d, &file)) {
        // Relative paths are written with `/`, so a design moves between systems with its parts.
        Some(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"),
        None => file.to_string_lossy().into_owned(),
    }
}

/// The file a stored link path names, for a design saved at `design`.
fn resolve(design: Option<&str>, link: &str) -> PathBuf {
    let p = Path::new(link);
    match base_dir(design) {
        Some(d) if p.is_relative() => d.join(p),
        _ => p.to_path_buf(),
    }
}

/// The links of `doc` stored for a design saved at `to` instead of at `from`.
pub(super) fn rebased(doc: &Document, from: Option<&str>, to: &str) -> std::collections::BTreeMap<u64, DesignLink> {
    doc.links.iter().map(|(c, l)| (*c, DesignLink { path: stored(Some(to), &resolve(from, &l.path)), feature: l.feature })).collect()
}

/// The bodies of a linked design: its name, the Import feature that holds them and the names
/// the design gives their faces (`Feature::link_faces`).
type Linked = (String, solidcraft_io::StepFeature, Vec<Vec<String>>);

/// The solid bodies of the design at `file`, evaluated, as an Import feature.
fn linked_bodies(file: &Path, own: Option<&str>) -> Result<Linked> {
    let shown = file.display().to_string();
    if own.is_some_and(|o| absolute(Path::new(o)) == absolute(file)) {
        return Err(EngineError::Other(format!("{shown}: a design can't be linked into itself")));
    }
    let bytes = solidcraft_io::vfs::read(&shown, solidcraft_io::MAX_DESIGN_BYTES).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    let doc = solidcraft_io::read_design(&bytes).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    session_bodies(&Session::new(doc), &shown)
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
    let step = solidcraft_io::export(&part.doc.painted(&world), &solids, solidcraft_io::Format::Step, &name)
        .map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    let mut feature = solidcraft_io::step_import_feature(&step, shown).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    if world.bodies.len() > solids.len() {
        feature.warnings.push(format!("{} mesh bodies of {shown} are not linked", world.bodies.len() - solids.len()));
    }
    // The STEP text keeps no face names: carry the design's own, so references into the linked
    // bodies follow them as they do inside the design (#3330).
    let faces = solidcraft_doc::naming::carried_face_names(&world, &feature.import.bodies);
    Ok((name, feature, faces))
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
    let (design_name, sf, faces) = linked_bodies(&file, s.path.as_deref())?;
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
    s.doc_mut().links.insert(comp, DesignLink { path: path.clone(), feature: id });
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.as_str()).collect();
    Ok(
        json!({"component": comp, "occurrence": occ, "name": name, "feature": id, "path": path, "bodies": bodies, "created_bodies": bodies, "warnings": sf.warnings}),
    )
}

fn update_links(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.update_links";
    let only = if p.get("component").is_some() { Some(component_key(s, p, "component", cmd)?) } else { None };
    if let Some(c) = only
        && !s.doc.links.contains_key(&c)
    {
        return Err(bad(cmd, "that component is not linked to a design"));
    }
    let links: Vec<(u64, DesignLink)> = s.doc.links.iter().filter(|(c, _)| only.is_none_or(|o| o == **c)).map(|(c, l)| (*c, l.clone())).collect();
    let (mut updated, mut unchanged, mut warnings) = (Vec::new(), Vec::new(), Vec::new());
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
        let Some(FeatureKind::Import { step: old, .. }) = s.doc.feature(link.feature).filter(|f| f.component == comp).map(|f| &f.kind) else {
            return Err(bad(
                cmd,
                format!("{cname}: its linked bodies (feature {}) are gone; break the link or insert the design again", link.feature),
            ));
        };
        let old = old.clone();
        let file = resolve(s.path.as_deref(), &link.path);
        let (_, sf, faces) = linked_bodies(&file, s.path.as_deref()).map_err(|e| bad(cmd, format!("{cname}: {e}")))?;
        warnings.extend(sf.warnings.iter().cloned());
        let same = matches!(&sf.kind, FeatureKind::Import { step, .. } if *step == old);
        let kept = s.doc.feature(link.feature).map(|f| &f.link_faces);
        if same && kept == Some(&faces) {
            unchanged.push(cname);
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
        if changed { updated.push(cname) } else { unchanged.push(cname) }
    }
    solidcraft_doc::progress::item(None);
    s.refresh();
    if let Some(e) = s.doc.links.values().find_map(|l| s.model.result(l.feature).and_then(|r| r.error.clone())) {
        return Err(EngineError::Other(e));
    }
    // A reference whose face the design no longer has says so here, not only in the joint list.
    if !updated.is_empty() {
        let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        warnings.extend(sol.errors.iter().chain(&sol.conflicts).map(|(_, name, why)| format!("joint {name}: {why}")));
    }
    Ok(json!({"updated": updated, "unchanged": unchanged, "warnings": warnings}))
}

/// The joints and joint origins on occurrences of `comp` that name its linked faces by their
/// place in the file (`F<feature>:face:<k>`, made before the design's own names were kept), which
/// an update can give to another face.
fn numbered_references(s: &Session, comp: u64, feature: u64) -> Vec<String> {
    let numbered = format!("F{feature}:face:");
    let on = |o: &solidcraft_doc::joints::JointOrigin| {
        s.doc.occurrences.iter().any(|x| x.id == o.occurrence && x.component == comp)
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
    if !matches!(s.doc.feature(link.feature), Some(f) if f.component == comp && matches!(f.kind, FeatureKind::Import { .. })) {
        return Err(bad(cmd, "the linked bodies are gone; break the link or insert the design again"));
    }
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let file = absolute(Path::new(given));
    if !file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("solidcraft")) {
        return Err(bad(cmd, "choose a saved .solidcraft design"));
    }
    // The ordinary lexical check in linked_bodies also works in the browser VFS. On native
    // hosts, reject an alias of this assembly too (a symlink or a path with different case).
    if let (Ok(candidate), Some(Ok(own))) = (file.canonicalize(), s.path.as_deref().map(std::fs::canonicalize))
        && candidate == own
    {
        return Err(bad(cmd, "a design can't be linked into itself"));
    }
    // Read/evaluate the candidate before changing anything. Session::execute owns the single
    // undo step and restores both the document and evaluated state on any later failure.
    let (_, sf, faces) = linked_bodies(&file, s.path.as_deref()).map_err(|e| bad(cmd, e.to_string()))?;
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
    s.doc_mut().links.insert(comp, DesignLink { path: path.clone(), feature: link.feature });
    s.refresh();
    if let Some(error) = s.model.result(link.feature).and_then(|r| r.error.clone()) {
        return Err(bad(cmd, error));
    }
    // Report the solver's diagnostics. A resolved joint can still describe the wrong
    // connection after a source edit; the reminder above does not depend on solve errors.
    let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
    let diagnostics =
        |rows: &[(u64, String, String)]| rows.iter().map(|(id, name, why)| json!({"joint": id, "name": name, "why": why})).collect::<Vec<_>>();
    for (_, name, why) in sol.errors.iter().chain(&sol.conflicts) {
        warnings.push(format!("joint {name}: {why}"));
    }
    Ok(json!({"component": comp, "feature": link.feature, "path": path, "previous_path": link.path, "geometry_changed": geometry_changed,
        "warnings": warnings, "joint_errors": diagnostics(&sol.errors), "joint_conflicts": diagnostics(&sol.conflicts)}))
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.break_link";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc_mut().links.remove(&comp).ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    Ok(json!({"component": comp, "path": link.path}))
}

fn list_links(s: &mut Session, _p: &Value) -> Result<Value> {
    let links: Vec<Value> = s
        .doc
        .links
        .iter()
        .map(|(c, l)| {
            let file = resolve(s.path.as_deref(), &l.path);
            let name = s.doc.components.iter().find(|x| x.id == *c).map(|x| x.name.clone());
            json!({"component": c, "name": name, "path": l.path, "file": file.display().to_string(), "found": file.is_file()})
        })
        .collect();
    Ok(json!({"links": links}))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn link_paths_are_kept_relative_to_the_design_and_follow_save_as() {
        let root = absolute(Path::new("projects"));
        let part = root.join("parts").join("plate.solidcraft");
        let design = root.join("assembly").join("case.solidcraft");
        let design = design.to_string_lossy().into_owned();
        assert_eq!(stored(Some(&design), &part), "../parts/plate.solidcraft");
        assert_eq!(absolute(&resolve(Some(&design), "../parts/plate.solidcraft")), part);
        // A design path given relative to the working folder works the same.
        assert_eq!(stored(Some("case.solidcraft"), &absolute(Path::new("parts/plate.solidcraft"))), "parts/plate.solidcraft");
        // Before the design has a file the path is kept whole; saving it makes it relative.
        let unsaved = stored(None, &part);
        assert!(Path::new(&unsaved).is_absolute(), "{unsaved}");
        let mut doc = Document::default();
        doc.links.insert(7, DesignLink { path: unsaved, feature: 3 });
        let moved = rebased(&doc, None, &design);
        assert_eq!(moved.get(&7).map(|l| l.path.as_str()), Some("../parts/plate.solidcraft"));
        let beside = root.join("parts").join("asm.solidcraft").to_string_lossy().into_owned();
        assert_eq!(
            rebased(&Document { links: moved, ..Document::default() }, Some(&design), &beside).get(&7).map(|l| l.path.as_str()),
            Some("plate.solidcraft")
        );
    }

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
}
