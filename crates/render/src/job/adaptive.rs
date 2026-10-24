//! Two-stage adaptive allocation. Independent pilot samples choose a bounded
//! production budget; pilot radiance is discarded, never stopped into the final
//! mean. Error thresholds guide allocation, not a certified convergence bound.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdaptiveSampling {
    pub pilot_samples: u32,
    pub min_samples: u32,
    pub relative_error: f64,
    pub absolute_error: f64,
    pub coverage_error: f64,
}
impl Default for AdaptiveSampling {
    fn default() -> Self {
        Self { pilot_samples: 32, min_samples: 32, relative_error: 0.05, absolute_error: 0.002, coverage_error: 0.01 }
    }
}
impl AdaptiveSampling {
    pub fn validate(self, maximum: u32) -> Result<()> {
        if !(16..=256).contains(&self.pilot_samples)
            || !(16..=maximum).contains(&self.min_samples)
            || maximum > 4096
            || !self.relative_error.is_finite()
            || !(0.0..=1.0).contains(&self.relative_error)
            || !self.absolute_error.is_finite()
            || !(1e-6..=1e6).contains(&self.absolute_error)
            || !self.coverage_error.is_finite()
            || !(0.001..=0.25).contains(&self.coverage_error)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "adaptive allocation needs 16..256 pilots, 16..maximum production samples, relative error 0..1, absolute error 0.000001..1000000 and coverage error 0.001..0.25",
            ));
        }
        Ok(())
    }
    /// Independent f64 allocation oracle for the native f32 implementation.
    /// Nonfinite pilot estimates select the cap rather than false convergence.
    pub fn target(self, mean: [f64; 4], m2: [f64; 4], maximum: u32) -> Result<u32> {
        self.validate(maximum)?;
        let mut target = self.min_samples;
        for (channel, (mean, m2)) in mean.into_iter().zip(m2).enumerate() {
            if !mean.is_finite() || !m2.is_finite() || mean < 0.0 || m2 < 0.0 {
                return Ok(maximum);
            }
            let tolerance = if channel == 3 { self.coverage_error } else { self.absolute_error + self.relative_error * mean };
            let ratio = (m2 / f64::from(self.pilot_samples - 1)).sqrt() / tolerance;
            let estimate = ratio.min(f64::from(maximum).sqrt()).powi(2).ceil() as u32;
            target = target.max(estimate.min(maximum));
        }
        Ok(target)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SamplingReport {
    pub pilot_samples_per_pixel: u32,
    pub minimum_production_samples: u32,
    pub maximum_production_samples: u32,
    pub production_samples: u64,
    pub primary_path_evaluations: u64,
    pub pixels_at_cap: u64,
    pub transmission_strata: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_variance_uses_minimum_but_coverage_and_chroma_can_require_the_cap() {
        let p = AdaptiveSampling::default();
        assert_eq!(p.target([1.0; 4], [0.0; 4], 128).unwrap(), 32);
        assert_eq!(p.target([1.0; 4], [0.0, 0.0, 0.0, 31.0], 128).unwrap(), 128);
        assert_eq!(p.target([1.0; 4], [31.0, 0.0, 0.0, 0.0], 128).unwrap(), 128);
        assert_eq!(p.target([1.0; 4], [f64::INFINITY; 4], 128).unwrap(), 128);
    }
    #[test]
    fn standard_error_allocation_matches_independent_closed_form() {
        let p = AdaptiveSampling { relative_error: 0.0, absolute_error: 0.1, ..Default::default() };
        assert_eq!(p.target([1.0; 4], [31.0 * 0.64, 0.0, 0.0, 0.0], 128).unwrap(), 64);
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 1.01] {
            assert!(AdaptiveSampling { relative_error: invalid, ..p }.validate(128).is_err());
        }
        assert!(p.validate(16).is_err());
        assert!(AdaptiveSampling { pilot_samples: 1, ..p }.validate(128).is_err());
        assert!(AdaptiveSampling { absolute_error: 0.0, ..p }.validate(128).is_err());
        assert!(serde_json::from_str::<AdaptiveSampling>(r#"{"noise_guarantee":true}"#).is_err());
    }
}
