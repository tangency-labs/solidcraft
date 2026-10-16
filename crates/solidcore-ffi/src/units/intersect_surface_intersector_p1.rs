//! FFI wrappers for unit `intersect.surface_intersector_p1`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_intersector_p1.cpp`
//! (`sc_intersect_surface_intersector_p1_run`). Upstream:
//! `source/SMLib/src/SmSurfaceIntersector.cpp`, `source/SMLib/inc/SmSurfaceIntersector.h`
//! (`SmTsectPnt`, `SmSurfaceIntersector`, `sm_ComputeCurveType`, `sm_IsPointOnCurve`,
//! `sm_FlushCrv`).
//!
//! [`run`] builds surfaces A and B (B may be A itself), the intersector
//! `SmSurfaceIntersector(A, dom1, B, dom2, fillet)`, the program's curves and its points
//! (a point arena; each op names points by index), then runs the [`Op`]s in order. The
//! record is `0` (the constructor has no status), then per op `op status [values]`
//! (values only for status 0), then the state.
//!
//! A point is 80 values: `type ptype[4] (uv pos, uv tangent) x 2, surface matrix [3][3]
//! x 2 (row major, 3 values each), curve pos, curve tangent, parameter, angle,
//! deviation, user[4]`. The state is `dom1[4] dom2[4] closed_u[2] closed_v[2]
//! use_edges[2] ang_tol approx_tol stepping_tol start_tol trace_dir last_through
//! ext_dist ext_clipping curve_closed check_interior self clipped boundary_test
//! start_point end_point` (point indices, `-1` = none), the curve, start and through
//! lists (`count ids`), then `npoints points`.
//!
//! | [`Op`]          | upstream                                         | values |
//! |-----------------|--------------------------------------------------|--------|
//! | `State`         | the protected members                            | state |
//! | `Point`         | the point                                        | point |
//! | `New`, `Copy`   | `new SmTsectPnt()`, `new SmTsectPnt(p)`          | new index |
//! | `Assign`        | `*dst = *src`                                    | |
//! | `IsBounded`     | `IsBounded`                                      | flag |
//! | `Classify`      | `ClassifyPointParams(uvs, srfs, doms[, tol])` (srfs: the intersector's surfaces or `NULL`) | `ptype[4]` |
//! | `SetExt`, `SetEdges` | `SetExtensionDistance`, `SetUseSurfaceEdges` | |
//! | `Delta`         | `ComputePointValuesDelta`                        | |
//! | `Singularity`   | `ComputeSingularityPoint`                        | |
//! | `StepSize`      | `ComputeStepSize`                                | new step |
//! | `Refine`        | `RefinePoint`                                    | `found [uv0 uv1]` |
//! | `TestSpan`      | `TestSpanAccuracy(a, b, NULL, ..)`               | `good deviation angle` (deviation [`UNSET`] when not set) |
//! | `AddCurve`, `AddStart`, `AddThrough` | `AddPointToCurve`, `AddStartPoint`, `AddThroughPoint` | |
//! | `Reverse`       | `ReverseCurveDirection`                          | |
//! | `OnStart`       | `IsPointOnStartPoint(p, list, skip)` (list: 0 start, 1 through, 2 curve) | flag |
//! | `OnTsectCurve`  | `sm_IsPointOnCurve(p, prev, next, tol)`          | flag |
//! | `OnCurve`       | `IsPointOnCurve(p, curves, skip)`                | flag |
//! | `CurveType`     | `sm_ComputeCurveType(A, B, curve i, curve j)`    | type |
//! | `Flush`         | `sm_FlushCrv(ctx, dim, poly, knots, out)`        | `count` then per curve `type dim knot_type form has [n p m cpts(x y z w) knots] ivl[2]` |
//! | `StepBack`      | `StepBackFromSingularity`                        | |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], well-formed
//! clamped B-splines, at most [`MAX_POINTS`] points, [`MAX_CURVES`] curves and
//! [`MAX_OPS`] ops. `Assign` to, or `Add*` of, a point that is already on a list
//! (undefined upstream: it corrupts the intrusive lists) is not run; its status is
//! [`REJECTED`].

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_intersector_p1_run(
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
/// Most points per program (including created ones).
pub const MAX_POINTS: usize = 64;
/// Most curves per program.
pub const MAX_CURVES: usize = 8;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// The deviation `TestSpan` reports when upstream leaves it unset.
pub const UNSET: f64 = -777.0;
/// The op status of an `Assign` / `Add*` the shim does not run (the point is linked).
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
    /// A default point.
    New,
    /// A copy of point `i`.
    Copy(usize),
    /// `point[dst] = point[src]`.
    Assign {
        /// Target (not on a list).
        dst: usize,
        /// Source.
        src: usize,
    },
    /// `IsBounded` of point `i`.
    IsBounded(usize),
    /// `ClassifyPointParams` on point `i`.
    Classify {
        /// The point.
        i: usize,
        /// UV points.
        uvs: [[f64; 2]; 2],
        /// Pass the intersector's surface (else `NULL`).
        surfaces: [bool; 2],
        /// Target domains.
        domains: [[f64; 4]; 2],
        /// The tolerance, `None` for the default.
        tol: Option<f64>,
    },
    /// `SetExtensionDistance`.
    SetExt(f64),
    /// `SetUseSurfaceEdges`.
    SetEdges(bool, bool),
    /// `ComputePointValuesDelta` with points `a`, `b`.
    Delta(usize, usize),
    /// `ComputeSingularityPoint`.
    Singularity {
        /// UV points.
        uvs: [[f64; 2]; 2],
        /// The point.
        i: usize,
        /// The previous point.
        prev: Option<usize>,
    },
    /// `ComputeStepSize(point, old)`.
    StepSize(usize, f64),
    /// `RefinePoint`.
    Refine {
        /// Guess on surface 0.
        uv0: [f64; 2],
        /// Guess on surface 1.
        uv1: [f64; 2],
        /// `SmIntersectionPointType` value.
        ip_type: u8,
        /// Plane normal.
        normal: Option<[f64; 3]>,
        /// Previous UV points.
        prev: Option<[[f64; 2]; 2]>,
        /// `bDoBoundaryTesting`.
        boundary: bool,
    },
    /// `TestSpanAccuracy(a, b)`.
    TestSpan(usize, usize),
    /// `AddPointToCurve`.
    AddCurve(usize),
    /// `AddStartPoint`.
    AddStart(usize),
    /// `AddThroughPoint`.
    AddThrough(usize),
    /// `ReverseCurveDirection`.
    Reverse,
    /// `IsPointOnStartPoint`.
    OnStart {
        /// The point.
        i: usize,
        /// 0 start, 1 through, 2 curve list.
        list: u8,
        /// Skip index.
        skip: Option<usize>,
    },
    /// `sm_IsPointOnCurve(p, prev, next, tol)`.
    OnTsectCurve(usize, usize, usize, f64),
    /// `IsPointOnCurve(p, curves, skip)`.
    OnCurve {
        /// The point.
        p: [f64; 3],
        /// Skip index.
        skip: Option<usize>,
    },
    /// `sm_ComputeCurveType(A, B, curve i, curve j)`.
    CurveType(usize, usize),
    /// `sm_FlushCrv`.
    Flush {
        /// Dimension, 1 to 3.
        dim: u32,
        /// Control polygon (`3 (knots - 1) + 1` points).
        poly: Vec<[f64; 3]>,
        /// Unique knots, increasing (none or at least 2).
        knots: Vec<f64>,
    },
    /// `StepBackFromSingularity`.
    StepBack {
        /// `SM_SP_V` (else `SM_SP_U`).
        v_dir: bool,
        /// Surface index.
        srf: u8,
        /// Step off the current point.
        current: bool,
        /// Previous point.
        prev: usize,
        /// Current point.
        curr: usize,
        /// Step size.
        step: f64,
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
            Op::New => {
                self.int(3);
                self.new_point()?;
            }
            Op::Copy(i) => {
                self.int(4);
                self.pt(*i)?;
                self.new_point()?;
            }
            Op::Assign { dst, src } => {
                self.int(5);
                self.pt(*dst)?;
                self.pt(*src)?;
            }
            Op::IsBounded(i) => {
                self.int(6);
                self.pt(*i)?;
            }
            Op::Classify {
                i,
                uvs,
                surfaces,
                domains,
                tol,
            } => {
                self.int(7);
                self.pt(*i)?;
                self.nums(&uvs[0])?;
                self.nums(&uvs[1])?;
                self.flag(surfaces[0]);
                self.flag(surfaces[1]);
                self.nums(&domains[0])?;
                self.nums(&domains[1])?;
                self.flag(tol.is_some());
                self.num(tol.unwrap_or(0.0))?;
            }
            Op::SetExt(d) => {
                self.int(8);
                self.num(*d)?;
            }
            Op::SetEdges(a, b) => {
                self.int(9);
                self.flag(*a);
                self.flag(*b);
            }
            Op::Delta(a, b) => {
                self.int(10);
                self.pt(*a)?;
                self.pt(*b)?;
            }
            Op::Singularity { uvs, i, prev } => {
                self.int(11);
                self.nums(&uvs[0])?;
                self.nums(&uvs[1])?;
                self.pt(*i)?;
                self.flag(prev.is_some());
                self.pt(prev.unwrap_or(*i))?;
            }
            Op::StepSize(i, old) => {
                self.int(12);
                self.pt(*i)?;
                self.num(*old)?;
            }
            Op::Refine {
                uv0,
                uv1,
                ip_type,
                normal,
                prev,
                boundary,
            } => {
                if *ip_type > 7 {
                    return Err(bad("point type"));
                }
                self.int(13);
                self.nums(uv0)?;
                self.nums(uv1)?;
                self.int(usize::from(*ip_type));
                self.flag(normal.is_some());
                self.nums(&normal.unwrap_or([0.0; 3]))?;
                self.flag(prev.is_some());
                let pv = prev.unwrap_or([[0.0; 2]; 2]);
                self.nums(&pv[0])?;
                self.nums(&pv[1])?;
                self.flag(*boundary);
            }
            Op::TestSpan(a, b) => {
                self.int(14);
                self.pt(*a)?;
                self.pt(*b)?;
            }
            Op::AddCurve(i) | Op::AddStart(i) | Op::AddThrough(i) => {
                self.int(match op {
                    Op::AddCurve(_) => 15,
                    Op::AddStart(_) => 16,
                    _ => 17,
                });
                self.pt(*i)?;
            }
            Op::Reverse => self.int(18),
            Op::OnStart { i, list, skip } => {
                if *list > 2 {
                    return Err(bad("list"));
                }
                self.int(19);
                self.pt(*i)?;
                self.int(usize::from(*list));
                self.flag(skip.is_some());
                let s = skip.unwrap_or(0);
                if s > 1000 {
                    return Err(bad("skip"));
                }
                self.int(s);
            }
            Op::OnTsectCurve(a, b, c, tol) => {
                self.int(20);
                self.pt(*a)?;
                self.pt(*b)?;
                self.pt(*c)?;
                self.num(*tol)?;
            }
            Op::OnCurve { p, skip } => {
                self.int(21);
                self.nums(p)?;
                self.flag(skip.is_some());
                let s = skip.unwrap_or(0);
                if s > 1000 {
                    return Err(bad("skip"));
                }
                self.int(s);
            }
            Op::CurveType(i, j) => {
                if *i >= self.ncurves || *j >= self.ncurves {
                    return Err(bad("curve index"));
                }
                self.int(22);
                self.int(*i);
                self.int(*j);
            }
            Op::Flush { dim, poly, knots } => {
                if !(1..=3).contains(dim) || knots.len() > 64 || poly.len() > 200 {
                    return Err(bad("flush sizes"));
                }
                let consistent = knots.is_empty()
                    || (knots.len() >= 2 && poly.len() == 3 * (knots.len() - 1) + 1);
                if !consistent || knots.windows(2).any(|w| w.first() >= w.last()) {
                    return Err(bad("flush polygon or knots"));
                }
                self.int(23);
                self.int(*dim as usize);
                self.int(knots.len());
                self.int(poly.len());
                for p in poly {
                    self.nums(p)?;
                }
                self.nums(knots)?;
            }
            Op::StepBack {
                v_dir,
                srf,
                current,
                prev,
                curr,
                step,
            } => {
                if *srf > 1 {
                    return Err(bad("surface index"));
                }
                self.int(24);
                self.flag(*v_dir);
                self.int(usize::from(*srf));
                self.flag(*current);
                self.pt(*prev)?;
                self.pt(*curr)?;
                self.num(*step)?;
            }
        }
        Ok(())
    }

    fn new_point(&mut self) -> Result<(), FfiError> {
        if self.npoints >= MAX_POINTS {
            return Err(bad("too many points"));
        }
        self.npoints += 1;
        Ok(())
    }
}

/// Appends the shim encoding of surface `s` to `v` (shared with the
/// `intersect.adv_surface_intersector` shim, which reads surfaces the same way).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected surface.
pub(crate) fn encode_surface(v: &mut Vec<f64>, s: &SurfaceIn) -> Result<(), FfiError> {
    let mut e = Enc {
        v: core::mem::take(v),
        npoints: 0,
        ncurves: 0,
    };
    let r = e.surface(s);
    *v = e.v;
    r
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
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_intersect_surface_intersector_p1_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
