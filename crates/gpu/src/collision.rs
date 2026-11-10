//! GPU-accelerated broadphase collision detection, LBVH construction & traversal,
//! and surface-surface intersection (SSI) seed residual evaluation.
//!
//! All operations here are strictly non-authoritative acceleration filters:
//! - Broadphase AABB overlaps produce candidate `(face_a, face_b)` or `(tri_a, tri_b)` pairs
//!   that are subsequently verified by exact B-rep / curve / surface evaluators on the CPU.
//! - Surface residual grids evaluate candidate UV pairs to seed exact Newton-Raphson SSI walking.

use solidcore_record::Vec3;
use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Aabb3, Mesh};

/// Maximum number of overlap pairs returned in a single GPU dispatch pass.
pub const MAX_OVERLAP_PAIRS: u32 = 262_144;

/// Maximum number of SSI seed candidates returned in a single GPU dispatch pass.
pub const MAX_SSI_SEEDS: u32 = 65_536;

/// Minimum workload size (`count_a * count_b`) before dispatching to Vulkan/wgpu by default.
pub const GPU_BROADPHASE_THRESHOLD: usize = 4_096;

/// Safe, zero-dependency conversion between fixed-size GPU structs and little-endian byte buffers.
pub trait GpuPod: Copy + Default {
    /// Number of 32-bit words representing one instance of `Self`.
    const WORDS: usize;
    /// Appends the `u32` words representing `self` into `out`.
    fn push_words(&self, out: &mut Vec<u32>);
    /// Reconstructs `Self` from a slice of `Self::WORDS` 32-bit words.
    fn from_words(w: &[u32]) -> Option<Self>;
}

/// Encodes a slice of [`GpuPod`] elements into a little-endian byte vector.
pub fn encode_pod_slice<T: GpuPod>(items: &[T]) -> Vec<u8> {
    let mut words = Vec::with_capacity(items.len() * T::WORDS);
    for item in items {
        item.push_words(&mut words);
    }
    if words.is_empty() {
        words.resize(T::WORDS.max(1), 0);
    }
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

/// Encodes a single [`GpuPod`] value into a little-endian byte vector.
pub fn encode_pod<T: GpuPod>(item: &T) -> Vec<u8> {
    encode_pod_slice(std::slice::from_ref(item))
}

/// Decodes a little-endian byte slice into a vector of [`GpuPod`] elements.
pub fn decode_pod_slice<T: GpuPod>(bytes: &[u8]) -> Vec<T> {
    let words: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    if T::WORDS == 0 {
        return Vec::new();
    }
    words.chunks_exact(T::WORDS).filter_map(T::from_words).collect()
}

impl GpuPod for u32 {
    const WORDS: usize = 1;
    /// Appends this `u32` word to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.push(*self);
    }
    /// Reads a single `u32` word from `w`.
    fn from_words(w: &[u32]) -> Option<Self> {
        w.first().copied()
    }
}

impl GpuPod for f32 {
    const WORDS: usize = 1;
    /// Appends the raw IEEE-754 bits of this `f32` to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.push(self.to_bits());
    }
    /// Reconstructs an `f32` from the first word in `w`.
    fn from_words(w: &[u32]) -> Option<Self> {
        w.first().copied().map(f32::from_bits)
    }
}

impl GpuPod for [f32; 4] {
    const WORDS: usize = 4;
    /// Appends the 4 `f32` words as `u32` bits to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend(self.map(f32::to_bits));
    }
    /// Reconstructs `[f32; 4]` from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some([f32::from_bits(*w.first()?), f32::from_bits(*w.get(1)?), f32::from_bits(*w.get(2)?), f32::from_bits(*w.get(3)?)])
    }
}

/// 32-byte aligned AABB for GPU storage buffers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuAabb {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    pub id: u32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
    pub pad: u32,
}

