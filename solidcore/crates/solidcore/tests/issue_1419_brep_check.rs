//! Regression tests for #1419: the whole-body `SmBrep::AssertValid` check behind the
//! capi `smlib_brep_check` (`check_brep` -> `BrepCheck`).
//!
//! Valid primitive bodies must report no failure at level 2, and bodies broken by a
//! direct edit must report the exact upstream report lines and `is_valid() == false`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::primitive::api_primitives as ap;
use solidcore::topology::brep::validation::{check_brep, BrepCheck, CheckLevel};
use solidcore::topology::Brep;
use solidcore::{Kernel, Vec3};

fn origin() -> Vec3 {
    Vec3::new(0.0, 0.0, 0.0)
}

fn bodies(k: &mut Kernel) -> Vec<(&'static str, Brep)> {
    vec![
        ("box", ap::create_box(k, origin(), 1.0, 2.0, 3.0).unwrap()),
        (
            "cylinder",
            ap::create_cylinder(k, origin(), 1.0, 2.0).unwrap(),
        ),
        ("sphere", ap::create_sphere(k, origin(), 1.0).unwrap()),
        ("torus", ap::create_torus(k, origin(), 3.0, 1.0).unwrap()),
    ]
}

fn check(k: &mut Kernel, b: &mut Brep) -> BrepCheck {
    check_brep(k, b, CheckLevel::Level2).unwrap()
}

/// Every non-empty body reaches `sm_AssertTestVertex1` and, at level 2,
/// `sm_AssertTestFace32/33/38` and `sm_AssertTestEdge13` (WP4, #1478).
#[test]
fn valid_bodies_report_no_failures_at_level_2() {
    let mut k = Kernel::default();
    for (name, mut b) in bodies(&mut k) {
        let c = check(&mut k, &mut b);
        assert_eq!(c.failure_count(), 0, "{name}: {:?}", c.lines());
        assert!(c.is_valid(), "{name}");
    }
}

/// Imported vertices can carry local gaps larger than 1000 body tolerances (#16183).
/// Four vertices reproduce the report's four failures without changing the exact box.
#[test]
fn local_vertex_tolerances_do_not_refuse_a_valid_body_or_its_cut() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::mass::integrator::properties::brep_volume_default;

    let mut k = Kernel::default();
    let mut b = ap::create_box(&mut k, origin(), 10.0, 10.0, 10.0).unwrap();
    b.set_tolerance(5e-6);
    let vertices = b.vertex_list().to_vec();
    let tolerances = [0.0056613767, 0.0056613767, 0.0055817271, 0.0055817271];
    for (&v, tolerance) in vertices.iter().zip(tolerances) {
        b.vertex_mut(v).unwrap().tolerance = tolerance;
    }
    let report = check(&mut k, &mut b);
    assert!(report.is_valid(), "{:?}", report.lines());
    assert_eq!(b.tolerance(), 5e-6);
    for (&v, tolerance) in vertices.iter().zip(tolerances) {
        assert_eq!(b.vertex(v).unwrap().tolerance, tolerance);
    }
    // A stored corner offset within the local uncertainty is valid as imported.
    let mut bounded = b;
    bounded.vertex_mut(vertices[0]).unwrap().point.x += 0.002;
    let (_, face_gap, _) = solidcore::topology::vertex::corner::is_within_xsect_tol_3d_of_faces(
        &k,
        &mut bounded,
        vertices[0],
        true,
    )
    .unwrap();
    assert!((face_gap - 0.002).abs() < 1e-9, "face gap {face_gap}");
    let report = check(&mut k, &mut bounded);
    assert!(report.is_valid(), "{:?}", report.lines());
    assert_eq!(
        bounded.vertex(vertices[0]).unwrap().tolerance,
        tolerances[0]
    );
    // Local uncertainty does not excuse an endpoint outside its vertex ball.
    let mut broken = ap::create_box(&mut k, origin(), 10.0, 10.0, 10.0).unwrap();
    broken.set_tolerance(5e-6);
    let broken_vertex = broken.vertex_list()[0];
    broken.vertex_mut(broken_vertex).unwrap().tolerance = tolerances[0];
    broken.vertex_mut(broken_vertex).unwrap().point.x += 0.1;
    for face in broken.face_list().to_vec() {
        let mut cache =
            solidcore::query::face::p2::face_surface_cache(&k, &mut broken, face).unwrap();
        let built =
            solidcore::query::face::p2::face_cache_tree(&k, &mut broken, face, &mut cache.cache);
        cache.release();
        built.unwrap();
    }
    let (_, face_gap, _) = solidcore::topology::vertex::corner::is_within_xsect_tol_3d_of_faces(
        &k,
        &mut broken,
        broken_vertex,
        true,
    )
    .unwrap();
    assert!((face_gap - 0.1).abs() < 1e-9, "face gap {face_gap}");
    // Measuring a cold face can heal its vertex while creating trim caches.
    // Restore the recorded bound to test a measured gap outside that bound.
    broken.vertex_mut(broken_vertex).unwrap().tolerance = tolerances[0];
    let report = check(&mut k, &mut broken);
    assert!(
        !report.is_valid(),
        "a displaced endpoint must still fail: tolerance {}, point {:?}, reports {:?}",
        broken.vertex(broken_vertex).unwrap().tolerance,
        broken.vertex(broken_vertex).unwrap().point,
        report.lines()
    );
    assert!(
        report
            .lines()
            .iter()
            .any(|line| line.contains("SmVertex::Test #1") || line.contains("SmBrep::Test #61")),
        "{:?}",
        report.lines()
    );
    assert_eq!(
        broken.vertex(broken_vertex).unwrap().tolerance,
        tolerances[0]
    );
    let tool = ap::create_cylinder(&mut k, Vec3::new(5.0, 5.0, -1.0), 1.5, 12.0).unwrap();
    let mut cut = boolean_difference(&mut k, bounded, tool).unwrap();
    let report = check(&mut k, &mut cut);
    assert!(report.is_valid(), "{:?}", report.lines());
    let expected = 1000.0 - std::f64::consts::PI * 1.5_f64.powi(2) * 10.0;
    let measured = brep_volume_default(&k, &mut cut, 1e-8).unwrap();
    assert!(
        (measured - expected).abs() < 1e-6,
        "expected {expected}, measured {measured}"
    );
    eprintln!("through cut: expected {expected}, measured {measured} mm^3");
}

