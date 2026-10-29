//! Section Analysis: a view-only cut through the model by up to six planes. What lies on the
//! positive side of every plane is hidden: one plane hides its side, two cut a wedge (a quarter,
//! square to each other) away. It changes no geometry and is not an undo step.

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
    .at("SOLID", "INSPECT").icon("section").noundo().params(
        "plane: XY|XZ|YZ | construction plane name or id | {origin, normal}; offset?: expr along the plane's normal (XY: +Z, XZ: +Y, YZ: +X); \
         at?: expr, the cut's world coordinate on the named plane's axis (XY: Z, XZ: Y, YZ: X), instead of offset; \
         flip?: bool; hatch?: bool (hatch the cut faces, in each body's colour; default true, or as it was with add); \
         add?: bool (add the plane to the section, starting one if there is none, instead of replacing it: the section hides what \
         lies on the positive side of every plane, so two square planes cut a quarter away; at most 6 planes); or remove: a plane's id (take that plane out; the last turns the section off); \
         or clear: true (all planes). Returns section: the plane made {id, origin, normal, hatch, and for a named plane axis and at} \
         (null when the section is off), and planes: [{id, origin, normal}], every plane of the section",
    )];

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section";
    if bool_(p, "clear").unwrap_or(false) {
        s.section.clear();
        s.status_slot(crate::status_items::StatusId::Section, false);
        s.revision += 1;
        return Ok(reply(s, Value::Null));
    }
    let add = match p.get("add") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`add` must be true or false"))?,
    };
    if let Some(v) = p.get("remove").filter(|v| !v.is_null()) {
        if p.get("plane").is_some() || add {
            return Err(bad(cmd, "give either a `plane` to cut with or the `remove` id of one, not both"));
        }
        let id = v.as_u64().ok_or_else(|| bad(cmd, "`remove` must be the id of one of the section's planes"))?;
        let at = s.section.iter().position(|c| c.id == id).ok_or_else(|| bad(cmd, format!("the section has no plane {id}")))?;
        s.section.remove(at);
        s.status_slot(crate::status_items::StatusId::Section, !s.section.is_empty());
        s.revision += 1;
        return Ok(reply(s, Value::Null));
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
        // The named planes pass through the world origin, so the offset is `at` signed by the normal.
        (_, Some(at), Some(i)) => at * axis_component(normal, i).signum(),
        (offset, None, _) => offset.unwrap_or(0.0),
    };
    let normal = if bool_(p, "flip").unwrap_or(false) { -normal } else { normal };
    let origin = origin + normal * offset * if bool_(p, "flip").unwrap_or(false) { -1.0 } else { 1.0 };
    if !origin.is_finite() || !normal.is_finite() || (normal.len() - 1.0).abs() > 1e-6 {
        return Err(bad(cmd, "the plane must be finite"));
    }
    let hatch = match bool_(p, "hatch") {
        Some(h) => h,
        None if add => s.section_hatch,
        None => true,
    };
    let id = if add {
        if s.section.len() >= MAX_SECTION_PLANES {
            return Err(bad(cmd, format!("a section cuts with at most {MAX_SECTION_PLANES} planes; remove one first")));
        }
        let tol = 1e-9 * (1.0 + origin.len());
        if s.section.iter().any(|c| (c.normal - normal).len() <= 1e-9 && c.normal.dot(origin - c.origin).abs() <= tol) {
            return Err(bad(cmd, "the section already cuts with this plane"));
        }
        s.section.iter().map(|c| c.id).max().unwrap_or(0) + 1
    } else {
        s.section.clear();
        1
    };
    s.section.push(SectionPlane { id, origin, normal });
    s.section_hatch = hatch;
    s.status_slot(crate::status_items::StatusId::Section, true);
    s.status_activated(crate::status_items::StatusId::Section);
    s.revision += 1;
    let mut made = json!({"id": id, "origin": origin, "normal": normal, "hatch": hatch});
    if let Some(i) = axis {
        made["axis"] = json!(["X", "Y", "Z"].get(i).copied().unwrap_or("?"));
        made["at"] = json!(axis_component(origin, i));
    }
    Ok(reply(s, made))
}

/// The reply: the plane made (or null) and every plane of the section.
fn reply(s: &Session, made: Value) -> Value {
    let planes: Vec<Value> = s.section.iter().map(|c| json!({"id": c.id, "origin": c.origin, "normal": c.normal})).collect();
    let made = if s.section.is_empty() { Value::Null } else { made };
    json!({"section": made, "planes": planes})
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
