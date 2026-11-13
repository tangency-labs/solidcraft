//! WR-682 (the SolidCraft fork's tangency-labs/smlib@5bdcddf, 14a6daa and 4c6571b; on every
//! kernel since its flag, `fillet_surface_plane_curve_on_plane_equation`, was retired: WR-682
//! addendum 2026-11-07): `sm_SrfSrfIntersection` traces the
//! curve of a surface and a plane between two points on both on the plane's equation, so
//! that the plane's own domain doesn't end it.
//!
//! The part: a cylinder R 10 about z and the plane z = 20 + x / 2, whose domain is a patch
//! that holds the points of their ellipse at 45 and 135 degrees but not the arc between
//! them, which bulges out of it to y = 10. The ellipse is known in closed form, so the 3D
//! curve, its uv on each surface and its length are each checked against it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_geom::p2::{srf_srf_intersection, SrfSrfCurves, SrfSrfOptions};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder_surface;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::Kernel;

const TOL: f64 = 1e-5;
const ANG_TOL: f64 = 0.2;

fn ellipse(deg: f64) -> Point3 {
    let t = deg.to_radians();
    Point3::new(10.0 * t.cos(), 10.0 * t.sin(), 20.0 + 5.0 * t.cos())
}

/// The cylinder as a B-spline surface, as a round's surface is: no analytic intersector takes
/// it.
fn cylinder(k: &Kernel) -> Surface {
    let cyl = create_cylinder_surface(k, Point3::new(0.0, 0.0, 0.0), 10.0, 40.0).unwrap();
    Surface::from(cyl.rev().base().clone())
}

/// The plane z = 20 + x / 2, on a patch to y = 8 and x = 8.
fn patch() -> Surface {
    let frame = Axis2Placement::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::new(1.0, 0.0, 0.5),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let mut plane = Plane::create_canonical(&frame).unwrap();
    let mut domain = Extent2d::default();
    domain.set_min_max_coords(-9.0, -8.0, 9.0, 8.0).unwrap();
    plane.adjust_step_uv_domain(&domain).unwrap();
    Surface::from(plane)
}

fn off_plane(x: Point3) -> f64 {
    (20.0 + 0.5 * x.x - x.z).abs() / 1.25f64.sqrt()
}

fn off_cylinder(x: Point3) -> f64 {
    (x.x.hypot(x.y) - 10.0).abs()
}

/// The length of the ellipse from `from` to `to` degrees.
fn ellipse_length(from: f64, to: f64) -> f64 {
    let n = 200_000;
    let mut last = ellipse(from);
    let mut sum = 0.0;
    for i in 1..=n {
        let p = ellipse(from + (to - from) * f64::from(i) / f64::from(n));
        sum += p.distance(last);
        last = p;
    }
    sum
}

fn intersect(
    k: &Kernel,
    a: &Surface,
    b: &Surface,
    from: f64,
    to: f64,
    dir: Option<Vec3>,
) -> Result<SrfSrfCurves, String> {
    let mut d = dir;
    srf_srf_intersection(
        k,
        Some(ellipse(from)),
        Some(ellipse(to)),
        d.as_mut(),
        None,
        a,
        b,
        TOL,
        ANG_TOL,
        SrfSrfOptions::default(),
    )
    .map_err(|e| format!("{e:?}"))
}

