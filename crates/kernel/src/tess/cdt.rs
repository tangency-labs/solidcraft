//! Faces the structured grid cannot take (planar faces, trimmed faces, holes): a constrained
//! Delaunay triangulation of the boundary loops in (u, v) scaled by the surface's mean metric,
//! seeded with an equilateral lattice, refined wherever a triangle sags more than the
//! tolerance from the surface, has a side longer than the target or bends more than
//! the facet angle, then Ruppert refinement for shape. Constraint edges are never split, so the
//! boundary stays exactly the shared edge nodes.

use std::collections::HashMap;

use solidcraft_geom::Vec3;

use super::size::Turn;
use spade::handles::FixedVertexHandle;
use spade::{AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation};

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

/// Smallest angle Ruppert refinement aims for.
const MIN_ANGLE_DEG: f64 = 25.0;
/// A side is split once it is this many times the target length (the lattice's own sides,
/// exactly the target, and the slightly longer ones where it meets the boundary, stay whole).
const LONG: f64 = 1.5;
/// Seeds closer than this fraction of the side length to the boundary are left out (the
/// boundary nodes are already there; a seed beside them makes a sliver).
const CLEARANCE: f64 = 0.5;

/// Rounds of sag refinement.
const SAG_ROUNDS: usize = 16;

/// The surface, for refining where the triangles leave it.
pub(crate) struct Sag<'a> {
    /// Point and unit normal (when not degenerate) at (u, v).
    pub eval: &'a dyn Fn(f64, f64) -> Option<(Vec3, Option<Vec3>)>,
    pub tol: f64,
    /// Longest triangle side (mm).
    pub h: f64,
    /// The facet angle, if any: corner normals turning more get a point in the middle.
    pub turn: Option<Turn>,
    /// Scale of u and v in the triangulation (the surface's mean |S_u|, |S_v|).
    pub scale: [f64; 2],
}

/// How to fill the inside of the loops.
pub(crate) struct Fill<'a> {
    /// Interior points (already inside the loops).
    pub seeds: &'a [[f64; 2]],
    /// Refinement, adding at most this many points in all.
    pub refine: Option<(Sag<'a>, usize)>,
    /// Ruppert refinement for shape after it. Shape matters for shading only where the normal
    /// varies: a plane is shaded the same by any triangles, so planar faces skip it (around many
    /// small holes it grades out from each rim and multiplies the triangles).
    pub shape: bool,
    /// A side crossing one already kept is left out instead of refusing the loops (the last
    /// resort for a boundary that crosses itself, see `fallback`).
    pub lenient: bool,
}

