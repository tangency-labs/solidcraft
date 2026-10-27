//! FFI wrappers for unit `curve.tangent_field`. Shim: `harness/cpp/shim/curve_tangent_field.cpp`.
//! Upstream: source/SMLib/src/SmTangentField.cpp (`SmTangentField`).
//!
//! Every op takes one flat `f64` input (validated here before the call) and returns the
//! shim's record: the op status (`0` = `SM_SUCCESS`) followed by the op's outputs.
//!
//! Blocks:
//! - curve: `kind dim ...`; kind `0` (`SmBSplineCurve`): `n p m cpts((n+1) x [x y z w]) knots(m+1)`
//!   (z = `f64::MAX` for a 2D curve, w = `-f64::MAX` for a polynomial one); kind `1`
//!   (`SmHermiteCurve`): `P1 D1 P2 D2` (12 values).
//! - surface: four corners `P00 P10 P01 P11` (12 values) of a bilinear `SmBSplineSurface`
//!   on `[0,1]^2`.
//! - field: `curve dir start(3) end(3) has_domain umin vmin umax vmax`.
//!
//! Ops ([`Op`]):
//! - `State`: `surface field` -> `0 type kinds(8) ivl_min ivl_max len type_string len class_string`.
//! - `Eval`: `surface field t n` -> `status [(n+1) x xyz]`.
//! - `EvalPoint`: `surface field t` -> `status [xyz]`.
//! - `Equal`: `surface field_a flags [surface_b] field_b` (`surface_b` present unless flag bit 0)
//!   -> `0 eq`.
//! - `Create`: `surface tol base3d base_uv(B-spline) start_curve start_param start_opposite
//!   end_curve end_param end_opposite` -> `status [start_tangent end_tangent]`.
//! - `ReadDb`: `file_type dim version nbytes bytes` -> `status`.
//! - `WriteRead`: `surface mode (field | create) file_type version cache_breaks angle_tol
//!   nparams params` (`mode` 0: a field block, built with the data constructor; 1: a
//!   `Create` block as for `Create`). The field is written (`WriteType` + `WriteToDB`) and
//!   read back (`ReadType` + `SmCurve::ReadFromDB`) -> `create_status [knots_status nknots
//!   knots (cache_breaks)] write_status [nbytes bytes read_status [type kinds(8) ivl_min
//!   ivl_max eq_to_original per-param(ep_status [xyz] ev1_status [2 x xyz] ev2_status)
//!   cont_status [min n conts] copy_status [copy_type per-param(status [xyz])]
//!   rewrite_status [nbytes bytes]]]`.
use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 64;
/// Largest degree.
pub const MAX_DEGREE: usize = 7;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 4.0;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 4096;
/// Most derivatives asked of `Evaluate`.
pub const MAX_DERIVS: usize = 3;

/// The shim's operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Type queries and the natural interval.
    State,
    /// `Evaluate`.
    Eval,
    /// `EvaluatePoint`.
    EvalPoint,
    /// `operator==`.
    Equal,
    /// `Create`.
    Create,
    /// `ReadFromDB`.
    ReadDb,
    /// `WriteToDB` then `SmCurve::ReadFromDB`, and queries on the curve read back.
    WriteRead,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::State => 0,
            Op::Eval => 1,
            Op::EvalPoint => 2,
            Op::Equal => 3,
            Op::Create => 4,
            Op::ReadDb => 5,
            Op::WriteRead => 6,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_tangent_field_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

/// A validated curve block: whether it is a B-spline, and its parameter range.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    bspline: bool,
    a: f64,
    b: f64,
}

