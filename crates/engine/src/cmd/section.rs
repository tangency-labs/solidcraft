//! Section Analysis: view-only cuts through the model by a plane (what is on the plane's
//! positive side is hidden). A design keeps any number of them (`Document::section_analyses`,
//! saved with it); at most one is shown at a time, and the session's `section` is that one's
//! cut, worked out again after every command, so a cut on a construction plane or a planar
//! face follows it when the model changes (#3977). They change no geometry; making, changing,
//! showing and hiding them are undo steps, as hiding and showing browser items is (#2219).

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use solidcraft_doc::section_analysis::{MAX_SECTIONS, SectionAnalyses, SectionAnalysis, SectionPlane};
use solidcraft_doc::{Document, ModelState};
use solidcraft_geom::{Plane, Vec3};

use super::CommandSpec;
use crate::params::{bad, bool_, expr, str_, vec3};
use crate::schema::section::{
    SectionCut, SectionDeleteParams, SectionDeleteResult, SectionEditParams, SectionEntry, SectionListParams, SectionListResult, SectionParams,
    SectionRenameParams, SectionRenameResult, SectionResult, SectionVisibilityParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::status_items::StatusId;
use crate::{Result, Session};

const SECTION_CONTRACT: TypedContract<SectionParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<SectionEditParams> = TypedContract::new();
const VISIBILITY_CONTRACT: TypedContract<SectionVisibilityParams> = TypedContract::new();
const RENAME_CONTRACT: TypedContract<SectionRenameParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<SectionDeleteParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<SectionListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("inspect.section", "Section Analysis", crate::checked!("inspect.section", section))
        .at("SOLID", "INSPECT")
        .icon("section")
        .contract(&SECTION_CONTRACT)
        .output(output_schema_for::<SectionResult>)
        .params(
            "plane: XY|XZ|YZ | construction plane name or id | {face: [x,y,z], body?, index?} (a planar face) | {origin, normal}; offset?: expr along the plane's normal (XY: +Z, XZ: +Y, YZ: +X); \
         at?: expr, the cut's world coordinate on the named plane's axis (XY: Z, XZ: Y, YZ: X), instead of offset; \
         flip?: bool; hatch?: bool (hatch the cut faces, in each body's colour; default true); name?; or clear: true (hide the section shown). \
         Makes a new section and shows it (the one shown before is hidden and kept); a cut on a construction plane or face follows it. \
         replace?: bool (default false): change the section shown in place instead (none shown: make one), and with clear delete it; \
         Returns the cut's origin and normal, and for a named plane its axis and at",
        ),
    CommandSpec::new("inspect.section.edit", "Edit Section Analysis", crate::checked!("inspect.section.edit", edit))
        .icon("section")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<SectionResult>)
        .params("section: id or name; plane?, offset?, at?, flip?, hatch?, name? as inspect.section (left out: kept). Shows it"),
    CommandSpec::new("inspect.section.visibility", "Show/Hide Section Analysis", crate::checked!("inspect.section.visibility", visibility))
        .icon("eye")
        .contract(&VISIBILITY_CONTRACT)
        .output(output_schema_for::<SectionResult>)
        .params("visible: bool; section?: id or name (showing one hides the other; without it, the Analysis folder)"),
    CommandSpec::new("inspect.section.rename", "Rename Section Analysis", crate::checked!("inspect.section.rename", rename))
        .icon("rename")
        .contract(&RENAME_CONTRACT)
        .output(output_schema_for::<SectionRenameResult>)
        .params("section: id or name; name"),
    CommandSpec::new("inspect.section.delete", "Delete Section Analysis", crate::checked!("inspect.section.delete", delete))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<SectionDeleteResult>)
        .params("section: id or name"),
    CommandSpec::new("inspect.section.list", "List Section Analyses", crate::checked!("inspect.section.list", list))
        .icon("list")
        .noundo()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<SectionListResult>)
        .params("→ sections: [{id, name, plane, offset, flip, hatch, shown, origin?, normal?, error?}], folder_hidden"),
];

/// A cut: a point of its plane and the unit normal of the side it hides.
pub type Cut = (Vec3, Vec3);

