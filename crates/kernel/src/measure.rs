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

/// [`measure`]'s volume, area, centre and box alone (for callers that need no topology counts,
/// such as an interference check).
pub fn measure_mass(b: &Body) -> Result<MassMeasure> {
    let _span = solidcraft_profile::span("kernel", "measure_mass");
    if b.triangle_mesh().is_some() {
        let m = measure(b)?;
        return Ok(MassMeasure { volume: m.volume, area: m.area, centroid: m.centroid, bbox: m.bbox, measured_by: m.measured_by, notes: m.notes });
    }
    let m = crate::guard("SMLib mass properties", || {
        b.smlib()?.mass_properties(ACCURACY).map_err(|e| crate::KernelError::smlib("SMLib mass properties", e))
    })?;
    let [x, y, z] = m.centroid;
    Ok(MassMeasure { volume: m.volume, area: m.area, centroid: Vec3::new(x, y, z), bbox: b.exact_bounds()?, measured_by: "smlib", notes: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoolOp, boolean, cylinder, torus};
    use solidcraft_geom::prelude::*;

    #[test]
    fn selected_faces_skip_exact_conversion_for_either_mesh_operand() {
        let mesh = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y], &[[0, 1, 2]]).unwrap();
        let exact = crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let mut unavailable = exact.clone();
        // A cached refusal is how ops_body represents a B-rep it could not convert.
        unavailable.ops = std::sync::Arc::new(std::sync::OnceLock::from(None));
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
}
