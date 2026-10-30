//! WR-592 (the fork's tangency-labs/smlib@994f129): a corner round along an earlier
//! round's end arc of the ball's own radius is a torus whose rail on the end face
//! collapses onto the arc's axis. With a sketch's residues (a bar profile whose corners
//! are 1e-10 off) that rail point came out 7e-11 off the axis and the surface of
//! revolution was nearly singular, not exactly. `fixes.fillet_torus_rail_on_axis` puts a
//! rail point within the tolerance of the axis on the axis. The case follows the fork's
//! capi test `corner_ball_after_a_round_is_exactly_singular`: a bar's front top edge
//! rounded r 2, then its vertical front left edge, its left top edge and the first
//! round's end arc rounded r 2 together.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3, PI};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// The bar's profile in z = 0 (extruded 10 up), its corners off by the residues of a
/// sketch solve.
const PROFILE: [[f64; 2]; 4] = [
    [0.4893, 0.0579],
    [41.6497, 0.0579 + 3e-10],
    [41.6497 + 1e-10, 16.0579],
    [0.4893 + 2e-10, 16.0579 - 2e-10],
];

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// The edge whose midpoint is nearest `p`.
fn nearest_edge(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    b.edges()
        .ids()
        .min_by(|&x, &y| {
            midpoint(k, b, x)
                .distance(p)
                .total_cmp(&midpoint(k, b, y).distance(p))
        })
        .unwrap()
}

fn bar(k: &mut Kernel) -> Brep {
    let cs: Vec<Curve> = PROFILE
        .iter()
        .zip(PROFILE.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// The smallest extent of a constant-u or constant-v side of a surface of revolution in
/// `b` (its collapsed rail: the largest distance of 8 points along it from its start).
fn collapsed_rail_extent(k: &Kernel, b: &Brep) -> f64 {
    let mut least = f64::INFINITY;
    for f in b.faces().ids() {
        let Some(s) = face::surface(b, f).unwrap() else {
            continue;
        };
        if !matches!(s.kind(), SurfaceKind::SurfOfRevolution(_)) {
            continue;
        }
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        let (lo, hi) = (d.min(), d.max());
        let extent = |at: &dyn Fn(f64) -> Point2| {
            let p0 = s.evaluate_point(k, at(0.0)).unwrap();
            (1..=8)
                .map(|i| {
                    s.evaluate_point(k, at(f64::from(i) / 8.0))
                        .unwrap()
                        .distance(p0)
                })
                .fold(0.0_f64, f64::max)
        };
        for side in [
            extent(&|t| Point2::new(lo.x, lo.y + (hi.y - lo.y) * t)),
            extent(&|t| Point2::new(hi.x, lo.y + (hi.y - lo.y) * t)),
            extent(&|t| Point2::new(lo.x + (hi.x - lo.x) * t, lo.y)),
            extent(&|t| Point2::new(lo.x + (hi.x - lo.x) * t, hi.y)),
        ] {
            least = least.min(side);
        }
    }
    least
}

/// The corner round's collapsed rail extent and the volume it removes.
fn corner_round(fix: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_torus_rail_on_axis = fix;
    let mut b = bar(&mut k);
    let front_top = nearest_edge(&k, &b, Point3::new(20.0, 0.0579, 10.0));
    fillet_edges(&mut k, &mut b, &[front_top], 2.0, 1, 1, 1.0).unwrap();
    let before = volume(&k, &mut b);
    let corner: Vec<EdgeId> = [
        Point3::new(0.4893, 0.0579, 4.0),
        Point3::new(0.4893, 9.058, 10.0),
        Point3::new(0.4893, 0.6437, 9.4142),
    ]
    .iter()
    .map(|&p| nearest_edge(&k, &b, p))
    .collect();
    fillet_edges(&mut k, &mut b, &corner, 2.0, 1, 1, 1.0).unwrap();
    (collapsed_rail_extent(&k, &b), before - volume(&k, &mut b))
}

/// The spandrel r^2 (1 - pi/4) along the vertical edge's 8 and the top edge's 14, and in
/// the corner's cube of side r what three rounds take, r^3 (1 - pi/6), less the first
/// round's 2 spandrels: 20 A + 8 (1 - pi/6) at r = 2.
fn removed() -> f64 {
    20.0 * 4.0 * (1.0 - PI / 4.0) + 8.0 * (1.0 - PI / 6.0)
}

#[test]
fn corner_round_rail_is_on_the_axis_with_the_fix() {
    let (extent, removed_volume) = corner_round(true);
    assert!(extent < 1e-13, "collapsed rail extent {extent:e}");
    assert!(
        (removed_volume - removed()).abs() < 1e-6 * removed(),
        "{removed_volume} {}",
        removed()
    );
}

#[test]
fn corner_round_rail_stays_off_the_axis_without_the_fix() {
    // upstream: the rail point keeps the residues, a circle about 1e-10 across
    let (extent, removed_volume) = corner_round(false);
    assert!(
        extent > 1e-11 && extent < 1e-9,
        "collapsed rail extent {extent:e}"
    );
    assert!(
        (removed_volume - removed()).abs() < 1e-6 * removed(),
        "{removed_volume} {}",
        removed()
    );
}
