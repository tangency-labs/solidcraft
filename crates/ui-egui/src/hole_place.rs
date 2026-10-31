//! Placing a Single hole on the canvas (#2171, #1047). Both behaviours are Preferences › Design
//! settings, off by default, so a hole goes where it is clicked and a second pick on its face
//! takes it out, as it always did.
//!
//! Create Hole › Snap to face and edge centres (`hole_snaps`): the planar face under the cursor offers snap points, the
//! middle of the face and the centres of its round edges (a boss standing on it, a rounded
//! corner, a hole already in it). While the cursor is over the face they show as small marks; a
//! pick within reach of one, or on one of those round edges, is taken at that point, and the mark
//! it takes is named. Sketch points on the face (#657) still come first. Ctrl or Alt held: no snap.
//!
//! Create Hole › Move holes by clicking or dragging (`hole_moves`): every placed hole has a handle at its
//! centre that drags it across the model (snapping as a pick would) and takes it out when
//! clicked, and a pick elsewhere on a face that already has a hole moves the hole there. The handle
//! is the shared placement marker (`place_gizmo`, one per hole through `place_gizmo/hole.rs`, #6818);
//! this file keeps what is the hole's: its snaps, and the moves of its face places.

use std::cell::RefCell;
use std::sync::Arc;

use egui::Pos2;
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::{Aabb3, Mesh, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_assembly::SnapKind;
use crate::selection::same_item;
use crate::theme::Tokens;
use crate::viewport::{Hit, Proj};

/// How near (screen points) a pick must come to a snap point to take it.
pub(crate) const POINT_REACH: f32 = 10.0;
/// How near a pick must come to a round edge to take its centre.
pub(crate) const RIM_REACH: f32 = 6.0;
/// The most triangles of one face looked through for its snaps.
const MAX_TRIANGLES: usize = 1 << 18;

/// A place a hole may snap to on a planar face.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snap {
    pub at: Vec3,
    pub kind: SnapKind,
    /// The round edges whose centre it is (a rim split into pieces makes one snap).
    pub rims: Vec<Vec<Vec3>>,
}

thread_local! {
    /// The last face's snaps, with the mesh they were found on (held, so a new mesh at the same
    /// address can't pass for it).
    static CACHE: RefCell<Option<(Arc<Mesh>, usize, Arc<Vec<Snap>>)>> = const { RefCell::new(None) };
}

/// The snaps of face `face` of mesh `m`: none unless the face is planar.
pub(crate) fn face_snaps(m: &Arc<Mesh>, face: usize) -> Arc<Vec<Snap>> {
    let cached = CACHE.with(|c| c.borrow().as_ref().filter(|(mm, f, _)| Arc::ptr_eq(mm, m) && *f == face).map(|(_, _, s)| s.clone()));
    if let Some(snaps) = cached {
        return snaps;
    }
    let snaps = Arc::new(find_snaps(m, face));
    CACHE.with(|c| *c.borrow_mut() = Some((m.clone(), face, snaps.clone())));
    snaps
}

