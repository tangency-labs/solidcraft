//! tangency-labs/solidcore#2369 (smlib#104, WR-616): a 20 mm box with the two top edges at
//! (20, 0, 20) rounded r 2 and the upright edge there r 3, all three in one fillet. The rounds
//! are of unequal radii, so no one ball touches them. With `fillet_blend_corner` the corner is
//! a Setback (the fork's `SmBlendCorner::ClassifySetBack`, tangency-labs/smlib `b5bb747`),
//! set back by the rule and filled G1: 15 faces. Without the flag it is the pinned
//! `SmFilletConvexNxNCorner`: 10 faces.
//!
//! The original fork b5bb747, run through `SmApiFilletEdgesPerEdge` (circular,
//! G1, thumbweight 1), gave 7928.142563966 with 15 faces. The thousandth-grid
//! search (20b1c01, #2344a) gives 7927.724282512234, independently checked by
//! `harness/cpp/tests/blend_setback_executive.cpp`. With the flag off the
//! value is this port's pinned corner, 7928.743041465 with 10 faces; the pinned C++ path
//! is gated by difftest. SolidCraft's engine case
//! `a_later_round_at_a_mitred_corner_is_blended_with_the_rounds_there` pins the old value,
//! 7928.743041, and is re-pinned there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletCornerType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};

/// Every opt-in fix on, with `fillet_blend_corner` (WR-616) on or off.
fn kernel(blend: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k.fixes.fillet_blend_corner = blend;
    k
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The box rounded (2, 2, 3) in one fillet: the corner at (20, 0, 20) as made, the volume
/// (before `check_brep`, which may refine tolerances), the face count.
fn rounded(blend: bool) -> ((FilletCornerType, Option<BlendCornerEvent>), f64, usize) {
    let mut k = kernel(blend);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let es: Vec<EdgeId> = [[20.0, 10.0, 20.0], [10.0, 0.0, 20.0], [20.0, 0.0, 10.0]]
        .iter()
        .map(|p| pick(&k, &b, *p))
        .collect();
    let corner = Point3::new(20.0, 0.0, 20.0);
    let made = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &es,
            &[2.0, 2.0, 3.0],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        let mut made = None;
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            let at = fc.vertex.map(|v| s.target().vertex(v).unwrap().point);
            if at.is_some_and(|p| p.distance(corner) < 1e-9) {
                made = Some((fc.corner_type(), fc.blend.as_ref().map(|b| b.event)));
            }
        }
        s.do_filleting(&mut k, None).unwrap();
        made.unwrap()
    };
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-8, &attrs).unwrap();
    assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .is_valid());
    (made, v, b.faces().ids().count())
}

fn assert_close(v: f64, want: f64) {
    assert!(
        (v - want).abs() <= 1e-9 * v.abs().max(want.abs()),
        "volume {v:.12}, want {want:.12}"
    );
}

#[test]
fn unequal_rounds_at_a_box_corner_are_the_setback() {
    let (made, v, faces) = rounded(true);
    assert_eq!(made.1, Some(BlendCornerEvent::Setback));
    assert_eq!(faces, 15);
    // Native SMLib 20b1c01 thousandth-grid search, independently built by
    // blend_setback_executive.cpp (20 20 20 2 2 3 -1 3).
    assert_close(v, 7927.724282512234);
}

#[test]
fn unequal_rounds_at_a_box_corner_without_the_flag_are_the_pinned_corner() {
    let (made, v, faces) = rounded(false);
    assert_eq!(made, (FilletCornerType::NByNConvex, None));
    assert_eq!(faces, 10);
    assert_close(v, 7928.743041465);
}
