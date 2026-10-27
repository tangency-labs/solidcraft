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
        pub fn sc_sweep_topology_sweep_whole(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
        pub fn sc_sweep_topology_sweep_body(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
/// Validate a script and execute it against upstream. Entity 64 denotes null;
/// 0..15 are vertices, 16..31 edges, 32..47 faces, 48..63 regions (two sets of eight).
/// Ops: same, higher, callback, erase-forward, erase-reverse, raw-map-insert,
/// repetitions, closure, start-frame, transform, vertices-same, vertices-higher,
/// end-frame (op 16, at `vector`),
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
            !(0..=16).contains(&op)
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
/// Whole-B-rep `DoSweep` (all six target arrays null) of an `SmApiCreateBox` solid:
/// `input` = `[length, width, height, kind (1 translation, 2 rotation), vx, vy, vz,
/// angle_deg, stitch, merge, repetitions]`, the rotation about axis `v` through the
/// origin. Returns the status (`0` = success), then on success the region, shell,
/// face, loop, edge and vertex counts, each vertex point in `GetVertices` order and
/// each `GetEdges` edge's start/end vertex indices (`-1` for none).
///
/// # Errors
/// Non-finite input, non-positive sizes, an unknown kind, more than four
/// repetitions, or native exceptions.
pub fn whole(input: [f64; 11]) -> Result<Vec<f64>, FfiError> {
    let reps = input[10];
    if input.iter().any(|v| !v.is_finite())
        || !(input[0] > 0.0 && input[1] > 0.0 && input[2] > 0.0)
        || (input[3] != 1.0 && input[3] != 2.0)
        || !(0.0..=4.0).contains(&reps)
        || reps.fract() != 0.0
    {
        return Err(FfiError::InvalidArgument(
            "invalid whole-B-rep sweep input".into(),
        ));
    }
    call_with_buffer(4096, |out, len| {
        // SAFETY: the fixed-size input is read only; C++ writes at most `cap`
        // doubles and retains no pointers to either buffer.
        unsafe {
            sys::sc_sweep_topology_sweep_whole(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
/// The upstream defect a valid [`body`] input would reach, if any: those inputs are
/// undefined in C++ and refused. Both are in the whole-B-rep branch (`targets` 0).
/// With tagging, the whole-B-rep `TagProfile` reads the
/// wire edges `RemoveAllTopology` has already deleted (UB-270), so the bodies with wire
/// edges (0, 1, 4) are refused with tagging on. A repetition that finds no wire edge
/// or shell vertex reuses the wire B-rep the previous repetition's union deleted
/// (UB-269): the lone wire (4) has none left after one repetition, and bodies 0, 1
/// and 3 none after two.
#[must_use]
pub fn body_undefined(input: &[f64; 16]) -> Option<&'static str> {
    let (body, repetitions, tagging) = (input[0], input[12], input[13] != 0.0);
    if input[15] != 0.0 {
        return None;
    }
    if tagging && (body == 0.0 || body == 1.0 || body == 4.0) {
        return Some("UB-270: whole-B-rep TagProfile reads deleted wire edges");
    }
    if (body == 4.0 && repetitions > 1.0)
        || ((body == 0.0 || body == 1.0 || body == 3.0) && repetitions > 2.0)
    {
        return Some("UB-269: a repetition without wires reuses the deleted wire B-rep");
    }
    None
}

/// Whole-B-rep `DoSweep` of a non-manifold body (`prog_test` `smSweepTest.cpp`):
/// `input` = `[body, kind (1 translation, 2 rotation), vx, vy, vz, angle_deg, cx, cy,
/// cz, eps, stitch, merge, repetitions, tagging, tag_id, targets]`, `targets` 0 for
/// the whole B-rep (all six target arrays null) or 1 for every face as faces-higher. `body` 0 is an
/// `SmApiCreateBox(origin, 4, 3, 2)` with a wire edge and a shell vertex, 1 the
/// `my_create_test_brep` face (inner loops, a vertex loop, an edge loop, a wire edge
/// and a shell vertex), 2 an empty B-rep, 3 a lone shell vertex, 4 a lone wire
/// edge, 5 the box alone and 6 the face of body 1 alone (see the shim). The
/// rotation is about axis `v` through `c` with tolerance `eps`. Returns the record of
/// [`whole`], then the wire-edge and shell-vertex counts, then per `GetFaces` face and
/// per `GetEdges` edge its `SM_AI_TAG` as `[present, eight longs]`.
///
/// # Errors
/// Non-finite input, an unknown body or kind, more than four repetitions, a
/// non-integral or out-of-range tag id, an input [`body_undefined`] refuses, or
/// native exceptions.
pub fn body(input: [f64; 16]) -> Result<Vec<f64>, FfiError> {
    let integral = |v: f64, lo: f64, hi: f64| (lo..=hi).contains(&v) && v.fract() == 0.0;
    if input.iter().any(|v| !v.is_finite())
        || !integral(input[0], 0.0, 6.0)
        || (input[1] != 1.0 && input[1] != 2.0)
        || !integral(input[12], 0.0, 4.0)
        || !integral(input[14], -1e9, 1e9)
        || (input[15] != 0.0 && input[15] != 1.0)
        || body_undefined(&input).is_some()
    {
        return Err(FfiError::InvalidArgument(
            "invalid non-manifold body sweep input".into(),
        ));
    }
    call_with_buffer(16384, |out, len| {
        // SAFETY: the fixed-size input is read only; C++ writes at most `cap`
        // doubles and retains no pointers to either buffer.
        unsafe {
            sys::sc_sweep_topology_sweep_body(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
/// [`body`] with explicit `DoSweep` target lists: `input[15]` (`targets`) must be 2
/// and `lists` = `[faces higher, edges higher, vertices higher, faces same, edges
/// same, vertices same, null_region]`. Each list selector is -1 for NULL, 0 for every
/// `GetFaces`/`GetEdges`/`GetVertices` entity, 1 for `GetWireEdges` (edges) or
/// `GetShellVertices` (vertices; faces: every face), 2 for an empty array, 3 for
/// every entity but the last, or `10 + i` for the single entity `i` of that list;
/// `null_region` 1 passes a NULL region. `kind` may also be 0 here: a base
/// `SmSweepGeometryCreation` (every sweep returns `SM_ERR`).
///
/// # Errors
/// As [`body`], plus a selector outside those values, an index past its list
/// (`SC_ERR_ARG` from the shim) or native exceptions.
pub fn body_lists(input: [f64; 16], lists: [f64; 7]) -> Result<Vec<f64>, FfiError> {
    let integral = |v: f64, lo: f64, hi: f64| (lo..=hi).contains(&v) && v.fract() == 0.0;
    if input.iter().chain(&lists).any(|v| !v.is_finite())
        || !integral(input[0], 0.0, 6.0)
        || !integral(input[1], 0.0, 2.0)
        || !integral(input[12], 0.0, 4.0)
        || !integral(input[14], -1e9, 1e9)
        || input[15] != 2.0
        || lists[..6]
            .iter()
            .any(|&l| !integral(l, -1.0, 10000.0) || (l > 3.0 && l < 10.0))
        || (lists[6] != 0.0 && lists[6] != 1.0)
    {
        return Err(FfiError::InvalidArgument(
            "invalid non-manifold body list sweep input".into(),
        ));
    }
    let mut all = [0.0; 23];
    all[..16].copy_from_slice(&input);
    all[16..].copy_from_slice(&lists);
    call_with_buffer(16384, |out, len| {
        // SAFETY: the fixed-size input is read only; C++ writes at most `cap`
        // doubles and retains no pointers to either buffer.
        unsafe {
            sys::sc_sweep_topology_sweep_body(
                all.as_ptr(),
                all.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
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
