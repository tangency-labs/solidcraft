//! Safe native oracle for curve-classification part 6.
//!
//! Each input interval is `lo hi` followed by three eight-number point records:
//! `kind gap t pre_snap stored_point x y z`. Kinds: 0 unknown, 1 curve, 2 face,
//! 3 edge, 4 vertex, 5 region, 6 nothing. Topology comes from a real unit box.
//! Flags bit 0 selects vertices-only merging; bit 1 enables paired fillet handling.
//! Geometry: 0 line, 1 circle, 2 ellipse, 3 quadratic B-spline, 4 constant B-spline,
//! and lines placed on the box: 5 from vertex (0,0,0) across face z=0 (direction
//! (0.5,1,0)), 6 along the edge (1,0,1)-(1,1,1), 7 from (0.25,0.25,0) along x inside
//! face z=0. [`run_picked`] chooses which of the box's faces, edges, vertices and
//! regions (indices into `GetFaces`, `GetEdges`, `GetVertices`, `GetRegions`) the
//! classification kinds 2 to 5 name, and can enable the box's editing (the state its
//! edge splits and `MakeEdgeInFaceWithVU` require); [`run`] names the first of each.
//! The classified range is [0,1]; supplied intervals form a partition of that range.
//! Native constructor status/null checks precede topology access. All objects are scoped.

use crate::{call_with_buffer, FfiError};

