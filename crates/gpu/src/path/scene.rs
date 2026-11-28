//! Sixteen-bin SAH BVH and immutable scene upload packing. World geometry is rebased around the camera target for f32.
#[path = "planar_patches.rs"]
mod planar_patches;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use solidcraft_geom::prelude::*;
use std::collections::HashMap;

use solidcore_record::Vec3;
use solidcraft_render::{
    ProductScene,
    job::{CancelToken, ErrorCode, JobError, Result},
    transmission::Transmission,
};
use solidcraft_viewport::surface::{OpaqueSurface, srgb8_to_linear};

pub(super) struct Triangle {
    pub p: [Vec3; 3],
    pub indices: [u32; 3],
    pub surface: OpaqueSurface,
    pub transmission: Option<Transmission>,
    pub medium: u32,
    pub scalar_face: bool,
    /// The paint flake layer's table entry + 1, zero without flakes.
    pub flakes: u32,
}
type MediumEdge = (usize, usize, i32);
// The material header word reserves the high two bits for model lanes and wavelength sampling.
pub(super) const MATERIAL_OFFSET_MASK: u32 = 0x3fff_ffff;

pub(super) fn model_bytes_per_triangle(scene: &ProductScene) -> u64 {
    let modeled = |t: Option<Transmission>| t.is_some_and(|t| t.model().is_some());
    if scene.meshes.iter().any(|m| modeled(m.transmission) || m.faces.iter().any(|f| modeled(f.transmission))) { 16 } else { 0 }
}
pub(super) fn medium_preparation_bytes(scene: &ProductScene) -> u64 {
    // Validation consumes one body's scratch at a time. Opaque/thin-sheet bodies
    // allocate none of the weld table: do not charge their triangles for it.
    let closed = |t: Option<Transmission>| t.is_some_and(|v| v.thin_wall_thickness_mm().is_none());
    let mut total = 0u64;
    let mut largest_medium = 0u64;
    for body in &scene.meshes {
        let triangles = body.mesh.triangles.len() as u64;
        total = total.saturating_add(triangles);
        if closed(body.transmission) || body.faces.iter().any(|face| closed(face.transmission)) {
            largest_medium = largest_medium.max(triangles);
        }
    }
    // Three representatives per triangle at most. The 320 includes positions,
    // full eight-slot hash entries, control bytes and worst-case table rounding.
    let scratch = 3 * (std::mem::size_of::<MediumEdge>() + 320) + std::mem::size_of::<usize>();
    total.saturating_mul(std::mem::size_of::<Triangle>() as u64).saturating_add(largest_medium.saturating_mul(scratch as u64))
}

pub(super) fn planar_preparation_bytes_per_triangle() -> usize {
    let common = std::mem::size_of::<Triangle>() + 2 * std::mem::size_of::<Node>();
    (common + planar_patches::temporary_bytes_per_triangle()).max(common + 100 + 80 + 4)
}

