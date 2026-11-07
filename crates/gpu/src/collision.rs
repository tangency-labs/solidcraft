//! GPU-accelerated broadphase collision detection, LBVH construction & traversal,
//! and surface-surface intersection (SSI) seed residual evaluation.
//!
//! All operations here are strictly non-authoritative acceleration filters:
//! - Broadphase AABB overlaps produce candidate `(face_a, face_b)` or `(tri_a, tri_b)` pairs
//!   that are subsequently verified by exact B-rep / curve / surface evaluators on the CPU.
//! - Surface residual grids evaluate candidate UV pairs to seed exact Newton-Raphson SSI walking.

use solidcore_record::Vec3;
use solidcraft_geom::tess::{EvalSurface, TessPlan};
use solidcraft_geom::{Aabb3, Mesh};
use wgpu::util::DeviceExt;

const COLLISION_SHADER: &str = include_str!("collision.wgsl");

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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CollisionParams {
    count_a: u32,
    count_b: u32,
    max_pairs: u32,
    tolerance: f32,
}

impl GpuPod for CollisionParams {
    const WORDS: usize = 4;
    /// Appends the 4 `u32` words of [`CollisionParams`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([self.count_a, self.count_b, self.max_pairs, self.tolerance.to_bits()]);
    }
    /// Decodes [`CollisionParams`] from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { count_a: *w.first()?, count_b: *w.get(1)?, max_pairs: *w.get(2)?, tolerance: f32::from_bits(*w.get(3)?) })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct MortonParams {
    count: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    scene_min: [f32; 4],
    scene_inv_extent: [f32; 4],
}

impl GpuPod for MortonParams {
    const WORDS: usize = 12;
    /// Appends the 12 `u32` words of [`MortonParams`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([self.count, self.pad0, self.pad1, self.pad2]);
        out.extend(self.scene_min.map(f32::to_bits));
        out.extend(self.scene_inv_extent.map(f32::to_bits));
    }
    /// Decodes [`MortonParams`] from a 12-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            count: *w.first()?,
            pad0: *w.get(1)?,
            pad1: *w.get(2)?,
            pad2: *w.get(3)?,
            scene_min: [f32::from_bits(*w.get(4)?), f32::from_bits(*w.get(5)?), f32::from_bits(*w.get(6)?), f32::from_bits(*w.get(7)?)],
            scene_inv_extent: [f32::from_bits(*w.get(8)?), f32::from_bits(*w.get(9)?), f32::from_bits(*w.get(10)?), f32::from_bits(*w.get(11)?)],
        })
    }
}

/// 30-bit 3D Morton code paired with original primitive index.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct GpuMortonItem {
    pub code: u32,
    pub index: u32,
}

impl GpuPod for GpuMortonItem {
    const WORDS: usize = 2;
    /// Appends the 2 `u32` words of this [`GpuMortonItem`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([self.code, self.index]);
    }
    /// Decodes a [`GpuMortonItem`] from a 2-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { code: *w.first()?, index: *w.get(1)? })
    }
}

/// Flattened LBVH node (32 bytes) for stackless/iterative GPU traversal.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuLbvhNode {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    /// For internal nodes: left child index. For leaves: primitive ID.
    pub left_or_prim: u32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
    /// For internal nodes: right child index. For leaves: `0x8000_0000` flag bit set.
    pub right_or_flag: u32,
}

impl GpuPod for GpuLbvhNode {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`GpuLbvhNode`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([
            self.min_x.to_bits(),
            self.min_y.to_bits(),
            self.min_z.to_bits(),
            self.left_or_prim,
            self.max_x.to_bits(),
            self.max_y.to_bits(),
            self.max_z.to_bits(),
            self.right_or_flag,
        ]);
    }
    /// Decodes a [`GpuLbvhNode`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            min_x: f32::from_bits(*w.first()?),
            min_y: f32::from_bits(*w.get(1)?),
            min_z: f32::from_bits(*w.get(2)?),
            left_or_prim: *w.get(3)?,
            max_x: f32::from_bits(*w.get(4)?),
            max_y: f32::from_bits(*w.get(5)?),
            max_z: f32::from_bits(*w.get(6)?),
            right_or_flag: *w.get(7)?,
        })
    }
}

