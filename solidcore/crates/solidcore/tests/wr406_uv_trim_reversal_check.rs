//! WR-406 (tangency-labs/solidcore#891) through the public check: a bored plate's
//! exact hole trims are straightened by `check_brep` by default, and the body it has
//! just changed is then reported as kinked; with
//! `Kernel::fixes.uv_trim_reversal_on_surface` the trims are left exact and the check
//! passes.
//!
//! The plate is built only through public operations: a 60 x 10 profile swept 4 up
//! (`create_linear_sweep`), bored through the 4 thickness by a radius-1 cylinder
//! (`create_cylinder`, `transform`, `boolean_difference`). The bore's frame is turned
//! 35 degrees about its own axis so the hole trims' control polygons start 35 degrees
//! round the thin ellipse each 60 x 4 face's (u, v) makes of the circle: the raw
//! (u, v) turn at one end then folds back far enough to be taken for a reversal, while
//! the turn on the face's surface is square.
//!
//! Measured outcomes are recorded in
//! `plan/upstream-issues/WR-406--addendum-2026-11-02-public-check-measured.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives as api;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeuseId, EdgeuseOwner};
use solidcore::Kernel;

/// The plate: a 60 x 10 profile swept 4 up, bored through the 4 thickness at
/// (x, z) = (30, 2) by a radius-1 cylinder whose frame is turned 35 degrees about
/// its own axis (the case whose raw (u, v) check folds).
fn bored_plate(k: &mut Kernel) -> Brep {
    bored_plate_turn(k, 35.0_f64.to_radians())
}

/// The same plate bored by a cylinder whose frame is turned `turn` radians about
/// the bore's own z axis (0 makes the bore axis-aligned: the control).
fn bored_plate_turn(k: &mut Kernel, turn: f64) -> Brep {
    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 10.0), (0.0, 10.0)];
    let profile: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            Curve::from(
                create_line_segment(Point3::new(a.0, a.1, 0.0), Point3::new(b.0, b.1, 0.0))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    let plate = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 4.0, true).unwrap();

    let mut bore = api::create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 1.0, 12.0).unwrap();
    // the bore's z axis onto +y (through the 4 thickness), its x axis turned `turn`
    // degrees from the plate's u axis about that z axis
    let (c, s) = (turn.cos(), turn.sin());
    let frame = Axis2Placement::new(
        Point3::new(30.0, -1.0, 2.0),
        Vec3::new(c, 0.0, -s),
        Vec3::new(-s, 0.0, -c),
    );
    transform(k, &mut bore, &frame, None, false).unwrap();
    boolean_difference(k, plate, bore).unwrap()
}

/// The face of the loop the edge use belongs to.
fn trim_face(b: &Brep, eu: EdgeuseId) -> Option<solidcore::topology::FaceId> {
    let lu = match b.edgeuse(eu).ok()?.owner {
        EdgeuseOwner::Loopuse(lu) => lu?,
        _ => return None,
    };
    b.faceuse(b.loopuse(lu).ok()?.faceuse?).ok()?.face
}

/// The stored UV trims with nine control points, in edge-list order: the two hole
/// trims, each with the Euclidean control points of its B-spline.
fn hole_trims(b: &Brep) -> Vec<(EdgeuseId, Vec<Vec3>)> {
    let mut out = Vec::new();
    for e in b.edge_list().to_vec() {
        for eu in b.edge(e).unwrap().edgeuses.clone() {
            let Some(trim) = eu1::uv_trim_curve_pointer(b, eu).unwrap() else {
                continue;
            };
            let CurveKind::BSpline(base) = trim.kind() else {
                continue;
            };
            if base.dim() != 2 {
                continue;
            }
            let Some(nurb) = base.gw_nurb() else {
                continue;
            };
            if nurb.cpts().len() != 9 {
                continue;
            }
            out.push((eu, nurb.cpts().iter().map(|p| p.to_euclid()).collect()));
        }
    }
    out
}

/// The (u, v) extents in 3D of the face that holds `eu`'s trim.
fn trim_face_extents(k: &Kernel, b: &Brep, eu: EdgeuseId) -> (f64, f64) {
    let face = trim_face(b, eu).expect("a trimmed edge use on a loop");
    let surface = b
        .face(face)
        .unwrap()
        .surface
        .clone()
        .expect("a face surface");
    let p00 = surface
        .evaluate_point(k, Point2::new(0.0, 0.0))
        .expect("the face's corner");
    let p10 = surface
        .evaluate_point(k, Point2::new(1.0, 0.0))
        .expect("the face's corner");
    let p01 = surface
        .evaluate_point(k, Point2::new(0.0, 1.0))
        .expect("the face's corner");
    (p00.distance(p10), p00.distance(p01))
}

