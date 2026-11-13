//! Faces the structured grid cannot take (planar faces, trimmed faces, holes): a constrained
//! Delaunay triangulation of the boundary loops in (u, v) scaled by the surface's mean metric,
//! seeded with an equilateral lattice, refined wherever a triangle sags more than the
//! tolerance from the surface, has a side longer than the target or bends more than
//! the facet angle, then Ruppert refinement for shape. Constraint edges are never split, so the
//! boundary stays exactly the shared edge nodes.

use crate::vec::VecExt as _;
use std::collections::HashMap;

use solidcore::Vec3;

use super::size::{SizeAt, Turn};
use spade::handles::FixedVertexHandle;
use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, Intersection, LineIntersectionIterator, Point2, PositionInTriangulation, RefinementParameters,
    Triangulation,
};

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

/// Smallest angle Ruppert refinement aims for.
const MIN_ANGLE_DEG: f64 = 25.0;
/// A side is split once it is this many times the target length (the lattice's own sides,
/// exactly the target, and the slightly longer ones where it meets the boundary, stay whole).
const LONG: f64 = 1.5;
/// Seeds closer than this fraction of the side length to the boundary are left out (the
/// boundary nodes are already there; a seed beside them makes a sliver).
const CLEARANCE: f64 = 0.5;

/// The smallest triangle (mm²) shaping still splits, for the chord tolerance `tol`: a side a
/// thousandth of the tolerance (as [`super::sag`] stops), squared. Round a corner sharper than the
/// angle aimed for, refinement would otherwise crowd points ever closer to it, down to triangles
/// with no area; a larger floor leaves the slivers of flat faces with no size unshaped.
pub(crate) fn smallest_area(tol: f64) -> f64 {
    if tol.is_finite() && tol > 0.0 { (1e-3 * tol).powi(2) } else { 0.0 }
}

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
    /// A side crossing one already kept is left out instead of refusing the loops: the last
    /// resort for a face whose boundary crosses itself in (u, v), so the display has no hole
    /// (the planner names the face in its stats as a defect upstream).
    pub lenient: bool,
    /// A flat face with no size asked of it (no `refine`): Ruppert refinement for shape alone.
    /// The boundary is kept as it is, so the neighbours' shared nodes are untouched.
    pub flat: Option<Flat>,
}

/// How a flat face with no size asked of it is shaped ([`Fill::flat`]).
pub(crate) struct Flat {
    /// The smallest angle (degrees) its triangles keep, where its boundary allows.
    pub min_angle: f64,
    /// The most interior points that may take.
    pub max_points: usize,
    /// Triangles smaller than this (mm²) are left as they are ([`smallest_area`]).
    pub min_area: f64,
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
    let from = |p: Point2<f64>| [p.x / scale[0], p.y / scale[1]];
    let Bounded { mut cdt, index, slits } = bounded(boundary, loops, scale, fill.lenient)?;
    for s in fill.seeds {
        if s.iter().all(|x| x.is_finite()) {
            let _ = cdt.insert(to(s, scale));
        }
    }
    if let (None, Some(flat)) = (&fill.refine, &fill.flat) {
        let constraints = cdt.num_constraints();
        let params = RefinementParameters::<f64>::new()
            .keep_constraint_edges()
            .exclude_outer_faces(true)
            .with_angle_limit(AngleLimit::from_deg(flat.min_angle))
            .with_min_required_area(flat.min_area)
            .with_max_additional_vertices(flat.max_points);
        cdt.refine(params);
        if cdt.num_constraints() != constraints {
            return None;
        }
    }
    if let Some((sag, budget)) = &fill.refine {
        let constraints = cdt.num_constraints();
        let (added, _) = refine_sag(&mut cdt, sag, None, *budget, &from, &slits);
        if fill.shape {
            let params = RefinementParameters::<f64>::new()
                .keep_constraint_edges()
                .exclude_outer_faces(true)
                .with_angle_limit(AngleLimit::from_deg(MIN_ANGLE_DEG))
                // Around a corner sharper than the angle (where two pieces' cuts meet) the
                // refinement would crowd points ever closer to it, down to triangles with no area.
                .with_min_required_area(smallest_area(sag.tol))
                .with_max_additional_vertices(budget.saturating_sub(added));
            let before = cdt.num_vertices();
            cdt.refine(params);
            let shaped = cdt.num_vertices().saturating_sub(before);
            // Shaping flips sides and adds points of its own; the triangles it leaves may
            // sag again (a long strip round a cylinder), so the sag is checked once more.
            if shaped > 0 {
                refine_sag(&mut cdt, sag, None, budget.saturating_sub(added + shaped), &from, &slits);
            }
        }
        // A split constraint would put a point on the boundary the neighbour does not have.
        if cdt.num_constraints() != constraints {
            return None;
        }
    }
    collect(&cdt, index, boundary, &slits, &from)
}

