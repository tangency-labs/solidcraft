//! Incremental opaque complete-pass consumer, not a progressive UI or camera sampler.
//!
//! Reuses RT2's accredited incremental PngEncoding, without another codec.
//! Exposure -> existing Reinhard -> sRGB occurs once, after linear accumulation.
//! Primary-background coverage/alpha mixing is intentionally outside this API.
//! A caller must supply the SAME job token/work callback and check generation and
//! cancellation under its publication lock before publishing CompletedPng.

use super::{CompletePass, MAX_BYTES, invalid, limit};
use crate::job::encoding::PngEncoding;
use crate::job::{CancelToken, CpuHost, ErrorCode, JobError, Phase, Progress, Reservation, Result};
use solidcraft_geom::{Vec3, srgb::encode8};

/// Encoded bytes retain the actual existing host lease. No detached byte getter.
pub struct CompletedPng {
    bytes: Vec<u8>,
    _lease: Reservation,
    width: u32,
    height: u32,
    samples: u32,
}
impl CompletedPng {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn samples_per_pixel(&self) -> u32 {
        self.samples
    }
}

pub enum ResolvePoll {
    Pending(Progress),
    Complete(CompletedPng),
}

pub struct FilmPngTask<'a> {
    pass: Option<CompletePass<'a>>,
    lease: Option<Reservation>,
    rgba: Vec<u8>,
    encoded: Vec<u8>,
    writer: Option<PngEncoding>,
    cancel: CancelToken,
    width: u32,
    height: u32,
    samples: u32,
    pixel: usize,
    row: usize,
    scale: f64,
    terminal: bool,
}

