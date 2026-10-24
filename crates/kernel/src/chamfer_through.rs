//! A convex chamfer whose setbacks run past the faces it joins: taller than its wall (a hole's
//! rim chamfered deeper than the plate is thick), or wider than its face so that it reaches the
//! faces beyond (a hole's countersink wider than the plate round it). The exact chamfers build
//! the chamfer face between the two setback curves and trim the faces beside it, which only
//! works while those curves lie on the faces; past them they are refused as too large.
//!
//! Here such a chamfer is cut as a boolean instead. The tool's cross-section is the triangle
//! between the chamfer line and the two faces' planes, each moved a hair (`δ`) outwards so the
//! tool shares no face with the body: its only face inside the body is the chamfer surface, run
//! on past both setbacks until it leaves the body, so every face it reaches trims it. A circular
//! rim between a planar face and a cylinder or cone about its axis revolves that triangle about
//! the axis; a straight edge between two planes sweeps it along the edge, on past each end where
//! the end face leans away from the edge (never past a concave corner).
//!
//! The result is checked before it is kept: a closed, valid solid in as many pieces as before,
//! smaller than before, within the body's bounds, and with no face of the tool left in it other
//! than the chamfer (one would be an undercut below material beyond the faces' planes). A cut
//! that fails a check is refused rather than returned.

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
use truck_modeling as mt;

use crate::blend::{ChamferSide, Cutter, cut_history, edge_frame, edge_ids, loop_split, plane_normal, side_setbacks, vtx};
use crate::body::{Body, from_p3};
use crate::history::{FaceOrigin, History};
use crate::{KernelError, Result, guard, record};

/// Where a chamfer's setbacks end, in order of precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Reach {
    /// On the faces beside the edge.
    Within,
    /// Past a face's outer boundary or a wall's far end: cut through.
    Past,
    /// Into other geometry: an opening in the face (#1080), something across the wall's end.
    Blocked,
}

/// Where a setback leaves a face: how far along it, through which of the face's loops (its
/// index in the face's boundaries) and which edge.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Exit {
    pub(crate) at: f64,
    pub(crate) wire: usize,
    pub(crate) edge: mt::EdgeID,
    /// The farthest of the boundaries looked at ([`wall_clearance`]: whether the walls' far end
    /// is all at one distance); `at` elsewhere.
    pub(crate) far: f64,
}

fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

/// An edge as points along it within `tol` (its ends for a line), in the order it is used.
fn polyline(edge: &mt::Edge, tol: f64) -> Option<Vec<Vec3>> {
    use mt::{BoundedCurve, ParameterDivision1D};
    let mut pts: Vec<Vec3> = if matches!(edge.curve(), mt::Curve::Line(_)) {
        vec![vtx(edge.absolute_front()), vtx(edge.absolute_back())]
    } else {
        let curve = edge.curve();
        curve.parameter_division(curve.range_tuple(), tol * 0.25).1.into_iter().map(from_p3).collect()
    };
    if pts.len() < 2 || pts.len() > 100_000 {
        return None;
    }
    if !edge.orientation() {
        pts.reverse();
    }
    Some(pts)
}

/// The first place where the segment from `p` (on the boundary of the planar `face`) to `q`
/// leaves the face, or touches its boundary, beyond `p` (by more than `near`: the boundary `p`
/// lies on, found within the fit of its curve): `Some(None)` when it stays on the face, `None`
/// when that can't be told.
pub(crate) fn face_exit(face: &mt::Face, p: Vec3, q: Vec3, tol: f64, near: f64) -> Option<Option<Exit>> {
    let tol = tol.max(4e-6);
    let (x, n) = ((q - p).normalized()?, plane_normal(face)?);
    let y = n.cross(x);
    let len = p.dist(q);
    if !len.is_finite() {
        return None;
    }
    let mut best: Option<Exit> = None;
    for (wire, w) in face.boundaries().iter().enumerate() {
        for edge in w.edge_iter() {
            let pts = polyline(edge, tol)?;
            for pair in pts.windows(2) {
                let (a, b) = (*pair.first()? - p, *pair.get(1)? - p);
                let (ax, ay, bx, by) = (a.dot(x), a.dot(y), b.dot(x), b.dot(y));
                if ![ax, ay, bx, by].iter().all(|v| v.is_finite()) {
                    return None;
                }
                let ay = if ay.abs() <= tol { 0.0 } else { ay };
                let by = if by.abs() <= tol { 0.0 } else { by };
                let mut hits = Vec::with_capacity(2);
                if ay == 0.0 {
                    hits.push(ax);
                }
                if by == 0.0 {
                    hits.push(bx);
                }
                if ay * by < 0.0 {
                    hits.push(ax - ay * (bx - ax) / (by - ay));
                }
                for at in hits {
                    if at > tol.max(near) && at <= len + tol && best.is_none_or(|e| at < e.at) {
                        best = Some(Exit { at, wire, edge: edge.id(), far: at });
                    }
                }
            }
        }
    }
    Some(best)
}

