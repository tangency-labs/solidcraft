//! tangency-labs/solidcore#1794 (WR-619): two 4 mm cubes, B turned atan(0.1) (5.71 deg)
//! about z, whose faces y = 20 (A) and F1 (B's y' = 0) stay within 5e-6 of each other across
//! the strip x in [1.9999, 2]. On each of the coplanar bottoms and tops, a proto edge from one
//! proto vertex back to itself holds two definitions: one is imprinted between two distinct
//! vertices in each body, the other is a speck (a line 5e-6 long, within 2.6e-6 of the vertex)
//! that lands on one vertex in each body and becomes a closed edge on it (in B, inside B's
//! side face). A's bottom sliver
//! piece is classified at that edge against a flat radial sector, deleted as inside B, and
//! the union comes back with no solid region (as the pinned C++). The opt-in
//! `Kernel::fixes.imprint_skips_collapsed_definition` (own design) makes no edge for such a
//! collapsed definition.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

const SIDE: f64 = 4.0;
const SLOPE: f64 = 0.1;
/// The accuracy asked of the volume integrator.
const ACCURACY: f64 = 1e-6;

/// A: [-2, 2] x [16, 20] x [0, 4]. B: a 4 mm cube turned about z so that its face F1
/// crosses y = 20 at x = 2 - d / SLOPE, with its corner edge at x = 2 - 2 d / SLOPE, d below
/// y = 20 (the `wr614_coincident_strip.json` fixture's arithmetic).
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

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.imprint_skips_collapsed_definition = fix;
    k
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

/// Edges from a vertex back to itself along a curve whose ends lie apart (closed edges on a
/// speck or an open curve; a real closed curve's ends coincide).
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
fn wr_619_imprint_skips_collapsed_definition_is_off_by_default() {
    assert!(!Kernel::default().fixes.imprint_skips_collapsed_definition);
}

#[test]
fn wr_619_upstream_strip_union_has_no_solid() {
    let mut k = kernel(false);
    let (a, b) = strip_cubes(&mut k, 5e-6);
    let u = boolean_union(&mut k, a, b).unwrap();
    assert_eq!(shape(&u).3, 0);
    assert!(!shape(&u).4);
}

#[test]
fn wr_619_strip_union_with_the_fix_is_one_solid() {
    let mut k = kernel(true);
    let (mut a, mut b) = strip_cubes(&mut k, 5e-6);
    eprintln!("WR619 operands: A {:?} B {:?}", shape(&a), shape(&b));
    assert_valid(&mut k, &mut a, "A");
    assert_valid(&mut k, &mut b, "B");
    let mut u = boolean_union(&mut k, a, b).unwrap();
    eprintln!("WR619 union with the fix: {:?}", shape(&u));
    assert_eq!(shape(&u).3, 1, "one solid region");
    assert!(shape(&u).4, "manifold solid");
    assert_eq!(collapsed_edges(&k, &u), 0, "no closed edge on a speck");
    assert_valid(&mut k, &mut u, "union");
    let want = 2.0 * SIDE * SIDE * SIDE;
    let v = volume(&k, &mut u);
    assert!(
        (v - want).abs() <= ACCURACY * want,
        "volume {v}, want {want}"
    );
}

/// The failing union 1e-5 apart (WR-614): recorded, whatever it gives.
#[test]
fn wr_619_strip_union_1e5_with_the_fix_is_recorded() {
    let mut k = kernel(true);
    let (a, b) = strip_cubes(&mut k, 1e-5);
    match boolean_union(&mut k, a, b) {
        Ok(mut u) => {
            let check = check_brep(&mut k, &mut u, CheckLevel::Level2).unwrap();
            eprintln!(
                "WR619 d=1e-5 with the fix: Ok {:?}, check failures {}",
                shape(&u),
                check.failure_count()
            );
        }
        Err(e) => eprintln!("WR619 d=1e-5 with the fix: Err {e:?}"),
    }
}

