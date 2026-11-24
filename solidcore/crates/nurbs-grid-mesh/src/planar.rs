#![allow(clippy::indexing_slicing, clippy::neg_cmp_op_on_partial_ord)]

//! Ear-clipping triangulation for planar faces with holes.
//!
//! A `PlanarBody` face is one outer loop plus zero or more inner loops (holes). This
//! module turns such a face into triangles by the classic bridge-and-ear-clip method:
//!
//! 1. Project the loops onto the face plane (2D coordinates, right-handed about the
//!    outer loop's Newell normal).
//! 2. Normalize winding internally: outer counter-clockwise, holes clockwise.
//! 3. Bridge every hole into the outer polygon with a zero-width corridor to the
//!    nearest visible ring vertex (the ring grows hole by hole).
//! 4. Ear-clip the resulting single ring. Each ear must be convex, contain no other
//!    ring vertex, and never produce a zero-area triangle.
//! 5. Check that the triangles' total area equals `outer area - hole areas`; a mismatch
//!    is an error rather than a silently wrong mesh.
//!
//! Holes are always represented; there is no fan-fallback that would ignore them.

use crate::MeshError;

/// A neutral planar face used by the triangulator.
#[allow(dead_code)]
struct PlanarBody {
    points: Vec<[f64; 3]>,
    faces: Vec<Vec<usize>>,
    holes: Vec<Vec<Vec<usize>>>,
}

impl PlanarBody {
    fn face_holes(&self, face: usize) -> &[Vec<usize>] {
        self.holes.get(face).map_or(&[], Vec::as_slice)
    }
    fn loops(&self, face: usize) -> impl Iterator<Item = &[usize]> {
        std::iter::once(self.faces[face].as_slice())
            .chain(self.face_holes(face).iter().map(Vec::as_slice))
    }
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}
fn unit(vector: [f64; 3], what: &str) -> Result<[f64; 3], String> {
    let length = norm(vector);
    if !length.is_finite() || length == 0.0 {
        return Err(format!("{what} is degenerate: {vector:?}"));
    }
    Ok([vector[0] / length, vector[1] / length, vector[2] / length])
}
fn newell_normal(points: &[[f64; 3]], loop_points: &[usize]) -> Option<[f64; 3]> {
    let origin = points[*loop_points.first()?];
    let mut normal = [0.0f64; 3];
    for position in 0..loop_points.len() {
        let a = sub3(points[loop_points[position]], origin);
        let b = sub3(
            points[loop_points[(position + 1) % loop_points.len()]],
            origin,
        );
        normal = add3(normal, cross3(a, b));
    }
    let length = norm(normal);
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    Some([normal[0] / length, normal[1] / length, normal[2] / length])
}

/// A point projected onto the face plane.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Uv {
    x: f64,
    y: f64,
}

impl Uv {
    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

fn cross2(a: Uv, b: Uv) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dot2(a: Uv, b: Uv) -> f64 {
    a.x * b.x + a.y * b.y
}

fn distance2(a: Uv, b: Uv) -> f64 {
    dot2(a.sub(b), a.sub(b))
}

fn same_position(a: Uv, b: Uv, tolerance: f64) -> bool {
    distance2(a, b) <= tolerance * tolerance
}

impl PlanarBody {
    /// Triangulates one face with earcut, preserving every loop vertex.
    fn earcut_face(
        &self,
        face: usize,
        coordinates: &[Uv],
        expected: f64,
        area_epsilon: f64,
    ) -> Option<Vec<[usize; 3]>> {
        let outer = self.faces.get(face)?;
        if outer.len() < 3 {
            return None;
        }
        let mut data = Vec::new();
        let mut map = Vec::new();
        let mut hole_starts = Vec::new();
        for (loop_index, loop_points) in self.loops(face).enumerate() {
            if loop_index > 0 {
                hole_starts.push(map.len());
            }
            for &index in loop_points {
                let point = coordinates.get(index)?;
                data.push(point.x);
                data.push(point.y);
                map.push(index);
            }
        }
        let flat = crate::earcut::earcut(&data, &hole_starts, 2);
        if flat.is_empty() {
            return None;
        }
        let mut triangles = Vec::with_capacity(flat.len() / 3);
        for triangle in flat.chunks_exact(3) {
            let a = *map.get(triangle[0])?;
            let b = *map.get(triangle[1])?;
            let c = *map.get(triangle[2])?;
            if a != b && b != c && c != a {
                triangles.push([a, b, c]);
            }
        }
        if triangles.len() < 3 {
            return None;
        }
        let sq = |a: Uv, b: Uv, c: Uv| 0.5 * cross2(b.sub(a), c.sub(a)).abs();
        let actual: f64 = triangles
            .iter()
            .map(|[a, b, c]| sq(coordinates[*a], coordinates[*b], coordinates[*c]))
            .sum();
        if actual.is_finite() && (actual - expected).abs() <= expected.abs() * 1.0e-9 + area_epsilon
        {
            Some(triangles)
        } else {
            None
        }
    }