/// [`face_exit`] up a wall turning about the line (`c`, `axis`) (a cylinder or cone): from `p` on
/// it along its straight line `t` (unit) for `len`. Each boundary point is placed by its angle
/// about the axis from `p` and how far along that line through it it lies.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wall_exit(wall: &mt::Face, c: Vec3, axis: Vec3, p: Vec3, t: Vec3, len: f64, tol: f64, near: f64) -> Option<Option<Exit>> {
    let tol = tol.max(4e-6);
    let k = axis.normalized()?;
    let z0 = (p - c).dot(k);
    let radial = p - c - k * z0;
    let r0 = radial.len();
    let e = radial.normalized()?;
    let w = k.cross(e);
    let (tr, tz) = (t.dot(e), t.dot(k));
    let place = |x: Vec3| {
        let v = x - c;
        let z = v.dot(k);
        let r = (v - k * z).len();
        let at = if tz.abs() >= tr.abs() { (z - z0) / tz } else { (r - r0) / tr };
        (v.dot(w).atan2(v.dot(e)), at)
    };
    let tol_angle = tol / r0.max(tol);
    let mut best: Option<Exit> = None;
    for (wire, lp) in wall.boundaries().iter().enumerate() {
        for edge in lp.edge_iter() {
            let pts = polyline(edge, tol)?;
            for pair in pts.windows(2) {
                let ((t1, l1), (t2, l2)) = (place(*pair.first()?), place(*pair.get(1)?));
                if ![t1, l1, t2, l2].iter().all(|v| v.is_finite()) {
                    return None;
                }
                // Across the far side of the axis, not through p's line.
                if (t1 - t2).abs() > std::f64::consts::PI {
                    continue;
                }
                let t1 = if t1.abs() <= tol_angle { 0.0 } else { t1 };
                let t2 = if t2.abs() <= tol_angle { 0.0 } else { t2 };
                let mut hits = Vec::with_capacity(2);
                if t1 == 0.0 {
                    hits.push(l1);
                }
                if t2 == 0.0 {
                    hits.push(l2);
                }
                if t1 * t2 < 0.0 {
                    hits.push(l1 + (l2 - l1) * t1 / (t1 - t2));
                }
                for at in hits {
                    if at > (tol * 4.0).max(near) && at <= len + tol && best.is_none_or(|x| at < x.at) {
                        best = Some(Exit { at, wire, edge: edge.id(), far: at });
                    }
                }
            }
        }
    }
    Some(best)
}

/// How far a planar face reaches from a whole circle bounding it (about `c`, radius `r`, its
/// loop `rim`) before its other boundaries: the nearest of them, `at` that distance across the
/// face from the circle (the face lies outside the circle when `outward`, a hole's rim, else
/// inside it). `Some(None)` when it has no other boundary; `None` when that can't be told.
pub(crate) fn face_clearance(face: &mt::Face, rim: usize, c: Vec3, r: f64, outward: bool, tol: f64) -> Option<Option<Exit>> {
    let mut best: Option<Exit> = None;
    for (wire, w) in face.boundaries().iter().enumerate() {
        if wire == rim {
            continue;
        }
        for edge in w.edge_iter() {
            let pts = polyline(edge, tol.max(4e-6))?;
            let gap = if outward {
                pts.windows(2).filter_map(|x| Some(c.dist_to_segment(*x.first()?, *x.get(1)?))).fold(f64::INFINITY, f64::min) - r
            } else {
                r - pts.iter().map(|x| x.dist(c)).fold(0.0, f64::max)
            };
            if !gap.is_finite() {
                return None;
            }
            if best.is_none_or(|b| gap < b.at) {
                best = Some(Exit { at: gap, wire, edge: edge.id(), far: gap });
            }
        }
    }
    Some(best)
}

