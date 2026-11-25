//! Spot welds: every style's bodies against their closed-form volumes at several sizes, the
//! kernel's check on every body, the ball that does not reach a face, the pin's dishes, the
//! placement at any spot and normal, stacks of three and four sheets, the refusals, the named
//! looks; and the site of a spot picked on stacks of boxes, with every typed refusal of a stack
//! that does not take it. Moved from solidcore's `weld_spot`, `weld_spot_site` and ops'
//! `weld_spot_looks` tests (#18409).

use std::f64::consts::PI;

use solidcraft_kernel::affine;
use solidcraft_kernel::services::boolean::union_copies;
use solidcraft_kernel::services::{CheckLevel, ExactBody as Body, Order, make_box, make_sphere};

use super::*;

const T_U: f64 = 1.0;
const T_L: f64 = 1.5;
const D: f64 = 5.0;

/// Six styles, the looks' own: marks; a ball of d with marks where it doesn't reach a face; marks
/// and a small ball; marks and a thin pin; marks and a pin with dished ends; dished marks.
const STYLES: [SpotStyle; 6] = [
    SpotLook::Marks.style(),
    SpotLook::Ball.style(),
    SpotLook::MarksSmallBall.style(),
    SpotLook::MarksThinPin.style(),
    SpotLook::MarksPinConcaveEnds.style(),
    SpotLook::Dent.style(),
];

/// Sheets and diameters (upper, lower, d): the usual spot, equal sheets, a large spot, a spot under
/// `0.6` of the sheet (the pin's dish held to a half sphere), a tiny one, and two thick-plate spots
/// at the proposed `5 √t` (the ball hidden in both sheets, in one).
const SIZES: [(f64, f64, f64); 7] = [
    (T_U, T_L, D),
    (2.0, 2.0, 7.0),
    (1.0, 1.5, 40.0),
    (3.0, 3.0, 1.0),
    (1.0, 1.0, 0.05),
    (8.0, 8.0, 14.142_135_623_730_951),
    (1.0, 8.0, 14.142_135_623_730_951),
];

const NONE: SpotStyle = SpotStyle { marks: None, ball: None, pin: None };

fn volume(b: &Body) -> f64 {
    b.volume(1e-9).unwrap()
}

/// The kernel's check, every test.
fn check(b: &Body, what: &str) {
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{what}: {}", report.text);
}

/// The lowest and highest height (along +Z) of a body: its tight box.
fn extent(b: &Body) -> (f64, f64) {
    let (lo, hi) = b.bounds().unwrap();
    (lo[2], hi[2])
}

fn spot(style: SpotStyle) -> SpotWeld {
    SpotWeld { at: [0.0; 3], normal: [0.0, 0.0, 1.0], thicknesses: vec![T_L, T_U], diameter: D, style }
}

fn sized(style: SpotStyle, (t_u, t_l, d): (f64, f64, f64)) -> SpotWeld {
    SpotWeld { thicknesses: vec![t_l, t_u], diameter: d, ..spot(style) }
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-6 * want.abs(), "{what}: volume {got} != {want}");
}

/// A flat ring between radii `a` and `b`, `h` high.
fn ring(a: f64, b: f64, h: f64) -> f64 {
    PI * (b * b - a * a) * h
}

/// A dished zone between radii `a` and `b`: the volume between the face and a sphere's cap that
/// stands `rim` off the face at radius `ro` and `middle` on the axis.
fn dish_zone(a: f64, b: f64, rim: f64, middle: f64, ro: f64) -> f64 {
    let c = rim - middle;
    let rs = (ro * ro + c * c) / (2.0 * c);
    let zc = middle + rs;
    PI * zc * (b * b - a * a) + 2.0 * PI / 3.0 * ((rs * rs - b * b).powf(1.5) - (rs * rs - a * a).powf(1.5))
}

/// The marks' three zone volumes (core, band, edge) on one face.
fn zone_volumes(m: &SpotMarks, d: f64) -> [f64; 3] {
    let r = [0.0, m.core * d / 2.0, m.band * d / 2.0, m.edge * d / 2.0];
    let zone = |i: usize| match m.top {
        MarkTop::Flat { thickness } => ring(r[i], r[i + 1], thickness * d),
        MarkTop::Dish { rim, middle } => dish_zone(r[i], r[i + 1], rim * d, middle * d, r[3]),
    };
    [zone(0), zone(1), zone(2)]
}

