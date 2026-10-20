//! FFI wrappers for unit `surface.surf_of_revolution_p2`.
//!
//! Shim: `harness/cpp/shim/surface_surf_of_revolution_p2.cpp`
//! (`sc_surface_surf_of_revolution_p2_eval`).
//! Upstream: `source/SMLib/src/SmSurfOfRevolution.cpp`.
//!
//! [`surface_op`] builds a surface of revolution from a [`Surf`] (the surface block of
//! unit `surface.surf_of_revolution_p1`) and runs one [`Op`] on it;
//! [`is_nurb_surface_surf_of_revolution`] runs the static recogniser. Records:
//!
//! * surface ops: the construction status (`0` = `SM_SUCCESS`, else the `SmStatus`),
//!   then on success the op's status and, on success, its outputs. A state is the layout
//!   of the `p1` FFI module. `Build`, `Adjust`, `Trim`: `state`; `StepInv`: `u v
//!   location`; `Offset`: the offset surface's `state`; `Gssi`: the other surface's
//!   construction status, then (on success) the status and `more ncurves curves ntypes
//!   types ndevs devs nuv1 nuv2 nnull1 nnull2` (`more` is always `0`).
//! * `is_nurb_surface_surf_of_revolution`: for a surface-of-revolution test surface its
//!   construction status; then `found` and, when found, the new surface's `state`.
//!
//! The wrappers accept only what upstream handles without undefined behaviour: the
//! surface blocks of the `p1` module, finite parameters, ordered domains (with positive
//! extents for the NURB reparameterisations `Adjust` and `Trim` run: a zero-width one
//! divides by zero in NLib; the STEP domain of `Adjust` must also have its sweep inside
//! `[-360, 360]` and at most 360 long), non-negative tolerances, independent plane axes,
//! finite non-negative tolerance scales, and for `Offset` an axis point inside
//! `[-MAX_AXIS_POINT, MAX_AXIS_POINT]` (upstream dereferences a failed axis segment).

use super::surface_surf_of_revolution_p1::{check_surface, push_surf, Build, Surf, SurfaceData};
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surf_of_revolution_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A domain argument: the surface's natural (NURB) or STEP domain, or explicit bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dom {
    /// `GetNaturalUVDomain()` (or `GetSTEPUVDomain()` for the STEP inversion).
    Natural,
    /// `umin vmin umax vmax`.
    Explicit([f64; 4]),
}

/// The other operand of [`Op::Gssi`].
#[derive(Debug, Clone, PartialEq)]
pub enum Other {
    /// Another surface of revolution (or the same one when `None`).
    Revolution(Option<Box<Surf>>),
    /// `SmPlane::CreateCanonical(A2P(origin, x, y))`.
    Plane {
        /// Origin, X and Y axes.
        frame: [[f64; 3]; 3],
    },
}

/// The upstream member run on the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The construction state only.
    Build,
    /// `AdjustSTEPUVDomain(domain)`, then the state.
    Adjust {
        /// New STEP domain `umin vmin umax vmax`.
        dom: [f64; 4],
    },
    /// `STEPInversion(domain, point, tol, .., guess)`.
    StepInv {
        /// STEP domain.
        dom: Dom,
        /// The point.
        point: [f64; 3],
        /// 3D tolerance.
        tol: f64,
        /// Optional STEP parameter guess.
        guess: Option<[f64; 2]>,
    },
    /// `CreateOffsetSurface(ctx, distance, tolerance)`, then the offset's state.
    Offset {
        /// Signed offset distance.
        dist: f64,
        /// Approximation tolerance.
        tol: f64,
    },
    /// `GlobalSurfaceIntersect` with another surface.
    Gssi {
        /// The other surface.
        other: Other,
        /// This surface's domain.
        dom: Dom,
        /// The other surface's domain.
        other_dom: Dom,
        /// Optional approximation tolerance.
        tol: Option<f64>,
    },
    /// `TrimWithDomain(domain)`, then the state.
    Trim {
        /// New NURB domain.
        dom: Dom,
    },
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Self::Build => 0,
            Self::Adjust { .. } => 1,
            Self::StepInv { .. } => 2,
            Self::Offset { .. } => 3,
            Self::Gssi { .. } => 4,
            Self::Trim { .. } => 5,
        }
    }
}