/// How far walls turning about the line (`c`, `axis`) reach from a whole circle along them
/// (from its point `p` along the wall's straight line `t`, unit): the nearest of their boundaries
/// other than the circle's edges `rim` and the seams along the walls. `Some(None)` when there is
/// none; `None` when that can't be told.
pub(crate) fn wall_clearance(walls: &[&mt::Face], rim: &[mt::EdgeID], c: Vec3, axis: Vec3, p: Vec3, t: Vec3, tol: f64) -> Option<Option<Exit>> {
    let tol = tol.max(4e-6);
    let k = axis.normalized()?;
    let z0 = (p - c).dot(k);
    let e = (p - c - k * z0).normalized()?;
    let r0 = (p - c - k * z0).len();
    let (tz, tr) = (t.dot(k), t.dot(e));
    // How far along the wall's line a point of it is, and its angle about the axis.
    let along = |x: Vec3| {
        let v = x - c;
        let z = v.dot(k);
        if tz.abs() >= tr.abs() { (z - z0) / tz } else { ((v - k * z).len() - r0) / tr }
    };
    let angle = |x: Vec3| {
        let v = x - c;
        v.dot(k.cross(e)).atan2(v.dot(e))
    };
    let mut best: Option<Exit> = None;
    let mut far = f64::NEG_INFINITY;
    for g in walls {
        for (wire, w) in g.boundaries().iter().enumerate() {
            for edge in w.edge_iter() {
                if rim.contains(&edge.id()) {
                    continue;
                }
                let pts = polyline(edge, tol)?;
                // A seam along the wall: a straight line at one angle about the axis.
                if let (true, Some(a), Some(b)) = (matches!(edge.curve(), mt::Curve::Line(_)), pts.first(), pts.last())
                    && (angle(*a) - angle(*b)).abs() < 1e-9
                {
                    continue;
                }
                for x in &pts {
                    let at = along(*x);
                    if !at.is_finite() {
                        return None;
                    }
                    if at > tol * 4.0 {
                        far = far.max(at);
                        if best.is_none_or(|b| at < b.at) {
                            best = Some(Exit { at, wire, edge: edge.id(), far: at });
                        }
                    }
                }
            }
        }
    }
    Some(best.map(|b| Exit { far, ..b }))
}

/// Whether loop `wire` of a planar face (outward normal `n`) is a circle about `c`.
pub(crate) fn coaxial_circle(face: &mt::Face, wire: usize, c: Vec3, n: Vec3, tol: f64) -> bool {
    let Some(w) = face.boundaries().get(wire).cloned() else { return false };
    let mut radius: Option<f64> = None;
    for edge in w.edge_iter() {
        let Some(pts) = polyline(edge, 1e-4) else { return false };
        for x in pts {
            let v = x - c;
            if v.dot(n).abs() > tol {
                return false;
            }
            let r = v.len();
            if radius.is_some_and(|r0| (r - r0).abs() > tol) {
                return false;
            }
            radius = Some(r);
        }
    }
    radius.is_some()
}

/// The outer loop of a planar face (its index in the face's boundaries): the one enclosing the
/// largest area.
pub(crate) fn outer_loop(face: &mt::Face) -> Option<usize> {
    let n = plane_normal(face)?;
    let x = n.any_perp();
    let y = n.cross(x);
    let mut best: Option<(usize, f64)> = None;
    for (i, w) in face.boundaries().iter().enumerate() {
        let mut twice = 0.0;
        for edge in w.edge_iter() {
            let pts = polyline(edge, 1e-4)?;
            for pair in pts.windows(2) {
                let (a, b) = (*pair.first()?, *pair.get(1)?);
                twice += a.dot(x) * b.dot(y) - b.dot(x) * a.dot(y);
            }
        }
        let area = twice.abs() * 0.5;
        if best.is_none_or(|(_, b)| area > b) {
            best = Some((i, area));
        }
    }
    best.map(|(i, _)| i)
}

