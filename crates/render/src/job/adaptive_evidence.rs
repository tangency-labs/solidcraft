//! Optional independent-pilot evidence contracts shared by both backends.
//! Camera observations within a map are conditional samples. Independently seeded
//! maps, repeated contribution events and production observations remain distinct.
use super::{AdaptiveAllocation, AdaptiveSampling};
use crate::job::{CausticOptions, ErrorCode, JobError, Result, caustics::CausticWork, photon_support::PhotonContributionSupport};
use serde::{Deserialize, Serialize};

/// Maximum number of independent maps admitted for the allocation assessment.
pub const MAX_MAP_VALIDATION_BATCHES: u32 = 8;
const ASSESSMENT_MAP_EPOCH: u32 = 0x8000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhotonMapIdentity {
    pub seed: u32,
    pub epoch: u32,
    pub radius_epoch: u32,
    pub assessment: bool,
}
impl PhotonMapIdentity {
    /// Assessment batches all use the radius of the first production map. They
    /// never occupy a production identity, including a fixed map at epoch zero.
    pub fn for_pass(options: CausticOptions, adaptive: Option<AdaptiveSampling>, pass: u32) -> Result<Self> {
        if pass >= crate::job::MAX_SAMPLES_PER_PIXEL + super::MAX_PILOT_SAMPLES {
            return Err(JobError::new(ErrorCode::InvalidRequest, "photon camera pass exceeds the admitted sampling domain"));
        }
        if let Some(policy) = adaptive {
            policy.validate(crate::job::MAX_SAMPLES_PER_PIXEL)?;
        }
        let batches = adaptive.map_or(0, |policy| policy.map_validation_batches);
        if let Some(policy) = adaptive.filter(|_| batches != 0) {
            let observations = policy.pilot_samples / batches;
            if observations == 0 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "photon assessment has no observations per map"));
            }
            if pass < policy.pilot_samples {
                return Ok(Self {
                    seed: options.seed,
                    epoch: ASSESSMENT_MAP_EPOCH | (pass / observations),
                    radius_epoch: if options.refresh_per_pass { policy.pilot_samples } else { 0 },
                    assessment: true,
                });
            }
        }
        let epoch = if options.refresh_per_pass { pass } else { 0 };
        Ok(Self { seed: options.seed, epoch, radius_epoch: epoch, assessment: false })
    }
}

/// Plane layout is dynamic only at admission. A group is beauty RGBA or one
/// component RGB (unused fourth channel). Each optional plane is fully prepaid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PilotEvidenceLayout {
    pub groups: u32,
    pub moments_per_group: u32,
}
impl PilotEvidenceLayout {
    pub fn new(options: AdaptiveSampling) -> Self {
        if !options.evidence_enabled() {
            return Self::default();
        }
        Self { groups: if options.component_uncertainty { 7 } else { 1 }, moments_per_group: if options.map_validation_batches > 0 { 5 } else { 2 } }
    }
    pub fn moment_planes(self) -> u32 {
        self.groups * self.moments_per_group
    }
    /// Two receiver-query summary planes and one finalized evidence plane.
    pub fn planes(self) -> u32 {
        if self.groups == 0 { 0 } else { self.moment_planes() + 3 }
    }
    /// Private component state must survive the deferred optical shader pass,
    /// even when full production component capture is disabled.
    pub fn scratch_planes(self) -> u32 {
        if self.groups > 1 { 6 } else { 0 }
    }
    pub fn gpu_planes(self) -> u32 {
        self.planes() + self.scratch_planes()
    }
}

