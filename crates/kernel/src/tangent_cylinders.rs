//! Cylinders of one radius whose axes cross (#788: a second bore of the same size drilled across
//! a bore, a pipe tee of equal pipes).
//!
//! Two such cylinders meet in two ellipses that cross each other at two points, where the
//! surfaces touch along a common normal. A mesh intersection cannot trace the curves through
//! there, and a boolean with a crossing in the middle of a face has no way to tell which of the
//! curves' four arms lies in which sector of the face. With the touching points on the edges of
//! the faces instead, each arm is a curve running from an edge to an edge, which the boolean
//! takes. So before the boolean, each of the two faces is split along the line of its cylinder
//! through the touching points (the solids are unchanged), and the result's history is composed
//! back over the operands as they were given.

use solidcraft_geom::{Plane, UnitVec3, Vec3};
use truck_modeling::{ParametricCurve, SearchNearestParameter};

use crate::body::{Body, from_p3, p3};
use crate::boolean_record as recorded;
use crate::history::BodyHistory;
use crate::history::History;
use crate::splitface::{SplitTool, split_any};
use crate::{BoolOp, FaceSurface, Result, face_surface};

/// Most faces split before the boolean (a face crossing several cylinders is split for each).
const MAX_SPLITS: usize = 16;

/// A cylindrical face and its cylinder.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cylinder {
    face: usize,
    origin: Vec3,
    axis: UnitVec3,
    radius: f64,
}

impl Cylinder {
    #[cfg(test)]
    /// `None` for an axis with no direction.
    pub(crate) fn new(face: usize, origin: Vec3, axis: Vec3, radius: f64) -> Option<Self> {
        Some(Self { face, origin, axis: axis.unit()?, radius })
    }
}

fn cylinders(body: &Body) -> Vec<Cylinder> {
    (0..body.face_count())
        .filter_map(|face| match face_surface(body, face)? {
            FaceSurface::Cylinder { origin, axis, radius } if origin.is_finite() && radius.is_finite() => {
                Some(Cylinder { face, origin, axis, radius })
            }
            _ => None,
        })
        .collect()
}

/// Where two cylinders of one radius with crossing axes touch, and the planes that hold the
/// touching points and one cylinder's axis each.
pub(crate) struct Crossing {
    pub(crate) points: [Vec3; 2],
    plane_x: Plane,
    plane_y: Plane,
}

pub(crate) fn crossing(x: &Cylinder, y: &Cylinder, tol: f64) -> Option<Crossing> {
    let finite = [x, y].iter().all(|c| c.origin.is_finite() && c.radius.is_finite() && c.radius > 0.0);
    if !finite || !tol.is_finite() || (x.radius - y.radius).abs() > tol {
        return None;
    }
    let across = x.axis.cross(y.axis);
    // Axes within a hair of parallel touch along a line, which is another matter.
    let sine = across.len();
    if !(sine > 1e-4) {
        return None;
    }
    // The closest points of the two axes, which meet in a point.
    let (b, w) = (x.axis.dot(y.axis), x.origin - y.origin);
    let (d, e) = (x.axis.dot(w), y.axis.dot(w));
    let denominator = 1.0 - b * b;
    if !(denominator > 1e-12) {
        return None;
    }
    let s = (b * e - d) / denominator;
    let t = (e - b * d) / denominator;
    let (on_x, on_y) = (x.origin + x.axis * s, y.origin + y.axis * t);
    if !(on_x.dist(on_y) <= tol) {
        return None;
    }
    let centre = (on_x + on_y) * 0.5;
    // Both cylinders reach their common normal `c` at a radius from the crossing.
    let c = across / sine;
    let points = [centre + c * x.radius, centre - c * x.radius];
    Some(Crossing { points, plane_x: Plane::from_normal(centre, x.axis.cross(c))?, plane_y: Plane::from_normal(centre, y.axis.cross(c))? })
}

/// Whether `p` is on face `face` of the body whose mesh is `mesh`: within `reach` of one of the
/// face's triangles.
fn on_face(mesh: &solidcraft_geom::Mesh, face: usize, p: Vec3, reach: f64) -> bool {
    mesh.triangles
        .iter()
        .zip(&mesh.tri_face)
        .filter(|(_, f)| **f as usize == face)
        .filter_map(|(t, _)| mesh.tri(t))
        .any(|[a, b, c]| crate::boolean_guard::point_triangle_dist(p, a, b, c) <= reach)
}

