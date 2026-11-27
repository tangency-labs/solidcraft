use serde::Serialize;
use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;

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

/// The relative accuracy SMLib integrates at: the finest the C API takes
/// ([`solidcraft_smlib::MEASURE_ACCURACY`], 1e-8). `smlib_brep_mass_properties` calls the
/// kernel's integration directly because SmApiBrepComputeMassProperties clamps to [1e-4, 1e-1],
/// which leaves curved faces off by 1e-5 to 1e-3; asking 1e-4 here threw that away.
const ACCURACY: f64 = solidcraft_smlib::MEASURE_ACCURACY;

/// Measure a body. A B-rep body's volume, area and centre are SMLib's, integrated on its exact
/// surfaces over its material regions (a sheet's: its faces' area and their centre, volume 0),
/// and its box is SMLib's exact box; a body SMLib cannot measure is an error. A mesh body is
/// measured from its triangles, exactly.
pub fn measure(b: &Body) -> Result<BodyMeasure> {
    let _span = solidcraft_profile::span("kernel", "measure");
    if let Some(mb) = &b.mesh {
        // Exact for a mesh body; topology counts are facets, sides and vertices.
        let (mm, bbox) = mb.measure();
        let edges = mb.edge_count();
        let (nf, nv) = (mb.face_count(), mb.vertex_count());
        let merged = TopoCounts { faces: nf, edges, vertices: nv, face_types: [("mesh".to_string(), nf)].into_iter().collect() };
        return Ok(BodyMeasure {
            volume: mm.volume,
            area: mm.area,
            centroid: mm.centroid,
            bbox,
            faces: nf,
            edges,
            vertices: nv,
            shells: 1,
            merged,
            measured_by: "mesh",
            notes: Vec::new(),
        });
    }
    let m = measure_mass(b)?;
    Ok(BodyMeasure {
        volume: m.volume,
        area: m.area,
        centroid: m.centroid,
        bbox: m.bbox,
        faces: b.face_count(),
        edges: b.edge_count(),
        vertices: b.vertex_count(),
        shells: b.shell_count(),
        merged: merged_topology(b)?,
        measured_by: m.measured_by,
        notes: m.notes,
    })
}

/// Maximum worker threads used by [`measure_many`] and [`measure_mass_many`].
const MEASURE_THREADS: usize = 16;

/// [`measure`]'s volume, area, centre and box alone (for callers that need no topology counts,
/// such as an interference check). SMLib's `(volume, area, centroid)` integration is cached per
/// record so repeated measurements (such as `assembly.bom` followed by `component.properties`)
/// reuse it without re-integrating.
pub fn measure_mass(b: &Body) -> Result<MassMeasure> {
    let _span = solidcraft_profile::span("kernel", "measure_mass");
    if let Some(mb) = &b.mesh {
        let (mm, bbox) = mb.measure();
        return Ok(MassMeasure { volume: mm.volume, area: mm.area, centroid: mm.centroid, bbox, measured_by: "mesh", notes: Vec::new() });
    }
    let (volume, area, centroid) = {
        // Held while integrating so callers on other threads wait for this measurement instead of
        // duplicating SMLib work on clones of the same body.
        let mut slot = b.mass.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = *slot {
            cached
        } else {
            let m = crate::guard("SMLib mass properties", || {
                b.smlib()?.mass_properties(ACCURACY).map_err(|e| crate::KernelError::smlib("SMLib mass properties", e))
            })?;
            let [x, y, z] = m.centroid;
            let computed = (m.volume, m.area, Vec3::new(x, y, z));
            *slot = Some(computed);
            computed
        }
    };
    if let Some(cell) = &b.volume
        && let Ok(mut vslot) = cell.lock()
        && vslot.is_none()
    {
        *vslot = Some((0, volume));
    }
    Ok(MassMeasure { volume, area, centroid, bbox: b.exact_bounds()?, measured_by: "smlib", notes: Vec::new() })
}

