//! Stake welds: a 1 mm top sheet over a web, the weld drawn on the sheet's top. The line snapped
//! onto the web's middle: within the weld tolerance without a word, beyond it with how far, and
//! refused only when no web lies under the weld's width (the renders' 0.05, 0.5, 1.4 and 2.2 mm);
//! a web wall split by an edge under the line across the start; an arc over a ring web; a corner
//! moved to where the moved pieces meet. The snapped path checked whole: a hole in the sheet, a
//! pocket in its underside and a hole in the web between the ends (built with a notice), a snap
//! that leaves the sheet, a web apart, overlapping (each built with a notice, refused out of reach)
//! or missing. The bodies on a wide web and on a thin one, cut to it, against
//! their closed-form volumes, every body through the kernel's check; the refusals.

use std::f64::consts::{PI, TAU};

use solidcraft_kernel::profiles::ProfileCurve;
use solidcraft_kernel::{SmlibCheck, box_solid, validity};

use super::*;
use crate::weld_seam::Turn;

/// The top sheet's thickness, the weld's width.
const T: f64 = 1.0;
const W: f64 = 2.4;

fn block(lo: [f64; 3], hi: [f64; 3]) -> Body {
    box_solid(Vec3::from(lo), Vec3::from(hi)).unwrap()
}

fn cut(a: &Body, b: &Body) -> Body {
    solidcraft_kernel::boolean(a, b, BoolOp::Cut).unwrap().unwrap()
}

/// The top sheet: 40 × 40 from z = 0 up 1 mm.
fn skin() -> Body {
    block([-20.0, -20.0, 0.0], [20.0, 20.0, T])
}

/// A straight web along y under the sheet, `t` thick, its top at `top`.
fn web(t: f64, top: f64) -> Body {
    block([-t / 2.0, -15.0, -10.0], [t / 2.0, 15.0, top])
}

/// The record face of `b` whose plane holds `p`, square to z.
fn face_at(b: &Body, p: [f64; 3]) -> usize {
    (0..b.face_count()).find(|&f| b.face_square_level(f, Vec3::new(0.0, 0.0, 1.0), p.into()).unwrap().is_some_and(|l| l.level.abs() < 1e-9)).unwrap()
}

