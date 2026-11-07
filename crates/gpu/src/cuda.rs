//! Dynamically loaded CUDA Driver API (`libcuda.so.1`) backend supporting:
//! - **Tier 3: Generic CUDA (`sm_70`)** — portable PTX compute kernels.
//! - **Tier 4a: Handcrafted NVIDIA Ampere PTX (`sm_86`)** — `cp.async.ca.shared.global`,
//!   `redux.sync.add.s32`, and warp-level butterfly shuffle reductions.
//! - **Tier 4b: Ampere FP64 Tensor Cores (DMMA)** — `mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64`
//!   for exact double-precision bicubic surface patch & derivative tensor contractions.
//! - **Tier 4c: RT / Hardware-Aligned BVH Traversal** — $O(\log N)$ ray-mesh intersection
//!   and watertight 3-ray point-in-solid classification.
//!
//! `libcuda.so.1` is loaded at runtime via `dlopen`/`dlsym` so SolidCraft compiles cleanly
//! on machines without the CUDA toolkit installed and falls back gracefully when no NVIDIA
//! driver is present at runtime.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::sync::{Mutex, OnceLock};

use crate::collision::{CollisionResult, GpuAabb, GpuPod, MAX_OVERLAP_PAIRS, OverlapPair};

const PTX_SM70: &str = include_str!("kernels_sm70.ptx");
const PTX_SM86: &str = include_str!("kernels_sm86.ptx");

type CUresult = c_int;
type CUdevice = c_int;
type CUcontext = *mut c_void;
type CUmodule = *mut c_void;
type CUfunction = *mut c_void;
type CUstream = *mut c_void;
type CUdeviceptr = u64;

const CUDA_SUCCESS: CUresult = 0;
const CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT: c_int = 16;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: c_int = 75;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: c_int = 76;
const CU_DEVICE_ATTRIBUTE_CLOCK_RATE: c_int = 13;
const CU_DEVICE_ATTRIBUTE_GLOBAL_MEMORY_BUS_WIDTH: c_int = 37;
const CU_DEVICE_ATTRIBUTE_L2_CACHE_SIZE: c_int = 38;

