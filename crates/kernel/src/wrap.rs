//! Wrapping sketch geometry onto a cylinder: the development of its surface.
//!
//! A cylinder's surface unfolds into a plane without stretching. A point at an arc length `s`
//! around the circumference and a height `z` along the axis is the point `(s, z)` of the
//! developed plane, and the developed plane's own lengths are the surface's lengths: a line
//! stays a line of the same length, an arc an arc of the same length, whatever the radius.
//! Wrapping is therefore the reverse of unfolding, and a wrapped curve keeps the length it
//! was drawn with.
//!
//! The development is written in the *unwrapped* plane's coordinates, so a sketch drawn on a
//! plane tangent to the cylinder is read in the surface's own coordinates: the sketch's `x`
//! runs around the circumference, its `y` along the axis. The map is a rotation of the plane
//! (the plane's normal and the cylinder's radial direction agree), so it keeps every length
//! and every angle.

use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::{KernelError, Result};

/// The most points a wrapped path may have.
const MAX_PATH_POINTS: usize = 20_000;
/// The chord error a wrapped path is sampled to (mm).
const PATH_TOLERANCE: f64 = 5e-4;

fn fail(s: &str) -> KernelError {
    KernelError::Invalid(format!("cylindrical wrap: {s}"))
}

/// A cylinder's frame: its axis, its radius, and the two directions on its surface.
#[derive(Clone, Copy, Debug)]
pub struct CylFrame {
    /// A point on the axis.
    pub origin: Vec3,
    /// The axis direction (unit).
    pub axis: Vec3,
    /// The radius.
    pub radius: f64,
    /// The outward radial direction (unit), perpendicular to the axis.
    pub radial: Vec3,
    /// The direction along the circumference (unit), `axis × radial`.
    pub tangent: Vec3,
}

impl CylFrame {
    /// A frame for a cylinder. `radial` must be perpendicular to `axis`; it points away from
    /// the axis, so the surface's outward normal.
    pub fn new(origin: Vec3, axis: Vec3, radius: f64, radial: Vec3) -> Option<Self> {
        let axis = axis.normalized()?;
        if !(radius.is_finite() && radius > 0.0) {
            return None;
        }
        let radial = radial.normalized()?;
        if radial.dot(axis).abs() > 1e-9 {
            return None;
        }
        let tangent = axis.cross(radial).normalized()?;
        Some(Self { origin, axis, radius, radial, tangent })
    }

    /// The point at an arc length `s` around the circumference, a height `z` along the axis and
    /// a distance `radius` from the axis.
    pub fn point(&self, s: f64, z: f64, radius: f64) -> Vec3 {
        let a = s / self.radius;
        self.origin + self.axis * z + (self.radial * a.cos() + self.tangent * a.sin()) * radius
    }

    /// The point on the surface.
    pub fn on_surface(&self, s: f64, z: f64) -> Vec3 {
        self.point(s, z, self.radius)
    }

    /// The height of a point along the axis.
    pub fn axial(&self, p: Vec3) -> f64 {
        (p - self.origin).dot(self.axis)
    }

    /// The angle of a point about the axis, in radians, measured from the radial direction.
    pub fn angle(&self, p: Vec3) -> f64 {
        let w = p - self.origin;
        let w = w - self.axis * w.dot(self.axis);
        w.dot(self.tangent).atan2(w.dot(self.radial))
    }
}

/// A plane read as the unwrapped surface of a cylinder.
#[derive(Clone, Copy, Debug)]
pub struct Development {
    /// The cylinder's frame, its radial direction turned to the plane's normal: the developed
    /// coordinates run the way the surface does.
    pub frame: CylFrame,
    /// The arc length of the plane's origin.
    pub s0: f64,
    /// The height of the plane's origin.
    pub z0: f64,
    /// The plane's `x` direction in (arc length, height).
    pub ex: Vec2,
    /// The plane's `y` direction in (arc length, height).
    pub ey: Vec2,
}

impl Development {
    /// The developed coordinates of a point in the plane.
    pub fn of(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.s0 + self.ex.x * p.x + self.ey.x * p.y, self.z0 + self.ex.y * p.x + self.ey.y * p.y)
    }

    /// Whether the map keeps lengths. It does when the plane stands square to the cylinder's
    /// axis, which is what makes the plane tangent to it.
    pub fn is_isometry(&self) -> bool {
        let a = self.ex.x * self.ex.x + self.ex.y * self.ex.y;
        let b = self.ey.x * self.ey.x + self.ey.y * self.ey.y;
        let c = self.ex.x * self.ey.x + self.ex.y * self.ey.y;
        (a - 1.0).abs() < 1e-9 && (b - 1.0).abs() < 1e-9 && c.abs() < 1e-9
    }
}

