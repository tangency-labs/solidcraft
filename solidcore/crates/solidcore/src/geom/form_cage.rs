//! Control-cage coordinates and topology for subdivision primitive families.
//! All sampling counts are bounded before constructing coordinates or faces.

use crate::geom::affine_fit;
use crate::{Result, SolidError, Vec3};
use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

pub mod extrusion;
pub mod projection;

/// Maximum number of authored path points accepted by a primitive cage.
pub const MAX_PATH_POINTS: usize = 128;
/// Maximum number of controls accepted by cage operations.
pub const MAX_POINTS: usize = 40_000;
/// Maximum number of faces accepted by cage operations.
pub const MAX_FACES: usize = 30_000;

/// Primitive control-cage topology.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Primitive {
    /// A rectangular solid.
    Box,
    /// An open rectangular grid.
    Plane,
    /// A spherified cube.
    Quadball,
    /// A cylinder with quad end caps.
    Cylinder,
    /// A latitude-longitude sphere with quad poles.
    Sphere,
    /// A periodic torus.
    Torus,
    /// A hollow, closed tube.
    Tube,
    /// A capped pipe on an open sampled path.
    Pipe,
}

/// A generated control cage before authoring creases or hierarchy.
pub struct CageGeometry {
    /// Control coordinates in millimetres.
    pub points: Vec<Vec3>,
    /// Oriented quad faces indexing the controls.
    pub faces: Vec<Vec<usize>>,
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "form cage",
        reason,
    }
}

fn finite(v: Vec3) -> bool {
    v.to_array().into_iter().all(f64::is_finite)
}

fn unit(v: Vec3) -> Option<Vec3> {
    let length = v.length();
    (length.is_finite() && length > 1e-9).then(|| v / length)
}

fn perpendicular(v: Vec3) -> Result<Vec3> {
    let axis = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    unit(v.cross(axis)).ok_or_else(|| bad("pipe frame"))
}

/// Check the authored size and sampling metadata independently of cage generation.
pub fn validate(
    dimensions: [f64; 3],
    segments: [usize; 3],
    radii: [f64; 2],
    path: &[Vec3],
) -> Result<()> {
    if dimensions.iter().any(|v| !(0.1..=10_000.0).contains(v))
        || segments.iter().any(|n| !(1..=16).contains(n))
    {
        return Err(bad(
            "primitive dimensions must be 0.1..10000 mm and segments 1..16",
        ));
    }
    if radii.iter().any(|r| !(0.0..=10_000.0).contains(r))
        || path.len() > MAX_PATH_POINTS
        || path.iter().any(|&p| !finite(p) || p.length() > 100_000.0)
    {
        return Err(bad(
            "primitive metadata must contain finite bounded radii and path points",
        ));
    }
    Ok(())
}

/// Generate a bounded cage. Face and vertex ordering are stable across rebuilds.
pub fn build(
    kind: Primitive,
    dimensions: [f64; 3],
    segments: [usize; 3],
    radii: [f64; 2],
    path: &[Vec3],
) -> Result<CageGeometry> {
    validate(dimensions, segments, radii, path)?;
    let [width, _, height] = dimensions;
    let [around, along, _] = segments;
    let [outer, inner] = radii;
    let radius = width / 2.0;
    match kind {
        Primitive::Box => Ok(box_grid(dimensions, segments, false)),
        Primitive::Plane => Ok(box_grid(dimensions, segments, true)),
        Primitive::Quadball => {
            let mut cage = box_grid([2.0; 3], [around; 3], false);
            for p in &mut cage.points {
                let (x, y, z) = (p.x, p.y, p.z);
                let (xx, yy, zz) = (x * x, y * y, z * z);
                *p = Vec3::new(
                    x * (1.0 - yy / 2.0 - zz / 2.0 + yy * zz / 3.0).max(0.0).sqrt(),
                    y * (1.0 - zz / 2.0 - xx / 2.0 + zz * xx / 3.0).max(0.0).sqrt(),
                    z * (1.0 - xx / 2.0 - yy / 2.0 + xx * yy / 3.0).max(0.0).sqrt(),
                ) * radius;
            }
            Ok(cage)
        }
        Primitive::Cylinder => Ok(cylinder(radius, height, around, along)),
        Primitive::Sphere => Ok(sphere(radius, around, along)),
        Primitive::Torus => torus(outer, inner, around, along),
        Primitive::Tube => tube(outer, inner, height, around, along),
        Primitive::Pipe => pipe(radius, path, around, along, height),
    }
}

