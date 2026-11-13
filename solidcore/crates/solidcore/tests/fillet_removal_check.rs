//! `SmBrep::DeleteFilletFace` of a four-sided fillet leaves a B-rep the check finds clean:
//! the sharp edge's new curve is owned under its edge's `Brep::owner_identity`, the
//! identity `check_brep` compares (Test #61, `ValidateGeometry(FALSE)`, "Bad Edge"
//! otherwise).
//!
//! The case: a 20 mm cube with its top front edge rounded r 2, the round removed: the
//! sharp cube again, six faces, twelve edges, check clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_removal::delete_fillet_face;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex};
use solidcore::Kernel;

#[test]
fn removing_a_four_sided_round_leaves_a_clean_brep() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let e = b
        .edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s, t].into_iter().flatten().all(|v| {
                let p = vertex::point(&b, v).unwrap();
                p.y == 0.0 && p.z == 20.0
            })
        })
        .unwrap();
    fillet_edges(&mut k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
    let round = *b
        .face_list()
        .iter()
        .find(|&&f| {
            !face2::surface(&b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
        })
        .unwrap();
    b.set_editing_enabled(true);
    delete_fillet_face(&mut k, &mut b, round, None).unwrap();
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(b.edges().ids().count(), 12);
    assert!(brep_is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}
