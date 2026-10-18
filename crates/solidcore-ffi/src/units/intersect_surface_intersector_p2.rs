//! FFI wrappers for unit `intersect.surface_intersector_p2`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_intersector_p2.cpp`
//! (`sc_intersect_surface_intersector_p2_run`). Upstream:
//! `source/SMLib/src/SmSurfaceIntersector.cpp` (`ComputePointValues`,
//! `ClassifyBoundaryStartPointIntersection`, `FlushCurve`, `DoBoundaryPlaneTest`,
//! `ComputeLawPointValues`, `sm_IsPointUnique`).
//!
//! [`run`] builds surfaces A and B (planes, B-splines or curve-bounded surfaces over
//! either; B may be A itself), the intersector
//! `SmSurfaceIntersector(A, dom1, B, dom2, fillet)` (its context set as
//! `DoIntersection` does), the program's curves and its points (a point arena; each op
//! names points by index), then runs the [`Op`]s in order. The `FlushCurve` output
//! arrays persist over the program; the 3D curve array starts with the program's
//! curves. The record is, per op, `op status [values]` (values only for status 0,
//! except where noted), then the state `approx_tol trace_dir start_tol` `curve list`
//! `start list` (`count ids`) `npoints points`.
//!
//! A point is 80 values: `type ptype[4] (uv pos, uv tangent) x 2, surface matrix [3][3]
//! x 2 (row major, 3 values each), curve pos, curve tangent, parameter, angle,
//! deviation, user[4]`. Curves are written as `type dim knot_type form has [n p m
//! cpts(x y z w) knots] ivl[2]`; curve outputs are `count curves`. The outputs of
//! `FlushCurve` / `DoBoundaryPlaneTest` are `3d curves` (`FlushCurve`: those after the
//! program's), `uv curves 1`, `uv curves 2`, `ntypes types`, `ndevs devs`.
//!
//! | [`Op`]        | upstream                                                       | values |
//! |---------------|----------------------------------------------------------------|--------|
//! | `State`       | the protected members                                          | state |
//! | `Point`       | the point                                                      | point |
//! | `AddCurve`, `AddStart` | `AddPointToCurve`, `AddStartPoint`                    | |
//! | `Cpv`         | `ComputePointValues(uvs, p, prev, step)`                       | point (also on error) |
//! | `Classify`    | `ClassifyBoundaryStartPointIntersection(p, deg)`               | `deg` |
//! | `Flush`       | `FlushCurve(out arrays)`                                        | outputs |
//! | `Unique`      | `sm_IsPointUnique(p, m_vStartPoints, out 3d curves, tol)`      | flag |
//! | `Bpt`         | `DoBoundaryPlaneTest(planes(A, dom1), planes(B, dom2), tol, box, ..)` (fresh output arrays) | `result` outputs |
//! | `Law`         | `ComputeLawPointValues(..., SmFilletLaw(), ..., p)`             | point (also on error) |
//! | `Members`     | sets `m_dCurveTraceDirection`, `m_dThisApproxTol3d`, `m_dStartParallelTolRadians` | |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], well-formed
//! clamped B-splines, at most [`MAX_POINTS`] points, [`MAX_CURVES`] curves and
//! [`MAX_OPS`] ops, and no `Cpv` whose previous point is its point (the Rust API
//! cannot alias them). The shim does not run (status [`REJECTED`]): `Add*` of a point
//! already on a list (it corrupts the intrusive lists); `Classify` of a point with a
//! non-finite UV value or both UV tangents zero (the gap walk may never leave the
//! domains). `Flush` always runs: knots that decrease are rejected by
//! `N_IgesValidate` inside `SmBSplineCurve::CreateCanonical` (an `SM_ERR` status).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_intersector_p2_run(
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