/// The centres of the face's round edges that lie in its plane, and the middle of the face (the
/// centre of its extent, when that is on the face and no round edge's centre is there already).
fn find_snaps(m: &Mesh, face: usize) -> Vec<Snap> {
    let tris: Vec<[Vec3; 3]> =
        m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f as usize == face).filter_map(|(t, _)| m.tri(t)).take(MAX_TRIANGLES).collect();
    let Some(&[a, _, _]) = tris.first() else { return Vec::new() };
    // The plane: the triangles' summed normals (counter-clockwise from outside), through a corner.
    let Some(n) = tris.iter().fold(Vec3::ZERO, |s, [p, q, r]| s + (*q - *p).cross(*r - *p)).normalized() else { return Vec::new() };
    let mut bounds = Aabb3::EMPTY;
    tris.iter().flatten().for_each(|p| bounds.add(*p));
    let tol = (bounds.diagonal() * 1e-6).max(1e-7);
    if tris.iter().flatten().any(|p| (*p - a).dot(n).abs() > tol * 10.0) {
        return Vec::new();
    }
    let same = |p: Vec3, q: Vec3| p.dist(q) <= tol * 100.0;
    let mut snaps: Vec<Snap> = Vec::new();
    for e in m.face_edges(face as u32) {
        let Some(pts) = m.edges.get(e) else { continue };
        let Some((c, axis)) = crate::dialogs_assembly::circle_of(pts) else { continue };
        // A circle or an arc in the face's plane (a seam or a slanted rim is no place for a hole).
        if (c - a).dot(n).abs() > tol * 100.0 || axis.dot(n).abs() < 1.0 - 1e-6 {
            continue;
        }
        match snaps.iter_mut().find(|s| same(s.at, c)) {
            Some(s) => s.rims.push(pts.clone()),
            None => snaps.push(Snap { at: c, kind: SnapKind::CircleCenter, rims: vec![pts.clone()] }),
        }
    }
    let c = bounds.center();
    let c = c - n * (c - a).dot(n);
    if on_face(c, n, &tris, tol) && !snaps.iter().any(|s| same(s.at, c)) {
        snaps.insert(0, Snap { at: c, kind: SnapKind::FaceCenter, rims: Vec::new() });
    }
    snaps
}

/// Does `p` (in the plane of normal `n`) lie on one of the triangles?
fn on_face(p: Vec3, n: Vec3, tris: &[[Vec3; 3]], tol: f64) -> bool {
    tris.iter().any(|[a, b, c]| [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (**v - **u).cross(p - **u).dot(n) >= -tol * (**v - **u).len()))
}

/// Is a Single hole being placed: the Hole dialog open on its face input?
pub(crate) fn placing(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| {
        d.active == 0
            && matches!(&d.kind, Kind::Hole { opts, .. } if !opts.multiple)
            && d.active_input().is_some_and(|i| crate::dialogs::snaps_to_points(i.accept))
    })
}

/// Do picks snap while a Single hole is placed (the preference is on)?
pub(crate) fn snapping(app: &SolveApp) -> bool {
    app.preferences.hole_snaps && placing(app)
}

/// The distance on screen from `p` to the segment from `a` to `b`.
fn to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let v = b - a;
    let t = if v.length_sq() > 0.0 { ((p - a).dot(v) / v.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + v * t)
}

/// The snap a pick at `pos` takes among `snaps`: the nearest snap point within [`POINT_REACH`],
/// else the centre of the nearest round edge within [`RIM_REACH`].
fn nearest(proj: &Proj, pos: Pos2, snaps: &[Snap]) -> Option<Snap> {
    let point = snaps
        .iter()
        .filter_map(|s| Some((proj.to_screen(s.at)?.distance(pos), s)))
        .filter(|(d, _)| *d <= POINT_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, s)) = point {
        return Some(s.clone());
    }
    snaps
        .iter()
        .filter_map(|s| {
            let d = s
                .rims
                .iter()
                .flat_map(|r| r.windows(2))
                .filter_map(|w| Some(to_segment(pos, proj.to_screen(*w.first()?)?, proj.to_screen(*w.get(1)?)?)))
                .fold(f32::INFINITY, f32::min);
            (d <= RIM_REACH).then_some((d, s))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s.clone())
}

