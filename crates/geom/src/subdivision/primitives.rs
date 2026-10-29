//! Parametric all-quad layouts for the Forms primitive families.
use super::{Cage, Result, bad};
use crate::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Parameters {
    pub kind: String,
    pub dimensions: [f64; 3],
    pub segments: [usize; 3],
    /// Torus major/minor radii, or tube outer/inner radii.
    pub radii: [f64; 2],
    pub path: Vec<Vec3>,
}
impl Default for Parameters {
    fn default() -> Self {
        Self { kind: "box".into(), dimensions: [40.0; 3], segments: [2; 3], radii: [20.0, 7.0], path: Vec::new() }
    }
}
impl Parameters {
    pub fn validate(&self) -> Result<()> {
        if self.dimensions.iter().any(|v| !v.is_finite() || !(0.1..=10_000.0).contains(v)) || self.segments.iter().any(|n| !(1..=16).contains(n)) {
            return Err(bad("primitive dimensions must be 0.1..10000 mm and segments 1..16"));
        }
        if self.radii.iter().any(|r| !r.is_finite() || !(0.0..=10_000.0).contains(r))
            || self.path.len() > 128
            || self.path.iter().any(|p| !p.is_finite() || p.len() > 100_000.0)
        {
            return Err(bad("primitive metadata must contain finite bounded radii and path points"));
        }
        if !matches!(self.kind.as_str(), "box" | "plane" | "face" | "quadball" | "cylinder" | "sphere" | "torus" | "tube" | "pipe") {
            return Err(bad("unknown Forms primitive"));
        }
        Ok(())
    }
    pub fn build(&self) -> Result<Cage> {
        self.validate()?;
        let radius = self.dimensions[0] / 2.0;
        let cage = match self.kind.as_str() {
            "box" => box_grid(self.dimensions, self.segments, false)?,
            "plane" | "face" => box_grid(self.dimensions, self.segments, true)?,
            "quadball" => {
                let mut c = box_grid([2.0; 3], [self.segments[0]; 3], false)?;
                for p in &mut c.points {
                    let (x, y, z) = (p.x, p.y, p.z);
                    let (xx, yy, zz) = (x * x, y * y, z * z);
                    *p = Vec3::new(
                        x * (1.0 - yy / 2.0 - zz / 2.0 + yy * zz / 3.0).max(0.0).sqrt(),
                        y * (1.0 - zz / 2.0 - xx / 2.0 + zz * xx / 3.0).max(0.0).sqrt(),
                        z * (1.0 - xx / 2.0 - yy / 2.0 + xx * yy / 3.0).max(0.0).sqrt(),
                    ) * radius;
                }
                c
            }
            "cylinder" => cylinder(radius, self.dimensions[2], self.segments[0], self.segments[1])?,
            "sphere" => sphere(radius, self.segments[0], self.segments[1])?,
            "torus" => torus(self.radii[0], self.radii[1], self.segments[0], self.segments[1])?,
            "tube" => tube(self.radii[0], self.radii[1], self.dimensions[2], self.segments[0], self.segments[1])?,
            "pipe" => pipe(radius, &self.path, self.segments[0], self.segments[1], self.dimensions[2])?,
            _ => return Err(bad("unknown Forms primitive")),
        };
        cage.validate()?;
        Ok(cage)
    }
}

