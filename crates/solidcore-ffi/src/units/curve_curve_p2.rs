//! FFI wrappers for unit `curve.curve_p2`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p2.cpp` (`sc_curve_curve_p2_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! Curve ops start with a curve block, the curve `c` the op runs on through `SmCurve*`:
//! `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built with
//! `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`) or
//! `1 dim P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`). Static ops have none. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest
//! (on success unless noted):
//!
//! | [`Op`]           | upstream                                  | parameters               | outputs |
//! |------------------|-------------------------------------------|--------------------------|---------|
//! | `Info`           | `GetType`, `GetDim`, `GetDegree`, `GetNumberNaturalKnots`, `GetNumberControlPoints`, `GetNaturalInterval`, `IsAnalytic`, `IsBounded`, `IsRational`, `GetOwner`, `IsKindOf` | | `type dim deg nnk ncp a b analytic bounded rational no_owner kind[9]` |
//! | `Eval`           | `Evaluate`                                | `t n left nonzero`       | `(n+1)` vectors |
//! | `EvalPoint`      | `EvaluatePoint`                           | `t`                      | point |
//! | `Ends`           | `GetEnds`                                 |                          | `P0 P1 w0 w1` |
//! | `Knots`          | `GetKnots`                                | `has_ivl a b`            | `nk K nm M` |
//! | `Continuities`   | `CalculateContinuities`                   | `angle`                  | `min nk C` |
//! | `ApproxLength`   | `ApproximateLength`                       | `a b samples want_turn`  | `len has_turn [turn]` |
//! | `EvalStep`       | `EvaluateSTEP`                            | `t n left`               | `(n+1)` vectors |
//! | `EvalGeom`       | `EvaluateGeometric`                       | `t n left`               | `(n+1)` vectors |
//! | `EvalContinuity` | `EvaluateContinuity`                      | `t left` curve block `t2 left2 angle` | `type this[9] other[9]` |
//! | `StepOff`        | `EuclidianStepOff`                        | `a b start same dist tol`| `clamped end` (also on `SM_ERR`) |
//! | `Split`          | `CreateBySplitCurve`                      | `t`                      | state of `c`, of the new curve |
//! | `ConvertT`       | `ConvertTFromNURBSToSTEP`, `ConvertTFromSTEPToNURBS` | `t`           | `step nurbs` (always) |
//! | `Repeated`       | `HasRepeatedEndControlPoints`             | `tol`                    | flag |
//! | `Equal`          | `operator==`                              | `self` [curve block]     | flag |
//! | `Copy`           | `Copy`                                    |                          | state |
//! | `Trim`           | `Trim`                                    | `a b`                    | `a' b'` state (always) |
//! | `Transform`      | `Transform`                               | `O X Y has_scale S`      | state (always) |
//! | `EditParam`      | `EditParameterization`                    | `a b`                    | state (always) |
//! | `Reverse`        | `ReverseParameterization`                 | `a b`                    | `[a' b']` state (always) |
//! | `Convert2d`      | `ConvertTo2D`                             |                          | state (always) |
//! | `Analytic`       | `CreateAnalyticCurve`                     | `a b`                    | made |
//! | `StepInversion`  | `STEPInversion`                           | `P`                      | |
//! | `DropFast`       | `DropPointFast`                           | `P`                      | |
//! | `IntersectLine`  | `IntersectWithLine`                       |                          | `more nsol` (always) |
//! | `Snap`           | `sm_SnapSolToSpecifiedEndpoint`           | `a b has_vec V sol tol`  | `sol good` |
//! | `LengthIfeo`     | `SmLengthIFEO::Evaluate` (B-spline only)  | `t`                      | value |
//! | `ReadDb`         | `SmCurve::ReadFromDB` (static, ASCII)     | `type dim version nb B[nb]` | `made [state]` (always) |
//! | `Flush`          | `sm_FlushCrv` (static)                    | `tol analytics nk K np P[3 np]` | `made [state] nk K np P` (always) |
//! | `AddToCurve`     | `sm_AddToCurve` (static)                  | `L0 L1 C0 C1 lt ct nk K np P` | `nk K np P` (always) |
//! | `Stack`          | `sm_PushPoint`, `sm_PopStack`, `sm_StackLook` | `ns S nops ops` (`0 t P T`, `1`, `2`) | per op `status [t P T] size` |
//! | `ParamDelta`     | `sm_ConvertToParameterDelta` (static)     | `T D`                    | value |
//! | `UnitizeTan`     | `sm_UnitizeTangent` (static)              | `P D1 D2 G`              | `U len` (always) |
//! | `ProjAngle`      | `sm_GetProjSignedAngle` (static)          | `P1 P2 C N S`            | angle |
//! | `TestParams`     | `sm_FindTestParams` (static)              | `a b deg nk K`           | `n params` |
//! | `TestStep`       | `sm_TestStep`                             | `t step C0 C1 N0 N1 chord ang dsq` | result |
//! | `PropertyGs`, `CcGs`, `CpGs` | `SmCurvePropertyGS`, `SmCCGlobalSolver`, `SmCPGlobalSolver` | trees, `op tol best nopt opt`, [`prop has_val val`], `which` nodes | `flag` or `idx swap` |
//!
//! A state is `0 dim n p m cpts knots` (B-spline), `1 dim P(0) C'(0) P(1) C'(1)` (Hermite,
//! evaluated back) or `-1`. A tree is `nn` nodes `parent c1 c2 aux has_data min[3]
//! max[3]` (node 0 the top; `has_data` points `m_pData` at a dummy the tests never read).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, parameters within a few curve
//! lengths of the curve's range, derivative counts up to 3, and solver branches that
//! never reach the (dummy) curve span data.