/// The default name of a new section, `name(n)` for the first free `n` (the UI passes its
/// translated `Section {n}`).
pub fn free_name(doc: &Document, name: impl Fn(usize) -> String) -> String {
    doc.section_analyses.free_name(name)
}

fn english_name(n: usize) -> String {
    format!("Section {n}")
}

/// Where Section Analysis `id` cuts now, or why it cannot (its plane is gone or not flat);
/// `None` when there is no such section.
pub fn status(s: &Session, id: u64) -> Option<std::result::Result<Cut, String>> {
    let sec = s.doc.section_analyses.get(id)?;
    Some(resolve(s, sec))
}

/// Faces found for the model and world they were found in: finding a face meshes its body, so
/// it is done once per model, not once per command or frame (an open dialog's preview, the
/// browser's rows).
#[derive(Default)]
pub(crate) struct Cache {
    key: Option<(Arc<ModelState>, Arc<ModelState>)>,
    found: Vec<(SectionPlane, std::result::Result<Cut, String>)>,
}

pub(crate) type SharedCache = Arc<Mutex<Cache>>;

/// The cut a section makes in this model: its plane found where it is now, moved `offset` along
/// the plane's normal, and turned round by `flip`.
fn resolve(s: &Session, sec: &SectionAnalysis) -> std::result::Result<Cut, String> {
    let (origin, normal) = base_plane(s, &sec.plane)?;
    let offset = if sec.offset.trim().is_empty() {
        0.0
    } else {
        s.doc.eval(&sec.offset, solidcraft_doc::expr::Kind::Length).map_err(|e| format!("distance: {e}"))?
    };
    if !origin.is_finite() || !offset.is_finite() || !normal.is_finite() {
        return Err("the plane must be finite".into());
    }
    let origin = origin + normal * offset;
    Ok((origin, if sec.flip { -normal } else { normal }))
}

fn base_plane(s: &Session, plane: &SectionPlane) -> std::result::Result<Cut, String> {
    match plane {
        SectionPlane::Origin { name } => Plane::named(name).map(|p| (p.origin, p.normal())).ok_or_else(|| format!("unknown plane `{name}`")),
        SectionPlane::Fixed { origin, normal } => {
            normal.normalized().map(|n| (*origin, n)).ok_or_else(|| "the plane needs a non-zero normal".to_string())
        }
        SectionPlane::Construction { feature } => {
            let name = s.doc.feature(*feature).map(|f| f.name.clone()).ok_or_else(|| "its construction plane was deleted".to_string())?;
            let model = s.model.state();
            let c = model.construct.iter().find(|c| c.feature == *feature);
            match c.map(|c| &c.geom) {
                Some(solidcraft_doc::construct::ConstructGeom::Plane(pl)) => Ok((pl.origin, pl.normal())),
                _ => Err(format!("construction plane `{name}` is not built (rolled back, suppressed or failed)")),
            }
        }
        SectionPlane::Face { body, face, at } => {
            let (model, world) = (s.model.state(), s.world_state());
            let same = |c: &Cache| c.key.as_ref().is_some_and(|(m, w)| Arc::ptr_eq(m, &model) && Arc::ptr_eq(w, &world));
            if let Ok(c) = s.section_cache.lock()
                && same(&c)
                && let Some((_, r)) = c.found.iter().find(|(x, _)| x == plane)
            {
                return r.clone();
            }
            let r = face_plane(&world, body, face, *at);
            if let Ok(mut c) = s.section_cache.lock() {
                if !same(&c) {
                    c.key = Some((model, world));
                    c.found.clear();
                }
                if c.found.len() >= 64 {
                    drop(c.found.remove(0));
                }
                c.found.push((plane.clone(), r.clone()));
            }
            r
        }
    }
}

/// A planar face by its persistent name: on its body, or on any body when that is gone (renamed
/// or split off), the piece nearest where it was picked.
fn face_plane(st: &ModelState, body: &str, face: &str, at: Vec3) -> std::result::Result<Cut, String> {
    use solidcraft_doc::naming;
    let hit = st.body(body).filter(|b| !b.body.is_mesh()).and_then(|b| naming::face_on_body(b, face, at).map(|(i, p, _)| (b, i, p)));
    let hit = hit.or_else(|| {
        let h = naming::locate_face(st, face, at)?;
        st.bodies.get(h.body).map(|b| (b, h.face, h.point))
    });
    let (b, index, point) = hit.ok_or_else(|| format!("the face `{face}` it was on no longer exists"))?;
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let faces = b.body.faces(tol).map_err(|e| format!("the face `{face}` cannot be read: {e}"))?;
    let n = faces.iter().find(|f| f.index == index).and_then(|f| f.plane_normal).ok_or_else(|| format!("the face `{face}` is no longer flat"))?;
    Ok((point, n))
}