const OP_IS_NURB: i32 = 20;

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn len(v: [f64; 3]) -> f64 {
    let [x, y, z] = v;
    (x * x + y * y + z * z).sqrt()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// Checks two axes are long enough and far from parallel.
fn axes(x: [f64; 3], y: [f64; 3], what: &str) -> Result<(), FfiError> {
    finite(&x, what)?;
    finite(&y, what)?;
    let (lx, ly) = (len(x), len(y));
    if lx < 1e-6 || ly < 1e-6 || len(cross(x, y)) < 1e-3 * lx * ly {
        return Err(bad(format!("{what}: degenerate or parallel axes")));
    }
    Ok(())
}

fn tolerance(t: f64, what: &str) -> Result<(), FfiError> {
    if !(t.is_finite() && t >= 0.0) {
        return Err(bad(format!("{what}: tolerance must not be negative")));
    }
    Ok(())
}

/// A domain block `natural d[4]`; `positive` requires positive extents.
fn push_dom(o: &mut Vec<f64>, d: Dom, positive: bool, what: &str) -> Result<(), FfiError> {
    match d {
        Dom::Natural => o.extend([1.0, 0.0, 0.0, 1.0, 1.0]),
        Dom::Explicit(b) => {
            finite(&b, what)?;
            let [a, v0, c, v1] = b;
            let ok = if positive {
                a < c && v0 < v1
            } else {
                a <= c && v0 <= v1
            };
            if !ok {
                return Err(bad(format!("{what}: domain not ordered")));
            }
            o.push(0.0);
            o.extend_from_slice(&b);
        }
    }
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Build => {}
        Op::Adjust { dom } => {
            finite(dom, "adjust")?;
            let [u0, v0, u1, v1] = *dom;
            if !(u0 < u1 && v0 < v1 && u0 >= -360.0 && u1 <= 360.0 && u1 - u0 <= 360.0) {
                return Err(bad("adjust: bad STEP domain"));
            }
            o.extend_from_slice(dom);
        }
        Op::StepInv {
            dom,
            point,
            tol,
            guess,
        } => {
            push_dom(o, *dom, false, "step inversion domain")?;
            finite(point, "step inversion")?;
            tolerance(*tol, "step inversion")?;
            let g = guess.unwrap_or([0.0, 0.0]);
            finite(&g, "step inversion guess")?;
            o.extend_from_slice(point);
            o.extend([*tol, f64::from(u8::from(guess.is_some()))]);
            o.extend_from_slice(&g);
        }
        Op::Offset { dist, tol } => {
            finite(&[*dist], "offset")?;
            if !(tol.is_finite() && *tol > 1e-9) {
                return Err(bad("offset: tolerance"));
            }
            o.extend([*dist, *tol]);
        }
        Op::Gssi {
            other,
            dom,
            other_dom,
            tol,
        } => {
            match other {
                Other::Revolution(None) => o.extend([0.0, 1.0]),
                Other::Revolution(Some(s)) => {
                    o.extend([0.0, 0.0]);
                    push_surf(o, s)?;
                }
                Other::Plane { frame } => {
                    let [origin, x, y] = *frame;
                    finite(&origin, "plane")?;
                    axes(x, y, "plane")?;
                    o.push(1.0);
                    o.extend_from_slice(&origin);
                    o.extend_from_slice(&x);
                    o.extend_from_slice(&y);
                }
            }
            push_dom(o, *dom, false, "surface domain")?;
            push_dom(o, *other_dom, false, "other domain")?;
            let t = tol.unwrap_or(0.0);
            tolerance(t, "intersection")?;
            o.extend([f64::from(u8::from(tol.is_some())), t]);
        }
        Op::Trim { dom } => push_dom(o, *dom, true, "trim domain")?,
    }
    Ok(())
}

fn call(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: well-formed surface blocks, bounded
        // counts, finite values, ordered domains, non-degenerate axes, non-negative
        // tolerances); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_surface_surf_of_revolution_p2_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Builds the surface `s` and runs `op` on it; returns the shim's record (layout in
/// the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn surface_op(s: &Surf, op: &Op) -> Result<Vec<f64>, FfiError> {
    if matches!(op, Op::Offset { .. }) {
        far_axis(s)?;
    }
    let mut input = Vec::new();
    push_surf(&mut input, s)?;
    push_op(&mut input, op)?;
    call(op.code(), &input)
}

/// Largest axis point coordinate `CreateOffsetSurface` is given.
pub const MAX_AXIS_POINT: f64 = 1e12;

/// `CreateOffsetSurface` trims the offset generator with the axis segment from the
/// axis point to `point + 1.5 * box size * Z` (`SmLine::CreateLineSegment`, status
/// ignored) and dereferences it: an axis point far beyond the generator's size makes
/// the segment degenerate and the line NULL. Bounds the axis point by
/// [`MAX_AXIS_POINT`].
fn far_axis(s: &Surf) -> Result<(), FfiError> {
    let p = match &s.build {
        Build::Canonical { point, .. } => point,
        Build::Ctor { origin, .. } => origin,
    };
    if p.iter().any(|v| v.abs() > MAX_AXIS_POINT) {
        return Err(bad("offset: axis point above MAX_AXIS_POINT"));
    }
    Ok(())
}

/// The test surface of [`is_nurb_surface_surf_of_revolution`].
#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    /// A B-spline surface.
    BSpline(SurfaceData),
    /// A surface of revolution (copied).
    Revolution(Box<Surf>),
}

/// `SmSurfOfRevolution::IsNurbSurfaceSurfOfRevolution(ctx, surface, .., scale)`; returns
/// the shim's record (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn is_nurb_surface_surf_of_revolution(test: &Test, scale: f64) -> Result<Vec<f64>, FfiError> {
    if !(scale.is_finite() && scale >= 0.0) {
        return Err(bad("is nurb: tolerance scale"));
    }
    let mut input = Vec::new();
    match test {
        Test::BSpline(surface) => {
            check_surface(surface)?;
            let n = surface.net.len() - 1;
            let m = surface.net.first().map_or(0, Vec::len) - 1;
            input.extend([0.0, scale]);
            input.extend([n as f64, m as f64, surface.p as f64, surface.q as f64]);
            for row in &surface.net {
                for p in row {
                    input.extend_from_slice(p);
                }
            }
            input.extend_from_slice(&surface.u);
            input.extend_from_slice(&surface.v);
        }
        Test::Revolution(s) => {
            input.extend([1.0, scale]);
            push_surf(&mut input, s)?;
        }
    }
    call(OP_IS_NURB, &input)
}