/// Checks the three curves against the ellipse and returns the 3D curve's length.
fn check_curves(
    k: &Kernel,
    cyl: &Surface,
    plane: &Surface,
    c: &SrfSrfCurves,
    uv_cyl: &solidcore::curve::curve::Curve,
    uv_plane: &solidcore::curve::curve::Curve,
) -> f64 {
    let curve = c.curve_3d.as_ref().unwrap();
    let ivl = curve.natural_interval();
    let n = 2000;
    let mut last = curve.evaluate_point(k, ivl.min()).unwrap();
    let mut length = 0.0;
    for i in 0..=n {
        let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / f64::from(n);
        let x = curve.evaluate_point(k, t).unwrap();
        length += x.distance(last);
        last = x;
        assert!(
            off_plane(x) <= TOL,
            "off the plane by {} at {t}",
            off_plane(x)
        );
        assert!(
            off_cylinder(x) <= TOL,
            "off the cylinder by {} at {t}",
            off_cylinder(x)
        );
        let w = uv_cyl.evaluate_point(k, t).unwrap();
        let on = cyl.evaluate_point(k, Point2::new(w.x, w.y)).unwrap();
        assert!(
            on.distance(x) <= TOL,
            "its uv on the cylinder is {} off at {t}",
            on.distance(x)
        );
        let w = uv_plane.evaluate_point(k, t).unwrap();
        // not clamped: past the patch is where this curve goes
        let SurfaceKind::Plane(p) = plane.kind() else {
            panic!("not a plane");
        };
        let on = p.evaluate_point_fast(Point2::new(w.x, w.y));
        assert!(
            on.distance(x) <= TOL,
            "its uv on the plane is {} off at {t}",
            on.distance(x)
        );
    }
    length
}

#[test]
fn the_curve_runs_past_the_plane_s_domain_to_its_end() {
    let k = Kernel::default();
    let (cyl, plane) = (cylinder(&k), patch());
    let c = intersect(&k, &cyl, &plane, 45.0, 135.0, None).unwrap();
    let curve = c.curve_3d.as_ref().unwrap();
    let ivl = curve.natural_interval();
    assert!(
        curve
            .evaluate_point(&k, ivl.min())
            .unwrap()
            .distance(ellipse(45.0))
            <= 1e-12
    );
    assert!(
        curve
            .evaluate_point(&k, ivl.max())
            .unwrap()
            .distance(ellipse(135.0))
            <= 1e-12
    );
    let length = check_curves(
        &k,
        &cyl,
        &plane,
        &c,
        c.uv_curve1.as_ref().unwrap(),
        c.uv_curve2.as_ref().unwrap(),
    );
    let want = ellipse_length(45.0, 135.0);
    assert!(
        (length - want).abs() <= 1e-4,
        "length {length}, want {want}"
    );
}

#[test]
fn the_uv_curves_follow_the_order_of_the_surfaces() {
    let k = Kernel::default();
    let (cyl, plane) = (cylinder(&k), patch());
    let c = intersect(&k, &plane, &cyl, 45.0, 135.0, None).unwrap();
    check_curves(
        &k,
        &cyl,
        &plane,
        &c,
        c.uv_curve2.as_ref().unwrap(),
        c.uv_curve1.as_ref().unwrap(),
    );
}

/// 14a6daa: the curve leaves its start the way it's told, not towards the end.
#[test]
fn the_start_direction_chooses_the_way_round() {
    let k = Kernel::default();
    let (cyl, plane) = (cylinder(&k), patch());
    let t = 45f64.to_radians();
    // the way the angle grows at 45 degrees, and the other way
    let up = Vec3::new(-t.sin(), t.cos(), -0.5 * t.sin());
    let short = intersect(&k, &cyl, &plane, 45.0, 135.0, Some(up)).unwrap();
    let length = check_curves(
        &k,
        &cyl,
        &plane,
        &short,
        short.uv_curve1.as_ref().unwrap(),
        short.uv_curve2.as_ref().unwrap(),
    );
    let want = ellipse_length(45.0, 135.0);
    assert!(
        (length - want).abs() <= 1e-4,
        "length {length}, want {want}"
    );
    // the long way, on the cylinder with its seam on the short arc, at 90 degrees: a trace that
    // ran off the surface's domain would only end at the routine's step limit
    let frame = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
    );
    let mut turned = Cone::create_canonical_cylinder(&k, &frame, 10.0).unwrap();
    let mut domain = Extent2d::default();
    domain.set_min_max_coords(0.0, 0.0, 360.0, 40.0).unwrap();
    turned.adjust_step_uv_domain(&k, &domain).unwrap();
    let turned = Surface::from(turned.rev().base().clone());
    let long = intersect(&k, &turned, &plane, 45.0, 135.0, Some(-up)).unwrap();
    let length = check_curves(
        &k,
        &turned,
        &plane,
        &long,
        long.uv_curve1.as_ref().unwrap(),
        long.uv_curve2.as_ref().unwrap(),
    );
    let want = ellipse_length(45.0, 135.0 - 360.0);
    assert!(
        (length - want).abs() <= 1e-4,
        "length {length}, want {want}"
    );
}

