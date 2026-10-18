//! FFI wrappers for unit `curve.composite_curve_p2`.
//!
//! Shim: `harness/cpp/shim/curve_composite_curve_p2.cpp` (`sc_curve_composite_curve_p2_eval`).
//! Upstream: `source/SMLib/src/SmCompositeCurve.cpp`, `source/SMLib/inc/SmCompositeCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve`), `1 dim
//! P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`), `2 3 P[3] V[3] d0 d1 scale io` (a
//! bounded `SmLine`) or `3 3 base N[3] dist` (an `SmOffsetCurve` of the curve block `base`
//! over its natural interval). A composite block is `dim n curve*n closed has_senses
//! sense*n has_gaps gap*n` (`SmCompositeCurve(dim, curves, closed, senses?, gaps?)`).
//! Offset parameters `P` are `xtol atol N[3] corner trim dist match`. A curve record is
//! `type a b P[3]*7 [n+1 Pw[4]*(n+1) m+1 U*(m+1)]` (the NURB for an `SmBSplineCurve`
//! proper), a composite record `dim n closed (transition sense gap_start gap_end owns
//! curve)*n`, a piece record `has_attr [corner left comp curve other orig err] curve`
//! (`orig`: `-1` for no originator, else its index in the original curves, `0` for
//! "not null" where the originator has been deleted). Every output starts with the
//! status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]       | upstream                                | parameters                                        | outputs |
//! |--------------|-----------------------------------------|---------------------------------------------------|---------|
//! | `Offsets`    | `CreateOffsetsOfManyCurves`             | `nc composite*nc dir*nc P`                         | `n piece*n ng gap*ng` (on success) |
//! | `Trimmed`    | `CreateTrimmedOffsets`                  | `composite direction P`                            | `n piece*n` (on success) |
//! | `Intersect`  | `IntersectAndTrimOffsets`               | `n curve*n (has corner left comp curve other orig err)*n dist tol no curve*no` | `n piece*n ni curve*ni` (pieces, then the trimmed inputs; on success) |
//! | `CheckBow`   | `sm_CheckBowTies` (shim copy)           | `n curve*n dist tol no curve*no`                   | `np P[3]*np` (always) |
//! | `FixBow`     | `sm_FixBowTie`                          | `curve np P[3]*np dist nb b*nb`                    | `nb b*nb` (always) |
//! | `Nurb`       | `MakeCompositeNurb`                     | `composite`                                        | `curve composite_record` (on success) |
//! | `ReadDb`     | `SmCompositeCurve::ReadFromDB`          | `dim file_type version nb bytes*nb`                | `composite_record` (on success) |
//! | `WriteDb`    | `WriteToDB` (fixture helper, other unit)| `composite file_type version`                      | `bytes` (always) |
//! | `Misc`       | `SmOffsetMapAttribute::MakeCopy`, `SmCompositeCurveSegment(...)` | `id corner left comp curve other err curve sense g0 g1` | `id corner left comp curve other orig_null err type transition sense g0 g1 owns curve` |
//!
//! `ReadDb` reports a composite with a `NULL` segment curve (not stored, or failed to
//! read; upstream ignores the segment status) as `SM_ERR_INVALID_INPUT`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines, finite Hermite data and bounded lines (as in unit
//! `curve.composite_curve_p1`), at most [`MAX_SEGMENTS`] curves per list; no Hermite
//! curves where upstream copies or joins them (`Copy` fails and upstream uses the
//! `NULL` copy; `SM_CAST_PTR` to `SmBSplineCurve` gives `NULL`); offset curves only as
//! the raw curves of `Intersect`, `CheckBow` and `FixBow`; for `Intersect` at least one
//! original curve and no fewer raw curves than originals (the upstream
//! `sm_CheckBowTies` indexes both arrays by the originals' count) and originator indices
//! inside the original curves; positive tolerances, a non-zero plane normal and offset
//! distance; no chamfer corners (`SM_OC_LINEAR_CHAMFER`: upstream's `sm_CreateCorner`
//! reads `sSolutions[0]` of an empty array whenever the chamfer line misses an
//! extension line, see `plan/findings.md`). `ReadDb` bytes are passed as given (at most [`MAX_DB_BYTES`]); callers send
//! only streams upstream reads without undefined behaviour (made by `WriteDb`, or
//! complete hand-written ASCII).

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 64;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Largest number of curves in a composite or curve list, and of composites.
pub const MAX_SEGMENTS: usize = 8;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 16;
/// Most points or breaks of the bow-tie ops.
pub const MAX_POINTS: usize = 64;
/// Largest type, id or index value.
pub const MAX_ID: usize = 100_000;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `CreateOffsetsOfManyCurves`.
    Offsets,
    /// `CreateTrimmedOffsets`.
    Trimmed,
    /// `IntersectAndTrimOffsets`.
    Intersect,
    /// `sm_CheckBowTies`.
    CheckBow,
    /// `sm_FixBowTie`.
    FixBow,
    /// `MakeCompositeNurb`.
    Nurb,
    /// `SmCompositeCurve::ReadFromDB`.
    ReadDb,
    /// `WriteToDB` (fixture helper).
    WriteDb,
    /// `MakeCopy` and the segment constructor.
    Misc,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Offsets => 0,
            Op::Trimmed => 1,
            Op::Intersect => 2,
            Op::CheckBow => 3,
            Op::FixBow => 4,
            Op::Nurb => 5,
            Op::ReadDb => 6,
            Op::WriteDb => 7,
            Op::Misc => 8,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_composite_curve_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
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

    /// A finite value of magnitude at most [`MAX_COORD`].
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad(format!("value {v} not finite or too large")));
        }
        Ok(v)
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if len_sq(v) < 1e-12 {
            return Err(bad("zero vector"));
        }
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
        Ok(self.index(1)? == 1)
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

