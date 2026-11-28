//! Section Analysis: a view-only cut through the model by up to six planes. Each plane hides
//! what lies on its positive side, so the part kept is what every plane keeps: two square to
//! each other keep a quarter. It changes no geometry; each change is an undo step of its own.

use serde_json::{Value, json};
use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::{bad, bool_, expr, str_};
use crate::schema::{TypedContract, output_schema_for, section as contract};
use crate::view::MAX_SECTION_PLANES;
use crate::{Result, SectionPlane, Session};
use solidcraft_doc::saved_sections::{MAX_SAVED_SECTIONS, SavedSection, SavedSectionPlane};
const SECTION: TypedContract<contract::SectionParams> = TypedContract::new();
const SAVE: TypedContract<contract::SectionSaveParams> = TypedContract::new();
const SHOW: TypedContract<contract::SectionNamedParams> = TypedContract::new();
const RENAME: TypedContract<contract::SectionRenameParams> = TypedContract::new();
const DELETE: TypedContract<contract::SectionNamedParams> = TypedContract::new();
const LIST: TypedContract<contract::SectionListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("inspect.section.save", "Save Section Analysis", crate::checked!("inspect.section.save", save))
        .icon("section")
        .contract(&SAVE)
        .output(output_schema_for::<contract::SectionSaved>)
        .params("name: what to call the Section Analysis on show (its planes, hidden ones too, and its hatch); a saved section of that name is replaced. Saved with the design"),
    CommandSpec::new("inspect.section.show", "Show Saved Section Analysis", crate::checked!("inspect.section.show", show))
        .icon("section")
        .contract(&SHOW)
        .output(output_schema_for::<contract::SectionShown>)
        .params("section: a saved section's name; it replaces the section on show, so the planes follow it. One undo step"),
    CommandSpec::new("inspect.section.rename", "Rename Saved Section Analysis", crate::checked!("inspect.section.rename", rename))
        .icon("rename")
        .contract(&RENAME)
        .output(output_schema_for::<contract::SectionSaved>)
        .params("section: a saved section's name, name: the new one"),
    CommandSpec::new("inspect.section.delete", "Delete Saved Section Analysis", crate::checked!("inspect.section.delete", delete))
        .icon("delete")
        .contract(&DELETE)
        .output(output_schema_for::<contract::SectionDeleted>)
        .params("section: a saved section's name; the section on show stays as it is"),
    CommandSpec::new("inspect.section.list", "List Saved Section Analyses", crate::checked!("inspect.section.list", list))
        .icon("list")
        .noundo()
        .contract(&LIST)
        .output(output_schema_for::<contract::SectionListResult>)
        .doc_only(),
    CommandSpec::new("inspect.section", "Section Analysis", crate::checked!("inspect.section", section))
    .contract(&SECTION).output(output_schema_for::<contract::SectionResult>)
    .at("SOLID", "INSPECT").icon("section").params(
        "plane: XY|XZ|YZ | construction plane name or id | {origin, normal}; offset?: expr, how far the cut moves against the plane's \
         normal as given (before any flip), so a positive distance cuts more away: into the part from a face's outward plane \
         (XY: -Z, XZ: -Y, YZ: -X; negative moves it the other way); \
         at?: expr, the cut's world coordinate on the named plane's axis (XY: Z, XZ: Y, YZ: X), instead of offset; \
         flip?: bool (hide the other side; the plane stays where it is); hatch?: bool (hatch the cut faces, in each body's colour; default true, or as it was with add or replace); \
         add?: bool (add the plane to the section, starting one if there is none, instead of replacing it: each plane hides what \
         lies on its positive side, so the section keeps only what every plane keeps, a quarter for two square planes; at most 6 planes, hidden ones included; \
         the new plane takes the lowest id no plane has); \
         replace?: a plane's id (move that plane to `plane`, keeping its id and the others; a hidden one is shown); or remove: a plane's id \
         (take that plane out; the last turns the section off); or hide / show: a plane's id or [ids] (a hidden plane stays in the \
         section and cuts nothing until shown); or folder: false (hide the Analysis folder: the planes on show stop cutting and \
         come back with folder: true; while it is hidden, hide and show change only which planes come back, and a plane added or \
         moved shows it again, with them); or clear: true (all planes). Each change is one undo step; the design is untouched. \
         Returns section: the plane made {id, origin, normal, hatch, and for a named plane axis and at} \
         (null when the section is off, or for remove, hide, show and folder), planes: [{id, origin, normal, hidden}], every plane \
         of the section by id, and folder: null, or while the folder is hidden the ids that come back with it",
    )];

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let before: Vec<u64> = s.section.iter().map(|c| c.id).collect();
    let made = change(s, p)?;
    // A plane on show shows the folder again, with the planes it hid.
    if !s.section.is_empty()
        && let Some(back) = s.section_folder.take()
    {
        set_shown(s, &back, true);
    }
    if let Some(back) = s.section_folder.as_mut() {
        back.retain(|id| s.section_hidden.iter().any(|c| c.id == *id));
        if s.section_hidden.is_empty() {
            s.section_folder = None;
        }
    }
    s.status_slot(crate::status_items::StatusId::Section, !s.section.is_empty());
    // A plane made or brought on show makes the section the newest effect.
    if !made.is_null() || s.section.iter().any(|c| !before.contains(&c.id)) {
        s.status_activated(crate::status_items::StatusId::Section);
    }
    Ok(reply(s, made))
}

