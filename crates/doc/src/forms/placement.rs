//! Rigid Form placement is independent of its local subdivision controls.
use crate::{DocError, Mat, Result};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_kernel::affine;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub origin: Vec3,
    pub axes: [Vec3; 3],
}
impl Default for Placement {
    fn default() -> Self {
        Self { origin: Vec3::ZERO, axes: [Vec3::X, Vec3::Y, Vec3::Z] }
    }
}
impl Placement {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
    pub fn matrix(&self) -> Mat {
        affine::from_frame(self.origin, self.axes)
    }
    pub fn from_matrix(m: &Mat) -> Result<Self> {
        if m.iter().flatten().any(|v| !v.is_finite()) || m[..3].iter().any(|c| c[3].abs() > 1e-10) || (m[3][3] - 1.0).abs() > 1e-10 {
            return Err(DocError::Invalid("Form placement must be a finite rigid transform".into()));
        }
        let column = |i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
        let p = Self { origin: column(3), axes: [column(0), column(1), column(2)] };
        p.validate()?;
        Ok(p)
    }
    pub fn validate(&self) -> Result<()> {
        if !self.origin.is_finite()
            || self.origin.x.abs().max(self.origin.y.abs()).max(self.origin.z.abs()) > 100_000.0
            || !affine::proper_axes(self.axes, 1e-8)
        {
            return Err(DocError::Invalid("Form placement requires a proper orthonormal frame and bounded origin".into()));
        }
        Ok(())
    }
    pub fn point(&self, p: Vec3) -> Vec3 {
        affine::point(&self.matrix(), p)
    }
    pub fn vector(&self, v: Vec3) -> Vec3 {
        affine::vector(&self.matrix(), v)
    }
    pub fn local_point(&self, p: Vec3) -> Vec3 {
        affine::frame_point(self.origin, self.axes, p)
    }
    pub fn local_vector(&self, v: Vec3) -> Vec3 {
        affine::frame_vector(self.axes, v)
    }
}
