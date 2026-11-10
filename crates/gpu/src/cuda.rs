//! Safe `cudarc` CUDA Driver API & PTX backend supporting:
//! - **Tier 3: Generic CUDA (`sm_70`)** — portable PTX compute kernels.
//! - **Tier 4a: Handcrafted NVIDIA Ampere PTX (`sm_86`)** — `cp.async.ca.shared.global`,
//!   `redux.sync.add.s32`, and warp-level butterfly shuffle reductions.
//! - **Tier 4b: Ampere F32 Tensor Cores (MMA)** — `mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32`
//!   with two-term TF32 error-free splitting across $K=8$ for single-precision bicubic surface patch
//!   & derivative tensor contractions on consumer and datacenter GPUs.
//! - **Tier 4c: RT / Hardware-Aligned BVH Traversal** — $O(\log N)$ ray-mesh intersection
//!   and watertight 3-ray point-in-solid classification.
//!
//! Uses `cudarc` with runtime dynamic loading (`dynamic-loading`) so SolidCraft compiles cleanly
//! on machines without the CUDA toolkit installed and falls back gracefully when no NVIDIA
//! driver is present at runtime.

#![allow(unsafe_code)]

use std::sync::{Arc, Mutex, OnceLock};

use cudarc::driver::{
    CudaContext, CudaFunction, CudaModule, CudaStream, DeviceRepr, LaunchConfig, PushKernelArg, ValidAsZeroBits,
    result::device as cu_device,
    sys::{CUdevice_attribute, is_culib_present},
};
use cudarc::nvrtc::Ptx;

use crate::collision::{GpuAabb, GpuPod, OverlapPair};

const PTX_SM70: &str = include_str!("kernels_sm70.ptx");
const PTX_SM86: &str = include_str!("kernels_sm86.ptx");

// SAFETY: `GpuAabb` and `OverlapPair` are `#[repr(C)]` plain-old-data structs of `u32` and `f32` fields
// for which all-zero bit patterns are valid.
unsafe impl DeviceRepr for GpuAabb {}
unsafe impl ValidAsZeroBits for GpuAabb {}
unsafe impl DeviceRepr for OverlapPair {}
unsafe impl ValidAsZeroBits for OverlapPair {}

/// Hardware capabilities of a discovered CUDA GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CudaDeviceInfo {
    pub ordinal: i32,
    pub name: String,
    pub compute_capability: (i32, i32),
    pub sm_count: i32,
    pub clock_khz: i32,
    pub memory_bus_bits: i32,
    pub l2_cache_bytes: i32,
    pub total_vram_bytes: usize,
    pub supports_ampere_ptx: bool,
    pub supports_f32_tensor_core: bool,
    pub supports_rt_core: bool,
}

/// Packed GPU triangle (48 bytes) shared by Vulkan (`wgpu`) and CUDA (`cudarc`) kernels.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuTri {
    pub v0: [f32; 4], // xyz = v0, w = bitcast<f32>(orig_tri_idx)
    pub v1: [f32; 4], // xyz = v1
    pub v2: [f32; 4], // xyz = v2
}

// SAFETY: `GpuTri` is `#[repr(C)]` with three `[f32; 4]` fields (48 bytes, no padding).
unsafe impl DeviceRepr for GpuTri {}
unsafe impl ValidAsZeroBits for GpuTri {}

impl GpuPod for GpuTri {
    const WORDS: usize = 12;
    /// Appends the 12 `u32` words of this [`GpuTri`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        self.v0.push_words(out);
        self.v1.push_words(out);
        self.v2.push_words(out);
    }
    /// Decodes a [`GpuTri`] from a 12-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            v0: <[f32; 4]>::from_words(w.get(0..4)?)?,
            v1: <[f32; 4]>::from_words(w.get(4..8)?)?,
            v2: <[f32; 4]>::from_words(w.get(8..12)?)?,
        })
    }
}

/// Flattened BVH node (32 bytes) for raycast and point-in-solid traversal.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuBvhNode {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    pub left_or_prim: u32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
    pub right_or_count: u32, // MSB 0x8000_0000 = leaf, low 31 bits = prim count
}

// SAFETY: `GpuBvhNode` is `#[repr(C)]` with 8 32-bit scalar fields (32 bytes, no padding).
unsafe impl DeviceRepr for GpuBvhNode {}
unsafe impl ValidAsZeroBits for GpuBvhNode {}

