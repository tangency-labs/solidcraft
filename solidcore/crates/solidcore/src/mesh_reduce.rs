//! Persistent indexed-mesh reduction, independent of display tessellation.
//!
//! Interior edge collapses preserve manifold connectivity, open boundaries,
//! sharp creases and per-corner normal seams. A spatial guard rejects new
//! intersections and refuses to change an already intersecting local patch.
//! Ill-conditioned candidates are rejected. The result may exceed the target.
//! No sampled or certified geometric error bound is claimed.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::{Result, SolidError};

mod collision;
mod quadric;
mod topology;

use collision::Bounds;
use quadric::Quadric;

/// The compacted positions and reduced indexed triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct ReducedMesh {
    /// Positions referenced by the output triangles, in input order.
    pub positions: Vec<[f64; 3]>,
    /// Triangle indices in triples.
    pub indices: Vec<u32>,
    /// Per-corner attributes in the same order as `indices`, if provided.
    pub corner_normals: Option<Vec<[f64; 3]>>,
    /// Input triangle count.
    pub original_triangles: usize,
    /// Output triangle count, possibly greater than the target.
    pub reduced_triangles: usize,
}

const OPERATION: &str = "mesh.reduce";
const CANCEL_STRIDE: usize = 4096;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OPERATION, reason)
}

fn invariant(reason: &'static str) -> SolidError {
    SolidError::port_invariant(OPERATION, reason)
}

fn buffer<T>(capacity: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| invalid("mesh reduction buffers are too large"))?;
    Ok(values)
}

fn filled<T: Clone>(len: usize, value: T) -> Result<Vec<T>> {
    let mut values = buffer(len)?;
    values.resize(len, value);
    Ok(values)
}

fn check_cancel(cancel: &mut impl FnMut() -> bool) -> Result<()> {
    if cancel() {
        Err(SolidError::cancelled(OPERATION))
    } else {
        Ok(())
    }
}

