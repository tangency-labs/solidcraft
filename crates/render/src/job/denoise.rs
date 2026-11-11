//! Original bounded CPU a-trous output filter. Raw film and transport are never borrowed mutably.
//! The independent f64 gather oracle remains in the renderer tests, outside this implementation.
use super::{CancelToken, DenoiseSettings, ErrorCode, JobError, Result};
use crate::radiance::{CameraPath, UncertaintyPixel};
#[path = "denoise_components.rs"]
mod components;
pub(crate) use components::ComponentDenoiseTask;

pub(crate) const GUIDE_BYTES: u64 = 96;
pub(crate) const MAX_STEP_PIXELS: usize = 256;
const OPTICAL_TAG: u32 = 0xa000_0000;
const OPTICAL_TIR_TAG: u32 = 0xa800_0000;
/// Exact integer range of the redundant reflected-body f32 lane on the GPU.
pub(crate) const OPTICAL_ROUTE_MAX: u32 = (1 << 24) - 1;
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

    /// The main identity already contains the reflected body. Under the TIR
    /// tag its redundant secondary body lane stores an exact boundary route.
    #[cfg(any(test, feature = "raytrace-cpu"))]
    pub(crate) fn with_optical_route(mut self, route: u32) -> Option<Self> {
        if !self.optical() || (self.body >> 9) & 0x1ff == 0 || self.body & 0x1ff == 0 || !(2..=OPTICAL_ROUTE_MAX).contains(&route) {
            return None;
        }
        self.body = (self.body & 0x07ff_ffff) | OPTICAL_TIR_TAG;
        self.reflected.body = route;
        Some(self)
    }

    pub(crate) fn optical_route(self) -> u32 {
        if self.body & 0xf800_0000 == OPTICAL_TIR_TAG { self.reflected.body } else { 0 }
    }

    fn compatible_route(self, other: Self, camera: Option<CameraPath>) -> bool {
        camera == Some(CameraPath::Reflected) || self.optical_route() == other.optical_route()
    }

    fn for_component(self, camera: Option<CameraPath>) -> Self {
        let Some(camera) = camera else { return self };
        if !self.optical() {
            return self;
        }
        let primary = (self.body >> 18) & 0x1ff;
        let reflected = (self.body >> 9) & 0x1ff;
        let transmitted = self.body & 0x1ff;
        let (surface, r, t) = match camera {
            CameraPath::Reflected if reflected != 0 => (if transmitted != 0 { self.reflected } else { self.surface() }, reflected, 0),
            CameraPath::Transmitted if transmitted != 0 => (self.surface(), 0, transmitted),
            _ => return Self::default(),
        };
        // This identity retains the primary silhouette and selected endpoint.
        // The other branch must not suppress a valid tap for this component.
        Self {
            albedo: surface.albedo,
            body: OPTICAL_TAG | (primary << 18) | (r << 9) | t,
            normal: surface.normal,
            depth: surface.depth,
            position: surface.position,
            footprint: surface.footprint,
            reflected: SurfaceGuide::default(),
        }
    }

    fn optical(self) -> bool {
        matches!(self.body & 0xf800_0000, OPTICAL_TAG | OPTICAL_TIR_TAG)
    }

    #[cfg(any(test, feature = "raytrace-cpu"))]
    pub(crate) fn from_surface(surface: SurfaceGuide) -> Self {
        Self {
            albedo: surface.albedo,
            body: surface.body,
            normal: surface.normal,
            depth: surface.depth,
            position: surface.position,
            footprint: surface.footprint,
            reflected: SurfaceGuide::default(),
        }
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
        if self.body & 0xf800_0000 == OPTICAL_TIR_TAG
            && ((self.body >> 9) & 0x1ff == 0 || self.body & 0x1ff == 0 || !(2..=OPTICAL_ROUTE_MAX).contains(&self.reflected.body))
        {
            return Err(invalid("invalid exact optical denoising route"));
        }
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
            || self.footprint.abs() < 0.000001
            || (self.footprint < 0.0 && self.depth < 0.0)
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
    uncertainty: Option<Vec<UncertaintyPixel>>,
    component: Option<CameraPath>,
}
impl DenoiseTask {
    pub(crate) fn new(width: u32, height: u32, source: Vec<[f32; 4]>) -> Result<Self> {
        Self::new_with_settings(width, height, source, DenoiseSettings::default())
    }
    pub(crate) fn new_with_settings(width: u32, height: u32, source: Vec<[f32; 4]>, settings: DenoiseSettings) -> Result<Self> {
        Self::new_with_uncertainty(width, height, source, settings, None)
    }
    pub(crate) fn new_with_uncertainty(
        width: u32,
        height: u32,
        source: Vec<[f32; 4]>,
        settings: DenoiseSettings,
        uncertainty: Option<Vec<UncertaintyPixel>>,
    ) -> Result<Self> {
        settings.validate()?;
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("CPU denoising dimensions overflow"))?;
        if width == 0 || height == 0 || source.len() != count || source.capacity() != count {
            return Err(invalid("CPU denoising requires one admitted complete linear pass"));
        }
        if settings.variance_guided != uncertainty.is_some()
            || uncertainty.as_ref().is_some_and(|values| values.len() != count || values.capacity() != count)
        {
            return Err(invalid("variance-guided denoising requires one admitted uncertainty plane"));
        }
        let mut destination = Vec::new();
        destination.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU denoising colour allocation failed"))?;
        if destination.capacity() != count {
            return Err(invalid("CPU denoising colour capacity exceeds admission"));
        }
        Ok(Self { width: width as usize, height: height as usize, source, destination, pixel: 0, pass: 0, settings, uncertainty, component: None })
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
            let value = filter_pixel_with_uncertainty(
                self.width,
                self.height,
                &self.source,
                guides,
                pixel,
                1 << self.pass,
                self.settings,
                self.uncertainty.as_deref(),
                self.component,
            )?;
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
    // A negative footprint denotes a non-opaque endpoint. Depth zero is an
    // environment direction; positive depth is the captured emitter ordinal.
    // These markers distinguish an endpoint from a surface with the same ID.
    if (a.footprint < 0.0) != (b.footprint < 0.0) {
        return 0.0;
    }
    if a.footprint < 0.0 {
        if a.depth != b.depth {
            return 0.0;
        }
        if a.depth == 0.0 {
            let footprint = f64::from(a.footprint.abs().max(b.footprint.abs())) * step as f64;
            let chord = difference(a.normal, b.normal);
            return (-dot(chord, chord) / (4.0 * footprint * footprint).max(1e-12)).exp();
        }
    }
    let albedo = difference(a.albedo, b.albedo);
    let position = difference(b.position, a.position);
    let normal = a.normal.map(f64::from);
    let neighbour_normal = b.normal.map(f64::from);
    let plane = dot(position, normal).abs() + dot(position, neighbour_normal).abs();
    let footprint = f64::from(a.footprint.abs().max(b.footprint.abs())) * step as f64;
    let depth = (f64::from(a.depth) - f64::from(b.depth)).abs() / (8.0 * footprint).max(0.000001);
    (-dot(albedo, albedo) / 0.04 - plane / (0.05 * footprint).max(0.000001) - depth).exp() * dot(normal, neighbour_normal).max(0.0).powi(32)
}
#[cfg(test)]
fn filter_pixel(
    width: usize,
    height: usize,
    source: &[[f32; 4]],
    guides: &[Guide],
    pixel: usize,
    step: usize,
    settings: DenoiseSettings,
) -> Result<[f32; 4]> {
    filter_pixel_with_uncertainty(width, height, source, guides, pixel, step, settings, None, None)
}

