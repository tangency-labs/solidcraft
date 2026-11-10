//! Multi-Backend GPU Acceleration & Runtime Auto-Tuning Dispatcher.
//!
//! Coordinates execution across seven hardware/software execution tiers:
//! 1. [`BackendTarget::CpuScalar`] — Single-threaded deterministic Rust reference.
//! 2. [`BackendTarget::CpuRayon`] — Multi-core work-stealing `rayon` parallel CPU execution.
//! 3. [`BackendTarget::VulkanCompute`] — Cross-platform `wgpu` / Vulkan compute shaders (`accel.wgsl`, `collision.wgsl`).
//! 4. [`BackendTarget::CudaGeneric`] — CUDA Driver API via `cudarc` (`dynamic-loading`) running `sm_70` PTX.
//! 5. [`BackendTarget::AmperePtx`] — Handcrafted NVIDIA Ampere (`sm_86`) PTX (`cp.async.ca.shared.global`, `redux.sync`, warp shuffle).
//! 6. [`BackendTarget::AmpereTensorCore`] — Handcrafted Ampere F32 Tensor Core MMA (`mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32`).
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
use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, Vec3Geom};
use wgpu::util::DeviceExt;

use crate::collision::{CollisionGpu, GpuPod, decode_pod_slice, encode_pod, encode_pod_slice};
#[cfg(not(target_arch = "wasm32"))]
use crate::cuda::CudaBackend;
use crate::cuda::{PatchEvalOutF32, PatchUvQueryF32};

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
    /// Handcrafted NVIDIA Ampere (`sm_86`) F32 Tensor Core MMA (`mma.sync.aligned.m16n8k8...f32`).
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
            Self::AmpereTensorCore => "Ampere Tensor Core (MMA f32)",
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
    /// Batched `f32` single-precision bicubic patch & derivative tensor contraction (Ampere MMA).
    TensorPatchEvalF32,
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
        Self::TensorPatchEvalF32,
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
            Self::TensorPatchEvalF32 => "Bicubic Tensor Patch Eval (f32 MMA)",
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

/// Shared Vulkan / `wgpu` compute pipelines for geometric kernel classes.
pub struct VulkanAccel {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    collision: CollisionGpu,
    patch_pipeline: wgpu::ComputePipeline,
    patch_bgl: wgpu::BindGroupLayout,
}

static GLOBAL_VULKAN: OnceLock<Option<VulkanAccel>> = OnceLock::new();

impl VulkanAccel {
    /// Returns the process-wide shared [`VulkanAccel`] instance if a `wgpu` adapter is available.
    pub fn global() -> Option<&'static Self> {
        GLOBAL_VULKAN.get_or_init(Self::new).as_ref()
    }

    /// Initializes a new [`VulkanAccel`] on the high-performance GPU adapter.
    pub fn new() -> Option<Self> {
        let policy = crate::env::policy().ok()?;
        let (adapter, device, queue) = solidcraft_viewport_gpu::open_headless(policy).ok()?;
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

        Some(Self { device, queue, adapter_name, collision, patch_pipeline, patch_bgl })
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

        let _guard = self.collision.lock_device();
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
    if queries.is_empty() || patch_cpts.len() < 16 {
        return Vec::new();
    }
    let num_patches = (patch_cpts.len() / 16) as u32;
    queries.iter().map(|&q| eval_one_bicubic_f32(patch_cpts, num_patches, q)).collect()
}

/// CPU `rayon` parallel evaluation of `f32` bicubic patches.
pub fn cpu_eval_bicubic_f32_rayon(patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32]) -> Vec<PatchEvalOutF32> {
    if queries.is_empty() || patch_cpts.len() < 16 {
        return Vec::new();
    }
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
    pub has_f32_tensor_core: bool,
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
        let _ = writeln!(out, "- **Ampere F32 Tensor Core (`mma.sync.aligned.m16n8k8...f32`)**: {}", self.has_f32_tensor_core);
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
#[allow(dead_code)]
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
        KernelOp::TensorPatchEvalF32 => {
            if let Some(c) = cuda {
                cands.push(BackendTarget::CudaGeneric);
                if c.has_f32_tensor_core() {
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
    let queries_f32: Vec<PatchUvQueryF32> = (0..max_n)
        .map(|i| {
            let t = (i as f32) / (max_n as f32);
            PatchUvQueryF32 { u: (t * 0.97 + 0.01).fract(), v: (t * 3.17 + 0.02).fract(), patch_idx: 0, pad: 0 }
        })
        .collect();

    if let Some(v) = vk {
        let _ = v.eval_bicubic_f32(&patch_f32, &queries_f32[..16]);
    }
    if let Some(c) = cuda {
        let _ = c.eval_bicubic_f32(&patch_f32, &queries_f32[..max_n], false);
        if c.has_ampere_ptx() {
            let _ = c.eval_bicubic_f32(&patch_f32, &queries_f32[..16], true);
        }
    }

    let mut kernels = Vec::with_capacity(KernelOp::ALL.len());

    for &op in KernelOp::ALL {
        let candidates = candidate_backends_for_op(op, vk, cuda);
        let mut buckets = Vec::with_capacity(sizes.len());
        for &n in sizes {
            let n = n.min(max_n).max(1);
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
        has_f32_tensor_core: cuda.is_some_and(|c| c.has_f32_tensor_core()),
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

    /// Evaluates a [`TessPlan`] using either CPU scalar or parallel `rayon` across faces.
    pub fn evaluate_tess_plan(&self, plan: &TessPlan) -> Mesh {
        if plan.vertex_count() < 512 {
            return plan.evaluate().0;
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
                        Some(s) => {
                            let (p, n) = s.eval(uv[0], uv[1]);
                            (solidcraft_geom::tess::from_kernel(p), solidcraft_geom::tess::from_kernel(n))
                        }
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
