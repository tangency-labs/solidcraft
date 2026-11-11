//! Prepaid optional pilot planes. No production sample enters these estimates.
use crate::{
    job::{
        CancelToken, ErrorCode, JobError, Result,
        adaptive::{AdaptiveAllocation, AdaptiveSampling, PilotEvidenceLayout, PilotPhotonSupport, PilotPixelEvidence},
    },
    radiance::RadianceComponents,
};

const MAX_PLANES: usize = 38;
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn limit(message: &str) -> JobError {
    JobError::new(ErrorCode::LimitExceeded, message)
}

#[derive(Default)]
pub(super) struct EvidenceFilm {
    layout: PilotEvidenceLayout,
    values: Vec<[f64; 4]>,
    count: usize,
    refreshed_maps: bool,
    configured: bool,
}
pub(super) struct Update {
    values: [[f64; 4]; MAX_PLANES],
    pub allocation: Option<AdaptiveAllocation>,
    pub summary: PilotPixelEvidence,
}
impl EvidenceFilm {
    pub fn bytes(pixels: u64, options: AdaptiveSampling) -> Result<u64> {
        pixels.checked_mul(u64::from(PilotEvidenceLayout::new(options).planes()) * 32).ok_or_else(|| limit("pilot evidence storage overflow"))
    }
    pub fn new(count: usize, options: AdaptiveSampling, cancel: &CancelToken) -> Result<Self> {
        let layout = PilotEvidenceLayout::new(options);
        let capacity = count.checked_mul(layout.planes() as usize).ok_or_else(|| limit("pilot evidence exceeds address space"))?;
        let mut values = Vec::new();
        cancel.check()?;
        values.try_reserve_exact(capacity).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve optional pilot evidence"))?;
        if values.capacity() != capacity {
            return Err(limit("pilot evidence capacity exceeds admission"));
        }
        Ok(Self { layout, values, count, refreshed_maps: false, configured: options.map_validation_batches == 0 })
    }
    pub fn configure_maps(&mut self, refreshed: bool) {
        self.refreshed_maps = refreshed;
        self.configured = true;
    }
    pub fn initialize_one(&mut self) {
        for _ in 0..self.layout.planes() {
            self.values.push([0.0; 4]);
        }
    }
    pub fn enabled(&self) -> bool {
        self.layout.groups != 0
    }
    pub fn ready(&self) -> bool {
        self.values.len() == self.count * self.layout.planes() as usize && self.configured
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        index: usize,
        ordinal: u32,
        beauty: [f64; 4],
        components: Option<&RadianceComponents>,
        support: PilotPhotonSupport,
        options: AdaptiveSampling,
        maximum: u32,
    ) -> Result<Update> {
        if !self.enabled() || !self.ready() || ordinal >= options.pilot_samples {
            return Err(invalid("pilot evidence is not ready for this observation"));
        }
        if options.component_uncertainty && components.is_none() {
            return Err(invalid("component pilot observation was not captured"));
        }
        if options.component_uncertainty
            && let Some(components) = components
        {
            for (sum, beauty) in components.beauty().into_iter().zip(beauty) {
                if !sum.is_finite() || (sum - beauty).abs() > 1e-10 * beauty.abs().max(1.0) {
                    return Err(invalid("pilot radiance components do not reproduce the complete observation"));
                }
            }
        }
        let planes = self.layout.planes() as usize;
        let first = index.checked_mul(planes).ok_or_else(|| limit("pilot evidence offset overflow"))?;
        let source = self.values.get(first..first + planes).ok_or_else(|| invalid("pilot evidence pixel is outside admission"))?;
        let mut update = Update { values: [[0.0; 4]; MAX_PLANES], allocation: None, summary: PilotPixelEvidence::default() };
        update.values[..planes].copy_from_slice(source);
        let batches = options.map_validation_batches;
        let per_batch = options.pilot_samples.checked_div(batches).unwrap_or(options.pilot_samples);
        let local_count = ordinal % per_batch + 1;
        let batch_count = ordinal / per_batch + 1;
        let last = ordinal + 1 == options.pilot_samples;
        let mut allocation = AdaptiveAllocation { samples: options.min_samples, budget_limited: false };
        for group in 0..self.layout.groups {
            let value = if group == 0 {
                beauty
            } else {
                let rgb = components
                    .and_then(|values| values.values().get(group as usize - 1))
                    .copied()
                    .ok_or_else(|| invalid("component pilot partition missing"))?;
                [rgb[0], rgb[1], rgb[2], 0.0]
            };
            let base = (group * self.layout.moments_per_group) as usize;
            for (channel, value) in value.into_iter().enumerate().take(if group == 0 { 4 } else { 3 }) {
                let delta = value - update.values[base][channel];
                update.values[base][channel] += delta / f64::from(local_count);
                update.values[base + 1][channel] += delta * (value - update.values[base][channel]);
                if batches > 0 && local_count == per_batch {
                    let batch_mean = update.values[base][channel];
                    let delta = batch_mean - update.values[base + 2][channel];
                    update.values[base + 2][channel] += delta / f64::from(batch_count);
                    update.values[base + 3][channel] += delta * (batch_mean - update.values[base + 2][channel]);
                    update.values[base + 4][channel] += update.values[base + 1][channel];
                    if !last {
                        update.values[base][channel] = 0.0;
                        update.values[base + 1][channel] = 0.0;
                    }
                }
                if last {
                    let (mean, within, map_variance) = if batches == 0 {
                        (update.values[base][channel], update.values[base + 1][channel] / f64::from(options.pilot_samples - 1), 0.0)
                    } else {
                        let within = update.values[base + 4][channel] / f64::from(batches * (per_batch - 1));
                        let between = update.values[base + 3][channel] / f64::from(batches - 1);
                        (update.values[base + 2][channel], within, (between - within / f64::from(per_batch)).max(0.0))
                    };
                    let variance = within + if self.refreshed_maps { map_variance } else { 0.0 };
                    let valid = update.values[base..base + self.layout.moments_per_group as usize]
                        .iter()
                        .all(|values| values[channel].is_finite() && values[channel] >= 0.0)
                        && mean.is_finite()
                        && mean >= 0.0
                        && within.is_finite()
                        && within >= 0.0
                        && map_variance.is_finite()
                        && variance.is_finite();
                    let next = if valid {
                        options.variance_allocation(mean, variance, group, channel, maximum)
                    } else {
                        AdaptiveAllocation { samples: maximum, budget_limited: true }
                    };
                    allocation.samples = allocation.samples.max(next.samples);
                    allocation.budget_limited |= next.budget_limited;
                    update.summary.component_budget_limited |= group > 0 && next.budget_limited;
                    update.summary.invalid |= !valid;
                    if valid {
                        update.summary.map_scatter_to_tolerance =
                            update.summary.map_scatter_to_tolerance.max(map_variance.sqrt() / options.evidence_tolerance(mean, group, channel));
                    }
                }
            }
        }
        let support_plane = self.layout.moment_planes() as usize;
        let previous = update.values[support_plane];
        let observed = support.queries > 0;
        let minimum = match (previous[2] > 0.0, support.minimum_effective_event_support) {
            (true, Some(value)) => previous[2].min(value),
            (true, None) => previous[2],
            (false, Some(value)) => value,
            (false, None) => 0.0,
        };
        update.values[support_plane] = [
            previous[0] + f64::from(u8::from(observed)),
            previous[1] + f64::from(u8::from(observed && support.positive_queries == 0)),
            minimum,
            f64::from(u8::from(previous[3] > 0.0 || support.invalid_queries > 0)),
        ];
        if last {
            let support = update.values[support_plane];
            update.summary.query_observations = support[0] as u32;
            update.summary.empty_observations = support[1] as u32;
            update.summary.minimum_query_effective_event_support = (support[2] > 0.0).then_some(support[2]);
            update.summary.invalid |= support[3] > 0.0;
            if update.summary.invalid {
                allocation = AdaptiveAllocation { samples: maximum, budget_limited: true };
            }
            update.values[support_plane + 2] = [
                f64::from(u8::from(update.summary.component_budget_limited)),
                update.summary.map_scatter_to_tolerance,
                f64::from(u8::from(update.summary.invalid)),
                0.0,
            ];
            update.allocation = Some(allocation);
        }
        Ok(update)
    }
    /// Called only after the owning film transaction has passed all fallible gates.
    pub fn commit(&mut self, index: usize, update: Update) {
        let planes = self.layout.planes() as usize;
        if let Some(destination) = self.values.get_mut(index * planes..(index + 1) * planes) {
            destination.copy_from_slice(&update.values[..planes]);
        }
    }
}