/// The closed-form volume of all of a style's bodies on sheets `t_u`, `t_l` at diameter `d`: marks
/// on the faces they are drawn on, a ball, a pin less each end's spherical cap.
fn style_volume(s: &SpotStyle, (t_u, t_l, d): (f64, f64, f64)) -> f64 {
    let mut v = 0.0;
    let ball = s.ball.map(|b| b.diameter * d / 2.0);
    if let Some(r) = ball {
        v += 4.0 / 3.0 * PI * r.powi(3);
    }
    if let Some(p) = s.pin {
        let r = p.diameter * d / 2.0;
        let cap = |t: f64| {
            p.dish.map_or(0.0, |f| {
                let c = (f * t).min(r);
                PI * c * (3.0 * r * r + c * c) / 6.0
            })
        };
        v += PI * r * r * (t_u + t_l) - cap(t_u) - cap(t_l);
    }
    if let Some(m) = s.marks {
        let one: f64 = zone_volumes(&m, d).iter().sum();
        for t in [t_u, t_l] {
            if m.faces == MarkFaces::Both || ball.is_none_or(|r| r <= t) {
                v += one;
            }
        }
    }
    v
}

fn pieces(s: &SpotWeld) -> Vec<SpotPiece> {
    spot_weld(s).unwrap()
}

/// The flat marks lie on both outer faces as three zones, each its closed-form ring.
#[test]
fn the_marks_lie_on_both_outer_faces_as_core_band_and_edge() {
    let got = pieces(&spot(STYLES[0]));
    assert_eq!(got.len(), 6);
    let want = zone_volumes(&MARKS, D);
    for (i, piece) in got.iter().enumerate() {
        let side = if i < 3 { SpotSide::Upper } else { SpotSide::Lower };
        let zone = [SpotZone::Core, SpotZone::Band, SpotZone::Edge][i % 3];
        assert_eq!((piece.side, piece.zone), (side, zone));
        close(volume(&piece.body), want[i % 3], "zone");
    }
}

/// Every style at every size in `SIZES` against its closed form: balls, pins, the dished pin ends
/// as spherical caps, the dished marks, and marks only where the ball is hidden.
#[test]
fn every_style_has_its_closed_form_volume() {
    for size in SIZES {
        for (i, style) in STYLES.iter().enumerate() {
            let got: f64 = pieces(&sized(*style, size)).iter().map(|p| volume(&p.body)).sum();
            close(got, style_volume(style, size), &format!("style {i} at {size:?}"));
        }
    }
}

/// Every body of every style passes the kernel's check, at every size in `SIZES`, on the axis and
/// moved to a spot with a tilted normal.
#[test]
fn every_body_of_every_style_passes_the_check() {
    for (at, normal) in [([0.0; 3], [0.0, 0.0, 1.0]), ([10.0, -3.0, 7.0], [1.0, 2.0, -2.0])] {
        for size in SIZES {
            for (i, style) in STYLES.iter().enumerate() {
                for piece in pieces(&SpotWeld { at, normal, ..sized(*style, size) }) {
                    check(&piece.body, &format!("style {i} at {size:?} {at:?} {:?}/{:?}", piece.zone, piece.side));
                }
            }
        }
    }
}

/// A ball of the spot diameter shows on a face only where its radius passes that sheet: on two
/// 8 mm plates at the proposed `5 √8` (radius 7.07) it is hidden in both, so the marks stand in on
/// both faces; on 1 mm over 8 mm it stands proud of the upper face only, and the marks stand in on
/// the lower.
#[test]
fn a_ball_that_does_not_reach_a_face_gets_marks_there() {
    let d = proposed_spot_diameter(&[8.0, 8.0]).unwrap();
    for ((t_u, t_l), want) in [((8.0, 8.0), vec![SpotSide::Upper, SpotSide::Lower]), ((1.0, 8.0), vec![SpotSide::Lower]), ((1.0, 1.5), vec![])] {
        let got = pieces(&sized(STYLES[1], (t_u, t_l, d)));
        let ball = got.iter().find(|p| p.zone == SpotZone::Inside).unwrap();
        let (lo, hi) = extent(&ball.body);
        let r = d / 2.0;
        assert!((hi - r).abs() < 1e-9 && (lo + r).abs() < 1e-9, "{lo}..{hi}");
        assert_eq!(hi > t_u, !want.contains(&SpotSide::Upper));
        assert_eq!(-lo > t_l, !want.contains(&SpotSide::Lower));
        let mut sides: Vec<SpotSide> = got.iter().filter(|p| p.zone == SpotZone::Core).map(|p| p.side).collect();
        sides.dedup();
        assert_eq!(sides, want, "{t_u} on {t_l}");
    }
}

