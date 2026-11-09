//! Immutable request-only planar opaque textures. Codecs run on the image worker.
#[cfg(feature = "raytrace-cpu")]
pub(crate) mod cpu_cache;
#[cfg(feature = "raytrace-cpu")]
pub mod cpu_preparation;

use super::{CancelToken, ErrorCode, JobError, Result};
use crate::{
    ProductScene,
    surface::{srgb8_to_linear, unit_direction},
    texture_input::{MapRole, TextureSource},
    texture_sampling::TextureView,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

pub const MAX_TEXTURE_BINDINGS: usize = 128;
/// Capture is not residency: each producer still prepays its complete working set.
/// Native jobs can retain OPUS's 296 MiB of unique float4 source interpretations.
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
pub const MAX_TEXTURE_PIXELS_BYTES: u64 = 512 * 1024 * 1024;
#[cfg(any(target_arch = "wasm32", not(target_pointer_width = "64")))]
pub const MAX_TEXTURE_PIXELS_BYTES: u64 = 96 * 1024 * 1024;
pub const MAX_TEXTURE_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PLANAR_REPEAT: f64 = 128.0;

fn invalid(message: impl Into<String>) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn v(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// Preserve the tangent direction and make its length meaningful before averaging.
/// Invalid/back-facing samples use the same flat fallback as mapped shading.
pub fn unit_normal_texel(rgb: [f32; 3]) -> [f32; 3] {
    let signed = v(rgb.map(|channel| 2.0 * f64::from(channel) - 1.0));
    if signed.z <= 0.0 {
        return [0.5, 0.5, 1.0];
    }
    let Ok(normal) = unit_direction(signed) else { return [0.5, 0.5, 1.0] };
    [normal.x, normal.y, normal.z].map(|channel| (0.5 * channel + 0.5) as f32)
}

/// For an average of unit normals, E[|n|²] is one. Its loss of squared
/// length measures direction variance without an additional texture plane.
pub fn normal_direction_variance(mean_rgb: [f64; 3]) -> f64 {
    (1.0 - v(mean_rgb.map(|channel| 2.0 * channel - 1.0)).len2()).clamp(0.0, 1.0)
}

/// `u` and `v` span one full repeat in world-space millimetres. Image v increases
/// downward; normal-map green points along the caller's positive v direction.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanarFrame {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}
impl PlanarFrame {
    pub fn validate(self) -> Result<()> {
        let u = v(self.u);
        let vv = v(self.v);
        let ul = u.len();
        let vl = vv.len();
        if self.origin.into_iter().any(|c| !c.is_finite() || c.abs() > 1e9)
            || !u.is_finite()
            || !vv.is_finite()
            || !(0.001..=1e6).contains(&ul)
            || !(0.001..=1e6).contains(&vl)
            || u.dot(vv).abs() > 1e-6 * ul * vl
        {
            return Err(invalid("planar texture frame requires finite origin and orthogonal u/v repeat vectors of 0.001..1000000 mm"));
        }
        Ok(())
    }
    pub fn uv(self, point: Vec3) -> Result<[f64; 2]> {
        self.validate()?;
        let d = point - v(self.origin);
        let u = v(self.u);
        let vv = v(self.v);
        let uv = [d.dot(u) / u.len2(), d.dot(vv) / vv.len2()];
        if uv.iter().any(|c| !c.is_finite() || c.abs() > MAX_PLANAR_REPEAT) {
            return Err(invalid("planar UV exceeds the GPU precision bound of 128 repeats"));
        }
        Ok(uv)
    }
    pub fn mapped_normal(self, normal: Vec3, geometric: Vec3, rgb: [f64; 3], strength: f64) -> Result<Vec3> {
        self.validate()?;
        if !strength.is_finite() || !(0.0..=1.0).contains(&strength) || rgb.iter().any(|c| !c.is_finite() || !(0.0..=1.0).contains(c)) {
            return Err(invalid("invalid tangent normal input"));
        }
        let n = unit_direction(normal).map_err(|e| invalid(e.to_string()))?;
        let g = unit_direction(geometric).map_err(|e| invalid(e.to_string()))?;
        let u = unit_direction(v(self.u)).map_err(|e| invalid(e.to_string()))?;
        let tangent = u - n * n.dot(u);
        if tangent.len2() < 1e-12 {
            return Ok(n);
        }
        let tangent = unit_direction(tangent).map_err(|e| invalid(e.to_string()))?;
        let handedness = if n.dot(v(self.u).cross(v(self.v))) < 0.0 { -1.0 } else { 1.0 };
        let local = Vec3::new((2.0 * rgb[0] - 1.0) * strength, (2.0 * rgb[1] - 1.0) * strength, 2.0 * rgb[2] - 1.0);
        if local.len2() < 1e-12 || local.z <= 0.0 {
            return Ok(n);
        }
        let mapped =
            unit_direction(tangent * local.x + n.cross(tangent) * (handedness * local.y) + n * local.z).map_err(|e| invalid(e.to_string()))?;
        Ok(if mapped.dot(g) <= 0.0 { n } else { mapped })
    }
}

#[derive(Clone)]
pub struct TextureMap {
    source: TextureSource,
    retained_bytes: usize,
    digest: [u8; 32],
    gpu_mip_filtering: bool,
}
impl std::fmt::Debug for TextureMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureMap").field("info", self.source.info()).finish()
    }
}
impl TextureMap {
    /// Keep the established single-level GPU sampler for temporary and built-in maps.
    pub fn new(bytes: Vec<u8>, role: MapRole) -> Result<Self> {
        Self::admit(bytes, role, false)
    }
    /// New design-owned images opt into footprint-filtered GPU mip chains.
    /// Portable metadata validation runs before opening the image codec.
    pub fn new_portable(bytes: Vec<u8>, role: MapRole) -> Result<Self> {
        Self::admit(bytes, role, true)
    }
    fn admit(bytes: Vec<u8>, role: MapRole, gpu_mip_filtering: bool) -> Result<Self> {
        if !matches!(role, MapRole::BaseColor | MapRole::NormalOpenGl | MapRole::Roughness | MapRole::Metallic) {
            return Err(invalid("only sRGB base color, OpenGL normal, linear roughness and linear metallic maps are supported"));
        }
        let retained_bytes = bytes.capacity();
        let digest = Sha256::digest(&bytes).into();
        let source = if gpu_mip_filtering { TextureSource::new_portable(bytes, role) } else { TextureSource::new(bytes, role) }
            .map_err(|e| invalid(e.to_string()))?;
        if source.info().width > 1024 || source.info().height > 1024 {
            return Err(invalid("runtime texture maps are limited to 1024x1024"));
        }
        Ok(Self { source, retained_bytes, digest, gpu_mip_filtering })
    }
    /// Content, role and GPU filtering policy identify immutable maps within one request.
    /// No file is reopened after its first capture.
    pub fn identity(&self) -> ([u8; 32], u8) {
        let role = match self.role() {
            MapRole::BaseColor => 0,
            MapRole::NormalOpenGl => 1,
            MapRole::Roughness => 2,
            MapRole::Metallic => 3,
            MapRole::AmbientOcclusion => 4,
        };
        (self.digest, role | if self.gpu_mip_filtering { 0x80 } else { 0 })
    }
    /// The portable GPU policy adds mip filtering and scalar-face precedence.
    pub fn gpu_mip_filtering(&self) -> bool {
        self.gpu_mip_filtering
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.source.info().width, self.source.info().height]
    }
    #[cfg(any(test, feature = "raytrace-cpu"))]
    pub(crate) fn source(&self) -> &TextureSource {
        &self.source
    }
    pub fn role(&self) -> MapRole {
        self.source.info().role
    }
    pub fn decode(&self, cancel: &CancelToken) -> Result<Vec<f32>> {
        self.decode_with_source(cancel).map(|(_, linear)| linear)
    }
    /// Decode once and retain exact source bytes for compact GPU upload alongside
    /// normalized linear pixels for mip generation.
    pub fn decode_with_source(&self, cancel: &CancelToken) -> Result<(crate::texture_input::DecodedTexture, Vec<f32>)> {
        cancel.check()?;
        let decoded = self.source.decode(|| cancel.is_cancelled()).map_err(|e| {
            if cancel.is_cancelled() { JobError::new(ErrorCode::Cancelled, "texture decoding cancelled") } else { invalid(e.to_string()) }
        })?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(decoded.rgba().len()).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "texture pixel allocation failed"))?;
        for (index, pixel) in decoded.rgba().as_chunks::<4>().0.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            for (channel, value) in pixel.iter().enumerate() {
                let linear = if self.role() == MapRole::BaseColor && channel < 3 { srgb8_to_linear(*value) } else { f64::from(*value) / 255.0 };
                rgba.push(linear as f32);
            }
        }
        Ok((decoded, rgba))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseColorMode {
    /// Legacy maps modulate the captured body reflectance.
    #[default]
    Modulate,
    /// Authored absolute reflectance replaces RGB, never alpha or engineering data.
    Replace,
}

