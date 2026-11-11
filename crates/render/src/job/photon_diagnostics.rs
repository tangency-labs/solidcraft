//! Bounded photon-estimator probes. Diagnostics never change transport.
//! Receiver-local and camera-weighted top lists describe one selected sample;
//! optional camera accounting follows one output pixel's complete trace footprint.
use super::{ErrorCode, JobError, Result};
use serde::Serialize;
#[path = "photon_accounting.rs"]
mod accounting;
use super::photon_support::PhotonContributionSupport;
use crate::radiance::CameraPath;
use accounting::PhotonLaneAccounting;
pub use accounting::{
    MAX_PROBE_PIXELS, PhotonCameraAccounting, PhotonEstimatorPartition, PhotonOutputAccounting, PhotonPixelAccounting, PhotonSampleAccounting,
};

pub const TOP_CONTRIBUTIONS: usize = 8;
pub const HISTOGRAM_BINS: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum EmissionMeasure {
    #[default]
    Unknown = 0,
    DiskArea = 1,
    SolidAngle = 2,
    AreaSolidAngle = 3,
    DiskAreaSolidAngle = 4,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonContribution {
    pub emitted_path: u32,
    pub source: u32,
    pub source_probability: f64,
    pub emission_density: f64,
    pub emission_measure: EmissionMeasure,
    pub light_depth: u32,
    pub reservoir_visits: u32,
    /// 1: opaque surface, 2: rough dielectric, 3: volume.
    pub receiver_kind: u32,
    pub receiver: u32,
    /// Zero-based camera lane; local values have not been averaged across lanes.
    /// In split mode lanes force reflection/transmission at the selected interface.
    pub stratum: u32,
    /// Flux includes the receiver-reservoir weight and emitted-photon normalization.
    pub photon_flux: [f64; 3],
    /// BRDF, BTDF or phase response before the surface measure correction.
    pub response: [f64; 3],
    pub normal_measure: f64,
    pub radius_mm: f64,
    /// Radiance at this receiver, before camera-path throughput and Fresnel-lane averaging.
    pub radiance: [f64; 3],
    /// Complete camera-prefix multiplier, before this receiving event.
    pub camera_prefix: [f64; 3],
    /// Receiver-local radiance multiplied by the prefix, before lane averaging.
    pub camera_radiance: [f64; 3],
    /// Effective lane count, including two forced lanes in split mode.
    pub fresnel_strata: u32,
    /// Camera scattering/interface events completed before this receiver.
    pub camera_depth: u32,
    /// Explicit dielectric camera-interface events before the receiver. An
    /// analytically collapsed smooth sheet is one represented transport event.
    pub camera_interfaces: u32,
    pub camera_path: CameraPath,
    pub receiver_roughness: Option<f64>,
    pub gather: u32,
    pub query_eligible: u32,
    pub query_contributing: u32,
    /// Inverse weight concentration of positive events, never an IID photon count.
    pub support: PhotonContributionSupport,
    /// Descriptive inverse normalized weight concentration of this query's
    /// positive events. It is not an independent effective photon sample count.
    pub query_effective_event_support: Option<f64>,
    /// Selected event's share of the final raw output mean; absent until final counts.
    pub final_output_radiance: Option<[f64; 3]>,
}

impl PhotonContribution {
    pub fn luminance(&self) -> f64 {
        luminance(self.radiance)
    }

    pub fn camera_luminance(&self) -> f64 {
        luminance(self.camera_radiance)
    }

    pub fn validate(&self) -> Result<()> {
        let scalars = [self.source_probability, self.emission_density, self.normal_measure, self.radius_mm];
        if scalars
            .into_iter()
            .chain(self.photon_flux)
            .chain(self.response)
            .chain(self.radiance)
            .chain(self.camera_prefix)
            .chain(self.camera_radiance)
            .any(|v| !v.is_finite() || v < 0.0)
            || self.source_probability > 1.0
            || !self.luminance().is_finite()
            || !self.camera_luminance().is_finite()
            || !matches!(self.fresnel_strata, 1 | 2 | 4)
            || self.stratum >= self.fresnel_strata
            || self.receiver_roughness.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || (self.support.accepted > 0
                && (!self.support.maximum_luminance.is_finite()
                    || self.support.maximum_luminance <= 0.0
                    || !self.support.normalized_sum.is_finite()
                    || self.support.normalized_sum < 1.0
                    || !self.support.normalized_squared_sum.is_finite()
                    || self.support.normalized_squared_sum < 1.0
                    || self.support.normalized_sum > f64::from(self.support.accepted) * (1.0 + 1e-5)
                    || self.support.normalized_squared_sum > self.support.normalized_sum * (1.0 + 1e-5)))
            || self.query_effective_event_support.is_some_and(|value| !value.is_finite() || value < 1.0 || value > f64::from(self.support.accepted))
            || self.final_output_radiance.is_some_and(|value| value.into_iter().any(|v| !v.is_finite() || v < 0.0))
        {
            return Err(JobError::new(ErrorCode::InvalidScene, "photon probe contains nonfinite or negative transport"));
        }
        Ok(())
    }
}

/// A selected production camera sample, accumulated over its receiver queries.
/// The histogram describes positive individual receiver-local contributions.
/// Bin 0 includes values below 2^-15; bin 31 includes values at least 2^15.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonDiagnostics {
    /// The configured first-interface split strategy. An individual path may
    /// reach no eligible boundary; strata fields then still count its two lanes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub split_first_interface: bool,
    pub selected: bool,
    pub pixel: [u32; 2],
    pub sample: u32,
    pub epoch: u32,
    pub seed: u32,
    pub radii_mm: [f64; 2],
    pub gathers: u32,
    pub visited: u32,
    pub eligible: u32,
    pub contributing: u32,
    /// Sum of receiver-local events across selected lanes, not final pixel radiance.
    pub local_radiance_sum: [f64; 3],
    pub histogram: [u32; HISTOGRAM_BINS],
    pub top_count: u32,
    pub largest: [PhotonContribution; TOP_CONTRIBUTIONS],
    /// Prefix-weighted selected-sample events before lane averaging.
    pub camera_radiance_sum: [f64; 3],
    pub camera_contributing: u32,
    pub camera_histogram: [u32; HISTOGRAM_BINS],
    pub camera_top_count: u32,
    pub largest_camera: [PhotonContribution; TOP_CONTRIBUTIONS],
    pub camera: PhotonCameraAccounting,
}

