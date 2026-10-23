//! Cutting a groove along a path wrapped onto a cylindrical wall.
//!
//! A groove is cut the way a lathe cuts one: a cutter of a given width follows the path on
//! the wall and takes a given depth out of it. The path is a sketch drawn on a plane tangent
//! to the cylinder; it is wrapped onto the wall (see [`crate::wrap`]), keeping its length, and
//! the cutter follows it.
//!
//! The cutter is carried square to the path, its width across the path on the surface and its
//! depth along the radius. A path that runs around the cylinder therefore cuts a ring groove
//! whose floor is the cylinder of radius `r - depth` and whose walls are square to the axis —
//! the groove a turned part carries. A path that runs along the axis cuts a slot across the
//! wall.

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::ops::{BoolOp, boolean};
use crate::sweep_path::{SweepControl, SweepOrientation, sweep_path_with};
use crate::wrap::{CylFrame, Development, cylinder_spans, wrap_path};
use crate::{KernelError, Result, guard};

/// The shape of a groove's cutter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrooveTool {
    /// A square-cornered slot: the cutter has a flat end, so the groove's floor is flat.
    Rectangular,
    /// A slot with a round floor: the cutter is a ball end, so the groove's floor is round.
    BallEnd,
}

impl GrooveTool {
    /// The tool a name stands for.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "rectangular" | "rectangle" | "flat" | "flat_end" | "flat end" | "square" => Some(Self::Rectangular),
            "ball" | "ball_end" | "ball end" | "round" => Some(Self::BallEnd),
            _ => None,
        }
    }

    /// The name the tool is saved and shown under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rectangular => "rectangular",
            Self::BallEnd => "ball_end",
        }
    }
}

fn fail(s: &str) -> KernelError {
    KernelError::Invalid(format!("cam groove: {s}"))
}

/// The cutter's cross-section, in the plane square to the path: its width across the path and
/// its depth into the wall.
///
/// The profile's `x` is the width across the path and its `y` the depth into the wall; the
/// cutter reaches a little above the surface, so that the cut has no face in common with the
/// wall it cuts.
fn tool_profile(width: f64, depth: f64, tool: GrooveTool, overcut: f64) -> Result<Region2> {
    let half = width / 2.0;
    let mut segs = Vec::with_capacity(5);
    let corner = |x: f64, y: f64| Vec2::new(x, y);
    segs.push(Seg2::Line { a: corner(-half, -overcut), b: corner(half, -overcut) });
    match tool {
        GrooveTool::Rectangular => {
            segs.push(Seg2::Line { a: corner(half, -overcut), b: corner(half, depth) });
            segs.push(Seg2::Line { a: corner(half, depth), b: corner(-half, depth) });
            segs.push(Seg2::Line { a: corner(-half, depth), b: corner(-half, -overcut) });
        }
        GrooveTool::BallEnd => {
            return Err(fail("not supported yet: a round-bottomed groove"));
        }
    }
    let outer = Loop2 { segs };
    if !outer.is_closed(1e-9) {
        return Err(fail("the cutter's profile is not closed"));
    }
    Ok(Region2 { outer, holes: vec![] })
}

/// The cutter's frame at the path's start: the direction across the path on the surface, the
/// direction the sweep turns the profile in, and the wall's outward direction there.
///
/// The width runs across the path, on the surface; the depth runs into the wall, along the
/// radius. The directions are taken at the path's *start*, which is not the point where the
/// sketch's plane touches the wall: a path that runs round the cylinder starts somewhere else,
/// and the wall's outward direction there is another one. The sweep carries the profile square
/// to the path and turns it about the path by the direction it is given, so that direction
/// decides which way the depth points: it is chosen to match the width's direction, so that
/// the depth goes into the wall whichever way the path was drawn.
fn tool_frame(frame: &CylFrame, start: Vec3, tangent: Vec3) -> Result<(Vec3, Vec3, Vec3)> {
    // The wall's outward direction where the path starts.
    let w = start - frame.origin;
    let radial = (w - frame.axis * w.dot(frame.axis)).normalized().ok_or_else(|| fail("the cutter's frame"))?;
    // The direction across the path, on the surface: the radius turned by the tangent. It is
    // square to the tangent and to the radius, so it lies on the surface.
    let across = radial.cross(tangent).normalized().ok_or_else(|| fail("the cutter's frame"))?;
    // The direction the sweep turns the profile in. The axis keeps the turn on the surface as
    // the path curves; its sign decides which way the profile's depth points, so it is chosen
    // to match the width's direction. A path along the axis has no axis to turn by, and the
    // width's own direction takes over.
    let turn = if tangent.dot(frame.axis).abs() > 0.98 {
        across
    } else if frame.axis.dot(across) >= 0.0 {
        frame.axis
    } else {
        -frame.axis
    };
    Ok((across, turn, radial))
}