fn check_stride(index: usize, cancel: &mut impl FnMut() -> bool) -> Result<()> {
    if index % CANCEL_STRIDE == 0 {
        check_cancel(cancel)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Face {
    vertices: [usize; 3],
}

// A sign is returned only when the determinant is safely separated from zero.
// The guard covers the roundoff of two subtractions, two products and a
// subtraction, including a normal-number floor; overflow and underflow refuse.
fn orient(a: [f64; 3], b: [f64; 3], c: [f64; 3], axis: usize) -> Option<bool> {
    let (x, y) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let ax = *a.get(x)?;
    let ay = *a.get(y)?;
    let bx = *b.get(x)? - ax;
    let by = *b.get(y)? - ay;
    let cx = *c.get(x)? - ax;
    let cy = *c.get(y)? - ay;
    let p = bx * cy;
    let q = by * cx;
    let det = p - q;
    let bound = (p.abs() + q.abs()) * 1.0e-14 + f64::MIN_POSITIVE;
    if det.is_finite() && bound.is_finite() && det.abs() > bound {
        Some(det > 0.0)
    } else {
        None
    }
}
// Exact floating expansions for collision predicates. An uncertain product
// residual or overflow makes the caller conservatively reject the candidate.
fn grow(expansion: &[f64], mut q: f64) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for &value in expansion {
        let sum = q + value;
        if !sum.is_finite() {
            return None;
        }
        let bv = sum - q;
        let error = (q - (sum - bv)) + (value - bv);
        if !bv.is_finite() || !error.is_finite() {
            return None;
        }
        if error != 0.0 {
            out.push(error);
        }
        q = sum;
    }
    if q != 0.0 {
        out.push(q);
    }
    Some(out)
}

fn difference(a: f64, b: f64) -> Option<Vec<f64>> {
    grow(&[a], -b)
}

fn product(a: &[f64], b: &[f64]) -> Option<Vec<f64>> {
    let mut result = Vec::new();
    for &x in a {
        for &y in b {
            let high = x * y;
            if !high.is_finite()
                || (x != 0.0
                    && y != 0.0
                    && high.abs() < f64::MIN_POSITIVE * 18_014_398_509_481_984.0)
            {
                return None;
            }
            let low = x.mul_add(y, -high);
            result = grow(&result, low)?;
            result = grow(&result, high)?;
        }
    }
    Some(result)
}

fn subtract_expansion(a: &[f64], b: &[f64]) -> Option<Vec<f64>> {
    let mut result = a.to_vec();
    for &value in b {
        result = grow(&result, -value)?;
    }
    Some(result)
}

fn face_points(face: &Face, positions: &[[f64; 3]]) -> Option<[[f64; 3]; 3]> {
    Some([
        *positions.get(*face.vertices.first()?)?,
        *positions.get(*face.vertices.get(1)?)?,
        *positions.get(*face.vertices.get(2)?)?,
    ])
}

// The outgoing links of a consistently wound interior star form one cycle.
// Keep the small, bounded link on the stack. Sorting also gives stable
// neighbours and linear-time link intersection without per-edge allocations.
const MAX_STAR: usize = 128;

struct Star {
    links: [(usize, usize); MAX_STAR],
    len: usize,
    boundary_end: Option<usize>,
}

impl Star {
    fn links(&self) -> Option<&[(usize, usize)]> {
        self.links.get(..self.len)
    }

    fn common_neighbours(&self, other: &Self) -> Option<usize> {
        let left = self.links()?;
        let right = other.links()?;
        let (mut a, mut b, mut count) = (0, 0, 0);
        while let (Some(&(u, _)), Some(&(v, _))) = (left.get(a), right.get(b)) {
            if u == v {
                count += 1;
            }
            if u <= v {
                a += 1;
            }
            if v <= u {
                b += 1;
            }
        }
        Some(
            count
                + usize::from(
                    other.boundary_end.is_some_and(|end| {
                        left.binary_search_by_key(&end, |&(from, _)| from).is_ok()
                    }),
                ),
        )
    }
}

fn star(
    vertex: usize,
    faces: &[Option<Face>],
    ids: &[usize],
    require_interior: bool,
) -> Option<Star> {
    if !(1..=MAX_STAR).contains(&ids.len()) || (require_interior && ids.len() < 3) {
        return None;
    }
    let mut result = Star {
        links: [(0, 0); MAX_STAR],
        len: ids.len(),
        boundary_end: None,
    };
    let links = result.links.get_mut(..ids.len())?;
    for (link, &id) in links.iter_mut().zip(ids) {
        let face = faces.get(id)?.as_ref()?;
        let i = face.vertices.iter().position(|&v| v == vertex)?;
        *link = (
            *face.vertices.get((i + 1) % 3)?,
            *face.vertices.get((i + 2) % 3)?,
        );
    }
    links.sort_unstable_by_key(|&(from, _)| from);
    if links
        .iter()
        .zip(links.iter().skip(1))
        .any(|(a, b)| a.0 == b.0)
    {
        return None;
    }
    let first = links.first()?.0;
    // A boundary link is a single oriented path. Its last vertex is not
    // a link source, so keep it separately in the common-neighbour test.
    let mut start = first;
    let mut open = false;
    if !require_interior {
        for &(from, _) in links.iter() {
            if !links.iter().any(|&(_, to)| to == from) {
                if open {
                    return None;
                }
                start = from;
                open = true;
            }
        }
    }
    let mut next = start;
    for step in 0..links.len() {
        let i = links.binary_search_by_key(&next, |&(from, _)| from).ok()?;
        next = links.get(i)?.1;
        if next == start {
            return (!open && step + 1 == links.len()).then_some(result);
        }
        if open
            && step + 1 == links.len()
            && links
                .binary_search_by_key(&next, |&(from, _)| from)
                .is_err()
        {
            result.boundary_end = Some(next);
            return Some(result);
        }
    }
    None
}

// Scale edges before the cross product to avoid overflow; poorly conditioned
// normals are refused rather than used to accept a flip or crease.
fn face_normal([a, b, c]: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let scale = u.iter().chain(&v).fold(0.0_f64, |m, x| m.max(x.abs()));
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    let u = u.map(|x| x / scale);
    let v = v.map(|x| x / scale);
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if !length.is_finite() || length <= 1.0e-14 {
        return None;
    }
    Some(n.map(|x| x / length))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

struct WorkingMesh<'a> {
    positions: &'a [[f64; 3]],
    faces: Vec<Option<Face>>,
    normals: Option<Vec<[[f64; 3]; 3]>>,
    incident: Vec<Vec<usize>>,
    bvh: collision::Bvh,
    // Once checked, an unchanged face stays clear: a later edit must check
    // its replacements against every unchanged face, including this one.
    clear: Vec<bool>,
}

struct Replacement {
    id: usize,
    face: Face,
    points: [[f64; 3]; 3],
    bounds: Bounds,
}

struct Collapse {
    retained_normal: Option<[f64; 3]>,
}

// Refuse turns above about 18 degrees: the score ranks endpoint collapses
// but does not measure fold angles. This quality limit is independent of the
// source crease threshold and the predicates' roundoff guards.
const MIN_REPLACEMENT_NORMAL_DOT: f64 = 0.95;

impl WorkingMesh<'_> {
    fn conflicts(
        &self,
        face: &Face,
        points: [[f64; 3]; 3],
        skip_id: usize,
        cancel: &mut impl FnMut() -> bool,
    ) -> Result<bool> {
        let scale = points.iter().flatten().fold(1.0_f64, |m, x| m.max(x.abs()));
        self.bvh.any_overlapping_bounds(
            Bounds::triangle(points),
            scale * (64.0 * f64::EPSILON),
            |id| {
                if id == skip_id {
                    return Ok(false);
                }
                let other = self
                    .faces
                    .get(id)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| invariant("missing indexed face"))?;
                let other_points = face_points(other, self.positions)
                    .ok_or_else(|| invariant("invalid indexed face"))?;
                Ok(collision::conflicts(
                    face.vertices,
                    points,
                    other.vertices,
                    other_points,
                ))
            },
            cancel,
        )
    }

    fn propose(
        &self,
        from: usize,
        to: usize,
        from_star: &Star,
        proposed: &mut Vec<Replacement>,
    ) -> Option<Collapse> {
        let (from_faces, to_faces) = (self.incident.get(from)?, self.incident.get(to)?);
        let to_star = star(to, &self.faces, to_faces, false)?;
        if from_star.common_neighbours(&to_star)? != 2 {
            return None;
        }
        if from_faces.iter().filter(|id| to_faces.contains(id)).count() != 2 {
            return None;
        }

        let mut retained_normal = None;
        if let Some(normals) = &self.normals {
            for (vertex, ids) in [(from, from_faces), (to, to_faces)] {
                let mut shared = None;
                for &id in ids {
                    let face = self.faces.get(id)?.as_ref()?;
                    let corner = face.vertices.iter().position(|&v| v == vertex)?;
                    let normal = *normals.get(id)?.get(corner)?;
                    if shared.is_some_and(|n| !topology::same_normal_direction(n, normal)) {
                        return None;
                    }
                    shared = Some(normal);
                }
                if vertex == to {
                    retained_normal = shared;
                }
            }
        }

        proposed.clear();
        for id in from_faces {
            let face = self.faces.get(*id)?.as_ref()?;
            if face.vertices.contains(&to) {
                continue;
            }
            let mut changed = *face;
            let corner = changed.vertices.iter().position(|v| *v == from)?;
            *changed.vertices.get_mut(corner)? = to;
            let replacement = face_points(&changed, self.positions)?;
            if dot(
                face_normal(face_points(face, self.positions)?)?,
                face_normal(replacement)?,
            ) <= MIN_REPLACEMENT_NORMAL_DOT
            {
                return None;
            }
            proposed.push(Replacement {
                id: *id,
                face: changed,
                points: replacement,
                bounds: Bounds::triangle(replacement),
            });
        }
        Some(Collapse { retained_normal })
    }

    fn candidate(
        &mut self,
        from: usize,
        to: usize,
        from_star: &Star,
        proposed: &mut Vec<Replacement>,
        cancel: &mut impl FnMut() -> bool,
    ) -> Result<Option<Collapse>> {
        let Some(collapse) = self.propose(from, to, from_star, proposed) else {
            return Ok(None);
        };
        let from_faces = self
            .incident
            .get(from)
            .ok_or_else(|| invariant("missing vertex incidence"))?;

        // Query each external triangle once for the whole fan. Leaf bounds
        // still gate each exact pair test, including pairs inside the fan.
        let patch = proposed
            .iter()
            .map(|r| r.bounds)
            .reduce(Bounds::union)
            .ok_or_else(|| invariant("empty collapse fan"))?;
        let scale = proposed
            .iter()
            .flat_map(|r| r.points.iter().flatten())
            .fold(1.0_f64, |m, x| m.max(x.abs()));
        let padding = scale * (64.0 * f64::EPSILON);
        let conflict = self.bvh.any_overlapping_bounds(
            patch,
            padding,
            |id| {
                if from_faces.contains(&id) {
                    return Ok(false);
                }
                let bounds = self
                    .bvh
                    .face_bounds(id)?
                    .ok_or_else(|| invariant("missing overlapping face bounds"))?;
                let mut hits = proposed
                    .iter()
                    .filter(|r| r.bounds.overlaps(bounds, padding));
                let Some(first) = hits.next() else {
                    return Ok(false);
                };
                let other = self
                    .faces
                    .get(id)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| invariant("missing overlapping face"))?;
                let points = face_points(other, self.positions)
                    .ok_or_else(|| invariant("invalid overlapping face"))?;
                Ok(std::iter::once(first).chain(hits).any(|r| {
                    collision::conflicts(r.face.vertices, r.points, other.vertices, points)
                }))
            },
            cancel,
        )?;
        if conflict {
            return Ok(None);
        }
        for (i, changed) in proposed.iter().enumerate() {
            for other in proposed.iter().take(i) {
                if changed.bounds.overlaps(other.bounds, padding)
                    && collision::conflicts(
                        changed.face.vertices,
                        changed.points,
                        other.face.vertices,
                        other.points,
                    )
                {
                    return Ok(None);
                }
            }
        }

        // Do not erase a pre-existing crossing. Check each original at most
        // once after it is proven clear, even when other candidates fail.
        for &id in from_faces {
            let clear = self
                .clear
                .get(id)
                .copied()
                .ok_or_else(|| invariant("invalid collapse fan face id"))?;
            if clear {
                continue;
            }
            let face = self
                .faces
                .get(id)
                .and_then(Option::as_ref)
                .ok_or_else(|| invariant("missing collapse fan face"))?;
            let points = face_points(face, self.positions)
                .ok_or_else(|| invariant("invalid collapse fan face"))?;
            if self.conflicts(face, points, id, cancel)? {
                return Ok(None);
            }
            *self
                .clear
                .get_mut(id)
                .ok_or_else(|| invariant("invalid collapse fan face id"))? = true;
        }
        Ok(Some(collapse))
    }
}