/// A pick for a Single hole's face place: the nearest face under the cursor is taken at the snap
/// within reach on it, or at the centre of a round edge the cursor is on (the edge lies between
/// the face and a hole's or a boss's wall, so the face under the cursor may be the wall).
pub(crate) fn snap_hits(app: &SolveApp, proj: &Proj, pos: Pos2, hits: &mut Vec<Hit>) {
    if !snapping(app) || crate::drag_snap::suppressed() {
        return;
    }
    let st = app.session.world_state();
    let face = hits.iter().position(|h| matches!(h, Hit::Face { .. }));
    let at_face = face.and_then(|i| match hits.get(i) {
        Some(Hit::Face { body, index, .. }) => {
            let m = st.body(body)?.mesh();
            nearest(proj, pos, &face_snaps(&m, *index)).map(|s| (body.clone(), *index, s.at))
        }
        _ => None,
    });
    let on_rim = || {
        hits.iter().find_map(|h| {
            let Hit::Edge { body, index, .. } = h else { return None };
            let m = st.body(body)?.mesh();
            let pts = m.edges.get(*index)?;
            m.edge_faces.get(*index)?.iter().find_map(|f| {
                let snaps = face_snaps(&m, *f as usize);
                snaps.iter().find(|s| s.rims.iter().any(|r| r == pts)).map(|s| (body.clone(), *f as usize, s.at))
            })
        })
    };
    let Some((body, index, point)) = at_face.or_else(on_rim) else { return };
    let hit = Hit::Face { body, index, point };
    match face {
        Some(i) => {
            if let Some(h) = hits.get_mut(i) {
                *h = hit;
            }
        }
        None => hits.push(hit),
    }
}

/// The snap a face place sits on, if any.
fn snap_under(app: &SolveApp, body: &str, index: usize, point: Vec3) -> (Arc<Vec<Snap>>, Option<Snap>) {
    let st = app.session.world_state();
    let Some(b) = st.body(body) else { return (Arc::new(Vec::new()), None) };
    let snaps = face_snaps(&b.mesh(), index);
    let on = snaps.iter().find(|s| s.at.dist(point) <= 1e-9 * (1.0 + s.at.len())).cloned();
    (snaps, on)
}

/// The holes placed so far: the index of each one's face place, and where it is. A round face
/// split at its seam is picked as both halves, the one picked first where the hole goes.
pub(crate) fn places(s: &Session, d: &Dialog) -> Vec<(usize, Vec3)> {
    let Some(inp) = d.inputs.first() else { return Vec::new() };
    let mut taken: Vec<Sel> = Vec::new();
    let mut out = Vec::new();
    for (i, x) in inp.items.iter().enumerate() {
        let Sel::Face { point, .. } = x else { continue };
        if taken.iter().any(|t| same_item(t, x)) {
            continue;
        }
        taken.extend(crate::viewport::grouped_selection(s, x));
        out.push((i, *point));
    }
    out
}

/// Move the hole placed by item `i` of the face input to the face place `to`. The place (with the
/// other half of a round face split at its seam) gives way to `to` (with its own). Not onto a face
/// another hole holds: a face takes one Single hole, so that would lose the other one.
pub(crate) fn move_place(d: &mut Dialog, s: &Session, i: usize, to: Sel) -> bool {
    let group = crate::viewport::grouped_selection(s, &to);
    let Some(inp) = d.inputs.first_mut() else { return false };
    let Some(old) = inp.items.get(i).cloned() else { return false };
    let own = crate::viewport::grouped_selection(s, &old);
    let mine = |x: &Sel| own.iter().any(|g| same_item(x, g));
    if inp.items.iter().any(|x| !mine(x) && group.iter().any(|g| same_item(x, g))) {
        return false;
    }
    let at = inp.items.iter().position(&mine).unwrap_or(i);
    inp.items.retain(|x| !mine(x));
    let at = at.min(inp.items.len());
    for (k, x) in group.into_iter().enumerate() {
        inp.items.insert(at + k, x);
    }
    d.error = None;
    true
}

