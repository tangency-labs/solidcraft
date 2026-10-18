//! FFI wrappers for unit `surface.surface_p8`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p8.cpp` (`sc_surface_surface_p8_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! Every input starts with a surface block: `0 oob form n m p q r s cpts(x y z w) u v` (an
//! `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`), `1 O[3] N[3]` (`SmPlane(O, N)`) or `3 A[3]
//! B[3] P[3] axis[3] swap` (`SmSurfOfRevolution::CreateCanonical` of the `SmLine(A, B)`
//! generator about the axis through `P`). A curve block is `0 dim n p m cpts(x y z w) knots`
//! (an `SmBSplineCurve(gw_CURVE*, dim, FALSE)`) or `1 A[3] B[3]` (`SmLine::CreateLineSegment`,
//! 3D). A domain is `u0 v0 u1 v1`, an interval `a b`. Every output starts with the status (`0`
//! = `SM_SUCCESS`, else the `SmStatus`; `-3` = a surface or curve the shim could not build);
//! the rest:
//!
//! | [`Op`]      | upstream                         | parameters after the surface | outputs |
//! |-------------|----------------------------------|------------------------------|---------|
//! | `Knots`     | `FindKnotsByMaxContinuity`       | `cont at`                    | `nu u[nu] nv v[nv]` |
//! | `Coinc`     | `CoincidenceCheck`               | `same surface2 tol whole`    | `coincident max_dist` |
//! | `Line`      | `GlobalLineIntersect`            | `dom P V has_ivl ivl ray tol`| `n` solutions |
//! | `Curve`     | `GlobalCurveIntersect`           | `dom curve ivl tol`          | `n` solutions |
//! | `LocalLine` | `LocalLineIntersect`             | `dom P V ivl dtol atol uv`   | `found [u v t dev pt[3] normal[3]]` |
//! | `Invert`    | `InvertPointNearSingularity`     | `P dom uv tol`               | `reason u v` |
//! | `Blend`     | `CreateBlendSurface`             | `curve1 ivl1 right surface2 curve2 ivl2 left tol` | `dom[4] points[5x5x3]` |
//! | `SpLocal`   | `SmSPGlobalSolver::LocalSolve`   | `dom P op tol best_sq`       | `more n (type nvars value u v)*` |
//!
//! A solution is `type nvars nobjs value end_value start[3] end[3] obj0 obj1` (objects `-1`
//! NULL, `0` the curve, `1` the surface; range ends are zero for point solutions). `same`
//! passes the first surface as its own `crOther`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway work:
//! well-formed clamped B-splines, planes with a non-zero normal, revolutions of a generator
//! of positive length not lying on the axis, lines of positive length, domains inside a
//! B-spline's natural domain (bounded elsewhere), intervals inside a B-spline curve's range,
//! and for `SpLocal` a B-spline surface. Inputs are finite.

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e6;
/// `SM_BIG_DOUBLE`.
const SM_BIG_DOUBLE: f64 = 1.0e20;

const NOW: f64 = -f64::MAX;
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Knots,
    Coinc,
    Line,
    Curve,
    LocalLine,
    Invert,
    Blend,
    SpLocal,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p8_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn positive(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad(format!("{what}: need a positive value")));
        }
        Ok(v)
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok((a, b))
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed for
/// curves when `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ && !planar {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// The parameter domain `[u0, v0, u1, v1]` of a B-spline surface block, `None` otherwise.
type Domain = Option<[f64; 4]>;

fn dist_to_axis(p: [f64; 3], o: [f64; 3], a: [f64; 3]) -> f64 {
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let la = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let c = [
        d[1] * a[2] - d[2] * a[1],
        d[2] * a[0] - d[0] * a[2],
        d[0] * a[1] - d[1] * a[0],
    ];
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() / la
}

/// A surface block: its domain and whether it is a plain B-spline.
fn surface_kind(rd: &mut Reader<'_>) -> Result<(Domain, bool), FfiError> {
    let kind = rd.index(3)?;
    if kind == 2 {
        return Err(bad(
            "curve-bounded surfaces are not part of this unit's harness",
        ));
    }
    if kind == 3 {
        let a = rd.vec3()?;
        let b = rd.vec3()?;
        let p = rd.vec3()?;
        let ax = rd.nonzero3()?;
        rd.flag()?;
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        if len < 1e-3 {
            return Err(bad("revolution generator too short"));
        }
        // one end on the axis is a true cone (its apex: a degenerate side, which upstream
        // handles); a generator along the axis sweeps no surface
        if dist_to_axis(a, p, ax) < 1e-3 && dist_to_axis(b, p, ax) < 1e-3 {
            return Err(bad("revolution generator lies on the axis"));
        }
        return Ok((None, false));
    }
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok((None, false));
    }
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    check_points(rd, (n + 1) * (m + 1), false, "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok((Some([u0, v0, u1, v1]), true))
}

/// A curve block; returns the parameter range of a B-spline (`None` for a line).
fn curve(rd: &mut Reader<'_>) -> Result<Option<(f64, f64)>, FfiError> {
    if rd.index(1)? == 1 {
        let a = rd.vec3()?;
        let b = rd.vec3()?;
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        if len < 1e-6 {
            return Err(bad("line too short"));
        }
        return Ok(None);
    }
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    Ok(Some(check_knots(&knots, p, "curve")?))
}