/// Bring the session's cut (`Session::section`, read by the view, picking, exports and the
/// CLI) in line with the design's shown section. A session whose design has no sections leaves
/// it alone, so a cut set some other way stays.
pub(crate) fn sync(s: &mut Session) {
    if s.doc.section_analyses.is_empty() {
        if s.section_derived {
            s.section_derived = false;
            if s.section.is_some() {
                s.section = None;
                s.revision += 1;
            }
            s.status_slot(StatusId::Section, false);
        }
        return;
    }
    s.section_derived = true;
    let visible = s.doc.section_analyses.visible().cloned();
    let (cut, hatch) = match &visible {
        Some(sec) => (resolve(s, sec).ok(), sec.hatch),
        None => (None, s.section_hatch),
    };
    if s.section != cut || s.section_hatch != hatch {
        s.section = cut;
        s.section_hatch = hatch;
        s.revision += 1;
    }
    // The viewport's status list keeps effects in the order they were turned on (#8545): a
    // section shown keeps its slot when another one replaces it, and leaves it when none is.
    s.status_slot(StatusId::Section, s.section.is_some());
}

/// A section shown on request is the newest effect in the viewport's status list (#8547).
fn activated(s: &mut Session) {
    if s.section.is_some() {
        s.status_activated(StatusId::Section);
    }
}

/// The design's sections, to change: a change that only touches them keeps the placed world
/// (the world cache follows the new design at once, so the cut worked out next reads it).
fn change<T>(s: &mut Session, f: impl FnOnce(&mut SectionAnalyses) -> T) -> T {
    let before = s.doc.clone();
    let out = f(&mut s.doc_mut().section_analyses);
    s.retarget_world_cache(&before);
    out
}

/// The section `section` names: its id (a number, or digits) or its name.
fn find(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let all = &s.doc.section_analyses;
    let found = match p.get("section") {
        Some(Value::Number(n)) => n.as_u64().and_then(|id| all.get(id)),
        Some(Value::String(name)) => {
            all.items.iter().find(|x| x.name == *name).or_else(|| name.trim().parse::<u64>().ok().and_then(|id| all.get(id)))
        }
        _ => return Err(bad(cmd, "`section` must be a section's id or name")),
    };
    found.map(|x| x.id).ok_or_else(|| bad(cmd, format!("no section analysis {}", p.get("section").map(Value::to_string).unwrap_or_default())))
}

fn check_name(all: &SectionAnalyses, name: &str, own: Option<u64>, cmd: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 128 {
        return Err(bad(cmd, "`name` must be 1…128 characters"));
    }
    if all.items.iter().any(|x| x.name == name && Some(x.id) != own) {
        return Err(bad(cmd, format!("another section is called `{name}`")));
    }
    Ok(name.to_string())
}

/// The `plane` parameter as a section keeps it, and for XY, XZ and YZ the world axis its normal
/// lies along.
fn plane_of(s: &Session, v: &Value, cmd: &str) -> Result<SectionPlane> {
    match v {
        Value::String(n) if Plane::named(n.trim()).is_some() => {
            // Apply the same ambiguity check as modelling commands before treating a name as an origin alias.
            super::features::plane_param(s, Some(v), cmd)?;
            let pl = Plane::named(n.trim()).ok_or_else(|| bad(cmd, format!("unknown plane `{n}`")))?;
            let name = ["XY", "XZ", "YZ"].into_iter().find(|x| Plane::named(x).is_some_and(|p| p.normal() == pl.normal())).unwrap_or("XY");
            Ok(SectionPlane::Origin { name: name.into() })
        }
        // A construction plane (name or id): followed where it is built.
        Value::String(_) | Value::Number(_) => match super::features::plane_param(s, Some(v), cmd)? {
            solidcraft_doc::PlaneRef::Construction { feature } => Ok(SectionPlane::Construction { feature: feature.0 }),
            _ => Err(bad(cmd, "`plane` must be a plane")),
        },
        Value::Object(o) if o.contains_key("face") => face_of(s, v, cmd),
        Value::Object(o) => {
            let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
            let normal =
                o.get("normal").and_then(vec3).and_then(|n| n.normalized()).ok_or_else(|| bad(cmd, "the plane needs a non-zero `normal`"))?;
            Ok(SectionPlane::Fixed { origin, normal })
        }
        _ => Err(bad(cmd, "`plane` must be XY, XZ, YZ, a construction plane name or id, {face: [x, y, z]} or {origin, normal}")),
    }
}

