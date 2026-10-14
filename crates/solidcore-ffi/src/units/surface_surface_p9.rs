//! Safe wrappers for `surface.surface_p9`; shim: `surface_surface_p9.cpp`.
//! Upstream: `source/SMLib/src/SmSurface.cpp` (tessellation, iso boundaries and CSI).
//! Inputs are one non-rational quadratic 3x3 Bezier patch or a real unit plane,
//! a domain within [0,1]^2,
//! bounded controls and, for CSI, a non-degenerate line. No counts cross the FFI.
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p9_eval(
            op: i32,
            analytic_plane: i32,
            xyz: *const f64,
            bounds: *const f64,
            direction: i32,
            chord: f64,
            angle: f64,
            max_side: f64,
            diagonals: i32,
            line: *const f64,
            output: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Evaluates a bounded surface case: op 0 splits, 1 iso boundaries, 2 CSI leaf solve.
///
/// # Errors
/// Invalid data is rejected before C++; shim failures return `FfiError::Status`.
pub fn eval(
    op: i32,
    analytic_plane: bool,
    net: &[[f64; 3]; 9],
    domain: [f64; 4],
    direction: i32,
    controls: [f64; 3],
    diagonals: bool,
    line: [[f64; 3]; 2],
) -> Result<Vec<f64>, FfiError> {
    eval_surface_kind(
        op,
        i32::from(analytic_plane),
        net,
        domain,
        direction,
        controls,
        diagonals,
        line,
    )
}

/// Evaluates a surface kind: 0 quadratic B-spline, 1 plane, 2 canonical revolution.
/// Revolution fixtures cover splits and iso boundaries; CSI uses kind 0 only.
///
/// # Errors
/// Invalid bounded records or C++ setup failures.
pub fn eval_surface_kind(
    op: i32,
    surface_kind: i32,
    net: &[[f64; 3]; 9],
    domain: [f64; 4],
    direction: i32,
    controls: [f64; 3],
    diagonals: bool,
    line: [[f64; 3]; 2],
) -> Result<Vec<f64>, FfiError> {
    let invalid = || FfiError::InvalidArgument("unsafe surface p9 input".into());
    if !(0..=2).contains(&surface_kind)
        || (surface_kind == 2 && op == 2)
        || !(0..=2).contains(&op)
        || !(0..=9).contains(&direction)
        || net
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 10.0)
        || domain
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || domain[2] - domain[0] < 1e-4
        || domain[3] - domain[1] < 1e-4
        || controls.iter().any(|v| !v.is_finite())
        || (controls[0] != 0.0 && !(1e-3..=10.0).contains(&controls[0].abs()))
        || (controls[1] != 0.0 && !(1.0..=90.0).contains(&controls[1].abs()))
        || (controls[2] != 0.0 && !(0.05..=100.0).contains(&controls[2].abs()))
        || line
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 20.0)
    {
        return Err(invalid());
    }
    if op == 0 && direction > 1 {
        return Err(invalid());
    }
    if op == 2 && line[0] == line[1] {
        return Err(invalid());
    }
    let xyz: Vec<f64> = net.iter().flatten().copied().collect();
    let ray: Vec<f64> = line.iter().flatten().copied().collect();
    call_with_buffer(128, |buffer, len| {
        // SAFETY: fixed input lengths, finite bounded values and valid enumerators
        // were checked above; the shim copies output only within buffer capacity.
        unsafe {
            sys::sc_surface_surface_p9_eval(
                op,
                surface_kind,
                xyz.as_ptr(),
                domain.as_ptr(),
                direction,
                controls[0],
                controls[1],
                controls[2],
                i32::from(diagonals),
                ray.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(all(test, feature = "cpp"))]
mod tests {
    use super::*;
    #[test]
    fn rejects_only_the_undefined_all_degenerate_count() {
        let net = [[0.0; 3]; 9];
        let line = [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0]];
        assert!(matches!(
            eval(
                0,
                false,
                &net,
                [0.0, 0.0, 1.0, 1.0],
                0,
                [0.02, 15.0, 0.5],
                false,
                line
            ),
            Err(FfiError::Status(_))
        ));
        for max_side in [1.0, 2.0, -0.5] {
            assert_eq!(
                eval(
                    0,
                    false,
                    &net,
                    [0.0, 0.0, 1.0, 1.0],
                    0,
                    [0.02, 15.0, max_side],
                    false,
                    line
                )
                .unwrap(),
                vec![0.0, 0.0, 1.0]
            );
        }
    }
}
