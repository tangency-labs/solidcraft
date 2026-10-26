//! Safe wrappers for `surface.surface_p9`; shim: `surface_surface_p9.cpp`.
//! Upstream: `source/SMLib/src/SmSurface.cpp` (tessellation, iso boundaries and CSI).
//! Inputs are one non-rational quadratic 3x3 Bezier patch, a real unit plane, a
//! canonical revolution, a cylinder or a canonical cone, finite values (a split/iso domain within [0,1]^2) and,
//! for CSI, a non-degenerate line. No counts cross the FFI.
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

/// Evaluates a surface kind: 0 quadratic B-spline, 1 plane, 2 canonical revolution,
/// 3 cylinder, 4 canonical cone (for 3 and 4 the domain holds fractions of the
/// natural domain). Revolution and cone kinds cover splits and iso boundaries; CSI
/// uses kinds 0 and 1 only.
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
    // Only finite values are required: scale, chord height, maximum side and
    // the CSI domain (disjoint, touching or sliver) are all defined in C++.
    // The angle range and the [0,1] split/iso domain stay bounded as before.
    if !(0..=6).contains(&surface_kind)
        // The one-node Rust CSI harness tree does not model `BuildTree`, which
        // subdivides a revolution (or cone); keep those CSI out of the comparison.
        || (surface_kind >= 2 && op == 2)
        || !(0..=2).contains(&op)
        || !(0..=9).contains(&direction)
        || net.iter().flatten().any(|v| !v.is_finite())
        || domain.iter().any(|v| !v.is_finite())
        || (op != 2
            && (domain.iter().any(|v| !(0.0..=1.0).contains(v))
                || domain[2] - domain[0] < 1e-4
                || domain[3] - domain[1] < 1e-4))
        || controls.iter().any(|v| !v.is_finite())
        || (controls[1] != 0.0 && !(1.0..=90.0).contains(&controls[1].abs()))
        || line.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(invalid());
    }
    // `FindTessellationSplits` converts `max_length / max_side + 1` to ULONG and
    // allocates that many splits. An iso curve of a 3x3 net is no longer than its
    // control polygon (< 7 * max |net|), so this keeps the count finite and below
    // the Rust port's documented 1,000,000-split limit.
    let net_max = net.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
    // The cone kinds' iso curves are shorter than 10 (sweep circle of radius 1.5).
    let size = if surface_kind >= 3 { 10.0 } else { net_max };
    if op == 0 && controls[2] > 0.0 && size / controls[2] > 1e5 {
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

    #[test]
    fn accepts_scale_and_csi_domain_rejects_unbounded_split_count() {
        let mut net = [[0.0; 3]; 9];
        for (index, p) in net.iter_mut().enumerate() {
            *p = [(index / 3) as f64 * 5e5, (index % 3) as f64 * 5e5, 0.0];
        }
        let line = [[5e5, 5e5, -5e6], [5e5, 5e5, 5e6]];
        let unit = [0.0, 0.0, 1.0, 1.0];
        assert!(eval(0, false, &net, unit, 0, [2e4, 15.0, 1e6], false, line).is_ok());
        assert!(eval(1, false, &net, unit, 0, [1e-5, 0.0, 0.0], false, line).is_ok());
        assert!(eval(
            2,
            false,
            &net,
            [2.0, 2.0, 3.0, 3.0],
            0,
            [0.0; 3],
            false,
            line
        )
        .is_ok());
        assert!(matches!(
            eval(0, false, &net, unit, 0, [0.0, 0.0, 1.0], false, line),
            Err(FfiError::InvalidArgument(_))
        ));
        assert!(matches!(
            eval(
                0,
                false,
                &net,
                [2.0, 2.0, 3.0, 3.0],
                0,
                [0.0; 3],
                false,
                line
            ),
            Err(FfiError::InvalidArgument(_))
        ));
    }
}