/// Numbers per interval (two endpoints, three eight-number classifications).
pub const WIDTH: usize = 26;
/// Largest fixture partition (native algorithms may create additional intervals).
pub const MAX_INTERVALS: usize = 32;

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p6_run(
            op: i32,
            kind: i32,
            scale: f64,
            tol: f64,
            flags: i32,
            index: i64,
            a: *const f64,
            na: i64,
            b: *const f64,
            nb: i64,
            input: *const f64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
        pub fn sc_intersect_curve_class_p6_run_picked(
            op: i32,
            kind: i32,
            scale: f64,
            tol: f64,
            flags: i32,
            index: i64,
            a: *const f64,
            na: i64,
            b: *const f64,
            nb: i64,
            input: *const f64,
            pick: *const i32,
            editing: i32,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// The box's face, edge, vertex and region counts (`SmApiCreateBox`'s unit box).
pub const PICK_LIMITS: [i32; 4] = [6, 12, 8, 2];

fn bad() -> FfiError {
    FfiError::InvalidArgument("invalid curve-classification fixture".into())
}

fn interval(v: &[f64]) -> Result<(), FfiError> {
    let v: &[f64; WIDTH] = v.try_into().map_err(|_| bad())?;
    if v.len() != WIDTH
        || v.iter().any(|x| !x.is_finite())
        || v[0] < 0.0
        || v[1] > 1.0
        || v[0] > v[1]
    {
        return Err(bad());
    }
    for pc in v[2..].as_chunks::<8>().0 {
        if pc[0].fract() != 0.0
            || !(0.0..=6.0).contains(&pc[0])
            || pc[1] < 0.0
            || pc[1] > 1e20
            || pc[2].abs() > 1e20
            || pc[3].abs() > 1e20
            || ![0.0, 1.0].contains(&pc[4])
            || pc[5..].iter().any(|x| x.abs() > 1e100)
        {
            return Err(bad());
        }
    }
    Ok(())
}

fn partition(v: &[f64]) -> Result<(), FfiError> {
    if !v.len().is_multiple_of(WIDTH) || v.len() / WIDTH > MAX_INTERVALS {
        return Err(bad());
    }
    let mut previous = 0.0;
    for ivl in v.as_chunks::<WIDTH>().0 {
        interval(ivl)?;
        if ivl[0] != previous || ivl[0] >= ivl[1] {
            return Err(bad());
        }
        previous = ivl[1];
    }
    if !v.is_empty() && previous != 1.0 {
        return Err(bad());
    }
    Ok(())
}

/// Validates the complete native fixture before passing it to C++.
/// Operations: 0 insert, 1 homogenize, 2 fix endpoint, 3 merge interval.
/// For endpoint repair the incoming start record's xyz supplies the caller's point.
/// Native preflight also rejects full merges whose UNKNOWN start cannot become
/// a vertex. Degenerate returns and vertices-only calls remain available.
///
/// # Errors
/// Rejects malformed layouts, flags, partitions and invalid operation indices.
/// Fillet merging requires equally sized, homogenized partner classifications.
#[allow(clippy::too_many_arguments)]
pub fn validate(
    op: i32,
    kind: i32,
    scale: f64,
    tol: f64,
    flags: i32,
    index: usize,
    a: &[f64],
    b: &[f64],
    input: &[f64],
) -> Result<(), FfiError> {
    if !(0..=3).contains(&op)
        || !(0..=7).contains(&kind)
        || !scale.is_finite()
        || !(0.0..=1e100).contains(&scale)
        || scale == 0.0
        || !tol.is_finite()
        || !(0.0..=1e10).contains(&tol)
        || !(0..=3).contains(&flags)
    {
        return Err(bad());
    }
    partition(a)?;
    partition(b)?;
    interval(input)?;
    let count = a.len() / WIDTH;
    if (op != 1 && count == 0)
        || (op == 1 && a.is_empty() != b.is_empty())
        || (op == 2 && index > count)
        || (op == 3 && index >= count)
        || (op == 3 && flags & 2 != 0 && b.len() / WIDTH != count)
    {
        return Err(bad());
    }
    if op == 3 && flags & 1 == 0 {
        let record: &[f64; WIDTH] = a
            .get(index * WIDTH..(index + 1) * WIDTH)
            .and_then(|x| x.try_into().ok())
            .ok_or_else(bad)?;
        let classes = [record[2], record[10], record[18]];
        let early = (classes[1] == 0.0 && flags & 1 == 0) || classes == [4.0, 3.0, 4.0];
        // MergeInterval casts classification objects to B-rep topology. A geometry
        // or NOTHING record cannot supply that topology; UNKNOWN is allowed.
        if !early && classes.iter().any(|x| *x == 1.0 || *x == 6.0) {
            return Err(bad());
        }
    }
    // Index is ignored by insert/homogenize, but must fit the native integer type.
    i64::try_from(index).map_err(|_| bad())?;
    Ok(())
}

/// [`validate`] with the objects [`run_picked`] names.
///
/// # Errors
/// Those of [`validate`]; a pick outside the box's lists.
#[allow(clippy::too_many_arguments)]
pub fn validate_picked(
    op: i32,
    kind: i32,
    scale: f64,
    tol: f64,
    flags: i32,
    index: usize,
    a: &[f64],
    b: &[f64],
    input: &[f64],
    pick: [i32; 4],
) -> Result<(), FfiError> {
    if pick
        .iter()
        .zip(PICK_LIMITS)
        .any(|(p, limit)| !(0..limit).contains(p))
    {
        return Err(bad());
    }
    validate(op, kind, scale, tol, flags, index, a, b, input)
}

/// [`run`] with the box's face, edge, vertex and region at `pick` (indices into its
/// lists) as the objects of classification kinds 2 to 5, and the box's editing
/// enabled (`m_bEditingEnabled`) when `editing`.
///
/// # Errors
/// See [`validate_picked`]; construction failures are reported as shim errors.
#[allow(clippy::too_many_arguments)]
pub fn run_picked(
    op: i32,
    kind: i32,
    scale: f64,
    tol: f64,
    flags: i32,
    index: usize,
    a: &[f64],
    b: &[f64],
    input: &[f64],
    pick: [i32; 4],
    editing: bool,
) -> Result<Vec<f64>, FfiError> {
    validate_picked(op, kind, scale, tol, flags, index, a, b, input, pick)?;
    call_with_buffer(32768, |out, len| {
        // SAFETY: as `run`; `pick` is four in-range indices, checked again natively.
        unsafe {
            sys::sc_intersect_curve_class_p6_run_picked(
                op,
                kind,
                scale,
                tol,
                flags,
                index as i64,
                a.as_ptr(),
                (a.len() / WIDTH) as i64,
                b.as_ptr(),
                (b.len() / WIDTH) as i64,
                input.as_ptr(),
                pick.as_ptr(),
                i32::from(editing),
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Calls the real native operation and returns status plus complete classification state.
///
/// # Errors
/// See [`validate`]; construction failures are reported as shim errors.
#[allow(clippy::too_many_arguments)]
pub fn run(
    op: i32,
    kind: i32,
    scale: f64,
    tol: f64,
    flags: i32,
    index: usize,
    a: &[f64],
    b: &[f64],
    input: &[f64],
) -> Result<Vec<f64>, FfiError> {
    validate(op, kind, scale, tol, flags, index, a, b, input)?;
    call_with_buffer(32768, |out, len| {
        // SAFETY: full records, finite values, enum/flag ranges, partitions and indices
        // are validated above. The shim checks constructor results and output capacity.
        unsafe {
            sys::sc_intersect_curve_class_p6_run(
                op,
                kind,
                scale,
                tol,
                flags,
                index as i64,
                a.as_ptr(),
                (a.len() / WIDTH) as i64,
                b.as_ptr(),
                (b.len() / WIDTH) as i64,
                input.as_ptr(),
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
    fn ivl() -> Vec<f64> {
        let mut v = vec![0.0, 1.0];
        for _ in 0..3 {
            v.extend([0.0, 0.0, 0.0, 1e20, 0.0, 0.0, 0.0, 0.0]);
        }
        v
    }
    #[test]
    fn rejects_malformed_records_and_invalid_indices() {
        let v = ivl();
        for n in 0..WIDTH {
            assert!(run(0, 0, 1.0, 1e-6, 0, 0, &v[..n], &v, &v).is_err());
        }
        for at in 0..WIDTH {
            for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut bad = v.clone();
                bad[at] = x;
                assert!(run(0, 0, 1.0, 1e-6, 0, 0, &bad, &v, &v).is_err());
            }
        }
        assert!(run(2, 0, 1.0, 1e-6, 0, 2, &v, &v, &v).is_err());
        assert!(run(3, 0, 1.0, 1e-6, 0, 1, &v, &v, &v).is_err());
        assert!(run(0, 0, 1.0, 1e-6, 4, 0, &v, &v, &v).is_err());
        assert!(run(1, 0, 1.0, 1e-6, 0, 0, &[], &v, &v).is_err());
    }
    #[test]
    fn native_degenerate_and_tiny_geometry_constructor_paths() {
        let v = ivl();
        for kind in 0..=4 {
            for scale in [1e-100, 1e-15, 1e-9, 1.0, 1e9, 1e100] {
                // Merge's unknown-interior exit must still invoke the real native API.
                let out = run(3, kind, scale, 1e-6, 0, 0, &v, &v, &v).unwrap();
                assert_eq!(out[0], 0.0);
            }
        }
    }
}

#[cfg(test)]
mod isolated_tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const CHILD: &str = "units::intersect_curve_class_p6::isolated_tests::native_probe_child";
    #[test]
    #[ignore = "only run by the subprocess safety matrix"]
    fn native_probe_child() {
        if let Ok(s) = std::env::var("SC_CC_P6_FULL") {
            let v: Vec<f64> = s.split(',').map(|x| x.parse().unwrap()).collect();
            let (na, nb) = (v[6] as usize, v[7] as usize);
            let a = &v[8..8 + na * WIDTH];
            let b = &v[8 + na * WIDTH..8 + (na + nb) * WIDTH];
            let input = &v[8 + (na + nb) * WIDTH..];
            let result = run(
                v[0] as i32,
                v[1] as i32,
                v[2],
                v[3],
                v[4] as i32,
                v[5] as usize,
                a,
                b,
                input,
            );
            if std::env::var_os("SC_CC_P6_EXPECT_REJECT").is_some() {
                assert!(
                    matches!(result, Err(FfiError::InvalidArgument(_))),
                    "{s}: {result:?}"
                );
                let mut out = [0.0];
                let mut len = 0;
                // SAFETY: the parent constructs complete, bounded records. Only the
                // partner count is invalid, and the shim must reject it before access.
                let status = unsafe {
                    sys::sc_intersect_curve_class_p6_run(
                        v[0] as i32,
                        v[1] as i32,
                        v[2],
                        v[3],
                        v[4] as i32,
                        v[5] as i64,
                        a.as_ptr(),
                        na as i64,
                        b.as_ptr(),
                        nb as i64,
                        input.as_ptr(),
                        out.as_mut_ptr(),
                        1,
                        &raw mut len,
                    )
                };
                assert_eq!(
                    status,
                    crate::SC_ERR_ARG,
                    "{s}: native guard accepted invalid partner"
                );
                return;
            }
            // Invalid native preconditions are fixture rejection, never a parity pass.
            assert!(
                matches!(
                    result,
                    Ok(_) | Err(FfiError::InvalidArgument(_) | FfiError::Status(crate::SC_ERR_ARG))
                ),
                "{s}: {result:?}"
            );
            return;
        }
        let s = std::env::var("SC_CC_P6_PROBE").unwrap();
        let args: Vec<f64> = s.split(',').map(|x| x.parse().unwrap()).collect();
        let (op, kind, flags, scale) = (args[0] as i32, args[1] as i32, args[2] as i32, args[3]);
        let mut a = vec![0.0, 1.0];
        for c in &args[4..7] {
            a.extend([*c, 0.0, 0.5, 1e20, 0.0, 0.0, 0.0, 0.0]);
        }
        let input = a.clone();
        // Malformed classification kinds for merge are rejected before native code.
        if validate(op, kind, scale, 1e-6, flags, 0, &a, &a, &input).is_err() {
            return;
        }
        let result = run(op, kind, scale, 1e-6, flags, 0, &a, &a, &input);
        assert!(
            matches!(result, Ok(_) | Err(FfiError::Status(crate::SC_ERR_ARG))),
            "{s}: {result:?}"
        );
    }
    fn isolated_probe(exe: &std::path::Path, variable: &str, probe: &str) {
        isolated_probe_with_rejection(exe, variable, probe, false);
    }
    fn isolated_rejected_probe(exe: &std::path::Path, probe: &str) {
        isolated_probe_with_rejection(exe, "SC_CC_P6_FULL", probe, true);
    }
    fn isolated_probe_with_rejection(
        exe: &std::path::Path,
        variable: &str,
        probe: &str,
        rejected: bool,
    ) {
        let mut command = Command::new(exe);
        command
            .args(["--ignored", "--exact", CHILD])
            .env(variable, probe)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if rejected {
            command.env("SC_CC_P6_EXPECT_REJECT", "1");
        }
        let mut child = command.spawn().unwrap();
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "native crash/failure for {probe}: {status}"
                );
                break;
            }
            if start.elapsed() > Duration::from_secs(5) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("native timeout for {probe}");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn record(lo: f64, hi: f64, classes: [i32; 3], stored: bool) -> Vec<f64> {
        let mut v = vec![lo, hi];
        for (kind, t) in classes.into_iter().zip([lo, (lo + hi) * 0.5, hi]) {
            v.extend([
                f64::from(kind),
                0.0,
                t,
                1e20,
                f64::from(stored),
                t,
                2.0 * t,
                -t,
            ]);
        }
        v
    }
    #[test]
    fn rejects_fillets_with_unhomogenized_partner_in_subprocesses() {
        let exe = std::env::current_exe().unwrap();
        let listing = Command::new(&exe)
            .args(["--ignored", "--list"])
            .output()
            .unwrap();
        assert!(String::from_utf8(listing.stdout).unwrap().contains(CHILD));
        for n in [1, 2, 3, MAX_INTERVALS] {
            for index in [0, n - 1] {
                for nb in [0, n - 1, n + 1] {
                    if nb == n || nb > MAX_INTERVALS {
                        continue;
                    }
                    let b: Vec<f64> = (0..nb)
                        .flat_map(|j| {
                            record(
                                j as f64 / nb as f64,
                                (j + 1) as f64 / nb as f64,
                                [0, 0, 0],
                                false,
                            )
                        })
                        .collect();
                    for classes in [[4, 3, 4], [2, 2, 2], [5, 5, 5]] {
                        let a: Vec<f64> = (0..n)
                            .flat_map(|j| {
                                record(
                                    j as f64 / n as f64,
                                    (j + 1) as f64 / n as f64,
                                    if j == index { classes } else { [0, 0, 0] },
                                    false,
                                )
                            })
                            .collect();
                        let input = record(0.0, 1.0, [0, 0, 0], false);
                        for kind in 0..5 {
                            for flags in [2, 3] {
                                let mut payload = vec![
                                    3.0,
                                    f64::from(kind),
                                    1.0,
                                    1e-6,
                                    f64::from(flags),
                                    index as f64,
                                    n as f64,
                                    nb as f64,
                                ];
                                payload.extend(&a);
                                payload.extend(&b);
                                payload.extend(&input);
                                let probe = payload
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(",");
                                isolated_rejected_probe(&exe, &probe);
                            }
                            // Without fillets the partner is unused by this early exit.
                            if classes == [4, 3, 4] {
                                for flags in [0, 1] {
                                    assert!(run(3, kind, 1.0, 1e-6, flags, index, &a, &b, &input)
                                        .is_ok());
                                }
                                // A correctly sized partner remains a real native call.
                                let partner: Vec<f64> = (0..n)
                                    .flat_map(|j| {
                                        record(
                                            j as f64 / n as f64,
                                            (j + 1) as f64 / n as f64,
                                            [0, 0, 0],
                                            false,
                                        )
                                    })
                                    .collect();
                                for flags in [2, 3] {
                                    assert!(run(
                                        3, kind, 1.0, 1e-6, flags, index, &a, &partner, &input
                                    )
                                    .is_ok());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_null_start_vertex_but_keeps_native_early_exits() {
        let input = record(0.0, 1.0, [0, 0, 0], false);
        for mid in [3, 4] {
            for end in [0, 2, 3, 4, 5] {
                let a = record(0.0, 1.0, [0, mid, end], false);
                for flags in [0, 2] {
                    assert!(matches!(
                        run(3, 0, 1.0, 1e-6, flags, 0, &a, &a, &input),
                        Err(FfiError::Status(crate::SC_ERR_ARG))
                    ));
                }
                assert!(run(3, 4, 1.0, 1e-6, 0, 0, &a, &a, &input).is_ok());
                // These call the real native API, rather than counting a guard as parity.
                assert!(run(3, 0, 1.0, 1e-6, 1, 0, &a, &a, &input).is_ok());
            }
        }
        let a = record(0.0, 1.0, [1, 0, 6], false);
        assert!(run(3, 0, 1.0, 1e-6, 0, 0, &a, &a, &input).is_ok());
    }
    #[test]
    fn failed_face_projection_is_a_fixture_rejection() {
        let mut a = record(0.0, 1.0, [0, 2, 0], false);
        a[6] = 1.0;
        a[7..10].fill(10.0);
        for kind in 0..5 {
            for flags in [0, 2] {
                assert_eq!(
                    run(3, kind, 1.0, 1e-6, flags, 0, &a, &a, &a),
                    Err(FfiError::Status(crate::SC_ERR_ARG))
                );
            }
        }
        // Projection succeeds at the box corner; UNKNOWN/FACE is not rejected wholesale.
        a[7..10].fill(0.0);
        assert!(run(3, 0, 1.0, 1e-6, 0, 0, &a, &a, &a).is_ok());
    }
    #[test]
    fn endpoint_repair_uses_the_callers_point() {
        let a = record(0.0, 1.0, [3, 0, 3], false);
        let mut input = record(0.0, 1.0, [0, 0, 0], false);
        // The edge comes from the box's native traversal; test all corners
        // without assuming which edge is first. Exactly its two ends promote.
        let mut promoted = 0;
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    input[7..10].copy_from_slice(&[x, y, z]);
                    let near = run(2, 0, 1.0, 1e-6, 0, 0, &a, &a, &input).unwrap();
                    assert_eq!(near[0], 0.0);
                    promoted += usize::from(near[1] == 1.0);
                }
            }
        }
        assert_eq!(
            promoted, 2,
            "only the edge's endpoints promote EDGE to VERTEX"
        );
        input[7..10].fill(10.0);
        let far = run(2, 0, 1.0, 1e-6, 0, 0, &a, &a, &input).unwrap();
        assert_eq!(far[0], 0.0);
        assert_eq!(far[1], 0.0, "a distant point leaves EDGE unchanged");
    }
    #[test]
    fn native_partition_sss_and_boundary_regressions_in_subprocesses() {
        let exe = std::env::current_exe().unwrap();
        let listing = Command::new(&exe)
            .args(["--ignored", "--list"])
            .output()
            .unwrap();
        assert!(String::from_utf8(listing.stdout).unwrap().contains(CHILD));
        for n in [1, 2, 3, MAX_INTERVALS] {
            for stored in [false, true] {
                let a: Vec<f64> = (0..n)
                    .flat_map(|j| {
                        record(
                            j as f64 / n as f64,
                            (j + 1) as f64 / n as f64,
                            [3, 0, 3],
                            stored,
                        )
                    })
                    .collect();
                let b = record(0.0, 1.0, [0, 0, 0], !stored);
                let input = record(0.25, 0.75, [1, 1, 1], stored);
                for op in 0..4 {
                    for index in [0, n / 2, if op == 2 { n } else { n - 1 }] {
                        let mut payload = vec![
                            f64::from(op),
                            0.0,
                            1.0,
                            1e-6,
                            0.0,
                            index as f64,
                            n as f64,
                            1.0,
                        ];
                        payload.extend(&a);
                        payload.extend(&b);
                        payload.extend(&input);
                        let probe = payload
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",");
                        isolated_probe(&exe, "SC_CC_P6_FULL", &probe);
                    }
                }
            }
        }
    }
    #[test]
    fn native_operation_geometry_flag_matrix_in_subprocesses() {
        let exe = std::env::current_exe().unwrap();
        let listing = Command::new(&exe)
            .args(["--ignored", "--list"])
            .output()
            .unwrap();
        assert!(String::from_utf8(listing.stdout).unwrap().contains(CHILD));
        for kind in 0..=4 {
            for flags in 0..=3 {
                for op in 0..=3 {
                    for code in 0..343 {
                        let classes = [code / 49, code / 7 % 7, code % 7];
                        let probe = format!(
                            "{op},{kind},{flags},1,{},{},{}",
                            classes[0], classes[1], classes[2]
                        );
                        isolated_probe(&exe, "SC_CC_P6_PROBE", &probe);
                    }
                }
            }
        }
    }
}
