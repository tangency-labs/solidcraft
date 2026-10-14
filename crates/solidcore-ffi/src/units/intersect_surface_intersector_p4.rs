//! FFI wrappers for unit `intersect.surface_intersector_p4`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_intersector_p4.cpp`
//! (`sc_intersect_surface_intersector_p4_run`). Upstream:
//! `source/SMLib/src/SmSurfaceIntersector.cpp` (`TraceCurve`, `FindInteriorCurves`,
//! `DoPointIntersection`, `DoLawIntersection`).
//!
//! [`run`] builds surfaces A and B (planes, B-splines or curve-bounded surfaces over
//! either; B may be A itself), the intersector
//! `SmSurfaceIntersector(A, dom1, B, dom2, fillet)` (its context set as
//! `DoIntersection` does), the program's curves and its points (a point arena; each op
//! names points by index), then runs the [`Op`]s in order. The `FlushCurve` /
//! `FindInteriorCurves` output arrays persist over the program; the 3D curve array
//! starts with the program's curves and is the `r3DCurves` of `TraceCurve`. The record
//! is, per op, `op status [values]`, then the state `approx_tol trace_dir start_tol
//! step_tol ang_tol last_through ext_dist ext_clip closed clipped interior start_point
//! end_point` (point indices, `-3` for none, `-1` for a point the intersector
//! allocated), the curve, start and through lists (`count refs`) and `npoints points`.
//! A list ref is a program point's index, or `-1` followed by the point.
//!
//! A point is 80 values: `type ptype[4] (uv pos, uv tangent) x 2, surface matrix [3][3]
//! x 2 (row major, 3 values each), curve pos, curve tangent, parameter, angle,
//! deviation, user[4]`. Curves are written as `type dim knot_type form has [n p m
//! cpts(x y z w) knots] ivl[2]`; curve outputs are `count curves`. The output arrays are
//! `3d curves` (those after the program's), `uv curves 1`, `uv curves 2`, `ntypes
//! types`, `ndevs devs`.
//!
//! | [`Op`]        | upstream                                                       | values |
//! |---------------|----------------------------------------------------------------|--------|
//! | `State`       | the protected members                                          | state |
//! | `Point`       | the point                                                      | point |
//! | `AddCurve`, `AddStart`, `AddThrough` | `AddPointToCurve`, `AddStartPoint`, `AddThroughPoint` | |
//! | `Cpv`         | `ComputePointValues(uvs, p, prev, step)`                       | point (also on error) |
//! | `Members`     | sets the trace direction and the tolerances                    | |
//! | `Ends`        | sets `m_pStartPoint`, `m_pEndPoint`                            | |
//! | `Ext`         | sets `m_bDoingExtensionClipping`, `m_dLastThroughParmeter`, `m_dExtensionDistance` | |
//! | `Trace`       | `TraceCurve(p, 3d curves)`                                     | |
//! | `Flush`       | `FlushCurve(output arrays)`                                    | outputs (also on error) |
//! | `Fic`         | `FindInteriorCurves(output arrays)`                            | outputs (also on error) |
//! | `Dpi`         | `DoPointIntersection(ctx, points, before, after, start dir, end dir, tol, ang tol, ..)` | single curve (status 0 only) |
//! | `Dli`         | `DoLawIntersection(ctx, points, SmFilletLaw(), orient, curve, ivl, orientations, avg, FALSE, tol, NULL, ..)` | single curve (status 0 only) |
//!
//! A single curve is `has [curve]` for the 3D curve and each UV curve, then the curve
//! type and the deviation (`-7` where upstream leaves them unset).
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], well-formed
//! clamped B-splines, at most [`MAX_POINTS`] points, [`MAX_CURVES`] curves, [`MAX_OPS`]
//! ops and [`MAX_OP_POINTS`] points per `Dpi`/`Dli`, no `Cpv` whose previous point is its
//! point and no `Dli` without curves. The shim does not run (status [`REJECTED`]):
//! `Add*` of a point already on a list (it corrupts the intrusive lists); `Trace` from a
//! start point on a list when there are no curve points (it is appended); `Dpi`/`Dli`
//! with a point twice or a point on a list (they are linked in place).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_intersector_p4_run(
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
pub const MAX_POINTS: usize = 64;
/// Most curves per program.
pub const MAX_CURVES: usize = 8;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most points of a `Dpi` or `Dli`.
pub const MAX_OP_POINTS: usize = 8;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// The op status of an op the shim does not run.
pub const REJECTED: f64 = -2.0;
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
    /// `SmCurveBoundedSurface(basis, {}, {}, TRUE)` over a plane or B-spline surface
    /// (implicit outer, no boundary curves).
    CurveBounded(Box<SurfaceIn>),
}

