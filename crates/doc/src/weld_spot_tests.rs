//! Spot welds: every style's bodies against their closed-form volumes at several sizes, the
//! kernel's check on every body, the ball that does not reach a face, the pin's dishes, the
//! placement at any spot and normal, stacks of three and four sheets, the refusals, the named
//! looks; and the site of a spot picked on stacks of boxes, with every typed refusal of a stack
//! that does not take it. Moved from solidcore's `weld_spot`, `weld_spot_site` and ops'
//! `weld_spot_looks` tests (#18409).

use std::f64::consts::PI;

use solidcore_record::Vec3;
use solidcraft_kernel::{SmlibCheck, affine, box_solid, join_apart, transform_matrix, validity};

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
    b.volume_at(1e-9).unwrap()
}

/// The kernel's check, every test: the record's, and the kernel's own at its strictest level.
fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

/// The lowest and highest height (along +Z) of a body: its tight box.
fn extent(b: &Body) -> (f64, f64) {
    let bb = b.exact_bounds().unwrap();
    (bb.min.z, bb.max.z)
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
            for v in &piece.body.record().vertices {
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
    refused(&SpotWeld { at: [2.0 * solidcraft_kernel::profiles::MAX_COORD, 0.0, 0.0], ..spot(STYLES[0]) });
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
/// A 40 × 40 sheet from `z` up `t`, centred on the z axis.
fn sheet(z: f64, t: f64) -> Body {
    block([-20.0, -20.0, z], [40.0, 40.0, t])
}

/// The box from `corner` of `size`.
fn block(corner: [f64; 3], size: [f64; 3]) -> Body {
    let lo = Vec3::from(corner);
    box_solid(lo, lo + Vec3::from(size)).unwrap()
}

/// The record face of a box whose plane holds `p` (a point inside its top or bottom).
fn face_at(b: &Body, p: [f64; 3]) -> usize {
    (0..b.face_count()).find(|&f| b.face_square_level(f, Vec3::new(0.0, 0.0, 1.0), p.into()).unwrap().is_some_and(|l| l.level.abs() < 1e-9)).unwrap()
}

fn site(sheets: &[&Body], picked: usize, p: [f64; 3]) -> Result<SpotSite> {
    spot_site(sheets, picked, face_at(sheets[picked], p), p, D)
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
    assert_eq!(spot_site(&[&lower, &upper], 1, face, [30.0, 0.0, 2.5], D), Err(SpotRefusal::PointOffFace));
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
    let aside = block([50.0, 0.0, 0.0], [5.0, 5.0, 1.0]);
    assert_eq!(site(&[&lower, &upper, &aside], 0, [0.0; 3]), Err(SpotRefusal::SheetMissed { sheet: 2 }));
    let below = sheet(-5.0, 1.0);
    assert_eq!(site(&[&lower, &upper, &below], 0, [0.0; 3]), Err(SpotRefusal::FaceNotOuter { sheet: 2 }));
}

/// A sheet the line crosses twice (two lumps, one above the other) is refused.
#[test]
fn a_sheet_crossed_twice_is_refused() {
    let lower = sheet(0.0, 1.5);
    let twice = join_apart(&[sheet(1.5, 1.0), sheet(4.0, 1.0)]).unwrap().unwrap();
    assert_eq!(site(&[&lower, &twice], 0, [0.0; 3]), Err(SpotRefusal::SheetCrossedAgain { sheet: 1 }));
}

/// A curved picked face is not flat; a sheet tilted 5° under the line is not square to it.
#[test]
fn a_face_not_flat_or_not_square_is_refused() {
    let ball = solidcraft_kernel::profiles::ball([0.0, 0.0, 10.0], 5.0, None).unwrap();
    let lower = sheet(0.0, 1.5);
    assert_eq!(spot_site(&[&ball, &lower], 0, 0, [0.0, 0.0, 15.0], D), Err(SpotRefusal::FaceNotFlat { sheet: 0, face: 0 }));
    // a sheet whose lower face passes through (0, 0, 1.5), tilted 5° about the y axis
    let tilted = block([-20.0, -20.0, 1.5], [40.0, 40.0, 1.0]);
    let tilted = transform_matrix(&tilted, affine::rotation([0.0, 0.0, 1.5], [0.0, 1.0, 0.0], 5f64.to_radians()).unwrap()).unwrap();
    let got = site(&[&lower, &tilted], 0, [0.0; 3]);
    assert!(matches!(got, Err(SpotRefusal::FaceNotSquare { sheet: 1, .. })), "{got:?}");
}

/// A spot of 5 on a 3 × 3 sheet passes its edges; it fits up to 3 (the distance to the nearest
/// edge, twice), and just under that it builds.
#[test]
fn a_footprint_past_a_face_edge_is_refused_with_the_diameter_that_fits() {
    let lower = sheet(0.0, 1.5);
    let small = block([-1.5, -1.5, 1.5], [3.0, 3.0, 1.0]);
    let got = site(&[&lower, &small], 0, [0.0; 3]);
    let Err(SpotRefusal::PastFaceEdge { sheet: 1, diameter, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(diameter, D);
    assert!((fits - 3.0).abs() < 1e-9, "fits {fits}");
    let face = face_at(&lower, [0.0; 3]);
    assert!(spot_site(&[&lower, &small], 0, face, [0.0; 3], 2.99).is_ok());
}

/// Fewer than two sheets, a picked sheet or face not listed, a point that is not finite or a
/// diameter that is not finite and positive are bad input.
#[test]
fn bad_site_input_is_refused_as_invalid_input() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let face = face_at(&lower, [0.0; 3]);
    let both: &[&Body] = &[&lower, &upper];
    let bad = |r: Result<SpotSite>| matches!(r, Err(SpotRefusal::Invalid(_)));
    assert!(bad(spot_site(&[&lower], 0, face, [0.0; 3], D)));
    assert!(bad(spot_site(both, 2, face, [0.0; 3], D)));
    assert!(bad(spot_site(both, 0, 99, [0.0; 3], D)));
    assert!(bad(spot_site(both, 0, face, [f64::NAN, 0.0, 0.0], D)));
    for d in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(bad(spot_site(both, 0, face, [0.0; 3], d)));
    }
}

// ---- on curved sheets (#21144) ----

fn pline(a: [f64; 3], b: [f64; 3]) -> solidcraft_kernel::profiles::ProfileCurve {
    solidcraft_kernel::profiles::ProfileCurve::Line { start: a, end: b }
}

/// A sphere-shell cap about +Z between radii `ri` and `ro`, out to polar angle `cap`.
fn dome(ri: f64, ro: f64, cap: f64) -> Body {
    let arc = |r: f64| solidcraft_kernel::profiles::ProfileCurve::Arc {
        center: [0.0; 3],
        axis: [0.0, 1.0, 0.0],
        reference: [0.0, 0.0, 1.0],
        radius: r,
        start: 0.0,
        end: cap,
    };
    let at = |r: f64| [r * cap.sin(), 0.0, r * cap.cos()];
    let p = [pline([0.0, 0.0, ri], [0.0, 0.0, ro]), arc(ro), pline(at(ro), at(ri)), arc(ri)];
    solidcraft_kernel::profiles::revolve_curves(&p, [0.0; 3], [0.0, 0.0, 1.0], std::f64::consts::TAU, true, None).unwrap()
}

/// A cylinder-shell sector about Z between radii `ri` and `ro`, `z` from -15 to 15, from angle 0
/// to `angle`.
fn shell(ri: f64, ro: f64, angle: f64) -> Body {
    let p = [
        pline([ri, 0.0, -15.0], [ro, 0.0, -15.0]),
        pline([ro, 0.0, -15.0], [ro, 0.0, 15.0]),
        pline([ro, 0.0, 15.0], [ri, 0.0, 15.0]),
        pline([ri, 0.0, 15.0], [ri, 0.0, -15.0]),
    ];
    solidcraft_kernel::profiles::revolve_curves(&p, [0.0; 3], [0.0, 0.0, 1.0], angle, true, None).unwrap()
}

/// The near-side volume between the spheres of radii `a < b` about the origin within `rho` of a
/// line through the origin.
fn sphere_disc(a: f64, b: f64, rho: f64) -> f64 {
    let cap = |r: f64| r.powi(3) - (r * r - rho * rho).powf(1.5);
    std::f64::consts::TAU / 3.0 * (cap(b) - cap(a))
}

/// The face the ray from `from` along `dir` meets first in `b`, and where.
fn first_hit(b: &Body, from: [f64; 3], dir: [f64; 3]) -> (usize, [f64; 3]) {
    let h = b.face_hits_by_ray(from, dir).unwrap();
    (h[0].face, h[0].point)
}

/// The unit direction at polar angle `polar` from +Z, `azimuth` round it.
fn radial(polar: f64, azimuth: f64) -> [f64; 3] {
    [polar.sin() * azimuth.cos(), polar.sin() * azimuth.sin(), polar.cos()]
}

/// Two flat, square sheets: the curved site is the flat site and the bodies on it are
/// [`spot_weld`]'s, every look.
#[test]
fn on_flat_square_sheets_the_curved_site_and_bodies_are_the_flat_ones() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let both: &[&Body] = &[&lower, &upper];
    let p = [2.0, 3.0, 2.5];
    let face = face_at(&upper, p);
    let flat = spot_site(both, 1, face, p, D).unwrap();
    let curved = spot_site_curved(both, 1, face, p, D).unwrap();
    assert!(near(flat.at, curved.at) && near(flat.normal, curved.normal), "{flat:?} {curved:?}");
    for (a, b) in flat.sheets.iter().zip(&curved.sheets) {
        assert!((a.sheet, a.near, a.far) == (b.sheet, b.near, b.far) && (a.thickness - b.thickness).abs() < 1e-9, "{a:?} {b:?}");
    }
    for style in STYLES {
        let spot = SpotWeld { at: flat.at, normal: flat.normal, thicknesses: flat.sheets.iter().map(|x| x.thickness).collect(), diameter: D, style };
        let want = pieces(&spot);
        let got = spot_weld_on(both, &curved, D, style).unwrap();
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(&want) {
            assert_eq!((g.zone, g.side), (w.zone, w.side));
            close(volume(&g.body), volume(&w.body), "flat stack through the curved path");
        }
    }
}

/// A pressed dome (concentric sphere shells, R 50 to 51.5 below, 51.5 to 52.5 above), picked on
/// its top off the revolve seam: the line runs radially in; every mark zone is the closed-form
/// solid between its sphere and the sphere the marks' thickness out, on the convex top and on the
/// concave underside, each outside its sheet; the thin pin is the solid between the outer spheres
/// within its radius; the ball is a ball.
#[test]
fn on_a_dome_each_mark_zone_and_the_pin_are_their_closed_form() {
    let lower = dome(50.0, 51.5, 0.5);
    let upper = dome(51.5, 52.5, 0.5);
    let both: &[&Body] = &[&lower, &upper];
    let dir = radial(0.25, 1.0);
    let (face, p) = first_hit(&upper, dir.map(|v| 60.0 * v), dir.map(|v| -v));
    let site = spot_site_curved(both, 1, face, p, D).unwrap();
    assert!(site.normal.iter().zip(dir).all(|(a, b)| (a + b).abs() < 1e-9), "{site:?}");
    let h = 0.006 * D;
    let radii = [0.0, 0.55 * D / 2.0, D / 2.0, 1.05 * D / 2.0];
    let marks = spot_weld_on(both, &site, D, SpotLook::Marks.style()).unwrap();
    assert_eq!(marks.len(), 6);
    for (i, piece) in marks.iter().enumerate() {
        // the first sheet is the picked one: its outer face (`Lower`) is the convex top
        let (a, b, sheet) = match piece.side {
            SpotSide::Lower => (52.5, 52.5 + h, &upper),
            SpotSide::Upper => (50.0 - h, 50.0, &lower),
            SpotSide::Through => panic!("no pin in this look"),
        };
        let z = i % 3;
        close(volume(&piece.body), sphere_disc(a, b, radii[z + 1]) - sphere_disc(a, b, radii[z]), "dome mark zone");
        assert!(boolean(&piece.body, sheet, BoolOp::Intersect).unwrap().is_none(), "mark {i} inside its sheet");
        check(&piece.body, "dome mark zone");
    }
    let pinned = spot_weld_on(both, &site, D, SpotLook::MarksThinPin.style()).unwrap();
    let pin: Vec<_> = pinned.iter().filter(|x| x.zone == SpotZone::Inside).collect();
    assert_eq!(pin.len(), 1);
    close(volume(&pin[0].body), sphere_disc(50.0, 52.5, 0.55 * D / 2.0), "dome pin");
    let ball = spot_weld_on(both, &site, D, SpotLook::Ball.style()).unwrap();
    let inside: Vec<_> = ball.iter().filter(|x| x.zone == SpotZone::Inside).collect();
    assert_eq!(inside.len(), 1);
    close(volume(&inside[0].body), 4.0 / 3.0 * PI * (D / 2.0).powi(3), "dome ball");
}

/// `∫ 2πρ h dρ` from `a` to `b` of the height `h(ρ) = s (√(rf² - ρ²) - rf)` off its tangent plane
/// at the axis of a sphere of radius `rf` crossed by the axis: `s = 1` for a convex face (seen
/// from outside), `-1` for a concave one.
fn sag_volume(rf: f64, s: f64, a: f64, b: f64) -> f64 {
    s * (2.0 * PI / 3.0 * ((rf * rf - a * a).powf(1.5) - (rf * rf - b * b).powf(1.5)) - PI * rf * (b * b - a * a))
}

/// What a dished end of depth `c` takes off a pin of radius `r` whose end is a sphere of radius
/// `rf` crossed by the axis (`s` as in [`sag_volume`]): the pin within the end's sphere, the
/// sphere through the rim at height 0 and `c` in on the axis.
fn dished_end(rf: f64, s: f64, r: f64, c: f64) -> f64 {
    let rp = (r * r + c * c) / (2.0 * c);
    // the end's face less the sphere's cap, as heights off the tangent plane
    let gap = |rho: f64| s * ((rf * rf - rho * rho).sqrt() - rf) - (rp - c - (rp * rp - rho * rho).sqrt());
    // on a convex end the sphere leaves the face inside the rim: where the two cross
    let edge = if gap(r) >= 0.0 {
        r
    } else {
        let (mut lo, mut hi) = (0.0, r);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if gap(mid) > 0.0 { lo = mid } else { hi = mid }
        }
        lo
    };
    // ∫ 2πρ (rp - c - √(rp² - ρ²)) dρ from 0 to edge, the cap's heights
    let cap = PI * (rp - c) * edge * edge - 2.0 * PI / 3.0 * (rp.powi(3) - (rp * rp - edge * edge).powf(1.5));
    sag_volume(rf, s, 0.0, edge) - cap
}

