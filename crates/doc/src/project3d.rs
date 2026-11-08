//! 3D sketch curves from the model: curves projected onto faces (on their display meshes), the
//! sides an intersection curve is taken on (bodies, faces and other sketches' curves), and spun profiles.

use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{LinkSource, WireCurve};

use crate::eval::ModelState;

/// The body of a body or face reference and, for a face reference, the faces (indices in the
/// body's face order) of the freeform surface its face is part of ([`smooth_patch`]), on the named
/// body or else the nearest body, so a spline wall of one face per span meets as a whole (#9009).
fn part_of<'a>(st: &'a ModelState, src: &LinkSource) -> Option<(&'a crate::ModelBody, Option<Vec<usize>>)> {
    match src {
        LinkSource::Body { body } => Some((st.body(body)?, None)),
        LinkSource::Face { body, at } => {
            let b = st.body(body).or_else(|| st.bodies.iter().min_by(|x, y| dist_to(&x.mesh(), *at).total_cmp(&dist_to(&y.mesh(), *at))))?;
            let (f, _, _) = crate::project::nearest_face(&b.mesh(), *at)?;
            Some((b, Some(smooth_patch(&b.body, usize::try_from(f).ok()?))))
        }
        _ => None,
    }
}

/// The faces one freeform surface with `face` makes, sorted: `face` and the spline faces joined to
/// it at least tangentially (G1, measured on the exact surfaces), never across a crease. A
/// fit-point spline's wall is one face per span, and a pick on any span means the whole wall; a
/// plane, a fillet or any other analytic face is the face alone, as are faces whose seams cannot
/// be measured.
fn smooth_patch(body: &crate::kernel::Body, face: usize) -> Vec<usize> {
    use solidcore_record::brep::Surface;
    let r = body.record();
    let freeform = |f: usize| r.faces.get(f).is_some_and(|f| matches!(f.surface, Surface::Spline { .. } | Surface::RationalSpline { .. }));
    if !freeform(face) {
        return vec![face];
    }
    // The two-sided edges between freeform faces, the only seams worth measuring.
    let between: Vec<(usize, [usize; 2])> = r
        .edges
        .iter()
        .enumerate()
        .filter_map(|(ei, e)| {
            let mut fs: Vec<usize> = e.uses.iter().map(|u| u.face as usize).collect();
            fs.sort_unstable();
            fs.dedup();
            match fs[..] {
                [x, y] if freeform(x) && freeform(y) => Some((ei, [x, y])),
                _ => None,
            }
        })
        .collect();
    let edges: Vec<usize> = between.iter().map(|(ei, _)| *ei).collect();
    let tangent: std::collections::BTreeSet<usize> = if edges.is_empty() {
        Default::default()
    } else {
        crate::kernel::seam_diagnostics(body, &edges, crate::kernel::MIN_SAMPLES, crate::kernel::ContinuityTolerances::default())
            .map(|rep| {
                rep.seams
                    .iter()
                    .filter(|s| matches!(s.verdict, crate::kernel::SeamVerdict::Smooth | crate::kernel::SeamVerdict::CurvatureJump))
                    .map(|s| s.edge)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut patch = std::collections::BTreeSet::from([face]);
    let mut todo = vec![face];
    while let Some(f) = todo.pop() {
        for (_, pair) in between.iter().filter(|(ei, pair)| tangent.contains(ei) && pair.contains(&f)) {
            for next in pair {
                if patch.insert(*next) {
                    todo.push(*next);
                }
            }
        }
    }
    patch.into_iter().collect()
}

/// One side of an intersection curve: a body (or a face patch of it), or a curve of another sketch.
enum Side<'a> {
    Part(&'a crate::ModelBody, Option<Vec<usize>>),
    Curve { plane: Plane, segs: Vec<Seg2>, pts: Vec<Vec3> },
}

fn side_of<'a>(st: &'a ModelState, src: &LinkSource) -> Option<Side<'a>> {
    if let Some((b, f)) = part_of(st, src) {
        return Some(Side::Part(b, f));
    }
    let LinkSource::SketchCurve { sketch, curve } = src else { return None };
    let ss = st.sketch(*sketch)?;
    let ci = ss.sketch.curve_index(curve)?;
    let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
    let pts: Vec<Vec3> = ss.sketch.segs(ci).iter().flat_map(|s| s.polyline(1e-3)).map(|p| ss.plane.to_world(p)).collect();
    (!segs.is_empty() && pts.len() >= 2).then_some(Side::Curve { plane: ss.plane, segs, pts })
}

/// Bounds of a point set; `None` when empty or not finite.
fn bounds_of<'a>(pts: impl IntoIterator<Item = &'a Vec3>) -> Option<(Vec3, Vec3)> {
    let mut it = pts.into_iter();
    let first = *it.next()?;
    let (lo, hi) = it.fold((first, first), |(l, h), p| (l.min(*p), h.max(*p)));
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// The offsets along `plane`'s unit normal `n` that cover the box `lo`..`hi`, padded.
fn span_over(plane: &Plane, n: Vec3, lo: Vec3, hi: Vec3) -> (f64, f64) {
    let corners =
        (0..8).map(|i| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }));
    let (s0, s1) = corners.map(|c| (c - plane.origin).dot(n)).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), s| (l.min(s), h.max(s)));
    let pad = (hi - lo).len() * 0.02 + 1e-3;
    (s0 - pad, s1 + pad)
}

