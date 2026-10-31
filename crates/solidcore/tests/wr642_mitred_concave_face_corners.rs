//! WR-642 (the fork's tangency-labs/smlib@4f06e72, the owner's ruling of 2026-10-30): with
//! `fillet_mitre_concave_face_corners`, two blends meeting where the common face's corner
//! between them is concave (a pocket's rim at its sharp corner, a boss's root at its sharp
//! corner, the side edge not filleted) are mitred, as at a corner of uniform convexity,
//! instead of being joined by a 3-sided face swept round the side edge.
//!
//! The expected volumes are the fork's closed forms: each edge's spandrel along the whole
//! edge, and in each corner's quadrant what both spandrel prisms hold. For a chamfer of leg
//! `d` that is `L d^2 / 2 + 4 d^3 / 3`; for a round of radius `r`,
//! `L r^2 (1 - pi/4) + 4 r^3 (5/3 - pi/2)`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(mitre: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_mitre_concave_face_corners = mitre;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn edge_between(k: &Kernel, b: &Brep, a: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(a) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(a) < 1e-9)
        })
        .expect("edge")
}

/// The closed loop of edges through the corners `c`.
fn rect_loop(c: [Point3; 4]) -> Vec<(Point3, Point3)> {
    (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect()
}

/// Chamfers (`chamfer`) or rounds the edges between the point pairs `edges` of the body
/// `make` builds, by `r`: the result's face count and volume.
fn blend(
    mitre: bool,
    make: fn(&mut Kernel) -> Brep,
    edges: &[(Point3, Point3)],
    r: f64,
    chamfer: bool,
) -> (usize, f64) {
    let mut k = kernel(mitre);
    let mut b = make(&mut k);
    let ids: Vec<EdgeId> = edges
        .iter()
        .map(|&(a, q)| edge_between(&k, &b, a, q))
        .collect();
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &ids, r, u32::from(!chamfer), 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    assert!(
        result.is_ok(),
        "r {r}: refused {} {}",
        info.code,
        info.comment
    );
    assert!(
        is_manifold_solid(&b).unwrap(),
        "r {r}: not a manifold solid"
    );
    b.refresh_cached_topology();
    (b.faces().ids().count(), volume(&k, &mut b))
}

/// A 60 x 40 x 10 plate with a 30 x 20 pocket 5 deep at its middle (rim perimeter 100).
fn pocketed_plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::ZERO, 60.0, 40.0, 10.0).unwrap();
    let pocket = create_box(k, p(15.0, 10.0, 5.0), 30.0, 20.0, 6.0).unwrap();
    boolean_difference(k, plate, pocket).unwrap()
}

fn pocket_rim() -> Vec<(Point3, Point3)> {
    rect_loop([
        p(15.0, 10.0, 10.0),
        p(45.0, 10.0, 10.0),
        p(45.0, 30.0, 10.0),
        p(15.0, 30.0, 10.0),
    ])
}

const POCKETED: f64 = 24_000.0 - 3_000.0;

/// A 60 x 60 x 5 plate with a 20 x 20 x 15 boss at its middle (root perimeter 80).
fn bossed_plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::ZERO, 60.0, 60.0, 5.0).unwrap();
    let boss = create_box(k, p(20.0, 20.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

fn boss_root() -> Vec<(Point3, Point3)> {
    rect_loop([
        p(20.0, 20.0, 5.0),
        p(40.0, 20.0, 5.0),
        p(40.0, 40.0, 5.0),
        p(20.0, 40.0, 5.0),
    ])
}

const BOSSED: f64 = 18_000.0 + 6_000.0;

fn chamfer_mitred(perimeter: f64, d: f64) -> f64 {
    perimeter * d * d / 2.0 + 4.0 * d * d * d / 3.0
}

fn round_mitred(perimeter: f64, r: f64) -> f64 {
    perimeter * r * r * (1.0 - PI / 4.0) + 4.0 * r * r * r * (5.0 / 3.0 - PI / 2.0)
}

fn assert_close(v: f64, expected: f64, abs: f64) {
    assert!(
        (v - expected).abs() <= abs,
        "volume {v} against {expected} (diff {:e})",
        v - expected
    );
}

#[test]
fn a_pocket_rim_chamfer_is_mitred_at_its_corners() {
    // d = 3 fits the 5 deep walls; d = 5.5 runs past the pocket's floor (a cliff
    // overflow, WR-636): the chamfer takes the same region either way.
    for d in [3.0, 5.5] {
        let (_, v) = blend(true, pocketed_plate, &pocket_rim(), d, true);
        assert_close(v, POCKETED - chamfer_mitred(100.0, d), 1e-6);
    }
    // without the flag the corners are upstream's swept 3-sided faces, not mitres
    let (_, v) = blend(false, pocketed_plate, &pocket_rim(), 3.0, true);
    assert!(
        (v - (POCKETED - chamfer_mitred(100.0, 3.0))).abs() > 1e-3,
        "volume {v}"
    );
}

#[test]
fn pocket_rim_rounds_are_mitred_at_their_corners() {
    // The fork: r 2 15 faces 20911.091276, r 5 11 faces 20415.559998; the mitre's curve is
    // the fitted fillet-fillet intersection, 3.5e-5 to 6e-5 mm^3 per corner.
    for (r, faces) in [(2.0, 15), (5.0, 11)] {
        let (n, v) = blend(true, pocketed_plate, &pocket_rim(), r, false);
        assert_eq!(n, faces, "r {r}");
        assert_close(v, POCKETED - round_mitred(100.0, r), 4.0 * 1e-4);
    }
    let (n, _) = blend(false, pocketed_plate, &pocket_rim(), 2.0, false);
    assert_ne!(n, 15, "upstream sweeps a 3-sided face at each corner");
}

#[test]
fn boss_root_rounds_are_mitred_at_their_corners() {
    // The fork: r 4 15 faces 24299.233396 against 24299.233158.
    let (n, v) = blend(true, bossed_plate, &boss_root(), 4.0, false);
    assert_eq!(n, 15);
    assert_close(v, BOSSED + round_mitred(80.0, 4.0), 4.0 * 1e-4);
    let (n, _) = blend(false, bossed_plate, &boss_root(), 4.0, false);
    assert_ne!(n, 15, "upstream sweeps a 3-sided face at each corner");
}