#[derive(Clone, Debug)]
pub struct TextureBinding {
    pub body_index: usize,
    pub body_name: String,
    pub frame: PlanarFrame,
    pub normal_strength: f64,
    pub base_color: Option<TextureMap>,
    pub normal: Option<TextureMap>,
    pub roughness: Option<TextureMap>,
    pub metallic: Option<TextureMap>,
    pub base_color_mode: BaseColorMode,
    /// OpenGL authoring can use +V image-up while legacy external inputs use rows-down.
    pub flip_v: bool,
}
impl TextureBinding {
    pub fn uv(&self, point: Vec3) -> Result<[f64; 2]> {
        let [u, v] = self.frame.uv(point)?;
        Ok([u, if self.flip_v { -v } else { v }])
    }
}
#[derive(Clone, Debug)]
pub struct RuntimeTextures {
    bindings: Vec<TextureBinding>,
    source_bytes: u64,
    pixel_bytes: u64,
    mip_pixel_bytes: u64,
    portable_tail_bytes: u64,
    filtering: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct TextureSummary {
    pub bodies: usize,
    pub encoded_bytes: u64,
    pub linear_pixel_bytes: u64,
    /// Complete float4 pyramid, including base pixels.
    pub mip_pixel_bytes: u64,
    /// GPU pixels for the selected policy, excluding records and upload scratch.
    pub gpu_pixel_bytes: u64,
    pub filtering: bool,
}
impl RuntimeTextures {
    pub fn new(mut bindings: Vec<TextureBinding>) -> Result<Self> {
        if bindings.is_empty() || bindings.len() > MAX_TEXTURE_BINDINGS {
            return Err(invalid("runtime textures require 1..128 distinct body bindings"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut source_bytes = 0u64;
        let mut pixel_bytes = 0u64;
        let mut mip_pixel_bytes = 0u64;
        let mut portable_tail_bytes = 0u64;
        let mut shared = std::collections::BTreeMap::<([u8; 32], u8), TextureMap>::new();
        for binding in &mut bindings {
            binding.frame.validate()?;
            if binding.body_index >= super::MAX_GPU_PATH_BODIES
                || binding.body_name.is_empty()
                || binding.body_name.len() > 1024
                || binding.body_name.chars().any(char::is_control)
                || !seen.insert(binding.body_index)
                || !binding.normal_strength.is_finite()
                || !(0.0..=1.0).contains(&binding.normal_strength)
                || (binding.base_color.is_none() && binding.normal.is_none() && binding.roughness.is_none() && binding.metallic.is_none())
            {
                return Err(invalid("invalid or duplicate runtime texture body binding"));
            }
            if binding.base_color.as_ref().is_some_and(|m| m.role() != MapRole::BaseColor)
                || binding.normal.as_ref().is_some_and(|m| m.role() != MapRole::NormalOpenGl)
                || binding.roughness.as_ref().is_some_and(|m| m.role() != MapRole::Roughness)
                || binding.metallic.as_ref().is_some_and(|m| m.role() != MapRole::Metallic)
            {
                return Err(invalid("texture role does not match its binding"));
            }
            for slot in [&mut binding.base_color, &mut binding.normal, &mut binding.roughness, &mut binding.metallic] {
                let Some(map) = slot else { continue };
                let identity = map.identity();
                if let Some(canonical) = shared.get(&identity) {
                    // Cloning the source shares the immutable encoded Arc, not just its hash.
                    *map = canonical.clone();
                    continue;
                }
                source_bytes = source_bytes.checked_add(map.retained_bytes as u64).ok_or_else(|| invalid("texture source accounting overflow"))?;
                let decoded = map.source.info().rgba_bytes.checked_mul(4).ok_or_else(|| invalid("texture pixel accounting overflow"))?;
                pixel_bytes = pixel_bytes.checked_add(decoded).ok_or_else(|| invalid("texture pixel accounting overflow"))?;
                let [mut width, mut height] = map.dimensions();
                let mip_start = mip_pixel_bytes;
                loop {
                    mip_pixel_bytes = mip_pixel_bytes
                        .checked_add(u64::from(width) * u64::from(height) * 16)
                        .ok_or_else(|| invalid("texture mip accounting overflow"))?;
                    if width == 1 && height == 1 {
                        break;
                    }
                    width = (width / 2).max(1);
                    height = (height / 2).max(1);
                }
                if map.gpu_mip_filtering() {
                    portable_tail_bytes = portable_tail_bytes
                        .checked_add(mip_pixel_bytes - mip_start - decoded)
                        .ok_or_else(|| invalid("portable texture mip accounting overflow"))?;
                }
                if source_bytes > MAX_TEXTURE_SOURCE_BYTES || pixel_bytes > MAX_TEXTURE_PIXELS_BYTES {
                    return Err(invalid(format!(
                        "unique texture sources exceed the host capture limit: {source_bytes}/{} encoded bytes, {pixel_bytes}/{} linear base bytes; select a larger native host or reduce explicitly authored sources",
                        MAX_TEXTURE_SOURCE_BYTES, MAX_TEXTURE_PIXELS_BYTES
                    )));
                }
                shared.insert(identity, map.clone());
            }
        }
        if source_bytes > MAX_TEXTURE_SOURCE_BYTES || pixel_bytes > MAX_TEXTURE_PIXELS_BYTES {
            return Err(invalid("unique texture sources exceed this host capture budget"));
        }
        Ok(Self { bindings, source_bytes, pixel_bytes, mip_pixel_bytes, portable_tail_bytes, filtering: false })
    }
    /// Opt in to anisotropic filtering and compact GPU storage. Otherwise the
    /// producer preserves its existing sampling policy, including portable mips.
    pub fn with_filtering(mut self, enabled: bool) -> Self {
        self.filtering = enabled;
        self
    }
    pub fn filtering(&self) -> bool {
        self.filtering
    }
    pub fn bindings(&self) -> &[TextureBinding] {
        &self.bindings
    }
    pub fn summary(&self) -> TextureSummary {
        TextureSummary {
            bodies: self.bindings.len(),
            encoded_bytes: self.source_bytes,
            linear_pixel_bytes: self.pixel_bytes,
            mip_pixel_bytes: self.mip_pixel_bytes,
            gpu_pixel_bytes: self.gpu_pixel_bytes(),
            filtering: self.filtering,
        }
    }
    fn gpu_pixel_bytes(&self) -> u64 {
        if self.filtering {
            self.pixel_bytes / 4 + (self.mip_pixel_bytes - self.pixel_bytes) / 2
        } else {
            self.pixel_bytes + self.portable_tail_bytes
        }
    }
    /// Actual/conservative captured source, binding, name and control-header
    /// backing. Available before a CPU or registered executor consumes maps.
    pub fn captured_bytes(&self) -> Result<u64> {
        let slots: usize = self
            .bindings
            .iter()
            .map(|binding| {
                [&binding.base_color, &binding.normal, &binding.roughness, &binding.metallic].into_iter().filter(|map| map.is_some()).count()
            })
            .sum();
        (size_of::<Self>() as u64 + 2 * size_of::<usize>() as u64)
            .checked_add(self.bindings.capacity() as u64 * size_of::<TextureBinding>() as u64)
            .and_then(|bytes| bytes.checked_add(self.bindings.iter().map(|b| b.body_name.capacity() as u64).sum::<u64>()))
            .and_then(|bytes| bytes.checked_add(self.source_bytes))
            // Shared encoded allocations are counted once in source_bytes. The
            // small control-header allowance conservatively counts every role.
            .and_then(|bytes| bytes.checked_add(slots as u64 * (size_of::<Vec<u8>>() + 2 * size_of::<usize>()) as u64))
            .ok_or_else(|| invalid("CPU texture source metadata overflow"))
    }
    #[cfg(feature = "raytrace-cpu")]
    pub(crate) fn cpu_source_bytes(&self) -> Result<u64> {
        self.captured_bytes()
    }
    /// Source snapshots, codec scratch, decoded bytes, staging and device buffer,
    /// plus lookup records. Conservatively includes overlapping preparation.
    pub fn working_bytes(&self) -> u64 {
        // Complete float preparation overlaps one packed host buffer, upload staging
        // and device residency. The decoder is sequential, never one per source.
        self.source_bytes + self.mip_pixel_bytes + self.gpu_pixel_bytes() * 3 + 64 * 1024 * 1024 + 8 * 1024 * 1024 + 2 * 1024 * 1024
    }
    pub fn validate_scene(&self, scene: &ProductScene) -> Result<()> {
        for binding in &self.bindings {
            let body = scene
                .meshes
                .get(binding.body_index)
                .filter(|body| body.name == binding.body_name)
                .ok_or_else(|| invalid("texture ordinal and exact body name must match render.scene"))?;
            if body.opacity != 1.0 || body.transmission.is_some() || body.faces.iter().any(|f| f.opacity != 1.0 || f.transmission.is_some()) {
                return Err(invalid("texture maps currently support opaque bodies only; glass and alpha maps are unsupported"));
            }
            let bounds = body.mesh.bounds();
            for x in [bounds.min.x, bounds.max.x] {
                for y in [bounds.min.y, bounds.max.y] {
                    for z in [bounds.min.z, bounds.max.z] {
                        binding.frame.uv(Vec3::new(x, y, z))?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Pure compatibility check before captured image-map bytes enter a CPU job.
/// Opaque direct and path lighting share the same cached mapped closure. Other
/// optical continuations are retained in the request and refused, never ignored.
pub fn validate_cpu_options(options: &super::RaytraceOptions) -> Result<()> {
    options.validate()?;
    if (!options.dielectrics.is_empty() && options.path.is_none())
        || options.anisotropic_faces.iter().any(Option::is_some)
        || options.reflections.as_ref().is_some_and(|r| !r.bodies.is_empty())
        || options.surface_effects.as_ref().is_some_and(|effects| !effects.is_empty())
    {
        return Err(invalid(
            "CPU surface maps support opaque direct or indirect lighting; preserve the maps and use GPU, or explicitly remove incompatible Glass, selected reflections, anisotropy or procedural surface effects",
        ));
    }
    Ok(())
}

/// Approximate a secondary ray's pixel cone by projecting two perpendicular
/// spans onto the receiver plane. Exact primary ray differentials stay separate.
#[cfg(feature = "raytrace-cpu")]
pub(crate) fn secondary_footprint(camera: &crate::Camera, height: u32, distance: f64, direction: Vec3, normal: Vec3) -> Result<Option<[Vec3; 2]>> {
    let pixel = 2.0 * camera.half_height() / f64::from(height);
    let diameter = if camera.fov > 0.0 { pixel * distance / camera.distance } else { pixel };
    if height == 0 || !diameter.is_finite() || diameter < 0.0 {
        return Err(invalid("invalid texture camera footprint"));
    }
    let cosine = normal.dot(direction);
    if cosine.abs() <= 1e-6 {
        return Ok(None);
    }
    let axis = if direction.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let tangent = unit_direction(direction.cross(axis)).map_err(|error| invalid(error.to_string()))?;
    let project = |delta: Vec3| (delta - direction * (normal.dot(delta) / cosine)) * diameter;
    let footprint = [project(tangent), project(direction.cross(tangent))];
    Ok(footprint.iter().all(|d| d.is_finite()).then_some(footprint))
}

/// Independent f64 bilinear reference over the worker's normalized pixels.
pub fn sample_map(rgba: &[f32], dimensions: [u32; 2], uv: [f64; 2]) -> Result<[f64; 3]> {
    TextureView::new(rgba, dimensions).and_then(|v| v.sample_repeat_opaque(uv)).map_err(|e| invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    fn png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
        bytes
    }
    fn frame() -> PlanarFrame {
        PlanarFrame { origin: [0.; 3], u: [2., 0., 0.], v: [0., 4., 0.] }
    }
    fn binding() -> TextureBinding {
        TextureBinding {
            body_index: 0,
            body_name: "opaque".into(),
            frame: frame(),
            normal_strength: 1.,
            base_color: Some(TextureMap::new(png(1, 1, &[128, 64, 255, 0]), MapRole::BaseColor).unwrap()),
            normal: None,
            roughness: None,
            metallic: None,
            base_color_mode: Default::default(),
            flip_v: false,
        }
    }
    #[test]
    fn explicit_color_semantics_and_repeat_filter_ignore_alpha() {
        let color = TextureMap::new(png(2, 1, &[128, 64, 255, 0, 255, 0, 128, 255]), MapRole::BaseColor).unwrap();
        let data = TextureMap::new(png(1, 1, &[128, 64, 255, 0]), MapRole::NormalOpenGl).unwrap();
        let c = color.decode(&CancelToken::default()).unwrap();
        let n = data.decode(&CancelToken::default()).unwrap();
        assert!((f64::from(c[0]) - srgb8_to_linear(128)).abs() < 1e-7);
        assert!((n[0] - 128. / 255.).abs() < 1e-7);
        let a = sample_map(&c, [2, 1], [0.25, 0.5]).unwrap();
        let b = sample_map(&c, [2, 1], [-0.75, 100.5]).unwrap();
        assert_eq!(a, b);
        assert!((a[0] - srgb8_to_linear(128)).abs() < 1e-7);
        let seam = sample_map(&c, [2, 1], [0., 0.]).unwrap();
        assert!((seam[0] - (1. + a[0]) / 2.).abs() < 1e-7);
        let cancel = CancelToken::default();
        cancel.cancel();
        assert_eq!(color.decode(&cancel).unwrap_err().code, ErrorCode::Cancelled);
    }
    #[test]
    fn tangent_frame_preserves_handedness_and_falls_back_safely() {
        let f = frame();
        assert_eq!(f.uv(Vec3::new(-1., 2., 900.)).unwrap(), [-0.5, 0.5]);
        let normal = f.mapped_normal(Vec3::Z, Vec3::Z, [0.5, 1., 1.], 1.).unwrap();
        assert!((normal - Vec3::new(0., 1., 1.).normalized().unwrap()).len() < 1e-12);
        let mirrored = PlanarFrame { v: [0., -4., 0.], ..f }.mapped_normal(Vec3::Z, Vec3::Z, [0.5, 1., 1.], 1.).unwrap();
        assert!((mirrored - Vec3::new(0., -1., 1.).normalized().unwrap()).len() < 1e-12);
        for rgb in [[0.5; 3], [0., 0., 0.], [1., 1., 0.5]] {
            assert_eq!(f.mapped_normal(Vec3::Z, Vec3::Z, rgb, 1.).unwrap(), Vec3::Z);
        }
        assert_eq!(f.mapped_normal(Vec3::X, Vec3::X, [1.; 3], 1.).unwrap(), Vec3::X);
        assert!(PlanarFrame { u: [0.; 3], ..f }.validate().is_err());
        assert!(PlanarFrame { v: [1., 4., 0.], ..f }.validate().is_err());
        assert!(f.uv(Vec3::new(258., 0., 0.)).is_err());
    }
    #[test]
    fn bounded_resources_reject_duplicate_roles_and_encoded_capacity() {
        let b = binding();
        let t = RuntimeTextures::new(vec![b.clone()]).unwrap();
        assert_eq!(t.summary().linear_pixel_bytes, 16);
        assert!(RuntimeTextures::new(vec![b.clone(), b]).is_err());
        let mut wrong = binding();
        wrong.normal = wrong.base_color.take();
        assert!(RuntimeTextures::new(vec![wrong]).is_err());
        let mut bytes = png(1, 1, &[0; 4]);
        bytes.reserve_exact(16 * 1024 * 1024);
        assert!(TextureMap::new(bytes, MapRole::BaseColor).is_err());
        assert!(TextureMap::new(png(1, 1, &[0; 4]), MapRole::AmbientOcclusion).is_err());
    }
    #[test]
    fn scalar_maps_are_linear_data_and_work_without_a_color_map() {
        let rough = TextureMap::new(png(1, 1, &[128, 128, 128, 255]), MapRole::Roughness).unwrap();
        let metal = TextureMap::new(png(1, 1, &[64, 64, 64, 255]), MapRole::Metallic).unwrap();
        let cancel = CancelToken::default();
        assert!((rough.decode(&cancel).unwrap()[0] - 128.0 / 255.0).abs() < 1e-7);
        assert!((metal.decode(&cancel).unwrap()[0] - 64.0 / 255.0).abs() < 1e-7);
        let mut entry = binding();
        entry.base_color = None;
        entry.roughness = Some(rough);
        entry.metallic = Some(metal);
        let resources = RuntimeTextures::new(vec![entry]).unwrap();
        assert_eq!(resources.bindings().len(), 1);
        let mut invalid = resources.bindings()[0].clone();
        invalid.normal = invalid.roughness.take();
        assert!(RuntimeTextures::new(vec![invalid]).is_err());
    }

    #[test]
    fn repeated_bodies_share_actual_immutable_sources_and_unique_pixel_accounting() {
        let a = binding();
        let b = binding();
        assert!(!a.base_color.as_ref().unwrap().source.shares_encoded_storage(&b.base_color.as_ref().unwrap().source));
        let one = RuntimeTextures::new(vec![a.clone()]).unwrap();
        let entries: Vec<_> = (0..128)
            .map(|index| {
                let mut entry = binding();
                entry.body_index = index;
                entry.body_name = format!("Body {index}");
                entry
            })
            .collect();
        let shared = RuntimeTextures::new(entries).unwrap();
        assert_eq!(one.source_bytes, shared.source_bytes);
        assert_eq!(one.pixel_bytes, shared.pixel_bytes);
        let first = shared.bindings()[0].base_color.as_ref().unwrap();
        for entry in shared.bindings() {
            assert!(first.source.shares_encoded_storage(&entry.base_color.as_ref().unwrap().source));
        }
        let mut extra = shared.bindings().to_vec();
        let mut last = b;
        last.body_index = 128;
        last.body_name = "Too many".into();
        extra.push(last);
        assert!(RuntimeTextures::new(extra).is_err());
        let data = TextureMap::new(png(1, 1, &[128, 64, 255, 0]), MapRole::NormalOpenGl).unwrap();
        assert_ne!(first.identity(), data.identity(), "same encoded bytes with different interpretation must not share decoded pixels");
    }

    #[test]
    fn legacy_and_portable_sampling_policies_survive_deduplication_in_both_orders() {
        let bytes = png(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]);
        let legacy = TextureMap::new(bytes.clone(), MapRole::BaseColor).unwrap();
        let portable = TextureMap::new_portable(bytes, MapRole::BaseColor).unwrap();
        assert!(!legacy.gpu_mip_filtering());
        assert!(portable.gpu_mip_filtering());
        assert_ne!(legacy.identity(), portable.identity(), "different filtering policies must not canonicalize to one map");
        assert_eq!(legacy.decode(&CancelToken::default()).unwrap(), portable.decode(&CancelToken::default()).unwrap());
        for maps in [[legacy.clone(), portable.clone()], [portable.clone(), legacy.clone()]] {
            let entries = maps
                .iter()
                .chain(&maps)
                .enumerate()
                .map(|(index, map)| TextureBinding {
                    body_index: index,
                    body_name: format!("Policy {index}"),
                    base_color: Some(map.clone()),
                    ..binding()
                })
                .collect();
            let resources = RuntimeTextures::new(entries).unwrap();
            assert_eq!(resources.summary().linear_pixel_bytes, 2 * 2 * 16, "each policy is budgeted once");
            assert_eq!(resources.summary().encoded_bytes, (legacy.retained_bytes() + portable.retained_bytes()) as u64);
            for (index, map) in maps.iter().enumerate() {
                let a = resources.bindings()[index].base_color.as_ref().unwrap();
                let b = resources.bindings()[index + 2].base_color.as_ref().unwrap();
                assert_eq!(a.identity(), map.identity());
                assert!(a.source.shares_encoded_storage(&b.source), "same-policy duplicates share captured ownership");
            }
            let a = resources.bindings()[0].base_color.as_ref().unwrap();
            let b = resources.bindings()[1].base_color.as_ref().unwrap();
            assert!(!a.source.shares_encoded_storage(&b.source), "mixed policies retain distinct immutable map records");
        }
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
    fn opus_texture_inventory_admits_exact_full_pyramids_without_dropping_sources() {
        let entries = (0_u8..74)
            .map(|index| {
                let mut entry = binding();
                entry.body_index = usize::from(index);
                entry.body_name = format!("OPUS material {index}");
                entry.base_color = None;
                entry.roughness = Some(TextureMap::new(png(512, 512, &[index, 128, 64, 255].repeat(512 * 512)), MapRole::Roughness).unwrap());
                entry
            })
            .collect();
        let captured = RuntimeTextures::new(entries).unwrap().with_filtering(true);
        let summary = captured.summary();
        assert_eq!(summary.bodies, 74);
        assert_eq!(summary.linear_pixel_bytes, 310_378_496);
        assert_eq!(summary.mip_pixel_bytes, 413_837_600);
        assert_eq!(summary.gpu_pixel_bytes, 129_324_176);
        assert!(summary.gpu_pixel_bytes < 128 * 1024 * 1024);
        assert!(summary.filtering);
        assert!(captured.working_bytes() >= summary.mip_pixel_bytes + 3 * summary.gpu_pixel_bytes + summary.encoded_bytes);
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn production_filter_smooths_the_major_axis_and_preserves_minor_axis_detail() {
        let mut entry = binding();
        entry.frame = PlanarFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
        let pixels: Vec<_> =
            (0..64).flat_map(|index| [if index % 2 == 0 { 0 } else { 255 }, if index / 8 % 2 == 0 { 0 } else { 255 }, 0, 255]).collect();
        let source = TextureMap::new(png(8, 8, &pixels), MapRole::BaseColor).unwrap();
        entry.base_color = Some(source.clone());
        let (prepared, _, _) = prepare_cpu_map(source, 3);
        assert_eq!(prepared.sample([0.0625, 0.0625], 0.0).unwrap(), [0.0; 3]);
        for u in [-0.9375, 0.0625, 0.17, 12.0625] {
            let (rgb, _) = prepared.sample_anisotropic([u, 0.0625], [1.0, 0.0], [0.0, 0.03125]).unwrap();
            assert!((rgb[0] - 0.5).abs() < 1e-10, "{rgb:?}");
            assert_eq!(rgb[1], 0.0, "minor-axis texel detail must survive");
        }
        let camera = crate::Camera { fov: std::f64::consts::FRAC_PI_2, distance: 10.0, ..Default::default() };
        let front = secondary_footprint(&camera, 100, 20.0, -Vec3::Z, Vec3::Z).unwrap().unwrap();
        assert!(front.iter().all(|v| (v.len() - 0.4).abs() < 1e-12));
        assert_eq!(front, secondary_footprint(&camera, 100, 20.0, -Vec3::Z, -Vec3::Z).unwrap().unwrap());
        assert!(secondary_footprint(&camera, 100, 20.0, Vec3::X, Vec3::Z).unwrap().is_none());
    }

    #[test]
    fn authored_image_up_coordinates_do_not_invert_the_normal_basis() {
        let mut entry = binding();
        entry.flip_v = true;
        entry.base_color_mode = BaseColorMode::Replace;
        assert_eq!(entry.uv(Vec3::new(1.0, 1.0, 0.0)).unwrap(), [0.5, -0.25]);
        let n = entry.frame.mapped_normal(Vec3::Z, Vec3::Z, [0.5, 0.75, 1.0], 1.0).unwrap();
        assert!(n.y > 0.0, "OpenGL green is still +V in world space; only the lookup row is flipped");
        let encoded = serde_json::to_value(entry.base_color_mode).unwrap();
        assert_eq!(encoded, "replace");
        assert!(serde_json::from_str::<BaseColorMode>("\"unspecified\"").is_err());
    }

    // Independent source-structure cost: one header and each4096-byte CRC
    // block; EXIF adds its header, each IFD entry, and one final transition.
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_metadata_work(map: &TextureMap) -> u64 {
        let bytes = map.source().png_encoded().unwrap();
        let mut at = 8;
        let mut work = 0;
        while at < bytes.len() {
            let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            work += 1 + (length as u64 + 4).div_ceil(4096);
            if &bytes[at + 4..at + 8] == b"eXIf" {
                let data = &bytes[at + 8..at + 8 + length];
                let little = &data[..2] == b"II";
                let offset =
                    if little { u32::from_le_bytes(data[4..8].try_into().unwrap()) } else { u32::from_be_bytes(data[4..8].try_into().unwrap()) }
                        as usize;
                let count = if little {
                    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
                } else {
                    u16::from_be_bytes(data[offset..offset + 2].try_into().unwrap())
                };
                work += u64::from(count) + 2;
            }
            at += length + 12;
        }
        work
    }
    #[cfg(feature = "raytrace-cpu")]
    fn prepare_cpu_map(map: TextureMap, quantum: usize) -> (cpu_preparation::PreparedMap, u64, u64) {
        prepare_cpu_map_with_quality(map, quantum, false)
    }

    #[cfg(feature = "raytrace-cpu")]
    fn prepare_cpu_map_with_quality(map: TextureMap, quantum: usize, quality: bool) -> (cpu_preparation::PreparedMap, u64, u64) {
        let bytes = cpu_preparation::MapTask::required_bytes(&map).unwrap();
        let mut task = cpu_preparation::MapTask::new_with_quality(map, bytes, CancelToken::default(), quality).unwrap();
        assert_eq!(task.working_bytes(), bytes);
        let mut work = 0;
        for _ in 0..100_000 {
            for _ in 0..quantum {
                if task
                    .step(&mut |units| {
                        assert_eq!(units, 1);
                        work += units;
                        Ok(())
                    })
                    .unwrap()
                {
                    return (task.finish().unwrap(), work, bytes);
                }
            }
        }
        panic!("bounded CPU-map preparation did not finish");
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_rows_match_explicit_color_roles_and_opaque_alpha_semantics() {
        // This independent transfer equation is not the implementation's lookup.
        let srgb = |byte: u8| {
            let value = f64::from(byte) / 255.0;
            if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
        };
        let bytes = png(2, 2, &[128, 64, 32, 0, 255, 0, 128, 255, 0, 255, 64, 0, 32, 128, 255, 255]);
        for role in [MapRole::BaseColor, MapRole::NormalOpenGl, MapRole::Roughness, MapRole::Metallic] {
            let map = TextureMap::new(bytes.clone(), role).unwrap();
            let gpu_reference = map.decode(&CancelToken::default()).unwrap();
            let metadata_work = cpu_metadata_work(&map);
            let (prepared, work, admitted) = prepare_cpu_map(map.clone(), 1);
            assert_eq!(prepared.identity().0, map.identity());
            assert_eq!(prepared.identity().1, crate::texture_sampling::mips::MIP_RECIPE_ID);
            assert!(prepared.retained_bytes() <= admitted);
            assert_eq!(prepared.dimensions(0), Some([2, 2]));
            assert_eq!(prepared.dimensions(1), Some([1, 1]));
            assert_eq!(prepared.level_count(), 2);
            assert_eq!(
                work,
                metadata_work + 5 + 2 + 2 + 3,
                "metadata checks add real bounded work; existing pixel/row/transition debit is unchanged"
            );
            for (index, pixel) in [[128_u8, 64, 32], [255, 0, 128], [0, 255, 64], [32, 128, 255]].into_iter().enumerate() {
                let uv = [(index % 2) as f64 / 2.0 + 0.25, (index / 2) as f64 / 2.0 + 0.25];
                let actual = prepared.sample(uv, 0.0).unwrap();
                for channel in 0..3 {
                    let expected = if role == MapRole::BaseColor { srgb(pixel[channel]) } else { f64::from(pixel[channel]) / 255.0 };
                    assert!((actual[channel] - expected).abs() < 1e-7);
                    assert_eq!(
                        actual[channel],
                        f64::from(gpu_reference[index * 4 + channel]),
                        "level-zero source-role semantics match the existing GPU upload reference"
                    );
                }
            }
            // Alpha-zero RGB still contributes to this explicitly opaque sampler.
            let expected: [f64; 3] =
                std::array::from_fn(|channel| gpu_reference.as_chunks::<4>().0.iter().map(|pixel| f64::from(pixel[channel])).sum::<f64>() / 4.0);
            for (actual, expected) in prepared.sample([0.3, -0.7], 1.0).unwrap().into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-7);
            }
            let (other, other_work, _) = prepare_cpu_map(map, 7);
            assert_eq!(work, other_work);
            for lod in [0.0, 0.25, 0.5, 1.0] {
                assert_eq!(prepared.sample([-2.75, 4.25], lod).unwrap(), other.sample([-2.75, 4.25], lod).unwrap());
            }
        }
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_rows_preserve_all_static_eight_bit_png_channel_layouts() {
        for (color, pixels, expected) in [
            (image::ExtendedColorType::L8, vec![64, 192], [[64_u8, 64, 64], [192, 192, 192]]),
            (image::ExtendedColorType::La8, vec![64, 0, 192, 255], [[64, 64, 64], [192, 192, 192]]),
            (image::ExtendedColorType::Rgb8, vec![64, 128, 192, 192, 128, 64], [[64, 128, 192], [192, 128, 64]]),
            (image::ExtendedColorType::Rgba8, vec![64, 128, 192, 0, 192, 128, 64, 255], [[64, 128, 192], [192, 128, 64]]),
        ] {
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes).write_image(&pixels, 1, 2, color).unwrap();
            let map = TextureMap::new(bytes, MapRole::Roughness).unwrap();
            let metadata_work = cpu_metadata_work(&map);
            let (prepared, work, _) = prepare_cpu_map(map, 1);
            assert_eq!(work, metadata_work + 3 + 2 + 2 + 3);
            for (row, expected) in expected.into_iter().enumerate() {
                let actual = prepared.sample([0.5, (row as f64 + 0.5) / 2.0], 0.0).unwrap();
                for channel in 0..3 {
                    assert_eq!(actual[channel], f64::from(f32::from(expected[channel]) / 255.0));
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_core_prepares_existing_rgb_and_gray_builtin_resources_without_a_second_loader() {
        let resources: [(MapRole, &[u8]); 4] = [
            (MapRole::BaseColor, include_bytes!("../../../../assets/render/starter/materials/exports/SURF-001/base_color.png")),
            (MapRole::NormalOpenGl, include_bytes!("../../../../assets/render/starter/materials/exports/SURF-001/normal_open_gl.png")),
            (MapRole::Roughness, include_bytes!("../../../../assets/render/starter/materials/exports/SURF-001/roughness.png")),
            (MapRole::Metallic, include_bytes!("../../../../assets/render/starter/materials/exports/SURF-001/metallic.png")),
        ];
        for (role, encoded) in resources {
            let map = TextureMap::new(encoded.to_vec(), role).unwrap();
            let existing = map.decode(&CancelToken::default()).unwrap();
            let metadata_work = cpu_metadata_work(&map);
            let (prepared, work, admitted) = prepare_cpu_map(map, 7);
            assert_eq!(prepared.dimensions(0), Some([512, 512]));
            assert_eq!(prepared.level_count(), 10);
            assert_eq!(work, metadata_work + 349_525 + 512 + 10 + 3);
            assert!(prepared.retained_bytes() < admitted);
            for [x, y] in [[0, 0], [257, 129], [511, 511]] {
                let actual = prepared.sample([(x as f64 + 0.5) / 512.0, (y as f64 + 0.5) / 512.0], 0.0).unwrap();
                let offset = (y * 512 + x) * 4;
                for channel in 0..3 {
                    assert_eq!(actual[channel], f64::from(existing[offset + channel]));
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_owned_mips_keep_odd_edge_area_and_exact_capacity_accounting() {
        let mut pixels = [0, 0, 0, 0].repeat(5);
        pixels[16] = 255;
        let map = TextureMap::new(png(5, 1, &pixels), MapRole::Roughness).unwrap();
        let metadata_work = cpu_metadata_work(&map);
        let (prepared, work, admitted) = prepare_cpu_map(map, 1);
        assert_eq!(prepared.dimensions(0), Some([5, 1]));
        assert_eq!(prepared.dimensions(1), Some([2, 1]));
        assert_eq!(prepared.dimensions(2), Some([1, 1]));
        assert_eq!(prepared.dimensions(3), None);
        assert_eq!(work, metadata_work + 8 + 1 + 3 + 3);
        assert!(prepared.retained_bytes() < admitted);
        let at = prepared.sample([0.75, 0.5], 1.0).unwrap();
        assert!((at[0] - 0.4).abs() < 1e-7);
        assert_eq!(at[1..], [0.0, 0.0]);
        assert!((prepared.sample([0.0, 0.0], 2.0).unwrap()[0] - 0.2).abs() < 1e-7);
        let a = prepared.sample([0.75, 0.5], 0.0).unwrap()[0];
        let b = prepared.sample([0.75, 0.5], 1.0).unwrap()[0];
        assert!((prepared.sample([0.75, 0.5], 0.5).unwrap()[0] - (a + b) * 0.5).abs() < 1e-15);
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_quality_normal_mips_broaden_only_unresolved_detail() {
        use crate::{clear_coat::ClearCoat, surface::OpaqueSurface, surface_patterns::ShadingSurface};
        let roughness = 0.2_f64;
        let strength = 0.65_f64;
        let original =
            OpaqueSurface::new(Vec3::new(0.8, 0.6, 0.4), 0.2, roughness, 1.7).unwrap().with_clear_coat(ClearCoat::new(0.7, 0.2, 1.4).unwrap());
        let surface = ShadingSurface { surface: original, shading_normal: Vec3::Z };
        let point = cpu_preparation::MapPoint { position: Vec3::new(1.0, 2.0, 0.0), geometric_normal: Vec3::Z, view_direction: Vec3::Z };
        // Opposite X directions have signed components (±205, 1, 205)/255.
        // Their unit mean loses exactly the squared X component.
        let expected_variance = 205_f64.powi(2) / (2.0 * 205_f64.powi(2) + 1.0);
        let expected_roughness = (roughness.powi(4) + strength.powi(2) * expected_variance).powf(0.25);
        for (quality, mixed, normal_strength, lod, expected) in [
            (true, true, strength, 1.0, expected_roughness),
            (true, true, strength, 0.0, roughness),
            (true, false, strength, 1.0, roughness),
            (true, true, 0.0, 1.0, roughness),
            (false, true, strength, 1.0, roughness),
        ] {
            let second = if mixed { 25 } else { 230 };
            let map = TextureMap::new(png(2, 1, &[230, 128, 230, 0, second, 128, 230, 255]), MapRole::NormalOpenGl).unwrap();
            let (prepared, _, _) = prepare_cpu_map_with_quality(map.clone(), 3, quality);
            let mut binding = binding();
            binding.normal = Some(map);
            binding.normal_strength = normal_strength;
            let mapped = prepared.apply(&binding, point, surface, lod).unwrap().surface;
            assert!((mapped.roughness() - expected).abs() < 1e-6);
            assert_eq!(mapped.clear_coat(), original.clear_coat());
            assert_eq!(mapped.base_color(), original.base_color());
            assert_eq!(mapped.metallic(), original.metallic());
            assert_eq!(mapped.dielectric_ior(), original.dielectric_ior());
        }
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_material_roles_preserve_coat_ior_and_apply_normal_frame_and_flip_explicitly() {
        use crate::{clear_coat::ClearCoat, surface::OpaqueSurface, surface_patterns::ShadingSurface};
        let source = OpaqueSurface::new(Vec3::new(0.8, 0.6, 0.4), 0.2, 0.5, 1.7).unwrap().with_clear_coat(ClearCoat::new(0.7, 0.2, 1.4).unwrap());
        let original = ShadingSurface { surface: source, shading_normal: Vec3::Z };
        for role in [MapRole::BaseColor, MapRole::Roughness, MapRole::Metallic, MapRole::NormalOpenGl] {
            let map = TextureMap::new(png(1, 1, &[128, 255, 255, 0]), role).unwrap();
            let (prepared, _, _) = prepare_cpu_map(map.clone(), 3);
            let mut binding = TextureBinding {
                body_index: 0,
                body_name: "opaque".into(),
                frame: frame(),
                normal_strength: 1.0,
                base_color: None,
                roughness: None,
                metallic: None,
                normal: None,
                base_color_mode: BaseColorMode::Replace,
                flip_v: false,
            };
            match role {
                MapRole::BaseColor => binding.base_color = Some(map),
                MapRole::Roughness => binding.roughness = Some(map),
                MapRole::Metallic => binding.metallic = Some(map),
                MapRole::NormalOpenGl => binding.normal = Some(map),
                _ => panic!("unexpected map role in the four-role CPU closure fixture"),
            }
            let changed = prepared
                .apply(
                    &binding,
                    cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                    original,
                    0.0,
                )
                .unwrap();
            assert_eq!(changed.surface.clear_coat(), source.clear_coat());
            assert_eq!(changed.surface.dielectric_ior(), source.dielectric_ior());
            match role {
                MapRole::BaseColor => {
                    let expected = ((128.0 / 255.0 + 0.055) / 1.055_f64).powf(2.4);
                    assert!((changed.surface.base_color().x - expected).abs() < 1e-7);
                    assert_eq!(changed.surface.base_color().y, 1.0);
                    assert_eq!(changed.surface.base_color().z, 1.0);
                    binding.base_color_mode = BaseColorMode::Modulate;
                    let modulated = prepared
                        .apply(
                            &binding,
                            cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                            original,
                            0.0,
                        )
                        .unwrap();
                    assert!((modulated.surface.base_color().x - 0.8 * expected).abs() < 1e-7);
                    assert_eq!(modulated.surface.base_color().y, 0.6);
                }
                MapRole::Roughness => {
                    assert!((changed.surface.roughness() - 128.0 / 255.0).abs() < 1e-7);
                    assert_eq!(changed.surface.metallic(), 0.2);
                }
                MapRole::Metallic => {
                    assert!((changed.surface.metallic() - 128.0 / 255.0).abs() < 1e-7);
                    assert_eq!(changed.surface.roughness(), 0.5);
                }
                MapRole::NormalOpenGl => {
                    let expected = Vec3::new(1.0 / 255.0, 1.0, 1.0).normalized().unwrap();
                    assert!((changed.shading_normal - expected).len() < 1e-7);
                    assert_eq!(changed.surface, source);
                    binding.frame.v = [0.0, -4.0, 0.0];
                    let mirrored = prepared
                        .apply(
                            &binding,
                            cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                            original,
                            0.0,
                        )
                        .unwrap();
                    assert!((mirrored.shading_normal - Vec3::new(expected.x, -expected.y, expected.z)).len() < 1e-7);
                    binding.frame.v = [0.0, 4.0, 0.0];
                    let grazing_view = Vec3::new(0.0, -1.0, 0.01);
                    let fallback = prepared
                        .apply(
                            &binding,
                            cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: grazing_view },
                            original,
                            0.0,
                        )
                        .unwrap();
                    assert_eq!(
                        fallback.shading_normal,
                        Vec3::Z,
                        "a normal map cannot turn the shading normal away from view, matching the GPU guard"
                    );
                    binding.normal_strength = 0.0;
                    assert_eq!(
                        prepared
                            .apply(
                                &binding,
                                cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                                original,
                                0.0
                            )
                            .unwrap()
                            .shading_normal,
                        Vec3::Z
                    );
                }
                _ => panic!("unexpected map role in the four-role CPU closure fixture"),
            }
        }
        let map = TextureMap::new(png(1, 2, &[0, 0, 0, 255, 255, 255, 255, 255]), MapRole::Roughness).unwrap();
        let (prepared, _, _) = prepare_cpu_map(map.clone(), 1);
        let mut binding = binding();
        binding.base_color = None;
        binding.roughness = Some(map);
        let point = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(
            prepared
                .apply(&binding, cpu_preparation::MapPoint { position: point, geometric_normal: Vec3::Z, view_direction: Vec3::Z }, original, 0.0)
                .unwrap()
                .surface
                .roughness(),
            crate::surface::MIN_ROUGHNESS
        );
        binding.flip_v = true;
        assert_eq!(
            prepared
                .apply(&binding, cpu_preparation::MapPoint { position: point, geometric_normal: Vec3::Z, view_direction: Vec3::Z }, original, 0.0)
                .unwrap()
                .surface
                .roughness(),
            1.0
        );
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_admission_cancel_and_work_exhaustion_never_publish_partial_levels() {
        let map = TextureMap::new(png(5, 3, &[128, 64, 32, 255].repeat(15)), MapRole::BaseColor).unwrap();
        let required = cpu_preparation::MapTask::required_bytes(&map).unwrap();
        assert_eq!(cpu_preparation::MapTask::new(map.clone(), required - 1, CancelToken::default()).err().unwrap().code, ErrorCode::LimitExceeded);
        assert!(cpu_preparation::MapTask::new(map.clone(), u64::MAX, CancelToken::default()).is_err());
        for phase in ["metadata_header", "metadata_bytes", "begin", "row", "linear", "finish_png", "new_level", "mip", "done"] {
            let cancel = CancelToken::default();
            let mut task = cpu_preparation::MapTask::new(map.clone(), required, cancel.clone()).unwrap();
            for _ in 0..1000 {
                if task.phase_name() == phase {
                    break;
                }
                task.step(&mut |_| Ok(())).unwrap();
            }
            assert_eq!(task.phase_name(), phase);
            let before = task.completed_rows();
            let mut charged = 0;
            if phase != "done" {
                assert_eq!(
                    task.step(&mut |_| {
                        charged += 1;
                        Err(JobError::new(ErrorCode::LimitExceeded, "test cumulative ceiling"))
                    })
                    .unwrap_err()
                    .code,
                    ErrorCode::LimitExceeded
                );
                assert_eq!(charged, 1);
                assert_eq!(task.phase_name(), phase);
                assert_eq!(task.completed_rows(), before);
            }
            cancel.cancel();
            assert_eq!(task.step(&mut |_| panic!("cancellation precedes the next debit")).unwrap_err().code, ErrorCode::Cancelled);
            assert_eq!(task.finish().err().unwrap().code, ErrorCode::Cancelled);
        }
    }

    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_core_refuses_unsupported_format_and_hostile_lod_source_or_frame() {
        let image = image::RgbImage::from_pixel(2, 2, image::Rgb([64, 128, 192]));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        let jpeg = TextureMap::new(out.into_inner(), MapRole::BaseColor).unwrap();
        assert!(cpu_preparation::MapTask::required_bytes(&jpeg).is_err());
        let map = TextureMap::new(png(1, 1, &[128, 128, 255, 255]), MapRole::NormalOpenGl).unwrap();
        let (prepared, _, _) = prepare_cpu_map(map.clone(), 1);
        for lod in [-1.0, 0.1, f64::NAN, f64::INFINITY] {
            assert!(prepared.sample([0.0, 0.0], lod).is_err());
        }
        for uv in [[f64::NAN, 0.0], [0.0, f64::INFINITY], [1_048_577.0, 0.0]] {
            assert!(prepared.sample(uv, 0.0).is_err());
        }
        let mut binding = binding();
        binding.normal = Some(map);
        let original = crate::surface_patterns::ShadingSurface {
            surface: crate::surface::OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 0.5, 1.5).unwrap(),
            shading_normal: Vec3::Z,
        };
        binding.normal_strength = f64::NAN;
        assert!(
            prepared
                .apply(
                    &binding,
                    cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                    original,
                    0.0
                )
                .is_err()
        );
        binding.normal_strength = 1.0;
        binding.frame.u = [0.0; 3];
        assert!(
            prepared
                .apply(
                    &binding,
                    cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                    original,
                    0.0
                )
                .is_err()
        );
        binding.frame = frame();
        binding.normal = Some(TextureMap::new(png(1, 1, &[0, 128, 255, 255]), MapRole::NormalOpenGl).unwrap());
        assert!(
            prepared
                .apply(
                    &binding,
                    cpu_preparation::MapPoint { position: Vec3::ZERO, geometric_normal: Vec3::Z, view_direction: Vec3::Z },
                    original,
                    0.0
                )
                .is_err()
        );
    }
    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_corrupt_admitted_pixels_fail_before_a_prepared_map_can_escape() {
        let mut bytes = png(3, 2, &[64, 128, 192, 255].repeat(6));
        let idat = bytes.windows(4).position(|chunk| chunk == b"IDAT").unwrap();
        bytes[idat + 4] ^= 0xff;
        // Source/header admission is intentionally distinct from actual decoding.
        let map = TextureMap::new(bytes, MapRole::BaseColor).unwrap();
        let required = cpu_preparation::MapTask::required_bytes(&map).unwrap();
        let mut task = cpu_preparation::MapTask::new(map, required, CancelToken::default()).unwrap();
        let mut failed = false;
        for _ in 0..1000 {
            match task.step(&mut |_| Ok(())) {
                Err(error) => {
                    assert_eq!(error.code, ErrorCode::InvalidRequest);
                    failed = true;
                    break;
                }
                Ok(done) => assert!(!done, "a corrupt PNG cannot complete map preparation"),
            }
        }
        assert!(failed);
        assert!(task.finish().is_err());
    }
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_metadata_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(kind);
        out.extend(data);
        let mut crc = u32::MAX;
        for byte in kind.iter().chain(data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 0 { crc >> 1 } else { (crc >> 1) ^ 0xedb88320 };
            }
        }
        out.extend((!crc).to_be_bytes());
        out
    }
    #[cfg(feature = "raytrace-cpu")]
    #[test]
    fn cpu_color_declarations_are_checked_cooperatively_after_legacy_capture() {
        let chromaticities =
            [31_270_u32, 32_900, 64_000, 33_000, 30_000, 50_000, 15_000, 6_000].into_iter().flat_map(u32::to_be_bytes).collect::<Vec<_>>();
        let plain = png(2, 2, &[128, 64, 32, 255].repeat(4));
        for (kind, data) in [(b"gAMA", 100_000_u32.to_be_bytes().to_vec()), (b"cHRM", chromaticities)] {
            let mut bytes = plain.clone();
            bytes.splice(33..33, cpu_metadata_chunk(kind, &data));
            let map = TextureMap::new(bytes.clone(), MapRole::BaseColor).unwrap();
            let required = cpu_preparation::MapTask::required_bytes(&map).unwrap();
            let mut task = cpu_preparation::MapTask::new(map, required, CancelToken::default()).unwrap();
            assert_eq!(task.phase_name(), "metadata_header");
            let mut charged = 0;
            let error = (0..100)
                .find_map(|_| {
                    task.step(&mut |units| {
                        assert_eq!(units, 1);
                        charged += units;
                        Ok(())
                    })
                    .err()
                })
                .unwrap();
            assert!(error.message.contains("normalized sRGB"));
            assert!(charged > 1, "strict metadata runs in charged continuations, not the constructor");
            assert_eq!(task.completed_rows(), 0, "conflicting color cannot reach pixel decoding");
            assert!(task.finish().is_err());
            for role in [MapRole::NormalOpenGl, MapRole::Roughness, MapRole::Metallic] {
                let (prepared, _, _) = prepare_cpu_map(TextureMap::new(bytes.clone(), role).unwrap(), 1);
                let (baseline, _, _) = prepare_cpu_map(TextureMap::new(plain.clone(), role).unwrap(), 1);
                for lod in [0.0, 0.5, 1.0] {
                    assert_eq!(prepared.sample([-0.25, 0.75], lod).unwrap(), baseline.sample([-0.25, 0.75], lod).unwrap());
                }
            }
        }
    }
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_with_metadata() -> TextureMap {
        let mut exif = b"II".to_vec();
        exif.extend(42_u16.to_le_bytes());
        exif.extend(8_u32.to_le_bytes());
        exif.extend(32_u16.to_le_bytes());
        for i in 0..32_u16 {
            exif.extend((if i == 0 { 0x112_u16 } else { 0xc000 + i }).to_le_bytes());
            exif.extend(3_u16.to_le_bytes());
            exif.extend(1_u32.to_le_bytes());
            exif.extend(1_u16.to_le_bytes());
            exif.extend(0_u16.to_le_bytes());
        }
        exif.extend(0_u32.to_le_bytes());
        let mut bytes = png(2, 2, &[128, 64, 32, 255].repeat(4));
        bytes.splice(33..33, cpu_metadata_chunk(b"eXIf", &exif));
        let end = bytes.len() - 12;
        bytes.splice(end..end, cpu_metadata_chunk(b"vpAg", &[17; 12_288]));
        TextureMap::new(bytes, MapRole::BaseColor).unwrap()
    }
    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_metadata_prepass_is_charged_cancellable_and_does_not_rewrite_source_or_pixels() {
        for phase in ["metadata_header", "metadata_bytes", "metadata_exif_header", "metadata_ifd"] {
            let map = cpu_map_with_metadata();
            let weak = map.source().encoded_weak();
            let required = cpu_preparation::MapTask::required_bytes(&map).unwrap();
            let cancel = CancelToken::default();
            let mut task = cpu_preparation::MapTask::new(map, required, cancel.clone()).unwrap();
            for _ in 0..1000 {
                if task.phase_name() == phase {
                    break;
                }
                task.step(&mut |units| {
                    assert_eq!(units, 1);
                    Ok(())
                })
                .unwrap();
            }
            assert_eq!(task.phase_name(), phase);
            assert_eq!(task.completed_rows(), 0, "all original metadata is checked before opening a codec");
            let mut charged = 0;
            assert_eq!(
                task.step(&mut |units| {
                    charged += units;
                    Err(JobError::new(ErrorCode::LimitExceeded, "metadata cumulative ceiling"))
                })
                .unwrap_err()
                .code,
                ErrorCode::LimitExceeded
            );
            assert_eq!(charged, 1);
            assert_eq!(task.phase_name(), phase);
            assert_eq!(task.completed_rows(), 0);
            cancel.cancel();
            assert_eq!(task.step(&mut |_| panic!("cancel precedes debit")).unwrap_err().code, ErrorCode::Cancelled);
            assert_eq!(task.finish().err().unwrap().code, ErrorCode::Cancelled);
            assert!(weak.upgrade().is_none(), "cancelled metadata work drops its only source ownership");
        }
        let map = cpu_map_with_metadata();
        let captured = map.source().png_encoded().unwrap();
        let digest = map.identity();
        let metadata_work = cpu_metadata_work(&map);
        let (prepared, work, _) = prepare_cpu_map(map.clone(), 1);
        assert_eq!(work, metadata_work + 5 + 2 + 2 + 3);
        assert_eq!(prepared.identity().0, digest);
        assert!(std::sync::Arc::ptr_eq(&captured, &map.source().png_encoded().unwrap()));
        let baseline = TextureMap::new(png(2, 2, &[128, 64, 32, 255].repeat(4)), MapRole::BaseColor).unwrap();
        let (baseline, _, _) = prepare_cpu_map(baseline, 7);
        for lod in [0.0, 0.25, 1.0] {
            assert_eq!(prepared.sample([-0.25, 0.75], lod).unwrap(), baseline.sample([-0.25, 0.75], lod).unwrap());
        }
    }
    #[test]
    #[cfg(feature = "raytrace-cpu")]
    fn cpu_map_prepass_validates_the_original_tail_crc_before_any_pixel_can_escape() {
        let mut bytes = png(1, 1, &[128, 64, 32, 255]);
        let mut tail = cpu_metadata_chunk(b"vpAg", &[17; 12_288]);
        let last = tail.len() - 1;
        tail[last] ^= 1;
        let end = bytes.len() - 12;
        bytes.splice(end..end, tail);
        let map = TextureMap::new(bytes, MapRole::Roughness).unwrap(); // Header admission remains separate from full CRC validation.
        let required = cpu_preparation::MapTask::required_bytes(&map).unwrap();
        let mut task = cpu_preparation::MapTask::new(map, required, CancelToken::default()).unwrap();
        let error = (0..1000).find_map(|_| task.step(&mut |_| Ok(())).err()).unwrap();
        assert!(error.message.contains("CRC"));
        assert_eq!(task.completed_rows(), 0);
        assert!(task.finish().is_err());
    }
}
