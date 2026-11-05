//! Multi-Backend GPU Acceleration & Runtime Auto-Tuning Dispatcher.
//!
//! Coordinates execution across seven hardware/software execution tiers:
//! 1. [`BackendTarget::CpuScalar`] — Single-threaded deterministic Rust reference.
//! 2. [`BackendTarget::CpuRayon`] — Multi-core work-stealing `rayon` parallel CPU execution.
//! 3. [`BackendTarget::VulkanCompute`] — Cross-platform `wgpu` / Vulkan compute shaders (`accel.wgsl`, `collision.wgsl`).
//! 4. [`BackendTarget::CudaGeneric`] — Dynamically loaded CUDA Driver API (`libcuda.so.1`) running `sm_70` PTX.
//! 5. [`BackendTarget::AmperePtx`] — Handcrafted NVIDIA Ampere (`sm_86`) PTX (`cp.async.ca.shared.global`, `redux.sync`, warp shuffle).
//! 6. [`BackendTarget::AmpereTensorCore`] — Handcrafted Ampere FP64 Tensor Core DMMA (`mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64`).
//! 7. [`BackendTarget::RtCoreBvh`] — Hardware-aligned wide-BVH / RT traversal on GPU (`classify_points_rt_bvh`, `raycast_rt_bvh`).
//!
//! On startup / first use (`OnceLock`), [`Dispatcher::global`] runs a scaling microbenchmark across
//! problem sizes $N \in \{16, 64, 256, 1024, 4096, 16384, 65536\}$ to measure actual device dispatch
//! overhead and compute throughput on the host machine, building an empirical lookup table mapping
//! `(KernelOp, ProblemSize) -> BackendTarget`.

use std::fmt::Write as _;
use std::sync::{OnceLock, mpsc};
use std::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use solidcore_record::Vec3;
use solidcraft_geom::compare::closest_on_triangle;
use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, MeshMeasure, Vec3Geom};
use wgpu::util::DeviceExt;

use crate::collision::{
    CollisionGpu, CollisionResult, GpuAabb, GpuPod, OverlapPair, cpu_bipartite_overlap, decode_pod_slice, encode_pod, encode_pod_slice,
    synthetic_aabb_grid,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::cuda::CudaBackend;
use crate::cuda::{GpuBvhNode, GpuRay, GpuRayHit, GpuTri, PatchEvalOutF32, PatchEvalOutF64, PatchUvQueryF32};

const ACCEL_SHADER: &str = include_str!("accel.wgsl");

/// Standard problem sizes evaluated by the startup scaling microbenchmark.
pub const BENCH_SIZES: &[usize] = &[16, 64, 256, 1024, 4096, 16384, 65536];

/// Execution backend target selected by the runtime dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BackendTarget {
    /// Single-threaded CPU scalar reference implementation.
    CpuScalar,
    /// Multi-core `rayon` parallel CPU implementation.
    CpuRayon,
    /// Cross-platform Vulkan / `wgpu` compute pipeline.
    VulkanCompute,
    /// Generic CUDA Driver API PTX (`compute_70`).
    CudaGeneric,
    /// Handcrafted NVIDIA Ampere (`sm_86`) PTX (`cp.async`, `redux.sync`, warp shuffle).
    AmperePtx,
    /// Handcrafted NVIDIA Ampere (`sm_86`) FP64 Tensor Core DMMA (`mma.sync.aligned.m8n8k4...f64`).
    AmpereTensorCore,
    /// Hardware-aligned BVH / RT traversal on GPU.
    RtCoreBvh,
}

impl BackendTarget {
    /// Short human-readable label for tables and logs.
    pub fn name(self) -> &'static str {
        match self {
            Self::CpuScalar => "CPU Scalar",
            Self::CpuRayon => "CPU Rayon",
            Self::VulkanCompute => "Vulkan (wgpu)",
            Self::CudaGeneric => "CUDA Generic (sm_70)",
            Self::AmperePtx => "Ampere PTX (sm_86)",
            Self::AmpereTensorCore => "Ampere Tensor Core (DMMA f64)",
            Self::RtCoreBvh => "RT / GPU BVH Traversal",
        }
    }

    /// Whether this target executes on a GPU.
    pub fn is_gpu(self) -> bool {
        !matches!(self, Self::CpuScalar | Self::CpuRayon)
    }
}

/// Accelerated geometric kernel operation classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KernelOp {
    /// Batched `f32` bicubic NURBS / Bezier surface position & normal evaluation.
    SurfaceEvalF32,
    /// Batched `f64` double-precision bicubic patch & derivative tensor contraction (Ampere DMMA).
    TensorPatchEvalF64,
    /// Batched watertight 3-ray majority-vote point-in-solid classification (`Mesh::contains`).
    PointInSolid,
    /// Batched closest ray-mesh intersection (`Mesh::raycast` / `MeshIndex::raycast`).
    RaycastMesh,
    /// Bipartite AABB broadphase collision detection.
    AabbBroadphase,
    /// Batched minimum squared distance from query points to mesh triangles.
    PointMeshDistance,
    /// Divergence-theorem signed volume, surface area, and first-moment reduction.
    MassProperties,
}

impl KernelOp {
    /// All kernel operations calibrated by the dispatcher.
    pub const ALL: &[KernelOp] = &[
        Self::SurfaceEvalF32,
        Self::TensorPatchEvalF64,
        Self::PointInSolid,
        Self::RaycastMesh,
        Self::AabbBroadphase,
        Self::PointMeshDistance,
        Self::MassProperties,
    ];

    /// Human-readable operation name.
    pub fn name(self) -> &'static str {
        match self {
            Self::SurfaceEvalF32 => "Surface Patch Eval (f32)",
            Self::TensorPatchEvalF64 => "Bicubic Tensor Patch Eval (f64 DMMA)",
            Self::PointInSolid => "Point-in-Solid Classification (3-Ray)",
            Self::RaycastMesh => "Ray-Mesh Intersection (BVH)",
            Self::AabbBroadphase => "AABB Broadphase Overlap",
            Self::PointMeshDistance => "Point-to-Mesh Distance",
            Self::MassProperties => "Mass Properties Integration",
        }
    }
}

// ── Vulkan / wgpu Compute Accelerator (`accel.wgsl`) ──────────────────────

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct PatchEvalParams {
    num_evals: u32,
    num_patches: u32,
    pad0: u32,
    pad1: u32,
}

impl GpuPod for PatchEvalParams {
    const WORDS: usize = 4;
    /// Appends the 4 `u32` words of [`PatchEvalParams`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend_from_slice(&[self.num_evals, self.num_patches, self.pad0, self.pad1]);
    }
    /// Decodes [`PatchEvalParams`] from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { num_evals: *w.first()?, num_patches: *w.get(1)?, pad0: *w.get(2)?, pad1: *w.get(3)? })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct MeshQueryParams {
    num_queries: u32,
    num_tris: u32,
    num_bvh_nodes: u32,
    pad: u32,
}

impl GpuPod for MeshQueryParams {
    const WORDS: usize = 4;
    /// Appends the 4 `u32` words of [`MeshQueryParams`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend_from_slice(&[self.num_queries, self.num_tris, self.num_bvh_nodes, self.pad]);
    }
    /// Decodes [`MeshQueryParams`] from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { num_queries: *w.first()?, num_tris: *w.get(1)?, num_bvh_nodes: *w.get(2)?, pad: *w.get(3)? })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct MassReduceOut {
    v6_area: [f32; 4],
    moment1: [f32; 4],
}

impl GpuPod for MassReduceOut {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of [`MassReduceOut`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        self.v6_area.push_words(out);
        self.moment1.push_words(out);
    }
    /// Decodes [`MassReduceOut`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { v6_area: <[f32; 4]>::from_words(w.get(0..4)?)?, moment1: <[f32; 4]>::from_words(w.get(4..8)?)? })
    }
}

/// Shared Vulkan / `wgpu` compute pipelines for all 7 geometric kernel classes.
pub struct VulkanAccel {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    collision: CollisionGpu,
    patch_pipeline: wgpu::ComputePipeline,
    patch_bgl: wgpu::BindGroupLayout,
    classify_tiled_pipeline: wgpu::ComputePipeline,
    classify_bvh_pipeline: wgpu::ComputePipeline,
    raycast_bvh_pipeline: wgpu::ComputePipeline,
    bvh_query_bgl: wgpu::BindGroupLayout,
    dist_pipeline: wgpu::ComputePipeline,
    dist_bgl: wgpu::BindGroupLayout,
    mass_pipeline: wgpu::ComputePipeline,
    mass_bgl: wgpu::BindGroupLayout,
}

static GLOBAL_VULKAN: OnceLock<Option<VulkanAccel>> = OnceLock::new();

