//! FFI wrappers for unit `curve.crv_in_volume`.
//!
//! Shim: `harness/cpp/shim/curve_crv_in_volume.cpp` (`sc_curve_crv_in_volume_eval`).
//! Upstream: `source/SMLib/src/SmCrvInVolume.cpp`, `source/SMLib/inc/SmCrvInVolume.h`.
//!
//! Object ops start with a block building the `SmCrvInVolume` `c` the op runs on:
//! a curve block (`0 dim oob n p m cpts(x y z w) knots`: an `SmBSplineCurve`; or
//! `1 dim P1[3] D1[3] P2[3] D2[3]`: an `SmHermiteCurve`), then `in_param copy_flag
//! owner_flag` (the constructor's arguments; the volume is a fresh identity
//! `SmTransform`), then `has_cache` and, when set, `need nb B[nb] nc C[nc]` loaded
//! into `m_bNeedBreaks`, `m_sBreaks`, `m_sConts`. Static ops have no block. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]       | upstream                                   | parameters              | outputs |
//! |--------------|--------------------------------------------|-------------------------|---------|
//! | `Info`       | `GetType`, `GetDim`, `GetDegree`, `GetNumberControlPoints`, `GetNaturalInterval`, `IsAnalytic`, `IsBounded`, `OwnsCurve`, `OwnsVolume`, `IsKindOf` | `nt T[nt]` | `type dim deg ncp a b analytic bounded owns_c owns_v kind[nt]` state |
//! | `Eval`       | `Evaluate`                                 | `t n left nonzero`      | `(n+1)` vectors (on success) |
//! | `EvalPoint`  | `EvaluatePoint`                            | `t`                     | point (on success) |
//! | `Knots`      | `GetKnots`                                 | `has_ivl a b want`      | `nk K [nm M]` cache |
//! | `Mults`      | `GenerateMults`                            | `nk K[nk]`              | `M` (on success) |
//! | `Nnk`        | `GetNumberNaturalKnots`                    |                         | `count` cache |
//! | `Conts`      | `CalculateContinuities`                    | `angle`                 | `min nc C` cache |
//! | `Equal`      | `operator==`                               | `self` [object block]   | flag |
//! | `Copy`       | `Copy` (copy constructor)                  |                         | state of the copy (on success) |
//! | `EditParam`  | `EditParameterization`                     | `a b`                   | state |
//! | `Reverse`    | `ReverseParameterization`                  | `a b`                   | `[a' b']` state |
//! | `Trim`       | `Trim(ivl, notify, FALSE)`                 | `a b mode` (0 `TRUE`, 1 `FALSE`, 2 `UNSURE`) | `a' b'` state |
//! | `Transform`  | `Transform`                                | `O X Y has_scale S`     | state |
//! | `ReadDb`     | `SmCrvInVolume::ReadFromDB` (static, ASCII) | `version nb B[nb]`     | `made [state]` |
//! | `Exact`      | `MakeExactBSplineIfPossible` (static: builds its own object) | `curve in_param volume` | `made [exact]` (on success) |
//!
//! A cache is `need nb B[nb] nc C[nc]`; a state is `owner_flag in_param` cache and the
//! base curve (`0 dim n p m cpts knots`, `1 dim P(0) C'(0) P(1) C'(1)` evaluated back,
//! or `-1`).
//!
//! `Exact` builds `SmCrvInVolume(curve, in_param, volume, 0, 3)` from an exact curve
//! block (a curve block as above; `2 P1[3] P2[3]`: `SmLine(P1, P2, 3)`; or
//! `3 C[3] X[3] Y[3] a0 a1 r`: `SmCircle(C, X, Y, SmExtent1d(a0, a1), r, 3)`) and a
//! volume chain `n` maps `kind [T(12) | C(11)] has_orient [O(12)]` (as unit
//! `surface.srf_in_volume`: `0` plain `SmVolume`, `1` `SmTransform(O, X, Y, &Z)`, `2`..`4`
//! default twist / unbend / bend volumes, `5`..`7` their component constructors with
//! `C` = `p0 axis mid a b`, never with an orient map), chained with `AddNextMap(m, 2)`.
//! Its exact record is `type dim had_nurb`, the NURB made by
//! `GetOrCreateGwNurbPointer` (`n p m cpts(x y z w) knots`, or `-1`), the natural
//! interval `a b`, then `st P[3] T[3]` of `Evaluate(t, 1, TRUE, .., TRUE)` at the
//! interval fractions `0, 1/4, 1/2, 3/4, 1`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped B-splines (as unit `curve.curve_p2`), finite bounded Hermite data, cached
//! breaks and parameters near the curve's range, continuity codes of the enum, cache
//! shapes for which upstream stays inside its arrays (`GetNumberNaturalKnots` sums
//! multiplicities the size of the knots only when the cached arrays agree;
//! `CalculateContinuities` needs a break; `GenerateMults` at most as many knots as
//! breaks), no `Copy` of an owned Hermite base curve (its `Copy` fails and both objects
//! would delete it), and database text without a volume type code (`22000..=22999`:
//! reading those needs volume data, and `SmVolume_TYPE` dereferences a null volume).
//! For `Exact` it also needs finite line ends, circles with independent axes, a
//! positive radius and `-360 <= a0 < a1 <= a0 + 360`, at most [`MAX_MAPS`] maps with
//! non-zero component axes, and no Hermite base curve under a twist head map (the twist
//! copies the curve it maps, a Hermite curve's `Copy` fails and upstream then
//! dereferences the `NULL` copy).

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
/// Most breaks, knots or type codes.
pub const MAX_ARRAY: usize = 1024;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 16;
/// Most maps of an `Exact` volume chain.
pub const MAX_MAPS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Info,
    Eval,
    EvalPoint,
    Knots,
    Mults,
    Nnk,
    Conts,
    Equal,
    Copy,
    EditParam,
    Reverse,
    Trim,
    Transform,
    ReadDb,
    Exact,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Info => 0,
            Op::Eval => 1,
            Op::EvalPoint => 2,
            Op::Knots => 3,
            Op::Mults => 4,
            Op::Nnk => 5,
            Op::Conts => 6,
            Op::Equal => 7,
            Op::Copy => 8,
            Op::EditParam => 9,
            Op::Reverse => 10,
            Op::Trim => 11,
            Op::Transform => 12,
            Op::ReadDb => 40,
            Op::Exact => 41,
        }
    }

    /// Whether the op takes no object block.
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 40
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_crv_in_volume_eval(
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

impl Reader<'_> {
    fn int(&mut self, min: i64, max: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < min as f64 || v > max as f64 {
            return Err(bad(format!("bad integer {v}")));
        }
        Ok(v as i64)
    }
}

