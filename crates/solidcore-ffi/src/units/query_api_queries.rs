//! Native public-query calls. Shim: `harness/cpp/shim/query_api_queries.cpp`.
//!
//! Fixed layout: origin[3], direction[3], scale, domain[4], query[3], parameter,
//! tight, null, `remove_curve`, `inside_out`, `edge_tolerance`, selector, kind.
//! Kinds: 0 line, 1 circle, 2 ellipse, 3 cubic B-spline, 4 plane, 5 quadratic
//! B-spline surface, 6 box, 7 empty B-rep, 8 rational cylinder, 9 rational sphere.
//! Kind 10 is a degree-one B-spline with an internal corner; kind 11 is a plane
//! face with two vertex loops; kind 12 is a planar square with a square hole.
//! Kind 13 is an analytic surface of revolution with a line generator (op 3).
//! Ops 7/8 optionally append the other box's
//! origin[3] and side length (26 values total).
//! Graph exports append `(upward_faceuse, rectangular_trim)` per face to the
//! shared `topology_edgeuse_p1` format.
//! Op 0 exports the native graph; 1..25
//! call `SmApi` queries (see the runner). Op 26 exports the graph with the selected
//! face boundary's last orientation set to Unknown when `inside_out` is true, as
//! op 11 does before querying. Results start with the exact `SmStatus`.
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_query_api_queries_eval(
            op: i32,
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Calls a native query after checking the complete fixed layout and the
/// construction ranges used by the kernel. Op 0 returns a raw topology export.
pub fn eval(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let bad = || FfiError::InvalidArgument("query.api_queries layout or construction range".into());
    if (input.len() != 22 && !(input.len() == 26 && matches!(op, 7 | 8)))
        || !(0..=27).contains(&op)
        || input.iter().any(|x| !x.is_finite() || x.abs() > 1e12)
    {
        return Err(bad());
    }
    let a: &[f64; 22] = input
        .get(..22)
        .ok_or_else(bad)?
        .try_into()
        .map_err(|_| bad())?;
    if !(0.0..=16.0).contains(&a[21])
        || a[21].fract() != 0.0
        || a[6] <= 1e-10
        || a[6] > 1e6
        || a[20] < 0.0
        || a[20].fract() != 0.0
        || a[20] > 1000.0
        || [a[15], a[16], a[17], a[18]]
            .iter()
            .any(|&flag| flag != 0.0 && flag != 1.0)
        || a[19] < 0.0
        || a[19] > 1.0
    {
        return Err(bad());
    }
    let kind = a[21] as i32;
    if matches!(kind, 0 | 4 | 11) && a[3] * a[3] + a[4] * a[4] + a[5] * a[5] < 1e-20 {
        return Err(bad());
    }
    let is_curve = kind <= 3 || kind == 10;
    if is_curve && (a[7] >= a[8] || (kind != 0 && (a[7] < 0.0 || a[8] > 1.0))) {
        return Err(bad());
    }
    if matches!(kind, 4 | 5 | 8 | 9 | 11)
        && (a[7] >= a[9]
            || a[8] >= a[10]
            || (kind == 5 && (a[7] < 0.0 || a[8] < 0.0 || a[9] > 1.0 || a[10] > 1.0)))
    {
        return Err(bad());
    }
    // Building the B-rep of a rational sphere (kind 9) of radius about 1e-5 .. 1e-4
    // does not return in upstream (the face's `SmSurface::LocalCurveIntersect` ->
    // `SmLocalSolveNd::SolveIt` inside `SmGlobalSolver::SolveBranch`, shape-dependent:
    // 1e-6 and 1e-3 return), so B-rep ops (all but 3) need a radius of at least 1e-3
    // (the fixtures use 1).
    if kind == 9 && op != 3 && a[6] < 1e-3 {
        return Err(bad());
    }
    if (op == 1 || op == 2) && !is_curve
        || op == 3 && !matches!(kind, 4 | 5 | 8 | 9 | 13)
        || kind == 13 && op != 3
        || (14..=16).contains(&kind) && op != 27
    {
        return Err(bad());
    }
    if input
        .get(25)
        .is_some_and(|&size| size <= 1e-10 || size > 1e6)
    {
        return Err(bad());
    }
    // Solvers/pickers dereference edge curves; face interior, classification and
    // closest-point paths also require them when this construction has faces.
    // A null top-level input still takes the API's guard.
    let requires_curve = matches!(op, 4 | 5 | 6 | 7 | 8 | 12 | 13 | 21)
        || matches!(op, 17..=19) && matches!(kind, 4 | 5 | 6 | 8 | 9 | 11 | 12)
        || matches!(op, 15 | 16) && a[15] != 0.0 && matches!(kind, 5 | 8 | 9);
    if requires_curve && a[17] != 0.0 && a[16] == 0.0 {
        return Err(bad());
    }
    call_with_buffer(2048, |out, len| {
        // SAFETY: layout and all construction/selector inputs checked above;
        // all output buffers are supplied by call_with_buffer.
        unsafe {
            sys::sc_query_api_queries_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_layout_before_ffi() {
        assert!(eval(1, &[]).is_err());
        assert!(eval(1, &[f64::NAN; 22]).is_err());
        assert!(eval(99, &[0.0; 22]).is_err());
    }

    #[test]
    fn guards_missing_curve_without_rejecting_a_null_edge() {
        let mut a = [0.; 22];
        a[3] = 1.;
        a[6] = 1.;
        a[8] = 1.;
        a[17] = 1.;
        for kind in [0., 3.] {
            a[21] = kind;
            for op in [4, 5, 6, 7, 8, 12, 13, 21] {
                a[16] = 0.;
                assert!(
                    matches!(eval(op, &a), Err(FfiError::InvalidArgument(_))),
                    "op {op}, kind {kind} must be rejected before FFI"
                );
                a[16] = 1.;
                assert_eq!(eval(op, &a).unwrap(), vec![1007.]);
            }
        }
    }

    #[test]
    fn native_null_pointer_contract_is_checked_outside_rust_difftest() {
        let mut a = [0.; 22];
        a[5] = 1.;
        a[6] = 1.;
        a[9] = 1.;
        a[10] = 1.;
        a[16] = 1.;
        for op in 1..=25 {
            a[21] = if op <= 2 {
                0.
            } else if op == 3 {
                4.
            } else {
                6.
            };
            a[8] = if op <= 2 { 1. } else { 0. };
            assert_eq!(eval(op, &a).unwrap(), vec![1007.], "native op {op}");
        }
    }

    #[test]
    fn rejects_missing_face_boundary_curves_before_native_traversal() {
        let mut a = [0.; 22];
        a[5] = 1.;
        a[6] = 1.;
        a[9] = 1.;
        a[10] = 1.;
        a[17] = 1.;
        for kind in [4., 5., 6., 8., 9., 11., 12.] {
            a[21] = kind;
            for op in 17..=19 {
                assert!(
                    matches!(eval(op, &a), Err(FfiError::InvalidArgument(_))),
                    "op {op}, kind {kind} must reject a removed boundary curve"
                );
            }
        }
        a[15] = 1.;
        for kind in [5., 8., 9.] {
            a[21] = kind;
            for op in [15, 16] {
                assert!(matches!(eval(op, &a), Err(FfiError::InvalidArgument(_))));
            }
        }
    }

    #[test]
    fn checks_second_box_before_native_construction() {
        let mut a = [0.; 26];
        a[6] = 1.;
        a[21] = 6.;
        a[25] = 1.;
        assert!(eval(9, &a).is_err());
        a[25] = 0.;
        assert!(eval(7, &a).is_err());
        a[25] = f64::NAN;
        assert!(eval(8, &a).is_err());
    }
}
