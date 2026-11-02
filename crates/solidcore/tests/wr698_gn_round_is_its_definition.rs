//! WR-698 (#2273, the fork's 4a10ed8): with `fillet_round_as_definition` (and
//! `fillet_gn_section_generator`) the G^n section generator's round is its definition,
//! a procedural blend (`SmProceduralBlend`) on the certified net it built: its faces,
//! the ball's offsets and sides, the section, the centre line as the leader, the feet's
//! uv curves as the seeds and the certified bound. A face that is itself a blend by its
//! definition is refused (its net is not the surface the feet are solved on).
//!
//! Expected values: fork 3bc54a4 (`SMLib` and `SM_API` only; 4a10ed8 gave the same to
//! 1e-9 on the planes, and its own pipe values before 3bc54a4's jets), built locally with the
//! harness' flags and driven through `SmApiFilletEdges` on the same bodies as
//! `wr685_gn_equal_area_per_station.rs`: each blend face's type, its points at 16 domain
//! fractions (`EvaluatePoint`, the definition's), within 1e-9 relative (an absolute
//! floor of 1e-9 times the body's size for coordinates the fork puts near 0), and the
//! body's volume within 1e-7 (the volume integrator's tolerance). The volume
//! integrates the definition's points, not the net's (the fork at this commit has no net
//! scope in the tessellator yet).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::PROCEDURAL_BLEND_TYPE;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel(definition: bool) -> Kernel {
    let mut fixes = UpstreamFixes::default();
    fixes.fillet_gn_section_generator = true;
    fixes.fillet_surface_plane_curve_on_plane_equation = true;
    fixes.fillet_round_as_definition = definition;
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The fork's values for one body.
struct Fork {
    volume: f64,
    /// Each blend face's points at the domain fractions (u, v), u and v each 0, 0.3,
    /// 0.5, 1 (u outer) and 0, 0.25, 0.7, 1.
    faces: &'static [&'static [[f64; 3]]],
}

/// The faces whose surface is a blend (any kind with the G^n section's degree 7 in v).
fn blend_faces(b: &Brep) -> Vec<Surface> {
    let mut out = Vec::new();
    for &f in b.face_list() {
        let Some(s) = b.face(f).unwrap().surface.clone() else {
            continue;
        };
        let net = match s.kind() {
            SurfaceKind::BSpline(bs) => bs,
            SurfaceKind::ProceduralBlend(pb) => pb.net(),
            _ => continue,
        };
        if net.degree(solidcore::nurbs::b_spline_surface::SurfParamType::V) == 7 {
            out.push(s);
        }
    }
    out
}

fn close(a: f64, b: f64, size: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()) || (a - b).abs() <= 1e-9 * size
}

fn check(what: &str, k: &Kernel, b: &mut Brep, want: &Fork, size: f64) {
    let faces = blend_faces(b);
    assert_eq!(faces.len(), want.faces.len(), "{what}: blend faces");
    let fr = [0.0, 0.3, 0.5, 1.0];
    let fc = [0.0, 0.25, 0.7, 1.0];
    for wf in want.faces {
        // the faces may come in another order: match by the first point
        let s = faces
            .iter()
            .find(|s| {
                let d = s.natural_uv_domain_with_nurb(k).unwrap();
                let p = s.evaluate_point(k, d.evaluate(0.0, 0.0)).unwrap();
                [p.x, p.y, p.z]
                    .iter()
                    .zip(wf[0])
                    .all(|(a, b)| close(*a, b, size))
            })
            .unwrap_or_else(|| panic!("{what}: no face starts at {:?}", wf[0]));
        assert_eq!(
            s.type_code(),
            PROCEDURAL_BLEND_TYPE,
            "{what}: a blend by its definition"
        );
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        let mut i = 0;
        for a in fr {
            for c in fc {
                let p = s.evaluate_point(k, d.evaluate(a, c)).unwrap();
                let q = wf[i];
                assert!(
                    [p.x, p.y, p.z]
                        .iter()
                        .zip(q)
                        .all(|(a, b)| close(*a, b, size)),
                    "{what}: point {i} at ({a}, {c}): {p:?}, the fork's {q:?}"
                );
                i += 1;
            }
        }
    }
    let v = volume_of(k, b);
    assert!(
        (v - want.volume).abs() <= 1e-7 * want.volume,
        "{what}: volume {v}, the fork's {}",
        want.volume
    );
}

