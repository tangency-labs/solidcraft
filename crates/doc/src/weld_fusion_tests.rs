//! Fusion spot and seam welds: a spot with a straight or tapered zone, partly into the second sheet
//! or melted through with its root; a seam along a line, and along a line and an arc; a sharp
//! corner; against their closed-form volumes, every body through the kernel's check; the shape a
//! process starts with; the refusals, an arc bulging past the size limit among them; and the site
//! of a fusion seam with an arc tighter than the weld, or between its widest part and the
//! resistance seam's.

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::Vec3;
use solidcraft_kernel::{Body, SmlibCheck, box_solid, validity};

use super::*;
use crate::weld_seam::Turn;

const T1: f64 = 1.0;
const T2: f64 = 1.5;
const W: f64 = 3.0;

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

/// The record's check, and the kernel's own at its strictest level.
fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

/// A body's box, lowest and highest corner.
fn bounds(b: &Body) -> ([f64; 3], [f64; 3]) {
    let bb = b.exact_bounds().unwrap();
    ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z])
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-6 * want.abs(), "{what}: volume {got} != {want}");
}

fn weld(path: Vec<SeamPiece>, shape: FusionShape, depth: f64) -> FusionWeld {
    FusionWeld { start: [0.0; 3], path, normal: [0.0, 0.0, 1.0], thicknesses: vec![T1, T2], width: W, shape, depth }
}

fn to(x: f64, y: f64) -> SeamPiece {
    SeamPiece::Line { to: [x, y, 0.0] }
}

/// A sphere's cap `2a` across and `h` high.
fn cap(a: f64, h: f64) -> f64 {
    PI * h * (3.0 * a * a + h * h) / 6.0
}

/// The circular segment `2a` across and `h` high (the cap's cross-section).
fn segment(a: f64, h: f64) -> f64 {
    let r = (a * a + h * h) / (2.0 * h);
    r * r * ((r - h) / r).acos() - (r - h) * (2.0 * r * h - h * h).sqrt()
}

/// The zone's radii at the top face and at its bottom.
fn zone_radii(shape: FusionShape) -> (f64, f64) {
    match shape {
        FusionShape::Straight => (0.35 * W, 0.35 * W),
        FusionShape::Tapered => (0.5 * W, 0.175 * W),
    }
}

/// Every body's volume: the cap, the zone and (melted through) the root, each `along` the path
/// (its cross-section times the path's length) plus the same body turned a whole turn (the round
/// ends of a smooth path, or the spot itself).
fn volumes_are(got: &Fusion, shape: FusionShape, depth: f64, along: f64, what: &str) {
    let (top, bottom) = zone_radii(shape);
    let l = T1 + depth * T2;
    let mut want = vec![
        (SpotZone::Core, SpotSide::Lower, segment(0.5 * W, 0.08 * W) * along + cap(0.5 * W, 0.08 * W)),
        (SpotZone::Inside, SpotSide::Through, (top + bottom) * l * along + PI * l * (top * top + top * bottom + bottom * bottom) / 3.0),
    ];
    if depth >= 1.0 {
        want.push((SpotZone::Core, SpotSide::Upper, segment(1.2 * bottom, 0.05 * W) * along + cap(1.2 * bottom, 0.05 * W)));
    }
    assert_eq!(got.pieces.len(), want.len(), "{what}");
    for (piece, (zone, side, v)) in got.pieces.iter().zip(want) {
        assert_eq!((piece.zone, piece.side), (zone, side), "{what}");
        close(volume(&piece.body), v, &format!("{what}: {zone:?} {side:?}"));
        check(&piece.body, what);
    }
}

/// A fusion spot, straight or tapered, half into the second sheet or melted through: the cap on
/// the top face, the zone down to its depth, the root only when melted through.
#[test]
fn a_fusion_spot_has_its_cap_its_zone_and_a_root_only_when_melted_through() {
    for shape in [FusionShape::Straight, FusionShape::Tapered] {
        for depth in [DEPTH, 1.0] {
            let got = fusion_weld(&weld(vec![], shape, depth)).unwrap();
            assert!(got.notices.is_empty());
            volumes_are(&got, shape, depth, 0.0, &format!("spot {shape:?} {depth}"));
            let (lo, hi) = bounds(&got.pieces[1].body);
            assert!((lo[2] + T1).abs() < 1e-9 && (hi[2] - depth * T2).abs() < 1e-9, "zone {lo:?}..{hi:?}");
            let (lo, _) = bounds(&got.pieces[0].body);
            assert!((lo[2] + T1 + 0.08 * W).abs() < 1e-9, "cap {lo:?}");
        }
    }
}

/// A fusion seam along a 20 mm line, and along a line and an arc of radius 10 a quarter turn on:
/// each body its cross-section along the path and its round ends.
#[test]
fn a_fusion_seam_is_the_spot_drawn_along_its_path() {
    let arc = SeamPiece::Arc { center: [10.0, 10.0, 0.0], to: [20.0, 10.0, 0.0], turn: Turn::Left };
    for (path, along) in [(vec![to(20.0, 0.0)], 20.0), (vec![to(10.0, 0.0), arc], 10.0 + 10.0 * FRAC_PI_2)] {
        for (shape, depth) in [(FusionShape::Straight, DEPTH), (FusionShape::Tapered, 1.0)] {
            let got = fusion_weld(&weld(path.clone(), shape, depth)).unwrap();
            assert!(got.notices.is_empty(), "{:?}", got.notices);
            volumes_are(&got, shape, depth, along, &format!("seam {along} {shape:?} {depth}"));
        }
    }
}

