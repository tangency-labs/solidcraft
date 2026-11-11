//! Affine matrices for STEP placements and transformations.
//!
//! STEP keeps placements as frames and transformation operators as directions and
//! scales.  Building their matrices and composing or inverting them is geometric
//! work, so it lives here rather than in `solidcore-record` or `solidcore-ops`.
//!
//! The matrix convention is the kernel's: rows 0–2 are the images of X, Y and Z,
//! row 3 is the translation (`p' = p·A + t`).

use crate::geom::vector3d::Vec3;

/// A 4×4 affine matrix in the kernel's row-major convention.
pub type Matrix4 = [[f64; 4]; 4];

/// The identity matrix.
pub const IDENTITY: Matrix4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `a · b` in the row-vector convention.
#[must_use]
#[allow(clippy::indexing_slicing)] // 4×4 loops over fixed-size arrays
pub fn concat(a: &Matrix4, b: &Matrix4) -> Matrix4 {
    let mut out = [[0.0; 4]; 4];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (col, cell) in out_row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[row][k] * b[k][col]).sum();
        }
    }
    out
}

/// The inverse of an affine matrix, or `None` when its linear part is singular or
/// the entries are not finite.
#[must_use]
pub fn inverse(m: &Matrix4) -> Option<Matrix4> {
    let (a, b, c) = (
        Vec3::new(m[0][0], m[0][1], m[0][2]),
        Vec3::new(m[1][0], m[1][1], m[1][2]),
        Vec3::new(m[2][0], m[2][1], m[2][2]),
    );
    let det = a.dot(b.cross(c));
    if !det.is_finite() || det == 0.0 {
        return None;
    }
    let x = b.cross(c);
    let y = c.cross(a);
    let z = a.cross(b);
    // The inverse's columns are the cofactor cross products; its translation is
    // `-(t · A⁻¹)` in the row-vector convention.
    let linear = [
        [x.x / det, y.x / det, z.x / det],
        [x.y / det, y.y / det, z.y / det],
        [x.z / det, y.z / det, z.z / det],
    ];
    let t = Vec3::new(m[3][0], m[3][1], m[3][2]);
    let translation = Vec3::new(
        -(t.x * linear[0][0] + t.y * linear[1][0] + t.z * linear[2][0]),
        -(t.x * linear[0][1] + t.y * linear[1][1] + t.z * linear[2][1]),
        -(t.x * linear[0][2] + t.y * linear[1][2] + t.z * linear[2][2]),
    );
    let out = [
        [linear[0][0], linear[0][1], linear[0][2], 0.0],
        [linear[1][0], linear[1][1], linear[1][2], 0.0],
        [linear[2][0], linear[2][1], linear[2][2], 0.0],
        [translation.x, translation.y, translation.z, 1.0],
    ];
    out.iter().flatten().all(|v| v.is_finite()).then_some(out)
}

fn unit(value: [f64; 3]) -> Option<Vec3> {
    let v = Vec3::from(value);
    v.unitize()
        .ok()
        .filter(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
}

/// The matrix of an `AXIS2_PLACEMENT`: origin, Z axis and X reference direction.
///
/// An omitted axis is +Z; an omitted reference direction chooses any X perpendicular
/// to Z. The reference direction is projected onto the plane normal to Z.
#[must_use]
pub fn frame_matrix(
    origin: [f64; 3],
    axis: Option<[f64; 3]>,
    ref_direction: Option<[f64; 3]>,
) -> Option<Matrix4> {
    let z = unit(axis.unwrap_or([0.0, 0.0, 1.0]))?;
    let x = if let Some(raw) = ref_direction {
        let v = Vec3::from(raw);
        let projected = v - z * v.dot(z);
        unit([projected.x, projected.y, projected.z])?
    } else {
        let candidate = if z.x.abs() < 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let v = candidate - z * candidate.dot(z);
        unit([v.x, v.y, v.z])?
    };
    let y = z.cross(x);
    Some([
        [x.x, x.y, x.z, 0.0],
        [y.x, y.y, y.z, 0.0],
        [z.x, z.y, z.z, 0.0],
        [origin[0], origin[1], origin[2], 1.0],
    ])
}

/// The matrix of a `CARTESIAN_TRANSFORMATION_OPERATOR_3D`.
///
/// An omitted direction is the matching global axis; an omitted scale is 1 (or
/// `scale` for the second and third scale).
#[must_use]
pub fn operator_matrix(
    axis1: Option<[f64; 3]>,
    axis2: Option<[f64; 3]>,
    axis3: Option<[f64; 3]>,
    origin: [f64; 3],
    scale: Option<f64>,
    scale2: Option<f64>,
    scale3: Option<f64>,
) -> Option<Matrix4> {
    let x = unit(axis1.unwrap_or([1.0, 0.0, 0.0]))?;
    let y = unit(axis2.unwrap_or([0.0, 1.0, 0.0]))?;
    let z = unit(axis3.unwrap_or([0.0, 0.0, 1.0]))?;
    let sx = scale.unwrap_or(1.0);
    let sy = scale2.or(scale).unwrap_or(1.0);
    let sz = scale3.or(scale).unwrap_or(1.0);
    if ![sx, sy, sz].iter().all(|v| v.is_finite() && *v != 0.0) {
        return None;
    }
    Some([
        [x.x * sx, x.y * sx, x.z * sx, 0.0],
        [y.x * sy, y.y * sy, y.z * sy, 0.0],
        [z.x * sz, z.y * sz, z.z * sz, 0.0],
        [origin[0], origin[1], origin[2], 1.0],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_inverse_is_the_identity() {
        assert_eq!(inverse(&IDENTITY), Some(IDENTITY));
    }

    #[test]
    fn a_translation_inverse_moves_back() {
        let mut translation = IDENTITY;
        translation[3] = [2.0, -3.0, 0.5, 1.0];
        let inverse = inverse(&translation).expect("translation is regular");
        assert_eq!(concat(&translation, &inverse), IDENTITY);
    }

    #[test]
    fn an_omitted_frame_axis_is_z_and_omitted_x_is_chosen() {
        let frame = frame_matrix([1.0, 2.0, 3.0], None, None).expect("default frame");
        assert_eq!(frame[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(frame[3], [1.0, 2.0, 3.0, 1.0]);
        assert!(
            (frame[0][0] * frame[0][0] + frame[0][1] * frame[0][1] + frame[0][2] * frame[0][2]
                - 1.0)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn an_operator_applies_its_scales() {
        let matrix = operator_matrix(None, None, None, [0.0; 3], Some(2.0), Some(3.0), None)
            .expect("operator");
        assert_eq!(matrix[0], [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [0.0, 3.0, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 2.0, 0.0]);
    }
}
