//! tangency-labs/solidcore#1794 (WR-614): two 4 mm cubes, B turned atan(0.1) (5.71 deg)
//! about z, whose faces y = 20 (A) and F1 (B's y' = 0) cross at x = 1.9999 and stay
//! within 1e-5 of each other across the strip x in [1.9998, 2]: B's corner edge lies
//! 1e-5 below A's face, A's corner edge 1e-5 off F1 (the default tolerances; at 5e-6 the
//! union happens to succeed, hitting WR-619 instead). The crossing is fixed only to
//! tol / sin, the whole strip, yet upstream makes it an edge whose end vertices are picked
//! per body and never related, and the merge fails (`SmMerge::MakeFaceuse`, `SM_ERR`).
//!
//! solidcore makes no curve for that crossing: before the two faces are intersected, both
//! sides of their crossing are bounded by an edge within tolerance of the other plane, so
//! the crossing lies inside a coincident pair of their pieces and is no event
//! (`topology_intersector::coincident_strip`, which replaced WR-614's removal after the
//! fact). B's corner on A's edge, and A's corner on B's, are events decided once before
//! intersecting, and the two edges between them are one coincident interval, where their
//! crossing is no event either. With the speck definition skipped at imprint (WR-619,
//! #2068), the strip union at d = 1e-5 is one valid manifold solid of both cubes' combined
//! volume. All of it is the default (the opt-in flags `boolean_coincident_strip_drops_crossing`
//! and `imprint_skips_collapsed_definition` were retired). The exact overlap of A and B is a
//! wedge of volume 20.2 d^2 (2.02e-9 at d = 1e-5), below the accuracy asked of the volume
//! integrator.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
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
use solidcore::{Brep, Kernel};

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
fn wr_614_union_1e5_matches_5e6_shape() {
    let mut k = Kernel::default();
    let (a, b) = strip_cubes(&mut k, 5e-6);
    let want = shape(&boolean_union(&mut k, a, b).unwrap());
    assert_eq!(want.0, 10);

    let mut k = Kernel::default();
    let (a, b) = strip_cubes(&mut k, D);
    let u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(shape(&u), want);
}

#[test]
fn wr_614_union_1e5_is_one_valid_solid() {
    let mut k = Kernel::default();

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
fn wr_614_all_booleans_at_1e5_and_5e6() {
    for d in [D, 5e-6] {
        let mut k = Kernel::default();

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

    for d in [4.95e-6, 5e-6, 1e-5] {
        let mut k = Kernel::default();

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
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), 12.0, SIDE, SIDE).unwrap();
    let (_, b) = strip_cubes(&mut k, D);
    let mut u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(shape(&u), (11, 27, 18, 1, true));
    assert_valid(&mut k, &mut u, "wider box union");
    // A (192) and B (64) overlap in the 20.2 d^2 wedge only.
    let want = 12.0 * SIDE * SIDE + SIDE * SIDE * SIDE;
    let vu = volume(&k, &mut u);
    assert!(
        (vu - want).abs() <= ACCURACY * want,
        "volume {vu}, want {want}"
    );

    let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), 12.0, SIDE, SIDE).unwrap();
    let (_, b) = strip_cubes(&mut k, D);
    let inter = boolean_intersection(&mut k, a, b).unwrap();
    assert_eq!(shape(&inter).3, 1, "one intersection solid");
    assert!(shape(&inter).4, "manifold intersection");
}

