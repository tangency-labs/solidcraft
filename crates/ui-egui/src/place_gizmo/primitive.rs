//! Box, Cylinder, Cone, Sphere, Torus and Coil: the marker places the primitive on the face under
//! the pointer, standing on it as a pre-selected face places it, while the dialog is open, from
//! Edit Feature too. The primitives do not turn in their face: no ring.
//!
//! Where the place is kept differs by primitive, and the marker moves it there:
//! - a Cylinder stands where its Face input is picked (and its reference edges measure from
//!   there, #6380), so a drag replaces that item, as a click on the input does;
//! - a Box placed by its typed corner or centre (#5638) has the drop written into those fields,
//!   so it stands on the face, centred across it;
//! - any other, and a Box a pre-selected face placed, takes the drop as its picked place.
//!
//! The marker sits at the dialog's picked place when there is one, else at the stored or typed
//! position (the centre, the base, or the middle of the box's side on its face), else where the
//! command puts a primitive with none (the origin). A typed Box's side on its face is found from
//! the model: the side that lies on a planar face looking at it, its bottom first.
//!
//! A Cylinder, Cone or Coil needs a planar face for its axis: a curved one does not take the drop.
//! A Torus is made flat, so its outline is drawn flat whatever face it is over.
//!
//! A Coil with a sketch profile has no marker: its place is the profile's, and the base and axis
//! the dialog would send from a picked place move the axis but not the section.

use std::cell::RefCell;
use std::sync::Arc;

use serde_json::Value;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;

use solidcraft_engine::Sel;

use super::{PlaceSource, Spec, circle, face_pick, len, rect, vec3_of};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::viewport::Hit;

pub(super) static SPEC: Spec = Spec {
    name: "primitive",
    serves: |app, d| super::on(app) && matches!(d.kind, Kind::Primitive { .. }) && !coil_with_profile(d),
    snap_commands: &[],
    move_tip: "Drag to place it on another face.",
    many: false,
    place: PlaceSource::Own { places: |app, d| super::one(frame(app, d)), move_to },
    click: None,
    footprint,
    freedom: super::anywhere,
    turn: None,
};

/// A planar face's plane: a point on it and its outward normal.
type FacePlane = (Vec3, Vec3);

thread_local! {
    /// The planes of the planar faces of the model state last asked about, found once per state.
    static PLANES: RefCell<(Option<Arc<solidcraft_engine::doc::ModelState>>, Arc<Vec<FacePlane>>)> = RefCell::new((None, Arc::new(Vec::new())));
}

/// The planes of the model's planar faces.
fn planes(app: &SolveApp) -> Arc<Vec<FacePlane>> {
    let st = app.session.world_state();
    if let Some(known) = PLANES.with(|c| c.borrow().0.as_ref().filter(|k| Arc::ptr_eq(k, &st)).map(|_| c.borrow().1.clone())) {
        return known;
    }
    let found: Vec<FacePlane> = st
        .bodies
        .iter()
        .flat_map(|b| b.body.faces((b.body.size() * 1e-3).max(1e-3)).unwrap_or_default())
        .filter_map(|f| Some((f.centroid, f.plane_normal?)))
        .collect();
    let found = Arc::new(found);
    PLANES.with(|c| *c.borrow_mut() = (Some(st), found.clone()));
    found
}

/// Is this a Coil whose section comes from a sketch (its Profile input holds something)?
fn coil_with_profile(d: &Dialog) -> bool {
    matches!(&d.kind, Kind::Primitive { cmd: "solid.coil", .. }) && d.inputs.iter().any(|i| !i.items.is_empty())
}

/// A size field's value (mm), zero when it is missing or does not evaluate.
fn size(app: &SolveApp, d: &Dialog, key: &str) -> f64 {
    let Kind::Primitive { fields, .. } = &d.kind else { return 0.0 };
    fields.iter().find(|f| f.0 == key).map(|f| len(app, &f.1)).unwrap_or(0.0)
}

/// The radius, from whichever of `radius` (Edit Feature) or `diameter` (a new one) the dialog holds.
fn radius(app: &SolveApp, d: &Dialog) -> f64 {
    let Kind::Primitive { fields, .. } = &d.kind else { return 0.0 };
    if fields.iter().any(|f| f.0 == "radius") { size(app, d, "radius") } else { size(app, d, "diameter") * 0.5 }
}

/// The box's length, width and height (mm).
fn box_size(app: &SolveApp, d: &Dialog) -> Vec3 {
    Vec3::new(size(app, d, "length"), size(app, d, "width"), size(app, d, "height"))
}

/// A stored or typed point of expressions (numbers, or strings in the design's terms).
fn expr_point(app: &SolveApp, v: &Value) -> Option<Vec3> {
    let c = |i: usize| -> Option<f64> {
        let x = v.as_array()?.get(i)?;
        x.as_f64()
            .or_else(|| app.session.doc.eval(&app.session.doc.with_design_unit(x.as_str()?, ValueKind::Length), ValueKind::Length).ok())
            .filter(|v| v.is_finite())
    };
    Some(Vec3::new(c(0)?, c(1)?, c(2)?))
}

/// Is `n` along a world axis (either way)?
fn square_to(n: Vec3, axis: Vec3) -> bool {
    n.dot(axis).abs() > 1.0 - 1e-9
}

/// The faces a Box can stand on, by their normal, in the order they are looked for.
const STANDS_ON: [Vec3; 6] = [Vec3::Z, Vec3::X, Vec3::new(-1.0, 0.0, 0.0), Vec3::Y, Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)];

