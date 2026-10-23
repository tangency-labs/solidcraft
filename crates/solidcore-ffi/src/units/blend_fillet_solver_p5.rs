//! Checked fixture wrapper for `SmFilletSolver`'s rollover operations.
//!
//! Layout: operation (0 process, 1 empty `TestRollOver`, 2 two real rails on a
//! rectangular face), geometry type (0..6), corner type (-1 null, 0..12), adjusted
//! list length and its 0/1 corner identities, then two classifications. Each is a
//! count (-1 null, 0..8) followed by (min, max, `PointClassType` 100..104 or 109) triples.
//! Process fixtures intentionally have no rail geometry or fillet surface: blend,
//! missing-class, single-interval and unsupported-corner returns are compared;
//! multiple intervals reach `IsConvexRadialSector` on a wire and the null-surface
//! check. Supported full rollover with an unknown midpoint needs real face/curve
//! data and is rejected for this incomplete fixture only. `TestRollOver` op 2 builds
//! real `SmLine` rails inside a real planar face; Rust uses the real classification
//! reset and propagates remaining topology/refinement dependencies.
//! Native fixture construction, evaluation and destruction run in a bounded Linux/macOS
//! worker executed with `posix_spawn` (a fresh allocator image). A signal, destructor abort or stalled worker is a native exception,
//! never an operation status or rejected fixture. No SMLib pointer crosses IPC.
//! The multi-interval rail-copy guard rejects null curves only after the
//! defined null-surface return; earlier returns remain covered.
//! Unsupported platforms fail closed rather than executing SMLib in process.
//! macOS uses close-by-default spawn actions to isolate concurrent workers even
//! between pipe creation and marking descriptors close-on-exec. Native parity
//! claims require a differential run on the reported platform.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_solver_p5_target(out: *mut f64, cap: i64, len: *mut i64) -> i32;
        pub fn sc_blend_fillet_solver_p5_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn integer(x: f64, lo: i32, hi: i32) -> bool {
    x.is_finite() && x.fract() == 0.0 && x >= f64::from(lo) && x <= f64::from(hi)
}

/// Whether a serialized fixture is complete and safe for this native fixture.
#[must_use]
pub fn validate(input: &[f64]) -> bool {
    let mut values = input.iter().copied();
    let Some(op) = values.next().filter(|&x| integer(x, 0, 2)) else {
        return false;
    };
    let Some(kind) = values.next().filter(|&x| integer(x, 0, 6)) else {
        return false;
    };
    let Some(corner) = values.next().filter(|&x| integer(x, -1, 12)) else {
        return false;
    };
    let Some(nadj) = values.next().filter(|&x| integer(x, 0, 16)) else {
        return false;
    };
    for _ in 0..nadj as usize {
        if !values.next().is_some_and(|x| integer(x, 0, 1)) {
            return false;
        }
    }
    let mut counts = Vec::new();
    let mut unknown = false;
    for _ in 0..2 {
        let Some(count) = values.next().filter(|&x| integer(x, -1, 8)) else {
            return false;
        };
        counts.push(count);
        for _ in 0..count.max(0.0) as usize {
            if values.next().is_none() || values.next().is_none() {
                return false;
            }
            let Some(class) = values
                .next()
                .filter(|&x| integer(x, 100, 104) || x == 109.0)
            else {
                return false;
            };
            unknown |= class == 100.0;
        }
    }
    if values.next().is_some() {
        return false;
    }
    !(op == 0.0
        && kind != 1.0
        && counts == [1.0, 1.0]
        && unknown
        && (corner == 3.0 || corner == 7.0))
}

