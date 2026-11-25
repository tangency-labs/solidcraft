//! A test-only seam (feature `wr57-replay`, off by default and never on in `cargo build`,
//! a release or a wasm build) at WR-450's three choices ([`Seam::AnalyticCopy`],
//! [`Seam::SplitTrims`], [`Seam::CarryGate`]) for the goldens' `wr450_rows` replay. (The
//! feature is named after the WR-57 substitution replay, #17856, which first used the seam;
//! it, the WR-587 loop normal hook and the #20438 target bound hook have been retired.)
//!
//! A test gives a kernel a [`ReplaySeam`] ([`Kernel::replay`]): a hook that sees each
//! outermost call's inputs, surface and answer, and may put another answer in its place.
//!
//! The seam's state lives on the [`Kernel`] (no global or thread local state): a kernel
//! made with [`Kernel::new`] has no hook, and [`wrap`] then only calls the function. The
//! kernel copies an operation makes for its caches share the caller's seam, as they share
//! its profiler.
//!
//! [`Kernel`]: crate::Kernel
//! [`Kernel::new`]: crate::Kernel::new
//! [`Kernel::replay`]: crate::Kernel::replay

use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::Surface;
use crate::error::{Result, SolidError};
use crate::Kernel;

/// Which choice a call went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seam {
    /// WR-450: whether `BSplineSurface::copy_and_add_analytics` keeps an exact copy (`[1]`)
    /// or the recognised analytic (`[0]`, upstream's choice); the call's surface is the
    /// analytic, its input empty.
    AnalyticCopy,
    /// WR-450: whether `MakeEdgeInFaceWithVU` drops the UV trims of the new face's edge
    /// uses (`[1]`) once its surface is the copy of the split face's, as main did before
    /// the retirement where the copy is of another kind (input `[1]`); never now (`[0]`).
    /// The call's surface is the split face's.
    SplitTrims,
    /// WR-450: whether `copy_uv_trim_curves_of_other_face` turns away the other operand's
    /// trim of an edge `MergeEdge` copied (`[1]`: its same-parameter gap is not proved within
    /// the edge's tolerance) or carries it (`[0]`, as before that gate, whatever its gap).
    /// The call's surface is the new face's, its input empty.
    CarryGate,
}

/// A call's answer: its value as bits, or its error.
pub type Answer = std::result::Result<Vec<u64>, SolidError>;

/// One outermost call, as the hook sees it.
pub struct Call<'a> {
    /// The entry point.
    pub seam: Seam,
    /// The call's inputs as bits.
    pub input: &'a [u64],
    /// The surface the call was made on.
    pub surface: &'a Surface,
    /// The kernel the call was made with.
    pub kernel: &'a Kernel,
    /// The function's own answer.
    pub own: Answer,
}

/// The hook: given a call, the answer the caller gets.
pub type Hook = Box<dyn FnMut(Call<'_>) -> Answer + Send>;

/// A kernel's seam: no hook (the default), or a hook shared with the kernel's copies.
#[derive(Clone, Default)]
pub struct ReplaySeam {
    hook: Option<Arc<Mutex<Hook>>>,
    /// The seam calls under way: a call inside another seam call (or inside the hook) is
    /// not an outermost call and goes straight to the function.
    depth: Arc<AtomicU32>,
}

impl ReplaySeam {
    /// A seam with `hook`.
    #[must_use]
    pub fn new(hook: Hook) -> Self {
        Self {
            hook: Some(Arc::new(Mutex::new(hook))),
            depth: Arc::default(),
        }
    }
}

impl core::fmt::Debug for ReplaySeam {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReplaySeam")
            .field("hook", &self.hook.is_some())
            .field("depth", &self.depth)
            .finish()
    }
}

/// A seam's value as bits.
pub trait Bits: Sized {
    /// The value's bits.
    fn to_bits(&self) -> Vec<u64>;
    /// The value of `bits`, `None` when they are not one.
    fn from_bits(bits: &[u64]) -> Option<Self>;
}

impl Bits for bool {
    fn to_bits(&self) -> Vec<u64> {
        vec![u64::from(*self)]
    }
    fn from_bits(bits: &[u64]) -> Option<Self> {
        match *bits {
            [0] => Some(false),
            [1] => Some(true),
            _ => None,
        }
    }
}

/// The choice `own` of `seam` ([`Seam::AnalyticCopy`], [`Seam::SplitTrims`],
/// [`Seam::CarryGate`]) on
/// `surface` with `input`, through `k`'s hook ([`wrap`]).
///
/// # Errors
///
/// As [`wrap`].
pub(crate) fn choose(
    k: &Kernel,
    surface: &Surface,
    seam: Seam,
    input: &[u64],
    own: bool,
) -> Result<bool> {
    wrap(k, surface, seam, input, || Ok(own))
}

/// Runs `f`, a call of `seam` on `surface` with `input`, through `k`'s hook. With no hook,
/// or inside another seam call, it only runs `f`.
///
/// # Errors
///
/// `f`'s error or the hook's; [`SolidError::InvalidInput`] when the hook's bits are not
/// a value, or the hook panicked on an earlier call.
pub(crate) fn wrap<T: Bits>(
    k: &Kernel,
    surface: &Surface,
    seam: Seam,
    input: &[u64],
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let seam_state = &k.replay;
    let Some(hook) = seam_state.hook.as_ref() else {
        return f();
    };
    if seam_state.depth.load(Ordering::Relaxed) > 0 {
        return f();
    }
    seam_state.depth.fetch_add(1, Ordering::Relaxed);
    let own = f().map(|t| t.to_bits());
    let answer = match hook.lock() {
        Ok(mut h) => h(Call {
            seam,
            input,
            surface,
            kernel: k,
            own,
        }),
        Err(_) => Err(SolidError::InvalidInput {
            operation: "surface::surface::replay::wrap",
            reason: "the hook panicked on an earlier call",
        }),
    };
    seam_state.depth.fetch_sub(1, Ordering::Relaxed);
    answer.and_then(|b| {
        T::from_bits(&b).ok_or(SolidError::InvalidInput {
            operation: "surface::surface::replay::wrap",
            reason: "the hook's answer is not a value of the seam",
        })
    })
}
