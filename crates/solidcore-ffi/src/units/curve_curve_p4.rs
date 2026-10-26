//! FFI wrappers for unit `curve.curve_p4`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p4.cpp` (`sc_curve_curve_p4_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built
//! with `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`),
//! `1 dim P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`) or `2 3 P[3] V[3] d0 d1 scale io`
//! (a bounded `SmLine(P, V, [d0, d1], scale, 3, ctx, NULL, io)`), or
//! `3 3 C[3] X[3] Y[3] d0 d1 rx ry io` (a genuine ownerless `SmEllipse`). An analytic operand is
//! `0 P V d0 d1 scale io` (a line), `1 C[3] X[3] Y[3] d0 d1 rx ry dim io` (an `SmEllipse` without
//! source NURB) or `2` and a curve block. Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]          | upstream                                  | parameters                         | outputs |
//! |-----------------|-------------------------------------------|------------------------------------|---------|
//! | `IsClosed`      | `IsClosed` (virtual or `SmCurve::`)       | curve `base a b tol`               | flag |
//! | `IsDegenerate`  | `IsDegenerate` (virtual or `SmCurve::`)   | curve `base tol has_ivl a b`       | flag |
//! | `PolarBox`      | `ApproximatePolarBox`                     | curve `a b count`                  | `state basis[9] umin vmin umax vmax` |
//! | `Snap`          | `SnapToKnots`                             | curve `t tol`                      | value |
//! | `Assert`        | `SmCurve::AssertValid`                    | curve `owner_kind given`           | `ok n (test owner_type tol3d value list)*n` |
//! | `FixChain`      | `SmCurve::FixChainCurvesList`             | `n (present [curve])*n tol del`    | `nchain ns starts nc (origin a b P0 P1 / -1)*nc` |
//! | `Filter`        | `sm_FilterSolution`                       | curve `a1 b1` curve `a2 b2 k1[2] k2[2] adj s0 s1 value` | keep |
//! | `ProjDist`      | `sm_GetProjDist`                          | curve `t P N rev D sop`            | value |
//! | `Property`      | `SmCurvePropertyEFO::Evaluate`            | curve `prop has_val val nv V[nv] sqtol t signal` | `f fp found` (always) |
//! | `CcTangent`     | `SmCCTangentIntENFO::StaticEvaluate`      | curve curve `x0 x1 jac`            | `f[2] J[4] found` (always) |
//! | `CpDistAt`      | `SmCPDistAtENFO::Evaluate`                | curve `P dsq sqtol x[4] jac`       | `f[4] J[16] found` (always) |
//! | `DistExtrema`   | `SmFindCurveDistanceExtremaEFO::Evaluate` | curve curve `a b tol t`            | `f fp found` (always) |
//! | `Analytic`      | `AnalyticCurveIntersect`                  | operand `f0 f1` operand `g0 g1 tol` | `more nsol (type nvar nobj o0 o1 value s0 s1 [value e0 e1])*` |
//! | `Planes`        | `HasPlaneCrossings`                       | curve `np (P N)*np`                | flag |
//!
//! `owner_kind` 1 sets an `SmHermiteCurve` as the owner. Interval fractions `f0 f1` are
//! of the operand's natural interval. Solution objects are `0` (this) or `1` (other).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, lines and ellipses with non-zero
//! axes and positive scales / radii, parameters within a few curve lengths of the
//! curve's range, and property option vectors / values present wherever upstream
//! reads them without a `NULL` check.

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 512;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e11;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 4.0;
/// Most curves of `FixChain`.
pub const MAX_CURVES: usize = 16;
/// Most planes of `Planes`.
pub const MAX_PLANES: usize = 4;
/// Most polar box samples.
pub const MAX_SAMPLES: usize = 401;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    IsClosed,
    IsDegenerate,
    PolarBox,
    Snap,
    Assert,
    FixChain,
    Filter,
    ProjDist,
    Property,
    CcTangent,
    CpDistAt,
    DistExtrema,
    Analytic,
    Planes,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::IsClosed => 0,
            Op::IsDegenerate => 1,
            Op::PolarBox => 2,
            Op::Snap => 3,
            Op::Assert => 4,
            Op::FixChain => 5,
            Op::Filter => 6,
            Op::ProjDist => 7,
            Op::Property => 8,
            Op::CcTangent => 9,
            Op::CpDistAt => 10,
            Op::DistExtrema => 11,
            Op::Analytic => 12,
            Op::Planes => 13,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p4_eval(
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

/// The parameter range of a validated curve block.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    a: f64,
    b: f64,
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

/// A line `P V d0 d1 scale io`; its analytic range.
fn line(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
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
    Ok((d0.min(d1), d0.max(d1)))
}

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(3)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
            let (a, b) = bspline(rd)?;
            Ok(CurveInfo { a, b })
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(CurveInfo { a: 0.0, b: 1.0 })
        }
        2 => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            // a line evaluates anywhere: only bound its parameters
            line(rd)?;
            Ok(CurveInfo {
                a: -MAX_COORD / 8.0,
                b: MAX_COORD / 8.0,
            })
        }
        _ => {
            if dim != 3 {
                return Err(bad("ellipse: dimension must be 3"));
            }
            // Same geometry predicates and order as the dedicated operand below;
            // only the ordinary block's dimension precedes its geometry.
            rd.vec3()?;
            let x = rd.nonzero3()?;
            let y = rd.nonzero3()?;
            let cross = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
                return Err(bad("ellipse: parallel axes"));
            }
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            if d0.abs() > 720.0 || d1.abs() > 720.0 {
                return Err(bad("ellipse: domain end beyond 720 degrees"));
            }
            if !(1e-4..=360.0).contains(&(d1 - d0).abs()) {
                return Err(bad("ellipse: domain length outside [1e-4, 360]"));
            }
            for _ in 0..2 {
                if rd.finite()? <= 1e-6 {
                    return Err(bad("ellipse: radius not positive"));
                }
            }
            rd.flag()?;
            // N_CreateEllipticalArcTol constructs normalized [0,1] knots;
            // the ownerless constructor does not reparameterize them.
            Ok(CurveInfo { a: 0.0, b: 1.0 })
        }
    }
}

