//! FFI wrappers for unit `sweep.rotational_sweep_geometry`. Shim:
//! `harness/cpp/shim/sweep_rotational_sweep_geometry.cpp`. Upstream:
//! `source/SMLib/src/SmRotationalSweepGeometry.cpp` (`SmRotationalSweepGeometry`).
//!
//! Every input starts with the sweep block
//! `base[3] axis[3] angle eps make_analytics brep_tol`, then per [`Op`]:
//!
//! | op | rest of the input | output (after the status `0` / `SmStatus`) |
//! |----|-------------------|--------------------------------------------|
//! | `IsClosed`   | `repeats`                        | `closed` |
//! | `StartCs`, `EndCs` | `pt[3]`                    | on success `origin[3] x[3] y[3]` |
//! | `Transform`  |                                  | `origin[3] x[3] y[3]` |
//! | `Copy`       |                                  | `base[3] axis[3] angle eps make_analytics` |
//! | `CheckCurve` | `curve t0 t1 tol pt[3] vec[3] amount` | on success `state` |
//! | `VertexHigher`, `VertexSame` | `pt[3] vtol`     | (vertex tolerance first) see the shim |
//! | `EdgeHigher`, `EdgeSame` | `edge`               | (fixture first) see the shim |
//! | `FaceSame`   | `surface ftol`                   | (face tolerance first) see the shim |
//!
//! Curve blocks: `0 n p m cpts[4(n+1)] knots[m+1]` (3D `SmBSplineCurve`),
//! `1 a[3] b[3]` (`SmBSplineCurve::CreateLineSegment`), `2 a[3] b[3] has_ivl t0 t1`
//! (`SmLine::CreateLineSegment`), `3 c[3] x[3] y[3] d0 d1 r io` (`SmCircle`, degrees),
//! `4 c[3] x[3] y[3] d0 d1 rx ry io` (`SmEllipse`), `5 p1[3] d1[3] p2[3] d2[3]`
//! (`SmHermiteCurve`). Edge block: `curve t0 t1 opposite closed off0[3] off1[3] etol
//! vtol0 vtol1`. Surface block: `0 n m p q r s cpts u v` (`SmBSplineSurface`) or
//! `1 o[3] x[3] y[3] su sv u0 v0 u1 v1` (finite `SmPlane`).
//!
//! Validation rejects what upstream would read out of range or dereference unset:
//! malformed B-splines (sizes, unclamped or decreasing knots, bad weights), zero or
//! parallel axes of the analytic curves and planes, zero-length line segments,
//! edge intervals outside the curve's domain, non-finite or huge values.
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 256;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Largest sweep angle magnitude (degrees).
pub const MAX_ANGLE: f64 = 1080.0;
/// Largest tolerance.
pub const MAX_TOL: f64 = 1e6;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `IsSweepClosed`.
    IsClosed,
    /// `GetStartCoordSystem`.
    StartCs,
    /// `GetEndCoordSystem`.
    EndCs,
    /// `GetSweepTransform`.
    Transform,
    /// `CheckCurveSweep`.
    CheckCurve,
    /// `VertexSweepHigher`.
    VertexHigher,
    /// `VertexSweepSame`.
    VertexSame,
    /// `EdgeSweepHigher`.
    EdgeHigher,
    /// `EdgeSweepSame`.
    EdgeSame,
    /// `FaceSweepSame`.
    FaceSame,
    /// `Copy`.
    Copy,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::IsClosed => 0,
            Op::StartCs => 1,
            Op::EndCs => 2,
            Op::Transform => 3,
            Op::CheckCurve => 4,
            Op::VertexHigher => 5,
            Op::VertexSame => 6,
            Op::EdgeHigher => 7,
            Op::EdgeSame => 8,
            Op::FaceSame => 9,
            Op::Copy => 10,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_sweep_rotational_sweep_geometry_eval(
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
    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-24 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    /// A tolerance in `[0, MAX_TOL]`.
    fn tol(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !(0.0..=MAX_TOL).contains(&v) {
            return Err(bad(format!("tolerance {v} outside [0, {MAX_TOL}]")));
        }
        Ok(v)
    }
}

/// Clamped, non-decreasing knots of degree `p` with interior multiplicity at most
/// `p`; returns the parameter range.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let m = knots.len() - 1;
    let short = || bad(format!("{what}: knots too short"));
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
        return Err(bad(format!("{what}: end multiplicity above p + 1")));
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

/// A 3D B-spline `n p m cpts knots`; returns its parameter range.
fn bspline(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, _, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in cpts {
        if z == NOZ || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: not 3D or mixed rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && !(1e-3..=1e3).contains(&w) {
            return Err(bad("curve: weight outside [1e-3, 1e3]"));
        }
        let d = if w == NOW { 1.0 } else { w };
        if [x / d, y / d, z / d].iter().any(|c| c.abs() > MAX_COORD) {
            return Err(bad("curve: point out of range"));
        }
    }
    check_knots(knots, p, "curve")
}

/// A segment `a[3] b[3]` with distinct ends.
fn segment(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let a = rd.vec3()?;
    let b = rd.vec3()?;
    let d2: f64 = a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum();
    if d2 < 1e-18 {
        return Err(bad("segment: coincident ends"));
    }
    Ok(())
}

