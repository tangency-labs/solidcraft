//! Checked stateful scripts for the real `SmTopologySweep` and stitch callback.
//! All state is returned as exact integers (not rounded floating-point ids); the
//! distance slot is retained for compatibility; `eval_with_values` also returns
//! untruncated distances and successful frame outputs as doubles. Repetitions
//! retain their u64 bit pattern in an i64 slot.
//! Each step emits `[status (0 for success), keep_first, maximum_distance]`.
//! Final state is `[tagging, tag_id, repetitions, merge, stitch]`, then for each
//! map (same, higher, reverse-same, reverse-higher), its count and 65 pairs of
//! `[contains_key, image_id]`; finally 16 edge tags, each `[present, eight longs]`.
//! Scripts are bounded to 256 instructions; the native fixture owns 64 real
//! objects and validates every reference before accessing them.
use crate::{call_with_buffer, FfiError};
mod sys {
    extern "C" {
        pub fn sc_sweep_topology_sweep_eval(
            steps: *const i64,
            n: i64,
            vector: *const f64,
            kind: i64,
            out: *mut i64,
            cap: i64,
            len: *mut i64,
            values: *mut f64,
            value_cap: i64,
        ) -> i32;
    }
}
/// Validate a script and execute it against upstream. Entity 64 denotes null;
/// 0..15 are vertices, 16..31 edges, 32..47 faces, 48..63 regions (two sets of eight).
/// Ops: same, higher, callback, erase-forward, erase-reverse, raw-map-insert,
/// repetitions, closure, start-frame, transform, vertices-same, vertices-higher,
/// option setters, and the null-B-rep `DoSweep` early return, edge-profile tagging, and a selected-vertex `DoSweep` (stitch/merge disabled). Each instruction has `[op,a,b,argument]`.
///
/// # Errors
/// Invalid script layouts/indices, non-finite coordinates, or native exceptions.
pub fn eval(steps: &[[i64; 4]], vector: [f64; 3], kind: u8) -> Result<Vec<i64>, FfiError> {
    eval_with_values(steps, vector, kind).map(|(state, _)| state)
}
/// Execute a validated script and return integer state plus 13 doubles per step:
/// distance, then frame origin/X/Y/Z (zero for unrequested or failed frames).
///
/// # Errors
/// As [`eval`].
pub fn eval_with_values(
    steps: &[[i64; 4]],
    vector: [f64; 3],
    kind: u8,
) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    if steps.len() > 256
        || kind > 2
        || vector.iter().any(|v| !v.is_finite())
        || steps.iter().any(|s| {
            let [op, a, b, arg] = *s;
            !(0..=15).contains(&op)
                || !(0..=64).contains(&a)
                || !(0..=64).contains(&b)
                || (op == 5 && !(0..=3).contains(&arg))
                || ((op == 10 || op == 11) && a >= 16)
                || (op == 14 && (!(16..32).contains(&a) || !(16..32).contains(&b)))
        })
    {
        return Err(FfiError::InvalidArgument(
            "invalid topology sweep script".into(),
        ));
    }
    let mut values = vec![0.0; steps.len() * 13];
    let state = call_with_buffer(2048, |out, len| {
        // SAFETY: each fixed-size instruction is contiguous, all indices/counts
        // are checked, and C++ retains no pointers to the live caller buffers.
        unsafe {
            sys::sc_sweep_topology_sweep_eval(
                steps.as_ptr().cast(),
                steps.len() as i64,
                vector.as_ptr(),
                i64::from(kind),
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
                values.as_mut_ptr(),
                values.len() as i64,
            )
        }
    })?;
    Ok((state, values))
}
#[cfg(test)]
mod tests {
    use super::eval;
    #[test]
    fn guard_rejects_bad_indices_and_layouts() {
        for s in [
            [16, 0, 0, 0],
            [5, 0, 0, 4],
            [10, 16, 0, 0],
            [0, -1, 0, 0],
            [0, 0, 65, 0],
            [5, 0, 0, -1],
        ] {
            assert!(eval(&[s], [1., 0., 0.], 0).is_err());
        }
        assert!(eval(&[[0, 0, 0, 0]; 257], [1., 0., 0.], 0).is_err());
        assert!(eval(&[], [f64::NAN, 0., 0.], 0).is_err());
    }
    #[test]
    fn callback_and_mapping_use_real_native_objects() {
        let out = eval(&[[0, 0, 1, 0], [2, 0, 1, 0]], [1., 0., 0.], 0).unwrap();
        assert_eq!(&out[3..6], &[0, 1, 0]);
        // Both directions now map the original vertex to itself; the old image
        // no longer has an inverse entry.
        assert_eq!(out[11], 1);
        assert_eq!(&out[12..16], &[1, 0, 0, 64]);
        assert_eq!(out[273], 1);
        assert_eq!(&out[274..278], &[1, 0, 0, 64]);
    }
    #[test]
    fn native_frame_and_distance_outputs_are_not_truncated() {
        let (_, values) =
            super::eval_with_values(&[[9, 0, 0, 0]], [0.375, -0.625, 1.25], 1).unwrap();
        assert_eq!(
            values,
            vec![-123.0, 0.375, -0.625, 1.25, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        );
    }
    #[test]
    fn native_tagging_observes_generator_metadata_and_retains_existing_tags() {
        let output = eval(
            &[
                [12, 0, 0, 741],
                [14, 16, 17, 0],
                [12, 0, 0, 888],
                [14, 17, 16, 0],
            ],
            [1., 0., 0.],
            0,
        )
        .unwrap();
        let tags = &output[output.len() - 16 * 9..];
        assert_eq!(&tags[..9], &[1, 741, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&tags[9..18], &[1, 741, 0, 1, 717, 29, 0, 0, 0]);
    }
}