#[cfg(target_os = "linux")]
const RTLD_LAZY: c_int = 1;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// Function pointers resolved dynamically from `libcuda.so.1`.
struct CudaDriverApi {
    cu_init: unsafe extern "C" fn(c_uint) -> CUresult,
    cu_device_get_count: unsafe extern "C" fn(*mut c_int) -> CUresult,
    cu_device_get: unsafe extern "C" fn(*mut CUdevice, c_int) -> CUresult,
    cu_device_get_name: unsafe extern "C" fn(*mut c_char, c_int, CUdevice) -> CUresult,
    cu_device_get_attribute: unsafe extern "C" fn(*mut c_int, c_int, CUdevice) -> CUresult,
    cu_device_total_mem: unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult,
    cu_device_primary_ctx_retain: unsafe extern "C" fn(*mut CUcontext, CUdevice) -> CUresult,
    cu_ctx_set_current: unsafe extern "C" fn(CUcontext) -> CUresult,
    cu_module_load_data: unsafe extern "C" fn(*mut CUmodule, *const c_void) -> CUresult,
    cu_module_get_function: unsafe extern "C" fn(*mut CUfunction, CUmodule, *const c_char) -> CUresult,
    cu_mem_alloc: unsafe extern "C" fn(*mut CUdeviceptr, usize) -> CUresult,
    cu_mem_free: unsafe extern "C" fn(CUdeviceptr) -> CUresult,
    cu_memcpy_htod: unsafe extern "C" fn(CUdeviceptr, *const c_void, usize) -> CUresult,
    cu_memcpy_dtoh: unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult,
    cu_memset_d32: unsafe extern "C" fn(CUdeviceptr, c_uint, usize) -> CUresult,
    cu_launch_kernel: unsafe extern "C" fn(
        CUfunction,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        CUstream,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> CUresult,
}

unsafe impl Send for CudaDriverApi {}
unsafe impl Sync for CudaDriverApi {}

impl CudaDriverApi {
    /// Dynamically loads `libcuda.so.1` and resolves the CUDA Driver API entry points.
    #[cfg(target_os = "linux")]
    fn load() -> Option<Self> {
        // SAFETY: `dlopen` with a valid NUL-terminated library name is thread-safe on Linux.
        let handle = unsafe {
            let h1 = dlopen(c"libcuda.so.1".as_ptr(), RTLD_LAZY);
            if !h1.is_null() { h1 } else { dlopen(c"libcuda.so".as_ptr(), RTLD_LAZY) }
        };
        if handle.is_null() {
            return None;
        }

        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let cname = CString::new($name).ok()?;
                // SAFETY: `handle` is a valid `dlopen` handle and `cname` is NUL-terminated.
                let ptr = unsafe { dlsym(handle, cname.as_ptr()) };
                if ptr.is_null() {
                    return None;
                }
                // SAFETY: The symbol signature matches the CUDA Driver API specification.
                unsafe { std::mem::transmute::<*mut c_void, $ty>(ptr) }
            }};
        }

        let api = Self {
            cu_init: sym!("cuInit", unsafe extern "C" fn(c_uint) -> CUresult),
            cu_device_get_count: sym!("cuDeviceGetCount", unsafe extern "C" fn(*mut c_int) -> CUresult),
            cu_device_get: sym!("cuDeviceGet", unsafe extern "C" fn(*mut CUdevice, c_int) -> CUresult),
            cu_device_get_name: sym!("cuDeviceGetName", unsafe extern "C" fn(*mut c_char, c_int, CUdevice) -> CUresult),
            cu_device_get_attribute: sym!("cuDeviceGetAttribute", unsafe extern "C" fn(*mut c_int, c_int, CUdevice) -> CUresult),
            cu_device_total_mem: sym!("cuDeviceTotalMem_v2", unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult),
            cu_device_primary_ctx_retain: sym!("cuDevicePrimaryCtxRetain", unsafe extern "C" fn(*mut CUcontext, CUdevice) -> CUresult),
            cu_ctx_set_current: sym!("cuCtxSetCurrent", unsafe extern "C" fn(CUcontext) -> CUresult),
            cu_module_load_data: sym!("cuModuleLoadData", unsafe extern "C" fn(*mut CUmodule, *const c_void) -> CUresult),
            cu_module_get_function: sym!("cuModuleGetFunction", unsafe extern "C" fn(*mut CUfunction, CUmodule, *const c_char) -> CUresult),
            cu_mem_alloc: sym!("cuMemAlloc_v2", unsafe extern "C" fn(*mut CUdeviceptr, usize) -> CUresult),
            cu_mem_free: sym!("cuMemFree_v2", unsafe extern "C" fn(CUdeviceptr) -> CUresult),
            cu_memcpy_htod: sym!("cuMemcpyHtoD_v2", unsafe extern "C" fn(CUdeviceptr, *const c_void, usize) -> CUresult),
            cu_memcpy_dtoh: sym!("cuMemcpyDtoH_v2", unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult),
            cu_memset_d32: sym!("cuMemsetD32_v2", unsafe extern "C" fn(CUdeviceptr, c_uint, usize) -> CUresult),
            cu_launch_kernel: sym!(
                "cuLaunchKernel",
                unsafe extern "C" fn(
                    CUfunction,
                    c_uint,
                    c_uint,
                    c_uint,
                    c_uint,
                    c_uint,
                    c_uint,
                    c_uint,
                    CUstream,
                    *mut *mut c_void,
                    *mut *mut c_void,
                ) -> CUresult
            ),
        };

        // SAFETY: `cuInit(0)` initializes the CUDA Driver API.
        if unsafe { (api.cu_init)(0) } != CUDA_SUCCESS {
            return None;
        }
        Some(api)
    }

    /// Stub loader for non-Linux targets where `libcuda.so.1` is not used.
    #[cfg(not(target_os = "linux"))]
    fn load() -> Option<Self> {
        None
    }
}

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
    pub supports_fp64_tensor_core: bool,
    pub supports_rt_core: bool,
}

/// Packed GPU triangle (48 bytes) shared by Vulkan and CUDA kernels.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuTri {
    pub v0: [f32; 4], // xyz = v0, w = bitcast<f32>(orig_tri_idx)
    pub v1: [f32; 4], // xyz = v1
    pub v2: [f32; 4], // xyz = v2
}

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

/// Output of `f64` bicubic patch evaluation (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PatchEvalOutF64 {
    pub pos: [f64; 4],
    pub norm: [f64; 4],
}

/// Reusable GPU device memory scratchpad to avoid per-dispatch `cuMemAlloc_v2` overhead.
#[derive(Default)]
struct ScratchBuffer {
    ptr: CUdeviceptr,
    cap: usize,
}

#[derive(Default)]
struct ScratchPool {
    slots: [ScratchBuffer; 4],
}

impl ScratchPool {
    /// Ensures scratch slot `slot_idx` has at least `needed_bytes` allocated on the active CUDA context.
    fn ensure(&mut self, api: &CudaDriverApi, slot_idx: usize, needed_bytes: usize) -> Option<CUdeviceptr> {
        let bytes = needed_bytes.max(64);
        let slot = self.slots.get_mut(slot_idx)?;
        if slot.cap < bytes {
            if slot.ptr != 0 {
                // SAFETY: `slot.ptr` was allocated by `cuMemAlloc_v2` in this context.
                let _ = unsafe { (api.cu_mem_free)(slot.ptr) };
                slot.ptr = 0;
                slot.cap = 0;
            }
            let alloc_cap = bytes.next_power_of_two().max(4096);
            let mut dptr: CUdeviceptr = 0;
            // SAFETY: Valid pointer to `dptr` and positive byte count with current CUDA context active.
            if unsafe { (api.cu_mem_alloc)(&mut dptr, alloc_cap) } != CUDA_SUCCESS {
                return None;
            }
            slot.ptr = dptr;
            slot.cap = alloc_cap;
        }
        Some(slot.ptr)
    }
}