/// Move the planes `ids` names between those on show and the hidden ones.
fn set_shown(s: &mut Session, ids: &[u64], shown: bool) {
    let (from, to) = if shown { (&mut s.section_hidden, &mut s.section) } else { (&mut s.section, &mut s.section_hidden) };
    to.extend(from.iter().filter(|c| ids.contains(&c.id)).copied());
    from.retain(|c| !ids.contains(&c.id));
    to.sort_by_key(|c| c.id);
}

/// The change `p` asks for; the plane made, or null.
fn change(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section";
    if bool_(p, "clear").unwrap_or(false) {
        s.section.clear();
        s.section_hidden.clear();
        s.refresh_display();
        return Ok(Value::Null);
    }
    let given = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    let add = match p.get("add") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`add` must be true or false"))?,
    };
    if given("folder") {
        if ["plane", "replace", "remove", "hide", "show"].iter().any(|k| given(k)) || add {
            return Err(bad(cmd, "`folder` is given alone, without `plane`, `add`, `replace`, `remove`, `hide` or `show`"));
        }
        let shown = bool_(p, "folder").ok_or_else(|| bad(cmd, "`folder` must be true or false"))?;
        match (shown, s.section_folder.take()) {
            (true, Some(back)) => set_shown(s, &back, true),
            (false, None) => {
                let on: Vec<u64> = s.section.iter().map(|c| c.id).collect();
                set_shown(s, &on, false);
                s.section_folder = Some(on);
            }
            (_, back) => s.section_folder = back,
        }
        s.refresh_display();
        return Ok(Value::Null);
    }
    if given("hide") || given("show") {
        if (given("hide") && given("show")) || ["plane", "replace", "remove"].iter().any(|k| given(k)) || add {
            return Err(bad(cmd, "`hide` and `show` take plane ids alone: give one of them without `plane`, `add`, `replace` or `remove`"));
        }
        let (key, hide) = if given("hide") { ("hide", true) } else { ("show", false) };
        let ids = plane_ids(s, p.get(key), key, cmd)?;
        // While the folder is hidden, a plane's eye changes only whether it comes back with it.
        if let Some(back) = s.section_folder.as_mut() {
            back.retain(|id| !ids.contains(id));
            if !hide {
                back.extend(&ids);
                back.sort_unstable();
                back.dedup();
            }
        } else {
            set_shown(s, &ids, !hide);
        }
        s.refresh_display();
        return Ok(Value::Null);
    }
    let id_of = |key: &str| -> Result<Option<u64>> {
        match p.get(key).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be the id of one of the section's planes"))),
        }
    };
    let replace = id_of("replace")?;
    if replace.is_some() && (add || given("remove")) {
        return Err(bad(cmd, "`replace` moves one plane: give it without `add` or `remove`"));
    }
    let known = |s: &Session, id: u64| s.section.iter().chain(&s.section_hidden).any(|c| c.id == id);
    if let Some(id) = replace
        && !known(s, id)
    {
        return Err(bad(cmd, format!("the section has no plane {id}")));
    }
    if let Some(v) = p.get("remove").filter(|v| !v.is_null()) {
        if p.get("plane").is_some() || add {
            return Err(bad(cmd, "give either a `plane` to cut with or the `remove` id of one, not both"));
        }
        let id = v.as_u64().ok_or_else(|| bad(cmd, "`remove` must be the id of one of the section's planes"))?;
        if !known(s, id) {
            return Err(bad(cmd, format!("the section has no plane {id}")));
        }
        s.section.retain(|c| c.id != id);
        s.section_hidden.retain(|c| c.id != id);
        s.refresh_display();
        return Ok(Value::Null);
    }
    let (origin, normal, axis) = plane(s, p, cmd)?;
    let length = |key: &str| -> Result<Option<f64>> {
        match expr(p, key) {
            Some(e) => s.doc.eval(&e, solidcraft_doc::expr::Kind::Length).map(Some).map_err(|e| bad(cmd, format!("{key}: {e}"))),
            None => Ok(None),
        }
    };
    let (offset, at) = (length("offset")?, length("at")?);
    let offset = match (offset, at, axis) {
        (Some(_), Some(_), _) => return Err(bad(cmd, "give either `offset` (along the normal) or `at` (a world coordinate), not both")),
        (_, Some(_), None) => return Err(bad(cmd, "`at` needs a named plane (XY, XZ, YZ); move an {origin, normal} plane by its origin")),
        // The named planes pass through the world origin, and the offset runs against the normal.
        (_, Some(at), Some(i)) => -at * axis_component(normal, i).signum(),
        (offset, None, _) => offset.unwrap_or(0.0),
    };
    let origin = origin - normal * offset;
    let normal = if bool_(p, "flip").unwrap_or(false) { -normal } else { normal };
    if !origin.is_finite() || !normal.is_finite() || (normal.len() - 1.0).abs() > 1e-6 {
        return Err(bad(cmd, "the plane must be finite"));
    }
    let hatch = match bool_(p, "hatch") {
        Some(h) => h,
        None if add || replace.is_some() => s.section_hatch,
        None => true,
    };
    let tol = 1e-9 * (1.0 + origin.len());
    let twice = |s: &Session| {
        s.section
            .iter()
            .chain(&s.section_hidden)
            .any(|c| Some(c.id) != replace && (c.normal - normal).len() <= 1e-9 && c.normal.dot(origin - c.origin).abs() <= tol)
    };
    if (add || replace.is_some()) && twice(s) {
        return Err(bad(cmd, "the section already cuts with this plane"));
    }
    let id = if let Some(id) = replace {
        s.section_hidden.retain(|c| c.id != id);
        match s.section.iter_mut().find(|c| c.id == id) {
            Some(c) => (c.origin, c.normal) = (origin, normal),
            None => {
                s.section.push(SectionPlane { id, origin, normal });
                s.section.sort_by_key(|c| c.id);
            }
        }
        id
    } else if add {
        if s.section.len() + s.section_hidden.len() >= MAX_SECTION_PLANES {
            return Err(bad(cmd, format!("a section cuts with at most {MAX_SECTION_PLANES} planes; remove one first")));
        }
        let id = free_id(s);
        s.section.push(SectionPlane { id, origin, normal });
        s.section.sort_by_key(|c| c.id);
        id
    } else {
        s.section.clear();
        s.section_hidden.clear();
        s.section.push(SectionPlane { id: 1, origin, normal });
        1
    };
    s.section_hatch = hatch;
    s.refresh_display();
    let mut made = json!({"id": id, "origin": origin, "normal": normal, "hatch": hatch});
    if let Some(i) = axis {
        made["axis"] = json!(["X", "Y", "Z"].get(i).copied().unwrap_or("?"));
        made["at"] = json!(axis_component(origin, i));
    }
    Ok(made)
}