fn pipe_section([u0, v0]: [Vec3; 2], [u1, v1]: [Vec3; 2], t: f64) -> Result<[Vec3; 3]> {
    let u = u0 * (1.0 - t) + u1 * t;
    let v = v0 * (1.0 - t) + v1 * t;
    affine_fit::frame(u, v).ok_or_else(|| bad("pipe frame flips"))
}

fn box_grid(d: [f64; 3], n: [usize; 3], plane: bool) -> CageGeometry {
    let [nx, ny, nz] = n.map(|v| v as i32);
    let [dx, dy, dz] = d;
    let plane_frame = [([0, 0, 0], [1, 0, 0], [0, 1, 0], nx, ny)];
    let box_frames = [
        ([0, 0, nz], [1, 0, 0], [0, 1, 0], nx, ny),
        ([0, ny, nz], [1, 0, 0], [0, 0, -1], nx, nz),
        ([nx, 0, 0], [0, 1, 0], [0, 0, 1], ny, nz),
        ([0, 0, 0], [0, 1, 0], [1, 0, 0], ny, nx),
        ([0, 0, 0], [1, 0, 0], [0, 0, 1], nx, nz),
        ([0, 0, 0], [0, 0, 1], [0, 1, 0], nz, ny),
    ];
    let frames = if plane {
        plane_frame.as_slice()
    } else {
        box_frames.as_slice()
    };
    let mut points = Vec::new();
    let mut faces = Vec::new();
    let mut ids = BTreeMap::new();
    for &([ox, oy, oz], [ux, uy, uz], [vx, vy, vz], ns, nt) in frames {
        for t in 0..nt {
            for s in 0..ns {
                let mut face = [0; 4];
                for (slot, (a, b)) in
                    face.iter_mut()
                        .zip([(s, t), (s + 1, t), (s + 1, t + 1), (s, t + 1)])
                {
                    let k = [
                        ox + a * ux + b * vx,
                        oy + a * uy + b * vy,
                        oz + a * uz + b * vz,
                    ];
                    let id = *ids.entry(k).or_insert_with(|| {
                        let id = points.len();
                        let [x, y, z] = k;
                        points.push(Vec3::new(
                            (f64::from(x) / f64::from(nx) - 0.5) * dx,
                            (f64::from(y) / f64::from(ny) - 0.5) * dy,
                            if plane {
                                0.0
                            } else {
                                (f64::from(z) / f64::from(nz) - 0.5) * dz
                            },
                        ));
                        id
                    });
                    *slot = id;
                }
                faces.push(face.to_vec());
            }
        }
    }
    CageGeometry { points, faces }
}
fn cylinder(radius: f64, height: f64, sides: usize, layers: usize) -> CageGeometry {
    let mut c = box_grid(
        [2.0 * radius, 2.0 * radius, height],
        [2 * sides, 2 * sides, layers],
        false,
    );
    for p in &mut c.points {
        let distance = p.x.abs().max(p.y.abs());
        if distance > radius * 1e-12 {
            let (u, v) = (p.x / distance, p.y / distance);
            let angle = if u.abs() >= v.abs() {
                if u >= 0.0 {
                    PI / 4.0 * v
                } else {
                    PI - PI / 4.0 * v
                }
            } else if v >= 0.0 {
                PI / 2.0 - PI / 4.0 * u
            } else {
                -PI / 2.0 + PI / 4.0 * u
            };
            p.x = distance * angle.cos();
            p.y = distance * angle.sin();
        }
    }
    c
}
fn sphere(radius: f64, longitude: usize, latitude: usize) -> CageGeometry {
    let n = 8 * longitude;
    let rings = 4 * latitude - 1;
    let mut points = vec![Vec3::Z * radius];
    for j in 1..=rings {
        let a = PI * j as f64 / (rings + 1) as f64;
        for i in 0..n {
            let b = TAU * i as f64 / n as f64;
            points.push(Vec3::new(a.sin() * b.cos(), a.sin() * b.sin(), a.cos()) * radius);
        }
    }
    let south = points.len();
    points.push(-Vec3::Z * radius);
    let at = |r: usize, i: usize| 1 + r * n + i.checked_rem(n).unwrap_or(0);
    let mut faces = Vec::new();
    for i in (0..n).step_by(2) {
        faces.push([0, at(0, i), at(0, i + 1), at(0, i + 2)].to_vec());
    }
    for j in 0..rings - 1 {
        for i in 0..n {
            faces.push([at(j, i), at(j + 1, i), at(j + 1, i + 1), at(j, i + 1)].to_vec());
        }
    }
    for i in (0..n).step_by(2) {
        faces.push(
            [
                south,
                at(rings - 1, i + 2),
                at(rings - 1, i + 1),
                at(rings - 1, i),
            ]
            .to_vec(),
        );
    }
    CageGeometry { points, faces }
}
fn torus(major: f64, minor: f64, u: usize, v: usize) -> Result<CageGeometry> {
    if !major.is_finite()
        || !minor.is_finite()
        || minor <= 0.0
        || major <= minor
        || major + minor > 10_000.0
    {
        return Err(bad("torus requires 0 < minor radius < major radius"));
    }
    let (n, m) = (4 * u, 4 * v);
    let mut points = Vec::with_capacity(n * m);
    for i in 0..n {
        let a = TAU * i as f64 / n as f64;
        for j in 0..m {
            let b = TAU * j as f64 / m as f64;
            let r = major + minor * b.cos();
            points.push(Vec3::new(r * a.cos(), r * a.sin(), minor * b.sin()));
        }
    }
    let at =
        |i: usize, j: usize| (i.checked_rem(n).unwrap_or(0)) * m + j.checked_rem(m).unwrap_or(0);
    let mut faces = Vec::with_capacity(n * m);
    for i in 0..n {
        for j in 0..m {
            faces.push([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)].to_vec());
        }
    }
    Ok(CageGeometry { points, faces })
}
fn tube(outer: f64, inner: f64, height: f64, sides: usize, layers: usize) -> Result<CageGeometry> {
    if !outer.is_finite()
        || !inner.is_finite()
        || inner <= 0.0
        || outer <= inner
        || outer > 10_000.0
    {
        return Err(bad("tube requires 0 < inner radius < outer radius"));
    }
    let n = 4 * sides;
    let wall = (layers + 1) * n;
    let mut points = Vec::with_capacity(2 * wall);
    for radius in [outer, inner] {
        for j in 0..=layers {
            for i in 0..n {
                let a = TAU * i as f64 / n as f64;
                points.push(Vec3::new(
                    radius * a.cos(),
                    radius * a.sin(),
                    height * (j as f64 / layers as f64 - 0.5),
                ));
            }
        }
    }
    let at = |inside: bool, j: usize, i: usize| {
        usize::from(inside) * wall + j * n + i.checked_rem(n).unwrap_or(0)
    };
    let mut faces = Vec::new();
    for j in 0..layers {
        for i in 0..n {
            faces.push(
                [
                    at(false, j, i),
                    at(false, j, i + 1),
                    at(false, j + 1, i + 1),
                    at(false, j + 1, i),
                ]
                .to_vec(),
            );
            faces.push(
                [
                    at(true, j, i),
                    at(true, j + 1, i),
                    at(true, j + 1, i + 1),
                    at(true, j, i + 1),
                ]
                .to_vec(),
            );
        }
    }
    for i in 0..n {
        faces.push(
            [
                at(false, layers, i),
                at(false, layers, i + 1),
                at(true, layers, i + 1),
                at(true, layers, i),
            ]
            .to_vec(),
        );
        faces.push(
            [
                at(false, 0, i + 1),
                at(false, 0, i),
                at(true, 0, i),
                at(true, 0, i + 1),
            ]
            .to_vec(),
        );
    }
    Ok(CageGeometry { points, faces })
}
fn pipe(
    radius: f64,
    input: &[Vec3],
    sides: usize,
    samples: usize,
    height: f64,
) -> Result<CageGeometry> {
    let default = [
        Vec3::new(0., 0., -height / 2.),
        Vec3::ZERO,
        Vec3::new(height / 3., 0., height / 2.),
    ];
    let path = if input.is_empty() {
        &default[..]
    } else {
        input
    };
    if !(2..=MAX_PATH_POINTS).contains(&path.len()) {
        return Err(bad("pipe requires 2..128 finite path points"));
    }
    let mut directions = Vec::new();
    let mut lengths = Vec::new();
    let mut total = 0.0;
    for edge in path.windows(2) {
        let [a, b] = edge else { continue };
        let delta = *b - *a;
        let len = delta.length();
        let direction = unit(delta).ok_or_else(|| bad("pipe path has coincident points"))?;
        directions.push(direction);
        lengths.push(len);
        total += len;
    }
    let layers = samples * (path.len() - 1);
    if layers > 256 {
        return Err(bad("pipe path sample budget exceeded"));
    }
    // The capped cylinder has two square quad grids and four side grids.
    let side = 2 * sides;
    let face_count = 2 * side * side + 4 * side * layers;
    if face_count > MAX_FACES {
        return Err(bad("pipe cage exceeds the editable face budget"));
    }
    let mut frames = Vec::new();
    let mut u = perpendicular(*directions.first().ok_or_else(|| bad("empty pipe path"))?)?;
    for i in 0..path.len() {
        let tangent = match (
            i.checked_sub(1).and_then(|j| directions.get(j)),
            directions.get(i),
        ) {
            (Some(a), Some(b)) => {
                unit(*a + *b).ok_or_else(|| bad("pipe path reverses direction"))?
            }
            (Some(a), None) => *a,
            (None, Some(b)) => *b,
            _ => return Err(bad("pipe frame")),
        };
        u = unit(u - tangent * u.dot(tangent)).map_or_else(|| perpendicular(tangent), Ok)?;
        frames.push((u, tangent.cross(u)));
    }
    let mut cage = cylinder(radius, total, sides, layers);
    for p in &mut cage.points {
        let mut distance = p.z + total / 2.0;
        let mut segment = 0;
        while segment + 1 < lengths.len() && distance > lengths.get(segment).copied().unwrap_or(0.0)
        {
            distance -= lengths.get(segment).copied().unwrap_or(0.0);
            segment += 1;
        }
        let length = lengths
            .get(segment)
            .copied()
            .ok_or_else(|| bad("pipe segment"))?;
        let t = (distance / length).clamp(0.0, 1.0);
        let a = *path.get(segment).ok_or_else(|| bad("pipe segment"))?;
        let b = *path.get(segment + 1).ok_or_else(|| bad("pipe segment"))?;
        let (u0, v0) = *frames.get(segment).ok_or_else(|| bad("pipe frame"))?;
        let (u1, v1) = *frames.get(segment + 1).ok_or_else(|| bad("pipe frame"))?;
        let [up, vp, _] = pipe_section([u0, v0], [u1, v1], t)?;
        *p = a * (1.0 - t) + b * t + up * p.x + vp * p.y;
    }
    Ok(cage)
}

