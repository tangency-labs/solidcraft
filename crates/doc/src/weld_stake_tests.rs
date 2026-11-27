//! Stake welds: a 1 mm top sheet over a web, the weld drawn on the sheet's top. The line snapped
//! onto the web's middle: within the weld tolerance without a word, beyond it with how far, and
//! refused only when no web lies under the weld's width (the renders' 0.05, 0.5, 1.4 and 2.2 mm);
//! a web standing apart, a path near the sheet's edge, an arc over a ring web, a corner. The bodies
//! on a wide web against their closed-form volumes, on a thin web cut to it, every body through
//! the kernel's check; the refusals.

use std::f64::consts::{PI, TAU};

use solidcore_record::Vec3;
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

/// The top sheet: 40 × 40 from z = 0 up 1 mm.
fn skin() -> Body {
    block([-20.0, -20.0, 0.0], [20.0, 20.0, T])
}

/// A straight web along y under the sheet, `t` thick, its top `gap` under it.
fn web(t: f64, gap: f64) -> Body {
    block([-t / 2.0, -15.0, -10.0], [t / 2.0, 15.0, -gap])
}

/// The record face of `b` whose plane holds `p`, square to z.
fn face_at(b: &Body, p: [f64; 3]) -> usize {
    (0..b.face_count()).find(|&f| b.face_square_level(f, Vec3::new(0.0, 0.0, 1.0), p.into()).unwrap().is_some_and(|l| l.level.abs() < 1e-9)).unwrap()
}

/// A line along y on the sheet's top, `off` beside the web's middle.
fn site_on(skin: &Body, web: &Body, off: f64) -> Result<StakeSite> {
    let start = [off, -10.0, T];
    stake_site(skin, face_at(skin, start), web, start, &[SeamPiece::Line { to: [off, 10.0, T] }], W, WELD_TOLERANCE)
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

fn bounds(b: &Body) -> ([f64; 3], [f64; 3]) {
    let bb = b.exact_bounds().unwrap();
    ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z])
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

// ---- where a stake weld sits ----

/// The renders' four lines over a 1.2 mm web: 0.05 mm off is snapped without a word; 0.5 and 1.4
/// mm off are snapped with how far; 2.2 mm off no web lies under the 2.4 mm weld, refused with
/// the nearest web's distance.
#[test]
fn a_line_off_the_web_is_snapped_onto_its_middle_or_refused_when_no_web_is_under_it() {
    let (s, w) = (skin(), web(1.2, 0.0));
    for (off, moved) in [(0.05, None), (0.5, Some(0.5)), (1.4, Some(1.4))] {
        let got = site_on(&s, &w, off).unwrap();
        assert!(near(got.start, [0.0, -10.0, 0.0]), "{off}: {got:?}");
        assert_eq!(got.path, vec![SeamPiece::Line { to: got.path_end() }]);
        assert!(near(got.path_end(), [0.0, 10.0, 0.0]), "{off}: {got:?}");
        assert!(near(got.normal, [0.0, 0.0, -1.0]) && (got.skin - T).abs() < 1e-9, "{got:?}");
        match (got.moved, moved) {
            (None, None) => {}
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-9, "{off}: moved {a}"),
            other => panic!("{off}: {other:?}"),
        }
    }
    let got = site_on(&s, &w, 2.2);
    let Err(StakeRefusal::NoWebUnder { piece: 0, nearest: Some(d) }) = got else {
        panic!("{got:?}");
    };
    assert!((d - 2.2).abs() < 1e-9, "{d}");
}

impl StakeSite {
    fn path_end(&self) -> [f64; 3] {
        match self.path.last() {
            Some(SeamPiece::Line { to } | SeamPiece::Arc { to, .. }) => *to,
            None => panic!("no path"),
        }
    }
}

/// A web standing 0.3 mm under the sheet is refused with the gap; a sheet only 2 mm wide leaves
/// room for a 2 mm weld at most.
#[test]
fn a_web_apart_or_a_line_near_the_edge_is_refused() {
    let got = site_on(&skin(), &web(1.2, 0.3), 0.0);
    let Err(StakeRefusal::WebApart { piece: 0, gap }) = got else {
        panic!("{got:?}");
    };
    assert!((gap - 0.3).abs() < 1e-9, "{gap}");
    let narrow = block([-1.0, -20.0, 0.0], [1.0, 20.0, T]);
    let got = site_on(&narrow, &web(1.2, 0.0), 0.0);
    let Err(StakeRefusal::PastFaceEdge { piece: 0, fits }) = got else {
        panic!("{got:?}");
    };
    assert!((fits - 2.0).abs() < 1e-9, "{fits}");
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
    let got = stake_site(&s, face_at(&s, start), &web, start, &path, W, WELD_TOLERANCE).unwrap();
    assert!(got.moved.is_some_and(|m| (m - 0.5).abs() < 1e-9), "{got:?}");
    assert!(near(got.start, [10.0, 0.0, 0.0]) && near(got.path_end(), [-10.0, 0.0, 0.0]), "{got:?}");
    for piece in built(&got, &s, &web).pieces {
        check(&piece.body, "arc");
    }
}