/// Run the actual upstream operation and return its status and post-call state.
///
/// # Errors
/// `InvalidArgument` for an incomplete/unsafe fixture, or native shim failure.
/// Native worker exceptions, signals, aborts and timeouts return `FfiError::Status(SC_ERR_EXCEPTION)`.
pub fn eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    if !validate(input) {
        return Err(FfiError::InvalidArgument("rollover fixture".into()));
    }
    let n = i64::try_from(input.len())
        .map_err(|_| FfiError::InvalidArgument("fixture length".into()))?;
    call_with_buffer(128, |out, len| {
        // SAFETY: validation bounds every native read and enum/count conversion;
        // the shim checks output capacity and retains no caller-owned pointers.
        unsafe {
            sys::sc_blend_fillet_solver_p5_eval(
                input.as_ptr(),
                n,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Export the native rectangular target fixture for exact Rust arena import.
///
/// # Errors
/// Native fixture construction or output errors.
pub fn target() -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |out, len| {
        // SAFETY: output buffer and length are writable; the shim bounds every write.
        unsafe {
            sys::sc_blend_fillet_solver_p5_target(out.as_mut_ptr(), out.len() as i64, &raw mut *len)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_follows_the_branch_that_uses_the_missing_geometry() {
        let recovery = [0., 0., 3., 0., 1., 0., 1., 100., 1., 0., 1., 102.];
        assert!(!validate(&recovery));
        assert!(eval(&recovery).is_err());
        let mut safe = recovery;
        safe[1] = 1.; // blend returns before the missing geometry is used
        assert!(eval(&safe).is_ok());
        safe[1] = 0.;
        safe[7] = 102.; // both rails classified
        assert!(eval(&safe).is_ok());
        assert_eq!(eval(&safe).unwrap().first(), Some(&0.0));
        safe[7] = 100.;
        safe[2] = 1.; // unsupported corner returns SM_ERR
        assert!(eval(&safe).is_ok());
        assert_eq!(eval(&safe).unwrap().first(), Some(&1001.0));
        for end in 0..safe.len() {
            assert!(eval(&safe[..end]).is_err());
        }
        safe[5] = f64::NAN; // this stored parameter is never evaluated
        assert!(eval(&safe).is_ok());
        safe[4] = 1e300;
        assert!(eval(&safe).is_err());
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    const SAFE: [f64; 12] = [0., 0., -1., 0., 1., 0., 1., 102., 1., 0., 1., 103.];
    #[test]
    fn worker_protocol_honors_empty_and_short_output_buffers() {
        let mut len = 0;
        // SAFETY: input contains SAFE.len() initialized doubles. A null output
        // with capacity zero is explicitly supported; len is writable.
        let status = unsafe {
            sys::sc_blend_fillet_solver_p5_eval(
                SAFE.as_ptr(),
                12,
                std::ptr::null_mut(),
                0,
                &raw mut len,
            )
        };
        assert_eq!(status, -1001);
        assert_eq!(len, 11);
        let mut out = [12345.; 10];
        // SAFETY: the initialized input and output pointers match their lengths.
        let status = unsafe {
            sys::sc_blend_fillet_solver_p5_eval(
                SAFE.as_ptr(),
                12,
                out.as_mut_ptr(),
                10,
                &raw mut len,
            )
        };
        assert_eq!(status, -1001);
        assert_eq!(out, [12345.; 10]);
        assert_eq!(len, 11);
        assert_eq!(eval(&SAFE).unwrap().len(), 11);
    }
    #[test]
    fn native_parser_checks_truncated_layout_independently_of_rust_guard() {
        let malformed = [0., 0., -1., 0., 1., 0.];
        let mut out = [0.; 128];
        let mut len = 0;
        // SAFETY: these slices are valid; only the serialized layout is malformed.
        let status = unsafe {
            sys::sc_blend_fillet_solver_p5_eval(
                malformed.as_ptr(),
                6,
                out.as_mut_ptr(),
                128,
                &raw mut len,
            )
        };
        assert_eq!(status, -1002);
    }
    #[test]
    fn repeated_concurrent_workers_return_identical_owned_snapshots() {
        let expected = eval(&SAFE).unwrap();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let expected = &expected;
                scope.spawn(move || {
                    for _ in 0..12 {
                        assert_eq!(eval(&SAFE).unwrap(), *expected);
                    }
                });
            }
        });
    }
}
