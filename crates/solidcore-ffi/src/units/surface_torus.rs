//! FFI wrappers for unit `surface.torus`.
//!
//! Shim: `harness/cpp/shim/surface_torus.cpp` (`sc_surface_torus_eval`).
//! Upstream: `source/SMLib/src/SmTorus.cpp`.
//!
//! [`torus_op`] builds a torus with `SmTorus::CreateCanonical` from a [`TorusIn`] and
//! runs one [`Op`] on it. The shim's flat record is the construction status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then on success the op's record:
//!
//! * `Build`: the state (no status). A state is `is_torus is_rev is_bspline is_surface
//!   type swap planar inside_out major minor O[3] X[3] Y[3] D[4] MAXD[4] have_polar
//!   angle_count [first last] polar_curve gen_curve has_nurb [p q nu nv cpts(4 nu nv,
//!   v fastest) nku KU nkv KV]`; a curve is `has [type step0 step1 has_nurb [p n
//!   cpts(4 n) nk knots] is_ellipse [O[3] X[3] Y[3] rx ry inside_out]]`.
//! * the other ops: their status, then on success `EvalStep`: `(k+1)^2` vectors (or the
//!   point); `ToStep`, `ToNurbs`: `u v`; `Iso`: `gap curve` (the shim answers
//!   `SC_ERR_ARG` without status when `SmBSplineSurface::CreateIsoParametricCurve`
//!   fails, since the torus override then reads an uninitialised curve); `Offset`:
//!   `has [rebuild_status] state`; `Rebuild`: the state (also after a failure);
//!   `Transform`: the state (also after a failure); `Eq`: the other torus's
//!   construction status, then on success `this == other`, `other == this`; `Copy`:
//!   `state copy == this`; `TrimDomain`: the domain after the call (`TrimWithDomain`
//!   writes it back), then the state (also after a failure).
//!
//! The wrappers accept only what upstream handles without undefined behaviour: finite
//! values, unit and independent placement axes (the base constructor passes the Y axis
//! to `IsOnPlane`, whose output a longer one leaves uninitialised), angular domains
//! inside `[-360, 360]` at most 360 long, generator circles with independent axes and a
//! positive radius, derivative orders up to 3.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_torus_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `SmCircle(c, x, y, [d0, d1], r, 3)`, the optional generator (`pOptCircleNurb`).
#[derive(Debug, Clone, PartialEq)]
pub struct CircleIn {
    /// Centre.
    pub c: [f64; 3],
    /// X axis.
    pub x: [f64; 3],
    /// Y axis.
    pub y: [f64; 3],
    /// Angular domain in degrees.
    pub d: [f64; 2],
    /// Radius.
    pub r: f64,
}

/// `SmTorus::CreateCanonical(SmAxis2Placement(origin, x, y), major, minor, .., swap,
/// inside_out, circle, dom)`.
#[derive(Debug, Clone, PartialEq)]
pub struct TorusIn {
    /// Torus centre.
    pub origin: [f64; 3],
    /// Major circle start direction.
    pub x: [f64; 3],
    /// Major circle 90 degree direction.
    pub y: [f64; 3],
    /// Major radius.
    pub major: f64,
    /// Minor radius.
    pub minor: f64,
    /// `bSwapUV`.
    pub swap: bool,
    /// `bInsideOut`.
    pub inside_out: bool,
    /// Optional STEP domain `umin vmin umax vmax` in degrees.
    pub dom: Option<[f64; 4]>,
    /// Optional generator.
    pub circle: Option<CircleIn>,
}

