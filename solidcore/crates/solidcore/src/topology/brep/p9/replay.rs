//! A test-only seam (feature `wr469-replay`, off by default and never on in `cargo build`,
//! a release or a wasm build) at WR-469's vertex step in `MakeEdgeInFaceWithVU`
//! ([`super::make_edge_in_face_with_vu_with_curve_identity`]).
//!
//! The step grows each end vertex's tolerance to twice its gap: the farthest edge end at
//! the vertex, the new edge's or any other edge's (tangency-labs/solidcraft#17985). The goldens crate's substitution replay
//! (`wr469_replay`) gives a kernel a [`VertexStepSeam`] ([`Kernel::wr469_replay`]): a hook
//! that sees each step's vertex, its tolerance before the step and the gap, and gives the
//! tolerance the step passes to `SetTolerance` (only if larger). Answering the tolerance
//! the vertex already has leaves it as the pinned C++ does, which is main's path before the
//! step. A golden row that is then main's row bit for bit moved only through the step.
//!
//! The seam's state lives on the [`Kernel`] (no global or thread local state): a kernel
//! made with [`Kernel::new`] has no hook, and [`step`] then answers the step's own
//! tolerance. The kernel copies an operation makes share the caller's seam.
//!
//! [`Kernel`]: crate::Kernel
//! [`Kernel::new`]: crate::Kernel::new
//! [`Kernel::wr469_replay`]: crate::Kernel::wr469_replay

use std::sync::{Arc, Mutex};

use crate::topology::VertexId;
use crate::Kernel;

/// One step, as the hook sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexStep {
    /// The new edge's end vertex.
    pub vertex: VertexId,
    /// Its tolerance before the step (after `RefreshTolerance(FALSE)`).
    pub before: f64,
    /// The farthest any edge end at the vertex (the new edge's or another edge's) lies
    /// from it.
    pub gap: f64,
    /// The tolerance the step passes: twice `gap`.
    pub own: f64,
}

/// The hook: given a step, the tolerance passed to `SetTolerance` (only if larger).
pub type Hook = Box<dyn FnMut(VertexStep) -> f64 + Send>;

/// A kernel's seam: no hook (the default), or a hook shared with the kernel's copies.
#[derive(Clone, Default)]
pub struct VertexStepSeam {
    hook: Option<Arc<Mutex<Hook>>>,
}

impl VertexStepSeam {
    /// A seam with `hook`.
    #[must_use]
    pub fn new(hook: Hook) -> Self {
        Self {
            hook: Some(Arc::new(Mutex::new(hook))),
        }
    }
}

impl core::fmt::Debug for VertexStepSeam {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VertexStepSeam")
            .field("hook", &self.hook.is_some())
            .finish()
    }
}

/// The tolerance the step passes for `step`: `step.own`, or the hook's answer. (A hook
/// that panicked on an earlier call is still called: the test that installed it has
/// failed already.)
pub(crate) fn step(k: &Kernel, step: VertexStep) -> f64 {
    let Some(hook) = k.wr469_replay.hook.as_ref() else {
        return step.own;
    };
    let mut h = hook
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    h(step)
}
