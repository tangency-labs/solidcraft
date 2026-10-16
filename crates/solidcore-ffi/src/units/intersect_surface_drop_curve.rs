//! FFI wrappers for unit `intersect.surface_drop_curve`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_drop_curve.cpp`
//! (`sc_intersect_surface_drop_curve_run`). Upstream: `source/SMLib/src/SmSurfaceDropCurve.cpp`,
//! `source/SMLib/inc/SmSurfaceDropCurve.h` (`SmSurfaceDropCurve`, `sm_TestPoint`, `sm_AddBreak`,
//! `sm_IsGrazingIntersection`), compiled into the shim.
//!
//! [`run`] builds a surface (a real `SmBSplineSurface`, or an `SmPlane` with its NURB made), a
//! curve (a real `SmLine` segment or `SmBSplineCurve`), an `SmSurfaceDropCurve` over a domain and
//! a curve interval,
//! sets `SetCurveIsOnSurface`, the optional tolerances, working interval, working domain, knot
//! vector and breaks, runs the [`Op`]s in order and returns the record:
//!
//! * the constructor's approximation and angle tolerances;
//! * per op `op status [values]` (see the table);
//! * the final state: tolerances, `GetMaxDistToSurface`, `GetCurveIsOnSurface`, the working
//!   interval `min max`, the working domain `umin vmin umax vmax`, the knot vector and the
//!   breaks (`has [n values]` each), the number of curve points, the start points (`n` then
//!   `type param uv[2] uvtan[2]` each) and the accumulated outputs (3D curves, UV curves, drop
//!   distances, deviations).
//!
//! Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1` marks an op that needs the last
//! `Cpv` point when there is none. A trace point is `type param drop nld c3d[0](3) c3d[1](3)
//! cuv[0](2) cuv[1](2) S00(3) S10(3) S01(3) S22(3)`. A curve is `type dim ivl[2]` then, for
//! B-splines that are not lines, `has [n p m cpts(x y z w) knots]`; a curve list is `n` curves,
//! a double list `n` values.
//!
//! | [`Op`]       | upstream                                                       | values |
//! |--------------|----------------------------------------------------------------|--------|
//! | `Cpv`        | `ComputePointValues(uv, pnt, prev)` on a new point (zero vectors, the given parameter); `prev` a copy of the last successful `Cpv` point when asked for and there is one | the point, `has_prev [prev uvtan(2)]` (status 0 only); the point becomes the last point |
//! | `Step`       | `ComputeStepSize(pnt(param), old, new)`                        | `new` |
//! | `AddBreak`   | `sm_AddBreak(values, new)`                                     | the array |
//! | `TestPoint`  | `sm_TestPoint(surface, domain, uv, point, tol, ..)`            | `good drop nld` (status 0 only) |
//! | `Graze`      | `sm_IsGrazingIntersection(curve, p1, curve2, p2)`              | `flag` |
//! | `Lps`        | `LocalPointSolve(pnt, guess, next(param), found, uv)`          | `found next [uv]` (status 0 only) |
//! | `Next`       | `ComputeNextPoint(curr, step, next, ..)`; `curr` a new trace point manager block holding the last point with the given type, appended to the curve points when `first` | `found dev ang clipped boundary next curr` (status 0 only) |
//! | `Span`       | `ComputeSpanDeviation(last, last, step, dev, ok)`              | `ok [dev]` (status 0 only) |
//! | `Trace2`     | `TraceCurve2(block holding the last point, acc..)`             | (none) |
//! | `Interior`   | `FindInteriorCurves(acc..)`                                     | (none) |
//! | `Boundary`   | `FindBoundaryStartPoints(acc..)`                                | the start points (status 0 only) |
//! | `DoTrace`    | `DoTrace(ctx, &tol, &ang, ..)` with the current tolerances      | the four outputs (status 0 only) |
//! | `Hooks`      | `TestPoint(pnt, done)` and `IsPointOnCurve(point, acc 3D curves)` | `done on_curve` |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`]; well-formed clamped
//! B-splines (at most [`MAX_INDEX`] + 1 control points per surface direction,
//! [`MAX_CURVE_INDEX`] + 1 per curve, degree at most [`MAX_DEGREE`], positive weights or none);
//! segments with distinct ends; planes with independent axes and positive scales; a domain with
//! positive extent, inside the surface's natural domain for a B-spline; a curve interval and
//! every curve parameter (`Cpv`, `Lps`, `Graze`) inside the curve's natural interval (upstream
//! evaluates them unclamped); for a B-spline surface, `Cpv` points and `TestPoint` domains inside
//! the natural domain; positive tolerances; a working domain of positive extent (inside the
//! natural domain for a B-spline), required by `Lps`, `Next` and `Span` (which reach the
//! surface point solves over it); at most [`MAX_ARRAY`] knots, breaks or `AddBreak` values; at
//! most [`MAX_OPS`] ops. `Step` needs a non-empty knot vector (given, or made by an earlier
//! `Boundary`/`DoTrace`) unless the parameter is within `1e-6` of the given working interval's
//! maximum before any op changed it (upstream reads the knot vector otherwise: a null or empty
//! one crashes); `Trace2` needs breaks and a non-empty knot vector (given, or made by an
//! earlier `Boundary`/`DoTrace`; its tracer fallback reaches `ComputeStepSize`).

