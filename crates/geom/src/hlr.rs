//! Exact orthographic hidden lines for placed portable B-rep bodies, and the silhouettes of their
//! curved faces: solidcore's (`solidcore_ops::hlr`), under their old path. See
//! `docs/drawing-hlr.md` for the contract and its measured limits. The kernel starts no threads;
//! the application's [`hidden_lines`] runs its work on [`Threads`].

pub use solidcore_ops::hlr::*;

/// Worker threads for the kernel's hidden lines, named `solidcraft-hlr`. The browser build has
/// none: there the work runs on the calling thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct Threads;

impl Workers for Threads {
    #[cfg(not(target_arch = "wasm32"))]
    fn run(&self, count: usize, work: &(dyn Fn() + Sync)) -> bool {
        std::thread::scope(|scope| {
            let started: Vec<_> =
                (0..count).filter_map(|_| std::thread::Builder::new().name("solidcraft-hlr".into()).spawn_scoped(scope, work).ok()).collect();
            let any = !started.is_empty();
            for worker in started {
                // A worker that panicked lost its items, which the kernel reports as an error.
                let _ = worker.join();
            }
            any
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn run(&self, _count: usize, _work: &(dyn Fn() + Sync)) -> bool {
        false
    }
}

/// The hidden-line drawing of the scene ([`solidcore_ops::hlr::hidden_lines`]), its work on up
/// to [`Options::threads`] [`Threads`].
///
/// # Errors
/// The kernel's refusal: an invalid placement or view, a budget exceeded, cancelled.
pub fn hidden_lines(
    defs: &[Prepared],
    scene: &[Placement],
    view: &View,
    opts: &Options,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Drawing, HlrError> {
    hidden_lines_on(defs, scene, view, opts, cancel, &Threads)
}
