//! FFI wrappers for unit `surface.plane_p1`.
//!
//! Shim: `harness/cpp/shim/surface_plane_p1.cpp` (`sc_surface_plane_p1_run`). Upstream:
//! `source/SMLib/src/SmPlane.cpp`, `source/SMLib/inc/SmPlane.h` (`SmPlane`).
//!
//! [`run`] builds plane A from a [`Make`], runs the [`Op`]s on it in order and returns
//! the record `make_status`, then per op `op status [values]` (values only for status
//! 0), then the state of A (nothing after a failed construction). Statuses are `0`
//! (`SM_SUCCESS`) or the `SmStatus`.
//!
//! A plane state is `type O[3] X[3] Y[3] Z[3] domain[4] scale[2] oob knot_type form`
//! then the NURB `has [n m p q r s cpts(x y z w) u v]` (the plane's, or the one a copy
//! makes when it has none). A curve is `type dim [P[3] V[3] dom[2] scale inside_out]`
//! (the bracket for lines) then `knot_type form has [n p m cpts(x y z w) knots]` and the
//! natural interval. Solutions are `count` then per solution `type nvars nobjs obj0
//! start_value start[3] end_value end[3]` (objects: `0` = A, `1` = another, `-1` =
//! none; unused parameters and the end of a single value are `0`).
//!
//! | [`Op`]          | upstream                                      | values |
//! |-----------------|-----------------------------------------------|--------|
//! | `State`         | getters                                       | state |
//! | `Evaluate`      | `Evaluate(uv, hu, hv, TRUE, TRUE, upper)`     | `(hu+1)(hv+1)` vectors, `D[i][j]` row by row |
//! | `EvalPoint`, `EvalFast` | `EvaluatePoint`, `EvaluatePointFast`  | point |
//! | `Normal`        | `EvaluateNormal(uv, TRUE, TRUE)`              | vector |
//! | `Project`       | `ProjectPointToUVDomain`                      | uv |
//! | `Queries`       | `IsBounded`, `IsPlanar`, `IsOnSeam`, `IsClosed`, both `IsSingularity`, `GetType`, `GetTypeString`, `GetClassString` | values, type chars, `-1`, class chars |
//! | `KindOf`        | `IsKindOf`                                    | flag |
//! | `Adjust`, `MakeNurb`, `Reparam`, `Toggle`, `Rebuild`, `Reverse`, `Swap` | `AdjustSTEPUVDomain`, `MakeNurb`, `Reparameterize`, `ToggleSwapUVBit`, `RebuildSTEPFromNURBParameters`, `Reverse`, `SwapUV` | |
//! | `Drop`          | `DropPointFast(dom, op, p, NULL, tol, target, SM_SR_ALL)` | solutions |
//! | `Line`          | `GlobalLineIntersect`                         | solutions |
//! | `TrimLine`      | `TrimPlaneLineToPlaneDomain`                  | `found [start end]` |
//! | `Iso`           | `CreateIsoParametricCurve`                    | curve, gap, `has [uv curve]` |
//! | `DropCurve`     | `DropCurve`                                   | `max_drop max_dev count curves` |
//! | `Coincidence`   | `CoincidenceCheck`                            | `flag max_dist` |
//! | `Eq`            | `operator==`                                  | flag |
//! | `Copy`          | `Copy`                                        | state |
//! | `Grazing`       | `sm_IsGrazingIntersection` (file static, copied into the shim) | flag |
//! | `Inherited`     | of a copy: `GetNaturalUVDomain`, `GetMaxAnalyticDomain`, `IsBilinear`, `IsParallelToVector(v)`, `GetDegree` and `GetNumberControlPoints` (u, v) | values |
//! | `Extend`        | of a copy: `CreateExtendedSurface(dist, cont)` or `CreateExtendedSurface(dir, dist, cont)` | the extension's NURB `has [..]` |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], well-formed
//! clamped B-splines (at most [`MAX_INDEX`] + 1 control points per direction, degree
//! at most [`MAX_DEGREE`], positive weights or none) and at most [`MAX_OPS`] ops.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_plane_p1_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest control point index `n` (curves: 32, surfaces: 16 per direction).
pub const MAX_INDEX: usize = 16;
/// Largest degree.
pub const MAX_DEGREE: usize = 8;
/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;

/// How plane A (or another plane) is built.
#[derive(Debug, Clone, PartialEq)]
pub enum Make {
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Bounded {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// u and v scales.
        scale: [f64; 2],
        /// `umin vmin umax vmax`, stored as given.
        domain: [f64; 4],
    },
    /// `SmPlane(o, normal, ctx)`.
    Normal {
        /// Origin.
        o: [f64; 3],
        /// Normal.
        n: [f64; 3],
    },
    /// `SmPlane::CreateCanonical(ctx, SmAxis2Placement(o, x, y), p)`.
    Canonical {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
    },
}