fn box_body(k: &mut Kernel) -> Brep {
    ap::create_box(k, origin(), 1.0, 2.0, 3.0).unwrap()
}

#[test]
fn invalid_vertex_tolerances_are_still_refused() {
    let mut k = Kernel::default();
    for tolerance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e-8] {
        let mut b = box_body(&mut k);
        let v = b.vertex_list()[0];
        b.vertex_mut(v).unwrap().tolerance = tolerance;
        let result = check_brep(&mut k, &mut b, CheckLevel::Level2);
        match result {
            Ok(report) => {
                assert!(!report.is_valid(), "tolerance {tolerance}");
                assert!(
                    report
                        .lines()
                        .iter()
                        .any(|line| line.contains("Vertex tolerance")),
                    "{:?}",
                    report.lines()
                );
            }
            Err(error) => assert!(error.to_string().contains("vertex tolerance is not finite")),
        }
    }
}

/// A finite radius is local only while it remains clear of separate topology.
/// The origin corner's ball reaches the opposite x=1 face at radius 1.
#[test]
fn vertex_ball_must_not_reach_non_incident_topology() {
    for (radius, accepted) in [(0.1, true), (0.999, true), (1.0, false), (2.0, false)] {
        let mut k = Kernel::default();
        let mut b = box_body(&mut k);
        let v = b.vertex_list()[0];
        b.vertex_mut(v).unwrap().tolerance = radius;
        let result = check_brep(&mut k, &mut b, CheckLevel::Level2);
        if accepted {
            let report = result.unwrap();
            assert!(report.is_valid(), "radius {radius}: {:?}", report.lines());
            assert_eq!(b.vertex(v).unwrap().tolerance, radius);
        } else {
            let report = result.unwrap();
            assert!(!report.is_valid(), "radius {radius}");
            assert!(
                report
                    .lines()
                    .iter()
                    .any(|line| line.contains("Vertex tolerance reaches a non-adjacent face")),
                "radius {radius}: {:?}",
                report.lines()
            );
        }
    }
}

/// Same lines as the C++ capi sequence (difftest `topology.assert_valid`,
/// `box_mutation{2,9}_capi`).
type Mutation = Box<dyn Fn(&mut Brep)>;