/// Displace each control by a fixed distance along its area-weighted incident face normal.
pub fn offset_controls(points: &[Vec3], faces: &[Vec<usize>], distance: f64) -> Result<Vec<Vec3>> {
    if points.len() > MAX_POINTS || faces.len() > MAX_FACES || !distance.is_finite() {
        return Err(bad("control normal budget or distance exceeded"));
    }
    let mut normals = vec![Vec3::ZERO; points.len()];
    let point = |index| {
        points
            .get(index)
            .copied()
            .filter(|&p| finite(p))
            .ok_or_else(|| bad("invalid normal control"))
    };
    for face in faces {
        let &[a, b, c, d] = face.as_slice() else {
            return Err(bad("control normals require quad faces"));
        };
        let origin = point(a)?;
        let b = point(b)? - origin;
        let c = point(c)? - origin;
        let d = point(d)? - origin;
        let mut normal = Vec3::ZERO;
        normal += b.cross(c);
        normal += c.cross(d);
        for &index in face {
            *normals
                .get_mut(index)
                .ok_or_else(|| bad("invalid normal control"))? += normal;
        }
    }
    points
        .iter()
        .zip(normals)
        .map(|(&p, normal)| {
            let n = unit(normal).ok_or_else(|| bad("undefined vertex normal"))?;
            let moved = p + n * distance;
            if !finite(moved) {
                return Err(bad("offset control is not finite"));
            }
            Ok(moved)
        })
        .collect()
}