/// A planar face picked by a point on it (and its body and index, as the view picks it): kept
/// by its persistent name, so the cut follows the face.
fn face_of(s: &Session, v: &Value, cmd: &str) -> Result<SectionPlane> {
    let point = v.get("face").and_then(vec3).filter(|p| p.is_finite()).ok_or_else(|| bad(cmd, "`face` must be a point [x, y, z] on the face"))?;
    let st = s.world_state();
    let body = match v.get("body") {
        None | Some(Value::Null) => None,
        Some(Value::String(b)) => Some(st.body(b).ok_or_else(|| bad(cmd, format!("no body `{b}`")))?),
        Some(_) => return Err(bad(cmd, "`body` must be a body name")),
    };
    let index = match v.get("index") {
        None | Some(Value::Null) => None,
        Some(i) => Some(i.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(cmd, "`index` must be a face index"))?),
    };
    let (b, index) = match (body, index) {
        (Some(b), Some(i)) => (b, i),
        (Some(b), None) => (b, solidcraft_doc::appearance::face_index_at(b, point).ok_or_else(|| bad(cmd, "no face of that body is at the point"))?),
        (None, Some(_)) => return Err(bad(cmd, "a face `index` needs its `body`")),
        (None, None) => st
            .bodies
            .iter()
            .filter_map(|b| {
                solidcraft_doc::appearance::nearest_face(b, point)
                    .filter(|(d, _)| *d <= solidcraft_doc::appearance::pick_tolerance(b))
                    .map(|(d, i)| (d, b, i))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, b, i)| (b, i))
            .ok_or_else(|| bad(cmd, "no face is at the point"))?,
    };
    let name = if b.body.is_mesh() { None } else { solidcraft_doc::naming::face_names(b).get(index).filter(|n| !n.is_empty()).cloned() };
    if let Some(face) = name {
        // Found as the section will find it from now on (and kept for that).
        let plane = SectionPlane::Face { body: b.name.clone(), face, at: point };
        base_plane(s, &plane).map_err(|_| bad(cmd, "the face must be planar"))?;
        return Ok(plane);
    }
    // A face with no name (a mesh): where it is now.
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let faces = b.body.faces(tol).map_err(|e| bad(cmd, format!("the face cannot be read: {e}")))?;
    let f = faces.iter().find(|f| f.index == index).ok_or_else(|| bad(cmd, format!("body `{}` has no face {index}", b.name)))?;
    let normal = f.plane_normal.ok_or_else(|| bad(cmd, "the face must be planar"))?;
    Ok(SectionPlane::Fixed { origin: point, normal })
}

/// The world axis a named plane's normal lies along.
fn axis_of(plane: &SectionPlane) -> Option<usize> {
    let SectionPlane::Origin { name } = plane else { return None };
    let n = Plane::named(name)?.normal();
    [n.x, n.y, n.z].iter().position(|c| c.abs() > 0.5)
}

