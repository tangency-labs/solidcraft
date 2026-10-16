//! FFI wrappers for unit `geom.geom_utility_p1`.
//!
//! Shims: `harness/cpp/shim/geom_geom_utility_p1.cpp` (exported `smgu_*` functions) and
//! `harness/cpp/shim/geom_geom_utility_p1_static.cpp` (the file-static helpers).
//! Upstream: `source/SMLib/src/SmGeomUtility.cpp`.
//!
//! | Rust                  | shim                                         | upstream               |
//! |-----------------------|----------------------------------------------|------------------------|
//! | [`call`]              | `sc_geom_geom_utility_p1_call`               | the `smgu_*` of [`Op`] |
//! | [`add_point`]         | `sc_geom_geom_utility_p1_add_point`          | `sm_AddPoint`          |
//! | [`add_closest_point`] | `sc_geom_geom_utility_p1_add_closest_point`  | `sm_AddClosestPoint`   |

use crate::{call_with_buffer, check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_geom_utility_p1_call(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_geom_utility_p1_add_point(
            io: *mut f64,
            tol: f64,
            pnt: *const f64,
            status: *mut i32,
        ) -> i32;
        pub fn sc_geom_geom_utility_p1_add_closest_point(
            io: *mut f64,
            dist: f64,
            param1: f64,
            param2: f64,
            tol: f64,
            status: *mut i32,
        ) -> i32;
    }
}

/// Operations of [`call`]; the input layout of each is documented in the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Op {
    /// `smgu_PolygonArea`: `[normal, points...]` (at least one point).
    PolygonArea = 2,
    /// `smgu_TriangleAreaSquared`: `[p0, p1, p2]`.
    TriangleAreaSquared = 3,
    /// `smgu_TriangleDecomposition`: `[base, left, right]`.
    TriangleDecomposition = 4,
    /// `smgu_ParallelogramCrossVectors`: `[base, side, d_tb, d_bs]`.
    Parallelogram = 5,
    /// `smgu_LinePointDistance` (3d): `[lp, lv, tp]`.
    LinePointDistance = 6,
    /// `smgu_LineLineClosestPoint`: `[p1, v1, p2, v2]`.
    LineLineClosestPoint = 7,
    /// `smgu_LinePlaneIntersect` (normal): `[lp, lv, pp, pn]`.
    LinePlane = 8,
    /// `smgu_LinePlaneIntersect` (tangents): `[lp, lv, pp, su, sv]`.
    LinePlaneUv = 9,
    /// `smgu_LineClosestPoint`: `[lp, lv, tp]`.
    LineClosestPoint = 10,
    /// `smgu_SegmentPointDistance`: `[s, e, tp]`.
    SegmentPointDistance = 11,
    /// `smgu_SegmentPointAngleDegToPerp`: `[s, e, tp]`.
    SegmentAngle = 12,
    /// `smgu_CircleCenterFrom3Points`: `[a, b, c]` (2d).
    CircleCenter3Points = 13,
    /// `smgu_CircleFrom3Points`: `[sp, mp, ep, dimension, closed]`.
    Circle3Points = 14,
    /// `smgu_CircleFindSilhouettePoints`: `[center, normal, radius, perspective, eye]`.
    Silhouette = 15,
    /// `smgu_ConesAreDisjointPerpen`: `[v1, a1, v2, a2]`.
    ConesDisjoint = 16,
    /// `smgu_LineCoPlanarCircleIntersect`: `[lp, lv, cc, cn, r, tol]`.
    LineCircle = 17,
    /// `smgu_CircleCoPlanarCircleIntersect`: `[n, c1, r1, c2, r2, tol]`.
    CircleCircle = 18,
    /// `smgu_LineIntersectSphere`: `[lp, lv, c, r, tol]`.
    LineSphere = 19,
    /// `smgu_IntersectThreePlanes`: `[p1, v1, p2, v2, p3, v3]`.
    ThreePlanes = 20,
    /// `smgu_PlaneEvaluatePoint`: `[origin, x, y, uv]`.
    PlaneEval = 21,
    /// `smgu_PlaneDropPoint`: `[origin, x, y, p]`.
    PlaneDrop = 22,
    /// `smgu_PlanePointDistance`: `[pp, pn, tp]`.
    PlanePointDistance = 23,
    /// `smgu_PointProjectToPlane`: `[p, pp, pn]`.
    PointProject = 24,
    /// `smgu_VectorProjectToPlane`: `[v, n]`.
    VectorProject = 25,
    /// `smgu_TransformPointToStartPlane`: `[p, origin, x, y, smin, smax, tol, snap]`.
    StartPlane = 26,
    /// `smgu_PointPerspectiveToPlane`: `[p, pp, pn, eye]`.
    Perspective = 27,
    /// `smgu_VecLinCombTwoVectors`: `[b1, b2, v]`.
    VecLinComb = 28,
    /// `smgu_BestFitPointToPoints`: `[tol, points...]`.
    BestFitPoint = 29,
    /// `smgu_BestFitLineToPoints`: `[tol, points...]`.
    BestFitLine = 30,
    /// `smgu_VecTripleProduct`: `[a, b, c]`.
    TripleProduct = 31,
    /// `smgu_3x3Copy`: 9 values.
    Copy3x3 = 32,
    /// `smgu_3x3Determinate` (coefficients): 9 values.
    Det3x3 = 33,
    /// `smgu_Determinant3Vectors`: `[v1, v2, v3]`.
    Det3Vectors = 34,
    /// `smgu_3x3Inverse`: `[a0, a1, a2]`.
    Inverse3x3 = 35,
    /// `smgu_4x4Copy`: 16 values.
    Copy4x4 = 36,
    /// `smgu_4x4Determinate`: 16 values.
    Det4x4 = 37,
    /// `smgu_SolveQuadraticEqn`: `[c0, c1, c2, tol]`.
    Quadratic = 38,
    /// `smgu_DirectionalDerivs`: `[dx, dy, hu, hv, derivs...]`.
    DirDerivs = 39,
    /// `smgu_VolDirectionalDerivs`: `[dx, dy, dz, n, derivs...]`.
    VolDirDerivs = 40,
    /// `smgu_IsDogLeg` (2d): `[p0, p1, p2, p3, bend_deg, parallel_deg]`.
    DogLeg = 41,
    /// `smgu_AddSorted`: `[value, idx, increasing, with_indices, n, values(n), indices(n)]`.
    AddSorted = 42,
    /// `smgu_ShellSort_double_array`: the values.
    ShellSort = 43,
    /// `smgu_DoubleLessThan`: `[a, b]`.
    DoubleLess = 44,
}