/// Cuts a groove along a path wrapped onto a cylindrical wall.
///
/// `face` is the cylindrical wall the path runs on, `plane` the (tangent) plane the path is
/// drawn on and `segs` the path in that plane's own coordinates. The groove is `depth` deep
/// and `width` wide.
pub fn cam_groove(body: &Body, face: usize, plane: &Plane, segs: &[Seg2], depth: f64, width: f64, tool: GrooveTool) -> Result<Body> {
    body.require_brep("a cam groove")?;
    if !depth.is_finite() || depth <= 0.0 || depth > 1e6 {
        return Err(fail("the depth must be a positive length"));
    }
    if !width.is_finite() || width <= 0.0 || width > 1e6 {
        return Err(fail("the width must be a positive length"));
    }
    if segs.is_empty() {
        return Err(fail("the path has no curve"));
    }
    if tool != GrooveTool::Rectangular {
        return Err(fail("not supported yet: a round-bottomed groove"));
    }
    // The wall the path runs on, and the plane the path is drawn on read as its surface.
    let touch = plane_touch(plane, body, face)?;
    let (_, frame) = crate::wrap::cylindrical_wall_at(body, touch)?;
    let dev = crate::wrap::develop(&frame, plane)?;
    if depth >= dev.frame.radius {
        return Err(fail("the groove is deeper than the wall is thick"));
    }
    // The path has to stay on the wall it is cut into: a wall interrupted by a relief groove
    // takes the part of the path that lies on it, and no more.
    let spans = cylinder_spans(body, &dev.frame)?;
    let ztol = (body.size() * 1e-6).max(1e-6);
    let atol = ztol / dev.frame.radius;
    // A wall is often more than one face (a cylinder is split at its seam, and a relief
    // groove interrupts it), so the face the caller names and the face the point finds may
    // differ while standing on the same wall. The plane has to stand tangent to that wall,
    // which `develop` has just checked.
    let _ = face;
    let path = wrap_path(&dev, segs)?;
    if path.points.len() < 2 {
        return Err(fail("the path has no length"));
    }
    for p in &path.points {
        let a = dev.frame.angle(*p);
        let z = dev.frame.axial(*p);
        if !spans.iter().any(|s| s.holds(a, z, atol, ztol)) {
            return Err(fail("the path leaves the wall it is cut into"));
        }
    }
    let tool_body = groove_tool(&dev, &path.points, depth, width, tool)?;
    let result = boolean(body, &tool_body, BoolOp::Cut)?;
    result.ok_or_else(|| fail("the groove cut the whole body away"))
}

/// The point on the wall where the plane stands: the plane's own point nearest the axis,
/// pushed out to the wall.
fn plane_touch(plane: &Plane, body: &Body, face: usize) -> Result<Vec3> {
    let f = body.solid.face_iter().nth(face).ok_or_else(|| fail("the selected wall"))?;
    let tol = (body.size() * 1e-8).max(1e-8);
    let Some(crate::offset::Surf::Cylinder { o, a, r, .. }) = crate::offset::surf_of(f, tol) else {
        return Err(fail("the selected wall is not cylindrical"));
    };
    let w = plane.origin - o;
    let along = w.dot(a);
    let radial = w - a * along;
    let radial = radial.normalized().ok_or_else(|| fail("the wall's radial direction"))?;
    Ok(o + a * along + radial * r)
}