/// A B-spline `n p m cpts knots`; returns its parameter range.
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
    Ok((a, b))
}

/// A validated object block.
#[derive(Debug, Clone, Copy)]
struct Block {
    bspline: bool,
    a: f64,
    b: f64,
    owner_flag: u64,
    /// `Some((nb, nc))` when the cache is loaded and current (`need == FALSE`).
    cached: Option<(usize, usize)>,
}

/// A parameter within [`MAX_REL_DIST`] ranges of the curve's.
fn near(t: f64, a: f64, b: f64) -> Result<f64, FfiError> {
    let far = MAX_REL_DIST * (b - a);
    if t < a - far || t > b + far {
        return Err(bad("parameter too far outside the curve"));
    }
    Ok(t)
}

/// An object block (see the module docs).
fn object_block(rd: &mut Reader<'_>) -> Result<Block, FfiError> {
    let kind = rd.index(1)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    let (bspline, a, b) = if kind == 0 {
        rd.flag()?;
        let (a, b) = bspline(rd)?;
        (true, a, b)
    } else {
        for _ in 0..4 {
            rd.vec3()?;
        }
        (false, 0.0, 1.0)
    };
    rd.flag()?;
    let copy_flag = rd.index(3)? as u64;
    let mut owner_flag = rd.index(7)? as u64;
    if copy_flag == 1 || copy_flag == 3 {
        owner_flag |= 1;
    }
    if copy_flag == 2 || copy_flag == 3 {
        owner_flag |= 2;
    }
    let mut cached = None;
    if rd.flag()? {
        let need = rd.flag()?;
        let nb = rd.index(MAX_ARRAY)?;
        for _ in 0..nb {
            let t = rd.finite()?;
            near(t, a, b)?;
        }
        let nc = rd.index(MAX_ARRAY)?;
        for _ in 0..nc {
            rd.int(0, 13)?;
        }
        if !need {
            cached = Some((nb, nc));
        }
    }
    Ok(Block {
        bspline,
        a,
        b,
        owner_flag,
        cached,
    })
}

