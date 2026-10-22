//! FFI wrappers for unit `topology.face_p3`.
//!
//! Shim: `harness/cpp/shim/topology_face_p3.cpp` (`sc_topology_face_p3_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp`.
//!
//! Every op takes a B-rep block of [`super::topology_vertexuse`] (the shim builds a fresh
//! `SmBrep` per call and deletes it after; the graph export of the same block is
//! [`super::topology_vertexuse::eval`] with `Op::Export`) and six arguments (unused ones
//! are `0`). Indices refer to that export's traversal order. Every op's output starts
//! with `0`, or with `-1 status` when the B-rep could not be built. `-2` right after the
//! leading `0` (ops `LoopOrient` to `Intersect`) is "upstream would dereference null":
//! the target face(s) (for `LoopOrient`, the selected loop; see below) reach a missing
//! surface or edge curve, so the shim did not call the function. "tolerances"
//! are those of every face, edge and vertex; "use states" are, per edge use, whether it
//! has a UV trim curve and whether its edge/face and edge/CCW-edge gaps are initialised,
//! then per vertex use whether its vertex/face and vertex/edge gaps are; "samples" of a
//! curve are its natural interval and `status x y z` at 0, 1/2 and 1 of it.
//!
//! | [`Op`]        | arguments                                   | outputs |
//! |---------------|---------------------------------------------|---------|
//! | `Types`       | (none)                                      | `n_f type* n_e type*`: each face's surface type and each edge's curve type (`-1` for none) |
//! | `Lamina`      | `face which` (`IsLamina`, `HasLaminaEdge`, `HasConsecutiveLaminaEdges`) | `flag` |
//! | `SetTol`      | `face tol only_if_larger cascade`           | tolerances |
//! | `Notify`      | `face op` (`PRE_EDIT`, `POST_EDIT`, `CHANGE_GEOMETRY`, `ADD_TO_BREP`, `RM_FROM_BREP`) | `rect_trim`, use states |
//! | `LoopOrient`  | `face loop want_bounds do_3d`               | `status orient [u0 v0 u1 v1]`, use states |
//! | `Degenerate`  | `face skip_quick has_tol tol`               | `flag` |
//! | `Composites`  | `face`                                      | `status n` then per composite `segments t0 t1 closed` and per segment `transition sense gap0 gap1 samples` |
//! | `UvTrim`      | `face adjust own_curves` (`own_curves`: pass copies of the face's UV trim curves, cleared first) | `status gaps*6`, tolerances, use states |
//! | `Intersect`   | `face other approx_tol ang_tol_deg coincidence want_points` (`coincidence` `-1` none, `0`, `1`) | `status n samples* n_pts xyz*` |
//!
//! Blocks with a value above [`super::topology_edgeuse_p2::MAX_BLOCK_MAGNITUDE`] are
//! refused first (HANG-29).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: valid B-rep
//! blocks (checked by [`super::topology_vertexuse`]), indices inside the B-rep's export,
//! the flags and selectors above, finite `SetTol` / `Degenerate` tolerances (any sign:
//! upstream only compares them), a finite positive approximation tolerance and an angle
//! in `[0, 180]` degrees for `Intersect` (0 is allowed: upstream only converts it to
//! radians and passes it on; a probe decides whether any case hangs). Inputs upstream would dereference null on
//! (missing surface or edge curve) are not rejected here: the shim outputs `-2` for them
//! without calling the function (`LoopOrient` checks the selected loop, which may lie on
//! another face: a vertex loop reads no geometry; an edge loop needs the face's surface,
//! its edges' curves and, without `do_3d`, all geometry of its own face).

use super::topology_vertexuse::{self as fixture, Op as FixtureOp};
use crate::{call_with_buffer, FfiError};

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Types = 0,
    Lamina = 1,
    SetTol = 2,
    Notify = 3,
    LoopOrient = 4,
    Degenerate = 5,
    Composites = 6,
    UvTrim = 7,
    Intersect = 8,
}

/// The number of arguments every op takes.
pub const N_ARGS: usize = 6;

/// The `SmNotifyOperation`s of `Notify`, by index.
pub const NOTIFY_OPS: usize = 5;