/// A tool's faces other than its chamfer face, to find in a result (where none may be left).
enum Sides {
    /// A wedge swept along an edge from `p0` (direction `d`, `len` long), its cross-section the
    /// triangle `o`, `a`, `b` (offsets across the edge), running `e0` before and `e1` past it.
    Wedge { p0: Vec3, d: Vec3, len: f64, o: Vec3, a: Vec3, b: Vec3, e0: f64, e1: f64, delta: f64 },
    /// Segments in (distance from the axis, height along it) turned about the axis (`centre`,
    /// `axis`).
    Ring { centre: Vec3, axis: Vec3, segs: Vec<(Vec2, Vec2)>, delta: f64 },
}

/// Whether `x` lies within `reach` of the segment `a` → `b`, between its ends.
fn on_segment3(x: Vec3, a: Vec3, b: Vec3, reach: f64) -> bool {
    let ab = b - a;
    let u = (x - a).dot(ab) / ab.len2().max(1e-300);
    (-0.01..=1.01).contains(&u) && (a + ab * u).dist(x) <= reach
}

/// Whether `x` (in the triangle's plane) lies in the triangle `o`, `a`, `b`, or within `reach` of it.
fn in_triangle(x: Vec3, o: Vec3, a: Vec3, b: Vec3, reach: f64) -> bool {
    let n = (a - o).cross(b - o);
    let inside = [(o, a), (a, b), (b, o)].iter().all(|(p, q)| (*q - *p).cross(x - *p).dot(n) >= 0.0);
    inside || on_segment3(x, o, a, reach) || on_segment3(x, a, b, reach) || on_segment3(x, b, o, reach)
}

fn on_segment2(x: Vec2, a: Vec2, b: Vec2, reach: f64) -> bool {
    let ab = b - a;
    let u = (x - a).dot(ab) / ab.len2().max(1e-300);
    (-0.01..=1.01).contains(&u) && (a + ab * u).dist(x) <= reach
}

impl Sides {
    /// How far the tool's other faces stand off the faces beside the edge.
    fn delta(&self) -> f64 {
        match *self {
            Sides::Wedge { delta, .. } | Sides::Ring { delta, .. } => delta,
        }
    }

    /// Whether `x` (a point of a result face) lies within `reach` of one of the tool's faces
    /// other than its chamfer face.
    fn holds(&self, x: Vec3, reach: f64) -> bool {
        match *self {
            Sides::Wedge { p0, d, len, o, a, b, e0, e1, .. } => {
                let v = x - p0;
                let along = v.dot(d);
                let across = v - d * along;
                let cap = (along + e0).abs() <= reach || (along - len - e1).abs() <= reach;
                on_segment3(across, o, a, reach) || on_segment3(across, o, b, reach) || (cap && in_triangle(across, o, a, b, reach))
            }
            Sides::Ring { centre, axis, ref segs, .. } => {
                let v = x - centre;
                let z = v.dot(axis);
                let q = Vec2::new((v - axis * z).len(), z);
                segs.iter().any(|(a, b)| on_segment2(q, *a, *b, reach))
            }
        }
    }
}

/// The cross-section triangle `(o', a', b')` of a chamfer from the edge at the origin: the
/// setback points `a` (on face 1, whose outward normal is `n1`) and `b` (on face 2, `n2`), with
/// the faces' lines moved `delta` outwards and the chamfer line run on to meet them. `None` unless
/// the edge is convex.
fn triangle<V>(a: V, b: V, n1: V, n2: V, delta: f64, dot: impl Fn(V, V) -> f64) -> Option<(V, V, V)>
where
    V: Copy + std::ops::Add<Output = V> + std::ops::Sub<Output = V> + std::ops::Mul<f64, Output = V>,
{
    let (da, db) = (dot(n1, a - b), dot(n2, b - a));
    let k = 1.0 + dot(n1, n2);
    if !(da > 1e-12 && db > 1e-12 && k > 1e-9) {
        return None;
    }
    let o = (n1 + n2) * (delta / k);
    Some((o, a + (a - b) * (delta / da), b + (b - a) * (delta / db)))
}