impl Triangle {
    fn centre(&self, axis: usize) -> f64 {
        self.p.iter().map(|p| [p.x, p.y, p.z].get(axis).copied().unwrap_or(0.0)).sum::<f64>() / 3.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    first: u32,
    count: u32,
}

/// Distinct paint flake layers one job can shade; the shader's parameter table holds this many.
pub(super) const MAX_FLAKE_LAYERS: usize = 64;

/// Bytes of the shader's paint flake table: four vec4s per layer.
pub(super) const FLAKE_PARAMETER_BYTES: usize = MAX_FLAKE_LAYERS * 64;

/// The shader's paint flake table. Positions reach the shader relative to `origin`, so each
/// layer also carries `origin` in its own cells: the whole cells modulo the pattern period
/// and the fraction, so the GPU finds the same world cells as the CPU.
pub(super) fn flake_parameters(layers: &[solidcraft_viewport::flakes::FlakeLayer], origin: Vec3) -> Result<Vec<u8>> {
    use solidcraft_viewport::flakes::FLAKE_PERIOD;
    if layers.len() > MAX_FLAKE_LAYERS {
        return Err(invalid("at most 64 distinct paint flake layers are supported"));
    }
    let mut bytes = reserved(FLAKE_PARAMETER_BYTES)?;
    for layer in layers {
        let q = origin / layer.size_mm();
        if !q.is_finite() || q.x.abs().max(q.y.abs()).max(q.z.abs()) > 1.0e12 {
            return Err(invalid("the camera target is beyond the paint flake pattern's reach"));
        }
        let whole = Vec3::new(q.x.floor(), q.y.floor(), q.z.floor());
        let fraction = q - whole;
        vector(&mut bytes, Vec3::new(layer.coverage(), 1.0 / layer.size_mm(), layer.spread()), layer.depth() as f32);
        vector(&mut bytes, layer.color(), layer.metallic() as f32);
        vector(&mut bytes, Vec3::new(layer.roughness(), fraction.x, fraction.y), fraction.z as f32);
        for value in [whole.x, whole.y, whole.z] {
            bytes.extend_from_slice(&((value as i64).rem_euclid(FLAKE_PERIOD) as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    bytes.resize(FLAKE_PARAMETER_BYTES, 0);
    Ok(bytes)
}

const HASH_CHUNK_ITEMS: usize = 8192;

/// Exact immutable preparation identity for GPU geometry, BVH and optical certificates.
///
/// Keyed on every captured mesh/appearance/optical field read by [`flatten_with_media`],
/// the exact `f64` rebase origin (`camera.target`), and `nested_media`, so a cache hit
/// is guaranteed bit-for-bit identical to a fresh build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PreparedKey {
    scene_hash: [u8; 32],
    origin_bits: [u64; 3],
    nested_media: bool,
}

fn hash_vec3(hasher: &mut Sha256, v: Vec3) {
    hasher.update(v.x.to_bits().to_le_bytes());
    hasher.update(v.y.to_bits().to_le_bytes());
    hasher.update(v.z.to_bits().to_le_bytes());
}

fn hash_option_f64(hasher: &mut Sha256, value: Option<f64>) {
    match value {
        None => hasher.update([0u8]),
        Some(v) => {
            hasher.update([1u8]);
            hasher.update(v.to_bits().to_le_bytes());
        }
    }
}

fn hash_option_surface(hasher: &mut Sha256, surface: Option<OpaqueSurface>) {
    match surface {
        None => hasher.update([0u8]),
        Some(s) => {
            hasher.update([1u8]);
            hash_vec3(hasher, s.base_color());
            hasher.update(s.metallic().to_bits().to_le_bytes());
            hasher.update(s.roughness().to_bits().to_le_bytes());
            hasher.update(s.dielectric_ior().to_bits().to_le_bytes());
            hasher.update([u8::from(s.metal_energy_compensation())]);
            match s.clear_coat() {
                None => hasher.update([0u8]),
                Some(coat) => {
                    hasher.update([1u8]);
                    hasher.update(coat.weight().to_bits().to_le_bytes());
                    hasher.update(coat.roughness().to_bits().to_le_bytes());
                    hasher.update(coat.ior().to_bits().to_le_bytes());
                }
            }
            match s.flakes() {
                None => hasher.update([0u8]),
                Some(flake) => {
                    hasher.update([1u8]);
                    hash_vec3(hasher, flake.color());
                    for value in [flake.coverage(), flake.size_mm(), flake.spread(), flake.depth(), flake.metallic(), flake.roughness()] {
                        hasher.update(value.to_bits().to_le_bytes());
                    }
                }
            }
        }
    }
}

fn hash_option_transmission(hasher: &mut Sha256, transmission: Option<Transmission>) {
    match transmission {
        None => hasher.update([0u8]),
        Some(t) => {
            hasher.update([1u8]);
            hash_vec3(hasher, t.extinction());
            hash_option_f64(hasher, t.thin_wall_thickness_mm());
            hasher.update([u8::from(t.shadow_transmission())]);
            match t.model() {
                None => hasher.update([0u8]),
                Some(model) => {
                    hasher.update([1u8]);
                    hasher.update(model.roughness.to_bits().to_le_bytes());
                    match model.dispersion {
                        None => hasher.update([0u8]),
                        Some(d) => {
                            hasher.update([1u8]);
                            hasher.update(d.abbe_number.to_bits().to_le_bytes());
                        }
                    }
                    match model.thin_film {
                        None => hasher.update([0u8]),
                        Some(f) => {
                            hasher.update([1u8]);
                            hasher.update(f.ior.to_bits().to_le_bytes());
                            hasher.update(f.thickness_nm.to_bits().to_le_bytes());
                        }
                    }
                }
            }
            match t.subsurface() {
                None => hasher.update([0u8]),
                Some(s) => {
                    hasher.update([1u8]);
                    hasher.update(s.scattering_per_mm.to_bits().to_le_bytes());
                    hasher.update(s.anisotropy.to_bits().to_le_bytes());
                }
            }
        }
    }
}

fn hash_vec3_chunk(chunk: &[Vec3]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 24 * 64];
    for group in chunk.chunks(64) {
        let bytes = group.len() * 24;
        for (slot, v) in buf.as_chunks_mut::<24>().0.iter_mut().zip(group) {
            slot[0..8].copy_from_slice(&v.x.to_bits().to_le_bytes());
            slot[8..16].copy_from_slice(&v.y.to_bits().to_le_bytes());
            slot[16..24].copy_from_slice(&v.z.to_bits().to_le_bytes());
        }
        if let Some(slice) = buf.get(..bytes) {
            hasher.update(slice);
        }
    }
    hasher.finalize().into()
}

fn hash_tri_chunk(chunk: &[[u32; 3]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 12 * 128];
    for group in chunk.chunks(128) {
        let bytes = group.len() * 12;
        for (slot, t) in buf.as_chunks_mut::<12>().0.iter_mut().zip(group) {
            slot[0..4].copy_from_slice(&t[0].to_le_bytes());
            slot[4..8].copy_from_slice(&t[1].to_le_bytes());
            slot[8..12].copy_from_slice(&t[2].to_le_bytes());
        }
        if let Some(slice) = buf.get(..bytes) {
            hasher.update(slice);
        }
    }
    hasher.finalize().into()
}

fn hash_u32_chunk(chunk: &[u32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 4 * 256];
    for group in chunk.chunks(256) {
        let bytes = group.len() * 4;
        for (slot, v) in buf.as_chunks_mut::<4>().0.iter_mut().zip(group) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
        if let Some(slice) = buf.get(..bytes) {
            hasher.update(slice);
        }
    }
    hasher.finalize().into()
}

fn hash_slice_parallel<T: Sync>(hasher: &mut Sha256, slice: &[T], cancel: &CancelToken, chunk_hash: fn(&[T]) -> [u8; 32]) -> Result<()> {
    cancel.check()?;
    hasher.update((slice.len() as u64).to_le_bytes());
    if slice.len() <= HASH_CHUNK_ITEMS {
        hasher.update(chunk_hash(slice));
        return Ok(());
    }
    let digests: Vec<[u8; 32]> = slice
        .par_chunks(HASH_CHUNK_ITEMS)
        .map(|chunk| {
            cancel.check()?;
            Ok(chunk_hash(chunk))
        })
        .collect::<Result<Vec<_>>>()?;
    for digest in digests {
        hasher.update(digest);
    }
    Ok(())
}

impl PreparedKey {
    pub(super) fn from_scene(scene: &ProductScene, origin: Vec3, nested_media: bool, cancel: &CancelToken) -> Result<Self> {
        cancel.check()?;
        let mut hasher = Sha256::new();
        hasher.update((scene.meshes.len() as u64).to_le_bytes());
        for body in &scene.meshes {
            cancel.check()?;
            hasher.update((body.name.len() as u64).to_le_bytes());
            hasher.update(body.name.as_bytes());
            hasher.update(body.color);
            hasher.update(body.opacity.to_bits().to_le_bytes());
            hash_option_surface(&mut hasher, body.surface);
            hash_option_transmission(&mut hasher, body.transmission);
            hasher.update((body.faces.len() as u64).to_le_bytes());
            for (index, face) in body.faces.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    cancel.check()?;
                }
                hasher.update(face.face.to_le_bytes());
                hasher.update(face.color);
                hasher.update(face.opacity.to_bits().to_le_bytes());
                hash_option_surface(&mut hasher, face.surface);
                hash_option_transmission(&mut hasher, face.transmission);
            }
            let mesh = &body.mesh;
            hash_slice_parallel(&mut hasher, &mesh.positions, cancel, hash_vec3_chunk)?;
            hash_slice_parallel(&mut hasher, &mesh.normals, cancel, hash_vec3_chunk)?;
            hash_slice_parallel(&mut hasher, &mesh.triangles, cancel, hash_tri_chunk)?;
            hash_slice_parallel(&mut hasher, &mesh.tri_face, cancel, hash_u32_chunk)?;
        }
        Ok(Self { scene_hash: hasher.finalize().into(), origin_bits: [origin.x.to_bits(), origin.y.to_bits(), origin.z.to_bits()], nested_media })
    }
}

/// Bounded single-entry cache for one immutable prepared scene.
///
/// Eviction returns the displaced value so callers can drop device buffers
/// outside any mutex guard.
pub(super) struct PreparedSceneCache<T> {
    entry: Option<(PreparedKey, T)>,
}

impl<T> Default for PreparedSceneCache<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}

impl<T> PreparedSceneCache<T> {
    /// Non-mutating lookup for read-only preflight validation.
    pub(super) fn peek(&self, key: &PreparedKey) -> Option<&T> {
        self.entry.as_ref().filter(|(cached, _)| cached == key).map(|(_, value)| value)
    }

