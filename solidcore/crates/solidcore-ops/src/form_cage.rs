//! Generate subdivision control cages through the kernel's coordinate primitives.

use solidcore::geom::form_cage as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use kernel::{Primitive, MAX_FACES, MAX_POINTS};
pub use solidcore::geom::regularized_fit::solve as fit_displacements;
use solidcore::geom::spline_interpolation;
pub use solidcore::geom::spline_interpolation::curve as interpolate_curve;

/// Coordinates and oriented quad faces of a generated control cage.
pub struct CageGeometry {
    /// Control coordinates in millimetres.
    pub points: Vec<Vec3>,
    /// Faces indexing the controls, in source order.
    pub faces: Vec<Vec<usize>>,
}

/// Check authored cage dimensions and sampling metadata before construction.
pub fn validate(
    dimensions: [f64; 3],
    segments: [usize; 3],
    radii: [f64; 2],
    path: &[Vec3],
) -> solidcore::Result<()> {
    let path = checked_path(path)?;
    kernel::validate(dimensions, segments, radii, &path)
}

/// Return the control coordinates and oriented faces in their stable source order.
pub fn build(
    kind: Primitive,
    dimensions: [f64; 3],
    segments: [usize; 3],
    radii: [f64; 2],
    path: &[Vec3],
) -> solidcore::Result<CageGeometry> {
    let path = checked_path(path)?;
    let cage = kernel::build(kind, dimensions, segments, radii, &path)?;
    Ok(CageGeometry {
        points: cage.points.into_iter().map(record_vec3).collect(),
        faces: cage.faces,
    })
}

/// Offset existing cage controls along their incident face normals.
pub fn offset_controls(
    points: &[Vec3],
    faces: &[Vec<usize>],
    distance: f64,
) -> solidcore::Result<Vec<Vec3>> {
    if points.len() > MAX_POINTS || faces.len() > MAX_FACES {
        return Err(solidcore::SolidError::InvalidInput {
            operation: "form cage",
            reason: "control normal budget exceeded",
        });
    }
    let points = points.iter().copied().map(vec3).collect::<Vec<_>>();
    kernel::offset_controls(&points, faces, distance)
        .map(|result| result.into_iter().map(record_vec3).collect())
}

fn checked_path(path: &[Vec3]) -> solidcore::Result<Vec<solidcore::Vec3>> {
    if path.len() > kernel::MAX_PATH_POINTS {
        return Err(solidcore::SolidError::InvalidInput {
            operation: "form cage",
            reason: "primitive metadata exceeds the path point limit",
        });
    }
    Ok(path.iter().copied().map(vec3).collect())
}

/// Fit a V-fast square sample grid and return clamped knots and U-fast cubic controls.
pub fn fit_cubic_tensor(
    samples: &[Vec3],
    parameters: &[f64],
) -> solidcore::Result<spline_interpolation::Interpolated<3>> {
    if !(4..=65).contains(&parameters.len()) || samples.len() != parameters.len() * parameters.len()
    {
        return Err(solidcore::SolidError::InvalidInput {
            operation: "form cage",
            reason: "a cubic tensor fit requires a complete bounded grid",
        });
    }
    let samples = samples.iter().copied().map(vec3).collect::<Vec<_>>();
    spline_interpolation::cubic_tensor(&samples, parameters)
}

/// Construct exact homogeneous controls for a ruled trim strip.
pub fn ruled_strip(seam: &[[f64; 4]], delta: Vec3) -> solidcore::Result<Vec<[f64; 4]>> {
    kernel::ruled_strip(seam, vec3(delta))
}
