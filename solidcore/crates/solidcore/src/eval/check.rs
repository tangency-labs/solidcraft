//! The kernel's checks of a record's geometric records, which the record's own structural
//! validation ([`Brep::validate`]) leaves to the kernel because they compute: every frame is
//! orthonormal, every plane spans two directions, and every placed revolution turns about a unit
//! axis under a non-singular placement. [`validate`] and [`validate_built`] are the record's
//! validation and these together: what a record must pass before the kernel takes it.
use crate::prelude::*;
use solidcore_record::brep::{Brep, Curve, Frame, Result, Surface, require};

use solidcore_record::Vec3;

/// [`Brep::validate`] (a record from outside the kernel), then [`geometry`].
pub fn validate(b: &Brep) -> Result<()> {
    b.validate()?;
    geometry(b)
}

/// [`Brep::validate_built`] (a record the kernel built), then [`geometry`].
pub fn validate_built(b: &Brep) -> Result<()> {
    b.validate_built()?;
    geometry(b)
}

/// Every edge's and face's geometric records: their frames, planes and placements ([`curve`],
/// [`surface`]). Call on a record its structural validation accepted.
pub fn geometry(b: &Brep) -> Result<()> {
    for e in &b.edges {
        curve(&e.curve)?;
        if let Some(n) = &e.nurbs {
            curve(n)?;
        }
    }
    for f in &b.faces {
        surface(&f.surface)?;
        if let Some(n) = &f.nurbs {
            surface(n)?;
        }
    }
    Ok(())
}

fn vector(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A frame's axes are unit and perpendicular, to 1e-8.
pub fn frame(f: &Frame) -> Result<()> {
    let (x, y) = (vector(f.x_axis), vector(f.y_axis));
    require((x.len() - 1.).abs() <= 1e-8 && (y.len() - 1.).abs() <= 1e-8 && x.dot(y).abs() <= 1e-8, "nonorthonormal frame")
}

/// A curve's geometric records, and those of the surfaces of an intersection curve. Call on a
/// curve its structural validation accepted.
pub fn curve(c: &Curve) -> Result<()> {
    match c {
        Curve::Circle { frame: f, .. } | Curve::Ellipse { frame: f, .. } => frame(f),
        Curve::SurfaceIntersection { surfaces, .. } => surfaces.iter().try_for_each(|s| surface(s)),
        Curve::Line { .. } | Curve::Polyline { .. } | Curve::Spline { .. } | Curve::RationalSpline { .. } => Ok(()),
    }
}

/// A surface's geometric records, and its profile's for a placed revolution. Call on a surface
/// its structural validation accepted.
pub fn surface(s: &Surface) -> Result<()> {
    match s {
        Surface::Plane { u_axis, v_axis, .. } => require(vector(*u_axis).cross(vector(*v_axis)).len() > 1e-12, "degenerate plane"),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let o = vector(*origin);
            require((vector(*u_point) - o).cross(vector(*v_point) - o).len() > 1e-12, "degenerate plane")
        }
        Surface::Cylinder { frame: f, .. } | Surface::Sphere { frame: f, .. } | Surface::Cone { frame: f, .. } | Surface::Torus { frame: f, .. } => {
            frame(f)
        }
        Surface::PlacedRevolution { profile, axis, matrix, .. } => {
            require((vector(*axis).len() - 1.0).abs() <= 1e-8, "revolution axis is not unit length")?;
            let [x, y, z, _] = matrix.map(|c| vector([c[0], c[1], c[2]]));
            require(x.cross(y).dot(z).abs() > 1e-12, "singular revolution placement")?;
            curve(profile)
        }
        Surface::BallPatch { .. } | Surface::Spline { .. } | Surface::RationalSpline { .. } => Ok(()),
    }
}