/// The cross-section polygon, counter-clockwise, and the index of its chamfer segment (from the
/// first point to the second).
fn ccw(pts: &[Vec2]) -> (Loop2, usize) {
    let lp = Loop2::polygon(pts);
    if lp.signed_area() < 0.0 {
        let lp = lp.reversed();
        let last = lp.segs.len().saturating_sub(1);
        (lp, last)
    } else {
        (lp, 0)
    }
}

/// The face of a swept tool made from profile segment `segment`.
fn swept_face(body: &Body, segment: usize) -> Option<usize> {
    body.history().and_then(|h| h.faces.iter().position(|f| f.origins == [FaceOrigin::Profile { segment }]))
}

/// The tools for the picks, and the faces to delete before the cut (walls of holes the chamfers
/// use up, see [`ring`]).
type Tools = (Vec<(Cutter, Sides)>, Vec<usize>);

/// The tools for the picks, or why there are none: `Ok(None)` when they are not a case for a cut
/// through (the exact path's refusal stands).
fn tools(cur: &Body, edges: &[Vec3], d1: f64, sides: Option<(ChamferSide, bool)>) -> Result<Option<Tools>> {
    let size = cur.size();
    let (groups, rest) = loop_split(cur, edges)?.unwrap_or_else(|| (Vec::new(), edges.to_vec()));
    let mut out = Vec::new();
    let mut fill = Vec::new();
    let mut straight = rest;
    let mut past = false;
    for g in &groups {
        let solid = cur.deep_copy();
        let ids = edge_ids(cur, &solid, g)?;
        let Some(rim) = crate::loopblend::rim(&solid, &ids, size, d1, sides) else {
            straight.extend(g.iter().copied());
            continue;
        };
        let rim = rim?;
        if !rim.convex || rim.reach == Reach::Blocked {
            return Ok(None);
        }
        past |= rim.reach == Reach::Past;
        let edge = Body::unique_edges(&solid).iter().position(|e| e.id() == rim.edge).ok_or_else(|| KernelError::Failed("rim edge".into()))?;
        let (tool, filled) = ring(&rim, size, edge)?;
        out.push(tool);
        if filled {
            fill.extend(rim.walls.iter().copied());
        }
    }
    for p in &straight {
        let Ok((edge, mut f)) = edge_frame(cur, *p) else { return Ok(None) };
        let (s1, s2) = side_setbacks(&mut f, d1, sides)?;
        match wedge(cur, edge, &f, s1, s2, size)? {
            Some((Some(tool), reach)) => {
                past |= reach == Reach::Past;
                out.push(tool);
            }
            _ => return Ok(None),
        }
    }
    Ok((past && !out.is_empty()).then_some((out, fill)))
}

