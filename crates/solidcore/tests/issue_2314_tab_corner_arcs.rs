//! tangency-labs/solidcore#2314: a tabbed plate's two R2 corner arcs rounded r 0.5 together
//! (the tab's 4 mm straight top between them, no vertex shared), with WR-706
//! (`UpstreamFixes::fillet_section_end_trim_at_creases`).
//!
//! The plate is #2223's. Each corner arc runs on tangentially into the tab's side at one end
//! and into the tab's top at the other, so WR-674 ends each round at its two sections, and
//! WR-706 cuts and closes each section end in the trim. The arc at x > 0 ends on the plane
//! y = 280/9 (its side end, at (4, 280/9)) and so does the arc at x < 0 (at (-4, 280/9)):
//! the closing of a section end found the round's section among the whole sheet's lamina
//! edges, found both rounds' sections on that plane and refused the pair (1001,
//! `TangentContinuationEndUnsupported`). It now looks only at the end's own round's faces.
//!
//! Expected values are closed form (#2223's file header): each arc's round removes the
//! Pappus wedge (1 - pi/4) r^2 (pi/2) (2 - c), c = r (10 - 3 pi) / (3 (4 - pi)); the two
//! wedges lie at opposite corners of the tab and are disjoint, so the pair removes twice it.
//! With the R25 rim as well, the rim's 7.958672926831696 (#2176) is added: its round ends on
//! the tab's sides at y = sqrt(609) < 280/9, clear of both wedges.
//!
//! With `fillet_blend_corner` (WR-697, in this test's set of fixes) the fork's End at a
//! smooth vertex (smlib#124, 41a39ec) ends each corner arc's round at its smooth vertices on
//! the planes square to it, before WR-674's section ends are reached: the same wedges. The
//! tests of the path without it turn the blend corner off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::blend_refusal::BlendRefusal;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};
use std::f64::consts::PI;

const RADIUS: f64 = 0.5;
/// A point on the top rim arc.
const RIM: [f64; 3] = [0.0, -25.0, 2.0];
/// A point on the R2 corner arc at x < 0 (#2314's `CORNER_L`).
const CORNER_L: [f64; 3] = [-3.414_213_56, 32.525_324_7, 2.0];
/// A point on the R2 corner arc at x > 0 (#2314's `CORNER`).
const CORNER: [f64; 3] = [3.414_213_56, 32.525_324_7, 2.0];
/// What the rim's round removes alone (#2176's `the_neighbours_keep_their_rounds`, which
/// pins the fork's 7.958672926831696: 18e96f1 and 63c7962, `SmApiFilletEdges` r 0.5).
const RIM_REMOVED: f64 = 7.958_672_926_831_696;

/// Every opt-in fix on (as in the issue), with WR-706 and the blend corner (WR-697) as given.
fn kernel_with(wr706: bool, blend_corner: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        *f.flag_mut(name).unwrap() = true;
    }
    f.fillet_section_end_trim_at_creases = wr706;
    f.fillet_blend_corner = blend_corner;
    k.fixes = f;
    k
}

/// The volume to the fork's comparison accuracy (1e-10, as #2176's `fine_volume`).
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The edge nearest `p`.
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

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        create_arc(
            Vec3::new(c.0, c.1, 0.0),
            r,
            from.to_degrees(),
            to.to_degrees(),
        )
        .unwrap(),
    )
}

