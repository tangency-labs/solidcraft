//! Bounded linear RGB statistics, independent of rendering and display transforms.
//! Samples use the renderer's linear-sRGB primaries; source HDR conversion is a
//! caller prerequisite. Display-sRGB primary backgrounds need a separate coverage
//! policy before camera samples can be mixed; this module does not invent one.
//!
//! The caller reserves `required_bytes` in its EXISTING job/host lease before
//! construction, and supplies a charge callback debiting that job's cumulative
//! work budget. This module creates no pool, thread, cache or cancellation token.
//! Fallible allocation checks actual capacity against that same admission.
//!
//! Samples for each pixel must arrive in the same order for bitwise reproducible
//! results. Splitting contiguous updates into smaller calls preserves that order;
//! arbitrary floating-point reductions or reordered samples are not equivalent.
//!
//! `complete_pass` borrows the film immutably and refuses uneven/empty counts.
//! Its iterator exposes linear means only; the consumer applies exposure, tone
//! mapping and encoding ONCE, in bounded cancellable chunks. Publishing requires
//! a fully resolved owned snapshot with its own host lease, generation check and
//! cancellation/publication lock. Neither a partial update nor this borrowed view
//! is a publishable frame. Progressive host/UI wiring is a separate integration.

use crate::job::{CancelToken, ErrorCode, JobError, Result};

#[path = "film_png.rs"]
pub mod png;

// Camera passes additionally retain coverage. Their scanline protocol proves a
// uniform complete pass incrementally, without borrowing a self-owned resolver.
#[cfg(feature = "raytrace-cpu")]
#[path = "film_primary.rs"]
pub(crate) mod primary;

pub const MAX_DIMENSION: u32 = 16_384;
pub const MAX_PIXELS: usize = 16_777_216;
pub const MAX_SAMPLES: u32 = 1_048_576;
pub const MAX_UPDATE: usize = 256;
pub const MAX_BYTES: u64 = 384 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pixel {
    mean: [f64; 3],
    m2: [f64; 3],
    count: u32,
}

/// Unbiased sample variance, zero by convention before two samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelStatistics {
    pub mean: [f64; 3],
    pub variance: [f64; 3],
    pub count: u32,
}

/// Storage stays private so callers cannot bypass finite/count invariants.
pub struct FilmAccumulator {
    width: u32,
    height: u32,
    sample_limit: u32,
    pixels: Vec<Pixel>,
}

/// An immutable uniform pass; not an owned or publishable preview image.
pub struct CompletePass<'a> {
    film: &'a FilmAccumulator,
    samples_per_pixel: u32,
}

fn invalid(message: &'static str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn limit(message: &'static str) -> JobError {
    JobError::new(ErrorCode::LimitExceeded, message)
}

fn dimensions(width: u32, height: u32, sample_limit: u32) -> Result<usize> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION || sample_limit == 0 || sample_limit > MAX_SAMPLES {
        return Err(invalid("invalid film dimensions or sample limit"));
    }
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or_else(|| limit("film dimensions overflow"))?;
    usize::try_from(pixels).ok().filter(|n| *n <= MAX_PIXELS).ok_or_else(|| limit("film pixel limit exceeded"))
}

fn bytes_for_capacity(capacity: usize) -> Result<u64> {
    let bytes = u64::try_from(capacity)
        .ok()
        .and_then(|n| n.checked_mul(std::mem::size_of::<Pixel>() as u64))
        .and_then(|n| n.checked_add(std::mem::size_of::<FilmAccumulator>() as u64))
        .and_then(|n| n.checked_add(std::mem::size_of::<[Pixel; MAX_UPDATE]>() as u64))
        .filter(|n| *n <= MAX_BYTES)
        .ok_or_else(|| limit("film storage limit exceeded"))?;
    Ok(bytes)
}

impl Pixel {
    fn updated(self, rgb: [f64; 3], sample_limit: u32) -> Result<Self> {
        if rgb.iter().any(|c| !c.is_finite() || *c < 0.0) {
            return Err(invalid("film samples must be finite nonnegative linear RGB"));
        }
        let count = self.count.checked_add(1).filter(|n| *n <= sample_limit).ok_or_else(|| limit("film sample limit exceeded"))?;
        let mut result = self;
        for ((mean, m2), value) in result.mean.iter_mut().zip(result.m2.iter_mut()).zip(rgb) {
            let delta = value - *mean;
            *mean += delta / f64::from(count);
            *m2 += delta * (value - *mean);
            if !mean.is_finite() || !m2.is_finite() || *mean < 0.0 || *m2 < 0.0 {
                return Err(invalid("film statistics exceed finite numeric range"));
            }
        }
        result.count = count;
        Ok(result)
    }

    fn statistics(self) -> PixelStatistics {
        let denominator = f64::from(self.count.saturating_sub(1).max(1));
        PixelStatistics { mean: self.mean, variance: self.m2.map(|m2| m2 / denominator), count: self.count }
    }
}

