//! Safe Rust access to the upstream SMLib C++ kernel, used as the oracle by
//! the `difftest` comparison runner.
//!
//! * Without feature `cpp` this crate is empty (it always compiles, including
//!   for `wasm32-unknown-unknown`).
//! * With feature `cpp`, `build.rs` compiles `harness/cpp/shim/*.cpp` and links
//!   the static libraries in `bin/cpp/lib`. Each file `src/units/<unit>.rs`
//!   becomes the module [`units`]`::<unit>` automatically (no registry edits).
//!
//! This is the only crate in the workspace allowed to contain `unsafe`. Every
//! `extern "C"` declaration must be private to its unit module and wrapped by
//! a safe function that validates all lengths/indices *before* the call so the
//! C++ side can never read or write out of bounds.
//!
//! Native shim bodies share a recursive mutex, acquired before native state is
//! accessed and held through object destruction and callback restoration. This
//! also covers wrappers that do not use [`call_with_buffer`]: upstream's API
//! context, notification callbacks and system tolerances are process-global.
//! Concurrent callers are therefore serialized at each native entrypoint.

#![allow(unsafe_code)] // the only crate allowed unsafe: safe wrappers over extern "C" shims
#![deny(missing_docs)]

use std::fmt;

/// Error returned by a safe FFI wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiError {
    /// The wrapper rejected the arguments before calling C++ (would be UB / out of range).
    InvalidArgument(String),
    /// The shim returned a nonzero status (upstream error flag or `SC_ERR_*` code).
    Status(i32),
}

impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FfiError::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
            FfiError::Status(s) => write!(f, "{}", status_name(*s)),
        }
    }
}

impl std::error::Error for FfiError {}

/// Shim status: success.
pub const SC_OK: i32 = 0;
/// Shim status: a C++ exception was caught at the boundary.
pub const SC_ERR_EXCEPTION: i32 = -1000;
/// Shim status: a caller-provided output buffer was too small.
pub const SC_ERR_BUFFER: i32 = -1001;
/// Shim status: the shim rejected its arguments.
pub const SC_ERR_ARG: i32 = -1002;
/// Status of a case whose C++ side ran in a fresh child process ([`isolate`]) that did
/// not finish normally (a native crash, the 600 s alarm, an abnormal exit).
pub const SC_ERR_CHILD_CRASHED: i32 = -1100;
/// Shim status: the isolated worker's pipes could not be made or written
/// (`harness/cpp/shim/sc_rollover_worker.h`; the shim also logs the stage and errno).
pub const SC_ERR_WORKER_PIPE: i32 = -1120;
/// Shim status: the isolated worker could not be spawned.
pub const SC_ERR_WORKER_SPAWN: i32 = -1121;
/// Shim status: the isolated worker did not reply within its timeout and was killed.
pub const SC_ERR_WORKER_TIMEOUT: i32 = -1122;
/// Shim status: the isolated worker's reply was short, unreadable or malformed.
pub const SC_ERR_WORKER_IO: i32 = -1123;
/// Shim status: the isolated worker exited with a nonzero code.
pub const SC_ERR_WORKER_EXIT: i32 = -1124;
/// Shim status: the isolated worker was killed by a signal (a native crash or abort).
pub const SC_ERR_WORKER_SIGNAL: i32 = -1125;
/// Shim status: the isolated worker could not be reaped.
pub const SC_ERR_WORKER_WAIT: i32 = -1126;
/// Shim status on wasm32: the C++ kernel is not linked (returned by every stub of the
/// module that `scripts/wasm_stub.py` generates for `difftest --digest-dir`).
pub const SC_ERR_UNLINKED: i32 = -1009;

/// Human readable, stable name of a shim status code (used in difftest outputs).
pub fn status_name(s: i32) -> String {
    match s {
        SC_OK => "ok".into(),
        SC_ERR_EXCEPTION => "cpp_exception".into(),
        SC_ERR_BUFFER => "buffer_too_small".into(),
        SC_ERR_ARG => "shim_rejected_args".into(),
        SC_ERR_UNLINKED => "cpp_unlinked".into(),
        SC_ERR_CHILD_CRASHED => "cpp_child_crashed".into(),
        SC_ERR_WORKER_PIPE => "cpp_worker_pipe_failed".into(),
        SC_ERR_WORKER_SPAWN => "cpp_worker_spawn_failed".into(),
        SC_ERR_WORKER_TIMEOUT => "cpp_worker_timeout".into(),
        SC_ERR_WORKER_IO => "cpp_worker_short_reply".into(),
        SC_ERR_WORKER_EXIT => "cpp_worker_nonzero_exit".into(),
        SC_ERR_WORKER_SIGNAL => "cpp_worker_signal".into(),
        SC_ERR_WORKER_WAIT => "cpp_worker_not_reaped".into(),
        other => format!("status_{other}"),
    }
}

