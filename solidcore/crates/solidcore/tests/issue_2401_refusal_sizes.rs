//! #2401 (carried over from tangency-labs/smlib#91; the generic blend, hub #2243): the
//! refusals that had no size through the fit name the largest size that builds, each decided
//! from its own geometry, with `fixes.fillet_gn_section_generator` (WR-685/WR-696):
//!
//! - **An open end of an overflow** (`CutFilletAtOpenEnd`'s refusals, and a blend the trim
//!   keeps no length of where no corner takes any): the overflow is absent below the size at
//!   which the end's blend stays on its faces (`LargestSizeOnItsFaces`), so that size, open.
//! - **A polygon face taken whole** (a cube's top chamfered half its width): the face is
//!   taken whole once the blends' balls reach its deepest point (`NoteFitOfPolygonsTakenWhole`'s
//!   bound), judged before the trace, whose rails of no length failed first.
//! - **A collapsed offset** (the fork's `middle_edge_of_an_arc_tangent_chain_r2`): a ball as
//!   wide as a face it rolls on has its centre fixed; its zone ends in a section of the ball
//!   at an end of the edge only where the plane through that end square to the edge holds the
//!   centre. The arc chain's middle edge does not (refused, naming the face's radius, open);
//!   the tray's seam and the cornered block's rim do, and build as before.
//!
//! Inputs: the fork's capi tests (`ops_kernel.cpp` at 18e96f1): `chamfers_taking_a_square_top_refused`,
//! `middle_edge_of_an_arc_tangent_chain_r2` (`arc_wall_plate`), and the tray (#416) and the
//! cornered block (#417) of its main block. The arc-ended box and the 10 x 6 top are this
//! test's own. Values: closed forms.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, SolidError};

/// SolidCraft's set of fixes, `fillet_gn_section_generator` as given.
fn kernel(gn: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_gn_section_generator = gn;
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 0.1)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

fn lumps(b: &Brep) -> usize {
    let infinite = b.infinite_region();
    regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count()
}

/// Blends the edges at `at` by `r` (`xsect` 1 a round, 0 a chamfer) through the public API.
fn blend(k: &mut Kernel, b: &mut Brep, at: &[Point3], r: f64, xsect: u32) -> solidcore::Result<()> {
    let edges: Vec<EdgeId> = at.iter().map(|&q| pick(k, b, q)).collect();
    fillet_edges(k, b, &edges, r, xsect, 2, 0.0)
}

/// The refusal's reason and its fit (size, kind).
fn refusal(r: solidcore::Result<()>) -> (Option<NoRoomReason>, f64, FitKind) {
    match r {
        Err(SolidError::FilletRefused { reason, fit, .. }) => (reason, fit.size, fit.kind),
        other => panic!("not refused with a size: {other:?}"),
    }
}

/// Refused for `reason`, naming `want` (a radius) to 1e-9 relative.
fn assert_refused(r: solidcore::Result<()>, reason: NoRoomReason, want: f64) -> f64 {
    let (got_reason, fit, kind) = refusal(r);
    assert_eq!(got_reason, Some(reason));
    assert_eq!(kind, FitKind::Radius);
    assert!((fit - want).abs() <= 1e-9 * want, "fit {fit}, want {want}");
    fit
}

/// One solid, `check_brep` clean at level 2, its volume `want` within `rel`.
fn assert_one_sound_solid(k: &mut Kernel, b: &mut Brep, want: f64, rel: f64) {
    assert_eq!(lumps(b), 1, "one lump");
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    let v = volume_of(k, b);
    assert!(
        (v - want).abs() <= rel * want,
        "volume {v:.12}, want {want:.12}"
    );
}

/// The fit margin: a thousand of the blended edge's tolerances.
fn margin(k: &Kernel, b: &Brep, at: Point3) -> f64 {
    1000.0 * edge::tolerance(b, pick(k, b, at)).unwrap()
}

// ---- an open end of an overflow -------------------------------------------------------