fn layout(width: u32, height: u32) -> Result<(usize, usize, u64)> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(invalid("film PNG dimensions exceed image host limits"));
    }
    let count = u64::from(width) * u64::from(height);
    if count > 4_000_000 {
        return Err(limit("film PNG pixel limit exceeded"));
    }
    let rgba = usize::try_from(count * 4).map_err(|_| limit("film PNG address space exceeded"))?;
    let encoded = usize::try_from(count * 8 + 65_536).map_err(|_| limit("film PNG encoded limit exceeded"))?;
    // Same conservative 48-byte/output-pixel scratch class as CPU raster admission,
    // plus codec/object overhead. Includes RGBA, encoded capacity and row codec work.
    let bytes = count
        .checked_mul(48)
        .and_then(|n| n.checked_add(262_144))
        .and_then(|n| n.checked_add(std::mem::size_of::<FilmPngTask<'static>>() as u64))
        .filter(|n| *n <= MAX_BYTES)
        .ok_or_else(|| limit("film PNG memory limit exceeded"))?;
    Ok((rgba, encoded, bytes))
}

impl<'a> FilmPngTask<'a> {
    /// Task+source admission. The source film's original lifetime lease must also
    /// remain held by its owner; this conservatively rebills the borrowed source.
    pub fn required_bytes(pass: &CompletePass<'_>) -> Result<u64> {
        let (width, height) = pass.dimensions();
        let (_, _, bytes) = layout(width, height)?;
        bytes
            .checked_add(pass.film.allocated_bytes()?)
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| limit("film and PNG snapshot exceed per-job memory limit"))
    }

    pub fn new(pass: CompletePass<'a>, host: &CpuHost, exposure_ev: f64, cancel: CancelToken) -> Result<Self> {
        cancel.check()?;
        if !exposure_ev.is_finite() || !(-16.0..=16.0).contains(&exposure_ev) {
            return Err(invalid("film PNG exposure must be finite in -16..16"));
        }
        let bytes = Self::required_bytes(&pass)?;
        let lease = host.reserve_film_snapshot(bytes)?;
        let (width, height) = pass.dimensions();
        let (rgba_len, encoded_len, _) = layout(width, height)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(rgba_len).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "film PNG pixels allocation failed"))?;
        let mut encoded = Vec::new();
        encoded.try_reserve_exact(encoded_len).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "film PNG output allocation failed"))?;
        if rgba.capacity() != rgba_len || encoded.capacity() != encoded_len {
            return Err(limit("actual film PNG capacity exceeds admitted layout"));
        }
        cancel.check()?;
        let samples = pass.samples_per_pixel();
        Ok(Self {
            pass: Some(pass),
            lease: Some(lease),
            rgba,
            encoded,
            writer: None,
            cancel,
            width,
            height,
            samples,
            pixel: 0,
            row: 0,
            scale: 2.0_f64.powf(exposure_ev),
            terminal: false,
        })
    }

    fn release(&mut self) {
        self.writer = None;
        self.rgba = Vec::new();
        self.pass = None;
        self.encoded = Vec::new();
        self.lease = None;
        self.terminal = true;
    }

    /// At most 256 pixel units and four encoding rows, charged to caller's job.
    /// A failure terminally drops scratch/lease; no partial PNG can be returned.
    pub fn step(&mut self, units: usize, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<ResolvePoll> {
        let result = self.advance(units, charge);
        if result.is_err() {
            self.release();
        }
        result
    }

    fn advance(&mut self, units: usize, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<ResolvePoll> {
        self.cancel.check()?;
        if self.terminal || units == 0 || units > 256 {
            return Err(invalid("invalid film PNG step"));
        }
        let count = self.width as usize * self.height as usize;
        let mut rows = 0;
        for _ in 0..units {
            self.cancel.check()?;
            if self.pixel < count {
                charge(1)?;
                let pass = self.pass.as_ref().ok_or_else(|| invalid("missing complete film pass"))?;
                let rgb = pass.film.pixel_statistics(self.pixel)?.mean;
                let mapped = crate::studio::tone_map(Vec3::new(rgb[0], rgb[1], rgb[2]) * self.scale)
                    .map_err(|_| invalid("film exposure exceeds finite radiance range"))?;
                self.rgba.extend_from_slice(&[encode8(mapped.x), encode8(mapped.y), encode8(mapped.z), 255]);
                self.pixel += 1;
            } else if self.writer.is_none() {
                charge(1)?;
                let (_, limit, _) = layout(self.width, self.height)?;
                self.writer =
                    Some(PngEncoding::with_output(self.width, self.height, limit as u64, self.cancel.clone(), std::mem::take(&mut self.encoded))?);
                self.pass = None;
            } else {
                if rows == 4 {
                    break;
                }
                charge(if self.row < self.height as usize { u64::from(self.width) } else { 1 })?;
                let writer = self.writer.as_mut().ok_or_else(|| invalid("missing film PNG writer"))?;
                let output = writer
                    .step(&self.rgba)
                    .map_err(|e| if self.cancel.is_cancelled() { JobError::new(ErrorCode::Cancelled, "film PNG cancelled") } else { e })?;
                self.row = writer.row();
                rows += 1;
                let Some(bytes) = output else {
                    continue;
                };
                self.cancel.check()?;
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err(invalid("invalid film PNG output"));
                }
                self.rgba = Vec::new();
                self.writer = None;
                self.pass = None;
                let mut lease = self.lease.take().ok_or_else(|| invalid("missing film PNG lease"))?;
                lease.shrink(bytes.capacity() as u64 + 1024)?;
                self.terminal = true;
                return Ok(ResolvePoll::Complete(CompletedPng {
                    bytes,
                    _lease: lease,
                    width: self.width,
                    height: self.height,
                    samples: self.samples,
                }));
            }
        }
        let encoding = self.pixel == count;
        Ok(ResolvePoll::Pending(Progress {
            phase: if encoding { Phase::Encoding } else { Phase::Preparing },
            done: if encoding { self.row as u64 } else { self.pixel as u64 },
            total: if encoding { u64::from(self.height) } else { count as u64 },
        }))
    }
}

#[cfg(test)]
#[path = "film_png_tests.rs"]
mod tests;