    /// Look up for execution: clones on hit, or detaches any mismatched entry
    /// so the caller can drop its resources before preparing the next scene.
    pub(super) fn lookup_or_take_mismatch(&mut self, key: &PreparedKey) -> (Option<T>, Option<T>)
    where
        T: Clone,
    {
        if let Some((_, value)) = self.entry.as_ref().filter(|(cached, _)| cached == key) {
            return (Some(value.clone()), None);
        }
        (None, self.take())
    }

    /// Replace the single cached slot, returning any displaced entry for drop outside the lock.
    pub(super) fn replace(&mut self, key: PreparedKey, value: T) -> Option<T> {
        self.entry.replace((key, value)).map(|(_, previous)| previous)
    }

    /// Evict the cached entry, returning it for drop outside the lock.
    pub(super) fn take(&mut self) -> Option<T> {
        self.entry.take().map(|(_, previous)| previous)
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entry.is_none()
    }
}

pub(super) struct Upload {
    pub geometry: Vec<u8>,
    /// Distinct paint flake layers, indexed by each triangle's `flakes` entry - 1.
    pub flakes: Vec<solidcraft_viewport::flakes::FlakeLayer>,
    pub nodes: Vec<u8>,
    pub epsilon: f32,
    pub active_glass_model: bool,
    pub photon_guide: Option<[f32; 4]>,
    pub photon_bounds: Option<[f32; 6]>,
}

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
pub(super) fn reserved<T>(count: usize) -> Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "path tracer allocation failed"))?;
    Ok(v)
}