/// Sampled surface point and UV coordinates for SSI seed hunting.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuSurfaceSample {
    /// `xyz` = 3D position, `w` = parameter `u`.
    pub pos_u: [f32; 4],
    /// `x` = parameter `v`, `yzw` = unit surface normal.
    pub v_norm: [f32; 4],
}

impl GpuPod for GpuSurfaceSample {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`GpuSurfaceSample`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend(self.pos_u.map(f32::to_bits));
        out.extend(self.v_norm.map(f32::to_bits));
    }
    /// Decodes a [`GpuSurfaceSample`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            pos_u: [f32::from_bits(*w.first()?), f32::from_bits(*w.get(1)?), f32::from_bits(*w.get(2)?), f32::from_bits(*w.get(3)?)],
            v_norm: [f32::from_bits(*w.get(4)?), f32::from_bits(*w.get(5)?), f32::from_bits(*w.get(6)?), f32::from_bits(*w.get(7)?)],
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct SsiSeedParams {
    count_a: u32,
    count_b: u32,
    max_seeds: u32,
    threshold_sq: f32,
}

impl GpuPod for SsiSeedParams {
    const WORDS: usize = 4;
    /// Appends the 4 `u32` words of [`SsiSeedParams`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([self.count_a, self.count_b, self.max_seeds, self.threshold_sq.to_bits()]);
    }
    /// Decodes [`SsiSeedParams`] from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { count_a: *w.first()?, count_b: *w.get(1)?, max_seeds: *w.get(2)?, threshold_sq: f32::from_bits(*w.get(3)?) })
    }
}

/// Candidate UV seed pair where two surfaces approach within threshold distance.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeedCandidate {
    pub u_a: f32,
    pub v_a: f32,
    pub u_b: f32,
    pub v_b: f32,
    pub dist_sq: f32,
    pub idx_a: u32,
    pub idx_b: u32,
    pub pad: u32,
}

impl GpuPod for SeedCandidate {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`SeedCandidate`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend([
            self.u_a.to_bits(),
            self.v_a.to_bits(),
            self.u_b.to_bits(),
            self.v_b.to_bits(),
            self.dist_sq.to_bits(),
            self.idx_a,
            self.idx_b,
            self.pad,
        ]);
    }
    /// Decodes a [`SeedCandidate`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            u_a: f32::from_bits(*w.first()?),
            v_a: f32::from_bits(*w.get(1)?),
            u_b: f32::from_bits(*w.get(2)?),
            v_b: f32::from_bits(*w.get(3)?),
            dist_sq: f32::from_bits(*w.get(4)?),
            idx_a: *w.get(5)?,
            idx_b: *w.get(6)?,
            pad: *w.get(7)?,
        })
    }
}

/// Vulkan / `wgpu` compute pipelines for broadphase collision, LBVH, and SSI seed evaluation.
pub struct CollisionGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    dispatch_lock: std::sync::Arc<std::sync::Mutex<()>>,
    bipartite_pipeline: wgpu::ComputePipeline,
    self_pipeline: wgpu::ComputePipeline,
    lbvh_pipeline: wgpu::ComputePipeline,
    overlap_bgl: wgpu::BindGroupLayout,
    morton_pipeline: wgpu::ComputePipeline,
    morton_bgl: wgpu::BindGroupLayout,
    ssi_pipeline: wgpu::ComputePipeline,
    ssi_bgl: wgpu::BindGroupLayout,
}