// Within a narrow quadric-error band, ties use arrival order rather than
// repeatedly editing the first patch in vertex order. Stale tickets are
// rescored against the accumulated quadrics before they can commit.
type Work = BinaryHeap<Reverse<(u16, usize, usize)>>;

#[derive(Clone, Copy)]
enum QueueState {
    Idle,
    Ready,
    Deferred,
}

fn band(cost: u64) -> u16 {
    (cost >> 50) as u16
}

fn local(position: [f64; 3], origin: [f64; 3]) -> [f64; 3] {
    [
        position[0] - origin[0],
        position[1] - origin[1],
        position[2] - origin[2],
    ]
}

fn score(
    from: usize,
    to: usize,
    positions: &[[f64; 3]],
    quadrics: &[Quadric],
    origin: [f64; 3],
) -> Option<(u64, u64, usize)> {
    let a = *positions.get(from)?;
    let b = *positions.get(to)?;
    let cost = quadrics
        .get(from)?
        .evaluate_pair_at(quadrics.get(to)?, local(b, origin))?;
    let delta = local(b, a);
    Some((cost.to_bits(), dot(delta, delta).to_bits(), to))
}

fn queue_vertex(
    work: &mut Work,
    queued: &mut [QueueState],
    ticket: &mut usize,
    vertex: usize,
    priority: u16,
) -> Option<()> {
    let flag = queued.get_mut(vertex)?;
    if matches!(flag, QueueState::Idle) {
        *flag = QueueState::Ready;
        work.push(Reverse((priority, *ticket, vertex)));
        *ticket = ticket.checked_add(1)?;
    }
    Some(())
}

