//! Offset of a curve along a face ([`offset_on_face`]): closed-form values on planes, cylinders, a
//! spline extrusion (all developable: the offset unrolled) and a sphere (along its meridians), the
//! deviation reported on a ruled loft, the cut at a face's boundary, and the refusals.

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3, brep};
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::prelude::*;
use std::f64::consts::{PI, TAU};

use crate::*;

const TOL: f64 = 1e-4;

fn points(c: &(brep::Curve, [f64; 2]), n: usize) -> Vec<Vec3> {
    let e = CurveEval::new(&c.0).unwrap();
    let [a, b] = c.1;
    (0..=n).map(|k| e.point(a + (b - a) * k as f64 / n as f64).unwrap()).collect()
}

fn length(c: &(brep::Curve, [f64; 2])) -> f64 {
    points(c, 4000).windows(2).map(|w| w[0].dist(w[1])).sum()
}

fn ends(c: &(brep::Curve, [f64; 2])) -> (Vec3, Vec3) {
    let p = points(c, 1);
    (p[0], p[1])
}

/// The pair of ends in either order matches `(a, b)`.
fn same_ends((p, q): (Vec3, Vec3), a: Vec3, b: Vec3, tol: f64) -> bool {
    (p.dist(a) <= tol && q.dist(b) <= tol) || (p.dist(b) <= tol && q.dist(a) <= tol)
}

/// A strip of the XZ plane, x in [0, 40], z in [0, 30]: one planar face.
fn plane_strip() -> Body {
    extrude_sheet(&Plane::XY, &[vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(40.0, 0.0) }]], 0.0, 30.0).unwrap()
}

#[test]
fn offset_of_a_plane_strips_side_edge_is_the_parallel_line() {
    let body = plane_strip();
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(0.0, 0.0, 15.0)), 10.0, Vec3::new(20.0, 0.0, 15.0), TOL).unwrap();
    assert_eq!(r.curves.len(), 1);
    assert!(!r.clipped && r.deviation <= 1e-9, "{r:?}");
    assert!(points(&r.curves[0], 64).iter().all(|p| (p.x - 10.0).abs() < 1e-9 && p.y.abs() < 1e-9), "{r:?}");
    assert!(same_ends(ends(&r.curves[0]), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 30.0), 1e-9));
    // The bottom edge (z = 0), by 7: the line z = 7 across the whole strip.
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(20.0, 0.0, 0.0)), 7.0, Vec3::new(20.0, 0.0, 15.0), TOL).unwrap();
    assert!(points(&r.curves[0], 64).iter().all(|p| (p.z - 7.0).abs() < 1e-9));
    assert!((length(&r.curves[0]) - 40.0).abs() < 1e-9);
}

#[test]
fn offset_of_a_discs_rim_is_the_smaller_circle() {
    let body = patch_region(&Plane::XY, &Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] }).unwrap();
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(10.0, 0.0, 0.0)), 3.0, Vec3::ZERO, TOL).unwrap();
    let total: f64 = r.curves.iter().map(length).sum();
    assert!((total - TAU * 7.0).abs() < 1e-4, "{total}");
    for c in &r.curves {
        assert!(points(c, 64).iter().all(|p| (p.x.hypot(p.y) - 7.0).abs() < TOL && p.z.abs() < 1e-9));
    }
    assert!(r.deviation <= TOL, "{r:?}");
}

#[test]
fn offset_past_the_radius_inward_folds_and_is_refused() {
    let body = patch_region(&Plane::XY, &Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] }).unwrap();
    let e = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(10.0, 0.0, 0.0)), 12.0, Vec3::ZERO, TOL).unwrap_err();
    assert!(e.to_string().contains("folds over itself"), "{e}");
}

/// The scratch sheet: an arc 40 wide rising 8 (radius 29, centre (20, −21)), extruded 30 along z.
fn scratch_sheet() -> Body {
    let start = 21f64.atan2(20.0);
    let arc = Seg2::Arc { center: Vec2::new(20.0, -21.0), radius: 29.0, start, sweep: PI - 2.0 * start };
    extrude_sheet(&Plane::XY, &[vec![arc]], 0.0, 30.0).unwrap()
}

