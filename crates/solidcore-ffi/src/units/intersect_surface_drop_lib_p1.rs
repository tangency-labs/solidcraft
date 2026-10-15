//! FFI wrappers for unit `intersect.surface_drop_lib_p1`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_drop_lib_p1.cpp`
//! (`sc_intersect_surface_drop_lib_p1_run`). Upstream:
//! `source/SMLib/src/SmSurfaceDropLib.cpp` (`SmDropPt`, `sm_PushPoint`, `sm_PopStack`,
//! `sm_StackLook`, `sm_FixBoundaryCurves`, `sm_RemoveDuplicateDropPointSolutions`,
//! `SmSurface::DropIsoCurve`, `SmSurface::DropAndTrimCurve`).
//!
//! [`run`] runs one [`Op`] and returns its record. A drop point is [`PT_LEN`] values: `t
//! good crv[3][3] uv[2][2] pd3d[2][3] normal[3] from_left_u from_left_v drop_to_surf
//! approx_dev normal_to_crv_deg srf_to_crv_deg speed_ratio on_boundary on_seam on_pole
//! degen leaving entering deriv_rtn` (the `SmBoolean`s are `0`, `1` or `2` = `UNSURE`). A
//! status is `0` for `SM_SUCCESS`, else the `SmStatus`. A B-spline result curve is `degree
//! ncp cpts(x y z w) nknots knots mults ivl_min ivl_max`.
//!
//! | [`Op`]         | upstream                                                  | record |
//! |----------------|-----------------------------------------------------------|--------|
//! | `Ctor`         | `SmDropPt(t, degen)`                                      | point |
//! | `Assign`       | `dst = src; dst = dst`                                    | point |
//! | `Uninit`       | `SetUninitialized`                                        | point |
//! | `GoodTangent`  | `IsGoodTangentDrop(on_pole)`                              | flag |
//! | `Moving`       | `IsMovingAlongBoundary`                                   | `SmPointObjectContainmentType` |
//! | `Span`         | `start.TestSpanAccuracy(.., end, ..)`                     | `status [pass max_drop max_drop_param max_dev]` |
//! | `Seam`         | `start.CheckSeamJump(surface, closure, tol, end)`         | `status end` |
//! | `FixTangents`  | `start.FixBoundaryTangents(dom, end, tx, ty, tol, change)` | `status change start end` |
//! | `Stack`        | `sm_PushPoint` / `sm_PopStack` / `sm_StackLook` on one stack | per pop/look `status [t u v du dv]`, then `size stack` |
//! | `FixCurves`    | `sm_FixBoundaryCurves(poly, dom, tx, ty, change = FALSE)` | `change poly` |
//! | `Dups`         | `sm_RemoveDuplicateDropPointSolutions` (`m_vStart = (u, v)`, value = index) | `count (index u v)*` |
//! | `Iso`          | `DropIsoCurve`                                            | `status [ok max_drop max_dev has [curve]]` |
//! | `Trim`         | `DropAndTrimCurve(.., keep, on_surface, NULL)`            | `status [max_drop max_dev n curves]` |
//!
//! A surface of revolution whose `CreateCanonical` fails makes the record
//! [[`NO_SURFACE`]]. [`run`] accepts only finite inputs of magnitude at most
//! [`MAX_COORD`] (drop point members included), well-formed clamped B-splines, circles
//! with a positive radius, an increasing interval and independent axes, segments of
//! positive length, and at most [`MAX_ITEMS`] stack ops, polygon points or solutions. The
//! shim does not run (record [[`REJECTED`]]): `Moving` when an evaluation it makes fails
//! (upstream ignores the status and goes on with the vectors' `SM_UNDEF_DOUBLE` defaults
//! or partial writes, a path the port does not model), `Span` when a Hermite
//! sample outside the domain or its clamp does not evaluate (same), `FixCurves` of `3k`
//! points, `k >= 2` (upstream reads past the array).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_drop_lib_p1_run(
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
/// Most stack ops, polygon points or solutions.
pub const MAX_ITEMS: usize = 256;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// The record of an op the shim does not run.
pub const REJECTED: f64 = -2.0;
/// The record of an op whose surface of revolution could not be made.
pub const NO_SURFACE: f64 = -3.0;
/// Values per drop point.
pub const PT_LEN: usize = 38;

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
    /// `SmSurfOfRevolution::CreateCanonical(ctx, gen, point, axis, rev, swap)`.
    Revolution {
        /// The generator (a line or a circle).
        gen: CurveIn,
        /// A point on the axis.
        point: [f64; 3],
        /// The axis direction.
        axis: [f64; 3],
        /// `bSwapUV`.
        swap: bool,
    },
}