/// The lowest id no plane of the section has (a section with room for one more always has one):
/// ids stay within 1 to [`MAX_SECTION_PLANES`], so no two planes share a hatch angle
/// (`solidcraft_viewport::section_hatch_of`).
fn free_id(s: &Session) -> u64 {
    (1..=MAX_SECTION_PLANES as u64).find(|id| !s.section.iter().chain(&s.section_hidden).any(|c| c.id == *id)).unwrap_or(1)
}

/// The ids `hide` or `show` names: one id or a list, each one of the section's planes.
fn plane_ids(s: &Session, v: Option<&Value>, key: &str, cmd: &str) -> Result<Vec<u64>> {
    let one = |v: &Value| v.as_u64().ok_or_else(|| bad(cmd, format!("`{key}` must be a plane's id or a list of them")));
    let ids = match v {
        Some(Value::Array(a)) => a.iter().map(one).collect::<Result<Vec<u64>>>()?,
        Some(v) => vec![one(v)?],
        None => Vec::new(),
    };
    match ids.iter().find(|id| !s.section.iter().chain(&s.section_hidden).any(|c| c.id == **id)) {
        Some(id) => Err(bad(cmd, format!("the section has no plane {id}"))),
        None => Ok(ids),
    }
}

/// The reply: the plane made (or null) and every plane of the section, by id.
fn reply(s: &Session, made: Value) -> Value {
    let mut all: Vec<(&SectionPlane, bool)> = s.section.iter().map(|c| (c, false)).chain(s.section_hidden.iter().map(|c| (c, true))).collect();
    all.sort_by_key(|(c, _)| c.id);
    let planes: Vec<Value> = all.iter().map(|(c, hidden)| json!({"id": c.id, "origin": c.origin, "normal": c.normal, "hidden": hidden})).collect();
    let made = if s.section.is_empty() && s.section_hidden.is_empty() { Value::Null } else { made };
    json!({"section": made, "planes": planes, "folder": s.section_folder})
}