/// Reduce a persistent mesh with topology, crease, seam and crossing guards.
///
/// `corner_normals`, when supplied, has exactly one finite vector per index.
/// The target is a lower limit: a collapse that would pass it is not performed.
/// The result can contain more triangles than requested, including the unchanged
/// input, when the conservative restrictions prevent further collapses.
///
/// # Errors
/// Returns an error for malformed indices, non-finite data, degenerate triangles,
/// duplicate faces, or edges shared by more than two faces or with inconsistent winding.
pub fn reduce_mesh(
    positions: &[[f64; 3]],
    indices: &[u32],
    corner_normals: Option<&[[f64; 3]]>,
    target_triangles: usize,
) -> Result<ReducedMesh> {
    reduce_mesh_with_cancel(positions, indices, corner_normals, target_triangles, || {
        false
    })
}

/// Reduce a persistent mesh, stopping cooperatively when `cancel` returns true.
pub fn reduce_mesh_with_cancel(
    positions: &[[f64; 3]],
    indices: &[u32],
    corner_normals: Option<&[[f64; 3]]>,
    target_triangles: usize,
    mut cancel: impl FnMut() -> bool,
) -> Result<ReducedMesh> {
    check_cancel(&mut cancel)?;
    if indices.len() % 3 != 0 {
        return Err(invalid("indices are not triangle triples"));
    }
    for (i, position) in positions.iter().enumerate() {
        check_stride(i, &mut cancel)?;
        if position.iter().any(|x| !x.is_finite()) {
            return Err(invalid("non-finite position"));
        }
    }
    if let Some(normals) = corner_normals {
        if normals.len() != indices.len() {
            return Err(invalid("invalid corner normals"));
        }
        for (i, normal) in normals.iter().enumerate() {
            check_stride(i, &mut cancel)?;
            if normal.iter().any(|x| !x.is_finite()) {
                return Err(invalid("invalid corner normals"));
            }
        }
    }
    let original_triangles = indices.len() / 3;
    check_cancel(&mut cancel)?;
    let protected =
        topology::validate_with_cancel(indices, positions, corner_normals, &mut cancel)?;
    check_cancel(&mut cancel)?;
    let origin = positions.first().copied().unwrap_or([0.; 3]);
    let mut quadrics = filled(positions.len(), Quadric::default())?;
    let mut faces = buffer(original_triangles)?;
    let mut incident = filled(positions.len(), Vec::<usize>::new())?;
    for (id, tri) in indices.chunks_exact(3).enumerate() {
        check_stride(id, &mut cancel)?;
        let [a, b, c] = tri else {
            return Err(invalid("invalid triangle"));
        };
        let vertices = [*a as usize, *b as usize, *c as usize];
        let face = Face { vertices };
        let [p, q, r] =
            face_points(&face, positions).ok_or_else(|| invalid("invalid triangle indices"))?;
        if !(0..3).any(|axis| orient(p, q, r, axis).is_some()) {
            return Err(invalid("degenerate or numerically uncertain triangle"));
        }
        let quadric = Quadric::from_triangle(local(p, origin), local(q, origin), local(r, origin))
            .ok_or_else(|| invalid("triangle is outside the finite quadric range"))?;
        for vertex in vertices {
            let total = quadrics
                .get_mut(vertex)
                .ok_or_else(|| invalid("invalid triangle indices"))?;
            *total = total
                .merge(&quadric)
                .ok_or_else(|| invalid("vertex quadric overflow"))?;
        }
        for vertex in vertices {
            let ids = incident
                .get_mut(vertex)
                .ok_or_else(|| invalid("invalid triangle indices"))?;
            ids.try_reserve(1)
                .map_err(|_| invalid("mesh incidence is too large"))?;
            ids.push(id);
        }
        faces.push(Some(face));
    }
    check_cancel(&mut cancel)?;
    let bvh = collision::Bvh::new_with_cancel(
        faces.len(),
        |id| {
            faces
                .get(id)?
                .as_ref()
                .and_then(|face| face_points(face, positions))
        },
        &mut cancel,
    )?;
    check_cancel(&mut cancel)?;
    let normals = if let Some(normals) = corner_normals {
        let mut grouped = buffer(original_triangles)?;
        for (id, triangle) in normals.chunks_exact(3).enumerate() {
            check_stride(id, &mut cancel)?;
            let &[a, b, c] = triangle else {
                return Err(invalid("invalid corner normals"));
            };
            grouped.push([a, b, c]);
        }
        Some(grouped)
    } else {
        None
    };
    let mut mesh = WorkingMesh {
        positions,
        faces,
        normals,
        incident,
        bvh,
        clear: filled(original_triangles, false)?,
    };
    let mut remaining = original_triangles;
    let mut pending = Work::new();
    pending
        .try_reserve_exact(positions.len())
        .map_err(|_| invalid("mesh work queue is too large"))?;
    let mut queued = filled(positions.len(), QueueState::Idle)?;
    let mut ticket = 0;
    for (vertex, ids) in mesh.incident.iter().enumerate() {
        check_stride(vertex, &mut cancel)?;
        if protected.get(vertex).copied().unwrap_or(true) {
            continue;
        }
        let Some(link) = star(vertex, &mesh.faces, ids, true) else {
            continue;
        };
        let links = link
            .links()
            .ok_or_else(|| invariant("invalid vertex star size"))?;
        let Some(cost) = links
            .iter()
            .filter_map(|&(to, _)| score(vertex, to, positions, &quadrics, origin))
            .min()
        else {
            continue;
        };
        queue_vertex(&mut pending, &mut queued, &mut ticket, vertex, band(cost.0))
            .ok_or_else(|| invalid("mesh reduction work queue overflow"))?;
    }
    let mut proposed = buffer(MAX_STAR)?;
    let mut neighbours = buffer(MAX_STAR)?;
    check_cancel(&mut cancel)?;
    let mut attempts = 0usize;
    while remaining >= target_triangles.saturating_add(2) {
        if attempts % 256 == 0 {
            check_cancel(&mut cancel)?;
        }
        attempts = attempts.wrapping_add(1);
        let Some(Reverse((priority, _, from))) = pending.pop() else {
            break;
        };
        let flag = queued
            .get_mut(from)
            .ok_or_else(|| invalid("invalid queued vertex"))?;
        let previous = std::mem::replace(flag, QueueState::Idle);
        if priority == 0 && matches!(previous, QueueState::Deferred) {
            queue_vertex(&mut pending, &mut queued, &mut ticket, from, priority)
                .ok_or_else(|| invalid("mesh reduction work queue overflow"))?;
            continue;
        }
        let Some(from_star) = mesh
            .incident
            .get(from)
            .and_then(|ids| star(from, &mesh.faces, ids, true))
        else {
            continue;
        };
        let links = from_star
            .links()
            .ok_or_else(|| invariant("invalid vertex star size"))?;
        neighbours.clear();
        neighbours.extend(
            links
                .iter()
                .filter_map(|&(to, _)| score(from, to, positions, &quadrics, origin)),
        );
        neighbours.sort_unstable();
        for &(cost, _, to) in &neighbours {
            let next = band(cost);
            if next > priority {
                queue_vertex(&mut pending, &mut queued, &mut ticket, from, next)
                    .ok_or_else(|| invalid("mesh reduction work queue overflow"))?;
                break;
            }
            let Some(merged) = quadrics.get(from).and_then(|a| a.merge(quadrics.get(to)?)) else {
                continue;
            };
            // The cost orders candidates. It never overrides a guard.
            let Some(Collapse { retained_normal }) =
                mesh.candidate(from, to, &from_star, &mut proposed, &mut cancel)?
            else {
                continue;
            };
            *quadrics
                .get_mut(to)
                .ok_or_else(|| invalid("invalid retained vertex"))? = merged;
            let affected = std::mem::take(
                mesh.incident
                    .get_mut(from)
                    .ok_or_else(|| invalid("invalid collapse vertex"))?,
            );
            let mut changed = proposed.iter().peekable();
            for id in affected {
                let slot = mesh
                    .faces
                    .get_mut(id)
                    .ok_or_else(|| invalid("invalid affected face"))?;
                let face = slot
                    .take()
                    .ok_or_else(|| invalid("missing affected face"))?;
                for vertex in face.vertices {
                    if vertex == from {
                        continue;
                    }
                    let ids = mesh
                        .incident
                        .get_mut(vertex)
                        .ok_or_else(|| invalid("invalid incident vertex"))?;
                    let i = ids
                        .iter()
                        .position(|&other| other == id)
                        .ok_or_else(|| invalid("missing incident face"))?;
                    // Remove by moving the last face ID into its slot. `i` was found
                    // in this vec; the slot is absent only when it was the last one.
                    let last = ids
                        .pop()
                        .ok_or_else(|| invariant("empty incident face list"))?;
                    if let Some(slot) = ids.get_mut(i) {
                        *slot = last;
                    }
                    if !protected.get(vertex).copied().unwrap_or(true) {
                        // Flat regions have many zero-cost ties. Let untouched
                        // stars go first instead of growing a long fan in
                        // the input's vertex order.
                        if priority == 0 {
                            let flag = queued
                                .get_mut(vertex)
                                .ok_or_else(|| invalid("invalid incident vertex"))?;
                            if matches!(flag, QueueState::Ready) {
                                *flag = QueueState::Deferred;
                            }
                        }
                        queue_vertex(&mut pending, &mut queued, &mut ticket, vertex, priority)
                            .ok_or_else(|| invalid("mesh reduction work queue overflow"))?;
                    }
                }
                if face.vertices.contains(&to) {
                    remaining -= 1;
                    mesh.bvh.update(id, None)?;
                } else {
                    let replacement = changed
                        .next_if(|replacement| replacement.id == id)
                        .ok_or_else(|| invalid("missing replacement face"))?;
                    let face = replacement.face;
                    if let Some(normals) = &mut mesh.normals {
                        let corner = face
                            .vertices
                            .iter()
                            .position(|&v| v == to)
                            .ok_or_else(|| invalid("missing retained corner"))?;
                        let value =
                            retained_normal.ok_or_else(|| invalid("missing retained normal"))?;
                        *normals
                            .get_mut(id)
                            .and_then(|n| n.get_mut(corner))
                            .ok_or_else(|| invalid("missing replacement normal"))? = value;
                    }
                    for vertex in face.vertices {
                        let ids = mesh
                            .incident
                            .get_mut(vertex)
                            .ok_or_else(|| invariant("invalid replacement vertex"))?;
                        ids.try_reserve(1)
                            .map_err(|_| invalid("mesh incidence is too large"))?;
                        ids.push(id);
                    }
                    mesh.bvh.update(id, Some(replacement.points))?;
                    *slot = Some(face);
                    *mesh
                        .clear
                        .get_mut(id)
                        .ok_or_else(|| invalid("invalid replacement face id"))? = true;
                }
            }
            break;
        }
    }
    check_cancel(&mut cancel)?;
    let mut out_positions = Vec::new();
    let mut remap = buffer(positions.len())?;
    for (i, (position, used)) in positions.iter().zip(&mesh.incident).enumerate() {
        check_stride(i, &mut cancel)?;
        if used.is_empty() {
            remap.push(None);
        } else {
            let index = u32::try_from(out_positions.len())
                .map_err(|_| invalid("vertex index exceeds u32"))?;
            out_positions
                .try_reserve(1)
                .map_err(|_| invalid("mesh output is too large"))?;
            remap.push(Some(index));
            out_positions.push(*position);
        }
    }
    let mut out_indices = buffer(remaining * 3)?;
    let mut out_normals = corner_normals.map(|_| buffer(remaining * 3)).transpose()?;
    for (id, face) in mesh.faces.into_iter().enumerate() {
        check_stride(id, &mut cancel)?;
        let Some(face) = face else { continue };
        for vertex in face.vertices {
            out_indices.push(
                remap
                    .get(vertex)
                    .copied()
                    .flatten()
                    .ok_or_else(|| invalid("invalid output vertex"))?,
            );
        }
        if let (Some(out), Some(normals)) = (&mut out_normals, &mesh.normals) {
            out.extend(
                normals
                    .get(id)
                    .ok_or_else(|| invalid("missing output normal"))?,
            );
        }
    }
    check_cancel(&mut cancel)?;
    Ok(ReducedMesh {
        positions: out_positions,
        indices: out_indices,
        corner_normals: out_normals,
        original_triangles,
        reduced_triangles: remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: u32) -> (Vec<[f64; 3]>, Vec<u32>) {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for y in 0..=n {
            for x in 0..=n {
                positions.push([f64::from(x), f64::from(y), 0.0]);
            }
        }
        for y in 0..n {
            for x in 0..n {
                let a = y * (n + 1) + x;
                let b = a + 1;
                let c = a + n + 1;
                let d = c + 1;
                indices.extend([a, b, d, a, d, c]);
            }
        }
        (positions, indices)
    }

    fn area(positions: &[[f64; 3]], indices: &[u32]) -> f64 {
        indices
            .chunks_exact(3)
            .map(|t| {
                let [a, b, c] = [
                    positions[t[0] as usize],
                    positions[t[1] as usize],
                    positions[t[2] as usize],
                ];
                let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                assert!(cross > 0.0);
                cross * 0.5
            })
            .sum()
    }

    #[test]
    fn cancellation_stops_after_reduction_has_started() {
        let (positions, indices) = grid(100);
        let mut full_run_checks = 0;
        let complete = reduce_mesh_with_cancel(&positions, &indices, None, 1000, || {
            full_run_checks += 1;
            false
        })
        .unwrap();
        assert!(complete.reduced_triangles < complete.original_triangles);
        let stop_at = full_run_checks / 2;
        assert!(stop_at > 1);
        let mut checks = 0;
        let cancelled = reduce_mesh_with_cancel(&positions, &indices, None, 1000, || {
            checks += 1;
            checks >= stop_at
        });
        assert!(cancelled.unwrap_err().is_cancelled());
        assert_eq!(checks, stop_at, "cancelled work must not continue");
    }

    #[test]
    fn planar_grid_reduces_without_changing_area_or_boundary() {
        let (positions, indices) = grid(8);
        let result = reduce_mesh(&positions, &indices, None, 80).unwrap();
        assert!(result.reduced_triangles < result.original_triangles);
        assert!(result.reduced_triangles >= 80);
        assert_eq!(result.indices.len(), 3 * result.reduced_triangles);
        assert!(result.positions.len() < positions.len());
        assert_eq!(area(&result.positions, &result.indices), 64.0);
        for y in 0..=8 {
            for x in 0..=8 {
                if x == 0 || x == 8 || y == 0 || y == 8 {
                    assert!(result
                        .positions
                        .contains(&[f64::from(x), f64::from(y), 0.0]));
                }
            }
        }
    }

    #[test]
    fn an_interior_fan_reduces_to_fixed_boundary_and_keeps_corner_attributes() {
        let positions = [
            [0., 0., 0.],
            [2., 0., 0.],
            [2., 2., 0.],
            [0., 2., 0.],
            [1., 1., 0.],
        ];
        let faces = [0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4];
        let vertex_normals = [
            [0., 0., 1.],
            [0.6, 0., 0.8],
            [0., 0.6, 0.8],
            [-0.6, 0., 0.8],
            [0., -0.6, 0.8],
        ];
        let normals = faces.map(|i| vertex_normals[i as usize]);
        let result = reduce_mesh(&positions, &faces, Some(&normals), 2).unwrap();
        assert_eq!(result.reduced_triangles, 2);
        assert_eq!(result.positions, positions[..4]);
        assert_eq!(area(&result.positions, &result.indices), 4.);
        let mut edges = std::collections::BTreeMap::new();
        for t in result.indices.chunks_exact(3) {
            for [a, b] in [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]] {
                *edges.entry([a.min(b), a.max(b)]).or_insert(0) += 1;
            }
        }
        let boundary: Vec<_> = edges
            .into_iter()
            .filter_map(|(edge, n)| (n == 1).then_some(edge))
            .collect();
        assert_eq!(boundary, [[0, 1], [0, 3], [1, 2], [2, 3]]);
        let expected: Vec<_> = result
            .indices
            .iter()
            .map(|&i| vertex_normals[i as usize])
            .collect();
        assert_eq!(result.corner_normals, Some(expected));
    }

    #[test]
    fn tilted_planar_grid_reduces_and_keeps_its_plane() {
        let (mut positions, indices) = grid(8);
        for p in &mut positions {
            let [x, y, _] = *p;
            *p = [x + y, x - y, x + 2.0 * y];
        }
        let result = reduce_mesh(&positions, &indices, None, 80).unwrap();
        assert!(result.reduced_triangles < result.original_triangles);
        assert!(result.positions.len() < positions.len());
        for p in &result.positions {
            assert!(positions.contains(p));
            assert_eq!(2.0 * p[2], 3.0 * p[0] - p[1]);
        }
    }

    #[test]
    fn normal_seams_are_preserved() {
        let (positions, indices) = grid(5);
        let normals: Vec<_> = indices
            .iter()
            .enumerate()
            .map(|(i, _)| [i as f64, 0.0, 1.0])
            .collect();
        let result = reduce_mesh(&positions, &indices, Some(&normals), 0).unwrap();
        assert_eq!(result.indices, indices);
        assert_eq!(result.corner_normals, Some(normals));
    }

    #[test]
    fn a_corner_normal_seam_at_only_one_endpoint_protects_the_edge() {
        let positions = [
            [0., 0., 0.],
            [2., 0., 0.],
            [2., 2., 0.],
            [0., 2., 0.],
            [1., 1., 0.],
        ];
        let indices = [0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4];
        let mut normals = [[0., 0., 1.]; 12];
        normals[1] = [0.6, 0., 0.8];
        let result = reduce_mesh(&positions, &indices, Some(&normals), 2).unwrap();
        assert_eq!(result.indices, indices);
        assert_eq!(result.corner_normals.as_deref(), Some(normals.as_slice()));

        // A change in length alone does not create a shading seam.
        normals[1] = [0., 0., 1.];
        normals[2] = [0., 0., 2.];
        let result = reduce_mesh(&positions, &indices, Some(&normals), 2).unwrap();
        assert_eq!(result.reduced_triangles, 2);
        assert_eq!(area(&result.positions, &result.indices), 4.);
    }

    #[test]
    fn gently_curved_scan_reduces() {
        let (mut positions, indices) = grid(8);
        for p in &mut positions {
            p[2] = 0.01 * (p[0] * p[0] + p[1] * p[1]) + 0.001 * ((p[0] + p[1]) % 2.0);
        }
        let result = reduce_mesh(&positions, &indices, None, 80).unwrap();
        assert!(result.reduced_triangles < result.original_triangles);
        assert!(result.reduced_triangles >= 80);
        assert_eq!(result.indices.len(), 3 * result.reduced_triangles);
    }

    #[test]
    fn malformed_meshes_are_refused() {
        let (positions, indices) = grid(2);
        assert!(reduce_mesh(&positions, &[0, 1], None, 0).is_err());
        assert!(reduce_mesh(&positions, &[0, 1, u32::MAX], None, 0).is_err());
        assert!(reduce_mesh(&positions, &indices, Some(&[]), 0).is_err());
        assert!(reduce_mesh(&positions, &[0, 1, 4, 0, 1, 3], None, 0).is_err());
        let mut bad = positions;
        bad[0][0] = f64::NAN;
        assert!(reduce_mesh(&bad, &indices, None, 0).is_err());
    }
}