/// The `offset` (or `at`) parameter as the expression a section keeps; `None` when neither is given.
fn offset_of(s: &Session, p: &Value, plane: &SectionPlane, cmd: &str) -> Result<Option<String>> {
    let length = |key: &str| -> Result<Option<(String, f64)>> {
        match expr(p, key) {
            Some(e) => {
                let v = s.doc.eval(&e, solidcraft_doc::expr::Kind::Length).map_err(|e| bad(cmd, format!("{key}: {e}")))?;
                if !v.is_finite() {
                    return Err(bad(cmd, "the plane must be finite"));
                }
                Ok(Some((e.into_string(), v)))
            }
            None => Ok(None),
        }
    };
    let (offset, at) = (length("offset")?, length("at")?);
    match (offset, at, axis_of(plane)) {
        (Some(_), Some(_), _) => Err(bad(cmd, "give either `offset` (along the normal) or `at` (a world coordinate), not both")),
        (_, Some(_), None) => Err(bad(cmd, "`at` needs a named plane (XY, XZ, YZ); move an {origin, normal} plane by its origin")),
        // The named planes pass through the world origin, so the offset is `at` signed by the normal.
        (_, Some((_, at)), Some(i)) => {
            let n = Plane::named(match plane {
                SectionPlane::Origin { name } => name,
                _ => "XY",
            })
            .map(|p| p.normal())
            .unwrap_or(Vec3::Z);
            Ok(Some(format!("{} mm", at * axis_component(n, i).signum())))
        }
        (Some((e, _)), None, _) => Ok(Some(e)),
        (None, None, _) => Ok(None),
    }
}

fn axis_component(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// The reply of the commands that change what is shown: the cut shown now.
fn reply(s: &Session, id: Option<u64>) -> Result<Value> {
    let section = match (s.doc.section_analyses.visible(), s.section) {
        (Some(sec), Some((origin, normal))) => {
            let axis = axis_of(&sec.plane);
            Some(SectionCut {
                id: sec.id,
                name: sec.name.clone(),
                origin: [origin.x, origin.y, origin.z],
                normal: [normal.x, normal.y, normal.z],
                hatch: sec.hatch,
                axis: axis.map(|i| ["X", "Y", "Z"].get(i).copied().unwrap_or("?").to_string()),
                at: axis.map(|i| axis_component(origin, i)),
            })
        }
        _ => None,
    };
    serde_json::to_value(SectionResult { section, id, param_warnings: Vec::new() }).map_err(|e| bad("inspect.section", e.to_string()))
}

/// A section as given, checked: it must cut now (its plane is built, its distance evaluates).
fn checked_cut(s: &Session, sec: &SectionAnalysis, cmd: &str) -> Result<()> {
    resolve(s, sec).map(|_| ()).map_err(|e| bad(cmd, e))
}

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section";
    // One cut at a time, as before a design kept several (#3977): the app asks for it when the
    // user's preference says so. It only steers this run; the design keeps what it has.
    let replace = bool_(p, "replace").unwrap_or(false);
    let shown = s.doc.section_analyses.shown.filter(|id| s.doc.section_analyses.get(*id).is_some());
    if bool_(p, "clear").unwrap_or(false) {
        match shown {
            Some(id) if replace => drop(discard(s, id)),
            Some(_) => change(s, |all| all.shown = None),
            None => {}
        }
        sync(s);
        return Ok(json!({"section": null}));
    }
    let plane = plane_of(s, p.get("plane").unwrap_or(&Value::Null), cmd)?;
    let offset = offset_of(s, p, &plane, cmd)?.unwrap_or_default();
    let all = &s.doc.section_analyses;
    let replaced = shown.filter(|_| replace).and_then(|id| all.get(id));
    if replaced.is_none() && all.items.len() >= MAX_SECTIONS {
        return Err(bad(cmd, "too many section analyses"));
    }
    let own = replaced.map(|x| x.id);
    let name = match (str_(p, "name"), replaced) {
        (Some(n), _) => check_name(all, n, own, cmd)?,
        (None, Some(x)) => x.name.clone(),
        (None, None) => all.free_name(english_name),
    };
    let id = own.unwrap_or_else(|| all.next_id());
    let flip = bool_(p, "flip").unwrap_or(false);
    let sec = SectionAnalysis { id, name, plane, offset, flip, hatch: bool_(p, "hatch").unwrap_or(true) };
    checked_cut(s, &sec, cmd)?;
    change(s, |all| {
        match all.get_mut(id) {
            Some(x) => *x = sec,
            None => all.items.push(sec),
        }
        all.shown = Some(id);
        all.folder_hidden = false;
    });
    sync(s);
    activated(s);
    reply(s, Some(id))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.edit";
    let id = find(s, p, cmd)?;
    let Some(mut sec) = s.doc.section_analyses.get(id).cloned() else { return Err(bad(cmd, "no such section analysis")) };
    if let Some(v) = p.get("plane").filter(|v| !v.is_null()) {
        sec.plane = plane_of(s, v, cmd)?;
    }
    if let Some(offset) = offset_of(s, p, &sec.plane, cmd)? {
        sec.offset = offset;
    }
    if let Some(flip) = bool_(p, "flip") {
        sec.flip = flip;
    }
    if let Some(hatch) = bool_(p, "hatch") {
        sec.hatch = hatch;
    }
    if let Some(n) = str_(p, "name") {
        sec.name = check_name(&s.doc.section_analyses, n, Some(id), cmd)?;
    }
    checked_cut(s, &sec, cmd)?;
    let all = &s.doc.section_analyses;
    if all.get(id) != Some(&sec) || all.shown != Some(id) || all.folder_hidden {
        change(s, |all| {
            if let Some(x) = all.get_mut(id) {
                *x = sec;
            }
            all.shown = Some(id);
            all.folder_hidden = false;
        });
    }
    sync(s);
    activated(s);
    reply(s, Some(id))
}

