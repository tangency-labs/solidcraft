//! FFI wrappers for unit `nlib.funcs_adv`.
//!
//! Shim: `harness/cpp/shim/nlib_funcs_adv.cpp` (`sc_nlib_funcs_adv_eval`).
//! Upstream: `source/SMLib/src/NL_FuncsAdv.cpp`.
//!
//! | [`Op`]        | upstream                    | input                          |
//! |---------------|-----------------------------|--------------------------------|
//! | `Refine`      | `N_CrvFuncRefine`           | `fun nx x[0..nx) inplace`      |
//! | `Deriv`       | `N_CrvFuncDeriv`            | `fun d`                        |
//! | `MulFunc`     | `N_CrvFuncMultiplyCrvFunc`  | `fun same [fun]`               |
//! | `MulCrv`      | `N_CrvFuncMultiplyCrv`      | `fun curve`                    |
//! | `MulCrv4d`    | `N_CrvFuncMultiplyCrv4d`    | `fun curve`                    |
//!
//! A function is encoded `n p m`, `n+1` values, `m+1` knots; a curve `n p m`, `n+1`
//! control points `x y z w`, `m+1` knots. [`eval`] accepts only well-formed clamped
//! inputs (`m = n+p+1`, `p <= n`, finite data, non-decreasing knots with end
//! multiplicity exactly `p+1` and interior multiplicity at most `p`, curves of one
//! dimension and rationality). Degree `0` is accepted for a single segment (`n = 0`)
//! except by `Refine` (where upstream writes past its arrays), and by the products
//! also with `n > 0` (upstream rejects that before touching the data). Everything else
//! is rejected before the call.

use crate::{call_with_buffer, FfiError};

/// Largest value / control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest number of refinement knots accepted by [`eval`].
pub const MAX_REFINE: usize = 1024;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvFuncRefine`.
    Refine,
    /// `N_CrvFuncDeriv`.
    Deriv,
    /// `N_CrvFuncMultiplyCrvFunc`.
    MulFunc,
    /// `N_CrvFuncMultiplyCrv`.
    MulCrv,
    /// `N_CrvFuncMultiplyCrv4d`.
    MulCrv4d,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Refine => 0,
            Op::Deriv => 1,
            Op::MulFunc => 2,
            Op::MulCrv => 3,
            Op::MulCrv4d => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_funcs_adv_eval(
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

/// Sequential reader over the input.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Reader<'a> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        self.index(1).map(|v| v == 1)
    }

    fn take(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        self.at = end;
        Ok(s)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// How a degree-0 input is accepted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zero {
    /// Not at all.
    Reject,
    /// Only as a single segment (`n = 0`).
    Single,
    /// With any `n` (upstream errors before reading it when `n > 0`).
    Any,
}

/// The coefficients and knots of a function or curve.
struct Entity<'a> {
    coefs: &'a [f64],
    knots: &'a [f64],
}

/// Reads the header and checks the knots of a function or curve with `width` values
/// per coefficient.
fn entity<'a>(rd: &mut Reader<'a>, width: usize, zero: Zero) -> Result<Entity<'a>, FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if m != n + p + 1 {
        return Err(bad("need m = n + p + 1"));
    }
    let coefs = rd.take(width * (n + 1))?;
    let knots = rd.take(m + 1)?;
    if p == 0 {
        match zero {
            Zero::Reject => return Err(bad("degree 0 not accepted here")),
            Zero::Single if n != 0 => return Err(bad("degree 0 needs n = 0")),
            Zero::Any if n != 0 => return Ok(Entity { coefs, knots }),
            _ => {}
        }
    }
    if p > n {
        return Err(bad("need p <= n"));
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("knots too short");
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("knots not clamped"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("end multiplicity above p + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("interior multiplicity above the degree"));
        }
    }
    if p == 0 && !inner.is_empty() {
        return Err(bad("degree 0 with interior knots"));
    }
    Ok(Entity { coefs, knots })
}

fn function<'a>(rd: &mut Reader<'a>, zero: Zero) -> Result<&'a [f64], FfiError> {
    let Entity {
        coefs: values,
        knots,
    } = entity(rd, 1, zero)?;
    if values.iter().any(|v| !v.is_finite()) {
        return Err(bad("function: non-finite value"));
    }
    Ok(knots)
}

fn curve(rd: &mut Reader<'_>, zero: Zero) -> Result<(), FfiError> {
    let cps = entity(rd, 4, zero)?.coefs;
    let (pts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *pts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in pts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    match op {
        Op::Refine => {
            function(&mut rd, Zero::Reject)?;
            let nx = rd.index(MAX_REFINE)?;
            let x = rd.take(nx)?;
            if x.iter().any(|v| !v.is_finite()) || x.windows(2).any(|w| w.first() > w.last()) {
                return Err(bad("refine: knots not non-decreasing"));
            }
            rd.flag()?;
        }
        Op::Deriv => {
            function(&mut rd, Zero::Single)?;
            rd.index(MAX_DEGREE)?;
        }
        Op::MulFunc => {
            function(&mut rd, Zero::Any)?;
            if !rd.flag()? {
                function(&mut rd, Zero::Any)?;
            }
        }
        Op::MulCrv | Op::MulCrv4d => {
            function(&mut rd, Zero::Any)?;
            curve(&mut rd, Zero::Any)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record
/// (status, then outputs on success).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including the shapes upstream relies
        // on); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_nlib_funcs_adv_eval(
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