/// The dished marks are thick at the rim and thin in the middle: each zone's volume, and each body
/// between the face and the rim height.
#[test]
fn the_dish_is_thick_at_the_rim_and_thin_in_the_middle() {
    let got = pieces(&spot(STYLES[5]));
    assert_eq!(got.len(), 6);
    let want = zone_volumes(&STYLES[5].marks.unwrap(), D);
    let rim = 0.04 * D;
    for (i, piece) in got.iter().enumerate() {
        close(volume(&piece.body), want[i % 3], "dish zone");
        let (lo, hi) = extent(&piece.body);
        if piece.side == SpotSide::Upper {
            assert!(lo >= T_U - 1e-9 && hi <= T_U + rim + 1e-9, "{lo}..{hi}");
        } else {
            assert!(lo >= -T_L - rim - 1e-9 && hi <= -T_L + 1e-9, "{lo}..{hi}");
        }
    }
}

/// Each end of the dished pin is dished from the sheet it lies in, `0.3` of it, and held to a half
/// sphere of the pin's radius below `0.6` of the sheet: the pin runs from face to face either way.
#[test]
fn each_pin_end_is_dished_from_its_own_sheet_at_most_a_half_sphere() {
    for size in [(1.0, 3.0, 5.0), (3.0, 3.0, 1.0)] {
        let got = pieces(&sized(STYLES[4], size));
        let pin = got.iter().find(|p| p.zone == SpotZone::Inside).unwrap();
        let (lo, hi) = extent(&pin.body);
        assert!((lo + size.1).abs() < 1e-9 && (hi - size.0).abs() < 1e-9, "pin from {lo} to {hi}");
        let r = size.2 / 2.0;
        let cap = |t: f64| {
            let c = (0.3 * t).min(r);
            PI * c * (3.0 * r * r + c * c) / 6.0
        };
        close(volume(&pin.body), PI * r * r * (size.0 + size.1) - cap(size.0) - cap(size.1), "pin");
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A spot at (10, −3, 7) with normal (1, 2, −2): every body's vertices lie at the right heights
/// along the normal and within the edge's radius of the spot's axis.
#[test]
fn a_spot_is_placed_at_its_point_along_its_normal() {
    let at = [10.0, -3.0, 7.0];
    let unit = [1.0 / 3.0, 2.0 / 3.0, -2.0 / 3.0];
    for style in STYLES {
        for piece in pieces(&SpotWeld { at, normal: [1.0, 2.0, -2.0], ..spot(style) }) {
            for v in piece.body.to_record().unwrap().0.vertices {
                let p = [v.point[0] - at[0], v.point[1] - at[1], v.point[2] - at[2]];
                let along = dot(p, unit);
                let off = (dot(p, p) - along * along).max(0.0).sqrt();
                assert!(off <= 1.05 * D / 2.0 + 1e-9, "off the axis by {off}");
                let (lo, hi) = match piece.side {
                    SpotSide::Upper => (T_U, T_U + 0.04 * D),
                    SpotSide::Lower => (-T_L - 0.04 * D, -T_L),
                    SpotSide::Through => (-T_L.max(D / 2.0), T_U.max(D / 2.0)),
                };
                assert!(along >= lo - 1e-9 && along <= hi + 1e-9, "{along} not in {lo}..{hi}");
            }
        }
    }
}

fn refused(s: &SpotWeld) {
    match spot_weld(s) {
        Err(SpotRefusal::Invalid(_)) => {}
        other => panic!("not refused as the spot's invalid input: {other:?}"),
    }
}

/// Bad sizes, points, normals and styles are refused as the spot's own invalid input.
#[test]
fn bad_sizes_normals_and_styles_are_refused_as_invalid_input() {
    let s = spot(STYLES[0]);
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        for thicknesses in [vec![t, T_U], vec![T_L, t], vec![T_L, T_U, t]] {
            refused(&SpotWeld { thicknesses, ..s.clone() });
        }
        refused(&SpotWeld { diameter: t, ..s.clone() });
    }
    for thicknesses in [vec![], vec![T_L]] {
        refused(&SpotWeld { thicknesses, ..s.clone() });
    }
    // a ball (looks B and C) only on two sheets
    for style in [STYLES[1], STYLES[2]] {
        refused(&SpotWeld { thicknesses: vec![T_L, T_U, T_U], ..spot(style) });
    }
    refused(&SpotWeld { normal: [0.0; 3], ..s.clone() });
    refused(&SpotWeld { at: [f64::NAN, 0.0, 0.0], ..s.clone() });
    let bad_marks = [
        SpotMarks { core: 0.0, ..MARKS },
        SpotMarks { band: 0.5, ..MARKS },
        SpotMarks { edge: 1.0, ..MARKS },
        SpotMarks { top: MarkTop::Flat { thickness: 0.0 }, ..MARKS },
        SpotMarks { top: MarkTop::Dish { rim: 0.04, middle: 0.04 }, ..MARKS },
        // deeper than its outer radius: 0.9 d deep under a radius of 0.525 d
        SpotMarks { top: MarkTop::Dish { rim: 1.0, middle: 0.1 }, ..MARKS },
    ];
    let mut styles = vec![
        NONE,
        SpotStyle { ball: Some(SpotBall { diameter: -1.0 }), ..NONE },
        SpotStyle { pin: Some(SpotPin { diameter: 1.0, dish: Some(0.0) }), ..NONE },
    ];
    styles.extend(bad_marks.map(|m| SpotStyle { marks: Some(m), ..NONE }));
    for style in styles {
        refused(&SpotWeld { style, ..s.clone() });
    }
    // a dish of exactly a half sphere (0.75 deep under a radius of 0.75 of d) still builds
    let half = SpotMarks { band: 1.0, edge: 1.5, top: MarkTop::Dish { rim: 1.0, middle: 0.25 }, ..MARKS };
    let built = spot_weld(&SpotWeld { style: SpotStyle { marks: Some(half), ..NONE }, ..s.clone() });
    assert!(built.is_ok(), "{built:?}");
    // a pin of 20 on two 1 mm sheets: its dished ends meet on the axis at a dish of 1 and cross at
    // 1.2; at 0.99 they stay 0.02 apart, and it builds and passes the check
    let pinned = |dish| SpotWeld {
        thicknesses: vec![1.0, 1.0],
        style: SpotStyle { pin: Some(SpotPin { diameter: 20.0 / D, dish: Some(dish) }), ..NONE },
        ..s.clone()
    };
    for dish in [1.0, 1.2] {
        refused(&pinned(dish));
    }
    for piece in pieces(&pinned(0.99)) {
        check(&piece.body, "pin dished 0.99");
    }
}

/// Finite sizes the model cannot hold are refused as the spot's own invalid input before an ops
/// call sees them: a diameter or sheet so large the spot passes the coordinate limit, or so small a
/// piece is thinner than the zone tolerance (a mark of `0.006 d` at `d = 1e-10`, a `1e-9` sheet).
#[test]
fn finite_sizes_the_model_cannot_hold_are_refused() {
    for style in STYLES {
        for size in [(T_U, T_L, 1e200), (1e200, T_L, D), (T_U, T_L, 1e-10), (1e-9, T_L, D)] {
            refused(&sized(style, size));
        }
    }
    // far from the origin: the spot passes the limit though each piece is small
    refused(&SpotWeld { at: [2.0 * sweep::MAX_COORD, 0.0, 0.0], ..spot(STYLES[0]) });
}

/// The proposed diameter is `5 √t` of the thinnest sheet; fewer than two sheets, or a thickness
/// that is not finite and positive, is refused instead of giving NaN.
#[test]
fn the_proposed_diameter_is_five_root_t_of_the_thinnest_sheet() {
    assert!((proposed_spot_diameter(&[1.0, 1.5]).unwrap() - 5.0).abs() < 1e-12);
    assert!((proposed_spot_diameter(&[4.0, 2.25]).unwrap() - 7.5).abs() < 1e-12);
    assert!((proposed_spot_diameter(&[2.0, 1.0, 4.0]).unwrap() - 5.0).abs() < 1e-12);
    let no = |t: &[f64]| matches!(proposed_spot_diameter(t), Err(SpotRefusal::Invalid(_)));
    assert!(no(&[]) && no(&[1.0]));
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(no(&[t, 1.0]) && no(&[1.0, t]) && no(&[1.0, 1.0, t]));
    }
}