// Camera scatter controls a display heuristic, never a confidence statement.
// Reuse the unfiltered moments at every scale; filtered neighbours are correlated.
#[allow(clippy::too_many_arguments)]
fn filter_pixel_with_uncertainty(
    width: usize,
    height: usize,
    source: &[[f32; 4]],
    guides: &[Guide],
    pixel: usize,
    step: usize,
    settings: DenoiseSettings,
    uncertainty: Option<&[UncertaintyPixel]>,
    component: Option<CameraPath>,
) -> Result<[f32; 4]> {
    let center = checked_color(*source.get(pixel).ok_or_else(|| invalid("CPU denoising source pixel missing"))?)?;
    let original = guides.get(pixel).copied().ok_or_else(|| invalid("CPU denoising guide missing"))?;
    original.validate()?;
    let a = original.for_component(component);
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
    let center_uncertainty =
        uncertainty.map(|values| values.get(pixel).copied().ok_or_else(|| invalid("CPU denoising uncertainty missing"))).transpose()?;
    if let Some(value) = center_uncertainty {
        value.validate()?;
    }
    let strength = settings.strength
        * center_uncertainty.filter(|value| value.count >= 2).map_or(1.0, |value| {
            let noise = maximum(value.variance_of_mean.map(f64::from));
            noise / (noise + (0.01 + 0.02 * center_peak).powi(2))
        });
    if strength == 0.0 {
        return Ok(center);
    }
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
            let neighbour = guides.get(index).copied().ok_or_else(|| invalid("CPU denoising neighbour guide missing"))?;
            if !original.compatible_route(neighbour, component) {
                continue;
            }
            let b = neighbour.for_component(component);
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
            let tap_uncertainty = uncertainty
                .map(|values| values.get(index).copied().ok_or_else(|| invalid("CPU denoising neighbour uncertainty missing")))
                .transpose()?;
            if let Some(value) = tap_uncertainty {
                value.validate()?;
            }
            let color_distance = match (center_uncertainty, tap_uncertainty) {
                (Some(a), Some(b)) if a.count >= 2 && b.count >= 2 => {
                    let noise =
                        maximum(std::array::from_fn(|channel| f64::from(a.variance_of_mean[channel]) + f64::from(b.variance_of_mean[channel])));
                    contrast * contrast / (9.0 * noise).max(1e-12)
                }
                _ => contrast / color_scale,
            };
            let weight = wx * wy * guide_weight * (-contrast_strength * color_distance).exp() * peak_weight;
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
    if strength != 1.0 {
        mean = std::array::from_fn(|channel| center_rgb[channel] + (mean[channel] - center_rgb[channel]) * strength);
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
    #[test]
    fn uncertainty_guided_filter_preserves_quiet_detail_and_reduces_noisy_scatter() {
        let guide = Guide {
            body: 1,
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide::default(),
        };
        let guides = vec![guide; 25];
        let mut source = vec![[0.5, 0.25, 0.125, 1.0]; 25];
        source[12] = [0.7, 0.35, 0.175, 1.0];
        let settings = DenoiseSettings { passes: 1, variance_guided: true, ..Default::default() };
        let run = |uncertainty: Vec<UncertaintyPixel>| {
            let mut task = DenoiseTask::new_with_uncertainty(5, 5, source.clone(), settings, Some(uncertainty)).unwrap();
            assert!(task.step(&guides, &CancelToken::default()).unwrap());
            task.finish().unwrap()
        };
        let quiet = vec![UncertaintyPixel { variance_of_mean: [0.0; 3], count: 32 }; 25];
        assert_eq!(run(quiet), source, "resolved detail with zero observed scatter is unchanged");
        let noisy = vec![UncertaintyPixel { variance_of_mean: [0.04; 3], count: 32 }; 25];
        let result = run(noisy);
        assert!(result[12][0] < 0.6);
        assert_eq!(result[12][3], source[12][3]);
        let unknown = run(vec![UncertaintyPixel { variance_of_mean: [0.0; 3], count: 1 }; 25]);
        let legacy = filter_pixel(5, 5, &source, &guides, 12, 1, DenoiseSettings { passes: 1, ..Default::default() }).unwrap();
        assert_eq!(unknown[12], legacy, "a single observation is unknown, not zero uncertainty");
        assert_eq!(source[12], [0.7, 0.35, 0.175, 1.0]);
        assert!(DenoiseTask::new_with_settings(5, 5, source.clone(), settings).is_err());
        assert!(DenoiseTask::new_with_uncertainty(5, 5, source, settings, Some(vec![UncertaintyPixel::default(); 24])).is_err());
    }

    #[test]
    fn directional_and_emitter_guides_have_distinct_bounded_correspondence() {
        let environment = SurfaceGuide { body: 511, albedo: [0.0; 3], normal: [0.0, 0.0, 1.0], depth: 0.0, position: [0.0; 3], footprint: -0.01 };
        environment.validate().unwrap();
        assert_eq!(surface_weight(environment, environment, 1), 1.0);
        let opposite = SurfaceGuide { normal: [0.0, 0.0, -1.0], ..environment };
        assert!(surface_weight(environment, opposite, 1) < 1e-8);
        let opaque = SurfaceGuide { footprint: 0.01, ..environment };
        assert_eq!(surface_weight(environment, opaque, 1), 0.0, "a reserved endpoint ID cannot collide with a body ID");
        let emitter = SurfaceGuide { body: 1, depth: 1.0, ..environment };
        emitter.validate().unwrap();
        assert_eq!(surface_weight(emitter, SurfaceGuide { depth: 2.0, ..emitter }, 1), 0.0);
        assert_eq!(surface_weight(emitter, emitter, 1), 1.0);
    }
    #[test]
    fn component_filter_requires_captured_cpu_path_observations() {
        let mut options = crate::job::RaytraceOptions {
            denoise: true,
            denoise_settings: DenoiseSettings { component_filtering: true, ..Default::default() },
            path: Some(crate::job::CpuPathOptions::default()),
            progressive: Some(crate::job::CpuProgressiveOptions::default()),
            ..Default::default()
        };
        let error = options.validate().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("component filtering"));
        options.radiance_components = true;
        options.validate().unwrap();
        options.progressive = None;
        assert!(options.validate().is_err(), "component moments need complete production observations");
        options.progressive = Some(crate::job::CpuProgressiveOptions::default());
        options.path = None;
        assert!(options.validate().is_err(), "legacy branching transport cannot supply this partition");
    }

    #[test]
    fn independent_component_filter_uses_each_branch_and_preserves_raw_alpha() {
        use crate::radiance::{LightPath, RadianceComponents};
        use crate::radiance_film::RadianceFilm;
        let guide = Guide {
            body: Guide::optical_body(1, 2, 3).unwrap(),
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide { body: 2, albedo: [0.5; 3], normal: [0.0, 0.0, 1.0], depth: 1.0, position: [0.0; 3], footprint: 1.0 },
        };
        let mut guides = vec![Guide { body: Guide::optical_body(1, 2, 4).unwrap(), ..guide }; 25];
        guides[12] = guide;
        let cancel = CancelToken::default();
        let run = |other_transmission: f64| {
            let mut film = RadianceFilm::new(5, 5, true, true, &cancel).unwrap();
            let mut beauty = Vec::with_capacity(25);
            for pixel in 0..25 {
                let reflection = if pixel == 12 { 0.8 } else { 0.4 };
                let transmission = if pixel == 12 { 0.3 } else { other_transmission };
                for delta in [-0.2, 0.2] {
                    let mut sample = RadianceComponents::default();
                    sample.add(CameraPath::Reflected, LightPath::NonCaustic, [reflection + delta; 3]).unwrap();
                    sample.add(CameraPath::Transmitted, LightPath::NonCaustic, [transmission; 3]).unwrap();
                    film.observe(pixel, sample.beauty(), Some(&sample)).unwrap();
                }
                beauty.push([(reflection + transmission) as f32, (reflection + transmission) as f32, (reflection + transmission) as f32, 1.0]);
            }
            let raw = beauty.clone();
            let frame = film.component_frame(&cancel).unwrap();
            let settings = DenoiseSettings { passes: 1, component_filtering: true, ..Default::default() };
            let mut task = ComponentDenoiseTask::new(5, 5, beauty, frame, settings).unwrap();
            for _ in 0..100 {
                if task.step(&guides, &cancel).unwrap() {
                    break;
                }
            }
            let filtered = task.finish().unwrap();
            assert!(filtered.iter().zip(&raw).all(|(a, b)| a[3] == b[3]));
            assert!(filtered[12][0] > 0.7 && filtered[12][0] < 0.95, "reflection noise should fall despite a transmission edge: {:?}", filtered[12]);
            assert_eq!(raw[12], [1.1, 1.1, 1.1, 1.0]);
            filtered[12]
        };
        assert_eq!(run(0.8), run(8.0), "unrelated transmitted radiance cannot guide or contaminate reflection filtering");
        assert_eq!(ComponentDenoiseTask::EXTRA_BYTES_PER_PIXEL, 164);
    }

    #[test]
    fn component_filter_retains_independent_pilot_beauty_and_checks_cancellation() {
        let cancel = CancelToken::default();
        let film = crate::radiance_film::RadianceFilm::new(1, 1, true, true, &cancel).unwrap();
        let raw = vec![[0.5, 0.25, 0.125, 0.5]];
        let settings = DenoiseSettings { passes: 1, component_filtering: true, ..Default::default() };
        let mut task = ComponentDenoiseTask::new(1, 1, raw.clone(), film.component_frame(&cancel).unwrap(), settings).unwrap();
        for _ in 0..100 {
            if task.step(&[Guide::default()], &cancel).unwrap() {
                break;
            }
        }
        assert_eq!(task.finish().unwrap(), raw);
        let mut interrupted = ComponentDenoiseTask::new(1, 1, raw, film.component_frame(&cancel).unwrap(), settings).unwrap();
        cancel.cancel();
        assert_eq!(interrupted.step(&[Guide::default()], &cancel).unwrap_err().code, ErrorCode::Cancelled);
    }
    #[test]
    fn exact_tir_routes_separate_transmission_without_suppressing_reflection_or_opt_out() {
        let base = Guide {
            body: Guide::optical_body(1, 2, 3).unwrap(),
            albedo: [0.5; 3],
            normal: [0.0, 0.0, 1.0],
            depth: 1.0,
            position: [0.0; 3],
            footprint: 1.0,
            reflected: SurfaceGuide { body: 2, albedo: [0.5; 3], normal: [0.0, 0.0, 1.0], depth: 1.0, position: [0.0; 3], footprint: 1.0 },
        };
        let a = base.with_optical_route(2939).unwrap();
        let b = base.with_optical_route(2940).unwrap();
        assert_eq!(std::mem::size_of::<Guide>(), 96);
        assert!(f32::from_bits(a.body).is_finite() && f32::from_bits(a.body) < 0.0);
        assert_eq!(a.optical_route(), 2939);
        assert!(base.with_optical_route(OPTICAL_ROUTE_MAX + 1).is_none());
        assert!(Guide { body: Guide::optical_body(1, 0, 3).unwrap(), ..base }.with_optical_route(2939).is_none());
        let mut pixels = vec![[0.4, 0.4, 0.4, 1.0]; 25];
        pixels[12] = [0.6, 0.6, 0.6, 1.0];
        let mut guides = vec![b; 25];
        guides[12] = a;
        let settings = DenoiseSettings { passes: 1, ..Default::default() };
        let run = |component, settings| filter_pixel_with_uncertainty(5, 5, &pixels, &guides, 12, 1, settings, None, component).unwrap();
        assert_eq!(run(None, settings), pixels[12]);
        assert_eq!(run(Some(CameraPath::Transmitted), settings), pixels[12], "projection must not erase the original route");
        assert!(run(Some(CameraPath::Reflected), settings)[0] < 0.55, "reflection does not depend on the transmitted internal route");
        for component in [None, Some(CameraPath::Reflected), Some(CameraPath::Transmitted)] {
            assert_eq!(run(component, DenoiseSettings { optical_filter: false, ..settings }), pixels[12]);
        }
    }
}