#[cfg(test)]
pub(super) fn flatten(scene: &ProductScene, origin: Vec3, cancel: &CancelToken) -> Result<Upload> {
    flatten_with_media(scene, origin, cancel, false)
}
pub(super) fn flatten_with_media(scene: &ProductScene, origin: Vec3, cancel: &CancelToken, nested: bool) -> Result<Upload> {
    let model_bytes = model_bytes_per_triangle(scene) as usize;
    let material_bytes = 80 + model_bytes;
    let mut material_flags = if model_bytes > 0 { 0x80000000 } else { 0 };
    let mut active_glass_model = false;
    let transmissions =
        scene.meshes.iter().flat_map(|body| std::iter::once(body.transmission).chain(body.faces.iter().map(|face| face.transmission)));
    for model in transmissions.filter_map(|transmission| transmission.and_then(Transmission::model)) {
        active_glass_model |= !model.is_identity();
        if model.spectral() {
            material_flags |= 0x40000000;
            break;
        }
    }
    let count = scene.meshes.iter().map(|m| m.mesh.triangles.len()).sum::<usize>();
    let mut triangles = reserved(count)?;
    let vertex_count = scene.meshes.iter().map(|m| m.mesh.positions.len()).sum::<usize>();
    let mut vertices = reserved(vertex_count * 32)?;
    let mut vertex_offset = 0u32;
    let mut media_bounds = reserved::<super::media_hierarchy::Region>(solidcraft_render::job::MAX_GPU_DIELECTRICS)?;
    let mut optical_lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut optical_hi = -optical_lo;
    let mut flake_layers = Vec::new();
    for (body_index, body) in scene.meshes.iter().enumerate() {
        cancel.check()?;
        let body_start = triangles.len();
        let mut authored_optical = false;
        let mut faces = reserved(body.faces.len())?;
        faces.extend(body.faces.iter());
        faces.sort_unstable_by_key(|f| f.face);
        if faces.windows(2).any(|f| f[0].face == f[1].face) {
            return Err(invalid("duplicate face appearance"));
        }
        let mesh = &body.mesh;
        if mesh.normals.len() != mesh.positions.len() || mesh.tri_face.len() != mesh.triangles.len() {
            return Err(invalid("path tracer requires normals and a face ID for every triangle"));
        }
        for (index, (position, normal)) in mesh.positions.iter().zip(&mesh.normals).enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            let position = *position - origin;
            if !position.is_finite() || [position.x, position.y, position.z].iter().any(|x| x.abs() > 1e7) {
                return Err(invalid("path geometry must be finite and within 1e7 mm of the camera target"));
            }
            let normal = solidcraft_viewport::surface::unit_direction(*normal).map_err(|_| invalid("invalid mesh normal"))?;
            vector(&mut vertices, position, 0.0);
            vector(&mut vertices, normal, 0.0);
        }
        for (index, indices) in mesh.triangles.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            let face = mesh.tri_face.get(index).copied().ok_or_else(|| invalid("missing triangle face"))?;
            let look = faces.binary_search_by_key(&face, |f| f.face).ok().and_then(|i| faces.get(i));
            let opacity = look.map_or(body.opacity, |f| f.opacity);
            let transmission = look.map_or(body.transmission, |f| f.transmission);
            authored_optical |= transmission.is_some();
            if opacity != 1.0 && transmission.is_none() {
                return Err(invalid("gpu-path requires explicit transmission for nonopaque preview appearances"));
            }
            let color = look.map_or(body.color, |f| f.color);
            let surface = look
                .map_or(body.surface, |f| f.surface)
                .map(Ok)
                .unwrap_or_else(|| {
                    OpaqueSurface::new(Vec3::new(srgb8_to_linear(color[0]), srgb8_to_linear(color[1]), srgb8_to_linear(color[2])), 0.0, 0.5, 1.5)
                })
                .map_err(|e| invalid(&e.to_string()))?;
            let mut p = [Vec3::ZERO; 3];
            let mut global_indices = [0u32; 3];
            for (corner, &vertex) in indices.iter().enumerate() {
                let position = *mesh.positions.get(vertex as usize).ok_or_else(|| invalid("triangle index out of bounds"))? - origin;
                if !position.is_finite() || [position.x, position.y, position.z].iter().any(|x| x.abs() > 1e7) {
                    return Err(invalid("path geometry must be finite and within 1e7 mm of the camera target"));
                }
                if let Some(v) = p.get_mut(corner) {
                    *v = position;
                }
                if let Some(v) = global_indices.get_mut(corner) {
                    *v = vertex.checked_add(vertex_offset).ok_or_else(|| invalid("vertex offset overflow"))?;
                }
            }
            if (p[1] - p[0]).cross(p[2] - p[0]).len() <= 1e-12 {
                // Preserve the historical source-degenerate pole-face skip for
                // both sheets and solids. The optical target must retain faces,
                // every retained face must survive upload, and closed targets
                // still require the complete source and uploaded shell proofs.
                continue;
            }
            if let Some(t) = transmission
                && let Some(scatter) = t.subsurface()
            {
                t.with_subsurface(scatter).map_err(|e| invalid(&e.to_string()))?;
            }
            if let Some(model) = transmission.and_then(|t| t.model()) {
                model.validate(surface.dielectric_ior()).map_err(|e| invalid(&e.to_string()))?;
                let roughness = model.roughness as f32;
                let alpha = roughness * roughness;
                if model.roughness > 0.0 && (alpha * alpha < f32::MIN_POSITIVE || !(1.0_f32 / (std::f32::consts::PI * alpha * alpha)).is_finite()) {
                    return Err(invalid("authored glass roughness is below GPU microfacet arithmetic precision"));
                }
            }
            if transmission
                .is_some_and(|t| t.model().is_some_and(|m| m.roughness > 0.0) && t.thin_wall_thickness_mm().is_some() && t.shadow_transmission())
            {
                return Err(invalid("rough thin-sheet glass requires disabled straight shadow transmission"));
            }
            if transmission.is_some() && surface.clear_coat().is_some() {
                return Err(invalid("clear coat and transmission cannot share a surface"));
            }
            if transmission.is_some() && surface.metallic() != 0.0 {
                return Err(invalid("transmission requires a nonmetallic surface"));
            }
            if transmission.is_some() && surface.has_flakes() {
                return Err(invalid("paint flakes and transmission cannot share a surface"));
            }
            let flakes = match surface.flakes() {
                None => 0,
                Some(layer) => {
                    let index = match flake_layers.iter().position(|known| *known == layer) {
                        Some(index) => index,
                        None if flake_layers.len() < MAX_FLAKE_LAYERS => {
                            flake_layers.push(layer);
                            flake_layers.len() - 1
                        }
                        None => return Err(invalid("at most 64 distinct paint flake layers are supported")),
                    };
                    index as u32 + 1
                }
            };
            triangles.push(Triangle {
                p,
                indices: global_indices,
                surface,
                transmission,
                medium: body_index as u32 + 1,
                scalar_face: look.is_some(),
                flakes,
            });
        }
        if authored_optical && !triangles[body_start..].iter().any(|t| t.transmission.is_some()) {
            return Err(invalid("glass boundary has no nondegenerate optical triangles"));
        }
        validate_medium(&triangles[body_start..], cancel)?;
        // Prove the boundary actually sent to the shader as well as the source
        // seam topology. Rounding must not close an authored gap, collapse a
        // valid face, reverse its orientation or break its uploaded shell.
        for (index, triangle) in triangles[body_start..].iter_mut().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            if triangle.transmission.is_some() {
                triangle.p = uploaded_optical_positions(triangle.p)?;
            }
        }
        validate_medium(&triangles[body_start..], cancel)?;
        let body_triangles = &triangles[body_start..];
        if body_triangles.iter().any(|t| t.transmission.is_some()) {
            if media_bounds.len() >= solidcraft_render::job::MAX_GPU_DIELECTRICS {
                return Err(invalid("at most 128 disjoint dielectric bodies are supported"));
            }
            let closed = body_triangles.iter().any(|t| t.transmission.is_some_and(|v| v.thin_wall_thickness_mm().is_none()));
            let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
            let mut hi = -lo;
            for t in body_triangles {
                for p in t.p {
                    lo = lo.min(p);
                    hi = hi.max(p);
                    if t.transmission.is_some() {
                        optical_lo = optical_lo.min(p);
                        optical_hi = optical_hi.max(p);
                    }
                }
            }
            media_bounds.push(super::media_hierarchy::Region {
                lo,
                hi,
                closed,
                triangles: body_start..triangles.len(),
                medium: body_index as u32 + 1,
                name: &body.name,
            });
        }
        vertex_offset = vertex_offset.checked_add(mesh.positions.len() as u32).ok_or_else(|| invalid("vertex offset overflow"))?;
    }
    let parents = super::media_hierarchy::parents(&media_bounds, &triangles, nested, cancel)?;
    let nodes = build_bvh(&mut triangles, cancel)?;
    let mut representatives = std::collections::BTreeMap::new();
    for (index, t) in triangles.iter().enumerate() {
        if t.transmission.is_some_and(|m| m.thin_wall_thickness_mm().is_none()) {
            representatives.entry(t.medium).or_insert(index as u32 + 1);
        }
    }
    let patches = planar_patches::ids(&triangles, cancel)?;
    let extent = nodes.first().map_or(1.0, |n| n.lo.iter().zip(n.hi).map(|(a, b)| b - a).fold(0.0f32, f32::max));
    let mut geometry = reserved(16 + triangles.len().max(1) * (100 + model_bytes) + vertices.len())?;
    let mut materials = reserved(triangles.len().max(1) * material_bytes)?;
    let vertex_word = 4 + triangles.len().max(1) * 4;
    let material_word = vertex_word + vertices.len() / 4;
    let patch_word = material_word + triangles.len().max(1) * (material_bytes / 4);
    let material_word = u32::try_from(material_word)
        .ok()
        .filter(|word| word & !MATERIAL_OFFSET_MASK == 0)
        .ok_or_else(|| invalid("material table offset exceeds its packed geometry layout"))?;
    for word in [triangles.len() as u32, vertex_word as u32, material_word | material_flags, patch_word as u32] {
        geometry.extend_from_slice(&word.to_le_bytes());
    }
    for (material, t) in triangles.into_iter().enumerate() {
        if material % 1024 == 0 {
            cancel.check()?;
        }
        for index in t.indices {
            geometry.extend_from_slice(&index.to_le_bytes());
        }
        geometry.extend_from_slice(&(material as u32).to_le_bytes());
        vector(&mut materials, t.surface.base_color(), if t.transmission.is_some() { 1.0 } else { 0.0 });
        vector(
            &mut materials,
            Vec3::new(t.surface.metallic(), t.surface.roughness(), t.surface.dielectric_ior()),
            t.transmission.and_then(|t| t.thin_wall_thickness_mm()).unwrap_or(-1.0) as f32,
        );
        vector(&mut materials, t.transmission.map_or(Vec3::ZERO, |t| t.extinction()), t.medium as f32);
        let coat = t.surface.clear_coat().map_or(Vec3::new(0.0, 0.5, 1.5), |c| Vec3::new(c.weight(), c.roughness(), c.ior()));
        vector(&mut materials, coat, if t.transmission.is_some_and(|t| t.shadow_transmission()) { 1.0 } else { 0.0 });
        let scatter = t.transmission.and_then(|m| m.subsurface());
        let parent = parents.iter().find(|(id, _)| *id == t.medium).map_or(0, |(_, id)| *id);
        let parent_triangle =
            if parent == 0 { 0 } else { *representatives.get(&parent).ok_or_else(|| invalid("missing certified parent material"))? };
        if scatter.is_some_and(|m| m.active()) {
            let scale = t.p.iter().map(|p| p.x.abs().max(p.y.abs()).max(p.z.abs())).fold(1.0, f64::max);
            if scatter.is_some_and(|m| m.scattering_per_mm.recip() < scale / 1_048_576.0) {
                return Err(invalid("subsurface mean free path falls below camera-relative GPU position precision"));
            }
        }
        vector(
            &mut materials,
            Vec3::new(
                scatter.map_or(0.0, |s| s.scattering_per_mm),
                scatter.map_or(0.0, |s| s.anisotropy),
                (u32::from(scene.meshes.get(t.medium.saturating_sub(1) as usize).is_some_and(|body| body.name.starts_with("__render_stage__/")))
                    | (u32::from(t.scalar_face) << 1)
                    | (t.flakes << 2)) as f64,
            ),
            parent_triangle as f32,
        );
        if model_bytes > 0 {
            let model = t.transmission.and_then(|t| t.model());
            vector(
                &mut materials,
                Vec3::new(
                    model.map_or(0.0, |m| m.roughness),
                    model.and_then(|m| m.dispersion).map_or(0.0, |d| d.abbe_number),
                    model.and_then(|m| m.thin_film).map_or(0.0, |f| f.thickness_nm),
                ),
                model.and_then(|m| m.thin_film).map_or(0.0, |f| f.ior) as f32,
            );
        }
    }
    if geometry.len() == 16 {
        geometry.resize(32, 0);
        materials.resize(material_bytes, 0);
    }
    geometry.extend_from_slice(&vertices);
    drop(vertices);
    geometry.extend_from_slice(&materials);
    drop(materials);
    if patches.is_empty() {
        geometry.extend_from_slice(&0u32.to_le_bytes());
    } else {
        for (index, patch) in patches.into_iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            geometry.extend_from_slice(&patch.to_le_bytes());
        }
    }
    let mut node_bytes = reserved(nodes.len() * 32)?;
    for node in nodes {
        for x in node.lo {
            node_bytes.extend_from_slice(&x.to_le_bytes());
        }
        node_bytes.extend_from_slice(&node.first.to_le_bytes());
        for x in node.hi {
            node_bytes.extend_from_slice(&x.to_le_bytes());
        }
        node_bytes.extend_from_slice(&node.count.to_le_bytes());
    }
    // Exact extrema of retained f32 optical vertices, including sheets. Unlike
    // an emission proposal, this conservative origin certificate is always used.
    let photon_bounds = (optical_lo.is_finite() && optical_hi.is_finite()).then_some([
        optical_lo.x as f32,
        optical_lo.y as f32,
        optical_lo.z as f32,
        optical_hi.x as f32,
        optical_hi.y as f32,
        optical_hi.z as f32,
    ]);
    let center = optical_lo * 0.5 + optical_hi * 0.5;
    let radius = (optical_hi - optical_lo).len() * 0.5;
    let photon_guide = if center.is_finite() && radius.is_finite() && radius > 0.0 {
        // The union already contains retained f32 optical vertices. Cover the
        // center's conversion and the shader's projected-basis arithmetic too.
        let margin = 32.0 * f64::from(f32::EPSILON) * (center.len() + radius);
        let radius = ((radius + margin) as f32).next_up();
        radius.is_finite().then_some([center.x as f32, center.y as f32, center.z as f32, radius])
    } else {
        None
    };
    Ok(Upload {
        geometry,
        flakes: flake_layers,
        nodes: node_bytes,
        epsilon: (extent * 1e-7).max(1e-5),
        active_glass_model,
        photon_guide,
        photon_bounds,
    })
}