impl CollisionGpu {
    /// Creates a new [`CollisionGpu`] instance on the default headless `wgpu` adapter (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new() -> Option<Self> {
        let (_, device, queue) = crate::open_headless().ok()?;
        Some(Self::from_device(device, queue))
    }

    /// Acquires the device-level dispatch lock serializing concurrent host command submissions and polls.
    pub(crate) fn lock_device(&self) -> std::sync::MutexGuard<'_, ()> {
        match self.dispatch_lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Returns `true` if a storage buffer of `bytes` bytes fits within the device's binding and buffer size limits.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn fits_storage(&self, bytes: usize) -> bool {
        let limits = self.device.limits();
        let bytes = bytes as u64;
        bytes <= limits.max_storage_buffer_binding_size && bytes <= limits.max_buffer_size
    }

    /// Creates a [`CollisionGpu`] sharing an existing `wgpu::Device` and `wgpu::Queue`.
    pub fn from_device(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("solidcraft-collision-wgsl"),
            source: wgpu::ShaderSource::Wgsl(COLLISION_SHADER.into()),
        });

        let entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 0 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only } },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let overlap_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("collision-overlap-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, true), entry(3, false), entry(4, false)],
        });
        let overlap_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("collision-overlap-pl"),
            bind_group_layouts: &[Some(&overlap_bgl)],
            immediate_size: 0,
        });

        let make_pipeline = |pl: &wgpu::PipelineLayout, ep: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(ep),
                layout: Some(pl),
                module: &shader,
                entry_point: Some(ep),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        let bipartite_pipeline = make_pipeline(&overlap_pl, "find_bipartite_overlaps");
        let self_pipeline = make_pipeline(&overlap_pl, "find_self_overlaps");
        let lbvh_pipeline = make_pipeline(&overlap_pl, "query_lbvh");

        let morton_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("collision-morton-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, false)],
        });
        let morton_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("collision-morton-pl"),
            bind_group_layouts: &[Some(&morton_bgl)],
            immediate_size: 0,
        });
        let morton_pipeline = make_pipeline(&morton_pl, "compute_morton_codes");

        let ssi_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("collision-ssi-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, true), entry(3, false), entry(4, false)],
        });
        let ssi_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("collision-ssi-pl"),
            bind_group_layouts: &[Some(&ssi_bgl)],
            immediate_size: 0,
        });
        let ssi_pipeline = make_pipeline(&ssi_pl, "evaluate_surface_residuals");

        Self {
            device,
            queue,
            dispatch_lock: std::sync::Arc::new(std::sync::Mutex::new(())),
            bipartite_pipeline,
            self_pipeline,
            lbvh_pipeline,
            overlap_bgl,
            morton_pipeline,
            morton_bgl,
            ssi_pipeline,
            ssi_bgl,
        }
    }

    /// Finds all overlapping pairs `(a.id, b.id)` between `aabbs_a` and `aabbs_b` within `tolerance`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn bipartite_overlap(&self, aabbs_a: &[GpuAabb], aabbs_b: &[GpuAabb], tolerance: f32) -> Option<CollisionResult> {
        if aabbs_a.is_empty() || aabbs_b.is_empty() {
            return Some(CollisionResult::default());
        }
        let max_pairs = MAX_OVERLAP_PAIRS;
        let params = CollisionParams {
            count_a: u32::try_from(aabbs_a.len()).ok()?,
            count_b: u32::try_from(aabbs_b.len()).ok()?,
            max_pairs,
            tolerance: tolerance.max(0.0),
        };
        let wg_x = params.count_a.div_ceil(16);
        let wg_y = params.count_b.div_ceil(16);
        let max_wg = self.device.limits().max_compute_workgroups_per_dimension;
        if wg_x > max_wg || wg_y > max_wg || !self.fits_storage(std::mem::size_of_val(aabbs_a)) || !self.fits_storage(std::mem::size_of_val(aabbs_b))
        {
            return None;
        }
        self.run_overlap_kernel(&self.bipartite_pipeline, &params, &encode_pod_slice(aabbs_a), &encode_pod_slice(aabbs_b), (wg_x, wg_y, 1))
    }

    /// Finds all self-overlapping pairs `(a[i].id, a[j].id)` with `i < j` within `tolerance`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn self_overlap(&self, aabbs: &[GpuAabb], tolerance: f32) -> Option<CollisionResult> {
        if aabbs.len() < 2 {
            return Some(CollisionResult::default());
        }
        let max_pairs = MAX_OVERLAP_PAIRS;
        let count = u32::try_from(aabbs.len()).ok()?;
        let params = CollisionParams { count_a: count, count_b: count, max_pairs, tolerance: tolerance.max(0.0) };
        let wg = count.div_ceil(16);
        if wg > self.device.limits().max_compute_workgroups_per_dimension || !self.fits_storage(std::mem::size_of_val(aabbs)) {
            return None;
        }
        let dummy = [GpuAabb::default()];
        self.run_overlap_kernel(&self.self_pipeline, &params, &encode_pod_slice(aabbs), &encode_pod_slice(&dummy), (wg, wg, 1))
    }

    /// Queries an LBVH tree (`nodes`) against `query_aabbs` in $O(N \log M)$ time on the GPU.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn lbvh_query(&self, query_aabbs: &[GpuAabb], nodes: &[GpuLbvhNode], tolerance: f32) -> Option<CollisionResult> {
        if query_aabbs.is_empty() || nodes.is_empty() {
            return Some(CollisionResult::default());
        }
        let max_pairs = MAX_OVERLAP_PAIRS;
        let count_a = u32::try_from(query_aabbs.len()).ok()?;
        let count_b = u32::try_from(nodes.len()).ok()?;
        let params = CollisionParams { count_a, count_b, max_pairs, tolerance: tolerance.max(0.0) };
        let wg_x = count_a.div_ceil(256);
        if wg_x > self.device.limits().max_compute_workgroups_per_dimension
            || !self.fits_storage(std::mem::size_of_val(query_aabbs))
            || !self.fits_storage(std::mem::size_of_val(nodes))
        {
            return None;
        }
        self.run_overlap_kernel(&self.lbvh_pipeline, &params, &encode_pod_slice(query_aabbs), &encode_pod_slice(nodes), (wg_x, 1, 1))
    }

    /// Computes 30-bit 3D Morton codes on the GPU and returns them sorted by `(code, index)`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compute_morton(&self, aabbs: &[GpuAabb]) -> Option<Vec<GpuMortonItem>> {
        if aabbs.is_empty() {
            return Some(Vec::new());
        }
        let count = u32::try_from(aabbs.len()).ok()?;
        let wg_x = count.div_ceil(256);
        let out_bytes = aabbs.len().checked_mul(std::mem::size_of::<GpuMortonItem>())?;
        if wg_x > self.device.limits().max_compute_workgroups_per_dimension
            || !self.fits_storage(std::mem::size_of_val(aabbs))
            || !self.fits_storage(out_bytes)
        {
            return None;
        }
        let (mut min_x, mut min_y, mut min_z) = (f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y, mut max_z) = (f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for b in aabbs {
            let cx = (b.min_x + b.max_x) * 0.5;
            let cy = (b.min_y + b.max_y) * 0.5;
            let cz = (b.min_z + b.max_z) * 0.5;
            min_x = min_x.min(cx);
            min_y = min_y.min(cy);
            min_z = min_z.min(cz);
            max_x = max_x.max(cx);
            max_y = max_y.max(cy);
            max_z = max_z.max(cz);
        }
        let inv = |lo: f32, hi: f32| {
            let d = hi - lo;
            if d > 1e-12 { 1.0 / d } else { 0.0 }
        };
        let params = MortonParams {
            count,
            pad0: 0,
            pad1: 0,
            pad2: 0,
            scene_min: [min_x, min_y, min_z, 0.0],
            scene_inv_extent: [inv(min_x, max_x), inv(min_y, max_y), inv(min_z, max_z), 0.0],
        };

        let _guard = self.lock_device();
        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("morton-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let in_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("morton-in"),
            contents: &encode_pod_slice(aabbs),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (aabbs.len() * std::mem::size_of::<GpuMortonItem>()) as u64;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("morton-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("morton-read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("morton-bg"),
            layout: &self.morton_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: in_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: out_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("morton-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("morton-pass"), timestamp_writes: None });
            pass.set_pipeline(&self.morton_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(wg_x, 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_size);
        self.queue.submit(Some(enc.finish()));

        let slice = read_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let mut items: Vec<GpuMortonItem> = decode_pod_slice(&mapped);
        drop(mapped);
        read_buf.unmap();
        items.sort_unstable();
        Some(items)
    }

    /// Evaluates UV sample grids on two surfaces (`surf_a`, `surf_b`) and returns candidate
    /// UV pairs whose 3D positions lie within `distance_threshold`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn surface_residuals(
        &self,
        surf_a: &EvalSurface,
        u_range_a: (f64, f64),
        v_range_a: (f64, f64),
        surf_b: &EvalSurface,
        u_range_b: (f64, f64),
        v_range_b: (f64, f64),
        grid_res: u32,
        distance_threshold: f32,
    ) -> Option<Vec<SeedCandidate>> {
        let res = grid_res.clamp(2, 256);
        let sample_grid = |surf: &EvalSurface, u_r: (f64, f64), v_r: (f64, f64)| -> Vec<GpuSurfaceSample> {
            let mut out = Vec::with_capacity((res * res) as usize);
            for iu in 0..res {
                let u = u_r.0 + (u_r.1 - u_r.0) * (iu as f64 / (res - 1) as f64);
                for iv in 0..res {
                    let v = v_r.0 + (v_r.1 - v_r.0) * (iv as f64 / (res - 1) as f64);
                    let (p, n) = surf.eval(u, v);
                    out.push(GpuSurfaceSample {
                        pos_u: [p.x as f32, p.y as f32, p.z as f32, u as f32],
                        v_norm: [v as f32, n.x as f32, n.y as f32, n.z as f32],
                    });
                }
            }
            out
        };

        let samples_a = sample_grid(surf_a, u_range_a, v_range_a);
        let samples_b = sample_grid(surf_b, u_range_b, v_range_b);
        let count_a = u32::try_from(samples_a.len()).ok()?;
        let count_b = u32::try_from(samples_b.len()).ok()?;
        let max_seeds = MAX_SSI_SEEDS;
        let params = SsiSeedParams { count_a, count_b, max_seeds, threshold_sq: distance_threshold * distance_threshold };

        let _guard = self.lock_device();
        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ssi-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let a_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ssi-a"),
            contents: &encode_pod_slice(&samples_a),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let b_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ssi-b"),
            contents: &encode_pod_slice(&samples_b),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_bytes = (max_seeds as usize * std::mem::size_of::<SeedCandidate>()) as u64;
        let seeds_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssi-seeds"),
            size: out_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let count_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ssi-count"),
            contents: &0u32.to_le_bytes(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let count_read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssi-count-read"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssi-bg"),
            layout: &self.ssi_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: a_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: b_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: seeds_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: count_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ssi-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ssi-pass"), timestamp_writes: None });
            pass.set_pipeline(&self.ssi_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(count_a.div_ceil(16), count_b.div_ceil(16), 1);
        }
        enc.copy_buffer_to_buffer(&count_buf, 0, &count_read, 0, 4);
        self.queue.submit(Some(enc.finish()));

        let count_slice = count_read.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        count_slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;

        let raw_count = {
            let mapped = count_slice.get_mapped_range().ok()?;
            let c = *decode_pod_slice::<u32>(&mapped).first()?;
            drop(mapped);
            count_read.unmap();
            c
        };
        let actual = raw_count.min(max_seeds) as usize;
        if actual == 0 {
            return Some(Vec::new());
        }

        let copy_bytes = (actual * std::mem::size_of::<SeedCandidate>()) as u64;
        let seeds_read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssi-seeds-read"),
            size: copy_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc2 = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ssi-copy-enc") });
        enc2.copy_buffer_to_buffer(&seeds_buf, 0, &seeds_read, 0, copy_bytes);
        self.queue.submit(Some(enc2.finish()));

        let s_slice = seeds_read.slice(..);
        let (tx2, rx2) = std::sync::mpsc::channel();
        s_slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx2.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx2.recv().ok()?.ok()?;

        let mapped = s_slice.get_mapped_range().ok()?;
        let mut seeds: Vec<SeedCandidate> = decode_pod_slice(&mapped);
        drop(mapped);
        seeds_read.unmap();
        seeds.sort_by(|a, b| a.dist_sq.total_cmp(&b.dist_sq));
        Some(seeds)
    }

    /// Dispatches an AABB overlap compute pipeline and reads back the sorted [`OverlapPair`] buffer.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_overlap_kernel(
        &self,
        pipeline: &wgpu::ComputePipeline,
        params: &CollisionParams,
        bytes_a: &[u8],
        bytes_b: &[u8],
        workgroups: (u32, u32, u32),
    ) -> Option<CollisionResult> {
        let pairs_bytes = (params.max_pairs as usize).checked_mul(std::mem::size_of::<OverlapPair>())?;
        if !self.fits_storage(bytes_a.len()) || !self.fits_storage(bytes_b.len()) || !self.fits_storage(pairs_bytes) {
            return None;
        }
        let _guard = self.lock_device();
        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlap-params"),
            contents: &encode_pod(params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let a_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlap-a"),
            contents: bytes_a,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let b_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlap-b"),
            contents: bytes_b,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let pairs_bytes = pairs_bytes as u64;
        let pairs_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlap-pairs"),
            size: pairs_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let count_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlap-count"),
            contents: &0u32.to_le_bytes(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let count_read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlap-count-read"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlap-bg"),
            layout: &self.overlap_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: a_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: b_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: pairs_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: count_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlap-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("overlap-pass"), timestamp_writes: None });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(workgroups.0, workgroups.1, workgroups.2);
        }
        enc.copy_buffer_to_buffer(&count_buf, 0, &count_read, 0, 4);
        self.queue.submit(Some(enc.finish()));

        let c_slice = count_read.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        c_slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;

        let raw_count = {
            let mapped = c_slice.get_mapped_range().ok()?;
            let c = *decode_pod_slice::<u32>(&mapped).first()?;
            drop(mapped);
            count_read.unmap();
            c
        };
        let overflowed = raw_count > params.max_pairs;
        let actual = raw_count.min(params.max_pairs) as usize;
        if actual == 0 {
            return Some(CollisionResult { pairs: Vec::new(), overflowed });
        }

        let copy_bytes = (actual * std::mem::size_of::<OverlapPair>()) as u64;
        let pairs_read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlap-pairs-read"),
            size: copy_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc2 = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlap-copy-enc") });
        enc2.copy_buffer_to_buffer(&pairs_buf, 0, &pairs_read, 0, copy_bytes);
        self.queue.submit(Some(enc2.finish()));

        let p_slice = pairs_read.slice(..);
        let (tx2, rx2) = std::sync::mpsc::channel();
        p_slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx2.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx2.recv().ok()?.ok()?;

        let mapped = p_slice.get_mapped_range().ok()?;
        let mut pairs: Vec<OverlapPair> = decode_pod_slice(&mapped);
        drop(mapped);
        pairs_read.unmap();
        pairs.sort_unstable();
        Some(CollisionResult { pairs, overflowed })
    }
}