mod sys {
    extern "C" {
        pub fn sc_topology_face_p3_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            args: *const f64,
            n_args: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn raw(op: Op, path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; block was validated as a
        // complete B-rep block by `fixture::eval`; args holds N_ARGS values checked by
        // `check_args` against the B-rep's counts (or none for `Types`); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_topology_face_p3_eval(
                op as i32,
                cpath.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                args.as_ptr(),
                args.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn index(v: f64, count: usize, what: &str) -> Result<(), FfiError> {
    if v.fract() != 0.0 || v < 0.0 || v >= count as f64 {
        return Err(bad(format!("{what} index {v} out of range ({count})")));
    }
    Ok(())
}

fn flag(v: f64, what: &str) -> Result<(), FfiError> {
    if v != 0.0 && v != 1.0 {
        return Err(bad(format!("{what}: flag must be 0 or 1")));
    }
    Ok(())
}

fn tol(v: f64, what: &str) -> Result<(), FfiError> {
    // SetTolerance / IsDegenerate are defined for any finite value.
    if !v.is_finite() {
        return Err(bad(format!("{what}: tolerance must be finite")));
    }
    Ok(())
}

/// Checks the op's arguments against the B-rep's counts (see [`fixture::counts`]).
fn check_args(op: Op, a: &[f64], c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, _, n_f, _, n_l, _, _, _, _] = c;
    let [a0, a1, a2, a3, a4, a5] =
        <[f64; N_ARGS]>::try_from(a).map_err(|_| bad(format!("need {N_ARGS} arguments")))?;
    let unused = |xs: &[f64]| -> Result<(), FfiError> {
        if xs.iter().any(|&x| x != 0.0) {
            return Err(bad("unused arguments must be 0"));
        }
        Ok(())
    };
    if op == Op::Types {
        return unused(a);
    }
    index(a0, n_f, "face")?;
    match op {
        Op::Types => {}
        Op::Lamina => {
            index(a1, 3, "lamina query")?;
            unused(&[a2, a3, a4, a5])?;
        }
        Op::SetTol => {
            tol(a1, "SetTolerance")?;
            flag(a2, "only_if_larger")?;
            flag(a3, "cascade")?;
            unused(&[a4, a5])?;
        }
        Op::Notify => {
            index(a1, NOTIFY_OPS, "notify operation")?;
            unused(&[a2, a3, a4, a5])?;
        }
        Op::LoopOrient => {
            index(a1, n_l, "loop")?;
            flag(a2, "want_bounds")?;
            flag(a3, "do_3d")?;
            unused(&[a4, a5])?;
        }
        Op::Degenerate => {
            flag(a1, "skip_quick")?;
            flag(a2, "has_tol")?;
            tol(a3, "IsDegenerate")?;
            unused(&[a4, a5])?;
        }
        Op::Composites => unused(&[a1, a2, a3, a4, a5])?,
        Op::UvTrim => {
            flag(a1, "adjust")?;
            flag(a2, "pass own UV curves")?;
            unused(&[a3, a4, a5])?;
        }
        Op::Intersect => {
            index(a1, n_f, "other face")?;
            if !(a2.is_finite() && a2 > 0.0) {
                return Err(bad("approximation tolerance must be finite and positive"));
            }
            // 0 is valid: upstream only converts it to radians (`SmFace.cpp:11342`) and
            // hands it to the surface/surface intersector
            if !(a3.is_finite() && (0.0..=180.0).contains(&a3)) {
                return Err(bad("angle tolerance outside [0, 180] degrees"));
            }
            if ![-1.0, 0.0, 1.0].contains(&a4) {
                return Err(bad("coincidence must be -1, 0 or 1"));
            }
            flag(a5, "want_points")?;
        }
    }
    Ok(())
}

/// Runs `op` with `args` on the B-rep built from `block` (see the module docs); `path`
/// is the `.smb` file of a file block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed block or argument; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    super::topology_edgeuse_p2::check_block_magnitudes(block)?;
    // validates the complete block and counts the B-rep's entities
    let export = fixture::eval(FixtureOp::Export, path, block)?;
    let Some(c) = fixture::counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    if op == Op::Types {
        check_args(op, &[0.0; N_ARGS], c)?;
        return raw(op, path, block, &[]);
    }
    check_args(op, args, c)?;
    raw(op, path, block, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_reject_bad_arguments() -> Result<(), FfiError> {
        // the wire network of my_test_topology has no faces: every face op is rejected
        let net = [3.0, 0.0, 0.0];
        let types = eval(Op::Types, "", &net, &[])?;
        assert_eq!(types.first(), Some(&0.0));
        assert!(matches!(
            eval(Op::Lamina, "", &net, &[0.0; N_ARGS]),
            Err(FfiError::InvalidArgument(_))
        ));
        // a plane face
        let face = [
            2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0,
        ];
        assert!(eval(Op::Lamina, "", &face, &[0.0, 2.0, 0.0, 0.0, 0.0, 0.0]).is_ok());
        for args in [
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0, 0.0, 0.0],
            [0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        ] {
            assert!(matches!(
                eval(Op::Lamina, "", &face, &args),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        for args in [
            [0.0, 0.0, 0.0, 30.0, 0.0, 0.0],
            [0.0, 0.0, 1e-3, 181.0, 0.0, 0.0],
            [0.0, 0.0, 1e-3, 30.0, 2.0, 0.0],
            [0.0, 1.0, 1e-3, 30.0, 0.0, 0.0],
        ] {
            assert!(matches!(
                eval(Op::Intersect, "", &face, &args),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        assert!(matches!(
            eval(Op::SetTol, "", &face, &[0.0, f64::NAN, 1.0, 1.0, 0.0, 0.0]),
            Err(FfiError::InvalidArgument(_))
        ));
        assert!(matches!(
            eval(Op::Notify, "", &face, &[0.0, 5.0, 0.0, 0.0, 0.0, 0.0]),
            Err(FfiError::InvalidArgument(_))
        ));
        Ok(())
    }
}