impl VulkanAccel {
    /// Returns the process-wide shared [`VulkanAccel`] instance if a `wgpu` adapter is available.
    pub fn global() -> Option<&'static Self> {
        GLOBAL_VULKAN.get_or_init(Self::new).as_ref()
    }

    /// Initializes a new [`VulkanAccel`] on the high-performance GPU adapter.
    pub fn new() -> Option<Self> {
        let (adapter, device, queue) = crate::open_headless().ok()?;
        let adapter_name = adapter.get_info().name;
        let collision = CollisionGpu::from_device(device.clone(), queue.clone());

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("solidcraft-accel-wgsl"),
            source: wgpu::ShaderSource::Wgsl(ACCEL_SHADER.into()),
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

        // 1. Patch Eval (bindings 0..=3)
        let patch_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("accel-patch-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, true), entry(3, false)],
        });
        let patch_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("accel-patch-pl"),
            bind_group_layouts: &[Some(&patch_bgl)],
            immediate_size: 0,
        });
        let patch_pipeline = make_pipeline(&patch_pl, "eval_bicubic_patches");

        // 2. Classify / Raycast with BVH (bindings 0..=4)
        let bvh_query_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("accel-bvh-query-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, true), entry(3, true), entry(4, false)],
        });
        let bvh_query_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("accel-bvh-query-pl"),
            bind_group_layouts: &[Some(&bvh_query_bgl)],
            immediate_size: 0,
        });
        let classify_tiled_pipeline = make_pipeline(&bvh_query_pl, "classify_points_mesh");
        let classify_bvh_pipeline = make_pipeline(&bvh_query_pl, "classify_points_bvh");
        let raycast_bvh_pipeline = make_pipeline(&bvh_query_pl, "raycast_mesh_bvh");

        // 3. Point-to-Mesh Distance (bindings 0..=3)
        let dist_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("accel-dist-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, true), entry(3, false)],
        });
        let dist_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("accel-dist-pl"),
            bind_group_layouts: &[Some(&dist_bgl)],
            immediate_size: 0,
        });
        let dist_pipeline = make_pipeline(&dist_pl, "point_mesh_distance");

        // 4. Mass Properties Reduction (bindings 0..=2)
        let mass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("accel-mass-bgl"),
            entries: &[entry(0, true), entry(1, true), entry(2, false)],
        });
        let mass_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("accel-mass-pl"),
            bind_group_layouts: &[Some(&mass_bgl)],
            immediate_size: 0,
        });
        let mass_pipeline = make_pipeline(&mass_pl, "mass_properties_reduce");

        Some(Self {
            device,
            queue,
            adapter_name,
            collision,
            patch_pipeline,
            patch_bgl,
            classify_tiled_pipeline,
            classify_bvh_pipeline,
            raycast_bvh_pipeline,
            bvh_query_bgl,
            dist_pipeline,
            dist_bgl,
            mass_pipeline,
            mass_bgl,
        })
    }

    /// Name of the underlying Vulkan / `wgpu` adapter.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Access the underlying [`CollisionGpu`] instance.
    pub fn collision(&self) -> &CollisionGpu {
        &self.collision
    }

    /// Evaluates a batch of `f32` bicubic Bezier/B-spline patches on Vulkan/wgpu.
    pub fn eval_bicubic_f32(&self, patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32]) -> Option<Vec<PatchEvalOutF32>> {
        if queries.is_empty() || patch_cpts.len() < 16 {
            return Some(Vec::new());
        }
        let num_evals = u32::try_from(queries.len()).ok()?;
        let num_patches = u32::try_from(patch_cpts.len() / 16).ok()?;
        let params = PatchEvalParams { num_evals, num_patches, pad0: 0, pad1: 0 };

        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-patch-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let cpts_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-patch-cpts"),
            contents: &encode_pod_slice(patch_cpts),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let q_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-patch-queries"),
            contents: &encode_pod_slice(queries),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (queries.len() * std::mem::size_of::<PatchEvalOutF32>()) as u64;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-patch-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-patch-read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vk-patch-bg"),
            layout: &self.patch_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: cpts_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: q_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: out_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vk-patch-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vk-patch-pass"), timestamp_writes: None });
            pass.set_pipeline(&self.patch_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(num_evals.div_ceil(64), 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_size);
        self.queue.submit(Some(enc.finish()));

        self.read_mapped_vec(&read_buf)
    }

    /// Classifies points (`[x, y, z, 0.0]`) against a closed mesh (either tiled or BVH traversal).
    /// Returns `None` when `use_bvh` is requested for non-empty `tris` with an empty `bvh_nodes` slice.
    pub fn classify_points(&self, points: &[[f32; 4]], tris: &[GpuTri], bvh_nodes: &[GpuBvhNode], use_bvh: bool) -> Option<Vec<bool>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![false; points.len()]);
        }
        if use_bvh && bvh_nodes.is_empty() {
            return None;
        }
        let num_queries = u32::try_from(points.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let num_bvh_nodes = u32::try_from(bvh_nodes.len()).ok()?;
        let params = MeshQueryParams { num_queries, num_tris, num_bvh_nodes, pad: 0 };

        let dummy_bvh = [GpuBvhNode::default()];
        let bvh_slice = if bvh_nodes.is_empty() { &dummy_bvh[..] } else { bvh_nodes };

        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-cls-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pts_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-cls-pts"),
            contents: &encode_pod_slice(points),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let tris_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-cls-tris"),
            contents: &encode_pod_slice(tris),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bvh_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-cls-bvh"),
            contents: &encode_pod_slice(bvh_slice),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (points.len() * std::mem::size_of::<u32>()) as u64;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-cls-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-cls-read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vk-cls-bg"),
            layout: &self.bvh_query_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: pts_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: tris_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: bvh_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: out_buf.as_entire_binding() },
            ],
        });

        let pipeline = if use_bvh { &self.classify_bvh_pipeline } else { &self.classify_tiled_pipeline };

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vk-cls-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vk-cls-pass"), timestamp_writes: None });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(num_queries.div_ceil(128), 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_size);
        self.queue.submit(Some(enc.finish()));

        let raw: Vec<u32> = self.read_mapped_vec(&read_buf)?;
        Some(raw.into_iter().map(|v| v != 0).collect())
    }

    /// Executes batched BVH-accelerated ray-mesh intersection on Vulkan/wgpu.
    /// Returns `None` if `tris` is non-empty and `bvh_nodes` is empty so the dispatcher can fall back.
    pub fn raycast_bvh(&self, rays: &[GpuRay], tris: &[GpuTri], bvh_nodes: &[GpuBvhNode]) -> Option<Vec<GpuRayHit>> {
        if rays.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![GpuRayHit { t: -1.0, tri_idx: u32::MAX, pad0: 0, pad1: 0 }; rays.len()]);
        }
        if bvh_nodes.is_empty() {
            return None;
        }
        let num_queries = u32::try_from(rays.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let num_bvh_nodes = u32::try_from(bvh_nodes.len()).ok()?;
        let params = MeshQueryParams { num_queries, num_tris, num_bvh_nodes, pad: 0 };

        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-rc-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let rays_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-rc-rays"),
            contents: &encode_pod_slice(rays),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let tris_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-rc-tris"),
            contents: &encode_pod_slice(tris),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bvh_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-rc-bvh"),
            contents: &encode_pod_slice(bvh_nodes),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (rays.len() * std::mem::size_of::<GpuRayHit>()) as u64;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-rc-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-rc-read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vk-rc-bg"),
            layout: &self.bvh_query_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: rays_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: tris_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: bvh_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: out_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vk-rc-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vk-rc-pass"), timestamp_writes: None });
            pass.set_pipeline(&self.raycast_bvh_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(num_queries.div_ceil(128), 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_size);
        self.queue.submit(Some(enc.finish()));

        self.read_mapped_vec(&read_buf)
    }

    /// Computes minimum squared distances from `points` to `tris` on Vulkan/wgpu.
    pub fn point_mesh_distance(&self, points: &[[f32; 4]], tris: &[GpuTri]) -> Option<Vec<f32>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![f32::INFINITY; points.len()]);
        }
        let num_queries = u32::try_from(points.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let params = MeshQueryParams { num_queries, num_tris, num_bvh_nodes: 0, pad: 0 };

        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-dist-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pts_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-dist-pts"),
            contents: &encode_pod_slice(points),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let tris_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-dist-tris"),
            contents: &encode_pod_slice(tris),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (points.len() * std::mem::size_of::<f32>()) as u64;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-dist-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-dist-read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vk-dist-bg"),
            layout: &self.dist_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: pts_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: tris_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: out_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vk-dist-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vk-dist-pass"), timestamp_writes: None });
            pass.set_pipeline(&self.dist_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(num_queries.div_ceil(128), 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_size);
        self.queue.submit(Some(enc.finish()));

        self.read_mapped_vec(&read_buf)
    }

    /// Computes divergence-theorem volume, surface area, and centroid on Vulkan/wgpu.
    pub fn mass_properties(&self, tris: &[GpuTri]) -> Option<MeshMeasure> {
        if tris.is_empty() {
            return Some(MeshMeasure::default());
        }
        let num_tris = u32::try_from(tris.len()).ok()?;
        let num_groups = num_tris.div_ceil(256);
        let params = MeshQueryParams { num_queries: 0, num_tris, num_bvh_nodes: 0, pad: 0 };

        let u_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-mass-params"),
            contents: &encode_pod(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let tris_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vk-mass-tris"),
            contents: &encode_pod_slice(tris),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out_size = (num_groups as usize * std::mem::size_of::<MassReduceOut>()) as u64;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-mass-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vk-mass-read"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vk-mass-bg"),
            layout: &self.mass_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: u_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tris_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: out_buf.as_entire_binding() },
            ],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vk-mass-enc") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vk-mass-pass"), timestamp_writes: None });
            pass.set_pipeline(&self.mass_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(num_groups, 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_size);
        self.queue.submit(Some(enc.finish()));

        let partials: Vec<MassReduceOut> = self.read_mapped_vec(&read_buf)?;
        let p0 = Vec3::new(tris[0].v0[0] as f64, tris[0].v0[1] as f64, tris[0].v0[2] as f64);
        let (mut v6, mut area, mut mx, mut my, mut mz) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for p in &partials {
            v6 += p.v6_area[0] as f64;
            area += p.v6_area[1] as f64;
            mx += p.moment1[0] as f64;
            my += p.moment1[1] as f64;
            mz += p.moment1[2] as f64;
        }
        let volume = v6 / 6.0;
        let centroid = if v6.abs() > 1e-12 { p0 + Vec3::new(mx / (4.0 * v6), my / (4.0 * v6), mz / (4.0 * v6)) } else { Vec3::ZERO };
        Some(MeshMeasure { volume, area, centroid })
    }

    /// Maps a staging readback buffer synchronously and decodes its contents into a `Vec<T>`.
    fn read_mapped_vec<T: GpuPod>(&self, read_buf: &wgpu::Buffer) -> Option<Vec<T>> {
        let slice = read_buf.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let vec: Vec<T> = decode_pod_slice(&mapped);
        drop(mapped);
        read_buf.unmap();
        Some(vec)
    }
}

// ── GPU BVH Builder for Raycast & Point-in-Solid ──────────────────────────

/// Converts a [`Mesh`] into packed [`GpuTri`]s and a flattened [`GpuBvhNode`] hierarchy.
pub fn build_gpu_bvh_from_mesh(mesh: &Mesh) -> (Vec<GpuTri>, Vec<GpuBvhNode>) {
    let mut raw_tris = Vec::with_capacity(mesh.triangles.len());
    for (idx, t) in mesh.triangles.iter().enumerate() {
        let Some([a, b, c]) = mesh.tri(t) else { continue };
        raw_tris.push(GpuTri {
            v0: [a.x as f32, a.y as f32, a.z as f32, f32::from_bits(idx as u32)],
            v1: [b.x as f32, b.y as f32, b.z as f32, 0.0],
            v2: [c.x as f32, c.y as f32, c.z as f32, 0.0],
        });
    }
    build_gpu_bvh(&raw_tris)
}

/// Builds a flattened top-down median-split BVH over `tris`, returning `(ordered_tris, nodes)`.
pub fn build_gpu_bvh(tris: &[GpuTri]) -> (Vec<GpuTri>, Vec<GpuBvhNode>) {
    if tris.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut order: Vec<usize> = (0..tris.len()).collect();
    let mut ordered_tris = Vec::with_capacity(tris.len());
    let mut nodes = Vec::with_capacity(tris.len() * 2);
    build_bvh_node(tris, &mut order, &mut ordered_tris, &mut nodes);
    (ordered_tris, nodes)
}

/// Returns the padded `(min, max, centroid)` bounding box of a single [`GpuTri`].
fn tri_bounds_center(t: &GpuTri) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let min_x = t.v0[0].min(t.v1[0]).min(t.v2[0]) - 1e-6;
    let min_y = t.v0[1].min(t.v1[1]).min(t.v2[1]) - 1e-6;
    let min_z = t.v0[2].min(t.v1[2]).min(t.v2[2]) - 1e-6;
    let max_x = t.v0[0].max(t.v1[0]).max(t.v2[0]) + 1e-6;
    let max_y = t.v0[1].max(t.v1[1]).max(t.v2[1]) + 1e-6;
    let max_z = t.v0[2].max(t.v1[2]).max(t.v2[2]) + 1e-6;
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let cz = (min_z + max_z) * 0.5;
    ([min_x, min_y, min_z], [max_x, max_y, max_z], [cx, cy, cz])
}

