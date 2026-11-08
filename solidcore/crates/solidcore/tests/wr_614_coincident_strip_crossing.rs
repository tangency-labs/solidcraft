//! tangency-labs/solidcore#1794 (WR-614): two 4 mm cubes, B turned atan(0.1) (5.71 deg)
//! about z, whose faces y = 20 (A) and F1 (B's y' = 0) cross at x = 1.9999 and stay
//! within 1e-5 of each other across the strip x in [1.9998, 2]: B's corner edge lies
//! 1e-5 below A's face, A's corner edge 1e-5 off F1 (the default tolerances; at 5e-6 the
//! union happens to succeed, hitting WR-619 instead). The crossing is fixed only to
//! tol / sin, the whole strip, yet becomes an edge whose end vertices are picked per
//! body and never related, and the merge fails (`SmMerge::MakeFaceuse`, `SM_ERR`).
//!
//! `Kernel::fixes.boolean_coincident_strip_drops_crossing` drops that crossing before
//! `ResolveIntersections`. Paired with `Kernel::fixes.imprint_skips_collapsed_definition`
//! (WR-619, #2068), the strip union at d = 1e-5 succeeds as one valid manifold solid of
//! both cubes' combined volume.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::error::SM_ERR;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel, SolidError};

const SIDE: f64 = 4.0;
const SLOPE: f64 = 0.1;
const D: f64 = 1e-5;
const ACCURACY: f64 = 1e-6;

/// A: [-2, 2] x [16, 20] x [0, 4]. B: a 4 mm cube turned about z so that its face F1
/// crosses y = 20 at x = 2 - d / SLOPE, with its corner edge at x = 2 - 2 d / SLOPE,
/// d below y = 20.
fn strip_cubes(k: &mut Kernel, d: f64) -> (Brep, Brep) {
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
    let half = d / SLOPE;
    let shift = Vec3::new((2.0 - half) - half, 20.0 - d, 0.0);
    api_translate(k, Some(TransformObject::Brep(&mut b)), shift).unwrap();
    (a, b)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, ACCURACY, &attrs).unwrap()
}

/// Faces, edges, vertices, the regions other than the infinite one, and manifold.
fn shape(b: &Brep) -> (usize, usize, usize, usize, bool) {
    let solids = p1::regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|&r| Some(r) != b.infinite_region())
        .count();
    (
        p1::faces(b, None).unwrap().len(),
        p1::edges(b, None).unwrap().len(),
        p1::vertices(b, None).unwrap().len(),
        solids,
        is_manifold_solid(b).unwrap(),
    )
}

fn collapsed_edges(k: &Kernel, b: &Brep) -> usize {
    let mut n = 0;
    for e in p1::edges(b, None).unwrap() {
        let ends = edge::vertices(b, e, None).unwrap();
        if ends[0].is_none() || ends[0] != ends[1] {
            continue;
        }
        let c = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        let p = c.evaluate_point(k, ivl.min()).unwrap();
        let q = c.evaluate_point(k, ivl.max()).unwrap();
        if p.distance(q) > 1e-9 {
            n += 1;
        }
    }
    n
}

fn assert_valid(k: &mut Kernel, b: &mut Brep, what: &str) {
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert_eq!(check.failure_count(), 0, "{what}: {:?}", check.lines());
}

#[test]
fn wr_614_coincident_strip_drops_crossing_is_off_by_default() {
    let mut k = Kernel::default();
    assert!(!k.fixes.boolean_coincident_strip_drops_crossing);
    assert!(UpstreamFixes::NAMES.contains(&"boolean_coincident_strip_drops_crossing"));
    *k.fixes
        .flag_mut("boolean_coincident_strip_drops_crossing")
        .unwrap() = true;
    assert!(k.fixes.boolean_coincident_strip_drops_crossing);
}

#[test]
fn wr_614_union_fails_upstream() {
    let mut k = Kernel::default();
    let (a, b) = strip_cubes(&mut k, D);
    match boolean_union(&mut k, a, b) {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR),
        other => panic!("upstream's union fails with SM_ERR, got {other:?}"),
    }
}

#[test]
fn wr_614_union_with_strip_fix_alone_matches_5e6_shape() {
    let mut k = Kernel::default();
    let (a, b) = strip_cubes(&mut k, 5e-6);
    let want = shape(&boolean_union(&mut k, a, b).unwrap());
    assert_eq!(want.0, 12);

    let mut k = Kernel::default();
    k.fixes.boolean_coincident_strip_drops_crossing = true;
    let (a, b) = strip_cubes(&mut k, D);
    let u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(shape(&u), want);
}

