//! Shared render-quality controls and scene-linear supersample reconstruction.
use serde::{Deserialize, Serialize};

use super::{CancelToken, ErrorCode, JobError, Result};

pub const MAX_UI_SAMPLES: u32 = 16_384;
pub const MAX_CPU_THREADS: u16 = 64;
pub const MAX_OUTPUT_DIMENSION: u32 = 8192;
pub const MAX_OUTPUT_PIXELS: u64 = 33_554_432;
pub const MAX_TRACE_DIMENSION: u32 = MAX_OUTPUT_DIMENSION * 2;
pub const MAX_TRACE_PIXELS: u64 = MAX_OUTPUT_PIXELS * 4;

/// Conservative display filtering. Raw radiance and alpha never change.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DenoiseSettings {
    pub passes: u32,
    pub strength: f64,
    pub firefly_suppression: bool,
    /// Filter supported mirror and glass guides; false retains legacy opaque-only denoising.
    pub optical_filter: bool,
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self { passes: 3, strength: 1.0, firefly_suppression: false, optical_filter: true }
    }
}

impl DenoiseSettings {
    pub fn validate(self) -> Result<()> {
        if !(1..=3).contains(&self.passes) || !self.strength.is_finite() || !(0.0..=1.0).contains(&self.strength) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "denoising requires 1..3 passes and strength 0..1"));
        }
        Ok(())
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

pub fn single_supersample(scale: &u32) -> bool {
    *scale == 1
}

/// Output and transport bounds are distinct. A 2x setting changes both dimensions,
/// never the selected output size. Byte/work/device admission remains the caller's.
pub fn supersampled_dimensions(width: u32, height: u32, scale: u32) -> Result<(u32, u32)> {
    if !matches!(scale, 1 | 2) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "supersampling must be 1 or 2"));
    }
    if width == 0
        || height == 0
        || width > MAX_OUTPUT_DIMENSION
        || height > MAX_OUTPUT_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_OUTPUT_PIXELS
    {
        return Err(JobError::new(ErrorCode::LimitExceeded, "output requires 1..8192 sides and at most 33554432 pixels"));
    }
    let trace_width = width.checked_mul(scale).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "supersampled width overflow"))?;
    let trace_height = height.checked_mul(scale).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "supersampled height overflow"))?;
    if trace_width > MAX_TRACE_DIMENSION || trace_height > MAX_TRACE_DIMENSION || u64::from(trace_width) * u64::from(trace_height) > MAX_TRACE_PIXELS
    {
        return Err(JobError::new(ErrorCode::LimitExceeded, "supersampled trace grid exceeds its bounded dimensions"));
    }
    Ok((trace_width, trace_height))
}

/// Resolve coverage-premultiplied linear RGBA. Dimensions describe the final
/// output; the input contains (width*scale)*(height*scale) transport pixels.
/// Each channel is averaged in f64 before a single f32 rounding. No exposure,
/// unpremultiplication, denoising or tone mapping enters this operation.
pub fn resolve_supersampling(width: u32, height: u32, scale: u32, linear: &[[f32; 4]], cancel: &CancelToken) -> Result<Vec<[f32; 4]>> {
    cancel.check()?;
    let (trace_width, trace_height) = supersampled_dimensions(width, height, scale)?;
    let input_count = usize::try_from(u64::from(trace_width) * u64::from(trace_height))
        .map_err(|_| JobError::new(ErrorCode::LimitExceeded, "supersampling input exceeds address space"))?;
    let count = usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| JobError::new(ErrorCode::LimitExceeded, "supersampling output exceeds address space"))?;
    if linear.len() != input_count {
        return Err(JobError::new(ErrorCode::InvalidRequest, "supersampling requires the complete trace grid"));
    }
    let mut result = Vec::new();
    result.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "supersampling output allocation failed"))?;
    if result.capacity() != count {
        return Err(JobError::new(ErrorCode::LimitExceeded, "supersampling output capacity exceeds admission"));
    }
    for y in 0..height as usize {
        cancel.check()?;
        for x in 0..width as usize {
            result.push(resolve_pixel(width, scale, linear, y * width as usize + x)?);
        }
    }
    cancel.check()?;
    Ok(result)
}

/// Fixed-size subpixel addresses for an already admitted output pixel.
pub(crate) fn supersample_indices(width: u32, scale: u32, pixel: usize) -> Result<([usize; 4], usize)> {
    if width == 0 || !matches!(scale, 1 | 2) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "invalid supersampling pixel layout"));
    }
    let width = width as usize;
    let scale = scale as usize;
    let stride = width * scale;
    let base = (pixel / width) * scale * stride + (pixel % width) * scale;
    Ok(if scale == 1 { ([base; 4], 1) } else { ([base, base + 1, base + stride, base + stride + 1], 4) })
}

/// One cooperative scene-linear resolve; the caller has admitted the full grid.
pub(crate) fn resolve_pixel(width: u32, scale: u32, linear: &[[f32; 4]], pixel: usize) -> Result<[f32; 4]> {
    let (indices, count) = supersample_indices(width, scale, pixel)?;
    let mut sum = [0.0_f64; 4];
    for index in &indices[..count] {
        let pixel = linear.get(*index).ok_or_else(|| JobError::new(ErrorCode::Internal, "supersampling source pixel disappeared"))?;
        if pixel.iter().any(|value| !value.is_finite() || *value < 0.0) || pixel[3] > 1.0 || (pixel[3] == 0.0 && pixel[..3] != [0.0; 3]) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "supersampling input has invalid radiance or coverage"));
        }
        for (sum, value) in sum.iter_mut().zip(pixel) {
            *sum += f64::from(*value);
        }
    }
    Ok(sum.map(|value| (value / count as f64) as f32))
}