/// Where intersection sides `a` and `b` meet, as whole curves ([`join_meeting`]): the edges
/// SMLib's imprint of one by the other makes, each its exact record curve, its points at the
/// display tolerance. A sketch curve stands in for its extrusion along its sketch's normal, both
/// ways and as far as the other side reaches: a sheet made here for this alone and never added to
/// the model (#11048).
pub(crate) fn meeting(st: &ModelState, a: &LinkSource, b: &LinkSource) -> crate::Result<Vec<(Vec<Vec3>, Vec<WireCurve>)>> {
    let lost = |s: &LinkSource| crate::DocError::Unknown(format!("{} no longer exists", s.describe()));
    let (sa, sb) = (side_of(st, a).ok_or_else(|| lost(a))?, side_of(st, b).ok_or_else(|| lost(b))?);
    let unit = |p: &Plane, s: &LinkSource| p.normal().normalized().ok_or_else(|| lost(s));
    // How far each curve is swept: across the other side's bounds, or for two curves as far as a
    // crossing can lie (a + s·na = b + t·nb with a, b within the curves' bounds, so |s|·sin ≤ |a − b|).
    let (ra, rb) = match (&sa, &sb) {
        (Side::Curve { plane: pa, pts: qa, .. }, Side::Curve { plane: pb, pts: qb, .. }) => {
            let sin = unit(pa, a)?.cross(unit(pb, b)?).len();
            if sin < 1e-6 {
                return Err(crate::DocError::Invalid("the two curves' sketches are parallel, so their extrusions never cross in a curve".into()));
            }
            let (lo, hi) = bounds_of(qa.iter().chain(qb)).ok_or_else(|| lost(a))?;
            let reach = (hi - lo).len() / sin * 1.05 + 1e-3;
            ((-reach, reach), (-reach, reach))
        }
        (Side::Curve { plane, .. }, Side::Part(mb, _)) => {
            let m = mb.body.exact_bounds()?;
            (span_over(plane, unit(plane, a)?, m.min, m.max), (0.0, 0.0))
        }
        (Side::Part(mb, _), Side::Curve { plane, .. }) => {
            let m = mb.body.exact_bounds()?;
            ((0.0, 0.0), span_over(plane, unit(plane, b)?, m.min, m.max))
        }
        (Side::Part(..), Side::Part(..)) => ((0.0, 0.0), (0.0, 0.0)),
    };
    let sheet = |s: &Side, (s0, s1): (f64, f64)| -> crate::Result<Option<crate::kernel::Body>> {
        match s {
            Side::Curve { plane, segs, .. } => Ok(Some(crate::kernel::extrude_sheet(plane, std::slice::from_ref(segs), s0, s1)?)),
            Side::Part(..) => Ok(None),
        }
    };
    let (ka, kb) = (sheet(&sa, ra)?, sheet(&sb, rb)?);
    let (Some((ba, fa)), Some((bb, fb))) = (body_of(&sa, ka.as_ref()), body_of(&sb, kb.as_ref())) else { return Err(lost(a)) };
    let chord = (ba.size().max(bb.size()) * 1e-3).max(1e-3);
    // The pieces end at the imprint's shared vertices, which the kernel places within its
    // intersection tolerance; ten times that, as when it sews sheets, joins them and nothing else.
    let tol = 10.0 * ba.record().tolerance.max(bb.record().tolerance).max(1e-9);
    Ok(join_meeting(crate::kernel::meeting_edges(ba, fa, bb, fb, chord)?, tol))
}

