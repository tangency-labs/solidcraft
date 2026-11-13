//! Orthogonal projection onto authored or least-squares line and plane axes.

use super::{bad, finite, unit, MAX_POINTS};
use crate::geom::matrix::Matrix;
use crate::{Result, Vec3};

/// Geometric target for the projection.
#[derive(Clone, Copy)]
pub enum Projection {
    /// Project onto a line.
    Line,
    /// Project onto a plane.
    Plane,
}

/// Project onto a line direction or plane normal through the supplied origin.
/// An omitted direction fits the corresponding principal axis. An omitted
/// origin uses the selection centroid.
pub fn project(
    points: &[Vec3],
    tool: Projection,
    direction: Option<Vec3>,
    origin: Option<Vec3>,
) -> Result<Vec<Vec3>> {
    let minimum = match tool {
        Projection::Line => 2,
        Projection::Plane => 3,
    };
    if !(minimum..=MAX_POINTS).contains(&points.len())
        || points.iter().any(|&point| !bounded(point))
        || origin.is_some_and(|point| !bounded(point))
    {
        return Err(bad(
            "select enough finite points within the Form coordinate budget",
        ));
    }
    let center = points
        .iter()
        .copied()
        .fold(Vec3::ZERO, |sum, point| sum + point)
        / points.len() as f64;
    let axis = match direction {
        Some(direction) => {
            unit(direction).ok_or_else(|| bad("projection direction must be finite and nonzero"))?
        }
        None => principal_axis(points, center, tool)?,
    };
    let origin = origin.unwrap_or(center);
    points
        .iter()
        .map(|&point| {
            let component = axis * (point - origin).dot(axis);
            let projected = match tool {
                Projection::Line => origin + component,
                Projection::Plane => point - component,
            };
            if !bounded(projected) {
                return Err(bad("projected points exceed the Form coordinate budget"));
            }
            Ok(projected)
        })
        .collect()
}

fn bounded(point: Vec3) -> bool {
    finite(point) && point.x.abs().max(point.y.abs()).max(point.z.abs()) <= 100_000.0
}

fn principal_axis(points: &[Vec3], center: Vec3, tool: Projection) -> Result<Vec3> {
    let scale = points
        .iter()
        .map(|&point| (point - center).length())
        .fold(0.0_f64, f64::max);
    if scale < 1e-10 {
        return Err(bad("coincident points do not define a best-fit direction"));
    }
    let mut covariance = Matrix::new(3, 3);
    for &point in points {
        let q = ((point - center) / scale).to_array();
        for (i, a) in q.into_iter().enumerate() {
            for (j, b) in q.into_iter().enumerate() {
                covariance.add_to(i, j, a * b / points.len() as f64)?;
            }
        }
    }
    let eigen = covariance.eigen_system_jacobi()?;
    let [smallest, middle, largest] = eigen.values.as_slice() else {
        return Err(bad("missing covariance axes"));
    };
    let (column, gap) = match tool {
        Projection::Line => (2, largest - middle),
        Projection::Plane => (0, middle - smallest),
    };
    if gap.abs() < 1e-10 * largest.abs().max(1e-14) {
        return Err(bad(
            "selection has no unique best-fit direction; specify an axis",
        ));
    }
    let axis = Vec3::new(
        eigen.vectors.at(0, column)?,
        eigen.vectors.at(1, column)?,
        eigen.vectors.at(2, column)?,
    );
    unit(axis).ok_or_else(|| bad("invalid best-fit direction"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_tilted_axes_and_rejects_ambiguous_fits() {
        let axis = unit(Vec3::X + Vec3::Z).unwrap();
        let perpendicular = unit(Vec3::X - Vec3::Z).unwrap();
        let center = Vec3::new(7.0, -3.0, 2.0);
        let line = [
            center - axis * 4.0,
            center + axis * 4.0,
            center + perpendicular,
            center - perpendicular,
        ];
        let fitted = project(&line, Projection::Line, None, None).unwrap();
        for (actual, expected) in fitted.into_iter().zip([line[0], line[1], center, center]) {
            assert!((actual - expected).length() < 1e-12);
        }
        let plane = [
            center - axis * 4.0,
            center + axis * 4.0,
            center + Vec3::Y * 3.0,
            center - Vec3::Y * 3.0,
            center + perpendicular,
            center - perpendicular,
        ];
        let fitted = project(&plane, Projection::Plane, None, None).unwrap();
        for (actual, expected) in fitted
            .into_iter()
            .zip([plane[0], plane[1], plane[2], plane[3], center, center])
        {
            assert!((actual - expected).length() < 1e-12);
        }
        assert!(project(&line[..2], Projection::Plane, None, None).is_err());
        assert!(project(
            &[
                center - axis,
                center + axis,
                center - perpendicular,
                center + perpendicular
            ],
            Projection::Line,
            None,
            None
        )
        .is_err());
    }
}
