//! Runs a case's C++ side in a fresh forked child process
//! (`harness/cpp/shim/sc_isolate.cpp`).
//!
//! Upstream keeps state in function statics that the first call in a process fixes
//! (ND-3: `SmFilletSolver::ComputePointValues`' `sdStaticStep`). Run in-process, a
//! case's C++ result would depend on the cases before it. [`fresh_process`] forks, runs
//! the closure (its shim calls) in the child, sends the result back through a pipe and
//! ends the child, so every case starts from the parent's state and nothing it latches
//! outlives it. Any latched state a runner compares is read inside the same closure,
//! after the run. Nothing read in the child is ever handed to the Rust kernel: it is a
//! compared output.
//!
//! A child that dies (signal, the 600 s alarm, an abnormal exit) or sends a short
//! message is [`SC_ERR_CHILD_CRASHED`](crate::SC_ERR_CHILD_CRASHED). Where fork is not
//! available (non-Unix, wasm32) or fails, the closure runs in-process.

use crate::FfiError;

#[cfg(unix)]
extern "C" {
    fn sc_isolate_fork(fd: *mut i32) -> i64;
    fn sc_isolate_exit() -> !;
    fn sc_isolate_wait(pid: i64) -> i32;
}

const TAG_OK: u8 = 0;
const TAG_STATUS: u8 = 1;
const TAG_INVALID: u8 = 2;

fn encode(r: &Result<Vec<f64>, FfiError>) -> Vec<u8> {
    let mut b = Vec::new();
    match r {
        Ok(v) => {
            b.push(TAG_OK);
            b.extend_from_slice(&(v.len() as u64).to_le_bytes());
            for x in v {
                b.extend_from_slice(&x.to_bits().to_le_bytes());
            }
        }
        Err(FfiError::Status(s)) => {
            b.push(TAG_STATUS);
            b.extend_from_slice(&s.to_le_bytes());
        }
        Err(FfiError::InvalidArgument(m)) => {
            b.push(TAG_INVALID);
            b.extend_from_slice(&(m.len() as u64).to_le_bytes());
            b.extend_from_slice(m.as_bytes());
        }
    }
    b
}

fn decode(b: &[u8]) -> Option<Result<Vec<f64>, FfiError>> {
    let (&tag, rest) = b.split_first()?;
    let word = |s: &[u8]| -> Option<u64> { Some(u64::from_le_bytes(s.get(..8)?.try_into().ok()?)) };
    match tag {
        TAG_OK => {
            let n = usize::try_from(word(rest)?).ok()?;
            let body = rest.get(8..)?;
            if body.len() != n.checked_mul(8)? {
                return None;
            }
            Some(Ok(body
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| f64::from_le_bytes(*c))
                .collect()))
        }
        TAG_STATUS => {
            let s: [u8; 4] = rest.try_into().ok()?;
            Some(Err(FfiError::Status(i32::from_le_bytes(s))))
        }
        TAG_INVALID => {
            let n = usize::try_from(word(rest)?).ok()?;
            let body = rest.get(8..)?;
            if body.len() != n {
                return None;
            }
            Some(Err(FfiError::InvalidArgument(
                String::from_utf8_lossy(body).into_owned(),
            )))
        }
        _ => None,
    }
}

/// Runs `case` (a case's C++ calls) in a fresh forked child and returns its result.
///
/// # Errors
///
/// The closure's own error, or [`FfiError::Status`]`(`[`SC_ERR_CHILD_CRASHED`](crate::SC_ERR_CHILD_CRASHED)`)`
/// when the child did not finish normally.
pub fn fresh_process(
    case: impl FnOnce() -> Result<Vec<f64>, FfiError>,
) -> Result<Vec<f64>, FfiError> {
    #[cfg(unix)]
    {
        use std::io::{Read, Write};
        use std::os::fd::FromRawFd;

        let mut fd: i32 = -1;
        // SAFETY: `fd` is a valid i32 the shim writes once.
        let pid = unsafe { sc_isolate_fork(&raw mut fd) };
        if pid == 0 {
            // The child: run the case, send its result, end without returning (and
            // without unwinding into the parent's frames).
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(case));
            if let Ok(r) = r {
                // SAFETY: `fd` is the pipe's write end, owned by this child only.
                let mut pipe = unsafe { std::fs::File::from_raw_fd(fd) };
                let _ = pipe.write_all(&encode(&r));
            }
            // SAFETY: ends the child process; never returns.
            unsafe { sc_isolate_exit() }
        }
        if pid > 0 {
            let mut buf = Vec::new();
            // SAFETY: `fd` is the pipe's read end, owned by this parent only.
            let mut pipe = unsafe { std::fs::File::from_raw_fd(fd) };
            let read = pipe.read_to_end(&mut buf);
            drop(pipe);
            // SAFETY: `pid` is this process's child, waited for once.
            let ok = unsafe { sc_isolate_wait(pid) } == 1;
            return match (ok, read) {
                (true, Ok(_)) => {
                    decode(&buf).unwrap_or(Err(FfiError::Status(crate::SC_ERR_CHILD_CRASHED)))
                }
                _ => Err(FfiError::Status(crate::SC_ERR_CHILD_CRASHED)),
            };
        }
    }
    case()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for r in [
            Ok(vec![1.5, -0.0, f64::NAN, 1e-300]),
            Ok(vec![]),
            Err(FfiError::Status(-1002)),
            Err(FfiError::InvalidArgument("bad".into())),
        ] {
            let d = decode(&encode(&r)).unwrap();
            match (&r, &d) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (x, y) in a.iter().zip(b) {
                        assert_eq!(x.to_bits(), y.to_bits());
                    }
                }
                _ => assert_eq!(r, d),
            }
        }
        assert!(decode(&[]).is_none());
        assert!(decode(&[TAG_OK, 1, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn child_result_and_crash() {
        assert_eq!(fresh_process(|| Ok(vec![2.0, 3.0])), Ok(vec![2.0, 3.0]));
        assert_eq!(
            fresh_process(|| Err(FfiError::Status(7))),
            Err(FfiError::Status(7))
        );
        // A panicking child sends nothing: reported as a crash, never unwound here.
        assert_eq!(
            fresh_process(|| panic!("child")),
            Err(FfiError::Status(crate::SC_ERR_CHILD_CRASHED))
        );
        // State the child changes does not reach the parent.
        let mut seen = 0;
        let r = fresh_process(|| {
            seen = 1;
            Ok(vec![f64::from(seen)])
        });
        assert_eq!(r, Ok(vec![1.0]));
        assert_eq!(seen, 0);
    }
}