/// The kernel body a side is intersected as: its model body (and face patch), or the curve's `sheet`.
fn body_of<'b>(side: &'b Side<'b>, sheet: Option<&'b crate::kernel::Body>) -> Option<(&'b crate::kernel::Body, Option<&'b [usize]>)> {
    match side {
        Side::Part(mb, f) => Some((&mb.body, f.as_deref())),
        Side::Curve { .. } => sheet.map(|k| (k, None)),
    }
}

/// The meeting edges as whole curves: edges whose ends meet (within `tol`) joined end to end, each
/// curve its points and its exact pieces in order along them. The imprint splits a crossing at
/// every face boundary of either side, so one crossing of two spline walls comes as one edge per
/// span; it is one curve.
fn join_meeting(edges: Vec<crate::kernel::MeetingEdge>, tol: f64) -> Vec<(Vec<Vec3>, Vec<WireCurve>)> {
    let mut left: Vec<(Vec<Vec3>, Vec<WireCurve>)> =
        edges.into_iter().filter(|e| !e.points.is_empty()).map(|e| (e.points, vec![WireCurve { curve: e.curve, interval: e.interval }])).collect();
    let flip = |(mut p, c): (Vec<Vec3>, Vec<WireCurve>)| {
        p.reverse();
        (p, c.iter().rev().map(reversed).collect::<Vec<_>>())
    };
    let mut out = Vec::new();
    while let Some(mut cur) = left.pop() {
        while let (Some(cs), Some(ce)) = (cur.0.first().copied(), cur.0.last().copied()) {
            if cur.0.len() > 2 && cs.dist(ce) < tol {
                break;
            }
            let ends = |w: &(Vec<Vec3>, Vec<WireCurve>)| w.0.first().copied().zip(w.0.last().copied());
            let Some(i) = left.iter().position(|w| ends(w).is_some_and(|(s, e)| [s, e].iter().any(|q| q.dist(ce) < tol || q.dist(cs) < tol))) else {
                break;
            };
            let next = left.swap_remove(i);
            let Some((s, e)) = ends(&next) else { break };
            // Append after the end or prepend before the start, turning `next` to run on.
            let (mut head, tail) = if s.dist(ce) < tol {
                (cur, next)
            } else if e.dist(ce) < tol {
                (cur, flip(next))
            } else if e.dist(cs) < tol {
                (next, cur)
            } else {
                (flip(next), cur)
            };
            head.0.extend(tail.0.into_iter().skip(1));
            head.1.extend(tail.1);
            cur = head;
        }
        out.push(cur);
    }
    out
}

