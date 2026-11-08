//! solidcraft#13085 (tangency-labs/solidcore#3150): a revolve cut whose tool has a cylindrical
//! face a hair (1e-10 to 6e-10) outside the wall it is meant to coincide with, meeting it at a
//! torus/cylinder tangent junction. The intersection curve's hit on the cup's rim is
//! ill-conditioned there, so the proto vertex at the corner keeps the mean of that hit and the
//! corner, and the end fit of `SmEdgeDefinition::Imprint` (the imprint flags) stops the rim
//! edge short of the corner vertex the B-rep already had: loops out of tolerance. With
//! `imprint_fits_edge_end_to_final_vertex` the end goes to that vertex (WR-753).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_ops::boolean::{boolean, BooleanOp};
use solidcore_ops::sweep::{extrude, revolve, Curve, DraftCorner};
use solidcore_ops::{session, Body, CheckLevel};

/// SolidCraft's end-fit flags and the fix under test.
fn imprint_fixes() {
    session::set_fixes([
        "imprint_trims_edge_to_proto_vertex",
        "imprint_extends_edge_to_proto_vertex",
        "imprint_fits_edge_end_to_final_vertex",
    ])
    .unwrap();
}

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

fn arc(
    center: [f64; 3],
    axis: [f64; 3],
    reference: [f64; 3],
    radius: f64,
    start: f64,
    end: f64,
) -> Curve {
    Curve::Arc {
        center,
        axis,
        reference,
        radius,
        start,
        end,
    }
}

/// The cup: a half-disc of radius 24.75 (y <= 0) with a 7 mm lug down to y = -34.75 (its end
/// round r 3.5 about (0, -31.25), its junctions with the ring filleted r 5 about
/// (+-8.5, -28.5099), tangent to the ring and to the lug's sides x = +-3.5) and a 2.5 mm eye
/// hole through the lug, extruded 20; a half-disc of radius 23.75 cut 19 deep from the top
/// (a 1 mm wall and floor).
fn cup() -> Body {
    const R: f64 = 24.75;
    const FY: f64 = -28.509_866_713_120_786;
    let z = [0.0, 0.0, 1.0];
    let zn = [0.0, 0.0, -1.0];
    let x = [1.0, 0.0, 0.0];
    // The ring's tangent point with a fillet: along the line from the origin through the
    // fillet's centre, 24.75 out.
    // atan2(-23.718292475621443, -7.071428571428571)
    let t = -1.860_548_028_230_959_4_f64;
    let outline = [
        line([R, 0.0, 0.0], [-R, 0.0, 0.0]),
        arc([0.0; 3], z, x, R, PI, t + TAU),
        arc([-8.5, FY, 0.0], zn, x, 5.0, -(PI + t), 0.0),
        line([-3.5, FY, 0.0], [-3.5, -31.25, 0.0]),
        arc([0.0, -31.25, 0.0], z, x, 3.5, PI, TAU),
        line([3.5, -31.25, 0.0], [3.5, FY, 0.0]),
        arc([8.5, FY, 0.0], zn, x, 5.0, -PI, t),
        arc([0.0; 3], z, x, R, -(PI + t), 0.0),
        arc([0.0, -31.25, 0.0], zn, x, 2.5, -TAU, 0.0),
    ];
    let body = extrude(&outline, z, 20.0, 0.0, DraftCorner::Curvature, true).unwrap();
    let inner = [
        line([23.75, 0.0, 1.0], [-23.75, 0.0, 1.0]),
        arc([0.0, 0.0, 1.0], z, x, 23.75, PI, TAU),
    ];
    let hollow = extrude(&inner, z, 19.0, 0.0, DraftCorner::Curvature, true).unwrap();
    boolean(body, hollow, BooleanOp::Subtract)
        .unwrap()
        .expect("the cup")
}

/// The tool: the profile Y in [-34.75, wall], Z in [5, 20], its lower corner a quarter arc of
/// radius 10 about (-34.75, 15) ending tangent to the vertical line at (`wall`, 15), revolved
/// `angle` about Z, placed symmetrically about the YZ plane (the profile turned by -angle/2,
/// whose sine and cosine `half` gives).
fn tool(wall: f64, half: (f64, f64)) -> Body {
    let (s, c) = (-half.0, half.1);
    let turn = |p: [f64; 3]| [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]];
    let turn_dir = |d: [f64; 3]| [c * d[0] - s * d[1], s * d[0] + c * d[1], d[2]];
    let profile = [
        line(turn([0.0, -34.75, 20.0]), turn([0.0, -34.75, 5.0])),
        arc(
            turn([0.0, -34.75, 15.0]),
            turn_dir([1.0, 0.0, 0.0]),
            turn_dir([0.0, 1.0, 0.0]),
            10.0,
            -FRAC_PI_2,
            0.0,
        ),
        line(turn([0.0, wall, 15.0]), turn([0.0, wall, 20.0])),
        line(turn([0.0, wall, 20.0]), turn([0.0, -34.75, 20.0])),
    ];
    revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], angle, true).unwrap()
}

/// The two revolve angles, 28 and 36 degrees: their names, and the sine and cosine of half
/// of each (the ops tests compute no geometry, so the values are written out).
const ANGLES: [(&str, (f64, f64)); 2] = [
    (
        "28 deg",
        (0.241_921_895_599_667_73, 0.970_295_726_275_996_5),
    ),
    ("36 deg", (0.309_016_994_374_947_4, 0.951_056_516_295_153_5)),
];

/// The cut, or why it failed: the boolean's error, or no body.
fn cut(wall: f64, angle: (&str, (f64, f64))) -> Result<Body, String> {
    match boolean(cup(), tool(wall, angle.1), BooleanOp::Subtract) {
        Ok(Some(b)) => Ok(b),
        Ok(None) => Err("no body".to_string()),
        Err(e) => Err(format!("{e}")),
    }
}

/// The tool's vertical face exactly on the cup's outer wall: the cut is right.
#[test]
fn a_tool_face_on_the_wall_cuts() {
    imprint_fixes();
    for angle in ANGLES {
        let b = cut(-24.75, angle).unwrap_or_else(|e| panic!("{}: {e}", angle.0));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(
            report.is_valid(),
            "{}:
{}",
            angle.0,
            report.text
        );
    }
}

/// The tool's vertical face a hair outside the wall (the reporter's sketch left it 4e-10
/// out): at 36 degrees the tool's end planes reach into the lug's fillets and the rim edge
/// between the fillets' tangent points is rebuilt from the intersection; at 28 degrees they
/// clear the fillets. Both cuts are right.
#[test]
fn a_tool_face_a_hair_outside_the_wall_cuts_too() {
    imprint_fixes();
    for off in [1e-10, 4e-10, 6e-10] {
        for angle in ANGLES {
            let b = cut(-24.75 - off, angle)
                .unwrap_or_else(|e| panic!("offset {off}, {}: {e}", angle.0));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(
                report.is_valid(),
                "offset {off}, {}:
{}",
                angle.0,
                report.text
            );
        }
    }
}
