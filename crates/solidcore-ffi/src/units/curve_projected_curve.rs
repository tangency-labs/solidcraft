//! FFI wrappers for unit `curve.projected_curve`.
//!
//! Shim: `harness/cpp/shim/curve_projected_curve.cpp` (`sc_curve_projected_curve_eval`).
//! Upstream: `source/SMLib/src/SmProjectedCurve.cpp`, `source/SMLib/inc/SmProjectedCurve.h`.
//!
//! Curve ops start with a projected-curve block: a base curve block (as unit
//! `curve.curve_p2`: `0 dim oob n p m cpts(x y z w) knots`, an `SmBSplineCurve`, or
//! `1 dim P1[3] D1[3] P2[3] D2[3]`, an `SmHermiteCurve`; `2` is kind `0` with the given
//! knots written back over the curve's own NURB after construction) followed by
//! `ctor has_P P[3] has_V V[3] has_A A[3] type owns`: `ctor` 0 builds the curve with
//! the constructor (a vector without its `has_` flag is passed as `NULL`), 1 with
//! `SetCanonical` on an empty curve (missing vectors default-constructed, i.e.
//! `SM_UNDEF_DOUBLE`); `type` is the `SmProjectionType` (0..=3), `owns` the
//! `SmBoolean` ownership flag (0..=2). Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]         | upstream                           | parameters             | outputs |
//! |----------------|------------------------------------|------------------------|---------|
//! | `State`        | the construction, `GetType`, `GetDim`, `GetDegree`, `GetNumberNaturalKnots`, `GetNumberControlPoints`, `GetNaturalInterval`, `IsAnalytic`, `IsBounded`, `IsRational`, `IsKindOf` | | status of `SetCanonical` (0 for the constructor), `state type dim deg nnk ncp a b analytic bounded rational kind[7]` |
//! | `MakeOrtho`    | `MakeOrthoNormal`                  |                        | state (always) |
//! | `Eval`         | `Evaluate`                         | `t n left nonzero`     | `(n+1)` vectors |
//! | `EvalPoint`    | `EvaluatePoint`                    | `t`                    | point |
//! | `Knots`        | `GetKnots`                         | `has_ivl a b`          | `nk K nm M` |
//! | `Continuities` | `CalculateContinuities`            | `angle`                | `min nk C` |
//! | `EditParam`    | `EditParameterization`             | `a b`                  | state (always) |
//! | `Reverse`      | `ReverseParameterization`          | `a b`                  | `[a' b']` state (always) |
//! | `Trim`         | `Trim`                             | `a b mode` (0 `TRUE`, 1 `FALSE`, 2 `UNSURE`) | `a' b'` state (always) |
//! | `Transform`    | `Transform`                        | `O X Y has_scale S`    | state (always) |
//! | `Copy`         | `Copy`                             |                        | `state shared equal` |
//! | `CloneCtor`    | the copy constructor `SmProjectedCurve(const SmProjectedCurve&)` | | `state shared equal eval knots` |
//! | `Equal`        | `operator==`                       | `self` [projected-curve block] | flag |
//! | `Ends`         | `SmCurve::GetEnds` (base default)  |                        | `P0 P1 w0 w1` |
//! | `ReadDb`       | `SmProjectedCurve::ReadFromDB` (static, no block) | `version binary nb B[nb]` | `[state type]` |
//!
//! A state is `P[3] V[3] A[3] type owns` then the base curve: `0 dim n p m cpts knots`
//! (B-spline), `1 dim P(0) C'(0) P(1) C'(1)` (Hermite, evaluated back), `-1` (other
//! type) or `-2` (none). `Copy` of a curve owning a Hermite curve (no `Copy`: upstream
//! returns a projected curve with a null base curve) reports `SM_ERR`.
//!
//! `CloneCtor` runs the copy constructor itself (not the virtual `Copy`) with the
//! original and a borrowed base curve alive until the copy is destroyed; it reports
//! the copy's state, whether the copy shares the original's base curve (upstream: the
//! pointer, so exactly when the base is not owned), `copy == original`, `eval` (the
//! copy's `EvaluatePoint` at the middle of the original's natural interval) and
//! `knots` (the copy's `GetKnots`, so a borrowed copy's raw near-coincident knots stay
//! distinct while an owned copy's knot fix-up merges them), each as its status then
//! `nk K nm M` / the point when it is `SM_SUCCESS`. Its defined domain is the base
//! curves the native copy constructor can copy: [`validate`] rejects an owned or
//! unsure Hermite base curve, which upstream would copy into a null base curve
//! (undefined use), instead of reporting a status for it (`Copy` keeps that `SM_ERR`
//! report); the shim's own null-base net (unreachable through [`validate`]) reports
//! `SC_ERR_ARG`, never an upstream status.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: the base
//! curves of unit `curve.curve_p2` (well-formed clamped B-splines of degree 1..=9,
//! finite bounded Hermite data), finite bounded vectors, parameters within a few curve
//! lengths of the curve's range, derivative counts up to 3, and database streams of at
//! most [`MAX_DB_BYTES`] bytes (well-formed streams are the caller's duty: upstream
//! trusts the counts it reads).

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 512;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 4.0;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 16;
/// Highest derivative count of `Eval`.
pub const MAX_DERIVS: usize = 3;