/// A face's triangles as [`mesh`] made them.
pub(crate) struct Meshed {
    /// The points it added, after the boundary's.
    pub extra: Vec<[f64; 2]>,
    /// The triangles, counter-clockwise in (u, v).
    pub triangles: Vec<[u32; 3]>,
    /// Its refinement stopped at its bound of points, or its rounds, with a triangle still off
    /// the surface or longer than asked.
    pub capped: bool,
}

/// The mesh path's start ([`super::Purpose::Mesh`]) on the region bounded by `loops` (as
/// [`triangulate`]): constrained Delaunay of the boundary with the lattice `seeds`, refined where
/// a triangle sags off the surface, has a side longer than [`LONG`] times the side asked at its
/// middle (`sag.h`, or less where `size_at` asks for less) or turns more than the facet angle,
/// adding at most `budget` points. Constraint edges are never split, so the boundary stays the
/// shared edge nodes; the triangles are shaped after, in 3D ([`super::shape`]). `None` when a loop
/// cannot be kept as constraints. Nothing is filled leniently.
pub(crate) fn mesh(
    boundary: &[[f64; 2]],
    loops: &[std::ops::Range<usize>],
    seeds: &[[f64; 2]],
    sag: &Sag,
    size_at: Option<SizeAt<'_>>,
    budget: usize,
) -> Option<Meshed> {
    if boundary.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    let scale = if sag.scale.iter().all(|k| k.is_finite() && *k > 0.0) { sag.scale } else { [1.0, 1.0] };
    let from = |p: Point2<f64>| [p.x / scale[0], p.y / scale[1]];
    let Bounded { mut cdt, index, slits } = bounded(boundary, loops, scale, false)?;
    for s in seeds {
        if s.iter().all(|x| x.is_finite()) {
            let _ = cdt.insert(to(s, scale));
        }
    }
    let constraints = cdt.num_constraints();
    let (_, settled) = refine_sag(&mut cdt, sag, size_at, budget, &from, &slits);
    // A split constraint would put a point on the boundary the neighbour does not have.
    if cdt.num_constraints() != constraints {
        return None;
    }
    let (extra, triangles) = collect(&cdt, index, boundary, &slits, &from)?;
    Some(Meshed { extra, triangles, capped: !settled })
}

/// A point of (u, v) in the triangulation's metric `scale`.
fn to(p: &[f64; 2], scale: [f64; 2]) -> Point2<f64> {
    // spade refuses coordinates below about 1e-43 in magnitude (other than zero), which
    // projection leaves where a parameter is zero in exact arithmetic.
    spade::mitigate_underflow(Point2::new(p[0] * scale[0], p[1] * scale[1]))
}

/// A face's boundary loops triangulated with their sides as constraints, before it is filled.
struct Bounded {
    cdt: Cdt,
    /// Each vertex's index in the output, by its handle's index (dense in spade).
    index: Vec<Option<u32>>,
    /// The sides the loops run along both ways ([`slits`]).
    slits: std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)>,
}

/// The constrained Delaunay triangulation of `boundary`'s points in the metric `scale` with the
/// sides of `loops` as constraints. `None` when a side crosses one already kept, unless
/// `lenient` (see [`Fill::lenient`]): then it is left out.
fn bounded(boundary: &[[f64; 2]], loops: &[std::ops::Range<usize>], scale: [f64; 2], lenient: bool) -> Option<Bounded> {
    let mut cdt = Cdt::new();
    let mut index: Vec<Option<u32>> = Vec::with_capacity(boundary.len());
    let mut handles: Vec<FixedVertexHandle> = Vec::with_capacity(boundary.len());
    for (i, p) in boundary.iter().enumerate() {
        let h = cdt.insert(to(p, scale)).ok()?;
        if index.len() <= h.index() {
            index.resize(h.index() + 1, None);
        }
        if let Some(slot @ None) = index.get_mut(h.index()) {
            *slot = Some(u32::try_from(i).ok()?);
        }
        handles.push(h);
    }
    // A side the loops run along both ways (a slit, where a face's boundary goes out along a
    // line its two sides share and back) bounds the face on neither.
    let mut sides: Vec<(FixedVertexHandle, FixedVertexHandle)> = Vec::new();
    for r in loops {
        let n = r.len();
        for k in 0..n {
            sides.push((*handles.get(r.start + k)?, *handles.get(r.start + (k + 1) % n)?));
        }
    }
    let slits = slits(&sides);
    for r in loops {
        let n = r.len();
        for k in 0..n {
            let (a, b) = (*handles.get(r.start + k)?, *handles.get(r.start + (k + 1) % n)?);
            if a == b || cdt.exists_constraint(a, b) {
                continue;
            }
            if !cdt.can_add_constraint(a, b) {
                if lenient {
                    continue;
                }
                return None;
            }
            cdt.add_constraint(a, b);
        }
    }
    Some(Bounded { cdt, index, slits })
}