/// Triangulate the region bounded by `loops` (ranges of `boundary`, each closed). Boundary
/// vertices keep their indices; added points follow. Triangles are counter-clockwise in
/// (u, v). `None` when a loop cannot be kept as constraints (it crosses itself or another).
pub(crate) fn triangulate(boundary: &[[f64; 2]], loops: &[std::ops::Range<usize>], fill: Fill) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    if boundary.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    let scale = match &fill.refine {
        Some((sag, _)) if sag.scale.iter().all(|k| k.is_finite() && *k > 0.0) => sag.scale,
        _ => [1.0, 1.0],
    };
    // spade refuses coordinates below about 1e-43 in magnitude (other than zero), which
    // projection leaves where a parameter is zero in exact arithmetic.
    let to = |p: &[f64; 2]| spade::mitigate_underflow(Point2::new(p[0] * scale[0], p[1] * scale[1]));
    let from = |p: Point2<f64>| [p.x / scale[0], p.y / scale[1]];
    let mut cdt = Cdt::new();
    let mut index: HashMap<FixedVertexHandle, u32> = HashMap::new();
    let mut handles: Vec<FixedVertexHandle> = Vec::with_capacity(boundary.len());
    for (i, p) in boundary.iter().enumerate() {
        let h = cdt.insert(to(p)).ok()?;
        index.entry(h).or_insert(u32::try_from(i).ok()?);
        handles.push(h);
    }
    for r in loops {
        let n = r.len();
        for k in 0..n {
            let (a, b) = (*handles.get(r.start + k)?, *handles.get(r.start + (k + 1) % n)?);
            if a == b || cdt.exists_constraint(a, b) {
                continue;
            }
            if !cdt.can_add_constraint(a, b) {
                if fill.lenient {
                    continue;
                }
                return None;
            }
            cdt.add_constraint(a, b);
        }
    }
    for s in fill.seeds {
        if s.iter().all(|x| x.is_finite()) {
            let _ = cdt.insert(to(s));
        }
    }
    if let Some((sag, budget)) = &fill.refine {
        let constraints = cdt.num_constraints();
        let added = refine_sag(&mut cdt, sag, *budget, &from);
        if fill.shape {
            let params = RefinementParameters::<f64>::new()
                .keep_constraint_edges()
                .exclude_outer_faces(true)
                .with_angle_limit(AngleLimit::from_deg(MIN_ANGLE_DEG))
                .with_max_additional_vertices(budget.saturating_sub(added));
            let before = cdt.num_vertices();
            cdt.refine(params);
            let shaped = cdt.num_vertices().saturating_sub(before);
            // Shaping flips sides and adds points of its own; the triangles it leaves may
            // sag again (a long strip round a cylinder), so the sag is checked once more.
            if shaped > 0 {
                refine_sag(&mut cdt, sag, budget.saturating_sub(added + shaped), &from);
            }
        }
        // A split constraint would put a point on the boundary the neighbour does not have.
        if cdt.num_constraints() != constraints {
            return None;
        }
    }
    let mut uv: Vec<[f64; 2]> = boundary.to_vec();
    let mut tris = Vec::new();
    let region = region(&cdt);
    for face in cdt.inner_faces() {
        if !region.get(face.index()).copied().unwrap_or(false) {
            continue;
        }
        let vs = face.vertices();
        let mut t = [0u32; 3];
        for (slot, v) in t.iter_mut().zip(vs) {
            *slot = match index.get(&v.fix()) {
                Some(&i) => i,
                None => {
                    let i = u32::try_from(uv.len()).ok()?;
                    uv.push(from(v.position()));
                    index.insert(v.fix(), i);
                    i
                }
            };
        }
        tris.push(t);
    }
    let extra = uv.split_off(boundary.len());
    Some((extra, tris))
}

/// The band between a face's boundary `loops` (ranges of the first `n` points of `points`) and
/// the staircase around a block of grid cells inside it (`stairs`, sides between grid points
/// of `points`): a constrained Delaunay triangulation of both with no point added, keeping the
/// faces inside the boundary and outside the block (an odd number of constraints from the
/// outside). Distances in the metric `scale`. Triangles counter-clockwise in (u, v), indices
/// into `points`; `None` when a constraint cannot be kept (the staircase meets the boundary).
pub(crate) fn band(points: &[[f64; 2]], n: usize, loops: &[std::ops::Range<usize>], stairs: &[(u32, u32)], scale: [f64; 2]) -> Option<Vec<[u32; 3]>> {
    let to = |p: &[f64; 2]| spade::mitigate_underflow(Point2::new(p[0] * scale[0], p[1] * scale[1]));
    let mut cdt = Cdt::new();
    let mut index: HashMap<FixedVertexHandle, u32> = HashMap::new();
    let mut handle: HashMap<u32, FixedVertexHandle> = HashMap::new();
    let mut add = |cdt: &mut Cdt, i: u32| -> Option<FixedVertexHandle> {
        if let Some(h) = handle.get(&i) {
            return Some(*h);
        }
        let p = points.get(i as usize)?;
        if !p.iter().all(|x| x.is_finite()) {
            return None;
        }
        let h = cdt.insert(to(p)).ok()?;
        // Two points at one place (pole copies) share a vertex: the first index names it.
        index.entry(h).or_insert(i);
        handle.insert(i, h);
        Some(h)
    };
    let mut sides: Vec<(FixedVertexHandle, FixedVertexHandle)> = Vec::new();
    for r in loops {
        let len = r.len();
        for k in 0..len {
            let (a, b) = (u32::try_from(r.start + k).ok()?, u32::try_from(r.start + (k + 1) % len).ok()?);
            if (a as usize) >= n || (b as usize) >= n {
                return None;
            }
            sides.push((add(&mut cdt, a)?, add(&mut cdt, b)?));
        }
    }
    for &(a, b) in stairs {
        sides.push((add(&mut cdt, a)?, add(&mut cdt, b)?));
    }
    for (a, b) in sides {
        if a == b || cdt.exists_constraint(a, b) {
            continue;
        }
        if !cdt.can_add_constraint(a, b) {
            return None;
        }
        cdt.add_constraint(a, b);
    }
    let region = region(&cdt);
    let mut tris = Vec::new();
    for face in cdt.inner_faces() {
        if !region.get(face.index()).copied().unwrap_or(false) {
            continue;
        }
        let mut t = [0u32; 3];
        for (slot, v) in t.iter_mut().zip(face.vertices()) {
            *slot = *index.get(&v.fix())?;
        }
        tris.push(t);
    }
    Some(tris)
}