fn param(rd: &mut Reader<'_>, c: &Block) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    near(t, c.a, c.b)
}

fn validate_object_op(op: Op, rd: &mut Reader<'_>, c: &Block) -> Result<(), FfiError> {
    match op {
        Op::Info => {
            let nt = rd.index(MAX_ARRAY)?;
            for _ in 0..nt {
                rd.int(-(1 << 30), 1 << 30)?;
            }
        }
        Op::Eval => {
            param(rd, c)?;
            rd.index(5)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::EvalPoint => {
            param(rd, c)?;
        }
        Op::Knots => {
            rd.flag()?;
            param(rd, c)?;
            param(rd, c)?;
            rd.flag()?;
        }
        Op::Mults => {
            let nk = rd.index(MAX_ARRAY)?;
            for _ in 0..nk {
                param(rd, c)?;
            }
            if let Some((nb, _)) = c.cached {
                if nk > nb {
                    return Err(bad("mults: more knots than breaks"));
                }
            }
        }
        Op::Nnk => {
            if let Some((nb, nc)) = c.cached {
                if nb != nc {
                    return Err(bad("nnk: cached breaks and continuities differ in size"));
                }
            }
        }
        Op::Conts => {
            let a = rd.finite()?;
            if !(0.0..=90.0).contains(&a) {
                return Err(bad("angle out of range"));
            }
            if let Some((nb, _)) = c.cached {
                if nb == 0 {
                    return Err(bad("conts: no cached breaks"));
                }
            }
        }
        Op::Equal => {
            if !rd.flag()? {
                object_block(rd)?;
            }
        }
        Op::Copy => {
            let owns_curve = c.owner_flag == 1 || c.owner_flag == 3;
            if !c.bspline && owns_curve {
                return Err(bad("copy: an owned Hermite base curve cannot be copied"));
            }
        }
        Op::Reverse | Op::Trim => {
            param(rd, c)?;
            param(rd, c)?;
            if op == Op::Trim {
                rd.index(2)?;
            }
        }
        Op::EditParam => {
            let x = param(rd, c)?;
            let y = param(rd, c)?;
            if (y - x).abs() < 1e-6 * (c.b - c.a) {
                return Err(bad("degenerate new parameterisation"));
            }
        }
        Op::Transform => {
            for _ in 0..3 {
                rd.vec3()?;
            }
            rd.flag()?;
            rd.vec3()?;
        }
        Op::ReadDb | Op::Exact => return Err(bad("not an object op")),
    }
    Ok(())
}

/// A non-zero vector.
fn nonzero3(rd: &mut Reader<'_>) -> Result<[f64; 3], FfiError> {
    let v = rd.vec3()?;
    if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
        return Err(bad("zero vector"));
    }
    Ok(v)
}

