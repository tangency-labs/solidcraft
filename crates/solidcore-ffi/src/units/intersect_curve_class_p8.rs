//! Native oracle for the five boundary/discontinuity operations of curve classification p8.
//!
//! Input: `op dim curve_kind surface_kind nu nv Axyz Bx umin vmin umax vmax
//! sx sy By Bz tmin tmax source_tol height_or_param_space interval_count`, followed
//! by `min max mid_class` per interval. Curve kinds: segment, circle (Bx radius),
//! stationary quadratic B-spline. Surface kinds: plane, flat grid, warped grid,
//! flat grid with a discontinuous-volume target; grid counts 2..4. Volume targets
//! are an identity transform (kinds 0/1) or a C0 trilinear B-spline (2/3).
//! Output: native status (0 success), interval count, then `min max` and
//! `class object t gap` for start/mid/end. Objects: -1 null, 1 surface, 2 volume,
//! 3+ temporary objects in first-seen traversal order. Native guards exclude
//! malformed records, nonfinite values, invalid surface domains/scales, degenerate
//! lines and exactly zero circle radii. Finite magnitudes, positive UV spans and
//! intervals outside the curve's natural domain have no additional guard limits;
//! differential coverage is limited to the committed fixtures and generated cases.

use crate::{call_with_buffer, FfiError};
mod sys {
    unsafe extern "C" {
        pub fn sc_intersect_curve_class_p8_run(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Validate the wire record without calling native code.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let bad = || FfiError::InvalidArgument("curve_class_p8 record".into());
    if input.len() < 23 || input.iter().any(|x| !x.is_finite()) {
        return Err(bad());
    }
    let (header, ivls) = input.split_at_checked(23).ok_or_else(bad)?;
    let h: &[f64; 23] = header.try_into().map_err(|_| bad())?;
    let code = |at: usize, lo: f64, hi: f64| {
        h.get(at)
            .is_some_and(|x| x.fract() == 0.0 && (lo..=hi).contains(x))
    };
    if !code(0, 0.0, 4.0)
        || !code(1, 2.0, 3.0)
        || !code(2, 0.0, 2.0)
        || !code(3, 0.0, 3.0)
        || !code(4, 2.0, 4.0)
        || !code(5, 2.0, 4.0)
        || !code(22, 1.0, 16.0)
    {
        return Err(bad());
    }
    if input.len() != 23 + 3 * h[22] as usize
        || h[12] <= h[10]
        || h[13] <= h[11]
        || h[14] <= 0.0
        || h[15] <= 0.0
        || h[18] > h[19]
    {
        return Err(bad());
    }
    if h[2] == 0.0 && h[6] == h[9] && h[7] == h[16] && h[8] == h[17] {
        return Err(bad());
    }
    if h[2] == 1.0 && h[9] == 0.0 {
        return Err(bad());
    }
    let mut expected = h[18];
    for v in ivls.as_chunks::<3>().0 {
        let &[a, b, class] = v;
        if a > b
            || a != expected
            || ![100.0, 107.0, 108.0, 110.0].contains(&class)
            || (h[0] != 4.0 && class == 108.0)
        {
            return Err(bad());
        }
        expected = b;
    }
    if expected != h[19] {
        return Err(bad());
    }

    Ok(())
}

/// Run a validated boundary/discontinuity operation and return status plus interval state.
pub fn run(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: validated input shape; all buffers and the length pointer are live.
        unsafe {
            sys::sc_intersect_curve_class_p8_run(
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
    use super::validate;
    #[test]
    fn rejects_malformed_records_before_native_call() {
        let valid = vec![
            0., 2., 0., 1., 2., 2., 0.2, 0.2, 0., 0.8, 0., 0., 1., 1., 1., 1., 0.8, 0., 0., 1.,
            1e-7, 0., 1., 0., 1., 100.,
        ];
        assert!(validate(&valid).is_ok());
        for at in [0, 1, 2, 3, 4, 5, 22] {
            let mut v = valid.clone();
            v[at] = 0.5;
            assert!(validate(&v).is_err());
        }
        for n in 0..valid.len() {
            assert!(validate(&valid[..n]).is_err());
        }
        let mut v = valid.clone();
        v[23] = 0.1;
        assert!(validate(&v).is_err());
        let mut v = valid.clone();
        v.extend([0., 1., 100.]);
        assert!(validate(&v).is_err());
    }

    #[test]
    fn accepts_native_supported_extreme_records() {
        let valid = vec![
            0., 2., 0., 1., 2., 2., 0.2, 0.2, 0., 0.8, 0., 0., 1., 1., 1., 1., 0.8, 0., 0., 1.,
            1e-7, 0., 1., 0., 1., 100.,
        ];
        for radius in [1e-300, 1e-13, 1e-12, 2e-12, -0.1] {
            let mut v = valid.clone();
            v[2] = 1.;
            v[9] = radius;
            assert!(validate(&v).is_ok(), "radius {radius}");
        }
        for magnitude in [1e13, 1e14] {
            let mut v = valid.clone();
            v[6] = magnitude;
            assert!(validate(&v).is_ok(), "magnitude {magnitude}");
        }
        let mut v = valid.clone();
        v[12] = 1e-8;
        v[13] = 1e-8;
        assert!(validate(&v).is_ok());
        for kind in [0., 1., 2.] {
            let mut v = valid.clone();
            v[2] = kind;
            v[18] = -0.5;
            v[19] = 360.5;
            v[23] = -0.5;
            v[24] = 360.5;
            assert!(validate(&v).is_ok());
        }
        for radius in [0., -0.] {
            let mut v = valid.clone();
            v[2] = 1.;
            v[9] = radius;
            assert!(validate(&v).is_err());
        }
        let mut v = valid;
        v[9] = v[6];
        v[16] = v[7];
        v[17] = v[8];
        assert!(validate(&v).is_err());
    }
}