use core::cmp::Ordering;

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_drop_curve_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest control point index `n` of a surface direction.
pub const MAX_INDEX: usize = 16;
/// Largest control point index `n` of a curve.
pub const MAX_CURVE_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 8;
/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most values of a knot vector, break array or `AddBreak` array.
pub const MAX_ARRAY: usize = 64;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// `SM_EFF_ZERO_SQRT`: `ComputeStepSize`'s end-of-interval tolerance.
const EFF_ZERO_SQRT: f64 = 1.0e-6;

/// A clamped B-spline surface: `SmBSplineSurface(gw_SURFACE*, FALSE, ctx)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceData {
    /// u degree.
    pub p: usize,
    /// v degree.
    pub q: usize,
    /// Control net, row `i` (u index) of `x y z w` points.
    pub net: Vec<Vec<[f64; 4]>>,
    /// u knots.
    pub u: Vec<f64>,
    /// v knots.
    pub v: Vec<f64>,
}

/// The surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// A B-spline surface.
    BSpline(SurfaceData),
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
}

/// A clamped 3D B-spline curve: `SmBSplineCurve(3, n, m, p, Pw, U)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Degree.
    pub p: usize,
    /// Control points as stored (`x y z w`, `w` = [`NOW`] when non-rational).
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`n + p + 2` of them).
    pub knots: Vec<f64>,
}

/// A curve.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine::CreateLineSegment(ctx, 3, a, b, l)` (natural interval `[0, 1]`).
    Segment([f64; 3], [f64; 3]),
    /// A B-spline curve.
    BSpline(CurveData),
}

/// One operation (see the module table).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `ComputePointValues`.
    Cpv {
        /// The point's curve parameter.
        param: f64,
        /// The surface parameter point.
        uv: [f64; 2],
        /// Pass the last point as `pPrevPnt`.
        use_prev: bool,
    },
    /// `ComputeStepSize`.
    Step {
        /// The point's curve parameter.
        param: f64,
        /// The old step size.
        old: f64,
    },
    /// `sm_AddBreak`.
    AddBreak {
        /// The array.
        values: Vec<f64>,
        /// The value inserted.
        new: f64,
    },
    /// `sm_TestPoint`.
    TestPoint {
        /// The working domain `umin vmin umax vmax`.
        domain: [f64; 4],
        /// The parameter point.
        uv: [f64; 2],
        /// The 3D point.
        point: [f64; 3],
        /// The tolerance.
        tol: f64,
    },
    /// `sm_IsGrazingIntersection` of the drop curve with another curve.
    Graze {
        /// The drop curve's parameter.
        p1: f64,
        /// The other curve.
        curve: CurveIn,
        /// Its parameter.
        p2: f64,
    },
    /// `LocalPointSolve`.
    Lps {
        /// The next point's curve parameter.
        param: f64,
        /// The guess.
        guess: [f64; 2],
    },
    /// `ComputeNextPoint` from the last point.
    Next {
        /// Append the current point to the curve points first.
        first: bool,
        /// The current point's type (`0..=4`).
        kind: u8,
        /// The step size.
        step: f64,
    },
    /// `ComputeSpanDeviation` of the last point with itself.
    Span {
        /// The step size.
        step: f64,
    },
    /// `TraceCurve2` from the last point.
    Trace2,
    /// `FindInteriorCurves`.
    Interior,
    /// `FindBoundaryStartPoints`.
    Boundary,
    /// `DoTrace`.
    DoTrace,
    /// `TestPoint` and `IsPointOnCurve`.
    Hooks {
        /// The point tested on the accumulated 3D curves.
        point: [f64; 3],
    },
}

