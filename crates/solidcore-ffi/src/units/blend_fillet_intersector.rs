//! FFI wrappers for unit `blend.fillet_intersector`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_intersector.cpp` (`sc_blend_fillet_intersector_run`).
//! Upstream: `source/SMLib/src/SmFilletIntersector.cpp`,
//! `source/SMLib/inc/SmFilletIntersector.h` (`SmFilletIntersector`, `sm_FlushCrv`,
//! `sm_FindGuessMidTsectPnt`).
//!
//! [`run`] builds surfaces A and B (B may be A itself), the solver
//! `SmConstantRadiusFS(ctx, tol, ang_tol, tangency_tol, r0, r1, A, B, orient0, orient1)`, the
//! intersector `SmFilletIntersector(A, dom1, B, dom2, solver)` with its angle and distance
//! tolerances, `m_bClippedByThroughPoint` and `m_dCurveTraceDirection` set, and a point arena,
//! then runs the one [`Op`]. The record is `op status [values]` (status `0` = `SM_SUCCESS`,
//! else the `SmStatus`; values per op below), the state `dom1[4] dom2[4] ang_tol approx_tol
//! boundary_test touch_boundary self_intersect clipped trace_dir kind_of(SmFilletIntersector)
//! kind_of(SmAdvSurfaceIntersector) kind_of(7) offset0 offset1 curve_count` (the solver's
//! offset distances), then `npoints` and every point.
//!
//! A point is 80 values: `type ptype[4] (uv pos, uv tangent) x 2, surface matrix [3][3] x 2
//! (row major, 3 values each), curve pos, curve tangent, parameter, angle, deviation, user[4]`.
//!
//! | [`Op`]     | upstream                                                  | values |
//! |------------|-----------------------------------------------------------|--------|
//! | `State`    | (nothing)                                                 | |
//! | `Span`     | `TestSpanAccuracy(a, b, mid)`                             | `good deviation angle` ([`UNSET`] when not set) |
//! | `Next`     | `ComputeNextPoint(a, step, b)`                            | `good deviation angle clipped boundary_hit` |
//! | `Flush`    | `AddPointToCurve` of the points, then `FlushCurve`         | `#curve_types #deviations` |
//! | `Mid`      | `sm_FindGuessMidTsectPnt(a, b, f, new point)`             | the point |
//! | `FlushCrv` | `sm_FlushCrv(ctx, dim, poly, knots, out)`                 | `count` then per curve `type dim knot_type form has [n p m cpts(x y z w) knots] ivl[2]` |
//! | `Delta`    | `ComputePointValuesDelta(uv, uv_delta, a, b)`             | |
//! | `Values`   | `ComputePointValues(uv, a, prev, NULL)`                   | |
//! | `Law`      | `EvaluateLawPoint(param, uv0, uv1, law, FALSE, [0,1], [1,1], origin, normal)` | on success `curve_param uv0 uv1` |
//! | `SetSelf`  | `SetSelfIntersect(flag)`                                  | |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`] (weights also
//! [`NOW`]), well-formed clamped B-spline surfaces, at most [`MAX_POINTS`] points, distinct
//! points where upstream takes two references, and `sm_FlushCrv` polygons that match their
//! increasing knots (anything else `SmBSplineCurve::CreateCanonical` reads past). `Flush`
//! takes at most one point: with two or more, `FlushCurve` dereferences the solver's
//! `m_pExecutive` (unset outside an `SmFilletExecutive`) and the rails of its fillet
//! geometry (`NULL`), which crashes.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_intersector_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_intersector_probe_next(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most points per program.
pub const MAX_POINTS: usize = 16;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// A deviation, angle or offset upstream leaves unset.
pub const UNSET: f64 = -777.0;
/// Values per point.
pub const POINT_LEN: usize = 80;

/// A surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// u and v scales.
        scale: [f64; 2],
        /// `umin vmin umax vmax`.
        domain: [f64; 4],
    },
    /// `SmBSplineSurface(gw_SURFACE*, FALSE, ctx)`.
    BSpline {
        /// u degree.
        p: usize,
        /// v degree.
        q: usize,
        /// Rows (u index) of `[x, y, z, w]` (`w` = [`NOW`] when non-rational).
        net: Vec<Vec<[f64; 4]>>,
        /// u knots.
        u: Vec<f64>,
        /// v knots.
        v: Vec<f64>,
    },
}