fn luminance(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}

fn add_count(target: &mut u32, count: u32) -> Result<()> {
    *target = target.checked_add(count).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "photon diagnostic counter overflow"))?;
    Ok(())
}

impl PhotonDiagnostics {
    pub fn new(pixel: [u32; 2], sample: u32, epoch: u32, seed: u32, radii_mm: [f64; 2]) -> Self {
        Self { selected: true, pixel, sample, epoch, seed, radii_mm, ..Self::default() }
    }

    pub fn visit(&mut self) -> Result<()> {
        add_count(&mut self.visited, 1)
    }

    pub fn begin_gather(&mut self) -> Result<()> {
        add_count(&mut self.gathers, 1)
    }

    fn keep_largest(&mut self, contribution: PhotonContribution, camera: bool) {
        let (entries, count) = if camera { (&mut self.largest_camera, &mut self.camera_top_count) } else { (&mut self.largest, &mut self.top_count) };
        let score = |entry: PhotonContribution| if camera { entry.camera_luminance() } else { entry.luminance() };
        let mut carry = contribution;
        for entry in entries {
            if score(carry) > score(*entry) {
                std::mem::swap(entry, &mut carry);
            }
        }
        *count = (*count + 1).min(TOP_CONTRIBUTIONS as u32);
    }

