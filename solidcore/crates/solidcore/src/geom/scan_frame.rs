//! Reusable plane-fitting, right-handed 3-2-1 reference-frame construction, and rigid alignment
//! mathematics for scan/mesh placement (#14353).
//!
//! Keeps fitting and frame math inside `solidcore` so both `solidcore-ops` recipes and SolidCraft
//! commands share one exact, deterministic, proper-rigid (`det(R) = +1`) implementation:
//!
//! - [`fit_plane`]: fits a plane to `>= 3` noncollinear points via [`best_fit_plane_to_points`]
//!   and reports unsigned **RMS** and **maximum absolute** residuals in caller units (unlike
//!   [`BestFitPlane::max_plane_gap`], which is signed).
//! - [`project_direction_onto_plane`]: projects a two-point or explicit direction into a primary
//!   plane and rejects coincident points or near-normal directions.
//! - [`project_origin_onto_plane`]: projects an explicit origin point orthogonally onto the
//!   primary plane and returns both positions plus the off-plane distance.
//! - [`Frame3D::from_plane_direction_origin`]: builds a strict right-handed orthonormal frame
//!   with optional normal and direction reversal (`det = +1`, never mirrored).
//! - [`rigid_alignment`]: maps a source frame onto a target frame with translation and proper
//!   rotation only (no scale, shear, or reflection) and composes with existing column-major
//!   placements via [`compose_column_major`].

use crate::error::{Result, SolidError};
use crate::geom::geom_utility::p1::PointSetType;
use crate::geom::geom_utility::p2::best_fit_plane_to_points;
use crate::geom::vector3d::{Point3, Vec3};

const OP_FIT_PLANE: &str = "geom::scan_frame::fit_plane";
const OP_EXPLICIT_PLANE: &str = "geom::scan_frame::plane_from_origin_normal";
const OP_PROJECT_DIR: &str = "geom::scan_frame::project_direction_onto_plane";
const OP_PROJECT_ORIGIN: &str = "geom::scan_frame::project_origin_onto_plane";
const OP_BUILD_FRAME: &str = "geom::scan_frame::Frame3D::from_plane_direction_origin";
const OP_ALIGN: &str = "geom::scan_frame::rigid_alignment";

/// Minimum sine of the angle between a direction vector and the plane normal (~0.00057°).
/// Directions closer to normal than this are rejected as degenerate.
const MIN_IN_PLANE_SIN: f64 = 1e-5;

/// Result of fitting or validating a primary reference plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneFit {
    /// Point on the plane (centroid of the sample set for fitted planes).
    pub origin: Point3,
    /// Unit normal of the plane.
    pub normal: Vec3,
    /// Number of sample points used (`0` when constructed from an existing explicit plane).
    pub sample_count: usize,
    /// Root-mean-square of unsigned distances from samples to the plane, in document units.
    pub rms_residual: f64,
    /// Maximum **absolute** distance from any sample to the plane, in document units.
    pub max_abs_residual: f64,
    /// Whether `max_abs_residual <= tol` (a noisy sample set is never labelled planar).
    pub is_planar: bool,
}

/// Origin projected orthogonally onto the primary plane, retaining both positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedOrigin {
    /// Original explicit origin point supplied by the caller.
    pub raw_origin: Point3,
    /// Orthogonal projection of `raw_origin` onto the primary plane.
    pub projected_origin: Point3,
    /// Unsigned distance between `raw_origin` and `projected_origin`, in document units.
    pub off_plane_distance: f64,
}

/// Right-handed orthonormal 3D reference frame (`x × y = z`, `det = +1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame3D {
    /// Frame origin.
    pub origin: Point3,
    /// Primary in-plane unit axis (X).
    pub x_axis: Vec3,
    /// Derived in-plane unit axis (`z_axis × x_axis`, Y).
    pub y_axis: Vec3,
    /// Plane unit normal axis (Z).
    pub z_axis: Vec3,
}

