//! Original bounded homogeneous participating-medium intent. World millimetres;
//! scalar extinction and RGB scattering albedo. Not a density grid, atmosphere,
//! spectral model, surface subsurface response, or measured material property.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HomogeneousVolume {
    pub min_mm: [f64; 3],
    pub max_mm: [f64; 3],
    pub extinction_per_mm: f64,
    pub scattering_albedo: [f64; 3],
    /// Mean cosine using propagation directions; positive means forward scattering.
    pub anisotropy: f64,
}
impl Default for HomogeneousVolume {
    fn default() -> Self {
        Self { min_mm: [-100.0; 3], max_mm: [100.0; 3], extinction_per_mm: 0.001, scattering_albedo: [0.8; 3], anisotropy: 0.0 }
    }
}
impl HomogeneousVolume {
    pub fn active(self) -> bool {
        self.extinction_per_mm > 0.0
    }
    pub fn validate(self) -> Result<()> {
        if self.min_mm.into_iter().zip(self.max_mm).any(|(a, b)| !a.is_finite() || !b.is_finite() || a.abs() > 1e9 || b.abs() > 1e9 || b - a < 1e-6)
            || !self.extinction_per_mm.is_finite()
            || !(0.0..=1000.0).contains(&self.extinction_per_mm)
            || self.scattering_albedo.iter().any(|a| !a.is_finite() || !(0.0..=1.0).contains(a))
            || !self.anisotropy.is_finite()
            || !(-0.95..=0.95).contains(&self.anisotropy)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "volume requires finite ordered world-mm bounds within +/-1e9, extents >=0.000001 mm, extinction 0..1000/mm, albedo 0..1 and anisotropy -0.95..0.95",
            ));
        }
        Ok(())
    }
    pub fn relative_bounds(self, target: [f64; 3]) -> Result<[[f32; 3]; 2]> {
        self.validate()?;
        let min = std::array::from_fn(|i| (self.min_mm[i] - target[i]) as f32);
        let max = std::array::from_fn(|i| (self.max_mm[i] - target[i]) as f32);
        if min.into_iter().zip(max).any(|(a, b)| !a.is_finite() || !b.is_finite() || b <= a) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "volume bounds collapse at GPU camera-relative precision"));
        }
        let scale = min.into_iter().chain(max).map(|v| f64::from(v.abs())).fold(1.0, f64::max);
        if self.active() && self.extinction_per_mm.recip() < scale / 1_048_576.0 {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "volume mean free path is below the admitted GPU position precision; reduce extinction or move/resize the volume explicitly",
            ));
        }
        Ok([min, max])
    }
    /// Independent f64 ray/box segment oracle. Direction is a unit propagation vector.
    pub fn segment(self, origin: [f64; 3], direction: [f64; 3], limit_mm: f64) -> Result<[f64; 2]> {
        self.validate()?;
        let norm: f64 = direction.iter().map(|d| d * d).sum();
        if origin.iter().chain(direction.iter()).any(|v| !v.is_finite()) || !limit_mm.is_finite() || limit_mm < 0.0 || (norm - 1.0).abs() > 1e-9 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "volume ray needs finite origin, unit direction and nonnegative finite length"));
        }
        let mut near: f64 = 0.0;
        let mut far = limit_mm;
        for axis in 0..3 {
            let (o, d) = (origin[axis], direction[axis]);
            if d == 0.0 {
                if o < self.min_mm[axis] || o > self.max_mm[axis] {
                    return Ok([0.0, 0.0]);
                }
            } else {
                let a = (self.min_mm[axis] - o) / d;
                let b = (self.max_mm[axis] - o) / d;
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            if near >= far {
                return Ok([0.0, 0.0]);
            }
        }
        Ok([near, far])
    }
    pub fn transmittance(self, distance_mm: f64) -> Result<f64> {
        self.validate()?;
        if !distance_mm.is_finite() || distance_mm < 0.0 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "volume distance must be finite and nonnegative"));
        }
        Ok((-self.extinction_per_mm * distance_mm).exp())
    }
    pub fn phase(self, propagation_cosine: f64) -> Result<f64> {
        self.validate()?;
        if !propagation_cosine.is_finite() || !(-1.0..=1.0).contains(&propagation_cosine) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "volume phase cosine must be -1..1"));
        }
        let g = self.anisotropy;
        Ok((1.0 - g * g) / (4.0 * std::f64::consts::PI * (1.0 + g * g - 2.0 * g * propagation_cosine).powf(1.5)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn box_lengths_and_beer_attenuation_are_world_scale_exact() {
        let medium = HomogeneousVolume { min_mm: [-1.0; 3], max_mm: [1.0; 3], extinction_per_mm: 0.5, ..Default::default() };
        assert_eq!(medium.segment([0.0, 0.0, 4.0], [0.0, 0.0, -1.0], 100.0).unwrap(), [3.0, 5.0]);
        assert_eq!(medium.segment([0.0; 3], [1.0, 0.0, 0.0], 100.0).unwrap(), [0.0, 1.0]);
        assert_eq!(medium.segment([2.0, 0.0, 4.0], [0.0, 0.0, -1.0], 100.0).unwrap(), [0.0, 0.0]);
        assert_eq!(medium.segment([0.0, 0.0, 4.0], [0.0, 0.0, -1.0], 4.0).unwrap(), [3.0, 4.0]);
        assert_eq!(medium.transmittance(2.0).unwrap(), (-1.0_f64).exp());
        assert!((medium.transmittance(0.75).unwrap() * medium.transmittance(1.25).unwrap() - medium.transmittance(2.0).unwrap()).abs() < 1e-15);
    }
    #[test]
    fn phase_is_normalized_with_its_declared_mean_cosine() {
        for g in [-0.9, -0.5, 0.0, 0.5, 0.9] {
            let medium = HomogeneousVolume { anisotropy: g, ..Default::default() };
            let mut integral = 0.0;
            let mut moment = 0.0;
            let steps = 100_000;
            for i in 0..steps {
                let cosine = -1.0 + 2.0 * (f64::from(i) + 0.5) / f64::from(steps);
                let density = medium.phase(cosine).unwrap() * 4.0 * std::f64::consts::PI / f64::from(steps);
                integral += density;
                moment += density * cosine;
            }
            assert!((integral - 1.0).abs() < 2e-6, "g={g} integral={integral}");
            assert!((moment - g).abs() < 2e-6, "g={g} moment={moment}");
        }
    }
    #[test]
    fn invalid_media_and_gpu_collapsed_extents_are_refused_not_clamped() {
        for density in [f64::NAN, f64::INFINITY, -0.1, 1000.1] {
            assert!(HomogeneousVolume { extinction_per_mm: density, ..Default::default() }.validate().is_err());
        }
        assert!(HomogeneousVolume { anisotropy: 1.0, ..Default::default() }.validate().is_err());
        assert!(HomogeneousVolume { scattering_albedo: [1.01; 3], ..Default::default() }.validate().is_err());
        assert!(HomogeneousVolume { min_mm: [1.0; 3], max_mm: [1.0; 3], ..Default::default() }.validate().is_err());
        let tiny = HomogeneousVolume { min_mm: [1e8; 3], max_mm: [1e8 + 0.001; 3], ..Default::default() };
        assert!(tiny.validate().is_ok());
        assert!(tiny.relative_bounds([0.0; 3]).is_err());
        assert!(tiny.relative_bounds([1e8; 3]).is_ok());
        assert!(!HomogeneousVolume { extinction_per_mm: 0.0, ..Default::default() }.active());
        assert!(serde_json::from_str::<HomogeneousVolume>(r#"{"density_grid":"arbitrary"}"#).is_err());
    }
}
