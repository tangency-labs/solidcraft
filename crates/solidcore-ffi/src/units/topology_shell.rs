//! Native shell/region queries on genuine SMLib B-rep fixtures.
//!
//! Upstream: `SmShell.cpp`, `SmRegion.cpp` and their headers.
//! Fixture blocks use the validated `topology.edgeuse_p1` protocol. Results are
//! flat integer-valued records; pointers remain inside the native callback.

use super::topology_edgeuse_p1::{self as fixture, Op};
use crate::{call_with_buffer, FfiError};

fn invalid(reason: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(reason.into())
}

/// Compare-ready shell/region records, optionally setting each region's native
/// void state. `SmBoolean` is an integer; its indeterminate state is preserved.
///
/// # Errors
/// Malformed fixture inputs, failed native construction, or unsafe native
/// mate/owner references are harness errors rather than comparison results.
pub fn query(path: &str, block: &[f64], void_state: Option<i32>) -> Result<Vec<f64>, FfiError> {
    let export = fixture::eval(Op::Export, path, block)?;
    if fixture::counts(&export).is_none() {
        return Err(invalid("native topology fixture construction failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: the existing fixture export validated the entire input block.
        // Strings, arrays and length pointers remain live throughout this call;
        // the shim guards native pointers and honours the output capacity.
        unsafe {
            sys::sc_topology_shell_query(
                path.as_ptr(),
                block.as_ptr(),
                count,
                i32::from(void_state.is_some()),
                void_state.unwrap_or(0),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Native `SmShell::GetWireEdges`, including native stack traversal order.
///
/// # Errors
/// Malformed fixtures, native construction failures or unsafe wire heads.
pub fn wire_edges(path: &str, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    let export = fixture::eval(Op::Export, path, block)?;
    if fixture::counts(&export).is_none() {
        return Err(invalid("native topology fixture construction failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: the fixture export validated the complete input. All buffers
        // and pointers remain live, and the shim guards native wire heads.
        unsafe {
            sys::sc_topology_shell_wire_edges(
                path.as_ptr(),
                block.as_ptr(),
                count,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Native child transfer plus all affected raw topology state and wire queries.
/// Empty shells use the real native constructor in the B-rep's context. At most
/// 64 extra shells and 64 candidate entries are accepted by this fixture adapter.
/// `pre` lists (source, recipient) transfers without a candidate list that run first
/// (their statuses end the output).
///
/// # Errors
/// Malformed fixture data, out-of-range identities, failed native setup or a
/// nullable candidate on a branch that would dereference it in native code.
pub fn transfer(
    path: &str,
    block: &[f64],
    extra_empty: usize,
    source: usize,
    recipient: usize,
    candidates: Option<&[Option<usize>]>,
    split_faceuse: bool,
    pre: &[(usize, usize)],
) -> Result<Vec<f64>, FfiError> {
    let export = fixture::eval(Op::Export, path, block)?;
    let counts = fixture::counts(&export)
        .ok_or_else(|| invalid("native topology fixture construction failed"))?;
    if split_faceuse && extra_empty == 0 {
        return Err(invalid("split fixture needs an extra shell"));
    }
    if extra_empty > 64 {
        return Err(invalid("too many extra shells"));
    }
    let shell_count = counts
        .get(1)
        .copied()
        .and_then(|n| n.checked_add(extra_empty))
        .ok_or_else(|| invalid("shell count overflow"))?;
    let edge_count = counts
        .get(7)
        .copied()
        .ok_or_else(|| invalid("missing edge count"))?;
    if source >= shell_count || recipient >= shell_count {
        return Err(invalid("shell index out of range"));
    }
    if pre.len() > 8 {
        return Err(invalid("too many preliminary transfers"));
    }
    let mut pre_flat = Vec::new();
    for &(from, to) in pre {
        if from >= shell_count || to >= shell_count {
            return Err(invalid("preliminary shell index out of range"));
        }
        pre_flat.push(i64::try_from(from).map_err(|_| invalid("shell index overflow"))?);
        pre_flat.push(i64::try_from(to).map_err(|_| invalid("shell index overflow"))?);
    }
    let entries = candidates.unwrap_or(&[]);
    if entries.len() > 64 {
        return Err(invalid("too many wire candidates"));
    }
    let mut flat = Vec::new();
    for &entry in entries {
        flat.push(match entry {
            None => -1,
            Some(index) if index < edge_count => {
                i64::try_from(index).map_err(|_| invalid("edge index overflow"))?
            }
            Some(_) => return Err(invalid("edge index out of range")),
        });
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    let source = i64::try_from(source).map_err(|_| invalid("shell index overflow"))?;
    let recipient = i64::try_from(recipient).map_err(|_| invalid("shell index overflow"))?;
    call_with_buffer(4096, |buffer, len| {
        // SAFETY: the existing fixture wrapper validated the entire block.
        // All identities, lengths and flags are bounded above; the native shim
        // guards nullable candidates before unsafe branches, keeps native objects
        // inside its callback and honours the output capacity.
        unsafe {
            sys::sc_topology_shell_transfer(
                path.as_ptr(),
                block.as_ptr(),
                count,
                extra_empty as i32,
                source,
                recipient,
                i32::from(candidates.is_some()),
                flat.as_ptr(),
                flat.len() as i64,
                i32::from(split_faceuse),
                pre_flat.as_ptr(),
                pre.len() as i64,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Set arbitrary native shell metadata and compare checked getters and the
/// unchanged physical head/count. `None` constructs a real empty native shell.
/// Integer C data retains every type-code bit, including values beyond 2^53.
///
/// # Errors
/// Malformed fixtures, failed native setup or an out-of-range shell identity.
pub fn type_members(
    path: &str,
    block: &[f64],
    shell: Option<usize>,
    code: i64,
) -> Result<[i64; 11], FfiError> {
    let export = fixture::eval(Op::Export, path, block)?;
    let counts =
        fixture::counts(&export).ok_or_else(|| invalid("native type fixture build failed"))?;
    let shell = match shell {
        None => -1,
        Some(index) if index < counts.get(1).copied().unwrap_or(0) => {
            i64::try_from(index).map_err(|_| invalid("shell index overflow"))?
        }
        Some(_) => return Err(invalid("shell index out of range")),
    };
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    let mut output = [0_i64; 11];
    // SAFETY: the complete block was validated above. All pointers and lengths
    // remain valid, and the shim bounds the identity, type code and output buffer.
    let status = unsafe {
        sys::sc_topology_shell_type_members(
            path.as_ptr(),
            block.as_ptr(),
            count,
            shell,
            code,
            output.as_mut_ptr(),
            output.len() as i64,
        )
    };
    crate::check(status)?;
    Ok(output)
}

/// Native shell/region degeneracy using the actual default or caller tolerance.
/// Non-finite tolerances are defined comparisons in `IsPointSized` and remain
/// allowed; vertex-shell short circuits ignore the tolerance as upstream does.
///
/// # Errors
/// Malformed fixture data, failed native setup or a native shim failure.
pub fn degeneracy(path: &str, block: &[f64], tolerance: Option<f64>) -> Result<Vec<f64>, FfiError> {
    let export = fixture::eval(Op::Export, path, block)?;
    if fixture::counts(&export).is_none() {
        return Err(invalid("native degeneracy fixture build failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: the entire block was validated; buffers/lengths stay live and
        // the shim honours capacity. Tolerance is only used in defined numeric
        // comparisons, never to determine an allocation or array index.
        unsafe {
            sys::sc_topology_shell_degeneracy(
                path.as_ptr(),
                block.as_ptr(),
                count,
                i32::from(tolerance.is_some()),
                tolerance.unwrap_or(0.0),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Execute native shell ray queries, retaining output writes on ordinary errors.
///
/// # Errors
/// Malformed fixtures, failed native setup, or a status of `SC_ERR_ARG` for inputs
/// that would crash native code (non-finite ray values once a shell has boundary
/// faces, which reach the intersection solver).
pub fn ray_fire(
    path: &str,
    block: &[f64],
    start: [f64; 3],
    direction: [f64; 3],
    tolerance: f64,
    radius: Option<f64>,
) -> Result<Vec<f64>, FfiError> {
    let export = fixture::eval(Op::Export, path, block)?;
    if fixture::counts(&export).is_none() {
        return Err(invalid("native ray fixture build failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: complete fixture export validates the input block; arrays and
        // pointers stay live and the shim checks references and output capacity.
        unsafe {
            sys::sc_topology_shell_ray(
                path.as_ptr(),
                block.as_ptr(),
                count,
                start.as_ptr(),
                direction.as_ptr(),
                tolerance,
                i32::from(radius.is_some()),
                radius.unwrap_or(0.0),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Native circular region ordering after adding actual empty shells at its head.
/// The new shells remain region-owned and are destroyed with the fixture.
/// Optional gathering uses native base list operations to put face-use shells
/// in the first region before testing the multiple-shell selection branch.
///
/// # Errors
/// Malformed fixture data, out-of-range prefix or failed native setup.
pub fn region_order(
    path: &str,
    block: &[f64],
    prefix_empty: usize,
    gather_faces: bool,
) -> Result<Vec<f64>, FfiError> {
    if prefix_empty > 8 {
        return Err(invalid("region prefix outside 0..=8"));
    }
    let export = fixture::eval(Op::Export, path, block)?;
    if fixture::counts(&export).is_none() {
        return Err(invalid("native ordering fixture build failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: the complete fixture and prefix were validated; all buffers
        // remain live and the shim checks capacity before copying output.
        unsafe {
            sys::sc_topology_shell_region_order(
                path.as_ptr(),
                block.as_ptr(),
                count,
                prefix_empty as i32,
                i32::from(gather_faces),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Native region shell filtering with real long attributes on constructed shells.
/// Attribute ids retain all 64 bits; absent filtering passes a NULL pointer.
///
/// # Errors
/// Malformed fixture data, invalid tag pattern or failed/colliding native setup.
pub fn region_filter(
    path: &str,
    block: &[f64],
    attribute_id: u64,
    enabled: bool,
    pattern: u8,
) -> Result<Vec<f64>, FfiError> {
    if pattern > 2 {
        return Err(invalid("invalid region tag pattern"));
    }
    let export = fixture::eval(Op::Export, path, block)?;
    if fixture::counts(&export).is_none() {
        return Err(invalid("native filtering fixture build failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path contains NUL"))?;
    let count = i64::try_from(block.len()).map_err(|_| invalid("fixture too long"))?;
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: complete fixture and pattern validation precede this call;
        // the shim preserves the integer id and honours the output capacity.
        unsafe {
            sys::sc_topology_shell_region_filter(
                path.as_ptr(),
                block.as_ptr(),
                count,
                attribute_id,
                i32::from(enabled),
                i32::from(pattern),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

mod sys {
    unsafe extern "C" {
        pub fn sc_topology_shell_region_filter(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            attribute_id: u64,
            enabled: i32,
            pattern: i32,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
        pub fn sc_topology_shell_region_order(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            prefix_empty: i32,
            gather_faces: i32,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
        pub fn sc_topology_shell_ray(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            start: *const f64,
            direction: *const f64,
            tolerance: f64,
            has_radius: i32,
            radius: f64,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
        pub fn sc_topology_shell_degeneracy(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            has_tolerance: i32,
            tolerance: f64,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
        pub fn sc_topology_shell_type_members(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            shell_index: i64,
            type_code: i64,
            output: *mut i64,
            capacity: i64,
        ) -> i32;
        pub fn sc_topology_shell_transfer(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            extra_empty: i32,
            source: i64,
            recipient: i64,
            has_candidates: i32,
            candidates: *const i64,
            candidate_count: i64,
            split_faceuse: i32,
            pre: *const i64,
            pre_count: i64,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
        pub fn sc_topology_shell_wire_edges(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
        pub fn sc_topology_shell_query(
            path: *const std::ffi::c_char,
            input: *const f64,
            count: i64,
            set_void: i32,
            void_state: i32,
            output: *mut f64,
            capacity: i64,
            output_len: *mut i64,
        ) -> i32;
    }
}