#[test]
fn offset_on_the_scratch_sheet_is_the_unrolled_offset_not_the_projection() {
    let body = scratch_sheet();
    assert_eq!(body.record().faces.len(), 1);
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(0.0, 0.0, 15.0)), 10.0, Vec3::new(20.0, 8.0, 15.0), TOL).unwrap();
    // 10 mm along the arc from (0, 0): the angle from the centre turns by 10/29.
    let phi = 21f64.atan2(-20.0) - 10.0 / 29.0;
    let (x, y) = (20.0 + 29.0 * phi.cos(), -21.0 + 29.0 * phi.sin());
    assert!((x - 8.276).abs() < 1e-3, "{x}");
    // The projection along the sketch normal would put it at x = 10: 1.724 mm off.
    assert!((10.0 - x - 1.724).abs() < 1e-3, "{x}");
    assert_eq!(r.curves.len(), 1);
    assert!(r.deviation <= 1e-6, "{r:?}");
    assert!(points(&r.curves[0], 64).iter().all(|p| (p.x - x).abs() < 1e-6 && (p.y - y).abs() < 1e-6), "{r:?} vs {x} {y}");
    assert!(same_ends(ends(&r.curves[0]), Vec3::new(x, y, 0.0), Vec3::new(x, y, 30.0), 1e-6));
    // The arc edge at z = 0, by 12: the same arc at z = 12, 29 × 1.52205 = 44.139 mm long.
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(20.0, 8.0, 0.0)), 12.0, Vec3::new(20.0, 8.0, 15.0), TOL).unwrap();
    let sweep = PI - 2.0 * 21f64.atan2(20.0);
    assert!((length(&r.curves[0]) - 29.0 * sweep).abs() < 1e-3, "{}", length(&r.curves[0]));
    assert!(points(&r.curves[0], 64).iter().all(|p| (p.z - 12.0).abs() < 1e-6 && ((p.x - 20.0).hypot(p.y + 21.0) - 29.0).abs() < TOL));
}

#[test]
fn offset_of_a_cylinders_rim_is_the_circle_up_its_side() {
    let body = cylinder(Vec3::ZERO, Vec3::Z, 5.0, 20.0).unwrap();
    let (face, _) = body
        .record()
        .faces
        .iter()
        .enumerate()
        .find(|(_, f)| !matches!(f.surface, brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. }))
        .unwrap();
    let rim = Vec3::new(5.0 * 2f64.cos(), 5.0 * 2f64.sin(), 0.0);
    let r = offset_on_face(&body, face, OffsetFrom::Edge(rim), 6.0, Vec3::new(rim.x, rim.y, 10.0), TOL).unwrap();
    let total: f64 = r.curves.iter().map(length).sum();
    assert!((total - TAU * 5.0).abs() < 1e-3, "{total}");
    for c in &r.curves {
        assert!(points(c, 16).iter().all(|p| (p.z - 6.0).abs() < 1e-6 && (p.x.hypot(p.y) - 5.0).abs() < TOL));
    }
}

/// A cubic profile extruded 30 along z: a spline surface (developable).
fn spline_sheet() -> (Body, Seg2) {
    let cubic = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(10.0, 15.0), p2: Vec2::new(30.0, -10.0), p3: Vec2::new(40.0, 5.0) };
    (extrude_sheet(&Plane::XY, &[vec![cubic]], 0.0, 30.0).unwrap(), cubic)
}