/// Whether `p` lies on an edge of face `face` (within `margin`).
fn on_boundary(body: &Body, face: usize, p: Vec3, margin: f64) -> bool {
    let Some(face) = body.solid.face_iter().nth(face) else { return false };
    face.absolute_boundaries().iter().any(|wire| {
        wire.edge_iter().any(|edge| {
            let curve = edge.curve();
            curve.search_nearest_parameter(p3(p), None, 100).is_some_and(|t| from_p3(curve.subs(t)).dist(p) <= margin)
        })
    })
}

/// The split the bodies need next: `(whether in the first body, face, plane)`, if a touching
/// point of two cylinders lies on both their faces but on the edges of neither yet.
fn next_split(a: &Body, b: &Body, tol: f64) -> Option<(bool, usize, Plane)> {
    let (in_b, in_a) = (cylinders(b), cylinders(a));
    if in_a.is_empty() || in_b.is_empty() {
        return None;
    }
    let size = a.size().max(b.size());
    let (margin, chord) = ((size * 1e-6).max(1e-9), (size * 1e-3).max(1e-6));
    let mut meshes = None;
    for x in &in_a {
        for y in &in_b {
            let Some(crossing) = crossing(x, y, tol) else { continue };
            if meshes.is_none() {
                meshes = Some((a.tessellate(chord).ok()?, b.tessellate(chord).ok()?));
            }
            let (ma, mb) = meshes.as_ref()?;
            for p in crossing.points {
                if !(on_face(ma, x.face, p, 3.0 * chord) && on_face(mb, y.face, p, 3.0 * chord)) {
                    continue;
                }
                if !on_boundary(a, x.face, p, margin) {
                    return Some((true, x.face, crossing.plane_x));
                }
                if !on_boundary(b, y.face, p, margin) {
                    return Some((false, y.face, crossing.plane_y));
                }
            }
        }
    }
    None
}

/// Whether the bodies have cylinders of one radius crossing each other that touch inside both
/// their faces, so that [`boolean`] has faces to split.
pub(crate) fn needed(a: &Body, b: &Body) -> bool {
    !a.is_mesh() && !b.is_mesh() && !SPLITTING.with(|c| c.get()) && next_split(a, b, (a.size().max(b.size()) * 1e-7).max(1e-9) * 10.0).is_some()
}

thread_local! {
    /// Set while the boolean of the split bodies runs (no second round of splitting).
    static SPLITTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The boolean `op` of `a` and `b` with their faces split along the touching points of equal
/// cylinders crossing each other, `run` doing the boolean of the split bodies. `None` when the
/// bodies have no such cylinders, or the boolean of the split bodies gave no (checked) result:
/// the caller then takes its other ways.
pub(crate) fn boolean(a: &Body, b: &Body, op: BoolOp, run: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Result<Option<Body>>> {
    if a.is_mesh() || b.is_mesh() || SPLITTING.with(|c| c.get()) {
        return None;
    }
    let size = a.size().max(b.size());
    let tol = (size * 1e-7).max(1e-9) * 10.0;
    let ((mut x, mut hx), (mut y, mut hy)) = ((a.clone(), Some(History::identity(a))), (b.clone(), Some(History::identity(b))));
    let mut split = false;
    for _ in 0..MAX_SPLITS {
        let Some((first, face, plane)) = next_split(&x, &y, tol) else { break };
        let (body, history) = if first { (&x, &hx) } else { (&y, &hy) };
        let (mut done, _) = split_any(body, &[face], &SplitTool::Plane(plane)).ok()?;
        // The split is no authored boundary: a later feature may still merge what it separates.
        done.regularization_protected = body.regularization_protected;
        let through = recorded::through(&done, 0, history.as_ref());
        if first {
            (x, hx) = (done, through);
        } else {
            (y, hy) = (done, through);
        }
        split = true;
    }
    if !split {
        return None;
    }
    SPLITTING.with(|c| c.set(true));
    let result = crate::guard("boolean of split faces", || run(&x, &y, op));
    SPLITTING.with(|c| c.set(false));
    let Ok(result) = result else { return None };
    // Held to what a boolean can do, as a retry's result is (see `boolean_guard`).
    crate::boolean_guard::check_retry(a, b, op, result.as_ref()).ok()?;
    let counts = crate::record::counts(&[a, b]);
    Some(Ok(result.map(|body| {
        // Each step's history over the operands `[a, b]`.
        let history = recorded::of_pair(&body, &recorded::placed(hx.clone(), 0, 2), &recorded::placed(hy.clone(), 1, 2));
        body.with_checked_history(history, &counts)
    })))
}