    pub fn observe(&mut self, contribution: PhotonContribution) -> Result<()> {
        contribution.validate()?;
        add_count(&mut self.eligible, 1)?;
        let value = contribution.luminance();
        if value <= 0.0 {
            return Ok(());
        }
        add_count(&mut self.contributing, 1)?;
        let bin = (value.log2().floor() + 16.0).clamp(0.0, (HISTOGRAM_BINS - 1) as f64) as usize;
        let count = self.histogram.get_mut(bin).ok_or_else(|| JobError::new(ErrorCode::Internal, "photon histogram index"))?;
        add_count(count, 1)?;
        for (sum, value) in self.local_radiance_sum.iter_mut().zip(contribution.radiance) {
            *sum += value;
            if !sum.is_finite() {
                return Err(JobError::new(ErrorCode::InvalidScene, "photon diagnostic radiance overflow"));
            }
        }
        self.keep_largest(contribution, false);
        let camera_value = contribution.camera_luminance();
        if camera_value > 0.0 {
            add_count(&mut self.camera_contributing, 1)?;
            let bin = (camera_value.log2().floor() + 16.0).clamp(0.0, (HISTOGRAM_BINS - 1) as f64) as usize;
            add_count(&mut self.camera_histogram[bin], 1)?;
            for (sum, value) in self.camera_radiance_sum.iter_mut().zip(contribution.camera_radiance) {
                *sum += value;
                if !sum.is_finite() {
                    return Err(JobError::new(ErrorCode::InvalidScene, "photon camera radiance overflow"));
                }
            }
            self.keep_largest(contribution, true);
        }
        Ok(())
    }

    pub fn merge(&mut self, other: &Self) -> Result<()> {
        let mut camera = self.camera;
        camera.merge(&other.camera)?;
        if !other.selected {
            self.camera = camera;
            return Ok(());
        }
        if !self.selected {
            *self = *other;
            self.camera = camera;
            return Ok(());
        }
        if self.pixel != other.pixel
            || self.sample != other.sample
            || self.epoch != other.epoch
            || self.seed != other.seed
            || self.radii_mm != other.radii_mm
        {
            return Err(JobError::new(ErrorCode::Internal, "photon diagnostic sample identities disagree"));
        }
        let first_gather = self.gathers;
        add_count(&mut self.gathers, other.gathers)?;
        add_count(&mut self.visited, other.visited)?;
        add_count(&mut self.eligible, other.eligible)?;
        add_count(&mut self.contributing, other.contributing)?;
        for (target, source) in self.histogram.iter_mut().zip(other.histogram) {
            add_count(target, source)?;
        }
        for (target, source) in self.local_radiance_sum.iter_mut().zip(other.local_radiance_sum) {
            *target += source;
            if !target.is_finite() {
                return Err(JobError::new(ErrorCode::InvalidScene, "photon diagnostic radiance overflow"));
            }
        }
        for entry in other.largest.iter().take(other.top_count as usize) {
            let mut entry = *entry;
            add_count(&mut entry.gather, first_gather)?;
            self.keep_largest(entry, false);
        }
        add_count(&mut self.camera_contributing, other.camera_contributing)?;
        for (target, source) in self.camera_histogram.iter_mut().zip(other.camera_histogram) {
            add_count(target, source)?;
        }
        for (target, source) in self.camera_radiance_sum.iter_mut().zip(other.camera_radiance_sum) {
            *target += source;
            if !target.is_finite() {
                return Err(JobError::new(ErrorCode::InvalidScene, "photon camera radiance overflow"));
            }
        }
        for entry in other.largest_camera.iter().take(other.camera_top_count as usize) {
            let mut entry = *entry;
            add_count(&mut entry.gather, first_gather)?;
            self.keep_largest(entry, true);
        }
        self.camera = camera;
        Ok(())
    }