/// The point at arc length `s` along the cubic from its start (Simpson's rule, bisection).
fn cubic_at_length(c: Seg2, s: f64) -> Vec2 {
    let Seg2::Cubic { p0, p1, p2, p3 } = c else { panic!() };
    let at = |t: f64| p0 * (1.0 - t).powi(3) + p1 * (3.0 * t * (1.0 - t).powi(2)) + p2 * (3.0 * t * t * (1.0 - t)) + p3 * t.powi(3);
    let speed = |t: f64| ((p1 - p0) * (3.0 * (1.0 - t).powi(2)) + (p2 - p1) * (6.0 * t * (1.0 - t)) + (p3 - p2) * (3.0 * t * t)).len();
    let len = |b: f64| {
        let n = 2000;
        let h = b / n as f64;
        (0..=n)
            .map(|k| {
                speed(k as f64 * h)
                    * if k == 0 || k == n {
                        1.0
                    } else if k % 2 == 1 {
                        4.0
                    } else {
                        2.0
                    }
            })
            .sum::<f64>()
            * h
            / 3.0
    };
    let (mut a, mut b) = (0.0, 1.0);
    for _ in 0..60 {
        let m = 0.5 * (a + b);
        if len(m) < s { a = m } else { b = m }
    }
    at(0.5 * (a + b))
}

#[test]
fn offset_on_a_spline_extrusion_is_the_profile_point_at_that_arc_length() {
    let (body, cubic) = spline_sheet();
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(0.0, 0.0, 15.0)), 10.0, Vec3::new(20.0, 2.5, 15.0), TOL).unwrap();
    let q = cubic_at_length(cubic, 10.0);
    assert_eq!(r.curves.len(), 1);
    assert!(r.deviation <= TOL, "{r:?}");
    assert!(points(&r.curves[0], 64).iter().all(|p| (p.x - q.x).abs() < 1e-5 && (p.y - q.y).abs() < 1e-5), "{r:?} vs {q:?}");
    assert!((length(&r.curves[0]) - 30.0).abs() < 1e-5);
}

#[test]
fn offset_on_a_sphere_runs_down_the_meridians() {
    // A zone of the sphere of radius 20 about y, latitudes 0.2 to 1.2 rad, all the way round.
    let arc = Seg2::Arc { center: Vec2::ZERO, radius: 20.0, start: 0.2, sweep: 1.0 };
    let body = revolve_sheet(&Plane::XY, &[vec![arc]], Vec2::ZERO, Vec2::Y, 0.0, TAU).unwrap();
    // Picked opposite the profile (the seam).
    let low = Vec3::new(-20.0 * 0.2f64.cos(), 20.0 * 0.2f64.sin(), 0.0);
    let toward = Vec3::new(-20.0 * 0.7f64.cos(), 20.0 * 0.7f64.sin(), 0.0);
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(low), 5.0, toward, TOL).unwrap();
    // Latitude 0.2 + 5/20: a circle of radius 20 cos 0.45 at height 20 sin 0.45.
    let (rad, h) = (20.0 * 0.45f64.cos(), 20.0 * 0.45f64.sin());
    let total: f64 = r.curves.iter().map(length).sum();
    assert!((total - TAU * rad).abs() < 1e-3, "{total} vs {}", TAU * rad);
    for c in &r.curves {
        for p in points(c, 32) {
            assert!((p.y - h).abs() < TOL && (p.x.hypot(p.z) - rad).abs() < TOL, "{p:?}");
        }
    }
    assert!(r.deviation <= TOL, "{r:?}");
}

#[test]
fn offset_on_a_ruled_loft_reports_its_deviation() {
    // An arc at z = 0 lofted to a line at z = 30: a ruled surface that twists.
    let arc = Seg2::Arc { center: Vec2::new(20.0, -21.0), radius: 29.0, start: 21f64.atan2(20.0), sweep: PI - 2.0 * 21f64.atan2(20.0) };
    let top = Plane { origin: Vec3::new(0.0, 0.0, 30.0), ..Plane::XY };
    let body = loft_sheet(&[(Plane::XY, vec![arc]), (top, vec![Seg2::Line { a: Vec2::new(40.0, 0.0), b: Vec2::new(0.0, 0.0) }])]).unwrap();
    let s = SurfaceEval::new(body.record().faces[0].parameters()).unwrap();
    // The bottom arc, by 8 up the face.
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(20.0, 8.0, 0.0)), 8.0, Vec3::new(20.0, 4.0, 15.0), 0.01)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!r.curves.is_empty());
    assert!(r.deviation > 0.0 && r.deviation <= 0.01, "{r:?}");
    for c in &r.curves {
        for p in points(c, 32) {
            let uv = s.project(p, None, 1.0).unwrap();
            assert!(s.point(uv).unwrap().dist(p) <= 0.01, "{p:?} is off the face");
            // Along the face it is 8 from the bottom edge; straight up, no more than that.
            assert!(p.z <= 8.0 + 1e-6 && p.z > 4.0, "{p:?}");
        }
    }
}

