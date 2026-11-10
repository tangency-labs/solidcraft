//! Bounded camera-estimator accounting for one output pixel's trace footprint.
//! These are estimator labels, independent of the physical caustic partition.
//! Values stay coverage-premultiplied and scene-linear through the raw resolve.
use crate::job::{ErrorCode, JobError, Result};
use serde::Serialize;

pub const MAX_PROBE_PIXELS: usize = 4;
type Rgb = [f64; 3];

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
fn finite_rgb(rgb: Rgb) -> bool {
    rgb.into_iter().all(|value| value.is_finite() && value >= 0.0)
}
fn add(target: &mut Rgb, value: Rgb) -> Result<()> {
    if !finite_rgb(value) {
        return Err(invalid("photon camera accounting received invalid radiance"));
    }
    let sum = std::array::from_fn(|axis| target[axis] + value[axis]);
    if !finite_rgb(sum) {
        return Err(invalid("photon camera accounting radiance overflow"));
    }
    *target = sum;
    Ok(())
}

/// Contributions of the photon estimator and all other estimators. Neither is a
/// physical caustic/non-caustic label. Beauty retains the actual transport value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonEstimatorPartition {
    pub photon: Rgb,
    pub other_estimators: Rgb,
    pub beauty: Rgb,
}
impl PhotonEstimatorPartition {
    pub fn residual(&self) -> Rgb {
        std::array::from_fn(|axis| self.beauty[axis] - self.photon[axis] - self.other_estimators[axis])
    }
    pub fn validate(&self, tolerance: f64) -> Result<()> {
        if !finite_rgb(self.photon)
            || !finite_rgb(self.other_estimators)
            || !finite_rgb(self.beauty)
            || !tolerance.is_finite()
            || tolerance < 0.0
            || self.residual().into_iter().zip(self.beauty).any(|(error, beauty)| error.abs() > tolerance * beauty.abs().max(1.0))
        {
            return Err(invalid("photon and other estimators do not reconstruct raw camera radiance"));
        }
        Ok(())
    }
    pub(crate) fn add_weighted(&mut self, value: Self, weight: f64) -> Result<()> {
        if !weight.is_finite() || weight <= 0.0 {
            return Err(invalid("photon camera accounting weight is invalid"));
        }
        add(&mut self.photon, value.photon.map(|v| v * weight))?;
        add(&mut self.other_estimators, value.other_estimators.map(|v| v * weight))?;
        add(&mut self.beauty, value.beauty.map(|v| v * weight))
    }
}

/// One complete production camera observation, already averaged over its lanes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonSampleAccounting {
    pub sample: u32,
    /// Effective complete camera lanes, including two forced split children.
    pub strata: u32,
    pub radiance: PhotonEstimatorPartition,
    pub coverage: f64,
}

/// Production sums for one internal trace pixel. Each pixel retains its own
/// denominator; neighboring adaptive pixels must never pool sample counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonPixelAccounting {
    pub pixel: [u32; 2],
    pub first_sample: u32,
    pub samples: u32,
    /// Fixed or independently allocated production count; known at final resolve.
    pub planned_samples: Option<u32>,
    pub radiance_sum: PhotonEstimatorPartition,
    pub covered_samples: u32,
}
impl PhotonPixelAccounting {
    pub(crate) fn observe(&mut self, sample: PhotonSampleAccounting) -> Result<()> {
        if self.samples == 0 {
            self.first_sample = sample.sample;
        }
        if self.first_sample.checked_add(self.samples) != Some(sample.sample)
            || !matches!(sample.strata, 1 | 2 | 4)
            || (sample.coverage != 0.0 && sample.coverage != 1.0)
        {
            return Err(invalid("photon camera observations are incomplete or out of order"));
        }
        self.radiance_sum.add_weighted(sample.radiance, 1.0)?;
        self.samples = self.samples.checked_add(1).ok_or_else(|| invalid("photon camera observation count overflow"))?;
        self.covered_samples =
            self.covered_samples.checked_add(u32::from(sample.coverage == 1.0)).ok_or_else(|| invalid("photon camera coverage overflow"))?;
        Ok(())
    }
    fn merge(&mut self, other: &Self) -> Result<()> {
        if self.pixel != other.pixel {
            return Err(invalid("photon camera footprint identities disagree"));
        }
        if other.samples == 0 {
            return Ok(());
        }
        if self.samples == 0 {
            *self = *other;
            return Ok(());
        }
        if self.first_sample.checked_add(self.samples) != Some(other.first_sample) {
            return Err(invalid("photon camera worker observation ranges overlap or have a gap"));
        }
        self.radiance_sum.add_weighted(other.radiance_sum, 1.0)?;
        self.samples = self.samples.checked_add(other.samples).ok_or_else(|| invalid("photon camera observation count overflow"))?;
        self.covered_samples = self.covered_samples.checked_add(other.covered_samples).ok_or_else(|| invalid("photon camera coverage overflow"))?;
        Ok(())
    }
}