fn uploaded_optical_positions(source: [Vec3; 3]) -> Result<[Vec3; 3]> {
    let stored = source.map(|p| Vec3::new(f64::from(p.x as f32), f64::from(p.y as f32), f64::from(p.z as f32)));
    let original_normal = (source[1] - source[0]).cross(source[2] - source[0]);
    let stored_normal = (stored[1] - stored[0]).cross(stored[2] - stored[0]);
    let [a, b, c] = stored.map(|p| [p.x as f32, p.y as f32, p.z as f32]);
    let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
    let shader_normal = Vec3::new(f64::from(u[1] * v[2] - u[2] * v[1]), f64::from(u[2] * v[0] - u[0] * v[2]), f64::from(u[0] * v[1] - u[1] * v[0]));
    if !stored.iter().all(|p| p.is_finite())
        || stored_normal.len() == 0.0
        || original_normal.dot(stored_normal) <= 0.0
        || !shader_normal.is_finite()
        || stored_normal.dot(shader_normal) <= 0.0
    {
        return Err(invalid("glass triangle collapses or reverses orientation at camera-relative GPU position precision"));
    }
    Ok(stored)
}

const MEDIUM_WELD_MM: f64 = 1e-6;

#[derive(Default)]
struct MediumBucket {
    representatives: [usize; 8],
    len: usize,
}

/// Only topology uses these representatives; every uploaded vertex stays unchanged.
/// Checking neighboring cells avoids rounding-boundary cracks in curved CAD seams.
struct MediumWeld {
    cells: HashMap<[i64; 3], MediumBucket>,
    positions: Vec<Vec3>,
    limit: usize,
}

impl MediumWeld {
    fn new(limit: usize) -> Result<Self> {
        let mut cells = HashMap::new();
        cells.try_reserve(limit).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "glass topology allocation failed"))?;
        Ok(Self { cells, positions: reserved(limit)?, limit })
    }

    fn key(&mut self, point: Vec3) -> Result<usize> {
        let xyz = [point.x, point.y, point.z];
        // Leave room for neighboring cells and refuse saturated integer casts.
        if !xyz.into_iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
            return Err(invalid("glass topology position exceeds finite coordinate bounds"));
        }
        let cell = xyz.map(|v| (v / MEDIUM_WELD_MM).floor() as i64);
        let mut found: Option<usize> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    let Some(bucket) = self.cells.get(&neighbor) else { continue };
                    for &index in bucket.representatives.iter().take(bucket.len) {
                        let other = self.positions.get(index).ok_or_else(|| invalid("missing glass topology representative"))?;
                        let delta = point - *other;
                        if delta.dot(delta) <= MEDIUM_WELD_MM * MEDIUM_WELD_MM && found.is_none_or(|old| index < old) {
                            found = Some(index);
                        }
                    }
                }
            }
        }
        if let Some(index) = found {
            return Ok(index);
        }
        if self.positions.len() >= self.limit {
            return Err(invalid("glass topology representative budget exceeded"));
        }
        let index = self.positions.len();
        let bucket = self.cells.entry(cell).or_default();
        // Split a unit cell into eight half-size cubes: points sharing one are
        // at most sqrt(3)/2 tolerances apart, so a cell has at most eight reps.
        let slot = bucket.representatives.get_mut(bucket.len).ok_or_else(|| invalid("glass topology cell budget exceeded"))?;
        *slot = index;
        bucket.len += 1;
        self.positions.push(point);
        Ok(index)
    }
}