/// Insert the parameters of the surface point under every triangle centre that lies more than
/// the tolerance from the triangle or whose corner normals turn more than the facet angle, and
/// under the middle of every non-constraint side that sags more than the tolerance or is
/// longer than [`LONG`] times `h`, until none does or `budget` points were added. Returns the points added.
fn refine_sag(cdt: &mut Cdt, sag: &Sag, budget: usize, from: &dyn Fn(Point2<f64>) -> [f64; 2]) -> usize {
    let mut added = 0;
    let mut cache: HashMap<FixedVertexHandle, Option<(Vec3, Option<Vec3>)>> = HashMap::new();
    // The widest turn between two corner normals.
    let spread = |n: [Option<Vec3>; 3]| match n {
        [Some(a), Some(b), Some(c)] => [(a, b), (b, c), (c, a)].iter().map(|(x, y)| x.dot(*y).clamp(-1.0, 1.0).acos()).fold(0.0, f64::max),
        _ => 0.0,
    };
    for _ in 0..SAG_ROUNDS {
        let mut todo: Vec<Point2<f64>> = Vec::new();
        let mut at = |h: FixedVertexHandle, p: Point2<f64>| {
            *cache.entry(h).or_insert_with(|| {
                let q = from(p);
                (sag.eval)(q[0], q[1])
            })
        };
        let region = region(cdt);
        for face in cdt.inner_faces() {
            if !region.get(face.index()).copied().unwrap_or(false) {
                continue;
            }
            let vs = face.vertices();
            let ps = vs.map(|v| v.position());
            let c = Point2::new((ps[0].x + ps[1].x + ps[2].x) / 3.0, (ps[0].y + ps[1].y + ps[2].y) / 3.0);
            let (Some((a, na)), Some((b, nb)), Some((d, nd))) = (at(vs[0].fix(), ps[0]), at(vs[1].fix(), ps[1]), at(vs[2].fix(), ps[2])) else {
                continue;
            };
            // How far the surface lies from the triangle at `q`.
            let gap = |q: Point2<f64>, chord: Vec3| {
                let uv = from(q);
                (sag.eval)(uv[0], uv[1]).map_or(0.0, |(s, _)| s.dist(chord))
            };
            let off = |q: Point2<f64>, chord: Vec3| gap(q, chord) > sag.tol;
            let middle = gap(c, (a + b + d) * (1.0 / 3.0));
            if middle > sag.tol || sag.turn.is_some_and(|t| t.exceeds(spread([na, nb, nd]), middle)) {
                todo.push(c);
                continue;
            }
            for (e, (pa, pb)) in face.adjacent_edges().into_iter().zip([(a, b), (b, d), (d, a)]) {
                if e.is_constraint_edge() {
                    continue;
                }
                let [p, q] = e.positions();
                let m = Point2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y));
                if pa.dist(pb) > LONG * sag.h || off(m, (pa + pb) * 0.5) {
                    todo.push(m);
                }
            }
        }
        if todo.is_empty() {
            break;
        }
        todo.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        todo.dedup();
        for p in todo {
            if added >= budget {
                return added;
            }
            if insert_off_boundary(cdt, p) {
                added += 1;
            }
        }
    }
    added
}

