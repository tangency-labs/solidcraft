//! Checked native primitive p3 fixtures. Op 0 is the actual file-local ruled
//! offset helper; ops 1..6 invoke the public sweep/skin/swung entry points.
//! Op 0 has 20 settings followed by three seven-value B-spline curve records.
//! Other ops have ten settings followed by at most 32 curve records. Curves use
//! p1's `[kind, x, y, z, a, b, c]` line/arc layout. Native construction and finite
//! sample preflights reject geometry that cannot be represented by the fixture.

#![allow(clippy::indexing_slicing)] // fixed fixture layout validated before indexing

use crate::{call_with_buffer, FfiError};
mod sys {
    extern "C" {
        pub fn sc_primitive_primitive_creation_p3_eval(
            op: i32,
            data: *const f64,
            n: i64,
            output: *mut f64,
            cap: i64,
            length: *mut i64,
        ) -> i32;
    }
}
fn bad() -> FfiError {
    FfiError::InvalidArgument("primitive p3 layout or geometry".into())
}
fn integer(x: f64, max: f64) -> bool {
    x.is_finite() && x.fract() == 0.0 && (0.0..=max).contains(&x)
}
fn curve(data: &[f64]) -> bool {
    let [kind, x, y, z, a, b, c] = data else {
        return false;
    };
    if !data.iter().all(|v| v.is_finite()) {
        return false;
    }
    if *kind == 0.0 {
        x != a || y != b || z != c
    } else {
        *kind == 1.0 && *a > 0.0 && c > b && c - b <= 360.0 && b.abs() <= 360.0 && c.abs() <= 360.0
    }
}
/// Validate shapes and indices before native indexing or pointer dereferences.
pub fn validate(op: i32, data: &[f64]) -> Result<(), FfiError> {
    if !data.iter().all(|v| v.is_finite()) {
        return Err(bad());
    }
    if op == 0 {
        let [corner, has_map, index, other, intersection, missing, unmarried0, unmarried1, _tol, _same, _offset, _ox, _oy, _oz, xx, xy, xz, yx, yy, yz, rest @ ..] =
            data
        else {
            return Err(bad());
        };
        if data.len() != 41
            || ![
                *corner,
                *has_map,
                *index,
                *other,
                *intersection,
                *missing,
                *unmarried0,
                *unmarried1,
            ]
            .iter()
            .all(|v| integer(*v, 1.0))
            || !rest.as_chunks::<7>().0.iter().all(|chunk| curve(chunk))
        {
            return Err(bad());
        }
        let x2 = xx * xx + xy * xy + xz * xz;
        let y2 = yx * yx + yy * yy + yz * yz;
        if !x2.is_finite() || !y2.is_finite() || x2 <= 1e-300 || y2 <= 1e-300 {
            return Err(bad());
        }
    } else if (1..=7).contains(&op) {
        if data.len() < 10
            || !(data.len() - 10).is_multiple_of(7)
            || data.len() > 10 + 32 * 7
            || !data
                .get(10..)
                .unwrap_or_default()
                .as_chunks::<7>()
                .0
                .iter()
                .all(|chunk| curve(chunk))
        {
            return Err(bad());
        }
        match op {
            1 if !integer(data[4], 4096.0) || !integer(data[5], 1.0) || !integer(data[6], 1.0) => {
                return Err(bad())
            }
            3 if !integer(data[7], 4096.0) || !integer(data[8], 1.0) || !integer(data[9], 1.0) => {
                return Err(bad())
            }
            4 if data.len() == 10 || !integer(data[1], 16.0) || data[1] < 1.0 => return Err(bad()),
            5 if !integer(data[1], 1.0) => return Err(bad()),
            6 if !integer(data[2], ((data.len() - 10) / 7) as f64) => return Err(bad()),
            7 if data.len() != 10
                || !integer(data[0], 4.0)
                || !integer(data[1], 3.0)
                || !integer(data[2], 2.0) =>
            {
                return Err(bad())
            }
            _ => {}
        }
    } else {
        return Err(bad());
    }
    Ok(())
}
/// Call the native entry point. The returned record includes status, partial
/// curves, marriage flags and the ruled surface samples when one was produced.
pub fn eval(op: i32, data: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, data)?;
    call_with_buffer(256, |output, length| {
        // SAFETY: layout, curve counts, flags and indices checked above; all
        // input/output buffers remain live for the complete synchronous call.
        unsafe {
            sys::sc_primitive_primitive_creation_p3_eval(
                op,
                data.as_ptr(),
                data.len() as i64,
                output.as_mut_ptr(),
                output.len() as i64,
                &raw mut *length,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn related_indices_and_short_records_are_rejected() {
        assert!(eval(0, &[0.0; 40]).is_err());
        let mut values = vec![0.0; 41];
        values[2] = 2.0;
        assert!(eval(0, &values).is_err());
        assert!(eval(3, &[0.0; 9]).is_err());
        assert!(eval(4, &[0., 3., 0., 0., 0., 0., 0., 0., 0., 0.]).is_err());
    }
}