/// A 10 x 20 x 30 box whose end y = 20 is cut by a vertical cylinder of radius 12 about
/// (5, 30): that end is a concave cylinder, 18 <= y <= 19.0868 across the box.
fn arc_ended_box(k: &mut Kernel) -> Brep {
    let b = create_box(k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let c = create_cylinder(k, Point3::new(5.0, 30.0, -1.0), 12.0, 32.0).unwrap();
    boolean_difference(k, b, c).unwrap()
}

/// Where the cylindrical end is, at `x`.
fn end_at(x: f64) -> f64 {
    30.0 - (144.0 - (x - 5.0) * (x - 5.0)).sqrt()
}

/// Simpson's rule on `n` (even) steps.
fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    let mut s = f(a) + f(b);
    for i in 1..n {
        s += f(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0
}

/// The arc-ended box rounded `r` along its top edge x = 10, z = 30: the box less the
/// cylinder's cut, less the round's corner section (r - sqrt(r^2 - (x - 10 + r)^2) high at
/// x) along y from 0 to the cylinder, in x = 10 - r + r sin(t).
fn arc_ended_box_volume(r: f64) -> f64 {
    let n = 4000;
    let cut = simpson(|x| (20.0 - end_at(x)) * 30.0, 0.0, 10.0, n);
    let removed = simpson(
        |t| {
            let x = 10.0 - r + r * t.sin();
            (r - r * t.cos()) * end_at(x) * r * t.cos()
        },
        0.0,
        PI / 2.0,
        n,
    );
    6000.0 - cut - removed
}

const TOP_EDGE: Point3 = Point3::new(10.0, 9.0, 30.0);

/// Past r = 10 the rail on the top runs past its far edge x = 0: a cliff overflow, which
/// reaches the cylindrical end, a face its plane cannot cut the round by
/// (`CliffEndFaceNotPlanar`). Below 10 the round stays on its faces: the fit is 10 (open)
/// less the margin.
#[test]
fn an_overflow_past_a_cylindrical_end_names_where_it_stays_on_its_faces() {
    for r in [12.0, 10.01] {
        let mut k = kernel(true);
        let mut b = arc_ended_box(&mut k);
        let m = margin(&k, &b, TOP_EDGE);
        let got = blend(&mut k, &mut b, &[TOP_EDGE], r, 1);
        assert_refused(got, NoRoomReason::CliffEndFaceNotPlanar, 10.0 - m);
    }
}

/// The named size builds: one solid of the closed-form volume, to the round's end on the
/// cylinder, whose curve is fitted within the fillet's tolerance (4e-5, four of the body's)
/// over the round's section, (1 - pi/4) r^2: 8.6e-4 here, 1.7e-7 of the volume (the build
/// is 3.5e-4 off).
#[test]
fn an_overflow_past_a_cylindrical_end_builds_at_its_named_size() {
    let mut k = kernel(true);
    let mut b = arc_ended_box(&mut k);
    let fit = 10.0 - margin(&k, &b, TOP_EDGE);
    blend(&mut k, &mut b, &[TOP_EDGE], fit, 1).unwrap();
    let want = arc_ended_box_volume(fit);
    let bound = 4.0e-5 * (1.0 - PI / 4.0) * fit * fit;
    assert_one_sound_solid(&mut k, &mut b, want, bound / want);
}

// ---- a polygon face taken whole -------------------------------------------------------

/// The four top edges of the box `w x h x 10` (`w >= h`).
fn top_edges(w: f64, h: f64) -> [Point3; 4] {
    [
        Point3::new(w / 2.0, 0.0, 10.0),
        Point3::new(w, h / 2.0, 10.0),
        Point3::new(w / 2.0, h, 10.0),
        Point3::new(0.0, h / 2.0, 10.0),
    ]
}

/// The box `w x h x 10` with its top's four edges chamfered `d`: the top slab `d` thick
/// less the prismatoid from the `w x h` top to the `(w - 2d) x (h - 2d)` one.
fn chamfered_top_volume(w: f64, h: f64, d: f64) -> f64 {
    let (a1, am, a2) = (w * h, (w - d) * (h - d), (w - 2.0 * d) * (h - 2.0 * d));
    10.0 * w * h - (w * h * d - d / 6.0 * (a1 + 4.0 * am + a2))
}

/// The fork's `chamfers_taking_a_square_top_refused`: a 10 cube's four top edges chamfered
/// 5 take the whole top, whose deepest point, its centre, is 5 from each edge (open). SMLib
/// failed in the trace first ("Fillet Surface Computation Failure", no size); now the
/// polygon's own event refuses it, naming 5 less the margin. Just over, 5.01, the same.
#[test]
fn chamfers_taking_a_square_top_name_its_depth() {
    for d in [5.0, 5.01] {
        let mut k = kernel(true);
        let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let m = margin(&k, &b, top_edges(10.0, 10.0)[0]);
        let got = blend(&mut k, &mut b, &top_edges(10.0, 10.0), d, 0);
        assert_refused(got, NoRoomReason::FaceConsumed, 5.0 - m);
    }
}

/// The named size builds: the fork's closed form, `100 d - d (100 + s^2 + 10 s) / 3` off.
#[test]
fn chamfers_taking_a_square_top_build_at_the_named_size() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let fit = 5.0 - margin(&k, &b, top_edges(10.0, 10.0)[0]);
    blend(&mut k, &mut b, &top_edges(10.0, 10.0), fit, 0).unwrap();
    assert_one_sound_solid(&mut k, &mut b, chamfered_top_volume(10.0, 10.0, fit), 1e-9);
}

/// A 10 x 6 top chamfered 3.5 (jyviko's case on tangency-labs/smlib#91, where the fit named
/// 0.094, `BandsMeetAt` of two chamfers meeting at a vertex): the top is taken whole past
/// its depth 3 (half its width), so the polygon's event refuses it first, naming 3 less the
/// margin, and that builds.
#[test]
fn chamfers_taking_a_rectangular_top_name_its_depth_and_build_there() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 6.0, 10.0).unwrap();
    let m = margin(&k, &b, top_edges(10.0, 6.0)[0]);
    let got = blend(&mut k, &mut b, &top_edges(10.0, 6.0), 3.5, 0);
    let fit = assert_refused(got, NoRoomReason::FaceConsumed, 3.0 - m);
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 6.0, 10.0).unwrap();
    blend(&mut k, &mut b, &top_edges(10.0, 6.0), fit, 0).unwrap();
    assert_one_sound_solid(&mut k, &mut b, chamfered_top_volume(10.0, 6.0, fit), 1e-9);
}

