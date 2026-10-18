//! FFI wrappers for unit `nurbs.b_spline_curve_p1`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_curve_p1.cpp` (`sc_nurbs_b_spline_curve_p1_eval`).
//! Upstream: `source/SMLib/src/SmBSplineCurve.cpp`, `source/SMLib/inc/SmBSplineCurve.h`.
//!
//! The input starts with a curve block saying how `this` is built: `ctor`, then for
//! `ctor != 0` `dim oob`, then for `ctor >= 2` a curve. `ctor` is one of [`Ctor`]:
//! `0` none (static ops), `1` `SmBSplineCurve(lDim)` (no NURB), `2` / `3` the fast
//! constructor with an owned / borrowed NURB, `4` the `(lDim, n, m, p, Pw, U)`
//! constructor, `5` `(lDim, const gw_CURVE*)`, `6` the copy constructor of a `2`
//! curve. A curve is `n p m`, `n+1` control points `x y z w`, `m+1` knots. The op's
//! parameters follow:
//!
//! | [`Op`]           | upstream                       | parameters                               |
//! |------------------|--------------------------------|------------------------------------------|
//! | `State`          | (none)                         |                                          |
//! | `Evaluate`       | `Evaluate`                     | `t nd from_left non_zero`                |
//! | `EvaluatePoint`  | `EvaluatePoint`                | `t`                                      |
//! | `Intervals`      | `GetNaturalInterval`, `GetSTEPInterval` |                                 |
//! | `Knots`          | `GetKnots`                     | `has_ivl a b`                            |
//! | `ControlPolygon` | `GetControlPolygon`            |                                          |
//! | `ControlPoints`  | `GetControlPointsPointer`      |                                          |
//! | `Ends`           | `GetEnds`                      |                                          |
//! | `Counts`         | `GetDegree`, `GetNumber*`, `IsRational`, `GetInsideOut`, `GetOutOfBoundsEnabled` |   |
//! | `Greville`       | `GetGrevilleAbscissa`          | `index`                                  |
//! | `Convert2d/3d`   | `ConvertTo2D`, `ConvertTo3D`   |                                          |
//! | `EditParam`      | `EditParameterization`         | `a b notify`                             |
//! | `EditEnd`        | `EditEndPoint`                 | `x y z start has_w w`                    |
//! | `Fixup`          | `FixupKnotVector`              | `tol`                                    |
//! | `RemoveExtra`    | `RemoveExtraKnots`             | `tol`                                    |
//! | `SetFromGw`      | `SetFromGwNurb`                | `has` [curve]                            |
//! | `Transform`      | `Transform`                    | frame `has_scale sx sy sz`               |
//! | `Mirror`         | `CreateMirrorCurve`            | frame                                    |
//! | `Projection`     | `CreatePlaneProjection`        | `type px py pz nx ny nz ax ay az`        |
//! | `Eq`             | `operator==`                   | a second curve block                     |
//! | `IsKindOf`       | `IsKindOf`                     | `type`                                   |
//! | `Type`           | `GetType`, `GetClassType`      |                                          |
//! | `Continuities`   | `CalculateContinuities`        | `angle_tol`                              |
//! | `Tessellate`     | `Tessellate`                   | `a b chord angle min_segments`           |
//! | `MakeNurb`       | `MakeNurb`                     |                                          |
//! | `Copy`           | `Copy`                         |                                          |
//! | `Canonical`      | `CreateCanonical`              | `dim deg ncp P[3 ncp] form nk K[nk] M[nk] kt has_w [nw W[nw]] has_trim a b` |
//! | `Circle`         | `CreateCircleSegment`          | `dim` frame `r a0 a1 param`              |
//! | `Arc`            | `CreateArcFromPoints`          | `C[3] P1[3] P2[3] param`                 |
//! | `Degenerate`     | `CreateDegenerateCurve`        | `dim P[3]`                               |
//! | `Line`           | `CreateLineSegment`            | `dim P0[3] P1[3]`                        |
//! | `Fair`           | `FairBlendCurveDerivatives`    | `np P[3 np] nt T[3 nt] has_h nh H[3 nh] prevent` |
//!
//! A frame is `O[3] X[3] Y[3]` (`SmAxis2Placement::SetCanonical`; its failure status
//! is the op status). The last eight ops are static (`ctor == 0`); the others need a
//! curve. The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`), the
//! op's outputs on success, then for curve ops the state of `this`; ops creating a
//! curve output its state on success. A state is `dim oob borrowed knot_type form
//! has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: dimension
//! 1 to 3, well-formed clamped curves (`m = n+p+1`, `1 <= p <= n`, `p <= 17`, finite
//! data, consistent dimension and rationality, non-decreasing knots with end
//! multiplicity exactly `p+1` and interior multiplicity at most `p`), a curve without
//! NURB (`ctor == 1`) only for the ops that handle one (`State`, `Intervals`,
//! `EditParam`, `RemoveExtra`, `SetFromGw`, `Eq`, `IsKindOf`, `Type`, `MakeNurb`,
//! `Copy`), `Evaluate` derivative counts up to 8 except 5 (`sm_CrvDerivs` overruns its
//! arrays there), `CreateCanonical` data whose expanded knots cover `ncp + deg + 1`
//! entries once dimension, degree and weight count pass upstream's checks, and
//! finite values everywhere.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;

