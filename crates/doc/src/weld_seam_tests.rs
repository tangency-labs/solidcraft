//! Resistance seam welds: every zone of a band along a line, a line and an arc turning either way,
//! a sharp corner (its notice and the rounded union), spots apart and overlapping, against their
//! closed-form volumes; the kernel's check on every body, on a tilted seam too; the refusals, an
//! arc bulging past the size limit among them; and the site of a seam drawn on stacks of boxes,
//! with a notch the path passes, an arc tighter than the seam and a joint off the face.

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::Vec3;
use solidcraft_kernel::{BoolOp, SmlibCheck, box_solid, validity};

use super::*;
use crate::weld_spot::SpotRefusal;

const T_U: f64 = 1.0;
const T_L: f64 = 1.5;
const W: f64 = 5.0;
/// The marks' thickness, `0.006 w`.
const H: f64 = 0.006 * W;
/// The zones' radii: core, band, edge.
const R: [f64; 3] = [0.55 * W / 2.0, W / 2.0, 1.05 * W / 2.0];

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

/// The record's check, and the kernel's own at its strictest level.
fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= 1e-6 * want.abs(), "{what}: volume {got} != {want}");
}

fn seam(path: Vec<SeamPiece>, look: SeamLook) -> SeamWeld {
    SeamWeld { start: [0.0; 3], path, normal: [0.0, 0.0, 1.0], thicknesses: vec![T_L, T_U], width: W, look }
}

fn band(path: Vec<SeamPiece>) -> SeamWeld {
    seam(path, SeamLook::Band)
}

fn to(x: f64, y: f64) -> SeamPiece {
    SeamPiece::Line { to: [x, y, 0.0] }
}

/// Every zone's volume on both faces against `area`, the region within a radius of the path:
/// upper core, band, edge, then the same on the lower face.
fn zones_are(got: &Seam, area: impl Fn(f64) -> f64, what: &str) {
    assert_eq!(got.pieces.len(), 6, "{what}");
    let want = [area(R[0]), area(R[1]) - area(R[0]), area(R[2]) - area(R[1])].map(|a| a * H);
    for (i, piece) in got.pieces.iter().enumerate() {
        let side = if i < 3 { SpotSide::Upper } else { SpotSide::Lower };
        let zone = [SpotZone::Core, SpotZone::Band, SpotZone::Edge][i % 3];
        assert_eq!((piece.side, piece.zone), (side, zone), "{what}");
        close(volume(&piece.body), want[i % 3], &format!("{what}: {zone:?} {side:?}"));
        check(&piece.body, what);
    }
}

/// The region within `r` of a smooth path of length `l`: a strip with a half disc at each end.
fn smooth(r: f64, l: f64) -> f64 {
    2.0 * r * l + PI * r * r
}

/// A band along a 20 mm line: each zone the strip and round ends of its radius, on both outer
/// faces, standing off them by the marks' thickness.
#[test]
fn a_band_along_a_line_has_round_ends_on_both_faces() {
    let got = seam_weld(&band(vec![to(20.0, 0.0)])).unwrap();
    assert!(got.notices.is_empty());
    zones_are(&got, |r| smooth(r, 20.0), "line");
    for piece in &got.pieces {
        let bb = piece.body.exact_bounds().unwrap();
        let (lo, hi) = ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z]);
        let (lo, hi) = (lo[2], hi[2]);
        match piece.side {
            SpotSide::Upper => assert!((lo - T_U).abs() < 1e-9 && (hi - T_U - H).abs() < 1e-9, "{lo}..{hi}"),
            SpotSide::Lower => assert!((hi + T_L).abs() < 1e-9 && (lo + T_L + H).abs() < 1e-9, "{lo}..{hi}"),
            SpotSide::Through => panic!("a seam has no body through the sheets"),
        }
    }
}

/// A line and an arc of radius 10 a quarter turn on, turning left and turning right: smooth, so
/// no notice, and each zone the strip of the path's whole length with round ends.
#[test]
fn a_line_and_an_arc_either_way_make_one_smooth_band() {
    let length = 10.0 + 10.0 * FRAC_PI_2;
    let left = SeamPiece::Arc { center: [10.0, 10.0, 0.0], to: [20.0, 10.0, 0.0], turn: Turn::Left };
    let right = SeamPiece::Arc { center: [10.0, -10.0, 0.0], to: [20.0, -10.0, 0.0], turn: Turn::Right };
    for (arc, what) in [(left, "left"), (right, "right")] {
        let got = seam_weld(&band(vec![to(10.0, 0.0), arc])).unwrap();
        assert!(got.notices.is_empty(), "{what}: {:?}", got.notices);
        zones_are(&got, |r| smooth(r, length), what);
    }
}