/// CPU reference implementation for bipartite AABB overlap.
pub fn cpu_bipartite_overlap(aabbs_a: &[GpuAabb], aabbs_b: &[GpuAabb], tolerance: f32) -> CollisionResult {
    let tol = tolerance.max(0.0);
    let mut pairs = Vec::new();
    let mut overflowed = false;
    for a in aabbs_a {
        for b in aabbs_b {
            if a.overlaps(b, tol) {
                if pairs.len() < MAX_OVERLAP_PAIRS as usize {
                    pairs.push(OverlapPair { a: a.id, b: b.id });
                } else {
                    overflowed = true;
                }
            }
        }
    }
    pairs.sort_unstable();
    CollisionResult { pairs, overflowed }
}

/// CPU reference implementation for self AABB overlap (`i < j`).
pub fn cpu_self_overlap(aabbs: &[GpuAabb], tolerance: f32) -> CollisionResult {
    let tol = tolerance.max(0.0);
    let mut pairs = Vec::new();
    let mut overflowed = false;
    for (i, a) in aabbs.iter().enumerate() {
        for b in aabbs.get(i + 1..).unwrap_or(&[]) {
            if a.overlaps(b, tol) {
                if pairs.len() < MAX_OVERLAP_PAIRS as usize {
                    pairs.push(OverlapPair { a: a.id, b: b.id });
                } else {
                    overflowed = true;
                }
            }
        }
    }
    pairs.sort_unstable();
    CollisionResult { pairs, overflowed }
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
                bx.add(surf.eval(u, v).0);
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

/// Builds a flattened top-down median-split LBVH over `aabbs` sorted by `morton` order.
pub fn build_simple_lbvh(aabbs: &[GpuAabb], morton: &[GpuMortonItem]) -> Vec<GpuLbvhNode> {
    if aabbs.is_empty() || morton.is_empty() {
        return Vec::new();
    }
    let mut nodes = Vec::with_capacity(morton.len() * 2);
    build_lbvh_recursive(aabbs, morton, &mut nodes);
    nodes
}

/// Recursively constructs a subtree of [`GpuLbvhNode`]s over a Morton-sorted slice `span`.
fn build_lbvh_recursive(aabbs: &[GpuAabb], span: &[GpuMortonItem], nodes: &mut Vec<GpuLbvhNode>) -> u32 {
    let idx = nodes.len() as u32;
    nodes.push(GpuLbvhNode::default());

    if span.len() <= 1 {
        let prim_idx = span.first().map_or(0, |m| m.index as usize);
        let bx = aabbs.get(prim_idx).copied().unwrap_or_default();
        if let Some(slot) = nodes.get_mut(idx as usize) {
            *slot = GpuLbvhNode {
                min_x: bx.min_x,
                min_y: bx.min_y,
                min_z: bx.min_z,
                left_or_prim: bx.id,
                max_x: bx.max_x,
                max_y: bx.max_y,
                max_z: bx.max_z,
                right_or_flag: 0x8000_0000,
            };
        }
        return idx;
    }

    let mid = span.len() / 2;
    let left_slice = span.get(..mid).unwrap_or(&[]);
    let right_slice = span.get(mid..).unwrap_or(&[]);
    let left_child = build_lbvh_recursive(aabbs, left_slice, nodes);
    let right_child = build_lbvh_recursive(aabbs, right_slice, nodes);

    let l_node = nodes.get(left_child as usize).copied().unwrap_or_default();
    let r_node = nodes.get(right_child as usize).copied().unwrap_or_default();
    if let Some(slot) = nodes.get_mut(idx as usize) {
        *slot = GpuLbvhNode {
            min_x: l_node.min_x.min(r_node.min_x),
            min_y: l_node.min_y.min(r_node.min_y),
            min_z: l_node.min_z.min(r_node.min_z),
            left_or_prim: left_child,
            max_x: l_node.max_x.max(r_node.max_x),
            max_y: l_node.max_y.max(r_node.max_y),
            max_z: l_node.max_z.max(r_node.max_z),
            right_or_flag: right_child,
        };
    }
    idx
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