impl GpuPod for GpuBvhNode {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`GpuBvhNode`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend_from_slice(&[
            self.min_x.to_bits(),
            self.min_y.to_bits(),
            self.min_z.to_bits(),
            self.left_or_prim,
            self.max_x.to_bits(),
            self.max_y.to_bits(),
            self.max_z.to_bits(),
            self.right_or_count,
        ]);
    }
    /// Decodes a [`GpuBvhNode`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self {
            min_x: f32::from_bits(*w.first()?),
            min_y: f32::from_bits(*w.get(1)?),
            min_z: f32::from_bits(*w.get(2)?),
            left_or_prim: *w.get(3)?,
            max_x: f32::from_bits(*w.get(4)?),
            max_y: f32::from_bits(*w.get(5)?),
            max_z: f32::from_bits(*w.get(6)?),
            right_or_count: *w.get(7)?,
        })
    }
}

/// Packed ray (32 bytes) for batch raycast queries.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuRay {
    pub origin: [f32; 4],
    pub dir: [f32; 4],
}

// SAFETY: `GpuRay` is `#[repr(C)]` with two `[f32; 4]` fields (32 bytes, no padding).
unsafe impl DeviceRepr for GpuRay {}
unsafe impl ValidAsZeroBits for GpuRay {}

impl GpuPod for GpuRay {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`GpuRay`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        self.origin.push_words(out);
        self.dir.push_words(out);
    }
    /// Decodes a [`GpuRay`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { origin: <[f32; 4]>::from_words(w.get(0..4)?)?, dir: <[f32; 4]>::from_words(w.get(4..8)?)? })
    }
}

/// Packed raycast hit result (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuRayHit {
    pub t: f32,
    pub tri_idx: u32,
    pub pad0: u32,
    pub pad1: u32,
}

// SAFETY: `GpuRayHit` is `#[repr(C)]` with four 32-bit fields (16 bytes, no padding).
unsafe impl DeviceRepr for GpuRayHit {}
unsafe impl ValidAsZeroBits for GpuRayHit {}

impl GpuPod for GpuRayHit {
    const WORDS: usize = 4;
    /// Appends the 4 `u32` words of this [`GpuRayHit`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend_from_slice(&[self.t.to_bits(), self.tri_idx, self.pad0, self.pad1]);
    }
    /// Decodes a [`GpuRayHit`] from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { t: f32::from_bits(*w.first()?), tri_idx: *w.get(1)?, pad0: *w.get(2)?, pad1: *w.get(3)? })
    }
}

/// UV query for `f32` bicubic patch evaluation (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PatchUvQueryF32 {
    pub u: f32,
    pub v: f32,
    pub patch_idx: u32,
    pub pad: u32,
}

// SAFETY: `PatchUvQueryF32` is `#[repr(C)]` with four 32-bit fields (16 bytes, no padding).
unsafe impl DeviceRepr for PatchUvQueryF32 {}
unsafe impl ValidAsZeroBits for PatchUvQueryF32 {}

impl GpuPod for PatchUvQueryF32 {
    const WORDS: usize = 4;
    /// Appends the 4 `u32` words of this [`PatchUvQueryF32`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        out.extend_from_slice(&[self.u.to_bits(), self.v.to_bits(), self.patch_idx, self.pad]);
    }
    /// Decodes a [`PatchUvQueryF32`] from a 4-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { u: f32::from_bits(*w.first()?), v: f32::from_bits(*w.get(1)?), patch_idx: *w.get(2)?, pad: *w.get(3)? })
    }
}

/// Output of `f32` bicubic patch evaluation (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PatchEvalOutF32 {
    pub pos: [f32; 4],
    pub norm: [f32; 4],
}

// SAFETY: `PatchEvalOutF32` is `#[repr(C)]` with two `[f32; 4]` fields (32 bytes, no padding).
unsafe impl DeviceRepr for PatchEvalOutF32 {}
unsafe impl ValidAsZeroBits for PatchEvalOutF32 {}