/// Recursively partitions `span` along its longest centroid extent to populate `nodes` and `ordered_tris`.
fn build_bvh_node(tris: &[GpuTri], span: &mut [usize], ordered_tris: &mut Vec<GpuTri>, nodes: &mut Vec<GpuBvhNode>) -> u32 {
    let node_idx = nodes.len() as u32;
    nodes.push(GpuBvhNode::default());

    let (mut bmin, mut bmax) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let (mut cmin, mut cmax) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for &idx in span.iter() {
        let Some(t) = tris.get(idx) else { continue };
        let (mn, mx, c) = tri_bounds_center(t);
        for k in 0..3 {
            bmin[k] = bmin[k].min(mn[k]);
            bmax[k] = bmax[k].max(mx[k]);
            cmin[k] = cmin[k].min(c[k]);
            cmax[k] = cmax[k].max(c[k]);
        }
    }

    let extent = [cmax[0] - cmin[0], cmax[1] - cmin[1], cmax[2] - cmin[2]];
    let axis = if extent[0] >= extent[1] && extent[0] >= extent[2] {
        0
    } else if extent[1] >= extent[2] {
        1
    } else {
        2
    };

    if span.len() <= 4 || extent[axis] <= 1e-7 {
        let first = ordered_tris.len() as u32;
        for &idx in span.iter() {
            if let Some(t) = tris.get(idx) {
                ordered_tris.push(*t);
            }
        }
        let count = (ordered_tris.len() as u32).saturating_sub(first);
        if let Some(slot) = nodes.get_mut(node_idx as usize) {
            *slot = GpuBvhNode {
                min_x: bmin[0],
                min_y: bmin[1],
                min_z: bmin[2],
                left_or_prim: first,
                max_x: bmax[0],
                max_y: bmax[1],
                max_z: bmax[2],
                right_or_count: 0x8000_0000 | count,
            };
        }
        return node_idx;
    }

    let mid = span.len() / 2;
    span.select_nth_unstable_by(mid, |&a, &b| {
        let ca = tris.get(a).map_or(0.0, |t| tri_bounds_center(t).2[axis]);
        let cb = tris.get(b).map_or(0.0, |t| tri_bounds_center(t).2[axis]);
        ca.total_cmp(&cb)
    });
    let (left_span, right_span) = span.split_at_mut(mid);
    let left_child = build_bvh_node(tris, left_span, ordered_tris, nodes);
    let right_child = build_bvh_node(tris, right_span, ordered_tris, nodes);

    if let Some(slot) = nodes.get_mut(node_idx as usize) {
        *slot = GpuBvhNode {
            min_x: bmin[0],
            min_y: bmin[1],
            min_z: bmin[2],
            left_or_prim: left_child,
            max_x: bmax[0],
            max_y: bmax[1],
            max_z: bmax[2],
            right_or_count: right_child,
        };
    }
    node_idx
}

// ── CPU Scalar & Rayon Reference Implementations ──────────────────────────

/// Evaluates a single `f32` rational bicubic Bezier patch at `(q.u, q.v)`.
fn eval_one_bicubic_f32(patch_cpts: &[[f32; 4]], num_patches: u32, q: PatchUvQueryF32) -> PatchEvalOutF32 {
    let p_idx = q.patch_idx.min(num_patches.saturating_sub(1)) as usize;
    let cpts = patch_cpts.get(p_idx * 16..(p_idx + 1) * 16).unwrap_or(&[]);
    if cpts.len() < 16 {
        return PatchEvalOutF32::default();
    }
    let (u, v) = (q.u, q.v);
    let (iu, iv) = (1.0 - u, 1.0 - v);
    let (iu2, u2, iv2, v2) = (iu * iu, u * u, iv * iv, v * v);
    let bu = [iu2 * iu, 3.0 * u * iu2, 3.0 * u2 * iu, u2 * u];
    let dbu = [-3.0 * iu2, 3.0 * iu * (1.0 - 3.0 * u), 3.0 * u * (2.0 - 3.0 * u), 3.0 * u2];
    let bv = [iv2 * iv, 3.0 * v * iv2, 3.0 * v2 * iv, v2 * v];
    let dbv = [-3.0 * iv2, 3.0 * iv * (1.0 - 3.0 * v), 3.0 * v * (2.0 - 3.0 * v), 3.0 * v2];

    let mut h = [0.0f32; 4];
    let mut dhu = [0.0f32; 4];
    let mut dhv = [0.0f32; 4];
    for i in 0..4 {
        for j in 0..4 {
            let cp = cpts[i * 4 + j];
            let w_uv = bu[i] * bv[j];
            let w_du = dbu[i] * bv[j];
            let w_dv = bu[i] * dbv[j];
            for c in 0..4 {
                h[c] = cp[c].mul_add(w_uv, h[c]);
                dhu[c] = cp[c].mul_add(w_du, dhu[c]);
                dhv[c] = cp[c].mul_add(w_dv, dhv[c]);
            }
        }
    }
    let inv_w = if h[3].abs() > 1e-12 { 1.0 / h[3] } else { 1.0 };
    let (px, py, pz) = (h[0] * inv_w, h[1] * inv_w, h[2] * inv_w);
    let dux = (dhu[0] - px * dhu[3]) * inv_w;
    let duy = (dhu[1] - py * dhu[3]) * inv_w;
    let duz = (dhu[2] - pz * dhu[3]) * inv_w;
    let dvx = (dhv[0] - px * dhv[3]) * inv_w;
    let dvy = (dhv[1] - py * dhv[3]) * inv_w;
    let dvz = (dhv[2] - pz * dhv[3]) * inv_w;
    let cx = duy * dvz - duz * dvy;
    let cy = duz * dvx - dux * dvz;
    let cz = dux * dvy - duy * dvx;
    let len_n = (cx * cx + cy * cy + cz * cz).sqrt();
    let (nx, ny, nz) = if len_n > 1e-12 { (cx / len_n, cy / len_n, cz / len_n) } else { (0.0, 0.0, 1.0) };
    PatchEvalOutF32 { pos: [px, py, pz, 1.0], norm: [nx, ny, nz, len_n] }
}

/// CPU scalar evaluation of `f32` bicubic patches.
pub fn cpu_eval_bicubic_f32_scalar(patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32]) -> Vec<PatchEvalOutF32> {
    let num_patches = (patch_cpts.len() / 16) as u32;
    queries.iter().map(|&q| eval_one_bicubic_f32(patch_cpts, num_patches, q)).collect()
}

/// CPU `rayon` parallel evaluation of `f32` bicubic patches.
pub fn cpu_eval_bicubic_f32_rayon(patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32]) -> Vec<PatchEvalOutF32> {
    let num_patches = (patch_cpts.len() / 16) as u32;
    #[cfg(not(target_arch = "wasm32"))]
    {
        queries.par_iter().map(|&q| eval_one_bicubic_f32(patch_cpts, num_patches, q)).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_eval_bicubic_f32_scalar(patch_cpts, queries)
    }
}

/// Evaluates a single `f64` double-precision rational bicubic Bezier patch at `(q[0], q[1])`.
fn eval_one_bicubic_f64(patch_cpts: &[[f64; 4]], num_patches: usize, q: [f64; 4]) -> PatchEvalOutF64 {
    let p_idx = (q[2] as usize).min(num_patches.saturating_sub(1));
    let cpts = patch_cpts.get(p_idx * 16..(p_idx + 1) * 16).unwrap_or(&[]);
    if cpts.len() < 16 {
        return PatchEvalOutF64::default();
    }
    let (u, v) = (q[0], q[1]);
    let (iu, iv) = (1.0 - u, 1.0 - v);
    let (iu2, u2, iv2, v2) = (iu * iu, u * u, iv * iv, v * v);
    let bu = [iu2 * iu, 3.0 * u * iu2, 3.0 * u2 * iu, u2 * u];
    let dbu = [-3.0 * iu2, 3.0 * iu * (1.0 - 3.0 * u), 3.0 * u * (2.0 - 3.0 * u), 3.0 * u2];
    let bv = [iv2 * iv, 3.0 * v * iv2, 3.0 * v2 * iv, v2 * v];
    let dbv = [-3.0 * iv2, 3.0 * iv * (1.0 - 3.0 * v), 3.0 * v * (2.0 - 3.0 * v), 3.0 * v2];

    let mut h = [0.0f64; 4];
    let mut dhu = [0.0f64; 4];
    let mut dhv = [0.0f64; 4];
    for i in 0..4 {
        for j in 0..4 {
            let cp = cpts[i * 4 + j];
            let w_uv = bu[i] * bv[j];
            let w_du = dbu[i] * bv[j];
            let w_dv = bu[i] * dbv[j];
            for c in 0..4 {
                h[c] += cp[c] * w_uv;
                dhu[c] += cp[c] * w_du;
                dhv[c] += cp[c] * w_dv;
            }
        }
    }
    let inv_w = if h[3].abs() > 1e-15 { 1.0 / h[3] } else { 1.0 };
    let (px, py, pz) = (h[0] * inv_w, h[1] * inv_w, h[2] * inv_w);
    let dux = (dhu[0] - px * dhu[3]) * inv_w;
    let duy = (dhu[1] - py * dhu[3]) * inv_w;
    let duz = (dhu[2] - pz * dhu[3]) * inv_w;
    let dvx = (dhv[0] - px * dhv[3]) * inv_w;
    let dvy = (dhv[1] - py * dhv[3]) * inv_w;
    let dvz = (dhv[2] - pz * dhv[3]) * inv_w;
    let cx = duy * dvz - duz * dvy;
    let cy = duz * dvx - dux * dvz;
    let cz = dux * dvy - duy * dvx;
    let len_n = (cx * cx + cy * cy + cz * cz).sqrt();
    let (nx, ny, nz) = if len_n > 1e-15 { (cx / len_n, cy / len_n, cz / len_n) } else { (0.0, 0.0, 1.0) };
    PatchEvalOutF64 { pos: [px, py, pz, 1.0], norm: [nx, ny, nz, len_n] }
}

/// CPU scalar evaluation of `f64` double-precision bicubic patches.
pub fn cpu_eval_bicubic_f64_scalar(patch_cpts: &[[f64; 4]], queries: &[[f64; 4]]) -> Vec<PatchEvalOutF64> {
    let num_patches = patch_cpts.len() / 16;
    queries.iter().map(|&q| eval_one_bicubic_f64(patch_cpts, num_patches, q)).collect()
}

/// CPU `rayon` parallel evaluation of `f64` double-precision bicubic patches.
pub fn cpu_eval_bicubic_f64_rayon(patch_cpts: &[[f64; 4]], queries: &[[f64; 4]]) -> Vec<PatchEvalOutF64> {
    let num_patches = patch_cpts.len() / 16;
    #[cfg(not(target_arch = "wasm32"))]
    {
        queries.par_iter().map(|&q| eval_one_bicubic_f64(patch_cpts, num_patches, q)).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_eval_bicubic_f64_scalar(patch_cpts, queries)
    }
}