/// A 3D curve.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine(a, b, 3, ctx)` with its NURB made.
    Line {
        /// Start.
        a: [f64; 3],
        /// End.
        b: [f64; 3],
    },
    /// `SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, ctx)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Parameter interval (radians).
        d: [f64; 2],
        /// Radius.
        r: f64,
    },
    /// `SmBSplineCurve(gw_CURVE*, 3, FALSE)`.
    BSpline {
        /// Degree.
        p: usize,
        /// Control points (`x y z w`, `w` = [`NOW`] when non-rational).
        cpts: Vec<[f64; 4]>,
        /// Knots (`n + p + 2`).
        knots: Vec<f64>,
    },
}

/// A span stack op.
#[derive(Debug, Clone, PartialEq)]
pub enum StackOp {
    /// `sm_PushPoint(t, [uv, duv], stack)`.
    Push {
        /// Curve parameter.
        t: f64,
        /// UV point.
        uv: [f64; 2],
        /// UV tangent.
        duv: [f64; 2],
    },
    /// `sm_PopStack`.
    Pop,
    /// `sm_StackLook`.
    Look,
}

/// An op (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `SmDropPt(t, degen)`.
    Ctor {
        /// `dT`.
        t: f64,
        /// `bDegenCurve` (0, 1, 2).
        degen: u8,
    },
    /// `operator=`.
    Assign(Vec<f64>),
    /// `SetUninitialized`.
    Uninit(Vec<f64>),
    /// `IsGoodTangentDrop`.
    GoodTangent {
        /// The drop point.
        pt: Vec<f64>,
        /// `bOnPole` (0, 1, 2).
        on_pole: u8,
    },
    /// `IsMovingAlongBoundary`.
    Moving {
        /// The surface.
        surface: SurfaceIn,
        /// The drop point.
        pt: Vec<f64>,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `dUVTolX`, `dUVTolY`.
        tol: [f64; 2],
        /// `dAngLimitDeg`.
        ang: f64,
        /// `eTestBdry`: 0 `SM_SP_U`, 1 `SM_SP_V`, 2 `SM_SP_UNKNOWN`, 3 `SM_SP_BOTH`, 4
        /// `SM_SP_NEITHER`.
        test: u8,
    },
    /// `TestSpanAccuracy`.
    Span {
        /// The surface.
        surface: SurfaceIn,
        /// The curve.
        curve: CurveIn,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// The span start (this).
        start: Vec<f64>,
        /// The span end.
        end: Vec<f64>,
        /// `sApproxTol3d`.
        tol: f64,
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// `bDoRefinement`.
        refine: bool,
    },
    /// `CheckSeamJump`.
    Seam {
        /// The surface.
        surface: SurfaceIn,
        /// `eSrfClosure`: 0 `SM_SP_U`, 1 `SM_SP_V`, 2 `SM_SP_BOTH`, 3 `SM_SP_NEITHER`.
        closure: u8,
        /// `sApproxTol3d`.
        tol: f64,
        /// The span start (this).
        start: Vec<f64>,
        /// The span end.
        end: Vec<f64>,
    },
    /// `FixBoundaryTangents`.
    FixTangents {
        /// `crUVDomain`.
        domain: [f64; 4],
        /// The span start (this).
        start: Vec<f64>,
        /// The span end.
        end: Vec<f64>,
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// `sApproxTol3d`.
        tol: f64,
    },
    /// Span stack ops on one stack.
    Stack(Vec<StackOp>),
    /// `sm_FixBoundaryCurves`.
    FixCurves {
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// The control polygon.
        poly: Vec<[f64; 3]>,
    },
    /// `sm_RemoveDuplicateDropPointSolutions`.
    Dups {
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// The start UVs of the solutions.
        uvs: Vec<[f64; 2]>,
    },
    /// `DropIsoCurve`.
    Iso {
        /// The surface.
        surface: SurfaceIn,
        /// The curve.
        curve: CurveIn,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `crInterval` (`SmExtent1d(a, b)`).
        interval: [f64; 2],
        /// `crStartUVPoint`.
        start_uv: [f64; 2],
        /// `sApproxTol3d`.
        tol: f64,
    },
    /// `DropAndTrimCurve`.
    Trim {
        /// The surface.
        surface: SurfaceIn,
        /// The curve.
        curve: CurveIn,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `crInterval` (`SmExtent1d(a, b)`).
        interval: [f64; 2],
        /// `sApproxTol3d`.
        tol: f64,
        /// `bKeepAllDropCurves`.
        keep_all: bool,
        /// `bOptCurveIsOnSurface`.
        on_surface: bool,
    },
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