/// The intersection between `from` and `to`, as `sm_SrfSrfIntersection` gives it.
fn curves(
    k: &Kernel,
    a: &Surface,
    b: &Surface,
    from: Point3,
    to: Point3,
) -> solidcore::Result<SrfSrfCurves> {
    srf_srf_intersection(
        k,
        Some(from),
        Some(to),
        None,
        None,
        a,
        b,
        TOL,
        ANG_TOL,
        SrfSrfOptions::default(),
    )
}

/// Not traced on the plane's equation: the intersectors' own answer, the status 1001 they
/// give where the plane's domain or an end off a surface stops them.
fn assert_not_found(a: &Surface, b: &Surface, from: Point3, to: Point3) {
    let r = curves(&Kernel::default(), a, b, from, to);
    // a typed blend failure keeps the status of the bare `Upstream` (#13790)
    let e = r.unwrap_err();
    assert_eq!(solidcore::error::upstream_status(&e), Some(1001), "{e:?}");
}

/// Not traced on the plane's equation: the intersectors' exact line from `from` to `to`
/// (every point of the curve on the segment), not the trace's Hermite spans.
fn assert_line(a: &Surface, b: &Surface, from: Point3, to: Point3) {
    let k = Kernel::default();
    let c = curves(&k, a, b, from, to).unwrap().curve_3d.unwrap();
    let ivl = c.natural_interval();
    let ends = [ivl.min(), ivl.max()].map(|t| c.evaluate_point(&k, t).unwrap());
    let fits = |p: Point3, q: Point3| ends[0].distance(p) < 1e-9 && ends[1].distance(q) < 1e-9;
    assert!(fits(from, to) || fits(to, from), "{ends:?}");
    let along = (to - from) / (to - from).length();
    for i in 1..16 {
        let p = c
            .evaluate_point(&k, ivl.evaluate(f64::from(i) / 16.0))
            .unwrap();
        let off = (p - from) - along * (p - from).dot(along);
        assert!(
            off.length() < 1e-9,
            "{p:?} off the line by {}",
            off.length()
        );
    }
}

/// The analytic intersector's answer stands for the cylinder as itself: its section ends at
/// the plane's domain, so between these points it finds none.
#[test]
fn an_analytic_answer_is_kept() {
    let k = Kernel::default();
    let cyl =
        Surface::from(create_cylinder_surface(&k, Point3::new(0.0, 0.0, 0.0), 10.0, 40.0).unwrap());
    assert_not_found(&cyl, &patch(), ellipse(45.0), ellipse(135.0));
}