/// The solver `SmConstantRadiusFS(ctx, tol, ang_tol, tangency_tol, r0, r1, A, B, o0, o1)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolverIn {
    /// Offset radii of A and B.
    pub radii: [f64; 2],
    /// `bOrientationSurface1/2` (`FALSE` offsets against the normal).
    pub orient: [bool; 2],
    /// `dThisApproxTol3d`.
    pub tol: f64,
    /// `dAngleTolerance`.
    pub ang_tol: f64,
    /// `dTangencyTolerance`.
    pub tangency_tol: f64,
}

/// The op (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Only the state.
    State,
    /// `TestSpanAccuracy(a, b, mid)`.
    Span {
        /// Current point.
        a: usize,
        /// Next point.
        b: usize,
        /// Optional mid point.
        mid: Option<usize>,
    },
    /// `ComputeNextPoint(a, step, b)` (`a != b`).
    Next {
        /// Current point.
        a: usize,
        /// Step size.
        step: f64,
        /// Next point.
        b: usize,
    },
    /// `FlushCurve` after adding these points (at most one) to the curve.
    Flush(Vec<usize>),
    /// `sm_FindGuessMidTsectPnt(a, b, f)`.
    Mid {
        /// Current point.
        a: usize,
        /// Next point.
        b: usize,
        /// Fraction.
        f: f64,
    },
    /// `sm_FlushCrv(dim, poly, knots)`.
    FlushCrv {
        /// Dimension, 1 to 3.
        dim: u32,
        /// Unique increasing knots (none or at least 2).
        knots: Vec<f64>,
        /// Control polygon, `3 (knots - 1) + 1` points.
        poly: Vec<[f64; 3]>,
    },
    /// `ComputePointValuesDelta(uv, uv_delta, a, b)` (`a != b`).
    Delta {
        /// UV points.
        uv: [[f64; 2]; 2],
        /// Delta UV points.
        uv_delta: [[f64; 2]; 2],
        /// Point.
        a: usize,
        /// Delta point.
        b: usize,
    },
    /// `ComputePointValues(uv, a, prev, NULL)` (`prev != a`).
    Values {
        /// UV points.
        uv: [[f64; 2]; 2],
        /// Point.
        a: usize,
        /// Previous point.
        prev: Option<usize>,
    },
    /// `EvaluateLawPoint(param, uv0, uv1, .., origin, normal)`.
    Law {
        /// Curve parameter (unused upstream).
        param: f64,
        /// Guess on A.
        uv0: [f64; 2],
        /// Guess on B.
        uv1: [f64; 2],
        /// Plane origin.
        origin: [f64; 3],
        /// Plane normal.
        normal: [f64; 3],
    },
    /// `SetSelfIntersect(flag)`.
    SetSelf(bool),
}

/// A program (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// Surface A.
    pub a: SurfaceIn,
    /// Surface B, `None` for A itself.
    pub b: Option<SurfaceIn>,
    /// Domain of A.
    pub dom1: [f64; 4],
    /// Domain of B.
    pub dom2: [f64; 4],
    /// The solver.
    pub solver: SolverIn,
    /// `m_dThisAngTolRad`.
    pub ang_tol: f64,
    /// `m_dThisApproxTol3d`.
    pub approx_tol: f64,
    /// `m_bClippedByThroughPoint`.
    pub clipped: bool,
    /// `m_dCurveTraceDirection`.
    pub trace_dir: f64,
    /// Points ([`POINT_LEN`] values each).
    pub points: Vec<Vec<f64>>,
    /// The op.
    pub op: Op,
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("value not finite or too large"))
    }
}

/// Clamped, non-decreasing knots with interior multiplicity at most `p`.
fn check_knots(knots: &[f64], ncp: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != ncp + p + 1 {
        return Err(bad("knot count"));
    }
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots decreasing"));
    }
    let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
        return Err(bad("knot count"));
    };
    if km <= k0
        || knots.iter().take(p + 1).any(|&k| k != k0)
        || knots.iter().rev().take(p + 1).any(|&k| k != km)
    {
        return Err(bad("knots not clamped"));
    }
    let mut run = 0usize;
    let mut last = f64::NAN;
    for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > p {
            return Err(bad("interior knot multiplicity"));
        }
    }
    Ok(())
}

