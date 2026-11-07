//! Original bounded CPU a-trous output filter. Raw film and transport are never borrowed mutably.
//! The independent f64 gather oracle remains in the renderer tests, outside this implementation.
use super::{CancelToken, ErrorCode, JobError, Result};

pub(crate) const GUIDE_BYTES: u64 = 96;
pub(crate) const MAX_STEP_PIXELS: usize = 256;
const OPTICAL_TAG: u32 = 0xa000_0000;
#[cfg(any(test, feature = "raytrace-cpu"))]
const OPTICAL_ID_MAX: u32 = 0x1ff;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SurfaceGuide {
    pub albedo: [f32; 3],
    /// Captured body ordinal plus one, or a packed optical correspondence.
    /// Zero means that guidance is unavailable.
    pub body: u32,
    pub normal: [f32; 3],
    pub depth: f32,
    /// Camera-relative position, converted only after the f64 subtraction.
    pub position: [f32; 3],
    pub footprint: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Guide {
    pub albedo: [f32; 3],
    /// Captured body ordinal plus one, or a packed optical correspondence.
    /// Zero means that guidance is unavailable.
    pub body: u32,
    pub normal: [f32; 3],
    pub depth: f32,
    /// Camera-relative position, converted only after the f64 subtraction.
    pub position: [f32; 3],
    pub footprint: f32,
    /// Reflected endpoint when the main guide describes transmission.
    pub reflected: SurfaceGuide,
}
const _: [(); GUIDE_BYTES as usize] = [(); std::mem::size_of::<Guide>()];

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
impl Guide {
    /// Identity is shared with WGSL. Each endpoint uses a nonzero captured
    /// body ID. A zero reflected or transmitted ID denotes an absent branch.
    #[cfg(any(test, feature = "raytrace-cpu"))]
    pub(crate) fn optical_body(primary: u32, reflected: u32, transmitted: u32) -> Option<u32> {
        (primary != 0 && (reflected != 0 || transmitted != 0) && [primary, reflected, transmitted].into_iter().all(|id| id <= OPTICAL_ID_MAX))
            .then_some(OPTICAL_TAG | (primary << 18) | (reflected << 9) | transmitted)
    }

    fn optical(self) -> bool {
        self.body & 0xf800_0000 == OPTICAL_TAG
    }

    pub(crate) fn surface(self) -> SurfaceGuide {
        SurfaceGuide {
            albedo: self.albedo,
            body: self.body,
            normal: self.normal,
            depth: self.depth,
            position: self.position,
            footprint: self.footprint,
        }
    }
    pub(crate) fn validate(self) -> Result<()> {
        self.surface().validate()?;
        self.reflected.validate()
    }
}
impl SurfaceGuide {
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
fn surface_weight(a: SurfaceGuide, b: SurfaceGuide, step: usize) -> f64 {
    if a.body != b.body {
        return 0.0;
    }
    if a.body == 0 {
        return 1.0;
    }
    let albedo = difference(a.albedo, b.albedo);
    let position = difference(b.position, a.position);
    let normal = a.normal.map(f64::from);
    let neighbour_normal = b.normal.map(f64::from);
    let plane = dot(position, normal).abs() + dot(position, neighbour_normal).abs();
    let footprint = f64::from(a.footprint.max(b.footprint)) * step as f64;
    let depth = (f64::from(a.depth) - f64::from(b.depth)).abs() / (8.0 * footprint).max(0.000001);
    (-dot(albedo, albedo) / 0.04 - plane / (0.05 * footprint).max(0.000001) - depth).exp() * dot(normal, neighbour_normal).max(0.0).powi(32)
}
fn filter_pixel(width: usize, height: usize, source: &[[f32; 4]], guides: &[Guide], pixel: usize, step: usize) -> Result<[f32; 4]> {
    let center = checked_color(*source.get(pixel).ok_or_else(|| invalid("CPU denoising source pixel missing"))?)?;
    let a = *guides.get(pixel).ok_or_else(|| invalid("CPU denoising guide missing"))?;
    a.validate()?;
    let optical = a.optical();
    if a.body == 0 || center[3] < 0.999 || (optical && step > 2) {
        return Ok(center);
    }
    let mut mean = [0.0; 3];
    let mut total = 0.0;
    let center_rgb = rgb(center);
    let center_peak = maximum(center_rgb);
    let contrast_strength = if optical { 4.0 } else { 1.0 };
    let mut support_count = 0;
    let mut support_peak: f64 = 0.0;
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
            let value_rgb = rgb(value);
            let value_peak = maximum(value_rgb);
            let guide_weight = surface_weight(a.surface(), b.surface(), step) * surface_weight(a.reflected, b.reflected, step);
            if !optical && step == 1 && index != pixel && kx.abs_diff(2) <= 1 && ky.abs_diff(2) <= 1 && guide_weight >= 0.5 {
                support_count += 1;
                support_peak = support_peak.max(value_peak);
            }
            let contrast = maximum(std::array::from_fn(|i| (center_rgb[i] - value_rgb[i]).abs()));
            let brighter = center_peak.max(value_peak);
            let color_scale = 0.25 + 0.5 * brighter;
            let peak_bound = 16.0 * (0.25 + center_peak.min(value_peak));
            // Keep a high contrast tap from spreading a single bright sample.
            let peak_weight = if brighter > peak_bound { (peak_bound / brighter).powi(2) } else { 1.0 };
            let weight = wx * wy * guide_weight * (-contrast_strength * contrast / color_scale).exp() * peak_weight;
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
    // Clamp only isolated outliers with enough compatible immediate neighbours.
    if !optical && step == 1 && support_count >= 5 && center_peak > 16.0 * (0.25 + support_peak) {
        let cap = 2.0 * support_peak + 0.25;
        let peak = maximum(mean);
        if peak > cap {
            let scale = cap / peak;
            mean = mean.map(|channel| channel * scale);
        }
    }
    checked_color([mean[0] as f32, mean[1] as f32, mean[2] as f32, center[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_glass_filter_preserves_edges_within_the_same_reflected_body() {
        let mut guide = Guide {
            body: Guide::optical_body(1, 2, 3).unwrap(),
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide::default(),
        };
        guide.reflected = SurfaceGuide { body: 2, ..guide.surface() };
        let mut colors = [[0.5, 0.5, 0.5, 1.0]; 25];
        colors[12] = [0.7, 0.7, 0.7, 1.0];
        let smooth = filter_pixel(5, 5, &colors, &[guide; 25], 12, 1).unwrap();
        assert!(smooth[0] < 0.65);
        for edge in 0..4 {
            let mut neighbor = guide;
            match edge {
                0 => neighbor.reflected.albedo = [1.0, 0.0, 0.0],
                1 => neighbor.reflected.normal = [1.0, 0.0, 0.0],
                2 => neighbor.reflected.depth = 200.0,
                _ => neighbor.reflected.position = [0.0, 0.0, 2.0],
            }
            let mut guides = [neighbor; 25];
            guides[12] = guide;
            let filtered = filter_pixel(5, 5, &colors, &guides, 12, 1).unwrap();
            assert!(filtered[0] > 0.699, "reflected edge {edge}: {filtered:?}");
            assert_eq!(filtered[3], 1.0);
        }
        let invalid = Guide { reflected: SurfaceGuide { depth: f32::NAN, ..guide.reflected }, ..guide };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn optical_filter_respects_both_endpoints_and_preserves_unsupported_glints() {
        let identity = Guide::optical_body(1, 2, 3).unwrap();
        let boundary = Guide::optical_body(1, 4, 3).unwrap();
        assert_ne!(identity, boundary);
        let packed = f32::from_bits(Guide::optical_body(511, 511, 511).unwrap());
        assert!(packed.is_normal() && packed.is_sign_negative());
        assert!(Guide::optical_body(512, 2, 3).is_none());

        let guide = Guide {
            body: identity,
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide::default(),
        };
        let mut guides = [guide; 25];
        let mut colors = [[0.5, 0.5, 0.5, 1.0]; 25];
        colors[12] = [0.7, 0.7, 0.7, 1.0];
        assert!(filter_pixel(5, 5, &colors, &guides, 12, 1).unwrap()[0] < 0.65);
        assert_eq!(filter_pixel(5, 5, &colors, &guides, 12, 4).unwrap(), colors[12]);

        colors[12] = [512.0, 128.0, 32.0, 1.0];
        let glint = filter_pixel(5, 5, &colors, &guides, 12, 1).unwrap();
        assert!(glint[0] > 511.0);
        assert_eq!(glint[3], colors[12][3]);

        colors[12] = [0.5, 0.5, 0.5, 1.0];
        colors[13] = [1.0; 4];
        guides[13].body = boundary;
        assert_eq!(filter_pixel(5, 5, &colors, &guides, 12, 1).unwrap(), colors[12]);
    }
}