/// All [`NOW`] or all in `[1e-6, MAX_COORD]`.
fn check_weights<'a>(mut ws: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), FfiError> {
    let Some(first) = ws.next() else {
        return Err(bad("no control points"));
    };
    let rational = first[3] != NOW;
    let ok = |w: f64| {
        if rational {
            (1e-6..=MAX_COORD).contains(&w)
        } else {
            w == NOW
        }
    };
    if !ok(first[3]) || ws.any(|c| !ok(c[3])) {
        return Err(bad("weights"));
    }
    Ok(())
}

fn cross_len(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let c = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

fn len(a: &[f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

struct Enc {
    v: Vec<f64>,
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
    fn tri(&mut self, x: u8) -> Result<(), FfiError> {
        if x > 2 {
            return Err(bad("SmBoolean not 0, 1 or 2"));
        }
        self.int(usize::from(x));
        Ok(())
    }
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        self.v.push(c[3]);
        Ok(())
    }

    fn pt(&mut self, p: &[f64]) -> Result<(), FfiError> {
        if p.len() != PT_LEN {
            return Err(bad("drop point length"));
        }
        let tri = |x: f64| x == 0.0 || x == 1.0 || x == 2.0;
        // SmBoolean members: good, from_left_u/v, on_boundary .. entering
        let flags = [1usize, 24, 25, 31, 32, 33, 34, 35, 36];
        if flags.iter().any(|&i| !p.get(i).copied().is_some_and(tri)) {
            return Err(bad("drop point flags"));
        }
        let rtn = p.get(37).copied().unwrap_or(-1.0);
        if !((0.0..=1.0e8).contains(&rtn) && rtn.fract() == 0.0) {
            return Err(bad("drop point status"));
        }
        self.nums(p)
    }

    fn curve(&mut self, c: &CurveIn) -> Result<(), FfiError> {
        match c {
            CurveIn::Line { a, b } => {
                if a == b {
                    return Err(bad("zero-length segment"));
                }
                self.int(0);
                self.nums(a)?;
                self.nums(b)
            }
            CurveIn::Circle { c, x, y, d, r } => {
                // (NaN is rejected by `num` below)
                if *r <= 1e-9 || d[0] >= d[1] {
                    return Err(bad("circle radius or interval"));
                }
                if len(x) < 1e-9 || len(y) < 1e-9 || cross_len(x, y) < 1e-6 * len(x) * len(y) {
                    return Err(bad("circle axes"));
                }
                self.int(1);
                self.nums(c)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(d)?;
                self.num(*r)
            }
            CurveIn::BSpline { p, cpts, knots } => {
                if !(1..=8).contains(p) {
                    return Err(bad("curve degree"));
                }
                if !(2..=64).contains(&cpts.len()) || *p >= cpts.len() {
                    return Err(bad("curve size"));
                }
                check_knots(knots, cpts.len(), *p)?;
                check_weights(cpts.iter())?;
                self.int(2);
                self.int(*p);
                self.int(cpts.len());
                for c in cpts {
                    self.cpt(c)?;
                }
                self.nums(knots)
            }
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
                if len(x) < 1e-9 || len(y) < 1e-9 || cross_len(x, y) < 1e-6 * len(x) * len(y) {
                    return Err(bad("plane axes"));
                }
                if scale.iter().any(|s| s.abs() < 1e-9) {
                    return Err(bad("plane scale"));
                }
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
                if !(2..=33).contains(&nu) || !(2..=33).contains(&nv) || *p >= nu || *q >= nv {
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
            SurfaceIn::Revolution {
                gen,
                point,
                axis,
                swap,
            } => {
                if len(axis) < 1e-9 {
                    return Err(bad("revolution axis"));
                }
                self.int(2);
                match gen {
                    CurveIn::Line { .. } | CurveIn::Circle { .. } => {
                        // the shim's generator block has no B-spline form
                        let mut g = Enc { v: Vec::new() };
                        g.curve(gen)?;
                        self.v.extend(g.v);
                    }
                    CurveIn::BSpline { .. } => return Err(bad("revolution generator")),
                }
                self.nums(point)?;
                self.nums(axis)?;
                self.flag(*swap);
                Ok(())
            }
        }
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::Ctor { t, degen } => {
                self.int(1);
                self.num(*t)?;
                self.tri(*degen)
            }
            Op::Assign(p) => {
                self.int(2);
                self.pt(p)
            }
            Op::Uninit(p) => {
                self.int(3);
                self.pt(p)
            }
            Op::GoodTangent { pt, on_pole } => {
                self.int(4);
                self.pt(pt)?;
                self.tri(*on_pole)
            }
            Op::Moving {
                surface,
                pt,
                domain,
                tol,
                ang,
                test,
            } => {
                if *test > 4 {
                    return Err(bad("test boundary"));
                }
                self.int(5);
                self.surface(surface)?;
                self.pt(pt)?;
                self.nums(domain)?;
                self.nums(tol)?;
                self.num(*ang)?;
                self.int(usize::from(*test));
                Ok(())
            }
            Op::Span {
                surface,
                curve,
                domain,
                start,
                end,
                tol,
                uv_tol,
                refine,
            } => {
                self.int(6);
                self.surface(surface)?;
                self.curve(curve)?;
                self.nums(domain)?;
                self.pt(start)?;
                self.pt(end)?;
                self.num(*tol)?;
                self.nums(uv_tol)?;
                self.flag(*refine);
                Ok(())
            }
            Op::Seam {
                surface,
                closure,
                tol,
                start,
                end,
            } => {
                if *closure > 3 {
                    return Err(bad("closure"));
                }
                self.int(7);
                self.surface(surface)?;
                self.int(usize::from(*closure));
                self.num(*tol)?;
                self.pt(start)?;
                self.pt(end)
            }
            Op::FixTangents {
                domain,
                start,
                end,
                uv_tol,
                tol,
            } => {
                self.int(8);
                self.nums(domain)?;
                self.pt(start)?;
                self.pt(end)?;
                self.nums(uv_tol)?;
                self.num(*tol)
            }
            Op::Stack(ops) => {
                if ops.len() > MAX_ITEMS {
                    return Err(bad("too many stack ops"));
                }
                self.int(9);
                self.int(ops.len());
                for s in ops {
                    match s {
                        StackOp::Push { t, uv, duv } => {
                            self.int(0);
                            self.num(*t)?;
                            self.nums(uv)?;
                            self.nums(duv)?;
                        }
                        StackOp::Pop => self.int(1),
                        StackOp::Look => self.int(2),
                    }
                }
                Ok(())
            }
            Op::FixCurves {
                domain,
                uv_tol,
                poly,
            } => {
                if poly.len() > MAX_ITEMS {
                    return Err(bad("too many polygon points"));
                }
                self.int(10);
                self.nums(domain)?;
                self.nums(uv_tol)?;
                self.int(poly.len());
                for p in poly {
                    self.nums(p)?;
                }
                Ok(())
            }
            Op::Dups { uv_tol, uvs } => {
                if uvs.len() > MAX_ITEMS {
                    return Err(bad("too many solutions"));
                }
                self.int(11);
                self.nums(uv_tol)?;
                self.int(uvs.len());
                for p in uvs {
                    self.nums(p)?;
                }
                Ok(())
            }
            Op::Iso {
                surface,
                curve,
                domain,
                interval,
                start_uv,
                tol,
            } => {
                self.int(12);
                self.surface(surface)?;
                self.curve(curve)?;
                self.nums(domain)?;
                self.nums(interval)?;
                self.nums(start_uv)?;
                self.num(*tol)
            }
            Op::Trim {
                surface,
                curve,
                domain,
                interval,
                tol,
                keep_all,
                on_surface,
            } => {
                self.int(13);
                self.surface(surface)?;
                self.curve(curve)?;
                self.nums(domain)?;
                self.nums(interval)?;
                self.num(*tol)?;
                self.flag(*keep_all);
                self.flag(*on_surface);
                Ok(())
            }
        }
    }
}

/// Encodes `op` as the shim's input.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected op.
pub fn encode(op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc { v: Vec::new() };
    e.op(op)?;
    Ok(e.v)
}

/// Runs `op` (see the module docs) and returns its record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(op: &Op) -> Result<Vec<f64>, FfiError> {
    let input = encode(op)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_intersect_surface_drop_lib_p1_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