/// Reads a plane as the unwrapped surface of a cylinder.
///
/// The plane must stand tangent to the cylinder: its normal is the cylinder's radial
/// direction and it is one radius away from the axis. The frame's radial direction is taken
/// from the plane, so that the developed coordinates run the way the surface does.
pub fn develop(frame: &CylFrame, plane: &Plane) -> Result<Development> {
    if !frame.radius.is_finite() || frame.radius <= 0.0 {
        return Err(fail("the cylinder has no radius"));
    }
    if !frame.origin.is_finite() {
        return Err(fail("the cylinder's frame has no origin"));
    }
    let n = plane.normal();
    if !n.is_finite() || n.len() < 0.5 {
        return Err(fail("the sketch plane has no normal"));
    }
    if n.dot(frame.axis).abs() > 1e-7 {
        return Err(fail("the sketch plane must stand tangent to the cylinder: its normal must be square to the cylinder's axis"));
    }
    let w = plane.origin - frame.origin;
    let signed = w.dot(n);
    if (signed.abs() - frame.radius).abs() > frame.radius * 1e-6 + 1e-9 {
        return Err(fail("the sketch plane must stand tangent to the cylinder: it has to touch it, one radius from the axis"));
    }
    // The plane's origin lies on the outside of the cylinder when it is on the far side of the
    // axis from the plane's normal.
    let outward = if signed < 0.0 { -n } else { n };
    let axis = frame.axis;
    let tangent = axis.cross(outward).normalized().ok_or_else(|| fail("the cylinder's frame"))?;
    let dev = Development {
        frame: CylFrame { origin: frame.origin, axis, radius: frame.radius, radial: outward, tangent },
        s0: w.dot(tangent),
        z0: w.dot(axis),
        ex: Vec2::new(plane.x.dot(tangent), plane.x.dot(axis)),
        ey: Vec2::new(plane.y.dot(tangent), plane.y.dot(axis)),
    };
    if !dev.is_isometry() {
        return Err(fail("the sketch plane is not square to the cylinder's axis"));
    }
    Ok(dev)
}

/// A path of sketch segments, wrapped onto a cylinder.
#[derive(Clone, Debug)]
pub struct WrappedPath {
    /// The path's points on the surface.
    pub points: Vec<Vec3>,
    /// The developed coordinates of the points.
    pub developed: Vec<Vec2>,
    /// The path's length along the surface.
    pub length: f64,
}

/// Wraps a path of sketch segments onto the cylinder.
///
/// The path is sampled finely enough that the wrapped polyline stands within
/// [`PATH_TOLERANCE`] of the surface, and the developed coordinates of the points are kept so
/// that a caller can measure the path in the surface's own coordinates.
pub fn wrap_path(dev: &Development, segs: &[Seg2]) -> Result<WrappedPath> {
    if segs.is_empty() || segs.len() > 1024 {
        return Err(fail("a wrapped path needs 1…1024 segments"));
    }
    let frame = dev.frame;
    let mut developed = Vec::new();
    let mut length = 0.0;
    // The surface curves away from the chord by the step's square over eight times the
    // radius, so the step is kept short enough for that to stay under the tolerance.
    let max = (8.0 * frame.radius * PATH_TOLERANCE).sqrt().max(1e-6);
    for seg in segs {
        if !seg.start().is_finite() || !seg.end().is_finite() {
            return Err(fail("the path has a bad point"));
        }
        let poly = seg.polyline(PATH_TOLERANCE);
        if poly.len() < 2 {
            return Err(fail("the path has a segment without length"));
        }
        // The curve's own length, which the wrapping keeps.
        length += seg.length();
        // How many points the segment would add, counted before any of them is made: a path
        // long enough to need more than the whole budget is refused, not sampled.
        let wanted = poly
            .windows(2)
            .map(|w| (w[0].dist(w[1]) / max).ceil().max(1.0) as usize)
            .fold(0usize, |total, n| total.saturating_add(n))
            .saturating_add(1);
        if developed.len().saturating_add(wanted) > MAX_PATH_POINTS {
            return Err(fail("the path is too long to wrap"));
        }
        for pair in poly.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let step = a.dist(b);
            let n = (step / max).ceil().max(1.0);
            for j in 0..(n as usize) {
                let t = j as f64 / n;
                developed.push(dev.of(a.lerp(b, t)));
            }
        }
        if let Some(last) = poly.last() {
            developed.push(dev.of(*last));
        }
    }
    let points: Vec<Vec3> = developed.iter().map(|p| frame.on_surface(p.x, p.y)).collect();
    Ok(WrappedPath { points, developed, length })
}

