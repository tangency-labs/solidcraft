//! Safe native oracle for curve classification part 9.
//!
//! The 22-scalar header is `op surface cache check3d cross big_gaps uv_okay defaults
//! dim_minus_2 rectangle scale zone nv ne lo hi source_start[3] source_end[3]`.
//! It is followed by `nv` UV/gap triples and `ne` edge records:
//! `kind start[3] end[3] lo hi same_source edge_zone p1[3] d1[3] p2[3] d2[3]`.
//! `kind` 0 is the line `SmLine(start, end, 3)` (or the source curve for a shared
//! edge); kind 1 is `SmHermiteCurve(p1, d1, p2, d2, 3)` and its four vectors are zero
//! for a line. Coordinates are scaled in 3D; source coordinates stay in UV for
//! trim-boundary operations. Cache states are unbuilt, one leaf, modified, containment
//! missing, two u leaves, under construction, and two v leaves.
//! Counts, domains, flags and native ownership preconditions are checked on both sides.

use crate::{call_with_buffer, FfiError};
/// One edge record's scalars (`kind` + the two endpoints + interval + flags + the four
/// Hermite vectors).
const EDGE_RECORD: usize = 23;
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
    if input.len() != 22 + 3 * nv + EDGE_RECORD * ne
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
        .as_chunks::<EDGE_RECORD>()
        .0
    {
        // `kind start[3] end[3] lo hi same edge_zone p1[3] d1[3] p2[3] d2[3]`.
        int(e[0], 1)?;
        int(e[9], 1)?;
        if e[7] > e[8]
            || e[10] < 0.
            || e[1..7].iter().any(|x| !(x * scale).is_finite())
            || e[11..].iter().any(|x| !(x * scale).is_finite())
        {
            return Err(invalid());
        }
        if e[9] == 1. {
            // A Hermite edge has its own curve: it cannot be the source curve.
            shared += 1;
            if e[0] != 0. || header[8] != 1. {
                return Err(invalid());
            }
        } else if e[0] == 0. && e[1..4].iter().zip(&e[4..7]).all(|(a, b)| a == b) {
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
    /// The 22-scalar header of an `IntersectEdges` recipe with `ne` edges, no vertices
    /// and a dim-3 source from (0,0,0) to (1,0,0).
    fn header(ne: f64) -> Vec<f64> {
        let mut h = vec![0., 0., 0., 0., 0., 0., 1., 0., 1., 0., 1., 1e-6];
        h.extend([0., ne, 0., 1.]);
        h.extend([0., 0., 0., 1., 0., 0.]);
        h
    }
    /// One line edge record with the source curve (`same`) and its four zero Hermite
    /// vectors.
    fn shared_line() -> Vec<f64> {
        let mut r = vec![0., 0., 0., 0., 1., 0., 0., 0., 1., 1., 1e-6];
        r.extend([0.; 12]);
        r
    }
    /// One Hermite edge record `p1 d1 p2 d2`.
    fn hermite() -> Vec<f64> {
        let mut r = vec![1., 0.3, 0.6, -0.4, 0.7, 0.4, 0.4, 0., 1., 0., 1e-6];
        r.extend([0.3, 0.6, -0.4]);
        r.extend([0.4, 0.0, 0.9]);
        r.extend([0.7, 0.4, 0.4]);
        r.extend([0.4, 0.0, 0.9]);
        r
    }
    #[test]
    fn guards_counts_and_shared_curve_ownership() {
        let mut i = header(0.);
        assert!(validate(&i).is_ok());
        i[13] = 1e12;
        assert!(validate(&i).is_err());
        i[13] = 1.;
        assert!(validate(&i).is_err());
        i.extend(shared_line());
        assert!(validate(&i).is_ok());
        i[13] = 2.;
        i.extend(shared_line());
        assert!(validate(&i).is_err());
    }
    #[test]
    fn guards_hermite_edge_records() {
        let mut i = header(1.);
        i.extend(hermite());
        assert!(validate(&i).is_ok());
        // A Hermite edge carries its own curve, never the source curve.
        let mut same = i.clone();
        same[22 + 9] = 1.;
        assert!(validate(&same).is_err());
        // A nonconstant cubic may close on itself; the wire constructor glues its ends.
        let mut closed = i.clone();
        closed[22 + 17..22 + 20].copy_from_slice(&[0.3, 0.6, -0.4]);
        assert!(validate(&closed).is_ok());
        // A line record still needs distinct endpoints, and unknown kinds are rejected.
        let mut repeated = i.clone();
        repeated[22] = 0.;
        repeated[22 + 1..22 + 4].copy_from_slice(&[0.3, 0.6, -0.4]);
        repeated[22 + 4..22 + 7].copy_from_slice(&[0.3, 0.6, -0.4]);
        assert!(validate(&repeated).is_err());
        let mut kind = i;
        kind[22] = 2.;
        assert!(validate(&kind).is_err());
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
