//! Delete Face with healing: faces are removed and their neighbours extend to close the
//! gap, as when a fillet, chamfer, boss or hole is taken off a part.
//!
//! The body is rebuilt from its own topology. Faces away from the gap are kept as they are.
//! Each neighbouring face (planar) keeps its edges that don't touch the gap; a loop that ran
//! entirely along deleted faces is dropped for a hole's rim or a boss's footprint, or restored
//! at the moved corners for a face enclosed by rounds. A corner on the
//! gap moves to where its planes meet the neighbours across the gap (a fillet's tangent points
//! go to the sharp corner), and the stretch of loop that bordered deleted faces closes with a
//! straight edge between the moved corners. Two equal-radius cylindrical neighbours may
//! instead meet at a conic on their bisector plane. Other curved neighbours, and gaps the
//! neighbours cannot close, are refused.

use std::collections::{BTreeSet, HashMap};

use solidcraft_geom::{Aabb3, Vec3};
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3};
use crate::{KernelError, Result, guard};

fn fail(m: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("Delete Face: {}", m.into()))
}

/// Outward plane (unit normal, offset) of a face, if it is planar: sampled along its edges,
/// the surface's outward normal stays the same and the points stay on one plane.
fn face_plane(f: &mt::Face, tol: f64) -> Option<(Vec3, f64)> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
    let surf = f.oriented_surface();
    let mut pts: Vec<(Vec3, Vec3)> = Vec::new();
    let mut last: Option<(f64, f64)> = None;
    for w in f.boundaries() {
        for e in w.edge_iter() {
            let c = e.oriented_curve();
            let (t0, t1) = c.range_tuple();
            for k in 0..6 {
                let p = c.subs(t0 + (t1 - t0) * k as f64 / 6.0);
                let (u, v) = surf.search_nearest_parameter(p, last, 50)?;
                last = Some((u, v));
                let n = surf.normal(u, v);
                pts.push((from_p3(p), Vec3::new(n.x, n.y, n.z).normalized()?));
            }
        }
    }
    let (p0, n0) = *pts.first()?;
    let d = n0.dot(p0);
    pts.iter().all(|(p, n)| n.dot(n0) > 1.0 - 1e-9 && (n0.dot(*p) - d).abs() <= tol).then_some((n0, d))
}

/// Two equal-radius cylinders meeting across a removed corner can be extended to a
/// bisector plane. Their intersection there is the same conic from either cylinder.
/// Keep this deliberately narrow: arbitrary curved neighbours still need a general
/// surface/surface intersection and must not be healed with straight chords.
struct CylinderMitre {
    faces: HashMap<usize, (Vec3, mt::Edge)>,
    normal: Vec3,
    offset: f64,
    closing_plane: Option<usize>,
}

impl CylinderMitre {
    fn project(&self, face: usize, p: Vec3) -> Option<Vec3> {
        let (axis, _) = self.faces.get(&face)?;
        Some(p + *axis * ((self.offset - self.normal.dot(p)) / self.normal.dot(*axis)))
    }

    fn matrix(&self, face: usize) -> Option<mt::Matrix4> {
        let (axis, _) = self.faces.get(&face)?;
        let a = *axis / self.normal.dot(*axis);
        let col = |v: Vec3, w| mt::Vector4::new(v.x, v.y, v.z, w);
        Some(mt::Matrix4::from_cols(
            col(Vec3::X - a * self.normal.x, 0.0),
            col(Vec3::Y - a * self.normal.y, 0.0),
            col(Vec3::Z - a * self.normal.z, 0.0),
            col(a * self.offset, 1.0),
        ))
    }

