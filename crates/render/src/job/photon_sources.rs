//! Bounded emission-source proposals, shared by CPU and GPU photon transport.
//! These weights never change source radiometry or camera-path MIS densities.
use super::{ErrorCode, JobError, PunctualLightOptions, Result};
use std::f64::consts::{PI, TAU};

pub const MAX_PHOTON_SOURCES: usize = super::MAX_DIRECTIONAL_LIGHTS + super::MAX_PUNCTUAL_LIGHTS + super::MAX_AREA_LIGHTS + 1;
pub const SOURCE_BINS: u32 = 1 << 24;
/// Five percent uniform exploration keeps reciprocal source probability near at most
/// 20 times the source count, including dim sources with efficient optical paths.
const UNIFORM_SHARE: f64 = 0.05;

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhotonSourceDistribution {
    cumulative: [u32; MAX_PHOTON_SOURCES],
    count: usize,
}
impl PhotonSourceDistribution {
    pub fn new(powers: &[f64]) -> Result<Self> {
        if powers.is_empty() || powers.len() > MAX_PHOTON_SOURCES || powers.iter().any(|p| !p.is_finite() || *p < 0.0) {
            return Err(invalid("photon source powers require one to twenty-one finite nonnegative values"));
        }
        let maximum = powers.iter().copied().fold(0.0_f64, f64::max);
        // Scaling prevents overflow without dropping any source's support.
        let total = if maximum > 0.0 { powers.iter().map(|p| p / maximum).sum::<f64>() } else { 0.0 };
        let mut cumulative = [0; MAX_PHOTON_SOURCES];
        let mut sum = 0.0;
        let mut previous = 0;
        for (index, power) in powers.iter().enumerate() {
            let probability = if total > 0.0 {
                (1.0 - UNIFORM_SHARE) * (*power / maximum) / total + UNIFORM_SHARE / powers.len() as f64
            } else {
                1.0 / powers.len() as f64
            };
            sum += probability;
            let end = if index + 1 == powers.len() { SOURCE_BINS } else { (sum * f64::from(SOURCE_BINS)).round() as u32 };
            if end <= previous || end > SOURCE_BINS {
                return Err(invalid("photon source quantization lost support"));
            }
            cumulative[index] = end;
            previous = end;
        }
        Ok(Self { cumulative, count: powers.len() })
    }
    pub fn cumulative(&self) -> &[u32] {
        &self.cumulative[..self.count]
    }
    /// The GPU draw has exactly 24 uniform bits. CPU quantizes its independent
    /// draw to the same bins; both divide by the represented bin width.
    pub fn select(&self, choice: f64) -> Result<(usize, f64)> {
        if !choice.is_finite() || !(0.0..1.0).contains(&choice) {
            return Err(invalid("photon source sample must be in [0,1)"));
        }
        let bin = (choice * f64::from(SOURCE_BINS)) as u32;
        let mut previous = 0;
        for (index, end) in self.cumulative().iter().copied().enumerate() {
            if bin < end {
                return Ok((index, f64::from(end - previous) / f64::from(SOURCE_BINS)));
            }
            previous = end;
        }
        Err(invalid("photon source sample exceeds its distribution"))
    }
}

pub fn luminance(rgb: [f64; 3]) -> Result<f64> {
    if rgb.into_iter().any(|x| !x.is_finite() || x < 0.0) {
        return Err(invalid("photon source radiance must be finite and nonnegative"));
    }
    checked_power(0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
}
pub fn checked_power(value: f64) -> Result<f64> {
    if value.is_finite() && value >= 0.0 { Ok(value) } else { Err(invalid("photon source power exceeds finite bounds")) }
}
pub fn disk_power(radius: f64, integrated_radiance: f64) -> Result<f64> {
    if !radius.is_finite() || radius < 0.0 || !integrated_radiance.is_finite() || integrated_radiance < 0.0 {
        return Err(invalid("invalid photon launch disk power"));
    }
    checked_power(PI * radius * radius * integrated_radiance)
}
pub fn rectangle_power(area: f64, radiance: [f64; 3]) -> Result<f64> {
    if !area.is_finite() || area < 0.0 {
        return Err(invalid("invalid photon rectangle area"));
    }
    checked_power(PI * area * luminance(radiance)?)
}
pub fn punctual_power(light: &PunctualLightOptions) -> Result<f64> {
    light.validate()?;
    let solid_angle = light.cone.map_or(4.0 * PI, |spot| {
        // Integrate the cubic feather in cos(theta), including its flat core.
        // sin² forms retain narrow cones without subtracting nearly equal cosines.
        TAU * ((spot.inner_half_angle_radians * 0.5).sin().powi(2) + (spot.outer_half_angle_radians * 0.5).sin().powi(2))
    });
    checked_power(luminance(light.color)? * light.intensity * light.reference_distance_mm.powi(2) * solid_angle)
}