/// Largest `hu`/`hv` accepted for [`Op::DirDerivs`] and `n` for [`Op::VolDirDerivs`].
pub const MAX_DERIV: usize = 6;

fn small_int(v: Option<&f64>, max: usize, what: &str) -> Result<usize, FfiError> {
    match v {
        Some(&x) if x >= 0.0 && x <= max as f64 && x.fract() == 0.0 => Ok(x as usize),
        _ => Err(FfiError::InvalidArgument(format!(
            "{what} must be an integer in 0..={max}"
        ))),
    }
}

fn need(input: &[f64], n: usize) -> Result<(), FfiError> {
    if input.len() == n {
        Ok(())
    } else {
        Err(FfiError::InvalidArgument(format!(
            "expected {n} inputs, got {}",
            input.len()
        )))
    }
}

/// Validates `input` for `op` (lengths, counts, integer arguments) so the shim never
/// reads out of bounds or casts an out-of-range double.
fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let len = input.len();
    let pts_after = |head: usize, min: usize| {
        if len >= head + 3 * min && (len - head).is_multiple_of(3) {
            Ok(())
        } else {
            Err(FfiError::InvalidArgument(format!(
                "bad point list length {len}"
            )))
        }
    };
    match op {
        Op::PolygonArea => pts_after(3, 1),
        Op::BestFitPoint | Op::BestFitLine => pts_after(1, 0),
        Op::TriangleDecomposition => need(input, 3),
        Op::DoubleLess => need(input, 2),
        Op::Quadratic => need(input, 4),
        Op::VectorProject | Op::CircleCenter3Points => need(input, 6),
        Op::Parallelogram | Op::ConesDisjoint => need(input, 8),
        Op::TriangleAreaSquared
        | Op::LinePointDistance
        | Op::LineClosestPoint
        | Op::SegmentPointDistance
        | Op::SegmentAngle
        | Op::PlanePointDistance
        | Op::PointProject
        | Op::VecLinComb
        | Op::TripleProduct
        | Op::Copy3x3
        | Op::Det3x3
        | Op::Det3Vectors
        | Op::Inverse3x3 => need(input, 9),
        Op::DogLeg => need(input, 10),
        Op::Silhouette | Op::LineSphere | Op::PlaneEval => need(input, 11),
        Op::LineLineClosestPoint
        | Op::LinePlane
        | Op::CircleCircle
        | Op::PlaneDrop
        | Op::Perspective => need(input, 12),
        Op::LineCircle => need(input, 14),
        Op::LinePlaneUv => need(input, 15),
        Op::StartPlane | Op::Copy4x4 | Op::Det4x4 => need(input, 16),
        Op::ThreePlanes => need(input, 18),
        Op::Circle3Points => {
            need(input, 11)?;
            small_int(input.get(9), 10, "dimension").map(|_| ())
        }
        Op::DirDerivs => {
            let hu = small_int(input.get(2), MAX_DERIV, "hu")?;
            let hv = small_int(input.get(3), MAX_DERIV, "hv")?;
            need(input, 4 + 3 * (hu + 1) * (hv + 1))
        }
        Op::VolDirDerivs => {
            let n = small_int(input.get(3), MAX_DERIV, "n")?;
            need(input, 4 + 3 * (n + 1) * (n + 1) * (n + 1))
        }
        Op::AddSorted => {
            small_int(input.get(1), u32::MAX as usize, "idx")?;
            let n = small_int(input.get(4), 1 << 20, "n")?;
            need(input, 5 + 2 * n)?;
            for i in 0..n {
                small_int(input.get(5 + n + i), u32::MAX as usize, "index")?;
            }
            Ok(())
        }
        Op::ShellSort => Ok(()),
    }
}

