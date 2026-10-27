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
//! [`set_heap_pad`] (off by default) makes the child allocate a few small blocks before
//! the closure and free them after it. That moves the C++ side's heap addresses and
//! nothing else, so a C++ result that changes with it depends on addresses (ND-2,
//! ND-258): `difftest --heap-pads`. [`padded_children`] counts the children that padded.
//!
//! A child that dies (signal, the 600 s alarm, an abnormal exit) or sends a short
//! message is [`SC_ERR_CHILD_CRASHED`](crate::SC_ERR_CHILD_CRASHED). Where fork is not
//! available (non-Unix, wasm32) or fails, the closure runs in-process.

use crate::FfiError;

#[cfg(unix)]
extern "C" {
    fn sc_isolate_fork(fd: *mut i32) -> i64;
    fn sc_isolate_alarm(seconds: u32);
    fn sc_isolate_exit() -> !;
    fn sc_isolate_wait(pid: i64) -> i32;
}

// Per thread: `fresh_process` forks from the thread that calls it, so a setting made on
// one thread never pads (or counts) another thread's children, e.g. parallel tests.
thread_local! {
    /// Blocks the forked child allocates before a case ([`set_heap_pad`]); 0 = none.
    static HEAP_PAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Forked children that applied a nonzero pad ([`padded_children`]).
    static PADDED_CHILDREN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Sets how many small blocks (24 to 264 bytes) every later [`fresh_process`] child
/// started from this thread allocates before running its case and frees after it; `0`
/// (the default) allocates nothing. Only forked children are padded: a case run
/// in-process (no fork, or a failed fork) is not.
pub fn set_heap_pad(blocks: usize) {
    HEAP_PAD.with(|p| p.set(blocks));
}

/// This thread's [`set_heap_pad`] setting.
#[must_use]
pub fn heap_pad() -> usize {
    HEAP_PAD.with(std::cell::Cell::get)
}

/// How many [`fresh_process`] children this thread has forked with a nonzero
/// [`set_heap_pad`]. A case run in-process (no fork, or a failed fork) is never padded
/// and never counted, so a caller tells a padded run from an unpadded one by the change
/// in this count.
#[must_use]
pub fn padded_children() -> usize {
    PADDED_CHILDREN.with(std::cell::Cell::get)
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
    fresh_process_within(MAX_SECONDS, case)
}

/// The child's default (and longest) run time in seconds.
pub const MAX_SECONDS: u32 = 600;

/// [`fresh_process`] with the child's alarm at `seconds` (clamped to `1..=`[`MAX_SECONDS`])
/// instead of [`MAX_SECONDS`]: for a case whose native call may never end, so that a
/// child that runs past it is reported as crashed sooner.
///
/// # Errors
///
/// As [`fresh_process`].
pub fn fresh_process_within(
    seconds: u32,
    case: impl FnOnce() -> Result<Vec<f64>, FfiError>,
) -> Result<Vec<f64>, FfiError> {
    #[cfg(unix)]
    {
        use std::io::{Read, Write};
        use std::os::fd::FromRawFd;

        let mut fd: i32 = -1;
        // read once, before the fork: the child pads with it and the parent counts it
        let pad_blocks = heap_pad();
        // SAFETY: `fd` is a valid i32 the shim writes once.
        let pid = unsafe { sc_isolate_fork(&raw mut fd) };
        if pid == 0 {
            // The child: run the case, send its result, end without returning (and
            // without unwinding into the parent's frames).
            // SAFETY: only sets this child's alarm.
            unsafe { sc_isolate_alarm(seconds.clamp(1, MAX_SECONDS)) };
            // Held across the case so the C++ side allocates around it (`set_heap_pad`).
            let pad: Vec<Vec<u8>> = (0..pad_blocks)
                .map(|i| vec![1u8; 24 + 40 * (i % 7)])
                .collect();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(case));
            drop(std::hint::black_box(pad));
            if let Ok(r) = r {
                // SAFETY: `fd` is the pipe's write end, owned by this child only.
                let mut pipe = unsafe { std::fs::File::from_raw_fd(fd) };
                let _ = pipe.write_all(&encode(&r));
            }
            // SAFETY: ends the child process; never returns.
            unsafe { sc_isolate_exit() }
        }
        if pid > 0 {
            if pad_blocks > 0 {
                PADDED_CHILDREN.with(|n| n.set(n.get() + 1));
            }
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
    #[cfg(not(unix))]
    let _ = seconds;
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

    #[cfg(unix)]
    #[test]
    fn child_past_its_alarm() {
        assert_eq!(
            fresh_process_within(1, || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                Ok(vec![1.0])
            }),
            Err(FfiError::Status(crate::SC_ERR_CHILD_CRASHED))
        );
    }
}