/// One shim operation (codes in the shim).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    State,
    MakeOrtho,
    Eval,
    EvalPoint,
    Knots,
    Continuities,
    EditParam,
    Reverse,
    Trim,
    Transform,
    Copy,
    CloneCtor,
    Equal,
    Ends,
    /// `AssertValid(list or NULL, level, SM_NO_WALK)`: args `level with_list`.
    AssertValid,
    ReadDb,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::State => 0,
            Op::MakeOrtho => 1,
            Op::Eval => 2,
            Op::EvalPoint => 3,
            Op::Knots => 4,
            Op::Continuities => 5,
            Op::EditParam => 6,
            Op::Reverse => 7,
            Op::Trim => 8,
            Op::Transform => 9,
            Op::Copy => 10,
            Op::Equal => 11,
            Op::Ends => 12,
            Op::CloneCtor => 13,
            Op::AssertValid => 14,
            Op::ReadDb => 20,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_projected_curve_eval(
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

/// The parameter range of a validated block.
#[derive(Debug, Clone, Copy)]
struct Range {
    a: f64,
    b: f64,
}

/// A B-spline `n p m cpts knots`.
fn bspline(rd: &mut Reader<'_>) -> Result<Range, FfiError> {
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
    Ok(Range { a, b })
}

/// A base curve block: its parameter range and kind (`0`/`2` B-spline, `1` Hermite).
#[derive(Debug, Clone, Copy)]
struct BaseBlock {
    range: Range,
    kind: usize,
    /// A Hermite base whose power basis may be undefined (dimension 3 with a member
    /// component beyond `1e16`).
    hermite_big: bool,
}

/// A projected-curve block: the base curve block and the ownership flag (`0`
/// borrowed, `1` owned, `2` unsure).
#[derive(Debug, Clone, Copy)]
struct Block {
    base: BaseBlock,
    owns: usize,
    /// `SmProjectionType`.
    ptype: usize,
}

/// A base curve block.
fn curve_block(rd: &mut Reader<'_>) -> Result<BaseBlock, FfiError> {
    let kind = rd.index(2)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    // 2: a B-spline whose raw knots are written back after construction
    let mut hermite_big = false;
    let range = if kind == 0 || kind == 2 {
        rd.flag()?;
        bspline(rd)?
    } else {
        for _ in 0..4 {
            let v = rd.vec3()?;
            hermite_big |= dim == 3 && v.iter().any(|c| c.abs() > 1e16);
        }
        Range { a: 0.0, b: 1.0 }
    };
    Ok(BaseBlock {
        range,
        kind,
        hermite_big,
    })
}

/// A projected-curve block (see the module docs).
fn projected_block(rd: &mut Reader<'_>) -> Result<Block, FfiError> {
    let base = curve_block(rd)?;
    rd.index(1)?; // ctor
    for _ in 0..3 {
        rd.flag()?;
        rd.vec3()?;
    }
    let ptype = rd.index(3)?; // SmProjectionType
    let owns = rd.index(2)?; // SmBoolean
    Ok(Block { base, owns, ptype })
}

/// A parameter within [`MAX_REL_DIST`] ranges of the curve's.
fn param(rd: &mut Reader<'_>, c: Range) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    let far = MAX_REL_DIST * (c.b - c.a);
    if t < c.a - far || t > c.b + far {
        return Err(bad("parameter too far outside the curve"));
    }
    Ok(t)
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op == Op::ReadDb {
        rd.index(1000)?; // version
        rd.flag()?;
        let nb = rd.index(MAX_DB_BYTES)?;
        for _ in 0..nb {
            rd.index(255)?;
        }
        return rd.done();
    }
    let c = projected_block(&mut rd)?;
    // the copy constructor copies an owned base curve with `Copy`, which a Hermite
    // curve has not: upstream ignores the status, keeping a null base curve that
    // crashes on first use. That input is outside the operation's defined domain
    // (`Copy` keeps its own SM_ERR report below).
    if op == Op::CloneCtor && c.owns != 0 && c.base.kind == 1 {
        return Err(bad(
            "clone_ctor: an owned or unsure Hermite base curve has no Copy",
        ));
    }
    match op {
        Op::State | Op::MakeOrtho | Op::Copy | Op::CloneCtor | Op::Ends | Op::ReadDb => {}
        Op::Eval => {
            param(&mut rd, c.base.range)?;
            rd.index(MAX_DERIVS)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::EvalPoint => {
            param(&mut rd, c.base.range)?;
        }
        Op::Knots => {
            rd.flag()?;
            param(&mut rd, c.base.range)?;
            param(&mut rd, c.base.range)?;
        }
        Op::Continuities => {
            let a = rd.finite()?;
            if a <= 0.0 {
                return Err(bad("angle tolerance must be positive"));
            }
        }
        Op::EditParam | Op::Reverse => {
            param(&mut rd, c.base.range)?;
            param(&mut rd, c.base.range)?;
        }
        Op::Trim => {
            param(&mut rd, c.base.range)?;
            param(&mut rd, c.base.range)?;
            rd.index(2)?;
        }
        Op::Transform => {
            for _ in 0..3 {
                rd.vec3()?;
            }
            rd.flag()?;
            rd.vec3()?;
        }
        Op::Equal => {
            if !rd.flag()? {
                projected_block(&mut rd)?;
            }
        }
        Op::AssertValid => {
            if rd.index(4)? == 3 {
                return Err(bad("assert_valid: test level 3"));
            }
            // the base curve's check (projection type known): a Hermite base's failed
            // test 1 with a report list reads past the one-entry default label table
            if rd.flag()? && c.ptype != 3 && c.base.hermite_big {
                return Err(bad(
                    "assert_valid: undefined Hermite power basis reads past the label table",
                ));
            }
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve blocks, every index, count
        // and flag the shim passes to upstream); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_projected_curve_eval(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The Hermite base curve block (`1 dim P1 D1 P2 D2`).
    fn hermite_base() -> Vec<f64> {
        vec![1., 3., 0., 0., 0., 1., 0., 0., 1., 1., 0.5, 0., 1., 0.]
    }

    /// `base` followed by `ctor has_P P[3] has_V V[3] has_A A[3] type owns`.
    fn projected(base: &[f64], owns: f64) -> Vec<f64> {
        let mut v = base.to_vec();
        v.extend([0., 1., 0., 0., 0., 1., 0., 0., 1., 0., 0., 0., 0., 0., owns]);
        v
    }

    #[test]
    fn clone_ctor_domain_rejects_owned_or_unsure_hermite_base_only() {
        // a borrowed Hermite base curve is shared by the copy constructor: valid
        validate(Op::CloneCtor, &projected(&hermite_base(), 0.)).unwrap();
        for owns in [1., 2.] {
            // an owned or unsure one has no `Copy`: upstream keeps a null base curve
            // (undefined use), so the input is rejected before C++ instead of run (the
            // wording is incidental, the category is the contract)
            assert!(matches!(
                validate(Op::CloneCtor, &projected(&hermite_base(), owns)),
                Err(FfiError::InvalidArgument(_))
            ));
            // `Copy` keeps its own domain: the shim reports the base curve's SM_ERR
            validate(Op::Copy, &projected(&hermite_base(), owns)).unwrap();
        }
        // a B-spline base curve copies, whatever the ownership flag
        let bspline = [
            0., 3., 0., 1., 1., 3., 0., 0., 0., NOW, 1., 0., 0., NOW, 0., 0., 1., 1.,
        ];
        for owns in [0., 1., 2.] {
            validate(Op::CloneCtor, &projected(&bspline, owns)).unwrap();
        }
    }
}
