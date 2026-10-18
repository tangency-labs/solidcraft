//! FFI wrappers for unit `surface.cone_p1`.
//!
//! Shim: `harness/cpp/shim/surface_cone_p1.cpp` (`sc_surface_cone_p1_eval`).
//! Upstream: `source/SMLib/src/SmCone.cpp`, `source/SMLib/src/SmCylinder.cpp`.
//!
//! [`cone_op`] builds a cone or cylinder from a [`Build`] and runs one [`Op`] on it;
//! [`coincidence`] runs (a verbatim copy of) the file-static
//! `sm_TotalCoincidenceChecker` on two curves. Both return the shim's flat record, whose
//! statuses are raw `SmStatus` values (`1000` = `SM_SUCCESS`):
//!
//! * cone ops: the construction status, then on success the op's record: its status
//!   and, on success, its outputs. `Build`: the state (no status). A state is `type
//!   n type_string[n] n class_string[n] is_cyl is_cone is_rev is_bspline is_surface is_cylinder base semi inside swap
//!   planar make_nurb_gen bounded O[3] X[3] Y[3] D[4] bot_r top_r bot_h top_h r(1.25)
//!   lin[2] lin_param MAXD[4] apex_status apex_cyl apex[3] apex_param canon_O[3]
//!   canon_r canon_semi canon_swap canon_inside height_ok [height] have_polar
//!   angle_count [first last] polar_curve gen_curve has_nurb [p q nu nv cpts(4 nu nv,
//!   v fastest) nku KU nkv KV]`, a curve `has [type step0 step1 has_nurb [p n cpts(4
//!   n) nk knots]]`. `EvalStep` / `Eval`: `(hu+1)(hv+1)` vectors (or the point);
//!   `EvalPoint`: the point; `ToStep` / `ToNurbs`: `u v`; `Iso`: `gap curve uv_curve`;
//!   `IsoCircle`: `curve`; `Inversion`: `u v location` (also after `SM_ERR` when the
//!   outputs were written); `Positive`: `negated point[3] apex_param`; `Adjust` /
//!   `Transform`: the new state; `Eq`: the other cone's construction status then (on
//!   success) `this == other`, `other == this`; `Copy`: `copy == this` and the copy's
//!   state.
//! * `coincidence`: `status are_coinc reversed` (`SmBoolean`s: `2` is `UNSURE`).
//!
//! `ToNurbs`, `IsoCircle` and the state's height need the polar converter's curve
//! (upstream dereferences it unchecked); without it the shim answers `SC_ERR_ARG`
//! (-1002) for those ops and leaves the height out. `IsoCircle` also answers
//! `SC_ERR_ARG` when the STEP inversion `CreateIsoCircleFromPoint` makes leaves the
//! height unset, or when the NURB conversion of its result fails and leaves the NURB
//! parameter it then uses unset (always without `swap_uv`; with it, when the polar
//! converter wrote nothing): upstream then reads it uninitialised.
//!
//! The wrappers accept only what upstream handles without undefined behaviour: finite
//! values (any placement, radii, angles, heights: upstream returns statuses or
//! degenerate surfaces for those), derivative orders up to [`MAX_ORDER`], an
//! `AdjustSTEPUVDomain` sweep interval that is not reversed (upstream loops forever on
//! one), independent axes for `Transform` (whose upstream body is unported, so it is
//! never compared), lines of two distinct points, circles and ellipses with
//! independent axes, positive radii and an angular domain inside `[-360, 360]` at most
//! 360 long.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest derivative order passed (upstream rejects orders above 3 / 4 itself).
pub const MAX_ORDER: usize = 8;