impl GpuPod for GpuAabb {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`GpuAabb`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([
            self.min_x.to_bits(),
            self.min_y.to_bits(),
            self.min_z.to_bits(),
            self.id,
            self.max_x.to_bits(),
            self.max_y.to_bits(),
            self.max_z.to_bits(),
            self.pad,
        ]);
    }
    /// Decodes a [`GpuAabb`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            min_x: f32::from_bits(*w.first()?),
            min_y: f32::from_bits(*w.get(1)?),
            min_z: f32::from_bits(*w.get(2)?),
            id: *w.get(3)?,
            max_x: f32::from_bits(*w.get(4)?),
            max_y: f32::from_bits(*w.get(5)?),
            max_z: f32::from_bits(*w.get(6)?),
            pad: *w.get(7)?,
        })
    }
}

impl GpuAabb {
    /// Converts an exact `f64` [`Aabb3`] into an outward-rounded `f32` [`GpuAabb`].
    ///
    /// Outward rounding (`next_down` on min, `next_up` on max) guarantees conservative
    /// broadphase filtering: no true intersection in `f64` is ever missed due to `f32` cast.
    pub fn from_aabb3(b: &Aabb3, id: u32) -> Self {
        if b.is_empty() {
            return Self {
                min_x: f32::INFINITY,
                min_y: f32::INFINITY,
                min_z: f32::INFINITY,
                id,
                max_x: f32::NEG_INFINITY,
                max_y: f32::NEG_INFINITY,
                max_z: f32::NEG_INFINITY,
                pad: 0,
            };
        }
        Self {
            min_x: (b.min.x as f32).next_down(),
            min_y: (b.min.y as f32).next_down(),
            min_z: (b.min.z as f32).next_down(),
            id,
            max_x: (b.max.x as f32).next_up(),
            max_y: (b.max.y as f32).next_up(),
            max_z: (b.max.z as f32).next_up(),
            pad: 0,
        }
    }

    /// Tests whether two [`GpuAabb`]s overlap within `tol`.
    pub fn overlaps(&self, other: &Self, tol: f32) -> bool {
        (self.min_x - tol <= other.max_x)
            && (self.max_x + tol >= other.min_x)
            && (self.min_y - tol <= other.max_y)
            && (self.max_y + tol >= other.min_y)
            && (self.min_z - tol <= other.max_z)
            && (self.max_z + tol >= other.min_z)
    }
}

/// A pair of overlapping primitive/face IDs.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OverlapPair {
    pub a: u32,
    pub b: u32,
}

impl GpuPod for OverlapPair {
    const WORDS: usize = 2;
    /// Appends the 2 `u32` IDs of this [`OverlapPair`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([self.a, self.b]);
    }
    /// Decodes an [`OverlapPair`] from a 2-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { a: *w.first()?, b: *w.get(1)? })
    }
}

/// Result of a broadphase overlap query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollisionResult {
    pub pairs: Vec<OverlapPair>,
    pub overflowed: bool,
}

/// Vulkan / `wgpu` compute device handle and synchronization lock.
pub struct CollisionGpu {
    device: wgpu::Device,
    _queue: wgpu::Queue,
    dispatch_lock: std::sync::Arc<std::sync::Mutex<()>>,
}

impl CollisionGpu {
    /// Creates a new [`CollisionGpu`] instance on the default headless `wgpu` adapter (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new() -> Option<Self> {
        let policy = crate::env::policy().ok()?;
        let (_, device, queue) = solidcraft_viewport_gpu::open_headless(policy).ok()?;
        Some(Self::from_device(device, queue))
    }

    /// Acquires the device-level dispatch lock serializing concurrent host command submissions and polls.
    #[allow(dead_code)]
    pub(crate) fn lock_device(&self) -> std::sync::MutexGuard<'_, ()> {
        match self.dispatch_lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Returns `true` if a storage buffer of `bytes` bytes fits within the device's binding and buffer size limits.
    #[allow(dead_code)]
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn fits_storage(&self, bytes: usize) -> bool {
        let limits = self.device.limits();
        let bytes = bytes as u64;
        bytes <= limits.max_storage_buffer_binding_size && bytes <= limits.max_buffer_size
    }

    /// Creates a [`CollisionGpu`] sharing an existing `wgpu::Device` and `wgpu::Queue`.
    pub fn from_device(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, _queue: queue, dispatch_lock: std::sync::Arc::new(std::sync::Mutex::new(())) }
    }
}