/// The upstream member run on the torus.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The construction state only.
    Build,
    /// `EvaluateSTEP(uv, k, k, ..)`, or `EvaluateSTEPPoint` when `point`.
    EvalStep {
        /// Major angle in degrees.
        u: f64,
        /// Minor angle in degrees.
        v: f64,
        /// Derivative order (both directions), at most 3.
        k: usize,
        /// Use `EvaluateSTEPPoint`.
        point: bool,
    },
    /// `ConvertUVFromNURBSToSTEP`.
    ToStep {
        /// NURB u.
        u: f64,
        /// NURB v.
        v: f64,
    },
    /// `ConvertUVFromSTEPToNURBS`.
    ToNurbs {
        /// STEP u.
        u: f64,
        /// STEP v.
        v: f64,
    },
    /// `CreateIsoParametricCurve`.
    Iso {
        /// `0` = `SM_SP_U`, `1` = `SM_SP_V`.
        param: u8,
        /// Iso parameter.
        iso: f64,
        /// Optional trim domain.
        dom: Option<[f64; 4]>,
    },
    /// `CreateOffsetSurface`, then optionally `RebuildSTEPFromNURBParameters` on the
    /// offset torus.
    Offset {
        /// Signed offset distance.
        dist: f64,
        /// Approximation tolerance.
        tol: f64,
        /// Rebuild the offset torus.
        rebuild: bool,
    },
    /// `RebuildSTEPFromNURBParameters`.
    Rebuild,
    /// `Transform(SmAxis2Placement(origin, x, y), scale)`.
    Transform {
        /// Placement origin.
        origin: [f64; 3],
        /// Placement X axis.
        x: [f64; 3],
        /// Placement Y axis.
        y: [f64; 3],
        /// Optional scale.
        scale: Option<[f64; 3]>,
    },
    /// `operator==` with another torus.
    Eq {
        /// The other torus.
        other: Box<TorusIn>,
    },
    /// `Copy`.
    Copy,
    /// `TrimWithDomain(SmExtent2d(u0, v0, u1, v1))` (inherited from
    /// `SmSurfOfRevolution`).
    TrimDomain {
        /// The new NURB domain `[u0, v0, u1, v1]`.
        dom: [f64; 4],
    },
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Op::Build => 0,
            Op::EvalStep { .. } => 1,
            Op::ToStep { .. } => 2,
            Op::ToNurbs { .. } => 3,
            Op::Iso { .. } => 4,
            Op::Offset { .. } => 5,
            Op::Rebuild => 6,
            Op::Transform { .. } => 7,
            Op::Eq { .. } => 8,
            Op::Copy => 9,
            Op::TrimDomain { .. } => 10,
        }
    }
}

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

/// Checks two axes are long enough, far from parallel, and of a length whose square
/// does not overflow: unitizing an axis of infinite length gives a zero axis, so a
/// generator circle on it gets no NURB, which `SmSurfOfRevolution`'s constructor then
/// evaluates (`EvaluatePoint` dereferences the NULL NURB).
fn axes(x: [f64; 3], y: [f64; 3], what: &str) -> Result<(), FfiError> {
    finite(&x, what)?;
    finite(&y, what)?;
    let (lx, ly) = (len(x), len(y));
    if !lx.is_finite() || !ly.is_finite() {
        return Err(bad(format!("{what}: axis length overflows")));
    }
    if lx < 1e-6 || ly < 1e-6 || len(cross(x, y)) < 1e-3 * lx * ly {
        return Err(bad(format!("{what}: degenerate or parallel axes")));
    }
    Ok(())
}

/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1e-12;

/// Checks a vector is a unit vector within `SM_EFF_ZERO` on its squared length.
fn unit(v: [f64; 3], what: &str) -> Result<(), FfiError> {
    let [x, y, z] = v;
    if ((x * x + y * y + z * z) - 1.0).abs() > EFF_ZERO {
        return Err(bad(format!("{what}: not a unit vector")));
    }
    Ok(())
}

fn domain(d: [f64; 4], what: &str) -> Result<(), FfiError> {
    finite(&d, what)?;
    let [a, b, c, e] = d;
    if !(a <= c && b <= e) {
        return Err(bad(format!("{what}: domain not ordered")));
    }
    Ok(())
}

