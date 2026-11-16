//! Original bounded CPU a-trous output filter. Raw film and transport are never borrowed mutably.
//! The independent f64 gather oracle remains in the renderer tests, outside this implementation.
use super::{CancelToken, DenoiseSettings, ErrorCode, JobError, Result};

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
    settings: DenoiseSettings,
}
impl DenoiseTask {
    pub(crate) fn new(width: u32, height: u32, source: Vec<[f32; 4]>) -> Result<Self> {
        Self::new_with_settings(width, height, source, DenoiseSettings::default())
    }
    pub(crate) fn new_with_settings(width: u32, height: u32, source: Vec<[f32; 4]>, settings: DenoiseSettings) -> Result<Self> {
        settings.validate()?;
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("CPU denoising dimensions overflow"))?;
        if width == 0 || height == 0 || source.len() != count || source.capacity() != count {
            return Err(invalid("CPU denoising requires one admitted complete linear pass"));
        }
        let mut destination = Vec::new();
        destination.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU denoising colour allocation failed"))?;
        if destination.capacity() != count {
            return Err(invalid("CPU denoising colour capacity exceeds admission"));
        }
        Ok(Self { width: width as usize, height: height as usize, source, destination, pixel: 0, pass: 0, settings })
    }
    pub(crate) fn next_pixels(&self) -> usize {
        (self.source.len() - self.pixel).min(MAX_STEP_PIXELS)
    }
    /// One work unit is one pixel visit with a fixed 25-tap gather, as for bounded finishing kernels.
    /// The caller debits next_pixels before calling; no work, limit or cancellation is reset here.
    pub(crate) fn step(&mut self, guides: &[Guide], cancel: &CancelToken) -> Result<bool> {
        cancel.check()?;
        if self.pass >= self.settings.passes as usize || guides.len() != self.source.len() {
            return Err(invalid("CPU denoising guide count or pass is invalid"));
        }
        let end = self.pixel + self.next_pixels();
        for pixel in self.pixel..end {
            cancel.check()?;
            let value = filter_pixel(self.width, self.height, &self.source, guides, pixel, 1 << self.pass, self.settings)?;
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
        Ok(self.pass == self.settings.passes as usize)
    }
    pub(crate) fn finish(self) -> Result<Vec<[f32; 4]>> {
        if self.pass != self.settings.passes as usize {
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
fn filter_pixel(
    width: usize,
    height: usize,
    source: &[[f32; 4]],
    guides: &[Guide],
    pixel: usize,
    step: usize,
    settings: DenoiseSettings,
) -> Result<[f32; 4]> {
    let center = checked_color(*source.get(pixel).ok_or_else(|| invalid("CPU denoising source pixel missing"))?)?;
    let a = *guides.get(pixel).ok_or_else(|| invalid("CPU denoising guide missing"))?;
    a.validate()?;
    let optical = a.optical();
    if a.body == 0 || center[3] < 0.999 || settings.strength == 0.0 || (optical && (!settings.optical_filter || step > 2)) {
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
            if settings.firefly_suppression
                && !optical
                && step == 1
                && index != pixel
                && kx.abs_diff(2) <= 1
                && ky.abs_diff(2) <= 1
                && guide_weight >= 0.5
            {
                support_count += 1;
                support_peak = support_peak.max(value_peak);
            }
            let contrast = maximum(std::array::from_fn(|i| (center_rgb[i] - value_rgb[i]).abs()));
            let brighter = center_peak.max(value_peak);
            let color_scale = 0.25 + 0.5 * brighter;
            let peak_bound = 16.0 * (0.25 + center_peak.min(value_peak));
            // Keep a high contrast tap from spreading a single bright sample.
            let peak_weight = if settings.firefly_suppression && brighter > peak_bound { (peak_bound / brighter).powi(2) } else { 1.0 };
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
    if settings.firefly_suppression && !optical && step == 1 && support_count >= 5 && center_peak > 16.0 * (0.25 + support_peak) {
        let cap = 2.0 * support_peak + 0.25;
        let peak = maximum(mean);
        if peak > cap {
            let scale = cap / peak;
            mean = mean.map(|channel| channel * scale);
        }
    }
    if settings.strength != 1.0 {
        mean = std::array::from_fn(|channel| center_rgb[channel] + (mean[channel] - center_rgb[channel]) * settings.strength);
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
        let smooth = filter_pixel(5, 5, &colors, &[guide; 25], 12, 1, DenoiseSettings::default()).unwrap();
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
            let filtered = filter_pixel(5, 5, &colors, &guides, 12, 1, DenoiseSettings::default()).unwrap();
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
        assert!(filter_pixel(5, 5, &colors, &guides, 12, 1, DenoiseSettings::default()).unwrap()[0] < 0.65);
        assert_eq!(filter_pixel(5, 5, &colors, &guides, 12, 4, DenoiseSettings::default()).unwrap(), colors[12]);

        colors[12] = [512.0, 128.0, 32.0, 1.0];
        let glint = filter_pixel(5, 5, &colors, &guides, 12, 1, DenoiseSettings { firefly_suppression: true, ..Default::default() }).unwrap();
        assert!(glint[0] > 511.0);
        assert_eq!(glint[3], colors[12][3]);

        colors[12] = [0.5, 0.5, 0.5, 1.0];
        colors[13] = [1.0; 4];
        guides[13].body = boundary;
        assert_eq!(filter_pixel(5, 5, &colors, &guides, 12, 1, DenoiseSettings::default()).unwrap(), colors[12]);
    }

    #[test]
    fn optical_filter_preference_restores_legacy_opaque_only_output() {
        let surface = Guide {
            body: 1,
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide::default(),
        };
        let mirror = Guide { body: Guide::optical_body(2, 1, 0).unwrap(), ..surface };
        let glass = Guide { body: Guide::optical_body(3, 1, 4).unwrap(), reflected: surface.surface(), ..surface };
        let mut guides: Vec<_> = (0..75)
            .map(|pixel| match pixel % 15 {
                0..=4 => surface,
                5..=9 => mirror,
                _ => glass,
            })
            .collect();
        guides[0] = Guide::default();
        let mut raw = vec![[0.5, 0.25, 0.125, 1.0]; 75];
        raw[1] = [0.2, 0.1, 0.05, 0.5];
        for center in [32, 37, 42] {
            raw[center] = [0.7, 0.35, 0.175, 1.0];
        }
        // Recreate the previous guide policy independently: optical first hits
        // supplied no guide, while ordinary opaque surfaces remained filterable.
        let legacy_guides: Vec<_> = guides.iter().map(|guide| if guide.optical() { Guide::default() } else { *guide }).collect();
        let run = |settings: DenoiseSettings, guides: &[Guide]| {
            let mut task = DenoiseTask::new_with_settings(15, 5, raw.clone(), settings).unwrap();
            for _ in 0..3 {
                if task.step(guides, &CancelToken::default()).unwrap() {
                    break;
                }
            }
            task.finish().unwrap()
        };
        assert!(DenoiseSettings::default().optical_filter);
        for passes in 1..=3 {
            for firefly_suppression in [false, true] {
                let settings = DenoiseSettings { passes, firefly_suppression, ..Default::default() };
                let enabled = run(settings, &guides);
                let disabled = run(DenoiseSettings { optical_filter: false, ..settings }, &guides);
                assert_eq!(disabled, run(settings, &legacy_guides));
                assert!(disabled[32][0] < raw[32][0], "opaque denoising remains active");
                assert!(enabled[37][0] < raw[37][0], "supported mirror filtering remains the default");
                assert!(enabled[42][0] < raw[42][0], "supported glass filtering remains the default");
                for (pixel, guide) in guides.iter().enumerate() {
                    assert_eq!(enabled[pixel][3].to_bits(), raw[pixel][3].to_bits());
                    assert_eq!(disabled[pixel][3].to_bits(), raw[pixel][3].to_bits());
                    if guide.optical() {
                        assert_eq!(disabled[pixel].map(f32::to_bits), raw[pixel].map(f32::to_bits));
                    } else {
                        assert_eq!(enabled[pixel].map(f32::to_bits), disabled[pixel].map(f32::to_bits));
                    }
                }
            }
        }
    }

    #[test]
    fn selected_passes_and_strength_preserve_alpha_and_default_behavior() {
        let guide = Guide {
            body: 1,
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide::default(),
        };
        let guides = [guide; 25];
        let mut colors = vec![[0.5, 0.25, 0.125, 1.0]; 25];
        colors[12] = [0.7, 0.4, 0.2, 1.0];
        colors[0] = [0.2, 0.1, 0.05, 0.5];
        let cancel = CancelToken::default();
        let run = |settings: DenoiseSettings| {
            let mut task = DenoiseTask::new_with_settings(5, 5, colors.clone(), settings).unwrap();
            let mut visits = 0;
            for _ in 0..3 {
                visits += task.next_pixels();
                if task.step(&guides, &cancel).unwrap() {
                    break;
                }
            }
            assert_eq!(visits, 25 * settings.passes as usize);
            let result = task.finish().unwrap();
            for (filtered, original) in result.iter().zip(&colors) {
                assert_eq!(filtered[3], original[3]);
            }
            assert_eq!(result[0], colors[0]);
            result
        };
        let one = run(DenoiseSettings { passes: 1, ..Default::default() });
        let half = run(DenoiseSettings { passes: 1, strength: 0.5, ..Default::default() });
        for channel in 0..3 {
            let expected = (f64::from(colors[12][channel]) + f64::from(one[12][channel])) * 0.5;
            assert!((f64::from(half[12][channel]) - expected).abs() < 1e-7);
        }
        let two = run(DenoiseSettings { passes: 2, ..Default::default() });
        let three = run(DenoiseSettings::default());
        assert!(one[12][0] > two[12][0]);
        // On this 5x5 fixture, the centre has no neighbours at spacing 4.
        assert_eq!(two[12], three[12]);
        for passes in 1..=3 {
            assert_eq!(run(DenoiseSettings { passes, strength: 0.0, ..Default::default() }), colors);
        }
        let mut legacy = DenoiseTask::new(5, 5, colors.clone()).unwrap();
        for _ in 0..3 {
            legacy.step(&guides, &cancel).unwrap();
        }
        assert_eq!(legacy.finish().unwrap(), three);
        for settings in [
            DenoiseSettings { passes: 0, ..Default::default() },
            DenoiseSettings { passes: 4, ..Default::default() },
            DenoiseSettings { strength: f64::NAN, ..Default::default() },
        ] {
            assert!(DenoiseTask::new_with_settings(5, 5, colors.clone(), settings).is_err());
        }
    }

    #[test]
    fn firefly_control_preserves_legacy_opaque_filter_and_optical_glints() {
        let guide = Guide {
            body: 1,
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide::default(),
        };
        let mut colors = [[0.5, 0.125, 0.0625, 1.0]; 25];
        colors[12] = [4096.0, 1024.0, 512.0, 1.0];
        let off = DenoiseSettings::default();
        assert!(!off.firefly_suppression);
        assert_eq!(serde_json::from_str::<DenoiseSettings>("{}").unwrap(), off);
        let on = DenoiseSettings { firefly_suppression: true, ..off };
        let capped = filter_pixel(5, 5, &colors, &[guide; 25], 12, 1, on).unwrap();
        let uncapped = filter_pixel(5, 5, &colors, &[guide; 25], 12, 1, off).unwrap();
        assert!(capped[0] <= 1.25);
        // Independent legacy opaque oracle: the constant 5x5 guide has total
        // kernel weight 256, of which the centre owns 36. All other taps share
        // the original contrast weight, with neither peak attenuation nor a cap.
        let tap_weight = (-(4096.0_f64 - 0.5) / (0.25 + 0.5 * 4096.0)).exp();
        for channel in 0..3 {
            let expected = (36.0 * f64::from(colors[12][channel]) + 220.0 * tap_weight * f64::from(colors[0][channel])) / (36.0 + 220.0 * tap_weight);
            assert!((f64::from(uncapped[channel]) - expected).abs() < 0.0002, "{uncapped:?} vs {expected}");
        }
        assert_eq!((capped[3], uncapped[3]), (1.0, 1.0));
        let optical = Guide { body: Guide::optical_body(1, 0, 2).unwrap(), ..guide };
        let a = filter_pixel(5, 5, &colors, &[optical; 25], 12, 1, on).unwrap();
        let b = filter_pixel(5, 5, &colors, &[optical; 25], 12, 1, off).unwrap();
        assert!(a[0] > 4095.0);
        assert!(b[0] > 4096.0 * 0.99, "the disabled opaque cap must not flatten an optical glint: {b:?}");
        assert_eq!((a[3], b[3]), (1.0, 1.0));
        assert_eq!(filter_pixel(5, 5, &colors, &[optical; 25], 12, 4, on).unwrap(), colors[12]);
    }
}
