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

fn box_body(k: &mut Kernel) -> Brep {
    ap::create_box(k, origin(), 1.0, 2.0, 3.0).unwrap()
}

/// Same lines as the C++ capi sequence (difftest `topology.assert_valid`,
/// `box_mutation{1,2,9}_capi`).
type Mutation = Box<dyn Fn(&mut Brep)>;

#[test]
fn mutated_bodies_report_exact_lines() {
    let mut k = Kernel::default();
    let cases: Vec<(&str, Mutation, Vec<&str>)> = vec![
        (
            "vertex tolerance 0.1",
            Box::new(|b: &mut Brep| { let v = b.vertex_list()[0]; b.vertex_mut(v).unwrap() }.tolerance = 0.1),
            vec![
                "SmBrep::Test #82 Tolerance: Vertex tolerance relative to brep tolerance (offset=   0.1, tolerance=  0.01) [owner SmVertex]",
                "AssertValid skipped: ValidatePointers failed",
            ],
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
        assert!(!c.is_valid(), "{name}");
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
