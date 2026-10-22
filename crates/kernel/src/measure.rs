use serde::Serialize;
use solidcraft_geom::{Aabb3, Vec3};

use crate::Result;
use crate::body::Body;
use crate::topo::{TopoCounts, merged_topology};

/// Measured properties of a body.
#[derive(Clone, Debug, Serialize)]
pub struct BodyMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
    pub bbox: Aabb3,
    /// Raw kernel topology counts (closed curves and surfaces may be split in pieces).
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub shells: usize,
    /// Counts after merging split pieces of the same analytic surface/curve (comparable with
    /// systems that use periodic faces and edges).
    pub merged: TopoCounts,
    /// What volume, area and centre come from: `smlib` (exact, integrated on the surfaces) or
    /// `mesh` (a mesh body's own triangles).
    pub measured_by: &'static str,
    /// Anything the measurement says about itself.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Volume, area, centre and box of a body, as [`measure`] works them out, without the topology
/// counts.
#[derive(Clone, Debug, Serialize)]
pub struct MassMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
    pub bbox: Aabb3,
    /// As [`BodyMeasure::measured_by`].
    pub measured_by: &'static str,
    /// As [`BodyMeasure::notes`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// The relative accuracy SMLib integrates at: the finest of SmApiBrepComputeMassProperties'
/// documented range ("clamped to [1e-4, 1e-1]", SmApiQueries.h). Its cost is "governed by
/// dRelativeAccuracy".
const ACCURACY: f64 = 1e-4;

/// Measure a body. A solid body's volume, area and centre are SMLib's, integrated on its exact
/// surfaces, and its box is SMLib's exact box; a body SMLib cannot measure is an error. A mesh
/// body is measured from its triangles, exactly.
pub fn measure(b: &Body) -> Result<BodyMeasure> {
    if let Some((pos, tris)) = b.triangle_mesh() {
        // Exact for a mesh body; topology counts are facets, sides and vertices.
        let m = b.mesh(&crate::MeshTolerance::chord(1.0))?;
        let mm = m.measure();
        let edges = b.mesh.as_ref().map(|m| m.edge_count()).unwrap_or(0);
        let merged =
            TopoCounts { faces: tris.len(), edges, vertices: pos.len(), face_types: [("mesh".to_string(), tris.len())].into_iter().collect() };
        return Ok(BodyMeasure {
            volume: mm.volume,
            area: mm.area,
            centroid: mm.centroid,
            bbox: m.bounds(),
            faces: tris.len(),
            edges,
            vertices: pos.len(),
            shells: 1,
            merged,
            measured_by: "mesh",
            notes: Vec::new(),
        });
    }
    let m = measure_mass(b)?;
    let topo = b.mesh(&crate::MeshTolerance::chord((b.size() * 1e-3).max(1e-3)))?;
    Ok(BodyMeasure {
        volume: m.volume,
        area: m.area,
        centroid: m.centroid,
        bbox: m.bbox,
        faces: b.face_count(),
        edges: b.edge_count(),
        vertices: b.vertex_count(),
        shells: b.shell_count(),
        merged: merged_topology(b, &topo)?,
        measured_by: m.measured_by,
        notes: m.notes,
    })
}

