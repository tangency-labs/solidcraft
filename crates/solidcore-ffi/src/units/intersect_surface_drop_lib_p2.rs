//! FFI wrappers for unit `intersect.surface_drop_lib_p2`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_drop_lib_p2.cpp`
//! (`sc_intersect_surface_drop_lib_p2_run`). Upstream:
//! `source/SMLib/src/SmSurfaceDropLib.cpp` (`SmDropPt::SetProperties`,
//! `SmDropPt::GetNextPoint`, `sm_AddToCurve`, `sm_FlushCrv`).
//!
//! [`run`] runs one [`Op`] and returns its record. A drop point is [`PT_LEN`] values, as
//! in the `intersect.surface_drop_lib_p1` wrapper: `t good crv[3][3] uv[2][2] pd3d[2][3]
//! normal[3] from_left_u from_left_v drop_to_surf approx_dev normal_to_crv_deg
//! srf_to_crv_deg speed_ratio on_boundary on_seam on_pole degen leaving entering
//! deriv_rtn` (the `SmBoolean`s are `0`, `1` or `2` = `UNSURE`). A status is `0` for
//! `SM_SUCCESS`, else the `SmStatus`. The offset is `SmOffsetSurface(offset, surface,
//! FALSE, singularities)`. A growing curve state is `npoly poly(x y z) nknots knots
//! ncurves curves`, a B-spline curve `degree ncp cpts(x y z w) nknots knots mults
//! ivl_min ivl_max`.
//!
//! | [`Op`]   | upstream                                                       | record |
//! |----------|----------------------------------------------------------------|--------|
//! | `Set`    | `pt.SetProperties(surface, offset, sing, tol, dom, adjust)`    | `status pt offset_distance` |
//! | `Next`   | `start.GetNextPoint(surface, offset, sing, poles, dom, closure, curve, tol, tx, ty, end_t, end)` | `status end offset_distance` |
//! | `Add`    | `sm_AddToCurve` per span (from empty arrays, stopping at a failure), then `sm_FlushCrv` when asked | per call `status change`, then the state |
//! | `Flush`  | `sm_FlushCrv(poly, knots)`                                     | `status change` state |
//!
//! A surface of revolution whose `CreateCanonical` fails makes the record
//! [[`NO_SURFACE`]]. [`run`] accepts only finite inputs of magnitude at most
//! [`MAX_COORD`] (drop point members included), well-formed clamped B-splines, circles
//! with a positive radius, an increasing interval and independent axes, segments of
//! positive length, singularity flags within `0..=31`, at most [`MAX_ITEMS`] points,
//! spans or knots, and for `Flush` no knots or `n >= 2` increasing knots with `3n - 2`
//! polygon points (the shapes `sm_AddToCurve` builds; others read past the arrays). The
//! shim does not run (record [[`REJECTED`]]) a `Next` whose start evaluation (status
//! ignored upstream, which goes on with whatever the evaluator left) fails.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_drop_lib_p2_run(
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
/// Most drop points, spans, polygon points or knots of an op.
pub const MAX_ITEMS: usize = 20_000;
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