/// `w` run the other way: the same points, its parameter reversed (record curves have no
/// direction flag, so the curve itself is rewritten).
fn reversed(w: &WireCurve) -> WireCurve {
    use solidcore_record::brep::Curve;
    let [t0, t1] = w.interval;
    let mirror = |knots: &[f64]| {
        let (a, b) = (knots.first().copied().unwrap_or(0.0), knots.last().copied().unwrap_or(0.0));
        (knots.iter().rev().map(|k| a + b - k).collect::<Vec<_>>(), a + b)
    };
    let (curve, interval) = match &w.curve {
        Curve::Line { start, end } => (Curve::Line { start: *end, end: *start }, [1.0 - t1, 1.0 - t0]),
        Curve::Circle { frame, radius } => (Curve::Circle { frame: flipped(frame), radius: *radius }, [-t1, -t0]),
        Curve::Ellipse { frame, radii } => (Curve::Ellipse { frame: flipped(frame), radii: *radii }, [-t1, -t0]),
        Curve::Polyline { points } => {
            let n = points.len().saturating_sub(1) as f64;
            (Curve::Polyline { points: points.iter().rev().copied().collect() }, [n - t1, n - t0])
        }
        Curve::Spline { knots, points } => {
            let (k, s) = mirror(knots);
            (Curve::Spline { knots: k, points: points.iter().rev().copied().collect() }, [s - t1, s - t0])
        }
        Curve::RationalSpline { knots, homogeneous } => {
            let (k, s) = mirror(knots);
            (Curve::RationalSpline { knots: k, homogeneous: homogeneous.iter().rev().copied().collect() }, [s - t1, s - t0])
        }
        Curve::SurfaceIntersection { surfaces, leader, tolerance } => {
            let r = reversed(&WireCurve { curve: (**leader).clone(), interval: w.interval });
            (Curve::SurfaceIntersection { surfaces: surfaces.clone(), leader: Box::new(r.curve), tolerance: *tolerance }, r.interval)
        }
    };
    WireCurve { curve, interval }
}

/// `f` with its y axis turned over, so a circle or ellipse on it runs the other way.
fn flipped(f: &solidcore_record::brep::Frame) -> solidcore_record::brep::Frame {
    let [x, y, z] = f.y_axis;
    solidcore_record::brep::Frame { origin: f.origin, x_axis: f.x_axis, y_axis: [-x, -y, -z] }
}

fn dist_to(m: &Mesh, p: Vec3) -> f64 {
    crate::project::nearest_face(m, p).map(|x| x.2).unwrap_or(f64::INFINITY)
}

fn tris(m: &Mesh, face: Option<u32>) -> Vec<[Vec3; 3]> {
    m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| face.is_none_or(|x| x == **f)).filter_map(|(t, _)| m.tri(t)).collect()
}