/// How far a box side may be from the face plane and still stand on it (mm): a drop is written to
/// the micrometre.
const ON_PLANE: f64 = 1e-3;

/// Where a typed Box stands: the middle of its side that lies on a planar face of the model
/// looking at it (its bottom first), else the middle of its bottom.
fn box_frame(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    let half = box_size(app, d) * 0.5;
    let centre = match d.extra.get("center") {
        Some(v) => expr_point(app, v)?,
        None => d.extra.get("corner").and_then(|v| expr_point(app, v)).unwrap_or(Vec3::ZERO) + half,
    };
    let side = |n: Vec3| centre - Vec3::new(n.x * half.x, n.y * half.y, n.z * half.z);
    let planes = planes(app);
    let n = STANDS_ON
        .into_iter()
        .find(|n| planes.iter().any(|(q, m)| m.dot(*n) > 1.0 - 1e-9 && (side(*n) - *q).dot(*m).abs() < ON_PLANE))
        .unwrap_or(Vec3::Z);
    Some((side(n), n))
}

fn frame(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    let Kind::Primitive { cmd, at, .. } = &d.kind else { return None };
    let axis = || d.extra.get("axis").and_then(vec3_of).and_then(|a| a.normalized()).unwrap_or(Vec3::Z);
    match (*cmd, at) {
        (_, Some((p, Some(n)))) => return Some((*p, *n)),
        // A box at a vertex or on a curved face is centred on the point: the marker at its bottom.
        ("solid.box", Some((p, None))) => return Some((*p - Vec3::Z * (box_size(app, d).z * 0.5), Vec3::Z)),
        // No face to stand on: the axis it is made along, the stored one or the command's.
        ("solid.cylinder" | "solid.coil" | "solid.cone", Some((p, None))) => return Some((*p, axis())),
        (_, Some((p, None))) => return Some((*p, Vec3::Z)),
        (_, None) => {}
    }
    match *cmd {
        "solid.box" => box_frame(app, d),
        "solid.cylinder" | "solid.coil" | "solid.cone" => Some((d.extra.get("base").and_then(|v| expr_point(app, v)).unwrap_or(Vec3::ZERO), axis())),
        _ => Some((d.extra.get("center").and_then(|v| expr_point(app, v)).unwrap_or(Vec3::ZERO), Vec3::Z)),
    }
}

/// Stand the primitive on the face under the pointer, as a pre-selected face places it: through
/// the Cylinder's Face input (its place follows in the same frame), into a typed Box's corner or
/// centre (the Box's point pick, which no longer says where it is, is dropped), else as the
/// picked place. A Cylinder, Cone or Coil does not take a curved face: it has no one axis there.
fn move_to(app: &SolveApp, d: &mut Dialog, hits: &[Hit], _: usize) -> bool {
    let s = &app.session;
    let planar = |sel: &Sel| match sel {
        Sel::Face { body, index, point } => crate::dialogs::planar_face(s, body, *index).map(|(_, n)| (*point, n)),
        _ => None,
    };
    if crate::hole_refs::takes(d) {
        let Some(to) = face_pick(d, s, hits) else { return false };
        let Some((p, n)) = planar(&to) else { return false };
        if let Some(inp) = d.inputs.first_mut() {
            inp.items = vec![to];
        }
        if let Kind::Primitive { at, .. } = &mut d.kind {
            *at = Some((p, Some(n)));
        }
        d.error = None;
        return true;
    }
    let Some((p, n)) = hits.iter().find_map(|h| match h {
        Hit::Face { body, index, point } => Some((*point, planar(&Sel::Face { body: body.clone(), index: *index, point: *point }).map(|(_, n)| n))),
        _ => None,
    }) else {
        return false;
    };
    let sz = box_size(app, d);
    let Kind::Primitive { cmd, at, .. } = &mut d.kind else { return false };
    match *cmd {
        "solid.cone" | "solid.coil" if n.is_none() => return false,
        "solid.box" => {
            if at.is_none() {
                crate::primitive_placement::dropped(&mut d.extra, p, n.unwrap_or(Vec3::Z), sz);
            } else {
                *at = Some((p, n));
            }
            if let Some(inp) = d.inputs.first_mut() {
                inp.items.clear();
            }
        }
        _ => *at = Some((p, n)),
    }
    d.error = None;
    true
}

/// The primitive's outline on the face: the box's side that stands on it (a face square to an
/// axis), the cylinder's, cone's or coil's base circle, the sphere's widest circle across the
/// face, the torus's outer circle lying flat, as it is made.
fn footprint(app: &SolveApp, d: &Dialog, base: Vec3, n: Vec3) -> Vec<Vec3> {
    let Kind::Primitive { cmd, .. } = &d.kind else { return Vec::new() };
    match *cmd {
        "solid.box" => {
            let sz = box_size(app, d) * 0.5;
            if square_to(n, Vec3::Z) {
                rect(base, n, Vec3::X, sz.x, sz.y)
            } else if square_to(n, Vec3::X) {
                rect(base, n, Vec3::Y, sz.y, sz.z)
            } else if square_to(n, Vec3::Y) {
                rect(base, n, Vec3::X, sz.x, sz.z)
            } else {
                Vec::new()
            }
        }
        "solid.torus" => circle(base, Vec3::Z, size(app, d, "major") + size(app, d, "minor")),
        _ => circle(base, n, radius(app, d)),
    }
}

#[cfg(test)]
#[path = "primitive_tests.rs"]
mod tests;
