//! The frame a profile wraps onto a cylindrical wall in, from a plane tangent to the wall.
//!
//! The wall's own normal points away from its axis outside (a convex wall) and towards it in a
//! bore. The wrapped profile's x then runs so that x, then the axis, turn about that normal: the
//! same handedness of a loop on either wall.

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// A unit vector along `a`, or `None` where it is zero or not finite.
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n.is_finite() && n > 0.0).then(|| scale(a, 1.0 / n))
}

/// The directions at the line where a plane touches a cylindrical wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallFrame {
    /// From the axis to the line of contact.
    pub radial: [f64; 3],
    /// The direction a profile's x runs in on the wall: the axis crossed with `radial` outside,
    /// `radial` crossed with the axis in a bore.
    pub tangent: [f64; 3],
}

/// The frame of the plane with unit `normal`, through the point `w` from a point of the axis
/// (unit direction `axis`), on the wall of radius `radius`: `None` unless the plane is parallel
/// to the axis and `tol` (times ten) from touching the wall. An exterior wall needs the normal
/// pointing away from the axis; a bore takes either side.
#[must_use]
pub fn wall_frame(
    normal: [f64; 3],
    w: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    convex: bool,
    tol: f64,
) -> Option<WallFrame> {
    if dot(normal, axis).abs() > 1e-8 {
        return None;
    }
    let s = dot(w, normal);
    let radial = if convex {
        if (s - radius).abs() > tol * 10.0 {
            return None;
        }
        normal
    } else {
        if (s.abs() - radius).abs() > tol * 10.0 {
            return None;
        }
        scale(normal, if s < 0.0 { -1.0 } else { 1.0 })
    };
    let radial = unit(radial)?;
    let tangent = if convex {
        cross(axis, radial)
    } else {
        cross(radial, axis)
    };
    Some(WallFrame { radial, tangent })
}
