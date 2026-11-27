//! #22467 follow-up: `solid.fillet` (`Turn::Auto`, `Corners::Filled`) at `x = 2000` matches
//! `x = 0` in geometry, tolerances, and speed:
//! 1. Planar rounds with 1-round open ends / 3-round corners (e.g. all 12 edges of a box) take
//!    `exact_round::round_on` under `Turn::Auto` just as under `Turn::Mitre`.
//! 2. Blends that require `FilletSession` + `stitch_and_orient` (e.g. 11 of 12 box edges with
//!    2-edge `Turn::Auto` corners + 6 `BallPatch` corners, or 2 adjacent edges) centre rational
//!    curve/surface enclosures and close `BallPatch`/iso trims cleanly without far-origin blowup.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

mod common;

use std::f64::consts::PI;
use std::time::Instant;

use common::{all_fixes, kernel_copy, volume};
use solidcore_ops::{
    blend_with, make_box, pick, Blend, BlendOptions, Body, CheckLevel, Corners, Turn,
};

fn auto_options() -> BlendOptions {
    BlendOptions {
        turn: Turn::Auto,
        ..BlendOptions::default()
    }
}

/// Exact Steiner volume of a `60 x 40 x 20` box rounded by `r = 4` on all 12 edges.
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

/// 11 of 12 edge midpoints of a `60 x 40 x 20` box at `[x0, 0, 0]`: leaves the vertical edge at
/// `(x0, 0, 10)` sharp so two vertices have 2 rounded edges (forcing `FilletSession` under
/// `Turn::Auto`) and six vertices have 3 rounded edges (building 6 `BallPatch` corners).
fn eleven_box_edge_midpoints(x0: f64) -> [[f64; 3]; 11] {
    [
        [x0 + 30.0, 0.0, 20.0],
        [x0 + 60.0, 20.0, 20.0],
        [x0 + 30.0, 40.0, 20.0],
        [x0, 20.0, 20.0],
        [x0 + 30.0, 0.0, 0.0],
        [x0 + 60.0, 20.0, 0.0],
        [x0 + 30.0, 40.0, 0.0],
        [x0, 20.0, 0.0],
        [x0 + 60.0, 0.0, 10.0],
        [x0 + 60.0, 40.0, 10.0],
        [x0, 40.0, 10.0],
    ]
}

fn fillet_box_all_12_auto(x0: f64) -> Body {
    let body = make_box([x0, 0.0, 0.0], [60.0, 40.0, 20.0]).unwrap();
    let edges: Vec<usize> = (0..12).collect();
    blend_with(
        &body,
        Blend::Round,
        &edges,
        &[4.0],
        &[],
        Corners::Filled,
        auto_options(),
    )
    .unwrap();
    body
}

fn fillet_box_11_edges_auto(x0: f64) -> Body {
    let body = make_box([x0, 0.0, 0.0], [60.0, 40.0, 20.0]).unwrap();
    let pts = eleven_box_edge_midpoints(x0);
    let edges = pick::edges(&body, &pts, 1e-3).unwrap().unwrap();
    assert_eq!(edges.len(), 11);
    blend_with(
        &body,
        Blend::Round,
        &edges,
        &[4.0],
        &[],
        Corners::Filled,
        auto_options(),
    )
    .unwrap();
    body
}

fn fillet_box_2_adjacent_auto(x0: f64) -> Body {
    let body = make_box([x0, 0.0, 0.0], [60.0, 40.0, 20.0]).unwrap();
    let pts = [[x0 + 30.0, 0.0, 20.0], [x0 + 60.0, 20.0, 20.0]];
    let edges = pick::edges(&body, &pts, 1e-3).unwrap().unwrap();
    assert_eq!(edges.len(), 2);
    blend_with(
        &body,
        Blend::Round,
        &edges,
        &[4.0],
        &[],
        Corners::Filled,
        auto_options(),
    )
    .unwrap();
    body
}