/// The `plane` parameter: a point on it, its unit normal and, for a named plane, the world axis
/// along its normal.
fn plane(s: &Session, p: &Value, cmd: &str) -> Result<(Vec3, Vec3, Option<usize>)> {
    Ok(match p.get("plane") {
        Some(Value::String(n)) if Plane::named(n.trim()).is_some() => {
            // Apply the same ambiguity check as modelling commands before treating a name as an origin alias.
            super::features::plane_param(s, p.get("plane"), cmd)?;
            let pl = Plane::named(n.trim()).ok_or_else(|| bad(cmd, format!("unknown plane `{n}`")))?;
            let normal = pl.normal();
            let axis = [normal.x, normal.y, normal.z].iter().position(|c| c.abs() > 0.5);
            (pl.origin, normal, axis)
        }
        // A construction plane (name or id): where it stands in the world, its component placed,
        // as the viewport draws it. The cut and the reply are in the world.
        Some(v @ (Value::String(_) | Value::Number(_))) => {
            let r = super::features::plane_param(s, Some(v), cmd)?;
            let solidcraft_doc::PlaneRef::Construction { feature, .. } = &r else { return Err(bad(cmd, "`plane` must be a plane")) };
            let st = s.world_state()?;
            let c = st
                .construct
                .iter()
                .find(|c| c.feature == feature.0)
                .ok_or_else(|| bad(cmd, format!("construction plane `{feature}` is not built (rolled back, suppressed or failed)")))?;
            let solidcraft_doc::construct::ConstructGeom::Plane(pl) = &c.geom else {
                return Err(bad(cmd, format!("construction plane `{}` is not built (rolled back, suppressed or failed)", c.name)));
            };
            (pl.origin, pl.normal(), None)
        }
        Some(o @ Value::Object(_)) => {
            let origin = match o.get("origin") {
                None => Vec3::ZERO,
                Some(v) => crate::params::vec3(v).ok_or_else(|| bad(cmd, "the plane's `origin` must be [x, y, z]"))?,
            };
            let n = o
                .get("normal")
                .and_then(crate::params::vec3)
                .and_then(|n| n.normalized())
                .ok_or_else(|| bad(cmd, "the plane needs a non-zero `normal`"))?;
            (origin, n, None)
        }
        _ => return Err(bad(cmd, "`plane` must be XY, XZ, YZ, a construction plane name or id, or {origin, normal}")),
    })
}

fn axis_component(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Every plane of the section by id, hidden ones included.
fn plane_states(s: &Session) -> Vec<contract::SectionPlaneState> {
    let mut all: Vec<(&SectionPlane, bool)> = s.section.iter().map(|c| (c, false)).chain(s.section_hidden.iter().map(|c| (c, true))).collect();
    all.sort_by_key(|(c, _)| c.id);
    all.iter().map(|(c, hidden)| contract::SectionPlaneState { id: c.id, origin: arr(c.origin), normal: arr(c.normal), hidden: *hidden }).collect()
}

/// A name from the parameter `key`: trimmed, 1 to 128 characters.
fn name_of(p: &Value, key: &str, cmd: &str) -> Result<String> {
    let name = str_(p, key).map(str::trim).unwrap_or_default();
    if SavedSection::name_is_valid(name) { Ok(name.to_string()) } else { Err(bad(cmd, format!("`{key}` must be 1…128 characters"))) }
}

/// The index of the saved section called `name`.
fn saved_index(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let name = str_(p, "section").ok_or_else(|| bad(cmd, "`section` must be the name of a saved section"))?;
    s.doc.saved_sections.iter().position(|x| x.name == name).ok_or_else(|| bad(cmd, format!("no saved section `{name}`")))
}

/// What a saved section shows, or why it can't be shown (a design file can hold anything).
fn state_of(saved: &SavedSection) -> std::result::Result<crate::SectionState, String> {
    if saved.planes.len() > MAX_SECTION_PLANES {
        return Err(format!("the saved section `{}` has more than {MAX_SECTION_PLANES} planes", saved.name));
    }
    let mut state = crate::SectionState { hatch: saved.hatch, ..Default::default() };
    for (i, c) in saved.planes.iter().enumerate() {
        let id_ok = c.id >= 1 && c.id <= MAX_SECTION_PLANES as u64 && !saved.planes.iter().take(i).any(|o| o.id == c.id);
        if !id_ok || !c.origin.is_finite() || !c.normal.is_finite() || (c.normal.len() - 1.0).abs() > 1e-6 {
            return Err(format!("the saved section `{}` holds a plane that can't be shown", saved.name));
        }
        let plane = SectionPlane { id: c.id, origin: c.origin, normal: c.normal };
        if c.hidden { state.hidden.push(plane) } else { state.planes.push(plane) }
    }
    state.planes.sort_by_key(|c| c.id);
    state.hidden.sort_by_key(|c| c.id);
    Ok(state)
}

/// Saved planes are copies of the planes shown, so equal means the stored values are the same.
fn same_planes(a: &[SectionPlane], b: &[SectionPlane]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.id == b.id && a.origin == b.origin && a.normal == b.normal)
}