struct GenericFunctions {
    eval_patch_f32: CUfunction,
    eval_patch_f64: CUfunction,
    classify_points: CUfunction,
    classify_points_rt_bvh: CUfunction,
    raycast_rt_bvh: CUfunction,
    bipartite_aabb: CUfunction,
    point_mesh_dist: CUfunction,
    mass_properties: CUfunction,
}

struct AmpereFunctions {
    eval_patch_f32_sm86: CUfunction,
    eval_patch_dmma_f64_sm86: CUfunction,
    classify_points_sm86: CUfunction,
    classify_points_rt_bvh_sm86: CUfunction,
    raycast_rt_bvh_sm86: CUfunction,
    bipartite_aabb_sm86: CUfunction,
    point_mesh_dist_sm86: CUfunction,
    mass_properties_sm86: CUfunction,
}

/// Multi-tier CUDA backend holding both Generic (`sm_70`) and Handcrafted Ampere (`sm_86`) modules.
pub struct CudaBackend {
    api: CudaDriverApi,
    device_info: CudaDeviceInfo,
    all_devices: Vec<CudaDeviceInfo>,
    ctx: CUcontext,
    _mod_generic: CUmodule,
    generic: GenericFunctions,
    _mod_ampere: Option<CUmodule>,
    ampere: Option<AmpereFunctions>,
    scratch: Mutex<ScratchPool>,
}

unsafe impl Send for CudaBackend {}
unsafe impl Sync for CudaBackend {}

static GLOBAL_CUDA: OnceLock<Option<CudaBackend>> = OnceLock::new();