/// A rim's tool: the cross-section triangle turned about its axis. A hole's chamfer that uses up
/// its wall exactly passes through the wall's far rim, where the boolean cannot meet it: the hole
/// is filled first (its walls deleted, `true`) and the tool is the whole cone, from the axis out,
/// crossing the far face where the rim was.
fn ring(rim: &crate::loopblend::Rim, size: f64, edge: usize) -> Result<((Cutter, Sides), bool)> {
    let o = Vec2::new(rim.radius, 0.0);
    let (m, t) = (Vec2::new(rim.m.0, rim.m.1), Vec2::new(rim.t.0, rim.t.1));
    let (a, b) = (o + m * rim.sf, o + t * rim.sw);
    // The face's outward normal is the axis; the wall's is square to it, away from the material.
    let nf = Vec2::new(0.0, 1.0);
    let mut nw = Vec2::new(-t.y, t.x);
    if nw.dot(m) > 0.0 {
        nw = nw * -1.0;
    }
    let delta = (size * 1e-3).max(1e-6).min(rim.radius * 0.05).min(rim.sf.min(rim.sw) * 0.05);
    let (o1, a1, b1) = triangle(a - o, b - o, nf, nw, delta, |x: Vec2, y: Vec2| x.dot(y))
        .ok_or_else(|| unsupported("the chamfer's faces meet at an angle it can't cut"))?;
    let (o1, a1, b1) = (o + o1, o + a1, o + b1);
    if [o1, a1, b1].iter().any(|q| q.x <= delta) {
        return Err(KernelError::TooLarge("the chamfer would run across the axis and end in a point".into()));
    }
    let fill = rim.used_up && m.x > 0.0;
    let (pts, segs): (Vec<Vec2>, Vec<(Vec2, Vec2)>) = if fill {
        // On along the chamfer line past the far rim, clear of the far face, then to the axis.
        let away = b - a;
        let room = 0.5 * b.x / (a.x - b.x).max(1e-300);
        let step = ((2.0 * delta) / away.y.abs().max(1e-300)).min(room);
        let far = b + away * step;
        if (far.y - b.y).abs() < delta * 1.5 {
            return Err(unsupported("a chamfer using up a hole's wall this steeply"));
        }
        let (axis_far, axis_near) = (Vec2::new(0.0, far.y), Vec2::new(0.0, a1.y));
        (vec![a1, far, axis_far, axis_near], vec![(far, axis_far), (axis_near, a1)])
    } else {
        (vec![a1, b1, o1], vec![(o1, a1), (o1, b1)])
    };
    // The section's plane: radially out at an odd angle (so the tool's seam lies away from the
    // planes the body tends to be cut along) and along the axis.
    let k = rim.axis;
    let r0 = rim.axis.any_perp();
    let (s, co) = 0.4142f64.sin_cos();
    let x = r0 * co + k.cross(r0) * s;
    let plane = Plane { origin: rim.centre, x, y: k };
    let (lp, chamfer) = ccw(&pts);
    let body = crate::build::revolve(&plane, &[Region2 { outer: lp, holes: vec![] }], Vec2::ZERO, Vec2::new(0.0, 1.0), std::f64::consts::TAU)?
        .pop()
        .ok_or_else(|| KernelError::Failed("chamfer tool".into()))?;
    // Filled, the rim's edge is gone: the chamfer face is new.
    let face = if fill { None } else { swept_face(&body, chamfer) };
    Ok(((Cutter { body, face, edge, extension_inside: false }, Sides::Ring { centre: rim.centre, axis: k, segs, delta }), fill))
}