/// An L web and a path turning its corner: 0.5 mm off is refused at the corner; 0.05 mm off is
/// within the tolerance, kept as drawn, and built with the corner's notice.
#[test]
fn a_path_with_a_corner_is_kept_within_the_tolerance_and_refused_beyond_it() {
    let a = block([-0.6, -15.0, -10.0], [0.6, 0.6, 0.0]);
    let b = block([-0.6, -0.6, -10.0], [15.0, 0.6, 0.0]);
    let web = solidcraft_kernel::boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    let s = skin();
    let corner = |off: f64| {
        let start = [off, -10.0, T];
        let path = [SeamPiece::Line { to: [off, -off, T] }, SeamPiece::Line { to: [10.0, -off, T] }];
        stake_site(&s, face_at(&s, start), &web, start, &path, W, WELD_TOLERANCE)
    };
    let got = corner(0.5);
    let Err(StakeRefusal::SnapAtCorner { joint: 1, by }) = got else {
        panic!("{got:?}");
    };
    assert!((by - 0.5).abs() < 1e-9, "{by}");
    let got = corner(0.05).unwrap();
    assert!(got.moved.is_none() && near(got.start, [0.05, -10.0, 0.0]), "{got:?}");
    let stake = built(&got, &s, &web);
    assert_eq!(stake.notices.len(), 1, "{:?}", stake.notices);
    for piece in stake.pieces {
        check(&piece.body, "corner");
    }
}

// ---- the bodies ----

/// The tapered zone's radius `d` below the sheet's top: 1.2 there to 0.42 at its bottom, 2 mm down.
fn radius(d: f64) -> f64 {
    0.5 * W - (0.5 * W - 0.175 * W) * d / (2.0 * T)
}

/// The zone's volume between `d0` and `d1` below the top along a 20 mm line: its trapezoid swept
/// along the line, and a whole frustum for the two round ends.
fn zone(d0: f64, d1: f64) -> f64 {
    let (r0, r1) = (radius(d0), radius(d1));
    (r0 + r1) * (d1 - d0) * 20.0 + PI * (d1 - d0) / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1)
}

/// On a web 4 mm thick the zone is whole: its part in the sheet and its part in the web against
/// their closed forms. On the 1.2 mm web the web's part is cut to the web, the sheet's unchanged.
#[test]
fn the_zone_is_cut_to_the_metal() {
    let s = skin();
    for (t, whole) in [(4.0, true), (1.2, false)] {
        let w = web(t, 0.0);
        let stake = built(&site_on(&s, &w, 0.0).unwrap(), &s, &w);
        assert_eq!(stake.pieces.len(), 3);
        for piece in &stake.pieces {
            check(&piece.body, &format!("web {t}"));
        }
        let [cap, in_skin, in_web] = stake.pieces.as_slice() else {
            panic!("{} pieces", stake.pieces.len());
        };
        assert_eq!((cap.zone, in_skin.side, in_web.side), (SpotZone::Core, SpotSide::Lower, SpotSide::Upper));
        let want = zone(0.0, T);
        assert!((volume(&in_skin.body) - want).abs() <= 1e-6 * want, "skin part {} != {want}", volume(&in_skin.body));
        let (lo, hi) = bounds(&in_web.body);
        assert!(lo[2] > -T - 1e-9 && hi[2] < 1e-9, "web part {lo:?}..{hi:?}");
        let want = zone(T, 2.0 * T);
        if whole {
            assert!((volume(&in_web.body) - want).abs() <= 1e-6 * want, "web part {} != {want}", volume(&in_web.body));
        } else {
            assert!(volume(&in_web.body) < want && lo[0] > -0.6 - 1e-9 && hi[0] < 0.6 + 1e-9, "web part {lo:?}..{hi:?}");
        }
    }
}

/// A width, depth or skin of no size, a face the sheet does not have, a tolerance of none are bad
/// input.
#[test]
fn bad_stake_welds_are_refused_as_invalid_input() {
    let (s, w) = (skin(), web(1.2, 0.0));
    let line = [SeamPiece::Line { to: [0.0, 10.0, T] }];
    let start = [0.0, -10.0, T];
    let f = face_at(&s, start);
    assert!(matches!(stake_site(&s, 99, &w, start, &line, W, WELD_TOLERANCE), Err(StakeRefusal::Invalid(_))));
    assert!(matches!(stake_site(&s, f, &w, start, &line, 0.0, WELD_TOLERANCE), Err(StakeRefusal::Invalid(_))));
    assert!(matches!(stake_site(&s, f, &w, start, &line, W, 0.0), Err(StakeRefusal::Invalid(_))));
    let site = site_on(&s, &w, 0.0).unwrap();
    let ok = StakeWeld { start: site.start, path: site.path, normal: site.normal, skin: T, depth: T, width: W, shape: FusionShape::Tapered };
    for weld in [StakeWeld { depth: 0.0, ..ok.clone() }, StakeWeld { width: f64::NAN, ..ok.clone() }, StakeWeld { skin: -1.0, ..ok }] {
        assert!(matches!(stake_weld(&weld, &s, &w), Err(StakeRefusal::Invalid(_))), "{weld:?}");
    }
}