/// An op (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `SetProperties`.
    Set {
        /// The surface.
        surface: SurfaceIn,
        /// The drop point.
        pt: Vec<f64>,
        /// The offset's initial distance.
        offset: f64,
        /// `lSingularities` (`0..=31`).
        singularities: u32,
        /// `sApproxTol3d`.
        tol: f64,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `bAdjustPolePt`.
        adjust: bool,
    },
    /// `GetNextPoint`.
    Next {
        /// The surface.
        surface: SurfaceIn,
        /// The curve.
        curve: CurveIn,
        /// The span start (this).
        start: Vec<f64>,
        /// The offset's initial distance.
        offset: f64,
        /// `lSingularities` (`0..=31`).
        singularities: u32,
        /// `sSrfPolePoints` (`NULL` when `None`).
        poles: Option<Box<[[f64; 3]; 4]>>,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `eSrfClosure`: 0 `SM_SP_U`, 1 `SM_SP_V`, 2 `SM_SP_BOTH`, 3 `SM_SP_NEITHER`.
        closure: u8,
        /// `sApproxTol3d`.
        tol: f64,
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// `dEndT`.
        end_t: f64,
    },
    /// `sm_AddToCurve` per span, then `sm_FlushCrv`.
    Add {
        /// The surface.
        surface: SurfaceIn,
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `sApproxTol3d`.
        tol: f64,
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// The drop points.
        pts: Vec<Vec<f64>>,
        /// The spans (start, end indices into `pts`).
        spans: Vec<[usize; 2]>,
        /// Flush at the end.
        flush: bool,
    },
    /// `sm_FlushCrv`.
    Flush {
        /// `crUVDomain`.
        domain: [f64; 4],
        /// `dUVTolX`, `dUVTolY`.
        uv_tol: [f64; 2],
        /// The control polygon.
        poly: Vec<[f64; 3]>,
        /// The breakpoints.
        knots: Vec<f64>,
    },
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

    fn sing(&mut self, s: u32) -> Result<(), FfiError> {
        if s > 31 {
            return Err(bad("singularities"));
        }
        self.v.push(f64::from(s));
        Ok(())
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::Set {
                surface,
                pt,
                offset,
                singularities,
                tol,
                domain,
                adjust,
            } => {
                self.int(1);
                self.surface(surface)?;
                self.pt(pt)?;
                self.num(*offset)?;
                self.sing(*singularities)?;
                self.num(*tol)?;
                self.nums(domain)?;
                self.flag(*adjust);
                Ok(())
            }
            Op::Next {
                surface,
                curve,
                start,
                offset,
                singularities,
                poles,
                domain,
                closure,
                tol,
                uv_tol,
                end_t,
            } => {
                if *closure > 3 {
                    return Err(bad("closure"));
                }
                self.int(2);
                self.surface(surface)?;
                self.curve(curve)?;
                self.pt(start)?;
                self.num(*offset)?;
                self.sing(*singularities)?;
                match poles {
                    Some(ps) => {
                        self.int(1);
                        for p in ps.iter() {
                            self.nums(p)?;
                        }
                    }
                    None => self.int(0),
                }
                self.nums(domain)?;
                self.int(usize::from(*closure));
                self.num(*tol)?;
                self.nums(uv_tol)?;
                self.num(*end_t)
            }
            Op::Add {
                surface,
                domain,
                tol,
                uv_tol,
                pts,
                spans,
                flush,
            } => {
                if pts.len() > MAX_ITEMS || spans.len() > MAX_ITEMS {
                    return Err(bad("too many points or spans"));
                }
                if spans.iter().flatten().any(|&i| i >= pts.len()) {
                    return Err(bad("span index"));
                }
                self.int(3);
                self.surface(surface)?;
                self.nums(domain)?;
                self.num(*tol)?;
                self.nums(uv_tol)?;
                self.int(pts.len());
                for p in pts {
                    self.pt(p)?;
                }
                self.int(spans.len());
                for s in spans {
                    self.int(s[0]);
                    self.int(s[1]);
                }
                self.flag(*flush);
                Ok(())
            }
            Op::Flush {
                domain,
                uv_tol,
                poly,
                knots,
            } => {
                if poly.len() > MAX_ITEMS || knots.len() > MAX_ITEMS {
                    return Err(bad("too many polygon points or knots"));
                }
                let nk = knots.len();
                if nk != 0 && (nk < 2 || poly.len() != 3 * nk - 2) {
                    return Err(bad("polygon and knot counts"));
                }
                if knots
                    .windows(2)
                    .any(|w| w.last().partial_cmp(&w.first()) != Some(core::cmp::Ordering::Greater))
                {
                    return Err(bad("knots not increasing"));
                }
                self.int(4);
                self.nums(domain)?;
                self.nums(uv_tol)?;
                self.int(poly.len());
                for p in poly {
                    self.nums(p)?;
                }
                self.int(nk);
                self.nums(knots)
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
    call_with_buffer(4096, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_intersect_surface_drop_lib_p2_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
