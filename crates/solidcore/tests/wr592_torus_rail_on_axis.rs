//! WR-592 (the fork's tangency-labs/smlib@994f129): a corner round along an earlier
//! round's end arc of the ball's own radius is a torus whose rail on the end face
//! collapses onto the arc's axis. With a sketch's residues (a bar profile whose corners
//! are 1e-10 off) that rail point came out 7e-11 off the axis and the surface of
//! revolution was nearly singular, not exactly. `fixes.fillet_torus_rail_on_axis` puts a
//! rail point within the tolerance of the axis on the axis. The case follows the fork's
//! capi test `corner_ball_after_a_round_is_exactly_singular`: a bar's front top edge
//! rounded r 2, then its vertical front left edge, its left top edge and the first
//! round's end arc rounded r 2 together.
//!
//! The fork's test asserts the result passes `smlib_brep_check` (level 2). This bar is
//! rebuilt here (the fork reads its own, `tests/data/corner-after-round.txt`, with other
//! residues and nine faces), and on it the pinned SMLib with the fork's hunk gives what
//! the port gives, bit for bit: the fix clears the loop gap the off-axis rail leaves, but
//! the corner's surface of revolution still fails `SmSurfOfRevolution` #7 by roundoff
//! (its polar circle starts 1.19e-10 degrees past the sweep's 0; the check allows
//! 9.1e-11). On the fork's own input the same build passes, so the fork's pass depends on
//! its input, not on another fix:
//! `plan/findings/blend.fillet_solver_p2--wr592-revolution-polar-start-angle.md`. The
//! checks below are those expectations.
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
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
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

/// The volume at the accuracy the fork's test asks (`smlib_brep_volume(.., 1e-12, ..)`).
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

/// The capi `smlib_brep_check(brep, 2)` report, one line per failed test.
fn check(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    check_brep(k, b, CheckLevel::Level2).unwrap().lines()
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

/// The corner round's collapsed rail extent, the volume it removes and its check report.
fn corner_round(fix: bool) -> (f64, f64, Vec<String>) {
    let mut k = Kernel::default();
    k.fixes.fillet_torus_rail_on_axis = fix;
    let mut b = bar(&mut k);
    let front_top = nearest_edge(&k, &b, Point3::new(20.0, 0.0579, 10.0));
    fillet_edges(&mut k, &mut b, &[front_top], 2.0, 1, 1, 1.0).unwrap();
    // the first round, the corner round's input, is sound
    assert_eq!(check(&mut k, &mut b), Vec::<String>::new());
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
    let removed = before - volume(&k, &mut b);
    (
        collapsed_rail_extent(&k, &b),
        removed,
        check(&mut k, &mut b),
    )
}

/// The fork's `near(v0 - v1, want, 1e-8)`: within `1e-8` of the value, relative.
fn near_removed(removed_volume: f64) -> bool {
    (removed_volume - removed()).abs() <= 1e-8 * removed()
}

/// The spandrel r^2 (1 - pi/4) along the vertical edge's 8 and the top edge's 14, and in
/// the corner's cube of side r what three rounds take, r^3 (1 - pi/6), less the first
/// round's 2 spandrels: 20 A + 8 (1 - pi/6) at r = 2.
fn removed() -> f64 {
    20.0 * 4.0 * (1.0 - PI / 4.0) + 8.0 * (1.0 - PI / 6.0)
}

#[test]
fn corner_round_rail_is_on_the_axis_with_the_fix() {
    let (extent, removed_volume, report) = corner_round(true);
    assert!(extent < 1e-13, "collapsed rail extent {extent:e}");
    // 20.979356723289 (pinned SMLib with the fork's hunk: the same)
    assert!(
        near_removed(removed_volume),
        "{removed_volume} {}",
        removed()
    );
    // the loop gap is gone; #7 is left, as the pinned SMLib with the fix leaves it on
    // this bar (the fork's own input passes)
    assert_eq!(
        report,
        ["SmSurfOfRevolution::Test #7 Parameterization: Bad PolarCurve Anal and NURB intervals does not match appropriate Surface Domain Intervals (tolerance= 1e-12) [owner SmSurfOfRevolution]"]
    );
}

#[test]
fn corner_round_rail_stays_off_the_axis_without_the_fix() {
    // upstream: the rail point keeps the residues, a circle about 1e-10 across
    let (extent, removed_volume, report) = corner_round(false);
    assert!(
        extent > 1e-11 && extent < 1e-9,
        "collapsed rail extent {extent:e}"
    );
    // 20.979356723547 (the pinned SMLib: the same)
    assert!(
        near_removed(removed_volume),
        "{removed_volume} {}",
        removed()
    );
    // upstream: the nearly collapsed rail is a loop gap not on a singularity, which
    // stops the check before AssertValid (whose SmSurface #5 is the fork's "before")
    assert_eq!(
        report,
        [
            "SmTrimmingTools::Test #2 UVEdge/UVEdge Gap: too large V Dir gap between loop Edge endPoints that are not on a singularity (offset=     1, tolerance=   0.1) [owner SmLoop]",
            "AssertValid skipped: ValidatePointers failed",
        ]
    );
}