/// Through three and four stacked sheets, every look without a ball: the marks lie on the first and
/// the last sheet's outer faces, the pin runs from face to face with each end dished from its own
/// outer sheet, every body has its closed-form volume and passes the check.
#[test]
fn a_spot_through_a_stack_reaches_both_outer_faces() {
    let stacks: [&[f64]; 2] = [&[1.0, 1.5, 1.0], &[2.0, 0.8, 1.2, 3.0]];
    for t in stacks {
        let (first, last) = (t[0], t[t.len() - 1]);
        let above: f64 = t.iter().skip(1).sum();
        let d = proposed_spot_diameter(t).unwrap();
        for (i, style) in STYLES.iter().enumerate().filter(|(_, s)| s.ball.is_none()) {
            let got = pieces(&SpotWeld { thicknesses: t.to_vec(), diameter: d, ..spot(*style) });
            // the two outer sheets as a two-sheet spot, plus the pin through the middle
            let pin = style.pin.map_or(0.0, |p| PI * (p.diameter * d / 2.0).powi(2) * (above - last));
            let want = style_volume(style, (last, first, d)) + pin;
            let mut sum = 0.0;
            for piece in &got {
                let (lo, hi) = extent(&piece.body);
                let ok = match piece.side {
                    SpotSide::Upper => (lo - above).abs() < 1e-9,
                    SpotSide::Lower => (hi + first).abs() < 1e-9,
                    SpotSide::Through => (lo + first).abs() < 1e-9 && (hi - above).abs() < 1e-9,
                };
                assert!(ok, "style {i} on {t:?} {:?}: {lo}..{hi}", piece.side);
                check(&piece.body, &format!("style {i} on {t:?}"));
                sum += volume(&piece.body);
            }
            close(sum, want, &format!("style {i} on {t:?}"));
        }
    }
}