/// Ray–triangle hit parameter (Möller–Trumbore), any sign.
fn ray_hit(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

/// Most triangles [`flat_outline`] looks through.
const FLAT_OUTLINE_TRIANGLES: usize = 2_000_000;

/// The outline of the flat patch of a mesh body's triangles at `at`: the triangle nearest `at`
/// and every triangle joined to it edge to edge on its plane, traced as closed loops, each with its
/// first point again at its end. An imported mesh is one face, so this flat patch is what a pick
/// on one of its faces means (#13464). Empty when the mesh has no triangle or is too large.
pub(crate) fn flat_outline(m: &Mesh, at: Vec3) -> Vec<Vec<Vec3>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    if m.triangles.is_empty() || m.triangles.len() > FLAT_OUTLINE_TRIANGLES {
        return Vec::new();
    }
    let size = m.bounds().diagonal().max(1e-9);
    // Corners at one place are one corner: an STL repeats each corner for every triangle.
    let weld = size * 1e-9;
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut corners: Vec<Vec3> = Vec::new();
    let mut tri: Vec<[usize; 3]> = Vec::with_capacity(m.triangles.len());
    for t in &m.triangles {
        let Some(ps) = m.tri(t) else { continue };
        let mut out = [0; 3];
        for (o, p) in out.iter_mut().zip(ps) {
            if !p.is_finite() {
                return Vec::new();
            }
            let key = ((p.x / weld).round() as i64, (p.y / weld).round() as i64, (p.z / weld).round() as i64);
            *o = *ids.entry(key).or_insert_with(|| {
                corners.push(p);
                corners.len() - 1
            });
        }
        tri.push(out);
    }
    let at_of = |t: &[usize; 3]| t.map(|i| corners.get(i).copied().unwrap_or_default());
    let normal = |t: &[usize; 3]| {
        let [a, b, c] = at_of(t);
        (b - a).cross(c - a).normalized()
    };
    // The triangle nearest the pick, and its plane.
    let Some(seed) = (0..tri.len()).filter(|i| tri.get(*i).and_then(normal).is_some()).min_by(|x, y| {
        let d = |i: &usize| {
            tri.get(*i).map_or(f64::INFINITY, |t| {
                let [a, b, c] = at_of(t);
                crate::project::closest_on_triangle(at, a, b, c).dist(at)
            })
        };
        d(x).total_cmp(&d(y))
    }) else {
        return Vec::new();
    };
    let Some((n0, p0)) = tri.get(seed).and_then(|t| Some((normal(t)?, at_of(t)[0]))) else { return Vec::new() };
    let flat = |t: &[usize; 3]| normal(t).is_some_and(|n| n.dot(n0) > 1.0 - 1e-6) && at_of(t).iter().all(|p| (*p - p0).dot(n0).abs() <= size * 1e-6);
    // Triangles by the edges they share.
    let mut by_edge: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, t) in tri.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    // The flat patch: from the seed across shared edges onto triangles on its plane.
    let mut patch: HashSet<usize> = HashSet::from([seed]);
    let mut queue = VecDeque::from([seed]);
    while let Some(i) = queue.pop_front() {
        let Some(t) = tri.get(i) else { continue };
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            for j in by_edge.get(&(a.min(b), a.max(b))).into_iter().flatten() {
                if !patch.contains(j) && tri.get(*j).is_some_and(flat) {
                    patch.insert(*j);
                    queue.push_back(*j);
                }
            }
        }
    }
    // Its outline: the edges only one of its triangles has, each the way its triangle runs.
    let mut next: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in &patch {
        let Some(t) = tri.get(*i) else { continue };
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let inside = by_edge.get(&(a.min(b), a.max(b))).map_or(0, |ts| ts.iter().filter(|j| patch.contains(j)).count());
            if inside == 1 {
                next.entry(a).or_default().push(b);
            }
        }
    }
    let mut loops = Vec::new();
    let mut starts: Vec<usize> = next.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        while let Some(mut v) = next.get_mut(&start).and_then(Vec::pop) {
            let mut pts: Vec<Vec3> = [start, v].iter().filter_map(|i| corners.get(*i).copied()).collect();
            let mut steps = 0;
            while v != start && steps < patch.len() * 3 {
                let Some(w) = next.get_mut(&v).and_then(Vec::pop) else { break };
                pts.extend(corners.get(w).copied());
                v = w;
                steps += 1;
            }
            if v == start && pts.len() >= 4 {
                loops.push(pts);
            }
        }
    }
    loops
}

