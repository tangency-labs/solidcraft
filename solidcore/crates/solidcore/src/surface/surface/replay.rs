//! A test-only seam (feature `wr57-replay`, off by default and never on in a release or
//! wasm build) at the two WR-57 entry points: [`Surface::find_degen_param_for_direction`]
//! and the singular point inversion ([`Surface::invert_point_near_singularity`]).
//!
//! The goldens crate's substitution replay (`wr57_replay`, #17856) installs a hook that
//! sees each outermost call's inputs and answer and may put another answer in its place:
//! main's, from a committed tape. A golden row that then equals main's row bit for bit
//! moved only through those answers.
//!
//! Nothing here changes a result while no hook is installed: [`wrap`] then only calls
//! the function. The hook lives in a thread local because the seam must reach every
//! kernel a runner makes; the library is single threaded, and the feature exists only for
//! the goldens tests (a dev-dependency feature).
//!
//! [`Surface::find_degen_param_for_direction`]: super::Surface::find_degen_param_for_direction
//! [`Surface::invert_point_near_singularity`]: super::Surface::invert_point_near_singularity

use std::cell::{Cell, RefCell};

use super::p7::DegenParam;
use super::p8::SingularityInversion;
use crate::error::{Result, SolidError};
use crate::geom::local_solve_nd::TerminationReason;
use crate::geom::vector2d::Point2;

/// Which entry point a call went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seam {
    /// [`super::Surface::find_degen_param_for_direction`].
    Degen,
    /// The singular point inversion (closest point or maximize).
    Invert,
}

/// A call's answer: its value as bits, or its error.
pub type Answer = std::result::Result<Vec<u64>, SolidError>;

/// The hook: the seam, the call's inputs as bits and the function's own answer; it
/// returns the answer the caller gets.
pub type Hook = Box<dyn FnMut(Seam, &[u64], Answer) -> Answer>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Installs `hook` (or removes the hook, with `None`) on this thread; returns the one
/// it replaces.
pub fn install(hook: Option<Hook>) -> Option<Hook> {
    HOOK.with(|c| c.replace(hook))
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

/// Runs `f`, an outermost call of `seam` with `input`, through the installed hook. With
/// no hook, or inside another seam's call, it only runs `f`.
///
/// # Errors
///
/// `f`'s error or the hook's; [`SolidError::InvalidInput`] when the hook's bits are not
/// a value.
pub(crate) fn wrap<T: Bits>(seam: Seam, input: &[u64], f: impl FnOnce() -> Result<T>) -> Result<T> {
    let active = DEPTH.with(Cell::get) == 0 && HOOK.with(|c| c.borrow().is_some());
    if !active {
        return f();
    }
    DEPTH.with(|d| d.set(d.get() + 1));
    let own = f();
    DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    let mut hook = HOOK.with(|c| c.borrow_mut().take());
    let answer = own.map(|t| t.to_bits());
    let answer = match hook.as_mut() {
        Some(h) => h(seam, input, answer),
        None => answer,
    };
    HOOK.with(|c| *c.borrow_mut() = hook);
    answer.and_then(|b| {
        T::from_bits(&b).ok_or(SolidError::InvalidInput {
            operation: "surface::surface::replay::wrap",
            reason: "the hook's answer is not a value of the seam",
        })
    })
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
    dir: crate::geom::vector3d::Vec3,
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
    test_pt: crate::geom::vector3d::Vec3,
    uv_domain: &crate::geom::extent2d::Extent2d,
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
