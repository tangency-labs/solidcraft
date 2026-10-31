//! WR-656: `SmFeatureExecutive::DoDefeature` of a feature whose base patch cannot be built
//! (one face of a 30 x 20 x 10 box, `SM_RB_EXTEND_SURFACE` / `SM_RB_FROM_OTHER_BREP`: the
//! four side faces extended do not close the top) applies no patch and returns success
//! with the face cut out and the hole open: 5 faces, not a manifold solid, volume 0 (as
//! the pinned C++). The opt-in `Kernel::fixes.defeature_refuses_open_base` refuses it
//! with `SM_ERR` at a loop edge and puts the box back. Features whose patch is built (a
//! plate's hole, a rim round, a corner ball of a rounded box) come off as before.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::volume;
use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::SM_ERR;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn box_30_20_10(k: &mut Kernel) -> Brep {
    create_box(k, p(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap()
}

/// A 40 x 30 x 5 plate with a through hole of radius 5.
fn plate_hole(k: &mut Kernel) -> Brep {
    let a = create_box(k, p(0.0, 0.0, 0.0), 40.0, 30.0, 5.0).unwrap();
    let c = create_cylinder(k, p(20.0, 15.0, -1.0), 5.0, 7.0).unwrap();
    boolean_difference(k, a, c).unwrap()
}

/// The box with every edge rounded r 3.
fn box_fillet3(k: &mut Kernel) -> Brep {
    let mut b = box_30_20_10(k);
    let edges = p1::edges(&b, None).unwrap();
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    b
}

/// A cylinder of radius 10, height 20, both rims rounded r 2.
fn cyl_rim_fillet2(k: &mut Kernel) -> Brep {
    let mut b = create_cylinder(k, p(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    let rims: Vec<_> = p1::edges(&b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::is_closed(&b, e).unwrap())
        .collect();
    fillet_edges(k, &mut b, &rims, 2.0, 1, 1, 1.0).unwrap();
    b
}

/// A body builder.
type Make = fn(&mut Kernel) -> Brep;

/// The body's faces, whether it is a manifold solid, and its volume.
type Shape = (usize, bool, f64);

fn shape(k: &Kernel, b: &mut Brep) -> Shape {
    b.refresh_cached_topology();
    (
        p1::faces(b, None).unwrap().len(),
        is_manifold_solid(b).unwrap(),
        volume(k, b).unwrap(),
    )
}

/// Defeatures face `face` (index in the face list) of `make`'s body, as the matrix does;
/// the shape before, `do_defeature`'s result and the shape after.
fn defeature(fix: bool, make: Make, face: usize) -> (Shape, solidcore::Result<()>, Shape) {
    let mut k = Kernel::default();
    k.fixes.defeature_refuses_open_base = fix;
    let mut b = make(&mut k);
    let before = shape(&k, &mut b);
    let f = p1::faces(&b, None).unwrap()[face];
    let r = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        ex.add_feature(
            &mut k,
            &[f],
            RebuildBrepType::ExtendSurface,
            RebuildBrepType::FromOtherBrep,
        )
        .unwrap()
        .unwrap();
        let r = ex.do_defeature(&mut k);
        if r.is_err() {
            assert!(ex.features().is_empty());
        }
        r
    };
    let after = shape(&k, &mut b);
    (before, r, after)
}

#[test]
fn wr_656_defeature_refuses_open_base_is_off_by_default() {
    assert!(!Kernel::default().fixes.defeature_refuses_open_base);
}

#[test]
fn wr_656_upstream_defeature_leaves_the_box_open() {
    let (_, r, after) = defeature(false, box_30_20_10, 0);
    r.unwrap();
    assert_eq!(after.0, 5);
    assert!(!after.1);
    assert!(after.2.abs() < 1e-6, "volume {}", after.2);
}

#[test]
fn wr_656_defeature_with_the_fix_refuses_and_keeps_the_box() {
    for face in 0..6 {
        let (before, r, after) = defeature(true, box_30_20_10, face);
        match r {
            Err(SolidError::Upstream {
                code,
                entity: Some(EntityRef::Edge(_)),
                ..
            }) => assert_eq!(code, SM_ERR),
            other => panic!("face {face}: refused with SM_ERR at an edge, got {other:?}"),
        }
        assert_eq!(after, before, "face {face}");
        assert_eq!(after.0, 6);
        assert!(after.1);
        assert!((after.2 - 6000.0).abs() < 1e-6, "volume {}", after.2);
    }
}

/// Features whose patch is built: the result is the flag-off one, with the volume the
/// matrix measured.
#[test]
fn wr_656_fix_leaves_patched_defeatures_alone() {
    let cases: [(Make, usize, f64); 3] = [
        // the hole's wall: the plate filled in
        (plate_hole, 6, 6000.0),
        // a corner ball of the rounded box
        (box_fillet3, 6, 5574.29843),
        // a rim round of the cylinder
        (cyl_rim_fillet2, 3, 6231.65916),
    ];
    for (make, face, want) in cases {
        let (_, off_r, off) = defeature(false, make, face);
        let (_, on_r, on) = defeature(true, make, face);
        off_r.unwrap();
        on_r.unwrap();
        assert_eq!((on.0, on.1), (off.0, off.1), "face {face}");
        assert!(
            (on.2 - off.2).abs() < 1e-9 * off.2,
            "face {face}: {on:?} vs {off:?}"
        );
        assert!(on.1, "face {face}");
        assert!((on.2 - want).abs() < 1e-4, "face {face}: volume {}", on.2);
    }
}
