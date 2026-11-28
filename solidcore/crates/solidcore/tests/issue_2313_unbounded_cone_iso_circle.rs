//! WR-713 (#2313): `SmCone::CreateIsoCircleFromPoint` on unbounded cones. Upstream's
//! unbounded cone is parameterised unlike its NURB in two ways:
//! - inside out, `ConvertUVFromSTEPToNURBS` negates the height, but the NURB runs over the
//!   STEP heights reversed, so the circle through a point at height 1 is near
//!   `SM_INFINITE_PARAMETER` (1234562.33 for the issue's cone);
//! - of negative semi-angle, the constructor leaves the generator parallel to the axis and
//!   runs the cone from its apex (above the origin) upwards, so no circle comes back for a
//!   point below the apex.
//!
//! The port (the flags `cone_unbounded_parameterized_as_nurb` and
//! `cone_iso_circle_at_axial_height` until their retirement) parameterises unbounded cones as
//! their NURB: the circle through a point on the cone lies on the cone at the point's axial
//! height, to 1e-9 relative, bounded or unbounded.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::det;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::cone::Cone;
use solidcore::Kernel;

const TOL: f64 = 1e-3;
const REL: f64 = 1e-9;

fn kernel() -> Kernel {
    Kernel::default()
}

/// The issue's placement (origin, Z axis) and a tilted, offset one.
fn placements() -> [Axis2Placement; 2] {
    let x = Vec3::new(1.0, 2.0, -0.5).unitize_or_z();
    let y = Vec3::new(-2.0, 1.0, 0.0);
    let y = (y - y.dot(x) * x).unitize_or_z();
    [
        Axis2Placement::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ),
        Axis2Placement::new(Point3::new(3.0, -2.0, 7.5), x, y),
    ]
}

/// The point on `cone` at axial height `h`, in the direction (0.6, 0.8) of its placement.
fn point_at(pl: &Axis2Placement, cone: &Cone, h: f64) -> Point3 {
    let r = cone.radius(h);
    pl.origin() + (0.6 * r) * pl.x_axis() + (0.8 * r) * pl.y_axis() + h * pl.z_axis()
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= REL * a.abs().max(b.abs()).max(scale)
}

/// Asserts that `circle` lies on `cone` at the axial height `h`: at eight points around it,
/// the axial height is `h` and the distance from the axis is the cone's radius there, to
/// `REL` relative to the size of the point `p` the circle was asked through.
fn assert_on_cone_at(
    k: &Kernel,
    pl: &Axis2Placement,
    cone: &Cone,
    circle: &Curve,
    h: f64,
    p: Point3,
    what: &str,
) {
    let scale = (p - pl.origin()).length().max(1.0);
    let ivl = circle.natural_interval();
    for i in 0..8 {
        let t = ivl.min() + f64::from(i) / 8.0 * (ivl.max() - ivl.min());
        let q = circle.evaluate_point(k, t).unwrap();
        let d = q - pl.origin();
        let qh = d.dot(pl.z_axis());
        let qr = (d - qh * pl.z_axis()).length();
        assert!(
            close(qh, h, scale),
            "{what}: circle at height {qh}, point at {h}"
        );
        let r = cone.radius(h).abs();
        assert!(
            close(qr, r, scale),
            "{what}: circle radius {qr}, cone radius {r} at {h}"
        );
    }
}

/// Inside out or not, swapped or not, on both placements.
fn each_cone(
    height: Option<f64>,
    bot: f64,
    semi: f64,
    mut f: impl FnMut(&Kernel, &Axis2Placement, &Cone, String),
) {
    let k = kernel();
    for (n, pl) in placements().iter().enumerate() {
        for inside_out in [false, true] {
            for swap in [false, true] {
                let cone = Cone::create_canonical_with_semi_angle(
                    &k, pl, bot, semi, height, swap, inside_out,
                )
                .unwrap();
                let what = format!(
                    "bot {bot} semi {semi} height {height:?} placement {n} inside_out {inside_out} swap {swap}"
                );
                f(&k, pl, &cone, what);
            }
        }
    }
}

/// Heights on an unbounded cone of base radius `bot` and semi-angle `semi`: next to the apex
/// (1e-3 inside it), at the base, the issue's height 1 (or -1 past a negative cone's apex
/// side), and far from the apex (1e4).
fn unbounded_heights(bot: f64, semi: f64) -> Vec<f64> {
    let apex = -bot / det::tan(semi.to_radians());
    let inward = if semi > 0.0 { 1.0 } else { -1.0 };
    vec![
        apex + inward * 1e-3,
        apex + inward * 0.5,
        0.0,
        1.0,
        -1.0,
        inward * 1e4,
    ]
    .into_iter()
    .filter(|h| (h - apex) * inward > 0.0)
    .collect()
}

