//! Generate subdivision control cages through the kernel's coordinate primitives.

use solidcore::geom::form_cage as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use kernel::extrusion::{Selection as ExtrusionSelection, Wall as ExtrusionWall};
pub use kernel::projection::Projection;
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

/// The translated cage and the identity of its generated controls and faces.
pub struct CageExtrusion {
    /// New controls and face cycles.
    pub cage: CageGeometry,
    /// Selected source controls mapped to their cap controls.
    pub cap: std::collections::BTreeMap<usize, usize>,
    /// Appended side faces in result order.
    pub walls: Vec<ExtrusionWall>,
}

/// Construct the stable-ID extrusion of selected quad faces or free edges.
pub fn extrude(
    points: &[Vec3],
    faces: &[Vec<usize>],
    selection: ExtrusionSelection<'_>,
    delta: Vec3,
    influence: bool,
) -> solidcore::Result<CageExtrusion> {
    let points = checked_controls(points)?;
    let result = kernel::extrusion::extrude(&points, faces, selection, vec3(delta), influence)?;
    Ok(CageExtrusion {
        cage: record_cage(result.cage),
        cap: result.cap,
        walls: result.walls,
    })
}

/// Translate one control without exceeding the editable coordinate budget.
pub fn displaced(point: Vec3, delta: Vec3) -> solidcore::Result<Vec3> {
    kernel::extrusion::displaced(vec3(point), vec3(delta)).map(record_vec3)
}

/// Project selected controls onto a specified or best-fit line or plane.
pub fn project(
    points: &[Vec3],
    projection: Projection,
    direction: Option<Vec3>,
    origin: Option<Vec3>,
) -> solidcore::Result<Vec<Vec3>> {
    let points = checked_controls(points)?;
    kernel::projection::project(&points, projection, direction.map(vec3), origin.map(vec3))
        .map(|points| points.into_iter().map(record_vec3).collect())
}

fn checked_controls(points: &[Vec3]) -> solidcore::Result<Vec<solidcore::Vec3>> {
    if points.len() > MAX_POINTS {
        return Err(solidcore::SolidError::InvalidInput {
            operation: "form cage",
            reason: "editable cage budget exceeded",
        });
    }
    Ok(points.iter().copied().map(vec3).collect())
}

pub(crate) fn record_cage(cage: kernel::CageGeometry) -> CageGeometry {
    CageGeometry {
        points: cage.points.into_iter().map(record_vec3).collect(),
        faces: cage.faces,
    }
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
    kernel::build(kind, dimensions, segments, radii, &path).map(record_cage)
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