/// How `this` is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctor {
    /// No curve (static ops).
    None = 0,
    /// `SmBSplineCurve(lDim)`: no NURB.
    Empty = 1,
    /// `SmBSplineCurve(gw_CURVE*, lDim, FALSE)`.
    FastOwned = 2,
    /// `SmBSplineCurve(gw_CURVE*, lDim, TRUE)`.
    FastBorrowed = 3,
    /// `SmBSplineCurve(lDim, n, m, p, Pw, U)`.
    Data = 4,
    /// `SmBSplineCurve(lDim, const gw_CURVE*)`.
    GwCopy = 5,
    /// The copy constructor of a [`Ctor::FastOwned`] curve.
    Copy = 6,
}

impl Ctor {
    /// The code in the input.
    #[must_use]
    pub fn code(self) -> f64 {
        f64::from(self as i32)
    }
}

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    State,
    Evaluate,
    EvaluatePoint,
    Intervals,
    Knots,
    ControlPolygon,
    ControlPoints,
    Ends,
    Counts,
    Greville,
    Convert2d,
    Convert3d,
    EditParam,
    EditEnd,
    Fixup,
    RemoveExtra,
    SetFromGw,
    Transform,
    Mirror,
    Projection,
    Eq,
    IsKindOf,
    Type,
    Continuities,
    Tessellate,
    MakeNurb,
    Copy,
    Canonical,
    Circle,
    Arc,
    Degenerate,
    Line,
    Fair,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::State => 0,
            Op::Evaluate => 1,
            Op::EvaluatePoint => 2,
            Op::Intervals => 3,
            Op::Knots => 4,
            Op::ControlPolygon => 5,
            Op::ControlPoints => 6,
            Op::Ends => 7,
            Op::Counts => 8,
            Op::Greville => 9,
            Op::Convert2d => 10,
            Op::Convert3d => 11,
            Op::EditParam => 12,
            Op::EditEnd => 13,
            Op::Fixup => 14,
            Op::RemoveExtra => 15,
            Op::SetFromGw => 16,
            Op::Transform => 17,
            Op::Mirror => 18,
            Op::Projection => 19,
            Op::Eq => 20,
            Op::IsKindOf => 21,
            Op::Type => 22,
            Op::Continuities => 23,
            Op::Tessellate => 24,
            Op::MakeNurb => 25,
            Op::Copy => 26,
            Op::Canonical => 30,
            Op::Circle => 31,
            Op::Arc => 32,
            Op::Degenerate => 33,
            Op::Line => 34,
            Op::Fair => 35,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        matches!(
            self,
            Op::Canonical | Op::Circle | Op::Arc | Op::Degenerate | Op::Line | Op::Fair
        )
    }

    /// Whether the op handles a curve without NURB.
    #[must_use]
    pub fn handles_empty(self) -> bool {
        matches!(
            self,
            Op::State
                | Op::Intervals
                | Op::EditParam
                | Op::RemoveExtra
                | Op::SetFromGw
                | Op::Eq
                | Op::IsKindOf
                | Op::Type
                | Op::MakeNurb
                | Op::Copy
        )
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_curve_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<(), FfiError> {
        for _ in 0..k {
            self.finite()?;
        }
        Ok(())
    }
}

/// Reads and validates a curve `n p m cpts knots`; returns `n`.
fn curve(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let mut pts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        pts.push([rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?]);
    }
    let mut knots = Vec::with_capacity(m + 1);
    for _ in 0..=m {
        knots.push(rd.finite()?);
    }
    let [_, _, z0, w0] = *pts.first().ok_or_else(|| bad("empty curve"))?;
    for &[_, _, z, w] in &pts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
    }
    if knots.windows(2).any(|w| w.first() > w.last()) {
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
    Ok(n)
}

