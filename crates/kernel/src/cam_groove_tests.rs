//! Adversarial and numerical-robustness tests for the cam groove: the cutter that follows a
//! path wrapped onto a cylindrical wall.
//!
//! The claims under test: the groove that comes out is a sound shell whose volume changed by
//! the amount the geometry says it should; a wall that a relief groove interrupts still takes
//! a groove where it is there; and hostile input is refused rather than crashing.

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::wrap::CylFrame;
use crate::{Body, GrooveTool, KernelError, Result, cam_groove, cylinder_spans, cylindrical_wall_at};

/// An error for a test that cannot set up its geometry.
fn setup(what: &str) -> KernelError {
    KernelError::Invalid(format!("test setup: {what}"))
}

/// A cylinder of the given radius and height, and the plane tangent to it at the middle of
/// its height, whose `x` runs around the circumference and its `y` along the axis.
fn sleeve(radius: f64, height: f64) -> Result<(Body, Plane)> {
    let body = crate::cylinder(Vec3::ZERO, Vec3::Z, radius, height)?;
    let plane = Plane::new(Vec3::new(0.0, radius, height / 2.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    Ok((body, plane))
}

/// The face of a cylinder's wall that carries a point.
fn face_at(body: &Body, frame: &CylFrame, at: Vec3) -> Result<usize> {
    let angle = frame.angle(at);
    let z = frame.axial(at);
    for span in cylinder_spans(body, frame)? {
        if span.holds(angle, z, 1e-6, 1e-6) {
            return Ok(span.face);
        }
    }
    Err(setup("no face at the point"))
}

/// The face of the wall the sketch plane stands on.
fn wall_face(body: &Body, plane: &Plane, radius: f64) -> Result<usize> {
    let at = Vec3::new(0.0, radius, plane.origin.z);
    let (_, frame) = cylindrical_wall_at(body, at)?;
    face_at(body, &frame, at)
}

/// The volume of a body, measured on its own mesh.
fn volume(body: &Body) -> Result<f64> {
    Ok(body.tessellate_with(1e-4, true)?.measure().volume)
}

/// The point on a cylinder of the given radius, at the given angle and height.
fn on_cylinder(radius: f64, angle: f64, z: f64) -> Vec3 {
    (Vec3::Y * angle.cos() - Vec3::X * angle.sin()) * radius + Vec3::Z * z
}

/// What a slot of the given width cuts out of a wall of radius 10: the cutter carries a
/// straight profile, so the slot is a chord of the wall, and what it takes is the strip
/// between the circle of radius 10 and the line y = 9, over the chord from x = -2 to x = 2,
/// times the slot's length.
fn slot_volume(length: f64) -> f64 {
    let half = 2.0;
    let circle = |x: f64| (100.0 - x * x).sqrt();
    let strip = 2.0 * ((half / 2.0) * circle(half) + 50.0 * (half / 10.0).asin()) - 9.0 * 4.0;
    length * strip
}

/// A cylinder with a relief groove turned into it: the wall is interrupted between z = 9 and
/// z = 11, where the surface drops to a radius of `radius - 1`.
fn relieved(radius: f64, height: f64) -> Result<Body> {
    let profile = Region2 {
        outer: Loop2::polygon(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(radius, 0.0),
            Vec2::new(radius, 9.0),
            Vec2::new(radius - 1.0, 9.0),
            Vec2::new(radius - 1.0, 11.0),
            Vec2::new(radius, 11.0),
            Vec2::new(radius, height),
            Vec2::new(0.0, height),
        ]),
        holes: vec![],
    };
    // The profile is drawn in the (radius, height) plane and turned about the axis.
    let plane = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    let mut bodies = crate::revolve(&plane, &[profile], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?;
    bodies.pop().ok_or_else(|| setup("the revolve"))
}

#[test]
fn a_ring_groove_is_cut_and_its_volume_is_exact() -> Result<()> {
    let (body, plane) = sleeve(8.0, 16.0)?;
    let before = volume(&body)?;
    // A path around the whole circumference, 3 mm wide and 1.5 mm deep, at half the height.
    let turn = std::f64::consts::TAU * 8.0;
    let segs = [Seg2::Line { a: Vec2::new(-turn / 2.0, 0.0), b: Vec2::new(turn / 2.0, 0.0) }];
    let face = wall_face(&body, &plane, 8.0)?;
    let result = cam_groove(&body, face, &plane, &segs, 1.5, 3.0, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    // What the groove takes is the wall between the cylinder of radius 8 and the cylinder of
    // radius 6.5, over the ring's width.
    let expected = std::f64::consts::PI * (8.0 * 8.0 - 6.5 * 6.5) * 3.0;
    let after = volume(&result)?;
    assert!((before - after - expected).abs() < 0.5, "{} vs {expected}", before - after);
    // The wall is gone between radius 6.5 and 8, over the ring's width.
    let mesh = result.tessellate_with(0.01, true)?;
    assert!(!mesh.contains(Vec3::new(0.0, 7.25, 8.0)), "the groove's floor is missing");
    assert!(mesh.contains(Vec3::new(0.0, 6.0, 8.0)), "the groove went too deep");
    assert!(mesh.contains(Vec3::new(0.0, 7.25, 4.0)), "the groove is the wrong width");
    assert!(mesh.contains(Vec3::new(0.0, 7.25, 12.0)), "the groove is the wrong width");
    Ok(())
}

#[test]
fn an_open_track_cuts_only_its_own_length() -> Result<()> {
    let (body, plane) = sleeve(8.0, 16.0)?;
    let before = volume(&body)?;
    // A quarter turn of the circumference, 3 mm wide and 1.5 mm deep.
    let quarter = std::f64::consts::FRAC_PI_2 * 8.0;
    let segs = [Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(quarter, 0.0) }];
    let face = wall_face(&body, &plane, 8.0)?;
    let result = cam_groove(&body, face, &plane, &segs, 1.5, 3.0, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    // What the groove takes is the wall between the two cylinders, over the sector it covers
    // and the ring's width.
    let expected = (8.0 * 8.0 - 6.5 * 6.5) / 2.0 * std::f64::consts::FRAC_PI_2 * 3.0;
    let after = volume(&result)?;
    assert!((before - after - expected).abs() < 0.5, "{} vs {expected}", before - after);
    let mesh = result.tessellate_with(0.01, true)?;
    // The groove is there where the path ran...
    assert!(!mesh.contains(on_cylinder(7.25, std::f64::consts::FRAC_PI_4, 8.0)), "the groove is missing");
    // ... and not where it did not.
    assert!(mesh.contains(on_cylinder(7.25, 3.0 * std::f64::consts::FRAC_PI_4, 8.0)), "the groove is the wrong length");
    assert!(mesh.contains(on_cylinder(7.25, -std::f64::consts::FRAC_PI_4, 8.0)), "the groove is the wrong length");
    Ok(())
}

#[test]
fn a_slot_along_the_axis_is_a_straight_cut_across_the_wall() -> Result<()> {
    let (body, plane) = sleeve(10.0, 20.0)?;
    let before = volume(&body)?;
    // A path along the axis (the sketch's `y` runs along it), 4 mm around the circumference
    // and 4 mm along the axis, 1 mm deep.
    let segs = [Seg2::Line { a: Vec2::new(0.0, -2.0), b: Vec2::new(0.0, 2.0) }];
    let face = wall_face(&body, &plane, 10.0)?;
    let result = cam_groove(&body, face, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    let expected = slot_volume(4.0);
    let after = volume(&result)?;
    assert!((before - after - expected).abs() < 0.05, "{} vs {expected}", before - after);
    let mesh = result.tessellate_with(0.01, true)?;
    assert!(!mesh.contains(Vec3::new(0.0, 9.5, 10.0)), "the slot is missing");
    assert!(mesh.contains(Vec3::new(0.0, 8.5, 10.0)), "the slot went too deep");
    // The slot is as wide as the cutter, and it stops where the path stopped.
    assert!(!mesh.contains(Vec3::new(1.5, 9.5, 10.0)), "the slot is the wrong width");
    assert!(mesh.contains(Vec3::new(2.5, 9.5, 10.0)), "the slot is the wrong width");
    assert!(mesh.contains(Vec3::new(0.0, 9.5, 6.0)), "the slot ran past its start");
    assert!(mesh.contains(Vec3::new(0.0, 9.5, 14.0)), "the slot ran past its end");
    Ok(())
}

#[test]
fn a_groove_is_cut_on_a_wall_a_relief_interrupted() -> Result<()> {
    let body = relieved(10.0, 20.0)?;
    // The wall is in pieces: below the relief and above it.
    let (_, frame) = cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 4.0))?;
    let spans = cylinder_spans(&body, &frame)?;
    assert!(spans.len() >= 2, "{spans:?}");
    // Every point of the wall, below the relief and above it, is held by one of the spans.
    for i in 0..32 {
        let angle = std::f64::consts::TAU * i as f64 / 32.0;
        for z in [4.0, 15.0] {
            let p = frame.on_surface(angle * frame.radius, frame.axial(Vec3::new(0.0, 10.0, z)));
            assert!(spans.iter().any(|s| s.holds(frame.angle(p), frame.axial(p), 1e-6, 1e-6)), "angle {angle} z {z}");
        }
    }
    // A slot on the piece of wall below the relief.
    let lower = face_at(&body, &frame, Vec3::new(0.0, 10.0, 4.0))?;
    let plane = Plane::new(Vec3::new(0.0, 10.0, 4.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    let segs = [Seg2::Line { a: Vec2::new(0.0, -2.0), b: Vec2::new(0.0, 2.0) }];
    let before = volume(&body)?;
    let result = cam_groove(&body, lower, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    let expected = slot_volume(4.0);
    let after = volume(&result)?;
    assert!((before - after - expected).abs() < 0.05, "{} vs {expected}", before - after);
    let mesh = result.tessellate_with(0.01, true)?;
    assert!(!mesh.contains(Vec3::new(0.0, 9.5, 6.0)), "the slot is missing");
    assert!(mesh.contains(Vec3::new(0.0, 8.5, 6.0)), "the slot went too deep");
    // The relief itself is untouched: the wall is interrupted there, so there is no material
    // between radius 9 and 10.
    assert!(!mesh.contains(Vec3::new(0.0, 9.5, 10.0)), "the relief was cut into");
    assert!(mesh.contains(Vec3::new(0.0, 8.5, 10.0)), "the relief was cut into");
    Ok(())
}

#[test]
fn a_groove_is_cut_on_the_wall_above_a_relief() -> Result<()> {
    let body = relieved(10.0, 20.0)?;
    let (_, frame) = cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 15.0))?;
    // The piece of wall above the relief is a wall of its own, and it takes a groove.
    let upper = face_at(&body, &frame, Vec3::new(0.0, 10.0, 15.0))?;
    let plane = Plane::new(Vec3::new(0.0, 10.0, 15.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    let segs = [Seg2::Line { a: Vec2::new(0.0, -2.0), b: Vec2::new(0.0, 2.0) }];
    let before = volume(&body)?;
    let result = cam_groove(&body, upper, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    let expected = slot_volume(4.0);
    let after = volume(&result)?;
    assert!((before - after - expected).abs() < 0.05, "{} vs {expected}", before - after);
    let mesh = result.tessellate_with(0.01, true)?;
    assert!(!mesh.contains(Vec3::new(0.0, 9.5, 17.0)), "the slot is missing");
    assert!(mesh.contains(Vec3::new(0.0, 8.5, 17.0)), "the slot went too deep");
    // The wall below the relief is untouched.
    assert!(mesh.contains(Vec3::new(0.0, 9.5, 6.0)), "the groove was cut in the wrong place");
    Ok(())
}

#[test]
fn a_path_that_runs_into_a_relief_is_refused() -> Result<()> {
    let body = relieved(10.0, 20.0)?;
    let (_, frame) = cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 4.0))?;
    let lower = face_at(&body, &frame, Vec3::new(0.0, 10.0, 4.0))?;
    let plane = Plane::new(Vec3::new(0.0, 10.0, 4.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    // A path that runs off the lower wall and over the relief is refused.
    let over = [Seg2::Line { a: Vec2::new(0.0, -2.0), b: Vec2::new(0.0, 10.0) }];
    assert!(cam_groove(&body, lower, &plane, &over, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // So is a path that runs off the end of the wall.
    let off = [Seg2::Line { a: Vec2::new(0.0, -2.0), b: Vec2::new(0.0, 20.0) }];
    assert!(cam_groove(&body, lower, &plane, &off, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // So is a path that runs into the relief from above.
    let upper = face_at(&body, &frame, Vec3::new(0.0, 10.0, 15.0))?;
    let plane = Plane::new(Vec3::new(0.0, 10.0, 15.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    let down = [Seg2::Line { a: Vec2::new(0.0, 2.0), b: Vec2::new(0.0, -10.0) }];
    assert!(cam_groove(&body, upper, &plane, &down, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    Ok(())
}

#[test]
fn a_groove_that_reaches_through_a_thin_wall_is_cut() -> Result<()> {
    // A sleeve: an outer cylinder of radius 5 with a hole of radius 3.5 through it, so the
    // wall is 1.5 mm thick.
    let (body, plane) = sleeve(5.0, 20.0)?;
    let inner = crate::cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 3.5, 22.0)?;
    let tube = crate::boolean(&body, &inner, crate::BoolOp::Cut)?.ok_or_else(|| setup("the hole"))?;
    let turn = std::f64::consts::TAU * 5.0;
    let segs = [Seg2::Line { a: Vec2::new(-turn / 2.0, 0.0), b: Vec2::new(turn / 2.0, 0.0) }];
    let face = wall_face(&tube, &plane, 5.0)?;
    // A groove that stops short of the hole leaves the wall standing: the material below the
    // groove's floor still joins the wall above and below it.
    let before = volume(&tube)?;
    let shallow = cam_groove(&tube, face, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular)?;
    assert_eq!(shallow.shell_count(), 1);
    assert!(shallow.validity().is_empty(), "{:?}", shallow.validity());
    let expected = std::f64::consts::PI * (5.0 * 5.0 - 4.0 * 4.0) * 4.0;
    let after = volume(&shallow)?;
    assert!((before - after - expected).abs() < 0.5, "{} vs {expected}", before - after);
    // A groove deeper than the wall cuts through it, and the tube falls into two pieces.
    let deep = cam_groove(&tube, face, &plane, &segs, 2.0, 4.0, GrooveTool::Rectangular)?;
    assert_eq!(deep.shell_count(), 2);
    assert!(deep.validity().is_empty(), "{:?}", deep.validity());
    let expected = std::f64::consts::PI * (5.0 * 5.0 - 3.5 * 3.5) * 4.0;
    let after = volume(&deep)?;
    assert!((before - after - expected).abs() < 0.5, "{} vs {expected}", before - after);
    Ok(())
}

#[test]
fn hostile_grooves_are_refused() -> Result<()> {
    let (body, plane) = sleeve(10.0, 20.0)?;
    let face = wall_face(&body, &plane, 10.0)?;
    let segs = [Seg2::Line { a: Vec2::new(0.0, -2.0), b: Vec2::new(0.0, 2.0) }];
    // A path with no curve.
    assert!(cam_groove(&body, face, &plane, &[], 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A path with a point that is not a number.
    let bad = [Seg2::Line { a: Vec2::new(f64::NAN, -2.0), b: Vec2::new(0.0, 2.0) }];
    assert!(cam_groove(&body, face, &plane, &bad, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A path with no length.
    let nothing = [Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 0.0) }];
    assert!(cam_groove(&body, face, &plane, &nothing, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A depth that is not a length, and a width that is not a length.
    for (depth, width) in
        [(0.0, 4.0), (-1.0, 4.0), (f64::NAN, 4.0), (f64::INFINITY, 4.0), (1.0, 0.0), (1.0, -4.0), (1.0, f64::NAN), (1.0, f64::INFINITY)]
    {
        assert!(cam_groove(&body, face, &plane, &segs, depth, width, GrooveTool::Rectangular).is_err(), "{depth} {width}");
    }
    // A groove deeper than the wall is thick.
    assert!(cam_groove(&body, face, &plane, &segs, 10.0, 4.0, GrooveTool::Rectangular).is_err());
    assert!(cam_groove(&body, face, &plane, &segs, 20.0, 4.0, GrooveTool::Rectangular).is_err());
    // A round bottom has to be at least half as deep as it is wide.
    assert!(cam_groove(&body, face, &plane, &segs, 1.0, 4.0, GrooveTool::BallEnd).is_err());
    // A path that runs off the end of the cylinder.
    let off = [Seg2::Line { a: Vec2::new(0.0, 8.0), b: Vec2::new(0.0, 12.0) }];
    assert!(cam_groove(&body, face, &plane, &off, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A sketch plane that does not stand on the wall.
    let away = Plane::new(Vec3::new(0.0, 30.0, 10.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    assert!(cam_groove(&body, face, &away, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    let turned = Plane::new(Vec3::new(0.0, 10.0, 10.0), -Vec3::X, Vec3::Y).ok_or_else(|| setup("the plane"))?;
    assert!(cam_groove(&body, face, &turned, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A face that is not a wall.
    assert!(cam_groove(&body, 99, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A body that is a mesh has no wall to cut.
    let mesh = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y], &[[0, 1, 2]])?;
    assert!(cam_groove(&mesh, 0, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    // A body with no cylindrical wall at all.
    let block = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0))?;
    assert!(cam_groove(&block, 0, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    Ok(())
}

#[test]
fn a_groove_on_a_very_large_cylinder_is_cut() -> Result<()> {
    // The groove is a short slot along the axis, so the cutter stays small however big the
    // cylinder is.
    let (large, large_plane) = sleeve(10_000.0, 20.0)?;
    let segs = [Seg2::Line { a: Vec2::new(0.0, -5.0), b: Vec2::new(0.0, 5.0) }];
    let face = wall_face(&large, &large_plane, 10_000.0)?;
    let result = cam_groove(&large, face, &large_plane, &segs, 5.0, 20.0, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    let mesh = result.tessellate_with(0.5, true)?;
    assert!(!mesh.contains(Vec3::new(0.0, 9_997.5, 10.0)), "the slot is missing");
    assert!(mesh.contains(Vec3::new(0.0, 9_992.5, 10.0)), "the slot went too deep");
    Ok(())
}

#[test]
fn a_groove_on_a_very_small_cylinder_is_cut() -> Result<()> {
    // The groove is a ring, so its width runs along the axis and nothing has to fit across
    // the tiny radius. A body this fine is below what the kernel resolves when it is long
    // (a needle 20 mm long is refused), so the test uses a short one.
    let (small, small_plane) = sleeve(0.01, 2.0)?;
    let before = volume(&small)?;
    let turn = std::f64::consts::TAU * 0.01;
    let segs = [Seg2::Line { a: Vec2::new(-turn / 2.0, 0.0), b: Vec2::new(turn / 2.0, 0.0) }];
    let face = wall_face(&small, &small_plane, 0.01)?;
    let result = cam_groove(&small, face, &small_plane, &segs, 0.005, 0.05, GrooveTool::Rectangular)?;
    assert_eq!(result.shell_count(), 1);
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    // What the groove takes is the wall between the cylinder of radius 0.01 and the cylinder
    // of radius 0.005, over the ring's width.
    let expected = std::f64::consts::PI * (0.01 * 0.01 - 0.005 * 0.005) * 0.05;
    let after = volume(&result)?;
    assert!((before - after - expected).abs() < expected * 0.2, "{} vs {expected}", before - after);
    Ok(())
}

/// The cutter's depth goes into the wall whichever way the path is drawn.
///
/// The sweep turns the profile about the path, and the turn decided which way the depth
/// pointed: on a wall that a relief interrupts, a path drawn the other way round cut a groove
/// as deep as the cutter's overcut instead of the depth asked for.
#[test]
fn a_circumferential_path_is_cut_to_its_depth_however_it_is_drawn() -> Result<()> {
    let body = relieved(10.0, 20.0)?;
    let (_, frame) = cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 4.0))?;
    let face = face_at(&body, &frame, Vec3::new(0.0, 10.0, 4.0))?;
    let plane = Plane::new(Vec3::new(0.0, 10.0, 4.0), -Vec3::X, Vec3::Z).ok_or_else(|| setup("the plane"))?;
    // The path runs around the cylinder, drawn one way and the other.
    let turn = std::f64::consts::TAU * 10.0;
    for segs in [
        [Seg2::Line { a: Vec2::new(-turn / 2.0, -2.0), b: Vec2::new(turn / 2.0, 2.0) }],
        [Seg2::Line { a: Vec2::new(turn / 2.0, 2.0), b: Vec2::new(-turn / 2.0, -2.0) }],
    ] {
        let before = volume(&body)?;
        let result = cam_groove(&body, face, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular)?;
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        let after = volume(&result)?;
        println!("segs {segs:?}: removed {}", before - after);
        assert!(after < before, "{after} vs {before}");
        let mesh = result.tessellate_with(0.01, true)?;
        // The groove is a full depth deep, not as deep as the cutter's overcut.
        assert!(!mesh.contains(Vec3::new(0.0, 9.5, 4.0)), "the groove is not deep enough");
        assert!(mesh.contains(Vec3::new(0.0, 8.5, 4.0)), "the groove went too deep");
    }
    Ok(())
}
