//! tangency-labs/solidcore#1794 (WR-619): two 4 mm cubes, B turned atan(0.1) (5.71 deg)
//! about z, whose faces y = 20 (A) and F1 (B's y' = 0) stay within 5e-6 of each other
//! across the strip x in [1.9999, 2]. SMLib's union deletes two of A's faces (on y = 20 and
//! on its bottom z = 0) each between a solid region and the outside, so both solids merge
//! into the infinite region and the union has no solid region (as the pinned C++,
//! `boolean.api_brep` case `wr614_strip_union_d5e-06`). The opt-in
//! `Kernel::fixes.boolean_union_refuses_solid_leak` refuses such a union with `SM_ERR`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::SM_ERR;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, EntityRef, Kernel, SolidError};

const SIDE: f64 = 4.0;
const SLOPE: f64 = 0.1;
/// How far each strip edge lies off the other cube's face.
const D: f64 = 5e-6;

/// A: [-2, 2] x [16, 20] x [0, 4]. B: a 4 mm cube turned about z so that its face F1
/// crosses y = 20 at x = 2 - D / SLOPE, with its corner edge at x = 2 - 2 D / SLOPE,
/// D below y = 20 (the `wr614_coincident_strip.json` fixture's arithmetic).
fn operands(k: &mut Kernel) -> (Brep, Brep) {
    let a = create_box(k, Point3::new(-2.0, 16.0, 0.0), SIDE, SIDE, SIDE).unwrap();
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), SIDE, SIDE, SIDE).unwrap();
    let o = Point3::new(0.0, 0.0, 0.0);
    let deg = SLOPE.atan().to_degrees();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut b)),
        o,
        Vec3::new(0.0, 0.0, 1.0),
        deg,
    )
    .unwrap();
    let half = D / SLOPE;
    let shift = Vec3::new((2.0 - half) - half, 20.0 - D, 0.0);
    api_translate(k, Some(TransformObject::Brep(&mut b)), shift).unwrap();
    (a, b)
}

/// The regions other than the infinite one.
fn solid_regions(b: &Brep) -> usize {
    p1::regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|&r| Some(r) != b.infinite_region())
        .count()
}

#[test]
fn wr_619_union_refuses_solid_leak_is_off_by_default() {
    assert!(!Kernel::default().fixes.boolean_union_refuses_solid_leak);
}

#[test]
fn wr_619_upstream_union_has_no_solid() {
    let mut k = Kernel::default();
    let (a, b) = operands(&mut k);
    let u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(solid_regions(&u), 0);
    assert!(!is_manifold_solid(&u).unwrap());
}

#[test]
fn wr_619_union_with_the_fix_is_refused() {
    let mut k = Kernel::default();
    k.fixes.boolean_union_refuses_solid_leak = true;
    let (a, b) = operands(&mut k);
    match boolean_union(&mut k, a, b) {
        Err(SolidError::Upstream {
            code,
            entity: Some(EntityRef::Face(_)),
            ..
        }) => assert_eq!(code, SM_ERR),
        other => panic!("refused with SM_ERR at a face, got {other:?}"),
    }
}

/// Two overlapping boxes: the fix leaves an ordinary union alone.
#[test]
fn wr_619_fix_leaves_an_ordinary_union_alone() {
    let union = |fix: bool| {
        let mut k = Kernel::default();
        k.fixes.boolean_union_refuses_solid_leak = fix;
        let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 4.0, 4.0).unwrap();
        let b = create_box(&mut k, Point3::new(2.0, 1.0, 1.0), 4.0, 2.0, 2.0).unwrap();
        let u = boolean_union(&mut k, a, b).unwrap();
        (
            p1::faces(&u, None).unwrap().len(),
            p1::edges(&u, None).unwrap().len(),
            solid_regions(&u),
            is_manifold_solid(&u).unwrap(),
        )
    };
    let off = union(false);
    assert_eq!((off.2, off.3), (1, true));
    assert_eq!(union(true), off);
}