/// 4c6571b: a flat B-spline surface meets the plane in a line, the analytic intersector's.
#[test]
fn a_flat_surface_is_not_traced() {
    let frame = Axis2Placement::new(
        Point3::new(-20.0, -20.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let mut flat = Plane::create_canonical(&frame).unwrap();
    let mut domain = Extent2d::default();
    domain.set_min_max_coords(0.0, 0.0, 40.0, 60.0).unwrap();
    flat.adjust_step_uv_domain(&domain).unwrap();
    flat.make_nurb().unwrap();
    let nurb = Surface::from(flat.base().clone());
    // the wall y = -20 cuts the plane z = 20 + x / 2 in the line y = -20
    let at = |x: f64| Point3::new(x, -20.0, 20.0 + 0.5 * x);
    let frame = Axis2Placement::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::new(1.0, 0.0, 0.5),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let mut wide = Plane::create_canonical(&frame).unwrap();
    let mut big = Extent2d::default();
    big.set_min_max_coords(-30.0, -30.0, 30.0, 30.0).unwrap();
    wide.adjust_step_uv_domain(&big).unwrap();
    assert_line(&nurb, &Surface::from(wide), at(-10.0), at(10.0));
}

/// An end where the surface touches the plane is a singular point: not traced.
#[test]
fn a_surface_touching_the_plane_is_not_traced() {
    let k = Kernel::default();
    let cyl = cylinder(&k);
    // the plane x = 10 touches the cylinder along the line x = 10, y = 0
    let frame = Axis2Placement::new(
        Point3::new(10.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let mut touching = Plane::create_canonical(&frame).unwrap();
    let mut d = Extent2d::default();
    d.set_min_max_coords(-30.0, -30.0, 30.0, 60.0).unwrap();
    touching.adjust_step_uv_domain(&d).unwrap();
    assert_line(
        &cyl,
        &Surface::from(touching),
        Point3::new(10.0, 0.0, 5.0),
        Point3::new(10.0, 0.0, 30.0),
    );
}

/// An end off the surface isn't taken either.
#[test]
fn an_end_off_a_surface_is_not_traced() {
    let k = Kernel::default();
    let cyl = cylinder(&k);
    let off = Point3::new(9.0, 0.0, 24.5);
    assert_not_found(&cyl, &patch(), ellipse(45.0), off);
}

/// The plane z = `z`, on a patch that holds the circle of radius 10 about z.
fn level(z: f64) -> Surface {
    let frame = Axis2Placement::new(
        Point3::new(0.0, 0.0, z),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let mut plane = Plane::create_canonical(&frame).unwrap();
    let mut domain = Extent2d::default();
    domain.set_min_max_coords(-30.0, -30.0, 30.0, 30.0).unwrap();
    plane.adjust_step_uv_domain(&domain).unwrap();
    Surface::from(plane)
}

/// The cylinder of radius 10 about z as itself (the analytic intersector's: it meets a level
/// plane in the whole circle, closed, whose parameter starts and ends at the cylinder's seam),
/// its seam at `seam_deg` degrees about z.
fn seamed_cylinder(k: &Kernel, seam_deg: f64) -> Surface {
    let t = seam_deg.to_radians();
    let frame = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(t.cos(), t.sin(), 0.0),
        Vec3::new(-t.sin(), t.cos(), 0.0),
    );
    let mut cylinder = Cone::create_canonical_cylinder(k, &frame, 10.0).unwrap();
    let mut domain = Extent2d::default();
    domain.set_min_max_coords(0.0, 0.0, 360.0, 40.0).unwrap();
    cylinder.adjust_step_uv_domain(k, &domain).unwrap();
    Surface::from(cylinder)
}

/// The point at `deg` degrees on the circle of radius 10 about z at height 5.
fn on_circle(deg: f64) -> Point3 {
    let t = deg.to_radians();
    Point3::new(10.0 * t.cos(), 10.0 * t.sin(), 5.0)
}

/// With `closed_by_start_direction` (not upstream), the closed circle a cylinder and a plane
/// meet in, between two points, runs from the start the way the start direction says, also
/// where an end lies at the circle's closure (the cylinder's seam, where the circle's parameter
/// starts and ends: the end has two parameters) and where the arc between the points runs
/// through it. The arc is checked by its ends, by lying on the circle and by its length, 10
/// times the angle it sweeps (the short and the long way round differ by at least 10 (2 pi - 2
/// angles)).
#[test]
fn a_closed_curve_runs_from_its_start_the_way_the_start_direction_says() {
    let k = Kernel::default();
    let plane = level(5.0);
    // (the seam, from, to, the angle swept from `from`): the chord as the start direction, as
    // the cap edge along a boss's foot passes it
    let cases = [
        // the end at the closure, the start before it
        (0.0, -10.0, 0.0, 10.0),
        // the start at the closure
        (0.0, 0.0, 10.0, 10.0),
        // the end at the closure, coming down to it
        (0.0, 10.0, 0.0, -10.0),
        // the arc through the closure
        (5.0, -10.0, 10.0, 20.0),
        (5.0, 10.0, -10.0, -20.0),
        // the closure away from the arc
        (90.0, -10.0, 0.0, 10.0),
    ];
    for (seam, from, to, swept) in cases {
        let cylinder = seamed_cylinder(&k, seam);
        let mut dir = on_circle(to) - on_circle(from);
        check_arc(
            &k,
            &cylinder,
            &plane,
            (from, to),
            &mut dir,
            true,
            swept,
            &format!("seam {seam}, {from} -> {to}"),
        );
    }
    // the long way round where the start direction points that way: from -10 down to 0 = -360
    let cylinder = seamed_cylinder(&k, 0.0);
    let t = (-10f64).to_radians();
    let mut down = Vec3::new(t.sin(), -t.cos(), 0.0);
    check_arc(
        &k,
        &cylinder,
        &plane,
        (-10.0, 0.0),
        &mut down,
        true,
        -350.0,
        "seam 0, -10 -> 0 the long way",
    );
}

/// Without `closed_by_start_direction` (every caller but a turned-back rail's cap edge, most
/// passing the chord as the start direction), the closed circle between two points is
/// upstream's: the span between the parameters the points drop to. An arc longer than a half
/// turn stays one though the chord heads the other way at its start, and an end at the
/// closure keeps the parameter the drop gives (the long way round from -10 to the seam at 0).
#[test]
fn a_closed_curve_is_upstream_s_span_without_the_option() {
    let k = Kernel::default();
    let plane = level(5.0);
    let cylinder = seamed_cylinder(&k, 0.0);
    // 20 -> 290: the 270 degree arc, the span between their parameters
    let mut chord = on_circle(290.0) - on_circle(20.0);
    check_arc(
        &k,
        &cylinder,
        &plane,
        (20.0, 290.0),
        &mut chord,
        false,
        270.0,
        "seam 0, 20 -> 290",
    );
    // -10 -> 0 at the seam: the seam's point drops to the circle's start
    let mut chord = on_circle(0.0) - on_circle(-10.0);
    check_arc(
        &k,
        &cylinder,
        &plane,
        (-10.0, 0.0),
        &mut chord,
        false,
        -350.0,
        "seam 0, -10 -> 0 as upstream",
    );
}

/// Checks that `sm_SrfSrfIntersection` of `a` and `b` from the circle's point at `ends.0`
/// degrees to the one at `ends.1`, with the start direction `dir` and
/// `closed_by_start_direction`, is the circle's arc from `ends.0` sweeping `swept` degrees.
#[allow(clippy::too_many_arguments)]
fn check_arc(
    k: &Kernel,
    a: &Surface,
    b: &Surface,
    ends: (f64, f64),
    dir: &mut Vec3,
    closed_by_start_direction: bool,
    swept: f64,
    what: &str,
) {
    let (from, to) = ends;
    let c = srf_srf_intersection(
        k,
        Some(on_circle(from)),
        Some(on_circle(to)),
        Some(dir),
        None,
        a,
        b,
        TOL,
        ANG_TOL,
        SrfSrfOptions {
            closed_by_start_direction,
            ..SrfSrfOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let curve = c.curve_3d.as_ref().unwrap();
    let ivl = curve.natural_interval();
    let start = curve.evaluate_point(k, ivl.min()).unwrap();
    let end = curve.evaluate_point(k, ivl.max()).unwrap();
    assert!(
        start.distance(on_circle(from)) <= 1e-9,
        "{what}: starts at {start:?}"
    );
    assert!(
        end.distance(on_circle(to)) <= 1e-9,
        "{what}: ends at {end:?}"
    );
    let n = 20_000;
    let mut last = start;
    let mut length = 0.0;
    for i in 1..=n {
        let p = curve
            .evaluate_point(k, ivl.evaluate(f64::from(i) / f64::from(n)))
            .unwrap();
        let off = (p.x.hypot(p.y) - 10.0).abs().max((p.z - 5.0).abs());
        assert!(off <= 1e-9, "{what}: {p:?} off the circle by {off}");
        length += p.distance(last);
        last = p;
    }
    let want = 10.0 * swept.abs().to_radians();
    assert!(
        (length - want).abs() <= 1e-4,
        "{what}: length {length}, want {want}"
    );
}