fn len_sq(v: [f64; 3]) -> f64 {
    v.iter().map(|c| c * c).sum::<f64>()
}

/// A B-spline `n p m cpts knots`.
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
    Ok(())
}

/// A line `P V d0 d1 scale io`.
fn line(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    rd.nonzero3()?;
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if (d1 - d0).abs() < 1e-6 {
        return Err(bad("line: domain too small"));
    }
    let scale = rd.finite()?;
    if scale <= 1e-6 {
        return Err(bad("line: scale not positive"));
    }
    rd.flag()?;
    Ok(())
}

/// Which curve kinds a list accepts.
#[derive(Debug, Clone, Copy)]
struct Kinds {
    hermite: bool,
    offset: bool,
}

const COPYABLE: Kinds = Kinds {
    hermite: false,
    offset: false,
};
const RAW: Kinds = Kinds {
    hermite: false,
    offset: true,
};
const ANY: Kinds = Kinds {
    hermite: true,
    offset: false,
};

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>, kinds: Kinds) -> Result<(), FfiError> {
    let kind = rd.index(3)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
            bspline(rd)
        }
        1 => {
            if !kinds.hermite {
                return Err(bad("curve block: no Hermite curve here"));
            }
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(())
        }
        2 => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            line(rd)
        }
        _ => {
            if !kinds.offset || dim != 3 {
                return Err(bad("curve block: no offset curve here"));
            }
            curve_block(
                rd,
                Kinds {
                    hermite: false,
                    offset: false,
                },
            )?;
            rd.nonzero3()?;
            rd.finite()?;
            Ok(())
        }
    }
}

fn curve_list(rd: &mut Reader<'_>, kinds: Kinds) -> Result<usize, FfiError> {
    let n = rd.index(MAX_SEGMENTS)?;
    for _ in 0..n {
        curve_block(rd, kinds)?;
    }
    Ok(n)
}

/// A composite block (see the module docs): its segment count.
fn composite_block(rd: &mut Reader<'_>, kinds: Kinds) -> Result<usize, FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("composite: dimension must be 2 or 3"));
    }
    let n = curve_list(rd, kinds)?;
    let closed = rd.flag()?;
    if rd.flag()? {
        for _ in 0..n {
            rd.flag()?;
        }
    }
    if rd.flag()? {
        if closed && n == 0 {
            return Err(bad("composite: gaps of a closed composite without curves"));
        }
        for _ in 0..n {
            rd.finite()?;
        }
    }
    Ok(n)
}