impl GpuPod for PatchEvalOutF32 {
    const WORDS: usize = 8;
    /// Appends the 8 `u32` words of this [`PatchEvalOutF32`] to `out`.
    fn push_words(&self, out: &mut Vec<u32>) {
        self.pos.push_words(out);
        self.norm.push_words(out);
    }
    /// Decodes a [`PatchEvalOutF32`] from an 8-word `u32` slice.
    fn from_words(w: &[u32]) -> Option<Self> {
        Some(Self { pos: <[f32; 4]>::from_words(w.get(0..4)?)?, norm: <[f32; 4]>::from_words(w.get(4..8)?)? })
    }
}

struct GenericFunctions {
    eval_patch_f32: CudaFunction,
}

struct AmpereFunctions {
    eval_patch_f32_sm86: CudaFunction,
}

/// Multi-tier `cudarc` CUDA backend holding both Generic (`sm_70`) and Handcrafted Ampere (`sm_86`) modules.
pub struct CudaBackend {
    device_info: CudaDeviceInfo,
    all_devices: Vec<CudaDeviceInfo>,
    ctx: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    _mod_generic: Arc<CudaModule>,
    generic: GenericFunctions,
    _mod_ampere: Option<Arc<CudaModule>>,
    ampere: Option<AmpereFunctions>,
    dispatch_lock: Mutex<()>,
}

static GLOBAL_CUDA: OnceLock<Option<CudaBackend>> = OnceLock::new();