#[test]
fn unbounded_cone_iso_circles_at_the_points_height() {
    for (bot, semi) in [
        (1.0, 10.0),
        (2.0, -15.0),
        (0.5, 40.0),
        (3.0, -40.0),
        (1.0, 80.0),
    ] {
        each_cone(None, bot, semi, |k, pl, cone, what| {
            for h in unbounded_heights(bot, semi) {
                let p = point_at(pl, cone, h);
                let c = cone.create_iso_circle_from_point(k, p, TOL).unwrap();
                let c = c.unwrap_or_else(|| panic!("{what}: no circle at height {h}"));
                assert_on_cone_at(k, pl, cone, &c, h, p, &format!("{what} h {h}"));
            }
        });
    }
}

#[test]
fn bounded_cone_iso_circles_at_the_points_height() {
    for (bot, semi, height) in [
        (1.0, 10.0, 2.0),
        (2.0, -15.0, 3.0),
        (1.0, 45.0, 1.0),
        (2.0, -20.0, 5.0),
    ] {
        each_cone(Some(height), bot, semi, |k, pl, cone, what| {
            for h in [0.0, 1e-3, 0.5 * height, height - 1e-3, height] {
                let p = point_at(pl, cone, h);
                let c = cone.create_iso_circle_from_point(k, p, TOL).unwrap();
                let c = c.unwrap_or_else(|| panic!("{what}: no circle at height {h}"));
                assert_on_cone_at(k, pl, cone, &c, h, p, &format!("{what} h {h}"));
            }
        });
    }
}

/// The issue's trigger: the point `(0.6 r, 0.8 r, 1)`, `r` the cone's radius at height 1.
/// Upstream's circle on the inside-out cone (base radius 1, semi-angle 10) is at height
/// 1234562.33 (the pinned C++, `surface.cone_p1` fixtures `issue_2313.json`), and on the
/// cone of semi-angle -15 (base radius 2) there is none. The circle through the point is at
/// height 1 with radius `1 + tan 10° = 1.17633` and `2 - tan 15° = sqrt 3`.
#[test]
fn issue_trigger_circles_are_through_the_point() {
    let pl = &placements()[0];
    let k = kernel();
    let check = |cone: &Cone, r: f64, what: &str| {
        let c = cone
            .create_iso_circle_from_point(&k, point_at(pl, cone, 1.0), TOL)
            .unwrap()
            .unwrap_or_else(|| panic!("{what}: no circle"));
        let ivl = c.natural_interval();
        for i in 0..4 {
            let t = ivl.min() + f64::from(i) / 4.0 * (ivl.max() - ivl.min());
            let q = c.evaluate_point(&k, t).unwrap();
            assert!(close(q.z, 1.0, 1.0), "{what}: circle at height {}", q.z);
            let qr = (q.x * q.x + q.y * q.y).sqrt();
            assert!(close(qr, r, 1.0), "{what}: circle radius {qr}, want {r}");
        }
    };
    let cone =
        Cone::create_canonical_with_semi_angle(&k, pl, 1.0, 10.0, None, false, true).unwrap();
    check(
        &cone,
        1.0 + det::tan(10f64.to_radians()),
        "inside out, semi 10",
    );
    for inside_out in [false, true] {
        let cone =
            Cone::create_canonical_with_semi_angle(&k, pl, 2.0, -15.0, None, false, inside_out)
                .unwrap();
        check(
            &cone,
            2.0 - det::tan(15f64.to_radians()),
            &format!("semi -15, inside out {inside_out}"),
        );
    }
}