fn angular(d0: f64, d1: f64, what: &str) -> Result<(), FfiError> {
    finite(&[d0, d1], what)?;
    if !(d0 < d1 && d0 >= -360.0 && d1 <= 360.0 && d1 - d0 <= 360.0) {
        return Err(bad(format!("{what}: bad angular domain")));
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn push_torus(o: &mut Vec<f64>, t: &TorusIn) -> Result<(), FfiError> {
    finite(&t.origin, "origin")?;
    axes(t.x, t.y, "placement")?;
    unit(t.x, "placement X")?;
    unit(t.y, "placement Y")?;
    finite(&[t.major, t.minor], "radii")?;
    o.extend_from_slice(&t.origin);
    o.extend_from_slice(&t.x);
    o.extend_from_slice(&t.y);
    o.extend([t.major, t.minor, flag(t.swap), flag(t.inside_out)]);
    o.push(flag(t.dom.is_some()));
    let d = t.dom.unwrap_or([0.0, 0.0, 360.0, 360.0]);
    domain(d, "torus domain")?;
    let [u0, v0, u1, v1] = d;
    angular(u0, u1, "torus domain u")?;
    angular(v0, v1, "torus domain v")?;
    o.extend_from_slice(&d);
    o.push(flag(t.circle.is_some()));
    match &t.circle {
        Some(CircleIn { c, x, y, d, r }) => {
            finite(c, "circle")?;
            axes(*x, *y, "circle")?;
            let [d0, d1] = *d;
            angular(d0, d1, "circle")?;
            if !(r.is_finite() && *r > 1e-6) {
                return Err(bad("circle: radius"));
            }
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *r]);
        }
        None => o.extend([0.0; 12]),
    }
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Build | Op::Rebuild | Op::Copy => {}
        Op::EvalStep { u, v, k, point } => {
            finite(&[*u, *v], "eval")?;
            if *k > 3 {
                return Err(bad("eval: order above 3"));
            }
            o.extend([*u, *v, *k as f64, flag(*point)]);
        }
        Op::ToStep { u, v } | Op::ToNurbs { u, v } => {
            finite(&[*u, *v], "convert")?;
            o.extend([*u, *v]);
        }
        Op::Iso { param, iso, dom } => {
            if *param > 1 {
                return Err(bad("iso: param"));
            }
            finite(&[*iso], "iso")?;
            o.extend([f64::from(*param), *iso, flag(dom.is_some())]);
            let d = dom.unwrap_or([0.0, 0.0, 0.0, 0.0]);
            domain(d, "iso domain")?;
            o.extend_from_slice(&d);
        }
        Op::Offset { dist, tol, rebuild } => {
            finite(&[*dist, *tol], "offset")?;
            o.extend([*dist, *tol, flag(*rebuild)]);
        }
        Op::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            finite(origin, "transform")?;
            axes(*x, *y, "transform")?;
            let s = scale.unwrap_or([1.0, 1.0, 1.0]);
            finite(&s, "transform scale")?;
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.push(flag(scale.is_some()));
            o.extend_from_slice(&s);
        }
        Op::Eq { other } => push_torus(o, other)?,
        Op::TrimDomain { dom } => {
            domain(*dom, "trim domain")?;
            o.extend_from_slice(dom);
        }
    }
    Ok(())
}

/// Builds the torus `t` and runs `op` on it; returns the shim's record (layout in the
/// module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn torus_op(t: &TorusIn, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_torus(&mut input, t)?;
    push_op(&mut input, op)?;
    let code = op.code();
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `code` (built and validated above: finite values, unit independent
        // placement axes, non-degenerate circles, ordered angular domains, orders up to
        // 3); buf is valid for buf.len() writes and the shim honours the capacity; len
        // is valid.
        unsafe {
            sys::sc_surface_torus_eval(
                code,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
