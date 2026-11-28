//! tangency-labs/solidcore#1794 (WR-619): two 4 mm cubes, B turned atan(0.1) (5.71 deg)
//! about z, whose faces y = 20 (A) and F1 (B's y' = 0) cross with B's corner edge 1.5e-5
//! below A's face. Upstream's union deleted faces of A each between a solid region and the
//! outside, so both solids merged into the infinite region and the union had no solid region
//! (`boolean.api_brep` case `wr614_strip_union_d1.5e-05`). 1.5e-5 is past the 1e-5 zone
//! tolerance but inside the intersection tolerance (their sum, 2e-5), the kernel's identity
//! radius: since #15718 decides B's corner on A's face as one event (`split_edges_at_vertices`)
//! the union is the narrow strip's closed form, one valid solid of volume 128 - 20.2 d^2, as
//! `wr_614_coincident_strip_crossing.rs` derives for d <= 1e-5. The union's WR-619 guard
//! (`boolean/merge/p4.rs`, refusing a union where the solid side of a deleted face between a
//! solid and the outside ends in no solid region) then
//! has no leak to refuse and leaves the union as it is.
//!
//! A union that still leaks is refused: tangency-labs/solidcraft#16350's wider A with a
//! pocket beside the crossing, at d = 5e-6, deletes faces until neither solid is left (0 solid
//! regions, volume 0). A union of two non-empty solids contains each of them, so a result with
//! no solid region is wrong whatever its faces; the guard refuses it with
//! `BooleanFailure::SolidLeaks` instead of returning it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::error::boolean::BooleanFailure;
use solidcore::error::Reason;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::det;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

const SIDE: f64 = 4.0;
const SLOPE: f64 = 0.1;
/// How far each strip edge lies off the other cube's face.
const D: f64 = 1.5e-5;

/// A: [-2, 2] x [16, 20] x [0, 4]. B: a 4 mm cube turned about z so that its face F1
/// crosses y = 20 at x = 2 - D / SLOPE, with its corner edge at x = 2 - 2 D / SLOPE,
/// D below y = 20 (the `wr614_coincident_strip.json` fixture's arithmetic).
fn operands(k: &mut Kernel) -> (Brep, Brep) {
    let a = create_box(k, Point3::new(-2.0, 16.0, 0.0), SIDE, SIDE, SIDE).unwrap();
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), SIDE, SIDE, SIDE).unwrap();
    let o = Point3::new(0.0, 0.0, 0.0);
    let deg = det::atan(SLOPE).to_degrees();
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

/// The union is one valid solid of the narrow strip's closed-form volume: the overlap of A
/// (64) and B (64) is the prism over P0 Q4 Q1, 4 high, of volume 20.2 d^2
/// (`wr_614_coincident_strip_crossing.rs`, `overlap_volume`), and every check of Level 2 (all
/// of them) passes. Vertex placement within the tolerance moves the volume by far less than
/// 1e-6 of it.
fn assert_one_valid_solid(k: &mut Kernel, u: &mut Brep, what: &str) {
    assert_eq!(solid_regions(u), 1, "{what}: one solid region");
    assert!(is_manifold_solid(u).unwrap(), "{what}: manifold");
    let check = check_brep(k, u, CheckLevel::Level2).unwrap();
    assert_eq!(check.failure_count(), 0, "{what}: {:?}", check.lines());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(k, u, 1e-6, &attrs).unwrap();
    let want = 2.0 * SIDE * SIDE * SIDE - 20.2 * D * D;
    assert!(
        (v - want).abs() <= 1e-6 * want,
        "{what}: volume {v}, want {want}"
    );
}

#[test]
fn wr_619_union_is_one_valid_solid_by_default() {
    let mut k = Kernel::default();
    let (a, b) = operands(&mut k);
    let mut u = boolean_union(&mut k, a, b).unwrap();
    assert_one_valid_solid(&mut k, &mut u, "default");
}

/// Two overlapping boxes: the guard leaves an ordinary union alone. The bar [2, 6] x [1, 3]
/// x [1, 3] runs out of the cube [0, 4]^3 through its face x = 4: the union is the cube's 6
/// faces (x = 4 with a square hole) and the bar's 5 outside faces, with the cube's 12 edges,
/// the hole's 4, the bar's 4 along x and its end's 4.
#[test]
fn wr_619_guard_leaves_an_ordinary_union_alone() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 4.0, 4.0).unwrap();
    let b = create_box(&mut k, Point3::new(2.0, 1.0, 1.0), 4.0, 2.0, 2.0).unwrap();
    let u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(p1::faces(&u, None).unwrap().len(), 11, "faces");
    assert_eq!(p1::edges(&u, None).unwrap().len(), 24, "edges");
    assert_eq!(solid_regions(&u), 1);
    assert!(is_manifold_solid(&u).unwrap());
}