/// A line along y on the sheet's top at x = `x`.
fn site_on(skin: &Body, web: &Body, x: f64) -> Result<StakeSite> {
    let start = [x, -10.0, T];
    stake_site(skin, face_at(skin, start), web, start, &[SeamPiece::Line { to: [x, 10.0, T] }], W, StakeTolerances::DEFAULT)
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

fn close(got: f64, want: f64) -> bool {
    (got - want).abs() < 1e-9
}

fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

fn built(site: &StakeSite, skin: &Body, web: &Body) -> Stake {
    let weld = StakeWeld {
        start: site.start,
        path: site.path.clone(),
        normal: site.normal,
        skin: site.skin,
        depth: site.skin,
        width: W,
        shape: FusionShape::Tapered,
    };
    stake_weld(&weld, skin, web).unwrap()
}

/// How far the line was moved, if the site says so.
fn moved(site: &StakeSite) -> Option<f64> {
    site.notices.iter().find_map(|n| if let StakeNotice::Moved { by } = *n { Some(by) } else { None })
}

impl StakeSite {
    fn path_end(&self) -> [f64; 3] {
        match self.path.last() {
            Some(SeamPiece::Line { to } | SeamPiece::Arc { to, .. }) => *to,
            None => panic!("no path"),
        }
    }
}

// ---- where a stake weld sits ----

/// The renders' four lines over a 1.2 mm web: 0.05 mm off is snapped without a word; 0.5 and 1.4
/// mm off are snapped with how far; 2.2 mm off no web lies under the 2.4 mm weld, refused with
/// the nearest web's distance.
#[test]
fn a_line_off_the_web_is_snapped_onto_its_middle_or_refused_when_no_web_is_under_it() {
    let (s, w) = (skin(), web(1.2, 0.0));
    for (off, want) in [(0.05, None), (0.5, Some(0.5)), (1.4, Some(1.4))] {
        let got = site_on(&s, &w, off).unwrap();
        assert!(near(got.start, [0.0, -10.0, 0.0]) && near(got.path_end(), [0.0, 10.0, 0.0]), "{off}: {got:?}");
        assert!(near(got.normal, [0.0, 0.0, -1.0]) && close(got.skin, T), "{got:?}");
        match (moved(&got), want) {
            (None, None) => {}
            (Some(a), Some(b)) => assert!(close(a, b), "{off}: moved {a}"),
            other => panic!("{off}: {other:?}"),
        }
    }
    let got = site_on(&s, &w, 2.2);
    let Err(StakeRefusal::NoWebUnder { nearest: Some(d) }) = got else {
        panic!("{got:?}");
    };
    assert!(close(d, 2.2), "{d}");
}

/// A web of two blocks overlapping from y = -11 to -9: the line across the start (y = -10) may
/// pass where the blocks' walls meet. Its crossings are counted once each and the stretches
/// classified, so the web's middle is still found.
#[test]
fn a_web_wall_split_under_the_start_still_gives_the_web_s_middle() {
    let a = block([-0.6, -15.0, -10.0], [0.6, -9.0, 0.0]);
    let b = block([-0.6, -11.0, -10.0], [0.6, 15.0, 0.0]);
    let w = solidcraft_kernel::boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    let got = site_on(&skin(), &w, 0.5).unwrap();
    assert!(near(got.start, [0.0, -10.0, 0.0]) && moved(&got).is_some_and(|m| close(m, 0.5)), "{got:?}");
}

/// A half circle drawn at radius 10.5 over a ring web whose middle is at radius 10: the arc is
/// snapped to radius 10, 0.5 mm, and built.
#[test]
fn an_arc_over_a_ring_web_is_snapped_to_the_web_s_radius() {
    let ring = [[9.4, 0.0, -10.0], [10.6, 0.0, -10.0], [10.6, 0.0, 0.0], [9.4, 0.0, 0.0]];
    let profile: Vec<ProfileCurve> = (0..4).map(|i| ProfileCurve::Line { start: ring[i], end: ring[(i + 1) % 4] }).collect();
    let web = profiles::revolve_curves(&profile, [0.0; 3], [0.0, 0.0, 1.0], TAU, true, None).unwrap();
    let s = skin();
    let start = [10.5, 0.0, T];
    let path = [SeamPiece::Arc { center: [0.0, 0.0, T], to: [-10.5, 0.0, T], turn: Turn::Left }];
    let got = stake_site(&s, face_at(&s, start), &web, start, &path, W, StakeTolerances::DEFAULT).unwrap();
    assert!(moved(&got).is_some_and(|m| close(m, 0.5)), "{got:?}");
    assert!(near(got.start, [10.0, 0.0, 0.0]) && near(got.path_end(), [-10.0, 0.0, 0.0]), "{got:?}");
    for piece in built(&got, &s, &web).pieces {
        check(&piece.body, "arc");
    }
}

/// An L web and a path turning its corner 0.5 mm off its middle: both legs move 0.5 mm, the corner
/// to where they meet, (0, 0); built, with the corner's notice. 0.05 mm off, within the
/// tolerance, the path is kept as drawn.
#[test]
fn a_path_with_a_corner_is_moved_to_where_its_moved_pieces_meet() {
    let a = block([-0.6, -15.0, -10.0], [0.6, 0.6, 0.0]);
    let b = block([-0.6, -0.6, -10.0], [15.0, 0.6, 0.0]);
    let web = solidcraft_kernel::boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    let s = skin();
    let corner = |off: f64| {
        let start = [off, -10.0, T];
        let path = [SeamPiece::Line { to: [off, -off, T] }, SeamPiece::Line { to: [10.0, -off, T] }];
        stake_site(&s, face_at(&s, start), &web, start, &path, W, StakeTolerances::DEFAULT)
    };
    let got = corner(0.5).unwrap();
    assert!(moved(&got).is_some_and(|m| close(m, 0.5)), "{got:?}");
    assert!(near(got.start, [0.0, -10.0, 0.0]) && near(got.path_end(), [10.0, 0.0, 0.0]), "{got:?}");
    assert!(matches!(got.path.first(), Some(SeamPiece::Line { to }) if near(*to, [0.0, 0.0, 0.0])), "{got:?}");
    let stake = built(&got, &s, &web);
    assert_eq!(stake.notices.len(), 1, "{:?}", stake.notices);
    for piece in stake.pieces {
        check(&piece.body, "corner");
    }
    let got = corner(0.05).unwrap();
    assert!(moved(&got).is_none() && near(got.start, [0.05, -10.0, 0.0]), "{got:?}");
}

/// A 2 × 1 mm hole through the sheet beside the line, 0.5 mm from it, between the line's ends:
/// a weld 1 mm wide at most. A line drawn 1.3 mm in from the sheet's edge over a web whose middle is 1.5 mm past
/// it is snapped off the sheet: refused.
#[test]
fn a_hole_in_the_sheet_or_a_snap_off_the_sheet_is_refused() {
    let holed = cut(&skin(), &block([0.5, -8.0, -1.0], [2.5, -7.0, 2.0]));
    let got = site_on(&holed, &web(1.2, 0.0), 0.0);
    let Err(StakeRefusal::PastFaceEdge { piece: 0, fits }) = got else {
        panic!("{got:?}");
    };
    assert!((fits - 1.0).abs() < 1e-6, "{fits}");
    let edge = block([-20.0, -20.0, 0.0], [0.0, 20.0, T]);
    let wide = block([-0.5, -15.0, -10.0], [3.5, 15.0, 0.0]);
    assert_eq!(site_on(&edge, &wide, -1.3), Err(StakeRefusal::OffFace));
}

/// A sheet stepped up to 3 mm past x = 0, the line drawn on its 1 mm part 1.3 mm from the step over
/// a web whose middle is 1.5 mm past it: the snap leaves the picked face for the taller part,
/// refused, though the sheet is there. A right-turning half circle of radius 0.5 over a web whose
/// middle is 0.9 mm towards its centre would turn inside out: refused.
#[test]
fn a_snap_off_the_picked_face_or_through_an_arc_s_centre_is_refused() {
    let low = block([-20.0, -20.0, 0.0], [0.1, 20.0, T]);
    let high = block([0.0, -20.0, 0.0], [20.0, 20.0, 3.0]);
    let stepped = solidcraft_kernel::boolean(&low, &high, BoolOp::Union).unwrap().unwrap();
    let wide = block([-0.5, -15.0, -10.0], [3.5, 15.0, 0.0]);
    assert_eq!(site_on(&stepped, &wide, -1.3), Err(StakeRefusal::OffFace));
    let s = skin();
    let start = [0.5, 0.0, T];
    let path = [SeamPiece::Arc { center: [0.0, 0.0, T], to: [-0.5, 0.0, T], turn: Turn::Right }];
    let off = block([-2.4, -15.0, -10.0], [1.6, 15.0, 0.0]);
    let got = stake_site(&s, face_at(&s, start), &off, start, &path, W, StakeTolerances::DEFAULT);
    assert!(matches!(got, Err(StakeRefusal::Invalid(_))), "{got:?}");
}

/// A pocket 0.4 mm deep in the sheet's underside between the line's ends: built, with the sheet
/// uneven by 0.4 as its notice. A hole through the web between the ends, which the path lowered
/// 0.5 mm into the web runs through: built, the web 0.5 nearer than at the start as its notice.
#[test]
fn a_pocket_in_the_sheet_or_a_hole_in_the_web_between_the_ends_is_built_with_a_notice() {
    let pocketed = cut(&skin(), &block([-1.0, 2.0, -1.0], [1.0, 4.0, 0.4]));
    let got = site_on(&pocketed, &web(1.2, 0.0), 0.0).unwrap();
    assert!(matches!(got.notices.as_slice(), [StakeNotice::SkinUneven { piece: 0, by }] if (by - 0.4).abs() < 1e-6), "{got:?}");
    let holed = cut(&web(1.2, 0.0), &block([-1.0, 2.0, -0.8], [1.0, 4.0, -0.2]));
    let got = site_on(&skin(), &holed, 0.0).unwrap();
    assert!(matches!(got.notices.as_slice(), [StakeNotice::WebNotAlong { piece: 0, by }] if (by - 0.5).abs() < 1e-6), "{got:?}");
}

/// The web's top under the 1 mm sheet: 0.05 mm, within 10 % of the sheet, taken without a word;
/// 0.3 and 0.6 mm (past half the sheet's thickness, where the line across is probed) built with
/// the gap as its notice; 1.2 mm, past the zone's reach, refused. 0.05 mm up into the sheet, within
/// the tolerance: no word. 0.3 mm up: built, with the overlap as its notice. Up through the sheet:
/// refused. A web nowhere across the start: none.
#[test]
fn a_web_apart_or_overlapping_is_built_with_how_far_and_refused_out_of_reach() {
    let s = skin();
    assert!(site_on(&s, &web(1.2, -0.05), 0.0).is_ok_and(|g| g.notices.is_empty()));
    for gap in [0.3, 0.6] {
        let w = web(1.2, -gap);
        let got = site_on(&s, &w, 0.0).unwrap();
        assert!(matches!(got.notices.as_slice(), [StakeNotice::Gap { gap: g }] if (g - gap).abs() < 1e-6), "{gap}: {got:?}");
        for piece in built(&got, &s, &w).pieces {
            check(&piece.body, "gap");
        }
    }
    let got = site_on(&s, &web(1.2, -1.2), 0.0);
    assert!(matches!(got, Err(StakeRefusal::WebApart { gap, .. }) if (gap - 1.2).abs() < 1e-6), "{got:?}");
    assert!(site_on(&s, &web(1.2, 0.05), 0.0).is_ok_and(|g| g.notices.is_empty()));
    let w = web(1.2, 0.3);
    let got = site_on(&s, &w, 0.0).unwrap();
    assert!(matches!(got.notices.as_slice(), [StakeNotice::Overlap { by }] if (by - 0.3).abs() < 1e-6), "{got:?}");
    for piece in built(&got, &s, &w).pieces {
        check(&piece.body, "overlap");
    }
    let got = site_on(&s, &web(1.2, 1.2), 0.0);
    assert!(matches!(got, Err(StakeRefusal::WebOverlap { by }) if (by - 1.2).abs() < 1e-6), "{got:?}");
    let across = block([-15.0, 5.0, -10.0], [15.0, 6.0, 0.0]);
    assert_eq!(site_on(&s, &across, 0.0), Err(StakeRefusal::NoWebUnder { nearest: None }));
}

// ---- the bodies ----

/// The tapered zone's radius `d` below the sheet's top: 1.2 there to 0.42 at its bottom, 2 mm down.
fn radius(d: f64) -> f64 {
    0.5 * W - (0.5 * W - 0.175 * W) * d / (2.0 * T)
}

/// The area of a circle of radius `r` within `|x| <= a`.
fn disc_in_strip(r: f64, a: f64) -> f64 {
    if r <= a { PI * r * r } else { 2.0 * (r * r * (a / r).asin() + a * (r * r - a * a).sqrt()) }
}

/// The zone between `d0` and `d1` below the top along a 20 mm line, within `|x| <= a`: its
/// trapezoid across the line (width `min(2r, 2a)`) swept 20 mm, and the two round ends together a
/// whole frustum clipped to the strip; integrated by Simpson's rule over depth.
fn zone(d0: f64, d1: f64, a: f64) -> f64 {
    let n = 20_000;
    let h = (d1 - d0) / f64::from(n);
    let f = |d: f64| (2.0 * radius(d)).min(2.0 * a) * 20.0 + disc_in_strip(radius(d), a);
    let mut sum = f(d0) + f(d1);
    for i in 1..n {
        sum += f(d0 + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// On a web 4 mm thick the zone is whole; on the 1.2 mm web its part in the web is cut to the web.
/// Both parts against their closed forms; the sheet's part the same on both.
#[test]
fn the_zone_is_cut_to_the_metal() {
    let s = skin();
    for t in [4.0, 1.2] {
        let w = web(t, 0.0);
        let stake = built(&site_on(&s, &w, 0.0).unwrap(), &s, &w);
        for piece in &stake.pieces {
            check(&piece.body, &format!("web {t}"));
        }
        let [cap, in_skin, in_web] = stake.pieces.as_slice() else {
            panic!("{} pieces", stake.pieces.len());
        };
        assert_eq!((cap.zone, in_skin.side, in_web.side), (SpotZone::Core, SpotSide::Lower, SpotSide::Upper));
        for (got, want, what) in [(volume(&in_skin.body), zone(0.0, T, 20.0), "skin"), (volume(&in_web.body), zone(T, 2.0 * T, t / 2.0), "web")] {
            assert!((got - want).abs() <= 1e-6 * want, "web {t}, {what} part {got} != {want}");
        }
    }
}

/// A width, depth or skin of no size, a face the sheet does not have, a tolerance of none, a start
/// off the picked face are bad input.
#[test]
fn bad_stake_welds_are_refused() {
    let (s, w) = (skin(), web(1.2, 0.0));
    let line = [SeamPiece::Line { to: [0.0, 10.0, T] }];
    let start = [0.0, -10.0, T];
    let f = face_at(&s, start);
    assert!(matches!(stake_site(&s, 99, &w, start, &line, W, StakeTolerances::DEFAULT), Err(StakeRefusal::Invalid(_))));
    assert!(matches!(stake_site(&s, f, &w, start, &line, 0.0, StakeTolerances::DEFAULT), Err(StakeRefusal::Invalid(_))));
    assert!(matches!(
        stake_site(&s, f, &w, start, &line, W, StakeTolerances { drift: 0.0, ..StakeTolerances::DEFAULT }),
        Err(StakeRefusal::Invalid(_))
    ));
    let above = [0.0, -10.0, T + 0.5];
    assert_eq!(
        stake_site(&s, f, &w, above, &[SeamPiece::Line { to: [0.0, 10.0, T + 0.5] }], W, StakeTolerances::DEFAULT),
        Err(StakeRefusal::OffFace)
    );
    let site = site_on(&s, &w, 0.0).unwrap();
    let ok = StakeWeld { start: site.start, path: site.path, normal: site.normal, skin: T, depth: T, width: W, shape: FusionShape::Tapered };
    for weld in [StakeWeld { depth: 0.0, ..ok.clone() }, StakeWeld { width: f64::NAN, ..ok.clone() }, StakeWeld { skin: -1.0, ..ok }] {
        assert!(matches!(stake_weld(&weld, &s, &w), Err(StakeRefusal::Invalid(_))), "{weld:?}");
    }
}
