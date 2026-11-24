//! A test-only seam (feature `wr57-replay`, off by default and never on in `cargo build`,
//! a release or a wasm build) at the loop normal
//! ([`crate::curve::curve::p3::compute_crvs_normal`]: [`LoopNormalHook`], the WR-587
//! retirement's replay, #18931) and at the global surface point solve's target bound
//! ([`TargetBoundHook`], #20438). (The feature is named after the WR-57 substitution
//! replay, #17856, which first used the seam and has since been retired.)
//!
//! A test gives a kernel a [`ReplaySeam`] ([`Kernel::replay`]): a hook that sees each
//! outermost call's inputs and answer, and may put another answer in its place.
//!
//! The seam's state lives on the [`Kernel`] (no global or thread local state): a kernel
//! made with [`Kernel::new`] has no hook, and the wrappers then only call the function.
//! The kernel copies an operation makes for its caches share the caller's seam, as they
//! share its profiler.
//!
//! [`Kernel`]: crate::Kernel
//! [`Kernel::new`]: crate::Kernel::new
//! [`Kernel::replay`]: crate::Kernel::replay

use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::Surface;
use crate::curve::curve::p2::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::vector3d::Vec3;
use crate::Kernel;

/// The loop normal seam's hook (the WR-587 retirement's replay, #18931): given the kernel,
/// the curves and [`crate::curve::curve::p3::compute_crvs_normal`]'s own answer (the normal
/// it wrote and its status), the answer the caller gets.
pub type LoopNormalHook =
    Box<dyn FnMut(&Kernel, &[Curve], (Vec3, Result<()>)) -> (Vec3, Result<()>) + Send>;

/// One answer the global surface point solve's target bound drops (#20438), as the
/// [`TargetBoundHook`] sees it: the solve's inputs and the dropped answer's value.
pub struct TargetBound<'a> {
    /// The kernel the solve was made with.
    pub kernel: &'a Kernel,
    /// The surface solved on.
    pub surface: &'a Surface,
    /// The solve's domain.
    pub domain: &'a Extent2d,
    /// The test point.
    pub point: Vec3,
    /// The operation (a minimize or a maximize).
    pub operation: SolverOperation,
    /// The dropped answer's value (its distance).
    pub value: f64,
    /// The target distance.
    pub target: f64,
    /// The solve's distance tolerance.
    pub tol: f64,
}

/// The target bound's hook: given an answer the bound drops, whether it is dropped
/// (`false` keeps it, as before #20438).
pub type TargetBoundHook = Box<dyn FnMut(TargetBound<'_>) -> bool + Send>;

/// A kernel's seam: no hook (the default), or a hook shared with the kernel's copies.
#[derive(Clone, Default)]
pub struct ReplaySeam {
    /// The loop normal's hook ([`LoopNormalHook`]), if any.
    loop_normal: Option<Arc<Mutex<LoopNormalHook>>>,
    /// The target bound's hook ([`TargetBoundHook`]), if any.
    target_bound: Option<Arc<Mutex<TargetBoundHook>>>,
    /// The seam calls under way: a call inside another seam call (or inside the hook) is
    /// not an outermost call and goes straight to the function.
    depth: Arc<AtomicU32>,
}

impl ReplaySeam {
    /// A seam with only the loop normal's `hook`.
    #[must_use]
    pub fn loop_normal(hook: LoopNormalHook) -> Self {
        Self {
            loop_normal: Some(Arc::new(Mutex::new(hook))),
            target_bound: None,
            depth: Arc::default(),
        }
    }

    /// A seam with only the target bound's `hook`.
    #[must_use]
    pub fn target_bound(hook: TargetBoundHook) -> Self {
        Self {
            loop_normal: None,
            target_bound: Some(Arc::new(Mutex::new(hook))),
            depth: Arc::default(),
        }
    }
}

impl core::fmt::Debug for ReplaySeam {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReplaySeam")
            .field("loop_normal", &self.loop_normal.is_some())
            .field("target_bound", &self.target_bound.is_some())
            .field("depth", &self.depth)
            .finish()
    }
}

/// Runs `f`, the loop normal of `curves` written into `normal`, through `k`'s loop normal
/// hook; with no hook, or inside another seam call (the hook's own calls among them), it
/// only runs `f`. [`SolidError::Unported`] and [`SolidError::Cancelled`] pass the hook by.
///
/// # Errors
///
/// `f`'s error or the hook's; [`SolidError::InvalidInput`] when the hook panicked on an
/// earlier call.
pub(crate) fn wrap_loop_normal(
    k: &Kernel,
    curves: &[Curve],
    normal: &mut Vec3,
    f: impl FnOnce(&mut Vec3) -> Result<()>,
) -> Result<()> {
    let seam_state = &k.replay;
    let Some(hook) = seam_state.loop_normal.as_ref() else {
        return f(normal);
    };
    if seam_state.depth.load(Ordering::Relaxed) > 0 {
        return f(normal);
    }
    seam_state.depth.fetch_add(1, Ordering::Relaxed);
    let own = f(normal);
    if let Err(e) = &own {
        if e.is_unported_or_cancelled() {
            seam_state.depth.fetch_sub(1, Ordering::Relaxed);
            return own;
        }
    }
    let (written, answer) = match hook.lock() {
        Ok(mut h) => h(k, curves, (*normal, own)),
        Err(_) => (
            *normal,
            Err(SolidError::InvalidInput {
                operation: "surface::surface::replay::wrap_loop_normal",
                reason: "the hook panicked on an earlier call",
            }),
        ),
    };
    seam_state.depth.fetch_sub(1, Ordering::Relaxed);
    *normal = written;
    answer
}

/// Whether the target bound drops the answer `bound` describes: the hook's word, if the
/// kernel has one ([`TargetBoundHook`]), else `true`. Inside another seam call (the
/// hook's own solves among them) the hook is not asked, so a hook that solves again with
/// its kernel does not lock its own mutex twice: the answer is dropped, as without a
/// hook. A hook that panicked on an earlier call drops it too.
pub(crate) fn wrap_target_bound(bound: TargetBound<'_>) -> bool {
    let seam_state = &bound.kernel.replay;
    let Some(hook) = seam_state.target_bound.as_ref() else {
        return true;
    };
    if seam_state.depth.load(Ordering::Relaxed) > 0 {
        return true;
    }
    seam_state.depth.fetch_add(1, Ordering::Relaxed);
    let drop = match hook.lock() {
        Ok(mut h) => h(bound),
        Err(_) => true,
    };
    seam_state.depth.fetch_sub(1, Ordering::Relaxed);
    drop
}