fn box_grid(d: [f64; 3], n: [usize; 3], plane: bool) -> Result<Cage> {
    let [nx, ny, nz] = n.map(|v| v as i32);
    let frames = if plane {
        vec![([0, 0, 0], [1, 0, 0], [0, 1, 0], nx, ny)]
    } else {
        vec![
            ([0, 0, nz], [1, 0, 0], [0, 1, 0], nx, ny),
            ([0, ny, nz], [1, 0, 0], [0, 0, -1], nx, nz),
            ([nx, 0, 0], [0, 1, 0], [0, 0, 1], ny, nz),
            ([0, 0, 0], [0, 1, 0], [1, 0, 0], ny, nx),
            ([0, 0, 0], [1, 0, 0], [0, 0, 1], nx, nz),
            ([0, 0, 0], [0, 0, 1], [0, 1, 0], nz, ny),
        ]
    };
    let mut points = Vec::new();
    let mut faces = Vec::new();
    let mut ids = BTreeMap::new();
    for (o, u, v, ns, nt) in frames {
        for t in 0..nt {
            for s in 0..ns {
                let mut face = [0; 4];
                for (slot, (a, b)) in [(s, t), (s + 1, t), (s + 1, t + 1), (s, t + 1)].into_iter().enumerate() {
                    let k = [o[0] + a * u[0] + b * v[0], o[1] + a * u[1] + b * v[1], o[2] + a * u[2] + b * v[2]];
                    let id = *ids.entry(k).or_insert_with(|| {
                        let id = points.len();
                        points.push(Vec3::new(
                            (k[0] as f64 / nx as f64 - 0.5) * d[0],
                            (k[1] as f64 / ny as f64 - 0.5) * d[1],
                            if plane { 0.0 } else { (k[2] as f64 / nz as f64 - 0.5) * d[2] },
                        ));
                        id
                    });
                    face[slot] = id;
                }
                faces.push(face.to_vec());
            }
        }
    }
    Ok(Cage { points, faces, ..Default::default() })
}
fn cylinder(radius: f64, height: f64, sides: usize, layers: usize) -> Result<Cage> {
    let mut c = box_grid([2.0 * radius, 2.0 * radius, height], [2 * sides, 2 * sides, layers], false)?;
    for p in &mut c.points {
        let distance = p.x.abs().max(p.y.abs());
        if distance > radius * 1e-12 {
            let (u, v) = (p.x / distance, p.y / distance);
            let angle = if u.abs() >= v.abs() {
                if u >= 0.0 { PI / 4.0 * v } else { PI - PI / 4.0 * v }
            } else if v >= 0.0 {
                PI / 2.0 - PI / 4.0 * u
            } else {
                -PI / 2.0 + PI / 4.0 * u
            };
            p.x = distance * angle.cos();
            p.y = distance * angle.sin();
        }
    }
    Ok(c)
}
fn sphere(radius: f64, longitude: usize, latitude: usize) -> Result<Cage> {
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
    let at = |r: usize, i: usize| 1 + r * n + i % n;
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
        faces.push([south, at(rings - 1, i + 2), at(rings - 1, i + 1), at(rings - 1, i)].to_vec());
    }
    Ok(Cage { points, faces, ..Default::default() })
}
fn torus(major: f64, minor: f64, u: usize, v: usize) -> Result<Cage> {
    if !major.is_finite() || !minor.is_finite() || minor <= 0.0 || major <= minor || major + minor > 10_000.0 {
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
    let at = |i: usize, j: usize| (i % n) * m + j % m;
    let mut faces = Vec::with_capacity(n * m);
    for i in 0..n {
        for j in 0..m {
            faces.push([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)].to_vec());
        }
    }
    Ok(Cage { points, faces, ..Default::default() })
}
fn tube(outer: f64, inner: f64, height: f64, sides: usize, layers: usize) -> Result<Cage> {
    if !outer.is_finite() || !inner.is_finite() || inner <= 0.0 || outer <= inner || outer > 10_000.0 {
        return Err(bad("tube requires 0 < inner radius < outer radius"));
    }
    let n = 4 * sides;
    let wall = (layers + 1) * n;
    let mut points = Vec::with_capacity(2 * wall);
    for radius in [outer, inner] {
        for j in 0..=layers {
            for i in 0..n {
                let a = TAU * i as f64 / n as f64;
                points.push(Vec3::new(radius * a.cos(), radius * a.sin(), height * (j as f64 / layers as f64 - 0.5)));
            }
        }
    }
    let at = |inside: bool, j: usize, i: usize| usize::from(inside) * wall + j * n + i % n;
    let mut faces = Vec::new();
    for j in 0..layers {
        for i in 0..n {
            faces.push([at(false, j, i), at(false, j, i + 1), at(false, j + 1, i + 1), at(false, j + 1, i)].to_vec());
            faces.push([at(true, j, i), at(true, j + 1, i), at(true, j + 1, i + 1), at(true, j, i + 1)].to_vec());
        }
    }
    for i in 0..n {
        faces.push([at(false, layers, i), at(false, layers, i + 1), at(true, layers, i + 1), at(true, layers, i)].to_vec());
        faces.push([at(false, 0, i + 1), at(false, 0, i), at(true, 0, i), at(true, 0, i + 1)].to_vec());
    }
    Ok(Cage { points, faces, ..Default::default() })
}
fn pipe(radius: f64, input: &[Vec3], sides: usize, samples: usize, height: f64) -> Result<Cage> {
    let default = [Vec3::new(0., 0., -height / 2.), Vec3::ZERO, Vec3::new(height / 3., 0., height / 2.)];
    let path = if input.is_empty() { &default[..] } else { input };
    if !(2..=128).contains(&path.len()) || path.iter().any(|p| !p.is_finite() || p.len() > 100_000.0) {
        return Err(bad("pipe requires 2..128 finite path points"));
    }
    let mut directions = Vec::new();
    let mut lengths = Vec::new();
    let mut total = 0.0;
    for edge in path.windows(2) {
        let delta = edge[1] - edge[0];
        let len = delta.len();
        let direction = delta.normalized().ok_or_else(|| bad("pipe path has coincident points"))?;
        directions.push(direction);
        lengths.push(len);
        total += len;
    }
    let layers = samples * (path.len() - 1);
    if layers > 256 {
        return Err(bad("pipe path sample budget exceeded"));
    }
    let mut frames = Vec::new();
    let mut u = directions.first().copied().ok_or_else(|| bad("empty pipe path"))?.any_perp().normalized().ok_or_else(|| bad("pipe frame"))?;
    for i in 0..path.len() {
        let tangent = match (i.checked_sub(1).and_then(|j| directions.get(j)), directions.get(i)) {
            (Some(a), Some(b)) => (*a + *b).normalized().ok_or_else(|| bad("pipe path reverses direction"))?,
            (Some(a), None) => *a,
            (None, Some(b)) => *b,
            _ => return Err(bad("pipe frame")),
        };
        u = (u - tangent * u.dot(tangent)).normalized().unwrap_or_else(|| tangent.any_perp().normalized().unwrap_or(Vec3::X));
        frames.push((u, tangent.cross(u)));
    }
    let mut cage = cylinder(radius, total, sides, layers)?;
    for p in &mut cage.points {
        let mut distance = p.z + total / 2.0;
        let mut segment = 0;
        while segment + 1 < lengths.len() && distance > lengths.get(segment).copied().unwrap_or(0.0) {
            distance -= lengths.get(segment).copied().unwrap_or(0.0);
            segment += 1;
        }
        let length = lengths.get(segment).copied().ok_or_else(|| bad("pipe segment"))?;
        let t = (distance / length).clamp(0.0, 1.0);
        let a = *path.get(segment).ok_or_else(|| bad("pipe segment"))?;
        let b = *path.get(segment + 1).ok_or_else(|| bad("pipe segment"))?;
        let (u0, v0) = *frames.get(segment).ok_or_else(|| bad("pipe frame"))?;
        let (u1, v1) = *frames.get(segment + 1).ok_or_else(|| bad("pipe frame"))?;
        let up = (u0 * (1.0 - t) + u1 * t).normalized().ok_or_else(|| bad("pipe frame flips"))?;
        let vp = (v0 * (1.0 - t) + v1 * t).normalized().ok_or_else(|| bad("pipe frame flips"))?;
        *p = a * (1.0 - t) + b * t + up * p.x + vp * p.y;
    }
    Ok(cage)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_pipe_junctions_share_quad_rims_for_three_to_six_branches_and_overrides() {
        use super::super::pipe_network::{Network, PathEdge, PipeSettings, SegmentOverride};
        let axes = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        for directions in [
            vec![Vec3::X, Vec3::new(-0.5, 0.866025403784, 0.0), Vec3::new(-0.5, -0.866025403784, 0.0)],
            vec![Vec3::X, -Vec3::X, Vec3::Y],
            vec![Vec3::X, Vec3::Y, Vec3::Z],
            axes[..4].to_vec(),
            axes[..5].to_vec(),
            axes.to_vec(),
        ] {
            let network = Network::new(
                directions.iter().enumerate().map(|(i, &d)| PathEdge { key: format!("edge{i}"), points: vec![Vec3::ZERO, d * 20.0] }).collect(),
            )
            .unwrap();
            let mut settings = PipeSettings { diameter: 2.0, ..Default::default() };
            settings.overrides.insert(network.chains()[0].key.clone(), SegmentOverride { diameter: Some(3.0), samples: Some(5) });
            let built = network.build(&settings).unwrap();
            assert!(built.cage.faces.iter().all(|f| f.len() == 4));
            assert!(built.cage.boundary_loops().unwrap().is_empty());
            assert_eq!(built.cage.points.len() as i64 - built.cage.edges().unwrap().len() as i64 + built.cage.faces.len() as i64, 2);
            assert_eq!(built.segments[0].diameter, 3.0);
            assert_eq!(built.segments[0].samples, 5);
            assert_eq!(built.segments.len(), directions.len());
            assert!(built.cage.mesh(1).unwrap().normals.iter().all(|p| p.is_finite()));
        }
        let short = Network::new(vec![
            PathEdge { key: "a".into(), points: vec![Vec3::ZERO, Vec3::X] },
            PathEdge { key: "b".into(), points: vec![Vec3::ZERO, Vec3::Y] },
            PathEdge { key: "c".into(), points: vec![Vec3::ZERO, Vec3::Z] },
        ])
        .unwrap();
        assert!(short.build(&PipeSettings::default()).is_err());
        assert!(short.build(&PipeSettings { diameter: f64::INFINITY, ..Default::default() }).is_err());
    }
    #[test]
    fn network_pipe_chains_keep_source_identity_and_distribute_intervals_by_length() {
        use super::super::pipe_network::{Network, PathEdge};
        let edge = |key: &str, a: Vec3, b: Vec3| PathEdge { key: key.into(), points: vec![a, b] };
        let source = vec![edge("a", Vec3::ZERO, Vec3::X), edge("b", Vec3::X, Vec3::X * 10.0)];
        let network = Network::new(source.clone()).unwrap();
        assert_eq!(network.chains().len(), 1);
        assert!(!network.chains()[0].closed);
        let points = network.sample(0, 3).unwrap();
        assert_eq!(points.len(), 7);
        assert_eq!(points[0], Vec3::ZERO);
        assert_eq!(points[1], Vec3::X);
        assert_eq!(points[6], Vec3::X * 10.0);
        let uneven =
            Network::new(vec![edge("a", Vec3::ZERO, Vec3::X), edge("b", Vec3::X, Vec3::X * 11.0), edge("c", Vec3::X * 11.0, Vec3::X * 111.0)])
                .unwrap();
        let samples = uneven.sample(0, 3).unwrap();
        assert_eq!(samples.len(), 10);
        assert_eq!(samples[2], Vec3::X * 11.0, "short edges retain one interval each while the long edge receives seven");
        let moved = source
            .into_iter()
            .rev()
            .map(|mut e| {
                for p in &mut e.points {
                    *p += Vec3::Y;
                }
                e
            })
            .collect();
        assert_eq!(Network::new(moved).unwrap().chains()[0].key, network.chains()[0].key);
        let branch =
            Network::new(vec![edge("a", Vec3::ZERO, Vec3::X * 10.0), edge("b", Vec3::ZERO, Vec3::Y * 10.0), edge("c", Vec3::ZERO, Vec3::Z * 10.0)])
                .unwrap();
        assert_eq!(branch.chains().len(), 3);
        assert!(branch.degree().contains(&3));
        let circle = (0..=16)
            .map(|i| {
                let a = TAU * i as f64 / 16.0;
                Vec3::new(a.cos(), a.sin(), 0.0)
            })
            .collect();
        let closed = Network::new(vec![PathEdge { key: "circle".into(), points: circle }]).unwrap();
        assert!(closed.chains()[0].closed);
        assert_eq!(closed.sample(0, 1).unwrap().len(), 4);
        assert!(network.sample(usize::MAX, 3).is_err());
        assert!(network.sample(0, usize::MAX).is_err());
        assert!(Network::new(vec![]).is_err());
        assert!(Network::new(vec![edge("a", Vec3::ZERO, Vec3::X), edge("a", Vec3::Y, Vec3::Z)]).is_err());
    }
    #[test]
    fn network_pipe_sweeps_have_quad_caps_and_closed_frames_on_nonplanar_paths() {
        use super::super::pipe_network::swept_path;
        let open = [Vec3::ZERO, Vec3::new(0.0, 0.0, 5.0), Vec3::new(3.0, 0.0, 10.0)];
        for density in [1, 2, 3] {
            let cage = swept_path(&open, 0.8, density, false, [true; 2]).unwrap();
            assert!(cage.boundary_loops().unwrap().is_empty());
            assert!(cage.faces.iter().all(|face| face.len() == 4));
            assert_eq!(cage.faces.len(), 16 * density + 8 * density * density);
            let mesh = cage.mesh(1).unwrap();
            let volume: f64 = mesh
                .triangles
                .iter()
                .map(|t| {
                    let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
                    a.dot(b.cross(c)) / 6.0
                })
                .sum();
            assert!(volume > 0.1);
            let open_end = swept_path(&open, 0.8, density, false, [false, true]).unwrap();
            assert_eq!(open_end.boundary_loops().unwrap()[0].len(), 8 * density);
        }
        let mut loop_points: Vec<_> = (0..32)
            .map(|i| {
                let a = TAU * i as f64 / 32.0;
                Vec3::new(8.0 * a.cos(), 8.0 * a.sin(), (2.0 * a).sin())
            })
            .collect();
        loop_points.push(loop_points[0]);
        let cage = swept_path(&loop_points, 0.5, 2, true, [false; 2]).unwrap();
        assert!(cage.boundary_loops().unwrap().is_empty());
        assert_eq!(cage.faces.len(), 32 * 16);
        assert!(cage.mesh(1).unwrap().normals.iter().all(|p| p.is_finite()));
        assert!(swept_path(&open, f64::NAN, 1, false, [true; 2]).is_err());
        assert!(swept_path(&[Vec3::ZERO, Vec3::X, Vec3::ZERO], 0.5, 1, false, [true; 2]).is_err());
        assert!(swept_path(&open, 0.5, usize::MAX, false, [true; 2]).is_err());
        assert!(swept_path(&[Vec3::ZERO, Vec3::X, Vec3::ZERO], 0.5, 1, true, [false; 2]).is_err());
    }
    #[test]
    fn dense_cylinder_caps_stay_inside_the_radius_and_perimeter_angles_are_uniform() {
        for sides in [1, 2, 4, 8] {
            let c = cylinder(3.0, 5.0, sides, 3).unwrap();
            assert!(c.points.iter().all(|p| p.x.hypot(p.y) <= 3.0 + 1e-10));
            let mut angles: Vec<_> = c
                .points
                .iter()
                .filter(|p| (p.z - 2.5).abs() < 1e-10 && (p.x.hypot(p.y) - 3.0).abs() < 1e-10)
                .map(|p| p.y.atan2(p.x).rem_euclid(TAU))
                .collect();
            angles.sort_by(f64::total_cmp);
            assert_eq!(angles.len(), 8 * sides);
            let expected = TAU / angles.len() as f64;
            for (a, b) in angles.iter().zip(angles.iter().cycle().skip(1)).take(angles.len()) {
                assert!(((b - a).rem_euclid(TAU) - expected).abs() < 1e-10);
            }
        }
    }
    #[test]
    fn primitive_topology_and_orientation() {
        for kind in ["box", "plane", "face", "quadball", "sphere", "cylinder", "torus", "tube", "pipe"] {
            let p = Parameters { kind: kind.into(), segments: [1; 3], ..Default::default() };
            let c = p.build().unwrap();
            c.validate().unwrap();
            let m = c.mesh(2).unwrap();
            assert!(!m.triangles.is_empty());
            assert!(m.positions.iter().all(|p| p.is_finite()));
            if !matches!(kind, "plane" | "face" | "pipe") {
                let volume: f64 = m
                    .triangles
                    .iter()
                    .map(|t| {
                        let [a, b, c] = t.map(|v| m.positions[v as usize]);
                        a.dot(b.cross(c)) / 6.0
                    })
                    .sum();
                assert!(volume > 0.0, "{kind}: {volume}");
            }
        }
    }
    #[test]
    fn independent_box_dimensions_and_caps() {
        let c = Parameters { dimensions: [10., 20., 30.], segments: [2, 3, 4], ..Default::default() }.build().unwrap();
        assert_eq!(c.faces.len(), 2 * (2 * 3 + 3 * 4 + 4 * 2));
        let b: crate::Aabb3 = c.points.iter().copied().collect();
        assert_eq!(b.size(), Vec3::new(10., 20., 30.));
        assert!(Parameters { kind: "tube".into(), radii: [5., 8.], ..Default::default() }.build().is_err());
        assert!(Parameters { radii: [f64::NAN, 1.0], ..Default::default() }.build().is_err());
        assert!(Parameters { path: vec![Vec3::new(f64::INFINITY, 0., 0.)], ..Default::default() }.build().is_err());
    }
}