/// Extracts per-face conservative [`GpuAabb`]s from a [`TessPlan`] by sampling its UV grid.
pub fn face_aabbs_from_plan(plan: &TessPlan) -> Vec<GpuAabb> {
    let mut out = Vec::with_capacity(plan.faces.len());
    for f in &plan.faces {
        let mut bx = Aabb3::EMPTY;
        if let Some(s_idx) = f.surface
            && let Some(surf) = plan.surfaces.get(s_idx as usize)
        {
            for &[u, v] in &f.uv {
                bx.add(solidcraft_geom::tess::from_kernel(surf.eval(u, v).0));
            }
        }
        for &p in &f.fixed {
            bx.add(p);
        }
        if !bx.is_empty() {
            out.push(GpuAabb::from_aabb3(&bx, f.face));
        }
    }
    out
}

/// Extracts per-face conservative [`GpuAabb`]s from a [`Mesh`] grouped by `tri_face`.
pub fn face_aabbs_from_mesh(mesh: &Mesh) -> Vec<GpuAabb> {
    let mut map: std::collections::BTreeMap<u32, Aabb3> = std::collections::BTreeMap::new();
    for (k, tri) in mesh.triangles.iter().enumerate() {
        let Some([a, b, c]) = mesh.tri(tri) else { continue };
        let face_id = mesh.tri_face.get(k).copied().unwrap_or(0);
        let entry = map.entry(face_id).or_insert(Aabb3::EMPTY);
        entry.add(a);
        entry.add(b);
        entry.add(c);
    }
    map.into_iter().map(|(id, bx)| GpuAabb::from_aabb3(&bx, id)).collect()
}

/// Extracts per-triangle [`GpuAabb`]s from a [`Mesh`].
pub fn triangle_aabbs_from_mesh(mesh: &Mesh) -> Vec<GpuAabb> {
    let mut out = Vec::with_capacity(mesh.triangles.len());
    for (i, tri) in mesh.triangles.iter().enumerate() {
        let Some([a, b, c]) = mesh.tri(tri) else { continue };
        let mut bx = Aabb3::EMPTY;
        bx.add(a);
        bx.add(b);
        bx.add(c);
        out.push(GpuAabb::from_aabb3(&bx, i as u32));
    }
    out
}

/// Builds a synthetic grid of [`GpuAabb`]s for benchmarking and testing.
pub fn synthetic_aabb_grid(count: usize, spacing: f32, box_half_ext: f32) -> Vec<GpuAabb> {
    let side = ((count as f64).cbrt().ceil() as usize).max(1);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let ix = (i % side) as f32;
        let iy = ((i / side) % side) as f32;
        let iz = (i / (side * side)) as f32;
        let cx = ix * spacing;
        let cy = iy * spacing;
        let cz = iz * spacing;
        out.push(GpuAabb {
            min_x: cx - box_half_ext,
            min_y: cy - box_half_ext,
            min_z: cz - box_half_ext,
            id: i as u32,
            max_x: cx + box_half_ext,
            max_y: cy + box_half_ext,
            max_z: cz + box_half_ext,
            pad: 0,
        });
    }
    out
}

/// Converts a slice of `(Vec3, Vec3)` min/max pairs into [`GpuAabb`]s.
pub fn aabbs_from_bounds(bounds: &[(Vec3, Vec3)]) -> Vec<GpuAabb> {
    bounds.iter().enumerate().map(|(i, (lo, hi))| GpuAabb::from_aabb3(&Aabb3 { min: *lo, max: *hi }, i as u32)).collect()
}
