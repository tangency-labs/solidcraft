//! Portable CPU consumer for the existing GPU gradient-sky radiance domain.
//! The GPU's generated512x256 image is constant along each row. Evaluate that
//! same row centre directly: no generated image, HDR quantization or hidden cache.
use super::{ErrorCode, JobError, Result};
use crate::lighting::environment::SkyGradient;
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CpuSky {
    pub gradient: SkyGradient,
    pub rotation_rad: f64,
    pub intensity: f64,
}
impl Default for CpuSky {
    fn default() -> Self {
        Self { gradient: Default::default(), rotation_rad: 0.0, intensity: 1.0 }
    }
}
impl CpuSky {
    pub fn validate(self) -> Result<()> {
        self.gradient.validate()?;
        if !self.rotation_rad.is_finite() || self.rotation_rad.abs() > 1e6 || !self.intensity.is_finite() || !(0.0..=100.0).contains(&self.intensity)
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU sky rotation must be finite within +/-1000000 radians and intensity0..100"));
        }
        Ok(())
    }
    pub fn radiance(self, direction: Vec3) -> Result<Vec3> {
        self.validate()?;
        let d = solidcraft_viewport::surface::unit_direction(direction)
            .map_err(|_| JobError::new(ErrorCode::InvalidRequest, "invalid CPU sky direction"))?;
        let row = ((d.z.clamp(-1.0, 1.0).acos() / std::f64::consts::PI * 256.0) as u32).min(255);
        let z = (std::f64::consts::PI * (f64::from(row) + 0.5) / 256.0).cos();
        let [r, g, b] = self.gradient.evaluate(z);
        // Rotation is retained/validated, as in GPU intent. This particular
        // world-Z gradient is axisymmetric, so longitude rotation is an identity.
        Ok(Vec3::new(f64::from(r), f64::from(g), f64::from(b)) * self.intensity)
    }
}
