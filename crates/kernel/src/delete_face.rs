//! Delete Face with healing: faces are removed and their planar neighbours extend to close the
//! gap, as when a fillet, chamfer, boss or hole is taken off a part.
//!
//! The body is rebuilt from its own schema 4 record. Faces away from the gap are kept as they
//! are. Each neighbouring face (planar) keeps its plane and its edges that don't touch the gap;
//! a loop that ran entirely along deleted faces is dropped for a hole's rim or a boss's
//! footprint, or restored at the moved corners for a face enclosed by rounds. A corner on the
//! gap moves to where its planes meet the neighbours across the gap (a fillet's tangent points
//! go to the sharp corner), and the stretch of loop that bordered deleted faces closes with a
//! straight edge between the moved corners. Anything else (curved neighbours, a gap the
//! neighbours cannot close) is refused, naming the face.

use std::collections::{BTreeSet, HashMap};

use solidcraft_geom::brep::Curve;
use solidcraft_geom::{Aabb3, Vec3};

use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::Change;
use crate::offset::{Surf, surf_of};
use crate::{KernelError, Result, guard};

fn fail(m: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("Delete Face: {}", m.into()))
}

type Key = (i64, i64, i64);

/// Key of a point for sharing vertices and edges between rebuilt faces.
fn key(p: Vec3, q: f64) -> Key {
    let r = |x: f64| (x / q).round() as i64;
    (r(p.x), r(p.y), r(p.z))
}

/// The vertices and straight edges the rebuilt faces share, by where they are.
struct Shared {
    q: f64,
    vertices: HashMap<Key, u32>,
    lines: HashMap<(Key, Key), u32>,
}

impl Shared {
    fn vertex(&mut self, asm: &mut Assembly, p: Vec3) -> Result<u32> {
        let k = key(p, self.q);
        if let Some(v) = self.vertices.get(&k) {
            return Ok(*v);
        }
        let v = asm.vertex(p)?;
        self.vertices.insert(k, v);
        Ok(v)
    }

    /// The straight edge from `a` to `b`, made once and used both ways.
    fn line(&mut self, asm: &mut Assembly, a: Vec3, b: Vec3) -> Result<AUse> {
        let (ka, kb) = (key(a, self.q), key(b, self.q));
        if let Some(e) = self.lines.get(&(ka, kb)) {
            return Ok(AUse { edge: *e, forward: true, given: None });
        }
        if let Some(e) = self.lines.get(&(kb, ka)) {
            return Ok(AUse { edge: *e, forward: false, given: None });
        }
        let (va, vb) = (self.vertex(asm, a)?, self.vertex(asm, b)?);
        let e = asm.line(va, vb)?;
        self.lines.insert((ka, kb), e);
        Ok(AUse { edge: e, forward: true, given: None })
    }
}

/// The corners along a gap stretch, closed to `a`: straight edges between them, with corners
/// that coincide or lie in line with their neighbours dropped (two rounds meeting in a mitre
/// leave the sharp corner between them).
fn close(pts: Vec<Vec3>, a: Vec3, q: f64) -> Vec<(Vec3, Vec3)> {
    let mut c: Vec<Vec3> = Vec::new();
    for p in pts.into_iter().chain(std::iter::once(a)) {
        if c.last().is_some_and(|l| l.dist(p) <= q * 10.0) {
            continue;
        }
        if let [.., x, y] = c[..]
            && (y - x).cross(p - y).len() <= q * 10.0 * (p - x).len()
            && (y - x).dot(p - y) > 0.0
        {
            c.pop();
        }
        c.push(p);
    }
    c.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
}

/// Whether the use's edge has a corner that moves.
fn corner_moves(asm: &Assembly, moved: &HashMap<u32, Vec3>, u: &AUse) -> bool {
    asm.ends(u).is_some_and(|(a, b)| moved.contains_key(&a) || moved.contains_key(&b))
}

/// Where vertex `v` is after the corners move.
fn position(asm: &Assembly, moved: &HashMap<u32, Vec3>, v: u32) -> Result<Vec3> {
    moved.get(&v).copied().or_else(|| asm.point(v)).ok_or_else(|| fail("vertex"))
}