/// The triangles of `cdt` inside the region its constraints bound (see [`region`]), with the
/// points added after `boundary`'s in the order the triangles first use them, each back in
/// (u, v) by `from`. `index`: each boundary vertex's index, by its handle's.
fn collect(
    cdt: &Cdt,
    mut index: Vec<Option<u32>>,
    boundary: &[[f64; 2]],
    slits: &std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)>,
    from: &dyn Fn(Point2<f64>) -> [f64; 2],
) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    let mut uv: Vec<[f64; 2]> = boundary.to_vec();
    let mut tris = Vec::new();
    let region = region(cdt, slits);
    for face in cdt.inner_faces() {
        if !region.get(face.index()).copied().unwrap_or(false) {
            continue;
        }
        let vs = face.vertices();
        let mut t = [0u32; 3];
        for (slot, v) in t.iter_mut().zip(vs) {
            let k = v.fix().index();
            *slot = match index.get(k).copied().flatten() {
                Some(i) => i,
                None => {
                    let i = u32::try_from(uv.len()).ok()?;
                    uv.push(from(v.position()));
                    if index.len() <= k {
                        index.resize(k + 1, None);
                    }
                    if let Some(s) = index.get_mut(k) {
                        *s = Some(i);
                    }
                    i
                }
            };
        }
        tris.push(t);
    }
    let extra = uv.split_off(boundary.len());
    Some((extra, tris))
}