/// Final output-pixel estimator means, before denoising, display transforms and
/// straight-alpha unpremultiplication. Raw RGBA comes from the actual film.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonOutputAccounting {
    pub pixel: [u32; 2],
    pub photon: Rgb,
    pub other_estimators: Rgb,
    pub accounted_beauty: Rgb,
    pub raw_rgba: [f64; 4],
    pub recombination_residual: Rgb,
}

/// Aggregate availability is independent of whether the requested detailed
/// production sample was reached. A completed output may have no selected sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PhotonCameraAccounting {
    pub supersampling: u32,
    pub output_pixel: [u32; 2],
    pub trace_pixel_count: u32,
    pub trace_pixels: [PhotonPixelAccounting; MAX_PROBE_PIXELS],
    pub selected_sample: Option<PhotonSampleAccounting>,
    pub output: Option<PhotonOutputAccounting>,
}
impl PhotonCameraAccounting {
    pub fn new(selected: [u32; 2], scale: u32) -> Result<Self> {
        if !matches!(scale, 1 | 2) {
            return Err(invalid("photon probe supersampling must be one or two"));
        }
        let output_pixel = selected.map(|v| v / scale);
        let base = output_pixel.map(|v| v * scale);
        let mut result = Self { supersampling: scale, output_pixel, trace_pixel_count: scale * scale, ..Self::default() };
        for index in 0..result.trace_pixel_count as usize {
            result.trace_pixels[index].pixel = [base[0] + index as u32 % scale, base[1] + index as u32 / scale];
        }
        Ok(result)
    }
    pub fn slot(&self, pixel: [u32; 2]) -> Option<usize> {
        self.trace_pixels[..self.trace_pixel_count.min(MAX_PROBE_PIXELS as u32) as usize].iter().position(|entry| entry.pixel == pixel)
    }
    pub(crate) fn merge(&mut self, other: &Self) -> Result<()> {
        if other.trace_pixel_count == 0 {
            return Ok(());
        }
        if self.trace_pixel_count == 0 {
            *self = *other;
            return Ok(());
        }
        if self.supersampling != other.supersampling
            || self.output_pixel != other.output_pixel
            || self.trace_pixel_count != other.trace_pixel_count
            || self.output.is_some()
            || other.output.is_some()
        {
            return Err(invalid("photon camera aggregate layouts disagree or were already finalized"));
        }
        for (target, source) in self.trace_pixels.iter_mut().zip(other.trace_pixels) {
            target.merge(&source)?;
        }
        if let Some(sample) = other.selected_sample {
            if self.selected_sample.is_some() {
                return Err(invalid("photon selected sample was completed more than once"));
            }
            self.selected_sample = Some(sample);
        }
        Ok(())
    }
    pub fn finalize(&mut self, raw: [f32; 4], counts: [u32; MAX_PROBE_PIXELS], tolerance: f64) -> Result<()> {
        if !matches!(self.supersampling, 1 | 2)
            || self.trace_pixel_count != self.supersampling * self.supersampling
            || raw.into_iter().any(|value| !value.is_finite() || value < 0.0)
            || raw[3] > 1.0
        {
            return Err(invalid("photon output accounting has invalid layout or raw radiance"));
        }
        let mut mean = PhotonEstimatorPartition::default();
        let mut coverage = 0.0;
        for (entry, expected) in self.trace_pixels.iter_mut().zip(counts).take(self.trace_pixel_count as usize) {
            if entry.first_sample != 0
                || entry.samples == 0
                || entry.samples != expected
                || entry.covered_samples > entry.samples
                || entry.planned_samples.is_some_and(|planned| planned != expected)
            {
                return Err(invalid("photon output accounting does not contain every production observation"));
            }
            entry.planned_samples = Some(expected);
            entry.radiance_sum.validate(tolerance)?;
            mean.add_weighted(entry.radiance_sum, 1.0 / (f64::from(entry.samples) * f64::from(self.trace_pixel_count)))?;
            coverage += f64::from(entry.covered_samples) / (f64::from(entry.samples) * f64::from(self.trace_pixel_count));
        }
        let raw_rgba = raw.map(f64::from);
        let residual = std::array::from_fn(|axis| raw_rgba[axis] - mean.photon[axis] - mean.other_estimators[axis]);
        if residual.iter().zip(raw_rgba).take(3).any(|(error, beauty)| error.abs() > tolerance * beauty.abs().max(1.0))
            || (coverage - raw_rgba[3]).abs() > tolerance.max(f64::from(f32::EPSILON))
        {
            return Err(invalid("photon output accounting disagrees with the raw film"));
        }
        self.output = Some(PhotonOutputAccounting {
            pixel: self.output_pixel,
            photon: mean.photon,
            other_estimators: mean.other_estimators,
            accounted_beauty: mean.beauty,
            raw_rgba,
            recombination_residual: residual,
        });
        Ok(())
    }
}