#[test]
fn wr_614_and_wr_619_union_1e5_is_one_valid_solid() {
    let mut k = Kernel::default();
    k.fixes.boolean_coincident_strip_drops_crossing = true;
    k.fixes.imprint_skips_collapsed_definition = true;

    let (mut a, mut b) = strip_cubes(&mut k, D);
    assert_valid(&mut k, &mut a, "A");
    assert_valid(&mut k, &mut b, "B");

    let mut u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(shape(&u).3, 1, "one solid region");
    assert!(shape(&u).4, "manifold solid");
    assert_eq!(collapsed_edges(&k, &u), 0, "no closed edge on a speck");
    assert_valid(&mut k, &mut u, "union d=1e-5");

    let want = 2.0 * SIDE * SIDE * SIDE;
    let v = volume(&k, &mut u);
    assert!(
        (v - want).abs() <= ACCURACY * want,
        "volume {v}, want {want}"
    );
}

#[test]
fn wr_614_and_wr_619_all_booleans_at_1e5_and_5e6() {
    for d in [D, 5e-6] {
        let mut k = Kernel::default();
        k.fixes.boolean_coincident_strip_drops_crossing = true;
        k.fixes.imprint_skips_collapsed_definition = true;

        let (a, b) = strip_cubes(&mut k, d);
        let mut u = boolean_union(&mut k, a, b).unwrap();
        assert_eq!(shape(&u).3, 1, "union d = {d}: one solid");
        assert!(shape(&u).4, "union d = {d}: manifold");
        assert_valid(&mut k, &mut u, "union");
        let want_u = 2.0 * SIDE * SIDE * SIDE;
        let vu = volume(&k, &mut u);
        assert!((vu - want_u).abs() <= ACCURACY * want_u);

        let (a, b) = strip_cubes(&mut k, d);
        let mut diff = boolean_difference(&mut k, a, b).unwrap();
        assert_eq!(shape(&diff), (6, 12, 8, 1, true), "difference d = {d}");
        assert_valid(&mut k, &mut diff, "difference");
        let want_d = SIDE * SIDE * SIDE;
        let vd = volume(&k, &mut diff);
        assert!((vd - want_d).abs() <= ACCURACY * want_d);

        let (a, b) = strip_cubes(&mut k, d);
        let inter = boolean_intersection(&mut k, a, b).unwrap();
        assert_eq!(shape(&inter).0, 0, "intersection d = {d}: empty");
        assert_eq!(shape(&inter).3, 0, "intersection d = {d}: no solid");
    }
}

#[test]
fn wr_614_all_upstream_fixes_union_1e5_is_valid_solid() {
    let mut fixes = UpstreamFixes::all();
    fixes.geometric_g2_continuity = false;
    let mut k = Kernel {
        fixes,
        ..Kernel::default()
    };

    let (a, b) = strip_cubes(&mut k, D);
    let mut u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(shape(&u).3, 1, "one solid region");
    assert!(shape(&u).4, "manifold solid");
    assert_eq!(collapsed_edges(&k, &u), 0, "no closed edge on a speck");
    assert_valid(&mut k, &mut u, "UpstreamFixes::all() union d=1e-5");

    let want = 2.0 * SIDE * SIDE * SIDE;
    let v = volume(&k, &mut u);
    assert!(
        (v - want).abs() <= ACCURACY * want,
        "volume {v}, want {want}"
    );
}

