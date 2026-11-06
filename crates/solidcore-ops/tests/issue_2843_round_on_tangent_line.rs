//! #2843: a round on a half cylinder's base edge that ends where the flat face runs tangent into
//! a corner round. The cylinder's end disc stands on the seam, so a face lies between the seam
//! and the other side edge: the rail ends on the seam, and the disc's plane ends the round.

#![allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

mod common;

use std::f64::consts::PI;

use common::edge_at;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::boolean::boolean;
use solidcore_ops::{make_box, make_cylinder, session, Body, BooleanOp, CheckLevel, Error};

/// The box 30 x 20 x 20 with its vertical edge at the origin rounded R5, joined with the r2
/// cylinder 6 long along +x through (x0, 0, 15).
fn operand(x0: f64) -> Result<Body, Error> {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let b = make_box([0.0; 3], [30.0, 20.0, 20.0]).unwrap();
    let e = edge_at(&b, [0.0, 0.0, 10.0]);
    blend(&b, Blend::Round, &[e], &[5.0], &[], Corners::Filled).unwrap();
    let c = make_cylinder([0.0; 3], 2.0, 6.0).unwrap();
    c.transform(&[
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [x0, 0.0, 15.0, 1.0],
    ])
    .unwrap();
    Ok(boolean(b, c, BooleanOp::Union)?.unwrap())
}

/// The volume of the operand at `x0` with its base edge at z = 13 blended 0.5.
fn blended(x0: f64, kind: Blend) -> f64 {
    let b = operand(x0).unwrap();
    let e = edge_at(&b, [x0 + 3.0, 0.0, 13.0]);
    blend(&b, kind, &[e], &[0.5], &[], Corners::Filled).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "x0 {x0}: {report}");
    b.volume(1e-9).unwrap()
}

/// The round runs the 6 between the cylinder's two end planes, so it adds 6 A, with A the
/// section between the face, the circle and the ball: its quadrilateral (centre, the feet, the
/// edge) less the circle's segment and the ball's sector.
#[test]
fn the_round_ends_on_the_end_disc() {
    let quad = 0.212_372_435_695_794_78;
    let segment = 2.0 * (0.2_f64.asin() - 0.2);
    let sector = 0.2_f64.acos() / 8.0;
    let area = quad - segment - sector;
    let expected = 12000.0 - 500.0 * (1.0 - PI / 4.0) + 12.0 * PI + 6.0 * area;
    let v = blended(5.0, Blend::Round);
    assert!((v - expected).abs() < 1e-6, "{v}, expected {expected}");
}

/// The chamfer is the same prism wherever the cylinder stands on the flat.
#[test]
fn the_chamfer_ends_on_the_end_disc() {
    let at = blended(5.0, Blend::Chamfer);
    let clear = blended(6.0, Blend::Chamfer);
    assert!((at - clear).abs() < 1e-6, "{at}, clear of the seam {clear}");
}

/// The case's render: `ISSUE_2843_OUT=<dir>` gets the operand and the round as `.smb`.
#[test]
#[ignore = "writes files"]
fn bodies_for_the_picture() {
    let dir = std::env::var("ISSUE_2843_OUT").unwrap();
    let b = operand(5.0).unwrap();
    std::fs::write(format!("{dir}/before.smb"), b.to_smb(false).unwrap()).unwrap();
    let e = edge_at(&b, [8.0, 0.0, 13.0]);
    blend(&b, Blend::Round, &[e], &[0.5], &[], Corners::Filled).unwrap();
    std::fs::write(format!("{dir}/round.smb"), b.to_smb(false).unwrap()).unwrap();
}