/// Delete faces (indices in the body's face order, as in its mesh's `tri_face`) and heal the
/// gap by extending the neighbouring planes. The result's history is over `[body]`.
pub fn delete_faces(body: &Body, faces: &[usize]) -> Result<Body> {
    body.require_brep("Delete Face")?;
    let rec = crate::native::brep_record(body)?;
    let n = rec.faces.len();
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
    guard("delete face", || {
        let mut asm = Assembly::from_record(&rec)?;
        // Edge and vertex → faces.
        let mut efaces: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut vfaces: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, f) in asm.faces.iter().enumerate() {
            for u in f.iter().flat_map(|f| &f.loops).flatten() {
                let l = efaces.entry(u.edge).or_default();
                if !l.contains(&i) {
                    l.push(i);
                }
                let (a, b) = asm.ends(u).ok_or_else(|| fail("edge"))?;
                for v in [a, b] {
                    let l = vfaces.entry(v).or_default();
                    if !l.contains(&i) {
                        l.push(i);
                    }
                }
            }
        }
        let gap_edge = |e: u32| efaces.get(&e).is_some_and(|fs| fs.iter().any(|x| del.contains(x)));
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
        for &f in &neighbours {
            match surf_of(&rec, f) {
                Some(Surf::Plane { n, d }) => {
                    planes.insert(f, (n, d));
                }
                _ => {
                    let by = efaces
                        .values()
                        .find(|fs| fs.contains(&f) && fs.iter().any(|x| del.contains(x)))
                        .and_then(|fs| fs.iter().find(|x| del.contains(x)).copied())
                        .unwrap_or(0);
                    return Err(fail(format!("face {by} borders face {f}, which is curved; only planar neighbours can extend to close the gap yet")));
                }
            }
        }
        // The deleted faces' vertices.
        let gap_vertices: BTreeSet<u32> = vfaces.iter().filter(|(_, fs)| fs.iter().any(|f| del.contains(f))).map(|(v, _)| *v).collect();
        // Corners across the gap: three neighbour planes meeting near the deleted faces.
        let gap: Aabb3 = gap_vertices.iter().filter_map(|v| asm.point(*v)).collect();
        let near_gap = gap.inflated(gap.diagonal().max(size * 1e-3));
        let on = |p: Vec3, f: usize| planes.get(&f).is_some_and(|(n, d)| (n.dot(p) - d).abs() <= size * 1e-6);
        let nb: Vec<(Vec3, f64)> = neighbours.iter().filter_map(|f| planes.get(f).copied()).collect();
        let mut corners: Vec<Vec3> = Vec::new();
        for (i, a) in nb.iter().enumerate() {
            for (j, b) in nb.iter().enumerate().skip(i + 1) {
                for c in nb.iter().skip(j + 1) {
                    let den = a.0.dot(b.0.cross(c.0));
                    if den.abs() < 1e-9 {
                        continue;
                    }
                    let x = (b.0.cross(c.0) * a.1 + c.0.cross(a.0) * b.1 + a.0.cross(b.0) * c.1) / den;
                    if x.is_finite() && near_gap.contains(x) {
                        corners.push(x);
                    }
                }
            }
        }
        // Where each vertex on the gap goes: the nearest corner on all of its remaining planes.
        let mut moved: HashMap<u32, Vec3> = HashMap::new();
        for v in &gap_vertices {
            let Some(fs) = vfaces.get(v) else { continue };
            let keep: Vec<usize> = fs.iter().copied().filter(|x| !del.contains(x)).collect();
            if keep.is_empty() || keep.len() >= 3 {
                continue;
            }
            if keep.iter().any(|x| !planes.contains_key(x)) {
                let f = fs.iter().find(|x| del.contains(x)).copied().unwrap_or(0);
                return Err(fail(format!("a corner of face {f} touches a face that cannot move")));
            }
            let p = asm.point(*v).ok_or_else(|| fail("vertex"))?;
            let best = corners.iter().filter(|c| keep.iter().all(|x| on(**c, *x))).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)));
            if let Some(c) = best {
                moved.insert(*v, *c);
            }
        }
        let q = size * 1e-7;
        let mut shared = Shared { q, vertices: HashMap::new(), lines: HashMap::new() };
        for (i, v) in asm.vertices.iter().enumerate() {
            if !moved.contains_key(&(i as u32)) {
                shared.vertices.insert(key(Vec3::from(v.point), q), i as u32);
            }
        }
        let mut history = Vec::new();
        let mut rebuilt: Vec<(usize, AFace)> = Vec::new();
        for fi in 0..n {
            if del.contains(&fi) {
                continue;
            }
            let f = asm.faces.get(fi).and_then(Option::as_ref).ok_or_else(|| fail("face"))?.clone();
            if !neighbours.contains(&fi) {
                // Untouched, unless one of its corners moves.
                if f.loops.iter().flatten().any(|u| corner_moves(&asm, &moved, u)) {
                    return Err(fail(format!("face {fi} meets the gap only at a corner; it cannot follow")));
                }
                history.push(crate::record::from_face(0, fi, Change::Kept));
                continue;
            }
            let mut loops: Vec<Vec<AUse>> = Vec::new();
            for l in &f.loops {
                let gaps = l.iter().filter(|u| gap_edge(u.edge)).count();
                if gaps == 0 && !l.iter().any(|u| corner_moves(&asm, &moved, u)) {
                    loops.push(l.clone());
                    continue;
                }
                if gaps == l.len() {
                    // A hole or boss footprint disappears when its walls are removed. A face
                    // surrounded by rounds does not: its whole boundary moves back to the
                    // intersections of the remaining planes (#1557), its restored edges shared
                    // with the adjacent rebuilt faces.
                    if !l.iter().any(|u| corner_moves(&asm, &moved, u)) {
                        continue;
                    }
                    let mut boundary = Vec::new();
                    for u in l {
                        let (a, b) = asm.ends(u).ok_or_else(|| fail("edge"))?;
                        let (Some(pa), Some(pb)) = (moved.get(&a).copied(), moved.get(&b).copied()) else {
                            return Err(fail(format!("face {fi} has a closed gap boundary that cannot be restored completely")));
                        };
                        if pa.dist(pb) > q * 10.0 {
                            let restored = shared.line(&mut asm, pa, pb)?;
                            asm.piece(restored.edge, u.edge);
                            boundary.push(restored);
                        }
                    }
                    if !boundary.is_empty() {
                        loops.push(boundary);
                    }
                    continue;
                }
                // Start after a gap stretch so stretches don't wrap around.
                let m = l.len();
                let is_gap = |k: usize| l.get(k).is_some_and(|u| gap_edge(u.edge));
                let start = (0..m).find(|i| is_gap((i + m - 1) % m) && !is_gap(*i)).unwrap_or(0);
                let mut edges: Vec<AUse> = Vec::new();
                // The moved corners along a gap stretch.
                let mut pending: Option<Vec<Vec3>> = None;
                for k in 0..m {
                    let Some(u) = l.get((start + k) % m) else { continue };
                    let (va, vb) = asm.ends(u).ok_or_else(|| fail("edge"))?;
                    let (a, b) = (position(&asm, &moved, va)?, position(&asm, &moved, vb)?);
                    if gap_edge(u.edge) {
                        pending.get_or_insert_with(Vec::new).extend([a, b]);
                        continue;
                    }
                    if let Some(p) = pending.take() {
                        for (x, y) in close(p, a, q) {
                            edges.push(shared.line(&mut asm, x, y)?);
                        }
                    }
                    if !corner_moves(&asm, &moved, u) {
                        edges.push(u.clone());
                    } else if matches!(asm.edges.get(u.edge as usize).map(|e| &e.curve), Some(Curve::Line { .. })) {
                        if a.dist(b) > q * 10.0 {
                            let stretched = shared.line(&mut asm, a, b)?;
                            asm.piece(stretched.edge, u.edge);
                            edges.push(stretched);
                        }
                    } else {
                        return Err(fail(format!("face {fi} has a curved edge that would have to stretch")));
                    }
                }
                // A gap stretch at the end closes back to the first edge.
                if let (Some(p), Some(first)) = (pending, edges.first()) {
                    let (v0, _) = asm.ends(first).ok_or_else(|| fail("edge"))?;
                    let a = asm.point(v0).ok_or_else(|| fail("vertex"))?;
                    for (x, y) in close(p, a, q) {
                        edges.push(shared.line(&mut asm, x, y)?);
                    }
                }
                if edges.is_empty() {
                    return Err(fail(format!("face {fi} would lose its boundary")));
                }
                loops.push(edges);
            }
            if loops.is_empty() {
                return Err(fail(format!("face {fi} would lose its boundary")));
            }
            // The face keeps its plane: every moved corner lies on it.
            rebuilt.push((fi, AFace { loops, domain: None, ..f }));
            history.push(crate::record::from_face(0, fi, Change::Modified));
        }
        for &fi in &del {
            if let Some(slot) = asm.faces.get_mut(fi) {
                *slot = None;
            }
        }
        for (fi, face) in rebuilt {
            if let Some(slot) = asm.faces.get_mut(fi) {
                *slot = Some(face);
            }
        }
        let finished = asm.finish().map_err(|e| fail(format!("the neighbouring faces do not close the gap ({e})")))?;
        crate::assemble::body_with_history(&finished, &asm, body, history)
    })
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::Vec3;

    use super::delete_faces;
    use crate::{BoolOp, boolean, box_solid, chamfer, cylinder, fillet};

    fn volume(b: &crate::Body) -> f64 {
        b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume
    }

    /// The faces of `b` with every triangle corner passing `keep`.
    fn faces_where(b: &crate::Body, keep: impl Fn(Vec3) -> bool) -> Vec<usize> {
        let m = b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap();
        let n = m.tri_face.iter().max().map(|x| *x as usize + 1).unwrap_or(0);
        (0..n)
            .filter(|f| {
                m.triangles.iter().zip(&m.tri_face).filter(|(_, x)| **x as usize == *f).all(|(t, _)| m.tri(t).unwrap().iter().all(|p| keep(*p)))
            })
            .collect()
    }

    #[test]
    fn issue1557_delete_closed_fillet_ring_keeps_the_floor() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let picks = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &picks, 2.0).unwrap();
        let record = crate::brep_record(&rounded).unwrap();
        let rounds: Vec<usize> =
            (0..record.faces.len()).filter(|i| !matches!(crate::offset::surf_of(&record, *i), Some(crate::offset::Surf::Plane { .. }))).collect();
        assert_eq!(rounds.len(), 4);
        let restored = delete_faces(&rounded, &rounds).unwrap();
        assert_eq!(restored.face_count(), 6);
        assert_eq!(restored.unmeshed_faces(), 0);
        assert!((volume(&restored) - 6250.0).abs() < 1e-6);
        assert!(restored.contains(Vec3::new(0.1, 0.1, 0.1)));
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
