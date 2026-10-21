//! Checked access to `SmPrimitiveCreation`'s maps and file-local path helpers.
//!
//! Shim: `harness/cpp/shim/primitive_primitive_creation_p1.cpp`. Each curve is
//! `[kind, x, y, z, a, b, c]`: kind 0 is a B-spline line from xyz to abc;
//! kind 1 is a quadratic circular arc centred on xyz, radius a, angles b/c.
//! Curves are finite and nondegenerate before entering native code. Native
//! preflights reject failed circle construction and the two-control-point G1
//! fallback that would read a third point; large unextended lines stay accepted.
//! Lists contain at most 32 curves; extension distances are bounded by `1e6`.
//! Primitive fixtures use finite coordinates/dimensions of magnitude at most
//! `1e6`, nondegenerate frames and well-resolved planar boxes. Pole-free sphere
//! fixtures use radius at least `0.1` and tolerance `1e-8..=1e-3`; elliptic-ended
//! cones use radii/height above `0.1` and tilts below ten degrees. These are the
//! fixture construction domain, not a claim of parity for other primitive inputs.
//! Selector 8 optionally appends rear X tilt; selector 9 optionally appends a
//! curve count and seven-number curve blocks. Op 6 builds up to four faces per
//! operand (planes or unit spheres) for `Boolean2D` validation; its layout is in
//! `fixtures/primitive.primitive_creation_p1/README.md`.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_primitive_primitive_creation_p1_eval(
            op: i32,
            data: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn bad() -> FfiError {
    FfiError::InvalidArgument("primitive p1 layout or geometry".into())
}
fn integer(v: f64, min: f64, max: f64) -> bool {
    v.is_finite() && v.fract() == 0.0 && v >= min && v <= max
}
fn value(data: &[f64], index: usize) -> f64 {
    data.get(index).copied().unwrap_or(f64::NAN)
}
fn curve(v: &[f64]) -> bool {
    let [kind, x, y, z, a, b, c] = v else {
        return false;
    };
    if v.iter().any(|v| !v.is_finite()) {
        return false;
    }
    if *kind == 0.0 {
        (*a - *x).powi(2) + (*b - *y).powi(2) + (*c - *z).powi(2) > 0.0
    } else {
        *kind == 1.0
            && *a >= 1e-15
            && (-360.0..=360.0).contains(b)
            && (-360.0..=360.0).contains(c)
            && c > b
            && c - b <= 360.0
    }
}

