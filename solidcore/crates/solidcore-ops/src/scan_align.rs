//! Thin recipe seam for 3-2-1 scan datum alignment (#14353).
//!
//! All least-squares plane fitting, residual computation, projection, orthonormal right-handed
//! frame construction, and proper rigid matrix computation live in [`solidcore::geom::scan_frame`].
//! This module only converts plain `[f64; 3]` / `[[f64; 4]; 4]` values across the `solidcore-ops`
//! seam without performing floating-point arithmetic.

use solidcore::geom::scan_frame::{
    self, Frame3D as CoreFrame3D, PlaneFit as CorePlaneFit, ProjectedOrigin as CoreProjectedOrigin,
    RigidAlignment as CoreRigidAlignment,
};
use solidcore::{Point3, Vec3};

use crate::error::status;
use crate::Result;

/// Default geometric tolerance (mm) for plane planarity and direction degeneracy checks.
pub const DEFAULT_TOL: f64 = 1e-6;

/// Fitted or validated primary plane with unsigned residual statistics in document units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneFit {
    /// Point on the plane (sample centroid for fitted planes).
    pub origin: [f64; 3],
    /// Unit normal of the plane.
    pub normal: [f64; 3],
    /// Number of sample points (`0` for an existing explicit plane).
    pub sample_count: usize,
    /// Root-mean-square of unsigned point-to-plane distances, in document units.
    pub rms_residual: f64,
    /// Maximum absolute point-to-plane distance, in document units.
    pub max_abs_residual: f64,
    /// Whether `max_abs_residual <= tol`.
    pub is_planar: bool,
}

/// Source origin before and after orthogonal projection onto the primary plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedOrigin {
    /// Explicit origin point supplied by the caller.
    pub raw_origin: [f64; 3],
    /// Orthogonal projection onto the primary plane.
    pub projected_origin: [f64; 3],
    /// Unsigned distance between `raw_origin` and `projected_origin`, in document units.
    pub off_plane_distance: f64,
}

/// Right-handed orthonormal 3D frame (`x × y = z`, `det = +1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame3D {
    /// Frame origin.
    pub origin: [f64; 3],
    /// Unit X axis (in-plane secondary direction).
    pub x_axis: [f64; 3],
    /// Unit Y axis (`z_axis × x_axis`).
    pub y_axis: [f64; 3],
    /// Unit Z axis (primary plane normal).
    pub z_axis: [f64; 3],
}

/// Proper rigid alignment solution (`det(R) = +1`, translation and rotation only).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanAlignment {
    /// Primary plane fit / validation statistics.
    pub plane: PlaneFit,
    /// Source origin and its projection onto the primary plane.
    pub origin: ProjectedOrigin,
    /// Source right-handed orthonormal frame.
    pub source: Frame3D,
    /// Target right-handed orthonormal frame.
    pub target: Frame3D,
    /// Column-major 4×4 transform (`m[col][row]`, matching `solidcraft_doc::Mat`).
    pub column_major: [[f64; 4]; 4],
    /// Row-major 4×4 transform (`m[row][col]`).
    pub row_major: [[f64; 4]; 4],
    /// Determinant of the 3×3 rotation submatrix (near `+1.0`).
    pub determinant: f64,
    /// Maximum absolute entry of `RᵀR − I` (near `0.0`).
    pub orthogonality_error: f64,
}

/// Primary plane input: either `>= 3` measured sample points or an existing explicit plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaneInput<'a> {
    /// Fit a least-squares plane through at least 3 noncollinear points.
    Points(&'a [[f64; 3]]),
    /// Reuse an existing explicit plane `(origin, normal)`.
    Explicit {
        /// Point on the plane.
        origin: [f64; 3],
        /// Plane normal (non-zero).
        normal: [f64; 3],
    },
}