/// An `Exact` curve block (see the module docs); returns whether it is a Hermite curve.
fn exact_curve(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let kind = rd.index(3)?;
    match kind {
        0 | 1 => {
            let dim = rd.index(3)?;
            if dim < 2 {
                return Err(bad("curve block: dimension must be 2 or 3"));
            }
            if kind == 0 {
                rd.flag()?;
                bspline(rd)?;
            } else {
                for _ in 0..4 {
                    rd.vec3()?;
                }
            }
            Ok(kind == 1)
        }
        2 => {
            rd.vec3()?;
            rd.vec3()?;
            Ok(false)
        }
        _ => {
            rd.vec3()?;
            let x = nonzero3(rd)?;
            let y = nonzero3(rd)?;
            let cross = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            let c2: f64 = cross.iter().map(|c| c * c).sum();
            let l2: f64 =
                x.iter().map(|c| c * c).sum::<f64>() * y.iter().map(|c| c * c).sum::<f64>();
            if c2 < 1e-6 * l2 {
                return Err(bad("circle: axes parallel"));
            }
            let (a0, a1) = (rd.finite()?, rd.finite()?);
            let r = rd.finite()?;
            if r < 1e-9 {
                return Err(bad("circle: need a positive radius"));
            }
            if a0 < -360.0 || a1 > a0 + 360.0 || a1 <= a0 || a1 > 360.0 {
                return Err(bad("circle: need -360 <= a0 < a1 <= a0 + 360"));
            }
            Ok(false)
        }
    }
}

/// An `Exact` volume chain (see the module docs); returns whether the head map is a
/// twist volume.
fn volume(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let n = rd.index(MAX_MAPS)?;
    if n == 0 {
        return Err(bad("volume without maps"));
    }
    let mut head_twist = false;
    for i in 0..n {
        let kind = rd.index(7)?;
        if kind == 1 {
            for _ in 0..4 {
                rd.vec3()?;
            }
        }
        let component = kind >= 5;
        if component {
            // origin / center, axis and mid direction (non-zero), then two numbers
            rd.vec3()?;
            nonzero3(rd)?;
            nonzero3(rd)?;
            rd.finite()?;
            rd.finite()?;
        }
        if i == 0 {
            head_twist = kind == 2 || kind == 5;
        }
        if rd.flag()? {
            if component {
                return Err(bad("component volume with an orient map"));
            }
            for _ in 0..4 {
                rd.vec3()?;
            }
        }
    }
    Ok(head_twist)
}

/// Whether `bytes` holds a digit run (as `operator>>` would extract it) in
/// `22000..=22999`, the volume type codes.
fn has_volume_type(bytes: &[u8]) -> bool {
    let mut value: u64 = 0;
    let mut in_run = false;
    for &c in bytes.iter().chain(core::iter::once(&b' ')) {
        if c.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add(u64::from(c - b'0'));
            in_run = true;
        } else {
            if in_run && (22000..=22999).contains(&value) {
                return true;
            }
            value = 0;
            in_run = false;
        }
    }
    false
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::ReadDb => {
            rd.int(0, 1 << 20)?;
            let nb = rd.index(MAX_DB_BYTES)?;
            let mut bytes = Vec::with_capacity(nb);
            for _ in 0..nb {
                bytes.push(rd.index(255)? as u8);
            }
            if has_volume_type(&bytes) {
                return Err(bad("read: the text names a volume type"));
            }
        }
        Op::Exact => {
            let hermite = exact_curve(rd)?;
            rd.flag()?;
            let head_twist = volume(rd)?;
            if hermite && head_twist {
                return Err(bad("exact: a twist copies the curve; a Hermite copy fails"));
            }
        }
        _ => return Err(bad("not a static op")),
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
    if op.is_static() {
        validate_static(op, &mut rd)?;
    } else {
        let c = object_block(&mut rd)?;
        validate_object_op(op, &mut rd, &c)?;
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
        // reads for `op` (validated above: well-formed curve blocks, every index, count,
        // flag and continuity code the shim passes to upstream, cache shapes upstream
        // indexes safely, no volume type in database text, exact curve and volume
        // blocks upstream builds and maps safely); buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_crv_in_volume_eval(
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
