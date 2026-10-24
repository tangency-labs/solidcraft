//! Native fillet-executive p3 oracle, with explicit constructor-state setup.
//!
//! UB-185: the native fixture initializes the upstream indeterminate
//! `m_bIsInitialized` member before every call; no uninitialized state is exposed.
//!
//! Input: `op target initialized prior_boolean editing want_error seed_code tol`
//! followed by the five executive flags (piecewise, insertion, global, trimming,
//! classification). `op` is 0 `DoFillet`, 1 `CreateFilletBrep`, 2 `DoFilleting`. Targets
//! are empty native B-reps with a raw tolerance-member fixture assignment. Output
//! contains status, error-info, flags and tolerances, then both comment lengths,
//! both upstream corner-type discriminants and the comment bytes; see the runner's
//! `output` function.

use crate::{call_with_buffer, reader::Cursor, FfiError};

extern "C" {
    fn sc_blend_fillet_executive_p3_eval(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Validate the fixed record before entering native code. Boolean and enum codes
/// are exact; tolerances are finite (the native constructor applies `fabs`).
/// Noncanonical Boolean encodings and NaN/infinite tolerances are rejected as
/// fixture-encoding choices, not as upstream UB guards. There are no input
/// allocations or recursive native paths for these empty B-rep fixtures.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut reader = Cursor::new(input);
    reader.index(2)?;
    for _ in 0..5 {
        reader.flag()?;
    }
    reader.index(7)?;
    let tolerance = reader.num()?;
    if !tolerance.is_finite() {
        return Err(FfiError::InvalidArgument("invalid tolerance".into()));
    }
    for _ in 0..5 {
        reader.flag()?;
    }
    reader.done()
}

/// Run one actual upstream p3 operation after validating the complete record.
pub fn eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    // The fixed fixture messages fit this capacity, so no operation is retried.
    call_with_buffer(512, |out, len| unsafe {
        sc_blend_fillet_executive_p3_eval(
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

extern "C" {
    fn sc_blend_fillet_executive_p3_upstream(
        example: i32,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Check the selector of the seven fixed upstream API examples (0 through 6).
pub fn validate_upstream_fixture(example: usize) -> Result<(), FfiError> {
    if example > 6 {
        return Err(FfiError::InvalidArgument(
            "unknown upstream fillet fixture".into(),
        ));
    }
    Ok(())
}

/// Execute the original `TestSmFillets.cpp` box example: 0 circular, 1 chamfer,
/// 2 first-four edges, 3 per-edge radii, 4 variable radius, 5/6 bevel false/true.
/// Results are operation status and final face/edge/vertex counts. Rust setup for
/// these API entrypoints belongs to executive p1, so they remain deferred and do
/// not establish geometry or topology parity for p3.
pub fn upstream_fixture(example: usize) -> Result<Vec<f64>, FfiError> {
    validate_upstream_fixture(example)?;
    call_with_buffer(4, |out, len| unsafe {
        sc_blend_fillet_executive_p3_upstream(
            example as i32,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

extern "C" {
    fn sc_blend_fillet_executive_p3_real_solid(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Validate the real-box fixture wire: XYZ dimensions, radius, cross-section,
/// continuity, thumbweight, selected-edge count, then owning-list edge indices.
/// Bounds describe the fixture's finite box and enum encoding, not a claim that
/// the public fillet API rejects other finite parameter values.
pub fn validate_real_solid(input: &[f64]) -> Result<(), FfiError> {
    let mut r = Cursor::new(input);
    for value in r.slice(3)? {
        if *value <= 1e-12 {
            return Err(FfiError::InvalidArgument(
                "non-positive box dimension".into(),
            ));
        }
    }
    r.slice(1)?;
    // Native ULONG is unsigned long (64-bit on this Linux reference). Reject
    // the rounded 2^64 endpoint before a floating-point-to-integer conversion.
    for _ in 0..2 {
        let code = r.num()?;
        if !code.is_finite() || code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0
        {
            return Err(FfiError::InvalidArgument("invalid native ULONG".into()));
        }
    }
    r.slice(1)?;
    let remaining = input.len().saturating_sub(8);
    let count = r.index(remaining)?;
    if count == 0 || count != remaining {
        return Err(FfiError::InvalidArgument(
            "invalid edge selection length".into(),
        ));
    }
    for _ in 0..count {
        r.index(11)?;
    }
    r.done()
}

/// Execute the real mutating public fillet API on a newly constructed box.
/// The record contains pre-call selected-edge samples, actual API status,
/// freshly traversed result geometry and actual precise-mass status/values.
pub fn real_solid(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_real_solid(input)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_solid(
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

extern "C" {
    fn sc_blend_fillet_executive_p3_real_prefix(
        input: *const f64,
        n: i64,
        checkpoint: i32,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Execute actual setup/corner checkpoints (1–5), or the real smooth prelude
/// followed by shared corner vertices and adjustment (6). Mode 7 adds the first
/// direct solver geometry operation and samples its actual surface and curves.
/// Mode 8 runs all solvers, then tangent adjustment, repeated vertex calculation
/// and boundary curves in each corner’s native order.
/// Mode 9 adds actual rail trimming and directed UV-curve observations before
/// classification, which remains enabled.
/// Mode 10 executes enabled rail classification and actual adjusted-corner repairs,
/// recording the attached classifications and returned corner identities.
/// Mode 11 allocates the actual intermediate body, runs all corner geometry then
/// all solver face construction, and records its directed topology and geometry.
/// These are retained component checkpoints, not completed public fillets.
/// Uninitialized corner status and uncomputed geometry are never read.
pub fn real_prefix(input: &[f64], checkpoint: u32) -> Result<Vec<f64>, FfiError> {
    validate_real_solid(input)?;
    if !(1..=13).contains(&checkpoint) {
        return Err(FfiError::InvalidArgument(
            "unknown fillet checkpoint".into(),
        ));
    }
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_prefix(
            input.as_ptr(),
            input.len() as i64,
            checkpoint as i32,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::validate;

    fn record() -> Vec<f64> {
        vec![
            0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1e-5, 0.0, 1.0, 0.0, 1.0, 1.0,
        ]
    }

    #[test]
    fn malformed_lengths_never_enter_native_code() {
        let values = record();
        for length in 0..13 {
            assert!(validate(values.get(..length).unwrap_or(&[])).is_err());
        }
        let mut values = values;
        values.push(0.0);
        assert!(validate(&values).is_err());
    }

    #[test]
    fn flag_and_enum_nonfinite_or_fractional_values_are_rejected() {
        for index in [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12] {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.5, 1e300] {
                let mut values = record();
                if let Some(slot) = values.get_mut(index) {
                    *slot = value;
                }
                assert!(validate(&values).is_err());
            }
        }
    }

    #[test]
    fn finite_tolerances_are_kept_without_sign_or_scale_cutoff() {
        for tolerance in [
            0.0,
            -0.0,
            -1e-300,
            -0.25,
            -1.0,
            -f64::MAX,
            9876543.21,
            1e-300,
            1e-9,
            1.0,
            1e100,
            f64::MAX,
        ] {
            let mut values = record();
            if let Some(slot) = values.get_mut(7) {
                *slot = tolerance;
            }
            assert!(validate(&values).is_ok());
        }
        for tolerance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut values = record();
            if let Some(slot) = values.get_mut(7) {
                *slot = tolerance;
            }
            assert!(validate(&values).is_err());
        }
    }
    #[test]
    fn upstream_selectors_reject_unknown_codes() {
        for example in 0..7 {
            assert!(super::validate_upstream_fixture(example).is_ok());
        }
        for example in [7, 8, usize::MAX] {
            assert!(super::validate_upstream_fixture(example).is_err());
        }
    }
}

#[cfg(test)]
mod real_solid_tests {
    use super::validate_real_solid;
    fn record() -> Vec<f64> {
        vec![10.0, 20.0, 30.0, 0.5, 1.0, 1.0, 1.0, 1.0, 0.0]
    }
    #[test]
    fn native_cross_section_defaults_and_g3_are_not_guarded_out() {
        let mut value = record();
        value[4] = 99.0;
        value[5] = 3.0;
        assert!(validate_real_solid(&value).is_ok());
        value[4] = 18446744073709549568.0;
        assert!(validate_real_solid(&value).is_ok());
        value[4] = 18446744073709551616.0;
        assert!(validate_real_solid(&value).is_err());
    }
    #[test]
    fn complete_edge_selection_is_validated_before_native_execution() {
        let mut value = record();
        value[7] = 2.0;
        assert!(validate_real_solid(&value).is_err());
        value.push(11.0);
        assert!(validate_real_solid(&value).is_ok());
        value[8] = 12.0;
        assert!(validate_real_solid(&value).is_err());
        value[8] = 0.5;
        assert!(validate_real_solid(&value).is_err());
    }
}
