//! #3045: a round on the edge where a plate's straight side meets its arc end. The round is the
//! cylinder of radius R tangent to the side and inside the arc's cylinder; it removes 8 times
//! (the thickness) the area between the corner and that circle.
//!
//! The failing configuration needs three of the fixes together: the arc face a cylinder
//! (`linear_sweep_arc_is_cylinder`) and its offsets continued as themselves
//! (`offset_extension_from_base`, `offset_extension_as_itself`). The continued cylinder is a
//! whole circle, so the drop of the rail's end onto it found the circle's far side, where the
//! solve has a second root outside the face. Measured on this plate (arc 16.25, bisected to
//! 0.01): without the fix the round failed with status 1001 for R from 6.26 to 11.8 and built
//! below and above; the radii below bracket both ends.

#![allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

mod common;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use common::{edge_at, kernel_copy};
use solidcore::surface::surface::SurfaceKind;
use solidcore::Point2;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{make_box, session, Body, CheckLevel};

/// The plate, scaled by `s`: 100 x 30 with the +x end an arc of radius 16.25 about (93.75, 15),
/// 8 thick.
struct Plate {
    s: f64,
}

impl Plate {
    fn cx(&self) -> f64 {
        93.75 * self.s
    }
    fn cy(&self) -> f64 {
        15.0 * self.s
    }
    fn rc(&self) -> f64 {
        16.25 * self.s
    }
    fn thickness(&self) -> f64 {
        8.0 * self.s
    }

    fn body(&self) -> Body {
        let s = self.s;
        let half = (self.cy() / self.rc()).asin();
        let line = |start: [f64; 3], end: [f64; 3]| Curve::Line { start, end };
        let profile = vec![
            line([0.0, 0.0, 0.0], [100.0 * s, 0.0, 0.0]),
            Curve::Arc {
                center: [self.cx(), self.cy(), 0.0],
                axis: [0.0, 0.0, 1.0],
                reference: [1.0, 0.0, 0.0],
                radius: self.rc(),
                start: -half,
                end: half,
            },
            line([100.0 * s, 30.0 * s, 0.0], [0.0, 30.0 * s, 0.0]),
            line([0.0, 30.0 * s, 0.0], [0.0, 0.0, 0.0]),
        ];
        extrude(
            &profile,
            [0.0, 0.0, 1.0],
            self.thickness(),
            0.0,
            DraftCorner::default(),
            true,
        )
        .unwrap()
    }

    /// The plate's volume: the 100 x 30 rectangle and the arc's segment past x = 100.
    fn volume(&self) -> f64 {
        let turn = 2.0 * (self.cy() / self.rc()).asin();
        let (rc, s) = (self.rc(), self.s);
        self.thickness() * (3000.0 * s * s + rc * rc / 2.0 * (turn - turn.sin()))
    }

    /// The round's centre abscissa, `cx + dx`: the circle of radius `r` tangent to the side
    /// (y = 0) and inside the arc's circle, `|centre - (cx, cy)| = rc - r`.
    fn round_x(&self, r: f64) -> f64 {
        self.cx() + ((self.rc() - r).powi(2) - (self.cy() - r).powi(2)).sqrt()
    }

    /// The area the round of radius `r` removes: between the corner (100, 0), the side and the
    /// arc, and the round's circle. Green's theorem round the loop corner, foot on the side,
    /// contact on the arc: each circular arc contributes `1/2 (r^2 (b - a) + cx r (sin b -
    /// sin a) + cy r (cos a - cos b))`, the side nothing.
    fn removed_area(&self, r: f64) -> f64 {
        let fx = self.round_x(r);
        let contact = (r - self.cy()).atan2(fx - self.cx());
        let corner = (-self.cy()).atan2(100.0 * self.s - self.cx());
        let arc = |(cx, cy): (f64, f64), rad: f64, a: f64, b: f64| {
            0.5 * (rad * rad * (b - a)
                + cx * rad * (b.sin() - a.sin())
                + cy * rad * (a.cos() - b.cos()))
        };
        (arc((fx, r), r, -FRAC_PI_2, contact)
            + arc((self.cx(), self.cy()), self.rc(), contact, corner))
        .abs()
    }
}

