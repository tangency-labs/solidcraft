//! Safe native oracle for curve classification part 9.
//!
//! The 22-scalar header is `op surface cache check3d cross big_gaps uv_okay defaults
//! dim_minus_2 rectangle scale zone nv ne lo hi source_start[3] source_end[3]`.
//! It is followed by `nv` UV/gap triples and `ne` edge records:
//! `start[3] end[3] lo hi same_source edge_zone`. Coordinates are scaled in 3D;
//! source coordinates stay in UV for trim-boundary operations. Cache states are
//! unbuilt, one leaf, modified, containment missing, two u leaves, under construction,
//! and two v leaves.
//! Counts, domains, flags and native ownership preconditions are checked on both sides.

use crate::{call_with_buffer, FfiError};
mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p9_run(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
fn invalid() -> FfiError {
    FfiError::InvalidArgument("invalid curve_class_p9 recipe".into())
}
/// Validates exact lengths and bounded native geometry/ownership before allocation.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let header = input.first_chunk::<22>().ok_or_else(invalid)?;
    if input.iter().any(|x| !x.is_finite()) {
        return Err(invalid());
    }
    let int = |x: f64, max: u32| -> Result<usize, FfiError> {
        if x < 0. || x > f64::from(max) || x.fract() != 0. {
            Err(invalid())
        } else {
            Ok(x as usize)
        }
    };
    for (j, &v) in header.iter().take(10).enumerate() {
        int(
            v,
            if j == 2 {
                6
            } else if j == 1 {
                2
            } else {
                1
            },
        )?;
    }
    let nv = int(header[12], 8)?;
    let ne = int(header[13], 8)?;
    if input.len() != 22 + 3 * nv + 10 * ne
        || header[10] <= 0.
        || header[11] < 0.
        || header[14] > header[15]
        || (header[0] == 1. && (header[8] != 0. || ne != 0))
        || (header[9] != 0. && nv != 0)
        || (header[0] == 0. && (nv != 0 || header[9] != 0.))
    {
        return Err(invalid());
    }
    if header[16..19]
        .iter()
        .zip(&header[19..22])
        .all(|(a, b)| a == b)
        || (header[8] == 0. && (header[18] != 0. || header[21] != 0.))
    {
        return Err(invalid());
    }
    let scale = header[10];
    let source_scale = if header[0] == 1. { 1. } else { scale };
    if header[16..22]
        .iter()
        .any(|x| !(x * source_scale).is_finite())
        || input
            .get(22..22 + 3 * nv)
            .ok_or_else(invalid)?
            .iter()
            .any(|x| !(x * scale).is_finite())
    {
        return Err(invalid());
    }
    let mut shared = 0;
    for e in input
        .get(22 + 3 * nv..)
        .ok_or_else(invalid)?
        .as_chunks::<10>()
        .0
    {
        int(e[8], 1)?;
        if e[6] > e[7] || e[9] < 0. || e[..6].iter().any(|x| !(x * scale).is_finite()) {
            return Err(invalid());
        }
        if e[8] == 1. {
            shared += 1;
            if header[8] != 1. {
                return Err(invalid());
            }
        } else if e[..3].iter().zip(&e[3..6]).all(|(a, b)| a == b) {
            return Err(invalid());
        }
    }
    // A native owned curve can belong to only one edge; preserve real pointer identity.
    if shared > 1 {
        return Err(invalid());
    }
    Ok(())
}
/// Runs the native methods on validated real geometry and complete topology.
pub fn run(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(8192, |out, len| {
        // SAFETY: exact input lengths, counts, flags, finite values and ownership are
        // checked above; the shim repeats those guards. Buffers are caller-owned.
        unsafe {
            sys::sc_intersect_curve_class_p9_run(
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
    fn guards_counts_and_shared_curve_ownership() {
        let mut i = vec![
            0., 0., 0., 0., 0., 0., 1., 0., 1., 0., 1., 1e-6, 0., 0., 0., 1., 0., 0., 0., 1., 0.,
            0.,
        ];
        assert!(validate(&i).is_ok());
        i[13] = 1e12;
        assert!(validate(&i).is_err());
        i[13] = 1.;
        assert!(validate(&i).is_err());
        i.extend([0., 0., 0., 1., 0., 0., 0., 1., 1., 1e-6]);
        assert!(validate(&i).is_ok());
        i[13] = 2.;
        i.extend([0., 0., 0., 1., 0., 0., 0., 1., 1., 1e-6]);
        assert!(validate(&i).is_err());
    }
    #[test]
    fn accepts_defined_inputs_beyond_the_old_fixture_ranges() {
        let base = vec![
            1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 1., 1e-6, 1., 0., 0., 1., 0.2, 0.5, 0., 0.8,
            0.5, 0., 0.5, 0.5, 0.,
        ];
        for (index, value) in [
            (10, 1e8),
            (11, 5.),
            (14, -0.5),
            (15, 1.5),
            (22, 1.2),
            (23, -0.2),
            (24, 2.),
        ] {
            let mut input = base.clone();
            input[index] = value;
            assert!(validate(&input).is_ok(), "index {index}, value {value}");
        }
        let mut point_interval = base;
        point_interval[14] = 0.5;
        point_interval[15] = 0.5;
        assert!(validate(&point_interval).is_ok());
    }
}