use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 512;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 4.0;
/// Most `ApproximateLength` samples.
pub const MAX_SAMPLES: usize = 400;
/// Most knots / points of the static array ops.
pub const MAX_ARRAY: usize = 1024;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 16;
/// Most tree nodes.
pub const MAX_NODES: usize = 16;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Info,
    Eval,
    EvalPoint,
    Ends,
    Knots,
    Continuities,
    ApproxLength,
    EvalStep,
    EvalGeom,
    EvalContinuity,
    StepOff,
    Split,
    ConvertT,
    Repeated,
    Equal,
    Copy,
    Trim,
    Transform,
    EditParam,
    Reverse,
    Convert2d,
    Analytic,
    StepInversion,
    DropFast,
    IntersectLine,
    Snap,
    LengthIfeo,
    ReadDb,
    Flush,
    AddToCurve,
    Stack,
    ParamDelta,
    UnitizeTan,
    ProjAngle,
    TestParams,
    TestStep,
    PropertyGs,
    CcGs,
    CpGs,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Info => 0,
            Op::Eval => 1,
            Op::EvalPoint => 2,
            Op::Ends => 3,
            Op::Knots => 4,
            Op::Continuities => 5,
            Op::ApproxLength => 6,
            Op::EvalStep => 7,
            Op::EvalGeom => 8,
            Op::EvalContinuity => 9,
            Op::StepOff => 10,
            Op::Split => 11,
            Op::ConvertT => 12,
            Op::Repeated => 13,
            Op::Equal => 14,
            Op::Copy => 15,
            Op::Trim => 16,
            Op::Transform => 17,
            Op::EditParam => 18,
            Op::Reverse => 19,
            Op::Convert2d => 20,
            Op::Analytic => 21,
            Op::StepInversion => 22,
            Op::DropFast => 23,
            Op::IntersectLine => 24,
            Op::Snap => 25,
            Op::LengthIfeo => 26,
            Op::ReadDb => 40,
            Op::Flush => 41,
            Op::AddToCurve => 42,
            Op::Stack => 43,
            Op::ParamDelta => 44,
            Op::UnitizeTan => 45,
            Op::ProjAngle => 46,
            Op::TestParams => 47,
            Op::TestStep => 48,
            Op::PropertyGs => 49,
            Op::CcGs => 50,
            Op::CpGs => 51,
        }
    }

    /// Whether the op takes no curve block.
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 40
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p2_eval(
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

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    /// A signed integer in `min..=max`.
    fn int(&mut self, min: i64, max: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < min as f64 || v > max as f64 {
            return Err(bad(format!("bad integer {v}")));
        }
        Ok(v as i64)
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

/// A validated curve block.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    bspline: bool,
    a: f64,
    b: f64,
}

fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// A B-spline `n p m cpts knots`.
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

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(1)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    if kind == 0 {
        rd.flag()?;
        let (a, b) = bspline(rd)?;
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

/// An interval `a b` (any order) of parameters near the curve's range.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(f64, f64), FfiError> {
    Ok((param(rd, c)?, param(rd, c)?))
}

fn derivs(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    rd.index(3)
}

fn validate_curve_op(op: Op, rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(), FfiError> {
    match op {
        Op::Info
        | Op::Ends
        | Op::Copy
        | Op::Convert2d
        | Op::IntersectLine
        | Op::StepInversion
        | Op::DropFast => {
            if matches!(op, Op::StepInversion | Op::DropFast) {
                rd.vec3()?;
            }
        }
        Op::Eval => {
            param(rd, c)?;
            derivs(rd)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::EvalPoint | Op::Split | Op::ConvertT => {
            param(rd, c)?;
        }
        Op::LengthIfeo => {
            if !c.bspline {
                return Err(bad("LengthIfeo needs a B-spline"));
            }
            param(rd, c)?;
        }
        Op::Knots => {
            rd.flag()?;
            interval(rd, c)?;
        }
        Op::Continuities => {
            let a = rd.finite()?;
            if !(0.0..=90.0).contains(&a) {
                return Err(bad("angle out of range"));
            }
        }
        Op::ApproxLength => {
            interval(rd, c)?;
            rd.index(MAX_SAMPLES)?;
            rd.flag()?;
        }
        Op::EvalStep | Op::EvalGeom => {
            param(rd, c)?;
            derivs(rd)?;
            rd.flag()?;
        }
        Op::EvalContinuity => {
            param(rd, c)?;
            rd.flag()?;
            let other = curve_block(rd)?;
            param(rd, &other)?;
            rd.flag()?;
            let a = rd.finite()?;
            if !(0.0..=90.0).contains(&a) {
                return Err(bad("angle out of range"));
            }
        }
        Op::StepOff => {
            interval(rd, c)?;
            param(rd, c)?;
            rd.flag()?;
            rd.finite()?;
            let tol = rd.finite()?;
            if tol <= 0.0 {
                return Err(bad("tolerance must be positive"));
            }
        }
        Op::Repeated => {
            let tol = rd.finite()?;
            if tol < 0.0 {
                return Err(bad("tolerance must not be negative"));
            }
        }
        Op::Equal => {
            if !rd.flag()? {
                curve_block(rd)?;
            }
        }
        Op::Trim | Op::Reverse | Op::Analytic => {
            interval(rd, c)?;
        }
        Op::EditParam => {
            let (a, b) = interval(rd, c)?;
            if (b - a).abs() < 1e-6 * (c.b - c.a) {
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
        Op::Snap => {
            interval(rd, c)?;
            rd.flag()?;
            rd.vec3()?;
            param(rd, c)?;
            rd.finite()?;
        }
        _ => return Err(bad("not a curve op")),
    }
    Ok(())
}

fn knots_and_points(rd: &mut Reader<'_>) -> Result<(usize, usize), FfiError> {
    let nk = rd.index(MAX_ARRAY)?;
    for _ in 0..nk {
        rd.finite()?;
    }
    let np = rd.index(MAX_ARRAY)?;
    for _ in 0..np {
        rd.vec3()?;
    }
    Ok((nk, np))
}

/// A tree; returns its node count. `curve_aux_with_data` is set when some node with
/// data has a curve aux type (`SM_AD_BEZIER_CURVE`/`SM_AD_CURVE_DATA`).
fn tree(rd: &mut Reader<'_>) -> Result<Vec<(usize, bool)>, FfiError> {
    let nn = rd.index(MAX_NODES)?;
    if nn == 0 {
        return Err(bad("empty tree"));
    }
    let mut nodes = Vec::with_capacity(nn);
    for k in 0..nn {
        let par = rd.int(-1, nn as i64 - 1)?;
        let c1 = rd.int(-1, nn as i64 - 1)?;
        let c2 = rd.int(-1, nn as i64 - 1)?;
        let leaf = c1 == -1 && c2 == -1;
        let inner = c1 > k as i64 && c2 > k as i64;
        if !leaf && !inner {
            return Err(bad("tree: children must follow their parent"));
        }
        let _ = par;
        let aux = rd.index(8)?;
        let data = rd.flag()?;
        let mut v = [0.0; 6];
        for x in &mut v {
            *x = rd.finite()?;
        }
        if v[0] > v[3] || v[1] > v[4] || v[2] > v[5] {
            return Err(bad("tree: inverted box"));
        }
        nodes.push((aux, data));
    }
    Ok(nodes)
}

fn is_curve_aux(aux: usize) -> bool {
    aux == 1 || aux == 2
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::ReadDb => {
            rd.int(0, 1 << 20)?;
            let dim = rd.index(3)?;
            if dim < 2 {
                return Err(bad("dimension must be 2 or 3"));
            }
            rd.index(64)?;
            let nb = rd.index(MAX_DB_BYTES)?;
            for _ in 0..nb {
                rd.index(255)?;
            }
        }
        Op::Flush => {
            rd.finite()?;
            rd.flag()?;
            let (nk, np) = knots_and_points(rd)?;
            if nk > 0 && np != 3 * (nk - 1) + 1 {
                return Err(bad("flush: polygon size must be 3 (knots - 1) + 1"));
            }
            if nk == 1 {
                return Err(bad("flush: a single knot makes no curve"));
            }
        }
        Op::AddToCurve => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            rd.finite()?;
            rd.finite()?;
            let (nk, np) = knots_and_points(rd)?;
            if nk > 0 && np == 0 {
                return Err(bad("add: knots without points"));
            }
        }
        Op::Stack => {
            let ns = rd.index(MAX_ARRAY)?;
            for _ in 0..ns {
                rd.finite()?;
            }
            let nops = rd.index(64)?;
            for _ in 0..nops {
                if rd.index(2)? == 0 {
                    rd.finite()?;
                    rd.vec3()?;
                    rd.vec3()?;
                }
            }
        }
        Op::ParamDelta => {
            rd.vec3()?;
            rd.vec3()?;
        }
        Op::UnitizeTan => {
            for _ in 0..4 {
                rd.vec3()?;
            }
        }
        Op::ProjAngle => {
            for _ in 0..5 {
                rd.vec3()?;
            }
        }
        Op::TestParams => {
            rd.finite()?;
            rd.finite()?;
            rd.index(32)?;
            let nk = rd.index(MAX_ARRAY)?;
            for _ in 0..nk {
                rd.finite()?;
            }
        }
        Op::TestStep => {
            rd.finite()?;
            rd.finite()?;
            for _ in 0..4 {
                rd.vec3()?;
            }
            for _ in 0..3 {
                rd.finite()?;
            }
        }
        Op::PropertyGs | Op::CcGs | Op::CpGs => validate_solver(op, rd)?,
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

/// Solver operations the tests use (all handled by the base test without owners).
const SOLVER_OPS: [i64; 6] = [0, 2, 3, 4, 5, 22];

fn validate_solver(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let ntrees = if op == Op::PropertyGs { 1 } else { 2 };
    let mut trees = Vec::new();
    for _ in 0..ntrees {
        trees.push(tree(rd)?);
    }
    let sop = rd.int(0, 30)?;
    if !SOLVER_OPS.contains(&sop) {
        return Err(bad("solver operation not supported by the tests"));
    }
    rd.finite()?;
    let best = rd.num()?;
    if !(best.is_finite() && (0.0..=1e40).contains(&best)) {
        return Err(bad("best answer out of range"));
    }
    let nopt = rd.index(4)?;
    if nopt != 3 {
        return Err(bad("three optional vectors are required"));
    }
    for _ in 0..nopt {
        rd.vec3()?;
    }
    let mut prop = 0;
    if op == Op::PropertyGs {
        prop = rd.index(28)?;
        if !rd.flag()? {
            return Err(bad("a property value is required"));
        }
        rd.finite()?;
    }
    let which = rd.index(1)?;
    if which == 1 && op != Op::PropertyGs {
        return Err(bad("FindBestSubdivisionNode only for the property solver"));
    }
    let mut nodes = Vec::new();
    for t in &trees {
        let i = rd.index(t.len() - 1)?;
        nodes.push(*t.get(i).ok_or_else(|| bad("node index"))?);
    }
    // never reach the curve span data (the shim's m_pData is a dummy)
    if which == 0 {
        match op {
            Op::PropertyGs => {
                let (aux, data) = nodes.first().copied().unwrap_or((0, false));
                let box_props = [16, 17, 18, 19, 20, 21, 22, 23];
                if data && is_curve_aux(aux) && !box_props.contains(&prop) {
                    return Err(bad("property test would read span data"));
                }
                if data && aux == 1 && prop == 16 {
                    return Err(bad("plane test would read the span pseudo box"));
                }
            }
            Op::CcGs => {
                let span = nodes.iter().all(|&(aux, data)| data && is_curve_aux(aux));
                if (sop == 5 || sop == 22) && span {
                    return Err(bad("normal test would read span data"));
                }
            }
            _ => {
                let (aux, data) = nodes.get(1).copied().unwrap_or((0, false));
                if sop == 5 && data && is_curve_aux(aux) {
                    return Err(bad("normal test would read span data"));
                }
            }
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
    let mut rd = Reader { input, at: 0 };
    if op.is_static() {
        validate_static(op, &mut rd)?;
    } else {
        let c = curve_block(&mut rd)?;
        validate_curve_op(op, &mut rd, &c)?;
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
        // and flag the shim passes to upstream, solver branches that never reach the
        // dummy span data); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p2_eval(
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