/// The wider case with a pocket in A's face y = 20, beside the crossing: A is the wider box
/// minus [2, 3] x [19, 21] x `cut`, and B the turned box of `strip_cubes` cut to z in
/// [`z0`, `z0` + `h`]. The hole ([0.5, 3.5], B over [1, 3]) is an inner loop of A's face;
/// its left edge x = 2 spans B's height, lies in the band of F1's plane and faces the
/// crossing, while A's face goes on past it beyond the tolerance (x up to 10). The notch
/// ([2, 5], B over [2.5, 3.5]) is the same edge on A's outer loop, made reentrant. In both,
/// the crossing at x = 2 - d / SLOPE is an exit, as in
/// `wr_614_wider_protruding_box_keeps_exit_crossing`: the union is one valid manifold solid
/// of volume |A| + |B| - overlap, the intersection is empty and A minus B is A less the
/// overlap (the pocket lies at x >= 2, clear of the wedge x <= 2).
fn pocketed_wider_case(d: f64, cut: (f64, f64), z0: f64, h: f64) -> Vec<String> {
    let mut failed = Vec::new();
    let what = format!("pocket z {cut:?}, d = {d}");
    let pocket = (cut.1 - cut.0).min(SIDE - cut.0);
    let va = 12.0 * SIDE * SIDE - pocket;
    let overlap = overlap_volume(d) / SIDE * h;
    let bodies = |k: &mut Kernel| {
        let a = create_box(k, Point3::new(-2.0, 16.0, 0.0), 12.0, SIDE, SIDE).unwrap();
        let p = create_box(k, Point3::new(2.0, 19.0, cut.0), 1.0, 2.0, cut.1 - cut.0).unwrap();
        let a = boolean_difference(k, a, p).unwrap();
        let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), SIDE, SIDE, h).unwrap();
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut b)),
            Point3::ZERO,
            Vec3::Z,
            SLOPE.atan().to_degrees(),
        )
        .unwrap();
        let half = d / SLOPE;
        let shift = Vec3::new((2.0 - half) - half, 20.0 - d, z0);
        api_translate(k, Some(TransformObject::Brep(&mut b)), shift).unwrap();
        (a, b)
    };
    let mut k = Kernel::default();
    let (a, b) = bodies(&mut k);
    match boolean_union(&mut k, a, b) {
        Err(e) => failed.push(format!("{what}: union refused: {e}")),
        Ok(mut u) => {
            let want = va + SIDE * SIDE * h - overlap;
            let v = volume(&k, &mut u);
            let check = check_brep(&mut k, &mut u, CheckLevel::Level2).unwrap();
            if shape(&u).3 != 1
                || !shape(&u).4
                || check.failure_count() != 0
                || (v - want).abs() > ACCURACY * want
            {
                failed.push(format!(
                    "{what}: union {:?}, volume {v} (want {want}), check {:?}",
                    shape(&u),
                    check.lines()
                ));
            }
        }
    }
    let (a, b) = bodies(&mut k);
    match boolean_intersection(&mut k, a, b) {
        Err(e) => failed.push(format!("{what}: intersection refused: {e}")),
        Ok(mut inter) => {
            let check = check_brep(&mut k, &mut inter, CheckLevel::Level2).unwrap();
            if shape(&inter).0 != 0 || shape(&inter).3 != 0 || check.failure_count() != 0 {
                failed.push(format!(
                    "{what}: intersection {:?}, check {:?}",
                    shape(&inter),
                    check.lines()
                ));
            }
        }
    }
    let (a, b) = bodies(&mut k);
    match boolean_difference(&mut k, a, b) {
        Err(e) => failed.push(format!("{what}: difference refused: {e}")),
        Ok(mut diff) => {
            let want = va - overlap;
            let v = volume(&k, &mut diff);
            let check = check_brep(&mut k, &mut diff, CheckLevel::Level2).unwrap();
            if shape(&diff).3 != 1
                || !shape(&diff).4
                || check.failure_count() != 0
                || (v - want).abs() > ACCURACY * want
            {
                failed.push(format!(
                    "{what}: difference {:?}, volume {v} (want {want}), check {:?}",
                    shape(&diff),
                    check.lines()
                ));
            }
        }
    }
    failed
}