/// An analytic operand (see the module docs).
fn operand(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match rd.index(2)? {
        0 => {
            line(rd)?;
        }
        1 => {
            rd.vec3()?;
            let x = rd.nonzero3()?;
            let y = rd.nonzero3()?;
            let cross = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
                return Err(bad("ellipse: parallel axes"));
            }
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            if d0.abs() > 720.0 || d1.abs() > 720.0 {
                return Err(bad("ellipse: domain end beyond 720 degrees"));
            }
            if !(1e-4..=360.0).contains(&(d1 - d0).abs()) {
                return Err(bad("ellipse: domain length outside [1e-4, 360]"));
            }
            for _ in 0..2 {
                if rd.finite()? <= 1e-6 {
                    return Err(bad("ellipse: radius not positive"));
                }
            }
            let dim = rd.index(3)?;
            if dim != 3 {
                return Err(bad("ellipse: dimension must be 3"));
            }
            rd.flag()?;
        }
        _ => {
            curve_block(rd)?;
        }
    }
    // the interval fractions
    for _ in 0..2 {
        let f = rd.finite()?;
        if !(-0.5..=1.5).contains(&f) {
            return Err(bad("interval fraction out of range"));
        }
    }
    Ok(())
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

/// Option vectors a property reads (when the vector array is not `NULL`).
fn property_vectors(prop: usize) -> usize {
    match prop {
        // perpendicular, parallel, silhouette vector / point, plane intersection,
        // distance to point, point inversion
        0 | 4 | 5 | 6 | 16 | 23 | 25 => 1,
        // angles, projected point, cylinder, tangent to circle, maxima, tangent through
        17..=22 | 24 | 26..=28 => 2,
        _ => 0,
    }
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::IsClosed => {
            let c = curve_block(rd)?;
            rd.flag()?;
            param(rd, &c)?;
            param(rd, &c)?;
            rd.finite()?;
        }
        Op::IsDegenerate => {
            let c = curve_block(rd)?;
            rd.flag()?;
            rd.finite()?;
            rd.flag()?;
            param(rd, &c)?;
            param(rd, &c)?;
        }
        Op::PolarBox => {
            let c = curve_block(rd)?;
            param(rd, &c)?;
            param(rd, &c)?;
            rd.index(MAX_SAMPLES)?;
        }
        Op::Snap => {
            let c = curve_block(rd)?;
            param(rd, &c)?;
            rd.finite()?;
        }
        Op::Assert => {
            curve_block(rd)?;
            rd.index(1)?;
            rd.flag()?;
        }
        Op::FixChain => {
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                if rd.flag()? {
                    curve_block(rd)?;
                }
            }
            if rd.finite()? < 0.0 {
                return Err(bad("negative tolerance"));
            }
            rd.flag()?;
        }
        Op::Filter => {
            let c1 = curve_block(rd)?;
            param(rd, &c1)?;
            param(rd, &c1)?;
            let c2 = curve_block(rd)?;
            param(rd, &c2)?;
            param(rd, &c2)?;
            for _ in 0..4 {
                rd.index(13)?;
            }
            rd.index(2)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
            rd.finite()?;
        }
        Op::ProjDist => {
            let c = curve_block(rd)?;
            param(rd, &c)?;
            rd.vec3()?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            rd.index(24)?;
        }
        Op::Property => {
            let c = curve_block(rd)?;
            let prop = rd.index(28)?;
            let has_val = rd.flag()?;
            rd.finite()?;
            let nv = rd.index(3)?;
            for _ in 0..nv {
                rd.vec3()?;
            }
            let need = property_vectors(prop);
            // projected point reads its vectors without a NULL check
            if (nv > 0 || matches!(prop, 20 | 21)) && nv < need {
                return Err(bad("property needs more option vectors"));
            }
            // values read without a NULL check (distance, cylinder radius)
            if matches!(prop, 22 | 23) && nv > 0 && !has_val {
                return Err(bad("property needs a value"));
            }
            rd.finite()?;
            param(rd, &c)?;
            rd.flag()?;
        }
        Op::CcTangent => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
            rd.flag()?;
        }
        Op::CpDistAt => {
            let c = curve_block(rd)?;
            rd.vec3()?;
            rd.finite()?;
            rd.finite()?;
            param(rd, &c)?;
            for _ in 0..3 {
                rd.finite()?;
            }
            rd.flag()?;
        }
        Op::DistExtrema => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            rd.finite()?;
            rd.finite()?;
            rd.finite()?;
            let t = param(rd, &c1)?;
            let far = MAX_REL_DIST * (c2.b - c2.a);
            if t < c2.a - far || t > c2.b + far {
                return Err(bad("parameter too far outside the second curve"));
            }
        }
        Op::Analytic => {
            operand(rd)?;
            operand(rd)?;
            rd.finite()?;
        }
        Op::Planes => {
            curve_block(rd)?;
            let np = rd.index(MAX_PLANES)?;
            for _ in 0..np {
                rd.vec3()?;
                rd.nonzero3()?;
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
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve blocks and operands, every
        // index, count and flag the shim passes to upstream, option vectors wherever
        // upstream dereferences them); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p4_eval(
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

    fn ellipse_distance_block() -> Vec<f64> {
        vec![3., 3., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 180., 2., 1., 0.]
    }

    fn ellipse_distance_input(block: &[f64]) -> Vec<f64> {
        let mut v = block.to_vec();
        v.extend([0.25, -0.5, 0.25, 1., 0., 0.5, 0.1, 0.2, 0.3, 1.]);
        v
    }

    #[test]
    fn ellipse_distance_generic_positions_and_normalized_range() {
        let block = ellipse_distance_block();
        let info = curve_block(&mut Reader::new(&block)).unwrap();
        assert_eq!((info.a, info.b), (0., 1.));
        validate(Op::CpDistAt, &ellipse_distance_input(&block)).unwrap();
        for (op, tail) in [
            (Op::IsClosed, vec![1., 0., 1., 1e-6]),
            (Op::IsDegenerate, vec![1., 1e-6, 1., 0., 1.]),
            (Op::PolarBox, vec![0., 1., 10.]),
            (Op::Snap, vec![0.5, 1e-6]),
            (Op::Assert, vec![0., 0.]),
            (Op::ProjDist, vec![0.5, 0., 0., 0., 0., 0., 1., 0., 1., 0., 0., 0.]),
            (Op::Property, vec![0., 0., 0., 1., 1., 0., 0., 0., 0.5, 0.]),
            (Op::Planes, vec![0.]),
        ] {
            let mut input = block.clone();
            input.extend(tail);
            validate(op, &input).unwrap();
        }
        let mut chain = vec![2., 1.];
        chain.extend_from_slice(&block);
        chain.push(1.);
        chain.extend_from_slice(&block);
        chain.extend([1e-6, 0.]);
        validate(Op::FixChain, &chain).unwrap();
        for (op, tail) in [
            (Op::CcTangent, vec![0.5, 0.5, 1.]),
            (Op::DistExtrema, vec![0., 1., 1e-6, 0.5]),
        ] {
            let mut input = block.clone();
            input.extend_from_slice(&block);
            input.extend(tail);
            validate(op, &input).unwrap();
        }
        let mut filter = block.clone();
        filter.extend([0., 1.]);
        filter.extend_from_slice(&block);
        filter.extend([0., 1., 0., 0., 0., 0., 0., 0.5, 0.5, 0.]);
        validate(Op::Filter, &filter).unwrap();
        let mut analytic = Vec::new();
        for _ in 0..2 {
            analytic.push(2.);
            analytic.extend_from_slice(&block);
            analytic.extend([0., 1.]);
        }
        analytic.push(1e-6);
        validate(Op::Analytic, &analytic).unwrap();
        let mut far = ellipse_distance_input(&block);
        far[21] = 6.;
        assert_eq!(validate(Op::CpDistAt, &far).unwrap_err().to_string(),
            bad("parameter too far outside the curve").to_string());
    }

    #[test]
    fn ellipse_distance_rejects_bad_geometry_without_native_execution() {
        let block = ellipse_distance_block();
        let full = ellipse_distance_input(&block);
        for len in 0..full.len() {
            assert!(validate(Op::CpDistAt, &full[..len]).is_err());
        }
        let mut trailing = full;
        trailing.push(0.);
        assert!(validate(Op::CpDistAt, &trailing).is_err());
        for (index, value) in [
            (0, 4.), (0, -1.), (1, 2.), (1, 4.), (15, 2.),
            (2, f64::NAN), (3, f64::INFINITY), (4, MAX_COORD * 2.),
            (5, 0.), (5, MAX_COORD * 2.), (9, 0.),
            (11, -721.), (12, 721.), (12, 0.00001), (12, 361.),
            (13, 0.), (14, -1.), (13, 1e-6), (14, f64::NAN), (13, MAX_COORD * 2.),
        ] {
            let mut bad_block = block.clone();
            bad_block[index] = value;
            assert!(validate(Op::CpDistAt, &ellipse_distance_input(&bad_block)).is_err(),
                "index {index}, value {value}");
        }
        let mut parallel = block;
        parallel[8..11].copy_from_slice(&[2., 0., 0.]);
        assert!(validate(Op::CpDistAt, &ellipse_distance_input(&parallel)).is_err());
    }

    #[test]
    fn ellipse_distance_old_protocol_and_first_error_controls() {
        let line = vec![2., 3., 0., 0., 0., 1., 0., 0., 0., 1., 1., 0.];
        let hermite = vec![1., 3., 0., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0.];
        let bspline = vec![0., 3., 0., 1., 1., 3., 0., 0., 0., NOW,
            1., 0., 0., NOW, 0., 0., 1., 1.];
        for block in [&line, &hermite, &bspline] {
            validate(Op::CpDistAt, &ellipse_distance_input(block)).unwrap();
        }
        for (input, message) in [
            (vec![2., 1., f64::NAN], "curve block: dimension must be 2 or 3"),
            (vec![2., 2., f64::NAN], "line block: dimension must be 3"),
            (vec![0., 3., 0., 0., 0., 0.], "curve: need 1 <= p <= n and m = n + p + 1"),
        ] {
            assert_eq!(validate(Op::CpDistAt, &input).unwrap_err().to_string(), bad(message).to_string());
        }
        // The dedicated operand still has dimension AFTER the radii.
        let mut old = vec![1.];
        old.extend_from_slice(&ellipse_distance_block()[2..15]);
        old.extend([3., 0., 0., 1.]);
        let mut rd = Reader::new(&old);
        operand(&mut rd).unwrap();
        rd.done().unwrap();
        old[12] = 0.; // radius error wins over the later invalid dimension/flag
        old[14] = 2.;
        old[15] = 2.;
        assert_eq!(operand(&mut Reader::new(&old)).unwrap_err().to_string(),
            bad("ellipse: radius not positive").to_string());
    }
}
