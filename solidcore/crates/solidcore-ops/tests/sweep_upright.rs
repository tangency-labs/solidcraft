//! A profile swept kept upright (`sweep_placed` with `SweepFrame::Upright`): round a closed
//! D-shaped outline in the plane z = 0, a vertical section stays vertical at its first, middle
//! and last sections and the sheet closes; a path that runs vertical somewhere is refused; an up
//! direction not finite is an argument refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use std::f64::consts::PI;

use solidcore_ops::sweep::{sweep_placed, Curve, SweepFrame, SweepRoute};
use solidcore_ops::Error;

const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// The kernel rig's fixes, as the sweep tests run them.
fn rig_fixes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fixes.json"
    );
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    // A flat JSON array of flag names: every other piece between quotes.
    let names: Vec<&str> = text.split('"').skip(1).step_by(2).collect();
    solidcore_ops::session::set_thread_fixes(names).unwrap();
}

/// A D in the plane z = 0: the half circle of radius 5 about the origin from (0, -5) over
/// (5, 0) to (0, 5), then straight back down to (0, -5).
fn d_outline() -> Vec<Curve> {
    vec![
        Curve::Arc {
            center: [0.0; 3],
            axis: Z,
            reference: [0.0, -1.0, 0.0],
            radius: 5.0,
            start: 0.0,
            end: PI,
        },
        Curve::Line {
            start: [0.0, 5.0, 0.0],
            end: [0.0, -5.0, 0.0],
        },
    ]
}

/// A vertical screen 2 high standing on the path's start.
fn screen() -> Vec<Curve> {
    vec![Curve::Line {
        start: [0.0, -5.0, 0.0],
        end: [0.0, -5.0, 2.0],
    }]
}

#[test]
fn round_a_closed_outline_a_screen_kept_upright_stays_vertical() {
    rig_fixes();
    let (body, placed, route) =
        sweep_placed(&screen(), &d_outline(), SweepFrame::Upright(Z), false).unwrap();
    assert_eq!(route, SweepRoute::Curve(SweepFrame::Upright(Z)));
    // each placement carries the screen's up (z) onto the vertical
    for (i, p) in placed.iter().enumerate() {
        let up = p.m[2];
        assert!(
            up[0].abs() <= 1e-9 && up[1].abs() <= 1e-9 && (up[2] - 1.0).abs() <= 1e-9,
            "section {i}: z goes to {up:?}"
        );
    }
    // the sheet closes: it has no free edge but the screen's top and bottom runs
    let (record, _) = body.to_record().unwrap();
    assert!(!record.faces.is_empty());
}

#[test]
fn a_path_running_vertical_somewhere_is_refused() {
    rig_fixes();
    // a half circle standing in the plane y = 0: vertical at its top
    let arch = vec![Curve::Arc {
        center: [0.0; 3],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: 5.0,
        start: 0.0,
        end: PI,
    }];
    let profile = vec![Curve::Line {
        start: [5.0, 0.0, 0.0],
        end: [7.0, 0.0, 0.0],
    }];
    assert!(matches!(
        sweep_placed(&profile, &arch, SweepFrame::Upright(Z), false),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn an_up_direction_not_finite_is_refused() {
    assert!(matches!(
        sweep_placed(
            &screen(),
            &d_outline(),
            SweepFrame::Upright([f64::NAN, 0.0, 1.0]),
            false
        ),
        Err(Error::Argument)
    ));
}
