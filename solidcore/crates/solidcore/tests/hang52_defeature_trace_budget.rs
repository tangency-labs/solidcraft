//! HANG-52: `FeatureExecutive::do_defeature` (`ExtendSurface`, `FromOtherBrep`) on face
//! 14 of a 30x20x10 box with every edge filleted at r3 takes about 16 minutes in release (11 in C++):
//! the start-point loop of the extended surface's intersection traces 40+ start points,
//! each creeping toward a surface boundary for ~15,000 steps. With
//! `fixes.ssi_start_loop_trace_budget` the loop's trace steps are bounded together, and
//! the call returns in under a minute. Its assertions do not show the bound fired: the
//! default run returns the same face and edge counts.
#![allow(clippy::unwrap_used)]

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::Kernel;

// Release: under a minute; a dev build: 27 min, too slow for `cargo test`. Only its
// running time tells the bound fired. The regression of the wiring is
// `do_intersection_bounds_its_start_point_loop_traces_when_opted_in` in
// `intersect/surface_intersector/p5.rs` (the step itself:
// `trace_steps_are_bounded_by_the_start_point_loop_budget` in `p4.rs`). Run this with
// `--release -- --ignored`.
#[test]
#[ignore = "under a minute in release, 27 min in a dev build"]
fn opted_in_defeature_of_a_filleted_box_face_returns_the_same_body() {
    let mut k = Kernel::default();
    k.fixes.ssi_start_loop_trace_budget = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let edges = p1::edges(&b, None).unwrap();
    fillet_edges(&mut k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    let face = b.face_list()[14];
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[face],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::FromOtherBrep,
    )
    .unwrap();
    // the status is upstream's (`ManifoldBoolean`'s failed intersection is ignored)
    ex.do_defeature(&mut k).unwrap();
    b.refresh_cached_topology();
    // what the 16-minute default run returns
    assert_eq!(b.face_list().len(), 25);
    assert_eq!(p1::edges(&b, None).unwrap().len(), 48);
}