/// The negative-angle unbounded cone runs from -infinity to its apex above
/// the origin, and its NURB-parameter points agree with its NURB's iso curves inside out.
#[test]
fn unbounded_cone_domain_and_nurb_points_agree() {
    let k = kernel();
    let pl = &placements()[0];
    let cone =
        Cone::create_canonical_with_semi_angle(&k, pl, 2.0, -15.0, None, false, false).unwrap();
    let apex = 2.0 / det::tan(15f64.to_radians());
    assert!(
        close(cone.top_height(), apex, 1.0),
        "top {}",
        cone.top_height()
    );
    assert!(cone.bot_height() < -1e6, "bottom {}", cone.bot_height());
    for (bot, semi) in [(1.0, 10.0), (2.0, -15.0)] {
        let cone =
            Cone::create_canonical_with_semi_angle(&k, pl, bot, semi, None, false, true).unwrap();
        let step = cone
            .convert_uv_from_nurbs_to_step(
                &k,
                cone.convert_uv_from_step_to_nurbs(&k, solidcore::math::Point2::new(90.0, 1.0))
                    .unwrap(),
            )
            .unwrap();
        assert!(
            close(step.y, 1.0, 1.0) && close(step.x, 90.0, 1.0),
            "round trip {step:?}"
        );
        let nurb_uv = cone
            .convert_uv_from_step_to_nurbs(&k, solidcore::math::Point2::new(0.0, 1.0))
            .unwrap();
        let p = cone.evaluate_point(&k, nurb_uv).unwrap();
        assert!(close(p.z, 1.0, 1.0), "semi {semi}: NURB point at {p:?}");
    }
}

/// `Cone::create_canonical` with `height <= 0` makes the infinite cone of `top_radius` as the
/// semi-angle; for `bot 1, top -20, height -1` (#18194) that is the cone of radius
/// `1 + tan(-20 deg) h`. Before WR-713's fix its generator stayed parallel to the axis, so
/// the net was the radius-1 cylinder while the definition said the cone.
///
/// Shown by construction, nothing sampled: the canonical data are the inputs exactly; the
/// domain runs from `-SM_INFINITE_PARAMETER` to the apex `1 / tan(20 deg)`; the NURB form
/// lies on the cone's implicit equation ([`lies_on`]: every Bernstein coefficient of every
/// Bézier patch, to the rounding of computing it) and not on the radius-1 cylinder; and with
/// positive weights every control point is at or below the apex, so the rational net (inside
/// the convex hull of its control points) is on the cone's lower nappe, the one its domain
/// names.
#[test]
fn create_canonical_negative_semi_angle_net_is_its_cone() {
    use solidcore::math::INFINITE_PARAMETER;
    use solidcore::nurbs::math::NOW;
    use solidcore::surface::implicit::{lies_on, Implicit};

    let k = kernel();
    let t = det::tan((-20f64).to_radians());
    let apex = -1.0 / t;
    for (n, pl) in placements().iter().enumerate() {
        for inside_out in [false, true] {
            for swap in [false, true] {
                let what = format!("placement {n}, inside out {inside_out}, swap {swap}");
                let cone = Cone::create_canonical(&k, pl, 1.0, -20.0, -1.0, swap, inside_out)
                    .unwrap_or_else(|e| panic!("{what}: {e:?}"));
                assert_eq!(cone.base_radius(), 1.0, "{what}");
                assert_eq!(cone.semi_angle_deg(), -20.0, "{what}");
                assert_eq!(cone.bot_height(), -INFINITE_PARAMETER, "{what}");
                assert!(
                    close(cone.top_height(), apex, 1.0),
                    "{what}: top {}",
                    cone.top_height()
                );

                let on = |tan_angle: f64| Implicit::Cone {
                    origin: pl.origin(),
                    axis: pl.z_axis(),
                    radius: 1.0,
                    tan_angle,
                };
                let form = cone.rev().base();
                assert!(lies_on(form, &on(t)).unwrap(), "{what}: net off its cone");
                assert!(
                    !lies_on(form, &on(0.0)).unwrap(),
                    "{what}: net on the radius-1 cylinder"
                );

                let nurb = form.gw_nurb().expect("the cone's NURB");
                let pts: Vec<(Vec3, f64)> = nurb
                    .rows()
                    .flatten()
                    .map(|c| {
                        let w = if c.w == NOW { 1.0 } else { c.w };
                        (Vec3::new(c.x / w, c.y / w, c.z / w), w)
                    })
                    .collect();
                let size = pts
                    .iter()
                    .map(|(p, _)| p.x.abs().max(p.y.abs()).max(p.z.abs()))
                    .fold(0.0, f64::max);
                let round = 64.0 * f64::EPSILON * size;
                for (p, w) in &pts {
                    assert!(*w > 0.0, "{what}: weight {w}");
                    let h = (*p - pl.origin()).dot(pl.z_axis());
                    assert!(
                        h <= apex + round,
                        "{what}: control point at {h}, apex {apex}"
                    );
                }
            }
        }
    }
}