/// Insert `p` unless it lies exactly on a side of the boundary (the middle of a sliver along
/// it, at a cone's apex): that would split the side, while the face's boundary nodes are shared
/// with its neighbours, which would not have the new one, and the face lost its plan.
fn insert_off_boundary(cdt: &mut Cdt, p: Point2<f64>) -> bool {
    let p = spade::mitigate_underflow(p);
    if !(p.x.is_finite() && p.y.is_finite()) {
        return false;
    }
    if let spade::PositionInTriangulation::OnEdge(e) = cdt.locate(p)
        && cdt.is_constraint_edge(e.as_undirected())
    {
        return false;
    }
    cdt.insert(p).is_ok()
}

/// Per face (by index): inside the region the constraint loops bound. A flood fill from the
/// outer face that flips parity each time it crosses a constraint edge (holes are out), linear
/// in the number of triangles.
fn region(cdt: &Cdt) -> Vec<bool> {
    use spade::handles::{FixedFaceHandle, InnerTag};
    let mut parity: Vec<Option<bool>> = vec![None; cdt.num_all_faces()];
    let mut todo: Vec<(FixedFaceHandle<InnerTag>, bool)> = Vec::new();
    for e in cdt.convex_hull() {
        // Hull edges have the outer face on one side; the other side is an inner face.
        for side in [e, e.rev()] {
            if let Some(inner) = side.face().as_inner() {
                todo.push((inner.fix(), e.is_constraint_edge()));
            }
        }
    }
    while let Some((h, odd)) = todo.pop() {
        match parity.get_mut(h.index()) {
            Some(slot @ None) => *slot = Some(odd),
            _ => continue,
        }
        for e in cdt.face(h).adjacent_edges() {
            if let Some(next) = e.rev().face().as_inner() {
                todo.push((next.fix(), odd ^ e.is_constraint_edge()));
            }
        }
    }
    parity.into_iter().map(|p| p.unwrap_or(false)).collect()
}

/// Seeds for a face: an equilateral (hexagonal) lattice over the parameter box `bbox` with
/// side `step` in each parameter (rows `step[1]·√3/2` apart, every other row shifted half a
/// step), keeping the points inside the region bounded by the loops (even–odd, so holes are
/// out) and at least [`CLEARANCE`] of a side from the boundary in the metric `scale`.
pub(crate) fn lattice(bbox: [(f64, f64); 2], step: [f64; 2], polys: &[&[[f64; 2]]], scale: [f64; 2]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    scan(bbox, step, polys, scale, &mut |q| {
        out.push(q);
        true
    });
    out
}

/// Whether the face has room for a seed of [`lattice`]: somewhere inside it lies a lattice point
/// clear of its boundary. A face narrower than the side everywhere (a thin strip) has none.
/// `true` too when the lattice is more than [`MAX_SEEDS`] points (or the box or step is not
/// finite), which [`lattice`] does not look through.
pub(crate) fn has_room(bbox: [(f64, f64); 2], step: [f64; 2], polys: &[&[[f64; 2]]], scale: [f64; 2]) -> bool {
    let mut found = false;
    let scanned = scan(bbox, step, polys, scale, &mut |_| {
        found = true;
        false
    });
    found || !scanned
}