/// A real exit beside an edge that looks like a bound: a hole in A's face y = 20 (an inner
/// loop) and a reentrant notch (the outer loop) each put a straight edge in the band of F1's
/// plane, spanning B's height and facing the crossing, while A's face goes on past it
/// beyond the tolerance. A rule that takes such an edge as the end of its face drops the
/// exit and leaves the union as two regions; the face-wide certificate keeps it
/// (`pocketed_wider_case`). Unions 189 + 32 and 190 + 16 less the overlap, differences 189
/// and 190 less the overlap.
#[test]
fn wr_614_wider_box_with_a_hole_or_notch_keeps_exit_crossing() {
    let failed: Vec<String> = [((0.5, 3.5), 1.0, 2.0), ((2.0, 5.0), 2.5, 1.0)]
        .into_iter()
        .flat_map(|(cut, z0, h)| pocketed_wider_case(D, cut, z0, h))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The exact overlap of A and B (narrow or wider A): the prism over the triangle P0, Q4,
/// Q1 between B's faces F4, F1 and A's face y = 20, 4 mm high. With s = SLOPE, P0 =
/// (2 - 2d/s, 20 - d) and F4 (direction (-s, 1)) and F1 (direction (1, s)) meet y = 20 at
/// Q4 = P0 + (-s d, d) and Q1 = P0 + (d/s, d); the triangle's area is half its base
/// |Q1 - Q4| = d/s + s d times its height d.
fn overlap_volume(d: f64) -> f64 {
    0.5 * (d / SLOPE + SLOPE * d) * d * SIDE
}

/// One offset of the sweeps below: the union of A (`width` along x from x = -2) and B at
/// offset d is one valid manifold solid of volume |A| + 64 - overlap, their intersection
/// is empty and valid, and A minus B is box A, valid, of volume |A| - overlap (the
/// overlap's faces lie within d of each other, so it is no piece of either result).
/// Returns what failed, or nothing.
fn union_one_solid_intersection_empty(width: f64, d: f64) -> Vec<String> {
    let mut failed = Vec::new();
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), width, SIDE, SIDE).unwrap();
    let (_, b) = strip_cubes(&mut k, d);
    match boolean_union(&mut k, a, b) {
        Err(e) => failed.push(format!("d = {d}: union refused: {e}")),
        Ok(mut u) => {
            let want = width * SIDE * SIDE + SIDE * SIDE * SIDE - overlap_volume(d);
            let v = volume(&k, &mut u);
            let check = check_brep(&mut k, &mut u, CheckLevel::Level2).unwrap();
            if shape(&u).3 != 1
                || !shape(&u).4
                || collapsed_edges(&k, &u) != 0
                || check.failure_count() != 0
                || (v - want).abs() > ACCURACY * want
            {
                failed.push(format!(
                    "d = {d}: union {:?}, {} speck edges, volume {v} (want {want}), check {:?}",
                    shape(&u),
                    collapsed_edges(&k, &u),
                    check.lines()
                ));
            }
        }
    }
    let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), width, SIDE, SIDE).unwrap();
    let (_, b) = strip_cubes(&mut k, d);
    match boolean_intersection(&mut k, a, b) {
        Err(e) => failed.push(format!("d = {d}: intersection refused: {e}")),
        Ok(mut inter) => {
            let check = check_brep(&mut k, &mut inter, CheckLevel::Level2).unwrap();
            if shape(&inter).0 != 0 || shape(&inter).3 != 0 || check.failure_count() != 0 {
                failed.push(format!(
                    "d = {d}: intersection {:?}, check {:?}",
                    shape(&inter),
                    check.lines()
                ));
            }
        }
    }
    let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), width, SIDE, SIDE).unwrap();
    let (_, b) = strip_cubes(&mut k, d);
    match boolean_difference(&mut k, a, b) {
        Err(e) => failed.push(format!("d = {d}: difference refused: {e}")),
        Ok(mut diff) => {
            let want = width * SIDE * SIDE - overlap_volume(d);
            let v = volume(&k, &mut diff);
            let check = check_brep(&mut k, &mut diff, CheckLevel::Level2).unwrap();
            if shape(&diff) != (6, 12, 8, 1, true)
                || check.failure_count() != 0
                || (v - want).abs() > ACCURACY * want
            {
                failed.push(format!(
                    "d = {d}: difference {:?}, volume {v} (want {want}), check {:?}",
                    shape(&diff),
                    check.lines()
                ));
            }
        }
    }
    failed
}

