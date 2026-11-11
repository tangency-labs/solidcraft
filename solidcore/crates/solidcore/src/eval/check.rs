//! The kernel's checks of a curve's and a surface's geometric records, which the record's own
//! structural validation leaves to the kernel because they compute: every frame is orthonormal,
//! every plane spans two directions, and every placed revolution turns about a unit axis under a
//! non-singular placement. A record must pass these and its structural validation before the
//! kernel takes it; `solidcore-ops`' `record_check` walks a record's edges and faces through them.
//!
//! Origin: SolidCraft's `crates/geom/src/brep/check.rs` (MIT OR Apache-2.0), moved here with its
//! history (RFC 0003). It takes the kernel's evaluation types.
use super::vec::{v3, VecExt};
use super::{Curve, Frame, Surface};

/// Why a geometric record is refused.
pub type Result<T> = std::result::Result<T, &'static str>;

fn require(ok: bool, what: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(what)
    }
}

/// A frame's axes are unit and perpendicular, to 1e-8.
pub fn frame(f: &Frame) -> Result<()> {
    let (x, y) = (v3(f.x_axis), v3(f.y_axis));
    require(
        (x.len() - 1.).abs() <= 1e-8 && (y.len() - 1.).abs() <= 1e-8 && x.dot(y).abs() <= 1e-8,
        "nonorthonormal frame",
    )
}

/// A curve's geometric records, and those of the surfaces of an intersection curve. Call on a
/// curve its structural validation accepted.
pub fn curve(c: &Curve) -> Result<()> {
    match c {
        Curve::Circle { frame: f, .. } | Curve::Ellipse { frame: f, .. } => frame(f),
        Curve::SurfaceIntersection { surfaces, .. } => surfaces.iter().try_for_each(|s| surface(s)),
        Curve::Line { .. }
        | Curve::Polyline { .. }
        | Curve::Spline { .. }
        | Curve::RationalSpline { .. } => Ok(()),
    }
}

/// A surface's geometric records, and its profile's for a placed revolution. Call on a surface
/// its structural validation accepted.
pub fn surface(s: &Surface) -> Result<()> {
    match s {
        Surface::Plane { u_axis, v_axis, .. } => require(
            v3(*u_axis).cross(v3(*v_axis)).len() > 1e-12,
            "degenerate plane",
        ),
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = v3(*origin);
            require(
                (v3(*u_point) - o).cross(v3(*v_point) - o).len() > 1e-12,
                "degenerate plane",
            )
        }
        Surface::Cylinder { frame: f, .. }
        | Surface::Sphere { frame: f, .. }
        | Surface::Cone { frame: f, .. }
        | Surface::Torus { frame: f, .. } => frame(f),
        Surface::PlacedRevolution {
            profile,
            axis,
            matrix,
            ..
        } => {
            require(
                (v3(*axis).len() - 1.0).abs() <= 1e-8,
                "revolution axis is not unit length",
            )?;
            let [x, y, z, _] = matrix.map(|c| v3([c[0], c[1], c[2]]));
            require(
                x.cross(y).dot(z).abs() > 1e-12,
                "singular revolution placement",
            )?;
            curve(profile)
        }
        Surface::BallPatch { .. } | Surface::Spline { .. } | Surface::RationalSpline { .. } => {
            Ok(())
        }
    }
}
