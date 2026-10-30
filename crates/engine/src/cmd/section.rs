//! Section Analysis: a view-only cut through the model by up to six planes. What lies on the
//! positive side of every plane is hidden: one plane hides its side, two cut a wedge (a quarter,
//! square to each other) away. It changes no geometry; each change is an undo step of its own.

use serde_json::{Value, json};
use solidcraft_geom::{Plane, Vec3};

use super::CommandSpec;
use crate::params::{bad, bool_, expr};
use crate::schema::{TypedContract, output_schema_for, section as contract};
use crate::view::MAX_SECTION_PLANES;
use crate::{Result, SectionPlane, Session};
const SECTION: TypedContract<contract::SectionParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new("inspect.section", "Section Analysis", crate::checked!("inspect.section", section))
    .contract(&SECTION).output(output_schema_for::<contract::SectionResult>)
    .at("SOLID", "INSPECT").icon("section").params(
        "plane: XY|XZ|YZ | construction plane name or id | {origin, normal}; offset?: expr, how far the cut moves against the plane's \
         normal as given (before any flip), so a positive distance cuts more away: into the part from a face's outward plane \
         (XY: -Z, XZ: -Y, YZ: -X; negative moves it the other way); \
         at?: expr, the cut's world coordinate on the named plane's axis (XY: Z, XZ: Y, YZ: X), instead of offset; \
         flip?: bool (hide the other side; the plane stays where it is); hatch?: bool (hatch the cut faces, in each body's colour; default true, or as it was with add or replace); \
         add?: bool (add the plane to the section, starting one if there is none, instead of replacing it: the section hides what \
         lies on the positive side of every plane, so two square planes cut a quarter away; at most 6 planes, hidden ones included); \
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
        s.revision += 1;
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
        s.revision += 1;
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
        s.revision += 1;
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
        s.revision += 1;
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
        let id = s.section.iter().chain(&s.section_hidden).map(|c| c.id).max().unwrap_or(0) + 1;
        s.section.push(SectionPlane { id, origin, normal });
        id
    } else {
        s.section.clear();
        s.section_hidden.clear();
        s.section.push(SectionPlane { id: 1, origin, normal });
        1
    };
    s.section_hatch = hatch;
    s.revision += 1;
    let mut made = json!({"id": id, "origin": origin, "normal": normal, "hatch": hatch});
    if let Some(i) = axis {
        made["axis"] = json!(["X", "Y", "Z"].get(i).copied().unwrap_or("?"));
        made["at"] = json!(axis_component(origin, i));
    }
    Ok(made)
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
        // A construction plane (name or id): where it is now.
        Some(v @ (Value::String(_) | Value::Number(_))) => {
            let r = super::features::plane_param(s, Some(v), cmd)?;
            let solidcraft_doc::PlaneRef::Construction { feature } = &r else { return Err(bad(cmd, "`plane` must be a plane")) };
            let st = s.model.state();
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