/// A seam along a tilted face (normal (1, 2, -2)) away from the origin builds and passes the check,
/// with the same volumes.
#[test]
fn a_tilted_seam_builds_and_passes_the_check() {
    let start = [10.0, -3.0, 7.0];
    let end = [14.0, -5.0, 7.0];
    let s = SeamWeld { start, path: vec![SeamPiece::Line { to: end }], normal: [1.0, 2.0, -2.0], ..band(vec![]) };
    let length = 20f64.sqrt();
    zones_are(&seam_weld(&s).unwrap(), |r| smooth(r, length), "tilted");
}

/// Two 10 mm lines meeting at a right angle: a notice for the corner at joint 1, a quarter turn
/// left; each zone the union of the two lines' regions (both hold the disc round the corner, and the
/// square inside it).
#[test]
fn a_sharp_corner_builds_rounded_with_a_notice() {
    let got = seam_weld(&band(vec![to(10.0, 0.0), to(10.0, 10.0)])).unwrap();
    let [SeamNotice::Corner { joint, angle }] = got.notices[..] else {
        panic!("{:?}", got.notices);
    };
    assert_eq!(joint, 1);
    assert!((angle - FRAC_PI_2).abs() < 1e-9, "{angle}");
    zones_are(&got, |r| 2.0 * smooth(r, 10.0) - r * r - 0.75 * PI * r * r, "corner");
}

/// Spots every 6 mm along 18 mm: four discs apart (6 is over the edge zone's 5.25), each zone their
/// rings.
#[test]
fn spots_apart_are_a_disc_each() {
    let got = seam_weld(&seam(vec![to(18.0, 0.0)], SeamLook::Spots { pitch: 6.0 })).unwrap();
    zones_are(&got, |r| 4.0 * PI * r * r, "apart");
}

/// Spots every 2 mm along 2 mm: two discs overlapping in every zone, each zone their union.
#[test]
fn overlapping_spots_are_their_union() {
    let got = seam_weld(&seam(vec![to(2.0, 0.0)], SeamLook::Spots { pitch: 2.0 })).unwrap();
    let d: f64 = 2.0;
    let lens = |r: f64| 2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt();
    zones_are(&got, |r| 2.0 * PI * r * r - lens(r), "overlapping");
}

/// An arc of radius 2 is tighter than the edge zone (2.625): refused with the width that fits,
/// `2 · 2 / 1.05`.
#[test]
fn an_arc_tighter_than_the_seam_is_refused_with_the_width_that_fits() {
    let tight = SeamPiece::Arc { center: [0.0, 2.0, 0.0], to: [2.0, 2.0, 0.0], turn: Turn::Left };
    let got = seam_weld(&band(vec![tight]));
    let Err(SeamRefusal::ArcTighterThanSeam { piece: 0, fits }) = got else {
        panic!("{got:?}");
    };
    assert!((fits - 4.0 / 1.05).abs() < 1e-9, "{fits}");
}

/// No path, a bad width, pitch, normal or stack, a line of no length, an arc whose ends are not as
/// far from its centre or of a whole circle are bad input.
#[test]
fn bad_seams_are_refused_as_invalid_input() {
    let bad = |s: SeamWeld| matches!(seam_weld(&s), Err(SeamRefusal::Invalid(_)));
    let ok = band(vec![to(10.0, 0.0)]);
    assert!(bad(band(vec![])));
    for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(bad(SeamWeld { width: w, ..ok.clone() }), "width {w}");
    }
    for p in [0.0, -1.0, f64::NAN] {
        assert!(bad(seam(vec![to(10.0, 0.0)], SeamLook::Spots { pitch: p })), "pitch {p}");
    }
    assert!(bad(SeamWeld { normal: [0.0; 3], ..ok.clone() }));
    assert!(bad(SeamWeld { thicknesses: vec![1.0], ..ok.clone() }));
    assert!(bad(SeamWeld { thicknesses: vec![1.0, 0.0], ..ok.clone() }));
    assert!(bad(band(vec![to(0.0, 0.0)])));
    assert!(bad(band(vec![SeamPiece::Arc { center: [0.0, 10.0, 0.0], to: [12.0, 10.0, 0.0], turn: Turn::Left }])));
    assert!(bad(band(vec![SeamPiece::Arc { center: [0.0, 10.0, 0.0], to: [0.0, 0.0, 0.0], turn: Turn::Left }])));
    assert!(bad(SeamWeld { start: [f64::NAN, 0.0, 0.0], ..ok.clone() }));
    // centre 5e8 out, but the long way round over the top reaches y = 1.01e9, past the size limit
    let bulge = SeamPiece::Arc { center: [0.0, 5e8, 0.0], to: [1e8, 0.0, 0.0], turn: Turn::Right };
    assert!(bad(SeamWeld { start: [-1e8, 0.0, 0.0], path: vec![bulge], ..ok }));
}

// ---- where a seam sits ----

/// A 40 × 40 sheet from `z` up `t`, centred on the z axis.
fn sheet(z: f64, t: f64) -> Body {
    block([-20.0, -20.0, z], [40.0, 40.0, t])
}

