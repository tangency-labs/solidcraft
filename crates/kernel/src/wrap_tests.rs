//! Adversarial and numerical-robustness tests for the cylindrical wrap: the development of a
//! cylinder's surface and the paths wrapped onto it.
//!
//! The claims under test: the wrapping is an isometry, so a curve keeps the length it was
//! drawn with to within 1e-5 mm whatever the radius; the seam costs the path nothing; and
//! hostile input is refused rather than crashing.

use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

use crate::wrap::{CylFrame, Development, WrappedPath, cylinder_spans, cylindrical_wall_at, develop, tangent_wall, wrap_path};
use crate::{Body, KernelError, Result};

/// The tolerance the feature claims: a wrapped curve keeps its length to this many mm.
const TOLERANCE: f64 = 1e-5;
/// The tolerance the path is sampled to, so that its chords stay within this of the surface.
const SAMPLING: f64 = 5e-4;

/// An error for a test that cannot set up its geometry.
fn setup(what: &str) -> KernelError {
    KernelError::Invalid(format!("test setup: {what}"))
}

/// A cylinder's frame: its axis is Z and its radial direction Y.
fn frame(radius: f64) -> Result<CylFrame> {
    CylFrame::new(Vec3::ZERO, Vec3::Z, radius, Vec3::Y).ok_or_else(|| setup("the frame"))
}

/// The plane tangent to the cylinder at `(0, radius, 0)`, its `x` running around the
/// circumference and its `y` along the axis.
fn tangent_plane(radius: f64) -> Result<Plane> {
    Plane::new(Vec3::new(0.0, radius, 0.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))
}

/// The development of a cylinder of the given radius, read on its tangent plane.
fn development(radius: f64) -> Result<Development> {
    let frame = frame(radius)?;
    let plane = tangent_plane(radius)?;
    develop(&frame, &plane)
}

/// The length of a path of segments as it lies on the surface, measured by sampling it finely.
///
/// A polyline's chord sum is shorter than the curve's own length; the sample is fine enough
/// for the shortfall to stay well under the micrometre the feature promises.
fn surface_length(dev: &Development, segs: &[Seg2], steps: usize) -> f64 {
    let mut length = 0.0;
    let mut previous: Option<Vec3> = None;
    for seg in segs {
        for i in 0..=steps {
            let p = dev.of(seg.point_at(i as f64 / steps as f64));
            let q = dev.frame.on_surface(p.x, p.y);
            if let Some(before) = previous {
                length += before.dist(q);
            }
            previous = Some(q);
        }
    }
    length
}

/// The greatest distance between the path's chords and the surface they run on.
fn chord_sagitta(dev: &Development, path: &WrappedPath) -> f64 {
    let mut worst: f64 = 0.0;
    for pair in path.points.windows(2) {
        let mid = (pair[0] + pair[1]) * 0.5;
        let w = mid - dev.frame.origin;
        let radial = w - dev.frame.axis * w.dot(dev.frame.axis);
        worst = worst.max((radial.len() - dev.frame.radius).abs());
    }
    worst
}

/// How far a point is from the cylinder's surface.
fn off_surface(dev: &Development, p: Vec3) -> f64 {
    let w = p - dev.frame.origin;
    let radial = w - dev.frame.axis * w.dot(dev.frame.axis);
    (radial.len() - dev.frame.radius).abs()
}

/// The turn the path makes about the axis, taken the short way round at every step.
fn turned_about(dev: &Development, points: &[Vec3]) -> f64 {
    let angles: Vec<f64> = points.iter().map(|p| dev.frame.angle(*p)).collect();
    let mut turned = 0.0;
    for pair in angles.windows(2) {
        turned += crate::wrap::unwrap(pair[1] - pair[0], 0.0);
    }
    turned
}