/// An ellipse or circle placement and domain `C X Y d0 d1`.
fn conic_frame(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    let x = rd.nonzero3()?;
    let y = rd.nonzero3()?;
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    let lx: f64 = x.iter().map(|c| c * c).sum();
    let ly: f64 = y.iter().map(|c| c * c).sum();
    if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 * lx * ly {
        return Err(bad("ellipse: parallel axes"));
    }
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if d0.abs() > 360.0 || d1.abs() > 360.0 {
        return Err(bad("ellipse: domain end beyond 360 degrees"));
    }
    if !(1e-4..=360.0).contains(&(d1 - d0)) {
        return Err(bad("ellipse: domain length outside [1e-4, 360]"));
    }
    Ok(())
}

fn radius(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let r = rd.finite()?;
    if !(1e-6..=1e7).contains(&r) {
        return Err(bad("ellipse: radius outside [1e-6, 1e7]"));
    }
    Ok(())
}

/// A curve block (see the module docs); returns the range edge intervals must lie in
/// (the curve's natural interval).
fn curve_block(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    match rd.index(5)? {
        0 => bspline(rd),
        1 => {
            segment(rd)?;
            Ok((0.0, 1.0))
        }
        2 => {
            segment(rd)?;
            let has = rd.flag()?;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            if !has {
                return Ok((0.0, 1.0));
            }
            if t1 - t0 < 1e-6 * t0.abs().max(t1.abs()).max(1.0) {
                return Err(bad("line: interval too small"));
            }
            Ok((t0, t1))
        }
        k @ (3 | 4) => {
            conic_frame(rd)?;
            radius(rd)?;
            if k == 4 {
                radius(rd)?;
            }
            rd.flag()?;
            // the NURB (natural) interval of an SmEllipse / SmCircle is [0, 1]
            Ok((0.0, 1.0))
        }
        _ => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok((0.0, 1.0))
        }
    }
}

/// An interval `t0 t1` with `t0 < t1` inside `[a, b]`.
fn interval(rd: &mut Reader<'_>, (a, b): (f64, f64)) -> Result<(), FfiError> {
    let t0 = rd.finite()?;
    let t1 = rd.finite()?;
    let slack = 1e-12 * (b - a);
    if t0 >= t1 || t0 < a - slack || t1 > b + slack {
        return Err(bad("interval outside the curve or empty"));
    }
    Ok(())
}

/// A surface block (see the module docs).
fn surface_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(1)? == 1 {
        rd.vec3()?;
        let x = rd.nonzero3()?;
        let y = rd.nonzero3()?;
        let cross = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ];
        let lx: f64 = x.iter().map(|c| c * c).sum();
        let ly: f64 = y.iter().map(|c| c * c).sum();
        if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 * lx * ly {
            return Err(bad("plane: parallel axes"));
        }
        for _ in 0..2 {
            let s = rd.finite()?;
            if !(1e-6..=1e6).contains(&s) {
                return Err(bad("plane: scale outside [1e-6, 1e6]"));
            }
        }
        let u0 = rd.finite()?;
        let v0 = rd.finite()?;
        let u1 = rd.finite()?;
        let v1 = rd.finite()?;
        if u1 - u0 < 1e-6 || v1 - v0 < 1e-6 {
            return Err(bad("plane: empty domain"));
        }
        return Ok(());
    }
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || q == 0 || p > n || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad("surface: bad sizes"));
    }
    let cps = rd.take(4 * (n + 1) * (m + 1))?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, _, w0] = *cpts.first().ok_or_else(|| bad("empty surface"))?;
    for &[x, y, z, w] in cpts {
        if (w == NOW) != (w0 == NOW) {
            return Err(bad("surface: mixed rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || (w != NOW && !(1e-3..=1e3).contains(&w))
        {
            return Err(bad("surface: bad control point"));
        }
        let d = if w == NOW { 1.0 } else { w };
        if [x / d, y / d, z / d].iter().any(|c| c.abs() > MAX_COORD) {
            return Err(bad("surface: point out of range"));
        }
    }
    let u = rd.take(r + 1)?;
    let v = rd.take(s + 1)?;
    check_knots(u, p, "surface u")?;
    check_knots(v, q, "surface v")?;
    Ok(())
}

/// The sweep block `base[3] axis[3] angle eps make_analytics brep_tol`.
fn sweep_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    rd.vec3()?;
    let angle = rd.finite()?;
    if angle.abs() > MAX_ANGLE {
        return Err(bad("sweep angle too large"));
    }
    let eps = rd.finite()?;
    if eps.abs() > MAX_TOL {
        return Err(bad("sweep eps too large"));
    }
    rd.flag()?;
    rd.tol()?;
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    sweep_block(rd)?;
    match op {
        Op::IsClosed => {
            rd.index(1 << 20)?;
        }
        Op::StartCs | Op::EndCs => {
            rd.vec3()?;
        }
        Op::Transform | Op::Copy => {}
        Op::CheckCurve => {
            let range = curve_block(rd)?;
            interval(rd, range)?;
            rd.tol()?;
            rd.vec3()?;
            rd.nonzero3()?;
            rd.finite()?;
        }
        Op::VertexHigher | Op::VertexSame => {
            rd.vec3()?;
            rd.tol()?;
        }
        Op::EdgeHigher | Op::EdgeSame => {
            let range = curve_block(rd)?;
            interval(rd, range)?;
            rd.flag()?;
            rd.flag()?;
            rd.vec3()?;
            rd.vec3()?;
            rd.tol()?;
            rd.tol()?;
            rd.tol()?;
        }
        Op::FaceSame => {
            surface_block(rd)?;
            rd.tol()?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve, edge and surface blocks,
        // every index, count and flag); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_sweep_rotational_sweep_geometry_eval(
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