/// Measure every body in `bodies` in parallel on desktop (serially in the browser build or for a
/// single body), returning results in the exact input order. Each body is measured identically to
/// [`measure`], so results are bit-for-bit identical to measuring sequentially.
///
/// Respects cooperative cancellation via [`crate::watch`]: if cancelled before starting a body,
/// that body returns a cancelled error without starting new kernel work.
pub fn measure_many(bodies: &[&Body]) -> Vec<Result<BodyMeasure>> {
    let _span = solidcraft_profile::span("kernel", "measure_many");
    if bodies.is_empty() {
        return Vec::new();
    }
    if let [body] = bodies {
        if crate::watch::cancelled() {
            return vec![Err(crate::watch::cancelled_error("measure"))];
        }
        return vec![measure(body)];
    }
    crate::par::map_on_threads(bodies, MEASURE_THREADS, |&b| {
        if crate::watch::cancelled() {
            return Err(crate::watch::cancelled_error("measure"));
        }
        measure(b)
    })
    .into_iter()
    .map(|r| r.unwrap_or_else(|| Err(crate::KernelError::Internal("parallel measure worker failed".into()))))
    .collect()
}

/// Mass properties (`volume`, `area`, `centroid`, `bbox`) of every body in `bodies` in parallel
/// on desktop (serially in the browser build or for a single body), returning results in the exact
/// input order. Each body is measured identically to [`measure_mass`], so results are bit-for-bit
/// identical to measuring sequentially.
///
/// Respects cooperative cancellation via [`crate::watch`].
pub fn measure_mass_many(bodies: &[&Body]) -> Vec<Result<MassMeasure>> {
    let _span = solidcraft_profile::span("kernel", "measure_mass_many");
    if bodies.is_empty() {
        return Vec::new();
    }
    if let [body] = bodies {
        if crate::watch::cancelled() {
            return vec![Err(crate::watch::cancelled_error("measure"))];
        }
        return vec![measure_mass(body)];
    }
    crate::par::map_on_threads(bodies, MEASURE_THREADS, |&b| {
        if crate::watch::cancelled() {
            return Err(crate::watch::cancelled_error("measure"));
        }
        measure_mass(b)
    })
    .into_iter()
    .map(|r| r.unwrap_or_else(|| Err(crate::KernelError::Internal("parallel measure worker failed".into()))))
    .collect()
}

/// How far `b` reaches along `plane`'s normal, measured from the plane: the least and greatest
/// level of its points, `[low, high]`, as solidcore-ops measures it
/// ([`solidcore_ops::Body::levels_along`]: the body moved rigidly into the plane's own frame and
/// its exact box read there), so the span is the body's own, not the shadow of its world box.
/// A mesh body has no solidcore form and is refused.
pub fn levels_along(b: &Body, plane: &solidcore_record::Plane) -> Result<[f64; 2]> {
    let Some(form) = b.ops_body() else {
        return Err(crate::KernelError::Failed("the body's reach along the plane's normal: the body has no solidcore form".into()));
    };
    let (ops, _) = &*form;
    let (o, x, y) = (plane.origin, plane.x, plane.y);
    ops.levels_along([o.x, o.y, o.z], [x.x, x.y, x.z], [y.x, y.y, y.z])
        .map_err(|e| crate::KernelError::Invalid(format!("the body's reach along the plane's normal can't be measured: {e}")))
}