/// Proper rigid alignment transform (`det(R) = +1`, no scale, shear, or mirroring).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidAlignment {
    /// Source reference frame that was mapped.
    pub source: Frame3D,
    /// Target reference frame that `source` maps onto.
    pub target: Frame3D,
    /// Column-major 4×4 transform matrix (`m[col][row]`, matching `solidcraft_doc::Mat`).
    pub column_major: [[f64; 4]; 4],
    /// Row-major 4×4 transform matrix (`m[row][col]`).
    pub row_major: [[f64; 4]; 4],
    /// Determinant of the 3×3 rotation submatrix (guaranteed near `+1.0`).
    pub determinant: f64,
    /// Maximum absolute entry of `RᵀR − I` (guaranteed near `0.0`).
    pub orthogonality_error: f64,
}

fn is_finite_vec(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn invalid(op: &'static str, msg: &'static str) -> SolidError {
    SolidError::invalid_input(op, msg)
}

/// Fits a least-squares primary plane to `>= 3` noncollinear points and computes unsigned
/// RMS and maximum **absolute** residuals in document units.
///
/// Rejects `< 3` points, non-finite coordinates, non-positive tolerance, and coincident or
/// collinear sample sets.
pub fn fit_plane(points: &[Point3], tol: f64) -> Result<PlaneFit> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid(
            OP_FIT_PLANE,
            "tolerance must be finite and positive",
        ));
    }
    if points.len() < 3 {
        return Err(invalid(
            OP_FIT_PLANE,
            "plane fit requires at least 3 points",
        ));
    }
    if points.iter().any(|&p| !is_finite_vec(p)) {
        return Err(invalid(OP_FIT_PLANE, "plane fit points must be finite"));
    }

    let raw = best_fit_plane_to_points(points, tol)?;
    match raw.set_type {
        PointSetType::Planar | PointSetType::Scattered => {}
        _ => {
            return Err(invalid(
                OP_FIT_PLANE,
                "plane fit points are coincident or collinear",
            ));
        }
    }

    let normal = raw
        .normal
        .unitize()
        .map_err(|_| invalid(OP_FIT_PLANE, "plane normal is degenerate"))?;
    if !is_finite_vec(normal) || !is_finite_vec(raw.point) {
        return Err(invalid(OP_FIT_PLANE, "fitted plane is not finite"));
    }

    // Compute unsigned RMS and maximum absolute residuals across all samples.
    // (`BestFitPlane::max_plane_gap` only tracks the largest positive signed gap.)
    let mut sum_sq = 0.0_f64;
    let mut max_abs = 0.0_f64;
    for &p in points {
        let abs_d = normal.dot(p - raw.point).abs();
        sum_sq += abs_d * abs_d;
        if abs_d > max_abs {
            max_abs = abs_d;
        }
    }
    let rms = (sum_sq / (points.len() as f64)).sqrt();

    Ok(PlaneFit {
        origin: raw.point,
        normal,
        sample_count: points.len(),
        rms_residual: rms,
        max_abs_residual: max_abs,
        is_planar: max_abs <= tol,
    })
}

/// Validates and normalizes an existing explicit plane `(origin, normal)`.
pub fn plane_from_origin_normal(origin: Point3, normal: Vec3) -> Result<PlaneFit> {
    if !is_finite_vec(origin) || !is_finite_vec(normal) {
        return Err(invalid(
            OP_EXPLICIT_PLANE,
            "plane origin and normal must be finite",
        ));
    }
    let n = normal
        .unitize()
        .map_err(|_| invalid(OP_EXPLICIT_PLANE, "plane normal must be non-zero"))?;
    Ok(PlaneFit {
        origin,
        normal: n,
        sample_count: 0,
        rms_residual: 0.0,
        max_abs_residual: 0.0,
        is_planar: true,
    })
}