// ---- a collapsed offset ---------------------------------------------------------------

/// The fork's `arc_wall_plate`: a 41 1/6 x 21 plate and the corner a circle of radius 21
/// leaves, 10 high, then its top front edge rounded R2 (SolidCraft's pick).
fn arc_wall_plate_rounded(k: &mut Kernel) -> Brep {
    let w = 41.0 + 1.0 / 6.0;
    let corner_box = create_box(k, Point3::new(w, -21.0, 0.0), 21.0, 21.0, 10.0).unwrap();
    let cyl = create_cylinder(k, Point3::new(w + 21.0, -21.0, 0.0), 21.0, 10.0).unwrap();
    let corner = boolean_difference(k, corner_box, cyl).unwrap();
    let plate = create_box(k, Point3::new(0.0, -21.0, 0.0), w, 21.0, 10.0).unwrap();
    let mut b = boolean_union(k, plate, corner).unwrap();
    blend(
        k,
        &mut b,
        &[Point3::new(20.583333333333332, -21.0, 10.0)],
        2.0,
        1,
    )
    .unwrap();
    b
}

/// The edge between the front round and the arc wall: the middle edge of the arc top edge's
/// tangent chain, whose ends are tangent ends (the fork's pick).
const MIDDLE: Point3 = Point3::new(41.17486688363082, -20.413218067968561, 9.415188112149071);

/// The fork's `middle_edge_of_an_arc_tangent_chain_r2`: a ball of the front round's radius
/// sits on that round's axis, 23 from the arc wall's: at x = 62 1/6 - sqrt 525, fixed. At
/// the edge's top end V = (62 1/6 - sqrt 437, -19, 10), where the edge runs on along the
/// arc's top edge, the plane square to the edge misses it by `(2 / 21) (x_V - x_C)`, 0.19
/// (the axis crosses that plane 21, not 23, from the arc's axis): its zone has no section
/// there, so no round ends there. SMLib failed in its
/// geometry (1001, "Fillet Surface Computation Failure"); now refused, naming 2 less the
/// margin (open: below 2 the round's offset is a cylinder again).
#[test]
fn the_middle_edge_at_its_round_s_radius_is_refused_as_a_collapsed_offset() {
    let mut k = kernel(true);
    let mut b = arc_wall_plate_rounded(&mut k);
    let m = margin(&k, &b, MIDDLE);
    let got = blend(&mut k, &mut b, &[MIDDLE], 2.0, 1);
    assert_refused(got, NoRoomReason::OffsetCollapses, 2.0 - m);
}

/// Just over, 2.5, the ball does not fit the front round (`BallIsRegular`): it names the
/// round's radius, which closed would be 2 itself, refused above; open, 2 less the margin.
#[test]
fn the_middle_edge_past_its_round_s_radius_names_that_radius_open() {
    let mut k = kernel(true);
    let mut b = arc_wall_plate_rounded(&mut k);
    let m = margin(&k, &b, MIDDLE);
    let got = blend(&mut k, &mut b, &[MIDDLE], 2.5, 1);
    assert_refused(got, NoRoomReason::BallDoesNotFit, 2.0 - m);
}

