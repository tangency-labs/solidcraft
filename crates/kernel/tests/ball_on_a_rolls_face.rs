//! The engine's `a_ball_centred_on_a_rolls_face_is_cut_where_it_turns_edge_on` (#6748) without
//! the sheet: the half roll r 48..50 about the z axis, z 0..40, on the y > 0 side, built the way
//! the sheet's fold builds it (`sheet_surface_fold`: each curved face a `SurfSpec::Revolved` line
//! the sew makes a record cylinder of, framed at the line, the rims arcs, the rest planes), less
//! the kernel's sphere r 6 about (0, d, 20) through `boolean`, which checks the result as an
//! `EachStep` evaluation does (SMLib's own boolean, then `validate_and_update_tolerances`, then
//! the Level 2 check, every operand imported from its record).
//!
//! At d = 50 both poles of the sphere, (0, 50, 26) and (0, 50, 14), lie on the roll's outside.
//! The plain-solidcore reproducer (primitive cylinder and sphere, no record round trip, no
//! tolerance update) checks clean there, so whatever differs is in this path.
//!
//! The fold's cylinder frame depends on the panel: its x axis points at the ruling the roll's
//! angle starts from, its z axis along the roll's turn (either way round), and its origin is that
//! ruling's first end. Every combination is run; the failing ones say which frame trips.
//!
//! With `BALL_DUMP_DIR` set, each case writes its operands as SMLib reads them (`<case>-roll.smb`,
//! `<case>-ball.smb`), for `solidcore/crates/solidcore/tests/ball_operands_from_solidcraft.rs`.

use std::f64::consts::PI;
use std::path::Path;

use solidcore_record::Vec3;
use solidcraft_kernel::{BoolOp, Body, EdgeSpec, FaceSpec, SurfSpec, boolean, sew, sphere};

const OUTER: f64 = 50.0;
const INNER: f64 = 48.0;
const HEIGHT: f64 = 40.0;

fn at(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn ends(e: &EdgeSpec) -> (Vec3, Vec3) {
    match e {
        EdgeSpec::Line { a, b } | EdgeSpec::Arc { a, b, .. } => (*a, *b),
        EdgeSpec::Chain { ctrl, .. } => (ctrl.first().copied().unwrap_or_default(), ctrl.last().copied().unwrap_or_default()),
    }
}

/// How the fold frames a roll's cylinder: which ruling its angle starts from (x = +R or -R), which
/// way its axis points, and which end of that ruling the line starts at.
#[derive(Clone, Copy, Debug)]
struct Frame {
    start_plus_x: bool,
    axis_up: bool,
    line_from_bottom: bool,
}

impl Frame {
    fn all() -> Vec<Frame> {
        let mut v = Vec::new();
        for start_plus_x in [true, false] {
            for axis_up in [true, false] {
                for line_from_bottom in [true, false] {
                    v.push(Frame { start_plus_x, axis_up, line_from_bottom });
                }
            }
        }
        v
    }

    fn name(self) -> String {
        format!(
            "start{}-axis{}-from{}",
            if self.start_plus_x { "+x" } else { "-x" },
            if self.axis_up { "+z" } else { "-z" },
            if self.line_from_bottom { "0" } else { "40" }
        )
    }

    /// The roll's surface of radius `r`, as `SurfSpec::Revolved`.
    fn revolved(self, r: f64) -> SurfSpec {
        let x = if self.start_plus_x { r } else { -r };
        let (z0, z1) = if self.line_from_bottom { (0.0, HEIGHT) } else { (HEIGHT, 0.0) };
        let axis = if self.axis_up { Vec3::Z } else { -Vec3::Z };
        SurfSpec::Revolved { a: at(x, 0.0, z0), b: at(x, 0.0, z1), axis_origin: Vec3::ZERO, axis, angle: PI }
    }
}

/// The half roll's outline at height `z`, counter-clockwise from above: the outside arc, the end
/// at -x, the inside arc back, the end at +x.
fn outline(z: f64) -> Vec<EdgeSpec> {
    vec![
        EdgeSpec::Arc { a: at(OUTER, 0.0, z), b: at(-OUTER, 0.0, z), mid: at(0.0, OUTER, z) },
        EdgeSpec::Line { a: at(-OUTER, 0.0, z), b: at(-INNER, 0.0, z) },
        EdgeSpec::Arc { a: at(-INNER, 0.0, z), b: at(INNER, 0.0, z), mid: at(0.0, INNER, z) },
        EdgeSpec::Line { a: at(INNER, 0.0, z), b: at(OUTER, 0.0, z) },
    ]
}

fn half_roll(frame: Frame) -> Result<Body, String> {
    let (bottom, top) = (outline(0.0), outline(HEIGHT));
    let mut faces = vec![
        FaceSpec { loops: vec![top.clone()], surface: SurfSpec::Plane, probe: None },
        FaceSpec { loops: vec![bottom.iter().rev().map(EdgeSpec::reversed).collect()], surface: SurfSpec::Plane, probe: None },
    ];
    for (i, (low, high)) in bottom.iter().zip(&top).enumerate() {
        let ((a, b), (c, d)) = (ends(low), ends(high));
        let (surface, probe) = match i {
            0 => (frame.revolved(OUTER), Some((at(0.0, OUTER, HEIGHT / 2.0), Vec3::Y))),
            2 => (frame.revolved(INNER), Some((at(0.0, INNER, HEIGHT / 2.0), -Vec3::Y))),
            _ => (SurfSpec::Plane, None),
        };
        faces.push(FaceSpec {
            loops: vec![vec![low.clone(), EdgeSpec::Line { a: b, b: d }, high.reversed(), EdgeSpec::Line { a: c, b: a }]],
            surface,
            probe,
        });
    }
    sew(&faces).map_err(|e| format!("sew: {e}"))
}

/// The roll framed by `frame` less the r 6 ball about (0, `d`, 20): the boolean's error, if any.
fn cut(frame: Frame, d: f64) -> Option<String> {
    let roll = match half_roll(frame) {
        Ok(r) => r,
        Err(e) => return Some(e),
    };
    let ball = sphere(at(0.0, d, 20.0), 6.0).unwrap();
    if let Some(dir) = std::env::var_os("BALL_DUMP_DIR") {
        let dir = Path::new(&dir);
        let case = format!("{}-d{d}", frame.name());
        roll.smlib().unwrap().write(&dir.join(format!("{case}-roll.smb")), false).unwrap();
        ball.smlib().unwrap().write(&dir.join(format!("{case}-ball.smb")), false).unwrap();
    }
    match boolean(&roll, &ball, BoolOp::Cut) {
        Ok(Some(_)) => None,
        Ok(None) => Some("the cut left nothing".into()),
        Err(e) => Some(e.to_string()),
    }
}

/// The control: the poles inside the metal, the cut's loops clear of them.
#[test]
fn a_ball_with_its_poles_inside_a_rolls_metal_is_cut() {
    let failed: Vec<(String, String)> = Frame::all().into_iter().filter_map(|f| cut(f, 49.0).map(|e| (f.name(), e))).collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The engine's case: the poles on the roll's outside.
#[test]
#[ignore = "solidcore: the trace leaves coincident control points at a sphere pole on a cylinder; #6748 follow-up"]
fn a_ball_with_its_poles_on_a_rolls_face_is_cut() {
    let failed: Vec<(String, String)> = Frame::all().into_iter().filter_map(|f| cut(f, 50.0).map(|e| (f.name(), e))).collect();
    assert!(failed.is_empty(), "{failed:#?}");
}