/// Watertight Plucker-style ray-triangle crossing predicate in `f32` with lexicographic edge tie-breaking.
fn ray_crosses_f32(a: [f32; 3], b: [f32; 3], c: [f32; 3], o: [f32; 3], d: [f32; 3]) -> bool {
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let along = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
    if along == 0.0 {
        return false;
    }
    let pa = [a[0] - o[0], a[1] - o[1], a[2] - o[2]];
    let pb = [b[0] - o[0], b[1] - o[1], b[2] - o[2]];
    let pc = [c[0] - o[0], c[1] - o[1], c[2] - o[2]];
    let signed_vol = |p: [f32; 3], q: [f32; 3], vp: [f32; 3], vq: [f32; 3]| -> bool {
        let cr = [vp[1] * vq[2] - vp[2] * vq[1], vp[2] * vq[0] - vp[0] * vq[2], vp[0] * vq[1] - vp[1] * vq[0]];
        let v = d[0] * cr[0] + d[1] * cr[1] + d[2] * cr[2];
        if v != 0.0 { v > 0.0 } else { (p[0], p[1], p[2]) < (q[0], q[1], q[2]) }
    };
    let s_ab = signed_vol(a, b, pa, pb);
    let s_bc = signed_vol(b, c, pb, pc);
    let s_ca = signed_vol(c, a, pc, pa);
    if s_ab != s_bc || s_ab != s_ca {
        return false;
    }
    ((pa[0] * n[0] + pa[1] * n[1] + pa[2] * n[2]) / along) > 0.0
}

/// Classifies a single 3D point `p` against `tris` (using `bvh` when non-empty) via 3-ray majority vote.
fn classify_one_point(p: [f32; 3], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> bool {
    let dirs: &[[f32; 3]] = &[[0.5773, 0.5774, 0.5776], [-0.6123, 0.3141, 0.7254], [0.2718, -0.8414, 0.4673]];
    let mut votes = 0u32;
    if !bvh.is_empty() {
        for &d in dirs {
            let inv_d = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
            let mut crossings = 0u32;
            let mut stack = [0u32; 64];
            let mut sp = 1usize;
            while sp > 0 {
                sp -= 1;
                let Some(node) = bvh.get(stack[sp] as usize) else { continue };
                if ray_aabb_f32(p, inv_d, node) < 0.0 {
                    continue;
                }
                if (node.right_or_count & 0x8000_0000) != 0 {
                    let first = node.left_or_prim as usize;
                    let count = (node.right_or_count & 0x7FFF_FFFF) as usize;
                    for t in tris.get(first..first + count).unwrap_or(&[]) {
                        let a = [t.v0[0], t.v0[1], t.v0[2]];
                        let b = [t.v1[0], t.v1[1], t.v1[2]];
                        let c = [t.v2[0], t.v2[1], t.v2[2]];
                        if ray_crosses_f32(a, b, c, p, d) {
                            crossings += 1;
                        }
                    }
                } else if sp + 2 <= 64 {
                    stack[sp] = node.right_or_count;
                    stack[sp + 1] = node.left_or_prim;
                    sp += 2;
                }
            }
            votes += crossings & 1;
        }
    } else {
        for &d in dirs {
            let mut crossings = 0u32;
            for t in tris {
                let a = [t.v0[0], t.v0[1], t.v0[2]];
                let b = [t.v1[0], t.v1[1], t.v1[2]];
                let c = [t.v2[0], t.v2[1], t.v2[2]];
                if ray_crosses_f32(a, b, c, p, d) {
                    crossings += 1;
                }
            }
            votes += crossings & 1;
        }
    }
    votes >= 2
}

/// CPU scalar point-in-solid classification.
pub fn cpu_classify_points_scalar(points: &[[f32; 4]], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> Vec<bool> {
    points.iter().map(|p| classify_one_point([p[0], p[1], p[2]], tris, bvh)).collect()
}

/// CPU `rayon` parallel point-in-solid classification.
pub fn cpu_classify_points_rayon(points: &[[f32; 4]], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> Vec<bool> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        points.par_iter().map(|p| classify_one_point([p[0], p[1], p[2]], tris, bvh)).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_classify_points_scalar(points, tris, bvh)
    }
}

/// Slab test returning the non-negative entry distance of a ray `(o, inv_d)` into BVH node `n`, or `-1.0` on miss.
fn ray_aabb_f32(o: [f32; 3], inv_d: [f32; 3], n: &GpuBvhNode) -> f32 {
    let tx1 = (n.min_x - o[0]) * inv_d[0];
    let tx2 = (n.max_x - o[0]) * inv_d[0];
    let mut tmin = tx1.min(tx2);
    let mut tmax = tx1.max(tx2);

    let ty1 = (n.min_y - o[1]) * inv_d[1];
    let ty2 = (n.max_y - o[1]) * inv_d[1];
    tmin = tmin.max(ty1.min(ty2));
    tmax = tmax.min(ty1.max(ty2));

    let tz1 = (n.min_z - o[2]) * inv_d[2];
    let tz2 = (n.max_z - o[2]) * inv_d[2];
    tmin = tmin.max(tz1.min(tz2));
    tmax = tmax.min(tz1.max(tz2));

    let t_enter = tmin.max(0.0);
    if tmax >= t_enter { t_enter } else { -1.0 }
}

/// Moller-Trumbore ray-triangle intersection in `f32`, returning hit parameter `t > 1e-7` or `-1.0` on miss.
fn ray_hit_tri_f32(a: [f32; 3], b: [f32; 3], c: [f32; 3], o: [f32; 3], d: [f32; 3]) -> f32 {
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let h = [d[1] * e2[2] - d[2] * e2[1], d[2] * e2[0] - d[0] * e2[2], d[0] * e2[1] - d[1] * e2[0]];
    let det = e1[0] * h[0] + e1[1] * h[1] + e1[2] * h[2];
    if det.abs() < 1e-12 {
        return -1.0;
    }
    let inv_det = 1.0 / det;
    let s = [o[0] - a[0], o[1] - a[1], o[2] - a[2]];
    let u = inv_det * (s[0] * h[0] + s[1] * h[1] + s[2] * h[2]);
    if !(0.0..=1.0).contains(&u) {
        return -1.0;
    }
    let q = [s[1] * e1[2] - s[2] * e1[1], s[2] * e1[0] - s[0] * e1[2], s[0] * e1[1] - s[1] * e1[0]];
    let v = inv_det * (d[0] * q[0] + d[1] * q[1] + d[2] * q[2]);
    if v < 0.0 || u + v > 1.0 {
        return -1.0;
    }
    let t = inv_det * (e2[0] * q[0] + e2[1] * q[1] + e2[2] * q[2]);
    if t > 1e-7 { t } else { -1.0 }
}

/// Traces a single [`GpuRay`] through `bvh` (or brute-force over `tris` when `bvh` is empty).
fn raycast_one_bvh(ray: &GpuRay, tris: &[GpuTri], bvh: &[GpuBvhNode]) -> GpuRayHit {
    let o = [ray.origin[0], ray.origin[1], ray.origin[2]];
    let d = [ray.dir[0], ray.dir[1], ray.dir[2]];
    let inv_d = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
    let mut best_t = 1e30f32;
    let mut best_tri = 0xFFFF_FFFFu32;

    if !bvh.is_empty() {
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let Some(node) = bvh.get(stack[sp] as usize) else { continue };
            let t_box = ray_aabb_f32(o, inv_d, node);
            if t_box < 0.0 || t_box > best_t {
                continue;
            }
            if (node.right_or_count & 0x8000_0000) != 0 {
                let first = node.left_or_prim as usize;
                let count = (node.right_or_count & 0x7FFF_FFFF) as usize;
                for t in tris.get(first..first + count).unwrap_or(&[]) {
                    let a = [t.v0[0], t.v0[1], t.v0[2]];
                    let b = [t.v1[0], t.v1[1], t.v1[2]];
                    let c = [t.v2[0], t.v2[1], t.v2[2]];
                    let th = ray_hit_tri_f32(a, b, c, o, d);
                    let orig_idx = t.v0[3].to_bits();
                    if th > 0.0 && (th < best_t || (th == best_t && orig_idx < best_tri)) {
                        best_t = th;
                        best_tri = orig_idx;
                    }
                }
            } else if sp + 2 <= 64 {
                let l = node.left_or_prim;
                let r = node.right_or_count;
                let tl = bvh.get(l as usize).map_or(-1.0, |n| ray_aabb_f32(o, inv_d, n));
                let tr = bvh.get(r as usize).map_or(-1.0, |n| ray_aabb_f32(o, inv_d, n));
                if tl >= 0.0 && tr >= 0.0 {
                    if tl <= tr {
                        stack[sp] = r;
                        stack[sp + 1] = l;
                    } else {
                        stack[sp] = l;
                        stack[sp + 1] = r;
                    }
                    sp += 2;
                } else if tl >= 0.0 {
                    stack[sp] = l;
                    sp += 1;
                } else if tr >= 0.0 {
                    stack[sp] = r;
                    sp += 1;
                }
            }
        }
    } else {
        for t in tris {
            let a = [t.v0[0], t.v0[1], t.v0[2]];
            let b = [t.v1[0], t.v1[1], t.v1[2]];
            let c = [t.v2[0], t.v2[1], t.v2[2]];
            let th = ray_hit_tri_f32(a, b, c, o, d);
            let orig_idx = t.v0[3].to_bits();
            if th > 0.0 && (th < best_t || (th == best_t && orig_idx < best_tri)) {
                best_t = th;
                best_tri = orig_idx;
            }
        }
    }

    GpuRayHit { t: if best_tri != 0xFFFF_FFFF { best_t } else { -1.0 }, tri_idx: best_tri, pad0: 0, pad1: 0 }
}

/// CPU scalar BVH raycast.
pub fn cpu_raycast_bvh_scalar(rays: &[GpuRay], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> Vec<GpuRayHit> {
    rays.iter().map(|r| raycast_one_bvh(r, tris, bvh)).collect()
}

/// CPU `rayon` parallel BVH raycast.
pub fn cpu_raycast_bvh_rayon(rays: &[GpuRay], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> Vec<GpuRayHit> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rays.par_iter().map(|r| raycast_one_bvh(r, tris, bvh)).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_raycast_bvh_scalar(rays, tris, bvh)
    }
}