/// All [`NOW`] or all in `(0, MAX_COORD]`.
fn check_weights<'a>(mut ws: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), FfiError> {
    let Some(first) = ws.next() else {
        return Err(bad("no control points"));
    };
    let rational = first[3] != NOW;
    let ok = |w: f64| {
        if rational {
            w > 0.0 && w <= MAX_COORD
        } else {
            w == NOW
        }
    };
    if !ok(first[3]) || ws.any(|c| !ok(c[3])) {
        return Err(bad("weights"));
    }
    Ok(())
}

struct Enc {
    v: Vec<f64>,
    npoints: usize,
}

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.v.push(finite(x)?);
        Ok(())
    }
    fn nums(&mut self, xs: &[f64]) -> Result<(), FfiError> {
        xs.iter().try_for_each(|&x| self.num(x))
    }
    fn int(&mut self, x: usize) {
        self.v.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.v.push(if b { 1.0 } else { 0.0 });
    }
    fn pt(&mut self, i: usize) -> Result<(), FfiError> {
        if i >= self.npoints {
            return Err(bad("point index"));
        }
        self.int(i);
        Ok(())
    }
    fn opt_pt(&mut self, i: Option<usize>) -> Result<(), FfiError> {
        match i {
            None => {
                self.v.push(-1.0);
                Ok(())
            }
            Some(i) => self.pt(i),
        }
    }

    fn surface(&mut self, s: &SurfaceIn) -> Result<(), FfiError> {
        match s {
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                self.int(0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)
            }
            SurfaceIn::BSpline { p, q, net, u, v } => {
                let nu = net.len();
                let nv = net.first().map_or(0, Vec::len);
                if !(1..=8).contains(p) || !(1..=8).contains(q) {
                    return Err(bad("surface degree"));
                }
                if !(2..=17).contains(&nu) || !(2..=17).contains(&nv) || *p >= nu || *q >= nv {
                    return Err(bad("surface size"));
                }
                if net.iter().any(|r| r.len() != nv) {
                    return Err(bad("ragged net"));
                }
                check_knots(u, nu, *p)?;
                check_knots(v, nv, *q)?;
                check_weights(net.iter().flatten())?;
                self.int(1);
                self.int(*p);
                self.int(*q);
                self.int(nu);
                self.int(nv);
                for c in net.iter().flatten() {
                    self.nums(&c[..3])?;
                    self.v.push(c[3]);
                }
                self.nums(u)?;
                self.nums(v)
            }
        }
    }

    fn point(&mut self, p: &[f64]) -> Result<(), FfiError> {
        if p.len() != POINT_LEN {
            return Err(bad("point length"));
        }
        let code = |x: f64, hi: f64| (0.0..=hi).contains(&x) && x.fract() == 0.0;
        let ok = p
            .iter()
            .take(5)
            .enumerate()
            .all(|(k, &t)| code(t, if k == 0 { 7.0 } else { 5.0 }));
        if !ok {
            return Err(bad("point type codes"));
        }
        self.nums(p)
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => self.int(0),
            Op::Span { a, b, mid } => {
                self.int(1);
                self.pt(*a)?;
                self.pt(*b)?;
                self.opt_pt(*mid)?;
            }
            Op::Next { a, step, b } => {
                if a == b {
                    return Err(bad("next point is the current point"));
                }
                self.int(2);
                self.pt(*a)?;
                self.num(*step)?;
                self.pt(*b)?;
            }
            Op::Flush(ids) => {
                if ids.len() > 1 {
                    return Err(bad("FlushCurve of two or more points needs an executive"));
                }
                self.int(3);
                self.int(ids.len());
                for &i in ids {
                    self.pt(i)?;
                }
            }
            Op::Mid { a, b, f } => {
                self.int(4);
                self.pt(*a)?;
                self.pt(*b)?;
                self.num(*f)?;
            }
            Op::FlushCrv { dim, knots, poly } => {
                if !(1..=3).contains(dim) || knots.len() > 64 || poly.len() > 200 {
                    return Err(bad("flush sizes"));
                }
                let consistent = knots.is_empty()
                    || (knots.len() >= 2 && poly.len() == 3 * (knots.len() - 1) + 1);
                if !consistent || knots.windows(2).any(|w| w.first() >= w.last()) {
                    return Err(bad("flush polygon or knots"));
                }
                self.int(5);
                self.int(*dim as usize);
                self.int(knots.len());
                self.nums(knots)?;
                self.int(poly.len());
                for p in poly {
                    self.nums(p)?;
                }
            }
            Op::Delta { uv, uv_delta, a, b } => {
                if a == b {
                    return Err(bad("delta point is the point"));
                }
                self.int(6);
                for p in uv.iter().chain(uv_delta.iter()) {
                    self.nums(p)?;
                }
                self.pt(*a)?;
                self.pt(*b)?;
            }
            Op::Values { uv, a, prev } => {
                if *prev == Some(*a) {
                    return Err(bad("previous point is the point"));
                }
                self.int(7);
                for p in uv {
                    self.nums(p)?;
                }
                self.pt(*a)?;
                self.opt_pt(*prev)?;
            }
            Op::Law {
                param,
                uv0,
                uv1,
                origin,
                normal,
            } => {
                self.int(8);
                self.num(*param)?;
                self.nums(uv0)?;
                self.nums(uv1)?;
                self.nums(origin)?;
                self.nums(normal)?;
            }
            Op::SetSelf(f) => {
                self.int(9);
                self.flag(*f);
            }
        }
        Ok(())
    }
}