/// The face of a body that carries a point on a cylinder, with the cylinder's frame.
///
/// The face must be an exterior cylindrical wall: a groove is cut into the outside of a
/// cylinder, never into a hole. A cylinder's wall is often split into a few faces (at its
/// seam, or where a relief groove interrupts it), so the face nearest the point is the one
/// taken.
pub fn cylindrical_wall_at(body: &Body, at: Vec3) -> Result<(usize, CylFrame)> {
    if !at.is_finite() {
        return Err(fail("the selected point is not finite"));
    }
    body.require_brep("a cam groove")?;
    let tol = (body.size() * 1e-8).max(1e-8);
    let mut found: Option<(f64, usize, CylFrame)> = None;
    for (index, face) in body.solid.face_iter().enumerate() {
        if index >= 50_000 {
            return Err(fail("the target search exceeds the face work limit"));
        }
        let Some(crate::offset::Surf::Cylinder { o, a, r, convex: true }) = crate::offset::surf_of(face, tol) else {
            continue;
        };
        let w = at - o;
        let along = w.dot(a);
        let radial = w - a * along;
        let off = (radial.len() - r).abs();
        if off > (body.size() * 1e-4).max(1e-4) {
            continue;
        }
        if found.as_ref().is_some_and(|(best, _, _)| off >= *best) {
            continue;
        }
        let outward = radial.normalized().ok_or_else(|| fail("the wall's radial direction"))?;
        let frame = CylFrame::new(o, a, r, outward).ok_or_else(|| fail("the wall's frame"))?;
        found = Some((off, index, frame));
    }
    found.map(|(_, index, frame)| (index, frame)).ok_or_else(|| fail("the selected point is not on an exterior cylindrical wall"))
}

/// How far a face of a cylinder reaches around it and along its axis.
#[derive(Clone, Copy, Debug)]
pub struct WallSpan {
    /// The face's index in the body.
    pub face: usize,
    /// The angle the face starts at.
    pub start: f64,
    /// How far it reaches around the cylinder, in radians.
    pub sweep: f64,
    /// The lowest height it reaches.
    pub zlo: f64,
    /// The highest height it reaches.
    pub zhi: f64,
}

impl WallSpan {
    /// Whether a point of the cylinder's surface lies on this face.
    pub fn holds(&self, angle: f64, z: f64, atol: f64, ztol: f64) -> bool {
        if z < self.zlo - ztol || z > self.zhi + ztol {
            return false;
        }
        let d = unwrap(angle - self.start, 0.0);
        d >= -atol && d <= self.sweep + atol
    }
}

/// The faces of a body that lie on a cylinder, with how far each of them reaches.
///
/// The wall of a cylinder is often more than one face: it is split at its seam, and a relief
/// groove or a shoulder interrupts it. Each face is measured on its own, so a path that runs
/// into a relief groove is refused while one that crosses the seam is not.
pub fn cylinder_spans(body: &Body, frame: &CylFrame) -> Result<Vec<WallSpan>> {
    body.require_brep("a cam groove")?;
    let tol = (body.size() * 1e-8).max(1e-8);
    let mut spans = Vec::new();
    for (index, face) in body.solid.face_iter().enumerate() {
        if index >= 50_000 {
            return Err(fail("the target search exceeds the face work limit"));
        }
        let Some(crate::offset::Surf::Cylinder { o, a, r, convex: true }) = crate::offset::surf_of(face, tol) else {
            continue;
        };
        // The cylinder's origin is only a point on its axis: two faces of the same wall (a
        // cylinder split by a relief groove) may name different points of it, so it is the
        // axis itself that has to agree.
        let along = (frame.origin - o).dot(a);
        if (o + a * along).dist(frame.origin) > tol * 10.0 || a.dot(frame.axis).abs() < 1.0 - 1e-9 || (r - frame.radius).abs() > tol * 10.0 {
            continue;
        }
        let (start, sweep, zlo, zhi) = face_angles(body, index, frame)?;
        spans.push(WallSpan { face: index, start, sweep, zlo, zhi });
    }
    if spans.is_empty() {
        return Err(fail("the wall has no face"));
    }
    Ok(spans)
}