/// Keep the section on show under a name, replacing a saved section of that name.
fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.save";
    let name = name_of(p, "name", cmd)?;
    // While the folder is hidden, the planes that come back with it are the ones that show.
    let back: Vec<u64> = s.section_folder.clone().unwrap_or_default();
    let mut planes: Vec<SavedSectionPlane> = s
        .section
        .iter()
        .map(|c| (c, false))
        .chain(s.section_hidden.iter().map(|c| (c, !back.contains(&c.id))))
        .map(|(c, hidden)| SavedSectionPlane { id: c.id, origin: c.origin, normal: c.normal, hidden })
        .collect();
    planes.sort_by_key(|c| c.id);
    if planes.is_empty() {
        return Err(bad(cmd, "there is no Section Analysis to save: make a section first"));
    }
    let known = s.doc.saved_sections.iter().position(|x| x.name == name);
    if known.is_none() && s.doc.saved_sections.len() >= MAX_SAVED_SECTIONS {
        return Err(bad(cmd, "too many saved sections"));
    }
    let count = planes.len();
    let saved = SavedSection { name: name.clone(), planes, hatch: s.section_hatch };
    let d = s.doc_mut();
    match known {
        Some(i) => {
            if let Some(slot) = d.saved_sections.get_mut(i) {
                *slot = saved;
            }
        }
        None => d.saved_sections.push(saved),
    }
    serde_json::to_value(contract::SectionSaved { section: name, planes: count, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}

/// Show a saved section in place of the one on show.
fn show(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.show";
    let i = saved_index(s, p, cmd)?;
    let saved = s.doc.saved_sections.get(i).cloned().ok_or_else(|| bad(cmd, "no such saved section"))?;
    let state = state_of(&saved).map_err(|m| bad(cmd, m))?;
    s.set_section_state(state);
    serde_json::to_value(contract::SectionShown { section: saved.name, planes: plane_states(s), hatch: s.section_hatch, param_warnings: Vec::new() })
        .map_err(|e| bad(cmd, e.to_string()))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.rename";
    let i = saved_index(s, p, cmd)?;
    let name = name_of(p, "name", cmd)?;
    if s.doc.saved_sections.iter().enumerate().any(|(j, x)| j != i && x.name == name) {
        return Err(bad(cmd, format!("another saved section is called `{name}`")));
    }
    let mut count = 0;
    if let Some(x) = s.doc_mut().saved_sections.get_mut(i) {
        x.name = name.clone();
        count = x.planes.len();
    }
    serde_json::to_value(contract::SectionSaved { section: name, planes: count, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section.delete";
    let i = saved_index(s, p, cmd)?;
    let gone = s.doc_mut().saved_sections.remove(i);
    serde_json::to_value(contract::SectionDeleted { deleted: gone.name, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "inspect.section.list";
    let on = (s.section_folder.is_none() && !(s.section.is_empty() && s.section_hidden.is_empty())).then(|| s.section_state());
    let shown = on.and_then(|on| {
        s.doc
            .saved_sections
            .iter()
            .find(|x| state_of(x).is_ok_and(|st| st.hatch == on.hatch && same_planes(&st.planes, &on.planes) && same_planes(&st.hidden, &on.hidden)))
            .map(|x| x.name.clone())
    });
    let sections = s
        .doc
        .saved_sections
        .iter()
        .map(|x| contract::SectionEntry {
            name: x.name.clone(),
            planes: x
                .planes
                .iter()
                .map(|c| contract::SectionPlaneState { id: c.id, origin: arr(c.origin), normal: arr(c.normal), hidden: c.hidden })
                .collect(),
            hatch: x.hatch,
        })
        .collect();
    serde_json::to_value(contract::SectionListResult { sections, shown, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}