/// A sharp corner builds, with the seam's notice.
#[test]
fn a_sharp_corner_builds_with_a_notice() {
    let got = fusion_weld(&weld(vec![to(10.0, 0.0), to(10.0, 10.0)], FusionShape::Tapered, DEPTH)).unwrap();
    assert!(matches!(got.notices[..], [SeamNotice::Corner { joint: 1, .. }]), "{:?}", got.notices);
    for piece in &got.pieces {
        check(&piece.body, "corner");
    }
}

/// Beam processes start tapered, arc straight.
#[test]
fn a_beam_process_starts_tapered_and_arc_straight() {
    assert_eq!(FusionShape::of_process(true), FusionShape::Tapered);
    assert_eq!(FusionShape::of_process(false), FusionShape::Straight);
}

/// Other than two sheets, a depth outside `0 < depth <= 1`, a bad width or normal, a path the seam
/// refuses, and an arc tighter than the weld (radius 1 under the cap's 1.5: at most 2 wide) are
/// refused.
#[test]
fn bad_fusion_welds_are_refused() {
    let bad = |w: FusionWeld| matches!(fusion_weld(&w), Err(FusionRefusal::Invalid(_)));
    let ok = weld(vec![], FusionShape::Straight, DEPTH);
    assert!(bad(FusionWeld { thicknesses: vec![1.0, 1.0, 1.0], ..ok.clone() }));
    assert!(bad(FusionWeld { thicknesses: vec![1.0], ..ok.clone() }));
    for depth in [0.0, -0.5, 1.5, f64::NAN] {
        assert!(bad(FusionWeld { depth, ..ok.clone() }), "depth {depth}");
    }
    for width in [0.0, f64::NAN, f64::INFINITY] {
        assert!(bad(FusionWeld { width, ..ok.clone() }), "width {width}");
    }
    assert!(bad(FusionWeld { normal: [0.0; 3], ..ok.clone() }));
    assert!(matches!(fusion_weld(&FusionWeld { path: vec![to(0.0, 0.0)], ..ok.clone() }), Err(FusionRefusal::Path(SeamRefusal::Invalid(_)))));
    let tight = SeamPiece::Arc { center: [0.0, 1.0, 0.0], to: [1.0, 1.0, 0.0], turn: Turn::Left };
    let got = fusion_weld(&FusionWeld { path: vec![tight], ..ok.clone() });
    let Err(FusionRefusal::ArcTighterThanSeam { piece: 0, fits }) = got else {
        panic!("{got:?}");
    };
    assert!((fits - 2.0).abs() < 1e-9, "{fits}");
    // ends 1e8 out, but the long way round over the top reaches y = 1.01e9, past the size limit
    let bulge = SeamPiece::Arc { center: [0.0, 5e8, 0.0], to: [1e8, 0.0, 0.0], turn: Turn::Right };
    assert!(bad(FusionWeld { start: [-1e8, 0.0, 0.0], path: vec![bulge], ..ok }));
}

// ---- where a fusion seam sits ----

/// A 40 × 40 sheet from `z` up `t`, centred on the z axis.
fn sheet(z: f64, t: f64) -> Body {
    box_solid(Vec3::new(-20.0, -20.0, z), Vec3::new(20.0, 20.0, z + t)).unwrap()
}

/// A fusion seam of width 3 drawn on the upper sheet's top along a half circle of `radius` about
/// the z axis.
fn arc_site(radius: f64) -> Result<SeamSite> {
    let lower = sheet(0.0, T2);
    let upper = sheet(T2, T1);
    let top = T1 + T2;
    let face = (0..upper.face_count())
        .find(|&f| upper.face_square_level(f, Vec3::new(0.0, 0.0, 1.0), [0.0, 0.0, top].into()).unwrap().is_some_and(|l| l.level.abs() < 1e-9))
        .unwrap();
    let path = [SeamPiece::Arc { center: [0.0, 0.0, top], to: [radius, 0.0, top], turn: Turn::Left }];
    fusion_seam_site(&[&lower, &upper], 1, face, [-radius, 0.0, top], &path, W)
}

/// An arc of radius 1.4 is tighter than the cap (1.5): refused at the site with the width that
/// fits, 2.8, as the build would. One of 1.55 is wider than the cap though tighter than the
/// resistance seam's edge zone (1.575): the fusion seam's site takes it, and the weld builds.
#[test]
fn a_fusion_seam_site_takes_the_arcs_the_weld_builds() {
    let got = arc_site(1.4);
    let Err(FusionRefusal::Path(SeamRefusal::ArcTighterThanSeam { piece: 0, fits })) = got else {
        panic!("{got:?}");
    };
    assert!((fits - 2.8).abs() < 1e-9, "{fits}");
    let s = arc_site(1.55).unwrap();
    let w = FusionWeld {
        start: s.start,
        path: s.path,
        normal: s.normal,
        thicknesses: s.sheets.iter().map(|x| x.thickness).collect(),
        width: W,
        shape: FusionShape::Tapered,
        depth: DEPTH,
    };
    for piece in fusion_weld(&w).unwrap().pieces {
        check(&piece.body, "arc of 1.55");
    }
}