mod sys {
    extern "C" {
        pub fn sc_surface_cone_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// How the cone is made.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// `SmCone::CreateCanonical(O, bot, semi, pOptHeight, swap, inside)`.
    CanonicalSemi {
        /// Bottom radius.
        bot: f64,
        /// Semi-angle in degrees.
        semi: f64,
        /// Optional height.
        height: Option<f64>,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
    },
    /// `SmCone::CreateCanonical(O, bot, top, height, swap, inside)`.
    Canonical {
        /// Bottom radius.
        bot: f64,
        /// Top radius (the semi-angle for `height <= 0`).
        top: f64,
        /// Height.
        height: f64,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
    },
    /// `SmCylinder::CreateCanonical(O, radius)`.
    CylCanonical {
        /// Radius.
        radius: f64,
    },
    /// The protected unbounded cone constructor, then `MakeNurb`.
    ConeUnbounded {
        /// Radius at the origin.
        base: f64,
        /// Semi-angle in degrees.
        semi: f64,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
    },
    /// The bounded cone constructor, then `MakeNurb`.
    ConeBounded {
        /// Bottom radius.
        bot: f64,
        /// Top radius.
        top: f64,
        /// Start angle in degrees.
        start: f64,
        /// End angle in degrees.
        end: f64,
        /// Height.
        height: f64,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
        /// `bMakeNurbGenCurve`.
        make_nurb_gen: bool,
    },
    /// The bounded cylinder constructor, then `MakeNurb`.
    CylBounded {
        /// Radius.
        radius: f64,
        /// Start angle in degrees.
        start: f64,
        /// End angle in degrees.
        end: f64,
        /// Height.
        height: f64,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
        /// `bMakeNurbGenCurve`.
        make_nurb_gen: bool,
    },
    /// The protected unbounded cylinder constructor, then `MakeNurb`.
    CylUnbounded {
        /// Radius.
        radius: f64,
        /// `bSwapUV`.
        swap: bool,
        /// `bInsideOut`.
        inside: bool,
    },
}

/// A cone to build.
#[derive(Debug, Clone, PartialEq)]
pub struct Build {
    /// Construction.
    pub mode: Mode,
    /// Placement origin.
    pub origin: [f64; 3],
    /// Placement X axis.
    pub x: [f64; 3],
    /// Placement Y axis.
    pub y: [f64; 3],
    /// Out-of-bounds evaluation (set after construction).
    pub oob: bool,
}

/// The upstream member run on the cone.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The construction state only.
    Build,
    /// `EvaluateSTEP(uv, hu, hv, ..)`, or `EvaluateSTEPPoint` when `point`.
    EvalStep {
        /// Angle in degrees.
        u: f64,
        /// Height.
        v: f64,
        /// u order.
        hu: usize,
        /// v order.
        hv: usize,
        /// Use `EvaluateSTEPPoint`.
        point: bool,
    },
    /// `Evaluate(uv, hu, hv, u_left, v_left, upper, .., non_zero)`.
    Eval {
        /// NURB u.
        u: f64,
        /// NURB v.
        v: f64,
        /// u order.
        hu: usize,
        /// v order.
        hv: usize,
        /// `bUFromLeft`.
        u_left: bool,
        /// `bVFromLeft`.
        v_left: bool,
        /// `bOnlyUpperHalf`.
        upper: bool,
        /// `bNonZeroTangents`.
        non_zero: bool,
    },
    /// `EvaluatePoint`.
    EvalPoint {
        /// NURB u.
        u: f64,
        /// NURB v.
        v: f64,
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
        /// Angle in degrees.
        u: f64,
        /// Height.
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
        /// Ask for the UV line.
        uv: bool,
    },
    /// `CreateIsoCircleFromPoint`.
    IsoCircle {
        /// Reference point.
        p: [f64; 3],
        /// Tolerance.
        tol: f64,
    },
    /// `STEPInversion(domain, p, tol, .., guess)`.
    Inversion {
        /// The point.
        p: [f64; 3],
        /// Tolerance.
        tol: f64,
        /// Optional guess.
        guess: Option<[f64; 2]>,
    },
    /// `GetPositivePoint`.
    Positive {
        /// The point.
        p: [f64; 3],
    },
    /// `AdjustSTEPUVDomain`.
    Adjust {
        /// New STEP domain `umin vmin umax vmax`.
        dom: [f64; 4],
    },
    /// `Transform(placement, scale)`.
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
    /// `operator==` with another cone.
    Eq {
        /// The other cone.
        other: Box<Build>,
    },
    /// `Copy`.
    Copy,
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Op::Build => 0,
            Op::EvalStep { .. } => 1,
            Op::Eval { .. } => 2,
            Op::EvalPoint { .. } => 3,
            Op::ToStep { .. } => 4,
            Op::ToNurbs { .. } => 5,
            Op::Iso { .. } => 6,
            Op::IsoCircle { .. } => 7,
            Op::Inversion { .. } => 8,
            Op::Positive { .. } => 9,
            Op::Adjust { .. } => 10,
            Op::Transform { .. } => 11,
            Op::Eq { .. } => 12,
            Op::Copy => 13,
        }
    }
}

