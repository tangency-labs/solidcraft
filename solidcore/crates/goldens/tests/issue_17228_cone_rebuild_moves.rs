//! #17228: the one `surface.cone_p2` row the `EFF_ZERO` branch's certificate moves
//! (`rebuild` status `1000 -> 1001`) is a net twisted about its own axis, off every
//! cylinder the rebuild can record by far more than rounding, so recording a cylinder
//! for it (main's `1000`) was wrong and the refusal is correct.
//!
//! The row's op is `rebuild` after a `move_last` edit: the last row of control points
//! along the generator (linear) direction is turned by `a` = 1e-4 degrees about the world
//! `z` axis. Both end radii stay equal, so the rebuild takes the `EFF_ZERO` branch, which on
//! main recorded a cylinder of the bottom radius without looking at the net.
//!
//! # The gap (PROVEN, closed form)
//!
//! The cylinder has radius `r` = 1 about `z` through the origin (its placement is the
//! identity) and the turn is about that same axis with no translation (all checked), so
//! both cylinders the rebuild tries, about the bottom circle's normal and about the centre
//! line through both circles' centres, are this one. The net is degree 1 with two rows in
//! the generator direction and its arc knots are clamped (checked), so at an end of the arc
//! the surface's generator is the straight segment between the end control points `B`
//! (bottom, unmoved) and `T` (top, turned by `a`), both at distance `r` from the axis. Its
//! midpoint `(B + T) / 2` is on the surface at distance `r cos(a / 2)` from the axis, off
//! the cylinder by `r (1 - cos(a / 2)) = 2 r sin^2(a / 4)` = 3.8e-13.
//!
//! The test computes that midpoint's distance from the axis in floating point (each
//! coordinate within a few ulps of `|B|, |T| <= sqrt(r^2 + h^2)`, `h` = 2) and checks the
//! measured gap agrees with the closed form within `gamma(8)` of that size, and exceeds
//! a hundred times that rounding: the net is off the cylinder by far more than the
//! rounding of its stored points. That margin is against `gamma(8)` of the size, not
//! against `implicit::lies_on`'s own allowance (`gamma(64)` of the patch size squared),
//! which the gap exceeds by roughly ten times, so it refuses (ARGUED). Rigid moves whose turn is a
//! rounding-level twist (`gen0291`, `gen1190`) are recorded on the centre-line cylinder
//! instead and keep their status.
//!
//! The row is re-run through the goldens runner first; it must give the blessed golden
//! line (status `1001`), and the edit is re-applied here with the runner's formulas to get
//! the net the rebuild saw.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use goldens::case::repo_root;
use goldens::{golden, registry, CaseContext};
use serde_json::Value;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::det;
use solidcore::math::PI;
use solidcore::nurbs::math::{gamma, CPoint, NOW};
use solidcore::surface::cone::Cone;
use solidcore::surface::implicit;
use std::fmt::Write as _;

/// `(case, main's status, the new status)`.
const ROWS: &[(&str, f64, f64)] = &[("adv_reb_cylarc_s1i0_id_tw", 1000.0, 1001.0)];

const UNIT: &str = "surface.cone_p2";

