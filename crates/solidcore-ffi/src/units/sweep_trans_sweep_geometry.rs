//! Safe native calls for `sweep.trans_sweep_geometry`.
//!
//! Input: op (0 start frame, 1 end frame, 2 transform, 3 vertex same,
//! 4 vertex higher, 5 check, 6 edge same, 7 edge higher, 8 face same),
//! sweep xyz, point xyz, start/end/edge/face tolerances, epsilon, analytics flag,
//! curve kind (0 real line, 1 rational Bezier, 2 real circle, 3 Hermite, 4 2D Bezier, 5 ellipse), point count,
//! interval min/max, swap flag, face kind (0 real plane, 1 extruded B-spline),
//! then xyzw control points (circle/ellipse: center xyz; radii in the first/last w entries).
//! Output begins with the upstream status; successful geometry includes native
//! kinds, parameter domains, ordered boundaries, endpoint roles, tolerances,
//! normals, and nine curve / 25 surface position and derivative samples.
// Fixed wire layout / test geometry indices are validated before use.
#![allow(clippy::indexing_slicing)]

use crate::{call_with_buffer, FfiError};
mod sys {
    extern "C" {
        pub fn sc_sweep_trans_sweep_geometry_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
/// Execute a validated sweep case against upstream C++.
///
/// # Errors
/// Invalid layouts or inputs that cannot construct a curve without undefined reads;
/// native shim failures. A native geometry constructor that fails returns its
/// upstream status as the whole output, before its null result can reach a
/// topology builder.
pub fn eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let bad = || FfiError::InvalidArgument("invalid translational sweep layout".into());
    if input.len() < 19 || input.iter().any(|v| !v.is_finite()) {
        return Err(bad());
    }
    // The native basis work arrays have NL_MAXDEG = 17 entries plus one.
    let n = input[14] as usize;
    if !(2..=18).contains(&n)
        || input[14] != n as f64
        || input.len() != 19 + 4 * n
        || !(0.0..=8.0).contains(&input[0])
        || input[0].fract() != 0.0
        || ![0.0, 1.0, 2.0, 3.0, 4.0, 5.0].contains(&input[13])
        || [12, 17, 18]
            .iter()
            .any(|&i| ![0.0, 1.0].contains(&input[i]))
        || input[15] > input[16]
    {
        return Err(bad());
    }
    if [1.0, 4.0].contains(&input[13]) && input[19..].as_chunks::<4>().0.iter().any(|p| p[3] <= 0.0)
    {
        return Err(bad());
    }
    if [2.0, 5.0].contains(&input[13]) && input[22] <= 0.0 {
        return Err(bad());
    }
    if input[13] == 5.0 && input[22 + 4 * (n - 1)] <= 0.0 {
        return Err(bad());
    }
    if input[13] == 0.0 && input[19..22] == input[19 + 4 * (n - 1)..22 + 4 * (n - 1)] {
        return Err(bad());
    }
    call_with_buffer(2048, |out, len| {
        // SAFETY: layout and all indexed counts are checked above. Both buffers and
        // len are valid for their advertised sizes; C++ retains no pointers.
        unsafe {
            sys::sc_sweep_trans_sweep_geometry_eval(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::eval;
    use crate::{FfiError, SC_ERR_ARG};

    fn line_input() -> Vec<f64> {
        let mut input = vec![0.0; 27];
        input[3] = 1.0;
        input[7..12].fill(1e-5);
        input[14] = 2.0;
        input[16] = 1.0;
        input[22] = 1.0;
        input[23] = 1.0;
        input[26] = 1.0;
        input
    }

    #[test]
    fn rejected_native_constructors_never_reach_topology_builders() {
        // These pass the layout guard. Before the shim checked build(), operations
        // 6 and 7 dereferenced null and terminated the process with SIGSEGV. The
        // constructor's failure status (SM_ERR_INVALID_INPUT, 1007) is now the
        // compared outcome of every curve-consuming operation, alone in the output.
        for kind in [0.0, 2.0] {
            for op in 0..=8 {
                for face_kind in [0.0, 1.0] {
                    let mut input = line_input();
                    input[0] = f64::from(op);
                    input[13] = kind;
                    input[18] = face_kind;
                    if kind == 0.0 {
                        input[23] = 1e-15;
                    } else {
                        input[22] = 1e-15;
                    }
                    let result = eval(&input);
                    if (5..=7).contains(&op) || (op == 8 && face_kind == 1.0) {
                        assert_eq!(result, Ok(vec![1007.0]), "{input:?}");
                    } else {
                        assert!(result.is_ok(), "{input:?}: {result:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn native_operations_cover_every_curve_kind_and_flag_combination() {
        for kind in 0..=5 {
            for op in 0..=8 {
                for analytics in [0.0, 1.0] {
                    for swap in [0.0, 1.0] {
                        for face_kind in [0.0, 1.0] {
                            let mut input = line_input();
                            input[0] = f64::from(op);
                            input[12] = analytics;
                            input[13] = f64::from(kind);
                            input[17] = swap;
                            input[18] = face_kind;
                            // Hermite is not B-spline-derived in this reference.
                            if op == 8 && kind == 3 && face_kind == 1.0 {
                                assert_eq!(eval(&input), Err(FfiError::Status(SC_ERR_ARG)));
                            } else {
                                assert!(eval(&input).is_ok(), "{input:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rejects_layouts_before_native_indexing() {
        assert!(eval(&[]).is_err());
        assert!(eval(&[0.0; 19]).is_err());
        let mut input = vec![0.0; 27];
        input[14] = 2.0;
        input[13] = 1.0;
        input[16] = 1.0;
        input[22] = 1.0;
        input[26] = 1.0;
        input[14] = 1e300;
        assert!(eval(&input).is_err());
        input[14] = 2.5;
        assert!(eval(&input).is_err());
        input[14] = 2.0;
        input[0] = 0.5;
        assert!(eval(&input).is_err());
        input[0] = 5.0;
        input[22] = 0.0;
        assert!(eval(&input).is_err());
        input[22] = 1.0;
        input[19] = f64::NAN;
        assert!(eval(&input).is_err());
    }
}
