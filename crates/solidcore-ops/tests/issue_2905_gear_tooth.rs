//! #2905: a round on a gear tooth's arc edges. The solver's first station sits past the arc
//! face's edge, on the face's extension. The class was read there with the offset point on the
//! extension and the face's point clamped to its edge, so the ball's radius came out 0.50094
//! for a round of 0.5, and the exact class's check refused the net (status 1001).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

use std::f64::consts::PI;

use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{session, Body, CheckLevel};
use solidcore_record::brep::Brep as Record;

const R: f64 = 0.5;

/// The tooth (SolidCraft's `65-sketch-gear-tooth`, its 30 degree sector of R40 root, R50 tip and
/// R15 flank arcs, 8 thick) with edge `e` rounded `R`: its volume before and after.
fn rounded(e: usize) -> (f64, f64) {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let record: Record = serde_json::from_str(include_str!("data/gear_tooth.record.json")).unwrap();
    let (body, _) = Body::from_record(&record, true).unwrap();
    let before = body.volume(1e-9).unwrap();
    blend(&body, Blend::Round, &[e], &[R], &[], Corners::Filled).unwrap();
    let report = body.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "edge {e}: {report}");
    (before, body.volume(1e-9).unwrap())
}

/// The tip arc's edge on the start face, R50 over 2 asin(2.5 / 50): a torus piece removes, by
/// Pappus, the corner's area times the path of its centroid. The round runs on past the arc at
/// both ends by less than its radius, so the rest is at most that area over `2 R`.
#[test]
fn the_tip_arc_rounds_by_its_torus() {
    let (before, after) = rounded(6);
    let area = R * R * (1.0 - PI / 4.0);
    let centroid = R * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    // 2 asin(2.5 / 50)
    let pappus = area * (50.0 - centroid) * 0.100_041_713_7;
    let removed = before - after;
    assert!(
        removed >= pappus && removed <= pappus + area * 2.0 * R,
        "removed {removed}, the torus piece {pappus}"
    );
}

/// The flank's edge, R15 and concave to the tip's, has no such form: what it held before the
/// exact extension, 3987.520, and a body that checks.
#[test]
fn a_flank_arc_rounds() {
    let (_, after) = rounded(5);
    assert!((after - 3987.520).abs() < 1e-3, "{after}");
}