/// CPU `rayon` parallel bipartite AABB overlap.
pub fn cpu_bipartite_overlap_rayon(aabbs_a: &[GpuAabb], aabbs_b: &[GpuAabb], tolerance: f32) -> CollisionResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tol = tolerance.max(0.0);
        let mut pairs: Vec<OverlapPair> = aabbs_a
            .par_iter()
            .flat_map_iter(|a| aabbs_b.iter().filter(move |b| a.overlaps(b, tol)).map(move |b| OverlapPair { a: a.id, b: b.id }))
            .collect();
        let overflowed = pairs.len() > crate::collision::MAX_OVERLAP_PAIRS as usize;
        pairs.truncate(crate::collision::MAX_OVERLAP_PAIRS as usize);
        pairs.sort_unstable();
        CollisionResult { pairs, overflowed }
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_bipartite_overlap(aabbs_a, aabbs_b, tolerance)
    }
}

/// Computes the minimum squared distance from a single point `p` to all triangles in `tris`.
fn point_mesh_dist_sq_one(p: [f32; 4], tris: &[GpuTri]) -> f32 {
    let pv = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
    let mut best = f64::INFINITY;
    for t in tris {
        let a = Vec3::new(t.v0[0] as f64, t.v0[1] as f64, t.v0[2] as f64);
        let b = Vec3::new(t.v1[0] as f64, t.v1[1] as f64, t.v1[2] as f64);
        let c = Vec3::new(t.v2[0] as f64, t.v2[1] as f64, t.v2[2] as f64);
        let q = closest_on_triangle(pv, a, b, c);
        let d2 = (q - pv).len2();
        if d2 < best {
            best = d2;
        }
    }
    best as f32
}

/// CPU scalar minimum squared distance from points to mesh triangles.
pub fn cpu_point_mesh_distance_scalar(points: &[[f32; 4]], tris: &[GpuTri]) -> Vec<f32> {
    points.iter().map(|&p| point_mesh_dist_sq_one(p, tris)).collect()
}

/// CPU `rayon` parallel minimum squared distance from points to mesh triangles.
pub fn cpu_point_mesh_distance_rayon(points: &[[f32; 4]], tris: &[GpuTri]) -> Vec<f32> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        points.par_iter().map(|&p| point_mesh_dist_sq_one(p, tris)).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_point_mesh_distance_scalar(points, tris)
    }
}

/// CPU scalar divergence-theorem mass properties (`MeshMeasure`).
pub fn cpu_mass_properties_scalar(tris: &[GpuTri]) -> MeshMeasure {
    let (mut v6, mut area, mut c) = (0.0f64, 0.0f64, Vec3::ZERO);
    for t in tris {
        let a = Vec3::new(t.v0[0] as f64, t.v0[1] as f64, t.v0[2] as f64);
        let b = Vec3::new(t.v1[0] as f64, t.v1[1] as f64, t.v1[2] as f64);
        let cc = Vec3::new(t.v2[0] as f64, t.v2[1] as f64, t.v2[2] as f64);
        let d = a.dot(b.cross(cc));
        v6 += d;
        c += (a + b + cc) * d;
        area += (b - a).cross(cc - a).len() * 0.5;
    }
    let volume = v6 / 6.0;
    let centroid = if v6.abs() > 1e-12 { c / (4.0 * v6) } else { Vec3::ZERO };
    MeshMeasure { volume, area, centroid }
}

/// CPU `rayon` parallel divergence-theorem mass properties (`MeshMeasure`).
pub fn cpu_mass_properties_rayon(tris: &[GpuTri]) -> MeshMeasure {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (v6, area, c) = tris
            .par_chunks(256)
            .map(|chunk| {
                let (mut lv6, mut larea, mut lc) = (0.0f64, 0.0f64, Vec3::ZERO);
                for t in chunk {
                    let a = Vec3::new(t.v0[0] as f64, t.v0[1] as f64, t.v0[2] as f64);
                    let b = Vec3::new(t.v1[0] as f64, t.v1[1] as f64, t.v1[2] as f64);
                    let cc = Vec3::new(t.v2[0] as f64, t.v2[1] as f64, t.v2[2] as f64);
                    let d = a.dot(b.cross(cc));
                    lv6 += d;
                    lc += (a + b + cc) * d;
                    larea += (b - a).cross(cc - a).len() * 0.5;
                }
                (lv6, larea, lc)
            })
            .reduce(|| (0.0, 0.0, Vec3::ZERO), |(v1, a1, c1), (v2, a2, c2)| (v1 + v2, a1 + a2, c1 + c2));
        let volume = v6 / 6.0;
        let centroid = if v6.abs() > 1e-12 { c / (4.0 * v6) } else { Vec3::ZERO };
        MeshMeasure { volume, area, centroid }
    }
    #[cfg(target_arch = "wasm32")]
    {
        cpu_mass_properties_scalar(tris)
    }
}

// ── Calibration Data Structures & Scaling Microbenchmark ──────────────────

/// Number of rotated-order timing samples collected per `(KernelOp, problem_size)` bucket.
pub const CALIBRATION_SAMPLES: usize = 3;

/// Measured median performance for a single `(KernelOp, problem_size, BackendTarget)` point.
#[derive(Clone, Debug)]
pub struct BackendTiming {
    pub backend: BackendTarget,
    pub latency_ns: u64,
    pub throughput_melems_per_sec: f64,
}

/// Measured performance across all available backends at a single problem size $N$.
#[derive(Clone, Debug)]
pub struct SizeBucketCalibration {
    pub problem_size: usize,
    /// Median latency and throughput per available backend (in canonical backend order).
    pub timings: Vec<BackendTiming>,
    /// Execution order of candidate backends for each sample pass (`0..CALIBRATION_SAMPLES`).
    pub sample_orders: Vec<Vec<BackendTarget>>,
    /// Raw per-sample timings `(backend, latency_ns)` recorded in each sample's rotated execution order.
    pub sample_timings_ns: Vec<Vec<(BackendTarget, u64)>>,
    pub fastest_backend: BackendTarget,
    pub fastest_latency_ns: u64,
    pub speedup_vs_scalar: f64,
}

/// Complete scaling profile and empirical crossover thresholds for one [`KernelOp`].
#[derive(Clone, Debug)]
pub struct KernelCalibration {
    pub op: KernelOp,
    pub buckets: Vec<SizeBucketCalibration>,
    /// Smallest problem size where `CpuRayon` beats `CpuScalar` (`None` if never).
    pub rayon_crossover_size: Option<usize>,
    /// Smallest problem size where a GPU backend beats both `CpuScalar` and `CpuRayon` (`None` if never).
    pub gpu_crossover_size: Option<usize>,
    /// Highest-throughput GPU backend at the largest calibrated size ($N = 65536$).
    pub preferred_gpu_backend: Option<BackendTarget>,
}

impl KernelCalibration {
    /// Selects the empirically fastest [`BackendTarget`] for `problem_size` by piecewise bucket lookup.
    pub fn select_backend(&self, problem_size: usize) -> BackendTarget {
        if self.buckets.is_empty() {
            return BackendTarget::CpuScalar;
        }
        // Find the nearest calibrated bucket >= problem_size, or the largest bucket if above 65536.
        for b in &self.buckets {
            if problem_size <= b.problem_size {
                return b.fastest_backend;
            }
        }
        self.buckets.last().map_or(BackendTarget::CpuScalar, |b| b.fastest_backend)
    }
}

/// Full hardware discovery and scaling microbenchmark report across all 7 kernel classes.
#[derive(Clone, Debug)]
pub struct CalibrationReport {
    pub cpu_threads: usize,
    pub vulkan_adapter: Option<String>,
    pub cuda_primary_device: Option<String>,
    pub cuda_all_devices: Vec<String>,
    pub has_ampere_ptx: bool,
    pub has_fp64_tensor_core: bool,
    pub kernels: Vec<KernelCalibration>,
    pub total_calibration_ms: f64,
}

impl CalibrationReport {
    /// Looks up the calibration profile for `op`.
    pub fn kernel(&self, op: KernelOp) -> Option<&KernelCalibration> {
        self.kernels.iter().find(|k| k.op == op)
    }

    /// Formats the complete calibration report as a GitHub-Flavored Markdown document.
    pub fn to_markdown(&self) -> String {
        let mut out = String::with_capacity(8192);
        let _ = writeln!(out, "# SolidCraft Multi-Backend GPU & Auto-Tuning Dispatcher Report\n");
        let _ = writeln!(out, "## Detected Hardware & Execution Tiers");
        let _ = writeln!(out, "- **CPU Rayon Threads**: {}", self.cpu_threads);
        let _ = writeln!(out, "- **Vulkan (`wgpu`) Adapter**: {}", self.vulkan_adapter.as_deref().unwrap_or("Unavailable"));
        let _ = writeln!(out, "- **Primary CUDA Device**: {}", self.cuda_primary_device.as_deref().unwrap_or("Unavailable"));
        if !self.cuda_all_devices.is_empty() {
            let _ = writeln!(out, "- **All Discovered CUDA GPUs**: {}", self.cuda_all_devices.join(", "));
        }
        let _ = writeln!(out, "- **Ampere Handcrafted PTX (`sm_86`)**: {}", self.has_ampere_ptx);
        let _ = writeln!(out, "- **Ampere FP64 Tensor Core (`mma.sync.aligned.m8n8k4...f64`)**: {}", self.has_fp64_tensor_core);
        let _ = writeln!(out, "- **Total Calibration Time**: {:.2} ms\n", self.total_calibration_ms);

        let _ = writeln!(out, "## Empirical Crossover Summary\n");
        let _ = writeln!(
            out,
            "| Kernel Operation | CPU -> Rayon Crossover ($N$) | CPU -> GPU Crossover ($N$) | Preferred Large-$N$ Backend | Max Speedup vs Scalar |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|");
        for k in &self.kernels {
            let rayon_x = k.rayon_crossover_size.map_or_else(|| "N/A".to_string(), |n| n.to_string());
            let gpu_x = k.gpu_crossover_size.map_or_else(|| "CPU Best".to_string(), |n| n.to_string());
            let pref = k.buckets.last().map_or("CPU Scalar", |b| b.fastest_backend.name());
            let max_speedup = k.buckets.iter().map(|b| b.speedup_vs_scalar).fold(1.0f64, f64::max);
            let _ = writeln!(out, "| **{}** | {} | {} | {} | **{:.2}x** |", k.op.name(), rayon_x, gpu_x, pref, max_speedup);
        }
        let _ = writeln!(out);

        for k in &self.kernels {
            let _ = writeln!(out, "### {}\n", k.op.name());
            let Some(first_bucket) = k.buckets.first() else { continue };
            let mut header = String::from("| Size ($N$) |");
            let mut sep = String::from("|---:|");
            for t in &first_bucket.timings {
                let _ = write!(header, " {} (µs) |", t.backend.name());
                let _ = write!(sep, "---:|");
            }
            let _ = write!(header, " Winner | Speedup vs Scalar |");
            let _ = write!(sep, "---|---:|");
            let _ = writeln!(out, "{header}");
            let _ = writeln!(out, "{sep}");

            for b in &k.buckets {
                let mut row = format!("| **{}** |", b.problem_size);
                for t in &b.timings {
                    let us = t.latency_ns as f64 / 1_000.0;
                    let _ = write!(row, " {:.2} |", us);
                }
                let _ = write!(row, " **{}** | {:.2}x |", b.fastest_backend.name(), b.speedup_vs_scalar);
                let _ = writeln!(out, "{row}");
            }
            let _ = writeln!(out);
        }

        out
    }
}