/// A genuine closed edge on a vertex (a circle whose ends coincide, its samples leaving the
/// vertex) is imprinted as before: a cylinder on a box's top, at radius 1 and at radius 1e-3
/// (well above the 1e-4 check tolerance; at 1.6e-5 its cap plane would be below it).
#[test]
fn wr_619_fix_keeps_closed_edges_on_a_vertex() {
    for radius in [1.0, 1e-3] {
        let run = |fix: bool| {
            let mut k = kernel(fix);
            let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 4.0, 4.0).unwrap();
            let b = create_cylinder(&mut k, Point3::new(2.0, 2.0, 4.0), radius, 1.0).unwrap();
            let mut u = boolean_union(&mut k, a, b).unwrap();
            assert_valid(&mut k, &mut u, "union");
            (shape(&u), volume(&k, &mut u))
        };
        let off = run(false);
        let on = run(true);
        eprintln!("WR619 circle radius {radius}: off {off:?} on {on:?}");
        assert_eq!(on.0, off.0, "radius {radius}");
        assert!(
            (on.1 - off.1).abs() <= ACCURACY * off.1,
            "radius {radius}: {} vs {}",
            on.1,
            off.1
        );
    }
}

/// 1.5e-5 apart the speck reaches 1.25e-5 from B-rep 1's vertex, past its definition's
/// tolerance (1e-5), so the fix leaves it: the union, difference and intersection are those
/// without the fix (shape, volume and Level-2 failures), as the pinned C++. (Skipping it there
/// leaves edges 1.25e-5 from the vertex, past half its tolerance.)
#[test]
fn wr_619_strip_1_5e5_is_unchanged_by_the_fix() {
    type Op = fn(&mut Kernel, Brep, Brep) -> solidcore::Result<Brep>;
    let ops: [(&str, Op); 3] = [
        ("union", boolean_union),
        ("difference", boolean_difference),
        ("intersection", boolean_intersection),
    ];
    for (name, op) in ops {
        let run = |fix: bool| {
            let mut k = kernel(fix);
            let (a, b) = strip_cubes(&mut k, 1.5e-5);
            let mut r = op(&mut k, a, b).unwrap();
            let store = AttributeStore::default();
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &|_| ObjectKey(0),
            };
            let v = brep_volume(&k, &mut r, ACCURACY, &attrs).ok();
            let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
            (shape(&r), v, check.failure_count())
        };
        let off = run(false);
        let on = run(true);
        eprintln!("WR619 1.5e-5 {name}: off {off:?} on {on:?}");
        assert_eq!(on, off, "{name}");
    }
}

/// A minus B, B touching A along the strip (their overlap is a wedge under 1e-8 mm^3): with
/// the fix, box A as it was (upstream leaves a non-manifold body with a sheet), 1e-5 and 5e-6
/// apart.
#[test]
fn wr_619_strip_difference_with_the_fix_is_box_a() {
    for d in [1e-5, 5e-6] {
        let mut k = kernel(true);
        let (a, b) = strip_cubes(&mut k, d);
        let mut r = boolean_difference(&mut k, a, b).unwrap();
        assert_eq!(shape(&r), (6, 12, 8, 1, true), "d = {d}");
        assert_eq!(
            collapsed_edges(&k, &r),
            0,
            "d = {d}: no closed edge on a speck"
        );
        assert_valid(&mut k, &mut r, "difference");
        let want = SIDE * SIDE * SIDE;
        let v = volume(&k, &mut r);
        assert!(
            (v - want).abs() <= ACCURACY * want,
            "d = {d}: volume {v}, want {want}"
        );
    }
}

/// A and B touching along the strip: with the fix, their intersection is empty (upstream
/// leaves two sheets of the strip), 1e-5 and 5e-6 apart.
#[test]
fn wr_619_strip_intersection_with_the_fix_is_empty() {
    for d in [1e-5, 5e-6] {
        let mut k = kernel(true);
        let (a, b) = strip_cubes(&mut k, d);
        let r = boolean_intersection(&mut k, a, b).unwrap();
        assert_eq!(shape(&r).0, 0, "d = {d}: no faces");
        assert_eq!(shape(&r).3, 0, "d = {d}: no solid region");
    }
}

/// An ordinary union of two boxes sharing their bottom plane is unchanged by the fix.
#[test]
fn wr_619_fix_leaves_coplanar_bottoms_alone() {
    let run = |fix: bool| {
        let mut k = kernel(fix);
        let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 4.0, 4.0).unwrap();
        let b = create_box(&mut k, Point3::new(2.0, 1.0, 0.0), 4.0, 2.0, 2.0).unwrap();
        let mut u = boolean_union(&mut k, a, b).unwrap();
        (shape(&u), volume(&k, &mut u))
    };
    let off = run(false);
    assert_eq!((off.0 .3, off.0 .4), (1, true));
    let on = run(true);
    assert_eq!(on.0, off.0);
    assert!((on.1 - off.1).abs() <= ACCURACY * off.1);
}
