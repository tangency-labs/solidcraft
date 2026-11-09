//! Ordered world-space aging, evaluated once per hit before its shared closure.
//! This is optical authorship, not corrosion kinetics or geometric displacement.
use crate::{
    job::{
        self,
        aging::{AgingKind, AgingLayer, AgingStack, SurfaceClass},
    },
    surface::{OpaqueSurface, srgb8_to_linear},
    surface_patterns::ShadingSurface,
};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

fn invalid(message: &'static str) -> job::JobError {
    job::JobError::new(job::ErrorCode::InvalidRequest, message)
}
fn hash(cell: [i64; 3], seed: u32) -> f64 {
    let [x, y, z] = cell.map(|n| (n as u64 & 4095) as u32);
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ z.wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h >> 8) / 16_777_216.0
}
fn mix(a: f64, b: f64, t: f64) -> f64 {
    if t == 0.0 {
        a
    } else if t == 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}
fn smooth(a: f64, b: f64, value: f64) -> f64 {
    let x = ((value - a) / (b - a)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn noise(point: Vec3, seed: u32) -> f64 {
    let base = [point.x.floor() as i64, point.y.floor() as i64, point.z.floor() as i64];
    let fraction = [point.x - point.x.floor(), point.y - point.y.floor(), point.z - point.z.floor()];
    let [x, y, z] = fraction.map(|v| v * v * (3.0 - 2.0 * v));
    let at = |dx, dy, dz| hash([base[0] + dx, base[1] + dy, base[2] + dz], seed);
    let low = mix(mix(at(0, 0, 0), at(1, 0, 0), x), mix(at(0, 1, 0), at(1, 1, 0), x), y);
    let high = mix(mix(at(0, 0, 1), at(1, 0, 1), x), mix(at(0, 1, 1), at(1, 1, 1), x), y);
    mix(low, high, z)
}
fn mask(layer: &AgingLayer, position: Vec3, outward: Vec3) -> job::Result<(f64, f64)> {
    // Subtract in f64 before scaling. Do not derive a world field from camera
    // rebasing: Scratch's floor(0.8*q) does not share noise's4096-cell period.
    let origin = Vec3::new(layer.origin_mm[0], layer.origin_mm[1], layer.origin_mm[2]);
    let q = (position - origin) / layer.scale_mm;
    if !q.is_finite() || q.x.abs().max(q.y.abs()).max(q.z.abs()) > 1.0e15 {
        return Err(invalid("CPU aging coordinates exceed the finite field bound"));
    }
    let coarse = noise(q, layer.seed);
    let fine = noise(q * 3.0 + Vec3::new(19.0, 7.0, 3.0), layer.seed + 1);
    let field = coarse * 0.75 + fine * 0.25;
    let threshold = 1.0 - layer.coverage;
    let mut coverage = if layer.coverage >= 1.0 { 1.0 } else { smooth(threshold - 0.09, threshold + 0.09, field) };
    if layer.kind == AgingKind::Scratches {
        let [x, y, z] = [outward.x.abs(), outward.y.abs(), outward.z.abs()];
        let [u, v] = if x > y && x > z {
            [q.y, q.z]
        } else if y > z {
            [q.x, q.z]
        } else {
            [q.x, q.y]
        };
        let band = (v * 0.8).floor() as i64;
        let line = ((u * 24.0 + hash([band, 0, 0], layer.seed) * 0.7).rem_euclid(1.0) - 0.5).abs();
        coverage *= 1.0 - smooth(0.025, 0.09, line);
        coverage *= smooth(0.18, 0.48, fine);
    }
    let slope = outward.z.max(0.0).powf(1.5);
    coverage *= mix(1.0, slope, layer.upward_bias);
    Ok(((coverage * layer.strength).clamp(0.0, 1.0), field))
}
fn apply(
    layer: &AgingLayer,
    original_metallic: f64,
    surface: OpaqueSurface,
    position: Vec3,
    outward: Vec3,
    body: usize,
) -> job::Result<OpaqueSurface> {
    if !layer.active()
        || layer.target.is_some_and(|target| target.body_index as usize != body)
        || (layer.surface_class == SurfaceClass::Metal && original_metallic < 0.5)
        || (layer.surface_class == SurfaceClass::Dielectric && original_metallic >= 0.5)
    {
        return Ok(surface);
    }
    let (weight, field) = mask(layer, position, outward)?;
    if weight == 0.0 {
        return Ok(surface);
    }
    let [r, g, b] = layer.color_srgb.map(srgb8_to_linear);
    let mut coat = Vec3::new(r, g, b);
    if matches!(layer.kind, AgingKind::Rust | AgingKind::Patina | AgingKind::Grime) {
        coat = coat * (0.45 + 0.7 * field);
    }
    if layer.kind == AgingKind::Fading {
        coat = surface.base_color().lerp(coat, 0.5);
    }
    coat = Vec3::new(coat.x.clamp(0.0, 1.0), coat.y.clamp(0.0, 1.0), coat.z.clamp(0.0, 1.0));
    let mut result = surface
        .with_authored_finish(
            if weight == 1.0 { coat } else { surface.base_color().lerp(coat, weight) },
            mix(surface.metallic(), layer.metallic, weight),
            mix(surface.roughness(), layer.roughness, weight),
        )
        .map_err(|_| invalid("invalid CPU aged surface"))?;
    if let Some(clear) = surface.clear_coat() {
        let clear = crate::clear_coat::ClearCoat::new(clear.weight() * (1.0 - weight), clear.roughness(), clear.ior())
            .map_err(|_| invalid("invalid CPU aged clear coat"))?;
        result = result.with_clear_coat(clear);
    }
    Ok(result)
}

#[derive(Clone, Copy)]
struct Hit {
    current: ShadingSurface,
    original_metallic: f64,
    position: Vec3,
    outward: Vec3,
    body: usize,
}
/// A single fallibly prepaid slot is reused by guides and then transport. Future
/// mapped surfaces enter begin() before aging; no map consumer is implied here.
#[derive(Default)]
pub(super) struct Task {
    hit: Option<Hit>,
    cursor: usize,
    ready: bool,
}
impl Task {
    pub(super) fn required_bytes() -> u64 {
        std::mem::size_of::<Self>() as u64
    }
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(super) fn started(&self) -> bool {
        self.hit.is_some()
    }
    pub(super) fn ready(&self) -> bool {
        self.ready
    }
    pub(super) fn begin(&mut self, input: ShadingSurface, position: Vec3, outward: Vec3, body: usize, eligible: bool) -> job::Result<()> {
        if !position.is_finite() || !outward.is_finite() || (outward.len() - 1.0).abs() > 1e-8 {
            return Err(invalid("CPU aging needs a finite hit and its original unit geometric normal"));
        }
        self.hit = Some(Hit { current: input, original_metallic: input.surface.metallic(), position, outward, body });
        self.cursor = 0;
        self.ready = !eligible;
        Ok(())
    }
    /// The caller charges exactly one existing cumulative-work unit before this
    /// one-layer continuation. No RNG, heap allocation or film mutation occurs.
    pub(super) fn advance(&mut self, stack: &AgingStack) -> job::Result<bool> {
        if self.ready {
            return Err(invalid("CPU aging hit is already complete"));
        }
        let mut hit = self.hit.ok_or_else(|| invalid("CPU aging hit is not initialized"))?;
        let layer = stack.get(self.cursor).ok_or_else(|| invalid("CPU aging layer cursor exceeds admission"))?;
        hit.current.surface = apply(layer, hit.original_metallic, hit.current.surface, hit.position, hit.outward, hit.body)?;
        self.hit = Some(hit);
        self.cursor += 1;
        self.ready = self.cursor == stack.len();
        Ok(self.ready)
    }
    pub(super) fn surface(&self) -> job::Result<ShadingSurface> {
        if !self.ready {
            return Err(invalid("CPU aging hit is incomplete"));
        }
        self.hit.map(|hit| hit.current).ok_or_else(|| invalid("CPU aging hit is missing"))
    }
    #[cfg(test)]
    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }
}

pub(super) fn eligible(options: &crate::job::RaytraceOptions, body: usize, name: &str) -> bool {
    !name.starts_with("__render_stage__/") && !options.dielectrics.iter().any(|optics| optics.body_index == body)
}

pub(super) fn validate_scene(scene: &crate::ProductScene, options: &crate::job::RaytraceOptions) -> job::Result<()> {
    if let Some(stack) = &options.aging {
        stack.validate_scene(scene)?;
        for layer in stack.iter().filter(|layer| layer.active()) {
            if layer.target.is_some_and(|target| options.dielectrics.iter().any(|optics| optics.body_index == target.body_index as usize)) {
                return Err(invalid("CPU aging supports opaque surfaces only; explicit certified dielectric targets are refused"));
            }
        }
    }
    Ok(())
}