// ── Synthetic Benchmark Fixtures ──────────────────────────────────────────

/// Builds a synthetic 4x4 `f32` bicubic patch control net for benchmarking and testing.
pub fn synthetic_bicubic_patch_f32() -> Vec<[f32; 4]> {
    let mut cpts = Vec::with_capacity(16);
    for i in 0..4 {
        for j in 0..4 {
            let x = (i as f32) * 3.0;
            let y = (j as f32) * 3.0;
            let z = ((i as f32 - 1.5).powi(2) + (j as f32 - 1.5).powi(2)) * 0.5;
            cpts.push([x, y, z, 1.0]);
        }
    }
    cpts
}

/// Builds a synthetic 4x4 `f64` bicubic patch control net for benchmarking and testing.
pub fn synthetic_bicubic_patch_f64() -> Vec<[f64; 4]> {
    let mut cpts = Vec::with_capacity(16);
    for i in 0..4 {
        for j in 0..4 {
            let x = (i as f64) * 3.0;
            let y = (j as f64) * 3.0;
            let z = ((i as f64 - 1.5).powi(2) + (j as f64 - 1.5).powi(2)) * 0.5;
            cpts.push([x, y, z, 1.0]);
        }
    }
    cpts
}

/// Builds a closed, watertight UV-sphere triangle mesh with `n_lat * n_lon * 2` triangles.
pub fn synthetic_closed_sphere_mesh(radius: f64, n_lat: usize, n_lon: usize) -> Mesh {
    use std::f64::consts::{PI, TAU};
    let lat = n_lat.max(4);
    let lon = n_lon.max(6);
    let mut positions = Vec::with_capacity((lat - 1) * lon + 2);
    // South pole (index 0) and North pole (index 1)
    positions.push(Vec3::new(0.0, 0.0, -radius));
    positions.push(Vec3::new(0.0, 0.0, radius));
    for i in 1..lat {
        let theta = PI * (i as f64) / (lat as f64);
        let (st, ct) = theta.sin_cos();
        for j in 0..lon {
            let phi = TAU * (j as f64) / (lon as f64);
            let (sp, cp) = phi.sin_cos();
            positions.push(Vec3::new(radius * st * cp, radius * st * sp, -radius * ct));
        }
    }
    let ring = |r: usize, c: usize| -> u32 { (2 + (r - 1) * lon + (c % lon)) as u32 };
    let mut triangles = Vec::with_capacity(lat * lon * 2);
    for j in 0..lon {
        triangles.push([0, ring(1, j + 1), ring(1, j)]);
        triangles.push([1, ring(lat - 1, j), ring(lat - 1, j + 1)]);
    }
    for i in 1..(lat - 1) {
        for j in 0..lon {
            let a = ring(i, j);
            let b = ring(i, j + 1);
            let c = ring(i + 1, j + 1);
            let d = ring(i + 1, j);
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    let normals = positions.iter().map(|p| p.normalized().unwrap_or(Vec3::Z)).collect();
    let tri_face = vec![0; triangles.len()];
    Mesh { positions, normals, triangles, tri_face, ..Mesh::default() }
}

/// Measures the wall-clock execution duration of `f` in nanoseconds.
fn time_ns(mut f: impl FnMut()) -> u64 {
    let t0 = Instant::now();
    f();
    t0.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

/// Returns the ordered list of available candidate [`BackendTarget`]s for `op` on the current host.
fn candidate_backends_for_op(op: KernelOp, vk: Option<&VulkanAccel>, cuda: Option<&CudaBackend>) -> Vec<BackendTarget> {
    let mut cands = vec![BackendTarget::CpuScalar, BackendTarget::CpuRayon];
    match op {
        KernelOp::SurfaceEvalF32 | KernelOp::AabbBroadphase | KernelOp::PointMeshDistance | KernelOp::MassProperties => {
            if vk.is_some() {
                cands.push(BackendTarget::VulkanCompute);
            }
            if let Some(c) = cuda {
                cands.push(BackendTarget::CudaGeneric);
                if c.has_ampere_ptx() {
                    cands.push(BackendTarget::AmperePtx);
                }
            }
        }
        KernelOp::TensorPatchEvalF64 => {
            if let Some(c) = cuda {
                cands.push(BackendTarget::CudaGeneric);
                if c.has_fp64_tensor_core() {
                    cands.push(BackendTarget::AmpereTensorCore);
                }
            }
        }
        KernelOp::PointInSolid => {
            if vk.is_some() {
                cands.push(BackendTarget::VulkanCompute);
            }
            if let Some(c) = cuda {
                cands.push(BackendTarget::CudaGeneric);
                if c.has_ampere_ptx() {
                    cands.push(BackendTarget::AmperePtx);
                }
                cands.push(BackendTarget::RtCoreBvh);
            } else if vk.is_some() {
                cands.push(BackendTarget::RtCoreBvh);
            }
        }
        KernelOp::RaycastMesh => {
            if vk.is_some() {
                cands.push(BackendTarget::VulkanCompute);
            }
            if cuda.is_some() {
                cands.push(BackendTarget::RtCoreBvh);
            }
        }
    }
    cands
}

/// Runs the scaling microbenchmark across `sizes` (defaults to [`BENCH_SIZES`]).
pub fn run_scaling_microbenchmark(sizes: &[usize]) -> CalibrationReport {
    let start_all = Instant::now();
    let vk = VulkanAccel::global();
    #[cfg(not(target_arch = "wasm32"))]
    let cuda = CudaBackend::global();
    #[cfg(target_arch = "wasm32")]
    let cuda: Option<&CudaBackend> = None;

    let cpu_threads = {
        #[cfg(not(target_arch = "wasm32"))]
        {
            rayon::current_num_threads()
        }
        #[cfg(target_arch = "wasm32")]
        {
            1
        }
    };

    let max_n = sizes.iter().copied().max().unwrap_or(65536).max(16);

    // Pre-generate benchmark datasets up to `max_n`.
    let patch_f32 = synthetic_bicubic_patch_f32();
    let patch_f64 = synthetic_bicubic_patch_f64();
    let queries_f32: Vec<PatchUvQueryF32> = (0..max_n)
        .map(|i| {
            let t = (i as f32) / (max_n as f32);
            PatchUvQueryF32 { u: (t * 0.97 + 0.01).fract(), v: (t * 3.17 + 0.02).fract(), patch_idx: 0, pad: 0 }
        })
        .collect();
    let queries_f64: Vec<[f64; 4]> = (0..max_n)
        .map(|i| {
            let t = (i as f64) / (max_n as f64);
            [(t * 0.97 + 0.01).fract(), (t * 3.17 + 0.02).fract(), 0.0, 0.0]
        })
        .collect();

    // Closed sphere mesh with 128 triangles for PointInSolid, Raycast, and PointMeshDistance
    let sphere_mesh = synthetic_closed_sphere_mesh(10.0, 8, 8);
    let (sphere_tris, sphere_bvh) = build_gpu_bvh_from_mesh(&sphere_mesh);

    let query_pts: Vec<[f32; 4]> = (0..max_n)
        .map(|i| {
            let f = (i as f32) / (max_n as f32);
            let r = 15.0 * (f - 0.5);
            [r, r * 0.7, r * -0.4, 0.0]
        })
        .collect();
    let query_rays: Vec<GpuRay> = (0..max_n)
        .map(|i| {
            let f = (i as f32) / (max_n as f32);
            let ox = (f - 0.5) * 8.0;
            let oy = ((f * 7.3).fract() - 0.5) * 8.0;
            GpuRay { origin: [ox, oy, -25.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] }
        })
        .collect();

    // Mass properties triangle array up to max_n
    let mass_tris: Vec<GpuTri> = (0..max_n).map(|i| sphere_tris[i % sphere_tris.len()]).collect();

    // Warmup GPU pipelines and pre-size scratch pools once before timed sweep
    if let Some(v) = vk {
        let _ = v.eval_bicubic_f32(&patch_f32, &queries_f32[..16]);
        let _ = v.classify_points(&query_pts[..16], &sphere_tris, &sphere_bvh, true);
        let _ = v.raycast_bvh(&query_rays[..16], &sphere_tris, &sphere_bvh);
        let _ = v.point_mesh_distance(&query_pts[..16], &sphere_tris);
        let _ = v.mass_properties(&mass_tris[..16]);
    }
    if let Some(c) = cuda {
        let _ = c.eval_bicubic_f32(&patch_f32, &queries_f32[..max_n], false);
        let _ = c.eval_bicubic_f64(&patch_f64, &queries_f64[..max_n], false);
        if c.has_ampere_ptx() {
            let _ = c.eval_bicubic_f32(&patch_f32, &queries_f32[..16], true);
            let _ = c.eval_bicubic_f64(&patch_f64, &queries_f64[..16], true);
        }
        let _ = c.classify_points(&query_pts[..16], &sphere_tris, false);
        let _ = c.classify_points_rt_bvh(&query_pts[..16], &sphere_tris, &sphere_bvh);
        let _ = c.raycast_rt_bvh(&query_rays[..16], &sphere_tris, &sphere_bvh);
        let _ = c.point_mesh_distance(&query_pts[..16], &sphere_tris, false);
        let _ = c.mass_properties(&mass_tris[..16], false);
    }

    let mut kernels = Vec::with_capacity(KernelOp::ALL.len());

    for &op in KernelOp::ALL {
        let candidates = candidate_backends_for_op(op, vk, cuda);
        let mut buckets = Vec::with_capacity(sizes.len());
        for &n in sizes {
            let n = n.min(max_n).max(1);
            let side_count = ((n as f64 * 16.0).sqrt().ceil() as usize).clamp(4, 1024);
            let (boxes_a, boxes_b) = if op == KernelOp::AabbBroadphase {
                (synthetic_aabb_grid(side_count, 2.0, 0.8), synthetic_aabb_grid(side_count, 2.0, 0.8))
            } else {
                (Vec::new(), Vec::new())
            };

            let mut sample_orders = Vec::with_capacity(CALIBRATION_SAMPLES);
            let mut sample_timings_ns = Vec::with_capacity(CALIBRATION_SAMPLES);

            for sample_idx in 0..CALIBRATION_SAMPLES {
                let mut order = candidates.clone();
                if !order.is_empty() {
                    let rot = sample_idx % order.len();
                    order.rotate_left(rot);
                }
                let mut raw_sample = Vec::with_capacity(order.len());
                for &backend in &order {
                    let ns = match (op, backend) {
                        (KernelOp::SurfaceEvalF32, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_eval_bicubic_f32_scalar(&patch_f32, &queries_f32[..n]));
                        }),
                        (KernelOp::SurfaceEvalF32, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_eval_bicubic_f32_rayon(&patch_f32, &queries_f32[..n]));
                        }),
                        (KernelOp::SurfaceEvalF32, BackendTarget::VulkanCompute) => time_ns(|| {
                            if let Some(v) = vk {
                                std::hint::black_box(v.eval_bicubic_f32(&patch_f32, &queries_f32[..n]));
                            }
                        }),
                        (KernelOp::SurfaceEvalF32, BackendTarget::CudaGeneric) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.eval_bicubic_f32(&patch_f32, &queries_f32[..n], false));
                            }
                        }),
                        (KernelOp::SurfaceEvalF32, BackendTarget::AmperePtx) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.eval_bicubic_f32(&patch_f32, &queries_f32[..n], true));
                            }
                        }),
                        (KernelOp::TensorPatchEvalF64, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_eval_bicubic_f64_scalar(&patch_f64, &queries_f64[..n]));
                        }),
                        (KernelOp::TensorPatchEvalF64, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_eval_bicubic_f64_rayon(&patch_f64, &queries_f64[..n]));
                        }),
                        (KernelOp::TensorPatchEvalF64, BackendTarget::CudaGeneric) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.eval_bicubic_f64(&patch_f64, &queries_f64[..n], false));
                            }
                        }),
                        (KernelOp::TensorPatchEvalF64, BackendTarget::AmpereTensorCore) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.eval_bicubic_f64(&patch_f64, &queries_f64[..n], true));
                            }
                        }),
                        (KernelOp::PointInSolid, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_classify_points_scalar(&query_pts[..n], &sphere_tris, &[]));
                        }),
                        (KernelOp::PointInSolid, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_classify_points_rayon(&query_pts[..n], &sphere_tris, &[]));
                        }),
                        (KernelOp::PointInSolid, BackendTarget::VulkanCompute) => time_ns(|| {
                            if let Some(v) = vk {
                                std::hint::black_box(v.classify_points(&query_pts[..n], &sphere_tris, &sphere_bvh, false));
                            }
                        }),
                        (KernelOp::PointInSolid, BackendTarget::CudaGeneric) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.classify_points(&query_pts[..n], &sphere_tris, false));
                            }
                        }),
                        (KernelOp::PointInSolid, BackendTarget::AmperePtx) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.classify_points(&query_pts[..n], &sphere_tris, true));
                            }
                        }),
                        (KernelOp::PointInSolid, BackendTarget::RtCoreBvh) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.classify_points_rt_bvh(&query_pts[..n], &sphere_tris, &sphere_bvh));
                            } else if let Some(v) = vk {
                                std::hint::black_box(v.classify_points(&query_pts[..n], &sphere_tris, &sphere_bvh, true));
                            }
                        }),
                        (KernelOp::RaycastMesh, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_raycast_bvh_scalar(&query_rays[..n], &sphere_tris, &sphere_bvh));
                        }),
                        (KernelOp::RaycastMesh, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_raycast_bvh_rayon(&query_rays[..n], &sphere_tris, &sphere_bvh));
                        }),
                        (KernelOp::RaycastMesh, BackendTarget::VulkanCompute) => time_ns(|| {
                            if let Some(v) = vk {
                                std::hint::black_box(v.raycast_bvh(&query_rays[..n], &sphere_tris, &sphere_bvh));
                            }
                        }),
                        (KernelOp::RaycastMesh, BackendTarget::RtCoreBvh) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.raycast_rt_bvh(&query_rays[..n], &sphere_tris, &sphere_bvh));
                            }
                        }),
                        (KernelOp::AabbBroadphase, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_bipartite_overlap(&boxes_a, &boxes_b, 1e-4));
                        }),
                        (KernelOp::AabbBroadphase, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_bipartite_overlap_rayon(&boxes_a, &boxes_b, 1e-4));
                        }),
                        (KernelOp::AabbBroadphase, BackendTarget::VulkanCompute) => time_ns(|| {
                            if let Some(v) = vk {
                                std::hint::black_box(v.collision().bipartite_overlap(&boxes_a, &boxes_b, 1e-4));
                            }
                        }),
                        (KernelOp::AabbBroadphase, BackendTarget::CudaGeneric) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.bipartite_aabb_overlap(&boxes_a, &boxes_b, 1e-4, false));
                            }
                        }),
                        (KernelOp::AabbBroadphase, BackendTarget::AmperePtx) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.bipartite_aabb_overlap(&boxes_a, &boxes_b, 1e-4, true));
                            }
                        }),
                        (KernelOp::PointMeshDistance, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_point_mesh_distance_scalar(&query_pts[..n], &sphere_tris));
                        }),
                        (KernelOp::PointMeshDistance, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_point_mesh_distance_rayon(&query_pts[..n], &sphere_tris));
                        }),
                        (KernelOp::PointMeshDistance, BackendTarget::VulkanCompute) => time_ns(|| {
                            if let Some(v) = vk {
                                std::hint::black_box(v.point_mesh_distance(&query_pts[..n], &sphere_tris));
                            }
                        }),
                        (KernelOp::PointMeshDistance, BackendTarget::CudaGeneric) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.point_mesh_distance(&query_pts[..n], &sphere_tris, false));
                            }
                        }),
                        (KernelOp::PointMeshDistance, BackendTarget::AmperePtx) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.point_mesh_distance(&query_pts[..n], &sphere_tris, true));
                            }
                        }),
                        (KernelOp::MassProperties, BackendTarget::CpuScalar) => time_ns(|| {
                            std::hint::black_box(cpu_mass_properties_scalar(&mass_tris[..n]));
                        }),
                        (KernelOp::MassProperties, BackendTarget::CpuRayon) => time_ns(|| {
                            std::hint::black_box(cpu_mass_properties_rayon(&mass_tris[..n]));
                        }),
                        (KernelOp::MassProperties, BackendTarget::VulkanCompute) => time_ns(|| {
                            if let Some(v) = vk {
                                std::hint::black_box(v.mass_properties(&mass_tris[..n]));
                            }
                        }),
                        (KernelOp::MassProperties, BackendTarget::CudaGeneric) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.mass_properties(&mass_tris[..n], false));
                            }
                        }),
                        (KernelOp::MassProperties, BackendTarget::AmperePtx) => time_ns(|| {
                            if let Some(c) = cuda {
                                std::hint::black_box(c.mass_properties(&mass_tris[..n], true));
                            }
                        }),
                        _ => 1,
                    };
                    raw_sample.push((backend, ns.max(1)));
                }
                sample_orders.push(order);
                sample_timings_ns.push(raw_sample);
            }

            let mut timings = Vec::with_capacity(candidates.len());
            for &backend in &candidates {
                let mut vals: Vec<u64> = sample_timings_ns.iter().filter_map(|s| s.iter().find(|&&(b, _)| b == backend).map(|&(_, ns)| ns)).collect();
                vals.sort_unstable();
                let median_ns = vals.get(vals.len() / 2).copied().unwrap_or(1).max(1);
                let melems = (n as f64) / (median_ns as f64 / 1_000.0);
                timings.push(BackendTiming { backend, latency_ns: median_ns, throughput_melems_per_sec: melems });
            }

            let scalar_ns = timings.first().map_or(1, |t| t.latency_ns);
            let fastest = timings.iter().min_by_key(|t| t.latency_ns).cloned().unwrap_or(BackendTiming {
                backend: BackendTarget::CpuScalar,
                latency_ns: scalar_ns,
                throughput_melems_per_sec: 0.0,
            });
            let speedup_vs_scalar = (scalar_ns as f64) / (fastest.latency_ns.max(1) as f64);

            buckets.push(SizeBucketCalibration {
                problem_size: n,
                timings,
                sample_orders,
                sample_timings_ns,
                fastest_backend: fastest.backend,
                fastest_latency_ns: fastest.latency_ns,
                speedup_vs_scalar,
            });
        }

        let rayon_crossover_size = buckets
            .iter()
            .find(|b| {
                let s = b.timings.iter().find(|t| t.backend == BackendTarget::CpuScalar);
                let r = b.timings.iter().find(|t| t.backend == BackendTarget::CpuRayon);
                matches!((s, r), (Some(s), Some(r)) if r.latency_ns < s.latency_ns)
            })
            .map(|b| b.problem_size);

        let gpu_crossover_size = buckets.iter().find(|b| b.fastest_backend.is_gpu()).map(|b| b.problem_size);

        let preferred_gpu_backend =
            buckets.last().and_then(|b| b.timings.iter().filter(|t| t.backend.is_gpu()).min_by_key(|t| t.latency_ns)).map(|t| t.backend);

        kernels.push(KernelCalibration { op, buckets, rayon_crossover_size, gpu_crossover_size, preferred_gpu_backend });
    }

    let cuda_primary_device = cuda.map(|c| {
        let d = c.device_info();
        format!(
            "{} (SM {}.{}, {} SMs, {:.1} GB VRAM)",
            d.name,
            d.compute_capability.0,
            d.compute_capability.1,
            d.sm_count,
            d.total_vram_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        )
    });
    let cuda_all_devices = cuda
        .map(|c| {
            c.all_devices()
                .iter()
                .map(|d| format!("#{} {} (SM {}.{}, {} SMs)", d.ordinal, d.name, d.compute_capability.0, d.compute_capability.1, d.sm_count))
                .collect()
        })
        .unwrap_or_default();

    CalibrationReport {
        cpu_threads,
        vulkan_adapter: vk.map(|v| v.adapter_name().to_string()),
        cuda_primary_device,
        cuda_all_devices,
        has_ampere_ptx: cuda.is_some_and(|c| c.has_ampere_ptx()),
        has_fp64_tensor_core: cuda.is_some_and(|c| c.has_fp64_tensor_core()),
        kernels,
        total_calibration_ms: start_all.elapsed().as_secs_f64() * 1_000.0,
    }
}

