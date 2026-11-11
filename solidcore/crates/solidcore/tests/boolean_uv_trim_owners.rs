//! Boolean results' UV trim curves are owned by the edge use holding them
//! (`SmEdgeuse::AssertValid` test #6), as SMLib's are: the edge uses `SmBrep::MergeBrep`
//! copies during the merge own their trims under the merge's notification namespace,
//! which the merge hands back to the B-rep's naming with the faces' and edges' geometry.
//! SMLib's check of these results is clean (the `boolean.api_brep` `test6_*` fixtures).

#![allow(clippy::unwrap_used)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::{Kernel, Point3};

fn test_6_lines(k: &mut Kernel, mut b: Brep) -> Vec<String> {
    check_brep(k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .into_iter()
        .filter(|l| l.contains("Test #6 "))
        .collect()
}

#[test]
fn box_union_trims_are_owned_by_their_edge_uses() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let b = create_box(&mut k, Point3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    let r = boolean_union(&mut k, a, b).unwrap();
    let lines = test_6_lines(&mut k, r);
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn crescent_union_trims_are_owned_by_their_edge_uses() {
    let mut k = Kernel::default();
    let small = create_cylinder(&mut k, Point3::new(10.0, 0.0, 0.0), 6.0, 10.0).unwrap();
    let big = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
    let crescent = boolean_difference(&mut k, small, big).unwrap();
    let cylinder = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
    let r = boolean_union(&mut k, cylinder, crescent).unwrap();
    let lines = test_6_lines(&mut k, r);
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn flat_hole_trims_are_owned_by_their_edge_uses() {
    let mut k = Kernel::default();
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(20.0, 20.0, 10.0), 3.0, 11.0).unwrap();
    let r = boolean_difference(&mut k, block, hole).unwrap();
    let lines = test_6_lines(&mut k, r);
    assert!(lines.is_empty(), "{lines:?}");
}