/// A curve.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine::CreateLineSegment(ctx, 3, a, b, l)`.
    Segment([f64; 3], [f64; 3]),
    /// `SmBSplineCurve(dim, n, m, p, Pw, U, ctx)`.
    BSpline {
        /// Image dimension, 2 or 3.
        dim: u32,
        /// Degree.
        p: usize,
        /// Control points (`x y z w`, `w` = [`NOW`] when non-rational).
        cpts: Vec<[f64; 4]>,
        /// Knots (`n + p + 2`).
        knots: Vec<f64>,
    },
}

/// An op (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The state.
    State,
    /// Point `i`.
    Point(usize),
    /// `AddPointToCurve`.
    AddCurve(usize),
    /// `AddStartPoint`.
    AddStart(usize),
    /// `AddThroughPoint`.
    AddThrough(usize),
    /// `ComputePointValues`.
    Cpv {
        /// UV points.
        uvs: [[f64; 2]; 2],
        /// The point.
        i: usize,
        /// The previous point (not `i`).
        prev: Option<usize>,
        /// The step size.
        step: Option<f64>,
    },
    /// Sets the trace direction and the tolerances.
    Members {
        /// `m_dCurveTraceDirection`.
        trace_dir: f64,
        /// `m_dThisApproxTol3d`.
        approx_tol: f64,
        /// `m_dStartParallelTolRadians`.
        start_tol: f64,
        /// `m_dSteppingParallelTolRadians`.
        step_tol: f64,
        /// `m_dThisAngTolRad`.
        ang_tol: f64,
    },
    /// Sets `m_pStartPoint` and `m_pEndPoint`.
    Ends {
        /// The start point.
        start: Option<usize>,
        /// The end point.
        end: Option<usize>,
    },
    /// Sets the extension clipping members.
    Ext {
        /// `m_bDoingExtensionClipping`.
        clip: bool,
        /// `m_dLastThroughParmeter`.
        last_through: f64,
        /// `m_dExtensionDistance`.
        distance: f64,
    },
    /// `TraceCurve` from point `i`.
    Trace(usize),
    /// `FlushCurve`.
    Flush,
    /// `FindInteriorCurves`.
    Fic,
    /// `DoPointIntersection`.
    Dpi(DpiOp),
    /// `DoLawIntersection`.
    Dli(DliOp),
}

/// The arguments of [`Op::Dpi`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DpiOp {
    /// `crTsectPoints`.
    pub points: Vec<usize>,
    /// `bExtendBeforeStart`.
    pub before: bool,
    /// `bExtendAfterEnd`.
    pub after: bool,
    /// `pOptStartDirection`.
    pub start_dir: Option<[f64; 3]>,
    /// `pOptEndDirection`.
    pub end_dir: Option<[f64; 3]>,
    /// `pOptApproxTol3d`.
    pub tol: Option<f64>,
    /// `pdOptAngTolRad`.
    pub ang_tol: Option<f64>,
}