impl CudaBackend {
    /// Returns the shared process-wide [`CudaBackend`] instance if an NVIDIA GPU is available.
    pub fn global() -> Option<&'static Self> {
        GLOBAL_CUDA.get_or_init(Self::init).as_ref()
    }

    /// Selects the highest-throughput CUDA device supporting at least `sm_70` (`compute_70` PTX),
    /// ranked by `(sm_count, compute_capability, total_vram_bytes)`.
    pub(crate) fn select_best_device(devices: &[CudaDeviceInfo]) -> Option<CudaDeviceInfo> {
        devices.iter().filter(|d| d.compute_capability >= (7, 0)).max_by_key(|d| (d.sm_count, d.compute_capability, d.total_vram_bytes)).cloned()
    }

    /// Probes all CUDA devices via `cudarc`, selects the highest-throughput GPU, and loads PTX modules.
    fn init() -> Option<Self> {
        // SAFETY: `is_culib_present` checks with `libloading` whether `libcuda` / `nvcuda` exists on the
        // host before invoking `cudarc`, preventing `cudarc`'s internal panic when no NVIDIA driver is installed.
        if !unsafe { is_culib_present() } {
            return None;
        }
        let count = CudaContext::device_count().ok()?;
        if count <= 0 {
            return None;
        }

        let mut all_devices = Vec::with_capacity(count as usize);
        for ordinal in 0..count {
            let Ok(dev) = cu_device::get(ordinal) else {
                continue;
            };
            let name = cu_device::get_name(dev).unwrap_or_else(|_| format!("CUDA Device {ordinal}"));
            let attr = |a: CUdevice_attribute| -> i32 {
                // SAFETY: `dev` is a valid device handle returned by `cu_device::get`.
                unsafe { cu_device::get_attribute(dev, a) }.unwrap_or(0)
            };
            let major = attr(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR);
            let minor = attr(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR);
            let sm_count = attr(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT);
            let clock_khz = attr(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_CLOCK_RATE);
            let memory_bus_bits = attr(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_GLOBAL_MEMORY_BUS_WIDTH);
            let l2_cache_bytes = attr(CUdevice_attribute::CU_DEVICE_ATTRIBUTE_L2_CACHE_SIZE);
            // SAFETY: `dev` is a valid device handle returned by `cu_device::get`.
            let total_vram_bytes = unsafe { cu_device::total_mem(dev) }.unwrap_or(0);

            let is_ampere_plus = major >= 8;
            let is_turing_plus = major > 7 || (major == 7 && minor >= 5);
            // Hardware F32 Tensor Cores (`mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32`)
            // are supported across all consumer and datacenter Ampere (sm_80, sm_86, sm_87),
            // Ada Lovelace (sm_89), Hopper (sm_90), and Blackwell (sm_100, sm_120) GPUs.
            let has_hw_f32_tensor_cores = is_ampere_plus;
            all_devices.push(CudaDeviceInfo {
                ordinal,
                name,
                compute_capability: (major, minor),
                sm_count,
                clock_khz,
                memory_bus_bits,
                l2_cache_bytes,
                total_vram_bytes,
                supports_ampere_ptx: is_ampere_plus,
                supports_f32_tensor_core: has_hw_f32_tensor_cores,
                supports_rt_core: is_turing_plus,
            });
        }

        // Select the highest-throughput GPU supporting at least sm_70 (by SM count, then compute capability, then VRAM).
        let best = Self::select_best_device(&all_devices)?;
        let ordinal = usize::try_from(best.ordinal).ok()?;
        let ctx = CudaContext::new(ordinal).ok()?;
        // SAFETY: All kernel launches and memory transfers in `CudaBackend` are serialized on `self.stream`
        // (`ctx.default_stream()`), so multi-stream `CUevent` tracking per buffer is unnecessary.
        unsafe { ctx.disable_event_tracking() };
        let stream = ctx.default_stream();

        let mod_generic = ctx.load_module(Ptx::from_src(PTX_SM70)).ok()?;
        let generic = GenericFunctions { eval_patch_f32: mod_generic.load_function("eval_bicubic_patch_f32_generic").ok()? };

        let (mod_ampere, ampere) = if best.supports_ampere_ptx {
            if let Ok(m86) = ctx.load_module(Ptx::from_src(PTX_SM86)) {
                let funcs = (|| -> Option<AmpereFunctions> {
                    Some(AmpereFunctions { eval_patch_f32_sm86: m86.load_function("eval_bicubic_patch_f32_sm86").ok()? })
                })();
                (Some(m86), funcs)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Some(Self {
            device_info: best,
            all_devices,
            ctx,
            stream,
            _mod_generic: mod_generic,
            generic,
            _mod_ampere: mod_ampere,
            ampere,
            dispatch_lock: Mutex::new(()),
        })
    }

    /// Information about the active primary CUDA GPU.
    pub fn device_info(&self) -> &CudaDeviceInfo {
        &self.device_info
    }

    /// Information about all discovered CUDA GPUs on the host.
    pub fn all_devices(&self) -> &[CudaDeviceInfo] {
        &self.all_devices
    }

    /// Whether the handcrafted Ampere `sm_86` PTX module is loaded and available.
    pub fn has_ampere_ptx(&self) -> bool {
        self.ampere.is_some()
    }

    /// Whether Ampere F32 Tensor Core (`mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32`) is available.
    pub fn has_f32_tensor_core(&self) -> bool {
        self.ampere.is_some() && self.device_info.supports_f32_tensor_core
    }

    /// Serializes stream dispatches across threads and binds the primary CUDA context to the calling OS thread.
    fn lock_and_bind(&self) -> Option<std::sync::MutexGuard<'_, ()>> {
        let guard = match self.dispatch_lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.ctx.bind_to_thread().ok()?;
        Some(guard)
    }

    /// Evaluates a batch of `f32` bicubic Bezier/B-spline patches (`use_ampere_sm86` selects `cp.async` PTX).
    pub fn eval_bicubic_f32(&self, patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32], use_ampere_sm86: bool) -> Option<Vec<PatchEvalOutF32>> {
        if queries.is_empty() || patch_cpts.len() < 16 {
            return Some(Vec::new());
        }
        let func = if use_ampere_sm86 { &self.ampere.as_ref()?.eval_patch_f32_sm86 } else { &self.generic.eval_patch_f32 };

        let num_patches = u32::try_from(patch_cpts.len() / 16).ok()?;
        let num_evals = u32::try_from(queries.len()).ok()?;

        let _guard = self.lock_and_bind()?;
        let d_cpts = self.stream.clone_htod(patch_cpts).ok()?;
        let d_q = self.stream.clone_htod(queries).ok()?;
        let mut d_out = self.stream.alloc_zeros::<PatchEvalOutF32>(queries.len()).ok()?;

        let block_x = 128u32;
        let cfg = LaunchConfig { grid_dim: (num_evals.div_ceil(block_x), 1, 1), block_dim: (block_x, 1, 1), shared_mem_bytes: 0 };

        // SAFETY: Kernel signature takes `(const float4*, const PatchUvQueryF32*, PatchEvalOutF32*, u32, u32)`.
        unsafe {
            self.stream.launch_builder(func).arg(&d_cpts).arg(&d_q).arg(&mut d_out).arg(&num_evals).arg(&num_patches).launch(cfg).ok()?;
        }

        self.stream.clone_dtoh(&d_out).ok()
    }
}
