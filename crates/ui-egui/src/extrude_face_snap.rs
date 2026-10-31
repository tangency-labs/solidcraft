//! Extrude's distance arrow stops at faces (#2398). While the arrow is dragged, a flat face
//! parallel to the profile takes the distance once the arrow's tip comes within a few screen
//! points of its plane: the distance becomes exactly the face's, and the face is outlined. A
//! cut dragged through a block so stops at the block's far side, and the outline shows it got
//! there. Only faces over the profile's footprint count (seen along the arrow), so faces off to
//! the side of the profile don't catch the arrow. Holding Alt or Ctrl (the free drag) turns
//! this off with the round steps. Preferences › Design › `extrude_arrow_face_stops` (on by
//! default) turns it off for good: the arrow then moves in round steps only, as before.
//!
//! The distance stays a distance: the face is not stored with the extrude (To Object does that).

use std::cell::RefCell;
use std::sync::Arc;

use egui::{Pos2, Stroke};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::ModelState;
use solidcraft_engine::geom::{Mesh, Plane, Vec2, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::theme::Tokens;
use crate::viewport::Proj;

/// How near (screen points) the arrow's tip must come to a face's plane to stop there.
pub const SNAP_PX: f32 = 8.0;
/// How closely (cosine) a face's normal must follow the arrow to count as parallel.
const PARALLEL: f64 = 1.0 - 1e-6;
/// The most faces looked at in one model, and the most points kept of each.
const MAX_FACES: usize = 20_000;
const MAX_POINTS: usize = 4_096;

/// A face the arrow can stop at: `distance` along the arrow from its base to the face's plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub body: String,
    pub face: usize,
    pub distance: f64,
}

/// A flat face of a body: a point on it, its unit normal and its corners' bounds.
#[derive(Clone, Debug)]
struct FlatFace {
    body: String,
    face: usize,
    point: Vec3,
    normal: Vec3,
    points: Vec<Vec3>,
}

type Faces = (Option<Arc<ModelState>>, Arc<Vec<FlatFace>>);

thread_local! {
    /// The face the arrow stopped at this frame (for the outline, scenarios and tests).
    static CURRENT: RefCell<Option<Stop>> = const { RefCell::new(None) };
    /// The flat faces of the last model state looked at.
    static FACES: RefCell<Faces> = RefCell::new((None, Arc::new(Vec::new())));
}

/// The face the arrow stopped at in the last frame drawn, if any.
#[cfg(test)]
pub fn current() -> Option<Stop> {
    CURRENT.with(|c| c.borrow().clone())
}

pub(crate) fn set(stop: Option<Stop>) {
    CURRENT.with(|c| *c.borrow_mut() = stop);
}

/// The flat faces of a mesh: per face, the plane its triangles lie in (within a tolerance
/// scaled to the mesh) and its points.
fn flat_faces_of(body: &str, m: &Mesh) -> Vec<FlatFace> {
    let mut by_face: Vec<(Vec3, Vec<Vec3>)> = Vec::new();
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let f = *f as usize;
        if f >= MAX_FACES {
            continue;
        }
        let Some([a, b, c]) = m.tri(t) else { continue };
        if by_face.len() <= f {
            by_face.resize_with(f + 1, || (Vec3::ZERO, Vec::new()));
        }
        let Some((n, pts)) = by_face.get_mut(f) else { continue };
        *n += (b - a).cross(c - a);
        if pts.len() + 3 <= MAX_POINTS {
            pts.extend([a, b, c]);
        }
    }
    let tol = (m.bounds().diagonal() * 1e-6).max(1e-7) * 10.0;
    by_face
        .into_iter()
        .enumerate()
        .filter_map(|(face, (n, points))| {
            let normal = n.normalized()?;
            let point = *points.first()?;
            let flat = points.iter().all(|p| (*p - point).dot(normal).abs() <= tol);
            (flat && normal.is_finite()).then(|| FlatFace { body: body.to_string(), face, point, normal, points })
        })
        .collect()
}

/// The flat faces of every body in `st`, kept while the state stays the same.
fn flat_faces(st: &Arc<ModelState>) -> Arc<Vec<FlatFace>> {
    if let Some(known) = FACES.with(|c| {
        let c = c.borrow();
        c.0.as_ref().filter(|k| Arc::ptr_eq(k, st)).map(|_| c.1.clone())
    }) {
        return known;
    }
    let faces: Arc<Vec<FlatFace>> = Arc::new(st.bodies.iter().flat_map(|b| flat_faces_of(&b.name, &b.mesh())).collect());
    FACES.with(|c| *c.borrow_mut() = (Some(st.clone()), faces.clone()));
    faces
}