#[test]
fn a_line_keeps_its_length_at_every_radius() -> Result<()> {
    for radius in [1.0, 10.0, 1000.0] {
        let dev = development(radius)?;
        assert!(dev.is_isometry());
        // Around the circumference, along the axis, and across the two.
        let around = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(6.0, 0.0) };
        let along = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 4.0) };
        let across = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(3.0, 4.0) };
        for seg in [around, along, across] {
            let path = wrap_path(&dev, std::slice::from_ref(&seg))?;
            // The path keeps the length it was drawn with.
            assert!((path.length - seg.length()).abs() < 1e-12, "radius {radius}: {}", path.length);
            // And it is as long on the surface as it was in the sketch.
            let measured = surface_length(&dev, std::slice::from_ref(&seg), 4000);
            assert!((measured - seg.length()).abs() < TOLERANCE, "radius {radius}: {measured} vs {}", seg.length());
            // Every point lies on the cylinder, and the chords stay within the sampling tolerance.
            for p in &path.points {
                assert!(off_surface(&dev, *p) < 1e-9, "radius {radius}: {}", off_surface(&dev, *p));
            }
            assert!(chord_sagitta(&dev, &path) < SAMPLING * 1.01, "radius {radius}: {}", chord_sagitta(&dev, &path));
        }
    }
    Ok(())
}

#[test]
fn an_arc_keeps_its_length_at_every_radius() -> Result<()> {
    for radius in [1.0, 10.0, 1000.0] {
        let dev = development(radius)?;
        let arc = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 3.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 };
        let path = wrap_path(&dev, std::slice::from_ref(&arc))?;
        assert!((path.length - arc.length()).abs() < 1e-12, "radius {radius}: {}", path.length);
        let measured = surface_length(&dev, std::slice::from_ref(&arc), 4000);
        assert!((measured - arc.length()).abs() < TOLERANCE, "radius {radius}: {measured} vs {}", arc.length());
        for p in &path.points {
            assert!(off_surface(&dev, *p) < 1e-9, "radius {radius}: {}", off_surface(&dev, *p));
        }
        assert!(chord_sagitta(&dev, &path) < SAMPLING * 1.01, "radius {radius}: {}", chord_sagitta(&dev, &path));
    }
    Ok(())
}

#[test]
fn a_path_of_several_curves_keeps_its_length() -> Result<()> {
    let dev = development(10.0)?;
    // A line, an arc and a line, drawn end to end.
    let segs = [
        Seg2::Line { a: Vec2::new(-3.0, 2.0), b: Vec2::new(1.0, 2.0) },
        Seg2::Arc { center: Vec2::new(1.0, 5.0), radius: 3.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::FRAC_PI_2 },
        Seg2::Line { a: Vec2::new(4.0, 5.0), b: Vec2::new(4.0, 8.0) },
    ];
    let want: f64 = segs.iter().map(|s| s.length()).sum();
    let path = wrap_path(&dev, &segs)?;
    assert!((path.length - want).abs() < 1e-12, "{} vs {want}", path.length);
    let measured = surface_length(&dev, &segs, 2000);
    assert!((measured - want).abs() < TOLERANCE, "{measured} vs {want}");
    // The path does not jump where the curves meet.
    for pair in path.points.windows(2) {
        assert!(pair[0].dist(pair[1]) < 1.0, "{} mm", pair[0].dist(pair[1]));
    }
    Ok(())
}

#[test]
fn a_path_across_the_seam_keeps_its_length() -> Result<()> {
    let dev = development(10.0)?;
    // The seam is where the angle wraps from pi to -pi: the path runs across it.
    let half = std::f64::consts::PI * 10.0;
    let line = Seg2::Line { a: Vec2::new(half - 2.0, 5.0), b: Vec2::new(half + 2.0, 5.0) };
    let path = wrap_path(&dev, std::slice::from_ref(&line))?;
    assert!((path.length - 4.0).abs() < 1e-12, "{}", path.length);
    // The developed coordinates run on past the seam without a jump back.
    for pair in path.developed.windows(2) {
        assert!(pair[1].x > pair[0].x, "{} -> {}", pair[0].x, pair[1].x);
    }
    // The points turn once through the seam, and the turn is the path's length over the radius.
    let turned = turned_about(&dev, &path.points);
    assert!((10.0 * turned - 4.0).abs() < 1e-9, "{turned}");
    // The seam costs the path nothing.
    let measured = surface_length(&dev, std::slice::from_ref(&line), 4000);
    assert!((measured - 4.0).abs() < TOLERANCE, "{measured}");
    Ok(())
}

