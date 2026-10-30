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
    fn sc_blend_fillet_executive_p3_real_variable_solid(
        input: *const f64,
        n: i64,
        end_radius: f64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_real_law_solid(
        input: *const f64,
        n: i64,
        law: *const f64,
        law_n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_real_solid_setback(
        input: *const f64,
        n: i64,
        setback: *const f64,
        setback_n: i64,
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

/// Validate the unchanged real-box wire and a finite linear-law endpoint.
/// The variable-radius public API has no thumbweight argument.
pub fn validate_real_variable_solid(input: &[f64], end_radius: f64) -> Result<(), FfiError> {
    validate_real_solid(input)?;
    if !end_radius.is_finite() || input.get(6) != Some(&1.0) {
        return Err(FfiError::InvalidArgument(
            "variable radius requires a finite endpoint and default thumbweight".into(),
        ));
    }
    Ok(())
}

/// Execute the actual public variable-radius API with a linear radius law.
/// Uses the same complete result record as [`real_solid`].
pub fn real_variable_solid(input: &[f64], end_radius: f64) -> Result<Vec<f64>, FfiError> {
    validate_real_variable_solid(input, end_radius)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_variable_solid(
            input.as_ptr(),
            input.len() as i64,
            end_radius,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// Largest degree and control-point count of a fixture radius law.
const LAW_MAX_DEGREE: f64 = 3.0;
const LAW_MAX_POINTS: usize = 64;

/// Validate the unchanged real-box wire (default thumbweight) and a B-spline radius
/// law block: degree, radius count, the radii (control points `(r, 0, 0)`), unique
/// knot count, the strictly increasing knots, their multiplicities (`degree + 1` at
/// the ends, `1..=degree` inside, summing to radii + degree + 1), then the edge map's
/// min and max. Every value finite. These bounds define the fixture format: within
/// them `SmBSplineCurve::CreateCanonical` builds the curve on both sides.
pub fn validate_real_law_solid(input: &[f64], law: &[f64]) -> Result<(), FfiError> {
    validate_real_solid(input)?;
    let bad = |what: &str| FfiError::InvalidArgument(format!("radius law: {what}"));
    if input.get(6) != Some(&1.0) {
        return Err(bad("default thumbweight required"));
    }
    if law.iter().any(|value| !value.is_finite()) {
        return Err(bad("non-finite value"));
    }
    let mut r = Cursor::new(law);
    let degree = r.num()?;
    if degree.fract() != 0.0 || !(1.0..=LAW_MAX_DEGREE).contains(&degree) {
        return Err(bad("degree outside 1..=3"));
    }
    let degree = degree as usize;
    let points = r.index(LAW_MAX_POINTS)?;
    if points <= degree {
        return Err(bad("fewer radii than degree + 1"));
    }
    r.slice(points)?;
    let knots = r.index(LAW_MAX_POINTS)?;
    if knots < 2 {
        return Err(bad("fewer than two knots"));
    }
    let values = r.slice(knots)?;
    if values.iter().zip(values.iter().skip(1)).any(|(a, b)| a >= b) {
        return Err(bad("knots not strictly increasing"));
    }
    let mut total = 0;
    for i in 0..knots {
        let end = i == 0 || i + 1 == knots;
        let mult = r.index(degree + 1)?;
        if (end && mult != degree + 1) || (!end && !(1..=degree).contains(&mult)) {
            return Err(bad("invalid knot multiplicity"));
        }
        total += mult;
    }
    if total != points + degree + 1 {
        return Err(bad("multiplicities do not match the radius count"));
    }
    r.slice(2)?;
    r.done()
}

/// `SetFilletParameters(SM_FS_VARIABLE_RADIUS)` with an `SmBSplineFilletLaw` built
/// from the law block, then `DoFillet` and `StitchAndOrient`, as
/// `SmApiVariableRadiusFillet` does with its linear law. Uses the same complete
/// result record as [`real_solid`].
pub fn real_law_solid(input: &[f64], law: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_real_law_solid(input, law)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_law_solid(
            input.as_ptr(),
            input.len() as i64,
            law.as_ptr(),
            law.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// Validate the unchanged box wire and the separate XYZ/distance fixture block.
/// Finiteness defines this bridge's input format, not the native setter's policy;
/// zero and negative finite distances remain representable.
pub fn validate_real_solid_setback(input: &[f64], setback: [f64; 4]) -> Result<(), FfiError> {
    validate_real_solid(input)?;
    if setback.iter().any(|value| !value.is_finite()) {
        return Err(FfiError::InvalidArgument(
            "non-finite corner setback fixture".into(),
        ));
    }
    Ok(())
}

/// Run the actual executive with a setback assignment after corner creation.
/// The output is the complete [`real_solid`] record, including ordinary operation
/// errors. A missing unique vertex or prepared corner is a malformed fixture;
/// callers must report bridge errors as harness errors, not C++-undefined inputs.
pub fn real_solid_setback(input: &[f64], setback: [f64; 4]) -> Result<Vec<f64>, FfiError> {
    validate_real_solid_setback(input, setback)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_solid_setback(
            input.as_ptr(),
            input.len() as i64,
            setback.as_ptr(),
            4,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

extern "C" {
    fn sc_blend_fillet_executive_p3_fillet_then_shell(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// tangency-labs/solidcore#460 comparison: the [`real_solid`] wire followed by the shell
/// distance and an open-top flag (`1`: the plane faces at the box's height are
/// removed; `0`: a closed shell). The real box is filleted, then shelled with
/// `SmApiShellBrepFull` (extended convex edges, merged results, no offset solid). The
/// record is the fillet status, the shell status, the number of open faces, and for a
/// shell result its vertex/edge/face counts, the manifold query's status and flag,
/// and the mass status with area, volume and mass.
pub fn fillet_then_shell(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let n = input.len();
    if n < 11 {
        return Err(FfiError::InvalidArgument("fillet-then-shell wire too short".into()));
    }
    let (wire, shell) = input.split_at(n - 2);
    validate_real_solid(wire)?;
    let &[distance, open_top] = shell else {
        return Err(FfiError::InvalidArgument("fillet-then-shell wire too short".into()));
    };
    if !distance.is_finite() || (open_top != 0.0 && open_top != 1.0) {
        return Err(FfiError::InvalidArgument("invalid shell distance or open-top flag".into()));
    }
    call_with_buffer(4096, |out, len| unsafe {
        sc_blend_fillet_executive_p3_fillet_then_shell(
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
    if !(1..=19).contains(&checkpoint) {
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

extern "C" {
    fn sc_blend_fillet_executive_p3_real_source(
        path: *const std::ffi::c_char,
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_real_variable_source(
        path: *const std::ffi::c_char,
        input: *const f64,
        n: i64,
        end_radius: f64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_surface_surface(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_surface_surface_bodies(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Number of source parameters on the [`real_source`] wire.
pub const SOURCE_PARAMS: usize = 12;

/// Largest edge index a source wire may name (the shim rejects indices past the
/// built body's edge list; this only keeps the conversion exact).
const MAX_SOURCE_EDGE: usize = 1 << 20;

/// Validate the non-box real-solid wire: source kind (1 `.smb` file, 2 cylinder,
/// 3 box union, 4 box difference), [`SOURCE_PARAMS`] source parameters, then radius,
/// cross-section, continuity, thumbweight, selected-edge count (0 = every edge) and
/// owning-list edge indices. `has_path` says whether a file path accompanies it
/// (exactly kind 1). Cylinder radius/height and box sizes must be positive, as
/// `SmApiCreateCylinder` / `SmApiCreateBox` require.
pub fn validate_real_source(input: &[f64], has_path: bool) -> Result<(), FfiError> {
    if input.iter().any(|v| !v.is_finite()) {
        return Err(FfiError::InvalidArgument("non-finite source wire".into()));
    }
    let mut r = Cursor::new(input);
    let kind = r.index(4)?;
    if kind == 0 || (kind == 1) != has_path {
        return Err(FfiError::InvalidArgument("invalid solid source".into()));
    }
    let params = r.slice(SOURCE_PARAMS)?;
    let positive: &[usize] = match kind {
        2 => &[3, 4],
        3 | 4 => &[3, 4, 5, 9, 10, 11],
        _ => &[],
    };
    for &i in positive {
        if params.get(i).is_none_or(|v| *v <= 1e-12) {
            return Err(FfiError::InvalidArgument("non-positive source size".into()));
        }
    }
    r.slice(1)?;
    for _ in 0..2 {
        let code = r.num()?;
        if code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0 {
            return Err(FfiError::InvalidArgument("invalid native ULONG".into()));
        }
    }
    r.slice(1)?;
    let head = 1 + SOURCE_PARAMS + 5;
    let remaining = input.len().saturating_sub(head);
    let count = r.index(remaining)?;
    if count != remaining {
        return Err(FfiError::InvalidArgument(
            "invalid edge selection length".into(),
        ));
    }
    for _ in 0..count {
        r.index(MAX_SOURCE_EDGE)?;
    }
    r.done()
}

/// Execute the real mutating public fillet API on a non-box source body (see
/// [`validate_real_source`]); `path` is the `.smb` file of a file source. The record
/// is that of [`real_solid`].
pub fn real_source(input: &[f64], path: Option<&str>) -> Result<Vec<f64>, FfiError> {
    validate_real_source(input, path.is_some())?;
    let cpath = path
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    let ptr = cpath.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_source(
            ptr,
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// Validate the non-box real-solid wire ([`validate_real_source`]) with a finite
/// linear-law endpoint and the default thumbweight (the variable-radius public API
/// has no thumbweight argument).
pub fn validate_real_variable_source(
    input: &[f64],
    has_path: bool,
    end_radius: f64,
) -> Result<(), FfiError> {
    validate_real_source(input, has_path)?;
    if !end_radius.is_finite() || input.get(1 + SOURCE_PARAMS + 3) != Some(&1.0) {
        return Err(FfiError::InvalidArgument(
            "variable radius requires a finite endpoint and default thumbweight".into(),
        ));
    }
    Ok(())
}

/// Execute the actual public variable-radius API with a linear radius law on a
/// non-box source body (see [`validate_real_variable_source`]). The record is that of
/// [`real_solid`].
pub fn real_variable_source(
    input: &[f64],
    path: Option<&str>,
    end_radius: f64,
) -> Result<Vec<f64>, FfiError> {
    validate_real_variable_source(input, path.is_some(), end_radius)?;
    let cpath = path
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    let ptr = cpath.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_real_variable_source(
            ptr,
            input.as_ptr(),
            input.len() as i64,
            end_radius,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

extern "C" {
    fn sc_blend_fillet_executive_p3_piecewise_solid(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_piecewise_source(
        path: *const std::ffi::c_char,
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// [`real_solid`] with the executive's piecewise merge selected: `SmApiFilletEdges`'
/// executive set-up, then `SetDoPiecewiseMerge(TRUE)` after `SetFilletParameters`,
/// `DoFillet` and `StitchAndOrient`. Same wire and record.
pub fn piecewise_solid(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_real_solid(input)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_piecewise_solid(
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// [`real_source`] with the executive's piecewise merge selected (see
/// [`piecewise_solid`]). Same wire and record.
pub fn piecewise_source(input: &[f64], path: Option<&str>) -> Result<Vec<f64>, FfiError> {
    validate_real_source(input, path.is_some())?;
    let cpath = path
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    let ptr = cpath.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_piecewise_source(
            ptr,
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

extern "C" {
    fn sc_blend_fillet_executive_p3_curve_sih_solid(
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
    fn sc_blend_fillet_executive_p3_curve_sih_source(
        path: *const std::ffi::c_char,
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Validate an `SmMakeCurveBlendSIH` step-back factor (finite and positive).
fn validate_step_back(step_back: f64) -> Result<(), FfiError> {
    if !step_back.is_finite() || step_back <= 0.0 {
        return Err(FfiError::InvalidArgument("invalid step-back factor".into()));
    }
    Ok(())
}

/// [`real_solid`] with an `SmMakeCurveBlendSIH(step_back)` self-intersection handler
/// installed on `SmApiFilletEdges`' executive (`SetSelfIntersectionHandler`), so
/// every `FlushCurve` runs `SmMakeCurveBlendSIH::SmoothOutIntersection`. Same wire
/// and record.
pub fn curve_sih_solid(step_back: f64, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_step_back(step_back)?;
    validate_real_solid(input)?;
    let mut wire = vec![step_back];
    wire.extend_from_slice(input);
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_curve_sih_solid(
            wire.as_ptr(),
            wire.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// [`real_source`] with an `SmMakeCurveBlendSIH(step_back)` handler installed (see
/// [`curve_sih_solid`]). Same wire and record.
pub fn curve_sih_source(
    step_back: f64,
    input: &[f64],
    path: Option<&str>,
) -> Result<Vec<f64>, FfiError> {
    validate_step_back(step_back)?;
    validate_real_source(input, path.is_some())?;
    let mut wire = vec![step_back];
    wire.extend_from_slice(input);
    let cpath = path
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    let ptr = cpath.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_curve_sih_source(
            ptr,
            wire.as_ptr(),
            wire.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// Validate the surface-surface wire: two surfaces' four corner points (24 values),
/// radius1, radius2, tolerance, cross-section (native ULONG), mirror and complement
/// (0/1).
pub fn validate_surface_surface(input: &[f64]) -> Result<(), FfiError> {
    if input.len() != 30 || input.iter().any(|v| !v.is_finite()) {
        return Err(FfiError::InvalidArgument(
            "invalid surface-surface wire".into(),
        ));
    }
    let code = input.get(27).copied().unwrap_or(-1.0);
    if code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0 {
        return Err(FfiError::InvalidArgument("invalid native ULONG".into()));
    }
    if input
        .get(28..30)
        .is_none_or(|f| f.iter().any(|v| *v != 0.0 && *v != 1.0))
    {
        return Err(FfiError::InvalidArgument("invalid flag".into()));
    }
    Ok(())
}

/// `SmApiSurfaceSurfaceFillet` on two `SmApiCreateSurfaceFromCornerPoints` surfaces,
/// as `TestSmSurfaceSurfaceFillet` calls it: `[status]`, then for a result B-rep its
/// vertex/edge/face counts and the topology and geometry sections of [`real_solid`].
pub fn surface_surface(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_surface_surface(input)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_surface_surface(
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

/// Validate the owned-surface surface-surface wire: two surfaces' four corner points
/// (24 values), owner1 (0 free, 1 own sheet body), owner2 (0 free, 1 own sheet body,
/// 2 a second face of surface 1's body, only with owner1 1, 3 surface 1 itself),
/// radius1, radius2, tolerance, cross-section (native ULONG), mirror and complement
/// (0/1).
pub fn validate_surface_surface_bodies(input: &[f64]) -> Result<(), FfiError> {
    if input.len() != 32 || input.iter().any(|v| !v.is_finite()) {
        return Err(FfiError::InvalidArgument(
            "invalid surface-surface bodies wire".into(),
        ));
    }
    let owner1 = input.get(24).copied().unwrap_or(-1.0);
    let owner2 = input.get(25).copied().unwrap_or(-1.0);
    if (owner1 != 0.0 && owner1 != 1.0)
        || ![0.0, 1.0, 2.0, 3.0].contains(&owner2)
        || (owner2 == 2.0 && owner1 != 1.0)
    {
        return Err(FfiError::InvalidArgument("invalid surface owner".into()));
    }
    let code = input.get(29).copied().unwrap_or(-1.0);
    if code < 0.0 || code.fract() != 0.0 || code >= 18446744073709551616.0 {
        return Err(FfiError::InvalidArgument("invalid native ULONG".into()));
    }
    if input
        .get(30..32)
        .is_none_or(|f| f.iter().any(|v| *v != 0.0 && *v != 1.0))
    {
        return Err(FfiError::InvalidArgument("invalid flag".into()));
    }
    Ok(())
}

/// `SmApiSurfaceSurfaceFillet` where either surface may already belong to a face of a
/// sheet body (see [`validate_surface_surface_bodies`]): `[status, has_result]`, the
/// result B-rep's counts, topology and geometry sections when present, the sheet body
/// count, then each sheet body's counts, topology and geometry after the call.
pub fn surface_surface_bodies(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_surface_surface_bodies(input)?;
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_surface_surface_bodies(
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}

#[cfg(test)]
mod source_tests {
    use super::{validate_real_source, validate_surface_surface, SOURCE_PARAMS};
    fn cylinder() -> Vec<f64> {
        let mut v = vec![2.0];
        v.extend([0.0, 0.0, 0.0, 5.0, 10.0]);
        v.resize(1 + SOURCE_PARAMS, 0.0);
        v.extend([0.5, 1.0, 1.0, 1.0, 0.0]);
        v
    }
    #[test]
    fn curve_sih_rejects_invalid_step_back() {
        for step_back in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(super::curve_sih_source(step_back, &cylinder(), None).is_err());
            assert!(super::curve_sih_solid(
                step_back,
                &[10.0, 10.0, 10.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0]
            )
            .is_err());
        }
    }
    #[test]
    fn sources_and_paths_are_paired() {
        assert!(validate_real_source(&cylinder(), false).is_ok());
        assert!(validate_real_source(&cylinder(), true).is_err());
        let mut file = cylinder();
        file[0] = 1.0;
        assert!(validate_real_source(&file, true).is_ok());
        assert!(validate_real_source(&file, false).is_err());
        file[0] = 5.0;
        assert!(validate_real_source(&file, true).is_err());
    }
    #[test]
    fn sizes_and_selection_are_checked() {
        let mut v = cylinder();
        v[4] = 0.0;
        assert!(validate_real_source(&v, false).is_err());
        let mut v = cylinder();
        let n = v.len();
        v[n - 1] = 1.0;
        assert!(validate_real_source(&v, false).is_err());
        v.push(2.0);
        assert!(validate_real_source(&v, false).is_ok());
        v.push(2.5);
        assert!(validate_real_source(&v, false).is_err());
    }
    #[test]
    fn surface_surface_wire() {
        let mut v = vec![0.0; 30];
        assert!(validate_surface_surface(&v).is_ok());
        v[28] = 2.0;
        assert!(validate_surface_surface(&v).is_err());
        v[28] = 1.0;
        v[27] = 0.5;
        assert!(validate_surface_surface(&v).is_err());
        assert!(validate_surface_surface(&v[..29]).is_err());
    }
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
    fn setback_block_keeps_the_complete_legacy_wire_and_finite_distance_domain() {
        let mut value = record();
        for distance in [-8.0, 0.0, 8.0] {
            assert!(
                super::validate_real_solid_setback(&value, [60.0, 40.0, 20.0, distance]).is_ok()
            );
        }
        for index in 0..4 {
            let mut block = [60.0, 40.0, 20.0, 8.0];
            block[index] = f64::NAN;
            assert!(super::validate_real_solid_setback(&value, block).is_err());
        }
        value[7] = 2.0;
        assert!(super::validate_real_solid_setback(&value, [60.0, 40.0, 20.0, 8.0]).is_err());
    }
    #[test]
    fn native_setback_block_count_is_checked_before_reading_it() {
        let value = record();
        let block = [60.0, 40.0, 20.0, 8.0];
        let mut out = [0.0; 1];
        let mut len = 0;
        for count in [-1, 0, 3, 5] {
            let status = unsafe {
                super::sc_blend_fillet_executive_p3_real_solid_setback(
                    value.as_ptr(),
                    value.len() as i64,
                    block.as_ptr(),
                    count,
                    out.as_mut_ptr(),
                    1,
                    &raw mut len,
                )
            };
            assert_eq!(status, crate::SC_ERR_ARG);
        }
    }
    #[test]
    fn variable_endpoint_is_separate_and_complete_wire_still_checked() {
        let mut value = record();
        for end in [-2.0, 0.0, 2.0] {
            assert!(super::validate_real_variable_solid(&value, end).is_ok());
        }
        for end in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(super::validate_real_variable_solid(&value, end).is_err());
        }
        value[6] = 0.5;
        assert!(super::validate_real_variable_solid(&value, 2.0).is_err());
        value[6] = 1.0;
        value[7] = 2.0;
        assert!(super::validate_real_variable_solid(&value, 2.0).is_err());
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

extern "C" {
    fn sc_blend_fillet_executive_p3_fillet_preview(
        path: *const std::ffi::c_char,
        input: *const f64,
        n: i64,
        out: *mut f64,
        cap: i64,
        len: *mut i64,
    ) -> i32;
}

/// Validate the [`fillet_preview`] wire: source kind (0 a box at the origin, its size in
/// the first three parameters; 1 to 4 those of [`validate_real_source`]),
/// [`SOURCE_PARAMS`] source parameters, the radius, the selected-edge count and the
/// owning-list edge indices. Every value is finite; box, cylinder and source box sizes
/// are positive, as the constructors require. The radius is not limited:
/// `SmApiFilletPreview` does not check it.
pub fn validate_fillet_preview(input: &[f64], has_path: bool) -> Result<(), FfiError> {
    if input.iter().any(|v| !v.is_finite()) {
        return Err(FfiError::InvalidArgument("non-finite preview wire".into()));
    }
    let mut r = Cursor::new(input);
    let kind = r.index(4)?;
    if (kind == 1) != has_path {
        return Err(FfiError::InvalidArgument("invalid solid source".into()));
    }
    let params = r.slice(SOURCE_PARAMS)?;
    let positive: &[usize] = match kind {
        0 => &[0, 1, 2],
        2 => &[3, 4],
        3 | 4 => &[3, 4, 5, 9, 10, 11],
        _ => &[],
    };
    for &i in positive {
        if params.get(i).is_none_or(|v| *v <= 1e-12) {
            return Err(FfiError::InvalidArgument("non-positive source size".into()));
        }
    }
    r.slice(1)?;
    let head = 1 + SOURCE_PARAMS + 2;
    let remaining = input.len().saturating_sub(head);
    let count = r.index(remaining)?;
    if count != remaining {
        return Err(FfiError::InvalidArgument(
            "invalid edge selection length".into(),
        ));
    }
    for _ in 0..count {
        r.index(MAX_SOURCE_EDGE)?;
    }
    r.done()
}

/// `SmApiFilletPreview` on a source body (see [`validate_fillet_preview`]); `path` is the
/// `.smb` file of a file source. The record is the API status, the body's vertex, edge
/// and face counts after the call, the number of preview surfaces and per surface `has`
/// then its `GetType`, natural UV domain (`umin vmin umax vmax`) and a 5 x 5 grid over
/// it of `EvaluatePoint` `status [x y z]`.
pub fn fillet_preview(input: &[f64], path: Option<&str>) -> Result<Vec<f64>, FfiError> {
    validate_fillet_preview(input, path.is_some())?;
    let cpath = path
        .map(std::ffi::CString::new)
        .transpose()
        .map_err(|_| FfiError::InvalidArgument("path has a NUL byte".into()))?;
    let ptr = cpath.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    call_with_buffer(65536, |out, len| unsafe {
        sc_blend_fillet_executive_p3_fillet_preview(
            ptr,
            input.as_ptr(),
            input.len() as i64,
            out.as_mut_ptr(),
            out.len() as i64,
            len,
        )
    })
}