fn visibility(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.visibility";
    let visible = bool_(p, "visible").ok_or_else(|| bad(cmd, "`visible` must be true or false"))?;
    let id = match p.get("section").filter(|v| !v.is_null()) {
        Some(_) => Some(find(s, p, cmd)?),
        None => None,
    };
    let all = &s.doc.section_analyses;
    match id {
        Some(id) => {
            let shown = if visible {
                Some(id)
            } else if all.shown == Some(id) {
                None
            } else {
                all.shown
            };
            if shown != all.shown {
                change(s, |all| all.shown = shown);
            }
        }
        None => {
            if all.items.is_empty() {
                return Err(bad(cmd, "there is no section analysis"));
            }
            if all.folder_hidden == visible {
                change(s, |all| all.folder_hidden = !visible);
            }
        }
    }
    sync(s);
    if visible {
        activated(s);
    }
    reply(s, id)
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.rename";
    let id = find(s, p, cmd)?;
    let name = check_name(&s.doc.section_analyses, str_(p, "name").unwrap_or_default(), Some(id), cmd)?;
    if s.doc.section_analyses.get(id).is_some_and(|x| x.name != name) {
        change(s, |all| {
            if let Some(x) = all.get_mut(id) {
                x.name.clone_from(&name);
            }
        });
    }
    serde_json::to_value(SectionRenameResult { id, name, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.delete";
    let id = find(s, p, cmd)?;
    let gone = discard(s, id).ok_or_else(|| bad(cmd, "no such section analysis"))?;
    sync(s);
    serde_json::to_value(SectionDeleteResult { deleted: gone.name, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}

/// Take section `id` out of the design; `None` when there is no such section.
fn discard(s: &mut Session, id: u64) -> Option<SectionAnalysis> {
    let i = s.doc.section_analyses.items.iter().position(|x| x.id == id)?;
    Some(change(s, |all| {
        let gone = all.items.remove(i);
        if all.shown == Some(id) {
            all.shown = None;
        }
        // The last one gone, the Analysis folder goes with it (and its eye with it).
        if all.items.is_empty() {
            *all = SectionAnalyses::default();
        }
        gone
    }))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "inspect.section.list";
    let all = s.doc.section_analyses.clone();
    let sections = all
        .items
        .iter()
        .map(|sec| {
            let r = resolve(s, sec);
            let (cut, error) = match r {
                Ok(c) => (Some(c), None),
                Err(e) => (None, Some(e)),
            };
            SectionEntry {
                id: sec.id,
                name: sec.name.clone(),
                plane: serde_json::to_value(&sec.plane).unwrap_or(Value::Null),
                offset: sec.offset.clone(),
                flip: sec.flip,
                hatch: sec.hatch,
                shown: all.shown == Some(sec.id),
                origin: cut.map(|(o, _)| [o.x, o.y, o.z]),
                normal: cut.map(|(_, n)| [n.x, n.y, n.z]),
                error,
            }
        })
        .collect();
    serde_json::to_value(SectionListResult { sections, folder_hidden: all.folder_hidden, param_warnings: Vec::new() })
        .map_err(|e| bad(cmd, e.to_string()))
}
