use serde::Serialize;
use solidcraft_geom::{Aabb3, Mesh, Vec3};

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
}

/// Per-face sums: area, 6 × signed volume and 24 × first moments (divergence theorem), taken
/// about `o` (the moments relative to it), and the sum of the area vectors (zero over a closed
/// surface). Over a closed surface the volume does not depend on `o`; over one that leaves a
/// gap, the gap is closed by a cone to `o`, so `o` near the body keeps that cone small.
fn face_sums(m: &Mesh, nf: usize, o: Vec3) -> Vec<(f64, f64, Vec3, Vec3)> {
    let mut out = vec![(0.0, 0.0, Vec3::ZERO, Vec3::ZERO); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let (Some([a, b, c]), Some(slot)) = (m.tri(t), out.get_mut(*f as usize)) else { continue };
        let n = (b - a).cross(c - a) * 0.5;
        let (a, b, c) = (a - o, b - o, c - o);
        let d = a.dot(b.cross(c));
        slot.0 += n.len();
        slot.1 += d;
        slot.2 += (a + b + c) * d;
        slot.3 += n;
    }
    out
}

/// Smallest radius of curvature along the mesh's edge polylines (straight edges ignored).
fn min_edge_radius(m: &Mesh) -> Option<f64> {
    let mut best: Option<f64> = None;
    for e in &m.edges {
        for w in e.windows(3) {
            let (Some(&a), Some(&b), Some(&c)) = (w.first(), w.get(1), w.get(2)) else { continue };
            let (ab, bc, ca) = (b - a, c - b, a - c);
            let cross = ab.cross(bc).len();
            if !(cross > 1e-12 * ab.len() * bc.len()) {
                continue;
            }
            let r = ab.len() * bc.len() * ca.len() / (2.0 * cross);
            if r.is_finite() && r > 0.0 && best.is_none_or(|x| r < x) {
                best = Some(r);
            }
        }
    }
    best
}

/// Volume, area, centre and box of a body, as [`measure`] works them out, without the topology
/// counts (and the tessellation they take).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct MassMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
    pub bbox: Aabb3,
}

/// The volume a mesh encloses, taken about `o` (a point in or near the body): a mesh's cracks (its
/// faces' edges sampled apart) count for their distance from that point, so a small part far out
/// would measure them many times over about the world's origin.
pub(crate) fn mesh_volume_about(m: &Mesh, o: Vec3) -> f64 {
    let o = if o.is_finite() { o } else { Vec3::ZERO };
    m.triangles.iter().filter_map(|t| m.tri(t)).map(|[a, b, c]| (a - o).dot((b - o).cross(c - o))).sum::<f64>() / 6.0
}

/// Measure a body from a fine tessellation (chord tolerance 5e-5 of the body size, finer for
/// small radii down to 5e-6). Faces whose fine tessellation disagrees with a coarse one (a
/// meshing failure) use the coarse result.
pub fn measure(b: &Body) -> Result<BodyMeasure> {
    if let Some((pos, tris)) = b.triangle_mesh() {
        // Exact for a mesh body; topology counts are facets, sides and vertices.
        let m = b.tessellate(1.0)?;
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
        });
    }
    let (m, topo) = mass(b, true)?;
    let topo = match topo {
        Some(t) => t,
        None => b.tessellate((b.size() * 1e-3).max(1e-3))?,
    };
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
    })
}

/// [`measure`]'s volume, area, centre and box alone: the same numbers, a tessellation fewer
/// (for callers that need no topology counts, such as an interference check).
pub fn measure_mass(b: &Body) -> Result<MassMeasure> {
    if b.triangle_mesh().is_some() {
        let m = measure(b)?;
        return Ok(MassMeasure { volume: m.volume, area: m.area, centroid: m.centroid, bbox: m.bbox });
    }
    Ok(mass(b, false)?.0)
}

