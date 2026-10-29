//! Safe wrappers for the real `SmIntegrator` / `SmODEIntegrator` C++ implementation.
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_mass_integrator_eval(
            op: i32,
            kind: i32,
            coeff: *const f64,
            initial: *const f64,
            n: i64,
            scale: *const f64,
            ns: i64,
            controls: *const f64,
            max_steps: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Executes scalar integration (0), ODE integration (1), or an adaptive step (2).
/// Evaluator kinds: polynomial/linear ODE (0), sinusoidal (1), exponential (2),
/// or polynomial returning `SM_ERR` at `t >= coeff[0]` (3). Four coefficients per
/// component. Controls: start, end, accuracy, initial step, minimum step, tiny,
/// algorithm selector. Output starts with the actual upstream status.
pub fn eval(
    op: i32,
    kind: i32,
    coefficients: &[f64],
    initial: &[f64],
    scale: &[f64],
    controls: &[f64; 7],
    max_steps: u32,
) -> Result<Vec<f64>, FfiError> {
    let n = initial.len();
    let early = op == 1 && controls[0] == controls[1];
    if !(0..=2).contains(&op)
        || !(0..=3).contains(&kind)
        || n > 4096
        || coefficients.len() != 4 * n
        || (!scale.is_empty() && scale.len() != n)
        || (op == 0 && n != 1)
        || (op == 2 && scale.len() != n)
        || max_steps > 1_000_000
        || (!early
            && (!coefficients.iter().all(|v| v.is_finite())
                || (op == 2 && !initial.iter().all(|v| v.is_finite()))
                || scale.iter().any(|v| v.is_nan())
                || controls.iter().enumerate().any(|(i, v)| {
                    if i == 2 {
                        v.is_nan()
                    } else {
                        !v.is_finite()
                    }
                })))
    {
        return Err(FfiError::InvalidArgument(
            "integrator dimensions/controls".into(),
        ));
    }
    call_with_buffer(2 * n + 8, |out, len| {
        // SAFETY: every input dimension is validated above; C++ reads seven controls
        // and writes at most out.len() values using sc_copy_out; len is writable.
        unsafe {
            sys::sc_mass_integrator_eval(
                op,
                kind,
                coefficients.as_ptr(),
                initial.as_ptr(),
                n as i64,
                scale.as_ptr(),
                scale.len() as i64,
                controls.as_ptr(),
                i64::from(max_steps),
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}

mod geometry_sys {
    extern "C" {
        pub fn sc_mass_integrator_edge(
            op: i32,
            kind: i32,
            p: *const f64,
            weights: *const f64,
            controls: *const f64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
        pub fn sc_mass_integrator_face(
            op: i32,
            kind: i32,
            p: *const f64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
/// Precise properties (0), API length (1), or B-rep properties (2), on a real edge.
/// Kind 0 is a rational quadratic Bezier with stored control coordinates and weights;
/// kind 1 is a real `SmLine` (p0 and direction p1). Controls: t0, t1, accuracy,
/// computation origin xyz, tube radius. The interval must lie within [0,1].
pub fn edge(
    op: i32,
    kind: i32,
    p: &[f64; 9],
    weights: &[f64; 3],
    controls: &[f64; 7],
) -> Result<Vec<f64>, FfiError> {
    let [a, b, _, _, _, _, _] = *controls;
    if !(0..=2).contains(&op)
        || !(0..=1).contains(&kind)
        || a < 0.0
        || a > b
        || b > 1.0
        || !p
            .iter()
            .chain(weights)
            .chain(controls)
            .all(|v| v.is_finite())
        || weights.iter().any(|w| *w <= 1e-12)
        || (kind == 1 && p.iter().skip(3).take(3).all(|v| *v == 0.0))
    {
        return Err(FfiError::InvalidArgument("edge geometry".into()));
    }
    call_with_buffer(32, |out, len| {
        // SAFETY: fixed-size inputs checked above; sc_copy_out bounds the output.
        unsafe {
            geometry_sys::sc_mass_integrator_edge(
                op,
                kind,
                p.as_ptr(),
                weights.as_ptr(),
                controls.as_ptr(),
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}
/// Face kinds: rectangular plane (0), disk (1), annulus (2), quadratic B-spline
/// patch (3), rotated plane (4), multi-span rational quadratic B-spline patch (5: u knot
/// 0.4, double v knot 0.5). Disk radii are 0.5 and 0.2.
/// Precise face properties (0), actual oriented trim export (1), API area
/// (2), centroid (3), B-rep area (4), volume (5), mass properties (6), rectangular surface area (7).
/// Input: plane origin xyz, UV scales, domain u0 v0 u1 v1, computation origin xyz,
/// accuracy, thickness, orientation (0 same, 1 opposite), precise-property flags.
pub fn face(op: i32, kind: i32, p: &[f64; 16]) -> Result<Vec<f64>, FfiError> {
    let [_, _, _, sx, sy, u0, v0, u1, v1, _, _, _, _, _, orientation, flags] = *p;
    if !(0..=7).contains(&op)
        || !(0..=5).contains(&kind)
        || !p.iter().all(|v| v.is_finite())
        || ((kind == 1 || kind == 2)
            && (sx != 1.0 || sy != 1.0 || u0 > -0.5 || v0 > -0.5 || u1 < 0.5 || v1 < 0.5))
        || (kind == 3 && (u0 < 0.0 || v0 < 0.0 || u1 > 1.0 || v1 > 1.0))
        || sx <= 0.0
        || sy <= 0.0
        || u0 >= u1
        || v0 >= v1
        || !(0.0..=f64::from(u32::MAX)).contains(&flags)
        || flags.fract() != 0.0
        || !(orientation == 0.0 || orientation == 1.0)
    {
        return Err(FfiError::InvalidArgument("plane face geometry".into()));
    }
    call_with_buffer(128, |out, len| {
        // SAFETY: all 16 input values are present and valid; output is bounded.
        unsafe {
            geometry_sys::sc_mass_integrator_face(
                op,
                kind,
                p.as_ptr(),
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}
