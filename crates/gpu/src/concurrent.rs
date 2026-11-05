//! Pipelines built on several threads at once (#4860).
//!
//! Direct3D 12 compiles a pipeline's shaders when the pipeline is made (wgpu writes the WGSL out
//! as HLSL and compiles that with FXC), and nothing compiled is kept between launches, so the
//! viewport's pipelines, built one after another, held the window back for seconds. Work
//! handed to [`Jobs::spawn`] runs on a thread of its own, and [`Job::join`] collects it.
//!
//! - wgpu's error scopes belong to the thread that pushed them, so a job runs under scopes of its
//!   own. A job that raised an error, or whose thread panicked or could not start, runs again on
//!   the calling thread when joined: its errors reach the caller's scopes and the device's handler
//!   as they did when every pipeline was built there.
//! - Threads only on Vulkan, Metal and Direct3D 12 with FXC, whose devices make pipelines from any
//!   thread. wgpu hands every thread one DXC compiler, which is not documented as safe to share,
//!   and OpenGL works through one context. There, and in the browser, which has no threads, each
//!   job runs when it is joined, one after another, as before.

#[cfg(target_arch = "wasm32")]
use std::marker::PhantomData;

/// The name of the threads jobs run on (in crash reports and debuggers).
#[cfg(not(target_arch = "wasm32"))]
const WORKER: &str = "solidcraft pipelines";

/// Jobs on `device`, collected before [`scope`] returns.
pub struct Jobs<'scope, 'env> {
    device: &'env wgpu::Device,
    /// The threads' scope; `None` where jobs run on the calling thread.
    #[cfg(not(target_arch = "wasm32"))]
    threads: Option<&'scope std::thread::Scope<'scope, 'env>>,
    #[cfg(target_arch = "wasm32")]
    threads: PhantomData<&'scope ()>,
}

/// Work handed to [`Jobs::spawn`].
pub struct Job<'scope, T, F> {
    work: F,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<std::thread::ScopedJoinHandle<'scope, Option<T>>>,
    #[cfg(target_arch = "wasm32")]
    worker: PhantomData<(&'scope (), T)>,
}

/// Runs `f` with a [`Jobs`] on `device`; every job is joined before this returns.
pub fn scope<'env, R>(device: &'env wgpu::Device, f: impl for<'scope> FnOnce(&Jobs<'scope, 'env>) -> R) -> R {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threaded = threads_allowed(device);
        std::thread::scope(|s| f(&Jobs { device, threads: threaded.then_some(s) }))
    }
    #[cfg(target_arch = "wasm32")]
    {
        f(&Jobs { device, threads: PhantomData })
    }
}

/// `device` makes pipelines on several threads at once (see the module); the shader of the GPU
/// plan evaluation compiles on a thread of its own by the same rule (`view::TessLater`).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn threads_allowed(device: &wgpu::Device) -> bool {
    threaded(device.adapter_info().backend, dx12_compiles_with_fxc)
}

/// Backends whose devices make pipelines on several threads at once; D3D12 when `fxc` says it
/// compiles with FXC.
#[cfg(not(target_arch = "wasm32"))]
fn threaded(backend: wgpu::Backend, fxc: impl FnOnce() -> bool) -> bool {
    match backend {
        wgpu::Backend::Vulkan | wgpu::Backend::Metal => true,
        wgpu::Backend::Dx12 => fxc(),
        _ => false,
    }
}

/// D3D12 compiles with FXC: the compiler every instance starts with ([`crate::dx12_compiler`]).
#[cfg(not(target_arch = "wasm32"))]
fn dx12_compiles_with_fxc() -> bool {
    matches!(crate::dx12_compiler(), wgpu::Dx12Compiler::Fxc)
}

impl<'scope, 'env> Jobs<'scope, 'env> {
    /// Start `work` on a thread of its own (or later, on the calling thread: see the module).
    /// `work` may run twice, so it only builds what it returns.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn spawn<T, F>(&self, work: F) -> Job<'scope, T, F>
    where
        T: Send + 'scope,
        F: Fn() -> T + Copy + Send + 'scope,
    {
        let device = self.device;
        let worker = self.threads.and_then(|s| std::thread::Builder::new().name(WORKER.into()).spawn_scoped(s, move || checked(device, work)).ok());
        Job { work, worker }
    }

    /// Run `work` when it is joined: the browser has no threads.
    #[cfg(target_arch = "wasm32")]
    pub fn spawn<T, F: Fn() -> T>(&self, work: F) -> Job<'scope, T, F> {
        let _ = self.device;
        Job { work, worker: PhantomData }
    }
}

impl<T, F: Fn() -> T> Job<'_, T, F> {
    /// What the job built: on its thread, or now on this one (see the module).
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))] // Only native jobs have a thread to take.
    pub fn join(mut self) -> T {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(built) = self.worker.take().and_then(|w| w.join().ok().flatten()) {
            return built;
        }
        (self.work)()
    }
}

