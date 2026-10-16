//! Native oracle for `intersect.curve_class_p5` (`SmCurveClass.cpp`).
//!
//! Flat input: `op shape dim_minus_2 ignore flag1 flag2 flag3 target index scale
//! source_zone angle n_intervals n_solutions optional_xsect_tol other_n_intervals`,
//! followed by both
//! classifications' boundaries, then their point-class codes (`3` per interval),
//! then solutions (`type start_t start_other end_t end_other start_gap end_gap`).
//! A negative optional tolerance selects upstream's default. Shapes are line/circle,
//! or a stationary quadratic source/target/both;
//! targets are real line/plane/vertex/face. Counts and native undefined branches are
//! checked before entry. Output contains status, operation outputs, both full interval
//! states and every mutable solution end. No native pointers cross the boundary.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p5_run(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn invalid() -> FfiError {
    FfiError::InvalidArgument("invalid curve_class_p5 input or undefined native path".into())
}

/// Validates a bounded oracle input before native execution.
///
/// Native topology mutation on bare objects and the
/// null-edge short-range tangency bug are excluded. Valid non-topology insertion,
/// extraction, vertex projection and merge skip paths remain covered.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    if input.iter().any(|x| !x.is_finite() || x.abs() > 1e21) {
        return Err(invalid());
    }
    let &[op, shape, dim, ignore, flag1, flag2, flag3, target, index, scale, tol, _angle, n, ns, _opt_tol, other_n] =
        input.first_chunk::<16>().ok_or_else(invalid)?;
    let int = |x: f64, max: u32| -> Result<usize, FfiError> {
        if x < 0.0 || x > f64::from(max) || x.fract() != 0.0 {
            return Err(invalid());
        }
        Ok(x as usize)
    };
    let op = int(op, 6)?;
    int(shape, 4)?;
    int(dim, 1)?;
    for flag in [ignore, flag1, flag2, flag3] {
        int(flag, 1)?;
    }
    let target = int(target, 3)?;
    int(index, 1)?;
    let n = int(n, 32)?;
    let ns = int(ns, 32)?;
    let other_n = int(other_n, 32)?;
    if input.len() != 18 + 4 * (n + other_n) + 7 * ns || scale <= 0.0 || scale > 1e6 || tol < 0.0 {
        return Err(invalid());
    }
    for (start, count) in [(16, n), (17 + n, other_n)] {
        let bounds = input.get(start..start + count + 1).ok_or_else(invalid)?;
        if bounds.windows(2).any(|w| matches!(w,[a,b] if a>=b)) {
            return Err(invalid());
        }
    }
    let at = 18 + n + other_n;
    let class_count = 3 * (n + other_n);
    let classes = input.get(at..at + class_count).ok_or_else(invalid)?;
    for &class in classes {
        let valid = class == 100.0
            || class == 110.0
            || (class == 106.0 && target == 0)
            || (class == 107.0 && target == 1)
            || (class == 104.0 && target == 2)
            || (class == 102.0 && target == 3);
        if !valid || ((op == 4 || op == 5) && (101.0..=104.0).contains(&class)) {
            return Err(invalid());
        }
    }
    if ((op == 0 || op == 1) && n == 0 && ns > 0)
        || (op == 3 && (n == 0 || other_n == 0))
        || (op == 4 && n != other_n)
        || (op == 6 && flag1 != 0.0 && flag2 != 0.0)
    {
        return Err(invalid());
    }

    for &[typ, t0, t1, t2, t3, gap0, gap1] in input
        .get(at + class_count..)
        .ok_or_else(invalid)?
        .as_chunks::<7>()
        .0
    {
        int(typ, 4)?;
        let _ = (t0, t1, t2, t3); // Finite curve parameters may span any subdomain.
        if gap0 < 0.0 || gap1 < 0.0 {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Executes a validated oracle input; native guards also reject dynamically unsafe paths.
pub fn run(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: input's exact size, every count/code and all native preconditions
        // above are validated; output and length have caller-owned valid storage.
        unsafe {
            sys::sc_intersect_curve_class_p5_run(
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
    fn rejects_malformed_lengths_counts_and_null_edge_tangency() {
        assert!(run(&[]).is_err());
        let mut v = vec![
            0., 0., 1., 1., 0., 0., 0., 0., 0., 1., 0., 0., 1., 1., -1., 1., 0., 1., 0., 1., 100.,
            100., 100., 100., 100., 100., 2., 0., 0., 0.001, 0.001, 0., 0.,
        ];
        assert!(run(&v).is_err());
        v[12] = 1e20;
        assert!(run(&v).is_err());
        v[12] = 1.;
        v.pop();
        assert!(run(&v).is_err());
    }
}
