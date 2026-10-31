//! FFI wrappers for unit `topology.edgeuse_p3`.
//!
//! Shim: `harness/cpp/shim/topology_edgeuse_p3.cpp` (`sc_topology_edgeuse_p3_eval`).
//! Upstream: `source/SMLib/src/SmEdgeuse.cpp`.
//!
//! Every op starts with a B-rep block of unit `topology.edgeuse_p1` (see
//! [`super::topology_edgeuse_p1`]; its `Export` op exports the graph) followed by the
//! edge use's index in the export's traversal order. Outputs start with `0`, or with
//! `-1 status` when the B-rep could not be built. `-2` in an output is "upstream would
//! dereference null". An edge use state is that of the `topology.edgeuse_p1` `SetUv` op
//! (`put_eu_state`); the edge state is its interval and tolerance. "The states" are the
//! edge use's and its mate's state (its UV trim curve as the full record of
//! `harness/cpp/shim/sc_trim_curve_record.h`) and the edge state.
//!
//! | [`Op`]         | parameters after `eu`                  | outputs |
//! |----------------|----------------------------------------|---------|
//! | `Create`       | `want_dev has_fail sample_lift`        | `-2`, or `status`; without a wire edge use also `dist dev` (`-1` without a curve), the curve's full UV trim curve record, optional lift samples, the record's path type (`-1` without record) and B-rep flag, then the states |
//! | `Binormal`     | `frac use_3d want_uv_vector`           | `-2`, or `status` and, on success, `P[3] B[3] T[3] N[3] u v du dv`, then the states (`CreateUVTrimCurve` may attach a curve) |
//! | `Normalized`   | `t parameter_space want_deriv ok_make` | `-2`, or `status` and, on success, `P[3] D[3]`, then the states |
//! | `ExportGaps` | no selector or arguments | `n (edgeuse_index gap edge_t u v)*`, initial cached fields in traversal order, without gap calculation |
//! | `GetOrCreate`  | `ok_make want_dist want_dev`           | `-2`, or `status dist dev has_curve`, then the states (`dist`/`dev` are `-2` when not asked) |
//!
//! Blocks with a value above [`super::topology_edgeuse_p2::MAX_BLOCK_MAGNITUDE`] are
//! refused first, before the block's `Export` builds the B-rep (HANG-29: building a
//! face on a surface with a `±1e300` control point never returns upstream; nightly
//! fuzz00246, fuzz00273, fuzz00280).
//!
//! [`eval`] accepts what [`super::topology_edgeuse_p1::eval`] accepts for the block, an
//! edge use index inside the export and finite parameters.
//!
//! Requested lift samples are `0` without a returned curve, `-2` for missing
//! geometry, or `5 curve_type` followed by five rows at edge-interval fractions
//! `0, .25, .5, .75, 1`: `status`, and on success `t xyz[3] uv[2] lifted_xyz[3]`.
//! These evaluate the new curve without attaching it to the edge use.

use super::topology_edgeuse_p1::{self as p1, bad, brep_block, Reader};
use super::topology_edgeuse_p2::check_block_magnitudes;
use crate::{call_with_buffer, FfiError};

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Create = 1,
    Binormal = 2,
    Normalized = 3,
    GetOrCreate = 4,
    ExportGaps = 5,
}

mod sys {
    extern "C" {
        pub fn sc_topology_edgeuse_p3_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::ExportGaps => {}
        Op::Create | Op::GetOrCreate => {
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Binormal => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Normalized => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval_impl` for both public entry points); buf is valid for buf.len() writes
        // and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_topology_edgeuse_p3_eval(
                op as i32,
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    eval_impl(op, path, input, None)
}

/// An immutable fixture exported only after this unit's guards accept its block.
///
/// Private fields prevent a weaker unit's export or caller-supplied counts from
/// constructing this proof. Each prepared operation still builds a fresh native body.
#[derive(Debug)]
pub struct PreparedBrep {
    fixture: p1::PreparedBrep,
}

impl PreparedBrep {
    /// Validate all fixture-block guards before performing the genuine native export.
    ///
    /// # Errors
    /// The unit's malformed-block refusals, or the native export's error.
    pub fn new(path: &str, block: &[f64]) -> Result<Self, FfiError> {
        Self::with_export(path, block, || p1::PreparedBrep::new(path, block))
    }

    fn with_export(
        path: &str,
        block: &[f64],
        export: impl FnOnce() -> Result<p1::PreparedBrep, FfiError>,
    ) -> Result<Self, FfiError> {
        let mut rd = Reader {
            input: block,
            at: 0,
        };
        brep_block(&mut rd, path)?;
        rd.done()?;
        // HANG-29: the export builds the B-rep, which never returns for such a value
        check_block_magnitudes(block)?;
        Ok(Self { fixture: export()? })
    }

    /// The immutable genuine native export, including a possible build-failure record.
    #[must_use]
    pub fn export(&self) -> &[f64] {
        self.fixture.export()
    }
}

/// Run an operation on a freshly rebuilt copy of a prepared fixture.
///
/// `tail` contains the edge-use index and operation arguments, without the B-rep
/// block. All ordinary block, magnitude, index and argument checks still run;
/// only the redundant validating export is reused. No mutable native body is kept.
///
/// # Errors
/// The same validation and native errors as [`eval`]. A changed file fixture whose
/// selected edge use no longer exists is refused by the native bounds check.
pub fn eval_prepared(op: Op, prepared: &PreparedBrep, tail: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut input = prepared.fixture.block().to_vec();
    input.extend_from_slice(tail);
    eval_impl(op, prepared.fixture.path(), &input, Some(prepared.export()))
}

fn eval_impl(
    op: Op,
    path: &str,
    input: &[f64],
    prepared_export: Option<&[f64]>,
) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    check_block_magnitudes(block)?;
    let fresh_export;
    let export = if let Some(export) = prepared_export {
        export
    } else {
        fresh_export = p1::eval(p1::Op::Export, path, block)?;
        &fresh_export
    };
    let Some(c) = p1::counts(export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export.to_vec());
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    if op != Op::ExportGaps {
        rd.below(n_eu)?;
    }
    args(op, &mut rd)?;
    rd.done()?;
    raw(op, path, input)
}

#[cfg(test)]
mod prepared_guard_tests {
    use super::*;

    #[test]
    fn magnitude_refusal_precedes_native_export() {
        let called = std::cell::Cell::new(false);
        // a point block p1's parser accepts, with a value in the HANG-29 regime; the
        // callback stands in for the native export and must not run
        let block = [4.0, 1.0, -1e300, 0.0, 0.0, 0.0, 0.0];
        let error = PreparedBrep::with_export("", &block, || {
            called.set(true);
            Err(bad("unexpected export callback"))
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid argument: B-rep value above 1e150 (HANG-29)"
        );
        assert!(!called.get());
    }
}
