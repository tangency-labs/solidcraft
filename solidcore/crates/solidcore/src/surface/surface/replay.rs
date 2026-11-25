//! A test-only seam (feature `wr57-replay`, off by default and never on in `cargo build`,
//! a release or a wasm build) at the two WR-57 entry points:
//! [`Surface::find_degen_param_for_direction`] and the singular point inversion
//! ([`Surface::invert_point_near_singularity`]), at WR-450's three choices
//! ([`Seam::AnalyticCopy`], [`Seam::SplitTrims`], [`Seam::CarryGate`]) for the goldens'
//! `wr450_rows` replay, at the loop normal ([`crate::curve::curve::p3::compute_crvs_normal`]:
//! [`LoopNormalHook`], the WR-587 retirement's replay, #18931), and at the global surface
//! point solve's target bound ([`TargetBoundHook`], #20438).
//!
//! The goldens crate's substitution replay (`wr57_replay`, #17856) gives a kernel a
//! [`ReplaySeam`] ([`Kernel::replay`]): a hook that sees each outermost call's inputs,
//! surface and answer, and may put another answer in its place: main's, from a committed
//! tape. A golden row that then equals main's row bit for bit moved only through those
//! answers.
//!
//! The seam's state lives on the [`Kernel`] (no global or thread local state): a kernel
//! made with [`Kernel::new`] has no hook, and [`wrap`] then only calls the function. The
//! kernel copies an operation makes for its caches share the caller's seam, as they share
//! its profiler.
//!
//! [`Surface::find_degen_param_for_direction`]: super::Surface::find_degen_param_for_direction
//! [`Surface::invert_point_near_singularity`]: super::Surface::invert_point_near_singularity
//! [`Kernel`]: crate::Kernel
//! [`Kernel::new`]: crate::Kernel::new
//! [`Kernel::replay`]: crate::Kernel::replay

use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::p7::DegenParam;
use super::p8::SingularityInversion;
use super::Surface;
use crate::curve::curve::p2::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::local_solve_nd::TerminationReason;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::Kernel;

/// Which entry point a call went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seam {
    /// [`super::Surface::find_degen_param_for_direction`].
    Degen,
    /// The singular point inversion (closest point or maximize).
    Invert,
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
    /// The call's inputs as bits ([`degen_input`], [`invert_input`]).
    pub input: &'a [u64],
    /// The surface the call was made on.
    pub surface: &'a Surface,
    /// The kernel the call was made with.
    pub kernel: &'a Kernel,
    /// For [`Seam::Invert`]: whether the inversion maximizes the distance (`false` for a
    /// closest point, and for [`Seam::Degen`]).
    pub farthest: bool,
    /// The function's own answer.
    pub own: Answer,
}

/// The hook: given a call, the answer the caller gets.
pub type Hook = Box<dyn FnMut(Call<'_>) -> Answer + Send>;

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
    hook: Option<Arc<Mutex<Hook>>>,
    /// The loop normal's hook ([`LoopNormalHook`]), if any.
    loop_normal: Option<Arc<Mutex<LoopNormalHook>>>,
    /// The target bound's hook ([`TargetBoundHook`]), if any.
    target_bound: Option<Arc<Mutex<TargetBoundHook>>>,
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
            loop_normal: None,
            target_bound: None,
            depth: Arc::default(),
        }
    }

    /// A seam with only the loop normal's `hook`.
    #[must_use]
    pub fn loop_normal(hook: LoopNormalHook) -> Self {
        Self {
            hook: None,
            loop_normal: Some(Arc::new(Mutex::new(hook))),
            target_bound: None,
            depth: Arc::default(),
        }
    }

    /// A seam with only the target bound's `hook`.
    #[must_use]
    pub fn target_bound(hook: TargetBoundHook) -> Self {
        Self {
            hook: None,
            loop_normal: None,
            target_bound: Some(Arc::new(Mutex::new(hook))),
            depth: Arc::default(),
        }
    }
}

impl core::fmt::Debug for ReplaySeam {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReplaySeam")
            .field("hook", &self.hook.is_some())
            .field("loop_normal", &self.loop_normal.is_some())
            .field("target_bound", &self.target_bound.is_some())
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

impl Bits for DegenParam {
    fn to_bits(&self) -> Vec<u64> {
        vec![
            self.uv.x.to_bits(),
            self.uv.y.to_bits(),
            u64::from(self.converged),
        ]
    }
    fn from_bits(bits: &[u64]) -> Option<Self> {
        match *bits {
            [x, y, c] if c <= 1 => Some(Self {
                uv: Point2::new(f64::from_bits(x), f64::from_bits(y)),
                converged: c == 1,
            }),
            _ => None,
        }
    }
}

impl Bits for SingularityInversion {
    fn to_bits(&self) -> Vec<u64> {
        vec![
            self.uv.x.to_bits(),
            self.uv.y.to_bits(),
            u64::from(self.reason.code().unsigned_abs()),
        ]
    }
    fn from_bits(bits: &[u64]) -> Option<Self> {
        let [x, y, r] = *bits else {
            return None;
        };
        let reason = [
            TerminationReason::Undefined,
            TerminationReason::FoundAnswerConverged,
            TerminationReason::FoundAnswerClose,
            TerminationReason::OutOfBounds,
            TerminationReason::BadJacobian,
            TerminationReason::UnableToConverge,
            TerminationReason::StillIterating,
        ]
        .into_iter()
        .find(|t| u64::from(t.code().unsigned_abs()) == r)?;
        Some(Self {
            uv: Point2::new(f64::from_bits(x), f64::from_bits(y)),
            reason,
        })
    }
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
    wrap(k, surface, seam, input, false, || Ok(own))
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
    farthest: bool,
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
            farthest,
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

/// The error a surface routine `operation` answers when it fails as `reason` says, built
/// as the routine builds it (a taped answer of main's).
#[must_use]
pub fn surface_failed(operation: &'static str, reason: &'static str) -> SolidError {
    SolidError::surface(
        operation,
        crate::error::surface::SurfaceFailure::Failed(reason),
    )
}

/// [`Seam::Degen`]'s inputs as bits.
pub(crate) fn degen_input(
    uv: Point2,
    dir: Vec3,
    dir_from_pole: bool,
    singular_dir: crate::nurbs::b_spline_surface::p1::SurfParamType,
    tol: f64,
) -> Vec<u64> {
    vec![
        uv.x.to_bits(),
        uv.y.to_bits(),
        dir.x.to_bits(),
        dir.y.to_bits(),
        dir.z.to_bits(),
        u64::from(dir_from_pole),
        u64::from(singular_dir == crate::nurbs::b_spline_surface::p1::SurfParamType::U),
        tol.to_bits(),
    ]
}

/// [`Seam::Invert`]'s inputs as bits.
pub(crate) fn invert_input(
    test_pt: Vec3,
    uv_domain: &Extent2d,
    uv_guess: Point2,
    tol: f64,
) -> Vec<u64> {
    let (lo, hi) = (uv_domain.min, uv_domain.max);
    vec![
        test_pt.x.to_bits(),
        test_pt.y.to_bits(),
        test_pt.z.to_bits(),
        lo.x.to_bits(),
        lo.y.to_bits(),
        hi.x.to_bits(),
        hi.y.to_bits(),
        uv_guess.x.to_bits(),
        uv_guess.y.to_bits(),
        tol.to_bits(),
    ]
}