/// The box from `corner` of `size`.
fn block(corner: [f64; 3], size: [f64; 3]) -> Body {
    let lo = Vec3::from(corner);
    box_solid(lo, lo + Vec3::from(size)).unwrap()
}

/// The record face of a box whose plane holds `p`.
fn face_at(b: &Body, p: [f64; 3]) -> usize {
    (0..b.face_count()).find(|&f| b.face_square_level(f, Vec3::new(0.0, 0.0, 1.0), p.into()).unwrap().is_some_and(|l| l.level.abs() < 1e-9)).unwrap()
}

fn site(sheets: &[&Body], picked: usize, start: [f64; 3], path: &[SeamPiece]) -> Result<SeamSite> {
    seam_site(sheets, picked, face_at(sheets[picked], start), start, path, W)
}

/// Drawn on the upper sheet's top from x = -10 to 10: the line runs down, the upper sheet first,
/// the path moved onto the faying face; the site builds the seam with its marks on the picked face
/// and the far one.
#[test]
fn a_seam_drawn_on_top_gives_the_frame_the_sheets_and_the_moved_path() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let s = site(&[&lower, &upper], 1, [-10.0, 0.0, 2.5], &[SeamPiece::Line { to: [10.0, 0.0, 2.5] }]).unwrap();
    assert!(s.normal.iter().zip([0.0, 0.0, -1.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{s:?}");
    assert!((s.start[2] - 1.5).abs() < 1e-9 && (s.start[0] + 10.0).abs() < 1e-9, "{s:?}");
    let [SeamPiece::Line { to }] = s.path[..] else {
        panic!("{s:?}");
    };
    assert!((to[0] - 10.0).abs() < 1e-9 && (to[2] - 1.5).abs() < 1e-9, "{to:?}");
    assert_eq!(s.sheets.iter().map(|x| x.sheet).collect::<Vec<_>>(), [1, 0]);
    let weld = SeamWeld {
        start: s.start,
        path: s.path,
        normal: s.normal,
        thicknesses: s.sheets.iter().map(|x| x.thickness).collect(),
        width: W,
        look: SeamLook::Band,
    };
    for piece in seam_weld(&weld).unwrap().pieces {
        let bb = piece.body.exact_bounds().unwrap();
        let (lo, hi) = ([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z]);
        match piece.side {
            // `Lower` is the first sheet's outer face: the picked one
            SpotSide::Lower => assert!(lo[2] >= 2.5 - 1e-9, "{lo:?}"),
            SpotSide::Upper => assert!(hi[2] <= 1e-9, "{hi:?}"),
            SpotSide::Through => panic!("a seam has no body through the sheets"),
        }
    }
}

/// The upper sheet has a notch from y = 1.5 out, 4 wide, over the middle of a path along the x
/// axis: both ends have room, but the path passes 1.5 from the notch, so a seam of 5 is refused
/// with the width that fits there, 3.
#[test]
fn a_path_passing_a_face_edge_is_refused_with_the_width_that_fits() {
    let lower = sheet(0.0, 1.5);
    let plate = sheet(1.5, 1.0);
    let notch = block([-2.0, 1.5, 1.0], [4.0, 30.0, 2.0]);
    let upper = solidcraft_kernel::boolean(&plate, &notch, BoolOp::Cut).unwrap().unwrap();
    let got = site(&[&lower, &upper], 0, [-15.0, 0.0, 0.0], &[SeamPiece::Line { to: [15.0, 0.0, 0.0] }]);
    let Err(SeamRefusal::PastFaceEdge { piece: 0, sheet: 1, width, fits, .. }) = got else {
        panic!("{got:?}");
    };
    assert_eq!(width, W);
    assert!((fits - 3.0).abs() < 1e-9, "fits {fits}");
}

/// An arc of radius 2 drawn on the top is tighter than the edge zone (2.625): refused at the site,
/// as the build refuses it, with the same width that fits.
#[test]
fn a_site_refuses_an_arc_tighter_than_the_seam() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let tight = SeamPiece::Arc { center: [0.0, 0.0, 2.5], to: [2.0, 0.0, 2.5], turn: Turn::Left };
    let got = site(&[&lower, &upper], 1, [-2.0, 0.0, 2.5], &[tight]);
    let Err(SeamRefusal::ArcTighterThanSeam { piece: 0, fits }) = got else {
        panic!("{got:?}");
    };
    assert!((fits - 4.0 / 1.05).abs() < 1e-9, "{fits}");
}

/// A path whose end runs off the picked face is refused at that joint with the spot's reason.
#[test]
fn a_joint_off_the_face_is_refused_at_that_joint() {
    let lower = sheet(0.0, 1.5);
    let upper = sheet(1.5, 1.0);
    let got = site(&[&lower, &upper], 0, [0.0; 3], &[SeamPiece::Line { to: [30.0, 0.0, 0.0] }]);
    assert_eq!(got, Err(SeamRefusal::AtJoint { joint: 1, why: SpotRefusal::PointOffFace }));
}