/// Check a wire-layout input before C++ (also used by the Rust runner).
pub fn validate(op: i32, data: &[f64]) -> Result<(), FfiError> {
    if data.iter().any(|x| !x.is_finite()) {
        return Err(bad());
    }
    let valid = match op {
        0 => {
            data.len() == 20
                && integer(value(data, 0), -1e6, 1e6)
                && data
                    .get(1..4)
                    .unwrap_or_default()
                    .iter()
                    .all(|v| integer(*v, 0.0, 1.0))
                && data
                    .get(4..)
                    .unwrap_or_default()
                    .iter()
                    .all(|v| integer(*v, -1.0, 3.0))
        }
        1 => {
            data.len() == 15
                && curve(data.get(..7).unwrap_or_default())
                && curve(data.get(7..14).unwrap_or_default())
        }
        2 => {
            data.len() >= 4
                && integer(value(data, 0), 0.0, 1.0)
                && integer(value(data, 1), 0.0, 1.0)
                && value(data, 2).abs() <= 1e6
                && integer(value(data, 3), 0.0, 32.0)
                && data.len() == 4 + 7 * value(data, 3) as usize
                && data
                    .get(4..)
                    .unwrap_or_default()
                    .as_chunks::<7>()
                    .0
                    .iter()
                    .all(|chunk| curve(chunk))
        }
        3 => {
            data.len() >= 10
                && integer(value(data, 0), 0.0, 32.0)
                && integer(value(data, 1), 0.0, 32.0)
                && data.len() == 10 + 7 * (value(data, 0) + value(data, 1)) as usize
                && data
                    .get(3..)
                    .unwrap_or_default()
                    .as_chunks::<7>()
                    .0
                    .iter()
                    .all(|chunk| curve(chunk))
        }
        4 => {
            if data.len() < 19
                || !integer(value(data, 0), 0.0, 10.0)
                || (value(data, 0) != 8.0 && value(data, 0) != 9.0 && data.len() != 19)
                || data.iter().any(|v| v.abs() > 1e6)
            {
                false
            } else {
                let shape = value(data, 0) as i32;
                let x = [value(data, 11), value(data, 12), value(data, 13)];
                let y = [value(data, 14), value(data, 15), value(data, 16)];
                let cross = [
                    x[1] * y[2] - x[2] * y[1],
                    x[2] * y[0] - x[0] * y[2],
                    x[0] * y[1] - x[1] * y[0],
                ];
                integer(value(data, 17), 0.0, 1.0)
                    && integer(value(data, 18), 0.0, 1.0)
                    && cross.iter().map(|v| v * v).sum::<f64>() > 1e-12
                    && match shape {
                        0 => value(data, 1) > 1e-4 && value(data, 2) > 1e-4,
                        2 => data.get(1..4).unwrap_or_default().iter().all(|v| *v > 1e-4),
                        // Native preflight checks the exact three-point circles
                        // after the primitive's own validation early returns.
                        1 | 3..=6 => data.len() == 19,
                        9 => {
                            data.len() == 19
                                || (integer(value(data, 19), 0.0, 32.0)
                                    && data.len() == 20 + 7 * value(data, 19) as usize
                                    && data
                                        .get(20..)
                                        .unwrap_or_default()
                                        .as_chunks::<7>()
                                        .0
                                        .iter()
                                        .all(|chunk| curve(chunk)))
                        }
                        7 => {
                            value(data, 1) >= 0.1
                                && value(data, 7) >= 1e-8
                                && value(data, 7) <= 1e-3
                        }
                        8 => {
                            (data.len() == 19 || (data.len() == 20 && value(data, 19).abs() < 10.0))
                                && value(data, 1) > 0.1
                                && value(data, 2) > 0.1
                                && value(data, 3) > 0.1
                                && data
                                    .get(4..7)
                                    .unwrap_or_default()
                                    .iter()
                                    .all(|v| v.abs() < 10.0)
                        }
                        10 => integer(value(data, 1), 0.0, 4.0),
                        _ => false,
                    }
            }
        }
        5 => {
            data.len() == 13
                && curve(data.get(..7).unwrap_or_default())
                && data
                    .get(7..)
                    .unwrap_or_default()
                    .iter()
                    .all(|v| v.abs() < 1e8)
                && data
                    .get(10..)
                    .unwrap_or_default()
                    .iter()
                    .map(|v| v * v)
                    .sum::<f64>()
                    > 1e-20
        }
        6 => {
            data.len() >= 7
                && integer(value(data, 0), 0.0, 4.0)
                && data
                    .get(1..4)
                    .unwrap_or_default()
                    .iter()
                    .all(|v| integer(*v, 0.0, 1.0))
                && integer(value(data, 4), 0.0, 4.0)
                && integer(value(data, 5), 0.0, 4.0)
                && (1e-8..=1e-3).contains(&value(data, 6))
                && data.len() == 7 + 7 * (value(data, 4) + value(data, 5)) as usize
                && data
                    .get(7..)
                    .unwrap_or_default()
                    .as_chunks::<7>()
                    .0
                    .iter()
                    .all(|face| {
                        integer(face[0], 0.0, 1.0)
                            && face[1].abs() <= 180.0
                            && face.iter().skip(2).all(|v| v.abs() <= 100.0)
                            && face[5] - face[3] >= 0.1
                            && face[6] - face[4] >= 0.1
                    })
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(bad())
    }
}

/// Run a validated helper call. The record includes map lookups, or every output
/// curve's interval and position/tangent at five parameter samples.
pub fn eval(op: i32, data: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, data)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: validation bounds every native read and count; output storage
        // and its length pointer are supplied by call_with_buffer.
        unsafe {
            sys::sc_primitive_primitive_creation_p1_eval(
                op,
                data.as_ptr(),
                data.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_boundary_rejects_null_and_short_layouts() {
        for op in 0..=6 {
            let mut length = 0;
            // SAFETY: each call must reject before reading the null input or
            // output pointer, because the layout is empty.
            let status = unsafe {
                sys::sc_primitive_primitive_creation_p1_eval(
                    op,
                    core::ptr::null(),
                    0,
                    core::ptr::null_mut(),
                    0,
                    &raw mut length,
                )
            };
            assert_eq!(status, crate::SC_ERR_ARG);
            assert!(eval(op, &[]).is_err());
        }
    }
    #[test]
    fn copies_all_maps_into_a_seeded_native_sweep() -> Result<(), FfiError> {
        let mut data = vec![19.0, 1.0, 0.0, 1.0];
        data.extend([
            1.0, -1.0, 3.0, 0.0, -1.0, 2.0, -1.0, 0.0, 3.0, 0.0, 1.0, 2.0, 2.0, -1.0, -1.0, 1.0,
        ]);
        let record = eval(0, &data)?;
        assert_eq!(record.len(), 42);
        assert_eq!(record.get(2..22), record.get(22..42));
        if let Some(flag) = data.get_mut(2) {
            *flag = 1.0;
        }
        let cleared = eval(0, &data)?;
        for map in cleared.get(2..).unwrap_or_default().as_chunks::<5>().0 {
            assert_eq!(map, &[0.0, -1.0, -1.0, -1.0, -1.0]);
        }
        Ok(())
    }
    #[test]
    fn extension_preflight_only_rejects_the_undefined_stage() -> Result<(), FfiError> {
        let line = [0.0, 0.0, 0.0, 1e300, 1.0, 1.0, 0.0];
        for (closed, periodic, rejected) in [(0.0, 0.0, false), (1.0, 1.0, false), (1.0, 0.0, true)]
        {
            let mut data = vec![closed, periodic, 0.1, 1.0];
            data.extend(line);
            let result = eval(2, &data);
            if rejected {
                assert!(matches!(result, Err(FfiError::Status(-1010))));
            } else {
                assert!(!result?.is_empty());
            }
        }
        Ok(())
    }
    #[test]
    fn review_layouts_bound_every_appended_record() {
        let mut cone = vec![
            8.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, -9.0,
        ];
        assert!(validate(4, &cone).is_ok());
        cone[19] = 10.0;
        assert!(validate(4, &cone).is_err());
        cone[0] = 9.0;
        cone[19] = 1.0;
        assert!(validate(4, &cone).is_err());
        cone.extend([0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        assert!(validate(4, &cone).is_ok());
        cone[19] = 33.0;
        assert!(validate(4, &cone).is_err());
        let mut boolean = vec![
            0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1e-5, 0.0, 0.01, 0.0, 0.0, 0.0, 1.0, 1.0,
        ];
        assert!(validate(6, &boolean).is_ok());
        boolean[5] = 1.0;
        assert!(validate(6, &boolean).is_err());
        boolean[5] = 0.0;
        boolean[4] = 5.0;
        assert!(validate(6, &boolean).is_err());
    }

    #[test]
    fn guard_rejects_short_inputs_and_oversized_lists() {
        for op in 0..=6 {
            assert!(validate(op, &[]).is_err());
        }
        assert!(validate(2, &[0.0, 0.0, 1.0, 33.0]).is_err());
        assert!(validate(1, &[0.0; 15]).is_err());
        assert!(validate(3, &[0.0; 10]).is_err());
    }
}