/// Every look builds on (1, 1.5) and (8, 8) at the proposed diameter and its bodies pass the check;
/// every look shows on both outer faces, the ball's included.
#[test]
fn every_look_builds_and_its_bodies_pass_the_check() {
    for look in SpotLook::ALL {
        for (t_u, t_l) in [(1.0, 1.5), (8.0, 8.0)] {
            let got = pieces(&SpotWeld { thicknesses: vec![t_l, t_u], diameter: proposed_spot_diameter(&[t_l, t_u]).unwrap(), ..spot(look.style()) });
            let marks = got.iter().filter(|p| p.zone == SpotZone::Core).count();
            let ball_shows = look == SpotLook::Ball && t_u < 2.0;
            assert_eq!(marks, if ball_shows { 0 } else { 2 }, "{look:?} on {t_u}/{t_l}");
            for piece in &got {
                check(&piece.body, &format!("{look:?}"));
            }
        }
    }
}

/// On three sheets the looks without a ball build, marks on both outer faces; the ball looks are
/// refused as invalid input.
#[test]
fn on_three_sheets_only_the_looks_without_a_ball_build() {
    let t = [1.0, 1.5, 1.0];
    for look in SpotLook::ALL {
        let got = spot_weld(&SpotWeld { thicknesses: t.to_vec(), diameter: proposed_spot_diameter(&t).unwrap(), ..spot(look.style()) });
        if look.style().ball.is_some() {
            assert!(matches!(got, Err(SpotRefusal::Invalid(_))), "{look:?}");
        } else {
            assert_eq!(got.unwrap().iter().filter(|p| p.zone == SpotZone::Core).count(), 2, "{look:?}");
        }
    }
}

#[test]
fn the_default_look_is_the_marks_alone() {
    let style = SpotLook::DEFAULT.style();
    assert_eq!(SpotLook::DEFAULT, SpotLook::Marks);
    assert_eq!(style.marks, Some(MARKS));
    assert!(style.ball.is_none() && style.pin.is_none());
}

// ---- where a spot sits ----

/// A body and its record order.
fn ordered(b: Body) -> (Body, Order) {
    let (_, order) = b.to_record().unwrap();
    (b, order)
}

/// A 40 × 40 sheet from `z` up `t`, centred on the z axis.
fn sheet(z: f64, t: f64) -> (Body, Order) {
    ordered(make_box([-20.0, -20.0, z], [40.0, 40.0, t]).unwrap())
}