/// Construct the two homogeneous rows of a regular ruled strip on an oriented seam.
/// Positive weights and strictly increasing projected controls certify transversality.
pub fn ruled_strip(seam: &[[f64; 4]], delta: Vec3) -> Result<Vec<[f64; 4]>> {
    let direction =
        unit(delta).ok_or_else(|| bad("trim extrusion requires finite nonzero displacement"))?;
    if !(2..=16_384).contains(&seam.len()) {
        return Err(bad("invalid trim extrusion control count"));
    }
    let projected = seam
        .iter()
        .map(|&[x, y, z, w]| {
            let point = Vec3::new(x / w, y / w, z / w);
            if w <= f64::MIN_POSITIVE || !w.is_finite() || !finite(point) {
                return Err(bad("invalid trim spline weight"));
            }
            Ok(point)
        })
        .collect::<Result<Vec<_>>>()?;
    let start = *projected.first().ok_or_else(|| bad("empty trim spline"))?;
    let end = *projected.last().ok_or_else(|| bad("empty trim spline"))?;
    let chord = end - start;
    let axis = unit(chord - direction * chord.dot(direction))
        .ok_or_else(|| bad("trim extrusion runs along the seam"))?;
    if projected
        .windows(2)
        .any(|pair| matches!(pair, [a, b] if (*b - *a).dot(axis) <= 1e-12))
    {
        return Err(bad("trim extrusion needs a regular transverse seam"));
    }
    let mut controls = seam.to_vec();
    for (&[x, y, z, w], p) in seam.iter().zip(projected) {
        let q = p + delta;
        if !finite(q) || q.x.abs().max(q.y.abs()).max(q.z.abs()) > 100_000.0 {
            return Err(bad(
                "trim extrusion coordinates must remain within 100000 mm",
            ));
        }
        let moved = Vec3::new(x, y, z) + delta * w;
        if !finite(moved) {
            return Err(bad(
                "trim extrusion homogeneous controls must remain finite",
            ));
        }
        controls.push([moved.x, moved.y, moved.z, w]);
    }
    Ok(controls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_sections_have_perpendicular_axes_through_nonplanar_turns() {
        let a = [Vec3::X, Vec3::Y];
        let b = [Vec3::Y, Vec3::Z];
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let axes = pipe_section(a, b, t).unwrap();
            assert!(affine_fit::proper_axes(axes, 1e-12));
            assert!(axes[0].cross(a[0] * (1.0 - t) + b[0] * t).length() < 1e-12);
        }
        assert!(pipe_section(a, [-a[0], -a[1]], 0.5).is_err());
    }

    #[test]
    fn pipe_face_budget_counts_the_end_caps_and_side_grids() {
        let path: Vec<_> = (0..17).map(|i| Vec3::Z * f64::from(i)).collect();
        assert!(matches!(
            build(Primitive::Pipe, [8.0; 3], [16, 16, 1], [0.0; 2], &path),
            Err(SolidError::InvalidInput {
                reason: "pipe cage exceeds the editable face budget",
                ..
            })
        ));
        let valid = build(Primitive::Pipe, [8.0; 3], [16, 13, 1], [0.0; 2], &path).unwrap();
        assert_eq!(valid.faces.len(), 2 * 32 * 32 + 4 * 32 * 13 * 16);
    }

    #[test]
    fn a_box_keeps_stable_oriented_faces_and_quadball_projects_to_its_radius() {
        let dims = [8.0, 10.0, 12.0];
        let box_cage = build(Primitive::Box, dims, [1; 3], [0.0; 2], &[]).unwrap();
        assert_eq!(box_cage.points.len(), 8);
        assert_eq!(box_cage.faces.len(), 6);
        assert_eq!(box_cage.faces[0], [0, 1, 2, 3]);
        assert_eq!(box_cage.points[0], Vec3::new(-4.0, -5.0, 6.0));
        assert!(build(Primitive::Sphere, dims, [usize::MAX; 3], [0.0; 2], &[]).is_err());
        let round = build(Primitive::Quadball, dims, [3; 3], [0.0; 2], &[]).unwrap();
        assert!(round
            .points
            .iter()
            .all(|point| (point.length() - 4.0).abs() < 1e-12));
    }

    #[test]
    fn flat_quad_control_offsets_follow_their_oriented_normal() {
        let points = [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y];
        assert_eq!(
            offset_controls(&points, &[vec![0, 1, 2, 3]], -0.25).unwrap(),
            points.map(|point| point - Vec3::Z * 0.25)
        );
        assert!(offset_controls(&points, &[vec![0, 1, 2, 4]], 0.25).is_err());
    }

    #[test]
    fn ruled_strip_keeps_exact_rational_rows_and_rejects_a_parallel_move() {
        let seam = [[0.0, 0.0, 0.0, 1.0], [4.0, 0.0, 0.0, 2.0]];
        assert_eq!(
            ruled_strip(&seam, Vec3::Z).unwrap(),
            [seam[0], seam[1], [0.0, 0.0, 1.0, 1.0], [4.0, 0.0, 2.0, 2.0]]
        );
        assert!(ruled_strip(&seam, Vec3::X).is_err());
    }
}