impl CudaBackend {
    /// Returns the shared process-wide [`CudaBackend`] instance if an NVIDIA GPU is available.
    pub fn global() -> Option<&'static Self> {
        GLOBAL_CUDA.get_or_init(Self::init).as_ref()
    }

    /// Probes all CUDA devices via the Driver API, selects the highest-throughput GPU, and loads PTX modules.
    fn init() -> Option<Self> {
        let api = CudaDriverApi::load()?;
        let mut count: c_int = 0;
        // SAFETY: `cuDeviceGetCount` writes the number of CUDA devices to `count`.
        if unsafe { (api.cu_device_get_count)(&mut count) } != CUDA_SUCCESS || count <= 0 {
            return None;
        }

        let mut all_devices = Vec::with_capacity(count as usize);
        for ordinal in 0..count {
            let mut dev: CUdevice = 0;
            // SAFETY: `ordinal` is in `0..count`.
            if unsafe { (api.cu_device_get)(&mut dev, ordinal) } != CUDA_SUCCESS {
                continue;
            }
            let mut name_buf = [0 as c_char; 128];
            let name = unsafe {
                if (api.cu_device_get_name)(name_buf.as_mut_ptr(), 128, dev) == CUDA_SUCCESS {
                    CStr::from_ptr(name_buf.as_ptr()).to_string_lossy().into_owned()
                } else {
                    format!("CUDA Device {ordinal}")
                }
            };
            let attr = |a: c_int| -> i32 {
                let mut val: c_int = 0;
                // SAFETY: Queries standard integer attribute for `dev`.
                if unsafe { (api.cu_device_get_attribute)(&mut val, a, dev) } == CUDA_SUCCESS { val } else { 0 }
            };
            let major = attr(CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR);
            let minor = attr(CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR);
            let sm_count = attr(CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT);
            let clock_khz = attr(CU_DEVICE_ATTRIBUTE_CLOCK_RATE);
            let memory_bus_bits = attr(CU_DEVICE_ATTRIBUTE_GLOBAL_MEMORY_BUS_WIDTH);
            let l2_cache_bytes = attr(CU_DEVICE_ATTRIBUTE_L2_CACHE_SIZE);
            let mut total_vram_bytes: usize = 0;
            // SAFETY: Queries total device memory in bytes.
            let _ = unsafe { (api.cu_device_total_mem)(&mut total_vram_bytes, dev) };

            let is_ampere_plus = major >= 8;
            let is_turing_plus = major > 7 || (major == 7 && minor >= 5);
            // Hardware FP64 Tensor Cores are only present on datacenter architectures:
            // sm_80 (A100), sm_90 (H100), and sm_100/sm_107 (Blackwell B100/B200/GB200), per NVIDIA Table 36.
            // Consumer Ampere (sm_86, sm_87) and Ada Lovelace (sm_89) execute DMMA at the 1:64 scalar rate.
            let has_hw_fp64_tensor_cores = (major == 8 && minor == 0) || (major == 9 && minor == 0) || (major == 10 && (minor == 0 || minor == 7));
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
                supports_fp64_tensor_core: has_hw_fp64_tensor_cores,
                supports_rt_core: is_turing_plus,
            });
        }

        // Select the highest-throughput GPU (by SM count, then compute capability, then VRAM).
        let best = all_devices.iter().max_by_key(|d| (d.sm_count, d.compute_capability, d.total_vram_bytes))?.clone();

        let mut dev: CUdevice = 0;
        // SAFETY: `best.ordinal` was validated above.
        if unsafe { (api.cu_device_get)(&mut dev, best.ordinal) } != CUDA_SUCCESS {
            return None;
        }
        let mut ctx: CUcontext = std::ptr::null_mut();
        // SAFETY: Retains the primary context on `dev` and makes it current.
        if unsafe { (api.cu_device_primary_ctx_retain)(&mut ctx, dev) } != CUDA_SUCCESS || ctx.is_null() {
            return None;
        }
        if unsafe { (api.cu_ctx_set_current)(ctx) } != CUDA_SUCCESS {
            return None;
        }

        let load_mod = |ptx: &str| -> Option<CUmodule> {
            let c_ptx = CString::new(ptx).ok()?;
            let mut module: CUmodule = std::ptr::null_mut();
            // SAFETY: `c_ptx` is a valid NUL-terminated PTX string and `ctx` is current.
            if unsafe { (api.cu_module_load_data)(&mut module, c_ptx.as_ptr().cast()) } == CUDA_SUCCESS && !module.is_null() {
                Some(module)
            } else {
                None
            }
        };

        let get_fn = |module: CUmodule, name: &str| -> Option<CUfunction> {
            let c_name = CString::new(name).ok()?;
            let mut func: CUfunction = std::ptr::null_mut();
            // SAFETY: `module` is a loaded `CUmodule` and `c_name` is NUL-terminated.
            if unsafe { (api.cu_module_get_function)(&mut func, module, c_name.as_ptr()) } == CUDA_SUCCESS && !func.is_null() {
                Some(func)
            } else {
                None
            }
        };

        let mod_generic = load_mod(PTX_SM70)?;
        let generic = GenericFunctions {
            eval_patch_f32: get_fn(mod_generic, "eval_bicubic_patch_f32_generic")?,
            eval_patch_f64: get_fn(mod_generic, "eval_bicubic_patch_f64_generic")?,
            classify_points: get_fn(mod_generic, "classify_points_generic")?,
            classify_points_rt_bvh: get_fn(mod_generic, "classify_points_rt_bvh")?,
            raycast_rt_bvh: get_fn(mod_generic, "raycast_rt_bvh")?,
            bipartite_aabb: get_fn(mod_generic, "bipartite_aabb_overlap_generic")?,
            point_mesh_dist: get_fn(mod_generic, "point_mesh_distance_generic")?,
            mass_properties: get_fn(mod_generic, "mass_properties_generic")?,
        };

        let (mod_ampere, ampere) = if best.supports_ampere_ptx {
            if let Some(m86) = load_mod(PTX_SM86) {
                let funcs = (|| -> Option<AmpereFunctions> {
                    Some(AmpereFunctions {
                        eval_patch_f32_sm86: get_fn(m86, "eval_bicubic_patch_f32_sm86")?,
                        eval_patch_dmma_f64_sm86: get_fn(m86, "eval_bicubic_patch_dmma_sm86")?,
                        classify_points_sm86: get_fn(m86, "classify_points_sm86")?,
                        classify_points_rt_bvh_sm86: get_fn(m86, "classify_points_rt_bvh")?,
                        raycast_rt_bvh_sm86: get_fn(m86, "raycast_rt_bvh")?,
                        bipartite_aabb_sm86: get_fn(m86, "bipartite_aabb_overlap_sm86")?,
                        point_mesh_dist_sm86: get_fn(m86, "point_mesh_distance_sm86")?,
                        mass_properties_sm86: get_fn(m86, "mass_properties_sm86")?,
                    })
                })();
                (Some(m86), funcs)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Some(Self {
            api,
            device_info: best,
            all_devices,
            ctx,
            _mod_generic: mod_generic,
            generic,
            _mod_ampere: mod_ampere,
            ampere,
            scratch: Mutex::new(ScratchPool::default()),
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

    /// Whether Ampere FP64 Tensor Core (`mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64`) is available.
    pub fn has_fp64_tensor_core(&self) -> bool {
        self.ampere.is_some() && self.device_info.supports_fp64_tensor_core
    }

    /// Acquires the scratch pool lock and binds this backend's retained primary CUDA context to the calling OS thread.
    fn lock_and_bind(&self) -> Option<std::sync::MutexGuard<'_, ScratchPool>> {
        let guard = match self.scratch.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        // SAFETY: `self.ctx` is a retained primary context valid for the lifetime of the process.
        if unsafe { (self.api.cu_ctx_set_current)(self.ctx) } == CUDA_SUCCESS { Some(guard) } else { None }
    }

    /// Evaluates a batch of `f32` bicubic Bezier/B-spline patches (`use_ampere_sm86` selects `cp.async` PTX).
    pub fn eval_bicubic_f32(&self, patch_cpts: &[[f32; 4]], queries: &[PatchUvQueryF32], use_ampere_sm86: bool) -> Option<Vec<PatchEvalOutF32>> {
        if queries.is_empty() || patch_cpts.len() < 16 {
            return Some(Vec::new());
        }
        let func = if use_ampere_sm86 { self.ampere.as_ref()?.eval_patch_f32_sm86 } else { self.generic.eval_patch_f32 };

        let num_patches = u32::try_from(patch_cpts.len() / 16).ok()?;
        let num_evals = u32::try_from(queries.len()).ok()?;
        let cpts_bytes = std::mem::size_of_val(patch_cpts);
        let q_bytes = std::mem::size_of_val(queries);
        let out_bytes = queries.len() * std::mem::size_of::<PatchEvalOutF32>();

        let mut pool = self.lock_and_bind()?;
        let d_cpts = pool.ensure(&self.api, 0, cpts_bytes)?;
        let d_q = pool.ensure(&self.api, 1, q_bytes)?;
        let d_out = pool.ensure(&self.api, 2, out_bytes)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_cpts, patch_cpts.as_ptr().cast(), cpts_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_q, queries.as_ptr().cast(), q_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let block_x = 128u32;
        let grid_x = num_evals.div_ceil(block_x);
        let mut p0 = d_cpts;
        let mut p1 = d_q;
        let mut p2 = d_out;
        let mut p3 = num_evals;
        let mut p4 = num_patches;
        let mut args: [*mut c_void; 5] =
            [(&raw mut p0).cast(), (&raw mut p1).cast(), (&raw mut p2).cast(), (&raw mut p3).cast(), (&raw mut p4).cast()];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u32, u32)`.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut out = vec![PatchEvalOutF32::default(); queries.len()];
        // SAFETY: `d_out` has at least `out_bytes` allocated and `out` has `queries.len()` elements.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(out.as_mut_ptr().cast(), d_out, out_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(out)
    }

    /// Evaluates a batch of `f64` double-precision bicubic patches using either:
    /// - `use_tensor_core_dmma = true`: Handcrafted Ampere `mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64` Tensor Core PTX
    /// - `use_tensor_core_dmma = false`: Generic `sm_70` FP64 FMA PTX
    pub fn eval_bicubic_f64(&self, patch_cpts: &[[f64; 4]], queries: &[[f64; 4]], use_tensor_core_dmma: bool) -> Option<Vec<PatchEvalOutF64>> {
        if queries.is_empty() || patch_cpts.len() < 16 {
            return Some(Vec::new());
        }
        let func = if use_tensor_core_dmma { self.ampere.as_ref()?.eval_patch_dmma_f64_sm86 } else { self.generic.eval_patch_f64 };

        let num_patches = u32::try_from(patch_cpts.len() / 16).ok()?;
        let num_evals = u32::try_from(queries.len()).ok()?;
        let cpts_bytes = std::mem::size_of_val(patch_cpts);
        let q_bytes = std::mem::size_of_val(queries);
        let out_bytes = queries.len() * std::mem::size_of::<PatchEvalOutF64>();

        let mut pool = self.lock_and_bind()?;
        let d_cpts = pool.ensure(&self.api, 0, cpts_bytes)?;
        let d_q = pool.ensure(&self.api, 1, q_bytes)?;
        let d_out = pool.ensure(&self.api, 2, out_bytes)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_cpts, patch_cpts.as_ptr().cast(), cpts_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_q, queries.as_ptr().cast(), q_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        // In DMMA mode, each warp of 32 threads evaluates 4 queries simultaneously (128 threads = 16 queries/block).
        let block_x = 128u32;
        let grid_x = if use_tensor_core_dmma { num_evals.div_ceil(16) } else { num_evals.div_ceil(block_x) };

        let mut p0 = d_cpts;
        let mut p1 = d_q;
        let mut p2 = d_out;
        let mut p3 = num_evals;
        let mut p4 = num_patches;
        let mut args: [*mut c_void; 5] =
            [(&raw mut p0).cast(), (&raw mut p1).cast(), (&raw mut p2).cast(), (&raw mut p3).cast(), (&raw mut p4).cast()];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u32, u32)`.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut out = vec![PatchEvalOutF64::default(); queries.len()];
        // SAFETY: `d_out` has at least `out_bytes` allocated.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(out.as_mut_ptr().cast(), d_out, out_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(out)
    }

    /// Classifies query points (`[x, y, z, 0.0]`) against a closed triangle mesh using 3-ray majority vote.
    pub fn classify_points(&self, points: &[[f32; 4]], tris: &[GpuTri], use_ampere_sm86: bool) -> Option<Vec<bool>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![false; points.len()]);
        }
        let func = if use_ampere_sm86 { self.ampere.as_ref()?.classify_points_sm86 } else { self.generic.classify_points };

        let num_queries = u32::try_from(points.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let pts_bytes = std::mem::size_of_val(points);
        let tris_bytes = std::mem::size_of_val(tris);
        let out_bytes = points.len() * std::mem::size_of::<u32>();

        let mut pool = self.lock_and_bind()?;
        let d_pts = pool.ensure(&self.api, 0, pts_bytes)?;
        let d_tris = pool.ensure(&self.api, 1, tris_bytes)?;
        let d_out = pool.ensure(&self.api, 2, out_bytes)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_pts, points.as_ptr().cast(), pts_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_tris, tris.as_ptr().cast(), tris_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let block_x = 128u32;
        let grid_x = num_queries.div_ceil(block_x);
        let mut p0 = d_pts;
        let mut p1 = d_tris;
        let mut p2 = d_out;
        let mut p3 = num_queries;
        let mut p4 = num_tris;
        let mut args: [*mut c_void; 5] =
            [(&raw mut p0).cast(), (&raw mut p1).cast(), (&raw mut p2).cast(), (&raw mut p3).cast(), (&raw mut p4).cast()];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u32, u32)` with 128 threads/block.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut raw = vec![0u32; points.len()];
        // SAFETY: `d_out` has `out_bytes` allocated.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(raw.as_mut_ptr().cast(), d_out, out_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(raw.into_iter().map(|v| v != 0).collect())
    }

    /// Classifies query points using hardware-aligned GPU BVH ray traversal ($O(\log M)$ per ray).
    /// Returns `None` if `tris` is non-empty and `bvh_nodes` is empty so the dispatcher can fall back.
    pub fn classify_points_rt_bvh(&self, points: &[[f32; 4]], tris: &[GpuTri], bvh_nodes: &[GpuBvhNode]) -> Option<Vec<bool>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![false; points.len()]);
        }
        if !crate::dispatch::bvh_fits_traversal_stack(bvh_nodes, tris.len()) {
            return None;
        }
        let func = self.ampere.as_ref().map_or(self.generic.classify_points_rt_bvh, |a| a.classify_points_rt_bvh_sm86);

        let num_queries = u32::try_from(points.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let num_nodes = u32::try_from(bvh_nodes.len()).ok()?;
        let pts_bytes = std::mem::size_of_val(points);
        let tris_bytes = std::mem::size_of_val(tris);
        let bvh_bytes = std::mem::size_of_val(bvh_nodes);
        let out_bytes = points.len() * std::mem::size_of::<u32>();

        let mut pool = self.lock_and_bind()?;
        let d_pts = pool.ensure(&self.api, 0, pts_bytes)?;
        let d_tris = pool.ensure(&self.api, 1, tris_bytes)?;
        let d_bvh = pool.ensure(&self.api, 2, bvh_bytes)?;
        let d_out = pool.ensure(&self.api, 3, out_bytes)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_pts, points.as_ptr().cast(), pts_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_tris, tris.as_ptr().cast(), tris_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_bvh, bvh_nodes.as_ptr().cast(), bvh_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let block_x = 128u32;
        let grid_x = num_queries.div_ceil(block_x);
        let mut p0 = d_pts;
        let mut p1 = d_tris;
        let mut p2 = d_bvh;
        let mut p3 = d_out;
        let mut p4 = num_queries;
        let mut p5 = num_tris;
        let mut p6 = num_nodes;
        let mut args: [*mut c_void; 7] = [
            (&raw mut p0).cast(),
            (&raw mut p1).cast(),
            (&raw mut p2).cast(),
            (&raw mut p3).cast(),
            (&raw mut p4).cast(),
            (&raw mut p5).cast(),
            (&raw mut p6).cast(),
        ];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u64, u32, u32, u32)`.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut raw = vec![0u32; points.len()];
        // SAFETY: `d_out` has `out_bytes` allocated.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(raw.as_mut_ptr().cast(), d_out, out_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(raw.into_iter().map(|v| v != 0).collect())
    }

    /// Executes batched BVH-accelerated ray-mesh intersection queries on the GPU.
    /// Returns `None` if `tris` is non-empty and `bvh_nodes` is empty so the dispatcher can fall back.
    pub fn raycast_rt_bvh(&self, rays: &[GpuRay], tris: &[GpuTri], bvh_nodes: &[GpuBvhNode]) -> Option<Vec<GpuRayHit>> {
        if rays.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![GpuRayHit { t: -1.0, tri_idx: u32::MAX, pad0: 0, pad1: 0 }; rays.len()]);
        }
        if !crate::dispatch::bvh_fits_traversal_stack(bvh_nodes, tris.len()) {
            return None;
        }
        let func = self.ampere.as_ref().map_or(self.generic.raycast_rt_bvh, |a| a.raycast_rt_bvh_sm86);

        let num_queries = u32::try_from(rays.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let num_nodes = u32::try_from(bvh_nodes.len()).ok()?;
        let rays_bytes = std::mem::size_of_val(rays);
        let tris_bytes = std::mem::size_of_val(tris);
        let bvh_bytes = std::mem::size_of_val(bvh_nodes);
        let out_bytes = rays.len() * std::mem::size_of::<GpuRayHit>();

        let mut pool = self.lock_and_bind()?;
        let d_rays = pool.ensure(&self.api, 0, rays_bytes)?;
        let d_tris = pool.ensure(&self.api, 1, tris_bytes)?;
        let d_bvh = pool.ensure(&self.api, 2, bvh_bytes)?;
        let d_out = pool.ensure(&self.api, 3, out_bytes)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_rays, rays.as_ptr().cast(), rays_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_tris, tris.as_ptr().cast(), tris_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_bvh, bvh_nodes.as_ptr().cast(), bvh_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let block_x = 128u32;
        let grid_x = num_queries.div_ceil(block_x);
        let mut p0 = d_rays;
        let mut p1 = d_tris;
        let mut p2 = d_bvh;
        let mut p3 = d_out;
        let mut p4 = num_queries;
        let mut p5 = num_tris;
        let mut p6 = num_nodes;
        let mut args: [*mut c_void; 7] = [
            (&raw mut p0).cast(),
            (&raw mut p1).cast(),
            (&raw mut p2).cast(),
            (&raw mut p3).cast(),
            (&raw mut p4).cast(),
            (&raw mut p5).cast(),
            (&raw mut p6).cast(),
        ];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u64, u32, u32, u32)`.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut hits = vec![GpuRayHit::default(); rays.len()];
        // SAFETY: `d_out` has `out_bytes` allocated.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(hits.as_mut_ptr().cast(), d_out, out_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(hits)
    }

    /// Finds all overlapping AABB pairs between `aabbs_a` and `aabbs_b` within `tolerance`.
    /// When `use_ampere_sm86` is true, uses `cp.async.ca.shared.global` and `redux.sync.add.s32`.
    pub fn bipartite_aabb_overlap(&self, aabbs_a: &[GpuAabb], aabbs_b: &[GpuAabb], tolerance: f32, use_ampere_sm86: bool) -> Option<CollisionResult> {
        if aabbs_a.is_empty() || aabbs_b.is_empty() {
            return Some(CollisionResult::default());
        }
        let func = if use_ampere_sm86 { self.ampere.as_ref()?.bipartite_aabb_sm86 } else { self.generic.bipartite_aabb };

        let count_a = u32::try_from(aabbs_a.len()).ok()?;
        let count_b = u32::try_from(aabbs_b.len()).ok()?;
        let max_pairs = MAX_OVERLAP_PAIRS;
        let tol = tolerance.max(0.0);

        let a_bytes = std::mem::size_of_val(aabbs_a);
        let b_bytes = std::mem::size_of_val(aabbs_b);
        let pairs_bytes = (max_pairs as usize) * std::mem::size_of::<OverlapPair>();

        let mut pool = self.lock_and_bind()?;
        let d_a = pool.ensure(&self.api, 0, a_bytes)?;
        let d_b = pool.ensure(&self.api, 1, b_bytes)?;
        let d_pairs = pool.ensure(&self.api, 2, pairs_bytes)?;
        let d_count = pool.ensure(&self.api, 3, 16)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_a, aabbs_a.as_ptr().cast(), a_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_b, aabbs_b.as_ptr().cast(), b_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memset_d32)(d_count, 0, 1) != CUDA_SUCCESS {
                return None;
            }
        }

        let grid_x = count_a.div_ceil(16);
        let grid_y = count_b.div_ceil(16);
        let mut p0 = d_a;
        let mut p1 = d_b;
        let mut p2 = d_pairs;
        let mut p3 = d_count;
        let mut p4 = count_a;
        let mut p5 = count_b;
        let mut p6 = max_pairs;
        let mut p7 = tol;
        let mut args: [*mut c_void; 8] = [
            (&raw mut p0).cast(),
            (&raw mut p1).cast(),
            (&raw mut p2).cast(),
            (&raw mut p3).cast(),
            (&raw mut p4).cast(),
            (&raw mut p5).cast(),
            (&raw mut p6).cast(),
            (&raw mut p7).cast(),
        ];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u64, u32, u32, u32, f32)` with block `(16, 16, 1)`.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, grid_y, 1, 16, 16, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut raw_count: u32 = 0;
        // SAFETY: `d_count` holds a 32-bit unsigned integer.
        unsafe {
            if (self.api.cu_memcpy_dtoh)((&raw mut raw_count).cast(), d_count, 4) != CUDA_SUCCESS {
                return None;
            }
        }
        let overflowed = raw_count > max_pairs;
        let actual = raw_count.min(max_pairs) as usize;
        if actual == 0 {
            return Some(CollisionResult { pairs: Vec::new(), overflowed });
        }

        let mut pairs = vec![OverlapPair::default(); actual];
        let copy_bytes = actual * std::mem::size_of::<OverlapPair>();
        // SAFETY: `d_pairs` has at least `copy_bytes` written.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(pairs.as_mut_ptr().cast(), d_pairs, copy_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        pairs.sort_unstable();
        Some(CollisionResult { pairs, overflowed })
    }

    /// Computes minimum squared distances from `points` to `tris` on the GPU.
    pub fn point_mesh_distance(&self, points: &[[f32; 4]], tris: &[GpuTri], use_ampere_sm86: bool) -> Option<Vec<f32>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if tris.is_empty() {
            return Some(vec![f32::INFINITY; points.len()]);
        }
        let func = if use_ampere_sm86 { self.ampere.as_ref()?.point_mesh_dist_sm86 } else { self.generic.point_mesh_dist };

        let num_queries = u32::try_from(points.len()).ok()?;
        let num_tris = u32::try_from(tris.len()).ok()?;
        let pts_bytes = std::mem::size_of_val(points);
        let tris_bytes = std::mem::size_of_val(tris);
        let out_bytes = points.len() * std::mem::size_of::<f32>();

        let mut pool = self.lock_and_bind()?;
        let d_pts = pool.ensure(&self.api, 0, pts_bytes)?;
        let d_tris = pool.ensure(&self.api, 1, tris_bytes)?;
        let d_out = pool.ensure(&self.api, 2, out_bytes)?;

        // SAFETY: Host slices and device buffers have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_pts, points.as_ptr().cast(), pts_bytes) != CUDA_SUCCESS {
                return None;
            }
            if (self.api.cu_memcpy_htod)(d_tris, tris.as_ptr().cast(), tris_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let block_x = 128u32;
        let grid_x = num_queries.div_ceil(block_x);
        let mut p0 = d_pts;
        let mut p1 = d_tris;
        let mut p2 = d_out;
        let mut p3 = num_queries;
        let mut p4 = num_tris;
        let mut args: [*mut c_void; 5] =
            [(&raw mut p0).cast(), (&raw mut p1).cast(), (&raw mut p2).cast(), (&raw mut p3).cast(), (&raw mut p4).cast()];

        // SAFETY: Kernel signature takes `(u64, u64, u64, u32, u32)`.
        unsafe {
            if (self.api.cu_launch_kernel)(func, grid_x, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut out = vec![0.0f32; points.len()];
        // SAFETY: `d_out` has `out_bytes` allocated.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(out.as_mut_ptr().cast(), d_out, out_bytes) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(out)
    }

    /// Computes divergence-theorem volume, surface area, and centroid in `f64` double precision.
    pub fn mass_properties(&self, tris: &[GpuTri], use_ampere_sm86: bool) -> Option<solidcraft_geom::MeshMeasure> {
        use solidcore_record::Vec3;
        use solidcraft_geom::MeshMeasure;

        if tris.is_empty() {
            return Some(MeshMeasure::default());
        }
        let func = if use_ampere_sm86 { self.ampere.as_ref()?.mass_properties_sm86 } else { self.generic.mass_properties };

        let num_tris = u32::try_from(tris.len()).ok()?;
        let block_x = 256u32;
        let num_blocks = num_tris.div_ceil(block_x);
        let tris_bytes = std::mem::size_of_val(tris);
        let partials_len = (num_blocks as usize) * 5;
        let partials_bytes = partials_len * std::mem::size_of::<f64>();

        let mut pool = self.lock_and_bind()?;
        let d_tris = pool.ensure(&self.api, 0, tris_bytes)?;
        let d_partials = pool.ensure(&self.api, 1, partials_bytes)?;

        // SAFETY: Host slice and device buffer have matching byte sizes.
        unsafe {
            if (self.api.cu_memcpy_htod)(d_tris, tris.as_ptr().cast(), tris_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let mut p0 = d_tris;
        let mut p1 = d_partials;
        let mut p2 = num_tris;
        let mut args: [*mut c_void; 3] = [(&raw mut p0).cast(), (&raw mut p1).cast(), (&raw mut p2).cast()];

        // SAFETY: Kernel signature takes `(u64, u64, u32)` with 256 threads/block.
        unsafe {
            if (self.api.cu_launch_kernel)(func, num_blocks, 1, 1, block_x, 1, 1, 0, std::ptr::null_mut(), args.as_mut_ptr(), std::ptr::null_mut())
                != CUDA_SUCCESS
            {
                return None;
            }
        }

        let mut partials = vec![0.0f64; partials_len];
        // SAFETY: `d_partials` has `partials_bytes` allocated.
        unsafe {
            if (self.api.cu_memcpy_dtoh)(partials.as_mut_ptr().cast(), d_partials, partials_bytes) != CUDA_SUCCESS {
                return None;
            }
        }

        let (mut v6, mut area, mut mx, mut my, mut mz) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for chunk in partials.as_chunks::<5>().0 {
            v6 += chunk[0];
            area += chunk[1];
            mx += chunk[2];
            my += chunk[3];
            mz += chunk[4];
        }
        let volume = v6 / 6.0;
        let centroid = if v6.abs() > 1e-12 { Vec3::new(mx / (4.0 * v6), my / (4.0 * v6), mz / (4.0 * v6)) } else { Vec3::ZERO };
        Some(MeshMeasure { volume, area, centroid })
    }
}