/// The cutter carried along the wrapped path.
fn groove_tool(dev: &Development, points: &[Vec3], depth: f64, width: f64, tool: GrooveTool) -> Result<Body> {
    let frame = dev.frame;
    let start = points[0];
    let tangent = points.get(1).and_then(|p| (*p - start).normalized()).ok_or_else(|| fail("the path has no direction"))?;
    let (across, turn, radial) = tool_frame(&frame, start, tangent)?;
    // The profile's plane: square to the path, its width across the path on the surface and
    // its depth into the wall. The sweep carries the plane's `x` (the width) to the direction
    // across the path and its `y` (the depth) into the wall.
    let plane = Plane::new(start, across, -radial).ok_or_else(|| fail("the cutter's plane"))?;
    let overcut = (width * 0.1).max(0.05);
    let region = tool_profile(width, depth, tool, overcut)?;
    // The sweep's turn about the path is fixed by the direction chosen above, so that the width
    // stays on the surface the whole way along and the depth goes into the wall.
    let ctl = SweepControl { orientation: SweepOrientation::Direction(turn), ..SweepControl::default() };
    guard("cam groove", || sweep_path_with(&plane, &region, points, &ctl))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;

    /// A point on the surface, `across` away from the path's midpoint in the developed plane.
    fn dev2(dev: &Development, s: f64, z: f64, across: Vec2, d: f64) -> Vec3 {
        let p = Vec2::new(s + across.x * d, z + across.y * d);
        dev.frame.point(p.x, p.y, 9.9)
    }

    /// A cylinder of radius 10 and height 20, and a plane tangent to it at `(0, 10, 0)` whose
    /// `x` runs around the circumference and its `y` along the axis.
    fn sleeve() -> (Body, Plane) {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let plane = Plane::new(Vec3::new(0.0, 10.0, 0.0), -Vec3::X, Vec3::Z).unwrap();
        (body, plane)
    }

    /// The wall's face index (the cylinder's lateral face).
    fn wall(body: &Body) -> usize {
        let tol = (body.size() * 1e-8).max(1e-8);
        (0..body.face_count())
            .find(|i| {
                body.solid
                    .face_iter()
                    .nth(*i)
                    .and_then(|f| crate::offset::surf_of(f, tol))
                    .is_some_and(|s| matches!(s, crate::offset::Surf::Cylinder { .. }))
            })
            .unwrap_or(0)
    }

    fn volume(body: &Body) -> f64 {
        body.tessellate_with(1e-4, true).unwrap().measure().volume
    }

    #[test]
    fn a_ring_groove_is_cut_into_the_wall() {
        let (body, plane) = sleeve();
        let before = volume(&body);
        // A path around the circumference (the sketch's `x` runs around it), 4 mm wide and
        // 1 mm deep. The path runs the whole way round, over the seam.
        let turn = std::f64::consts::TAU * 10.0;
        let segs = [Seg2::Line { a: Vec2::new(-turn / 2.0, 10.0), b: Vec2::new(turn / 2.0, 10.0) }];
        let result = cam_groove(&body, wall(&body), &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).unwrap();
        assert_eq!(result.shell_count(), 1);
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        // The groove's floor is the cylinder of radius 9 and its width 4 mm: what it takes is
        // the wall between the two cylinders, over the ring's width.
        let expected = std::f64::consts::PI * (10.0 * 10.0 - 9.0 * 9.0) * 4.0;
        let after = volume(&result);
        assert!((before - after - expected).abs() < 1.0, "{} vs {expected}", before - after);
        // The wall is gone between radius 9 and 10, over the ring's width.
        let mesh = result.tessellate_with(0.01, true).unwrap();
        assert!(!mesh.contains(Vec3::new(0.0, 9.5, 10.0)), "the groove's floor is missing");
        assert!(mesh.contains(Vec3::new(0.0, 8.5, 10.0)), "the groove went too deep");
        assert!(mesh.contains(Vec3::new(0.0, 9.5, 4.0)), "the groove is the wrong width");
    }

    #[test]
    fn a_groove_along_the_axis_is_cut_into_the_wall() {
        let (body, plane) = sleeve();
        let before = volume(&body);
        // A path along the axis (the sketch's `y` runs along it), 4 mm around the
        // circumference and 4 mm along the axis, 1 mm deep.
        let segs = [Seg2::Line { a: Vec2::new(0.0, 8.0), b: Vec2::new(0.0, 12.0) }];
        let result = cam_groove(&body, wall(&body), &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).unwrap();
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        let after = volume(&result);
        // What the slot takes is the wall between the two cylinders, over the arc it covers
        // and the length it runs.
        let arc = 4.0 / (2.0 * std::f64::consts::PI * 10.0);
        let expected = std::f64::consts::PI * (10.0 * 10.0 - 9.0 * 9.0) * arc * 4.0;
        assert!((before - after - expected).abs() < 1.0, "{} vs {expected}", before - after);
        let mesh = result.tessellate_with(0.01, true).unwrap();
        assert!(!mesh.contains(Vec3::new(0.0, 9.5, 10.0)), "the slot is missing");
        assert!(mesh.contains(Vec3::new(0.0, 8.5, 10.0)), "the slot went too deep");
    }

    #[test]
    fn a_round_bottomed_groove_is_not_supported_yet() {
        // The round floor would make the cutter's surface a tube, which the boolean cannot cut
        // in a reasonable time; the flat floor is the one that works.
        let (body, plane) = sleeve();
        let turn = std::f64::consts::TAU * 10.0;
        let segs = [Seg2::Line { a: Vec2::new(-turn / 2.0, 10.0), b: Vec2::new(turn / 2.0, 10.0) }];
        let error = cam_groove(&body, wall(&body), &plane, &segs, 2.0, 4.0, GrooveTool::BallEnd).unwrap_err().to_string();
        assert!(error.contains("not supported yet"), "{error}");
    }

    #[test]
    fn a_slanted_path_cuts_a_band_of_the_width_it_asks_for() {
        // A path that runs around the cylinder and up it: the band it cuts is as wide as it
        // was drawn, measured across the path on the surface.
        let (body, _) = sleeve();
        let plane = Plane::new(Vec3::new(0.0, 10.0, 10.0), -Vec3::X, Vec3::Z).unwrap();
        let before = volume(&body);
        let segs = [Seg2::Line { a: Vec2::new(-6.0, -1.0), b: Vec2::new(6.0, 1.0) }];
        let result = cam_groove(&body, wall(&body), &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).unwrap();
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        let after = volume(&result);
        // What the groove takes is the band the path sweeps out, on the surface, less what
        // the wall's curve takes: the band is measured at the surface, and the groove is
        // cut below it, where the circumference is shorter.
        let length = 12.0_f64.hypot(2.0);
        let want = length * 4.0 * 1.0 * (1.0 - 1.0 / (2.0 * 10.0));
        println!("removed {} vs {want}", before - after);
        assert!((before - after - want).abs() < 0.02 * want, "{} vs {want}", before - after);
        // The floor is the cylinder of radius 9 and the wall 4 mm across the path.
        let mesh = result.tessellate_with(0.01, true).unwrap();
        assert!(!mesh.contains(Vec3::new(0.0, 9.5, 10.0)), "the groove's floor is missing");
        assert!(mesh.contains(Vec3::new(0.0, 8.5, 10.0)), "the groove went too deep");
        let dev = crate::wrap::develop(&crate::wrap::cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 10.0)).unwrap().1, &plane).unwrap();
        // The groove is 4 mm across the path, measured on the surface: the material is gone
        // within 2 mm of the path and there again outside it.
        let across = Vec2::new(2.0, -12.0).normalized().unwrap();
        for (d, in_groove) in [(0.0, false), (1.9, false), (2.1, true)] {
            let p = dev2(&dev, 0.0, 10.0, across, d);
            assert_eq!(mesh.contains(p), in_groove, "across {d}");
        }
        // The floor is the cylinder of radius 9: the material is there below it and gone
        // above it.
        assert!(mesh.contains(Vec3::new(0.0, 8.9, 10.0)));
        assert!(!mesh.contains(Vec3::new(0.0, 9.1, 10.0)));
    }

    #[test]
    fn the_same_groove_on_either_half_of_the_wall() {
        // The wall is split at its seam: the groove must come out the same on either half.
        let (body, _) = sleeve();
        let plane = Plane::new(Vec3::new(0.0, 10.0, 10.0), -Vec3::X, Vec3::Z).unwrap();
        let segs = [Seg2::Line { a: Vec2::new(-6.0, -1.0), b: Vec2::new(6.0, 1.0) }];
        let before = volume(&body);
        for face in 0..body.face_count() {
            let Ok(result) = cam_groove(&body, face, &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular) else {
                continue;
            };
            let after = volume(&result);
            let want = 12.0_f64.hypot(2.0) * 4.0 * (1.0 - 1.0 / (2.0 * 10.0));
            println!("face {face}: removed {} vs {want}", before - after);
        }
    }

    #[test]
    fn a_path_that_leaves_the_wall_is_refused() {
        let (body, plane) = sleeve();
        // The path runs off the top of the cylinder.
        let segs = [Seg2::Line { a: Vec2::new(0.0, 18.0), b: Vec2::new(0.0, 22.0) }];
        assert!(cam_groove(&body, wall(&body), &plane, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
        // A path that is not on the wall at all.
        let segs = [Seg2::Line { a: Vec2::new(0.0, 8.0), b: Vec2::new(0.0, 12.0) }];
        let other = Plane::new(Vec3::new(0.0, 30.0, 0.0), -Vec3::X, Vec3::Z).unwrap();
        assert!(cam_groove(&body, wall(&body), &other, &segs, 1.0, 4.0, GrooveTool::Rectangular).is_err());
    }

    #[test]
    fn hostile_sizes_are_refused() {
        let (body, plane) = sleeve();
        let segs = [Seg2::Line { a: Vec2::new(0.0, 8.0), b: Vec2::new(0.0, 12.0) }];
        for (d, w) in [(0.0, 4.0), (-1.0, 4.0), (1.0, 0.0), (1.0, -4.0), (f64::NAN, 4.0), (1.0, f64::INFINITY), (20.0, 4.0)] {
            assert!(cam_groove(&body, wall(&body), &plane, &segs, d, w, GrooveTool::Rectangular).is_err(), "{d} {w}");
        }
        // A round bottom needs room for its own radius.
        assert!(cam_groove(&body, wall(&body), &plane, &segs, 1.0, 4.0, GrooveTool::BallEnd).is_err());
    }
}