// Volume transmission needs a consistently oriented closed two-manifold. CAD
// tessellation duplicates face-seam vertices; compare their actual Euclidean
// separation at the existing one-nanometre-in-mm tolerance, without modifying
// uploaded geometry or merging chains beyond a representative's tolerance.
fn validate_medium(triangles: &[Triangle], cancel: &CancelToken) -> Result<()> {
    let Some(first) = triangles.iter().find(|t| t.transmission.is_some_and(|v| v.thin_wall_thickness_mm().is_none())) else {
        return Ok(());
    };
    if triangles.iter().any(|t| t.transmission != first.transmission || t.surface.dielectric_ior() != first.surface.dielectric_ior()) {
        return Err(invalid("a closed glass body must use one homogeneous transmission medium; use separate bodies or thin walls"));
    }
    let vertex_count = triangles.len().checked_mul(3).ok_or_else(|| invalid("glass topology vertex count overflow"))?;
    let mut edges = reserved::<MediumEdge>(vertex_count)?;
    let mut weld = MediumWeld::new(vertex_count)?;
    let mut parents = reserved::<usize>(triangles.len())?;
    parents.extend(0..triangles.len());
    let origin = first.p[0];
    let mut six_volume = 0.0;
    for (index, triangle) in triangles.iter().enumerate() {
        if index % 1024 == 0 {
            cancel.check()?;
        }
        let keys = [weld.key(triangle.p[0])?, weld.key(triangle.p[1])?, weld.key(triangle.p[2])?];
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            if keys[a] == keys[b] {
                return Err(invalid("glass triangle collapses at the 1e-6 mm topology tolerance"));
            }
            let ordinal = i32::try_from(index + 1).map_err(|_| invalid("too many glass triangles"))?;
            edges.push(if keys[a] < keys[b] { (keys[a], keys[b], ordinal) } else { (keys[b], keys[a], -ordinal) });
        }
        six_volume += (triangle.p[0] - origin).dot((triangle.p[1] - origin).cross(triangle.p[2] - origin));
    }
    edges.sort_unstable();
    for edge in edges.chunks(2) {
        let [a, b] = edge else {
            return Err(invalid("volume glass requires a closed mesh; use thin_wall_thickness_mm for sheets"));
        };
        if a.0 != b.0 || a.1 != b.1 || a.2.signum() + b.2.signum() != 0 {
            return Err(invalid("volume glass requires a closed, consistently outward-oriented mesh"));
        }
        let first = medium_root(&mut parents, a.2.unsigned_abs() as usize - 1)?;
        let second = medium_root(&mut parents, b.2.unsigned_abs() as usize - 1)?;
        *parents.get_mut(first).ok_or_else(|| invalid("invalid glass component"))? = second;
    }
    // Groups with four or more coincident directed edges cannot be a two-manifold.
    if edges.windows(3).any(|e| e[0].0 == e[2].0 && e[0].1 == e[2].1) {
        return Err(invalid("nonmanifold glass edge"));
    }
    let component = medium_root(&mut parents, 0)?;
    for index in 0..parents.len() {
        if index % 1024 == 0 {
            cancel.check()?;
        }
        if medium_root(&mut parents, index)? != component {
            return Err(invalid("closed dielectric body must contain exactly one connected shell"));
        }
    }
    validate_vertex_links(&mut edges, &mut parents, cancel)?;
    if six_volume <= 0.0 || !six_volume.is_finite() {
        return Err(invalid("glass mesh must enclose positive outward-oriented volume"));
    }
    cancel.check()
}

// Reuse the already prepaid edge and face-component scratch. Each matched edge
// supplies one outgoing corner at either endpoint and the adjacent face there.
// In a two-manifold each vertex's incident faces form one circular link; a
// pinched vertex has multiple links even when every edge and the shell pass.
fn validate_vertex_links(edges: &mut [MediumEdge], parents: &mut [usize], cancel: &CancelToken) -> Result<()> {
    for (index, edge) in edges.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        if index % 1024 == 0 {
            cancel.check()?;
        }
        let [a, b] = edge;
        let first = *a;
        let second = *b;
        *a = (if first.2 > 0 { first.0 } else { first.1 }, second.2.unsigned_abs() as usize - 1, first.2.unsigned_abs() as i32);
        *b = (if second.2 > 0 { second.0 } else { second.1 }, first.2.unsigned_abs() as usize - 1, second.2.unsigned_abs() as i32);
    }
    edges.sort_unstable();
    let mut start = 0;
    while start < edges.len() {
        cancel.check()?;
        let vertex = edges.get(start).ok_or_else(|| invalid("missing glass vertex link"))?.0;
        let mut end = start + 1;
        while edges.get(end).is_some_and(|e| e.0 == vertex) {
            if end % 1024 == 0 {
                cancel.check()?;
            }
            end += 1;
        }
        let link = edges.get(start..end).ok_or_else(|| invalid("invalid glass vertex-link range"))?;
        for (index, edge) in link.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            let face = edge.2 as usize - 1;
            *parents.get_mut(face).ok_or_else(|| invalid("invalid glass vertex-link face"))? = face;
        }
        for (index, edge) in link.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            let first = medium_root(parents, edge.2 as usize - 1)?;
            let second = medium_root(parents, edge.1)?;
            *parents.get_mut(first).ok_or_else(|| invalid("invalid glass vertex-link component"))? = second;
        }
        let root = medium_root(parents, link.first().ok_or_else(|| invalid("empty glass vertex link"))?.2 as usize - 1)?;
        for (index, edge) in link.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            if medium_root(parents, edge.2 as usize - 1)? != root {
                return Err(invalid("closed glass vertex link must be one connected circular fan; pinched boundaries are unsupported"));
            }
        }
        start = end;
    }
    cancel.check()
}

fn medium_root(parents: &mut [usize], mut index: usize) -> Result<usize> {
    loop {
        let parent = *parents.get(index).ok_or_else(|| invalid("invalid glass component index"))?;
        if parent == index {
            return Ok(index);
        }
        let grandparent = *parents.get(parent).ok_or_else(|| invalid("invalid glass component parent"))?;
        *parents.get_mut(index).ok_or_else(|| invalid("invalid glass component index"))? = grandparent;
        index = grandparent;
    }
}

// The shader's traversal stack holds 32 nodes and needs at most depth + 1.
const MAX_BVH_DEPTH: usize = 30;
const SAH_BINS: usize = 16;
// Subtrees at or above this size split left/right builds across Rayon workers.
const PARALLEL_BVH_THRESHOLD: usize = 4096;
// Slices at or above this size parallelize SAH centroid bounds and 16-bin histograms.
const PARALLEL_BIN_THRESHOLD: usize = 16_384;
const PARALLEL_BIN_CHUNK: usize = 4096;
type Bounds = ([f32; 3], [f32; 3]);
const EMPTY_BOUNDS: Bounds = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);

fn grow(bounds: &mut Bounds, t: &Triangle) {
    for p in t.p {
        for (axis, value) in [p.x as f32, p.y as f32, p.z as f32].into_iter().enumerate() {
            bounds.0[axis] = bounds.0[axis].min(value);
            bounds.1[axis] = bounds.1[axis].max(value);
        }
    }
}

#[inline]
fn triangle_bounds(t: &Triangle) -> Bounds {
    let mut bounds = EMPTY_BOUNDS;
    grow(&mut bounds, t);
    bounds
}