/// A B-spline `n p m cpts knots`: clamped, valid multiplicities, sane values.
fn bspline(rd: &mut Reader<'_>, dim: usize) -> Result<(f64, f64), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    if (z0 == NOZ) != (dim == 2) {
        return Err(bad(
            "curve: 2D curves need z = f64::MAX, 3D curves a finite z",
        ));
    }
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || (z != NOZ && !z.is_finite())
            || (w != NOW && !w.is_finite())
        {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && w <= 1e-3 {
            return Err(bad("curve: non-positive weight"));
        }
        if euclid([x, y, z, w])
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad("curve: Euclidean point out of range"));
        }
    }
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
        return Err(bad("curve: knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("curve: knots too short");
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("curve: knots not clamped"));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad("curve: parameter range too small"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("curve: end multiplicity above p + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("curve: interior multiplicity above the degree"));
        }
    }
    Ok((a, b))
}

/// A curve block.
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(1)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    if kind == 0 {
        let (a, b) = bspline(rd, dim)?;
        Ok(CurveInfo {
            bspline: true,
            a,
            b,
        })
    } else {
        for _ in 0..4 {
            rd.vec3()?;
        }
        Ok(CurveInfo {
            bspline: false,
            a: 0.0,
            b: 1.0,
        })
    }
}

/// A parameter within [`MAX_REL_DIST`] ranges of the curve's.
fn param(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    let far = MAX_REL_DIST * (c.b - c.a);
    if t < c.a - far || t > c.b + far {
        return Err(bad("parameter too far outside the curve"));
    }
    Ok(t)
}

fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    for _ in 0..4 {
        rd.vec3()?;
    }
    Ok(())
}

/// A field block; returns its UV curve.
fn field(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let c = curve_block(rd)?;
    rd.index(2)?;
    rd.vec3()?;
    rd.vec3()?;
    rd.flag()?;
    let [u0, v0, u1, v1] = [rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?];
    if u0 > u1 || v0 > v1 {
        return Err(bad("field: domain min above max"));
    }
    Ok(c)
}

/// Most parameters of a `WriteRead` op.
pub const MAX_PARAMS: usize = 16;

/// A `Create` block (after the surface); returns the base UV curve.
fn create_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let tol = rd.finite()?;
    if tol < 0.0 {
        return Err(bad("create: negative tolerance"));
    }
    curve_block(rd)?;
    let uv = curve_block(rd)?;
    if !uv.bspline {
        return Err(bad("create: the base UV curve must be a B-spline"));
    }
    for _ in 0..2 {
        let c = curve_block(rd)?;
        param(rd, &c)?;
        rd.flag()?;
    }
    Ok(uv)
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::ReadDb => {
            rd.index(1)?;
            let dim = rd.index(3)?;
            if dim < 2 {
                return Err(bad("read: dimension must be 2 or 3"));
            }
            rd.index(1000)?;
            let nb = rd.index(MAX_DB_BYTES)?;
            for _ in 0..nb {
                rd.index(255)?;
            }
        }
        Op::Create => {
            surface(&mut rd)?;
            create_block(&mut rd)?;
        }
        Op::WriteRead => {
            surface(&mut rd)?;
            let c = if rd.index(1)? == 0 {
                field(&mut rd)?
            } else {
                create_block(&mut rd)?
            };
            rd.index(1)?;
            rd.index(1000)?;
            rd.flag()?;
            rd.finite()?;
            let np = rd.index(MAX_PARAMS)?;
            for _ in 0..np {
                param(&mut rd, &c)?;
            }
        }
        Op::State | Op::Eval | Op::EvalPoint => {
            surface(&mut rd)?;
            let c = field(&mut rd)?;
            if op != Op::State {
                param(&mut rd, &c)?;
            }
            if op == Op::Eval {
                rd.index(MAX_DERIVS)?;
            }
        }
        Op::Equal => {
            surface(&mut rd)?;
            field(&mut rd)?;
            let flags = rd.index(7)?;
            if flags & 1 == 0 {
                surface(&mut rd)?;
            }
            field(&mut rd)?;
        }
    }
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
    call_with_buffer(256, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve, surface and field blocks,
        // every index, count and flag the shim passes to upstream); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_tangent_field_eval(
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