/// A straight edge's tool (none where it would run into other geometry) and where its setbacks
/// reach; `None` when the edge is not between two faces.
fn wedge(cur: &Body, edge: usize, f: &crate::blend::EdgeFrame, s1: f64, s2: f64, size: f64) -> Result<Option<(Option<(Cutter, Sides)>, Reach)>> {
    let solid = &*cur.solid;
    let Some(e) = Body::unique_edges(solid).into_iter().nth(edge) else { return Ok(None) };
    let beside: Vec<&mt::Face> = solid.face_iter().filter(|g| g.edge_iter().any(|x| x.id() == e.id())).collect();
    let [g1, g2] = beside[..] else { return Ok(None) };
    // Face 1 is the one `f.n1` is the normal of.
    let (face1, face2) = if plane_normal(g1).is_some_and(|n| n.dot(f.n1) > 1.0 - 1e-6) { (g1, g2) } else { (g2, g1) };
    let len = f.p0.dist(f.p1);
    // Where the setbacks end, from points along the edge.
    let tol = (size * 1e-7).max(4e-6);
    let mut reach = Reach::Within;
    for (g, t, s) in [(face1, f.t1, s1), (face2, f.t2, s2)] {
        let outer = outer_loop(g);
        for k in 0..8 {
            let p = f.p0 + (f.p1 - f.p0) * ((f64::from(k) + 0.5) / 8.0);
            let Some(exit) = face_exit(g, p, p + t * s, tol, tol * 10.0) else { return Ok(None) };
            if let Some(x) = exit {
                reach = reach.max(if Some(x.wire) == outer { Reach::Past } else { Reach::Blocked });
            }
        }
    }
    if reach == Reach::Blocked {
        return Ok(Some((None, reach)));
    }
    let delta = (size * 1e-3).max(1e-6).min(s1.min(s2) * 0.05);
    let (o, a, b) = triangle(f.t1 * s1, f.t2 * s2, f.n1, f.n2, delta, |x: Vec3, y: Vec3| x.dot(y))
        .ok_or_else(|| unsupported("the chamfer's faces meet at an angle it can't cut"))?;
    // Each end: the one face there besides the two along the edge, planar and leaning away from
    // the edge, which trims the tool's run past the end; the tool stops a little past that face.
    let margin = (size * 0.01).max(delta * 10.0);
    let mesh = cur.tessellate((size * 1e-3).max(1e-6))?;
    let eps = (size * 1e-6).max(1e-9);
    let mut ends = [0.0f64; 2];
    for (slot, (v, out)) in ends.iter_mut().zip([(e.absolute_front().clone(), f.d * -1.0), (e.absolute_back().clone(), f.d)]) {
        let at_end: Vec<&mt::Face> =
            solid.face_iter().filter(|g| !std::ptr::eq(*g, face1) && !std::ptr::eq(*g, face2) && g.vertex_iter().any(|x| x == v)).collect();
        let ne = match at_end[..] {
            [g] => plane_normal(g),
            _ => None,
        };
        let Some(ne) = ne.filter(|ne| ne.dot(out) >= 0.05) else {
            return Err(unsupported("the edge ends at a concave corner or at more than one face, which the cut would run into"));
        };
        let past = [o, a, b].iter().map(|w| -ne.dot(*w) / ne.dot(out)).fold(0.0f64, f64::max);
        *slot = past + margin;
        // Nothing of the body beyond that face, within the tool.
        let pv = vtx(&v);
        let mut planes = prism_planes(pv, f.d, [o, a, b]);
        planes.push((ne * -1.0, -ne.dot(pv)));
        planes.push((out, out.dot(pv) + *slot));
        let hit = mesh.triangles.iter().filter_map(|t| mesh.tri(t)).any(|t| !crate::blend_fit::clip(&t, &planes, eps).is_empty());
        if hit {
            return Err(unsupported("the cut would run on past the end of the edge into other geometry"));
        }
    }
    let [e0, e1] = ends;
    let base = f.p0 - f.d * e0;
    let plane = Plane::from_normal(base, f.d).ok_or_else(|| KernelError::Failed("chamfer plane".into()))?;
    let (lp, chamfer) = ccw(&[plane.to_local(base + a), plane.to_local(base + b), plane.to_local(base + o)]);
    let body = crate::build::extrude(&plane, &[Region2 { outer: lp, holes: vec![] }], 0.0, len + e0 + e1)?
        .pop()
        .ok_or_else(|| KernelError::Failed("chamfer tool".into()))?;
    let face = swept_face(&body, chamfer);
    Ok(Some((Some((Cutter { body, face, edge, extension_inside: false }, Sides::Wedge { p0: f.p0, d: f.d, len, o, a, b, e0, e1, delta })), reach)))
}

/// The half-spaces `n·x <= c` of the prism swept along `d` from the triangle `tri` (offsets
/// from `p`).
fn prism_planes(p: Vec3, d: Vec3, tri: [Vec3; 3]) -> Vec<(Vec3, f64)> {
    let mut out = Vec::with_capacity(5);
    for i in 0..3 {
        let (Some(a), Some(b), Some(c)) = (tri.get(i), tri.get((i + 1) % 3), tri.get((i + 2) % 3)) else { continue };
        let Some(mut n) = d.cross(*b - *a).normalized() else { continue };
        if n.dot(*c - *a) > 0.0 {
            n = n * -1.0;
        }
        out.push((n, n.dot(p + *a)));
    }
    out
}