/// The wider A (`wr_614_wider_protruding_box_keeps_exit_crossing`) across offsets d, from
/// the union's refusal on main (d < 9.9e-6) down to where Q4 and Q1 are 2.5 intersection
/// tolerances apart: the union is one valid manifold solid of volume 192 + 64 - overlap.
/// The intersection's two large faces (A's y = 20 between Q4 and Q1, B's F1 between P0 and
/// Q1) lie within d, under the intersection tolerance, of each other everywhere (planar
/// pieces: the distance is affine along them, largest at P0), so no body bounded by them
/// is valid at that tolerance: the intersection is empty. B's face F4 between P0 and Q4 is
/// 1.005 d wide, within the intersection tolerance; as in the test above, the imprint skips
/// its collapsed definition (WR-619, the default).
#[test]
fn wr_614_wider_box_union_is_one_solid_and_intersection_empty_across_offsets() {
    let failed: Vec<String> = [1e-5, 9e-6, 7.45e-6, 7.4e-6, 5e-6, 3e-6, 2.5e-6]
        .into_iter()
        .flat_map(|d| union_one_solid_intersection_empty(12.0, d))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The narrow A ([-2, 2], the strip of the tests above) across offsets d: the union is one
/// valid manifold solid of volume 128 - overlap, and the intersection is empty (its faces
/// lie within d of each other, as in the wider case). WR-619 as in the wider case. Down to
/// d = 1.2e-6, where B's corner P0 and A's corner C are 20.02 d = 2.4e-5 apart, more than
/// the intersection tolerance (2e-5): two vertices. The crossing between them, 10 d from
/// both (within the tolerance below d = 2e-6), lies inside the coincident pair and is no
/// event, so it never chains the two into one point.
#[test]
fn wr_614_strip_union_is_one_solid_and_intersection_empty_across_offsets() {
    let failed: Vec<String> = [1e-5, 9e-6, 7.4e-6, 5e-6, 3e-6, 2e-6, 1.5e-6, 1.2e-6]
        .into_iter()
        .flat_map(|d| union_one_solid_intersection_empty(SIDE, d))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Below the offsets where B's corner P0, its drop P0' on A's edge, and the crossings Q4
/// and Q1 are pairwise within the intersection tolerance (2e-5) of each other, they are one
/// point: wider A from 10.05 d < 2e-5 (P0 to Q1), narrow A from 20.02 d < 2e-5 (P0 to A's
/// corner C). Every piece of B below A's face y = 20 is then shorter than the tolerance and
/// the overlap is no volume: the union is the two boxes touching along the line through P0,
/// two solid regions, valid at the tolerance, of volume |A| + 64 - overlap (invariant 8 of
/// `docs/validity-and-tolerances.md`); the intersection is empty and A minus B is box A.
#[test]
fn wr_614_union_below_the_collapse_offsets_is_two_touching_solids() {
    let mut failed = Vec::new();
    for (width, d) in [
        (12.0, 1.5e-6),
        (12.0, 1e-6),
        (12.0, 5e-7),
        (SIDE, 8e-7),
        (SIDE, 5e-7),
    ] {
        let mut k = Kernel::default();
        let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), width, SIDE, SIDE).unwrap();
        let (_, b) = strip_cubes(&mut k, d);
        match boolean_union(&mut k, a, b) {
            Err(e) => failed.push(format!("A {width}, d = {d}: union refused: {e}")),
            Ok(mut u) => {
                let want = width * SIDE * SIDE + SIDE * SIDE * SIDE - overlap_volume(d);
                let v = volume(&k, &mut u);
                let check = check_brep(&mut k, &mut u, CheckLevel::Level2).unwrap();
                if shape(&u).3 != 2
                    || collapsed_edges(&k, &u) != 0
                    || check.failure_count() != 0
                    || (v - want).abs() > ACCURACY * want
                {
                    failed.push(format!(
                        "A {width}, d = {d}: union {:?}, volume {v} (want {want}), check {:?}",
                        shape(&u),
                        check.lines()
                    ));
                }
            }
        }
        let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), width, SIDE, SIDE).unwrap();
        let (_, b) = strip_cubes(&mut k, d);
        match boolean_intersection(&mut k, a, b) {
            Err(e) => failed.push(format!("A {width}, d = {d}: intersection refused: {e}")),
            Ok(inter) if shape(&inter).0 != 0 || shape(&inter).3 != 0 => failed.push(format!(
                "A {width}, d = {d}: intersection {:?}",
                shape(&inter)
            )),
            Ok(_) => {}
        }
        let a = create_box(&mut k, Point3::new(-2.0, 16.0, 0.0), width, SIDE, SIDE).unwrap();
        let (_, b) = strip_cubes(&mut k, d);
        match boolean_difference(&mut k, a, b) {
            Err(e) => failed.push(format!("A {width}, d = {d}: difference refused: {e}")),
            Ok(diff) if shape(&diff) != (6, 12, 8, 1, true) => {
                failed.push(format!("A {width}, d = {d}: difference {:?}", shape(&diff)));
            }
            Ok(_) => {}
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Two boxes whose faces cross at 45 deg near x = 0 while both faces extend toward +x
/// (away from the crossing on A's side, so A's inward binormal points away from the
/// crossing) keep their crossing: one manifold solid of the exact union's volume,
/// 128 - 8 + 5.12e-10 (polygon clipping in exact rationals).
#[test]
fn wr_614_diverging_same_side_boxes_keep_crossing() {
    let mut k = Kernel::default();
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
    assert_eq!(shape(&u), (11, 25, 16, 1, true));
    let want = 120.000_000_000_512;
    let v = volume(&k, &mut u);
    assert!(
        (v - want).abs() <= ACCURACY * want,
        "volume {v}, want {want}"
    );
}