fn merge(bounds: &mut Bounds, other: &Bounds) {
    for axis in 0..3 {
        bounds.0[axis] = bounds.0[axis].min(other.0[axis]);
        bounds.1[axis] = bounds.1[axis].max(other.1[axis]);
    }
}

fn half_area(bounds: &Bounds) -> f64 {
    let [x, y, z] = [0, 1, 2].map(|axis| f64::from((bounds.1[axis] - bounds.0[axis]).max(0.0)));
    x * y + y * z + z * x
}

// Depth a median split adds below a node of `count` triangles.
fn median_levels(mut count: usize) -> usize {
    let mut levels = 0;
    while count > 4 {
        count = count.div_ceil(2);
        levels += 1;
    }
    levels
}

#[inline]
fn sah_bin(centre: f64, lo: f64, hi: f64) -> usize {
    let scaled = (centre - lo) / (hi - lo) * SAH_BINS as f64;
    (scaled as usize).min(SAH_BINS - 1)
}

fn sah_best_split(
    centre_lo: [f64; 3],
    centre_hi: [f64; 3],
    counts: &[[usize; SAH_BINS]; 3],
    boxes: &[[Bounds; SAH_BINS]; 3],
    depth: usize,
) -> Option<(usize, usize)> {
    let mut best: Option<(f64, usize, usize)> = None;
    for axis in 0..3 {
        if centre_hi[axis] - centre_lo[axis] <= 0.0 {
            continue;
        }
        let mut right_area = [0.0; SAH_BINS];
        let mut right_count = [0usize; SAH_BINS];
        let mut accumulated = EMPTY_BOUNDS;
        let mut count = 0;
        for b in (1..SAH_BINS).rev() {
            merge(&mut accumulated, &boxes[axis][b]);
            count += counts[axis][b];
            right_area[b] = half_area(&accumulated);
            right_count[b] = count;
        }
        accumulated = EMPTY_BOUNDS;
        count = 0;
        for b in 1..SAH_BINS {
            merge(&mut accumulated, &boxes[axis][b - 1]);
            count += counts[axis][b - 1];
            let right = right_count[b];
            if count == 0 || right == 0 || depth + 1 + median_levels(count.max(right)) > MAX_BVH_DEPTH {
                continue;
            }
            let cost = half_area(&accumulated) * count as f64 + right_area[b] * right as f64;
            if best.is_none_or(|(lowest, _, _)| cost < lowest) {
                best = Some((cost, axis, b));
            }
        }
    }
    best.map(|(_, axis, split)| (axis, split))
}

// Binned surface-area heuristic over triangle centres. A split is accepted only
// if a median split of its larger side still fits MAX_BVH_DEPTH, so the median
// fallback always remains available. Returns the left count after partitioning.
fn sah_split(triangles: &mut [Triangle], depth: usize) -> Option<usize> {
    let mut centre_lo = [f64::INFINITY; 3];
    let mut centre_hi = [f64::NEG_INFINITY; 3];
    for t in triangles.iter() {
        for axis in 0..3 {
            let c = t.centre(axis);
            centre_lo[axis] = centre_lo[axis].min(c);
            centre_hi[axis] = centre_hi[axis].max(c);
        }
    }
    let active = [0, 1, 2].map(|axis| centre_hi[axis] - centre_lo[axis] > 0.0);
    if active == [false; 3] {
        return None;
    }
    let mut counts = [[0usize; SAH_BINS]; 3];
    let mut boxes = [[EMPTY_BOUNDS; SAH_BINS]; 3];
    for t in triangles.iter() {
        let tri_bounds = triangle_bounds(t);
        for axis in 0..3 {
            if active[axis] {
                let b = sah_bin(t.centre(axis), centre_lo[axis], centre_hi[axis]);
                counts[axis][b] += 1;
                merge(&mut boxes[axis][b], &tri_bounds);
            }
        }
    }
    let (axis, split) = sah_best_split(centre_lo, centre_hi, &counts, &boxes, depth)?;
    let mut middle = 0;
    for i in 0..triangles.len() {
        if sah_bin(triangles[i].centre(axis), centre_lo[axis], centre_hi[axis]) < split {
            triangles.swap(i, middle);
            middle += 1;
        }
    }
    Some(middle)
}

fn sah_split_parallel(triangles: &mut [Triangle], depth: usize, cancel: &CancelToken) -> Result<Option<usize>> {
    if triangles.len() < PARALLEL_BIN_THRESHOLD {
        return Ok(sah_split(triangles, depth));
    }
    let (centre_lo, centre_hi) = triangles
        .par_chunks(PARALLEL_BIN_CHUNK)
        .try_fold(
            || ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(mut lo, mut hi), chunk| -> Result<([f64; 3], [f64; 3])> {
                cancel.check()?;
                for t in chunk {
                    for axis in 0..3 {
                        let c = t.centre(axis);
                        lo[axis] = lo[axis].min(c);
                        hi[axis] = hi[axis].max(c);
                    }
                }
                Ok((lo, hi))
            },
        )
        .try_reduce(
            || ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(mut lo, mut hi), (other_lo, other_hi)| {
                for ((dst_lo, src_lo), (dst_hi, src_hi)) in lo.iter_mut().zip(other_lo).zip(hi.iter_mut().zip(other_hi)) {
                    *dst_lo = dst_lo.min(src_lo);
                    *dst_hi = dst_hi.max(src_hi);
                }
                Ok((lo, hi))
            },
        )?;
    let active = [0, 1, 2].map(|axis| centre_hi[axis] - centre_lo[axis] > 0.0);
    if active == [false; 3] {
        return Ok(None);
    }
    let (counts, boxes) = triangles
        .par_chunks(PARALLEL_BIN_CHUNK)
        .try_fold(
            || ([[0usize; SAH_BINS]; 3], [[EMPTY_BOUNDS; SAH_BINS]; 3]),
            |(mut counts, mut boxes), chunk| -> Result<([[usize; SAH_BINS]; 3], [[Bounds; SAH_BINS]; 3])> {
                cancel.check()?;
                for t in chunk {
                    let tri_bounds = triangle_bounds(t);
                    for axis in 0..3 {
                        if active[axis] {
                            let b = sah_bin(t.centre(axis), centre_lo[axis], centre_hi[axis]);
                            counts[axis][b] += 1;
                            merge(&mut boxes[axis][b], &tri_bounds);
                        }
                    }
                }
                Ok((counts, boxes))
            },
        )
        .try_reduce(
            || ([[0usize; SAH_BINS]; 3], [[EMPTY_BOUNDS; SAH_BINS]; 3]),
            |(mut counts, mut boxes), (other_counts, other_boxes)| {
                for (((is_active, dst_counts), src_counts), (dst_boxes, src_boxes)) in
                    active.into_iter().zip(&mut counts).zip(&other_counts).zip(boxes.iter_mut().zip(&other_boxes))
                {
                    if is_active {
                        for ((dc, &sc), (db, sb)) in dst_counts.iter_mut().zip(src_counts).zip(dst_boxes.iter_mut().zip(src_boxes)) {
                            *dc += sc;
                            merge(db, sb);
                        }
                    }
                }
                Ok((counts, boxes))
            },
        )?;
    let Some((axis, split)) = sah_best_split(centre_lo, centre_hi, &counts, &boxes, depth) else {
        return Ok(None);
    };
    let mut middle = 0;
    for i in 0..triangles.len() {
        if i.is_multiple_of(4096) {
            cancel.check()?;
        }
        if sah_bin(triangles[i].centre(axis), centre_lo[axis], centre_hi[axis]) < split {
            triangles.swap(i, middle);
            middle += 1;
        }
    }
    Ok(Some(middle))
}