/// Converts a shim status into `Ok(())` / `Err(FfiError::Status)`.
pub fn check(status: i32) -> Result<(), FfiError> {
    if status == SC_OK {
        Ok(())
    } else {
        Err(FfiError::Status(status))
    }
}

/// Calls a shim that fills a caller-provided buffer of variable length.
///
/// `call(buf, &mut len)` must invoke the shim with `buf.as_mut_ptr()`,
/// `buf.len()` as capacity and `&raw mut len`. Shim contract (see
/// `sc_copy_out` in `harness/cpp/shim/sc_shim.h`): it always writes the
/// required length to `len`; if that exceeds the capacity it writes nothing
/// else and returns `SC_ERR_BUFFER`, and this helper retries once with an
/// exact-size buffer. The returned vector is truncated to `len`.
pub fn call_with_buffer<T: Copy + Default>(
    initial_cap: usize,
    mut call: impl FnMut(&mut [T], &mut i64) -> i32,
) -> Result<Vec<T>, FfiError> {
    let mut cap = initial_cap.max(1);
    for _ in 0..2 {
        let mut buf = vec![T::default(); cap];
        let mut len: i64 = 0;
        let st = call(&mut buf, &mut len);
        let n = usize::try_from(len)
            .map_err(|_| FfiError::InvalidArgument(format!("shim returned length {len}")))?;
        if st == SC_ERR_BUFFER && n > cap {
            cap = n;
            continue;
        }
        check(st)?;
        if n > cap {
            return Err(FfiError::InvalidArgument(format!(
                "shim returned length {n} > capacity {cap}"
            )));
        }
        buf.truncate(n);
        return Ok(buf);
    }
    Err(FfiError::Status(SC_ERR_BUFFER))
}

/// Whether this build links the C++ kernel (feature `cpp`; on wasm32 the shims are
/// stubs, see [`unlinked_calls`]).
pub const LINKED: bool = cfg!(feature = "cpp");

#[cfg(feature = "cpp")]
extern "C" {
    fn sc_cpp_stdlib_is_libstdcxx() -> i32;
}

/// Whether the linked C++ reference was built against libstdc++.
#[cfg(feature = "cpp")]
pub fn cpp_stdlib_is_libstdcxx() -> bool {
    unsafe { sc_cpp_stdlib_is_libstdcxx() != 0 }
}

/// Whether the linked C++ reference was built against libstdc++.
///
/// Without the C++ kernel there is no probe: nothing links against a C++
/// standard library, so treat it as "not libstdc++" would never be seen.
#[cfg(not(feature = "cpp"))]
pub fn cpp_stdlib_is_libstdcxx() -> bool {
    false
}

/// Number of shim calls so far that reached a stub instead of the C++ kernel: always 0
/// natively; on wasm32 the count kept by the stub module from `scripts/wasm_stub.py`.
pub fn unlinked_calls() -> u64 {
    #[cfg(all(feature = "cpp", target_arch = "wasm32"))]
    {
        extern "C" {
            fn sc_wasm_unlinked_calls() -> i64;
        }
        // SAFETY: the stub module's counter getter takes no arguments and touches no
        // memory of ours.
        let n = unsafe { sc_wasm_unlinked_calls() };
        u64::try_from(n).unwrap_or(0)
    }
    #[cfg(not(all(feature = "cpp", target_arch = "wasm32")))]
    {
        0
    }
}

#[cfg(feature = "cpp")]
pub mod isolate;
#[cfg(feature = "cpp")]
mod reader;
#[cfg(feature = "cpp")]
mod validate;

/// Per-unit safe wrappers, one module per `src/units/<unit>.rs` file
/// (generated by `build.rs`; only present with feature `cpp`).
#[cfg(feature = "cpp")]
pub mod units {
    include!(concat!(env!("OUT_DIR"), "/units.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_retry() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mut calls = 0;
        let v = call_with_buffer::<f64>(2, |buf, len| {
            calls += 1;
            *len = data.len() as i64;
            if buf.len() < data.len() {
                return SC_ERR_BUFFER;
            }
            buf[..data.len()].copy_from_slice(&data);
            SC_OK
        })
        .unwrap();
        assert_eq!(v, data);
        assert_eq!(calls, 2);
        assert_eq!(
            call_with_buffer::<i64>(4, |_, _| 1),
            Err(FfiError::Status(1))
        );
    }
}