/// The mass properties of a B-rep body from three tessellations: a coarse one (which sets how
/// fine the others go), then a fine and a medium one at once on desktop (one after the other
/// in the browser). With `topology`, also the coarse unrefined mesh the topology counts take,
/// made alongside them.
fn mass(b: &Body, topology: bool) -> Result<(MassMeasure, Option<Mesh>)> {
    let size = b.size();
    let coarse_tol = (size * 1e-3).max(1e-3);
    let coarse = b.tessellate_with(coarse_tol, true)?;
    // Chord error relative to a radius sets the volume error: keep it near 3e-4 of the
    // smallest radius (thin tubes, small fillets).
    let tight = min_edge_radius(&coarse).map(|r| r * 3e-4).unwrap_or(f64::INFINITY);
    let fine_tol = (size * 5e-5).min(tight).max(size * 5e-6).max(1e-4);
    let mut jobs = vec![(fine_tol, true), ((fine_tol * 5.0).max(2e-4), true)];
    if topology {
        jobs.push((coarse_tol, false));
    }
    // Independent tessellations of one body: each on its own thread, the calling thread's
    // profile counting their wall time once as tessellation. Not from inside a worker pool: a
    // pool thread blocked on threads of its own could leave their meshing (which runs on the
    // shared pool) without a worker.
    #[cfg(not(target_arch = "wasm32"))]
    let threads = if rayon::current_thread_index().is_none() { jobs.len() } else { 1 };
    #[cfg(target_arch = "wasm32")]
    let threads = 1;
    let made = crate::profile::timed(crate::profile::Kind::Tessellate, || {
        crate::par::map_on_threads(&jobs, threads, |&(tol, refine)| b.tessellate_with(tol, refine))
    });
    let mut made = made.into_iter().map(|m| m.unwrap_or_else(|| Err(crate::KernelError::Internal("tessellation failed".into()))));
    let missing = || crate::KernelError::Internal("tessellation missing".into());
    let fine = made.next().ok_or_else(missing)??;
    let medium = made.next().ok_or_else(missing)??;
    let topo = match made.next() {
        Some(m) => Some(m?),
        None => None,
    };
    let nf = b.face_count();
    // About the box's centre when the meshes leave only a crack: see `face_sums`. A body that is
    // open on purpose (a surface, or a cup whose lid is still a separate sheet) has no volume of
    // its own; it keeps the origin, so such bodies still compare with each other when the centre
    // of their boxes moves (a crowned lid rises, the open cup under it doesn't change).
    let (surface, gap) = face_sums(&fine, nf, Vec3::ZERO).iter().fold((0.0, Vec3::ZERO), |s, f| (s.0 + f.0, s.1 + f.3));
    let o = Some(fine.bounds().center()).filter(|c| c.is_finite() && gap.len() <= 0.01 * surface).unwrap_or(Vec3::ZERO);
    let (sf, sm, sc) = (face_sums(&fine, nf, o), face_sums(&medium, nf, o), face_sums(&coarse, nf, o));
    let close = |x: f64, y: f64| (x - y).abs() <= 0.01 * x.abs().max(y.abs()) + 1e-9;
    let (mut area, mut v6, mut mom) = (0.0, 0.0, Vec3::ZERO);
    for ((f, m), c) in sf.iter().zip(&sm).zip(&sc) {
        // Trust the finest tessellation that agrees with the next coarser one.
        let pick = if close(f.0, m.0) {
            f
        } else if close(m.0, c.0) {
            m
        } else {
            f
        };
        area += pick.0;
        v6 += pick.1;
        mom += pick.2;
    }
    // The moments back about the origin. A gap the meshes leave (a crack where a trimmed face
    // meets a hole cut into it) is closed by a cone to the box's centre, which keeps it small
    // (a crack of 0.3 mm² moved a part ring's volume by 7 % about the origin 1000 mm away; about
    // the centre, by 0.1 %). Every body is measured by the same meshes this way, so a body and the
    // pieces a boolean cuts it into add up: a closed display plan measured instead would mesh
    // some faces more coarsely than the sums do and break that.
    let mom = o * (4.0 * v6) + mom;
    let volume = v6 / 6.0;
    let centroid = if v6.abs() > 1e-12 { mom / (4.0 * v6) } else { fine.bounds().center() };
    Ok((MassMeasure { volume, area, centroid, bbox: coarse.bounds().union(&fine.bounds()) }, topo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoolOp, boolean, cylinder, torus};

    /// The mass properties alone are [`measure`]'s, to the bit, with the tessellations made at
    /// once; and on a curved intersection they are the textbook ones.
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