/// Reads a curve block; returns `(ctor, n)` (`n` of the NURB, if any).
fn curve_block(rd: &mut Reader<'_>) -> Result<(usize, Option<usize>), FfiError> {
    let ctor = rd.index(6)?;
    if ctor == 0 {
        return Ok((0, None));
    }
    let dim = rd.index(3)?;
    if dim == 0 {
        return Err(bad("dimension 0"));
    }
    rd.flag()?;
    let n = if ctor == 1 { None } else { Some(curve(rd)?) };
    Ok((ctor, n))
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Canonical => {
            let dim = rd.index(8)?;
            let deg = rd.index(40)?;
            let ncp = rd.index(MAX_INDEX)?;
            rd.finites(3 * ncp)?;
            rd.index(6)?;
            let nk = rd.index(MAX_INDEX)?;
            rd.finites(nk)?;
            let mut total = 0usize;
            for _ in 0..nk {
                total += rd.index(64)?;
            }
            rd.index(4)?;
            let weights_ok = if rd.flag()? {
                let nw = rd.index(MAX_INDEX)?;
                rd.finites(nw)?;
                nw == ncp
            } else {
                true
            };
            rd.flag()?;
            rd.finites(2)?;
            let checked = (1..=3).contains(&dim) && (1..=32).contains(&deg) && weights_ok;
            if checked && (ncp == 0 || total < ncp + deg + 1) {
                return Err(bad("canonical: need ncp >= 1 and ncp + deg + 1 knots"));
            }
        }
        Op::Circle => {
            rd.index(8)?;
            rd.finites(12)?;
            rd.flag()?;
        }
        Op::Arc => {
            rd.finites(9)?;
            rd.flag()?;
        }
        Op::Degenerate => {
            rd.index(8)?;
            rd.finites(3)?;
        }
        Op::Line => {
            rd.index(8)?;
            rd.finites(6)?;
        }
        Op::Fair => {
            let np = rd.index(4)?;
            rd.finites(3 * np)?;
            let nt = rd.index(4)?;
            rd.finites(3 * nt)?;
            rd.flag()?;
            let nh = rd.index(6)?;
            rd.finites(3 * nh)?;
            rd.flag()?;
        }
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

fn validate_curve_op(op: Op, rd: &mut Reader<'_>, n: Option<usize>) -> Result<(), FfiError> {
    if n.is_none() && !op.handles_empty() {
        return Err(bad("op needs a NURB"));
    }
    match op {
        Op::State
        | Op::Intervals
        | Op::ControlPolygon
        | Op::ControlPoints
        | Op::Ends
        | Op::Counts
        | Op::Convert2d
        | Op::Convert3d
        | Op::Type
        | Op::MakeNurb
        | Op::Copy => {}
        Op::Evaluate => {
            rd.finite()?;
            if rd.index(8)? == 5 {
                return Err(bad("evaluate: 5 derivatives overrun sm_CrvDerivs"));
            }
            rd.flag()?;
            rd.flag()?;
        }
        Op::EvaluatePoint | Op::Continuities => {
            rd.finite()?;
        }
        Op::Knots => {
            rd.flag()?;
            rd.finites(2)?;
        }
        Op::Greville => {
            rd.index(n.unwrap_or(0) + 3)?;
        }
        Op::EditParam => {
            rd.finites(2)?;
            rd.flag()?;
        }
        Op::EditEnd => {
            rd.finites(3)?;
            rd.flag()?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::Fixup | Op::RemoveExtra => {
            if rd.finite()? < 0.0 {
                return Err(bad("negative tolerance"));
            }
        }
        Op::SetFromGw => {
            if rd.flag()? {
                curve(rd)?;
            }
        }
        Op::Transform => {
            rd.finites(9)?;
            rd.flag()?;
            rd.finites(3)?;
        }
        Op::Mirror => rd.finites(9)?,
        Op::Projection => {
            rd.index(3)?;
            rd.finites(9)?;
        }
        Op::Eq => {
            let (ctor, _) = curve_block(rd)?;
            if ctor == 0 {
                return Err(bad("eq: the other curve needs a constructor"));
            }
        }
        Op::IsKindOf => {
            rd.index(1 << 20)?;
        }
        Op::Tessellate => {
            rd.finites(2)?;
            let chord = rd.finite()?;
            let angle = rd.finite()?;
            if !(chord > 0.0 || angle > 0.0) || chord < 0.0 || angle < 0.0 {
                return Err(bad("tessellate: need a positive tolerance"));
            }
            rd.index(1000)?;
        }
        _ => return Err(bad("not a curve op")),
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let (ctor, n) = curve_block(&mut rd)?;
    if (ctor == 0) != op.is_static() {
        return Err(bad("static ops need ctor 0, curve ops a constructor"));
    }
    if ctor == 0 {
        validate_static(op, &mut rd)?;
    } else {
        validate_curve_op(op, &mut rd, n)?;
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
        // reads for `op` (validated above: curve blocks with well-formed clamped curves,
        // every index, count and flag the shim passes to upstream, and NURB-less curves
        // only for ops that handle them); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_b_spline_curve_p1_eval(
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
