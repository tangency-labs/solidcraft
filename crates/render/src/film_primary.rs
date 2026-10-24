//! Bounded camera film: linear radiance sums and primary coverage, never display RGB.
//! Exactly one scanline-ordered update per pixel and absolute sample ordinal makes
//! a complete pass a state invariant. Initialization and updates are cancellable
//! single-pixel operations. The owning image job supplies its existing work lease.

use super::{invalid, limit};
use crate::job::{CancelToken, CpuBudget, CpuProgressiveOptions, ErrorCode, JobError, Result};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Pixel {
    sum: [f64; 3],
    covered: u32,
}

pub(crate) struct PrimaryFilm {
    pixels: Vec<Pixel>,
    count: usize,
    sample_limit: u32,
    samples: u32,
    next_pixel: usize,
}

impl PrimaryFilm {
    pub(crate) fn required_bytes(width: u32, height: u32, options: CpuProgressiveOptions, budget: CpuBudget) -> Result<u64> {
        options.validate_image_with_budget(width, height, budget)?;
        (u64::from(width) * u64::from(height))
            .checked_mul(std::mem::size_of::<Pixel>() as u64)
            .and_then(|n| n.checked_add(std::mem::size_of::<Self>() as u64))
            .ok_or_else(|| limit("primary film storage overflow"))
    }

    pub(crate) fn new(
        width: u32,
        height: u32,
        options: CpuProgressiveOptions,
        admitted_bytes: u64,
        cancel: &CancelToken,
        budget: CpuBudget,
    ) -> Result<Self> {
        cancel.check()?;
        let required = Self::required_bytes(width, height, options, budget)?;
        if admitted_bytes < required {
            return Err(limit("primary film is not covered by image admission"));
        }
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| limit("primary film address space exceeded"))?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve primary film"))?;
        if pixels.capacity() != count {
            return Err(limit("actual primary film capacity exceeds admission"));
        }
        cancel.check()?;
        Ok(Self { pixels, count, sample_limit: options.samples_per_pixel, samples: 0, next_pixel: 0 })
    }

    pub(crate) fn initialized(&self) -> usize {
        self.pixels.len()
    }

    pub(crate) fn ready(&self) -> bool {
        self.pixels.len() == self.count
    }

    pub(crate) fn initialize_one(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        cancel.check()?;
        if self.ready() {
            return Err(invalid("primary film is already initialized"));
        }
        charge()?;
        cancel.check()?;
        self.pixels.push(Pixel::default());
        Ok(())
    }

    pub(crate) fn samples(&self) -> u32 {
        self.samples
    }

    /// Validate everything before the one bounded mutation. Failed/cancelled
    /// updates never consume a sample ordinal or partially alter a pixel.
    pub(crate) fn add(
        &mut self,
        index: usize,
        ordinal: u32,
        radiance: [f64; 3],
        covered: bool,
        cancel: &CancelToken,
        charge: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        cancel.check()?;
        if !self.ready() || index != self.next_pixel || ordinal != self.samples || ordinal >= self.sample_limit {
            return Err(invalid("primary film requires the next pixel and absolute sample ordinal"));
        }
        if radiance.into_iter().any(|v| !v.is_finite() || !(0.0..=1e12).contains(&v)) || (!covered && radiance != [0.0; 3]) {
            return Err(invalid("invalid primary linear radiance or coverage"));
        }
        let pixel = self.pixels.get(index).copied().ok_or_else(|| invalid("primary film pixel out of range"))?;
        let sum = std::array::from_fn(|i| pixel.sum[i] + radiance[i]);
        if sum.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > f64::from(self.sample_limit) * 1e12) {
            return Err(invalid("primary film radiance sum exceeds finite range"));
        }
        let hits = pixel.covered.checked_add(u32::from(covered)).filter(|n| *n <= ordinal + 1).ok_or_else(|| invalid("primary coverage overflow"))?;
        charge()?;
        cancel.check()?;
        *self.pixels.get_mut(index).ok_or_else(|| invalid("primary film pixel disappeared"))? = Pixel { sum, covered: hits };
        self.next_pixel += 1;
        if self.next_pixel == self.count {
            self.next_pixel = 0;
            self.samples += 1;
        }
        Ok(())
    }

    /// A display consumer can only read a fully completed, immutable pass. The
    /// returned radiance is premultiplied by coverage; background is kept separate.
    pub(crate) fn pixel(&self, index: usize) -> Result<([f64; 3], u16, u16)> {
        if !self.ready() || self.next_pixel != 0 || self.samples == 0 {
            return Err(invalid("primary film has no uniform complete pass"));
        }
        let pixel = self.pixels.get(index).ok_or_else(|| invalid("primary film resolve index out of range"))?;
        let covered = u16::try_from(pixel.covered).map_err(|_| limit("primary coverage exceeds resolve representation"))?;
        let samples = u16::try_from(self.samples).map_err(|_| limit("primary sample count exceeds resolve representation"))?;
        Ok((pixel.sum.map(|sum| sum / f64::from(self.samples)), covered, samples))
    }
}