/// A positive finite tolerance.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v <= 0.0 {
        return Err(bad("tolerance not positive"));
    }
    Ok(v)
}

/// A non-zero finite distance.
fn distance(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v.abs() < 1e-6 {
        return Err(bad("distance too small"));
    }
    Ok(v)
}

/// Offset parameters `xtol atol N[3] corner trim dist match`.
fn params(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    tol(rd)?;
    tol(rd)?;
    rd.nonzero3()?;
    // no SM_OC_LINEAR_CHAMFER: upstream sm_CreateCorner reads sSolutions[0] of an empty
    // array whenever the chamfer line misses an extension line
    rd.index(1)?;
    rd.flag()?;
    distance(rd)?;
    rd.flag()?;
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Offsets => {
            let nc = rd.index(MAX_SEGMENTS)?;
            for _ in 0..nc {
                composite_block(rd, COPYABLE)?;
            }
            for _ in 0..nc {
                rd.index(4)?;
            }
            params(rd)?;
        }
        Op::Trimmed => {
            composite_block(rd, COPYABLE)?;
            rd.index(3)?;
            params(rd)?;
        }
        Op::Intersect => {
            let n = curve_list(rd, RAW)?;
            let at = rd.at;
            // the originator indices are checked once the original count is known
            for _ in 0..n {
                rd.flag()?;
                rd.flag()?;
                rd.flag()?;
                rd.index(MAX_ID)?;
                rd.index(MAX_ID)?;
                rd.index(MAX_ID)?;
                rd.num()?;
                rd.finite()?;
            }
            tol(rd)?;
            tol(rd)?;
            let no = curve_list(rd, COPYABLE)?;
            if no == 0 || n < no {
                return Err(bad(
                    "intersect: need 1 <= originals <= raw curves (sm_CheckBowTies indexes both)",
                ));
            }
            let mut back = Reader {
                input: rd.input,
                at,
            };
            for _ in 0..n {
                back.take(6)?;
                let orig = back.num()?;
                if orig.fract() != 0.0 || orig < -1.0 || orig >= no as f64 {
                    return Err(bad("intersect: originator index out of range"));
                }
                back.num()?;
            }
        }
        Op::CheckBow => {
            curve_list(rd, RAW)?;
            tol(rd)?;
            tol(rd)?;
            curve_list(rd, COPYABLE)?;
        }
        Op::FixBow => {
            curve_block(rd, RAW)?;
            let np = rd.index(MAX_POINTS)?;
            for _ in 0..np {
                rd.vec3()?;
            }
            tol(rd)?;
            let nb = rd.index(MAX_POINTS)?;
            for _ in 0..nb {
                rd.finite()?;
            }
        }
        Op::Nurb => {
            if composite_block(rd, COPYABLE)? == 0 {
                return Err(bad("nurb: no segments"));
            }
        }
        Op::ReadDb => {
            let dim = rd.index(3)?;
            if dim < 2 {
                return Err(bad("read: dimension must be 2 or 3"));
            }
            rd.index(1)?;
            rd.index(MAX_ID)?;
            let nb = rd.index(MAX_DB_BYTES)?;
            for _ in 0..nb {
                rd.index(255)?;
            }
        }
        Op::WriteDb => {
            composite_block(rd, ANY)?;
            rd.index(1)?;
            rd.index(MAX_ID)?;
        }
        Op::Misc => {
            rd.index(MAX_ID)?;
            rd.flag()?;
            rd.flag()?;
            rd.index(MAX_ID)?;
            rd.index(MAX_ID)?;
            rd.index(MAX_ID)?;
            rd.finite()?;
            curve_block(rd, ANY)?;
            rd.flag()?;
            rd.finite()?;
            rd.finite()?;
        }
    }
    Ok(())
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] describing the first problem.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
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
    call_with_buffer(1 << 14, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve and composite blocks, every
        // index, count and flag the shim passes to upstream); buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_composite_curve_p2_eval(
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