/// Visit the points of [`lattice`] in turn until `visit` returns `false`. Returns whether the
/// lattice was looked through (it is not when it is too large or ill-formed).
fn scan(bbox: [(f64, f64); 2], step: [f64; 2], polys: &[&[[f64; 2]]], scale: [f64; 2], visit: &mut dyn FnMut([f64; 2]) -> bool) -> bool {
    if !step.iter().all(|s| s.is_finite() && *s > 0.0) || !bbox.iter().all(|(a, b)| a.is_finite() && b.is_finite() && b > a) {
        return false;
    }
    let row = step[1] * 3f64.sqrt() * 0.5;
    let (rows, cols) = (((bbox[1].1 - bbox[1].0) / row).ceil(), ((bbox[0].1 - bbox[0].0) / step[0]).ceil());
    if !(rows * cols <= MAX_SEEDS as f64) {
        return false;
    }
    let near = Clearance::new(polys, scale, CLEARANCE * step[0] * scale[0]);
    let mut xs: Vec<f64> = Vec::new();
    for j in 1..rows as usize {
        let v = bbox[1].0 + row * j as f64;
        xs.clear();
        for poly in polys {
            let n = poly.len();
            for k in 0..n {
                let (Some(a), Some(b)) = (poly.get(k), poly.get((k + 1) % n)) else { continue };
                if (a[1] > v) != (b[1] > v) {
                    xs.push(a[0] + (v - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let shift = if j % 2 == 1 { 0.5 * step[0] } else { 0.0 };
        for i in 0..=cols as usize {
            let u = bbox[0].0 + shift + step[0] * i as f64;
            if u < bbox[0].1 && xs.partition_point(|x| *x < u) % 2 == 1 && !near.within([u, v]) && !visit([u, v]) {
                return true;
            }
        }
    }
    true
}

/// Most lattice seeds of one face.
const MAX_SEEDS: usize = 200_000;

/// Boundary segments bucketed on a square grid (cell = the clearance), for "is this point
/// within the clearance of the boundary" in constant time per point.
struct Clearance {
    cells: HashMap<(i64, i64), Vec<([f64; 2], [f64; 2])>>,
    scale: [f64; 2],
    r: f64,
}

impl Clearance {
    fn new(polys: &[&[[f64; 2]]], scale: [f64; 2], r: f64) -> Clearance {
        let mut cells: HashMap<(i64, i64), Vec<([f64; 2], [f64; 2])>> = HashMap::new();
        if !(r.is_finite() && r > 0.0) {
            return Clearance { cells, scale, r: 0.0 };
        }
        for poly in polys {
            let n = poly.len();
            for k in 0..n {
                let (Some(a), Some(b)) = (poly.get(k), poly.get((k + 1) % n)) else { continue };
                let (a, b) = ([a[0] * scale[0], a[1] * scale[1]], [b[0] * scale[0], b[1] * scale[1]]);
                let cell = |x: f64| (x / r).floor() as i64;
                let (x0, x1) = (cell(a[0].min(b[0])), cell(a[0].max(b[0])));
                let (y0, y1) = (cell(a[1].min(b[1])), cell(a[1].max(b[1])));
                // A segment much longer than the clearance (a straight side of a large face)
                // is checked from every cell of its box; cap that box.
                if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > 4096 {
                    continue;
                }
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        cells.entry((x, y)).or_default().push((a, b));
                    }
                }
            }
        }
        Clearance { cells, scale, r }
    }

    fn within(&self, p: [f64; 2]) -> bool {
        if self.r == 0.0 {
            return false;
        }
        let p = [p[0] * self.scale[0], p[1] * self.scale[1]];
        let (cx, cy) = ((p[0] / self.r).floor() as i64, (p[1] / self.r).floor() as i64);
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                let Some(segs) = self.cells.get(&(x, y)) else { continue };
                if segs.iter().any(|(a, b)| dist_to_segment(p, *a, *b) < self.r) {
                    return true;
                }
            }
        }
        false
    }
}

fn dist_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(uv: &[[f64; 2]], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| {
                let (a, b, c) = (uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]);
                0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
            })
            .sum()
    }

    #[test]
    fn square_with_a_hole_keeps_its_boundary() {
        let mut b = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        b.extend([[0.4, 0.4], [0.4, 0.6], [0.6, 0.6], [0.6, 0.4]]);
        let loops = [0..4, 4..8];
        let flat = |u: f64, v: f64| Some((Vec3::new(u, v, 0.0), Some(Vec3::Z)));
        for refine in [false, true] {
            let refine = refine.then_some((Sag { eval: &flat, tol: 1e-3, h: 0.1, turn: None, scale: [2.0, 1.0] }, 2000));
            let label = refine.is_some();
            let Some((extra, tris)) = triangulate(&b, &loops, Fill { seeds: &[[0.2, 0.2], [0.5, 0.5]], refine, shape: true, lenient: false }) else {
                panic!("meshes")
            };
            let mut uv = b.clone();
            uv.extend(extra);
            assert!((area(&uv, &tris) - 0.96).abs() < 1e-12, "refined: {label}");
            // No added point on the boundary: boundary segments stay whole.
            for r in &loops {
                for k in r.clone() {
                    let next = if k + 1 == r.end { r.start } else { k + 1 };
                    let (p, q) = (k as u32, next as u32);
                    let uses =
                        tris.iter().filter(|t| (0..3).any(|e| (t[e] == p && t[(e + 1) % 3] == q) || (t[e] == q && t[(e + 1) % 3] == p))).count();
                    assert_eq!(uses, 1, "segment {p}-{q}");
                }
            }
        }
    }

    #[test]
    fn a_point_on_a_boundary_side_is_not_inserted() {
        let mut cdt = Cdt::new();
        let corners = [[0.0, 0.0], [4.0, 0.0], [4.0, 1.0], [0.0, 1.0]];
        let Ok(handles) = corners.iter().map(|p| cdt.insert(Point2::new(p[0], p[1]))).collect::<Result<Vec<_>, _>>() else { panic!("insert") };
        for k in 0..4 {
            cdt.add_constraint(handles[k], handles[(k + 1) % 4]);
        }
        assert!(!insert_off_boundary(&mut cdt, Point2::new(2.0, 0.0)));
        assert!(!insert_off_boundary(&mut cdt, Point2::new(f64::NAN, 0.5)));
        assert_eq!((cdt.num_constraints(), cdt.num_vertices()), (4, 4));
        assert!(insert_off_boundary(&mut cdt, Point2::new(2.0, 0.5)));
        assert_eq!((cdt.num_constraints(), cdt.num_vertices()), (4, 5));
    }

    #[test]
    fn crossing_loops_are_refused() {
        let b = [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]];
        let whole = std::slice::from_ref(&(0..4));
        assert!(triangulate(&b, whole, Fill { seeds: &[], refine: None, shape: false, lenient: false }).is_none());
    }

    #[test]
    fn lattice_seeds_are_equilateral_and_clear_of_the_boundary() {
        let outer = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = [[4.0, 4.0], [4.0, 6.0], [6.0, 6.0], [6.0, 4.0]];
        let seeds = lattice([(0.0, 10.0), (0.0, 10.0)], [1.0, 1.0], &[&outer, &hole], [1.0, 1.0]);
        assert!(!seeds.is_empty());
        for p in &seeds {
            let clear = |poly: &[[f64; 2]]| (0..4).all(|k| dist_to_segment(*p, poly[k], poly[(k + 1) % 4]) >= 0.5);
            assert!(clear(&outer) && clear(&hole), "{p:?}");
        }
        // Nearest neighbours one side apart, six of them in the open.
        let near = |p: [f64; 2]| seeds.iter().filter(|q| ((q[0] - p[0]).hypot(q[1] - p[1]) - 1.0).abs() < 1e-9).count();
        assert!(seeds.iter().any(|p| near(*p) == 6));
        assert!(seeds.iter().all(|p| seeds.iter().all(|q| p == q || (q[0] - p[0]).hypot(q[1] - p[1]) > 1.0 - 1e-9)));
    }

    #[test]
    fn coordinates_that_underflow_are_taken_as_zero() {
        // A parameter that is zero in exact arithmetic, left at 1e-300 by projection.
        let b = [[1e-300, 0.0], [1.0, -1e-310], [1.0, 1.0], [0.0, 1.0]];
        let whole = std::slice::from_ref(&(0..4));
        let Some((extra, tris)) = triangulate(&b, whole, Fill { seeds: &[], refine: None, shape: false, lenient: false }) else { panic!("meshes") };
        assert!(extra.is_empty());
        assert_eq!(tris.len(), 2);
    }
}