/// The dished looks on the pressed dome: [`SpotLook::Dent`]'s three zones on the convex top and
/// on the concave underside are each the closed-form solid between the face and the flat look's
/// sphere placed at the point, each outside its sheet; [`SpotLook::MarksPinConcaveEnds`]' pin is
/// the solid between the outer spheres within its radius less what each end's sphere takes (on
/// the convex top it leaves the face inside the rim, on the concave underside it cuts the pin's
/// wall down to the rim).
#[test]
fn on_a_dome_the_dished_looks_are_their_closed_form() {
    let lower = dome(50.0, 51.5, 0.5);
    let upper = dome(51.5, 52.5, 0.5);
    let both: &[&Body] = &[&lower, &upper];
    let dir = radial(0.25, 1.0);
    let (face, p) = first_hit(&upper, dir.map(|v| 60.0 * v), dir.map(|v| -v));
    let site = spot_site_curved(both, 1, face, p, D).unwrap();
    // the dent's dish: 0.04 d high at the rim, 0.004 d in the middle
    let (rim, middle) = (0.04 * D, 0.004 * D);
    let radii = [0.0, 0.55 * D / 2.0, D / 2.0, 1.05 * D / 2.0];
    let dent = spot_weld_on(both, &site, D, SpotLook::Dent.style()).unwrap();
    assert_eq!(dent.len(), 6);
    for (i, piece) in dent.iter().enumerate() {
        // the first sheet is the picked one: its outer face (`Lower`) is the convex top
        let (rf, s, sheet) = match piece.side {
            SpotSide::Lower => (52.5, 1.0, &upper),
            SpotSide::Upper => (50.0, -1.0, &lower),
            SpotSide::Through => panic!("no pin in this look"),
        };
        let z = i % 3;
        let (a, b) = (radii[z], radii[z + 1]);
        close(volume(&piece.body), dish_zone(a, b, rim, middle, radii[3]) - sag_volume(rf, s, a, b), "dome dished zone");
        assert!(boolean(&piece.body, sheet, BoolOp::Intersect).unwrap().is_none(), "dished mark {i} inside its sheet");
        check(&piece.body, "dome dished zone");
    }
    let pinned = spot_weld_on(both, &site, D, SpotLook::MarksPinConcaveEnds.style()).unwrap();
    let pin: Vec<_> = pinned.iter().filter(|x| x.zone == SpotZone::Inside).collect();
    assert_eq!((pinned.len(), pin.len()), (7, 1));
    // the pin's ends: the convex top in the picked sheet (1.0 thick), the concave underside in the other (1.5)
    let r = D / 2.0;
    let want = sphere_disc(50.0, 52.5, r) - dished_end(52.5, 1.0, r, 0.3 * 1.0) - dished_end(50.0, -1.0, r, 0.3 * 1.5);
    close(volume(&pin[0].body), want, "dome pin with dished ends");
    check(&pin[0].body, "dome pin with dished ends");
}