/// The flat program [`run`] sends (validated).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything [`run`] rejects (module docs).
pub fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    if prog.points.len() > MAX_POINTS {
        return Err(bad("too many points"));
    }
    let mut e = Enc {
        v: Vec::new(),
        npoints: prog.points.len(),
    };
    e.surface(&prog.a)?;
    match &prog.b {
        None => e.int(2),
        Some(b) => e.surface(b)?,
    }
    e.nums(&prog.dom1)?;
    e.nums(&prog.dom2)?;
    let s = &prog.solver;
    e.nums(&s.radii)?;
    e.flag(s.orient[0]);
    e.flag(s.orient[1]);
    e.nums(&[s.tol, s.ang_tol, s.tangency_tol])?;
    e.nums(&[prog.ang_tol, prog.approx_tol])?;
    e.flag(prog.clipped);
    e.num(prog.trace_dir)?;
    e.int(prog.points.len());
    for p in &prog.points {
        e.point(p)?;
    }
    e.op(&prog.op)?;
    Ok(e.v)
}

/// Runs `prog` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected program; [`FfiError::Status`] for a shim
/// failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_blend_fillet_intersector_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Diagnostic companion for a non-through-point-clipped [`Op::Next`].
///
/// Calls the genuine C++ operation on fresh objects, with copy-only virtual-call
/// observers. Its original flat record is returned first as `length, values...`,
/// followed by `tag, length, values...` actual-call observations only. Tags 1/2
/// observe the plane solve (12/82 values), 3 observes the post-`StepSolve` point
/// and rescaled step (81 values), and 4 observes `ComputePointValues` output
/// (81 values). No supplemental geometry evaluations or reconstruction run.
/// Internal intersection, Hermite and span calculations are not observed.
/// This does not replace [`run`] or add comparison values to the corpus.
///
/// # Errors
///
/// The validation errors of [`encode`], or a shim failure. Other operations and
/// through-point clipping are outside this diagnostic's scope.
pub fn probe_next(prog: &Program) -> Result<Vec<f64>, FfiError> {
    if !matches!(prog.op, Op::Next { .. }) || prog.clipped {
        return Err(bad("probe requires Next without through-point clipping"));
    }
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    // The fixed fixture's complete record and checkpoints fit without a retry
    // (call_with_buffer may otherwise rerun a shim to grow its output buffer).
    call_with_buffer(4096, |buf: &mut [f64], len| {
        // SAFETY: same validated input/output ownership contract as run; no
        // observer or pointer survives this call.
        unsafe {
            sys::sc_blend_fillet_intersector_probe_next(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