/// R25 disc with a tab (sides x = +-4, R2 corners, top y = 280/9 + 2), 2 thick.
fn plate(k: &mut Kernel) -> Brep {
    let t = (4.0_f64 / 25.0).acos();
    let y = (625.0_f64 - 16.0).sqrt();
    let cy = 280.0 / 9.0;
    let curves = [
        arc((0.0, 0.0), 25.0, PI - t - 2.0 * PI, t),
        line((4.0, y), (4.0, cy)),
        arc((2.0, cy), 2.0, 0.0, PI / 2.0),
        line((2.0, cy + 2.0), (-2.0, cy + 2.0)),
        arc((-2.0, cy), 2.0, PI / 2.0, PI),
        line((-4.0, cy), (-4.0, y)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap()
}

/// The corner arc's wedge, by Pappus (the file header).
fn pappus() -> f64 {
    let c = RADIUS * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    (1.0 - PI / 4.0) * RADIUS * RADIUS * (PI / 2.0) * (2.0 - c)
}

/// The fillet of the edges nearest `picks`: the volume it removes and the body's face count,
/// the body checked sound.
fn removed(wr706: bool, picks: &[[f64; 3]]) -> Result<(f64, usize), SolidError> {
    removed_with(wr706, true, picks)
}

/// As [`removed`], with the blend corner (WR-697) as given.
fn removed_with(
    wr706: bool,
    blend_corner: bool,
    picks: &[[f64; 3]],
) -> Result<(f64, usize), SolidError> {
    let mut k = kernel_with(wr706, blend_corner);
    let mut b = plate(&mut k);
    let before = volume(&k, &mut b);
    let es: Vec<EdgeId> = picks.iter().map(|p| pick(&k, &b, *p)).collect();
    fillet_edges(&mut k, &mut b, &es, RADIUS, 1, 1, 1.0)?;
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    Ok((before - volume(&k, &mut b), b.face_list().len()))
}

#[test]
fn the_pappus_wedge_is_the_closed_form() {
    assert!(
        (pappus() - 0.159_135_839_721_916).abs() <= 1e-15,
        "{}",
        pappus()
    );
}

/// The issue's case: both corner arcs rounded, twice the wedge, a sound manifold solid with
/// the plate's 8 faces, the two rounds and their four flat ends.
#[test]
fn both_corner_arcs_round_together() {
    let (both, faces) = removed(true, &[CORNER, CORNER_L]).unwrap();
    let expected = 2.0 * pappus();
    assert!(
        (both - expected).abs() <= 1e-7,
        "removed {both}, two wedges {expected}"
    );
    assert_eq!(faces, 14);
}

/// Picked the other way round, the same body.
#[test]
fn both_corner_arcs_picked_the_other_way_round() {
    let (both, faces) = removed(true, &[CORNER_L, CORNER]).unwrap();
    assert!((both - 2.0 * pappus()).abs() <= 1e-7, "removed {both}");
    assert_eq!(faces, 14);
}

/// The pair is the sum of each arc alone, each the Pappus wedge. With
/// `fillet_blend_corner` (in this test's set of fixes) each arc ends at its two smooth
/// vertices on the planes square to it (the fork's End at a smooth vertex, smlib#124
/// 41a39ec), not at WR-674's section ends: the fork at 18e96f1 and 63c7962 removes
/// 0.159135839575356 for the arc at x > 0 and 0.159135839836381 for the one at x < 0.
#[test]
fn the_pair_is_the_sum_of_each_alone() {
    let (a, fa) = removed(true, &[CORNER]).unwrap();
    let (b, fb) = removed(true, &[CORNER_L]).unwrap();
    // from the Pappus wedge: x > 0 1.12e-10 less, x < 0 6.0e-11 more (asserted a decade
    // above, so a later drift is caught)
    assert!((a - pappus()).abs() <= 1.2e-9, "x > 0 alone {a}");
    assert!((b - pappus()).abs() <= 6e-10, "x < 0 alone {b}");
    // pinned at the values measured here: the fork at 18e96f1 and 63c7962
    // (`SmApiFilletEdges`, r 0.5, section 1, continuity 1) removes 0.159135839575356 and
    // 0.159135839836381; these lie 3.5e-11 and 5.4e-11 from it because of the
    // solidcore-native exact spine
    assert!(
        (a - 0.159_135_839_610_371_48).abs() <= 1e-12,
        "x > 0 alone {a}"
    );
    assert!(
        (b - 0.159_135_839_782_265_98).abs() <= 1e-12,
        "x < 0 alone {b}"
    );
    assert_eq!((fa, fb), (11, 11));
    let (both, _) = removed(true, &[CORNER, CORNER_L]).unwrap();
    assert!((both - a - b).abs() <= 1e-7, "{both} vs {}", a + b);
}

/// The rim and both corner arcs (the issue's third row): the rim's round plus two wedges,
/// with the plate's 8 faces, three rounds and the corners' four flat ends.
#[test]
fn rim_and_both_corner_arcs_round_together() {
    let (all, faces) = removed(true, &[RIM, CORNER, CORNER_L]).unwrap();
    let expected = RIM_REMOVED + 2.0 * pappus();
    assert!(
        (all - expected).abs() <= 1e-7,
        "removed {all}, rim + two wedges {expected}"
    );
    assert_eq!(faces, 15);
}

/// Without WR-706 (and without the blend corner, whose End at a smooth vertex ends the
/// rounds before WR-674's section ends do) the subtraction still refuses the pair (it builds
/// only a lone round with section ends), as before: the session's typed refusal of a round
/// that cannot end where its edge runs on tangentially, with upstream's status.
#[test]
fn without_the_fix_the_pair_is_refused() {
    let err = removed_with(false, false, &[CORNER, CORNER_L]).unwrap_err();
    assert!(
        matches!(
            err,
            SolidError::BlendRefused {
                code: 1001,
                refusal: BlendRefusal::ContinuesInto { .. },
                ..
            }
        ),
        "{err:?}"
    );
}