/// A chamfer of `d1` (the second side by `sides`) of the edges nearest `edges`, cut through the
/// faces it reaches: `Ok(None)` when that is not the case here (concave edges, setbacks within
/// their faces or running into other geometry, edges other than straight ones between planes and
/// whole circular rims), an error when the cut can't be made or its result fails the checks.
pub(crate) fn cut(body: &Body, edges: &[Vec3], d1: f64, sides: Option<(ChamferSide, bool)>) -> Result<Option<Body>> {
    if body.is_mesh() || edges.is_empty() {
        return Ok(None);
    }
    let counts = record::counts(&[body]);
    let cur = crate::boolean_record::healed(body)?;
    let Some((tools, fill)) = guard("chamfer through", || tools(&cur, edges, d1, sides))? else { return Ok(None) };
    // Holes whose walls a chamfer uses up, filled (history over `cur`).
    let target = if fill.is_empty() { cur.clone() } else { crate::delete_face::delete_faces(&cur, &fill)? };
    // The union of the tools (meeting ones mitre), cut from the body.
    let mut acc: Option<(Body, Option<History>)> = None;
    for (t, _) in &tools {
        acc = Some(match acc {
            None => (t.body.clone(), Some(History::identity(&t.body))),
            Some((a, h)) => {
                let u = crate::ops::boolean(&a, &t.body, crate::BoolOp::Union)?
                    .ok_or_else(|| KernelError::Failed("the chamfer tools did not join".into()))?;
                let h = crate::boolean_record::through(&u, 0, h.as_ref());
                (u, h)
            }
        });
    }
    let (tool, h) = acc.ok_or_else(|| KernelError::Failed("no chamfer tool".into()))?;
    let made =
        crate::ops::boolean(&target, &tool, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("the chamfer removed everything".into()))?;
    // Each chamfer face in place of its edge; after a fill the edges' order has changed, and the
    // faces are new.
    let filled = !fill.is_empty();
    let cutters: Vec<Cutter> =
        tools.iter().map(|(t, _)| Cutter { body: t.body.clone(), face: t.face.filter(|_| !filled), edge: t.edge, extension_inside: false }).collect();
    let h = cut_history(crate::boolean_record::through(&made, 1, h.as_ref()), &target, &cutters);
    let made = made.with_checked_history(h, &record::counts(&[&target]));
    let made = if filled { record::after(made, &target, &record::counts(&[&cur])) } else { made };
    let others: Vec<&Sides> = tools.iter().map(|(_, s)| s).collect();
    check(&cur, &made, &others)?;
    Ok(Some(record::after(made, &cur, &counts)))
}

/// Refuse a cut that isn't a chamfer of `cur`: an invalid solid, one in more pieces, one no
/// smaller, one sticking out of its bounds, or one where a face of a tool other than its chamfer
/// face is left (the tool ran into material beyond the faces' planes: an undercut).
fn check(cur: &Body, out: &Body, sides: &[&Sides]) -> Result<()> {
    let bad = |why: &str| Err(KernelError::Failed(format!("the cut through would {why}")));
    if !out.validity().is_empty() {
        return bad("not leave a valid solid");
    }
    if out.lumps()?.len() != cur.lumps()?.len() {
        return bad("cut the part in pieces");
    }
    let (before, after) = (crate::measure(cur)?, crate::measure(out)?);
    if !(after.volume < before.volume - before.volume.abs() * 1e-9) {
        return bad("not take anything away");
    }
    let size = cur.size();
    // Finer than the tools stand off the faces, so a face on a tool's side is told from one beside it.
    let delta = sides.iter().map(|s| s.delta()).fold(f64::INFINITY, f64::min);
    let chord = (size * 1e-4).min(delta / 12.0).max(1e-6);
    let (m0, m1) = (cur.tessellate(chord)?, out.tessellate(chord)?);
    let (b0, b1) = (m0.bounds(), m1.bounds());
    // Meshes of faces trimmed by fitted curves stray a few chords; a chamfer sticking out of the
    // part (what this is for) does so by far more.
    let slack = (chord * 10.0).max(size * 5e-4);
    if b1.min.x < b0.min.x - slack
        || b1.min.y < b0.min.y - slack
        || b1.min.z < b0.min.z - slack
        || b1.max.x > b0.max.x + slack
        || b1.max.y > b0.max.y + slack
        || b1.max.z > b0.max.z + slack
    {
        return bad("stick out of the part");
    }
    // Each face of the result at the middle of its largest triangle.
    let mut largest: Vec<Option<(f64, Vec3)>> = vec![None; out.face_count()];
    for (t, fi) in m1.triangles.iter().zip(&m1.tri_face) {
        let Some([a, b, c]) = m1.tri(t) else { continue };
        let area = (b - a).cross(c - a).len();
        if let Some(slot) = largest.get_mut(*fi as usize)
            && slot.is_none_or(|(x, _)| area > x)
        {
            *slot = Some((area, (a + b + c) / 3.0));
        }
    }
    if largest.iter().flatten().any(|(_, x)| sides.iter().any(|s| s.holds(*x, s.delta() / 3.0))) {
        return bad("leave material undercut beyond the faces beside the edge (a face of the cutting tool is left in the part)");
    }
    Ok(())
}