/// [`measure`]'s volume, area, centre and box alone (for callers that need no topology counts,
/// such as an interference check).
pub fn measure_mass(b: &Body) -> Result<MassMeasure> {
    if b.triangle_mesh().is_some() {
        let m = measure(b)?;
        return Ok(MassMeasure { volume: m.volume, area: m.area, centroid: m.centroid, bbox: m.bbox, measured_by: m.measured_by, notes: m.notes });
    }
    let m = crate::guard("SMLib mass properties", || {
        b.smlib()?.mass_properties(ACCURACY).map_err(|e| crate::KernelError::Failed(format!("SMLib mass properties: {e}")))
    })?;
    let [x, y, z] = m.centroid;
    Ok(MassMeasure { volume: m.volume, area: m.area, centroid: Vec3::new(x, y, z), bbox: b.exact_bounds()?, measured_by: "smlib", notes: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoolOp, boolean, cylinder, torus};

    /// The mass properties alone are [`measure`]'s, to the bit; and on a curved intersection
    /// they are the textbook ones.
    #[test]
    fn mass_alone_is_the_full_measure() {
        let ring = torus(Vec3::ZERO, 10.0, 2.0).unwrap();
        let post = cylinder(Vec3::new(10.0, 0.0, -5.0), Vec3::Z, 1.5, 10.0).unwrap();
        let shared = boolean(&ring, &post, BoolOp::Intersect).unwrap().unwrap();
        for b in [&ring, &shared] {
            let (full, mass) = (measure(b).unwrap(), measure_mass(b).unwrap());
            assert_eq!(full.volume.to_bits(), mass.volume.to_bits());
            assert_eq!(full.area.to_bits(), mass.area.to_bits());
            assert_eq!(full.centroid, mass.centroid);
            assert_eq!((full.bbox.min, full.bbox.max), (mass.bbox.min, mass.bbox.max));
        }
        let want = 2.0 * std::f64::consts::PI.powi(2) * 10.0 * 4.0;
        let got = measure_mass(&ring).unwrap().volume;
        assert!((got - want).abs() < 1e-3 * want, "{got} against {want}");
        assert!(measure(&ring).unwrap().merged.faces > 0);
    }

    /// A 10 mm cube at `at`, each side in n × n squares of two triangles, one face per side;
    /// without its first `open` triangles.
    fn cube(at: Vec3, n: usize, open: usize) -> Mesh {
        let mut m = Mesh::default();
        let s = 10.0;
        for axis in 0..3 {
            for side in [0.0, 1.0] {
                for i in 0..n {
                    for j in 0..n {
                        let corner = |a: usize, b: usize| {
                            let mut c = [0.0; 3];
                            c[axis] = side * s;
                            c[(axis + 1) % 3] = a as f64 * s / n as f64;
                            c[(axis + 2) % 3] = b as f64 * s / n as f64;
                            at + Vec3::new(c[0], c[1], c[2])
                        };
                        let k = m.positions.len() as u32;
                        m.positions.extend([corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1)]);
                        m.triangles.extend(if side > 0.0 { [[k, k + 1, k + 2], [k, k + 2, k + 3]] } else { [[k, k + 2, k + 1], [k, k + 3, k + 2]] });
                        m.tri_face.extend([(2 * axis) as u32 + side as u32; 2]);
                    }
                }
            }
        }
        m.triangles.drain(..open);
        m.tri_face.drain(..open);
        m
    }

    /// The summed per-face sums of `m` about `o`, with the moments about the origin (as
    /// `measure` passes them on).
    fn sums(m: &Mesh, o: Vec3) -> (f64, f64, Vec3, Vec3) {
        let (area, v6, mom, normal) =
            face_sums(m, 6, o).iter().fold((0.0, 0.0, Vec3::ZERO, Vec3::ZERO), |s, f| (s.0 + f.0, s.1 + f.1, s.2 + f.2, s.3 + f.3));
        (area, v6, o * (4.0 * v6) + mom, normal)
    }

    /// A cube 1000 mm from the origin with one 0.5 mm² facet left out: about the box's centre
    /// the sums miss a cone of that facet to the centre (0.08 %), where about the origin they are
    /// 17 % over. Closed, it measures the same about either.
    #[test]
    fn a_mesh_left_open_is_measured_about_its_centre() {
        let at = Vec3::new(1000.0, 1000.0, 1000.0);
        let (closed, open) = (cube(at, 10, 0), cube(at, 10, 1));
        let centre = at + Vec3::new(5.0, 5.0, 5.0);
        for o in [Vec3::ZERO, centre] {
            let (area, v6, mom, normal) = sums(&closed, o);
            assert!((area - 600.0).abs() < 1e-9 && (v6 / 6.0 - 1000.0).abs() < 1e-6 && normal.len() < 1e-9);
            assert!((mom / (4.0 * v6) - centre).len() < 1e-6 * at.len(), "{o:?}");
        }
        let (_, v6_origin, _, normal) = sums(&open, Vec3::ZERO);
        assert!((normal.len() - 0.5).abs() < 1e-9);
        // The facet faces the origin: leaving it out adds its cone to the origin, 167 mm³.
        assert!((v6_origin / 6.0 - (1000.0 + 0.5 * 1000.0 / 3.0)).abs() < 1e-3, "{}", v6_origin / 6.0);
        let (_, v6, mom, _) = sums(&open, centre);
        assert!((v6 / 6.0 - (1000.0 - 0.5 * 5.0 / 3.0)).abs() < 1e-6, "{}", v6 / 6.0);
        assert!((mom / (4.0 * v6) - centre).len() < 0.01, "{:?}", mom / (4.0 * v6));
    }

    // A body's measured volume does not depend on where it sits: the same revolved ring of cones
    // and cylinders, moved far from the origin, measures what it measures at the origin.
    use crate::{revolve, transform};
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    use std::f64::consts::{PI, TAU};

    /// Ring cross-section in (radius, height): cones top and bottom, cylinders inside and out.
    const PROFILE: [(f64, f64); 6] = [(5.0, 0.0), (9.0, 0.0), (11.0, 2.0), (11.0, 6.0), (8.0, 8.0), (5.0, 8.0)];

    /// Pappus: 2π × the section's first moment about the axis.
    fn ring_volume() -> f64 {
        let mut moment6 = 0.0;
        for (i, &(x0, y0)) in PROFILE.iter().enumerate() {
            let (x1, y1) = PROFILE[(i + 1) % PROFILE.len()];
            moment6 += (x0 + x1) * (x0 * y1 - x1 * y0);
        }
        TAU * moment6 / 6.0
    }

    fn revolved(plane: &Plane, pts: &[Vec2]) -> Body {
        let region = Region2 { outer: Loop2::polygon(pts), holes: vec![] };
        revolve(plane, &[region], Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap()
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs()
    }

    #[test]
    fn ring_volume_does_not_depend_on_placement() {
        let pts: Vec<Vec2> = PROFILE.iter().map(|&(r, z)| Vec2::new(r, z)).collect();
        let base = revolved(&Plane::XZ, &pts);
        let at_origin = measure(&base).unwrap();
        assert!(rel(at_origin.volume, ring_volume()) < 1e-4, "{} vs {}", at_origin.volume, ring_volume());
        for d in [100.0, 1000.0, 10000.0] {
            for axis in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(1.0, 1.0, 1.0)] {
                let m = measure(&transform(&base, axis * d, Vec3::ZERO, Vec3::Z, 0.0).unwrap()).unwrap();
                assert!(rel(m.volume, at_origin.volume) < 1e-6, "moved {d} along {axis:?}: {} vs {}", m.volume, at_origin.volume);
                let off = m.centroid - at_origin.centroid - axis * d;
                assert!(off.len() < 1e-6 * d, "centroid moved {d} along {axis:?}: off by {off:?}");
            }
        }
    }

    #[test]
    fn cylinder_and_cone_volumes_far_from_the_origin() {
        let (r, h) = (7.0, 12.0);
        // A frustum, radii 7 and 3, height 10: πh/3 (R² + Rr + r²).
        let frustum = [Vec2::new(0.0, 0.0), Vec2::new(7.0, 0.0), Vec2::new(3.0, 10.0), Vec2::new(0.0, 10.0)];
        let cone_volume = PI * 10.0 / 3.0 * (49.0 + 21.0 + 9.0);
        for at in [Vec3::ZERO, Vec3::new(5000.0, -3000.0, 8000.0)] {
            let cyl = measure(&cylinder(at, Vec3::Z, r, h).unwrap()).unwrap();
            assert!(rel(cyl.volume, PI * r * r * h) < 5e-4, "cylinder at {at:?}: {}", cyl.volume);
            let cone = measure(&revolved(&Plane { origin: at, ..Plane::XZ }, &frustum)).unwrap();
            assert!(rel(cone.volume, cone_volume) < 5e-4, "frustum at {at:?}: {} vs {cone_volume}", cone.volume);
        }
    }
}