impl FilmAccumulator {
    /// Required host-admitted storage, including the film object and the bounded
    /// atomic-update stack scratch used while this film is alive. Allocator
    /// bookkeeping is outside this byte contract; actual Vec capacity is checked.
    pub fn required_bytes(width: u32, height: u32, sample_limit: u32) -> Result<u64> {
        bytes_for_capacity(dimensions(width, height, sample_limit)?)
    }

    /// `admitted_bytes` is an existing host lease amount, not a second pool.
    /// The caller must preserve that lease for this object's entire lifetime.
    pub fn new(
        width: u32,
        height: u32,
        sample_limit: u32,
        admitted_bytes: u64,
        cancel: &CancelToken,
        charge: &mut impl FnMut(u64) -> Result<()>,
    ) -> Result<Self> {
        cancel.check()?;
        let count = dimensions(width, height, sample_limit)?;
        let required = bytes_for_capacity(count)?;
        if admitted_bytes < required || admitted_bytes > MAX_BYTES {
            return Err(limit("film is not covered by caller admission"));
        }
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve film storage"))?;
        if bytes_for_capacity(pixels.capacity())? > admitted_bytes {
            return Err(limit("actual film capacity exceeds caller admission"));
        }
        for _ in 0..count {
            cancel.check()?;
            charge(1)?;
            pixels.push(Pixel::default());
        }
        cancel.check()?;
        Ok(Self { width, height, sample_limit, pixels })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Actual retained capacity plus object and maximum update scratch allowance.
    pub fn allocated_bytes(&self) -> Result<u64> {
        bytes_for_capacity(self.pixels.capacity())
    }

    /// For diagnostics/variance decisions, not partially published image output.
    pub fn pixel_statistics(&self, index: usize) -> Result<PixelStatistics> {
        self.pixels.get(index).copied().map(Pixel::statistics).ok_or_else(|| invalid("film pixel index out of range"))
    }

    /// One sample per consecutive pixel, at most MAX_UPDATE. Validate the whole
    /// update before mutation. Validation/commit work is charged up front; failed
    /// work/cancel/input checks leave the film unchanged (budget stays consumed).
    /// After the last cancellation check, bounded commit is the linearization
    /// point: cancellation during commit is observed by the caller before publish.
    pub fn add_range(&mut self, start: usize, samples: &[[f64; 3]], cancel: &CancelToken, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        cancel.check()?;
        if samples.is_empty() || samples.len() > MAX_UPDATE {
            return Err(invalid("film update must contain 1..256 samples"));
        }
        let end = start.checked_add(samples.len()).ok_or_else(|| invalid("film update range overflow"))?;
        let pixels = self.pixels.get(start..end).ok_or_else(|| invalid("film update range out of bounds"))?;
        // Fixed bounded stack scratch gives atomic commit without a second heap
        // buffer or recomputation that might fail after earlier pixels mutated.
        let mut staged = [Pixel::default(); MAX_UPDATE];
        for ((pixel, sample), target) in pixels.iter().zip(samples).zip(staged.iter_mut()) {
            cancel.check()?;
            charge(2)?;
            *target = pixel.updated(*sample, self.sample_limit)?;
        }
        cancel.check()?;
        let target = self.pixels.get_mut(start..end).ok_or_else(|| invalid("film update range changed"))?;
        target.copy_from_slice(staged.get(..samples.len()).ok_or_else(|| invalid("film staging range invalid"))?);
        Ok(())
    }

    /// No allocation and no display transform. Each checked pixel charges one
    /// unit to the same job and checks its cancellation token.
    pub fn complete_pass<'a>(&'a self, cancel: &CancelToken, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<CompletePass<'a>> {
        cancel.check()?;
        let count = self.pixels.first().map(|p| p.count).ok_or_else(|| invalid("film has no pixels"))?;
        if count == 0 {
            return Err(invalid("film has no complete pass"));
        }
        for pixel in &self.pixels {
            cancel.check()?;
            charge(1)?;
            if pixel.count != count {
                return Err(invalid("film pass is incomplete"));
            }
        }
        cancel.check()?;
        Ok(CompletePass { film: self, samples_per_pixel: count })
    }
}

impl CompletePass<'_> {
    pub fn samples_per_pixel(&self) -> u32 {
        self.samples_per_pixel
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.film.dimensions()
    }

    /// Borrowed immutable linear means. Resolving/copying/encoding and its work
    /// accounting/cancellation remain caller-owned; no output buffer is allocated.
    pub fn pixels(&self) -> impl ExactSizeIterator<Item = [f64; 3]> + '_ {
        self.film.pixels.iter().map(|p| p.mean)
    }
}

#[cfg(test)]
#[path = "film_tests.rs"]
mod tests;