#[test]
fn wr_614_imported_tolerances_union_and_intersection_at_5e6() {
    // Issue #1794's imported tolerances: B-rep/face 1e-5 (so SmMerge's intersection
    // tolerance is 0.5 * 1e-5 = 5e-6), edge 1e-5, vertex 1.5e-5. Both inputs pass
    // Level-2 validation.
    let set_imported_tols = |k: &mut Kernel, a: &mut Brep, b: &mut Brep| {
        for body in [&mut *a, &mut *b] {
            for e in p1::edges(body, None).unwrap() {
                edge::set_tolerance(body, e, 1e-5, false, false).unwrap();
            }
            for v in p1::vertices(body, None).unwrap() {
                vertex::set_tolerance(body, v, 1.5e-5, false, false).unwrap();
            }
            assert_valid(k, body, "imported operand");
        }
    };

    // Without the fixes, imported tolerances lose the solid region at d = 5e-6 (WR-619)
    // and fail MakeFaceuse with SM_ERR at d = 1e-5 (WR-614).
    let mut k = Kernel::default();
    let (mut a, mut b) = strip_cubes(&mut k, 5e-6);
    set_imported_tols(&mut k, &mut a, &mut b);
    let broken = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(
        shape(&broken).3,
        0,
        "no solid region without fixes at d=5e-6"
    );

    let mut k = Kernel::default();
    let (mut a, mut b) = strip_cubes(&mut k, 1e-5);
    set_imported_tols(&mut k, &mut a, &mut b);
    match boolean_union(&mut k, a, b) {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR),
        other => {
            panic!("expected SM_ERR without fix at imported tolerances d=1e-5, got {other:?}")
        }
    }

    for d in [4.95e-6, 5e-6, 1e-5] {
        let mut k = Kernel::default();
        k.fixes.boolean_coincident_strip_drops_crossing = true;
        k.fixes.imprint_skips_collapsed_definition = true;

        let (mut a, mut b) = strip_cubes(&mut k, d);
        set_imported_tols(&mut k, &mut a, &mut b);
        let mut u = boolean_union(&mut k, a, b).unwrap();
        assert_eq!(shape(&u).3, 1, "d={d}: one solid");
        assert!(shape(&u).4, "d={d}: manifold");
        assert_eq!(collapsed_edges(&k, &u), 0, "d={d}: no speck edge");
        assert_valid(&mut k, &mut u, "imported tolerances union");
        let want = 2.0 * SIDE * SIDE * SIDE;
        let v = volume(&k, &mut u);
        assert!((v - want).abs() <= ACCURACY * want);

        let (mut a, mut b) = strip_cubes(&mut k, d);
        set_imported_tols(&mut k, &mut a, &mut b);
        let inter = boolean_intersection(&mut k, a, b).unwrap();
        assert_eq!(shape(&inter).0, 0, "d={d}: empty intersection");
        assert_eq!(shape(&inter).3, 0, "d={d}: no solid region");
    }
}

/// When A continues well past the crossing ([-2, 10] instead of [-2, 2]) so B emerges
/// out of A's y = 20 face up to y ~ 20.4, the crossing at x = 1.9999 is a real exit
/// edge, not a two-sided coincident strip, and must not be dropped.
#[test]
fn wr_614_wider_protruding_box_keeps_exit_crossing() {
    let run = |strip_fix: bool| {
        let mut k = Kernel::default();
        k.fixes.boolean_coincident_strip_drops_crossing = strip_fix;
        k.fixes.imprint_skips_collapsed_definition = true;

        let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), 12.0, SIDE, SIDE).unwrap();
        let (_, b) = strip_cubes(&mut k, D);
        let mut u = boolean_union(&mut k, a, b).unwrap();
        assert_eq!(shape(&u).3, 1, "one solid region");
        assert!(shape(&u).4, "manifold solid");
        assert_valid(&mut k, &mut u, "wider box union");
        let vu = volume(&k, &mut u);
        assert!(
            vu > 12.0 * SIDE * SIDE + 0.5,
            "protruding wedge must add volume above box A (192), got {vu}"
        );

        let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), 12.0, SIDE, SIDE).unwrap();
        let (_, b) = strip_cubes(&mut k, D);
        let inter = boolean_intersection(&mut k, a, b).unwrap();
        assert_eq!(shape(&inter).3, 1, "one intersection solid");
        assert!(shape(&inter).4, "manifold intersection");
        (shape(&u), vu, shape(&inter))
    };

    let without = run(false);
    let with = run(true);
    assert_eq!(with.0, without.0);
    assert!((with.1 - without.1).abs() <= ACCURACY * without.1);
    assert_eq!(with.2, without.2);
}

/// Two boxes whose faces cross at 45 deg near x = 0 while both faces extend toward +x
/// (away from the crossing on A's side, so A's inward binormal points away from the
/// crossing) keep their crossing unchanged.
#[test]
fn wr_614_diverging_same_side_boxes_keep_crossing() {
    let run = |fix: bool| {
        let mut k = Kernel::default();
        k.fixes.boolean_coincident_strip_drops_crossing = fix;
        let a = create_box(&mut k, Point3::new(8e-6, -2.0, 0.0), SIDE, SIDE, SIDE).unwrap();
        let mut b = create_box(&mut k, Point3::new(0.0, -2.0, 0.0), SIDE, SIDE, SIDE).unwrap();
        api_rotate(
            &mut k,
            Some(TransformObject::Brep(&mut b)),
            Point3::ZERO,
            Vec3::Z,
            45.0,
        )
        .unwrap();
        api_translate(
            &mut k,
            Some(TransformObject::Brep(&mut b)),
            Vec3::new(-8e-6, 2.0, 0.0),
        )
        .unwrap();
        let mut u = boolean_union(&mut k, a, b).unwrap();
        (shape(&u), volume(&k, &mut u))
    };
    let off = run(false);
    let on = run(true);
    assert_eq!(on.0, off.0);
    assert!((on.1 - off.1).abs() <= ACCURACY * off.1);
}
