//! Safe differential fixtures for `SmBrep::ValidatePointers` and
//! `SmBrep::ContainsCoincidentTopology`. The shim uses the pinned kernel unchanged.
use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;
mod sys {
    extern "C" {
        pub fn sc_topology_brep_validation_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
/// Fixture export (0), pointer validation (1), or coincidence search (2).
/// Arguments: level, walk, tolerance, `find_all`, mutation kind/index/value.
/// Mutations 8 through 10 remove required pointers and are pointer-check only;
/// they are restored by the native fixture before destruction.
pub fn eval(op: u8, path: &str, block: &[f64], args: [f64; 7]) -> Result<Vec<f64>, FfiError> {
    validate(op, path, block, args)?;
    let bad = || FfiError::InvalidArgument("invalid B-rep validation arguments".into());
    let path = CString::new(path).map_err(|_| bad())?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: validated block and fixed-length arguments remain live throughout
        // the call; output and length pointers are supplied by call_with_buffer.
        unsafe {
            sys::sc_topology_brep_validation_eval(
                i32::from(op),
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                args.as_ptr(),
                7,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// The argument and fixture-block checks [`eval`] applies before calling the shim: the
/// inputs it refuses are those that are undefined in or crash the C++. The difftest
/// runner applies the same checks to its Rust side (PR #502's convention), so an input
/// the shim never runs (e.g. a sheet grid of more than 512 faces) is never built in Rust
/// either.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a refused input.
pub fn validate(op: u8, path: &str, block: &[f64], args: [f64; 7]) -> Result<(), FfiError> {
    let bad = || FfiError::InvalidArgument("invalid B-rep validation arguments".into());
    if op > 2
        || !args.iter().all(|x| x.is_finite())
        || args[0].fract() != 0.0
        || !(0.0..=4.0).contains(&args[0])
        || ![args[1], args[3]].iter().all(|x| *x == 0.0 || *x == 1.0)
        || args[2].abs() > 1e6
        || args[4].fract() != 0.0
        || !(0.0..=10.0).contains(&args[4])
        || args[5].fract() != 0.0
        || !(0.0..=1e6).contains(&args[5])
        || args[6].abs() > 1e6
    {
        return Err(bad());
    }
    if op == 2 && args[4] != 0.0 {
        return Err(bad());
    }
    if (args[4] == 4.0 || args[4] == 5.0)
        && (!(0.0..=2.0).contains(&args[6]) || args[6].fract() != 0.0)
    {
        return Err(bad());
    }
    if block.first() == Some(&100.0) {
        if !matches!(block, [_, count] if count.fract() == 0.0 && (1.0..=512.0).contains(count)) {
            return Err(bad());
        }
    } else if block.first() == Some(&101.0) {
        if !matches!(block, [_, mode] if *mode == 1.0 || *mode == 2.0) {
            return Err(bad());
        }
    } else {
        let mut rd = Reader {
            input: block,
            at: 0,
        };
        brep_block(&mut rd, path)?;
        rd.done()?;
    }
    CString::new(path).map_err(|_| bad())?;
    Ok(())
}