/// The angles a face reaches around the cylinder, and the heights it reaches.
///
/// The face's angles are the ones its boundary runs through; the span is the shortest way
/// round that holds all of them, so a face that reaches past the seam is measured as the
/// piece it is.
fn face_angles(body: &Body, index: usize, frame: &CylFrame) -> Result<(f64, f64, f64, f64)> {
    let face = body.solid.face_iter().nth(index).ok_or_else(|| fail("the selected face"))?;
    let mut angles: Vec<f64> = Vec::new();
    let mut zlo = f64::INFINITY;
    let mut zhi = f64::NEG_INFINITY;
    let mut count = 0usize;
    for edge in face.edge_iter() {
        let mut points = Vec::new();
        match edge.curve() {
            truck_modeling::Curve::Line(_) => {
                points.push(crate::body::from_p3(edge.front().point()));
                points.push(crate::body::from_p3(edge.back().point()));
            }
            truck_modeling::Curve::NurbsCurve(curve) => {
                let at = curve.get_closure();
                for i in 0..=16 {
                    points.push(crate::body::from_p3(at(i as f64 / 16.0)));
                }
            }
            truck_modeling::Curve::BSplineCurve(curve) => {
                let at = curve.get_closure();
                for i in 0..=16 {
                    points.push(crate::body::from_p3(at(i as f64 / 16.0)));
                }
            }
            truck_modeling::Curve::IntersectionCurve(_) => {
                return Err(fail("the face has an edge this cannot measure"));
            }
        }
        for p in points {
            count = count.saturating_add(1);
            if count > 100_000 {
                return Err(fail("the face has too many points to measure"));
            }
            if !p.is_finite() {
                return Err(fail("the face has a bad point"));
            }
            angles.push(frame.angle(p));
            let z = frame.axial(p);
            zlo = zlo.min(z);
            zhi = zhi.max(z);
        }
    }
    if angles.is_empty() || !zlo.is_finite() {
        return Err(fail("the face has no measurable boundary"));
    }
    angles.sort_by(f64::total_cmp);
    // The largest gap between the angles is the way out of the face; the span is the rest.
    let mut gap = 0.0;
    let mut after = angles[0];
    for pair in angles.windows(2) {
        let g = pair[1] - pair[0];
        if g > gap {
            gap = g;
            after = pair[1];
        }
    }
    let wrap = angles[0] + std::f64::consts::TAU - angles[angles.len() - 1];
    if wrap > gap {
        gap = wrap;
        after = angles[0];
    }
    let sweep = (std::f64::consts::TAU - gap).max(0.0);
    Ok((after, sweep, zlo, zhi))
}

/// The exact point on a cylindrical wall that a picked point stands for.
///
/// A picked point lies on a tessellated chord, so it is off the wall: the point keeps its
/// axial and angular position and is pushed out onto the wall.
pub fn wall_anchor(body: &Body, face: usize, at: Vec3) -> Result<Vec3> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    body.require_brep("a cam groove")?;
    let f = body.solid.face_iter().nth(face).ok_or_else(|| fail("selected face"))?;
    let tol = (body.size() * 1e-8).max(1e-8);
    let Some(crate::offset::Surf::Cylinder { o, a, r, convex: true }) = crate::offset::surf_of(f, tol) else {
        return Err(fail("selected face is not an exterior cylinder"));
    };
    let w = at - o;
    let radial = w - a * w.dot(a);
    if (radial.len() - r).abs() > (body.size() * 1e-3).max(1e-3).min(r * 0.05) {
        return Err(fail("selected point is too far from cylinder"));
    }
    Ok(o + a * w.dot(a) + radial.normalized().ok_or_else(|| fail("radial point"))? * r)
}

