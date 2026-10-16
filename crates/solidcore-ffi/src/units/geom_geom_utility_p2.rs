//! FFI wrappers for unit `geom.geom_utility_p2`.
//!
//! Shim: `harness/cpp/shim/geom_geom_utility_p2.cpp`.
//! Upstream: `source/SMLib/src/SmGeomUtility.cpp` (`smgu_TrianglePointDistance`,
//! `smgu_LineLineAtDistance`, `smgu_IsPointOnSegment`,
//! `smgu_SegmentSegment{Projected,Perpspective}Intersect`, `smgu_SegmentSegmentIntersect`,
//! `smgu_SegmentSegmentClosestPoint`, `smgu_CirclePlaneIntersect`,
//! `smgu_LineCoPlanarEllipseIntersect`, `smgu_SphereCircleSphereCircleIntersect`,
//! `smgu_IntersectTwoPlanes`, `smgu_BestFitPlaneToPoints`, `smgu_4x4Mult`,
//! `smgu_4x4Inverse`, `smgu_FrenetFromDerivatives`, `smgu_EvaluateCurveContinuity`).
//!
//! | Rust   | shim                           | upstream                  |
//! |--------|--------------------------------|---------------------------|
//! | [`op`] | `sc_geom_geom_utility_p2_op`   | one `smgu_*` per [`Op`]   |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_geom_utility_p2_op(
            op: i32,
            input: *const f64,
            n_in: i64,
            ival: i64,
            out: *mut f64,
            cap: i64,
            out_n: *mut i64,
        ) -> i32;
    }
}

/// Output buffer size (every op writes at most this many values).
const CAP: usize = 64;

/// One upstream function. Input layouts (flat `f64`, points/vectors as xyz):
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `p0, p1, p2, test` -> `[dist, closest(3), u, v]`.
    TrianglePointDistance = 0,
    /// `lp1, lv1, lp2, lv2, at` -> `[params1(4), params2(4)]`.
    LineLineAtDistance = 1,
    /// `start, end, test, tol` -> `[on, dist, param]`.
    IsPointOnSegment = 2,
    /// `s1, e1, s2, e2, normal, tol` -> hits.
    SegSegProjected = 3,
    /// `s1, e1, s2, e2, plane point, plane unit normal, eye, tol` -> hits.
    SegSegPerspective = 4,
    /// `s, e, os, oe, tol` -> `[count, points(3 each)]`.
    SegSegIntersect = 5,
    /// `s, e, os, oe, tol` -> hits.
    SegSegClosest = 6,
    /// `tol, radius, origin, circle normal, plane point, plane normal` ->
    /// `[count, points]`.
    CirclePlane = 7,
    /// `line pnt, line vec, center, x axis, y axis, rx, ry, tol` -> `[count, params]`.
    LineEllipse = 8,
    /// `sphere center, radius, c1, n1, c2, n2, tol` -> `[count, points]`.
    SphereCircles = 9,
    /// `p1, v1, p2, v2` -> `[coincident (-1 untouched), line point, line vec]`.
    TwoPlanes = 10,
    /// `points..., tol` -> `[type, point, normal, max gap, max centroid gap]`.
    BestFitPlane = 11,
    /// `a(16), b(16)` row-major -> `a*b (16)`.
    Mult4x4 = 12,
    /// `a(16)` -> `inverse (16)`.
    Inverse4x4 = 13,
    /// `vals(4 vectors)`, `ival = n` -> `n + 1` vectors.
    Frenet = 14,
    /// `pos1, vec1, 2nd1, pos2, vec2, 2nd2, angle, 3rd1, 3rd2`, `ival` bit 0/1 = 3rd1/3rd2
    /// given -> `[continuity type]`.
    Continuity = 15,
}

impl Op {
    /// Exact input length (`None` = variable: [`Op::BestFitPlane`]).
    fn input_len(self) -> Option<usize> {
        Some(match self {
            Op::TrianglePointDistance | Op::Frenet | Op::TwoPlanes => 12,
            Op::LineLineAtDistance | Op::SegSegIntersect | Op::SegSegClosest => 13,
            Op::IsPointOnSegment => 10,
            Op::SegSegProjected | Op::Inverse4x4 => 16,
            Op::SegSegPerspective => 22,
            Op::CirclePlane => 14,
            Op::LineEllipse => 18,
            Op::SphereCircles => 17,
            Op::BestFitPlane => return None,
            Op::Mult4x4 => 32,
            Op::Continuity => 25,
        })
    }
}

/// Result of [`op`]: the upstream status (`0` when the function has none) and the
/// op's outputs.
#[derive(Debug, Clone, PartialEq)]
pub struct OpResult {
    /// Upstream `SmStatus`.
    pub status: i32,
    /// Outputs, layout per [`Op`].
    pub values: Vec<f64>,
}

/// Runs one upstream function on flat inputs `input` with integer argument `ival`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a wrong input length or `ival`;
/// [`FfiError::Status`] for a shim failure.
pub fn op(op: Op, input: &[f64], ival: i64) -> Result<OpResult, FfiError> {
    match op.input_len() {
        Some(n) if input.len() != n => {
            return Err(FfiError::InvalidArgument(format!(
                "{op:?} needs {n} inputs, got {}",
                input.len()
            )))
        }
        None if input.is_empty()
            || !(input.len() - 1).is_multiple_of(3)
            || input.len() > 3 * 4096 + 1 =>
        {
            return Err(FfiError::InvalidArgument(format!(
                "{op:?} needs 3k + 1 inputs, got {}",
                input.len()
            )))
        }
        _ => {}
    }
    if op == Op::Frenet && !(0..=3).contains(&ival) {
        return Err(FfiError::InvalidArgument(format!("frenet n {ival}")));
    }
    if op == Op::Continuity && !(0..=3).contains(&ival) {
        return Err(FfiError::InvalidArgument(format!(
            "continuity flags {ival}"
        )));
    }
    let mut out = vec![0.0f64; CAP];
    let mut n: i64 = 0;
    // SAFETY: `input` is valid for `input.len()` reads (the shim reads at most the
    // per-op length checked above); `out` is valid for CAP writes and every op writes
    // at most CAP values; `n` is a valid out-pointer.
    let st = unsafe {
        sys::sc_geom_geom_utility_p2_op(
            op as i32,
            input.as_ptr(),
            input.len() as i64,
            ival,
            out.as_mut_ptr(),
            CAP as i64,
            &raw mut n,
        )
    };
    check(st)?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| (1..=CAP).contains(&n))
        .ok_or_else(|| FfiError::InvalidArgument(format!("shim wrote {n} values")))?;
    out.truncate(n);
    let status = out.first().copied().unwrap_or(0.0) as i32;
    out.remove(0);
    Ok(OpResult {
        status,
        values: out,
    })
}
