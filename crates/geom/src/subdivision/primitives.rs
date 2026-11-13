//! Parametric all-quad layouts for the Forms primitive families.
use super::{Cage, Result, bad};
#[cfg(test)]
use crate::prelude::*;
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
#[cfg(test)]
use std::f64::consts::TAU;

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
    fn family(&self) -> Result<solidcore_ops::form_cage::Primitive> {
        use solidcore_ops::form_cage::Primitive;
        Ok(match self.kind.as_str() {
            "box" => Primitive::Box,
            "plane" | "face" => Primitive::Plane,
            "quadball" => Primitive::Quadball,
            "cylinder" => Primitive::Cylinder,
            "sphere" => Primitive::Sphere,
            "torus" => Primitive::Torus,
            "tube" => Primitive::Tube,
            "pipe" => Primitive::Pipe,
            _ => return Err(bad("unknown Forms primitive")),
        })
    }
    pub fn validate(&self) -> Result<()> {
        solidcore_ops::form_cage::validate(self.dimensions, self.segments, self.radii, &self.path).map_err(|error| bad(&error.to_string()))?;
        self.family()?;
        Ok(())
    }
    pub fn build(&self) -> Result<Cage> {
        let solidcore_ops::form_cage::CageGeometry { points, faces } =
            solidcore_ops::form_cage::build(self.family()?, self.dimensions, self.segments, self.radii, &self.path)
                .map_err(|error| bad(&error.to_string()))?;
        let cage = Cage { points, faces, ..Default::default() };
        cage.validate()?;
        Ok(cage)
    }
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
            let c =
                Parameters { kind: "cylinder".into(), dimensions: [6.0, 6.0, 5.0], segments: [sides, 3, 1], ..Default::default() }.build().unwrap();
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