/// A whole case.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The surface.
    pub surface: SurfaceIn,
    /// The tracer domain `umin vmin umax vmax`.
    pub domain: [f64; 4],
    /// The curve dropped.
    pub curve: CurveIn,
    /// The curve interval `[a, b]`.
    pub interval: [f64; 2],
    /// `SetCurveIsOnSurface`.
    pub on_surface: bool,
    /// `m_dThisApproxTol3d` override.
    pub tol: Option<f64>,
    /// `m_dThisAngTolRad` override.
    pub ang: Option<f64>,
    /// `m_vWorkingInterval` (`SmExtent1d(a, b)`).
    pub working_interval: Option<[f64; 2]>,
    /// `m_vWorkingUVDomain`.
    pub working_domain: Option<[f64; 4]>,
    /// `m_pCurveKnotVector`.
    pub knots: Option<Vec<f64>>,
    /// `m_pCurveBreaks`.
    pub breaks: Option<Vec<f64>>,
    /// The operations.
    pub ops: Vec<Op>,
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("non-finite or huge value"))
    }
}

/// Checks a clamped knot vector for `n1` control points of degree `p`.
fn check_knots(knots: &[f64], n1: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != n1 + p + 1 {
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

/// Checks the weights: all [`NOW`] or all in `(0, MAX_COORD]`.
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

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// `d` has positive extent (and, unless `nat` is `None`, lies inside `nat`; both
/// `umin vmin umax vmax`).
fn check_domain(d: [f64; 4], nat: Option<[f64; 4]>) -> Result<(), FfiError> {
    if !(d[0] < d[2] && d[1] < d[3]) {
        return Err(bad("domain extent"));
    }
    let Some(nat) = nat else {
        return Ok(());
    };
    if d[0] < nat[0] || d[1] < nat[1] || d[2] > nat[2] || d[3] > nat[3] {
        return Err(bad("domain outside the surface domain"));
    }
    Ok(())
}

/// `uv` inside `nat` (closed); anything finite when `nat` is `None`.
fn check_uv(uv: [f64; 2], nat: Option<[f64; 4]>) -> Result<(), FfiError> {
    let Some(nat) = nat else {
        return Ok(());
    };
    if uv[0] < nat[0] || uv[0] > nat[2] || uv[1] < nat[1] || uv[1] > nat[3] {
        return Err(bad("point outside the surface domain"));
    }
    Ok(())
}

/// `t` inside the closed interval `ivl`.
fn check_param(t: f64, ivl: (f64, f64)) -> Result<(), FfiError> {
    if t < ivl.0 || t > ivl.1 {
        return Err(bad("curve parameter outside the natural interval"));
    }
    Ok(())
}

struct Enc(Vec<f64>);

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(finite(x)?);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.0.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.0.push(if b { 1.0 } else { 0.0 });
    }
    fn nums(&mut self, v: &[f64]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        // the weight was checked (NL_NOW or positive)
        self.0.push(c[3]);
        Ok(())
    }
    fn array(&mut self, v: &[f64]) -> Result<(), FfiError> {
        if v.len() > MAX_ARRAY {
            return Err(bad("array too long"));
        }
        self.int(v.len());
        self.nums(v)
    }

    /// The surface block; returns its natural domain.
    fn surface(&mut self, s: &SurfaceIn) -> Result<[f64; 4], FfiError> {
        match s {
            SurfaceIn::BSpline(d) => {
                let n1 = d.net.len();
                let m1 = d.net.first().map_or(0, Vec::len);
                if !(2..=MAX_INDEX + 1).contains(&n1)
                    || !(2..=MAX_INDEX + 1).contains(&m1)
                    || d.net.iter().any(|r| r.len() != m1)
                {
                    return Err(bad("control net size"));
                }
                if d.p == 0 || d.q == 0 || d.p > MAX_DEGREE || d.q > MAX_DEGREE {
                    return Err(bad("degree"));
                }
                if d.p >= n1 || d.q >= m1 {
                    return Err(bad("degree too high for the net"));
                }
                check_knots(&d.u, n1, d.p)?;
                check_knots(&d.v, m1, d.q)?;
                check_weights(d.net.iter().flatten())?;
                self.0.push(0.0);
                self.int(n1 - 1);
                self.int(m1 - 1);
                self.int(d.p);
                self.int(d.q);
                self.int(d.u.len() - 1);
                self.int(d.v.len() - 1);
                for c in d.net.iter().flatten() {
                    self.cpt(c)?;
                }
                self.nums(&d.u)?;
                self.nums(&d.v)?;
                let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
                    (d.u.first(), d.u.last(), d.v.first(), d.v.last())
                else {
                    return Err(bad("knot count"));
                };
                Ok([u0, v0, u1, v1])
            }
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                let c = norm(cross(*x, *y));
                let independent =
                    c.partial_cmp(&(1e-6 * norm(*x) * norm(*y))) == Some(Ordering::Greater);
                if !independent || norm(*x) < 1e-9 || norm(*y) < 1e-9 {
                    return Err(bad("plane axes"));
                }
                if !(scale[0] > 1e-9 && scale[1] > 1e-9) {
                    return Err(bad("plane scale"));
                }
                if !(domain[0] < domain[2] && domain[1] < domain[3]) {
                    return Err(bad("plane domain"));
                }
                self.0.push(1.0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)?;
                Ok(*domain)
            }
        }
    }

    /// The curve block; returns its natural interval.
    fn curve(&mut self, c: &CurveIn) -> Result<(f64, f64), FfiError> {
        match c {
            CurveIn::Segment(a, b) => {
                if norm([b[0] - a[0], b[1] - a[1], b[2] - a[2]]) < 1e-9 {
                    return Err(bad("degenerate segment"));
                }
                self.0.push(0.0);
                self.nums(a)?;
                self.nums(b)?;
                Ok((0.0, 1.0))
            }
            CurveIn::BSpline(d) => {
                let n1 = d.cpts.len();
                if !(2..=MAX_CURVE_INDEX + 1).contains(&n1)
                    || d.p == 0
                    || d.p > MAX_DEGREE
                    || d.p >= n1
                {
                    return Err(bad("curve size"));
                }
                check_knots(&d.knots, n1, d.p)?;
                check_weights(d.cpts.iter())?;
                self.0.push(1.0);
                self.int(n1 - 1);
                self.int(d.p);
                self.int(d.knots.len() - 1);
                for c in &d.cpts {
                    self.cpt(c)?;
                }
                self.nums(&d.knots)?;
                match (d.knots.first(), d.knots.last()) {
                    (Some(&a), Some(&b)) => Ok((a, b)),
                    _ => Err(bad("knot count")),
                }
            }
        }
    }
}