/// The record face of a box whose plane holds `p` (a point inside its top or bottom).
fn face_at((b, o): &(Body, Order), p: [f64; 3]) -> usize {
    (0..o.faces.len()).find(|&f| b.face_square_level(o, f, [0.0, 0.0, 1.0], p).unwrap().is_some_and(|l| l.level.abs() < 1e-9)).unwrap()
}

fn listed<'a>(sheets: &[&'a (Body, Order)]) -> Vec<(&'a Body, &'a Order)> {
    sheets.iter().map(|&s| (&s.0, &s.1)).collect()
}

fn site(sheets: &[&(Body, Order)], picked: usize, p: [f64; 3]) -> Result<SpotSite> {
    spot_site(&listed(sheets), picked, face_at(sheets[picked], p), p, D)
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// Two sheets (1.5 mm below, 1.0 mm above), picked on the upper sheet's top: the line runs down,
/// the upper sheet first, the spot's point on the faying face; the site builds the spot with its
/// marks on both outer faces.
#[test]
fn two_sheets_picked_on_top_give_the_frame_and_each_sheet() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let s = site(&[&lower, &upper], 1, [2.0, 3.0, 2.5]).unwrap();
    assert!(near(s.normal, [0.0, 0.0, -1.0]), "{s:?}");
    assert!(near(s.at, [2.0, 3.0, 1.5]), "{s:?}");
    let got: Vec<(usize, f64)> = s.sheets.iter().map(|x| (x.sheet, x.thickness)).collect();
    assert_eq!((got[0].0, got[1].0), (1, 0));
    assert!((got[0].1 - 1.0).abs() < 1e-9 && (got[1].1 - 1.5).abs() < 1e-9, "{got:?}");
    assert_eq!(s.sheets[0].near, face_at(&upper, [0.0, 0.0, 2.5]));
    assert_eq!(s.sheets[1].far, face_at(&lower, [0.0, 0.0, 0.0]));
    // the site builds the spot: marks on the picked face (z = 2.5) and on the far one (z = 0)
    let spot = SpotWeld { at: s.at, normal: s.normal, thicknesses: s.sheets.iter().map(|x| x.thickness).collect(), diameter: D, style: STYLES[0] };
    for piece in pieces(&spot) {
        let (lo, hi) = extent(&piece.body);
        match piece.side {
            // `Lower` is the first sheet's outer face: the picked one
            SpotSide::Lower => assert!(lo >= 2.5 - 1e-9, "{lo}"),
            SpotSide::Upper => assert!(hi <= 1e-9, "{hi}"),
            SpotSide::Through => panic!("no pin in this style"),
        }
    }
}

/// Three sheets listed in any order, picked on the bottom: the line runs up, the sheets come in
/// stack order with their thicknesses.
#[test]
fn three_sheets_come_in_stack_order_from_the_picked_face() {
    let (a, b, c) = (sheet(0.0, 1.0), sheet(1.0, 2.0), sheet(3.0, 0.8));
    let s = site(&[&c, &a, &b], 1, [0.0; 3]).unwrap();
    assert!(near(s.normal, [0.0, 0.0, 1.0]));
    assert_eq!(s.sheets.iter().map(|x| x.sheet).collect::<Vec<_>>(), [1, 2, 0]);
    for (x, t) in s.sheets.iter().zip([1.0, 2.0, 0.8]) {
        assert!((x.thickness - t).abs() < 1e-9, "{x:?}");
    }
    assert!(near(s.at, [0.0, 0.0, 1.0]));
}

/// A point beside the picked face (past its edge, in its plane) is off the face.
#[test]
fn a_point_off_the_picked_face_is_refused() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let face = face_at(&upper, [0.0, 0.0, 2.5]);
    assert_eq!(spot_site(&listed(&[&lower, &upper]), 1, face, [30.0, 0.0, 2.5], D), Err(SpotRefusal::PointOffFace));
}

/// Sheets 0.5 apart are refused with the gap; sheets that overlap by 0.5 are refused.
#[test]
fn sheets_apart_or_overlapping_are_refused() {
    let lower = sheet(0.0, 1.5);
    let apart = sheet(2.0, 1.0);
    let Err(SpotRefusal::SheetsApart { sheet: 0, gap }) = site(&[&lower, &apart], 1, [0.0, 0.0, 3.0]) else {
        panic!("not apart");
    };
    assert!((gap - 0.5).abs() < 1e-9, "gap {gap}");
    let sunk = sheet(1.0, 1.0);
    assert_eq!(site(&[&lower, &sunk], 1, [0.0, 0.0, 2.0]), Err(SpotRefusal::SheetsOverlap { sheet: 0 }));
}