#[test]
fn issue_22467_far_origin_fillet_turn_auto_box_matches_origin() {
    all_fixes();

    let origin = fillet_box_all_12_auto(0.0);
    let far = fillet_box_all_12_auto(2000.0);

    let r_origin = origin.check(CheckLevel::All).unwrap();
    let r_far = far.check(CheckLevel::All).unwrap();
    assert!(r_origin.is_valid(), "origin fillet: {r_origin}");
    assert!(r_far.is_valid(), "far fillet: {r_far}");
    assert_eq!(far.solid_count().unwrap(), 1);
    assert_eq!(far.counts().unwrap(), origin.counts().unwrap());

    let expected = rounded_box_volume();
    let v_origin = volume(&origin);
    let v_far = volume(&far);
    assert!(
        (v_origin - expected).abs() < 1e-4,
        "origin volume {v_origin} vs expected {expected}",
    );
    assert!(
        (v_far - expected).abs() < 1e-4,
        "far volume {v_far} vs expected {expected}",
    );
    assert!(
        (v_far - v_origin).abs() < 1e-5,
        "far volume {v_far} drifted from origin {v_origin}",
    );

    let tol_origin = max_edge_tolerance(&origin);
    let tol_far = max_edge_tolerance(&far);
    assert!(
        tol_far <= tol_origin * 2.0 + 1e-6,
        "far max edge tolerance {tol_far:e} grew far above origin {tol_origin:e}",
    );
}

#[test]
fn issue_22467_far_origin_fillet_session_turn_auto_matches_origin() {
    all_fixes();

    for (label, build) in [
        (
            "11 edges (6 ball corners + 2 auto turns)",
            fillet_box_11_edges_auto as fn(f64) -> Body,
        ),
        (
            "2 adjacent edges (auto swept turn)",
            fillet_box_2_adjacent_auto as fn(f64) -> Body,
        ),
    ] {
        let origin = build(0.0);
        let far = build(2000.0);

        let r_origin = origin.check(CheckLevel::All).unwrap();
        let r_far = far.check(CheckLevel::All).unwrap();
        assert!(r_origin.is_valid(), "{label} origin: {r_origin}");
        assert!(r_far.is_valid(), "{label} far: {r_far}");
        assert_eq!(far.solid_count().unwrap(), 1, "{label}");
        assert_eq!(far.counts().unwrap(), origin.counts().unwrap(), "{label}");

        let v_origin = volume(&origin);
        let v_far = volume(&far);
        assert!(
            (v_far - v_origin).abs() < 1e-4,
            "{label}: far volume {v_far} drifted from origin {v_origin}",
        );

        let tol_origin = max_edge_tolerance(&origin);
        let tol_far = max_edge_tolerance(&far);
        assert!(
            tol_far <= tol_origin * 2.0 + 1e-6,
            "{label}: far max edge tolerance {tol_far:e} grew far above origin {tol_origin:e}",
        );
    }
}

#[test]
#[ignore = "release timing benchmark: run with `cargo test --release -p solidcore-ops --test ops issue_22467_far_origin_fillet_auto::issue_22467_far_origin_fillet_bench_release -- --ignored --nocapture`"]
fn issue_22467_far_origin_fillet_bench_release() {
    all_fixes();

    let measure = |iters: usize, f: fn(f64) -> Body, x0: f64| -> f64 {
        let _ = f(x0);
        let t0 = Instant::now();
        for _ in 0..iters {
            let _ = f(x0);
        }
        t0.elapsed().as_secs_f64() * 1e3 / (iters as f64)
    };

    for (label, iters, build) in [
        (
            "12-edge box (Turn::Auto, exact_round)",
            5,
            fillet_box_all_12_auto as fn(f64) -> Body,
        ),
        (
            "11-edge box (Turn::Auto, FilletSession + 6 BallPatch)",
            3,
            fillet_box_11_edges_auto as fn(f64) -> Body,
        ),
        (
            "2-edge box (Turn::Auto, FilletSession swept turn)",
            5,
            fillet_box_2_adjacent_auto as fn(f64) -> Body,
        ),
    ] {
        let origin_ms = measure(iters, build, 0.0);
        let far_ms = measure(iters, build, 2000.0);
        let ratio = far_ms / origin_ms.max(1e-6);
        eprintln!(
            "[#22467 bench] {label}: x=0 {origin_ms:.2} ms, x=2000 {far_ms:.2} ms (ratio {ratio:.2}x)",
        );
        assert!(
            far_ms <= origin_ms * 3.0 + 5.0,
            "{label}: far-origin ({far_ms:.2} ms) was {ratio:.2}x slower than origin ({origin_ms:.2} ms)",
        );
    }
}