/// A Single hole's face place picked on a face that already has a hole moves that hole there
/// (true), when holes move by clicking. A pick on the hole itself (within its radius) is left to
/// take it out, as before. A hole held along one reference edge goes along it only, and the pick
/// is spent either way (it never takes the hole out instead).
pub(crate) fn pick_moves(d: &mut Dialog, s: &Session, sel: &Sel) -> bool {
    let Kind::Hole { opts, diameter, .. } = &d.kind else { return false };
    let Sel::Face { point, .. } = sel else { return false };
    if opts.multiple || d.active != 0 {
        return false;
    }
    let radius = s.doc.eval(diameter, ValueKind::Length).ok().filter(|v| v.is_finite() && *v > 0.0).map_or(0.0, |v| v / 2.0);
    let group = crate::viewport::grouped_selection(s, sel);
    let Some(inp) = d.inputs.first() else { return false };
    let Some(i) = inp.items.iter().position(|x| group.iter().any(|g| same_item(x, g))) else { return false };
    let Some(Sel::Face { point: was, .. }) = inp.items.get(i) else { return false };
    if was.dist(*point) <= radius {
        return false;
    }
    match crate::hole_refs::freedom(d, i) {
        crate::place_gizmo::Freedom::Plane => move_place(d, s, i, sel.clone()),
        crate::place_gizmo::Freedom::Line(_) => {
            crate::hole_refs::drag_to(s, d, i, *point);
            true
        }
    }
}

/// Take out the hole placed by item `i` of the face input (with the other half of a round face
/// split at its seam).
pub(crate) fn remove_place(d: &mut Dialog, s: &Session, i: usize) {
    let Some(inp) = d.inputs.first_mut() else { return };
    let Some(old) = inp.items.get(i).cloned() else { return };
    let own = crate::viewport::grouped_selection(s, &old);
    inp.items.retain(|x| !own.iter().any(|g| same_item(x, g)));
    d.error = None;
}

/// The snap points of the face under the cursor while a Single hole is placed, and the one a
/// pick there takes, named. Called by the canvas after the placement handles.
pub(crate) fn marks(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !snapping(app) {
        return;
    }
    let Some(Hit::Face { body, index, point }) = &app.viewport.hover else { return };
    let t = Tokens::get();
    let (snaps, on) = snap_under(app, body, *index, *point);
    for s in snaps.iter() {
        let Some(c) = proj.to_screen(s.at) else { continue };
        if on.as_ref().is_some_and(|o| o.at == s.at) {
            let name = crate::i18n::tr_label(crate::dialogs_assembly::snap_name(s.kind));
            crate::icons::markers::snap(painter, c, None, s.kind, t.hover_profile_edge, Some(name));
        } else {
            crate::icons::markers::snap_hint(painter, c, s.kind, t.manipulator);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::geom::Vec3;

    /// A square face (two triangles) at z = 5 with a round edge about (2, 3) on it.
    fn mesh() -> Mesh {
        let p = |x: f64, y: f64| Vec3::new(x, y, 5.0);
        let rim: Vec<Vec3> = (0..=16).map(|k| f64::from(k) * std::f64::consts::TAU / 16.0).map(|a| p(2.0 + a.cos(), 3.0 + a.sin())).collect();
        Mesh {
            positions: vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            edges: vec![vec![p(0.0, 0.0), p(10.0, 0.0)], rim],
            edge_faces: vec![vec![0], vec![0, 1]],
            ..Mesh::default()
        }
    }

    #[test]
    fn a_planar_face_snaps_to_its_middle_and_its_round_edges_centres() {
        let snaps = find_snaps(&mesh(), 0);
        let at: Vec<(SnapKind, Vec3)> = snaps.iter().map(|s| (s.kind, s.at)).collect();
        assert_eq!(at.len(), 2, "{at:?}");
        assert!(at[0].0 == SnapKind::FaceCenter && at[0].1.dist(Vec3::new(5.0, 5.0, 5.0)) < 1e-9, "{at:?}");
        assert!(at[1].0 == SnapKind::CircleCenter && at[1].1.dist(Vec3::new(2.0, 3.0, 5.0)) < 1e-9, "{at:?}");
        assert!(find_snaps(&mesh(), 7).is_empty(), "no triangles, no snaps");
    }

    #[test]
    fn a_curved_face_has_no_snaps() {
        let mut m = mesh();
        if let Some(p) = m.positions.get_mut(2) {
            p.z = 6.0;
        }
        assert!(find_snaps(&m, 0).is_empty());
    }
}