/// A curve of the coincidence test.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine(a, b, 3)` with its NURB made.
    Line {
        /// Start point.
        a: [f64; 3],
        /// End point.
        b: [f64; 3],
    },
    /// `SmCircle(c, x, y, [d0, d1], r, 3)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Angular domain in degrees.
        d: [f64; 2],
        /// Radius.
        r: f64,
    },
    /// `SmEllipse(c, x, y, [d0, d1], rx, ry, 3)`.
    Ellipse {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Angular domain in degrees.
        d: [f64; 2],
        /// X radius.
        rx: f64,
        /// Y radius.
        ry: f64,
    },
}

const OP_COINCIDENCE: i32 = 20;

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

fn angular(d0: f64, d1: f64, what: &str) -> Result<(), FfiError> {
    finite(&[d0, d1], what)?;
    if !(d0 < d1 && d0 >= -360.0 && d1 <= 360.0 && d1 - d0 <= 360.0) {
        return Err(bad(format!("{what}: bad angular domain")));
    }
    Ok(())
}

fn b(v: bool) -> f64 {
    f64::from(u8::from(v))
}

fn push_build(o: &mut Vec<f64>, s: &Build) -> Result<(), FfiError> {
    finite(&s.origin, "origin")?;
    finite(&s.x, "placement")?;
    finite(&s.y, "placement")?;
    let (code, p): (f64, [f64; 8]) = match s.mode {
        Mode::CanonicalSemi {
            bot,
            semi,
            height: h,
            swap,
            inside,
        } => {
            finite(&[bot, semi, h.unwrap_or(0.0)], "canonical")?;
            (
                0.0,
                [bot, semi, b(h.is_some()), h.unwrap_or(0.0), b(swap), b(inside), 0.0, 0.0],
            )
        }
        Mode::Canonical {
            bot,
            top,
            height: h,
            swap,
            inside,
        } => {
            finite(&[bot, top, h], "canonical")?;
            (1.0, [bot, top, h, b(swap), b(inside), 0.0, 0.0, 0.0])
        }
        Mode::CylCanonical { radius } => {
            finite(&[radius], "cylinder")?;
            (2.0, [radius, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        }
        Mode::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        } => {
            finite(&[base, semi], "cone")?;
            (3.0, [base, semi, b(swap), b(inside), 0.0, 0.0, 0.0, 0.0])
        }
        Mode::ConeBounded {
            bot,
            top,
            start,
            end,
            height: h,
            swap,
            inside,
            make_nurb_gen,
        } => {
            finite(&[bot, top, start, end, h], "cone")?;
            (
                4.0,
                [bot, top, start, end, h, b(swap), b(inside), b(make_nurb_gen)],
            )
        }
        Mode::CylBounded {
            radius,
            start,
            end,
            height: h,
            swap,
            inside,
            make_nurb_gen,
        } => {
            finite(&[radius, start, end, h], "cylinder")?;
            (
                5.0,
                [radius, start, end, h, b(swap), b(inside), b(make_nurb_gen), 0.0],
            )
        }
        Mode::CylUnbounded {
            radius,
            swap,
            inside,
        } => {
            finite(&[radius], "radius")?;
            (6.0, [radius, b(swap), b(inside), 0.0, 0.0, 0.0, 0.0, 0.0])
        }
    };
    o.extend([code, b(s.oob)]);
    o.extend_from_slice(&s.origin);
    o.extend_from_slice(&s.x);
    o.extend_from_slice(&s.y);
    o.extend_from_slice(&p);
    Ok(())
}

fn order(k: usize, what: &str) -> Result<f64, FfiError> {
    if k > MAX_ORDER {
        return Err(bad(format!("{what}: order above {MAX_ORDER}")));
    }
    Ok(k as f64)
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Build | Op::Copy => {}
        Op::EvalStep {
            u,
            v,
            hu,
            hv,
            point,
        } => {
            finite(&[*u, *v], "eval_step")?;
            o.extend([*u, *v, order(*hu, "eval_step")?, order(*hv, "eval_step")?, b(*point)]);
        }
        Op::Eval {
            u,
            v,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            non_zero,
        } => {
            finite(&[*u, *v], "eval")?;
            o.extend([
                *u,
                *v,
                order(*hu, "eval")?,
                order(*hv, "eval")?,
                b(*u_left),
                b(*v_left),
                b(*upper),
                b(*non_zero),
            ]);
        }
        Op::EvalPoint { u, v } | Op::ToStep { u, v } | Op::ToNurbs { u, v } => {
            finite(&[*u, *v], "uv")?;
            o.extend([*u, *v]);
        }
        Op::Iso { param, iso, dom, uv } => {
            if *param > 1 {
                return Err(bad("iso: param"));
            }
            finite(&[*iso], "iso")?;
            o.extend([f64::from(*param), *iso, b(dom.is_some())]);
            let d = dom.unwrap_or([0.0, 0.0, 0.0, 0.0]);
            finite(&d, "iso domain")?;
            o.extend_from_slice(&d);
            o.push(b(*uv));
        }
        Op::IsoCircle { p, tol } => {
            finite(p, "iso circle")?;
            finite(&[*tol], "iso circle")?;
            o.extend_from_slice(p);
            o.push(*tol);
        }
        Op::Inversion { p, tol, guess } => {
            finite(p, "inversion")?;
            finite(&[*tol], "inversion")?;
            let g = guess.unwrap_or([0.0, 0.0]);
            finite(&g, "inversion guess")?;
            o.extend_from_slice(p);
            o.extend([*tol, b(guess.is_some())]);
            o.extend_from_slice(&g);
        }
        Op::Positive { p } => {
            finite(p, "positive")?;
            o.extend_from_slice(p);
        }
        Op::Adjust { dom } => {
            // a reversed sweep interval makes upstream's surface of revolution loop
            // forever (Reparameterize / MakeNurb)
            finite(dom, "adjust")?;
            let [u0, _, u1, _] = *dom;
            if u0 > u1 {
                return Err(bad("adjust: reversed sweep interval"));
            }
            o.extend_from_slice(dom);
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
            o.push(b(scale.is_some()));
            o.extend_from_slice(&s);
        }
        Op::Eq { other } => push_build(o, other)?,
    }
    Ok(())
}

