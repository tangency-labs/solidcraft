//! Two-stage adaptive allocation. Independent pilot samples choose a bounded
//! production budget; pilot radiance is discarded, never stopped into the final
//! mean. Error thresholds guide allocation, not a certified convergence bound.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};
#[path = "adaptive_evidence.rs"]
mod evidence;
pub use evidence::*;

/// Maximum independent pilot samples per pixel supported by adaptive allocation.
pub const MAX_PILOT_SAMPLES: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdaptiveSampling {
    pub pilot_samples: u32,
    pub min_samples: u32,
    /// Conservative scene-wide floor when captured optical media are present.
    /// Zero preserves the existing allocation; it is not a convergence claim.
    #[serde(skip_serializing_if = "no_optical_floor")]
    pub optical_min_samples: u32,
    /// Allocate from all six independent-pilot physical components as well as beauty.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub component_uncertainty: bool,
    /// Independently seeded, equal camera batches at one fixed assessment radius.
    /// Zero preserves the original map schedule; otherwise use 2, 4 or 8 batches.
    #[serde(skip_serializing_if = "no_optical_floor")]
    pub map_validation_batches: u32,
    pub relative_error: f64,
    pub absolute_error: f64,
    pub coverage_error: f64,
}
impl Default for AdaptiveSampling {
    fn default() -> Self {
        Self {
            pilot_samples: 32,
            min_samples: 32,
            optical_min_samples: 0,
            component_uncertainty: false,
            map_validation_batches: 0,
            relative_error: 0.05,
            absolute_error: 0.002,
            coverage_error: 0.01,
        }
    }
}
fn no_optical_floor(value: &u32) -> bool {
    *value == 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdaptiveAllocation {
    pub samples: u32,
    /// The independent pilot asks for more than the admitted cap, or contains
    /// invalid moments. This does not estimate production convergence.
    pub budget_limited: bool,
}

impl AdaptiveSampling {
    /// Resolve the conservative optical floor before film construction. Using
    /// captured scene intent keeps allocation independent of production samples.
    pub fn for_optical_scene(mut self, optical: bool) -> Self {
        if optical {
            self.min_samples = self.min_samples.max(self.optical_min_samples);
        }
        self
    }
    pub fn validate(self, maximum: u32) -> Result<()> {
        if !(16..=MAX_PILOT_SAMPLES).contains(&self.pilot_samples)
            || !(16..=maximum).contains(&self.min_samples)
            || (self.optical_min_samples != 0 && !(16..=maximum).contains(&self.optical_min_samples))
            || !matches!(self.map_validation_batches, 0 | 2 | 4 | 8)
            || (self.map_validation_batches > 0
                && (!self.pilot_samples.is_multiple_of(self.map_validation_batches) || self.pilot_samples / self.map_validation_batches < 2))
            || maximum > super::MAX_SAMPLES_PER_PIXEL
            || !self.relative_error.is_finite()
            || !(0.0..=1.0).contains(&self.relative_error)
            || !self.absolute_error.is_finite()
            || !(1e-6..=1e6).contains(&self.absolute_error)
            || !self.coverage_error.is_finite()
            || !(0.001..=0.25).contains(&self.coverage_error)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "adaptive allocation needs 16..256 pilots, 16..maximum production samples, optical minimum 0 or 16..maximum, relative error 0..1, absolute error 0.000001..1000000 and coverage error 0.001..0.25; map assessment batches must be 0, 2, 4 or 8 and divide the pilots",
            ));
        }
        Ok(())
    }
    /// Independent f64 allocation oracle for the native f32 implementation.
    /// Nonfinite pilot estimates select the cap rather than false convergence.
    pub fn target(self, mean: [f64; 4], m2: [f64; 4], maximum: u32) -> Result<u32> {
        self.allocation(mean, m2, maximum).map(|allocation| allocation.samples)
    }

    pub fn allocation(self, mean: [f64; 4], m2: [f64; 4], maximum: u32) -> Result<AdaptiveAllocation> {
        self.validate(maximum)?;
        let mut target = self.min_samples;
        let mut budget_limited = false;
        for (channel, (mean, m2)) in mean.into_iter().zip(m2).enumerate() {
            if !mean.is_finite() || !m2.is_finite() || mean < 0.0 || m2 < 0.0 {
                return Ok(AdaptiveAllocation { samples: maximum, budget_limited: true });
            }
            let tolerance = if channel == 3 { self.coverage_error } else { self.absolute_error + self.relative_error * mean };
            let ratio = (m2 / f64::from(self.pilot_samples - 1)).sqrt() / tolerance;
            let ceiling = f64::from(maximum).sqrt();
            budget_limited |= ratio > ceiling;
            let estimate = ratio.min(ceiling).powi(2).ceil() as u32;
            target = target.max(estimate.min(maximum));
        }
        Ok(AdaptiveAllocation { samples: target, budget_limited })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplingReport {
    pub pilot_samples_per_pixel: u32,
    pub minimum_production_samples: u32,
    pub maximum_production_samples: u32,
    pub production_samples: u64,
    pub primary_path_evaluations: u64,
    pub pixels_at_cap: u64,
    /// Pixels whose independent pilot requested more samples than the cap.
    /// A zero value does not certify absence of rare paths or photon-map error.
    #[serde(default)]
    pub budget_limited_pixels: u64,
    pub transmission_strata: u32,
    /// Forced reflection/transmission lanes at the first bare smooth closed
    /// interface. The configured transmission_strata remains one in this mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub split_first_interface: bool,
    /// Optional independent-pilot diagnostics; never a convergence certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pilot_evidence: Option<PilotEvidenceReport>,
}

impl SamplingReport {
    /// Complete production paths per camera observation; these lanes are correlated.
    pub const fn production_lanes(&self) -> u32 {
        super::first_interface_lanes(self.transmission_strata, self.split_first_interface)
    }
}

/// Display progress for an adaptive checkpoint. Production iterations do not imply
/// a uniform sample count: each pixel stops at its own independently chosen budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStage {
    Pilot,
    Production,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AdaptivePreview {
    pub stage: PreviewStage,
    pub completed: u32,
    pub target: u32,
    pub pilot_samples: u32,
    pub minimum_production_samples: u32,
    pub maximum_production_samples: u32,
}
impl AdaptivePreview {
    /// Derived only from captured sampling options and an absolute completed pass.
    pub fn at(options: super::JobOptions, completed: u32) -> Result<Option<Self>> {
        let Some(adaptive) = options.adaptive else { return Ok(None) };
        options.preview_target()?;
        Ok(Some(Self::captured(adaptive, options.samples, completed)?))
    }
    /// The same captured statistical policy is shared by native CPU and GPU jobs.
    /// The completed ordinal spans pilots plus production; it is not a per-pixel count.
    pub fn captured(adaptive: AdaptiveSampling, maximum: u32, completed: u32) -> Result<Self> {
        adaptive.validate(maximum)?;
        let target = adaptive
            .pilot_samples
            .checked_add(maximum)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "adaptive checkpoint target overflow"))?;
        if completed == 0 || completed > target {
            return Err(JobError::new(ErrorCode::InvalidRequest, "checkpoint exceeds captured sampling passes"));
        }
        Ok({
            let pilot = completed <= adaptive.pilot_samples;
            Self {
                stage: if pilot { PreviewStage::Pilot } else { PreviewStage::Production },
                completed: if pilot { completed } else { completed - adaptive.pilot_samples },
                target: if pilot { adaptive.pilot_samples } else { maximum },
                pilot_samples: adaptive.pilot_samples,
                minimum_production_samples: adaptive.min_samples,
                maximum_production_samples: maximum,
            }
        })
    }
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

    #[test]
    fn optical_floor_is_explicit_and_pilot_budget_failures_are_not_convergence() {
        let baseline = AdaptiveSampling::default();
        let optical = AdaptiveSampling { optical_min_samples: 96, ..baseline };
        assert_eq!(optical.for_optical_scene(false).target([1.0; 4], [0.0; 4], 128).unwrap(), 32);
        assert_eq!(optical.for_optical_scene(true).target([1.0; 4], [0.0; 4], 128).unwrap(), 96);
        assert_eq!(baseline.for_optical_scene(true), baseline);
        assert!(optical.validate(64).is_err());
        assert!(AdaptiveSampling { optical_min_samples: 15, ..baseline }.validate(128).is_err());
        let options = AdaptiveSampling { relative_error: 0.0, absolute_error: 0.125, ..baseline };
        let at_cap = options.allocation([1.0; 4], [31.0, 0.0, 0.0, 0.0], 64).unwrap();
        assert_eq!(at_cap, AdaptiveAllocation { samples: 64, budget_limited: false });
        let above_cap = options.allocation([1.0; 4], [31.5, 0.0, 0.0, 0.0], 64).unwrap();
        assert_eq!(above_cap, AdaptiveAllocation { samples: 64, budget_limited: true });
        assert!(options.allocation([f64::NAN; 4], [0.0; 4], 64).unwrap().budget_limited);
        assert_eq!(serde_json::from_str::<AdaptiveSampling>(r#"{}"#).unwrap().optical_min_samples, 0);
    }

    #[test]
    fn adaptive_checkpoint_metadata_tracks_pilots_and_variable_production_iterations() {
        for maximum in [16, 64, 4096] {
            for pilots in [16, 32, 256] {
                let options = super::super::JobOptions {
                    samples: maximum,
                    adaptive: Some(AdaptiveSampling { pilot_samples: pilots, min_samples: 16, ..Default::default() }),
                    ..Default::default()
                };
                let total = options.preview_target().unwrap();
                assert_eq!(total, pilots + maximum);
                assert!(AdaptivePreview::at(options, 0).is_err());
                assert!(AdaptivePreview::at(options, total + 1).is_err());
                for ordinal in 1..=total {
                    let info = AdaptivePreview::at(options, ordinal).unwrap().unwrap();
                    assert_eq!(info.pilot_samples, pilots);
                    assert_eq!((info.minimum_production_samples, info.maximum_production_samples), (16, maximum));
                    if ordinal <= pilots {
                        assert_eq!((info.stage, info.completed, info.target), (PreviewStage::Pilot, ordinal, pilots));
                    } else {
                        assert_eq!((info.stage, info.completed, info.target), (PreviewStage::Production, ordinal - pilots, maximum));
                    }
                }
            }
        }
        assert!(
            AdaptivePreview::at(super::super::JobOptions::default(), 4096).unwrap().is_none(),
            "CPU profile checkpoints do not use GPU sampling options"
        );
    }
}
