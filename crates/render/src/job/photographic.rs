//! Original bounded image-space lens/artistic operators. Not a ray-traced lens,
//! calibrated sensor, reconstructed optical coating, or volumetric light source.
//! Sampling/interpolation is display-linear; the original EXR never changes.
use super::{CancelToken, ErrorCode, GPU_PATH_LIMITS, JobError, Result};
use serde::{Deserialize, Serialize};
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flare {
    pub strength: f64,
    /// Relative scene-linear highlight threshold, independent of PNG clipping.
    pub threshold: f64,
    pub ghosts: u32,
    pub halo: f64,
    pub streak: f64,
}
impl Default for Flare {
    fn default() -> Self {
        Self { strength: 0.0, threshold: 1.5, ghosts: 3, halo: 0.25, streak: 0.4 }
    }
}
impl Flare {
    pub fn validate(self) -> Result<()> {
        if !self.strength.is_finite()
            || !(0.0..=2.0).contains(&self.strength)
            || !self.threshold.is_finite()
            || !(0.001..=1e6).contains(&self.threshold)
            || self.ghosts > 4
            || !self.halo.is_finite()
            || !(0.0..=2.0).contains(&self.halo)
            || !self.streak.is_finite()
            || !(0.0..=2.0).contains(&self.streak)
        {
            return Err(invalid("flare needs strength/halo/streak 0..2, threshold 0.001..1000000 and at most four ghosts per source"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Photographic {
    pub flare: Flare,
    pub chromatic_pixels: f64,
    pub radial_distortion: f64,
    pub soft_focus: f64,
    pub sharpening: f64,
    pub grain: f64,
    pub seed: u32,
}
impl Default for Photographic {
    fn default() -> Self {
        Self { flare: Default::default(), chromatic_pixels: 0.0, radial_distortion: 0.0, soft_focus: 0.0, sharpening: 0.0, grain: 0.0, seed: 0 }
    }
}
impl Photographic {
    pub fn active(self) -> bool {
        self.flare.strength > 0.0
            || self.chromatic_pixels != 0.0
            || self.radial_distortion != 0.0
            || self.soft_focus != 0.0
            || self.sharpening != 0.0
            || self.grain != 0.0
    }
    pub fn is_identity(&self) -> bool {
        !self.active()
    }
    pub fn validate(self) -> Result<()> {
        self.flare.validate()?;
        if !self.chromatic_pixels.is_finite()
            || !(0.0..=12.0).contains(&self.chromatic_pixels)
            || !self.radial_distortion.is_finite()
            || !(-0.3..=0.3).contains(&self.radial_distortion)
            || [self.soft_focus, self.sharpening].iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !self.grain.is_finite()
            || !(0.0..=0.15).contains(&self.grain)
        {
            return Err(invalid(
                "photographic effects require chromatic offset 0..12 pixels, radial coefficient -0.3..0.3, soft focus/sharpening 0..1 and grain 0..0.15",
            ));
        }
        Ok(())
    }
    pub fn scratch_bytes(self, width: u32, height: u32) -> Result<u64> {
        self.validate()?;
        if !self.active() {
            return Ok(0);
        }
        let count = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || u64::from(width) > GPU_PATH_LIMITS.max_dimension
            || u64::from(height) > GPU_PATH_LIMITS.max_dimension
            || count > GPU_PATH_LIMITS.max_pixels
        {
            return Err(invalid("photographic dimensions exceed the admitted output envelope"));
        }
        count.checked_mul(8).and_then(|n| n.checked_add(16_384)).ok_or_else(|| invalid("photographic scratch size overflow"))
    }
}
#[derive(Clone, Copy, Debug, Default)]
struct Highlight {
    position: [f64; 2],
    color: [f64; 3],
    radiance: f64,
}

/// The large bounded highlight/LUT state is fallibly allocated inside the
/// already admitted 16 KiB allowance, not embedded in every host task slot.
pub struct PhotographicTask {
    state: Vec<PhotographicState>,
}
impl PhotographicTask {
    pub fn new(width: u32, height: u32, source: Vec<u8>, linear: Vec<[f32; 4]>, options: Photographic, cancel: CancelToken) -> Result<Self> {
        options.scratch_bytes(width, height)?;
        if std::mem::size_of::<PhotographicState>() > 16_384 {
            return Err(invalid("photographic metadata exceeds its admitted allowance"));
        }
        let mut state = Vec::new();
        state.try_reserve_exact(1).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "photographic state allocation failed"))?;
        if state.capacity() != 1 {
            return Err(invalid("photographic state exceeds bounded capacity"));
        }
        state.push(PhotographicState::new(width, height, source, linear, options, cancel)?);
        Ok(Self { state })
    }
    pub fn step(&mut self, units: usize) -> Result<bool> {
        self.state.first_mut().ok_or_else(|| invalid("missing photographic state"))?.step(units)
    }
    pub fn progress(&self) -> (u64, u64) {
        self.state.first().map_or((0, 0), PhotographicState::progress)
    }
    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.state.pop().ok_or_else(|| invalid("missing photographic state"))?.finish()
    }
}

struct PhotographicState {
    width: usize,
    height: usize,
    options: Photographic,
    cancel: CancelToken,
    source: Vec<u8>,
    linear: Vec<[f32; 4]>,
    output: Vec<u8>,
    lookup: [f64; 256],
    tiles: [Highlight; 144],
    highlights: [Highlight; 4],
    source_count: usize,
    scanned: usize,
    pixel: usize,
    ready: bool,
}
impl PhotographicState {
    pub fn new(width: u32, height: u32, source: Vec<u8>, linear: Vec<[f32; 4]>, options: Photographic, cancel: CancelToken) -> Result<Self> {
        options.validate()?;
        cancel.check()?;
        let count = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || u64::from(width) > GPU_PATH_LIMITS.max_dimension
            || u64::from(height) > GPU_PATH_LIMITS.max_dimension
            || count > GPU_PATH_LIMITS.max_pixels
            || usize::try_from(count).ok() != Some(linear.len())
            || linear.len().checked_mul(4) != Some(source.len())
            || !options.active()
        {
            return Err(invalid("photographic stage requires an admitted opaque image, matching source pixels and an active effect"));
        }
        let mut output = Vec::new();
        output.try_reserve_exact(source.len()).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "photographic output allocation failed"))?;
        if output.capacity() != source.len() {
            return Err(invalid("photographic allocation exceeded admitted capacity"));
        }
        let linear = if options.flare.strength > 0.0 { linear } else { Vec::new() };
        let ready = linear.is_empty();
        Ok(Self {
            width: width as usize,
            height: height as usize,
            options,
            cancel,
            source,
            linear,
            output,
            lookup: std::array::from_fn(|i| crate::surface::srgb8_to_linear(i as u8)),
            tiles: [Highlight::default(); 144],
            highlights: [Highlight::default(); 4],
            source_count: 0,
            scanned: 0,
            pixel: 0,
            ready,
        })
    }
    fn detect(&mut self, budget: usize) -> Result<()> {
        let end = self.scanned.saturating_add(budget).min(self.linear.len());
        for index in self.scanned..end {
            if index % 1024 == 0 {
                self.cancel.check()?;
            }
            let sample = self.linear.get(index).ok_or_else(|| invalid("missing photographic source sample"))?;
            if sample.iter().any(|v| !v.is_finite() || *v < 0.0) || sample[3] > 1.0 {
                return Err(invalid("photographic highlight source is nonfinite or has invalid coverage"));
            }
            if sample[3] <= 0.0 {
                continue;
            }
            let rgb: [f64; 3] = std::array::from_fn(|i| f64::from(sample[i]) / f64::from(sample[3]));
            let luminance = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            if luminance <= self.options.flare.threshold {
                continue;
            }
            let x = index % self.width;
            let y = index / self.width;
            let tile = (y * 9 / self.height) * 16 + x * 16 / self.width;
            let slot = self.tiles.get_mut(tile).ok_or_else(|| invalid("invalid highlight tile"))?;
            if luminance > slot.radiance {
                let peak = rgb.iter().copied().fold(0.0, f64::max).max(1e-12);
                *slot = Highlight {
                    position: [(x as f64 + 0.5) / self.width as f64, (y as f64 + 0.5) / self.height as f64],
                    color: [rgb[0] / peak, rgb[1] / peak, rgb[2] / peak],
                    radiance: luminance,
                };
            }
        }
        self.scanned = end;
        if end == self.linear.len() {
            for _ in 0..4 {
                let mut best = None;
                for (index, value) in self.tiles.iter().enumerate() {
                    if value.radiance <= 0.0
                        || self
                            .highlights
                            .iter()
                            .take(self.source_count)
                            .any(|old| (old.position[0] - value.position[0]).hypot(old.position[1] - value.position[1]) < 0.09)
                    {
                        continue;
                    }
                    if best.is_none_or(|(_, radiance)| value.radiance > radiance) {
                        best = Some((index, value.radiance));
                    }
                }
                if let Some((index, _)) = best {
                    if let (Some(value), Some(target)) = (self.tiles.get(index), self.highlights.get_mut(self.source_count)) {
                        *target = *value;
                        self.source_count += 1;
                    }
                    if let Some(value) = self.tiles.get_mut(index) {
                        value.radiance = 0.0;
                    }
                }
            }
            self.linear = Vec::new();
            self.ready = true;
        }
        Ok(())
    }
    fn sample_channel(&self, x: f64, y: f64, channel: usize) -> Result<f64> {
        let x = x.clamp(0.0, (self.width - 1) as f64);
        let y = y.clamp(0.0, (self.height - 1) as f64);
        let a = x.floor() as usize;
        let b = y.floor() as usize;
        let dx = x - a as f64;
        let dy = y - b as f64;
        let mut values = [0.0; 4];
        for (i, (px, py)) in
            [(a, b), ((a + 1).min(self.width - 1), b), (a, (b + 1).min(self.height - 1)), ((a + 1).min(self.width - 1), (b + 1).min(self.height - 1))]
                .into_iter()
                .enumerate()
        {
            let offset = (py * self.width + px) * 4;
            if self.source.get(offset + 3).copied() != Some(255) {
                return Err(invalid("photographic effects require opaque output; alpha is never silently flattened"));
            }
            let byte = self.source.get(offset + channel).ok_or_else(|| invalid("photographic source bounds mismatch"))?;
            values[i] = self.lookup[usize::from(*byte)];
        }
        Ok((values[0] * (1.0 - dx) + values[1] * dx) * (1.0 - dy) + (values[2] * (1.0 - dx) + values[3] * dx) * dy)
    }
    fn mapped(&self, x: f64, y: f64) -> Result<[f64; 3]> {
        let short = self.width.min(self.height) as f64;
        let delta = [(x + 0.5 - self.width as f64 * 0.5) / short * 2.0, (y + 0.5 - self.height as f64 * 0.5) / short * 2.0];
        let r2 = delta[0] * delta[0] + delta[1] * delta[1];
        let factor = 1.0 + self.options.radial_distortion * r2;
        let center =
            [self.width as f64 * 0.5 - 0.5 + delta[0] * factor * short * 0.5, self.height as f64 * 0.5 - 0.5 + delta[1] * factor * short * 0.5];
        let chroma = self.options.chromatic_pixels * r2.min(2.0) * 0.5;
        let mut color = [0.0; 3];
        for (channel, value) in color.iter_mut().enumerate() {
            let shift = match channel {
                0 => chroma,
                2 => -chroma,
                _ => 0.0,
            };
            *value = self.sample_channel(center[0] + delta[0] * shift, center[1] + delta[1] * shift, channel)?;
        }
        Ok(color)
    }
    fn flare(&self, position: [f64; 2]) -> [f64; 3] {
        let mut result = [0.0; 3];
        let plan = self.options.flare;
        if plan.strength == 0.0 {
            return result;
        }
        let short = self.width.min(self.height) as f64;
        let aspect = [self.width as f64 / short, self.height as f64 / short];
        for source in self.highlights.iter().take(self.source_count) {
            let excess = (source.radiance - plan.threshold).max(0.0);
            let gain = plan.strength * excess / (excess + plan.threshold + 1.0);
            let dx = (position[0] - source.position[0]) * aspect[0];
            let dy = (position[1] - source.position[1]) * aspect[1];
            let halo = ((dx.hypot(dy) - 0.085) / 0.012).powi(2);
            let streak = (-0.5 * (dy * short / 1.7).powi(2)).exp() / (1.0 + (dx / 0.18).powi(2));
            let strength = plan.halo * (-0.5 * halo).exp() * 0.12 + plan.streak * streak * 0.25;
            for (channel, value) in result.iter_mut().enumerate() {
                *value += source.color[channel] * gain * strength;
            }
            for ghost in 0..plan.ghosts {
                let distance = 0.35 + f64::from(ghost) * 0.55;
                let center = [0.5 + (0.5 - source.position[0]) * distance, 0.5 + (0.5 - source.position[1]) * distance];
                let r = ((position[0] - center[0]) * aspect[0]).hypot((position[1] - center[1]) * aspect[1]) / (0.022 + f64::from(ghost) * 0.009);
                let falloff = (1.0 - (r - 0.72).abs() / 0.35).clamp(0.0, 1.0) * 0.055 + (-r * r * 1.3).exp() * 0.025;
                let tint = match ghost % 3 {
                    0 => [0.4, 0.8, 1.0],
                    1 => [1.0, 0.45, 0.22],
                    _ => [0.4, 1.0, 0.5],
                };
                for channel in 0..3 {
                    result[channel] += source.color[channel] * tint[channel] * gain * falloff;
                }
            }
        }
        result
    }
    pub fn step(&mut self, units: usize) -> Result<bool> {
        self.cancel.check()?;
        let budget = units.clamp(1, 16384);
        if !self.ready {
            self.detect(budget)?;
            return Ok(false);
        }
        let count = self.width * self.height;
        let end = self.pixel.saturating_add(budget).min(count);
        for index in self.pixel..end {
            if index % 256 == 0 {
                self.cancel.check()?;
            }
            let x = (index % self.width) as f64;
            let y = (index / self.width) as f64;
            let mut rgb = self.mapped(x, y)?;
            if self.options.soft_focus > 0.0 || self.options.sharpening > 0.0 {
                let mut blur = rgb;
                for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
                    let neighbor = self.mapped(x + dx, y + dy)?;
                    for channel in 0..3 {
                        blur[channel] += neighbor[channel];
                    }
                }
                for channel in 0..3 {
                    blur[channel] /= 5.0;
                    rgb[channel] +=
                        self.options.soft_focus * (blur[channel] - rgb[channel]) + self.options.sharpening * (rgb[channel] - blur[channel]);
                }
            }
            let position = [(x + 0.5) / self.width as f64, (y + 0.5) / self.height as f64];
            let flare = self.flare(position);
            let luminance = (rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722).max(0.0);
            let grain = (random(index as u32 ^ self.options.seed) - 0.5) * self.options.grain * (luminance + 0.03).sqrt();
            for channel in 0..3 {
                let linear = (rgb[channel] + flare[channel] + grain).max(0.0);
                let encoded = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
                self.output.push((encoded.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            self.output.push(255);
        }
        self.pixel = end;
        self.cancel.check()?;
        Ok(end == count)
    }
    pub fn progress(&self) -> (u64, u64) {
        (self.scanned as u64 + self.pixel as u64, (self.width * self.height * if self.options.flare.strength > 0.0 { 2 } else { 1 }) as u64)
    }
    pub fn finish(self) -> Result<Vec<u8>> {
        if self.pixel != self.width * self.height {
            return Err(invalid("photographic output is incomplete"));
        }
        Ok(self.output)
    }
}
fn random(mut value: u32) -> f64 {
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846ca68b);
    f64::from((value ^ (value >> 16)) >> 8) / 16777216.0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(options: Photographic, quantum: usize) -> Vec<u8> {
        let count = 32 * 24;
        let rgba = (0..count).flat_map(|i| [if i % 32 < 16 { 20 } else { 220 }, 100, 180, 255]).collect();
        let mut linear = vec![[0.4, 0.4, 0.4, 1.0]; count];
        linear[5 * 32 + 26] = [20.0, 10.0, 5.0, 1.0];
        let mut task = PhotographicTask::new(32, 24, rgba, linear, options, Default::default()).unwrap();
        while !task.step(quantum).unwrap() {}
        task.finish().unwrap()
    }
    #[test]
    fn deterministic_filters_are_quantum_independent_and_checked_before_allocation() {
        let options = Photographic {
            grain: 0.08,
            chromatic_pixels: 2.0,
            radial_distortion: 0.06,
            soft_focus: 0.1,
            sharpening: 0.2,
            flare: Flare { strength: 0.7, ..Default::default() },
            seed: 42,
        };
        assert_eq!(run(options, 1), run(options, 16384));
        assert_ne!(run(options, 1024), run(Photographic { seed: 43, ..options }, 1024));
        assert_eq!(Photographic::default().scratch_bytes(3840, 2160).unwrap(), 0);
        for v in [f64::NAN, f64::INFINITY, -0.1, 12.1] {
            assert!(Photographic { chromatic_pixels: v, ..Default::default() }.validate().is_err());
        }
        assert!(PhotographicTask::new(0, 1, vec![], vec![], options, Default::default()).is_err());
    }
    #[test]
    fn constants_survive_spatial_mapping_and_cancellation_preserves_source_contract() {
        let cancel = CancelToken::default();
        let options = Photographic { radial_distortion: 0.2, chromatic_pixels: 4.0, soft_focus: 0.5, sharpening: 0.2, ..Default::default() };
        let rgba = [77, 129, 201, 255].repeat(16);
        let mut task = PhotographicTask::new(4, 4, rgba.clone(), vec![[1.0; 4]; 16], options, cancel.clone()).unwrap();
        while !task.step(3).unwrap() {}
        assert_eq!(task.finish().unwrap(), rgba);
        let mut task = PhotographicTask::new(4, 4, rgba, vec![[1.0; 4]; 16], options, cancel.clone()).unwrap();
        cancel.cancel();
        assert!(task.step(1).is_err());
        assert!(task.finish().is_err());
    }
    #[test]
    fn flare_uses_actual_highlights_and_refuses_transparent_output() {
        let options = Photographic { flare: Flare { strength: 1.0, ..Default::default() }, ..Default::default() };
        let rgba = [100, 100, 100, 255].repeat(16);
        let mut task = PhotographicTask::new(4, 4, rgba.clone(), vec![[0.1, 0.1, 0.1, 1.0]; 16], options, Default::default()).unwrap();
        while !task.step(16).unwrap() {}
        assert_eq!(task.finish().unwrap(), rgba);
        let mut task = PhotographicTask::new(1, 1, vec![0, 0, 0, 0], vec![[0.0; 4]], options, Default::default()).unwrap();
        assert!(!task.step(1).unwrap());
        assert!(task.step(1).is_err());
        assert!(serde_json::from_str::<Photographic>(r#"{"physical_lens":true}"#).is_err());
    }
}
