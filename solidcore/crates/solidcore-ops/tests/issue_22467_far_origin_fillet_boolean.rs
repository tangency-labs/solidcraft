//! #22467: a boolean cut on a rounded body 2 m from the origin (past `MIN_SHIFT_CENTER_LENGTH_SQ`,
//! so the boolean translates the operands to the origin and translates the result back) keeps
//! exact geometry and tight tolerances without spending tens of seconds bisecting corner ball-patch
//! pole trims or refitting `psi` across moves.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

mod common;

use std::f64::consts::PI;

use common::{all_fixes, kernel_copy, volume};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore_ops::boolean::boolean;
use solidcore_ops::{blend, make_box, make_cylinder, Blend, Body, BooleanOp, CheckLevel, Corners};

/// A 60 x 40 x 20 box at `[x0, 0, 0]` with all 12 edges rounded `r = 4` (`Corners::Filled`).
fn rounded_box_at(x0: f64) -> Body {
    let body = make_box([x0, 0.0, 0.0], [60.0, 40.0, 20.0]).unwrap();
    let counts = body.counts().unwrap();
    assert_eq!(counts[1], 12);
    let edges: Vec<usize> = (0..12).collect();
    blend(&body, Blend::Round, &edges, &[4.0], &[], Corners::Filled).unwrap();
    body
}

/// Exact Steiner volume of a `60 x 40 x 20` box rounded by `r = 4` (Minkowski sum of the inner
/// `52 x 32 x 12` box and a sphere of radius `4`).
fn rounded_box_volume() -> f64 {
    let (x, y, z, r) = (52.0, 32.0, 12.0, 4.0);
    x * y * z
        + 2.0 * r * (x * y + y * z + z * x)
        + PI * r * r * (x + y + z)
        + (4.0 / 3.0) * PI * r * r * r
}

fn max_edge_tolerance(body: &Body) -> f64 {
    let (_, brep) = kernel_copy(body);
    brep.edges()
        .iter()
        .map(|(_, rec)| rec.tolerance)
        .fold(0.0_f64, f64::max)
}

#[test]
fn issue_22467_cut_on_far_origin_rounded_box_matches_origin() {
    all_fixes();

    let hole_vol = PI * 2.5 * 2.5 * 20.0;
    let expected_one = rounded_box_volume() - hole_vol;
    let expected_two = rounded_box_volume() - 2.0 * hole_vol;

    let origin_cut = boolean(
        rounded_box_at(0.0),
        make_cylinder([15.0, 20.0, -5.0], 2.5, 30.0).unwrap(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let origin_report = origin_cut.check(CheckLevel::All).unwrap();
    assert!(origin_report.is_valid(), "origin cut: {origin_report}");

    let far_cut = boolean(
        rounded_box_at(2000.0),
        make_cylinder([2015.0, 20.0, -5.0], 2.5, 30.0).unwrap(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let far_report = far_cut.check(CheckLevel::All).unwrap();
    assert!(far_report.is_valid(), "far cut: {far_report}");

    assert_eq!(far_cut.solid_count().unwrap(), 1);
    assert_eq!(far_cut.counts().unwrap(), origin_cut.counts().unwrap());

    let v_origin = volume(&origin_cut);
    let v_far = volume(&far_cut);
    assert!(
        (v_origin - expected_one).abs() < 1e-4,
        "origin volume {v_origin} vs expected {expected_one}"
    );
    assert!(
        (v_far - expected_one).abs() < 1e-4,
        "far volume {v_far} vs expected {expected_one}"
    );
    assert!(
        (v_far - v_origin).abs() < 1e-5,
        "far volume {v_far} drifted from origin {v_origin}"
    );

    let tol_origin = max_edge_tolerance(&origin_cut);
    let tol_far = max_edge_tolerance(&far_cut);
    assert!(
        tol_far <= tol_origin * 2.0 + 1e-6,
        "far max edge tolerance {tol_far:e} grew far above origin {tol_origin:e}"
    );

    // A second cut on the same far-origin body exercises cache reuse across consecutive moves.
    let far_second = boolean(
        far_cut,
        make_cylinder([2045.0, 20.0, -5.0], 2.5, 30.0).unwrap(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let second_report = far_second.check(CheckLevel::All).unwrap();
    assert!(second_report.is_valid(), "second far cut: {second_report}");
    assert_eq!(far_second.solid_count().unwrap(), 1);
    let v_second = volume(&far_second);
    assert!(
        (v_second - expected_two).abs() < 1e-4,
        "second far volume {v_second} vs expected {expected_two}"
    );

    // Deterministic work-counter check: the origin cut triggers no shift (`0` move-trim
    // bisection pieces), while the far-origin cut exercises `cover_moved_trims` across both
    // shifts without regressing to `MAX_PIECES` (32,768) bisections on the 8 corner-ball pole
    // trims (> 500,000 pieces before #22467).
    let (mut k_origin, origin_box) = kernel_copy(&rounded_box_at(0.0));
    let (_, origin_cyl) = kernel_copy(&make_cylinder([15.0, 20.0, -5.0], 2.5, 30.0).unwrap());
    k_origin.work_counters.reset();
    let _ = boolean_difference(&mut k_origin, origin_box, origin_cyl).unwrap();
    assert_eq!(
        k_origin.work_counters.move_trim_bisection_pieces(),
        0,
        "origin cut should not trigger rigid-move trim coverage"
    );

    let (mut k_far, far_box) = kernel_copy(&rounded_box_at(2000.0));
    let (_, far_cyl) = kernel_copy(&make_cylinder([2015.0, 20.0, -5.0], 2.5, 30.0).unwrap());
    k_far.work_counters.reset();
    let _ = boolean_difference(&mut k_far, far_box, far_cyl).unwrap();
    let far_pieces = k_far.work_counters.move_trim_bisection_pieces();
    assert!(
        far_pieces > 0 && far_pieces < 50_000,
        "far-origin cut evaluated {far_pieces} move-trim bisection pieces (expected in 1..50000)"
    );
}