// ── Runtime Auto-Tuning Dispatcher ────────────────────────────────────────

/// Runtime auto-tuning dispatcher that routes geometric kernel operations to the empirically
/// fastest backend for the given problem size, with automatic fallback to CPU if a GPU dispatch fails.
#[derive(Clone, Debug)]
pub struct Dispatcher {
    report: CalibrationReport,
    forced_backend: Option<BackendTarget>,
}

static GLOBAL_DISPATCHER: OnceLock<Dispatcher> = OnceLock::new();

impl Dispatcher {
    /// Returns the process-wide calibrated [`Dispatcher`], running the startup scaling microbenchmark on first access.
    pub fn global() -> &'static Self {
        GLOBAL_DISPATCHER.get_or_init(|| Self::calibrate(BENCH_SIZES))
    }

    /// Runs a fresh calibration over `sizes` and returns a new [`Dispatcher`].
    pub fn calibrate(sizes: &[usize]) -> Self {
        Self { report: run_scaling_microbenchmark(sizes), forced_backend: None }
    }

    /// Returns a copy of this dispatcher that forces all operations to use `target` (falling back to CPU if unavailable).
    pub fn with_forced_backend(&self, target: Option<BackendTarget>) -> Self {
        let mut copy = self.clone();
        copy.forced_backend = target;
        copy
    }

    /// Access the underlying [`CalibrationReport`].
    pub fn report(&self) -> &CalibrationReport {
        &self.report
    }

    /// Selects the optimal [`BackendTarget`] for `(op, problem_size)`.
    pub fn select_backend(&self, op: KernelOp, problem_size: usize) -> BackendTarget {
        if let Some(forced) = self.forced_backend {
            return forced;
        }
        self.report.kernel(op).map_or(BackendTarget::CpuScalar, |k| k.select_backend(problem_size))
    }

    /// Dispatches batched `f32` bicubic patch evaluation to the optimal backend.
    pub fn eval_bicubic_f32(&self, patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32]) -> (BackendTarget, Vec<PatchEvalOutF32>) {
        let target = self.select_backend(KernelOp::SurfaceEvalF32, queries.len());
        match target {
            BackendTarget::AmperePtx | BackendTarget::AmpereTensorCore | BackendTarget::RtCoreBvh => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.eval_bicubic_f32(patch_cpts, queries, true)
                {
                    return (BackendTarget::AmperePtx, out);
                }
            }
            BackendTarget::CudaGeneric => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.eval_bicubic_f32(patch_cpts, queries, false)
                {
                    return (BackendTarget::CudaGeneric, out);
                }
            }
            BackendTarget::VulkanCompute => {
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.eval_bicubic_f32(patch_cpts, queries)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_eval_bicubic_f32_rayon(patch_cpts, queries));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_eval_bicubic_f32_scalar(patch_cpts, queries));
            }
        }
        (BackendTarget::CpuScalar, cpu_eval_bicubic_f32_scalar(patch_cpts, queries))
    }

    /// Dispatches batched `f64` double-precision bicubic patch evaluation (using Ampere DMMA Tensor Cores when selected).
    pub fn eval_bicubic_f64(&self, patch_cpts: &[[f64; 4]], queries: &[[f64; 4]]) -> (BackendTarget, Vec<PatchEvalOutF64>) {
        let target = self.select_backend(KernelOp::TensorPatchEvalF64, queries.len());
        match target {
            BackendTarget::AmpereTensorCore | BackendTarget::AmperePtx | BackendTarget::RtCoreBvh => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.eval_bicubic_f64(patch_cpts, queries, true)
                {
                    return (BackendTarget::AmpereTensorCore, out);
                }
            }
            BackendTarget::CudaGeneric | BackendTarget::VulkanCompute => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.eval_bicubic_f64(patch_cpts, queries, false)
                {
                    return (BackendTarget::CudaGeneric, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_eval_bicubic_f64_rayon(patch_cpts, queries));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_eval_bicubic_f64_scalar(patch_cpts, queries));
            }
        }
        (BackendTarget::CpuScalar, cpu_eval_bicubic_f64_scalar(patch_cpts, queries))
    }

    /// Dispatches batched 3-ray watertight point-in-solid classification (`Mesh::contains`).
    pub fn classify_points(&self, points: &[[f32; 4]], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> (BackendTarget, Vec<bool>) {
        let target = self.select_backend(KernelOp::PointInSolid, points.len());
        match target {
            BackendTarget::RtCoreBvh => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global() {
                    if let Some(out) = c.classify_points_rt_bvh(points, tris, bvh) {
                        return (BackendTarget::RtCoreBvh, out);
                    }
                    let use_amp = c.has_ampere_ptx();
                    if let Some(out) = c.classify_points(points, tris, use_amp) {
                        return (if use_amp { BackendTarget::AmperePtx } else { BackendTarget::CudaGeneric }, out);
                    }
                }
                if let Some(v) = VulkanAccel::global() {
                    if let Some(out) = v.classify_points(points, tris, bvh, true) {
                        return (BackendTarget::RtCoreBvh, out);
                    }
                    if let Some(out) = v.classify_points(points, tris, bvh, false) {
                        return (BackendTarget::VulkanCompute, out);
                    }
                }
            }
            BackendTarget::AmperePtx | BackendTarget::AmpereTensorCore => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.classify_points(points, tris, true)
                {
                    return (BackendTarget::AmperePtx, out);
                }
            }
            BackendTarget::CudaGeneric => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.classify_points(points, tris, false)
                {
                    return (BackendTarget::CudaGeneric, out);
                }
            }
            BackendTarget::VulkanCompute => {
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.classify_points(points, tris, bvh, false)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_classify_points_rayon(points, tris, bvh));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_classify_points_scalar(points, tris, bvh));
            }
        }
        (BackendTarget::CpuScalar, cpu_classify_points_scalar(points, tris, bvh))
    }

    /// Dispatches batched BVH ray-mesh intersection (`Mesh::raycast` / `MeshIndex::raycast`).
    pub fn raycast_mesh(&self, rays: &[GpuRay], tris: &[GpuTri], bvh: &[GpuBvhNode]) -> (BackendTarget, Vec<GpuRayHit>) {
        let target = self.select_backend(KernelOp::RaycastMesh, rays.len());
        match target {
            BackendTarget::RtCoreBvh | BackendTarget::AmperePtx | BackendTarget::CudaGeneric | BackendTarget::AmpereTensorCore => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.raycast_rt_bvh(rays, tris, bvh)
                {
                    return (BackendTarget::RtCoreBvh, out);
                }
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.raycast_bvh(rays, tris, bvh)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::VulkanCompute => {
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.raycast_bvh(rays, tris, bvh)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_raycast_bvh_rayon(rays, tris, bvh));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_raycast_bvh_scalar(rays, tris, bvh));
            }
        }
        (BackendTarget::CpuScalar, cpu_raycast_bvh_scalar(rays, tris, bvh))
    }

    /// Dispatches bipartite AABB broadphase collision detection.
    pub fn bipartite_aabb_overlap(&self, aabbs_a: &[GpuAabb], aabbs_b: &[GpuAabb], tolerance: f32) -> (BackendTarget, CollisionResult) {
        let pair_work = aabbs_a.len().saturating_mul(aabbs_b.len()) / 16;
        let target = self.select_backend(KernelOp::AabbBroadphase, pair_work.max(1));
        match target {
            BackendTarget::AmperePtx | BackendTarget::AmpereTensorCore | BackendTarget::RtCoreBvh => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.bipartite_aabb_overlap(aabbs_a, aabbs_b, tolerance, true)
                {
                    return (BackendTarget::AmperePtx, out);
                }
            }
            BackendTarget::CudaGeneric => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.bipartite_aabb_overlap(aabbs_a, aabbs_b, tolerance, false)
                {
                    return (BackendTarget::CudaGeneric, out);
                }
            }
            BackendTarget::VulkanCompute => {
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.collision().bipartite_overlap(aabbs_a, aabbs_b, tolerance)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_bipartite_overlap_rayon(aabbs_a, aabbs_b, tolerance));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_bipartite_overlap(aabbs_a, aabbs_b, tolerance));
            }
        }
        (BackendTarget::CpuScalar, cpu_bipartite_overlap(aabbs_a, aabbs_b, tolerance))
    }

    /// Dispatches batched point-to-mesh minimum squared distance evaluation.
    pub fn point_mesh_distance(&self, points: &[[f32; 4]], tris: &[GpuTri]) -> (BackendTarget, Vec<f32>) {
        let target = self.select_backend(KernelOp::PointMeshDistance, points.len());
        match target {
            BackendTarget::AmperePtx | BackendTarget::AmpereTensorCore | BackendTarget::RtCoreBvh => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.point_mesh_distance(points, tris, true)
                {
                    return (BackendTarget::AmperePtx, out);
                }
            }
            BackendTarget::CudaGeneric => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.point_mesh_distance(points, tris, false)
                {
                    return (BackendTarget::CudaGeneric, out);
                }
            }
            BackendTarget::VulkanCompute => {
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.point_mesh_distance(points, tris)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_point_mesh_distance_rayon(points, tris));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_point_mesh_distance_scalar(points, tris));
            }
        }
        (BackendTarget::CpuScalar, cpu_point_mesh_distance_scalar(points, tris))
    }

    /// Dispatches divergence-theorem mass properties (`volume`, `area`, `centroid`) calculation.
    pub fn mass_properties(&self, tris: &[GpuTri]) -> (BackendTarget, MeshMeasure) {
        let target = self.select_backend(KernelOp::MassProperties, tris.len());
        match target {
            BackendTarget::AmperePtx | BackendTarget::AmpereTensorCore | BackendTarget::RtCoreBvh => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.mass_properties(tris, true)
                {
                    return (BackendTarget::AmperePtx, out);
                }
            }
            BackendTarget::CudaGeneric => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = CudaBackend::global()
                    && let Some(out) = c.mass_properties(tris, false)
                {
                    return (BackendTarget::CudaGeneric, out);
                }
            }
            BackendTarget::VulkanCompute => {
                if let Some(v) = VulkanAccel::global()
                    && let Some(out) = v.mass_properties(tris)
                {
                    return (BackendTarget::VulkanCompute, out);
                }
            }
            BackendTarget::CpuRayon => {
                return (BackendTarget::CpuRayon, cpu_mass_properties_rayon(tris));
            }
            BackendTarget::CpuScalar => {
                return (BackendTarget::CpuScalar, cpu_mass_properties_scalar(tris));
            }
        }
        (BackendTarget::CpuScalar, cpu_mass_properties_scalar(tris))
    }

    /// Evaluates a [`TessPlan`] using either CPU scalar or parallel `rayon` across faces.
    pub fn evaluate_tess_plan(&self, plan: &TessPlan) -> Mesh {
        if plan.vertex_count() < 512 {
            return plan.evaluate();
        }
        let face_evals: Vec<(Vec<Vec3>, Vec<Vec3>)> = plan
            .faces
            .par_iter()
            .map(|f| {
                let mut pos = Vec::with_capacity(f.uv.len());
                let mut nrm = Vec::with_capacity(f.uv.len());
                let surface = f.surface.and_then(|i| plan.surfaces.get(i as usize));
                for (k, uv) in f.uv.iter().enumerate() {
                    let (p, n) = match surface {
                        Some(s) => s.eval(uv[0], uv[1]),
                        None => (Vec3::ZERO, f.normals.get(k).copied().unwrap_or(Vec3::Z)),
                    };
                    let p = f.fixed.get(k).copied().unwrap_or(p);
                    pos.push(p);
                    nrm.push(if f.flip { -n } else { n });
                }
                (pos, nrm)
            })
            .collect();

        let mut out = Mesh::default();
        for (f, (pos, nrm)) in plan.faces.iter().zip(face_evals) {
            let base = out.positions.len();
            let n_verts = f.uv.len();
            out.positions.extend(pos);
            out.normals.extend(nrm);
            for t in &f.triangles {
                if t.iter().all(|&i| (i as usize) < n_verts) {
                    let at = |i: u32| u32::try_from(base + i as usize).unwrap_or(u32::MAX);
                    out.triangles.push([at(t[0]), at(t[1]), at(t[2])]);
                    out.tri_face.push(f.face);
                }
            }
        }
        out.edges = plan.edges.clone();
        out.edge_faces = plan.edge_faces.clone();
        out
    }
}