    /// Bracket a positive-contribution quantile using histogram edges.
    /// Open-ended bins return None for the corresponding bound.
    pub fn quantile_bounds(&self, quantile: f64) -> Result<Option<[Option<f64>; 2]>> {
        if !quantile.is_finite() || !(0.0..=1.0).contains(&quantile) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "photon quantile must be finite in 0..1"));
        }
        if self.contributing == 0 {
            return Ok(None);
        }
        let rank = (quantile * f64::from(self.contributing)).ceil().max(1.0) as u32;
        let mut cumulative = 0u64;
        for (index, count) in self.histogram.iter().copied().enumerate() {
            cumulative += u64::from(count);
            if cumulative >= u64::from(rank) {
                return Ok(Some([
                    (index != 0).then(|| 2.0_f64.powi(index as i32 - 16)),
                    (index + 1 != HISTOGRAM_BINS).then(|| 2.0_f64.powi(index as i32 - 15)),
                ]));
            }
        }
        Err(JobError::new(ErrorCode::Internal, "photon histogram and contribution count disagree"))
    }
}

/// Identity of one production camera lane. Pilots never reach these methods.
#[derive(Clone, Copy)]
pub struct PhotonCameraSample {
    pub pixel: [u32; 2],
    pub sample: u32,
    pub epoch: u32,
    pub stratum: u32,
    pub strata: u32,
}

/// Optional prepaid owner for detailed queries and one output pixel's aggregate.
/// Keeping reports and lane scratch off image/worker stacks preserves layouts.
#[derive(Default)]
pub struct PhotonProbe {
    states: Vec<ProbeState>,
}
struct ProbeState {
    report: PhotonDiagnostics,
    lanes: [PhotonLaneAccounting; MAX_PROBE_PIXELS],
}
impl PhotonProbe {
    pub fn required_bytes(options: Option<super::CausticOptions>) -> u64 {
        if options.is_some_and(|options| options.probe_pixel.is_some()) {
            // Owner, reusable query slot, and bounded merge/report copies can
            // overlap. Worker admission charges this allowance per worker too.
            (std::mem::size_of::<ProbeState>() + 3 * std::mem::size_of::<PhotonDiagnostics>() + 2 * std::mem::size_of::<PhotonEstimatorPartition>())
                as u64
        } else {
            0
        }
    }

    pub fn new(options: Option<super::CausticOptions>) -> Result<Self> {
        Self::with_supersampling(options, 1)
    }

    pub fn with_supersampling(options: Option<super::CausticOptions>, scale: u32) -> Result<Self> {
        let Some(options) = options.filter(|options| options.probe_pixel.is_some()) else { return Ok(Self::default()) };
        options.validate()?;
        let pixel = options.probe_pixel.ok_or_else(|| JobError::new(ErrorCode::Internal, "photon probe pixel disappeared"))?;
        let mut states = Vec::new();
        states.try_reserve_exact(1).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve bounded photon probe"))?;
        if states.capacity() != 1 {
            return Err(JobError::new(ErrorCode::LimitExceeded, "photon probe capacity exceeds its admission"));
        }
        states.push(ProbeState {
            report: PhotonDiagnostics {
                pixel,
                sample: options.probe_sample,
                seed: options.seed,
                radii_mm: options.radii_mm(0)?,
                camera: PhotonCameraAccounting::new(pixel, scale)?,
                ..Default::default()
            },
            lanes: Default::default(),
        });
        Ok(Self { states })
    }

    pub fn reset(&mut self) {
        if let Some(state) = self.states.first_mut() {
            let old = state.report;
            let mut camera = old.camera;
            for pixel in &mut camera.trace_pixels {
                *pixel = PhotonPixelAccounting { pixel: pixel.pixel, ..Default::default() };
            }
            camera.selected_sample = None;
            camera.output = None;
            state.report =
                PhotonDiagnostics { pixel: old.pixel, sample: old.sample, seed: old.seed, radii_mm: old.radii_mm, camera, ..Default::default() };
            state.lanes = Default::default();
        }
    }