/// The angle `a` brought into the half turn around `around`.
pub fn unwrap(a: f64, around: f64) -> f64 {
    around + (a - around + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// The exterior wall a sketch plane stands tangent to, when the body has exactly one.
///
/// A plane that touches a cylinder is a tangent plane to it, and the plane's normal is the
/// cylinder's radial direction. The wall is the face of that cylinder the plane touches.
pub fn tangent_wall(body: &Body, plane: &Plane) -> Result<Option<(usize, CylFrame)>> {
    body.require_brep("a cam groove")?;
    let tol = (body.size() * 1e-8).max(1e-8);
    let n = plane.normal();
    let mut found: Option<(usize, CylFrame)> = None;
    for (index, face) in body.solid.face_iter().enumerate() {
        if index >= 50_000 {
            return Err(fail("the target search exceeds the face work limit"));
        }
        let Some(crate::offset::Surf::Cylinder { o, a, r, convex: true }) = crate::offset::surf_of(face, tol) else {
            continue;
        };
        if n.dot(a).abs() > 1e-7 {
            continue;
        }
        if ((plane.origin - o).dot(n)).abs() - r > r * 1e-6 + 1e-9 {
            continue;
        }
        let radial = n * if (plane.origin - o).dot(n) > 0.0 { 1.0 } else { -1.0 };
        let frame = CylFrame::new(o, a, r, radial).ok_or_else(|| fail("the wall's frame"))?;
        // A wall is often more than one face (a cylinder is split at its seam), so faces of
        // the same cylinder are the same wall.
        if let Some((_, current)) = found {
            if !same_cylinder(&current, &frame, tol * 10.0) {
                return Err(fail("the sketch plane stands tangent to more than one wall"));
            }
            continue;
        }
        found = Some((index, frame));
    }
    Ok(found)
}

/// Whether two frames name the same cylinder: the same axis line and the same radius.
///
/// A cylinder's origin is only a point on its axis, so two faces of the same wall (a
/// cylinder split at its seam, or by a relief groove) may name different points of it.
fn same_cylinder(a: &CylFrame, b: &CylFrame, tol: f64) -> bool {
    let along = (a.origin - b.origin).dot(b.axis);
    (b.origin + b.axis * along).dist(a.origin) <= tol && a.axis.dot(b.axis).abs() > 1.0 - 1e-9 && (a.radius - b.radius).abs() <= tol
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec2;

    fn frame() -> CylFrame {
        CylFrame::new(Vec3::ZERO, Vec3::Z, 10.0, Vec3::Y).unwrap()
    }

    /// A plane tangent to the cylinder at the point `(0, 10, 0)`, its `x` running around the
    /// circumference and its `y` along the axis.
    fn tangent_plane() -> Plane {
        Plane::new(Vec3::new(0.0, 10.0, 0.0), -Vec3::X, Vec3::Z).unwrap()
    }

    /// The length of a straight developed run, wrapped and measured on the surface.
    fn wrapped_length(dev: &Development, from: Vec2, to: Vec2, steps: usize) -> f64 {
        let pts: Vec<Vec3> = (0..=steps)
            .map(|i| {
                let p = dev.of(from.lerp(to, i as f64 / steps as f64));
                dev.frame.on_surface(p.x, p.y)
            })
            .collect();
        pts.windows(2).map(|w| w[0].dist(w[1])).sum()
    }

    #[test]
    fn a_wrapped_curve_keeps_its_length() {
        let f = frame();
        let dev = develop(&f, &tangent_plane()).unwrap();
        assert!(dev.is_isometry());
        // A line along the circumference wraps to an arc of the same length: the arc's length
        // is the radius times the angle it turns through, and the angle is the arc length
        // divided by the radius.
        let line = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(6.0, 0.0) };
        let path = wrap_path(&dev, &[line]).unwrap();
        // The developed curve is the sketch's own curve, so it has the same length.
        assert!((path.length - line.length()).abs() < 1e-12, "{}", path.length);
        let first = dev.frame.angle(path.points[0]);
        let last = dev.frame.angle(path.points[path.points.len() - 1]);
        let turned = unwrap(last - first, 0.0);
        assert!((f.radius * turned.abs() - 6.0).abs() < 1e-9, "{}", f.radius * turned.abs());
        // Measured on the surface, the wrapped run is as long as the sketch's line.
        assert!((wrapped_length(&dev, Vec2::new(0.0, 0.0), Vec2::new(6.0, 0.0), 20_000) - 6.0).abs() < 1e-5);
        // A line along the axis wraps to a straight line of the same length.
        let along = Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 4.0) };
        let path = wrap_path(&dev, &[along]).unwrap();
        assert!((path.length - along.length()).abs() < 1e-12);
        assert!((path.points[0].dist(path.points[path.points.len() - 1]) - 4.0).abs() < 1e-9);
        // A line across the two keeps its length too: it wraps to a helix.
        assert!((wrapped_length(&dev, Vec2::new(0.0, 0.0), Vec2::new(3.0, 4.0), 20_000) - 5.0).abs() < 1e-5);
    }

    #[test]
    fn a_wrapped_arc_keeps_its_length() {
        let f = frame();
        let dev = develop(&f, &tangent_plane()).unwrap();
        let arc = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 3.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 };
        let path = wrap_path(&dev, &[arc]).unwrap();
        assert!((path.length - arc.length()).abs() < 1e-12, "{}", path.length);
        // The arc's length, measured on the surface, is the arc's own.
        let steps = 20_000;
        let mut previous = dev.of(arc.start());
        let mut length = 0.0;
        for i in 1..=steps {
            let p = dev.of(arc.point_at(i as f64 / steps as f64));
            length += dev.frame.on_surface(previous.x, previous.y).dist(dev.frame.on_surface(p.x, p.y));
            previous = p;
        }
        assert!((length - arc.length()).abs() < 1e-5, "{length} vs {}", arc.length());
    }

    #[test]
    fn the_wrapped_points_lie_on_the_cylinder() {
        let f = frame();
        let dev = develop(&f, &tangent_plane()).unwrap();
        let segs = [
            Seg2::Line { a: Vec2::new(-4.0, -3.0), b: Vec2::new(6.0, -3.0) },
            Seg2::Arc { center: Vec2::new(6.0, 0.0), radius: 3.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::FRAC_PI_2 },
        ];
        let path = wrap_path(&dev, &segs).unwrap();
        for p in &path.points {
            let w = *p - f.origin;
            let radial = w - f.axis * w.dot(f.axis);
            assert!((radial.len() - 10.0).abs() < 1e-9, "{}", radial.len());
        }
    }

    #[test]
    fn a_seam_crossing_path_keeps_its_length() {
        // The path runs past the seam, where the angle wraps from pi to -pi.
        let f = frame();
        let dev = develop(&f, &tangent_plane()).unwrap();
        let half = std::f64::consts::PI * 10.0;
        let line = Seg2::Line { a: Vec2::new(half - 2.0, 0.0), b: Vec2::new(half + 2.0, 0.0) };
        let path = wrap_path(&dev, &[line]).unwrap();
        assert!((path.length - 4.0).abs() < 1e-12, "{}", path.length);
        // The points run on past the seam instead of jumping back.
        let angles: Vec<f64> = path.points.iter().map(|p| dev.frame.angle(*p)).collect();
        let last = angles[angles.len() - 1];
        assert!(last > 2.0 || last < -2.0, "{last}");
        // The seam costs the path nothing: the run round the seam is as long as the line.
        let turned = unwrap(angles[angles.len() - 1] - angles[0], 0.0);
        assert!((f.radius * turned.abs() - 4.0).abs() < 1e-9, "{}", f.radius * turned.abs());
    }

    #[test]
    fn a_plane_that_is_not_tangent_is_refused() {
        let f = frame();
        assert!(develop(&f, &Plane::XY).is_err());
        assert!(develop(&f, &Plane::new(Vec3::new(0.0, 12.0, 0.0), -Vec3::X, Vec3::Z).unwrap()).is_err());
        // A plane square to the axis but not touching the cylinder.
        assert!(develop(&f, &Plane::new(Vec3::new(0.0, 0.0, 0.0), Vec3::X, Vec3::Y).unwrap()).is_err());
    }

    #[test]
    fn a_cylinder_face_is_found_by_its_point() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let (index, f) = cylindrical_wall_at(&body, Vec3::new(0.0, 10.0, 5.0)).unwrap();
        assert!((f.radius - 10.0).abs() < 1e-9);
        assert!(index < 3, "{index}");
        // A point off the wall is on no wall.
        assert!(cylindrical_wall_at(&body, Vec3::new(0.0, 0.0, 5.0)).is_err());
        // The wall's faces are measured apart, so that a path may cross the seam but not a
        // relief groove.
        let spans = cylinder_spans(&body, &f).unwrap();
        assert!(!spans.is_empty());
        let total: f64 = spans.iter().map(|s| s.sweep).sum();
        assert!((total - std::f64::consts::TAU).abs() < 1e-6, "{total}");
    }
}