/// Aggregate receiver-query support for one complete camera observation. Counts
/// describe query events; reused photon records do not become independent draws.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotPhotonSupport {
    pub queries: u32,
    pub positive_queries: u32,
    pub invalid_queries: u32,
    pub minimum_effective_event_support: Option<f64>,
}
impl PilotPhotonSupport {
    pub fn observe_query(&mut self, support: PhotonContributionSupport, valid: bool) {
        self.queries = self.queries.saturating_add(1);
        if !valid {
            self.invalid_queries = self.invalid_queries.saturating_add(1);
        }
        if let Some(concentration) = support.effective_event_support() {
            self.positive_queries = self.positive_queries.saturating_add(1);
            self.minimum_effective_event_support = Some(self.minimum_effective_event_support.map_or(concentration, |prior| prior.min(concentration)));
        }
    }
    pub fn merge(&mut self, other: Self) {
        self.queries = self.queries.saturating_add(other.queries);
        self.positive_queries = self.positive_queries.saturating_add(other.positive_queries);
        self.invalid_queries = self.invalid_queries.saturating_add(other.invalid_queries);
        if let Some(value) = other.minimum_effective_event_support {
            self.minimum_effective_event_support = Some(self.minimum_effective_event_support.map_or(value, |prior| prior.min(value)));
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PilotEvidenceReport {
    pub component_uncertainty: bool,
    /// Independently seeded assessment maps, not camera paths or photon records.
    pub map_batches: u32,
    pub camera_observations_per_map: u32,
    /// Fixed assessment radii in surface/volume order. Later progressive radii
    /// can differ, so these statistics are not calibrated production uncertainty.
    pub assessment_radii_mm: Option<[f64; 2]>,
    pub refreshed_production_maps: bool,
    pub evolving_production_radii: bool,
    pub component_budget_limited_pixels: u64,
    /// Estimated single-map scatter above the allocation tolerance. This is
    /// descriptive and does not measure finite-radius bias or rare unseen paths.
    pub map_scatter_above_tolerance_pixels: u64,
    pub invalid_evidence_pixels: u64,
    pub photon_query_observations: u64,
    pub photon_empty_observations: u64,
    pub minimum_query_effective_event_support: Option<f64>,
    pub maximum_map_scatter_to_tolerance: f64,
}
impl PilotEvidenceReport {
    pub fn new(options: AdaptiveSampling, caustics: Option<CausticOptions>) -> Result<Self> {
        let batches = options.map_validation_batches;
        let mut report = Self {
            component_uncertainty: options.component_uncertainty,
            map_batches: batches,
            camera_observations_per_map: options.pilot_samples.checked_div(batches).unwrap_or(0),
            ..Default::default()
        };
        if batches > 0 {
            let caustics = caustics.ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "photon map assessment requires caustics"))?;
            let identity = PhotonMapIdentity::for_pass(caustics, Some(options), 0)?;
            report.assessment_radii_mm = Some(caustics.radii_mm(identity.radius_epoch)?);
            report.refreshed_production_maps = caustics.refresh_per_pass;
            report.evolving_production_radii = caustics.progressive_radius;
        }
        Ok(report)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotPixelEvidence {
    pub component_budget_limited: bool,
    pub map_scatter_to_tolerance: f64,
    pub invalid: bool,
    pub query_observations: u32,
    pub empty_observations: u32,
    pub minimum_query_effective_event_support: Option<f64>,
}
impl PilotEvidenceReport {
    pub fn observe_pixel(&mut self, evidence: PilotPixelEvidence) {
        self.component_budget_limited_pixels += u64::from(evidence.component_budget_limited);
        self.map_scatter_above_tolerance_pixels += u64::from(evidence.map_scatter_to_tolerance > 1.0);
        self.invalid_evidence_pixels += u64::from(evidence.invalid);
        self.photon_query_observations += u64::from(evidence.query_observations);
        self.photon_empty_observations += u64::from(evidence.empty_observations);
        self.maximum_map_scatter_to_tolerance = self.maximum_map_scatter_to_tolerance.max(evidence.map_scatter_to_tolerance);
        if let Some(value) = evidence.minimum_query_effective_event_support {
            self.minimum_query_effective_event_support = Some(self.minimum_query_effective_event_support.map_or(value, |prior| prior.min(value)));
        }
    }
}

impl AdaptiveSampling {
    pub fn evidence_enabled(self) -> bool {
        self.component_uncertainty || self.map_validation_batches > 0
    }
    pub fn validate_caustics(self, caustics: Option<CausticOptions>) -> Result<()> {
        if self.map_validation_batches > 0 && caustics.is_none() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "independent photon map assessment requires caustics"));
        }
        Ok(())
    }
    /// Complete assessment builds plus canonical production builds. No extra
    /// initial canonical map is built merely to discard it during assessment.
    pub fn photon_work(self, caustics: CausticOptions, production_samples: u32) -> Result<Option<CausticWork>> {
        self.validate(production_samples)?;
        if self.map_validation_batches == 0 {
            return caustics.refresh_work(self.pilot_samples + production_samples);
        }
        caustics.validate()?;
        let maps = self.map_validation_batches + if caustics.refresh_per_pass { production_samples } else { 1 };
        let emitted_paths = u64::from(maps) * u64::from(caustics.photons);
        Ok(Some(CausticWork { maps, emitted_paths, max_light_path_events: emitted_paths * u64::from(caustics.max_depth) }))
    }
    pub fn variance_allocation(self, mean: f64, variance: f64, group: u32, channel: usize, maximum: u32) -> AdaptiveAllocation {
        if !mean.is_finite() || mean < 0.0 || !variance.is_finite() || variance < 0.0 {
            return AdaptiveAllocation { samples: maximum, budget_limited: true };
        }
        let tolerance = self.evidence_tolerance(mean, group, channel);
        let ratio = variance.sqrt() / tolerance;
        let ceiling = f64::from(maximum).sqrt();
        AdaptiveAllocation { samples: self.min_samples.max(ratio.min(ceiling).powi(2).ceil() as u32).min(maximum), budget_limited: ratio > ceiling }
    }
    pub fn evidence_tolerance(self, mean: f64, group: u32, channel: usize) -> f64 {
        if group == 0 && channel == 3 {
            self.coverage_error
        } else {
            self.absolute_error / if group == 0 { 1.0 } else { 6.0 } + self.relative_error * mean
        }
    }
}