/// Projects an explicit direction vector into the plane defined by unit `plane_normal`.
///
/// Rejects zero/non-finite directions and directions normal (or near-normal) to the plane.
pub fn project_direction_onto_plane(direction: Vec3, plane_normal: Vec3, tol: f64) -> Result<Vec3> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid(
            OP_PROJECT_DIR,
            "tolerance must be finite and positive",
        ));
    }
    if !is_finite_vec(direction) || !is_finite_vec(plane_normal) {
        return Err(invalid(
            OP_PROJECT_DIR,
            "direction and plane normal must be finite",
        ));
    }
    let n = plane_normal
        .unitize()
        .map_err(|_| invalid(OP_PROJECT_DIR, "plane normal must be non-zero"))?;
    let raw_len = direction.length();
    if !(raw_len.is_finite() && raw_len > tol) {
        return Err(invalid(
            OP_PROJECT_DIR,
            "direction vector is zero or shorter than tolerance",
        ));
    }
    let proj = direction - n * direction.dot(n);
    let proj_len = proj.length();
    if !(proj_len.is_finite() && proj_len > tol && proj_len / raw_len > MIN_IN_PLANE_SIN) {
        return Err(invalid(
            OP_PROJECT_DIR,
            "direction is normal or nearly normal to the primary plane",
        ));
    }
    Ok(proj / proj_len)
}

/// Projects the vector from `from` to `to` into the plane defined by `plane_normal`.
///
/// Rejects coincident points and directions normal to the plane.
pub fn project_points_direction_onto_plane(
    from: Point3,
    to: Point3,
    plane_normal: Vec3,
    tol: f64,
) -> Result<Vec3> {
    if !is_finite_vec(from) || !is_finite_vec(to) {
        return Err(invalid(OP_PROJECT_DIR, "direction points must be finite"));
    }
    project_direction_onto_plane(to - from, plane_normal, tol)
}

/// Projects an explicit `raw_origin` orthogonally onto `plane`, retaining both positions
/// and the unsigned off-plane distance.
pub fn project_origin_onto_plane(raw_origin: Point3, plane: &PlaneFit) -> Result<ProjectedOrigin> {
    if !is_finite_vec(raw_origin) || !is_finite_vec(plane.origin) || !is_finite_vec(plane.normal) {
        return Err(invalid(
            OP_PROJECT_ORIGIN,
            "origin and plane must be finite",
        ));
    }
    let n = plane
        .normal
        .unitize()
        .map_err(|_| invalid(OP_PROJECT_ORIGIN, "plane normal must be non-zero"))?;
    let signed = (raw_origin - plane.origin).dot(n);
    let projected = raw_origin - n * signed;
    Ok(ProjectedOrigin {
        raw_origin,
        projected_origin: projected,
        off_plane_distance: signed.abs(),
    })
}

impl Frame3D {
    /// World origin frame: origin `(0, 0, 0)`, axes `X=(1,0,0), Y=(0,1,0), Z=(0,0,1)`.
    #[must_use]
    pub const fn world() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            x_axis: Vec3::new(1.0, 0.0, 0.0),
            y_axis: Vec3::new(0.0, 1.0, 0.0),
            z_axis: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    /// Builds a strict right-handed orthonormal frame from a primary plane, a secondary direction,
    /// and a projected origin, allowing intentional reversal of the normal (`Z`) and/or in-plane
    /// direction (`X`) while always deriving `Y = Z × X` so `det = +1`.
    pub fn from_plane_direction_origin(
        plane: &PlaneFit,
        direction: Vec3,
        raw_origin: Point3,
        flip_normal: bool,
        flip_direction: bool,
        tol: f64,
    ) -> Result<(Self, ProjectedOrigin)> {
        let proj_origin = project_origin_onto_plane(raw_origin, plane)?;
        let z_base = plane
            .normal
            .unitize()
            .map_err(|_| invalid(OP_BUILD_FRAME, "plane normal must be non-zero"))?;
        let x_base = project_direction_onto_plane(direction, z_base, tol)?;

        let z_axis = if flip_normal { -z_base } else { z_base };
        let x_raw = if flip_direction { -x_base } else { x_base };
        // Re-orthogonalize X against Z for numerical tightness.
        let x_axis = (x_raw - z_axis * x_raw.dot(z_axis))
            .unitize()
            .map_err(|_| invalid(OP_BUILD_FRAME, "frame axes are degenerate"))?;
        let y_axis = z_axis
            .cross(x_axis)
            .unitize()
            .map_err(|_| invalid(OP_BUILD_FRAME, "frame cross product is degenerate"))?;

        Ok((
            Self {
                origin: proj_origin.projected_origin,
                x_axis,
                y_axis,
                z_axis,
            },
            proj_origin,
        ))
    }