/// The arguments of [`Op::Dli`].
#[derive(Debug, Clone, PartialEq)]
pub struct DliOp {
    /// `crTsectPoints`.
    pub points: Vec<usize>,
    /// `bLawOrient`.
    pub orient: bool,
    /// Index of the point curve among the program's curves.
    pub curve: usize,
    /// `crPointCurveInterval` (`SmExtent1d(a, b)`).
    pub interval: [f64; 2],
    /// `dSurfaceOrientations`.
    pub orientations: [f64; 2],
    /// `bUseNormalPlaneAveraging`.
    pub average: bool,
    /// `pOptApproxTol3d`.
    pub tol: Option<f64>,
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
    /// `bFromFilletIntersector`.
    pub fillet: bool,
    /// Curves.
    pub curves: Vec<CurveIn>,
    /// Points ([`POINT_LEN`] values each).
    pub points: Vec<Vec<f64>>,
    /// Ops.
    pub ops: Vec<Op>,
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
    ncurves: usize,
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
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        self.v.push(c[3]);
        Ok(())
    }
    fn pt(&mut self, i: usize) -> Result<(), FfiError> {
        if i >= self.npoints {
            return Err(bad("point index"));
        }
        self.int(i);
        Ok(())
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
                    self.cpt(c)?;
                }
                self.nums(u)?;
                self.nums(v)
            }
            SurfaceIn::CurveBounded(basis) => {
                if matches!(**basis, SurfaceIn::CurveBounded(_)) {
                    return Err(bad("nested curve-bounded surface"));
                }
                self.int(3);
                self.surface(basis)
            }
        }
    }

    fn curve(&mut self, c: &CurveIn) -> Result<(), FfiError> {
        match c {
            CurveIn::Segment(a, b) => {
                if a == b {
                    return Err(bad("zero-length segment"));
                }
                self.int(0);
                self.nums(a)?;
                self.nums(b)
            }
            CurveIn::BSpline { dim, p, cpts, knots } => {
                if !(2..=3).contains(dim) || !(1..=8).contains(p) {
                    return Err(bad("curve dim or degree"));
                }
                if !(2..=33).contains(&cpts.len()) || *p >= cpts.len() {
                    return Err(bad("curve size"));
                }
                check_knots(knots, cpts.len(), *p)?;
                check_weights(cpts.iter())?;
                self.int(1);
                self.int(*dim as usize);
                self.int(*p);
                self.int(cpts.len());
                for c in cpts {
                    self.cpt(c)?;
                }
                self.nums(knots)
            }
        }
    }

    fn point(&mut self, p: &[f64]) -> Result<(), FfiError> {
        if p.len() != POINT_LEN {
            return Err(bad("point length"));
        }
        let code = |x: f64, hi: f64| x >= 0.0 && x <= hi && x.fract() == 0.0;
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

    fn pts(&mut self, ids: &[usize]) -> Result<(), FfiError> {
        if ids.len() > MAX_OP_POINTS {
            return Err(bad("too many op points"));
        }
        self.int(ids.len());
        ids.iter().try_for_each(|&i| self.pt(i))
    }

    fn opt_num(&mut self, x: Option<f64>) -> Result<(), FfiError> {
        self.flag(x.is_some());
        self.num(x.unwrap_or(0.0))
    }

    fn opt_vec(&mut self, v: Option<[f64; 3]>) -> Result<(), FfiError> {
        self.flag(v.is_some());
        self.nums(&v.unwrap_or([0.0; 3]))
    }

    fn opt_pt(&mut self, i: Option<usize>) -> Result<(), FfiError> {
        self.flag(i.is_some());
        self.pt(i.unwrap_or(0))
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => self.int(1),
            Op::Point(i) => {
                self.int(2);
                self.pt(*i)?;
            }
            Op::AddCurve(i) | Op::AddStart(i) | Op::AddThrough(i) => {
                self.int(match op {
                    Op::AddCurve(_) => 3,
                    Op::AddStart(_) => 4,
                    _ => 5,
                });
                self.pt(*i)?;
            }
            Op::Cpv { uvs, i, prev, step } => {
                if *prev == Some(*i) {
                    return Err(bad("previous point is the point"));
                }
                self.int(6);
                self.nums(&uvs[0])?;
                self.nums(&uvs[1])?;
                self.pt(*i)?;
                self.opt_pt(*prev)?;
                self.flag(step.is_some());
                self.num(step.unwrap_or(0.0))?;
            }
            Op::Members {
                trace_dir,
                approx_tol,
                start_tol,
                step_tol,
                ang_tol,
            } => {
                self.int(7);
                self.nums(&[*trace_dir, *approx_tol, *start_tol, *step_tol, *ang_tol])?;
            }
            Op::Ends { start, end } => {
                self.int(8);
                self.opt_pt(*start)?;
                self.opt_pt(*end)?;
            }
            Op::Ext {
                clip,
                last_through,
                distance,
            } => {
                self.int(9);
                self.flag(*clip);
                self.num(*last_through)?;
                self.num(*distance)?;
            }
            Op::Trace(i) => {
                self.int(10);
                self.pt(*i)?;
            }
            Op::Flush => self.int(11),
            Op::Fic => self.int(14),
            Op::Dpi(d) => {
                self.int(12);
                self.pts(&d.points)?;
                self.flag(d.before);
                self.flag(d.after);
                self.opt_vec(d.start_dir)?;
                self.opt_vec(d.end_dir)?;
                self.opt_num(d.tol)?;
                self.opt_num(d.ang_tol)?;
            }
            Op::Dli(d) => {
                if d.curve >= self.ncurves {
                    return Err(bad("curve index"));
                }
                self.int(13);
                self.pts(&d.points)?;
                self.flag(d.orient);
                self.int(d.curve);
                self.nums(&d.interval)?;
                self.nums(&d.orientations)?;
                self.flag(d.average);
                self.opt_num(d.tol)?;
            }
        }
        Ok(())
    }
}

/// Encodes `prog` for the shim.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected program.
pub fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    if prog.ops.len() > MAX_OPS || prog.curves.len() > MAX_CURVES || prog.points.len() > MAX_POINTS
    {
        return Err(bad("too many ops, curves or points"));
    }
    let mut e = Enc {
        v: Vec::new(),
        npoints: prog.points.len(),
        ncurves: prog.curves.len(),
    };
    e.surface(&prog.a)?;
    match &prog.b {
        None => e.int(2),
        Some(b) => e.surface(b)?,
    }
    e.nums(&prog.dom1)?;
    e.nums(&prog.dom2)?;
    e.flag(prog.fillet);
    e.int(prog.curves.len());
    for c in &prog.curves {
        e.curve(c)?;
    }
    e.int(prog.points.len());
    for p in &prog.points {
        e.point(p)?;
    }
    e.int(prog.ops.len());
    for op in &prog.ops {
        e.op(op)?;
    }
    Ok(e.v)
}

/// Runs `prog` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(4096, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_intersect_surface_intersector_p4_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