    /// Triangulates one face, honoring its holes.
    pub fn face_triangles(&self, face: usize) -> Result<Vec<[usize; 3]>, String> {
        let outer = self
            .faces
            .get(face)
            .ok_or_else(|| format!("face {face} does not exist"))?;
        if outer.len() < 3 {
            return Err(format!(
                "face {face} outer loop has fewer than three vertices"
            ));
        }
        for (hole_index, hole) in self.face_holes(face).iter().enumerate() {
            if hole.len() < 3 {
                return Err(format!(
                    "face {face} hole {hole_index} has fewer than three vertices"
                ));
            }
        }
        // Validate indices before projecting (no panics on malformed bodies).
        for loop_points in self.loops(face) {
            for &index in loop_points {
                if index >= self.points.len() {
                    return Err(format!("face {face} references missing point {index}"));
                }
            }
        }
        let normal = newell_normal(&self.points, outer)
            .ok_or_else(|| format!("face {face} outer loop is degenerate"))?;
        let basis_a = if normal[0].abs() < 0.9 {
            unit([1.0, 0.0, 0.0], "face basis")?
        } else {
            unit([0.0, 1.0, 0.0], "face basis")?
        };
        let basis_a = unit(cross3(basis_a, normal), "face basis")?;
        let basis_b = cross3(normal, basis_a);
        let project = |index: usize| -> Result<Uv, String> {
            let point = *self
                .points
                .get(index)
                .ok_or_else(|| format!("face {face} references missing point {index}"))?;
            Ok(Uv {
                x: dot3(point, basis_a),
                y: dot3(point, basis_b),
            })
        };
        let mut coordinates = Vec::with_capacity(self.points.len());
        for index in 0..self.points.len() {
            coordinates.push(project(index)?);
        }
        let uv = |index: usize| coordinates[index];

        let scale = face_scale(self, face, &coordinates);
        let tolerance = scale * 1.0e-9;
        let area_epsilon = scale * scale * 1.0e-12;

        let outer_area = signed_area(outer, &uv);
        if !(outer_area > area_epsilon) {
            return Err(format!(
                "face {face} outer loop is degenerate (area {outer_area})"
            ));
        }
        let mut holes = Vec::with_capacity(self.face_holes(face).len());
        for (hole_index, hole) in self.face_holes(face).iter().enumerate() {
            let mut hole = hole.clone();
            if signed_area(&hole, &uv) > 0.0 {
                // Ear clipping needs holes opposite to the outer loop.
                hole.reverse();
            }
            let hole_area = signed_area(&hole, &uv).abs();
            if !(hole_area > area_epsilon) {
                return Err(format!(
                    "face {face} hole {hole_index} is degenerate (area {hole_area})"
                ));
            }
            holes.push(hole);
        }

        let expected = outer_area
            - holes
                .iter()
                .map(|hole| signed_area(hole, &uv).abs())
                .sum::<f64>();
        if let Some(triangles) = self.earcut_face(face, &coordinates, expected, area_epsilon) {
            return Ok(triangles);
        }

        let mut ring = outer.clone();
        for hole in &holes {
            ring = bridge_hole(&ring, hole, &holes, &uv, tolerance, face)?;
        }
        let mut triangles = Vec::new();
        ear_clip(&ring, &uv, tolerance, area_epsilon, &mut triangles, face)?;

        let actual = triangles
            .iter()
            .map(|[a, b, c]| 0.5 * cross2(uv(*b).sub(uv(*a)), uv(*c).sub(uv(*a))).abs())
            .sum::<f64>();
        if (actual - expected).abs() > expected.abs() * 1.0e-9 + area_epsilon {
            return Err(format!(
                "face {face} triangulation area {actual} does not match loop area {expected}"
            ));
        }
        Ok(triangles)
    }
}

/// Largest extent of the face's loops, used to scale tolerances.
fn face_scale(body: &PlanarBody, face: usize, coordinates: &[Uv]) -> f64 {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for loop_points in body.loops(face) {
        for &index in loop_points {
            let point = coordinates[index];
            min[0] = min[0].min(point.x);
            min[1] = min[1].min(point.y);
            max[0] = max[0].max(point.x);
            max[1] = max[1].max(point.y);
        }
    }
    let scale = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2)).sqrt();
    if scale > 0.0 && scale.is_finite() {
        scale
    } else {
        1.0
    }
}