/// The runner's rotation (`surface_cone_p2.rs` `rotation`), row-major.
fn rotation(axis: [f64; 3], angle: f64) -> [f64; 9] {
    let [ax, ay, az] = axis;
    let l = (ax * ax + ay * ay + az * az).sqrt();
    if l <= 0.0 {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let (x, y, z) = (ax / l, ay / l, az / l);
    let a = angle * PI / 180.0;
    let (s, c) = det::sin_cos(a);
    let t = 1.0 - c;
    [
        t * x * x + c,
        t * x * y - s * z,
        t * x * z + s * y,
        t * x * y + s * z,
        t * y * y + c,
        t * y * z - s * x,
        t * x * z - s * y,
        t * y * z + s * x,
        t * z * z + c,
    ]
}

fn arr3(v: &Value) -> [f64; 3] {
    let a = v.as_array().expect("3-vector");
    [
        a[0].as_f64().unwrap(),
        a[1].as_f64().unwrap(),
        a[2].as_f64().unwrap(),
    ]
}

/// The runner's `edit_nurb` for `MoveLast`: the last row of control points along the
/// generator direction turned and moved, in homogeneous form.
fn move_last(cone: &mut Cone, edit: &Value) {
    let (axis, angle, t) = (
        arr3(&edit["axis"]),
        edit["angle"].as_f64().unwrap(),
        arr3(&edit["t"]),
    );
    let r = rotation(axis, angle);
    let swap = cone.rev().swap_uv();
    let sur = cone.base_mut().get_or_create_gw_nurb().unwrap();
    let (n, m) = (sur.n(), sur.m());
    for i in 0..=n {
        for j in 0..=m {
            let last = if swap { i == n } else { j == m };
            if !last {
                continue;
            }
            let p = sur.cpt_mut(i, j).unwrap();
            let w = if p.w == NOW { 1.0 } else { p.w };
            let (x, y, z) = (p.x, p.y, p.z);
            p.x = r[0] * x + r[1] * y + r[2] * z + w * t[0];
            p.y = r[3] * x + r[4] * y + r[5] * z + w * t[1];
            p.z = r[6] * x + r[7] * y + r[8] * z + w * t[2];
        }
    }
}

fn clamped(knots: &[f64], p: usize) -> bool {
    let n = knots.len();
    n >= 2 * (p + 1)
        && knots[..=p].iter().all(|&k| k == knots[0])
        && knots[n - 1 - p..].iter().all(|&k| k == knots[n - 1])
}

#[test]
fn issue_17228_moved_row_is_off_every_cylinder_the_rebuild_tries() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let unit = registry()
        .into_iter()
        .find(|u| u.id() == UNIT)
        .expect("surface.cone_p2 registered");
    let gold = golden::read(&golden::default_dir(), UNIT).unwrap();
    let cases = unit.cases(&ctx).unwrap();
    let mut report = String::new();
    for &(name, old, new) in ROWS {
        let case = cases.iter().find(|c| c.name == name).expect("case exists");
        let line = &gold[name];
        let status = |v: &Value| v["values"]["status"][0].as_f64().unwrap();
        assert_eq!(status(line), new, "{name}: blessed status");
        assert_ne!(old, new);
        let out = unit.run_rust(case).unwrap();
        assert_eq!(out.values["status"], vec![new], "{name}: re-run status");

        let op = &case.inputs["op"];
        assert_eq!(op["kind"], "rebuild", "{name}");
        let edit = &op["edit"];
        assert_eq!(edit["kind"], "move_last", "{name}");
        let (axis, angle, t) = (
            arr3(&edit["axis"]),
            edit["angle"].as_f64().unwrap(),
            arr3(&edit["t"]),
        );
        // a pure turn about the cylinder's own axis: z through the origin
        assert_eq!(
            (axis, t),
            ([0.0, 0.0, 1.0], [0.0; 3]),
            "{name}: turn about z"
        );

        let k = Kernel::default();
        let mut cone = goldens::units::surface_cone_p2::case_cone(&k, &case.inputs)
            .unwrap()
            .unwrap();
        let pos = *cone.rev().position();
        assert_eq!(
            (pos.origin(), pos.z_axis()),
            (Vec3::ZERO, Vec3::Z),
            "{name}: cylinder about z"
        );
        let mode = &case.inputs["cone"]["mode"];
        let (radius, height) = (
            mode["radius"].as_f64().unwrap(),
            mode["height"].as_f64().unwrap(),
        );
        move_last(&mut cone, edit);
        let swap = cone.rev().swap_uv();
        let sur = cone.base_mut().get_or_create_gw_nurb().unwrap();
        let (n, m) = (sur.n(), sur.m());
        // generator direction: u with swap, else v
        let (gen_deg, gen_last, arc_deg, arc_knots) = if swap {
            (sur.p(), n, sur.q(), sur.v_knots().to_vec())
        } else {
            (sur.q(), m, sur.p(), sur.u_knots().to_vec())
        };
        assert_eq!(
            (gen_deg, gen_last),
            (1, 1),
            "{name}: one linear span of generators"
        );
        assert!(clamped(&arc_knots, arc_deg), "{name}: clamped arc knots");
        let euclid = |g: usize| -> Vec3 {
            // the arc's first control point, interpolated by the clamped knots
            let (i, j) = if swap { (g, 0) } else { (0, g) };
            let c = sur.cpt(i, j).unwrap();
            let w = if c.w == NOW { 1.0 } else { c.w };
            assert!(w > 0.0, "{name}: positive end weight");
            Vec3::new(c.x / w, c.y / w, c.z / w)
        };
        let (b, top) = (euclid(0), euclid(1));
        let mid = (b + top) * 0.5;
        let gap = radius - (mid.x * mid.x + mid.y * mid.y).sqrt();
        let a = angle.abs() * PI / 180.0;
        let closed = 2.0 * radius * det::powi(det::sin(a / 4.0), 2);
        let size = (radius * radius + height * height).sqrt();
        let rounding = gamma(8) * size;
        assert!(
            (gap - closed).abs() <= rounding,
            "{name}: gap {gap:e} vs closed form {closed:e} (rounding {rounding:e})"
        );
        assert!(
            closed > 100.0 * rounding,
            "{name}: gap {closed:e} not far beyond rounding {rounding:e}"
        );
        writeln!(
            report,
            "{name}: turn {angle:e} deg about z; mid-generator gap measured {gap:e}, \
             closed form 2 r sin^2(a/4) {closed:e}, rounding {rounding:e} ({:.0}x)",
            closed / rounding
        )
        .unwrap();
    }
    println!("{report}");
}

/// `(case, the status before, the new status)` of rows the rebuild now records on the centre line.
const ACCEPTED: &[(&str, f64, f64)] = &[("gen0152", 1001.0, 1000.0)];