fn push_curve(o: &mut Vec<f64>, c: &CurveIn) -> Result<(), FfiError> {
    match c {
        CurveIn::Line { a, b: e } => {
            finite(a, "line")?;
            finite(e, "line")?;
            let [ax, ay, az] = *a;
            let [bx, by, bz] = *e;
            if len([bx - ax, by - ay, bz - az]) < 1e-6 {
                return Err(bad("line: degenerate"));
            }
            o.push(1.0);
            o.extend_from_slice(a);
            o.extend_from_slice(e);
        }
        CurveIn::Circle { c, x, y, d, r } => {
            finite(c, "circle")?;
            axes(*x, *y, "circle")?;
            let [d0, d1] = *d;
            angular(d0, d1, "circle")?;
            if !(r.is_finite() && *r > 1e-6) {
                return Err(bad("circle: radius"));
            }
            o.push(2.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *r]);
        }
        CurveIn::Ellipse { c, x, y, d, rx, ry } => {
            finite(c, "ellipse")?;
            axes(*x, *y, "ellipse")?;
            let [d0, d1] = *d;
            angular(d0, d1, "ellipse")?;
            if !(rx.is_finite() && *rx > 1e-6 && ry.is_finite() && *ry > 1e-6) {
                return Err(bad("ellipse: radii"));
            }
            o.push(3.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *rx, *ry]);
        }
    }
    Ok(())
}

fn call(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: finite values, independent axes,
        // bounded semi-angles, positive bounded heights, valid angular and ordered
        // domains, bounded orders, non-degenerate curves); buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_cone_p1_eval(
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

/// Builds the cone `s` and runs `op` on it; returns the shim's record (layout in the
/// module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn cone_op(s: &Build, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_build(&mut input, s)?;
    push_op(&mut input, op)?;
    call(op.code(), &input)
}

/// `sm_TotalCoincidenceChecker(a, [a0, a1], b, [b0, b1], tol)`; returns the shim's
/// record (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn coincidence(
    a: &CurveIn,
    a_dom: [f64; 2],
    b: &CurveIn,
    b_dom: [f64; 2],
    tol: f64,
) -> Result<Vec<f64>, FfiError> {
    finite(&a_dom, "domain")?;
    finite(&b_dom, "domain")?;
    finite(&[tol], "tolerance")?;
    let mut input = Vec::new();
    push_curve(&mut input, a)?;
    input.extend_from_slice(&a_dom);
    push_curve(&mut input, b)?;
    input.extend_from_slice(&b_dom);
    input.push(tol);
    call(OP_COINCIDENCE, &input)
}