/// A domain operand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dom {
    /// `GetSTEPUVDomain()` at op time.
    Step,
    /// `SmExtent2d(umin, vmin, umax, vmax)`.
    Explicit([f64; 4]),
    /// `GetNaturalUVDomain()` of a copy at op time.
    Natural,
}

/// A clamped B-spline curve: `SmBSplineCurve(dim, n, m, p, Pw, U)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Image dimension, 2 or 3.
    pub dim: u32,
    /// Degree.
    pub p: usize,
    /// Control points as stored (`x y z w`, `w` = [`NOW`] when non-rational).
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`n + p + 2` of them).
    pub knots: Vec<f64>,
}

/// A curve operand.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine::CreateLineSegment(ctx, 3, a, b, l)`.
    Segment([f64; 3], [f64; 3]),
    /// A B-spline curve.
    BSpline(CurveData),
}

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

/// The other surface of a coincidence test.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// A plane.
    Plane(Box<Make>),
    /// A B-spline surface.
    BSpline(SurfaceData),
}

/// One operation on plane A.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The state of A.
    State,
    /// `Evaluate(uv, hu, hv, TRUE, TRUE, upper)`.
    Evaluate {
        /// Parameter.
        uv: [f64; 2],
        /// Highest u derivative (at most 4).
        hu: usize,
        /// Highest v derivative (at most 4).
        hv: usize,
        /// Upper half only (requires `hu <= hv`).
        upper: bool,
    },
    /// `EvaluatePoint(uv)`.
    EvalPoint([f64; 2]),
    /// `EvaluatePointFast(uv)`.
    EvalFast([f64; 2]),
    /// `EvaluateNormal(uv, TRUE, TRUE)`.
    Normal([f64; 2]),
    /// `ProjectPointToUVDomain(p)`.
    Project([f64; 3]),
    /// The simple queries at `uv`.
    Queries([f64; 2]),
    /// `IsKindOf(t)`.
    KindOf(u32),
    /// `AdjustSTEPUVDomain(dom)`.
    Adjust(Dom),
    /// `MakeNurb()`.
    MakeNurb,
    /// `Reparameterize(dom)`.
    Reparam(Dom),
    /// `ToggleSwapUVBit()`.
    Toggle,
    /// `RebuildSTEPFromNURBParameters()`.
    Rebuild,
    /// `DropPointFast(dom, op, p, NULL, tol, target, SM_SR_ALL)`.
    Drop {
        /// Domain.
        dom: Dom,
        /// `SmSolverOperationType` value (0 to 5).
        op: u8,
        /// Test point.
        p: [f64; 3],
        /// Distance tolerance.
        tol: f64,
        /// Optional target distance.
        target: Option<f64>,
    },
    /// `GlobalLineIntersect(dom, p, v, ivl, ray, tol)`.
    Line {
        /// Domain.
        dom: Dom,
        /// Line point.
        p: [f64; 3],
        /// Line vector.
        v: [f64; 3],
        /// Optional `SmExtent1d(a, b)`.
        ivl: Option<[f64; 2]>,
        /// Fire a ray.
        ray: bool,
        /// Distance tolerance.
        tol: f64,
    },
    /// `TrimPlaneLineToPlaneDomain(dom, a, b, tol)`.
    TrimLine {
        /// Domain.
        dom: Dom,
        /// Segment start.
        a: [f64; 3],
        /// Segment end.
        b: [f64; 3],
        /// 3D tolerance.
        tol: f64,
    },
    /// `CreateIsoParametricCurve(ctx, U or V, iso, 0, .., domain, &gap, want_uv)`.
    Iso {
        /// `false`: `SM_SP_U`, `true`: `SM_SP_V`.
        v_dir: bool,
        /// Iso parameter.
        iso: f64,
        /// Optional domain `SmExtent2d(umin, vmin, umax, vmax)`.
        domain: Option<[f64; 4]>,
        /// Also build the UV curve.
        want_uv: bool,
    },
    /// `DropCurve(ctx, dom, curve, ivl, tol, .., keep_all, NULL, creating)`.
    DropCurve {
        /// Domain.
        dom: Dom,
        /// The curve.
        curve: CurveIn,
        /// `SmExtent1d(a, b)`, or the curve's natural interval.
        ivl: Option<[f64; 2]>,
        /// Approximation tolerance.
        tol: f64,
        /// Keep all drop curves.
        keep_all: bool,
        /// Creating a UV trim curve.
        creating: bool,
    },
    /// `CoincidenceCheck(other, tol, .., whole, ..)`.
    Coincidence {
        /// Other surface.
        other: SurfaceIn,
        /// 3D tolerance.
        tol: f64,
        /// Whole surfaces.
        whole: bool,
    },
    /// `*A == *B`.
    Eq(Box<Make>),
    /// `Copy(ctx, p)`.
    Copy,
    /// `sm_IsGrazingIntersection(curve, t, CreateLineSegment(a, b), s)`.
    Grazing {
        /// The curve.
        curve: CurveIn,
        /// Curve parameter.
        t: f64,
        /// Line start and end.
        line: [[f64; 3]; 2],
        /// Line parameter.
        s: f64,
    },
    /// The inherited queries of a copy (`IsParallelToVector(v)`).
    Inherited([f64; 3]),
    /// `Reverse(U or V)` (`true`: `SM_SP_V`).
    Reverse(bool),
    /// `SwapUV()`.
    Swap,
    /// `CreateExtendedSurface` of a copy: without a direction the `SM_SP_BOTH`
    /// overload, else with `SmSurfParamType` `dir` (at most `SM_SP_VMAX`, 8). The
    /// distance must be positive.
    Extend {
        /// The `SmSurfParamType` code, or `None` for the overload without one.
        dir: Option<u8>,
        /// Extension distance.
        dist: f64,
        /// The `SmContinuityType` code (at most `SM_CT_CINFINITY`, 13).
        cont: u8,
    },
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

    fn make(&mut self, m: &Make) -> Result<(), FfiError> {
        match m {
            Make::Bounded {
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
            Make::Normal { o, n } => {
                // SmVector3d::MakeUnitOrthoVectors fails for |n|^2 < SM_EFF_ZERO_SQ and the
                // constructor then reads uninitialised axes (SE ignores the status)
                if n[0] * n[0] + n[1] * n[1] + n[2] * n[2] < 1.0e-24 {
                    return Err(bad("zero plane normal"));
                }
                self.int(1);
                self.nums(o)?;
                self.nums(n)
            }
            Make::Canonical { o, x, y } => {
                self.int(2);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)
            }
        }
    }

    fn dom(&mut self, d: &Dom) -> Result<(), FfiError> {
        match d {
            Dom::Step => {
                self.int(0);
                self.nums(&[0.0; 4])
            }
            Dom::Explicit(v) => {
                self.int(1);
                self.nums(v)
            }
            Dom::Natural => {
                self.int(2);
                self.nums(&[0.0; 4])
            }
        }
    }

    fn curve(&mut self, c: &CurveIn) -> Result<(), FfiError> {
        match c {
            CurveIn::Segment(a, b) => {
                if a == b {
                    return Err(bad("degenerate segment"));
                }
                self.int(0);
                self.nums(a)?;
                self.nums(b)
            }
            CurveIn::BSpline(d) => {
                let n1 = d.cpts.len();
                if !(2..=2 * MAX_INDEX + 1).contains(&n1)
                    || !(1..=MAX_DEGREE).contains(&d.p)
                    || d.p >= n1
                    || (d.dim != 2 && d.dim != 3)
                {
                    return Err(bad("B-spline curve size, degree or dimension"));
                }
                check_knots(&d.knots, n1, d.p)?;
                check_weights(d.cpts.iter())?;
                self.int(1);
                self.int(d.dim as usize);
                self.int(n1 - 1);
                self.int(d.p);
                self.int(d.knots.len() - 1);
                for c in &d.cpts {
                    self.cpt(c)?;
                }
                self.nums(&d.knots)
            }
        }
    }

    fn surface(&mut self, s: &SurfaceIn) -> Result<(), FfiError> {
        match s {
            SurfaceIn::Plane(m) => {
                self.int(0);
                self.make(m)
            }
            SurfaceIn::BSpline(d) => {
                let n1 = d.net.len();
                let m1 = d.net.first().map_or(0, Vec::len);
                if !(2..=MAX_INDEX + 1).contains(&n1)
                    || !(2..=MAX_INDEX + 1).contains(&m1)
                    || d.net.iter().any(|r| r.len() != m1)
                    || !(1..=MAX_DEGREE).contains(&d.p)
                    || !(1..=MAX_DEGREE).contains(&d.q)
                    || d.p >= n1
                    || d.q >= m1
                {
                    return Err(bad("B-spline surface size or degree"));
                }
                check_knots(&d.u, n1, d.p)?;
                check_knots(&d.v, m1, d.q)?;
                check_weights(d.net.iter().flatten())?;
                self.int(1);
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
                self.nums(&d.v)
            }
        }
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => self.int(1),
            Op::Evaluate { uv, hu, hv, upper } => {
                if *hu > 4 || *hv > 4 || (*upper && hu > hv) {
                    return Err(bad("derivative orders"));
                }
                self.int(2);
                self.nums(uv)?;
                self.int(*hu);
                self.int(*hv);
                self.flag(*upper);
            }
            Op::EvalPoint(uv) => {
                self.int(3);
                self.nums(uv)?;
            }
            Op::EvalFast(uv) => {
                self.int(4);
                self.nums(uv)?;
            }
            Op::Normal(uv) => {
                self.int(5);
                self.nums(uv)?;
            }
            Op::Project(p) => {
                self.int(6);
                self.nums(p)?;
            }
            Op::Queries(uv) => {
                self.int(7);
                self.nums(uv)?;
            }
            Op::KindOf(t) => {
                if *t > 100_000 {
                    return Err(bad("type"));
                }
                self.int(8);
                self.int(*t as usize);
            }
            Op::Adjust(d) => {
                self.int(9);
                self.dom(d)?;
            }
            Op::MakeNurb => self.int(10),
            Op::Reparam(d) => {
                self.int(11);
                self.dom(d)?;
            }
            Op::Toggle => self.int(12),
            Op::Rebuild => self.int(13),
            Op::Drop {
                dom,
                op,
                p,
                tol,
                target,
            } => {
                if *op > 5 {
                    return Err(bad("solver operation"));
                }
                self.int(14);
                self.dom(dom)?;
                self.int(usize::from(*op));
                self.nums(p)?;
                self.num(*tol)?;
                self.flag(target.is_some());
                self.num(target.unwrap_or(0.0))?;
            }
            Op::Line {
                dom,
                p,
                v,
                ivl,
                ray,
                tol,
            } => {
                self.int(15);
                self.dom(dom)?;
                self.nums(p)?;
                self.nums(v)?;
                self.flag(ivl.is_some());
                self.nums(&ivl.unwrap_or([0.0, 0.0]))?;
                self.flag(*ray);
                self.num(*tol)?;
            }
            Op::TrimLine { dom, a, b, tol } => {
                self.int(16);
                self.dom(dom)?;
                self.nums(a)?;
                self.nums(b)?;
                self.num(*tol)?;
            }
            Op::Iso {
                v_dir,
                iso,
                domain,
                want_uv,
            } => {
                self.int(17);
                self.flag(*v_dir);
                self.num(*iso)?;
                self.flag(domain.is_some());
                self.nums(&domain.unwrap_or([0.0; 4]))?;
                self.flag(*want_uv);
            }
            Op::DropCurve {
                dom,
                curve,
                ivl,
                tol,
                keep_all,
                creating,
            } => {
                self.int(18);
                self.dom(dom)?;
                self.curve(curve)?;
                self.flag(ivl.is_some());
                self.nums(&ivl.unwrap_or([0.0, 0.0]))?;
                self.num(*tol)?;
                self.flag(*keep_all);
                self.flag(*creating);
            }
            Op::Coincidence { other, tol, whole } => {
                self.int(19);
                self.surface(other)?;
                self.num(*tol)?;
                self.flag(*whole);
            }
            Op::Eq(m) => {
                self.int(20);
                self.make(m)?;
            }
            Op::Copy => self.int(21),
            Op::Grazing { curve, t, line, s } => {
                if line[0] == line[1] {
                    return Err(bad("degenerate segment"));
                }
                self.int(22);
                self.curve(curve)?;
                self.num(*t)?;
                self.nums(&line[0])?;
                self.nums(&line[1])?;
                self.num(*s)?;
            }
            Op::Inherited(v) => {
                self.int(23);
                self.nums(v)?;
            }
            Op::Reverse(v_dir) => {
                self.int(24);
                self.flag(*v_dir);
            }
            Op::Swap => self.int(25),
            Op::Extend { dir, dist, cont } => {
                if dir.is_some_and(|d| d > 8) || *cont > 13 {
                    return Err(bad("extension direction or continuity"));
                }
                // upstream halves a failing extension until it falls below dDist / 100,
                // which never happens for a zero distance (an endless loop)
                if dist.is_nan() || *dist <= 0.0 {
                    return Err(bad("extension distance must be positive"));
                }
                self.int(26);
                self.flag(dir.is_some());
                self.int(usize::from(dir.unwrap_or(0)));
                self.num(*dist)?;
                self.int(usize::from(*cont));
            }
        }
        Ok(())
    }
}

/// The flat program [`run`] hands the shim.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs [`run`] rejects.
pub fn encode(make: &Make, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    if ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    let mut e = Enc(Vec::new());
    e.make(make)?;
    e.int(ops.len());
    for op in ops {
        e.op(op)?;
    }
    Ok(e.0)
}

/// Builds plane A from `make`, runs `ops` on it and returns the record (see the module
/// docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(make: &Make, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    let prog = encode(make, ops)?;
    let n_in = i64::try_from(prog.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_surface_plane_p1_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
