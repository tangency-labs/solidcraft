//! Explicit CPU resource envelopes. Larger profiles are opt-in saved intent, not
//! automatic quality changes or promises about available RAM, speed or convergence.
use serde::{Deserialize, Serialize};

use super::{CpuProgressiveOptions, ErrorCode, JobError, Result};
use crate::RenderLimits;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
/// Native CPU reservations, including retained frames, share this finite ceiling.
pub const NATIVE_CPU_POOL_BYTES: u64 = 16 * GIB;
/// Hard implementation ceilings; individual requests retain their selected envelope.
pub const MAX_CPU_WORK: u64 = 16_000_000_000_000;
pub const MAX_CPU_TRIANGLES: usize = 8_000_000;
pub const MAX_CPU_BODIES: usize = 262_144;
/// Maximum admitted primary-film samples, shared with the display resolver.
pub const MAX_CPU_SAMPLES_PER_PIXEL: u32 = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuBudget {
    #[default]
    Compact,
    Studio,
    Workstation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CpuBudgetLimits {
    pub max_dimension: u64,
    pub max_pixels: u64,
    pub max_progressive_pixels: u64,
    pub max_samples_per_pixel: u32,
    pub max_camera_samples: u64,
    pub max_triangles: u64,
    pub max_bodies: usize,
    pub max_geometry_bytes: u64,
    pub max_job_bytes: u64,
    pub max_work_units: u64,
}

impl CpuBudget {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Studio, Self::Workstation];

    pub fn is_compact(&self) -> bool {
        *self == Self::Compact
    }

    pub const fn limits(self) -> CpuBudgetLimits {
        match self {
            Self::Compact => CpuBudgetLimits {
                max_dimension: 4096,
                max_pixels: 4_194_304,
                max_progressive_pixels: 1_048_576,
                max_samples_per_pixel: 64,
                max_camera_samples: 16_777_216,
                max_triangles: super::image::MAX_TRIANGLES,
                max_bodies: 16_384,
                max_geometry_bytes: super::image::MAX_GEOMETRY_BYTES,
                max_job_bytes: super::image::MAX_JOB_BYTES,
                max_work_units: 1_000_000_000,
            },
            Self::Studio => CpuBudgetLimits {
                max_dimension: 8192,
                max_pixels: 16_777_216,
                max_progressive_pixels: 16_777_216,
                max_samples_per_pixel: 1024,
                max_camera_samples: 4_294_967_296,
                max_triangles: 2_000_000,
                max_bodies: 65_536,
                max_geometry_bytes: GIB,
                max_job_bytes: 3 * GIB,
                max_work_units: 1_000_000_000_000,
            },
            Self::Workstation => CpuBudgetLimits {
                max_dimension: 16_384,
                max_pixels: 67_108_864,
                max_progressive_pixels: 67_108_864,
                max_samples_per_pixel: MAX_CPU_SAMPLES_PER_PIXEL,
                max_camera_samples: 68_719_476_736,
                max_triangles: MAX_CPU_TRIANGLES as u64,
                max_bodies: MAX_CPU_BODIES,
                max_geometry_bytes: 4 * GIB,
                max_job_bytes: 12 * GIB,
                max_work_units: MAX_CPU_WORK,
            },
        }
    }

    pub fn image_limits(self, progressive: bool) -> RenderLimits {
        let limits = self.limits();
        RenderLimits {
            max_dimension: limits.max_dimension,
            max_pixels: if progressive { limits.max_progressive_pixels } else { limits.max_pixels },
            max_scratch_bytes: limits.max_job_bytes,
        }
    }

    pub fn validate_samples(self, samples: u32) -> Result<()> {
        let maximum = self.limits().max_samples_per_pixel;
        if samples == 0 || samples > maximum {
            return Err(JobError::new(ErrorCode::InvalidRequest, format!("CPU {self:?} samples_per_pixel must be 1..{maximum}")));
        }
        Ok(())
    }

    /// No geometry reads or allocations. This also validates offline saved intent.
    pub fn validate_image(self, width: u32, height: u32, progressive: Option<CpuProgressiveOptions>) -> Result<()> {
        let (w, h) = (u64::from(width), u64::from(height));
        let limits = self.image_limits(progressive.is_some());
        let pixels = w.checked_mul(h).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "CPU pixel count overflow"))?;
        if w == 0 || h == 0 || w > limits.max_dimension || h > limits.max_dimension || pixels > limits.max_pixels {
            return Err(JobError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "CPU {self:?} accepts dimensions 1..{} and at most {} pixels; select a larger resource profile explicitly",
                    limits.max_dimension, limits.max_pixels
                ),
            ));
        }
        if let Some(options) = progressive {
            self.validate_samples(options.samples_per_pixel)?;
            if pixels.checked_mul(u64::from(options.samples_per_pixel)).is_none_or(|n| n > self.limits().max_camera_samples) {
                return Err(JobError::new(
                    ErrorCode::LimitExceeded,
                    format!("CPU {self:?} exceeds its {} camera-sample admission", self.limits().max_camera_samples),
                ));
            }
        }
        Ok(())
    }

    /// A file may preserve large intent on every platform; only execution needs
    /// the native 64-bit address space. No implicit downgrade to Compact occurs.
    pub fn validate_execution(self) -> Result<()> {
        if self != Self::Compact && (cfg!(target_arch = "wasm32") || usize::BITS < 64) {
            return Err(JobError::new(ErrorCode::HostUnavailable, "large CPU profiles require a native 64-bit host"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_intent_is_explicit_and_old_intent_remains_compact() {
        assert_eq!(CpuBudget::default(), CpuBudget::Compact);
        let p = Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 9 });
        assert!(CpuBudget::Compact.validate_image(3840, 2160, p).is_err());
        assert!(CpuBudget::Studio.validate_image(3840, 2160, p).is_ok());
        assert!(CpuBudget::Studio.validate_image(7680, 4320, p).is_err());
        assert!(CpuBudget::Workstation.validate_image(7680, 4320, p).is_ok());
        assert_eq!(serde_json::from_str::<CpuBudget>("\"workstation\"").unwrap(), CpuBudget::Workstation);
        assert!(serde_json::from_str::<CpuBudget>("\"unlimited\"").is_err());
    }

    #[test]
    fn all_envelopes_have_checked_finite_bounds_and_no_automatic_resize() {
        for budget in CpuBudget::ALL {
            let limits = budget.limits();
            assert!(limits.max_geometry_bytes < limits.max_job_bytes);
            assert!(limits.max_job_bytes < NATIVE_CPU_POOL_BYTES);
            assert!(limits.max_work_units <= MAX_CPU_WORK);
            assert!(budget.validate_image(0, 1, None).is_err());
            assert!(budget.validate_image(u32::MAX, u32::MAX, None).is_err());
            assert!(budget.validate_samples(0).is_err());
            assert!(budget.validate_samples(limits.max_samples_per_pixel + 1).is_err());
            assert!(budget.validate_samples(limits.max_samples_per_pixel).is_ok());
        }
        let p = Some(CpuProgressiveOptions { samples_per_pixel: 4096, seed: 0 });
        assert!(CpuBudget::Workstation.validate_image(8192, 8192, p).is_err());
        assert!(CpuBudget::Workstation.validate_image(3840, 2160, p).is_ok());
    }
}