/// Runs `op` on `input` in upstream: `[status, values...]` (only `[status]` when the
/// status is not `SM_SUCCESS` = 1000).
pub fn call(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    let n_in = i64::try_from(input.len()).map_err(|_| FfiError::InvalidArgument("len".into()))?;
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `input.len()` reads and its layout was checked
        // by `validate` for `op`; `buf` is valid for `buf.len()` writes; `len` is a
        // valid out-param. Nothing is retained after the call.
        unsafe {
            sys::sc_geom_geom_utility_p1_call(
                op as i32,
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn check_count(state: &[f64; 7]) -> Result<(), FfiError> {
    small_int(state.first(), 2, "count").map(|_| ())
}

/// Upstream `sm_AddPoint` on `state = [n, p0, p1]` (`n` in 0..=2): returns the status and
/// the updated state.
pub fn add_point(state: [f64; 7], tol: f64, pnt: [f64; 3]) -> Result<(i32, [f64; 7]), FfiError> {
    check_count(&state)?;
    let mut io = state;
    let mut status = 0i32;
    // SAFETY: `io` has the 7 slots the shim reads and writes, `pnt` 3 readable values,
    // `status` is a valid out-param; the count was checked to be 0, 1 or 2.
    let st = unsafe {
        sys::sc_geom_geom_utility_p1_add_point(io.as_mut_ptr(), tol, pnt.as_ptr(), &raw mut status)
    };
    check(st)?;
    Ok((status, io))
}

/// Upstream `sm_AddClosestPoint` on `state = [n, params1[2], params2[2], dists[2]]`
/// (`n` in 0..=2): returns the status and the updated state.
pub fn add_closest_point(
    state: [f64; 7],
    dist: f64,
    param1: f64,
    param2: f64,
    tol: f64,
) -> Result<(i32, [f64; 7]), FfiError> {
    check_count(&state)?;
    let mut io = state;
    let mut status = 0i32;
    // SAFETY: `io` has the 7 slots the shim reads and writes, `status` is a valid
    // out-param; the count was checked to be 0, 1 or 2.
    let st = unsafe {
        sys::sc_geom_geom_utility_p1_add_closest_point(
            io.as_mut_ptr(),
            dist,
            param1,
            param2,
            tol,
            &raw mut status,
        )
    };
    check(st)?;
    Ok((status, io))
}