/// A job dropped unjoined still waits for its thread: the scope would otherwise panic at its end
/// if that thread had panicked.
#[cfg(not(target_arch = "wasm32"))]
impl<T, F> Drop for Job<'_, T, F> {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// `work`'s result, or `None` when it raised a GPU error on this thread (logged: the job runs
/// again on the calling thread, where the error may not recur).
#[cfg(not(target_arch = "wasm32"))]
fn checked<T>(device: &wgpu::Device, work: impl Fn() -> T) -> Option<T> {
    let scopes = crate::error_scopes(device);
    let built = work();
    match crate::caught(scopes) {
        Some(e) => {
            log::warn!("GPU pipelines built again on the calling thread: {e}");
            None
        }
        None => Some(built),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn only_backends_that_make_pipelines_on_any_thread_get_threads() {
        for fxc in [true, false] {
            assert!(threaded(wgpu::Backend::Vulkan, || fxc));
            assert!(threaded(wgpu::Backend::Metal, || fxc));
            assert!(!threaded(wgpu::Backend::Gl, || fxc));
            assert!(!threaded(wgpu::Backend::BrowserWebGpu, || fxc));
            assert!(!threaded(wgpu::Backend::Noop, || fxc));
        }
        assert!(threaded(wgpu::Backend::Dx12, || true), "D3D12 with FXC");
        assert!(!threaded(wgpu::Backend::Dx12, || false), "D3D12 with DXC");
    }

    #[test]
    fn d3d12_never_loads_a_dxcompiler_found_on_path_unless_asked_to() {
        let beside = || Some(std::path::PathBuf::from(r"C:\Program Files\SolidCraft\dxcompiler.dll"));
        let is_fxc = |c: &wgpu::Dx12Compiler| matches!(c, wgpu::Dx12Compiler::Fxc);
        let by_name = |c: &wgpu::Dx12Compiler| matches!(c, wgpu::Dx12Compiler::DynamicDxc { dxc_path } if dxc_path == "dxcompiler.dll");
        let beside_program =
            |c: &wgpu::Dx12Compiler| matches!(c, wgpu::Dx12Compiler::DynamicDxc { dxc_path } if dxc_path.ends_with(r"SolidCraft\dxcompiler.dll"));
        // Left to SolidCraft (#6756): FXC, or DXC from the program's folder by its full path,
        // never a `dxcompiler.dll` Windows would find on PATH. wgpu does not trim the value: " fxc "
        // is a value it does not know.
        for setting in [None, Some("auto"), Some("AUTO"), Some(""), Some("unknown"), Some(" fxc ")] {
            assert!(is_fxc(&crate::dx12_compiler_for(setting, None)), "{setting:?} without DXC beside the program: FXC");
            assert!(beside_program(&crate::dx12_compiler_for(setting, beside())), "{setting:?} with DXC beside the program: that one");
        }
        // Named compilers stay as wgpu reads them; `dxc` keeps loading the DLL by name.
        for setting in [Some("fxc"), Some("FXC")] {
            assert!(is_fxc(&crate::dx12_compiler_for(setting, beside())), "{setting:?} keeps FXC even with DXC beside the program");
        }
        for setting in [Some("dxc"), Some("DynamicDxc")] {
            assert!(by_name(&crate::dx12_compiler_for(setting, None)), "{setting:?} loads dxcompiler.dll by name");
        }
        assert!(matches!(crate::dx12_compiler_for(Some("staticdxc"), None), wgpu::Dx12Compiler::StaticDxc));
    }

    /// A buffer wgpu refuses: mapped for reading and for writing at once.
    fn refused_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refused"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        })
    }

    #[test]
    fn jobs_hand_back_what_they_built() {
        let Ok((_, device, _)) = crate::open_headless() else {
            solidcraft_geom::note!("skipped: no GPU adapter");
            return;
        };
        let runs = AtomicUsize::new(0);
        let built = scope(&device, |jobs| {
            let a = jobs.spawn(|| {
                runs.fetch_add(1, Ordering::Relaxed);
                1
            });
            let b = jobs.spawn(|| {
                runs.fetch_add(1, Ordering::Relaxed);
                2
            });
            (a.join(), b.join())
        });
        assert_eq!(built, (1, 2));
        assert_eq!(runs.load(Ordering::Relaxed), 2, "a job that went well runs once");
    }

    #[test]
    fn an_error_on_a_job_reaches_the_callers_error_scope() {
        let Ok((_, device, _)) = crate::open_headless() else {
            solidcraft_geom::note!("skipped: no GPU adapter");
            return;
        };
        let runs = AtomicUsize::new(0);
        let caller = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let device_ref = &device;
        let runs_ref = &runs;
        scope(&device, |jobs| {
            let job = jobs.spawn(move || {
                runs_ref.fetch_add(1, Ordering::Relaxed);
                refused_buffer(device_ref)
            });
            drop(job.join());
        });
        let error = crate::popped(caller.pop());
        assert!(error.is_some(), "the caller's scope catches the job's error");
        let expected = if threaded(device.adapter_info().backend, dx12_compiles_with_fxc) { 2 } else { 1 };
        assert_eq!(runs.load(Ordering::Relaxed), expected, "the job ran again on the calling thread");
    }

    #[test]
    fn a_job_dropped_unjoined_whose_thread_panicked_does_not_panic_the_scope() {
        let Ok((_, device, _)) = crate::open_headless() else {
            solidcraft_geom::note!("skipped: no GPU adapter");
            return;
        };
        let finished = scope(&device, |jobs| {
            drop(jobs.spawn(|| {
                if std::thread::current().name() == Some(WORKER) {
                    panic!("a job's thread panicked");
                }
            }));
            true
        });
        assert!(finished);
    }

    #[test]
    fn a_job_whose_thread_panicked_runs_again_on_the_calling_thread() {
        let Ok((_, device, _)) = crate::open_headless() else {
            solidcraft_geom::note!("skipped: no GPU adapter");
            return;
        };
        let built = scope(&device, |jobs| {
            let job = jobs.spawn(|| {
                if std::thread::current().name() == Some(WORKER) {
                    panic!("a job's thread panicked");
                }
                7
            });
            job.join()
        });
        assert_eq!(built, 7);
    }
}