/// Resets the process-wide fixes when the test ends, a failed assertion included.
struct ResetFixes;

impl Drop for ResetFixes {
    fn drop(&mut self) {
        session::set_fixes([]).unwrap();
    }
}

/// `b` is valid, has volume `expected` and a round face that is the cylinder of radius `r`
/// about the axis through (`axis_x`, `r`): an exact surface, not a B-spline of that shape.
fn check_round(b: &Body, r: f64, axis_x: f64, expected: f64, what: &str) {
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{what}: {report}");
    let volume = b.volume(1e-9).unwrap();
    assert!(
        (volume - expected).abs() < 1e-6,
        "{what}: volume {volume}, closed form {expected}"
    );
    let (k, brep) = kernel_copy(b);
    let rounds: Vec<_> = brep
        .face_list()
        .iter()
        .filter_map(|&f| {
            let surface = brep.face(f).unwrap().surface.as_ref()?;
            match surface.kind() {
                SurfaceKind::Cone(c) if c.is_cylinder() && (c.bot_radius() - r).abs() < 1e-9 => {
                    let uv = surface.natural_uv_domain_with_nurb(&k).unwrap();
                    let mid = Point2::new(
                        0.5 * (uv.min().x + uv.max().x),
                        0.5 * (uv.min().y + uv.max().y),
                    );
                    surface.evaluate_point(&k, mid).ok()
                }
                _ => None,
            }
        })
        .collect();
    assert_eq!(rounds.len(), 1, "{what}: one cylinder of radius {r}");
    let p = rounds.first().copied().unwrap();
    assert!(
        ((p.x - axis_x).hypot(p.y - r) - r).abs() < 1e-9,
        "{what}: the round's cylinder is about ({axis_x}, {r})"
    );
}

fn check_plate_round(s: f64, r: f64, flags: &str) {
    let plate = Plate { s };
    let b = plate.body();
    let e = edge_at(&b, [100.0 * s, 0.0, plate.thickness() / 2.0]);
    let what = format!("{flags}, scale {s}, R {r}");
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
        .unwrap_or_else(|err| panic!("{what}: {err:?}"));
    let expected = plate.volume() - plate.thickness() * plate.removed_area(r);
    check_round(&b, r, plate.round_x(r), expected, &what);
}

/// The control the issue names: the same plate with a straight end, a round of R 10 on the
/// corner edge. It removes `(1 - pi/4) r^2` of area, 8 thick.
fn check_straight_end(flags: &str) {
    let b = make_box([0.0; 3], [100.0, 30.0, 8.0]).unwrap();
    let e = edge_at(&b, [100.0, 0.0, 4.0]);
    let r = 10.0;
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
        .unwrap_or_else(|err| panic!("{flags}, straight end: {err:?}"));
    let expected = 8.0 * (3000.0 - (1.0 - FRAC_PI_4) * r * r);
    check_round(
        &b,
        r,
        100.0 - r,
        expected,
        &format!("{flags}, straight end"),
    );
}

const SCALE_1: [f64; 10] = [3.0, 6.0, 6.25, 6.3, 6.5, 7.0, 10.0, 11.8, 11.9, 12.0];
const SCALE_2: [f64; 4] = [6.0, 12.5, 13.0, 20.0];

/// Without the guard the three fixes fail with status 1001 for R from 6.26 to 11.8 on the scale-1
/// plate (and the same fractions of the arc's radius on the scale-2 plate); with none of them
/// it always built. The radii bracket both ends of the band (6.25 and 6.3, 11.8 and 11.9) and
/// sit inside it (6.5, 7, 10). One test: the fixes are one process-wide setting.
#[test]
fn the_round_builds_for_every_radius() {
    let _reset = ResetFixes;
    for (flags, names) in [
        ("default flags", &[][..]),
        (
            "the three fixes",
            &[
                "linear_sweep_arc_is_cylinder",
                "offset_extension_from_base",
                "offset_extension_as_itself",
            ][..],
        ),
    ] {
        session::set_fixes(names.iter().copied()).unwrap();
        check_straight_end(flags);
        for r in SCALE_1 {
            check_plate_round(1.0, r, flags);
        }
        for r in SCALE_2 {
            check_plate_round(2.0, r, flags);
        }
    }
}