enum Subtree {
    Flat(Vec<Node>),
    Split { lo: [f32; 3], hi: [f32; 3], left: Box<Subtree>, right: Box<Subtree> },
}

impl Subtree {
    fn node_count(&self) -> usize {
        match self {
            Self::Flat(nodes) => nodes.len(),
            Self::Split { left, right, .. } => 1 + left.node_count() + right.node_count(),
        }
    }
}

fn build_subtree(triangles: &mut [Triangle], offset: usize, depth: usize, cancel: &CancelToken) -> Result<Subtree> {
    cancel.check()?;
    if triangles.len() < PARALLEL_BVH_THRESHOLD {
        let mut nodes = reserved(triangles.len().saturating_mul(2).max(1))?;
        nodes.push(Node::default());
        build(triangles, offset, &mut nodes, 0, depth, cancel)?;
        return Ok(Subtree::Flat(nodes));
    }
    let mut bounds = EMPTY_BOUNDS;
    for (i, t) in triangles.iter().enumerate() {
        if i.is_multiple_of(4096) {
            cancel.check()?;
        }
        grow(&mut bounds, t);
    }
    let (lo, hi) = bounds;
    let middle = match sah_split_parallel(triangles, depth, cancel)? {
        Some(middle) => middle,
        None => {
            let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(0);
            let middle = triangles.len() / 2;
            triangles.select_nth_unstable_by(middle, |a, b| a.centre(axis).total_cmp(&b.centre(axis)));
            middle
        }
    };
    let (left, right) = triangles.split_at_mut(middle);
    let (left_res, right_res) =
        rayon::join(|| build_subtree(left, offset, depth + 1, cancel), || build_subtree(right, offset + middle, depth + 1, cancel));
    Ok(Subtree::Split { lo, hi, left: Box::new(left_res?), right: Box::new(right_res?) })
}

fn flatten_subtree(subtree: Subtree, nodes: &mut Vec<Node>, index: usize) -> Result<()> {
    match subtree {
        Subtree::Split { lo, hi, left, right } => {
            let first = nodes.len();
            let first_u32 = u32::try_from(first).map_err(|_| invalid("BVH node index overflow"))?;
            nodes.push(Node::default());
            nodes.push(Node::default());
            *nodes.get_mut(index).ok_or_else(|| invalid("missing BVH node"))? = Node { lo, hi, first: first_u32, count: 0 };
            flatten_subtree(*left, nodes, first)?;
            flatten_subtree(*right, nodes, first + 1)?;
            Ok(())
        }
        Subtree::Flat(sub_nodes) => {
            let mut iter = sub_nodes.into_iter();
            let Some(mut root) = iter.next() else {
                return Err(invalid("missing BVH subtree root"));
            };
            if root.count == 0 && root.first != u32::MAX {
                let desc_start = nodes.len();
                let shift = u32::try_from(desc_start.checked_sub(1).ok_or_else(|| invalid("BVH node index underflow"))?)
                    .map_err(|_| invalid("BVH node index overflow"))?;
                root.first = root.first.checked_add(shift).ok_or_else(|| invalid("BVH node index overflow"))?;
                *nodes.get_mut(index).ok_or_else(|| invalid("missing BVH node"))? = root;
                for mut node in iter {
                    if node.count == 0 && node.first != u32::MAX {
                        node.first = node.first.checked_add(shift).ok_or_else(|| invalid("BVH node index overflow"))?;
                    }
                    nodes.push(node);
                }
            } else {
                *nodes.get_mut(index).ok_or_else(|| invalid("missing BVH node"))? = root;
            }
            Ok(())
        }
    }
}

/// Build the 16-bin SAH BVH, parallelizing upper levels with Rayon while
/// producing the exact bit-for-bit identical node array and triangle order
/// as sequential [`build`].
fn build_bvh(triangles: &mut [Triangle], cancel: &CancelToken) -> Result<Vec<Node>> {
    if triangles.len() < PARALLEL_BVH_THRESHOLD {
        let mut nodes = reserved(triangles.len().saturating_mul(2).max(1))?;
        nodes.push(Node::default());
        build(triangles, 0, &mut nodes, 0, 0, cancel)?;
        return Ok(nodes);
    }
    let subtree = build_subtree(triangles, 0, 0, cancel)?;
    let mut nodes = reserved(subtree.node_count())?;
    nodes.push(Node::default());
    flatten_subtree(subtree, &mut nodes, 0)?;
    Ok(nodes)
}

fn build(triangles: &mut [Triangle], offset: usize, nodes: &mut Vec<Node>, index: usize, depth: usize, cancel: &CancelToken) -> Result<()> {
    cancel.check()?;
    let mut bounds = EMPTY_BOUNDS;
    for t in triangles.iter() {
        grow(&mut bounds, t);
    }
    let (lo, hi) = bounds;
    let mut node = Node { lo, hi, first: offset as u32, count: triangles.len() as u32 };
    if triangles.is_empty() {
        node = Node { first: u32::MAX, ..Default::default() };
    }
    if triangles.len() > 4 {
        let middle = match sah_split(triangles, depth) {
            Some(middle) => middle,
            None => {
                let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(0);
                let middle = triangles.len() / 2;
                triangles.select_nth_unstable_by(middle, |a, b| a.centre(axis).total_cmp(&b.centre(axis)));
                middle
            }
        };
        let first = nodes.len();
        nodes.push(Node::default());
        nodes.push(Node::default());
        node.first = first as u32;
        node.count = 0;
        let (left, right) = triangles.split_at_mut(middle);
        build(left, offset, nodes, first, depth + 1, cancel)?;
        build(right, offset + middle, nodes, first + 1, depth + 1, cancel)?;
    }
    *nodes.get_mut(index).ok_or_else(|| invalid("missing BVH node"))? = node;
    Ok(())
}

pub(super) fn vector(bytes: &mut Vec<u8>, v: Vec3, w: f32) {
    for value in [v.x as f32, v.y as f32, v.z as f32, w] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
#[path = "scene_weld_tests.rs"]
mod weld_tests;

#[cfg(test)]
#[path = "scene_bvh_tests.rs"]
mod bvh_tests;
