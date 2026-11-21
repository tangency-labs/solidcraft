//! WR-713 (#2313): the root fixes made while retiring `cone_unbounded_parameterized_as_nurb`
//! and `cone_iso_circle_at_axial_height` (#14246), each against a closed form. A cone's NURB
//! is checked with [`lies_on`], which certifies the whole rational net on the implicit
//! cone (to the rounding of computing it), so no point is sampled.
//!
//! - Bounded cones on axes that are not orthonormal: the generator is built in the
//!   canonical frame. Upstream put it at `origin + radius X` with the caller's `X`, a
//!   cylinder of radius `radius |X|`.
//! - Unbounded cones: an axis of `1e300` is a direction; a zero axis, non-finite numbers
//!   and a semi-angle of 90 degrees modulo 180 (a plane) are refused; a semi-angle outside
//!   `(-90, 90)` is taken modulo 180 (the same cone); the generator is rotated as a
//!   vector, so an origin far from 0 does not cancel the rotation to its rounding; an
//!   apex beyond `SM_INFINITE_PARAMETER` leaves that end at the infinite parameter.
//! - Scaling an unbounded cone moves its apex end with it, and an unbounded cylinder's
//!   height parameter with it (`scaled_unbounded_cylinder_keeps_its_height_parameter`
//!   fails without the transform's scale test: `v = 1.5` for 3).
//! - Offsets of an unbounded cone: the kept cone of a truncated offset runs from the
//!   cone's own heights (upstream placed it as if the bottom were at the origin;
//!   `offsets_of_an_unbounded_cone_are_its_offset_cones`), and a copied offset's polar
//!   circle is rebuilt on the offset (`copied_offset_rebuilds_its_polar_circle_on_the_
//!   offset` fails without it).
//!
//! The relative tolerances (`1e-12`, and `1e-9 (1 + |v|)` between the offset's STEP and
//! NURB points) are not derived: each is orders of magnitude above the rounding of the
//! few operations it covers and orders below the defect it detects (`v` off by a factor
//! of 2, an apex off by the bottom height, a circle off by the offset distance).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point2, Point3, Vec3, INFINITE_PARAMETER};
use solidcore::surface::cone::Cone;
use solidcore::surface::implicit::{curve_lies_on, lies_on, Implicit};
use solidcore::Kernel;

fn unit(v: Vec3) -> Vec3 {
    v * (1.0 / v.length())
}

fn on(origin: Point3, axis: Vec3, radius: f64, tan_angle: f64) -> Implicit {
    Implicit::Cone {
        origin,
        axis,
        radius,
        tan_angle,
    }
}

/// `X` of length 0.974 and `Y` of length 0.855, perpendicular (as the `n_lib_p2`
/// `analytic028` row's cylinder).
fn skew_axes() -> (Point3, Vec3, Vec3) {
    let x = Vec3::new(0.3770377089403243, 0.5669923897893385, 0.6967084152767273);
    let y = Vec3::new(0.6, -0.2, 0.0);
    let y = y - (y.dot(x) / x.dot(x)) * x;
    (Point3::new(0.75, 1.17, -0.08), x, y)
}

#[test]
fn bounded_cones_on_axes_that_are_not_orthonormal_lie_on_their_cones() {
    let k = Kernel::default();
    let (o, x, y) = skew_axes();
    let z = unit(x.cross(y));
    assert!((x.length() - 1.0).abs() > 0.02);

    let mut cyl =
        Cone::new_cylinder_bounded(o, x, y, 1.52, -253.0, 60.0, 1.29, false, false, false);
    cyl.make_nurb(&k).unwrap();
    let form = cyl.rev().base();
    assert!(
        lies_on(form, &on(o, z, 1.52, 0.0)).unwrap(),
        "cylinder off radius 1.52"
    );
    assert!(
        !lies_on(form, &on(o, z, 1.52 * x.length(), 0.0)).unwrap(),
        "cylinder on upstream's radius 1.52 |X|"
    );

    let mut cone = Cone::new_bounded(o, x, y, 1.0, 2.0, 0.0, 300.0, 1.5, false, false, false);
    cone.make_nurb(&k).unwrap();
    assert!(
        lies_on(cone.rev().base(), &on(o, z, 1.0, 1.0 / 1.5)).unwrap(),
        "cone off its radii"
    );
}

