//! #22820: joining a sphere dome onto a tall cylinder's top (`center.z = 1260 > 1000`, so
//! `temporary_brep_transform` shifts the operands to the origin and translates the result back)
//! and running subsequent far-from-dome cylinder cuts keeps exact geometry and tight tolerances
//! without spending 8–30 s per boolean re-bisecting sphere trims during rigid moves.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

mod common;

use std::f64::consts::PI;
use std::time::Instant;

use common::{all_fixes, kernel_copy, rel, volume};
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore_ops::boolean::boolean;
use solidcore_ops::{make_cylinder, make_sphere, Body, BooleanOp, CheckLevel};

/// Cylinder Ø1200 x H2400 (`z in [0, 2400]`) joined with a Ø240 sphere dome on its top
/// (`center = [0, 0, 2400]`, radius `120`).
fn cylinder_and_sphere() -> (Body, Body) {
    let cyl = make_cylinder([0.0, 0.0, 0.0], 600.0, 2400.0).unwrap();
    let sph = make_sphere([0.0, 0.0, 2400.0], 120.0).unwrap();
    (cyl, sph)
}

/// Exact volume of the Ø1200 x H2400 cylinder plus the upper hemisphere of the Ø240 dome.
fn dome_volume() -> f64 {
    PI * 600.0 * 600.0 * 2400.0 + (2.0 / 3.0) * PI * 120.0 * 120.0 * 120.0
}

fn max_edge_tolerance(body: &Body) -> f64 {
    let (_, brep) = kernel_copy(body);
    brep.edges()
        .iter()
        .map(|(_, rec)| rec.tolerance)
        .fold(0.0_f64, f64::max)
}