    /// Determinant of the basis `[x_axis, y_axis, z_axis]`.
    #[must_use]
    pub fn determinant(&self) -> f64 {
        self.x_axis.cross(self.y_axis).dot(self.z_axis)
    }

    /// Transforms a world point by the frame's columns (`R_frame * local + origin`).
    #[must_use]
    pub fn to_world(&self, local: Point3) -> Point3 {
        self.origin + self.x_axis * local.x + self.y_axis * local.y + self.z_axis * local.z
    }
}

/// Computes the unique proper rigid transform (`det(R) = +1`, translation + rotation only)
/// mapping `source` onto `target`.
pub fn rigid_alignment(source: Frame3D, target: Frame3D) -> Result<RigidAlignment> {
    for v in [
        source.origin,
        source.x_axis,
        source.y_axis,
        source.z_axis,
        target.origin,
        target.x_axis,
        target.y_axis,
        target.z_axis,
    ] {
        if !is_finite_vec(v) {
            return Err(invalid(OP_ALIGN, "source and target frames must be finite"));
        }
    }
    if (source.determinant() - 1.0).abs() > 1e-6 || (target.determinant() - 1.0).abs() > 1e-6 {
        return Err(invalid(
            OP_ALIGN,
            "source and target frames must be right-handed orthonormal (det = +1)",
        ));
    }

    // R = R_target * R_source^T
    // Columns of R_source are [sx, sy, sz]; columns of R_target are [tx, ty, tz].
    // For any world vector v: R v = tx (sx·v) + ty (sy·v) + tz (sz·v).
    let (sx, sy, sz) = (source.x_axis, source.y_axis, source.z_axis);
    let (tx, ty, tz) = (target.x_axis, target.y_axis, target.z_axis);
    let r00 = tx.x * sx.x + ty.x * sy.x + tz.x * sz.x;
    let r01 = tx.x * sx.y + ty.x * sy.y + tz.x * sz.y;
    let r02 = tx.x * sx.z + ty.x * sy.z + tz.x * sz.z;
    let r10 = tx.y * sx.x + ty.y * sy.x + tz.y * sz.x;
    let r11 = tx.y * sx.y + ty.y * sy.y + tz.y * sz.y;
    let r12 = tx.y * sx.z + ty.y * sy.z + tz.y * sz.z;
    let r20 = tx.z * sx.x + ty.z * sy.x + tz.z * sz.x;
    let r21 = tx.z * sx.y + ty.z * sy.y + tz.z * sz.y;
    let r22 = tx.z * sx.z + ty.z * sy.z + tz.z * sz.z;

    let r_so = Vec3::new(
        r00 * source.origin.x + r01 * source.origin.y + r02 * source.origin.z,
        r10 * source.origin.x + r11 * source.origin.y + r12 * source.origin.z,
        r20 * source.origin.x + r21 * source.origin.y + r22 * source.origin.z,
    );
    let t = target.origin - r_so;

    let det = r00 * (r11 * r22 - r12 * r21) - r01 * (r10 * r22 - r12 * r20)
        + r02 * (r10 * r21 - r11 * r20);

    let cols = [
        Vec3::new(r00, r10, r20),
        Vec3::new(r01, r11, r21),
        Vec3::new(r02, r12, r22),
    ];
    let mut max_ortho_err = 0.0_f64;
    for (i, ci) in cols.iter().enumerate() {
        for (j, cj) in cols.iter().enumerate() {
            let err = (ci.dot(*cj) - if i == j { 1.0 } else { 0.0 }).abs();
            if err > max_ortho_err {
                max_ortho_err = err;
            }
        }
    }

    if !det.is_finite() || (det - 1.0).abs() > 1e-6 || max_ortho_err > 1e-6 {
        return Err(invalid(
            OP_ALIGN,
            "computed alignment matrix is not a proper rigid rotation",
        ));
    }

    let row_major = [
        [r00, r01, r02, t.x],
        [r10, r11, r12, t.y],
        [r20, r21, r22, t.z],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let column_major = [
        [r00, r10, r20, 0.0],
        [r01, r11, r21, 0.0],
        [r02, r12, r22, 0.0],
        [t.x, t.y, t.z, 1.0],
    ];

    Ok(RigidAlignment {
        source,
        target,
        column_major,
        row_major,
        determinant: det,
        orthogonality_error: max_ortho_err,
    })
}

/// Applies a column-major 4×4 rigid transform (`m[col][row]`) to a 3D point.
#[must_use]
pub fn apply_column_major_point(m: &[[f64; 4]; 4], p: Point3) -> Point3 {
    let [[c00, c01, c02, _], [c10, c11, c12, _], [c20, c21, c22, _], [c30, c31, c32, _]] = *m;
    Point3::new(
        c00 * p.x + c10 * p.y + c20 * p.z + c30,
        c01 * p.x + c11 * p.y + c21 * p.z + c31,
        c02 * p.x + c12 * p.y + c22 * p.z + c32,
    )
}

/// Applies the 3×3 rotational part of a column-major 4×4 matrix to a direction vector.
#[must_use]
pub fn apply_column_major_vec(m: &[[f64; 4]; 4], v: Vec3) -> Vec3 {
    let [[c00, c01, c02, _], [c10, c11, c12, _], [c20, c21, c22, _], _] = *m;
    Vec3::new(
        c00 * v.x + c10 * v.y + c20 * v.z,
        c01 * v.x + c11 * v.y + c21 * v.z,
        c02 * v.x + c12 * v.y + c22 * v.z,
    )
}

/// Composes two column-major 4×4 transforms `a * b` (apply `b` first, then `a`).
///
/// For composing a world-space alignment delta with an existing point-cloud transform before
/// calling `pointcloud.edit`: `final_matrix = compose_column_major(&world_delta, &existing)`.
#[must_use]
pub fn compose_column_major(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let [a0, a1, a2, a3] = *a;
    let mul_col = |[b0, b1, b2, b3]: [f64; 4]| {
        [
            a0[0] * b0 + a1[0] * b1 + a2[0] * b2 + a3[0] * b3,
            a0[1] * b0 + a1[1] * b1 + a2[1] * b2 + a3[1] * b3,
            a0[2] * b0 + a1[2] * b1 + a2[2] * b2 + a3[2] * b3,
            a0[3] * b0 + a1[3] * b1 + a2[3] * b2 + a3[3] * b3,
        ]
    };
    let [b0, b1, b2, b3] = *b;
    [mul_col(b0), mul_col(b1), mul_col(b2), mul_col(b3)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    #[test]
    fn synthetic_rotated_translated_scan_aligns_to_world_and_target_frames() {
        // Construct a synthetic rotated + translated scan frame in world coordinates.
        let z = Vec3::new(1.0, 2.0, 3.0).unitize().unwrap();
        let x = project_direction_onto_plane(Vec3::new(0.0, 1.0, -0.5), z, 1e-6).unwrap();
        let y = z.cross(x).unitize().unwrap();
        let o = pt(125.5, -42.25, 88.0);
        let true_frame = Frame3D {
            origin: o,
            x_axis: x,
            y_axis: y,
            z_axis: z,
        };

        // Four coplanar samples on a smaller functional face (not necessarily centered at o).
        let samples = [
            true_frame.to_world(pt(10.0, 5.0, 0.0)),
            true_frame.to_world(pt(30.0, 5.0, 0.0)),
            true_frame.to_world(pt(30.0, 20.0, 0.0)),
            true_frame.to_world(pt(10.0, 20.0, 0.0)),
        ];
        let dir_from = true_frame.to_world(pt(2.0, -4.0, 3.5)); // off-plane points!
        let dir_to = true_frame.to_world(pt(17.0, -4.0, -1.2));
        let raw_origin = true_frame.to_world(pt(0.0, 0.0, 4.25)); // off-plane by 4.25 mm

        let plane = fit_plane(&samples, 1e-6).unwrap();
        assert!(plane.is_planar);
        assert!(plane.rms_residual < 1e-12);
        assert!(plane.max_abs_residual < 1e-12);

        // Pick flip_normal if PCA normal happened to point opposite to +z.
        let flip_n = plane.normal.dot(z) < 0.0;
        let dir =
            project_points_direction_onto_plane(dir_from, dir_to, plane.normal, 1e-6).unwrap();
        let (src, proj_origin) =
            Frame3D::from_plane_direction_origin(&plane, dir, raw_origin, flip_n, false, 1e-6)
                .unwrap();
        assert!((proj_origin.off_plane_distance - 4.25).abs() < 1e-12);
        assert!(proj_origin.projected_origin.distance(o) < 1e-12);

        let align = rigid_alignment(src, Frame3D::world()).unwrap();
        assert!((align.determinant - 1.0).abs() < 1e-12);
        assert!(align.orthogonality_error < 1e-12);

        // Verify origin, plane samples, direction, normal, and pairwise distances.
        let aligned_o = apply_column_major_point(&align.column_major, proj_origin.projected_origin);
        assert!(aligned_o.distance(Point3::new(0.0, 0.0, 0.0)) < 1e-11);
        let aligned_n = apply_column_major_vec(&align.column_major, src.z_axis);
        assert!(aligned_n.distance(Vec3::new(0.0, 0.0, 1.0)) < 1e-12);
        let aligned_x = apply_column_major_vec(&align.column_major, src.x_axis);
        assert!(aligned_x.distance(Vec3::new(1.0, 0.0, 0.0)) < 1e-12);

        let moved: Vec<Point3> = samples
            .iter()
            .map(|&p| apply_column_major_point(&align.column_major, p))
            .collect();
        for &p in &moved {
            assert!(
                p.z.abs() < 1e-11,
                "aligned sample should lie on Z=0, got {p:?}"
            );
        }
        for i in 0..samples.len() {
            for j in (i + 1)..samples.len() {
                let d0 = samples[i].distance(samples[j]);
                let d1 = moved[i].distance(moved[j]);
                assert!(
                    (d0 - d1).abs() < 1e-11,
                    "pairwise distance changed: {d0} vs {d1}"
                );
            }
        }
    }

    #[test]
    fn noisy_samples_report_exact_rms_and_max_abs_residuals_and_not_planar() {
        // Symmetric +/- noise so signed max would differ if asymmetric; include an asymmetric
        // negative outlier (-0.5) larger in magnitude than any positive outlier (+0.2).
        let samples = [
            pt(-10.0, -10.0, 0.1),
            pt(10.0, -10.0, -0.1),
            pt(10.0, 10.0, 0.1),
            pt(-10.0, 10.0, -0.1),
            pt(0.0, 0.0, -0.5),
            pt(0.0, 0.0, 0.2),
            pt(5.0, -5.0, 0.15),
            pt(-5.0, 5.0, 0.15),
        ];
        let fit = fit_plane(&samples, 1e-3).unwrap();
        assert!(!fit.is_planar, "noisy set must not be labelled planar");
        // Verify directly against the returned plane normal & origin.
        let mut expected_max = 0.0_f64;
        let mut expected_sum_sq = 0.0_f64;
        for &p in &samples {
            let d = fit.normal.dot(p - fit.origin).abs();
            expected_max = expected_max.max(d);
            expected_sum_sq += d * d;
        }
        let expected_rms = (expected_sum_sq / (samples.len() as f64)).sqrt();
        assert!((fit.max_abs_residual - expected_max).abs() < 1e-12);
        assert!((fit.rms_residual - expected_rms).abs() < 1e-12);
        assert!(
            fit.max_abs_residual > 0.4,
            "negative outlier must be counted in max_abs_residual"
        );
    }

    #[test]
    fn all_axis_reversals_remain_right_handed_det_plus_one() {
        let plane = plane_from_origin_normal(pt(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, 1.0)).unwrap();
        for flip_n in [false, true] {
            for flip_d in [false, true] {
                let (src, _) = Frame3D::from_plane_direction_origin(
                    &plane,
                    Vec3::new(1.0, 0.0, 0.0),
                    pt(1.0, 2.0, 3.0),
                    flip_n,
                    flip_d,
                    1e-6,
                )
                .unwrap();
                assert!((src.determinant() - 1.0).abs() < 1e-12);
                let align = rigid_alignment(src, Frame3D::world()).unwrap();
                assert!((align.determinant - 1.0).abs() < 1e-12);
                assert!(align.orthogonality_error < 1e-12);
            }
        }
    }

    #[test]
    fn degenerate_and_hostile_inputs_are_rejected() {
        // Too few points
        assert!(fit_plane(&[pt(0.0, 0.0, 0.0), pt(1.0, 0.0, 0.0)], 1e-6).is_err());
        // Coincident points
        assert!(fit_plane(&[pt(1.0, 1.0, 1.0); 4], 1e-6).is_err());
        // Collinear points
        assert!(fit_plane(
            &[pt(0.0, 0.0, 0.0), pt(5.0, 0.0, 0.0), pt(10.0, 0.0, 0.0)],
            1e-6
        )
        .is_err());
        // NaN coordinate
        assert!(fit_plane(
            &[pt(0.0, 0.0, 0.0), pt(1.0, 0.0, 0.0), pt(0.0, f64::NAN, 0.0)],
            1e-6
        )
        .is_err());
        // Coincident direction points
        assert!(project_points_direction_onto_plane(
            pt(1.0, 2.0, 3.0),
            pt(1.0, 2.0, 3.0),
            Vec3::new(0.0, 0.0, 1.0),
            1e-6
        )
        .is_err());
        // Direction normal to plane
        assert!(project_direction_onto_plane(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, 1.0),
            1e-6
        )
        .is_err());
    }

    #[test]
    fn composes_existing_cloud_placement_correctly() {
        // Existing cloud transform: translate by (10, 20, 30) and rotate 90° around Z.
        let existing = [
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 20.0, 30.0, 1.0],
        ];
        // A point in cloud-local coordinates, mapped to world by `existing`.
        let local_o = pt(5.0, 0.0, 0.0);
        let world_o = apply_column_major_point(&existing, local_o); // (10, 25, 30)
        let plane = plane_from_origin_normal(world_o, Vec3::new(0.0, 0.0, 1.0)).unwrap();
        let (src, _) = Frame3D::from_plane_direction_origin(
            &plane,
            Vec3::new(0.0, 1.0, 0.0),
            world_o,
            false,
            false,
            1e-6,
        )
        .unwrap();
        let align = rigid_alignment(src, Frame3D::world()).unwrap();
        let composed = compose_column_major(&align.column_major, &existing);

        // Applying `composed` directly to `local_o` must land at world origin (0, 0, 0).
        let final_o = apply_column_major_point(&composed, local_o);
        assert!(
            final_o.distance(Point3::new(0.0, 0.0, 0.0)) < 1e-12,
            "got {final_o:?}"
        );
    }
}
