//! The fork's own regression test for WR-536 (tangency-labs/smlib@67b8a1e, capi test
//! `ops_kernel`), ported: four r 2 disks in one plane at the corners of a 3 m sheet's hole
//! pattern, (30, 20), (1500, 20), (2970, 980) and (30, 980), joined in pairs with
//! `boolean_2d` and then all four. `Boolean2D` rebuilds the second operand's faces on a
//! copy of the first's plane after growing it to cover them, and a plane grew only 100
//! face widths. The fork's test requires every edge tolerance of the pairs and of the
//! four-disk join at most 1e-4, and the join's four faces. In the port the pairs keep
//! their tolerances either way, but the four-disk join fails by default with 1006, as the
//! fork's record says it did; with `boolean2d_plane_covers_far_faces` it is four faces,
//! every edge tolerance within 1e-4.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::{Brep, Kernel, SolidError};

/// A planar face bounded by the circle r 2 about (x, y, 0) (`smlib_planar_faces` of one
/// full arc: `SmApiCreatePlanarFaces` of an `SmCircle`).
fn disk(k: &mut Kernel, x: f64, y: f64) -> Brep {
    let frame = Axis2Placement::new(Point3::new(x, y, 0.0), Vec3::X, Vec3::Y);
    let circle = Ellipse::create_canonical_circle(k, &frame, 2.0, None, None).unwrap();
    create_planar_faces(k, &[Curve::from(circle)]).unwrap()
}

fn union_2d(k: &mut Kernel, a: Brep, b: Brep) -> solidcore::Result<Brep> {
    match boolean_2d(k, a, b, Boolean2d::Union)? {
        Boolean2dOutcome::Result(r) => Ok(r),
        Boolean2dOutcome::Unassigned { .. } => panic!("2D boolean left its operands"),
    }
}

fn worst_edge_tolerance(b: &Brep) -> f64 {
    b.edge_list()
        .iter()
        .map(|&e| b.edge(e).unwrap().tolerance)
        .fold(0.0_f64, f64::max)
}

/// The two pairs' (face count, worst edge tolerance) and the four-disk join, with WR-536's
/// `fixed`.
fn joins(fixed: bool) -> ([(usize, f64); 2], solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.boolean2d_plane_covers_far_faces = fixed;
    let d: Vec<Brep> = [(30.0, 20.0), (1500.0, 20.0), (2970.0, 980.0), (30.0, 980.0)]
        .iter()
        .map(|&(x, y)| disk(&mut k, x, y))
        .collect();
    let mut d = d.into_iter();
    let (a, b, c, e) = (
        d.next().unwrap(),
        d.next().unwrap(),
        d.next().unwrap(),
        d.next().unwrap(),
    );
    let low = union_2d(&mut k, a, b).unwrap();
    let high = union_2d(&mut k, c, e).unwrap();
    let pairs = [&low, &high].map(|p| (p.face_list().len(), worst_edge_tolerance(p)));
    let all = union_2d(&mut k, low, high);
    (pairs, all)
}

#[test]
fn fork_67b8a1e_four_far_disks_join_with_the_fix() {
    let (pairs, all) = joins(true);
    for (faces, worst) in pairs {
        assert_eq!(faces, 2);
        assert!(worst <= 1e-4, "a pair's edge tolerance of {worst}");
    }
    let all = all.unwrap();
    assert_eq!(all.face_list().len(), 4);
    let worst = worst_edge_tolerance(&all);
    assert!(worst <= 1e-4, "an edge tolerance of {worst}");
}

/// Upstream: the four-disk join fails with 1006.
#[test]
fn fork_67b8a1e_four_far_disks_join_fails_by_default() {
    let (_, all) = joins(false);
    assert!(
        matches!(all, Err(SolidError::Upstream { code: 1006, .. })),
        "{:?}",
        all.map(|b| b.face_list().len())
    );
}