/// A display-only subdivision of an existing bounded UV triangulation.
pub(crate) struct Constrained {
    /// Original nodes retain their indices; inserted nodes follow.
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[u32; 3]>,
    /// Final node chains, recovered from retained constraint provenance.
    pub paths: Vec<Vec<u32>>,
}
/// Constrain every existing side and each display path. The caller certifies the
/// full rectangular native trim and supplies canonical shared boundary nodes.
/// This never decides or changes authored CAD topology.
pub(crate) fn constrain_existing(points: &[[f64; 2]], triangles: &[[u32; 3]], paths: &[Vec<[f64; 2]>]) -> Option<Constrained> {
    const MAX_INPUT: usize = 20_000;
    const MAX_OUTPUT: usize = 100_000;
    const MAX_WORK: usize = 20_000_000;
    let total = paths.iter().try_fold(points.len(), |count, path| count.checked_add(path.len()))?;
    if points.len() < 3 || total > MAX_INPUT || triangles.is_empty() || triangles.len() > MAX_INPUT * 2 {
        return None;
    }
    let finite = |p: &[f64; 2]| p.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x));
    if !points.iter().all(finite) || paths.iter().flatten().any(|p| !finite(p)) || paths.iter().any(|p| p.len() < 2) {
        return None;
    }
    let area = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut sign = 0.0_f64;
    let mut original_area = 0.0;
    let mut sides = std::collections::BTreeSet::new();
    for &[a, b, c] in triangles {
        let signed = area(*points.get(a as usize)?, *points.get(b as usize)?, *points.get(c as usize)?);
        if signed == 0.0 {
            continue;
        }
        if sign == 0.0 {
            sign = signed.signum();
        }
        if signed.signum() != sign {
            return None;
        }
        original_area += signed.abs();
        for [i, j] in [[a, b], [b, c], [c, a]] {
            sides.insert([i.min(j), i.max(j)]);
        }
    }
    if sign == 0.0 {
        return None;
    }
    // Refuse a coordinate Spade would change. Retaining the original prefix while
    // silently collapsing distinct tiny parameters would break node identity.
    let to = |p: &[f64; 2]| {
        let original = Point2::new(p[0], p[1]);
        let normalized = spade::mitigate_underflow(original);
        (original == normalized).then_some(original)
    };
    let mut cdt = Cdt::new();
    let mut index: HashMap<FixedVertexHandle, u32> = HashMap::new();
    let mut handles = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        let h = cdt.insert(to(p)?).ok()?;
        if let Some(&previous) = index.get(&h)
            && points.get(previous as usize)? != p
        {
            return None;
        }
        index.entry(h).or_insert(u32::try_from(i).ok()?);
        handles.push(h);
    }
    let mut endpoints = Vec::with_capacity(paths.len());
    for path in paths {
        let mut line = Vec::with_capacity(path.len());
        for p in path {
            line.push(cdt.insert(to(p)?).ok()?);
        }
        endpoints.push(line);
    }
    let duplicates = points.len().checked_sub(index.len())?;
    let mut work = 0usize;
    for [a, b] in sides {
        let (a, b) = (*handles.get(a as usize)?, *handles.get(b as usize)?);
        if a == b || cdt.exists_constraint(a, b) {
            continue;
        }
        let remaining = MAX_WORK.checked_sub(work)?;
        let count = LineIntersectionIterator::new_from_handles(&cdt, a, b).take(remaining.checked_add(1)?).count();
        work = work.checked_add(count)?;
        if work > MAX_WORK || !cdt.can_add_constraint(a, b) {
            return None;
        }
        cdt.add_constraint(a, b);
    }
    const MAX_MEMBERS: usize = 400_000;
    let ordered = |a: FixedVertexHandle, b: FixedVertexHandle| if a.index() < b.index() { [a, b] } else { [b, a] };
    let mut members: HashMap<[FixedVertexHandle; 2], std::collections::BTreeSet<usize>> = HashMap::new();
    let mut membership_count = 0usize;
    let mut segments = Vec::new();
    let mut logical_segments = Vec::with_capacity(endpoints.len());
    for line in &endpoints {
        let mut logical = Vec::new();
        for pair in line.windows(2) {
            let (start, end) = (*pair.first()?, *pair.get(1)?);
            if start == end {
                return None;
            }
            let id = segments.len();
            let mut at = start;
            let mut last_stationary_crossing = None;
            while at != end {
                let mut crossing = None;
                for hit in LineIntersectionIterator::new_from_handles(&cdt, at, end) {
                    work = work.checked_add(1)?;
                    if work > MAX_WORK {
                        return None;
                    }
                    if let Intersection::EdgeIntersection(edge) = hit
                        && edge.is_constraint_edge()
                    {
                        crossing = Some(edge.vertices().map(|vertex| vertex.fix()));
                        break;
                    }
                }
                let next = if let Some([x, y]) = crossing {
                    let (a, b, p, q) = (cdt.vertex(at).position(), cdt.vertex(end).position(), cdt.vertex(x).position(), cdt.vertex(y).position());
                    let (dx, dy, ex, ey) = (b.x - a.x, b.y - a.y, q.x - p.x, q.y - p.y);
                    let determinant = dx * ey - dy * ex;
                    if determinant.abs() <= 64.0 * f64::EPSILON * ((dx * ey).abs() + (dy * ex).abs()) {
                        return None;
                    }
                    let fraction = ((p.x - a.x) * ey - (p.y - a.y) * ex) / determinant;
                    let point = Point2::new(a.x + fraction * dx, a.y + fraction * dy);
                    if !finite(&[point.x, point.y]) || to(&[point.x, point.y])? != point {
                        return None;
                    }
                    let edge = cdt.get_edge_from_neighbors(x, y)?;
                    let edge_id = edge.fix().as_undirected();
                    let acceptable = match cdt.locate(point) {
                        PositionInTriangulation::OnEdge(found) => found.as_undirected() == edge_id,
                        PositionInTriangulation::OnFace(found) => {
                            edge.face().as_inner().is_some_and(|face| face.fix() == found)
                                || edge.rev().face().as_inner().is_some_and(|face| face.fix() == found)
                        }
                        PositionInTriangulation::OnVertex(found) => {
                            found == x
                                || found == y
                                || (found == end || found == at)
                                    && (edge.opposite_vertex().is_some_and(|vertex| vertex.fix() == found)
                                        || edge.rev().opposite_vertex().is_some_and(|vertex| vertex.fix() == found))
                        }
                        _ => false,
                    };
                    if !acceptable || cdt.num_vertices().checked_add(duplicates)?.checked_add(1)? > MAX_OUTPUT {
                        return None;
                    }
                    let old = ordered(x, y);
                    let previous = members.remove(&old).unwrap_or_default();
                    // Two child memberships replace one: enforce the final total
                    // before insertion or any per-owner graph allocation.
                    membership_count = membership_count.checked_add(previous.len())?;
                    if membership_count > MAX_MEMBERS {
                        return None;
                    }
                    let middle = cdt.insert(point).ok()?;
                    if middle != x && middle != y {
                        if let Some(edge) = cdt.get_edge_from_neighbors(x, y)
                            && edge.is_constraint_edge()
                        {
                            cdt.remove_constraint_edge(edge.fix().as_undirected());
                        }
                        for [a, b] in [[x, middle], [middle, y]] {
                            let remaining = MAX_WORK.checked_sub(work)?;
                            let steps = LineIntersectionIterator::new_from_handles(&cdt, a, b).take(remaining.checked_add(1)?).count();
                            work = work.checked_add(steps)?;
                            if work > MAX_WORK || cdt.try_add_constraint(a, b).is_empty() {
                                return None;
                            }
                            members.entry(ordered(a, b)).or_default().extend(previous.iter().copied());
                        }
                    } else {
                        // The arithmetic intersection was exactly an existing
                        // endpoint. Keep the original constraint and its labels.
                        membership_count = membership_count.checked_sub(previous.len())?;
                        members.entry(old).or_default().extend(previous);
                    }
                    middle
                } else {
                    end
                };
                if next == at {
                    // A computed intersection can round exactly to this guide
                    // endpoint. The known crossed edge was rerouted through that
                    // SAME vertex; no point was moved or selected by proximity.
                    let old = crossing.map(|[x, y]| ordered(x, y))?;
                    if last_stationary_crossing == Some(old) || cdt.exists_constraint(old[0], old[1]) {
                        return None;
                    }
                    last_stationary_crossing = Some(old);
                    continue;
                }
                let remaining = MAX_WORK.checked_sub(work)?;
                let steps = LineIntersectionIterator::new_from_handles(&cdt, at, next).take(remaining.checked_add(1)?).count();
                work = work.checked_add(steps)?;
                if work > MAX_WORK {
                    return None;
                }
                let added = cdt.try_add_constraint(at, next);
                if added.is_empty() {
                    return None;
                }
                if membership_count.checked_add(added.len())? > MAX_MEMBERS {
                    return None;
                }
                let mut here = at;
                for edge in added {
                    let edge = cdt.directed_edge(edge);
                    if !edge.is_constraint_edge() || edge.from().fix() != here {
                        return None;
                    }
                    let to = edge.to().fix();
                    if members.entry(ordered(here, to)).or_default().insert(id) {
                        membership_count = membership_count.checked_add(1)?;
                    }
                    here = to;
                }
                if here != next {
                    return None;
                }
                at = next;
            }
            segments.push([start, end]);
            logical.push(id);
        }
        logical_segments.push(logical);
    }
    let mut out = points.to_vec();
    out.try_reserve(cdt.num_vertices()).ok()?;
    for vertex in cdt.vertices() {
        if let std::collections::hash_map::Entry::Vacant(entry) = index.entry(vertex.fix()) {
            let p = vertex.position();
            if out.len() >= MAX_OUTPUT || !finite(&[p.x, p.y]) {
                return None;
            }
            entry.insert(u32::try_from(out.len()).ok()?);
            out.push([p.x, p.y]);
        }
    }
    let mut refined = Vec::new();
    let triangle_limit = MAX_OUTPUT.checked_mul(2)?;
    if cdt.num_inner_faces() > triangle_limit {
        return None;
    }
    refined.try_reserve(cdt.num_inner_faces()).ok()?;
    let mut new_area = 0.0;
    for face in cdt.inner_faces() {
        let vertices = face.vertices();
        let mut triangle = [0; 3];
        for (target, vertex) in triangle.iter_mut().zip(vertices) {
            *target = *index.get(&vertex.fix())?;
        }
        let signed = area(*out.get(triangle[0] as usize)?, *out.get(triangle[1] as usize)?, *out.get(triangle[2] as usize)?);
        if signed <= 0.0 {
            return None;
        }
        new_area += signed;
        if sign < 0.0 {
            triangle.swap(1, 2);
        }
        refined.push(triangle);
    }
    // This scalar is an additional check, not a general topology certificate:
    // the caller certifies a full rectangle and all original sides stay kept.
    let roundoff = 2048.0 * f64::EPSILON * (triangles.len() + refined.len()) as f64;
    if (new_area - original_area).abs() > roundoff {
        return None;
    }
    let mut graphs: Vec<HashMap<FixedVertexHandle, Vec<FixedVertexHandle>>> = vec![HashMap::new(); segments.len()];
    for ([a, b], owners) in members {
        if !cdt.exists_constraint(a, b) {
            return None;
        }
        work = work.checked_add(owners.len())?;
        if work > MAX_WORK {
            return None;
        }
        for owner in owners {
            let graph = graphs.get_mut(owner)?;
            graph.entry(a).or_default().push(b);
            graph.entry(b).or_default().push(a);
        }
    }
    let mut final_paths = Vec::with_capacity(endpoints.len());
    let mut path_nodes = 0usize;
    for logical in logical_segments {
        let mut path = Vec::new();
        for segment in logical {
            let &[start, end] = segments.get(segment)?;
            let graph = graphs.get(segment)?;
            let mut previous = None;
            let mut at = start;
            let mut visited = std::collections::HashSet::new();
            loop {
                if !visited.insert(at) {
                    return None;
                }
                let id = *index.get(&at)?;
                if path.last().copied() != Some(id) {
                    path_nodes = path_nodes.checked_add(1)?;
                    if path_nodes > MAX_OUTPUT.checked_mul(4)? {
                        return None;
                    }
                    path.push(id);
                }
                if at == end {
                    if graph.get(&at)?.len() != 1 || visited.len() != graph.len() {
                        return None;
                    }
                    break;
                }
                let neighbors = graph.get(&at)?;
                let mut next = neighbors.iter().copied().filter(|&h| Some(h) != previous);
                let only = next.next()?;
                if next.next().is_some() {
                    return None;
                }
                previous = Some(at);
                at = only;
            }
        }
        final_paths.push(path);
    }
    Some(Constrained { points: out, triangles: refined, paths: final_paths })
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
    // The band runs along both sides of a slit (see `triangulate`): its sides bound it on neither.
    let slits = slits(&sides);
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
    let region = region(&cdt, &slits);
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
/// longer than [`LONG`] times `h` (or than the size function `size_at` asks at its middle, where
/// it asks for less), until none does or `budget` points were added. Returns the points added,
/// and whether it settled: a round found nothing to add (not stopped by its budget or rounds).
/// `slits`: constraint sides the loops run along both ways, inside the region on both sides.
fn refine_sag(
    cdt: &mut Cdt,
    sag: &Sag,
    size_at: Option<SizeAt<'_>>,
    budget: usize,
    from: &dyn Fn(Point2<f64>) -> [f64; 2],
    slits: &std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)>,
) -> (usize, bool) {
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
        let region = region(cdt, slits);
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
                // A triangle's centroid is inside it, but for a sliver whose corners lie on one line
                // to rounding: an ear on consecutive boundary nodes of a nearly straight stretch. Its
                // centroid lies on the boundary chord, and inserting it there would split the
                // boundary, which the neighbouring face shares. Such a sliver is split at the middles
                // of its inner edges instead.
                let on_boundary = matches!(cdt.locate(c), PositionInTriangulation::OnEdge(e) if cdt.is_constraint_edge(e.as_undirected()));
                if !on_boundary {
                    todo.push(c);
                    continue;
                }
                for e in face.adjacent_edges() {
                    if !e.is_constraint_edge() {
                        let [p, q] = e.positions();
                        todo.push(Point2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y)));
                    }
                }
                continue;
            }
            for (e, (pa, pb)) in face.adjacent_edges().into_iter().zip([(a, b), (b, d), (d, a)]) {
                if e.is_constraint_edge() {
                    continue;
                }
                let [p, q] = e.positions();
                let m = Point2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y));
                if pa.dist(pb) > LONG * super::size::local(sag.h, size_at, (pa + pb) * 0.5) || off(m, (pa + pb) * 0.5) {
                    todo.push(m);
                }
            }
        }
        if todo.is_empty() {
            return (added, true);
        }
        todo.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        todo.dedup();
        for p in todo {
            if added >= budget {
                return (added, false);
            }
            // The boundary's nodes are its edges', shared with the neighbouring face: a point that
            // lands on the boundary (a sliver's middle on a nearly straight stretch, on the chord to
            // rounding) is no inner point, and inserting it would split the boundary.
            let p = spade::mitigate_underflow(p);
            if matches!(cdt.locate(p), PositionInTriangulation::OnEdge(e) if cdt.is_constraint_edge(e.as_undirected())) {
                continue;
            }
            if cdt.insert(p).is_ok() {
                added += 1;
            }
        }
    }
    (added, false)
}