/// Secondary direction input: either two distinct points or an explicit direction vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DirectionInput {
    /// Direction from `from` to `to`, projected into the primary plane.
    Points {
        /// Start point.
        from: [f64; 3],
        /// End point.
        to: [f64; 3],
    },
    /// Explicit direction vector, projected into the primary plane.
    Vector([f64; 3]),
}

fn to_pt([x, y, z]: [f64; 3]) -> Point3 {
    Point3::new(x, y, z)
}

fn to_vec([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}

fn from_vec(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn map_err(err: solidcore::SolidError) -> crate::Error {
    status(err)
}

fn from_plane_fit(p: CorePlaneFit) -> PlaneFit {
    PlaneFit {
        origin: from_vec(p.origin),
        normal: from_vec(p.normal),
        sample_count: p.sample_count,
        rms_residual: p.rms_residual,
        max_abs_residual: p.max_abs_residual,
        is_planar: p.is_planar,
    }
}

fn from_projected_origin(o: CoreProjectedOrigin) -> ProjectedOrigin {
    ProjectedOrigin {
        raw_origin: from_vec(o.raw_origin),
        projected_origin: from_vec(o.projected_origin),
        off_plane_distance: o.off_plane_distance,
    }
}

fn from_frame(f: CoreFrame3D) -> Frame3D {
    Frame3D {
        origin: from_vec(f.origin),
        x_axis: from_vec(f.x_axis),
        y_axis: from_vec(f.y_axis),
        z_axis: from_vec(f.z_axis),
    }
}

fn to_core_frame(f: Frame3D) -> CoreFrame3D {
    CoreFrame3D {
        origin: to_pt(f.origin),
        x_axis: to_vec(f.x_axis),
        y_axis: to_vec(f.y_axis),
        z_axis: to_vec(f.z_axis),
    }
}

impl Frame3D {
    /// World origin frame: `(0, 0, 0)` with axes `X`, `Y`, `Z`.
    #[must_use]
    pub const fn world() -> Self {
        Self {
            origin: [0.0, 0.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
        }
    }
}

/// Fits a least-squares plane to `>= 3` points and reports unsigned RMS and max absolute residuals.
///
/// # Errors
/// Returns [`Error::Refused`] for `< 3` points, non-finite coordinates, or coincident/collinear samples.
pub fn fit_plane(points: &[[f64; 3]], tol: f64) -> Result<PlaneFit> {
    let pts: Vec<Point3> = points.iter().copied().map(to_pt).collect();
    scan_frame::fit_plane(&pts, tol)
        .map(from_plane_fit)
        .map_err(map_err)
}

/// Validates and builds a right-handed target frame from `(origin, x_axis, z_axis)`.
///
/// Projects `x_axis` into the plane orthogonal to `z_axis` and derives `y_axis = z_axis × x_axis`.
///
/// # Errors
/// Returns [`Error::Refused`] for degenerate, parallel, or non-finite inputs.
pub fn build_frame(
    plane_origin: [f64; 3],
    plane_normal: [f64; 3],
    direction: [f64; 3],
    raw_origin: [f64; 3],
    flip_normal: bool,
    flip_direction: bool,
    tol: f64,
) -> Result<(Frame3D, ProjectedOrigin)> {
    let plane = scan_frame::plane_from_origin_normal(to_pt(plane_origin), to_vec(plane_normal))
        .map_err(map_err)?;
    let (frame, origin) = CoreFrame3D::from_plane_direction_origin(
        &plane,
        to_vec(direction),
        to_pt(raw_origin),
        flip_normal,
        flip_direction,
        tol,
    )
    .map_err(map_err)?;
    Ok((from_frame(frame), from_projected_origin(origin)))
}

/// Solves a complete 3-2-1 datum alignment from primary plane, secondary direction, and explicit
/// origin onto `target`, returning the rigid transform (`det = +1`) and fit/projection statistics.
///
/// # Errors
/// Returns [`Error::Refused`] if any input is incomplete, non-finite, or degenerate (collinear
/// plane points, coincident direction points, or direction normal to the plane).
#[allow(clippy::too_many_arguments)]
pub fn solve_alignment(
    plane: PlaneInput<'_>,
    direction: DirectionInput,
    raw_origin: [f64; 3],
    flip_normal: bool,
    flip_direction: bool,
    target: Frame3D,
    tol: f64,
) -> Result<ScanAlignment> {
    let core_plane = match plane {
        PlaneInput::Points(pts) => {
            let owned: Vec<Point3> = pts.iter().copied().map(to_pt).collect();
            scan_frame::fit_plane(&owned, tol).map_err(map_err)?
        }
        PlaneInput::Explicit { origin, normal } => {
            scan_frame::plane_from_origin_normal(to_pt(origin), to_vec(normal)).map_err(map_err)?
        }
    };
    let dir_vec = match direction {
        DirectionInput::Points { from, to } => scan_frame::project_points_direction_onto_plane(
            to_pt(from),
            to_pt(to),
            core_plane.normal,
            tol,
        )
        .map_err(map_err)?,
        DirectionInput::Vector(v) => {
            scan_frame::project_direction_onto_plane(to_vec(v), core_plane.normal, tol)
                .map_err(map_err)?
        }
    };
    let (core_source, core_origin) = CoreFrame3D::from_plane_direction_origin(
        &core_plane,
        dir_vec,
        to_pt(raw_origin),
        flip_normal,
        flip_direction,
        tol,
    )
    .map_err(map_err)?;
    let CoreRigidAlignment {
        source,
        target,
        column_major,
        row_major,
        determinant,
        orthogonality_error,
    } = scan_frame::rigid_alignment(core_source, to_core_frame(target)).map_err(map_err)?;

    Ok(ScanAlignment {
        plane: from_plane_fit(core_plane),
        origin: from_projected_origin(core_origin),
        source: from_frame(source),
        target: from_frame(target),
        column_major,
        row_major,
        determinant,
        orthogonality_error,
    })
}

/// Composes two column-major 4×4 matrices `a * b` (apply `b` first, then `a`).
#[must_use]
pub fn compose_column_major(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    scan_frame::compose_column_major(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn solve_alignment_fits_plane_projects_origin_and_keeps_det_plus_one() {
        let pts = [
            [10.0, 20.0, 5.0],
            [30.0, 20.0, 5.0],
            [30.0, 40.0, 5.0],
            [10.0, 40.0, 5.0],
        ];
        let sol = solve_alignment(
            PlaneInput::Points(&pts),
            DirectionInput::Points {
                from: [10.0, 20.0, 8.0],
                to: [10.0, 35.0, 3.0],
            },
            [10.0, 20.0, 9.0],
            false,
            false,
            Frame3D::world(),
            DEFAULT_TOL,
        )
        .unwrap();
        assert!(sol.plane.is_planar);
        assert!(sol.plane.rms_residual < 1e-12);
        assert!(sol.plane.max_abs_residual < 1e-12);
        assert!((sol.origin.off_plane_distance - 4.0).abs() < 1e-12);
        assert!((sol.determinant - 1.0).abs() < 1e-12);
        assert!(sol.orthogonality_error < 1e-12);
    }

    #[test]
    fn solve_alignment_rejects_degenerate_inputs_with_refused_reason() {
        let col = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let err = solve_alignment(
            PlaneInput::Points(&col),
            DirectionInput::Vector([0.0, 1.0, 0.0]),
            [0.0, 0.0, 0.0],
            false,
            false,
            Frame3D::world(),
            DEFAULT_TOL,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Refused { .. }), "got {err:?}");

        let err2 = solve_alignment(
            PlaneInput::Explicit {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            DirectionInput::Vector([0.0, 0.0, 10.0]),
            [0.0, 0.0, 0.0],
            false,
            false,
            Frame3D::world(),
            DEFAULT_TOL,
        )
        .unwrap_err();
        assert!(matches!(err2, Error::Refused { .. }), "got {err2:?}");
    }
}