#[test]
fn offset_of_a_trapezoids_base_is_cut_by_its_slanted_sides() {
    let outer = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(30.0, 20.0), Vec2::new(10.0, 20.0)]);
    let body = patch_region(&Plane::XY, &Region2 { outer, holes: vec![] }).unwrap();
    let r = offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(20.0, 0.0, 0.0)), 5.0, Vec3::new(20.0, 10.0, 0.0), TOL).unwrap();
    assert!(r.clipped);
    assert_eq!(r.curves.len(), 1);
    // To the boundary test's width (10⁻⁶ of the body's size): the cut ends on the slanted sides.
    assert!(same_ends(ends(&r.curves[0]), Vec3::new(2.5, 5.0, 0.0), Vec3::new(37.5, 5.0, 0.0), 1e-4), "{r:?}");
    assert!((length(&r.curves[0]) - 35.0).abs() < 2e-4);
}

#[test]
fn offset_of_a_curve_lying_on_the_face_goes_to_the_picked_side() {
    let body = plane_strip();
    let line = (brep::Curve::Line { start: [20.0, 0.0, 0.0], end: [20.0, 0.0, 30.0] }, [0.0, 1.0]);
    for (toward, x) in [(Vec3::new(35.0, 0.0, 15.0), 32.5), (Vec3::new(5.0, 0.0, 15.0), 7.5)] {
        let r = offset_on_face(&body, 0, OffsetFrom::Curves(std::slice::from_ref(&line)), 12.5, toward, TOL).unwrap();
        assert!(points(&r.curves[0], 16).iter().all(|p| (p.x - x).abs() < 1e-9), "{r:?}");
        assert!((length(&r.curves[0]) - 30.0).abs() < 1e-9);
    }
}

#[test]
fn offset_curve_refusals() {
    let body = plane_strip();
    let inside = Vec3::new(20.0, 0.0, 15.0);
    let edge = OffsetFrom::Edge(Vec3::new(0.0, 0.0, 15.0));
    let msg = |r: Result<FaceOffset>| r.unwrap_err().to_string();
    // The strip is 40 wide: 50 leaves it everywhere.
    assert!(msg(offset_on_face(&body, 0, edge, 50.0, inside, TOL)).contains("runs off the face everywhere"));
    assert!(msg(offset_on_face(&body, 0, edge, 0.0, inside, TOL)).contains("positive"));
    assert!(msg(offset_on_face(&body, 0, edge, -1.0, inside, TOL)).contains("positive"));
    assert!(msg(offset_on_face(&body, 0, edge, 5.0, inside, 0.0)).contains("tolerance"));
    assert!(msg(offset_on_face(&body, 0, OffsetFrom::Edge(Vec3::new(20.0, 9.0, 15.0)), 5.0, inside, TOL)).contains("not on an edge"));
    let off = (brep::Curve::Line { start: [20.0, 3.0, 0.0], end: [20.0, 3.0, 30.0] }, [0.0, 1.0]);
    assert!(msg(offset_on_face(&body, 0, OffsetFrom::Curves(std::slice::from_ref(&off)), 5.0, inside, TOL)).contains("not on the face"));
    let on = (brep::Curve::Line { start: [20.0, 0.0, 0.0], end: [20.0, 0.0, 30.0] }, [0.0, 1.0]);
    assert!(
        msg(offset_on_face(&body, 0, OffsetFrom::Curves(std::slice::from_ref(&on)), 5.0, Vec3::new(20.0, 0.0, 3.0), TOL))
            .contains("lies on the curve")
    );
    assert!(msg(offset_on_face(&body, 0, OffsetFrom::Curves(&[]), 5.0, inside, TOL)).contains("no curve"));
    assert!(offset_on_face(&body, 7, edge, 5.0, inside, TOL).is_err());
}