#[test]
fn unbounded_cone_with_a_huge_axis_takes_it_as_a_direction() {
    let k = Kernel::default();
    let o = Point3::new(0.0, 1.6, 1.6);
    let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1e300));
    let mut cone = Cone::new_unbounded(&k, o, x, y, 1.0, 20.0, false, false).unwrap();
    let pl = *cone.rev().position();
    assert_eq!(
        pl.z_axis(),
        Vec3::new(0.0, 1.0, 0.0),
        "axis {:?}",
        pl.z_axis()
    );
    cone.make_nurb(&k).unwrap();
    let t = 20f64.to_radians().tan();
    assert!(lies_on(cone.rev().base(), &on(o, pl.z_axis(), 1.0, t)).unwrap());
}

#[test]
fn unbounded_cone_refuses_inputs_that_define_no_cone() {
    let k = Kernel::default();
    let (o, x, y) = (
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let nan = f64::NAN;
    let cases: [(&str, Point3, Vec3, Vec3, f64, f64); 8] = [
        ("NaN origin", Point3::new(nan, 0.0, 0.0), x, y, 1.0, 20.0),
        ("infinite radius", o, x, y, f64::INFINITY, 20.0),
        ("semi-angle 90", o, x, y, 1.0, 90.0),
        ("semi-angle -90", o, x, y, 1.0, -90.0),
        ("semi-angle 270", o, x, y, 1.0, 270.0),
        ("NaN semi-angle", o, x, y, 1.0, nan),
        ("infinite semi-angle", o, x, y, 1.0, f64::INFINITY),
        ("zero Y", o, x, Vec3::new(0.0, 0.0, 0.0), 1.0, 20.0),
    ];
    for (what, o, x, y, r, semi) in cases {
        assert!(
            Cone::new_unbounded(&k, o, x, y, r, semi, false, false).is_err(),
            "{what}: built"
        );
    }
}

/// A semi-angle `s` outside (-90, 90) names the cone of `s - 180` (or `s + 180`): the
/// same `tan`, so the same radius `1 + tan(s) v` at each height, and the same generator
/// line. That cone is the nappe through the base circle (`v = 0`, radius 1): for a
/// negative representative it runs from `-SM_INFINITE_PARAMETER` up to the apex at
/// `-1 / tan(s)`. Upstream kept `s` and ran the domain from that apex up, on the nappe
/// without the base circle, where `1 + tan(s) v` is negative.
#[test]
fn unbounded_cone_takes_a_semi_angle_modulo_180() {
    let k = Kernel::default();
    let pl = Axis2Placement::IDENTITY;
    let z = Vec3::new(0.0, 0.0, 1.0);
    for (semi, rep) in [(120.0f64, -60.0f64), (-107.5, 72.5), (300.0, -60.0)] {
        let mut cone =
            Cone::create_canonical_with_semi_angle(&k, &pl, 1.0, semi, None, false, false).unwrap();
        assert_eq!(cone.semi_angle_deg(), rep, "semi {semi}");
        let t = rep.to_radians().tan();
        let apex = -1.0 / t;
        let (lo, hi) = (cone.bot_height(), cone.top_height());
        assert!(
            lo <= 0.0 && 0.0 <= hi,
            "semi {semi}: [{lo}, {hi}] misses v = 0"
        );
        let end = if rep < 0.0 { hi } else { lo };
        assert!(
            (end - apex).abs() <= 1e-12 * apex.abs(),
            "semi {semi}: apex end {end}, apex {apex}"
        );
        cone.make_nurb(&k).unwrap();
        assert!(
            lies_on(cone.rev().base(), &on(Point3::ZERO, z, 1.0, t)).unwrap(),
            "semi {semi}: off its cone"
        );
    }
}

#[test]
fn unbounded_cone_far_from_the_origin_lies_on_its_cone() {
    let k = Kernel::default();
    let o = Point3::new(1e5, -2e5, 3e5);
    let (x, y) = (Vec3::new(0.0, 0.6, 0.8), Vec3::new(1.0, 0.0, 0.0));
    let z = unit(x.cross(y));
    for semi in [30.0f64, -30.0, 1e-9] {
        let mut cone = Cone::new_unbounded(&k, o, x, y, 2.0, semi, false, false).unwrap();
        cone.make_nurb(&k).unwrap();
        let t = semi.to_radians().tan();
        assert!(
            lies_on(cone.rev().base(), &on(o, z, 2.0, t)).unwrap(),
            "semi {semi}: off its cone"
        );
    }
}

#[test]
fn apex_beyond_the_infinite_parameter_ends_there() {
    let k = Kernel::default();
    let pl = Axis2Placement::IDENTITY;
    for semi in [1e-9, -1e-9] {
        let cone =
            Cone::create_canonical_with_semi_angle(&k, &pl, 1.0, semi, None, false, false).unwrap();
        // the apex, at -1 / tan(semi), is 5.7e10 away: both ends at the infinite parameter
        assert_eq!(cone.bot_height(), -INFINITE_PARAMETER, "semi {semi}");
        assert_eq!(cone.top_height(), INFINITE_PARAMETER, "semi {semi}");
    }
}

#[test]
fn scaled_unbounded_cone_keeps_its_apex_end() {
    let k = Kernel::default();
    let pl = Axis2Placement::IDENTITY;
    let semi = 25.0f64;
    let t = semi.to_radians().tan();
    let mut cone =
        Cone::create_canonical_with_semi_angle(&k, &pl, 1.0, semi, None, false, false).unwrap();
    assert!((cone.bot_height() - (-1.0 / t)).abs() <= 1e-12);
    cone.transform(
        &k,
        &Axis2Placement::IDENTITY,
        Some(Vec3::new(2.0, 2.0, 2.0)),
    )
    .unwrap();
    let apex = cone.apex(&k).unwrap();
    assert!(
        (apex.point - Point3::new(0.0, 0.0, -2.0 / t)).length() <= 1e-12 * (2.0 / t),
        "apex {:?}",
        apex.point
    );
    let lo = cone.bot_height();
    let want = -2.0 / t;
    assert!(
        (lo - want).abs() <= 1e-12 * want.abs(),
        "apex end {lo}, apex {want}"
    );
    assert!(lies_on(
        cone.rev().base(),
        &on(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 2.0, t)
    )
    .unwrap());
}

/// An unbounded cylinder (radius 1 about `Z`) scaled by 2 about the origin is the cylinder
/// of radius 2, and its STEP height is still the height: `(2, 0, 3)` is on it at `u = 0`,
/// `v = 3`. Its height interval is `[-SM_INFINITE_PARAMETER, SM_INFINITE_PARAMETER]`
/// before and after (`Extent1d::scale` leaves infinite parameters), so upstream's test of
/// a changed interval length did not fire and the generator kept speed 1 over a line twice
/// as long: the inversion answered `v = 1.5`.
#[test]
fn scaled_unbounded_cylinder_keeps_its_height_parameter() {
    let k = Kernel::default();
    let mut cyl = Cone::new_cylinder_unbounded(
        &k,
        Point3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        1.0,
        false,
        false,
    )
    .unwrap();
    cyl.transform(
        &k,
        &Axis2Placement::IDENTITY,
        Some(Vec3::new(2.0, 2.0, 2.0)),
    )
    .unwrap();
    let inv = cyl
        .step_inversion(&k, Point3::new(2.0, 0.0, 3.0), 1e-9, None)
        .unwrap();
    assert!(inv.inside);
    assert!(
        (inv.uv.y - 3.0).abs() <= 1e-12 * 3.0 && inv.uv.x.abs() <= 1e-9,
        "STEP parameters {:?}, want (0, 3)",
        inv.uv
    );
    let p = cyl.evaluate_step_point(Point2::new(0.0, 3.0)).unwrap();
    assert!(
        (p - Point3::new(2.0, 0.0, 3.0)).length() <= 1e-12 * 3.0,
        "STEP point {p:?}"
    );
}

/// The polar converter's circle of a copied (outward) offset is an iso circle of the
/// offset cone, as `RebuildPolarConverter` makes it for any cone. Upstream's copy kept the
/// original's circle, a circle of the original cone, `d` off the offset.
#[test]
fn copied_offset_rebuilds_its_polar_circle_on_the_offset() {
    let k = Kernel::default();
    let pl = Axis2Placement::IDENTITY;
    let semi = 30.0f64;
    let (t, c) = (semi.to_radians().tan(), semi.to_radians().cos());
    let z = Vec3::new(0.0, 0.0, 1.0);
    let cone =
        Cone::create_canonical_with_semi_angle(&k, &pl, 1.0, semi, None, false, false).unwrap();
    let d = 0.5;
    let off = cone
        .create_offset_surface(&k, d, 1e-6)
        .unwrap()
        .expect("an offset");
    let mut pc = off.rev().polar_curve().expect("a polar circle").clone();
    pc.make_nurb(&k).unwrap();
    let r = 1.0 + d / c;
    assert!(
        curve_lies_on(pc.base(), &on(Point3::ZERO, z, r, t)).unwrap(),
        "the polar circle is off the offset cone of radius {r}"
    );
    assert!(
        !curve_lies_on(pc.base(), &on(Point3::ZERO, z, 1.0, t)).unwrap(),
        "the polar circle is on the original cone"
    );
}

/// The offset of an unbounded cone (base radius 1 at the origin, semi-angle 30°) by `d`
/// is the cone of radius `1 + d / cos(semi)` at the origin and the same semi-angle: the
/// radius moves by `cos(semi) d` at the height moved by `-sin(semi) d`. Outwards (`d >
/// 0`) the offset is a copy, from the original apex moved along its normal; inwards the
/// apex end collapses and the cone is truncated at the new apex.
#[test]
fn offsets_of_an_unbounded_cone_are_its_offset_cones() {
    let k = Kernel::default();
    let pl = Axis2Placement::IDENTITY;
    let semi = 30.0f64;
    let (t, c) = (semi.to_radians().tan(), semi.to_radians().cos());
    let z = Vec3::new(0.0, 0.0, 1.0);
    let cone =
        Cone::create_canonical_with_semi_angle(&k, &pl, 1.0, semi, None, false, false).unwrap();
    for d in [0.5, -0.5] {
        let off = cone
            .create_offset_surface(&k, d, 1e-6)
            .unwrap()
            .expect("an offset");
        let r = 1.0 + d / c;
        assert!(
            lies_on(off.rev().base(), &on(Point3::new(0.0, 0.0, 0.0), z, r, t)).unwrap(),
            "offset {d}: off the cone of radius {r}"
        );
        // its apex end: inwards the offset cone's apex; outwards the original apex moved
        // along its normal (cos(semi) X - sin(semi) Z) d
        let want = if d < 0.0 {
            -r / t
        } else {
            -1.0 / t - semi.to_radians().sin() * d
        };
        let end = off.bot_height() + off.rev().position().origin().z;
        // the truncated cone's heights come through the unbounded span: its rounding,
        // 16 ulp of `INFINITE_PARAMETER`, 4e-9
        let round = 16.0 * f64::EPSILON * INFINITE_PARAMETER;
        assert!(
            (end - want).abs() <= round,
            "offset {d}: apex end {end}, want {want}"
        );
        // STEP and NURB parameters name the same points (the polar converter is the offset's)
        for (u, v) in [(0.0, 3.0), (90.0, 10.0), (200.0, 1.0)] {
            let step = Point2::new(u, v);
            let p = off.evaluate_step_point(step).unwrap();
            let q = off
                .evaluate_point(&k, off.convert_uv_from_step_to_nurbs(&k, step).unwrap())
                .unwrap();
            assert!(
                (p - q).length() <= 1e-9 * (1.0 + v.abs()),
                "offset {d} at {step:?}: {p:?} vs {q:?}"
            );
        }
    }
}