/// Whether the endless line through `o` along `d` meets the box `b`, its sides included (never an
/// empty box, never a zero direction or a coordinate that is not finite). Each side counts a
/// millionth of the box's size beyond it, so a line read back from a body's own face still meets
/// its box (solidcore-ops' `measure::line_meets_box`).
pub fn line_meets_box(b: &Aabb3, o: Vec3, d: Vec3) -> bool {
    let k = |v: Vec3| [v.x, v.y, v.z];
    !b.is_empty() && solidcore_ops::measure::line_meets_box(k(b.min), k(b.max), k(o), k(d), 1e-6).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoolOp, boolean, cylinder, torus};
    use solidcraft_geom::prelude::*;

    /// A line meets a box when it passes through it or along a side, never when it passes by, and
    /// never an empty box or a zero direction.
    #[test]
    fn a_line_meets_a_box_it_passes_through() {
        let b = Aabb3 { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(20.0, 20.0, 20.0) };
        assert!(super::line_meets_box(&b, Vec3::new(10.0, 10.0, -50.0), Vec3::Z), "upright through the middle");
        assert!(super::line_meets_box(&b, Vec3::new(-5.0, 10.0, 10.0), Vec3::new(1.0, 1.0, 0.0)), "slanted through");
        assert!(super::line_meets_box(&b, Vec3::new(20.0, 10.0, 0.0), Vec3::Z), "along a side");
        assert!(!super::line_meets_box(&b, Vec3::new(30.0, 10.0, 0.0), Vec3::Z), "beside it");
        // A millionth of the box's size beyond a side still counts, a ten-thousandth does not.
        assert!(super::line_meets_box(&b, Vec3::new(20.00001, 10.0, 0.0), Vec3::Z), "just past a side");
        assert!(!super::line_meets_box(&b, Vec3::new(20.002, 10.0, 0.0), Vec3::Z), "clear of a side");
        assert!(!super::line_meets_box(&b, Vec3::new(10.0, 10.0, 0.0), Vec3::ZERO) && !super::line_meets_box(&Aabb3::EMPTY, Vec3::ZERO, Vec3::Z));
    }

    #[test]
    fn selected_faces_skip_exact_conversion_for_either_mesh_operand() {
        let mesh = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y], &[[0, 1, 2]]).unwrap();
        let exact = crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let mut unavailable = exact.clone();
        // A cached refusal is how ops_body represents a B-rep it could not convert.
        unavailable.ops = std::sync::Arc::new(crate::form_cell::FormCell::refused_for(()));
        assert!(mesh.is_mesh() && !unavailable.is_mesh());
        for (a, b) in [(&mesh, &unavailable), (&unavailable, &mesh)] {
            assert_eq!(crate::faces_closest(a, &[0], b, &[0]).unwrap(), None);
        }
        for (a, b) in [(&exact, &unavailable), (&unavailable, &exact)] {
            assert!(
                matches!(crate::faces_closest(a, &[0], b, &[0]), Err(crate::KernelError::Failed(message)) if message.contains("exact measurement is unavailable"))
            );
        }
    }

    /// The selected sides of nested cylinders are their radii apart; whole-body distance
    /// would instead see coincident caps. Both witnesses belong to the selected surfaces.
    #[test]
    fn selected_cylinder_faces_measure_their_exact_gap() {
        let a = cylinder(Vec3::ZERO, Vec3::Z, 20.0, 5.0).unwrap();
        let b = cylinder(Vec3::ZERO, Vec3::Z, 35.0, 5.0).unwrap();
        let side = |b: &Body| {
            (0..b.record().faces.len())
                .filter(|&f| matches!(crate::face_surface(b, f), Some(crate::FaceSurface::Cylinder { .. })))
                .collect::<Vec<_>>()
        };
        let (af, bf) = (side(&a), side(&b));
        assert!(!af.is_empty() && !bf.is_empty());
        let c = crate::faces_closest(&a, &af, &b, &bf).unwrap().unwrap();
        assert!(c.exact && (c.distance - 15.0).abs() < 1e-9, "{c:?}");
        assert!((c.from.x.hypot(c.from.y) - 20.0).abs() < 1e-9 && (c.to.x.hypot(c.to.y) - 35.0).abs() < 1e-9, "{c:?}");
    }

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

    /// A box's reach along a plane's normal, from the plane: away from the origin, and on a
    /// plane whose normal is not a world axis's direction (its world box would overstate it).
    #[test]
    fn levels_along_a_plane_are_the_bodys_own() {
        let block = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let raised = solidcore_record::Plane { origin: Vec3::new(3.0, 4.0, 5.0), ..solidcore_record::Plane::XY };
        let [lo, hi] = levels_along(&block, &raised).unwrap();
        assert!((lo + 5.0).abs() < 1e-9 && (hi - 5.0).abs() < 1e-9, "{lo} {hi}");
        // A plane through the origin whose normal is (1, 1, 0)/sqrt 2: the box's corners reach
        // from 0 to 20/sqrt 2 along it.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let tilted = solidcore_record::Plane { origin: Vec3::ZERO, x: Vec3::new(0.0, 0.0, 1.0), y: Vec3::new(s, -s, 0.0) };
        assert!((tilted.normal() - Vec3::new(s, s, 0.0)).len() < 1e-12, "{:?}", tilted.normal());
        let [lo, hi] = levels_along(&block, &tilted).unwrap();
        assert!(lo.abs() < 1e-9 && (hi - 20.0 * s).abs() < 1e-9, "{lo} {hi}");
    }

    /// A body's exact box is measured once and its clones share it; a moved copy is a new body
    /// and measures its own.
    #[test]
    fn exact_box_is_kept_per_record() {
        let post = cylinder(Vec3::ZERO, Vec3::Z, 1.5, 10.0).unwrap();
        let clone = post.clone();
        let first = post.exact_bounds().unwrap();
        assert_eq!(*clone.bounds.lock().unwrap(), Some(first));
        assert_eq!(clone.exact_bounds().unwrap(), first);
        let moved = transform(&post, Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0).unwrap();
        let b = moved.exact_bounds().unwrap();
        assert!((b.min.x - first.min.x - 5.0).abs() < 1e-9 && (b.max.x - first.max.x - 5.0).abs() < 1e-9, "{b:?} against {first:?}");
    }

    /// A tool whose vertices' box meets the body's is not apart from it, and neither exact box is
    /// measured to show it (a pattern's every cut asked for its target's, ever more holes); tools
    /// whose vertices' boxes do not meet are judged by the exact boxes, as before.
    #[test]
    fn a_tool_among_the_bodys_vertices_is_not_apart_without_measuring() {
        let plate = crate::box_solid(Vec3::ZERO, Vec3::new(200.0, 100.0, 1.0)).unwrap();
        let pin = crate::box_solid(Vec3::new(4.0, 4.0, -2.0), Vec3::new(6.0, 6.0, 2.0)).unwrap();
        assert!(!crate::apart(&plate, &pin).unwrap());
        assert_eq!(*plate.bounds.lock().unwrap(), None);
        assert_eq!(*pin.bounds.lock().unwrap(), None);
        // Clear of the plate: the exact boxes show it, measured as before.
        let far = crate::box_solid(Vec3::new(300.0, 0.0, 0.0), Vec3::new(310.0, 10.0, 1.0)).unwrap();
        assert!(crate::apart(&plate, &far).unwrap());
        assert!(plate.bounds.lock().unwrap().is_some());
    }

    /// Bodies whose vertices' boxes meet are not joined apart, and no exact box is measured to
    /// show it (a Combine join onto a rounded spline body measured it for tens of seconds);
    /// bodies whose vertices' boxes do not meet are judged by the exact boxes, as before.
    #[test]
    fn bodies_among_each_others_vertices_are_not_joined_apart_without_measuring() {
        let plate = crate::box_solid(Vec3::ZERO, Vec3::new(200.0, 100.0, 1.0)).unwrap();
        let on_top = crate::box_solid(Vec3::new(10.0, 10.0, 1.0), Vec3::new(20.0, 20.0, 5.0)).unwrap();
        let far = crate::box_solid(Vec3::new(300.0, 0.0, 0.0), Vec3::new(310.0, 10.0, 1.0)).unwrap();
        assert!(crate::union_apart(&[plate.clone(), far.clone(), on_top.clone()]).unwrap().is_none());
        assert_eq!(*plate.bounds.lock().unwrap(), None);
        assert_eq!(*on_top.bounds.lock().unwrap(), None);
        assert_eq!(*far.bounds.lock().unwrap(), None);
        // Clear of each other: the exact boxes show it, measured as before, and they are joined.
        let joined = crate::union_apart(&[plate.clone(), far.clone()]).unwrap().expect("apart");
        assert_eq!(joined.lumps().map_or(0, |l| l.len()), 2);
        assert!(plate.bounds.lock().unwrap().is_some() && far.bounds.lock().unwrap().is_some());
    }

    /// Overlapping cylinders whose only vertices lie on distant seams are still rejected by
    /// `union_apart` using their analytic circle edges, without measuring any exact box.
    #[test]
    fn overlapping_cylinders_are_not_joined_apart_without_measuring() {
        let a = cylinder(Vec3::ZERO, Vec3::Z, 5.0, 10.0).unwrap();
        let b = cylinder(Vec3::new(6.0, 0.0, 0.0), Vec3::Z, 5.0, 10.0).unwrap();
        assert!(crate::union_apart(&[a.clone(), b.clone()]).unwrap().is_none());
        assert_eq!(*a.bounds.lock().unwrap(), None);
        assert_eq!(*b.bounds.lock().unwrap(), None);
    }

    /// Contact tests reject clearly apart bodies from their exact boxes without meshing either,
    /// while pairs within the contact-mesh chord margin (`gap + 2 * tol`) still run the mesh test.
    #[test]
    fn meets_rejects_apart_bodies_without_meshing() {
        let a = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap().keeping_meshes();
        let far = crate::box_solid(Vec3::new(50.0, 0.0, 0.0), Vec3::new(60.0, 10.0, 10.0)).unwrap().keeping_meshes();
        // Size is ~17.32, so `tol = size * 1e-4 ≈ 0.00173`, `gap = 2 * tol ≈ 0.00346`, and
        // `gap + 2 * tol ≈ 0.00693`. A 0.005 gap sits past `gap` yet inside the chord margin, so
        // the exact-box prefilter must not skip meshing it.
        let near_chord = crate::box_solid(Vec3::new(10.005, 0.0, 0.0), Vec3::new(20.005, 10.0, 10.0)).unwrap().keeping_meshes();
        let touching = crate::box_solid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)).unwrap().keeping_meshes();
        assert!(!a.meets(&far).unwrap());
        assert_eq!(a.meets_each(std::slice::from_ref(&far)).unwrap(), vec![false]);
        assert_eq!(a.cached_mesh_count(), 0);
        assert_eq!(far.cached_mesh_count(), 0);
        assert!(!a.meets(&near_chord).unwrap());
        assert!(a.cached_mesh_count() > 0, "pair inside chord margin must not be skipped by exact-box prefilter");
        assert!(a.meets(&touching).unwrap());
        assert_eq!(a.meets_each(&[far, touching]).unwrap(), vec![false, true]);
    }

    /// A cube 1000 mm from the origin with one 0.5 mm² facet left out: about the box's centre
    /// the sums miss a cone of that facet to the centre (0.08 %), where about the origin they are
    /// 17 % over. Closed, it measures the same about either.
    // A body's measured volume does not depend on where it sits: the same revolved ring of cones
    // and cylinders, moved far from the origin, measures what it measures at the origin.
    use crate::{revolve, transform};
    use solidcore_record::{Loop2, Plane, Region2, Vec2};
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

    /// Adds a STEP entity and returns its id.
    fn ent(ents: &mut Vec<String>, e: String) -> usize {
        ents.push(e);
        ents.len()
    }

    fn placement(ents: &mut Vec<String>, origin: [f64; 3], axis: [f64; 3], reference: [f64; 3]) -> usize {
        let o = ent(ents, format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", origin[0], origin[1], origin[2]));
        let z = ent(ents, format!("DIRECTION('',({:?},{:?},{:?}))", axis[0], axis[1], axis[2]));
        let x = ent(ents, format!("DIRECTION('',({:?},{:?},{:?}))", reference[0], reference[1], reference[2]));
        ent(ents, format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }

    /// A 3 x 10 x 3 block (x 0..3, y 0..10, z 0..3) whose top is a cylinder of radius 10⁶ about an
    /// axis along x, so the top bows up 12.5 nm; its top edges are arcs of full circles, as an
    /// exporter writes a large-radius face. The loose box of such an arc holds the circle's whole
    /// quarter spans, a million millimetres off. The centre of mass was z = 6.75, outside the block
    /// (#17361): it is the block's, to the bow.
    #[test]
    fn a_large_radius_face_s_centre_of_mass_is_inside_the_body() {
        let (l, w, h, r) = (3.0_f64, 10.0_f64, 3.0_f64, 1.0e6_f64);
        let zc = h - r;
        let zt = zc + (r * r - w * w / 4.0).sqrt();
        let mut e = Vec::new();
        let corners = [[0.0, 0.0, 0.0], [l, 0.0, 0.0], [l, w, 0.0], [0.0, w, 0.0], [0.0, 0.0, zt], [l, 0.0, zt], [l, w, zt], [0.0, w, zt]];
        let mut v = Vec::new();
        for c in corners {
            let p = ent(&mut e, format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", c[0], c[1], c[2]));
            v.push(ent(&mut e, format!("VERTEX_POINT('',#{p})")));
        }
        // edges by their vertices (1-based corners); the two arcs run from y = 10 to y = 0
        let edge = |e: &mut Vec<String>, a: usize, b: usize, arc_at: Option<f64>| {
            let curve = match arc_at {
                Some(x) => {
                    let ax = placement(e, [x, w / 2.0, zc], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
                    ent(e, format!("CIRCLE('',#{ax},{r:?})"))
                }
                None => {
                    let (pa, pb) = (corners[a - 1], corners[b - 1]);
                    let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    let o = ent(e, format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", pa[0], pa[1], pa[2]));
                    let dd = ent(e, format!("DIRECTION('',({:?},{:?},{:?}))", d[0] / n, d[1] / n, d[2] / n));
                    let vec = ent(e, format!("VECTOR('',#{dd},1.0)"));
                    ent(e, format!("LINE('',#{o},#{vec})"))
                }
            };
            ent(e, format!("EDGE_CURVE('',#{},#{},#{curve},.T.)", v[a - 1], v[b - 1]))
        };
        let ends = [
            (1, 2, None),
            (2, 3, None),
            (3, 4, None),
            (4, 1, None),
            (1, 5, None),
            (2, 6, None),
            (3, 7, None),
            (4, 8, None),
            (5, 6, None),
            (7, 8, None),
            (8, 5, Some(0.0)),
            (7, 6, Some(l)),
        ];
        let mut edges = Vec::new();
        for (a, b, arc) in ends {
            edges.push(edge(&mut e, a, b, arc));
        }
        // each face: its loop (edge number, same sense) counter-clockwise about its outward normal
        let face = |e: &mut Vec<String>, lp: &[(usize, bool)], surface: usize| {
            let mut uses = Vec::new();
            for &(i, same) in lp {
                uses.push(format!("#{}", ent(e, format!("ORIENTED_EDGE('',*,*,#{},.{}.)", edges[i - 1], if same { "T" } else { "F" }))));
            }
            let lo = ent(e, format!("EDGE_LOOP('',({}))", uses.join(",")));
            let bound = ent(e, format!("FACE_OUTER_BOUND('',#{lo},.T.)"));
            ent(e, format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"))
        };
        let plane = |e: &mut Vec<String>, o: [f64; 3], n: [f64; 3], x: [f64; 3]| {
            let ax = placement(e, o, n, x);
            ent(e, format!("PLANE('',#{ax})"))
        };
        let mut faces = Vec::new();
        let s = plane(&mut e, [0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
        faces.push(face(&mut e, &[(4, false), (3, false), (2, false), (1, false)], s));
        let s = plane(&mut e, [0.0; 3], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
        faces.push(face(&mut e, &[(1, true), (6, true), (9, false), (5, false)], s));
        let s = plane(&mut e, [0.0, w, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
        faces.push(face(&mut e, &[(3, true), (8, true), (10, false), (7, false)], s));
        let s = plane(&mut e, [0.0; 3], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        faces.push(face(&mut e, &[(5, true), (11, false), (8, false), (4, true)], s));
        let s = plane(&mut e, [l, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        faces.push(face(&mut e, &[(2, true), (7, true), (12, true), (6, false)], s));
        let ax = placement(&mut e, [0.0, w / 2.0, zc], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let s = ent(&mut e, format!("CYLINDRICAL_SURFACE('',#{ax},{r:?})"));
        faces.push(face(&mut e, &[(9, true), (12, false), (10, true), (11, true)], s));
        let list: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
        let shell = ent(&mut e, format!("CLOSED_SHELL('',({}))", list.join(",")));
        let solid = ent(&mut e, format!("MANIFOLD_SOLID_BREP('Strip',#{shell})"));
        ent(&mut e, format!("SHAPE_REPRESENTATION('',(#{solid}),$)"));
        let mut text = String::from(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','hand','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n",
        );
        for (i, x) in e.iter().enumerate() {
            text.push_str(&format!("#{}={x};\n", i + 1));
        }
        text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");

        let imp = crate::step_import(&text).unwrap();
        assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
        assert!(imp.bodies[0].closed, "{:?}", imp.warnings);
        let m = measure(&imp.bodies[0].body).unwrap();
        // The block less a sliver under the bow: its centre is (1.5, 5, 1.5) to the bow's height.
        let bow = h - zt;
        assert!(rel(m.volume, l * w * h) < bow, "volume {}", m.volume);
        let off = m.centroid - Vec3::new(l / 2.0, w / 2.0, h / 2.0);
        assert!(off.len() < bow, "centre of mass {:?}, off by {off:?}", m.centroid);
    }

    /// Parallel [`measure_many`] matches sequential [`measure`] bit-for-bit on empty, single,
    /// and multi-body mixes of B-rep solids and mesh bodies, preserving exact input order.
    #[test]
    fn measure_many_matches_sequential_bit_for_bit() {
        assert!(measure_many(&[]).is_empty());

        let single = crate::box_solid(Vec3::ZERO, Vec3::new(3.0, 4.0, 5.0)).unwrap();
        let single_seq = measure(&single).unwrap();
        let single_par = measure_many(&[&single]).pop().unwrap().unwrap();
        assert_eq!(single_seq.volume.to_bits(), single_par.volume.to_bits());
        assert_eq!(single_seq.area.to_bits(), single_par.area.to_bits());

        // Build two independent sets of fresh (uncached) bodies so neither set warms the other.
        let build_set = || -> Vec<Body> {
            let mut out = Vec::new();
            for i in 0..6 {
                let d = f64::from(i + 1);
                out.push(crate::box_solid(Vec3::new(d * 10.0, d, 0.0), Vec3::new(d * 10.0 + d + 2.0, d + 3.0, d + 1.0)).unwrap());
            }
            for i in 0..5 {
                let d = f64::from(i + 1);
                out.push(cylinder(Vec3::new(0.0, d * 15.0, 0.0), Vec3::Z, 1.5 + 0.5 * d, 4.0 + d).unwrap());
            }
            out.push(torus(Vec3::new(50.0, -20.0, 5.0), 8.0, 2.0).unwrap());
            let mesh_pts = [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(4.0, 0.0, 0.0),
                Vec3::new(4.0, 3.0, 0.0),
                Vec3::new(0.0, 3.0, 0.0),
                Vec3::new(0.0, 0.0, 2.0),
                Vec3::new(4.0, 0.0, 2.0),
                Vec3::new(4.0, 3.0, 2.0),
                Vec3::new(0.0, 3.0, 2.0),
            ];
            let mesh_tris =
                [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [2, 3, 7], [2, 7, 6], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
            out.push(crate::mesh_body(&mesh_pts, &mesh_tris).unwrap());
            out
        };
        let seq_bodies = build_set();
        let par_bodies = build_set();
        let seq: Vec<BodyMeasure> = seq_bodies.iter().map(|b| measure(b).unwrap()).collect();
        let par_refs: Vec<&Body> = par_bodies.iter().collect();
        let par: Vec<BodyMeasure> = measure_many(&par_refs).into_iter().map(Result::unwrap).collect();
        assert_eq!(seq.len(), par.len());
        for (i, (a, b)) in seq.iter().zip(&par).enumerate() {
            assert_eq!(a.volume.to_bits(), b.volume.to_bits(), "volume mismatch at {i}");
            assert_eq!(a.area.to_bits(), b.area.to_bits(), "area mismatch at {i}");
            assert_eq!(
                (a.centroid.x.to_bits(), a.centroid.y.to_bits(), a.centroid.z.to_bits()),
                (b.centroid.x.to_bits(), b.centroid.y.to_bits(), b.centroid.z.to_bits()),
                "centroid mismatch at {i}"
            );
            assert_eq!(
                (a.bbox.min.x.to_bits(), a.bbox.min.y.to_bits(), a.bbox.min.z.to_bits()),
                (b.bbox.min.x.to_bits(), b.bbox.min.y.to_bits(), b.bbox.min.z.to_bits()),
                "bbox.min mismatch at {i}"
            );
            assert_eq!(
                (a.bbox.max.x.to_bits(), a.bbox.max.y.to_bits(), a.bbox.max.z.to_bits()),
                (b.bbox.max.x.to_bits(), b.bbox.max.y.to_bits(), b.bbox.max.z.to_bits()),
                "bbox.max mismatch at {i}"
            );
            assert_eq!((a.faces, a.edges, a.vertices, a.shells), (b.faces, b.edges, b.vertices, b.shells), "counts mismatch at {i}");
            assert_eq!(a.merged, b.merged, "merged counts mismatch at {i}");
            assert_eq!(a.measured_by, b.measured_by, "measured_by mismatch at {i}");
            assert_eq!(a.notes, b.notes, "notes mismatch at {i}");
        }
    }

    /// Parallel [`measure_mass_many`] matches sequential [`measure_mass`] bit-for-bit, propagates
    /// per-body errors at their exact indices without disrupting sibling bodies, and honours
    /// cooperative watch cancellation.
    #[test]
    fn measure_mass_many_matches_sequential_bit_for_bit_and_propagates_errors() {
        assert!(measure_mass_many(&[]).is_empty());

        let one = cylinder(Vec3::ZERO, Vec3::Z, 3.0, 8.0).unwrap();
        let one_seq = measure_mass(&one).unwrap();
        let one_par = measure_mass_many(&[&one]).pop().unwrap().unwrap();
        assert_eq!(one_seq.volume.to_bits(), one_par.volume.to_bits());
        assert_eq!(one_seq.area.to_bits(), one_par.area.to_bits());

        let mesh = crate::mesh_body(&[Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)], &[[0, 1, 2]]).unwrap();
        let mut broken = crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        broken.smlib = std::sync::Arc::new(crate::form_cell::FormCell::refused_for(crate::Refusal {
            face: None,
            surface: None,
            why: "simulated refusal".into(),
            internal: false,
        }));
        let ok_a = crate::box_solid(Vec3::ZERO, Vec3::new(5.0, 6.0, 7.0)).unwrap();
        let ok_b = cylinder(Vec3::new(10.0, 0.0, 0.0), Vec3::Z, 2.5, 9.0).unwrap();
        let refs = [&ok_a, &broken, &mesh, &ok_b];
        let seq: Vec<Result<MassMeasure>> = refs.iter().map(|b| measure_mass(b)).collect();
        // Fresh uncached clones for the parallel run.
        let ok_a2 = crate::box_solid(Vec3::ZERO, Vec3::new(5.0, 6.0, 7.0)).unwrap();
        let ok_b2 = cylinder(Vec3::new(10.0, 0.0, 0.0), Vec3::Z, 2.5, 9.0).unwrap();
        let par = measure_mass_many(&[&ok_a2, &broken, &mesh, &ok_b2]);
        assert_eq!(par.len(), 4);
        assert!(par[1].is_err(), "error body must stay an error at index 1");
        for i in [0, 2, 3] {
            let a = seq[i].as_ref().unwrap();
            let b = par[i].as_ref().unwrap();
            assert_eq!(a.volume.to_bits(), b.volume.to_bits(), "volume mismatch at {i}");
            assert_eq!(a.area.to_bits(), b.area.to_bits(), "area mismatch at {i}");
            assert_eq!(
                (a.centroid.x.to_bits(), a.centroid.y.to_bits(), a.centroid.z.to_bits()),
                (b.centroid.x.to_bits(), b.centroid.y.to_bits(), b.centroid.z.to_bits()),
                "centroid mismatch at {i}"
            );
            assert_eq!((a.bbox.min, a.bbox.max), (b.bbox.min, b.bbox.max), "bbox mismatch at {i}");
            assert_eq!(a.measured_by, b.measured_by);
        }

        // Watch cancellation stops measurement cleanly.
        struct Cancelled;
        impl crate::watch::Watch for Cancelled {
            fn cancelled(&self) -> bool {
                true
            }
            fn progress(&self, _stage: &'static str, _done: usize, _total: usize) {}
        }
        let cancelled = crate::watch::watching(std::sync::Arc::new(Cancelled), || measure_mass_many(&[&ok_a2, &ok_b2]));
        assert!(cancelled.iter().all(|r| matches!(r, Err(crate::KernelError::Failed(m)) if m.contains("cancelled"))));
    }

    /// Mass properties are cached per record and shared by clones, while a rigid transform clears
    /// the cached mass properties so the moved body reports its new centroid.
    #[test]
    fn mass_properties_are_cached_per_record_and_reset_on_move() {
        let post = cylinder(Vec3::ZERO, Vec3::Z, 2.0, 10.0).unwrap();
        let clone = post.clone();
        assert!(post.mass.lock().unwrap().is_none());
        let first = measure_mass(&post).unwrap();
        assert!(clone.mass.lock().unwrap().is_some());
        let second = measure_mass(&clone).unwrap();
        assert_eq!(first.volume.to_bits(), second.volume.to_bits());
        assert_eq!(first.area.to_bits(), second.area.to_bits());
        assert_eq!(first.centroid, second.centroid);
        let moved = transform(&post, Vec3::new(12.0, -4.0, 3.0), Vec3::ZERO, Vec3::Z, 0.0).unwrap();
        assert!(moved.mass.lock().unwrap().is_none(), "moved body must compute its own centroid");
        let m = measure_mass(&moved).unwrap();
        assert!((m.centroid - (first.centroid + Vec3::new(12.0, -4.0, 3.0))).len() < 1e-9);
    }
}