/// The sides a boundary runs along an even number of times (a slit: out along a line and back),
/// each as its vertices in index order: the region lies on both sides of them.
fn slits(sides: &[(FixedVertexHandle, FixedVertexHandle)]) -> std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)> {
    // Sorted, each side's runs are together: counted without hashing every side of the face.
    let mut keyed: Vec<(usize, usize, (FixedVertexHandle, FixedVertexHandle))> = sides
        .iter()
        .filter(|(a, b)| a != b)
        .map(|&(a, b)| if a.index() < b.index() { (a.index(), b.index(), (a, b)) } else { (b.index(), a.index(), (b, a)) })
        .collect();
    keyed.sort_unstable_by_key(|k| (k.0, k.1));
    keyed.chunk_by(|x, y| (x.0, x.1) == (y.0, y.1)).filter(|run| run.len() % 2 == 0).filter_map(|run| run.first().map(|k| k.2)).collect()
}

/// Per face (by index): inside the region the constraint loops bound. A flood fill from the
/// outer face that flips parity each time it crosses a constraint edge (holes are out), but for
/// the sides in `slits` (run along both ways, the region on both sides), linear in the number of
/// triangles.
fn region(cdt: &Cdt, slits: &std::collections::HashSet<(FixedVertexHandle, FixedVertexHandle)>) -> Vec<bool> {
    use spade::handles::{FixedFaceHandle, InnerTag};
    let counts = |e: spade::handles::DirectedEdgeHandle<'_, _, _, _, _>| {
        let [a, b] = e.vertices().map(|v| v.fix());
        e.is_constraint_edge() && (slits.is_empty() || !slits.contains(&if a.index() < b.index() { (a, b) } else { (b, a) }))
    };
    let mut parity: Vec<Option<bool>> = vec![None; cdt.num_all_faces()];
    let mut todo: Vec<(FixedFaceHandle<InnerTag>, bool)> = Vec::new();
    for e in cdt.convex_hull() {
        // Hull edges have the outer face on one side; the other side is an inner face.
        for side in [e, e.rev()] {
            if let Some(inner) = side.face().as_inner() {
                todo.push((inner.fix(), counts(e)));
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
                todo.push((next.fix(), odd ^ counts(e)));
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
    lattice_scanned(bbox, step, polys, scale).0
}

/// [`lattice`], and whether it was looked through: `false` (and no seeds) when it is more than
/// [`MAX_SEEDS`] points, or its box or step is not finite.
pub(crate) fn lattice_scanned(bbox: [(f64, f64); 2], step: [f64; 2], polys: &[&[[f64; 2]]], scale: [f64; 2]) -> (Vec<[f64; 2]>, bool) {
    let mut out = Vec::new();
    let whole = scan(bbox, step, polys, scale, &mut |q| {
        out.push(q);
        true
    });
    (out, whole)
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
            let Some((extra, tris)) = triangulate(&b, &loops, Fill { seeds: &[[0.2, 0.2], [0.5, 0.5]], refine, shape: true, lenient: false, flat: None }) else {
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
    fn crossing_loops_are_refused_unless_lenient() {
        let b = [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]];
        let whole = std::slice::from_ref(&(0..4));
        assert!(triangulate(&b, whole, Fill { seeds: &[], refine: None, shape: false, lenient: false, flat: None }).is_none());
        // Lenient, the crossing side is left out and what the rest encloses is filled.
        let Some((_, tris)) = triangulate(&b, whole, Fill { seeds: &[], refine: None, shape: false, lenient: true, flat: None }) else {
            panic!("a lenient fill")
        };
        assert!(!tris.is_empty());
    }

    /// The smallest angle of each triangle, in degrees.
    fn smallest_angles(uv: &[[f64; 2]], tris: &[[u32; 3]]) -> Vec<f64> {
        tris.iter()
            .map(|t| {
                let p = t.map(|i| uv[i as usize]);
                (0..3)
                    .map(|k| {
                        let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
                        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
                        (u[0] * v[1] - u[1] * v[0]).abs().atan2(u[0] * v[0] + u[1] * v[1]).to_degrees()
                    })
                    .fold(180.0, f64::min)
            })
            .collect()
    }

    #[test]
    fn a_flat_disc_keeps_its_rim_and_has_no_slivers() {
        // A disc's rim alone triangulates into fans of slivers; shaped, every triangle keeps the
        // angle, the rim stays whole and the points added stay within the bound.
        let n = 128;
        let rim: Vec<[f64; 2]> = (0..n).map(|k| (k as f64 * std::f64::consts::TAU / n as f64).sin_cos()).map(|(s, c)| [10.0 * c, 10.0 * s]).collect();
        let whole = std::iter::once(0..n).collect::<Vec<_>>();
        let bare = triangulate(&rim, &whole, Fill { seeds: &[], refine: None, shape: false, lenient: false, flat: None });
        let Some((_, bare)) = bare else { panic!("meshes") };
        assert!(smallest_angles(&rim, &bare).iter().any(|a| *a < 5.0), "the rim alone makes slivers");
        let flat = Flat { min_angle: 15.0, max_points: n, min_area: 1e-6 };
        let Some((extra, tris)) = triangulate(&rim, &whole, Fill { seeds: &[], refine: None, shape: false, lenient: false, flat: Some(flat) }) else {
            panic!("meshes")
        };
        assert!(extra.len() <= n);
        let mut uv = rim.clone();
        uv.extend(extra);
        let exact = 0.5 * n as f64 * 100.0 * (std::f64::consts::TAU / n as f64).sin();
        assert!((area(&uv, &tris) - exact).abs() < 1e-9);
        let worst = smallest_angles(&uv, &tris).into_iter().fold(180.0, f64::min);
        assert!(worst >= 15.0 - 1e-9, "smallest angle {worst}");
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
        let Some((extra, tris)) = triangulate(&b, whole, Fill { seeds: &[], refine: None, shape: false, lenient: false, flat: None }) else { panic!("meshes") };
        assert!(extra.is_empty());
        assert_eq!(tris.len(), 2);
    }
    #[test]
    fn display_constraints_recover_final_crossings_without_changing_uv_coverage() {
        let points = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let paths = vec![vec![[0.0, 0.25], [1.0, 0.25]], vec![[0.5, 0.0], [0.5, 1.0]], vec![[0.0, 0.75], [1.0, 0.75]]];
        for clockwise in [false, true] {
            let mut original = [[0, 1, 2], [0, 2, 3]];
            if clockwise {
                for triangle in &mut original {
                    triangle.swap(1, 2);
                }
            }
            let result = constrain_existing(&points, &original, &paths).unwrap();
            assert_eq!(&result.points[..points.len()], &points);
            let edges: std::collections::BTreeSet<_> =
                result.triangles.iter().flat_map(|&[a, b, c]| [[a.min(b), a.max(b)], [b.min(c), b.max(c)], [a.min(c), a.max(c)]]).collect();
            let mut sum = 0.0;
            for &[a, b, c] in &result.triangles {
                let [a, b, c] = [a, b, c].map(|id| result.points[id as usize]);
                let signed = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                assert!(if clockwise { signed < 0.0 } else { signed > 0.0 });
                sum += signed.abs();
            }
            assert!((sum - 2.0).abs() < 1e-12);
            for (input, path) in paths.iter().zip(&result.paths) {
                assert_eq!(result.points[*path.first().unwrap() as usize], input[0]);
                assert_eq!(result.points[*path.last().unwrap() as usize], input[1]);
                for pair in path.windows(2) {
                    assert!(edges.contains(&[pair[0].min(pair[1]), pair[0].max(pair[1])]), "every guide segment must be an actual skin side");
                }
            }
            let vertical: Vec<_> = result.paths[1].iter().map(|&id| result.points[id as usize]).collect();
            assert!(vertical.contains(&[0.5, 0.25]));
            assert!(vertical.contains(&[0.5, 0.5]));
            assert!(vertical.contains(&[0.5, 0.75]), "later constraints must subdivide an earlier guide's final chain");
        }
        let mut hostile = points;
        hostile[0][0] = f64::NAN;
        assert!(constrain_existing(&hostile, &[[0, 1, 2]], &paths).is_none());
        assert!(constrain_existing(&points, &[[0, 1, 99]], &paths).is_none());
        assert!(constrain_existing(&points, &[[0, 1, 2], [0, 2, 3]], &[vec![[0.5, 0.5], [0.5, 0.5]]]).is_none());
        let tiny = [[0.0, 0.0], [1e-100, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(constrain_existing(&tiny, &[[0, 1, 2], [0, 2, 3]], &paths).is_none(), "normalizing distinct original nodes is unsupported");
        let close = vec![vec![[0.0, 0.5], [1.0, 0.5]], vec![[0.0, 0.5 + 1e-12], [1.0, 0.5 + 1e-12]]];
        let separate = constrain_existing(&points, &[[0, 1, 2], [0, 2, 3]], &close).unwrap();
        for (input, path) in close.iter().zip(&separate.paths) {
            for &id in path {
                assert!((separate.points[id as usize][1] - input[0][1]).abs() < 8.0 * f64::EPSILON);
            }
        }
        assert!(separate.paths[0].iter().all(|id| !separate.paths[1].contains(id)), "nearby long constraints keep distinct identities");
    }

    #[test]
    fn display_constraints_keep_a_normal_sixteen_cell_guide_grid_with_bounded_work() {
        let cells = 20usize;
        let mut uv = Vec::new();
        let mut triangles = Vec::new();
        for y in 0..=cells {
            for x in 0..=cells {
                uv.push([x as f64 / cells as f64, y as f64 / cells as f64]);
                if x < cells && y < cells {
                    let a = (y * (cells + 1) + x) as u32;
                    let b = a + 1;
                    let c = a + (cells + 1) as u32;
                    triangles.extend([[a, b, c + 1], [a, c + 1, c]]);
                }
            }
        }
        let guides = 16usize;
        let mut paths = Vec::new();
        for y in 0..=guides {
            for x in 0..=guides {
                if x < guides {
                    paths.push((0..=24).map(|i| [(x as f64 + i as f64 / 24.0) / guides as f64, y as f64 / guides as f64]).collect());
                }
                if y < guides {
                    paths.push((0..=24).map(|i| [x as f64 / guides as f64, (y as f64 + i as f64 / 24.0) / guides as f64]).collect());
                }
            }
        }
        let refined = constrain_existing(&uv, &triangles, &paths).unwrap();
        assert_eq!(refined.paths.len(), 544);
        let sides: std::collections::BTreeSet<_> =
            refined.triangles.iter().flat_map(|&[a, b, c]| [[a.min(b), a.max(b)], [b.min(c), b.max(c)], [a.min(c), a.max(c)]]).collect();
        for path in refined.paths {
            for pair in path.windows(2) {
                assert!(sides.contains(&[pair[0].min(pair[1]), pair[0].max(pair[1])]));
            }
        }
    }
}