/// The fork's tray (tangency-labs/solidcore#416): a 25 x 25 x 10 box with its bottom edges
/// rounded R2, then its vertical edges R2 with the seams below them. On each seam both
/// bottom rounds' offsets collapse onto their axes, which cross at the ball's fixed centre;
/// the seam's ends' planes square to it hold that centre, so the zone closes and it builds:
/// each vertical edge loses (1 - pi/4) 4 along 8, each corner the octant of the rounds'
/// crossing (16/3) less the corner ball's (4 pi / 3).
#[test]
fn the_tray_s_seams_at_their_rounds_radius_build() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::ZERO, 25.0, 25.0, 10.0).unwrap();
    let bottom = [
        Point3::new(12.5, 0.0, 0.0),
        Point3::new(25.0, 12.5, 0.0),
        Point3::new(12.5, 25.0, 0.0),
        Point3::new(0.0, 12.5, 0.0),
    ];
    let edges: Vec<EdgeId> = bottom.iter().map(|&q| pick(&k, &b, q)).collect();
    fillet_edges(&mut k, &mut b, &edges, 2.0, 1, 1, 1.0).unwrap();
    let s2 = 2.0 - 2.0_f64.sqrt();
    let vertical = [
        Point3::new(0.0, 0.0, 6.0),
        Point3::new(25.0, 0.0, 6.0),
        Point3::new(25.0, 25.0, 6.0),
        Point3::new(0.0, 25.0, 6.0),
        Point3::new(s2, s2, s2),
        Point3::new(25.0 - s2, s2, s2),
        Point3::new(25.0 - s2, 25.0 - s2, s2),
        Point3::new(s2, 25.0 - s2, s2),
    ];
    let before = volume_of(&k, &mut b);
    let edges: Vec<EdgeId> = vertical.iter().map(|&q| pick(&k, &b, q)).collect();
    fillet_edges(&mut k, &mut b, &edges, 2.0, 1, 1, 1.0).unwrap();
    let removed = 4.0 * (32.0 * (1.0 - PI / 4.0) + 16.0 / 3.0 - 4.0 * PI / 3.0);
    assert_one_sound_solid(&mut k, &mut b, before - removed, 1e-7);
}

/// The fork's cornered block (tangency-labs/solidcore#417): a 297 x 100 x 60 box with its
/// vertical edges rounded R5, then its top rim R5. On each corner arc the corner round's
/// offset collapses onto its axis; every plane square to the arc holds the axis, so the zone
/// closes against the straight rim rounds and it builds: each straight edge loses
/// (1 - pi/4) 25 along 287 or 90, each corner (pi/2) (62.5 - 125/3).
#[test]
fn the_cornered_block_s_rim_at_its_corners_radius_builds() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::ZERO, 297.0, 100.0, 60.0).unwrap();
    let corners = [(0.0, 0.0), (297.0, 0.0), (0.0, 100.0), (297.0, 100.0)]
        .map(|(x, y)| Point3::new(x, y, 30.0));
    blend(&mut k, &mut b, &corners, 5.0, 1).unwrap();
    let a = 5.0 - 5.0 * 0.5_f64.sqrt();
    let rim = [
        Point3::new(148.5, 0.0, 60.0),
        Point3::new(297.0, 50.0, 60.0),
        Point3::new(148.5, 100.0, 60.0),
        Point3::new(0.0, 50.0, 60.0),
        Point3::new(a, a, 60.0),
        Point3::new(297.0 - a, a, 60.0),
        Point3::new(a, 100.0 - a, 60.0),
        Point3::new(297.0 - a, 100.0 - a, 60.0),
    ];
    let before = volume_of(&k, &mut b);
    blend(&mut k, &mut b, &rim, 5.0, 1).unwrap();
    let removed = 754.0 * 25.0 * (1.0 - PI / 4.0) + 4.0 * (PI / 2.0) * (62.5 - 125.0 / 3.0);
    assert_one_sound_solid(&mut k, &mut b, before - removed, 1e-7);
}

// ---- without the flag ------------------------------------------------------------------

/// Without `fillet_gn_section_generator` none of these is sized or judged early: the square
/// top's chamfers fail as before (`SM_ERR`, no refusal), as does the middle edge at r2.
#[test]
fn without_the_flag_the_classes_fail_as_before() {
    let mut k = kernel(false);
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let got = blend(&mut k, &mut b, &top_edges(10.0, 10.0), 5.0, 0);
    assert!(
        matches!(got, Err(SolidError::Upstream { code: 1001, .. })),
        "{got:?}"
    );
    let mut k = kernel(false);
    let mut b = arc_wall_plate_rounded(&mut k);
    let got = blend(&mut k, &mut b, &[MIDDLE], 2.0, 1);
    assert!(
        matches!(got, Err(SolidError::Upstream { code: 1001, .. })),
        "{got:?}"
    );
}