/// Project a world polyline onto face `f` along `dir` (either way, nearest hit). Points that
/// miss the face split the result.
pub(crate) fn onto_face(m: &Mesh, f: u32, pts: &[Vec3], dir: Vec3) -> Vec<Vec<Vec3>> {
    let Some(d) = dir.normalized() else { return Vec::new() };
    let ts = tris(m, Some(f));
    let size = m.bounds().diagonal().max(1e-9);
    // Resample so the projected curve follows the surface.
    let step = size / 200.0;
    let mut dense: Vec<Vec3> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((a.dist(b) / step).ceil() as usize).clamp(1, 2000);
        for i in 0..n {
            dense.push(a.lerp(b, i as f64 / n as f64));
        }
        if dense.len() > 200_000 {
            break;
        }
    }
    if let Some(l) = pts.last() {
        dense.push(*l);
    }
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    for p in dense {
        let hit = ts.iter().filter_map(|t| ray_hit(p, d, t)).min_by(|a, b| a.abs().total_cmp(&b.abs()));
        match hit {
            Some(t) => cur.push(p + d * t),
            None => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Outline of a body spun about the axis `origin + t·dir`: the largest distance from the axis
/// at each station along it, laid into the sketch plane on the side of the axis the plane's
/// in-plane perpendicular points to.
pub(crate) fn spun_outline(m: &Mesh, origin: Vec3, dir: Vec3, plane: &Plane) -> Vec<Vec2> {
    let Some(d) = dir.normalized() else { return Vec::new() };
    let Some(u) = plane.normal().cross(d).normalized() else { return Vec::new() };
    let ax = |p: Vec3| (p - origin).dot(d);
    let rad = |p: Vec3| {
        let v = p - origin;
        (v - d * v.dot(d)).len()
    };
    let (t0, t1) = m.positions.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(ax(*p)), b.max(ax(*p))));
    if !(t0.is_finite() && t1 > t0) {
        return Vec::new();
    }
    let n = 240;
    let mut out = Vec::new();
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let mut r = f64::NEG_INFINITY;
        for tri in &m.triangles {
            let Some(p) = m.tri(tri) else { continue };
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                let (ha, hb) = (ax(a) - t, ax(b) - t);
                if ha.abs() < 1e-12 {
                    r = r.max(rad(a));
                }
                if (ha > 0.0) != (hb > 0.0) && (ha - hb).abs() > 1e-15 {
                    r = r.max(rad(a.lerp(b, ha / (ha - hb))));
                }
            }
        }
        if r.is_finite() {
            out.push(plane.to_local(origin + d * t + u * r));
        }
    }
    out
}

/// An isoparametric curve of face `f` through `p`: the face cut by a plane through `p`. For
/// curved faces `u` runs around (the plane across the face's straight direction, e.g. a
/// cylinder's axis) and `v` along it; planar faces use the face plane's own axes. An explicit
/// direction `"x,y,z"` cuts along it.
pub(crate) fn iso_curve(m: &Mesh, f: u32, p: Vec3, dir: &str) -> Vec<Vec<Vec3>> {
    let ns: Vec<Vec3> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, x)| **x == f)
        .filter_map(|(t, _)| m.tri(t))
        .filter_map(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized())
        .collect();
    let Some(n0) = crate::project::nearest_face(m, p).and_then(|_| ns.first().copied()) else { return Vec::new() };
    // Normal at p: of the nearest triangle of the face.
    let np = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, x)| **x == f)
        .filter_map(|(t, _)| m.tri(t))
        .min_by(|a, b| tri_dist(p, a).total_cmp(&tri_dist(p, b)))
        .and_then(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized())
        .unwrap_or(n0);
    // The face's straight direction: perpendicular to how its normals vary.
    let straight = ns.iter().map(|n| np.cross(*n)).filter(|c| c.len() > 1e-3).fold(None::<Vec3>, |acc, c| match acc {
        None => c.normalized(),
        Some(a) => (a + if a.dot(c) < 0.0 { -c } else { c }).normalized(),
    });
    let along = match dir.trim().to_ascii_lowercase().as_str() {
        "u" | "v" => {
            let s = straight.unwrap_or_else(|| Plane::from_normal(p, np).map(|pl| pl.x).unwrap_or(Vec3::X));
            if dir.trim().eq_ignore_ascii_case("v") { s } else { np.cross(s) }
        }
        other => {
            let v: Vec<f64> = other.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
            match v[..] {
                [x, y, z] => Vec3::new(x, y, z),
                _ => return Vec::new(),
            }
        }
    };
    // Cutting plane through p containing the face normal and the curve direction.
    let Some(cut_n) = np.cross(along).normalized() else { return Vec::new() };
    // Nudged off the point so the cut does not run exactly along mesh edges (a seam); the side
    // that cuts more of the face wins.
    let nudge = m.bounds().diagonal().max(1e-9) * 1e-8;
    let cut = |off: f64| -> Vec<Vec<Vec3>> {
        let Some(pl) = Plane::from_normal(p + cut_n * off, cut_n) else { return Vec::new() };
        let geom = crate::project::section(&pl, m, Some(f));
        let pieces: Vec<Vec<Vec3>> =
            geom.iter().map(|g| link_polyline(g).into_iter().map(|q| pl.to_world(q)).collect()).filter(|w: &Vec<Vec3>| w.len() >= 2).collect();
        join_wires(pieces, m.bounds().diagonal().max(1e-9) * 1e-6)
    };
    let len = |ws: &Vec<Vec<Vec3>>| ws.iter().flat_map(|w| w.windows(2).map(|x| x[0].dist(x[1]))).sum::<f64>();
    let (a, b) = (cut(nudge), cut(-nudge));
    if len(&a) >= len(&b) { a } else { b }
}