#[test]
fn a_whole_turn_keeps_its_length() -> Result<()> {
    let dev = development(10.0)?;
    let turn = std::f64::consts::TAU * 10.0;
    let line = Seg2::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(turn, 5.0) };
    let path = wrap_path(&dev, std::slice::from_ref(&line))?;
    assert!((path.length - turn).abs() < 1e-9, "{} vs {turn}", path.length);
    // The path comes back to where it started.
    let last = path.points[path.points.len() - 1];
    assert!(path.points[0].dist(last) < 1e-9, "{}", path.points[0].dist(last));
    // It turns exactly once, and it is as long as the circumference.
    let turned = turned_about(&dev, &path.points);
    assert!((turned - std::f64::consts::TAU).abs() < 1e-9, "{turned}");
    let measured = surface_length(&dev, std::slice::from_ref(&line), 8000);
    assert!((measured - turn).abs() < TOLERANCE, "{measured} vs {turn}");
    Ok(())
}

#[test]
fn a_path_on_an_extreme_radius_keeps_its_length() -> Result<()> {
    for radius in [0.01, 10_000.0] {
        let dev = development(radius)?;
        // A short run around the circumference, and a run along the axis.
        let around = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.1, 0.0) };
        let along = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 4.0) };
        for seg in [around, along] {
            let path = wrap_path(&dev, std::slice::from_ref(&seg))?;
            assert!((path.length - seg.length()).abs() < 1e-12, "radius {radius}: {}", path.length);
            let measured = surface_length(&dev, std::slice::from_ref(&seg), 2000);
            assert!((measured - seg.length()).abs() < TOLERANCE, "radius {radius}: {measured} vs {}", seg.length());
            for p in &path.points {
                assert!(off_surface(&dev, *p) < 1e-9, "radius {radius}: {}", off_surface(&dev, *p));
            }
            assert!(chord_sagitta(&dev, &path) < SAMPLING * 1.01, "radius {radius}: {}", chord_sagitta(&dev, &path));
        }
    }
    Ok(())
}

#[test]
fn hostile_paths_are_refused() -> Result<()> {
    let dev = development(10.0)?;
    // A path with no curve at all.
    assert!(wrap_path(&dev, &[]).is_err());
    // More segments than the path may carry.
    let many: Vec<Seg2> = (0..1025).map(|i| Seg2::Line { a: Vec2::new(i as f64, 0.0), b: Vec2::new(i as f64 + 0.5, 0.0) }).collect();
    assert!(wrap_path(&dev, &many).is_err());
    // A point that is not a number, and one that is infinite.
    assert!(wrap_path(&dev, &[Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::new(1.0, 0.0) }]).is_err());
    assert!(wrap_path(&dev, &[Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(f64::INFINITY, 1.0) }]).is_err());
    assert!(wrap_path(&dev, &[Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: f64::NAN, start: 0.0, sweep: 1.0 }]).is_err());
    // A path too long to sample.
    let small = development(0.01)?;
    let long = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(200.0, 0.0) };
    assert!(wrap_path(&small, &[long]).is_err());
    Ok(())
}