/// #17228, the centre-line axis: `gen0152` is a canonical cylinder of radius 2.8 whose
/// net reaches 1e6 from the origin, its top row moved by about 0.49 (and turned by
/// 1.4e-8 degrees about the world `z` axis). The top circle is then off the cylinder about
/// the bottom circle's normal by far more than rounding, and the rebuild refused it since
/// #16982; but the two circles are still coaxial about the centre line through their
/// centres, each tilted `t` from it, so the net is on that cylinder within `r t^2 / 2`
/// plus the twist's `2 r sin^2(a / 4)` (closed form, evaluated with a MEASURED tilt: `t`
/// from the cross product of the recorded axes before and after the edit). That gap,
/// not the acceptance threshold (which is `implicit::lies_on`'s allowance), is below one
/// ulp of the net's coordinates, so the rebuild records that cylinder (status `1000`),
/// the net moved by no more than a few ulps
/// (MEASURED: the largest control point move, gated at 4 ulps of the largest
/// coordinate).
#[test]
fn issue_17228_rigidly_moved_rows_rebuild_on_the_centre_line() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let unit = registry()
        .into_iter()
        .find(|u| u.id() == UNIT)
        .expect("surface.cone_p2 registered");
    let gold = golden::read(&golden::default_dir(), UNIT).unwrap();
    let cases = unit.cases(&ctx).unwrap();
    let mut report = String::new();
    for &(name, old, new) in ACCEPTED {
        let case = cases.iter().find(|c| c.name == name).expect("case exists");
        let status = |v: &Value| v["values"]["status"][0].as_f64().unwrap();
        assert_eq!(status(&gold[name]), new, "{name}: blessed status");
        assert_ne!(old, new);
        let out = unit.run_rust(case).unwrap();
        assert_eq!(out.values["status"], vec![new], "{name}: re-run status");

        let op = &case.inputs["op"];
        assert_eq!(op["kind"], "rebuild", "{name}");
        let edit = &op["edit"];
        assert_eq!(edit["kind"], "move_last", "{name}");
        let angle = edit["angle"].as_f64().unwrap();
        let k = Kernel::default();
        let mut cone = goldens::units::surface_cone_p2::case_cone(&k, &case.inputs)
            .unwrap()
            .unwrap();
        let axis_before = cone.rev().position().z_axis();
        move_last(&mut cone, edit);
        let edited = cone.clone();
        cone.rebuild_step_from_nurb_parameters(&k).unwrap();
        assert!(
            cone.is_cylinder_class() && cone.semi_angle_deg() == 0.0,
            "{name}"
        );
        let radius = cone.base_radius();
        let axis_after = cone.rev().position().z_axis();
        // the edited net is off the cylinder about the end circle's normal (the #17228
        // certificate's `NetOffCylinder` had the rebuild kept that axis) and the rebuilt
        // net is certified about the recorded centre line
        let about = |axis: Vec3| implicit::Implicit::Cone {
            origin: cone.rev().position().origin(),
            axis,
            radius,
            tan_angle: 0.0,
        };
        assert!(
            !implicit::lies_on(edited.rev().base(), &about(axis_before)).unwrap(),
            "{name}: on the cylinder about the bottom normal"
        );
        assert!(
            implicit::lies_on(cone.rev().base(), &about(axis_after)).unwrap(),
            "{name}: not certified about the recorded axis"
        );
        // the tilt of both circles from the recorded centre line, up to its sign
        let tilt = axis_before.cross(axis_after).length();
        let a = angle.abs() * PI / 180.0;
        let gap = radius * tilt * tilt / 2.0 + 2.0 * radius * det::powi(det::sin(a / 4.0), 2);

        let before = edited.rev().base().gw_nurb().unwrap();
        let after = cone.rev().base().gw_nurb().unwrap();
        let euclid = |c: CPoint| {
            let w = if c.w == NOW { 1.0 } else { c.w };
            Vec3::new(c.x / w, c.y / w, c.z / w)
        };
        let (mut moved, mut size) = (0.0_f64, 0.0_f64);
        for i in 0..=before.n() {
            for j in 0..=before.m() {
                let (p, q) = (
                    euclid(before.cpt(i, j).unwrap()),
                    euclid(after.cpt(i, j).unwrap()),
                );
                moved = moved.max((p - q).length());
                size = size.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
            }
        }
        let ulp = size * f64::EPSILON;
        assert!(
            gap < ulp,
            "{name}: closed-form gap {gap:e} not below an ulp {ulp:e}"
        );
        assert!(
            moved <= 4.0 * ulp,
            "{name}: net moved {moved:e}, ulp {ulp:e}"
        );
        writeln!(
            report,
            "{name}: tilt {tilt:e} rad, turn {angle:e} deg; closed-form gap {gap:e}; \
             net moved {moved:e} (ulp of {size:e}: {ulp:e})"
        )
        .unwrap();
    }
    println!("{report}");
}