    pub fn enabled(&self) -> bool {
        !self.states.is_empty()
    }

    pub fn captures_pixel(&self, pixel: [u32; 2]) -> bool {
        self.states.first().is_some_and(|state| state.report.camera.slot(pixel).is_some())
    }

    pub fn record_radiance(&mut self, identity: PhotonCameraSample, photon: bool, radiance: [f64; 3]) -> Result<()> {
        let Some(state) = self.states.first_mut() else { return Ok(()) };
        let Some(slot) = state.report.camera.slot(identity.pixel) else { return Ok(()) };
        state.lanes[slot].record(identity.sample, identity.stratum, identity.strata, photon, radiance)
    }

    pub fn complete_lane(&mut self, identity: PhotonCameraSample, beauty: [f64; 3], covered: bool, radii_mm: [f64; 2]) -> Result<()> {
        let Some(state) = self.states.first_mut() else { return Ok(()) };
        let Some(slot) = state.report.camera.slot(identity.pixel) else { return Ok(()) };
        if let Some(sample) = state.lanes[slot].complete(identity.sample, identity.stratum, identity.strata, beauty, covered)? {
            state.report.camera.trace_pixels[slot].observe(sample)?;
            if identity.pixel == state.report.pixel && identity.sample == state.report.sample {
                if state.report.camera.selected_sample.is_some() {
                    return Err(JobError::new(ErrorCode::Internal, "selected photon camera sample repeated"));
                }
                state.report.selected = true;
                state.report.epoch = identity.epoch;
                state.report.radii_mm = radii_mm;
                state.report.camera.selected_sample = Some(sample);
            }
        }
        Ok(())
    }

    pub fn merge(&mut self, report: &PhotonDiagnostics) -> Result<()> {
        if !report.selected && report.camera.trace_pixels.iter().all(|pixel| pixel.samples == 0) {
            return Ok(());
        }
        let state = self.states.first_mut().ok_or_else(|| JobError::new(ErrorCode::Internal, "photon probe storage was not admitted"))?;
        if state.report.pixel != report.pixel || state.report.sample != report.sample || state.report.seed != report.seed {
            return Err(JobError::new(ErrorCode::Internal, "photon probe report does not match its selected camera sample"));
        }
        state.report.merge(report)
    }

    pub fn report(&self) -> Option<PhotonDiagnostics> {
        self.states.first().map(|state| state.report)
    }

    pub fn finalize(&mut self, raw: [f32; 4], counts: [u32; MAX_PROBE_PIXELS], tolerance: f64) -> Result<()> {
        let Some(state) = self.states.first_mut() else { return Ok(()) };
        if state.lanes.iter().any(|lane| !lane.idle()) {
            return Err(JobError::new(ErrorCode::Internal, "photon probe has an incomplete camera lane"));
        }
        state.report.finalize(raw, counts, tolerance)
    }
}
impl PhotonDiagnostics {
    pub fn finalize(&mut self, raw: [f32; 4], counts: [u32; MAX_PROBE_PIXELS], tolerance: f64) -> Result<()> {
        self.camera.finalize(raw, counts, tolerance)?;
        if self.camera.selected_sample.is_some() {
            let slot = self.camera.slot(self.pixel).ok_or_else(|| JobError::new(ErrorCode::Internal, "photon selected pixel missing"))?;
            let observations = self.camera.trace_pixels[slot].samples;
            for entry in
                self.largest.iter_mut().take(self.top_count as usize).chain(self.largest_camera.iter_mut().take(self.camera_top_count as usize))
            {
                entry.query_effective_event_support = entry.support.effective_event_support();
                let weight = 1.0 / (f64::from(entry.fresnel_strata) * f64::from(observations) * f64::from(self.camera.trace_pixel_count));
                entry.final_output_radiance = Some(entry.camera_radiance.map(|value| value * weight));
            }
        }
        Ok(())
    }
}