/// Dished marks on the inside of a small pipe (R 2): the face curves up into the dish's sphere,
/// so the marks would dip under it; the diameter named clears and builds.
#[test]
fn dished_marks_a_concave_face_curves_into_are_refused_with_a_diameter_that_builds() {
    let lower = shell(2.0, 2.8, 2.5);
    let upper = shell(2.8, 3.6, 2.5);
    let both: &[&Body] = &[&lower, &upper];
    let dir = [1.25_f64.cos(), 1.25_f64.sin(), 0.0];
    let (face, p) = first_hit(&lower, [0.0; 3], dir);
    let d = 1.2;
    let site = spot_site_curved(both, 0, face, p, d).unwrap();
    let got = spot_weld_on(both, &site, d, SpotLook::Dent.style());
    let Err(SpotRefusal::DishUnderFace { sheet: 0, diameter, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(diameter, d);
    let fits = fits.expect("a smaller dish clears the face");
    assert!(fits < d, "fits {fits}");
    let site = spot_site_curved(both, 0, face, p, fits).unwrap();
    let marks = spot_weld_on(both, &site, fits, SpotLook::Dent.style()).unwrap();
    assert_eq!(marks.len(), 6);
    for piece in &marks {
        let sheet = if piece.side == SpotSide::Lower { &lower } else { &upper };
        assert!(boolean(&piece.body, sheet, BoolOp::Intersect).unwrap().is_none(), "dished mark inside its sheet");
    }
}

/// Near the dome's rim the edges lie closer to the spot's line than half the diameter: refused
/// with twice the distance from the line to the nearest rim (on the R 50 face, 50 sin 0.03), and a
/// spot just under it fits.
#[test]
fn a_spot_near_a_dome_s_rim_is_refused_with_the_distance_to_the_line() {
    let lower = dome(50.0, 51.5, 0.5);
    let upper = dome(51.5, 52.5, 0.5);
    let both: &[&Body] = &[&lower, &upper];
    let dir = radial(0.47, 1.0);
    let (face, p) = first_hit(&upper, dir.map(|v| 60.0 * v), dir.map(|v| -v));
    let got = spot_site_curved(both, 1, face, p, D);
    let Err(SpotRefusal::PastFaceEdge { diameter, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(diameter, D);
    let want = 2.0 * 50.0 * 0.03_f64.sin();
    assert!((fits - want).abs() < 1e-6, "fits {fits} != {want}");
    assert!(spot_site_curved(both, 1, face, p, 0.99 * want).is_ok());
}

/// A sheet whose top curls over into a ledge beside the spot (one B-spline face): within the
/// footprint the face turns away from the line, so it is not single-valued there; a fit, when the
/// search names one, is a site that holds.
#[test]
fn a_face_curling_over_within_the_footprint_is_not_single_valued() {
    let lower = block([-30.0, -20.0, -1.0], [60.0, 40.0, 1.0]);
    let top = solidcraft_kernel::profiles::ProfileCurve::Nurbs {
        degree: 3,
        points: [(-30.0, 1.0), (-10.0, 1.0), (1.0, 1.0), (3.0, 1.0), (3.0, 4.0), (0.0, 4.0), (1.0, 5.5), (30.0, 5.5)]
            .iter()
            .map(|&(x, z)| [x, -20.0, z, 1.0])
            .collect(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0],
    };
    let profile = [
        pline([30.0, -20.0, 0.0], [-30.0, -20.0, 0.0]),
        pline([-30.0, -20.0, 0.0], [-30.0, -20.0, 1.0]),
        top,
        pline([30.0, -20.0, 5.5], [30.0, -20.0, 0.0]),
    ];
    let upper = solidcraft_kernel::profiles::extrude_curves(&profile, [0.0, 1.0, 0.0], 40.0, true).unwrap();
    let both: &[&Body] = &[&lower, &upper];
    let p = [0.0, 0.0, -1.0];
    let face = face_at(&lower, p);
    let got = spot_site_curved(both, 0, face, p, D);
    let Err(SpotRefusal::FaceNotSingleValued { sheet: 1, diameter, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(diameter, D);
    if let Some(fits) = fits {
        assert!(fits < D, "fits {fits}");
        assert!(spot_site_curved(both, 0, face, p, fits).is_ok(), "fits {fits}");
    }
}

/// Marks on the inside of a small pipe (R 2), thicker than its radius: the offset folds; the
/// diameter named builds there.
#[test]
fn marks_thicker_than_a_concave_face_curves_are_refused_with_a_diameter_that_builds() {
    let lower = shell(2.0, 2.8, 2.5);
    let upper = shell(2.8, 3.6, 2.5);
    let both: &[&Body] = &[&lower, &upper];
    let dir = [1.25_f64.cos(), 1.25_f64.sin(), 0.0];
    let (face, p) = first_hit(&lower, [0.0; 3], dir);
    let d = 1.2;
    let site = spot_site_curved(both, 0, face, p, d).unwrap();
    // marks 2 d thick: 2.4 off the R 2 face, towards its axis
    let style = SpotStyle { marks: Some(SpotMarks { top: MarkTop::Flat { thickness: 2.0 }, ..MARKS }), ..NONE };
    let got = spot_weld_on(both, &site, d, style);
    let Err(SpotRefusal::OffsetFolds { sheet: 0, diameter, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(diameter, d);
    assert!(fits < d && fits > 0.5, "fits {fits}");
    let site = spot_site_curved(both, 0, face, p, fits).unwrap();
    assert_eq!(spot_weld_on(both, &site, fits, style).unwrap().len(), 6);
}
