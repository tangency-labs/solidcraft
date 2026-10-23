//! Section Analysis: a view-only cut through the model by a plane (what is on the plane's
//! positive side is hidden). It changes no geometry and is not an undo step.

use serde_json::{Value, json};
use solidcraft_geom::{Plane, Vec3};

use super::CommandSpec;
use crate::params::{bad, bool_, expr, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new("inspect.section", "Section Analysis", section).at("SOLID", "INSPECT").icon("section").noundo().params(
        "plane: XY|XZ|YZ | construction plane name or id | {origin, normal}; offset?: expr along the plane's normal (XY: +Z, XZ: +Y, YZ: +X); \
         at?: expr, the cut's world coordinate on the named plane's axis (XY: Z, XZ: Y, YZ: X), instead of offset; \
         flip?: bool; second?: {plane, offset?, at?, flip?} for a perpendicular quarter section (remove either plane's positive side); \
         or clear: true. Returns the cut's origin and normal, and for a named plane its axis and at",
    )];

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.section";
    if bool_(p, "clear").unwrap_or(false) {
        s.section = None;
        s.section_second = None;
        s.revision += 1;
        return Ok(json!({"section": null}));
    }
    let second = match p.get("second") {
        None | Some(Value::Null) => None,
        Some(second) if second.is_object() && second.get("second").is_none() && second.get("clear").is_none() => {
            let mut scratch = s.scratch();
            section(&mut scratch, second)?;
            scratch.section
        }
        _ => return Err(bad(cmd, "`second` must be a plane specification, without nested sections or clear")),
    };
    // A named plane's world axis: which component of a point lies along its normal.
    let (origin, normal, axis) = match p.get("plane") {
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
            let solidcraft_doc::PlaneRef::Construction { name } = &r else { return Err(bad(cmd, "`plane` must be a plane")) };
            let st = s.model.state();
            let pl = st
                .construct
                .iter()
                .find(|c| &c.name == name)
                .and_then(|c| if let solidcraft_doc::construct::ConstructGeom::Plane(pl) = &c.geom { Some(*pl) } else { None })
                .ok_or_else(|| bad(cmd, format!("construction plane `{name}` is not built (rolled back, suppressed or failed)")))?;
            (pl.origin, pl.normal(), None)
        }
        Some(o @ Value::Object(_)) => {
            let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
            let n = o.get("normal").and_then(vec3).and_then(|n| n.normalized()).ok_or_else(|| bad(cmd, "the plane needs a non-zero `normal`"))?;
            (origin, n, None)
        }
        _ => return Err(bad(cmd, "`plane` must be XY, XZ, YZ, a construction plane name or id, or {origin, normal}")),
    };
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
    if !origin.is_finite() || !offset.is_finite() {
        return Err(bad(cmd, "the plane must be finite"));
    }
    let normal = if bool_(p, "flip").unwrap_or(false) { -normal } else { normal };
    let origin = origin + normal * offset * if bool_(p, "flip").unwrap_or(false) { -1.0 } else { 1.0 };
    if second.is_some_and(|(_, n)| n.dot(normal).abs() > 1e-6) {
        return Err(bad(cmd, "quarter-section planes must be perpendicular"));
    }
    s.section = Some((origin, normal));
    s.section_second = second;
    s.revision += 1;
    let mut section = json!({"origin": origin, "normal": normal});
    if let Some((o, n)) = second {
        section["second"] = json!({"origin": o, "normal": n});
    }
    if let Some(i) = axis {
        section["axis"] = json!(["X", "Y", "Z"].get(i).copied().unwrap_or("?"));
        section["at"] = json!(axis_component(origin, i));
    }
    Ok(json!({"section": section}))
}

fn axis_component(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod contribution_tests {
    use super::*;
    #[test]
    fn contribution_quarter_section_is_view_only_and_clear_resets_both_planes() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        let doc = s.doc.clone();
        let undo = s.undo.len();
        s.execute("inspect.section", &json!({"plane":"YZ","at":4,"flip":true,"second":{"plane":"XZ","at":6}})).unwrap();
        assert_eq!(s.section.unwrap().0.x, 4.0);
        assert_eq!(s.section_second.unwrap().0.y, 6.0);
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.undo.len(), undo);
        s.execute("inspect.section", &json!({"plane":"XY","at":3})).unwrap();
        assert!(s.section_second.is_none(), "a half section replaces the quarter section");
        s.execute("inspect.section", &json!({"plane":"YZ","second":{"plane":"XZ"}})).unwrap();
        s.execute("inspect.section", &json!({"clear":true})).unwrap();
        assert!(s.section.is_none() && s.section_second.is_none());
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.undo.len(), undo);
    }
    #[test]
    fn contribution_invalid_quarter_section_keeps_the_previous_cut() {
        let mut s = Session::default();
        s.execute("inspect.section", &json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6}})).unwrap();
        let cut = (s.section, s.section_second);
        for p in [
            json!({"plane":"XY","second":{"plane":"XY"}}),
            json!({"plane":"XY","second":{"plane":{"normal":[0,0,0]}}}),
            json!({"plane":"XY","second":{"plane":"XZ","second":{"plane":"YZ"}}}),
            json!({"plane":"XY","second":{"clear":true}}),
            json!({"plane":"XY","second":7}),
            json!({"plane":"XY","second":{"plane":"XZ","offset":"1e999 mm"}}),
        ] {
            assert!(s.execute("inspect.section", &p).is_err(), "{p}");
            assert_eq!((s.section, s.section_second), cut, "{p}");
        }
    }
}
