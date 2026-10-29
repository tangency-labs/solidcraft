//! Rigid Form placement is independent of its local subdivision controls.
use crate::{DocError, Mat, Result};
use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

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
        let [x, y, z] = self.axes;
        [[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [self.origin.x, self.origin.y, self.origin.z, 1.0]]
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
        let [x, y, z] = self.axes;
        if !self.origin.is_finite()
            || self.origin.x.abs().max(self.origin.y.abs()).max(self.origin.z.abs()) > 100_000.0
            || self.axes.iter().any(|v| !v.is_finite() || (v.len2() - 1.0).abs() > 1e-8)
            || x.dot(y).abs() > 1e-8
            || y.dot(z).abs() > 1e-8
            || z.dot(x).abs() > 1e-8
            || x.cross(y).dot(z) < 1.0 - 1e-8
        {
            return Err(DocError::Invalid("Form placement requires a proper orthonormal frame and bounded origin".into()));
        }
        Ok(())
    }
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.origin + self.vector(p)
    }
    pub fn vector(&self, v: Vec3) -> Vec3 {
        self.axes[0] * v.x + self.axes[1] * v.y + self.axes[2] * v.z
    }
    pub fn local_point(&self, p: Vec3) -> Vec3 {
        self.local_vector(p - self.origin)
    }
    pub fn local_vector(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.axes[0]), v.dot(self.axes[1]), v.dot(self.axes[2]))
    }
}