#[test]
fn mutated_bodies_report_exact_lines() {
    let mut k = Kernel::default();
    let cases: Vec<(&str, Mutation, Vec<&str>)> = vec![
        // A finite local tolerance alone does not invalidate exact geometry.
        (
            "vertex tolerance 0.1",
            Box::new(|b: &mut Brep| { let v = b.vertex_list()[0]; b.vertex_mut(v).unwrap() }.tolerance = 0.1),
            vec![],
        ),
        (
            "edge tolerance 0.1",
            Box::new(|b: &mut Brep| { let e = b.edge_list()[0]; b.edge_mut(e).unwrap() }.tolerance = 0.1),
            vec![
                "SmBrep::Test #59 Tolerance: Edge tolerance relative to brep tolerance (offset=   0.1, tolerance=  0.01) [owner SmEdge]",
                "AssertValid skipped: ValidatePointers failed",
            ],
        ),
        (
            "edge without curve",
            Box::new(|b: &mut Brep| { let e = b.edge_list()[0]; b.edge_mut(e).unwrap() }.curve = None),
            vec![
                "SmBrep::Test #133 Null: Edge curve is non-Null [owner SmEdge]",
                "AssertValid skipped: ValidatePointers failed",
            ],
        ),
    ];
    for (name, mutate, want) in cases {
        let mut b = box_body(&mut k);
        mutate(&mut b);
        let c = check(&mut k, &mut b);
        assert_eq!(c.lines(), want, "{name}");
        assert_eq!(c.is_valid(), want.is_empty(), "{name}");
        assert_eq!(c.failure_count(), want.len(), "{name}");
    }
}

/// A sphere pole moved without notification: `ValidatePointers` reuses the trimmed
/// surface cache made while the sphere was built (as upstream does), so the moved
/// vertex keeps its tolerance and the face loop orientation and the seam edge fail.
#[test]
fn moved_sphere_pole_reports_upstream_lines() {
    let mut k = Kernel::default();
    let mut b = ap::create_sphere(&mut k, origin(), 1.0).unwrap();
    let pole = b.vertex_list()[0];
    b.vertex_mut(pole).unwrap().point.x += 0.1;
    let c = check(&mut k, &mut b);
    assert_eq!(
        c.lines(),
        vec![
            "SmTrimmingTools::Test #3 Loop Orientation: Face Loop has bad orientation [owner SmFace]",
            "SmTrimmingTools::Test #4 Loop Orientation: Face Loop has zero area [owner SmFace]",
            "SmBrep::Test #61 Valid: Bad Edge fails pE->ValidateGeometry(FALSE) check [owner SmEdge]",
            "AssertValid skipped: ValidatePointers failed",
        ]
    );
    assert!(!c.is_valid());
    assert!((b.vertex(pole).unwrap().tolerance - 1e-5).abs() < 1e-12);
}

/// A planar sheet, the square 0..4 in z = 0 (`create_planar_faces`), and its face.
fn square(k: &mut Kernel) -> (Brep, solidcore::topology::FaceId) {
    use solidcore::curve::curve::Curve;
    use solidcore::curve::line::Line;
    let corners = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(4.0, 4.0, 0.0),
        Vec3::new(0.0, 4.0, 0.0),
    ];
    let sides: Vec<Curve> = (0..4)
        .map(|i| {
            Curve::from(
                Line::create_line_segment(3, corners[i], corners[(i + 1) % 4], None).unwrap(),
            )
        })
        .collect();
    let b = solidcore::primitive::api_trim_ops::create_planar_faces(k, &sides).unwrap();
    let f = b.face_list()[0];
    (b, f)
}

/// #15040: a face whose edge uses have no UV trim curves is checked like any other, not
/// passed unchecked. A sound one passes; one whose outer loop runs backwards fails Loop
/// Orientation as it does with its trims.
#[test]
fn a_face_without_uv_trims_is_checked_like_one_with_them() {
    use solidcore::topology::face::p2 as fp2;
    use solidcore::topology::trimming_tools::{clear_uv_trim_curves, face_uv_curves_present};
    let mut k = Kernel::default();

    let (mut b, f) = square(&mut k);
    let with_trims = check(&mut k, &mut b);
    clear_uv_trim_curves(&mut b, f).unwrap();
    assert!(!face_uv_curves_present(&b, f).unwrap());
    let without = check(&mut k, &mut b);
    assert_eq!(without.lines(), with_trims.lines());
    assert!(without.is_valid(), "{:?}", without.lines());

    let (mut b, f) = square(&mut k);
    let outer = fp2::loops(&b, f, false, None).unwrap()[0];
    solidcore::topology::loopuse::flip_loop_orientation(&mut b, outer).unwrap();
    assert!(face_uv_curves_present(&b, f).unwrap());
    let with_trims = check(&mut k, &mut b);
    assert!(
        with_trims
            .lines()
            .iter()
            .any(|l| l.contains("Loop Orientation")),
        "{:?}",
        with_trims.lines()
    );
    clear_uv_trim_curves(&mut b, f).unwrap();
    let without = check(&mut k, &mut b);
    assert_eq!(without.lines(), with_trims.lines());
    assert!(!without.is_valid());
}