#[test]
fn hostile_frames_and_planes_are_refused() -> Result<()> {
    // A frame needs a radius and a radial direction square to the axis.
    assert!(CylFrame::new(Vec3::ZERO, Vec3::Z, 0.0, Vec3::Y).is_none());
    assert!(CylFrame::new(Vec3::ZERO, Vec3::Z, -1.0, Vec3::Y).is_none());
    assert!(CylFrame::new(Vec3::ZERO, Vec3::Z, f64::NAN, Vec3::Y).is_none());
    assert!(CylFrame::new(Vec3::ZERO, Vec3::Z, f64::INFINITY, Vec3::Y).is_none());
    assert!(CylFrame::new(Vec3::ZERO, Vec3::Z, 10.0, Vec3::Z).is_none());
    assert!(CylFrame::new(Vec3::ZERO, Vec3::ZERO, 10.0, Vec3::Y).is_none());
    let frame = frame(10.0)?;
    // A frame whose origin is not a number is refused: the development would be made of
    // numbers that are not numbers.
    let broken = CylFrame::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z, 10.0, Vec3::Y).ok_or_else(|| setup("the frame"))?;
    assert!(develop(&broken, &tangent_plane(10.0)?).is_err());
    // A plane that is not square to the axis.
    assert!(develop(&frame, &Plane::XY).is_err());
    // A plane square to the axis that does not touch the cylinder.
    let away = Plane::new(Vec3::new(0.0, 12.0, 0.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    assert!(develop(&frame, &away).is_err());
    // A plane square to the axis that cuts through the axis.
    let through = Plane::new(Vec3::new(0.0, 0.0, 0.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    assert!(develop(&frame, &through).is_err());
    // The same tangent plane seen from the other side is still the surface.
    let flipped = Plane::new(Vec3::new(0.0, 10.0, 0.0), Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    let dev = develop(&frame, &flipped)?;
    assert!(dev.is_isometry());
    Ok(())
}

#[test]
fn a_wall_is_found_by_its_point_and_measured_in_spans() -> Result<()> {
    let body: Body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0)?;
    let (index, frame) = cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 5.0))?;
    assert!((frame.radius - 10.0).abs() < 1e-9, "{}", frame.radius);
    assert!(index < 3, "{index}");
    // The wall's faces are measured apart, so that a path may cross the seam but not a
    // relief groove.
    let spans = cylinder_spans(&body, &frame)?;
    assert!(!spans.is_empty());
    let total: f64 = spans.iter().map(|s| s.sweep).sum();
    assert!((total - std::f64::consts::TAU).abs() < 1e-6, "{total}");
    // Every point of the wall is held by one of the spans.
    for i in 0..64 {
        let angle = std::f64::consts::TAU * i as f64 / 64.0;
        assert!(spans.iter().any(|s| s.holds(angle, 5.0, 1e-6, 1e-6)), "angle {angle}");
    }
    // A point off the wall is on no wall, and neither is a point that is not a number.
    assert!(cylindrical_wall_at(&body, Vec3::new(0.0, 0.0, 5.0)).is_err());
    assert!(cylindrical_wall_at(&body, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
    // A body that is a mesh has no wall to find.
    let mesh = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y], &[[0, 1, 2]])?;
    assert!(cylindrical_wall_at(&mesh, Vec3::new(0.0, 10.0, 5.0)).is_err());
    assert!(cylinder_spans(&mesh, &frame).is_err());
    assert!(tangent_wall(&mesh, &tangent_plane(10.0)?).is_err());
    Ok(())
}

#[test]
fn a_tangent_plane_finds_the_wall_it_stands_on() -> Result<()> {
    let body: Body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0)?;
    let (index, frame) = tangent_wall(&body, &tangent_plane(10.0)?)?.ok_or_else(|| setup("no wall"))?;
    assert!((frame.radius - 10.0).abs() < 1e-9, "{}", frame.radius);
    // The wall it finds is one of the wall's faces.
    let spans = cylinder_spans(&body, &frame)?;
    assert!(spans.iter().any(|s| s.face == index), "{index}");
    // A plane that touches no cylinder stands on no wall.
    let away = Plane::new(Vec3::new(0.0, 30.0, 0.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    assert!(tangent_wall(&body, &away)?.is_none());
    // A body with no cylinder at all has no wall.
    let block = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0))?;
    assert!(tangent_wall(&block, &tangent_plane(10.0)?)?.is_none());
    // A plane that stands tangent to more than one wall is refused.
    let near = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0)?;
    let far = crate::cylinder(Vec3::new(25.0, 0.0, 100.0), Vec3::Z, 15.0, 20.0)?;
    let joined = crate::ops::join_pieces(vec![near, far])?.ok_or_else(|| setup("the join"))?;
    let plane = Plane::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Y, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    assert!(tangent_wall(&joined, &plane).is_err());
    Ok(())
}