/// The bounds, in the plane's own coordinates, of `points` seen along the plane's normal.
fn bounds_2d(plane: &Plane, points: impl IntoIterator<Item = Vec3>) -> Option<(Vec2, Vec2)> {
    let mut out: Option<(Vec2, Vec2)> = None;
    for p in points {
        let q = plane.to_local(p);
        if !(q.x.is_finite() && q.y.is_finite()) {
            continue;
        }
        out = Some(match out {
            None => (q, q),
            Some((lo, hi)) => (Vec2::new(lo.x.min(q.x), lo.y.min(q.y)), Vec2::new(hi.x.max(q.x), hi.y.max(q.y))),
        });
    }
    out
}

/// What the extrude starts from, as world points: its profiles' outlines, or its faces' points.
fn footprint(app: &SolveApp, d: &Dialog, faces: &[FlatFace]) -> Vec<Vec3> {
    let st = app.session.world_state();
    let mut out = Vec::new();
    for item in d.inputs.first().map(|i| i.items.as_slice()).unwrap_or_default() {
        match item {
            Sel::Profile { sketch, index } => {
                let (Some(ss), Some(plane)) = (st.sketch(*sketch), crate::canvas::profile_plane(app, *sketch)) else { continue };
                if let Some(p) = ss.profiles.get(*index) {
                    out.extend(p.region.outer.polyline(1e-2).into_iter().take(MAX_POINTS).map(|q| plane.to_world(q)));
                }
            }
            Sel::Face { body, index, .. } => {
                if let Some(f) = faces.iter().find(|f| f.body == *body && f.face == *index) {
                    out.extend(f.points.iter().copied());
                }
            }
            _ => {}
        }
    }
    out
}

/// The faces the open extrude's arrow (from `base` along `n`) can stop at: flat faces of shown
/// bodies, parallel to the arrow, off the base's own plane, over the footprint.
pub(crate) fn stops(app: &SolveApp, d: &Dialog, base: Vec3, n: Vec3) -> Vec<Stop> {
    if !app.preferences.extrude_arrow_face_stops || !matches!(d.kind, Kind::Extrude { .. }) {
        return Vec::new();
    }
    let Some(plane) = Plane::from_normal(base, n) else { return Vec::new() };
    let st = app.session.world_state();
    let faces = flat_faces(&st);
    let Some((lo, hi)) = bounds_2d(&plane, footprint(app, d, &faces)) else { return Vec::new() };
    let size = lo.dist(hi).max(1.0);
    let tol = size * 1e-6;
    let mut out: Vec<Stop> = faces
        .iter()
        .filter(|f| f.normal.dot(n).abs() >= PARALLEL && app.body_visible(&f.body))
        .filter_map(|f| {
            let distance = (f.point - base).dot(n);
            if !distance.is_finite() || distance.abs() <= tol {
                return None;
            }
            let (flo, fhi) = bounds_2d(&plane, f.points.iter().copied())?;
            let over = flo.x <= hi.x + tol && fhi.x >= lo.x - tol && flo.y <= hi.y + tol && fhi.y >= lo.y - tol;
            over.then(|| Stop { body: f.body.clone(), face: f.face, distance })
        })
        .collect();
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}

/// The stop nearest the arrow's tip on screen when the tip is at `along` (from `base` along
/// `n`), if one is within [`SNAP_PX`].
pub(crate) fn nearest<'a>(proj: &Proj, base: Vec3, n: Vec3, along: f64, stops: &'a [Stop]) -> Option<&'a Stop> {
    let tip = proj.to_screen(base + n * along)?;
    stops
        .iter()
        .filter_map(|s| Some((s, proj.to_screen(base + n * s.distance)?.distance(tip))))
        .filter(|(_, px)| *px <= SNAP_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
}

/// Outlines the face the arrow stopped at, as a hovered face's edges are drawn.
pub(crate) fn outline(app: &SolveApp, painter: &egui::Painter, proj: &Proj, stop: &Stop) {
    let st = app.session.world_state();
    let Some(b) = st.body(&stop.body) else { return };
    let m = b.mesh();
    let t = Tokens::get();
    let Ok(face) = u32::try_from(stop.face) else { return };
    for e in m.face_edges(face) {
        let Some(pts) = m.edges.get(e) else { continue };
        let screen: Vec<Pos2> = pts.iter().filter_map(|p| proj.to_screen(*p)).collect();
        if screen.len() < 2 {
            continue;
        }
        painter.add(egui::Shape::line(screen.clone(), Stroke::new(5.0, t.hover_edge_halo)));
        painter.add(egui::Shape::line(screen, Stroke::new(2.5, t.hover_edge)));
    }
}