/// Scratch belongs to the optional owner allocation, never every camera state.
#[derive(Clone, Copy, Default)]
pub(crate) struct PhotonLaneAccounting {
    active: bool,
    sample: u32,
    strata: u32,
    next_lane: u32,
    lane: PhotonEstimatorPartition,
    group: PhotonEstimatorPartition,
    coverage: Option<bool>,
}
impl PhotonLaneAccounting {
    fn prepare(&mut self, sample: u32, lane: u32, strata: u32) -> Result<()> {
        if !matches!(strata, 1 | 2 | 4) || lane >= strata {
            return Err(invalid("photon camera accounting has an invalid Fresnel lane"));
        }
        if !self.active {
            if lane != 0 {
                return Err(invalid("photon camera accounting is missing its first Fresnel lane"));
            }
            *self = Self { active: true, sample, strata, ..Self::default() };
        }
        if self.sample != sample || self.strata != strata || self.next_lane != lane {
            return Err(invalid("photon camera accounting Fresnel lanes are out of order"));
        }
        Ok(())
    }
    pub(crate) fn record(&mut self, sample: u32, lane: u32, strata: u32, photon: bool, value: Rgb) -> Result<()> {
        self.prepare(sample, lane, strata)?;
        add(if photon { &mut self.lane.photon } else { &mut self.lane.other_estimators }, value)
    }
    pub(crate) fn complete(&mut self, sample: u32, lane: u32, strata: u32, beauty: Rgb, covered: bool) -> Result<Option<PhotonSampleAccounting>> {
        self.prepare(sample, lane, strata)?;
        if self.coverage.is_some_and(|previous| previous != covered) {
            return Err(invalid("photon camera Fresnel lanes disagree on primary coverage"));
        }
        self.lane.beauty = beauty;
        self.lane.validate(1e-10)?;
        self.group.add_weighted(self.lane, 1.0 / f64::from(strata))?;
        self.coverage = Some(covered);
        self.lane = PhotonEstimatorPartition::default();
        self.next_lane += 1;
        if self.next_lane != strata {
            return Ok(None);
        }
        let result = PhotonSampleAccounting { sample, strata, radiance: self.group, coverage: f64::from(u8::from(covered)) };
        *self = Self::default();
        Ok(Some(result))
    }
    pub(crate) fn idle(&self) -> bool {
        !self.active
    }
}
