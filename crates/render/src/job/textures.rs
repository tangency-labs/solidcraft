//! Immutable request-only planar opaque textures. Codecs run on the image worker.
use super::{CancelToken, ErrorCode, JobError, Result};
use crate::{
    ProductScene,
    surface::{srgb8_to_linear, unit_direction},
    texture_input::{MapRole, TextureSource},
    texture_sampling::TextureView,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solidcraft_geom::Vec3;

pub const MAX_TEXTURE_BINDINGS: usize = 128;
pub const MAX_TEXTURE_PIXELS_BYTES: u64 = 96 * 1024 * 1024;
pub const MAX_TEXTURE_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PLANAR_REPEAT: f64 = 128.0;

fn invalid(message: impl Into<String>) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn v(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
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
}
impl std::fmt::Debug for TextureMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureMap").field("info", self.source.info()).finish()
    }
}
impl TextureMap {
    pub fn new(bytes: Vec<u8>, role: MapRole) -> Result<Self> {
        if !matches!(role, MapRole::BaseColor | MapRole::NormalOpenGl | MapRole::Roughness | MapRole::Metallic) {
            return Err(invalid("only sRGB base color, OpenGL normal, linear roughness and linear metallic maps are supported"));
        }
        let retained_bytes = bytes.capacity();
        let digest = Sha256::digest(&bytes).into();
        let source = TextureSource::new(bytes, role).map_err(|e| invalid(e.to_string()))?;
        if source.info().width > 1024 || source.info().height > 1024 {
            return Err(invalid("runtime texture maps are limited to 1024x1024"));
        }
        Ok(Self { source, retained_bytes, digest })
    }
    /// Content plus interpretation identifies immutable pixels within one request.
    /// No file is reopened after its first capture.
    pub fn identity(&self) -> ([u8; 32], u8) {
        let role = match self.role() {
            MapRole::BaseColor => 0,
            MapRole::NormalOpenGl => 1,
            MapRole::Roughness => 2,
            MapRole::Metallic => 3,
            MapRole::AmbientOcclusion => 4,
        };
        (self.digest, role)
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.source.info().width, self.source.info().height]
    }
    pub fn role(&self) -> MapRole {
        self.source.info().role
    }
    pub fn decode(&self, cancel: &CancelToken) -> Result<Vec<f32>> {
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
        Ok(rgba)
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
}
#[derive(Clone, Debug, Serialize)]
pub struct TextureSummary {
    pub bodies: usize,
    pub encoded_bytes: u64,
    pub linear_pixel_bytes: u64,
}
impl RuntimeTextures {
    pub fn new(mut bindings: Vec<TextureBinding>) -> Result<Self> {
        if bindings.is_empty() || bindings.len() > MAX_TEXTURE_BINDINGS {
            return Err(invalid("runtime textures require 1..128 distinct body bindings"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut source_bytes = 0u64;
        let mut pixel_bytes = 0u64;
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
                if source_bytes > MAX_TEXTURE_SOURCE_BYTES || pixel_bytes > MAX_TEXTURE_PIXELS_BYTES {
                    return Err(invalid("unique texture sources exceed 64 MiB encoded or 96 MiB linear pixels"));
                }
                shared.insert(identity, map.clone());
            }
        }
        if source_bytes > MAX_TEXTURE_SOURCE_BYTES || pixel_bytes > MAX_TEXTURE_PIXELS_BYTES {
            return Err(invalid("unique texture sources exceed 64 MiB encoded or 96 MiB linear pixel budget"));
        }
        Ok(Self { bindings, source_bytes, pixel_bytes })
    }
    pub fn bindings(&self) -> &[TextureBinding] {
        &self.bindings
    }
    pub fn summary(&self) -> TextureSummary {
        TextureSummary { bodies: self.bindings.len(), encoded_bytes: self.source_bytes, linear_pixel_bytes: self.pixel_bytes }
    }
    /// Source snapshots, codec scratch, decoded bytes, staging and device buffer,
    /// plus lookup records. Conservatively includes overlapping preparation.
    pub fn working_bytes(&self) -> u64 {
        self.source_bytes + self.pixel_bytes * 4 + 64 * 1024 * 1024 + 1024 * 1024
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
}