/// Join polylines whose ends meet into longer ones.
fn join_wires(mut ws: Vec<Vec<Vec3>>, tol: f64) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    while let Some(mut cur) = ws.pop() {
        let mut grew = true;
        while grew {
            grew = false;
            for i in 0..ws.len() {
                let (Some(cs), Some(ce)) = (cur.first().copied(), cur.last().copied()) else { break };
                let (Some(ws0), Some(we)) = (ws[i].first().copied(), ws[i].last().copied()) else { continue };
                let mut w = if ce.dist(ws0) < tol || cs.dist(we) < tol || ce.dist(we) < tol || cs.dist(ws0) < tol { ws.remove(i) } else { continue };
                if ce.dist(ws0) < tol {
                    cur.extend(w.drain(1..));
                } else if ce.dist(we) < tol {
                    w.reverse();
                    cur.extend(w.drain(1..));
                } else if cs.dist(we) < tol {
                    w.extend(cur.drain(1..));
                    cur = w;
                } else {
                    w.reverse();
                    w.extend(cur.drain(1..));
                    cur = w;
                }
                grew = true;
                break;
            }
        }
        out.push(cur);
    }
    out
}

fn tri_dist(p: Vec3, t: &[Vec3; 3]) -> f64 {
    let c = (t[0] + t[1] + t[2]) / 3.0;
    c.dist(p)
}

/// Sample a fitted planar curve.
fn link_polyline(g: &solidcraft_sketch::LinkGeom) -> Vec<Vec2> {
    use solidcraft_sketch::LinkGeom;
    match *g {
        LinkGeom::Line(a, b) => vec![a, b],
        LinkGeom::Circle(c, r) => (0..=96).map(|i| c + Vec2::from_angle(i as f64 / 96.0 * std::f64::consts::TAU) * r).collect(),
        LinkGeom::Arc { c, a, b } => {
            let (s0, mut sw) = ((a - c).angle(), (b - c).angle() - (a - c).angle());
            while sw <= 1e-12 {
                sw += std::f64::consts::TAU;
            }
            let r = c.dist(a);
            (0..=48).map(|i| c + Vec2::from_angle(s0 + sw * i as f64 / 48.0) * r).collect()
        }
        LinkGeom::Conic { a, apex, b, rho } => (0..=32).map(|i| solidcraft_sketch::conic_point(a, apex, b, rho, i as f64 / 32.0)).collect(),
        LinkGeom::Ellipse { c, major, minor } => {
            (0..=96).map(|i| solidcraft_sketch::ellipse_point(c, major, minor, i as f64 / 96.0 * std::f64::consts::TAU)).collect()
        }
        LinkGeom::Spline { ref pts, control, degree, periodic, ref nurbs } => {
            solidcraft_sketch::spline_polyline_mode(pts, control, degree, periodic, nurbs.as_ref())
        }
        LinkGeom::Point(_) => Vec::new(),
    }
}