/// What the validator tracks of the drop curve's state across ops.
struct State {
    /// The knot vector is set and non-empty.
    knots: bool,
    /// The breaks are set.
    breaks: bool,
    /// The working interval's maximum while no op has changed it.
    wi_max: Option<f64>,
}

/// Encodes and validates `prog` (see the module docs for what is accepted).
#[allow(clippy::too_many_lines)] // one flat encoder
fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc(Vec::new());
    let nat = e.surface(&prog.surface)?;
    // A plane evaluates linearly anywhere, so only B-spline points and domains are bounded.
    let bound = match prog.surface {
        SurfaceIn::BSpline(_) => Some(nat),
        SurfaceIn::Plane { .. } => None,
    };
    check_domain(prog.domain, bound)?;
    e.nums(&prog.domain)?;
    let civl = e.curve(&prog.curve)?;
    let [i0, i1] = prog.interval;
    check_param(i0, civl)?;
    check_param(i1, civl)?;
    e.nums(&prog.interval)?;
    e.flag(prog.on_surface);
    for v in [prog.tol, prog.ang] {
        if v.is_some_and(|t| t.partial_cmp(&0.0) != Some(Ordering::Greater)) {
            return Err(bad("tolerance"));
        }
        e.flag(v.is_some());
        e.num(v.unwrap_or(0.0))?;
    }
    e.flag(prog.working_interval.is_some());
    let wi = prog.working_interval.unwrap_or([0.0, 0.0]);
    e.nums(&wi)?;
    e.flag(prog.working_domain.is_some());
    if let Some(wd) = prog.working_domain {
        check_domain(wd, bound)?;
    }
    e.nums(&prog.working_domain.unwrap_or([0.0, 0.0, 0.0, 0.0]))?;
    let mut state = State {
        knots: prog.knots.as_ref().is_some_and(|k| !k.is_empty()),
        breaks: prog.breaks.is_some(),
        wi_max: prog
            .working_interval
            .map(|[a, b]| if a > b { a } else { b }),
    };
    for arr in [&prog.knots, &prog.breaks] {
        e.flag(arr.is_some());
        if let Some(a) = arr {
            e.array(a)?;
        }
    }
    if prog.ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    let has_wd = prog.working_domain.is_some();
    e.int(prog.ops.len());
    for op in &prog.ops {
        match op {
            Op::Cpv {
                param,
                uv,
                use_prev,
            } => {
                check_param(*param, civl)?;
                check_uv(*uv, bound)?;
                e.int(1);
                e.num(*param)?;
                e.nums(uv)?;
                e.flag(*use_prev);
            }
            Op::Step { param, old } => {
                let at_end = state
                    .wi_max
                    .is_some_and(|m| (*param - m).abs() < EFF_ZERO_SQRT);
                if !state.knots && !at_end {
                    return Err(bad("step size without a knot vector"));
                }
                e.int(2);
                e.num(*param)?;
                e.num(*old)?;
            }
            Op::AddBreak { values, new } => {
                e.int(3);
                e.array(values)?;
                e.num(*new)?;
            }
            Op::TestPoint {
                domain,
                uv,
                point,
                tol,
            } => {
                check_domain(*domain, bound)?;
                e.int(4);
                e.nums(domain)?;
                e.nums(uv)?;
                e.nums(point)?;
                e.num(*tol)?;
            }
            Op::Graze { p1, curve, p2 } => {
                check_param(*p1, civl)?;
                e.int(5);
                e.num(*p1)?;
                let c2 = e.curve(curve)?;
                check_param(*p2, c2)?;
                e.num(*p2)?;
            }
            Op::Lps { param, guess } => {
                check_param(*param, civl)?;
                if !has_wd {
                    return Err(bad("point solve without a working domain"));
                }
                e.int(6);
                e.num(*param)?;
                e.nums(guess)?;
            }
            Op::Next { first, kind, step } => {
                if *kind > 4 {
                    return Err(bad("point type"));
                }
                if !has_wd {
                    return Err(bad("next point without a working domain"));
                }
                e.int(7);
                e.flag(*first);
                e.int(usize::from(*kind));
                e.num(*step)?;
            }
            Op::Span { step } => {
                if !has_wd {
                    return Err(bad("span deviation without a working domain"));
                }
                e.int(8);
                e.num(*step)?;
            }
            Op::Trace2 => {
                if !state.breaks {
                    return Err(bad("trace without breaks"));
                }
                // the tracer fallback (SmSurfaceTracer::TraceCurve) reads the knot vector
                // in ComputeStepSize: a null or empty one crashes
                if !state.knots {
                    return Err(bad("trace without a knot vector"));
                }
                e.int(9);
                state.wi_max = None;
            }
            Op::Interior => e.int(10),
            Op::Boundary | Op::DoTrace => {
                e.int(if matches!(op, Op::Boundary) { 11 } else { 12 });
                state.knots = true;
                state.breaks = true;
                state.wi_max = None;
            }
            Op::Hooks { point } => {
                e.int(13);
                e.nums(point)?;
            }
        }
    }
    Ok(e.0)
}

/// Runs `prog` through the shim and returns the record (module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs outside what the module docs accept;
/// [`FfiError::Status`] for a shim failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("input too long"))?;
    call_with_buffer(16_384, |buf, len| {
        // SAFETY: input valid for n_in reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_intersect_surface_drop_curve_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