/// tangency-labs/solidcraft#16350's pocketed case: A = [-2, 10] x [16, 20] x [0, 4] less the
/// pocket [2, 3] x [19, 21] x `cut` (a hole in A's face y = 20 for (0.5, 3.5), a notch in its
/// outer loop for (2, 5)), B a 4 x 4 x `h` box turned atan(0.1) about z and placed at z0 as in
/// `operands`, its corner edge `d` below y = 20.
fn pocketed_operands(k: &mut Kernel, cut: (f64, f64), z0: f64, h: f64, d: f64) -> (Brep, Brep) {
    let a = create_box(k, Point3::new(-2.0, 16.0, 0.0), 12.0, SIDE, SIDE).unwrap();
    let p = create_box(k, Point3::new(2.0, 19.0, cut.0), 1.0, 2.0, cut.1 - cut.0).unwrap();
    let a = boolean_difference(k, a, p).unwrap();
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), SIDE, SIDE, h).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut b)),
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        det::atan(SLOPE).to_degrees(),
    )
    .unwrap();
    let half = d / SLOPE;
    let shift = Vec3::new((2.0 - half) - half, 20.0 - d, z0);
    api_translate(k, Some(TransformObject::Brep(&mut b)), shift).unwrap();
    (a, b)
}

/// The #16350 unions at d = 5e-6 delete faces between a solid and the outside until no solid
/// region is left. Without the guard they return that region-less body; with it they are
/// refused, naming a face the deletion would have removed.
#[test]
fn wr_619_union_leaking_both_solids_is_refused() {
    for (cut, z0, h) in [((0.5, 3.5), 1.0, 2.0), ((2.0, 5.0), 2.5, 1.0)] {
        let mut k = Kernel::default();
        let (a, b) = pocketed_operands(&mut k, cut, z0, h, 5e-6);
        let e = match boolean_union(&mut k, a, b) {
            Ok(u) => panic!(
                "pocket {cut:?}: union returned a body with {} solid regions",
                solid_regions(&u)
            ),
            Err(e) => e,
        };
        assert!(
            matches!(
                e.failure().map(|f| &f.reason),
                Some(Reason::Boolean(BooleanFailure::SolidLeaks { .. }))
            ),
            "pocket {cut:?}: {e:?}"
        );
    }
}

/// A partial leak: the #16350 pocketed union with a separate 2 mm cube far from both
/// operands added to one of them (A, then B), so the operand has two disjoint solids. The
/// pocketed strips' face deletion still merges their solid into the outside, while the cube
/// stays a solid region of its own: a region count sees a solid left, yet the union has lost
/// A and B. The guard checks each leak: the solid side of every deleted face between a solid
/// and the outside must still lie in a solid region, so the union is refused.
#[test]
fn wr_619_union_leaking_one_of_two_solids_is_refused() {
    for (cut, z0, h) in [((0.5, 3.5), 1.0, 2.0), ((2.0, 5.0), 2.5, 1.0)] {
        for cube_in_a in [true, false] {
            let mut k = Kernel::default();
            let (a, b) = pocketed_operands(&mut k, cut, z0, h, 5e-6);
            let c = create_box(&mut k, Point3::new(40.0, 40.0, 0.0), 2.0, 2.0, 2.0).unwrap();
            let (a, b) = if cube_in_a {
                (boolean_union(&mut k, a, c).unwrap(), b)
            } else {
                (a, boolean_union(&mut k, b, c).unwrap())
            };
            let with_cube = if cube_in_a { &a } else { &b };
            assert_eq!(
                solid_regions(with_cube),
                2,
                "pocket {cut:?}: two disjoint solids"
            );
            let e = match boolean_union(&mut k, a, b) {
                Ok(mut u) => {
                    let store = AttributeStore::default();
                    let attrs = PropertyAttributes {
                        store: &store,
                        key_of: &|_| ObjectKey(0),
                    };
                    let v = brep_volume(&k, &mut u, 1e-6, &attrs).unwrap_or(f64::NAN);
                    panic!(
                        "pocket {cut:?}, cube in A {cube_in_a}: union returned a body with {} \
                         solid regions, volume {v}",
                        solid_regions(&u)
                    )
                }
                Err(e) => e,
            };
            assert!(
                matches!(
                    e.failure().map(|f| &f.reason),
                    Some(Reason::Boolean(BooleanFailure::SolidLeaks { .. }))
                ),
                "pocket {cut:?}, cube in A {cube_in_a}: {e:?}"
            );
        }
    }
}