/// A listed sheet the line does not reach is missed; one on the outer side of the picked face means
/// the face is not an outer face of the stack.
#[test]
fn a_sheet_missed_or_beyond_the_picked_face_is_refused() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let aside = ordered(make_box([50.0, 0.0, 0.0], [5.0, 5.0, 1.0]).unwrap());
    assert_eq!(site(&[&lower, &upper, &aside], 0, [0.0; 3]), Err(SpotRefusal::SheetMissed { sheet: 2 }));
    let below = sheet(-5.0, 1.0);
    assert_eq!(site(&[&lower, &upper, &below], 0, [0.0; 3]), Err(SpotRefusal::FaceNotOuter { sheet: 2 }));
}

/// A sheet the line crosses twice (two lumps, one above the other) is refused.
#[test]
fn a_sheet_crossed_twice_is_refused() {
    let lower = sheet(0.0, 1.5);
    let (a, _) = sheet(1.5, 1.0);
    let (b, _) = sheet(4.0, 1.0);
    let twice = ordered(union_copies(&a, &b).unwrap().unwrap());
    assert_eq!(site(&[&lower, &twice], 0, [0.0; 3]), Err(SpotRefusal::SheetCrossedAgain { sheet: 1 }));
}

/// A curved picked face is not flat; a sheet tilted 5° under the line is not square to it.
#[test]
fn a_face_not_flat_or_not_square_is_refused() {
    let ball = ordered(make_sphere([0.0, 0.0, 10.0], 5.0).unwrap());
    let lower = sheet(0.0, 1.5);
    assert_eq!(spot_site(&listed(&[&ball, &lower]), 0, 0, [0.0, 0.0, 15.0], D), Err(SpotRefusal::FaceNotFlat { sheet: 0, face: 0 }));
    // a sheet whose lower face passes through (0, 0, 1.5), tilted 5° about the y axis
    let tilted = make_box([-20.0, -20.0, 1.5], [40.0, 40.0, 1.0]).unwrap();
    tilted.transform(&affine::rotation([0.0, 0.0, 1.5], [0.0, 1.0, 0.0], 5f64.to_radians()).unwrap()).unwrap();
    let tilted = ordered(tilted);
    let got = site(&[&lower, &tilted], 0, [0.0; 3]);
    assert!(matches!(got, Err(SpotRefusal::FaceNotSquare { sheet: 1, .. })), "{got:?}");
}

/// A spot of 5 on a 3 × 3 sheet passes its edges; it fits up to 3 (the distance to the nearest
/// edge, twice), and just under that it builds.
#[test]
fn a_footprint_past_a_face_edge_is_refused_with_the_diameter_that_fits() {
    let lower = sheet(0.0, 1.5);
    let small = ordered(make_box([-1.5, -1.5, 1.5], [3.0, 3.0, 1.0]).unwrap());
    let got = site(&[&lower, &small], 0, [0.0; 3]);
    let Err(SpotRefusal::PastFaceEdge { sheet: 1, diameter, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(diameter, D);
    assert!((fits - 3.0).abs() < 1e-9, "fits {fits}");
    let face = face_at(&lower, [0.0; 3]);
    assert!(spot_site(&listed(&[&lower, &small]), 0, face, [0.0; 3], 2.99).is_ok());
}

/// Fewer than two sheets, a picked sheet or face not listed, a point that is not finite or a
/// diameter that is not finite and positive are bad input.
#[test]
fn bad_site_input_is_refused_as_invalid_input() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let face = face_at(&lower, [0.0; 3]);
    let both = listed(&[&lower, &upper]);
    let bad = |r: Result<SpotSite>| matches!(r, Err(SpotRefusal::Invalid(_)));
    assert!(bad(spot_site(&listed(&[&lower]), 0, face, [0.0; 3], D)));
    assert!(bad(spot_site(&both, 2, face, [0.0; 3], D)));
    assert!(bad(spot_site(&both, 0, 99, [0.0; 3], D)));
    assert!(bad(spot_site(&both, 0, face, [f64::NAN, 0.0, 0.0], D)));
    for d in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(bad(spot_site(&both, 0, face, [0.0; 3], d)));
    }
}