/// Two edges of different wedges after the box's top back edge is chamfered 5 (the
/// chamfer's top edge and the box's bottom front edge), rounded r 2 at the equal area
/// default; and, without the definition flag, the same rounds stay B-splines.
#[test]
fn a_gn_round_is_its_definition() {
    const FORK: Fork = Fork {
        volume: 23462.221395287153,
        faces: &[
            &[
                [0.0, 24.17157287525382, 20.0],
                [-6.58972293617564e-18, 24.38297445045419, 19.99409258685693],
                [
                    -8.029832232476577e-16,
                    25.378086754236588,
                    19.601604593492233,
                ],
                [
                    -8.881682553047583e-16,
                    25.58578643762691,
                    19.414213562373096,
                ],
                [12.000183000000002, 24.171572875253823, 20.000000000000004],
                [12.000183000000003, 24.382974450454192, 19.994092586856933],
                [12.000183, 25.378086754236588, 19.601604593492226],
                [12.000183, 25.58578643762691, 19.41421356237309],
                [20.000305, 24.171572875253837, 20.0],
                [20.000305, 24.382974450454206, 19.99409258685693],
                [20.000304999999997, 25.378086754236595, 19.601604593492215],
                [20.000305, 25.58578643762692, 19.414213562373078],
                [40.000610000000016, 24.171572875253833, 20.000000000000004],
                [40.000610000000016, 24.382974450454203, 19.994092586856933],
                [40.00061, 25.378086754236595, 19.601604593492223],
                [40.00061000000001, 25.58578643762692, 19.414213562373085],
            ],
            &[
                [0.0, 1.9999999999999987, 0.0],
                [0.0, 1.5826788687379034, 0.024074835266159174],
                [0.0, 0.05768975730114455, 1.4473588091098626],
                [0.0, 0.0, 2.0],
                [12.000182999999994, 1.9999999999999987, 0.0],
                [12.000182999999991, 1.5826788687379034, 0.024074835266159215],
                [12.000182999999996, 0.05768975730114459, 1.4473588091098637],
                [12.000182999999996, 0.0, 2.0000000000000004],
                [20.000305000000004, 1.9999999999999982, 0.0],
                [20.000305, 1.582678868737903, 0.02407483526615916],
                [20.000305, 0.05768975730114452, 1.4473588091098621],
                [20.000305000000004, 0.0, 1.9999999999999996],
                [40.00061000000001, 1.9999999999999984, 0.0],
                [40.000609999999995, 1.5826788687379032, 0.024074835266159215],
                [40.000610000000016, 0.05768975730114459, 1.4473588091098637],
                [40.000610000000016, 0.0, 2.0000000000000004],
            ],
        ],
    };
    for definition in [true, false] {
        let mut k = kernel(definition);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let c = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[c], 5.0, 0, 2, 0.0).unwrap();
        let both = [
            pick(&k, &b, [20.0, 25.0, 20.0]),
            pick(&k, &b, [20.0, 0.0, 0.0]),
        ];
        fillet_edges(&mut k, &mut b, &both, 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
        if definition {
            check("chamfered box", &k, &mut b, &FORK, 40.0);
        } else {
            let faces = blend_faces(&b);
            assert_eq!(faces.len(), 2);
            assert!(faces
                .iter()
                .all(|s| matches!(s.kind(), SurfaceKind::BSpline(_))));
        }
    }
}

/// The fork's pipe on a plate: a plane and a cylinder, the round r 2 at the equal area
/// default, its definition's points on the cylinder.
#[test]
fn a_pipe_root_round_is_its_definition() {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::math::Vec3;
    use solidcore::primitive::api_primitives::create_cylinder;
    const FORK: Fork = Fork {
        volume: 40307.88563787835,
        faces: &[&[
            [0.0, 29.428090415820638, 12.666666666666668],
            [4.733382275710106e-19, 29.31171126155042, 12.247007081058515],
            [
                2.6124984748312377e-17,
                30.646012679999725,
                10.106338480640954,
            ],
            [3.4694469519536167e-17, 31.313708498984766, 10.0],
            [18.000273, 29.428090415820638, 12.66666666666667],
            [18.000273, 29.31171126155042, 12.247007081058516],
            [18.000272999999993, 30.64601267999972, 10.106338480640954],
            [18.000272999999996, 31.313708498984763, 10.0],
            [30.00045499999999, 29.428090415820627, 12.666666666666664],
            [30.000455, 29.311711261550418, 12.247007081058515],
            [30.000455000000002, 30.64601267999972, 10.106338480640956],
            [30.000455, 31.313708498984756, 9.999999999999998],
            [60.00091000000002, 29.428090415820634, 12.66666666666667],
            [60.00091000000002, 29.311711261550418, 12.247007081058516],
            [60.000910000000005, 30.646012679999718, 10.106338480640954],
            [60.00091000000001, 31.31370849898476, 10.0],
        ]],
    };
    let mut k = kernel(true);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(&mut k, &mut pipe, &pl, None, false).unwrap();
    let mut b = boolean_union(&mut k, plate, pipe).unwrap();
    let e = pick(&k, &b, [30.0, 28.0, 10.0]);
    fillet_edges(&mut k, &mut b, &[e], 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
    check("pipe", &k, &mut b, &FORK, 60.0);
}