/// Signed 2D area of a loop (positive = counter-clockwise in the projected frame).
fn signed_area(loop_points: &[usize], uv: &impl Fn(usize) -> Uv) -> f64 {
    let mut total = 0.0;
    for position in 0..loop_points.len() {
        let a = uv(loop_points[position]);
        let b = uv(loop_points[(position + 1) % loop_points.len()]);
        total += a.x * b.y - b.x * a.y;
    }
    total * 0.5
}

/// Splicing one hole into the ring with a zero-width bridge.
fn bridge_hole(
    ring: &[usize],
    hole: &[usize],
    all_holes: &[Vec<usize>],
    uv: &impl Fn(usize) -> Uv,
    tolerance: f64,
    face: usize,
) -> Result<Vec<usize>, String> {
    // Start at the hole's extreme +u vertex: a bridge from there tends to be visible.
    let mut start = 0usize;
    for index in 1..hole.len() {
        let candidate = uv(hole[index]);
        let best = uv(hole[start]);
        if candidate.x > best.x + tolerance
            || ((candidate.x - best.x).abs() <= tolerance && candidate.y > best.y)
        {
            start = index;
        }
    }
    let hole_vertex = hole[start];
    let hole_point = uv(hole_vertex);
    let mut candidates: Vec<usize> = (0..ring.len()).collect();
    candidates.sort_by(|left, right| {
        distance2(uv(ring[*left]), hole_point)
            .partial_cmp(&distance2(uv(ring[*right]), hole_point))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut bridge = None;
    for candidate in candidates {
        if segment_visible(ring[candidate], hole_vertex, ring, all_holes, uv, tolerance) {
            bridge = Some(candidate);
            break;
        }
    }
    let Some(position) = bridge else {
        return Err(format!(
            "face {face}: cannot bridge a hole into the outer loop (no visible ring vertex)"
        ));
    };
    let mut bridged = Vec::with_capacity(ring.len() + hole.len() + 2);
    bridged.extend_from_slice(&ring[..=position]);
    bridged.extend_from_slice(&hole[start..]);
    bridged.extend_from_slice(&hole[..start]);
    bridged.push(hole[start]);
    bridged.extend_from_slice(&ring[position..]);
    if bridged.len() <= ring.len() {
        return Err(format!("face {face}: hole bridge did not add vertices"));
    }
    Ok(bridged)
}

/// True when the segment `a`–`b` can become a hole bridge.
fn segment_visible(
    a: usize,
    b: usize,
    ring: &[usize],
    all_holes: &[Vec<usize>],
    uv: &impl Fn(usize) -> Uv,
    tolerance: f64,
) -> bool {
    let start = uv(a);
    let end = uv(b);
    if distance2(start, end) <= tolerance * tolerance {
        return false;
    }
    // The bridge must stay inside the current ring and outside every hole.
    let midpoint = Uv {
        x: (start.x + end.x) * 0.5,
        y: (start.y + end.y) * 0.5,
    };
    if !point_in_polygon(midpoint, ring, uv) {
        return false;
    }
    for hole in all_holes {
        if point_in_polygon(midpoint, hole, uv) {
            return false;
        }
    }
    // The bridge must not cross any ring edge or hole edge.
    let obstacles = std::iter::once(ring).chain(all_holes.iter().map(Vec::as_slice));
    for polygon in obstacles {
        for position in 0..polygon.len() {
            let c = polygon[position];
            let d = polygon[(position + 1) % polygon.len()];
            if c == a || c == b || d == a || d == b {
                continue;
            }
            let c_point = uv(c);
            let d_point = uv(d);
            if same_position(c_point, start, tolerance)
                || same_position(c_point, end, tolerance)
                || same_position(d_point, start, tolerance)
                || same_position(d_point, end, tolerance)
            {
                continue;
            }
            if segments_intersect(start, end, c_point, d_point, tolerance) {
                return false;
            }
        }
    }
    true
}

/// Even-odd point-in-polygon test on projected coordinates.
fn point_in_polygon(point: Uv, polygon: &[usize], uv: &impl Fn(usize) -> Uv) -> bool {
    let mut inside = false;
    for position in 0..polygon.len() {
        let a = uv(polygon[position]);
        let b = uv(polygon[(position + 1) % polygon.len()]);
        if (a.y > point.y) != (b.y > point.y) {
            let denominator = b.y - a.y;
            if denominator.abs() <= f64::MIN_POSITIVE {
                continue;
            }
            let x = a.x + (point.y - a.y) / denominator * (b.x - a.x);
            if point.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Proper segment intersection (touching counts as intersecting).
fn segments_intersect(a: Uv, b: Uv, c: Uv, d: Uv, tolerance: f64) -> bool {
    let epsilon = tolerance * tolerance * 1.0e-3;
    let o1 = cross2(b.sub(a), c.sub(a));
    let o2 = cross2(b.sub(a), d.sub(a));
    let o3 = cross2(d.sub(c), a.sub(c));
    let o4 = cross2(d.sub(c), b.sub(c));
    if ((o1 > epsilon && o2 < -epsilon) || (o1 < -epsilon && o2 > epsilon))
        && ((o3 > epsilon && o4 < -epsilon) || (o3 < -epsilon && o4 > epsilon))
    {
        return true;
    }
    // Collinear/touching cases: any endpoint on the other segment blocks the bridge.
    on_segment(a, c, d, epsilon)
        || on_segment(b, c, d, epsilon)
        || on_segment(c, a, b, epsilon)
        || on_segment(d, a, b, epsilon)
}

fn on_segment(point: Uv, a: Uv, b: Uv, epsilon: f64) -> bool {
    let collinear = cross2(b.sub(a), point.sub(a)).abs() <= epsilon;
    collinear
        && point.x >= a.x.min(b.x) - epsilon.max(0.0).sqrt()
        && point.x <= a.x.max(b.x) + epsilon.max(0.0).sqrt()
        && point.y >= a.y.min(b.y) - epsilon.max(0.0).sqrt()
        && point.y <= a.y.max(b.y) + epsilon.max(0.0).sqrt()
}

/// Ear clipping of one simple ring (holes already bridged in).
fn ear_clip(
    ring: &[usize],
    uv: &impl Fn(usize) -> Uv,
    tolerance: f64,
    area_epsilon: f64,
    triangles: &mut Vec<[usize; 3]>,
    face: usize,
) -> Result<(), String> {
    let mut ring = ring.to_vec();
    let mut guard = ring.len() * ring.len() + 16;
    while ring.len() > 3 {
        if guard == 0 {
            return Err(format!("face {face}: ear clipping did not terminate"));
        }
        guard -= 1;
        let count = ring.len();
        let mut ear = None;
        for index in 0..count {
            let previous = ring[(index + count - 1) % count];
            let current = ring[index];
            let next = ring[(index + 1) % count];
            let a = uv(previous);
            let b = uv(current);
            let c = uv(next);
            if distance2(a, b) <= tolerance * tolerance || distance2(b, c) <= tolerance * tolerance
            {
                continue;
            }
            let cross = cross2(b.sub(a), c.sub(b));
            if cross <= area_epsilon {
                continue;
            }
            let mut contains = false;
            for (other_index, &other) in ring.iter().enumerate() {
                if other_index == index
                    || other_index == (index + count - 1) % count
                    || other_index == (index + 1) % count
                {
                    continue;
                }
                let point = uv(other);
                if same_position(point, a, tolerance)
                    || same_position(point, b, tolerance)
                    || same_position(point, c, tolerance)
                {
                    continue;
                }
                if point_in_triangle(point, a, b, c, area_epsilon) {
                    contains = true;
                    break;
                }
            }
            if contains {
                continue;
            }
            ear = Some(index);
            break;
        }
        // Bridged hole rings can leave no strict ear even though the polygon is valid.
        // Fall back to the convex vertex with the fewest blocked points so a display mesh
        // still exists; the area check at the caller catches a genuinely bad cut.
        let index = if let Some(index) = ear {
            index
        } else {
            let mut best: Option<(usize, usize)> = None;
            for index in 0..count {
                let previous = ring[(index + count - 1) % count];
                let current = ring[index];
                let next = ring[(index + 1) % count];
                let a = uv(previous);
                let b = uv(current);
                let c = uv(next);
                if distance2(a, b) <= tolerance * tolerance
                    || distance2(b, c) <= tolerance * tolerance
                {
                    continue;
                }
                if cross2(b.sub(a), c.sub(b)) <= area_epsilon {
                    continue;
                }
                let mut blocked = 0usize;
                for (other_index, &other) in ring.iter().enumerate() {
                    if other_index == index
                        || other_index == (index + count - 1) % count
                        || other_index == (index + 1) % count
                    {
                        continue;
                    }
                    let point = uv(other);
                    if !same_position(point, a, tolerance)
                        && !same_position(point, b, tolerance)
                        && !same_position(point, c, tolerance)
                        && point_in_triangle(point, a, b, c, area_epsilon)
                    {
                        blocked += 1;
                    }
                }
                if best.is_none_or(|(_, count)| blocked < count) {
                    best = Some((index, blocked));
                }
            }
            best.map(|(index, _)| index).ok_or_else(|| {
                format!(
                    "face {face}: ear clipping stalled with {} vertices left",
                    ring.len()
                )
            })?
        };
        let previous = ring[(index + count - 1) % count];
        triangles.push([previous, ring[index], ring[(index + 1) % count]]);
        ring.remove(index);
    }
    if ring.len() == 3 {
        triangles.push([ring[0], ring[1], ring[2]]);
        Ok(())
    } else {
        Err(format!("face {face}: ear clipping produced no triangles"))
    }
}

/// Point inside (or on) a counter-clockwise triangle.
fn point_in_triangle(point: Uv, a: Uv, b: Uv, c: Uv, epsilon: f64) -> bool {
    cross2(b.sub(a), point.sub(a)) >= -epsilon
        && cross2(c.sub(b), point.sub(b)) >= -epsilon
        && cross2(a.sub(c), point.sub(c)) >= -epsilon
}

/// Triangulates one planar face with holes.
///
/// `points` are shared 3D points; `outer` and every loop in `holes` contain indices into
/// `points`. All loops are expected to be closed, simple and coplanar. The result is a
/// triangle list with indices into `points`; holes are never filled.
pub fn triangulate_planar(
    points: &[[f64; 3]],
    outer: &[usize],
    holes: &[Vec<usize>],
) -> Result<Vec<[usize; 3]>, MeshError> {
    let body = PlanarBody {
        points: points.to_vec(),
        faces: vec![outer.to_vec()],
        holes: vec![holes.to_vec()],
    };
    body.face_triangles(0).map_err(MeshError::BadPlanar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[[f64; 3]], triangles: &[[usize; 3]]) -> f64 {
        triangles
            .iter()
            .map(|[a, b, c]| {
                let u = sub3(points[*b], points[*a]);
                let v = sub3(points[*c], points[*a]);
                0.5 * cross3(u, v)[2]
            })
            .sum()
    }

    #[test]
    fn square_with_a_hole_keeps_the_hole_open() {
        let points = vec![
            [-10.0, -10.0, 0.0],
            [10.0, -10.0, 0.0],
            [10.0, 10.0, 0.0],
            [-10.0, 10.0, 0.0],
            [-4.0, -4.0, 0.0],
            [4.0, -4.0, 0.0],
            [4.0, 4.0, 0.0],
            [-4.0, 4.0, 0.0],
        ];
        let triangles =
            triangulate_planar(&points, &[0, 1, 2, 3], &[vec![4, 5, 6, 7]]).expect("triangulates");
        assert_eq!(triangles.len(), 8);
        assert!((area(&points, &triangles) - (400.0 - 64.0)).abs() < 1e-9);
    }

    fn polygon_area_2d(points: &[[f64; 3]], loop_indices: &[usize]) -> f64 {
        let mut area = 0.0;
        for i in 0..loop_indices.len() {
            let a = points[loop_indices[i]];
            let b = points[loop_indices[(i + 1) % loop_indices.len()]];
            area += a[0] * b[1] - b[0] * a[1];
        }
        area * 0.5
    }

    #[test]
    fn earcut_preserves_a_dense_hole_boundary() {
        let mut points = vec![
            [-10.0, -10.0, 0.0],
            [10.0, -10.0, 0.0],
            [10.0, 10.0, 0.0],
            [-10.0, 10.0, 0.0],
        ];
        let hole_start = points.len();
        for i in 0..90 {
            let angle = std::f64::consts::TAU * i as f64 / 90.0;
            points.push([4.0 * angle.cos(), 4.0 * angle.sin(), 0.0]);
        }
        let hole: Vec<usize> = (hole_start..points.len()).collect();
        let triangles = triangulate_planar(&points, &[0, 1, 2, 3], &[hole.clone()])
            .expect("dense holed cap triangulates");
        let expected = 400.0 - polygon_area_2d(&points, &hole).abs();
        assert!((area(&points, &triangles).abs() - expected).abs() < 1.0e-9);

        // Every hole boundary edge must be used by exactly one triangle. A fan that
        // ignored the hole, or a bridge that dropped vertices, fails this.
        let mut edge_uses = std::collections::BTreeMap::new();
        for [a, b, c] in &triangles {
            for (u, v) in [(*a, *b), (*b, *c), (*c, *a)] {
                let key = if u < v { (u, v) } else { (v, u) };
                *edge_uses.entry(key).or_insert(0) += 1;
            }
        }
        for i in 0..hole.len() {
            let a = hole[i];
            let b = hole[(i + 1) % hole.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            assert_eq!(
                edge_uses.get(&key),
                Some(&1),
                "hole edge {key:?} is not preserved"
            );
        }
    }

    #[test]
    fn earcut_handles_two_holes_and_keeps_them_open() {
        let mut points = vec![
            [-10.0, -10.0, 0.0],
            [10.0, -10.0, 0.0],
            [10.0, 10.0, 0.0],
            [-10.0, 10.0, 0.0],
        ];
        let mut holes = Vec::new();
        for center in [[-4.0, 0.0], [4.0, 0.0]] {
            let start = points.len();
            for i in 0..24 {
                let angle = std::f64::consts::TAU * i as f64 / 24.0;
                points.push([
                    center[0] + 2.5 * angle.cos(),
                    center[1] + 2.5 * angle.sin(),
                    0.0,
                ]);
            }
            holes.push((start..points.len()).collect::<Vec<_>>());
        }
        let triangles =
            triangulate_planar(&points, &[0, 1, 2, 3], &holes).expect("two-holed cap triangulates");
        let expected = 400.0
            - holes
                .iter()
                .map(|hole| polygon_area_2d(&points, hole).abs())
                .sum::<f64>();
        assert!((area(&points, &triangles).abs() - expected).abs() < 1.0e-9);
    }

    #[test]
    fn reflex_corner_on_a_candidate_edge_is_not_cut_across() {
        // A U-shaped face. The reflex corners sit on lines that a naive ear clip may use.
        let points = vec![
            [-10.0, -10.0, 0.0],
            [10.0, -10.0, 0.0],
            [10.0, 10.0, 0.0],
            [4.0, 10.0, 0.0],
            [4.0, -2.0, 0.0],
            [-4.0, -2.0, 0.0],
            [-4.0, 10.0, 0.0],
            [-10.0, 10.0, 0.0],
        ];
        let triangles =
            triangulate_planar(&points, &[0, 1, 2, 3, 4, 5, 6, 7], &[]).expect("triangulates");
        let expected = 20.0 * 20.0 - 8.0 * 12.0;
        assert!((area(&points, &triangles) - expected).abs() < 1e-9);
        assert!(triangles.len() >= 6);
    }
}