    fn surface(&self, fi: usize, face: &mt::Face, size: f64) -> Result<mt::Surface> {
        use mt::{BoundedCurve, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
        let (axis, edge) = self.faces.get(&fi).ok_or_else(|| fail("missing cylinder"))?;
        // Sweep the original section far enough to include both the old and new trim.
        // A trimmed NURBS cylinder's original parameter rectangle may be too short.
        let base = builder::translated(edge, crate::body::v3(-*axis * size));
        let swept: mt::Face = builder::tsweep(&base, crate::body::v3(*axis * (2.0 * size)));
        let mut surface = swept.oriented_surface();
        let c = edge.oriented_curve();
        let (lo, hi) = c.range_tuple();
        let p = c.subs((lo + hi) * 0.5);
        let old = face.oriented_surface();
        let (u, v) = old.search_nearest_parameter(p, None, 100).ok_or_else(|| fail("cannot orient the old cylinder"))?;
        let n = old.normal(u, v);
        let (u, v) = surface.search_nearest_parameter(p, None, 100).ok_or_else(|| fail("cannot orient the extended cylinder"))?;
        let m = surface.normal(u, v);
        if n.x * m.x + n.y * m.y + n.z * m.z < 0.0 {
            surface = mt::Invertible::inverse(&surface);
        }
        Ok(surface)
    }
}

fn cylinder_mitre(
    all: &[mt::Face],
    curved: &[usize],
    del: &BTreeSet<usize>,
    efaces: &HashMap<mt::EdgeID, Vec<usize>>,
    size: f64,
) -> Option<CylinderMitre> {
    use crate::surface_kind::{FaceSurface, surface_of};
    use mt::{BoundedCurve, ParametricCurve};
    let [first, second] = curved else { return None };
    let cylinder = |fi: usize| {
        let f = all.get(fi)?;
        let FaceSurface::Cylinder { origin, axis, radius } = surface_of(f, size)? else { return None };
        let edges: Vec<_> = f.edge_iter().filter(|e| efaces.get(&e.id()).is_some_and(|fs| fs.iter().any(|i| del.contains(i)))).collect();
        let [edge] = edges.as_slice() else { return None };
        let axis = Vec3::new(axis.x, axis.y, axis.z);
        let c = edge.oriented_curve();
        let (lo, hi) = c.range_tuple();
        let sample: Vec<_> = (0..=16).map(|i| from_p3(c.subs(lo + (hi - lo) * f64::from(i) / 16.0))).collect();
        let start = *sample.first()?;
        // Only a transverse section, not a seam or a partial longitudinal cut.
        if sample.iter().any(|p| (*p - start).dot(axis).abs() > size * 1e-7) || start.dist(*sample.last()?) < size * 1e-7 {
            return None;
        }
        Some((origin, axis, radius, (*edge).clone(), sample))
    };
    let (o1, a1, r1, e1, s1) = cylinder(*first)?;
    let (o2, a2, r2, e2, s2) = cylinder(*second)?;
    let tol = size * 1e-6;
    if (r1 - r2).abs() > tol {
        return None;
    }
    let dot = a1.dot(a2);
    let den = 1.0 - dot * dot;
    if den < 1e-6 {
        return None;
    }
    let w = o2 - o1;
    let t = (w.dot(a1) - dot * w.dot(a2)) / den;
    let u = (dot * w.dot(a1) - w.dot(a2)) / den;
    let center = o1 + a1 * t;
    if center.dist(o2 + a2 * u) > tol {
        return None;
    }
    let mut best: Option<(f64, CylinderMitre)> = None;
    for sign in [-1.0, 1.0] {
        let normal = (a1 + a2 * sign).normalized()?;
        let candidate = CylinderMitre {
            faces: HashMap::from([(*first, (a1, e1.clone())), (*second, (a2, e2.clone()))]),
            normal,
            offset: normal.dot(center),
            closing_plane: None,
        };
        let p1: Vec<_> = s1.iter().filter_map(|p| candidate.project(*first, *p)).collect();
        let p2: Vec<_> = s2.iter().filter_map(|p| candidate.project(*second, *p)).collect();
        if p1.len() != s1.len() || p2.len() != s2.len() {
            continue;
        }
        // Adjacent oriented faces must traverse their shared edge in opposite directions.
        if p1.iter().zip(p2.iter().rev()).any(|(a, b)| a.dist(*b) > tol) {
            continue;
        }
        let travel = p1.iter().zip(&s1).chain(p2.iter().zip(&s2)).map(|(a, b)| a.dist(*b)).fold(0.0, f64::max);
        if !travel.is_finite() || travel > size {
            continue;
        }
        if best.as_ref().is_none_or(|(d, _)| travel < *d) {
            best = Some((travel, candidate));
        }
    }
    best.map(|(_, m)| m)
}

/// Continue cylindrical neighbours to one remaining plane (the ends of a removed
/// edge round). Unlike a mitre, the two cylinders need not intersect each other.
fn cylinder_ends(
    all: &[mt::Face],
    curved: &[usize],
    del: &BTreeSet<usize>,
    efaces: &HashMap<mt::EdgeID, Vec<usize>>,
    planes: &HashMap<usize, (Vec3, f64)>,
    size: f64,
) -> Option<CylinderMitre> {
    use crate::surface_kind::{FaceSurface, surface_of};
    use mt::{BoundedCurve, ParametricCurve};
    if curved.is_empty() || curved.len() > 8 {
        return None;
    }
    let mut faces = HashMap::new();
    for &fi in curved {
        let f = all.get(fi)?;
        let FaceSurface::Cylinder { axis, .. } = surface_of(f, size)? else { return None };
        let edges: Vec<_> = f.edge_iter().filter(|e| efaces.get(&e.id()).is_some_and(|fs| fs.iter().any(|i| del.contains(i)))).collect();
        let [edge] = edges.as_slice() else { return None };
        faces.insert(fi, (axis.vec(), (*edge).clone()));
    }
    let mut candidates = Vec::new();
    for (&fi, &(normal, offset)) in planes {
        if faces.values().any(|(axis, _)| normal.dot(*axis).abs() < 1e-6) {
            continue;
        }
        let m = CylinderMitre { faces: faces.clone(), normal, offset, closing_plane: Some(fi) };
        let mut valid = true;
        for (&i, (_, e)) in &faces {
            let c = e.oriented_curve();
            let (lo, hi) = c.range_tuple();
            for k in 0..=16 {
                let p = from_p3(c.subs(lo + (hi - lo) * f64::from(k) / 16.0));
                let q = m.project(i, p)?;
                if !q.is_finite() || p.dist(q) > size {
                    valid = false;
                }
            }
        }
        if valid {
            candidates.push(m);
        }
    }
    if candidates.len() == 1 { candidates.pop() } else { None }
}

/// Add the preserved end arcs around a planar gap, then its straight middle.
/// Each arc must connect to one end of this gap; disconnected arcs are refused.
fn close_with_arcs(a: Vec3, b: Vec3, arcs: &[mt::Edge], tol: f64) -> Result<(Vec<mt::Edge>, Vec3, Vec3, Vec<mt::Edge>)> {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let (mut a, mut b) = (a, b);
    let mut unused = arcs.to_vec();
    while !unused.is_empty() {
        let mut found = None;
        for (i, e) in unused.iter().enumerate() {
            let (x, y) = (from_p3(e.front().point()), from_p3(e.back().point()));
            if x.dist(a) <= tol {
                left.push(e.clone());
                a = y;
                found = Some(i);
                break;
            }
            if y.dist(a) <= tol {
                left.push(e.inverse());
                a = x;
                found = Some(i);
                break;
            }
            if y.dist(b) <= tol {
                right.push(e.clone());
                b = x;
                found = Some(i);
                break;
            }
            if x.dist(b) <= tol {
                right.push(e.inverse());
                b = y;
                found = Some(i);
                break;
            }
        }
        if let Some(i) = found {
            unused.remove(i);
        } else {
            return Err(fail("a cylindrical end does not join the planar gap"));
        }
    }
    right.reverse();
    Ok((left, a, b, right))
}

type Key = (i64, i64, i64);

/// Key of a point for sharing vertices and edges between rebuilt faces.
fn key(p: Vec3, q: f64) -> Key {
    let r = |x: f64| (x / q).round() as i64;
    (r(p.x), r(p.y), r(p.z))
}

/// Delete faces (indices in the body's face order, as in its mesh's `tri_face`) and heal the
/// gap by extending the neighbouring planes. The result's history is over `[body]`.
pub fn delete_faces(body: &Body, faces: &[usize]) -> Result<Body> {
    match delete_faces_by_healing(body, faces) {
        Ok(result) => Ok(result),
        Err(error) => match fill_pocket_by_wall(body, faces) {
            Ok(Some(result)) => Ok(result),
            _ => Err(error),
        },
    }
}

/// A pocket cut into a cylindrical wall (a keyway): the region its own faces and the wall bound
/// is what fills it; NX extends the wall across the gap. Meet the wall by replacing one of the
/// pocket's faces with it, which the Replace Face sweep already does, and accept only a closed
/// result that leaves every deleted face's place solid.
fn fill_pocket_by_wall(body: &Body, faces: &[usize]) -> Result<Option<Body>> {
    use std::collections::HashMap;
    if faces.is_empty() || FILLING.with(|f| f.get()) {
        return Ok(None);
    }
    let all: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
    let point_of = |fi: usize| -> Option<Vec3> {
        let mut sum = Vec3::ZERO;
        let mut count = 0.0;
        for v in all.get(fi).into_iter().flat_map(|f| f.vertex_iter()) {
            sum += from_p3(v.point());
            count += 1.0;
        }
        if count > 0.0 { Some(sum / count) } else { None }
    };
    let mut gap = Vec3::ZERO;
    let mut n = 0.0;
    let mut points = Vec::new();
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for &fi in faces {
        let Some(p) = point_of(fi) else { return Ok(None) };
        for v in all.get(fi).into_iter().flat_map(|f| f.vertex_iter()) {
            let q = from_p3(v.point());
            lo = Vec3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
            hi = Vec3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
        }
        gap += p;
        n += 1.0;
        points.push(p);
    }
    let gap = gap / n;
    // The wall around the pocket: a cylindrical face sharing an edge with one of its faces.
    let mut efaces: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in all.iter().enumerate() {
        for e in f.edge_iter() {
            let v = efaces.entry(e.id()).or_default();
            if !v.contains(&i) {
                v.push(i);
            }
        }
    }
    let del: std::collections::BTreeSet<usize> = faces.iter().copied().collect();
    let mut wall = None;
    for fs in efaces.values() {
        if !fs.iter().any(|f| del.contains(f)) {
            continue;
        }
        for &fi in fs {
            if !del.contains(&fi) && matches!(crate::face_surface(body, fi), Some(crate::FaceSurface::Cylinder { .. })) {
                wall = Some(fi);
            }
        }
    }
    let Some(wall) = wall else { return Ok(None) };
    let size = body.size().max(1.0);
    let tol = size * 1e-6 + 1e-9;
    // What a fill may add: the pocket, not a sweep that ran off the body. A face replaced with
    // the wall can be swept the long way round (a side wall's normal runs across the boss), and
    // the result still holds the deleted faces' places: the box and the volume say no.
    let before = crate::measure(body)?.volume;
    let body_box = body.outline_bounds();
    let pocket = (hi - lo).x * (hi - lo).y * (hi - lo).z;
    FILLING.with(|f| f.set(true));
    let mut out = None;
    for (fi, point) in faces.iter().copied().zip(&points) {
        if !matches!(crate::face_surface(body, fi), Some(crate::FaceSurface::Plane { .. })) {
            continue;
        }
        let tool = crate::SplitTool::Face { body, face: wall };
        let Ok(candidate) = crate::replace_faces(body, &[*point], &tool) else { continue };
        let Ok(measure) = crate::measure(&candidate) else { continue };
        let box_ = candidate.outline_bounds();
        let inside = box_.min.x >= body_box.min.x - tol
            && box_.min.y >= body_box.min.y - tol
            && box_.min.z >= body_box.min.z - tol
            && box_.max.x <= body_box.max.x + tol
            && box_.max.y <= body_box.max.y + tol
            && box_.max.z <= body_box.max.z + tol;
        let added = measure.volume - before;
        if !inside || added <= 1e-9 || added > pocket.max(1e-9) * 2.0 {
            continue;
        }
        let filled = points.iter().all(|p| {
            let inward = *p - gap;
            let dir = if inward.len() > 1e-9 { inward.normalized().unwrap_or(Vec3::ZERO) } else { Vec3::ZERO };
            candidate.contains(*p - dir * (size * 1e-4)) || candidate.contains(*p + dir * (size * 1e-4))
        });
        if filled {
            out = Some(candidate);
            break;
        }
    }
    FILLING.with(|f| f.set(false));
    Ok(out)
}

thread_local! {
    /// `fill_pocket_by_wall` asks Replace Face, which can ask Delete Face: one level only.
    static FILLING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn delete_faces_by_healing(body: &Body, faces: &[usize]) -> Result<Body> {
    body.require_brep("Delete Face")?;
    let all: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
    let n = all.len();
    if faces.is_empty() {
        return Err(fail("no faces given"));
    }
    let del: BTreeSet<usize> = faces.iter().copied().collect();
    if let Some(bad) = del.iter().find(|f| **f >= n) {
        return Err(fail(format!("there is no face {bad} (the body has {n})")));
    }
    if del.len() >= n {
        return Err(fail("that would delete every face"));
    }
    let size = body.size().max(1e-9);
    // Edge and vertex → faces.
    let mut efaces: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    let mut vfaces: HashMap<mt::VertexID, Vec<usize>> = HashMap::new();
    for (i, f) in all.iter().enumerate() {
        for e in f.edge_iter() {
            let v = efaces.entry(e.id()).or_default();
            if !v.contains(&i) {
                v.push(i);
            }
        }
        for v in f.vertex_iter() {
            let l = vfaces.entry(v.id()).or_default();
            if !l.contains(&i) {
                l.push(i);
            }
        }
    }
    // Neighbours: faces sharing an edge with a deleted face; they must be planar.
    let mut neighbours: BTreeSet<usize> = BTreeSet::new();
    for fs in efaces.values() {
        if fs.iter().any(|f| del.contains(f)) {
            neighbours.extend(fs.iter().filter(|f| !del.contains(f)));
        }
    }
    if neighbours.is_empty() {
        return Err(fail("the faces don't border the rest of the body"));
    }
    let mut planes: HashMap<usize, (Vec3, f64)> = HashMap::new();
    let mut curved = Vec::new();
    for &f in &neighbours {
        // Planar by its geometry (boolean results may carry a plane as another surface type).
        match all.get(f).and_then(|x| face_plane(x, size * 1e-6)) {
            Some(p) => {
                planes.insert(f, p);
            }
            None => {
                curved.push(f);
            }
        }
    }
    let mitre = if curved.is_empty() {
        None
    } else {
        Some(guard("delete face curved neighbours", || {
            cylinder_mitre(&all, &curved, &del, &efaces, size).or_else(||cylinder_ends(&all,&curved,&del,&efaces,&planes,size))
                .ok_or_else(|| fail("curved neighbours cannot close this gap; supported repairs include matching cylindrical mitres and cylindrical ends meeting a common plane"))
        })?)
    };
    // Corners across the gap: three neighbour planes meeting near the deleted faces.
    let gap: Aabb3 = del.iter().flat_map(|f| all.get(*f).into_iter().flat_map(|x| x.vertex_iter())).map(|v| from_p3(v.point())).collect();
    let near_gap = gap.inflated(gap.diagonal().max(size * 1e-3));
    let near = |p: Vec3| near_gap.contains(p);
    let on = |p: Vec3, f: usize| planes.get(&f).is_some_and(|(n, d)| (n.dot(p) - d).abs() <= size * 1e-6);
    let nb: Vec<usize> = neighbours.iter().copied().collect();
    let mut corners: Vec<Vec3> = Vec::new();
    for i in 0..nb.len() {
        for j in (i + 1)..nb.len() {
            for k in (j + 1)..nb.len() {
                let (Some(a), Some(b), Some(c)) =
                    (nb.get(i).and_then(|x| planes.get(x)), nb.get(j).and_then(|x| planes.get(x)), nb.get(k).and_then(|x| planes.get(x)))
                else {
                    continue;
                };
                let den = a.0.dot(b.0.cross(c.0));
                if den.abs() < 1e-9 {
                    continue;
                }
                let x = (b.0.cross(c.0) * a.1 + c.0.cross(a.0) * b.1 + a.0.cross(b.0) * c.1) / den;
                if x.is_finite() && near(x) {
                    corners.push(x);
                }
            }
        }
    }
    // Where each vertex on the gap goes: the nearest corner on all of its remaining planes.
    let mut moved: HashMap<mt::VertexID, Vec3> = HashMap::new();
    for &f in &del {
        for v in all.get(f).into_iter().flat_map(|x| x.vertex_iter()) {
            let Some(fs) = vfaces.get(&v.id()) else { continue };
            let keep: Vec<usize> = fs.iter().copied().filter(|x| !del.contains(x)).collect();
            if keep.is_empty() || keep.len() >= 3 {
                continue;
            }
            if let Some(m) = &mitre {
                let projected: Vec<_> = keep.iter().filter_map(|i| m.project(*i, from_p3(v.point()))).collect();
                if let Some(p) = projected.first() {
                    if projected.iter().any(|q| p.dist(*q) > size * 1e-6) || keep.iter().filter(|i| planes.contains_key(i)).any(|i| !on(*p, *i)) {
                        return Err(fail("the cylindrical intersection cannot meet the neighbouring planes"));
                    }
                    moved.insert(v.id(), *p);
                    continue;
                }
            }
            if keep.iter().any(|x| !planes.contains_key(x)) {
                return Err(fail(format!("a corner of face {f} touches a face that cannot move")));
            }
            let p = from_p3(v.point());
            let best = corners.iter().filter(|c| keep.iter().all(|x| on(**c, *x))).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)));
            if let Some(c) = best {
                moved.insert(v.id(), *c);
            }
        }
    }
    let q = size * 1e-7;
    guard("delete face", || {
        // Shared vertices and new straight edges between the rebuilt faces.
        let mut verts: HashMap<Key, mt::Vertex> = HashMap::new();
        for v in body.solid.vertex_iter() {
            if !moved.contains_key(&v.id()) {
                verts.insert(key(from_p3(v.point()), q), v.clone());
            }
        }
        let mut curves: HashMap<(Key, Key), mt::Edge> = HashMap::new();
        let mut lines: HashMap<(Key, Key), mt::Edge> = HashMap::new();
        let mut line = |a: Vec3, b: Vec3, verts: &mut HashMap<Key, mt::Vertex>| -> mt::Edge {
            let (ka, kb) = (key(a, q), key(b, q));
            if let Some(e) = lines.get(&(ka, kb)) {
                return e.clone();
            }
            if let Some(e) = lines.get(&(kb, ka)) {
                return e.inverse();
            }
            let va = verts.entry(ka).or_insert_with(|| builder::vertex(p3(a))).clone();
            let vb = verts.entry(kb).or_insert_with(|| builder::vertex(p3(b))).clone();
            let e = builder::line(&va, &vb);
            lines.insert((ka, kb), e.clone());
            e
        };
        // Neighbours rebuilt from themselves, moved edges pieces of themselves; the edges that
        // close the gap are new.
        let mut rec = crate::record::Rebuild::default();
        let mut out: Vec<mt::Face> = Vec::new();
        let mut end_arcs = Vec::new();
        if let Some(m) = &mitre
            && m.closing_plane.is_some()
        {
            use mt::Transformed;
            for (&fi, (_, e)) in &m.faces {
                let a = m.project(fi, from_p3(e.front().point())).ok_or_else(|| fail("cylinder end"))?;
                let b = m.project(fi, from_p3(e.back().point())).ok_or_else(|| fail("cylinder end"))?;
                let (ka, kb) = (key(a, q), key(b, q));
                let va = verts.entry(ka).or_insert_with(|| builder::vertex(p3(a))).clone();
                let vb = verts.entry(kb).or_insert_with(|| builder::vertex(p3(b))).clone();
                let matrix = m.matrix(fi).ok_or_else(|| fail("cylinder projection"))?;
                let edge = mt::Edge::try_new(&va, &vb, e.oriented_curve().transformed(matrix)).map_err(|e| fail(format!("cylinder end: {e}")))?;
                rec.piece(&edge, e);
                curves.insert((ka, kb), edge.clone());
                end_arcs.push(edge);
            }
        }

        for (fi, f) in all.iter().enumerate() {
            if del.contains(&fi) {
                continue;
            }
            if !neighbours.contains(&fi) {
                // Untouched, unless one of its corners moves.
                if f.vertex_iter().any(|v| moved.contains_key(&v.id())) {
                    return Err(fail(format!("face {fi} meets the gap only at a corner; it cannot follow")));
                }
                out.push(f.clone());
                continue;
            }
            let pos = |v: &mt::Vertex| moved.get(&v.id()).copied().unwrap_or_else(|| from_p3(v.point()));
            let gap = |e: &mt::Edge| efaces.get(&e.id()).is_some_and(|fs| fs.iter().any(|x| del.contains(x)));
            // A face that only loses whole loops (a hole's rim, a pocket's opening) and has no
            // corner moving keeps its own surface: rebuilding a plane from its outer boundary
            // fails when imported edges are not exactly on one plane.
            let mut stays = f.vertex_iter().all(|v| !moved.contains_key(&v.id()));
            let mut kept: Vec<mt::Wire> = Vec::new();
            for w in f.absolute_boundaries() {
                let gaps = w.edge_iter().filter(|e| gap(e)).count();
                if gaps == 0 {
                    kept.push(w.clone());
                } else if gaps != w.len() {
                    stays = false;
                }
            }
            if stays && !kept.is_empty() {
                let mut face = mt::Face::try_new(kept, f.surface()).map_err(|e| fail(format!("face {fi} cannot close: {e}")))?;
                if !f.orientation() {
                    face.invert();
                }
                rec.face(&face, f);
                out.push(face);
                continue;
            }
            let mut wires: Vec<mt::Wire> = Vec::new();
            for w in f.boundaries() {
                let edges: Vec<mt::Edge> = w.edge_iter().cloned().collect();
                if edges.iter().all(gap) {
                    // A hole or boss footprint disappears when its walls are removed. A
                    // face surrounded by rounds does not: its whole boundary moves back to
                    // the intersections of the remaining planes (#1557). Keep that loop,
                    // sharing its restored edges with the adjacent rebuilt faces.
                    let moving = edges.iter().any(|e| moved.contains_key(&e.front().id()) || moved.contains_key(&e.back().id()));
                    if moving {
                        if !edges.iter().all(|e| moved.contains_key(&e.front().id()) && moved.contains_key(&e.back().id())) {
                            return Err(fail(format!("face {fi} has a closed gap boundary that cannot be restored completely")));
                        }
                        let mut boundary = Vec::new();
                        for e in &edges {
                            let (a, b) = (pos(e.front()), pos(e.back()));
                            if a.dist(b) > q * 10.0 {
                                let restored = line(a, b, &mut verts);
                                rec.piece(&restored, e);
                                boundary.push(restored);
                            }
                        }
                        if !boundary.is_empty() {
                            wires.push(boundary.into());
                        }
                    }
                    continue;
                }
                // Start after a gap stretch so stretches don't wrap around.
                let m = edges.len();
                let start = (0..m).find(|i| edges.get((i + m - 1) % m).is_some_and(gap) && edges.get(*i).is_some_and(|e| !gap(e))).unwrap_or(0);
                let mut new_edges: Vec<mt::Edge> = Vec::new();
                // The moved corners along a gap stretch (two rounds meeting in a mitre leave
                // the sharp corner between them).
                let mut pending: Option<Vec<Vec3>> = None;
                let close = |pts: Vec<Vec3>, a: Vec3| -> Vec<(Vec3, Vec3)> {
                    let mut c: Vec<Vec3> = Vec::new();
                    for p in pts.into_iter().chain(std::iter::once(a)) {
                        if c.last().is_some_and(|l| l.dist(p) <= q * 10.0) {
                            continue;
                        }
                        // Drop a corner in line with its neighbours.
                        if let [.., x, y] = c[..]
                            && (y - x).cross(p - y).len() <= q * 10.0 * (p - x).len()
                            && (y - x).dot(p - y) > 0.0
                        {
                            c.pop();
                        }
                        c.push(p);
                    }
                    c.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
                };
                for k in 0..m {
                    let Some(e) = edges.get((start + k) % m) else { continue };
                    if gap(e)
                        && let Some(matrix) = mitre.as_ref().and_then(|m| m.matrix(fi))
                    {
                        use mt::Transformed;
                        let (a, b) = (pos(e.front()), pos(e.back()));
                        let (ka, kb) = (key(a, q), key(b, q));
                        let edge = if let Some(old) = curves.get(&(ka, kb)) {
                            old.clone()
                        } else if let Some(old) = curves.get(&(kb, ka)) {
                            old.inverse()
                        } else {
                            let va = verts.entry(ka).or_insert_with(|| builder::vertex(p3(a))).clone();
                            let vb = verts.entry(kb).or_insert_with(|| builder::vertex(p3(b))).clone();
                            let curve = e.oriented_curve().transformed(matrix);
                            let edge = mt::Edge::try_new(&va, &vb, curve).map_err(|err| fail(format!("cylindrical intersection: {err}")))?;
                            curves.insert((ka, kb), edge.clone());
                            edge
                        };
                        rec.piece(&edge, e);
                        new_edges.push(edge);
                        continue;
                    }
                    if gap(e) {
                        pending.get_or_insert_with(Vec::new).extend([pos(e.front()), pos(e.back())]);
                        continue;
                    }
                    let (a, b) = (pos(e.front()), pos(e.back()));
                    if let Some(p) = pending.take() {
                        if mitre.as_ref().is_some_and(|m| m.closing_plane == Some(fi)) {
                            let start = *p.first().ok_or_else(|| fail("empty planar gap"))?;
                            let (left, x, y, right) = close_with_arcs(start, a, &end_arcs, q * 10.0)?;
                            new_edges.extend(left);
                            if x.dist(y) > q * 10.0 {
                                new_edges.push(line(x, y, &mut verts));
                            }
                            new_edges.extend(right);
                        } else {
                            for (x, y) in close(p, a) {
                                new_edges.push(line(x, y, &mut verts));
                            }
                        }
                    }
                    let moves = moved.contains_key(&e.front().id()) || moved.contains_key(&e.back().id());
                    if !moves {
                        new_edges.push(e.clone());
                    } else if matches!(e.oriented_curve(), mt::Curve::Line(_)) {
                        if a.dist(b) > q * 10.0 {
                            let moved = line(a, b, &mut verts);
                            rec.piece(&moved, e);
                            new_edges.push(moved);
                        }
                    } else {
                        return Err(fail(format!("face {fi} has a curved edge that would have to stretch")));
                    }
                }
                // A gap stretch at the end closes back to the first edge.
                if let (Some(p), Some(first)) = (pending, new_edges.first()) {
                    let a = from_p3(first.front().point());
                    if mitre.as_ref().is_some_and(|m| m.closing_plane == Some(fi)) {
                        let start = *p.first().ok_or_else(|| fail("empty planar gap"))?;
                        let (left, x, y, right) = close_with_arcs(start, a, &end_arcs, q * 10.0)?;
                        new_edges.extend(left);
                        if x.dist(y) > q * 10.0 {
                            new_edges.push(line(x, y, &mut verts));
                        }
                        new_edges.extend(right);
                    } else {
                        for (x, y) in close(p, a) {
                            new_edges.push(line(x, y, &mut verts));
                        }
                    }
                }
                if new_edges.is_empty() {
                    return Err(fail(format!("face {fi} would lose its boundary")));
                }
                wires.push(new_edges.into());
            }
            if let Some(m) = &mitre
                && m.faces.contains_key(&fi)
            {
                let face = mt::Face::try_new(wires, m.surface(fi, f, size)?).map_err(|e| fail(format!("extended cylinder {fi}: {e}")))?;
                rec.face(&face, f);
                out.push(face);
                continue;
            }
            let face = builder::try_attach_plane(&wires).map_err(|e| fail(format!("face {fi} cannot close: {e}")))?;
            // Keep the face's outward side.
            let (nrm, _) = planes.get(&fi).copied().ok_or_else(|| fail("plane"))?;
            let face = match face.surface() {
                mt::Surface::Plane(pl) => {
                    let pn = pl.normal();
                    let along = Vec3::new(pn.x, pn.y, pn.z).dot(nrm);
                    if (along > 0.0) == face.orientation() { face } else { face.inverse() }
                }
                _ => face,
            };
            rec.face(&face, f);
            out.push(face);
        }
        let shell: mt::Shell = out.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| fail(format!("the neighbouring faces do not close the gap ({e})")))?;
        crate::record::rebuilt(solid, &[&body.solid], &rec)
    })
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::Vec3;

    use super::delete_faces;
    use crate::{BoolOp, boolean, box_solid, chamfer, cylinder, fillet};

    fn volume(b: &crate::Body) -> f64 {
        b.tessellate(1e-3).unwrap().measure().volume
    }

    /// The faces of `b` with every triangle corner passing `keep`.
    fn faces_where(b: &crate::Body, keep: impl Fn(Vec3) -> bool) -> Vec<usize> {
        let m = b.tessellate(1e-3).unwrap();
        let n = m.tri_face.iter().max().map(|x| *x as usize + 1).unwrap_or(0);
        (0..n)
            .filter(|f| {
                m.triangles.iter().zip(&m.tri_face).filter(|(_, x)| **x as usize == *f).all(|(t, _)| m.tri(t).unwrap().iter().all(|p| keep(*p)))
            })
            .collect()
    }

    /// Remove a vertical round and its toroidal cap while retaining the two top
    /// edge rounds. The reference is constructed independently by omitting that
    /// corner before rounding the top; deletion must produce that same solid.
    /// A keyway cut into a boss: deleting its faces fills it back, the boss wall extending
    /// across the gap as NX's Delete Face does, though no planar heal closes it.
    #[test]
    fn delete_a_keyway_onto_its_boss_wall() {
        let boss = cylinder(Vec3::new(0.0, 0.0, 8.0), Vec3::Z, 12.0, 14.0).unwrap();
        let cutter = box_solid(Vec3::new(-1.75, -20.0, 12.0), Vec3::new(1.75, -5.0, 22.0)).unwrap();
        let body = boolean(&boss, &cutter, BoolOp::Cut).unwrap().unwrap();
        let selected = faces_where(&body, |p| p.x >= -1.76 && p.x <= 1.76 && p.y <= -4.99 && p.z >= 11.99);
        assert_eq!(selected.len(), 4, "{selected:?}");
        let filled = delete_faces(&body, &selected).unwrap();
        assert_eq!(filled.unmeshed_faces(), 0);
        let volume = filled.tessellate(1e-4).unwrap().measure().volume;
        let want = std::f64::consts::PI * 144.0 * 14.0;
        assert!((volume - want).abs() < 1.0, "{volume} vs {want}");
        assert!(filled.contains(Vec3::new(0.0, -11.0, 17.0)));
        assert!(!filled.contains(Vec3::new(0.0, -14.0, 17.0)));
    }

    #[test]
    fn delete_corner_round_under_two_cylindrical_neighbours() {
        let b = box_solid(Vec3::ZERO, Vec3::new(80.0, 50.0, 8.0)).unwrap();
        let vertical = [Vec3::new(0.0, 0.0, 4.0), Vec3::new(80.0, 0.0, 4.0), Vec3::new(80.0, 50.0, 4.0), Vec3::new(0.0, 50.0, 4.0)];
        let rounded = fillet(&b, &vertical, 6.0).unwrap();
        let h = 6.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
        let top = [
            Vec3::new(40.0, 0.0, 8.0),
            Vec3::new(h, h, 8.0),
            Vec3::new(80.0 - h, h, 8.0),
            Vec3::new(80.0, 25.0, 8.0),
            Vec3::new(80.0 - h, 50.0 - h, 8.0),
            Vec3::new(40.0, 50.0, 8.0),
            Vec3::new(h, 50.0 - h, 8.0),
            Vec3::new(0.0, 25.0, 8.0),
        ];
        let rounded = fillet(&rounded, &top, 2.0).unwrap();
        let selected = faces_where(&rounded, |p| p.x >= 74.0 - 1e-6 && p.y <= 6.0 + 1e-6);
        assert_eq!(selected.len(), 2, "{selected:?}");
        let restored = delete_faces(&rounded, &selected).unwrap();
        let reference = fillet(&b, &[vertical[0], vertical[2], vertical[3]], 6.0).unwrap();
        let reference = fillet(&reference, &top.iter().enumerate().filter(|(i, _)| *i != 2).map(|(_, p)| *p).collect::<Vec<_>>(), 2.0).unwrap();
        assert_eq!(restored.face_count(), reference.face_count());
        assert_eq!(restored.unmeshed_faces(), 0);
        let fine_volume = |b: &crate::Body| b.tessellate(1e-5).unwrap().measure().volume;
        // Analytic horizontal sections: three remaining corners of radius 6
        // below z=6, then radius 4+sqrt(4-(z-6)^2) under the top round.
        // Mesh volume includes chord/trim discretisation, so use a 5 ppm bound.
        let pi = std::f64::consts::PI;
        let k = 1.0 - pi / 4.0;
        let expected = 6.0 * (4000.0 - 108.0 * k) + 8000.0 - 260.0 * (4.0 - pi) + 4.0 * (40.0 / 3.0 - 4.0 * pi) - 3.0 * k * (112.0 / 3.0 + 8.0 * pi);
        for b in [&restored, &reference] {
            assert!((fine_volume(b) - expected).abs() < expected * 5e-6, "{} vs {expected}", fine_volume(b));
            assert_eq!(b.crossed_face_loops().unwrap(), 0);
        }
        for p in [Vec3::new(79.9, 0.1, 3.0), Vec3::new(79.0, 1.0, 7.0), Vec3::new(79.9, 0.1, 7.9)] {
            assert_eq!(restored.contains(p), reference.contains(p), "{p:?}");
        }
        // A lone cap has a third cylindrical neighbour and remains unsupported;
        // it must not silently remove or straighten an unselected round.
        let cap = faces_where(&rounded, |p| p.x >= 74.0 - 1e-6 && p.y <= 6.0 + 1e-6 && p.z >= 6.0 - 1e-6);
        assert_eq!(cap.len(), 1);
        assert!(delete_faces(&rounded, &cap).is_err());
    }

    #[test]
    fn delete_edge_round_with_cylindrical_ends_in_four_directions() {
        for (origin, l, w, h, r) in [(Vec3::ZERO, 40.0, 30.0, 20.0, 3.0), (Vec3::new(-120.0, 70.0, 15.0), 21.0, 17.0, 9.0, 1.2)] {
            let b = box_solid(origin, origin + Vec3::new(l, w, h)).unwrap();
            let picks =
                [Vec3::new(l / 2.0, 0.0, h), Vec3::new(l, w / 2.0, h), Vec3::new(l / 2.0, w, h), Vec3::new(0.0, w / 2.0, h)].map(|p| p + origin);
            let rounded = fillet(&b, &picks, r).unwrap();
            for side in 0..4 {
                let selected = faces_where(&rounded, |p| {
                    let p = p - origin;
                    p.z >= h - r - 1e-6
                        && match side {
                            0 => p.y <= r + 1e-6,
                            1 => p.x >= l - r - 1e-6,
                            2 => p.y >= w - r - 1e-6,
                            _ => p.x <= r + 1e-6,
                        }
                });
                assert_eq!(selected.len(), 1);
                let restored = delete_faces(&rounded, &selected).unwrap();
                let keep: Vec<_> = picks.iter().enumerate().filter(|(i, _)| *i != side).map(|(_, p)| *p).collect();
                let reference = fillet(&b, &keep, r).unwrap();
                assert_eq!(restored.face_count(), reference.face_count());
                assert_eq!(restored.unmeshed_faces(), 0);
                assert_eq!(restored.crossed_face_loops().unwrap(), 0);
                let v = |b: &crate::Body| b.tessellate(1e-5).unwrap().measure().volume;
                assert!((v(&restored) - v(&reference)).abs() < l * w * h * 2e-5, "side {side}: {} vs {}", v(&restored), v(&reference));
            }
        }
    }

    #[test]
    fn issue1557_delete_closed_fillet_ring_keeps_the_floor() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let picks = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &picks, 2.0).unwrap();
        let rounds: Vec<usize> =
            rounded.solid.face_iter().enumerate().filter(|(_, f)| crate::blend::plane_normal(f).is_none()).map(|(i, _)| i).collect();
        assert_eq!(rounds.len(), 4);
        let restored = delete_faces(&rounded, &rounds).unwrap();
        assert_eq!(restored.face_count(), 6);
        assert_eq!(restored.unmeshed_faces(), 0);
        assert!((volume(&restored) - 6250.0).abs() < 1e-6);
        assert!(restored.contains(Vec3::new(0.1, 0.1, 0.1)));
    }

    #[test]
    fn delete_pocket_keeps_an_imported_face_around_it() {
        use truck_modeling as mt;
        // A shallow round pocket in the top of a plate whose top edges bow 5e-5 mm off the
        // plane, as the edges of an imported part do: within the planar test, but too far for a
        // plane rebuilt from them. The top only loses its inner loop, so it keeps its surface.
        let plate = box_solid(Vec3::ZERO, Vec3::new(100.0, 100.0, 10.0)).unwrap();
        let tool = cylinder(Vec3::new(50.0, 50.0, 9.5), Vec3::Z, 7.5, 2.0).unwrap();
        let pocketed = boolean(&plate, &tool, BoolOp::Cut).unwrap().unwrap();
        let pocket = faces_where(&pocketed, |p| (p.x - 50.0).hypot(p.y - 50.0) < 7.5 + 1e-3 && p.z > 9.5 - 1e-6);
        assert!(!pocket.is_empty());
        let mut bowed = 0;
        for e in pocketed.solid.edge_iter() {
            let (a, b) = (e.front().point(), e.back().point());
            if let mt::Curve::Line(_) = e.curve()
                && (a.z - 10.0).abs() < 1e-9
                && (b.z - 10.0).abs() < 1e-9
            {
                let lift = mt::Vector3::new(0.0, 0.0, 1e-4);
                let mid = mt::Point3::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, 10.0) + lift;
                let c = if e.orientation() { vec![a, mid, b] } else { vec![b, mid, a] };
                e.set_curve(mt::Curve::BSplineCurve(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), c)));
                bowed += 1;
            }
        }
        assert_eq!(bowed, 4);
        let filled = delete_faces(&pocketed, &pocket).unwrap();
        assert_eq!(filled.face_count(), 6);
        assert_eq!(filled.unmeshed_faces(), 0);
        assert!((volume(&filled) - 100000.0).abs() < 1e-3, "{}", volume(&filled));
    }

    #[test]
    fn delete_fillet_chamfer_hole_and_boss_faces() {
        let sharp = 24000.0;
        let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
        // A fillet on a box edge: deleting it gives the sharp box back, exactly.
        let f = fillet(&b, &[Vec3::new(20.0, 30.0, 20.0)], 5.0).unwrap();
        assert!(volume(&f) < sharp - 1.0);
        let fi = faces_where(&f, |p| p.y > 25.0 - 1e-6 && p.z > 15.0 - 1e-6 && (p.y < 30.0 - 1e-9 || p.z < 20.0 - 1e-9));
        assert_eq!(fi.len(), 1, "{fi:?}");
        let back = delete_faces(&f, &fi).unwrap();
        assert!((volume(&back) - sharp).abs() < 1e-9 * sharp, "{}", volume(&back));
        assert_eq!(back.face_count(), 6);
        // A chamfer likewise.
        let c = chamfer(&b, &[Vec3::new(20.0, 30.0, 20.0)], 4.0).unwrap();
        let ci = faces_where(&c, |p| p.y > 26.0 - 1e-6 && p.z > 16.0 - 1e-6 && (p.y < 30.0 - 1e-9 || p.z < 20.0 - 1e-9));
        let back = delete_faces(&c, &ci).unwrap();
        assert!((volume(&back) - sharp).abs() < 1e-9 * sharp, "{}", volume(&back));
        // A through hole: deleting its wall fills it.
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 4.0, 12.0).unwrap();
        let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
        let wall = faces_where(&holed, |p| ((p.x - 20.0).hypot(p.y - 15.0) - 4.0).abs() < 1e-2);
        assert!(!wall.is_empty());
        let filled = delete_faces(&holed, &wall).unwrap();
        assert!((volume(&filled) - 12000.0).abs() < 1e-9 * 12000.0, "{}", volume(&filled));
        // A blind hole: its wall and floor.
        let drill = cylinder(Vec3::new(20.0, 15.0, 4.0), Vec3::Z, 3.0, 7.0).unwrap();
        let blind = boolean(&plate, &drill, BoolOp::Cut).unwrap().unwrap();
        let hole = faces_where(&blind, |p| (p.x - 20.0).hypot(p.y - 15.0) < 3.0 + 1e-3 && p.z > 4.0 - 1e-6);
        let filled = delete_faces(&blind, &hole).unwrap();
        assert!((volume(&filled) - 12000.0).abs() < 1e-9 * 12000.0, "{}", volume(&filled));
        // A boss on the plate: deleting its wall and top takes it off.
        let boss = cylinder(Vec3::new(10.0, 10.0, 10.0), Vec3::Z, 3.0, 5.0).unwrap();
        let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        let bf = faces_where(&bossed, |p| p.z > 10.0 - 1e-9 && (p.x - 10.0).hypot(p.y - 10.0) < 3.0 + 1e-6);
        let off = delete_faces(&bossed, &bf).unwrap();
        assert!((volume(&off) - 12000.0).abs() < 1e-9 * 12000.0, "{}", volume(&off));
        // Errors name what is wrong.
        assert!(delete_faces(&plate, &[99]).is_err());
        assert!(delete_faces(&plate, &[0, 1, 2, 3, 4, 5]).is_err());
        // One face of a box: the sides cannot close it.
        let top = faces_where(&plate, |p| (p.z - 10.0).abs() < 1e-9);
        assert!(delete_faces(&plate, &top).is_err());
    }
}
