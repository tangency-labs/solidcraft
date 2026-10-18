//! FFI wrappers for unit `curve.offset_curve`.
//!
//! Shim: `harness/cpp/shim/curve_offset_curve.cpp` (`sc_curve_offset_curve_op`).
//! Upstream: `source/SMLib/inc/SmOffsetCurve.h`, `source/SMLib/src/SmOffsetCurve.cpp`.
//!
//! Curve ops start with a base block, a spec and an optional outer spec:
//!
//! * base: `0 dim oob n p m cpts(x y z w) knots` (`SmBSplineCurve(gw_CURVE*, dim, FALSE)`
//!   with `SetOutOfBoundsEnabled(oob)`) or `1 dim P1[3] D1[3] P2[3] D2[3]`
//!   (`SmHermiteCurve`);
//! * spec: `dim a b N[3] dist owns nrev`: `SmOffsetCurve(dim, base, SmExtent1d(a, b), N,
//!   dist, owns)`, then `nrev` times `ReverseParameterization(GetNaturalInterval())`;
//! * `has_outer [spec]`: an offset of that offset, the curve the op runs on.
//!
//! [`Op::ReadDb`] has no curve: `file_type version nb bytes[nb]`. Probes are
//! `k t[k]`: per `t`, `EvaluatePoint` (`status [P]`) and `Evaluate(t, 3, TRUE, .., TRUE)`
//! (`status [4 vectors]`). A state is `type dim N[3] dist a b reversed base_type nnk`
//! (the members through `WriteToDB`). Every output starts with the op status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`):
//!
//! | [`Op`]         | upstream                    | parameters                     | outputs |
//! |----------------|-----------------------------|--------------------------------|---------|
//! | `State`        | `GetType`, `GetDim`, members, `GetNumberNaturalKnots` |      | state |
//! | `Eval`         | `Evaluate`                  | `t n left nonzero`             | `(n+1)` vectors |
//! | `EvalPoint`    | `EvaluatePoint`             | `t`                            | point |
//! | `Knots`        | `GetKnots`                  | `has_ivl a b want_mults`       | `nk K nm M` |
//! | `Continuities` | `CalculateContinuities`     | `angle`                        | `min n C` |
//! | `Reverse`      | `ReverseParameterization`   | `a b` probes                   | `a' b'` state probes (always) |
//! | `Trim`         | `Trim`                      | `a b mode` probes              | `a' b'` state probes (always) |
//! | `Eq`           | `operator==`                | `self copy [spec]`             | `this==other [other==this]` |
//! | `Copy`         | `Copy`                      | probes                         | state probes |
//! | `IsKindOf`     | `IsKindOf`                  | `type`                         | flag |
//! | `ReadDb`       | `SmOffsetCurve::ReadFromDB` | (no curve) see above, probes   | state probes |
//! | `WriteDb`      | `WriteToDB` (fixture helper, not in the unit) | `file_type version` | `bytes` (always) |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped B-splines and finite Hermite data, finite bounded values, derivative counts
//! up to [`MAX_DERIVS`], and no `Copy` of an owning offset whose base cannot be copied
//! (a Hermite curve: upstream then shares and deletes it twice).

use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 256;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Largest derivative count of [`Op::Eval`] (`NL_MAXDER`: a rational B-spline base
/// evaluated with more overflows a fixed NLib array).
pub const MAX_DERIVS: usize = 4;
/// Most probes.
pub const MAX_PROBES: usize = 32;
/// Most reversals in a spec.
pub const MAX_REVERSALS: usize = 4;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 16;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// One shim operation (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(missing_docs)]
pub enum Op {
    State = 0,
    Eval = 1,
    EvalPoint = 2,
    Knots = 3,
    Continuities = 4,
    Reverse = 5,
    Trim = 6,
    Eq = 7,
    Copy = 8,
    IsKindOf = 9,
    ReadDb = 10,
    WriteDb = 11,
}

mod sys {
    extern "C" {
        pub fn sc_curve_offset_curve_op(
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
    fn vec3(&mut self) -> Result<(), FfiError> {
        for _ in 0..3 {
            self.finite()?;
        }
        Ok(())
    }

    fn int(&mut self, min: i64, max: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < min as f64 || v > max as f64 {
            return Err(bad(format!("bad integer {v}")));
        }
        Ok(v as i64)
    }
}

fn bspline(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        let fin = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
        if !fin(x) || !fin(y) || (z != NOZ && !fin(z)) || (w != NOW && !fin(w)) {
            return Err(bad("curve: control point out of range"));
        }
        if w != NOW && w <= 1e-3 {
            return Err(bad("curve: non-positive weight"));
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
    Ok(())
}

/// A base block; true for a B-spline.
fn base_block(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let kind = rd.index(1)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("base: dimension must be 2 or 3"));
    }
    if kind == 0 {
        rd.flag()?;
        bspline(rd)?;
        Ok(true)
    } else {
        for _ in 0..4 {
            rd.vec3()?;
        }
        Ok(false)
    }
}

/// A spec block; returns its ownership flag.
fn spec(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("spec: dimension must be 2 or 3"));
    }
    rd.finite()?;
    rd.finite()?;
    rd.vec3()?;
    rd.finite()?;
    let owns = rd.flag()?;
    rd.index(MAX_REVERSALS)?;
    Ok(owns)
}

fn probes(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let k = rd.index(MAX_PROBES)?;
    for _ in 0..k {
        rd.finite()?;
    }
    Ok(())
}

/// Checks `input` against the layout of `op` (module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op == Op::ReadDb {
        rd.index(1)?;
        rd.index(64)?;
        let nb = rd.index(MAX_DB_BYTES)?;
        for _ in 0..nb {
            rd.index(255)?;
        }
        probes(&mut rd)?;
        return rd.done();
    }
    let is_bspline = base_block(&mut rd)?;
    let inner_owns = spec(&mut rd)?;
    if rd.flag()? {
        spec(&mut rd)?;
    }
    // Copying an offset that owns a Hermite base: upstream's base Copy fails, the copy
    // shares the base and both delete it.
    let copy_unsafe = !is_bspline && inner_owns;
    match op {
        // ReadDb was handled above
        Op::State | Op::ReadDb => {}
        Op::Eval => {
            rd.finite()?;
            rd.index(MAX_DERIVS)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::EvalPoint => {
            rd.finite()?;
        }
        Op::Knots => {
            rd.flag()?;
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
        }
        Op::Continuities => {
            let a = rd.finite()?;
            if !(0.0..=90.0).contains(&a) {
                return Err(bad("angle out of range"));
            }
        }
        Op::Reverse => {
            rd.finite()?;
            rd.finite()?;
            probes(&mut rd)?;
        }
        Op::Trim => {
            rd.finite()?;
            rd.finite()?;
            rd.index(2)?;
            probes(&mut rd)?;
        }
        Op::Eq => {
            let this = rd.flag()?;
            let copy = rd.flag()?;
            if copy && copy_unsafe && !this {
                return Err(bad("copy of an offset owning a Hermite curve"));
            }
            if !this && !copy {
                spec(&mut rd)?;
            }
        }
        Op::Copy => {
            if copy_unsafe {
                return Err(bad("copy of an offset owning a Hermite curve"));
            }
            probes(&mut rd)?;
        }
        Op::IsKindOf => {
            rd.int(0, 1 << 20)?;
        }
        Op::WriteDb => {
            rd.index(1)?;
            rd.index(64)?;
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
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve blocks, every index, count
        // and flag the shim passes to upstream); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_offset_curve_op(
                op as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