/// An interval inside a B-spline curve's range (bounded for a line).
fn interval_in(rd: &mut Reader<'_>, range: Option<(f64, f64)>) -> Result<(), FfiError> {
    let x = rd.finite()?;
    let y = rd.finite()?;
    if x >= y {
        return Err(bad("interval not ordered"));
    }
    match range {
        Some((a, b)) if x < a || y > b => Err(bad("interval outside the curve's range")),
        None if x.abs() > 1e4 || y.abs() > 1e4 => Err(bad("line interval too large")),
        _ => Ok(()),
    }
}

/// A `uv` point within one domain length of `dom` (bounded elsewhere).
fn uv(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    if let Some([u0, v0, u1, v1]) = dom {
        let (du, dv) = (u1 - u0, v1 - v0);
        if u < u0 - du || u > u1 + du || v < v0 - dv || v > v1 + dv {
            return Err(bad("uv too far outside the domain"));
        }
    } else if u.abs() > 1e4 || v.abs() > 1e4 {
        return Err(bad("uv too large"));
    }
    Ok(())
}

/// A non-degenerate domain, inside `dom` for a B-spline.
fn domain_in(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let d = rd.finites(4)?;
    let (Some(&a), Some(&b), Some(&c), Some(&e)) = (d.first(), d.get(1), d.get(2), d.get(3)) else {
        return Err(bad("domain too short"));
    };
    if a >= c || b >= e {
        return Err(bad("domain not ordered"));
    }
    if let Some([u0, v0, u1, v1]) = dom {
        if a < u0 || c > u1 || b < v0 || e > v1 {
            return Err(bad("domain outside the surface's"));
        }
    } else if a.abs() > 1e4 || b.abs() > 1e4 || c.abs() > 1e4 || e.abs() > 1e4 {
        return Err(bad("domain too large"));
    }
    Ok(())
}

/// `SM_SO_INTERSECT`, `SM_SO_MINIMIZE`, `SM_SO_MAXIMIZE`, `SM_SO_NORMALIZE`.
const SPLOCAL_OPS: [usize; 4] = [0, 3, 4, 5];

/// Checks `input` for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let (dom, is_bspline) = surface_kind(&mut rd)?;
    match op {
        Op::Knots => {
            rd.index(13)?;
            rd.flag()?;
        }
        Op::Coinc => {
            rd.flag()?;
            surface_kind(&mut rd)?;
            rd.positive("tolerance")?;
            rd.flag()?;
        }
        Op::Line => {
            domain_in(&mut rd, dom)?;
            rd.vec3()?;
            rd.vec3()?;
            if rd.flag()? {
                interval_in(&mut rd, None)?;
            } else {
                rd.finites(2)?;
            }
            rd.flag()?;
            rd.positive("tolerance")?;
        }
        Op::Curve => {
            domain_in(&mut rd, dom)?;
            let range = curve(&mut rd)?;
            interval_in(&mut rd, range)?;
            rd.positive("tolerance")?;
        }
        Op::LocalLine => {
            domain_in(&mut rd, dom)?;
            rd.vec3()?;
            rd.vec3()?;
            interval_in(&mut rd, None)?;
            rd.positive("distance tolerance")?;
            rd.positive("accuracy tolerance")?;
            uv(&mut rd, dom)?;
        }
        Op::Invert => {
            rd.vec3()?;
            domain_in(&mut rd, dom)?;
            uv(&mut rd, dom)?;
            rd.positive("tolerance")?;
        }
        Op::Blend => {
            // A line as a rail: `SmLine::GetNaturalInterval` returns the analytic domain until
            // `SyncronizeKnotsOfCurves` makes the NURB, then the NURB's, so upstream's rail
            // parameters (and the blend) depend on that switch; the port's line is a plain
            // B-spline rail. Recorded in plan/findings.md (surface.surface_p8, blend rails).
            if rd.input.get(rd.at) == Some(&1.0) {
                return Err(bad("blend rails must be B-spline curves"));
            }
            let r1 = curve(&mut rd)?;
            interval_in(&mut rd, r1)?;
            rd.flag()?;
            surface_kind(&mut rd)?;
            if rd.input.get(rd.at) == Some(&1.0) {
                return Err(bad("blend rails must be B-spline curves"));
            }
            let r2 = curve(&mut rd)?;
            interval_in(&mut rd, r2)?;
            rd.flag()?;
            let tol = rd.finite()?;
            if tol < 0.0 {
                return Err(bad("negative tolerance"));
            }
        }
        Op::SpLocal => {
            if !is_bspline {
                return Err(bad("this op needs a B-spline surface"));
            }
            domain_in(&mut rd, dom)?;
            rd.vec3()?;
            let sop = rd.index(15)?;
            if !SPLOCAL_OPS.contains(&sop) {
                return Err(bad(format!("solver operation {sop} not supported here")));
            }
            rd.positive("tolerance")?;
            // upstream commonly passes SM_BIG_DOUBLE (1e20), above the coordinate bound
            let best = rd.num()?;
            if !best.is_finite() || !(0.0..=SM_BIG_DOUBLE).contains(&best) {
                return Err(bad("best answer outside 0..=SM_BIG_DOUBLE"));
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input` (see the module docs) and returns the output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_p8_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