/// The arguments of [`Op::Law`].
#[derive(Debug, Clone, PartialEq)]
pub struct LawOp {
    /// `dParameter`.
    pub param: f64,
    /// `crUV0`.
    pub uv0: [f64; 2],
    /// `crUV1`.
    pub uv1: [f64; 2],
    /// `dDeltaStep`.
    pub delta: f64,
    /// `caNormals`.
    pub normals: [[f64; 3]; 2],
    /// `bAverageNormals`.
    pub average: bool,
    /// `bLawOrient`.
    pub orient: bool,
    /// Index of the point curve among the program's curves.
    pub curve: usize,
    /// `crPointCurveInterval` (`SmExtent1d(a, b)`).
    pub interval: [f64; 2],
    /// `dSurfaceOrientations`.
    pub orientations: [f64; 2],
    /// `bSkipComputingUVS`.
    pub skip: bool,
    /// The point.
    pub i: usize,
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
    /// `ClassifyBoundaryStartPointIntersection` of point `i`.
    Classify(usize),
    /// `FlushCurve`.
    Flush,
    /// `sm_IsPointUnique(p, start points, out 3d curves, tol)`.
    Unique {
        /// The point.
        p: [f64; 3],
        /// The tolerance.
        tol: f64,
    },
    /// `DoBoundaryPlaneTest`.
    Bpt {
        /// `d3DTol`.
        tol: f64,
        /// The bounding box `min max`.
        bbox: [[f64; 3]; 2],
        /// Domain of A's bounding planes.
        dom1: [f64; 4],
        /// Domain of B's bounding planes.
        dom2: [f64; 4],
    },
    /// `ComputeLawPointValues`.
    Law(LawOp),
    /// Sets the trace direction, the approximation tolerance and the start parallel
    /// tolerance.
    Members {
        /// `m_dCurveTraceDirection`.
        trace_dir: f64,
        /// `m_dThisApproxTol3d`.
        approx_tol: f64,
        /// `m_dStartParallelTolRadians`.
        start_tol: f64,
    },
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
            CurveIn::BSpline {
                dim,
                p,
                cpts,
                knots,
            } => {
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

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => self.int(1),
            Op::Point(i) => {
                self.int(2);
                self.pt(*i)?;
            }
            Op::AddCurve(i) | Op::AddStart(i) => {
                self.int(if matches!(op, Op::AddCurve(_)) { 3 } else { 4 });
                self.pt(*i)?;
            }
            Op::Cpv { uvs, i, prev, step } => {
                if *prev == Some(*i) {
                    return Err(bad("previous point is the point"));
                }
                self.int(5);
                self.nums(&uvs[0])?;
                self.nums(&uvs[1])?;
                self.pt(*i)?;
                self.flag(prev.is_some());
                self.pt(prev.unwrap_or(*i))?;
                self.flag(step.is_some());
                self.num(step.unwrap_or(0.0))?;
            }
            Op::Classify(i) => {
                self.int(6);
                self.pt(*i)?;
            }
            Op::Flush => self.int(7),
            Op::Unique { p, tol } => {
                self.int(8);
                self.nums(p)?;
                self.num(*tol)?;
            }
            Op::Bpt {
                tol,
                bbox,
                dom1,
                dom2,
            } => {
                self.int(9);
                self.num(*tol)?;
                self.nums(&bbox[0])?;
                self.nums(&bbox[1])?;
                self.nums(dom1)?;
                self.nums(dom2)?;
            }
            Op::Law(l) => {
                if l.curve >= self.ncurves {
                    return Err(bad("curve index"));
                }
                self.int(10);
                self.num(l.param)?;
                self.nums(&l.uv0)?;
                self.nums(&l.uv1)?;
                self.num(l.delta)?;
                self.nums(&l.normals[0])?;
                self.nums(&l.normals[1])?;
                self.flag(l.average);
                self.flag(l.orient);
                self.int(l.curve);
                self.nums(&l.interval)?;
                self.nums(&l.orientations)?;
                self.flag(l.skip);
                self.pt(l.i)?;
            }
            Op::Members {
                trace_dir,
                approx_tol,
                start_tol,
            } => {
                self.int(11);
                self.num(*trace_dir)?;
                self.num(*approx_tol)?;
                self.num(*start_tol)?;
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
            sys::sc_intersect_surface_intersector_p2_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