struct Measured {
    pointers_status: i32,
    assert_valid: Option<bool>,
    lines: Vec<String>,
    before: Vec<(EdgeuseId, Vec<Vec3>)>,
    after: Vec<(EdgeuseId, Vec<Vec3>)>,
}

fn measure(flag: bool) -> Measured {
    measure_turn(flag, 35.0_f64.to_radians())
}

/// The measurement for one flag setting and one bore turn about its own axis.
fn measure_turn(flag: bool, turn: f64) -> Measured {
    let mut k = Kernel::default();
    k.fixes.uv_trim_reversal_on_surface = flag;
    let mut body = bored_plate_turn(&mut k, turn);
    assert_eq!(
        faces(&body, None).unwrap().len(),
        7,
        "the bored plate's faces"
    );
    let before = hole_trims(&body);
    assert_eq!(before.len(), 2, "the plate has two hole trims");
    let check = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
    let lines = check.lines();
    let after = hole_trims(&body);
    eprintln!(
        "flag {flag}: pointers_status {} assert_valid {:?}, {} line(s) {:#?}",
        check.pointers_status,
        check.assert_valid,
        lines.len(),
        lines
    );
    for (tag, trims) in [("before", &before), ("after", &after)] {
        for (eu, cps) in trims {
            eprintln!("  {tag} {eu:?}: middle control point {:?}", cps[1]);
        }
    }
    Measured {
        pointers_status: check.pointers_status,
        assert_valid: check.assert_valid,
        lines,
        before,
        after,
    }
}

/// The kink report the default check emits after straightening a trim.
const KINK: &str = "SmBSplineCurve::Test #7 Bad CurveKink";

#[test]
fn wr406_bored_plate_check_reverses_or_keeps_the_hole_trims() {
    // the flag is opt-in: the default is the pinned C++ raw (u, v) test
    assert!(!Kernel::default().fixes.uv_trim_reversal_on_surface);

    // every trimmed face is one of the plate's 60 x 4 sides
    let mut k = Kernel::default();
    let body = bored_plate(&mut k);
    for (eu, _) in hole_trims(&body) {
        let (u, v) = trim_face_extents(&k, &body, eu);
        assert!(
            (u - 60.0).abs() < 1e-9 && (v - 4.0).abs() < 1e-9,
            "face {u} x {v}"
        );
    }

    // default: the check straightens each hole trim's first-end control point onto
    // the chord and then reports the body it changed as kinked (twice, one per trim)
    let off = measure(false);
    assert_eq!(off.pointers_status, 1000);
    assert_eq!(off.assert_valid, Some(false));
    assert_eq!(off.lines.len(), 2, "{:#?}", off.lines);
    for line in &off.lines {
        assert!(line.starts_with(KINK), "{line}");
    }
    assert_eq!(off.before.len(), off.after.len());
    for ((eu, before), (eu2, after)) in off.before.iter().zip(&off.after) {
        assert_eq!(eu, eu2);
        let moved: Vec<usize> = (0..before.len())
            .filter(|&i| before[i] != after[i])
            .collect();
        assert_eq!(moved, vec![1], "eu {eu:?} moved {moved:?}");
        assert_eq!(after[1].x, (before[0].x + before[2].x) / 2.0);
        assert_eq!(after[1].y, (before[0].y + before[2].y) / 2.0);
        assert_eq!(before[0], after[0]);
        assert_eq!(before[2], after[2]);
    }

    // with the flag: the same body's trims are judged on the faces' surfaces, left
    // exact, and the check passes with no report
    let on = measure(true);
    assert_eq!(on.pointers_status, 1000);
    assert_eq!(on.assert_valid, Some(true));
    assert!(on.lines.is_empty(), "{:#?}", on.lines);
    assert_eq!(on.before, on.after, "the flag left the trims alone");
    assert_eq!(off.before, on.before, "both runs start from the same trims");
    assert_eq!(off.before, on.after, "the flag's run is the untouched body");
}

/// The control: with an axis-aligned bore (`turn = 0`) the same plate builds, bores
/// and checks identically with the flag off and on (the 35-degree turn is what folds
/// the raw (u, v) reversal test).
#[test]
fn wr406_axis_aligned_bore_keeps_the_check_clean() {
    let mut results = Vec::new();
    for flag in [false, true] {
        let m = measure_turn(flag, 0.0);
        assert_eq!(m.pointers_status, 1000, "flag {flag}");
        assert_eq!(m.assert_valid, Some(true), "flag {flag}");
        assert!(m.lines.is_empty(), "flag {flag}: {:#?}", m.lines);
        assert_eq!(m.before, m.after, "the check did not touch the trims");
        results.push(m);
    }
    assert_eq!(
        results[0].before, results[1].before,
        "the flag did not touch the axis-aligned trims"
    );
}
