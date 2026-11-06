//! Original bounded CPU a-trous output filter. Raw film and transport are never borrowed mutably.
//! The independent f64 gather oracle remains in the renderer tests, outside this implementation.
use super::{CancelToken, ErrorCode, JobError, Result};

pub(crate) const GUIDE_BYTES: u64 = 48;
pub(crate) const MAX_STEP_PIXELS: usize = 256;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Guide {
    pub albedo: [f32; 3],
    /// Captured body ordinal plus one; zero means that first-hit guidance is unavailable.
    pub body: u32,
    pub normal: [f32; 3],
    pub depth: f32,
    /// Camera-relative position, converted only after the f64 subtraction.
    pub position: [f32; 3],
    pub footprint: f32,
}
const _: [(); GUIDE_BYTES as usize] = [(); std::mem::size_of::<Guide>()];

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
impl Guide {
    pub(crate) fn validate(self) -> Result<()> {
        if self.body == 0 {
            return Ok(());
        }
        let normal_length: f64 = self.normal.into_iter().map(|v| f64::from(v).powi(2)).sum();
        if !self.albedo.into_iter().all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            || !self.normal.into_iter().chain(self.position).all(f32::is_finite)
            || !(0.999..=1.001).contains(&normal_length)
            || !self.depth.is_finite()
            || !self.footprint.is_finite()
            || self.footprint < 0.000001
        {
            return Err(invalid("invalid CPU denoising guide"));
        }
        Ok(())
    }
}

pub(crate) struct DenoiseTask {
    width: usize,
    height: usize,
    source: Vec<[f32; 4]>,
    destination: Vec<[f32; 4]>,
    pixel: usize,
    pass: usize,
}
impl DenoiseTask {
    pub(crate) fn new(width: u32, height: u32, source: Vec<[f32; 4]>) -> Result<Self> {
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("CPU denoising dimensions overflow"))?;
        if width == 0 || height == 0 || source.len() != count || source.capacity() != count {
            return Err(invalid("CPU denoising requires one admitted complete linear pass"));
        }
        let mut destination = Vec::new();
        destination.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU denoising colour allocation failed"))?;
        if destination.capacity() != count {
            return Err(invalid("CPU denoising colour capacity exceeds admission"));
        }
        Ok(Self { width: width as usize, height: height as usize, source, destination, pixel: 0, pass: 0 })
    }
    pub(crate) fn next_pixels(&self) -> usize {
        (self.source.len() - self.pixel).min(MAX_STEP_PIXELS)
    }
    /// One work unit is one pixel visit with a fixed 25-tap gather, as for bounded finishing kernels.
    /// The caller debits next_pixels before calling; no work, limit or cancellation is reset here.
    pub(crate) fn step(&mut self, guides: &[Guide], cancel: &CancelToken) -> Result<bool> {
        cancel.check()?;
        if self.pass >= 3 || guides.len() != self.source.len() {
            return Err(invalid("CPU denoising guide count or pass is invalid"));
        }
        let end = self.pixel + self.next_pixels();
        for pixel in self.pixel..end {
            cancel.check()?;
            let value = filter_pixel(self.width, self.height, &self.source, guides, pixel, 1 << self.pass)?;
            if let Some(destination) = self.destination.get_mut(pixel) {
                *destination = value;
            } else if self.destination.len() == pixel && self.destination.len() < self.destination.capacity() {
                self.destination.push(value);
            } else {
                return Err(invalid("CPU denoising destination exceeds admission"));
            }
        }
        self.pixel = end;
        if self.pixel == self.source.len() {
            std::mem::swap(&mut self.source, &mut self.destination);
            self.pixel = 0;
            self.pass += 1;
        }
        cancel.check()?;
        Ok(self.pass == 3)
    }
    pub(crate) fn finish(self) -> Result<Vec<[f32; 4]>> {
        if self.pass != 3 {
            return Err(invalid("CPU denoising is incomplete"));
        }
        Ok(self.source)
    }
}

fn rgb(value: [f32; 4]) -> [f64; 3] {
    [f64::from(value[0]), f64::from(value[1]), f64::from(value[2])]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn difference(a: [f32; 3], b: [f32; 3]) -> [f64; 3] {
    std::array::from_fn(|i| f64::from(a[i]) - f64::from(b[i]))
}
fn maximum(value: [f64; 3]) -> f64 {
    value.into_iter().fold(0.0, f64::max)
}
fn checked_color(value: [f32; 4]) -> Result<[f32; 4]> {
    if value.into_iter().any(|v| !v.is_finite() || v < 0.0) || value[3] > 1.0 {
        return Err(invalid("invalid raw CPU radiance or coverage before denoising"));
    }
    Ok(value)
}
fn filter_pixel(width: usize, height: usize, source: &[[f32; 4]], guides: &[Guide], pixel: usize, step: usize) -> Result<[f32; 4]> {
    let center = checked_color(*source.get(pixel).ok_or_else(|| invalid("CPU denoising source pixel missing"))?)?;
    let a = *guides.get(pixel).ok_or_else(|| invalid("CPU denoising guide missing"))?;
    a.validate()?;
    if a.body == 0 || center[3] < 0.999 {
        return Ok(center);
    }
    let mut mean = [0.0; 3];
    let mut total = 0.0;
    let center_rgb = rgb(center);
    let kernel = [1.0, 4.0, 6.0, 4.0, 1.0];
    for (ky, wy) in kernel.into_iter().enumerate() {
        for (kx, wx) in kernel.into_iter().enumerate() {
            let x = (pixel % width) as i64 + (kx as i64 - 2) * step as i64;
            let y = (pixel / width) as i64 + (ky as i64 - 2) * step as i64;
            if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                continue;
            }
            let index = y as usize * width + x as usize;
            let b = *guides.get(index).ok_or_else(|| invalid("CPU denoising neighbour guide missing"))?;
            let value = checked_color(*source.get(index).ok_or_else(|| invalid("CPU denoising neighbour missing"))?)?;
            if b.body != a.body || (f64::from(value[3]) - f64::from(center[3])).abs() > 0.01 {
                continue;
            }
            b.validate()?;
            let albedo = difference(a.albedo, b.albedo);
            let position = difference(b.position, a.position);
            let normal = a.normal.map(f64::from);
            let neighbour_normal = b.normal.map(f64::from);
            let plane = dot(position, normal).abs() + dot(position, neighbour_normal).abs();
            let footprint = f64::from(a.footprint.max(b.footprint)) * step as f64;
            let depth = (f64::from(a.depth) - f64::from(b.depth)).abs() / (8.0 * footprint).max(0.000001);
            let value_rgb = rgb(value);
            let contrast = maximum(std::array::from_fn(|i| (center_rgb[i] - value_rgb[i]).abs()));
            let color_scale = 0.25 + 0.5 * maximum(center_rgb).max(maximum(value_rgb));
            let weight = wx
                * wy
                * (-dot(albedo, albedo) / 0.04 - plane / (0.05 * footprint).max(0.000001) - depth - contrast / color_scale).exp()
                * dot(normal, neighbour_normal).max(0.0).powi(32);
            if weight > 0.0 {
                total += weight;
                for (channel, value) in mean.iter_mut().zip(value_rgb) {
                    *channel += (value - *channel) * (weight / total);
                }
            }
        }
    }
    if !total.is_finite() || total <= 0.0 {
        return Err(invalid("CPU denoising centre did not contribute"));
    }
    checked_color([mean[0] as f32, mean[1] as f32, mean[2] as f32, center[3]])
}