#[test]
fn issue_22820_sphere_dome_join_and_subsequent_cuts_keep_exact_geometry_and_bounded_work() {
    all_fixes();

    let (cyl, sph) = cylinder_and_sphere();
    let domed = boolean(cyl, sph, BooleanOp::Union).unwrap().unwrap();
    let report = domed.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "domed union: {report}");
    assert_eq!(domed.solid_count().unwrap(), 1);

    let expected_dome = dome_volume();
    let v_domed = volume(&domed);
    assert!(
        rel(v_domed, expected_dome) < 1e-6,
        "domed volume {v_domed} vs expected {expected_dome}"
    );
    let tol_domed = max_edge_tolerance(&domed);

    // Subsequent hole cut far from the dome (`x = 350`, radius `40`, clear of the `r = 120` dome).
    let hole_r = 40.0;
    let hole_vol = PI * hole_r * hole_r * 2400.0;
    let cut1 = boolean(
        domed,
        make_cylinder([350.0, 0.0, -50.0], hole_r, 2500.0).unwrap(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let cut1_report = cut1.check(CheckLevel::All).unwrap();
    assert!(cut1_report.is_valid(), "first far hole cut: {cut1_report}");
    assert_eq!(cut1.solid_count().unwrap(), 1);
    let v_cut1 = volume(&cut1);
    let expected_cut1 = expected_dome - hole_vol;
    assert!(
        rel(v_cut1, expected_cut1) < 1e-6,
        "cut1 volume {v_cut1} vs expected {expected_cut1}"
    );
    let tol_cut1 = max_edge_tolerance(&cut1);
    assert!(
        tol_cut1 <= tol_domed * 2.0 + 1e-5,
        "cut1 max edge tolerance {tol_cut1:e} grew far above domed {tol_domed:e}"
    );

    // Second far-from-dome hole cut (as in a rectangular pattern of 3).
    let cut2 = boolean(
        cut1,
        make_cylinder([350.0, 150.0, -50.0], hole_r, 2500.0).unwrap(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let cut2_report = cut2.check(CheckLevel::All).unwrap();
    assert!(cut2_report.is_valid(), "second far hole cut: {cut2_report}");
    assert_eq!(cut2.solid_count().unwrap(), 1);
    let v_cut2 = volume(&cut2);
    let expected_cut2 = expected_dome - 2.0 * hole_vol;
    assert!(
        rel(v_cut2, expected_cut2) < 1e-6,
        "cut2 volume {v_cut2} vs expected {expected_cut2}"
    );

    // Deterministic work-counter check on a single `Kernel` across join + two far cuts:
    // the combined bbox midpoint is `(0, 0, 1260)` (`length_squared > 1e6`), so every boolean
    // shifts before and after the merge. Move-trim bisection pieces must stay tiny (well under
    // 10,000, vs ~200,000+ per move when sphere trims hit uncapped `MAX_PIECES` bisections).
    let (cyl_b, sph_b) = cylinder_and_sphere();
    let (mut k, cyl_brep) = kernel_copy(&cyl_b);
    let (_, sph_brep) = kernel_copy(&sph_b);
    k.work_counters.reset();
    let domed_brep = boolean_union(&mut k, cyl_brep, sph_brep).unwrap();
    let join_pieces = k.work_counters.move_trim_bisection_pieces();
    assert!(
        join_pieces < 10_000,
        "dome join evaluated {join_pieces} move-trim bisection pieces (expected < 10000)"
    );

    let (_, hole1_brep) = kernel_copy(&make_cylinder([350.0, 0.0, -50.0], hole_r, 2500.0).unwrap());
    k.work_counters.reset();
    let cut1_brep = boolean_difference(&mut k, domed_brep, hole1_brep).unwrap();
    let cut1_pieces = k.work_counters.move_trim_bisection_pieces();
    assert!(
        cut1_pieces < 10_000,
        "first far hole cut evaluated {cut1_pieces} move-trim bisection pieces (expected < 10000)"
    );

    let (_, hole2_brep) =
        kernel_copy(&make_cylinder([350.0, 150.0, -50.0], hole_r, 2500.0).unwrap());
    k.work_counters.reset();
    let _cut2_brep = boolean_difference(&mut k, cut1_brep, hole2_brep).unwrap();
    let cut2_pieces = k.work_counters.move_trim_bisection_pieces();
    assert!(
        cut2_pieces < 10_000,
        "second far hole cut evaluated {cut2_pieces} move-trim bisection pieces (expected < 10000)"
    );
}

#[test]
#[ignore = "timing benchmark for #22820 (run with --release --ignored --nocapture)"]
fn bench_issue_22820_dome_boolean_speed() {
    all_fixes();

    let (cyl_b, sph_b) = cylinder_and_sphere();
    let (mut k, cyl_brep) = kernel_copy(&cyl_b);
    let (_, sph_brep) = kernel_copy(&sph_b);

    k.work_counters.reset();
    let t0 = Instant::now();
    let domed = boolean_union(&mut k, cyl_brep, sph_brep).unwrap();
    let join_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let join_pieces = k.work_counters.move_trim_bisection_pieces();

    let (_, hole1) = kernel_copy(&make_cylinder([350.0, 0.0, -50.0], 40.0, 2500.0).unwrap());
    k.work_counters.reset();
    let t1 = Instant::now();
    let cut1 = boolean_difference(&mut k, domed, hole1).unwrap();
    let cut1_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let cut1_pieces = k.work_counters.move_trim_bisection_pieces();

    let (_, hole2) = kernel_copy(&make_cylinder([350.0, 150.0, -50.0], 40.0, 2500.0).unwrap());
    let (_, hole3) = kernel_copy(&make_cylinder([350.0, -150.0, -50.0], 40.0, 2500.0).unwrap());
    k.work_counters.reset();
    let t2 = Instant::now();
    let cut2 = boolean_difference(&mut k, cut1, hole2).unwrap();
    let _cut3 = boolean_difference(&mut k, cut2, hole3).unwrap();
    let pattern_ms = t2.elapsed().as_secs_f64() * 1000.0;
    let pattern_pieces = k.work_counters.move_trim_bisection_pieces();

    eprintln!(
        "issue #22820 bench: join={join_ms:.1} ms ({join_pieces} pieces), \
         cut1={cut1_ms:.1} ms ({cut1_pieces} pieces), \
         pattern2={pattern_ms:.1} ms ({pattern_pieces} pieces)"
    );
}
