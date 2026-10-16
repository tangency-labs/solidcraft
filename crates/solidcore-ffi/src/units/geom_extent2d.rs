//! FFI wrappers for unit `geom.extent2d`.
//!
//! Shim: `harness/cpp/shim/geom_extent2d.cpp`.
//! Upstream: `source/SMLib/src/SmExtent2d.cpp` (`SmExtent2d`).
//!
//! | Rust   | shim                  | upstream                         |
//! |--------|-----------------------|----------------------------------|
//! | [`op`] | `sc_geom_extent2d_op` | `SmExtent2d` methods, per [`Op`] |
//!
//! Every operation takes a flat `f64` input (extents are
//! `[min.x, min.y, max.x, max.y]`, built without swapping) and returns a flat `f64`
//! output; the layouts are documented on [`Op`] and in the shim.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_extent2d_op(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Operation selector of `sc_geom_extent2d_op` (`A`, `B`: extents of 4 values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmExtent2d(a, b)`: in `a, b` -> extent.
    FromPoints = 0,
    /// `SmExtent2d(p)`: in `p` -> extent.
    FromPoint = 1,
    /// `SmExtent2d()`: -> extent, `IsInit`, `HasNegativeArea`.
    New = 2,
    /// `SmExtent2d(const SmExtent3d&)`: in 6 bounds -> extent.
    FromExtent3d = 3,
    /// `AddPoint2d` repeatedly: in `A, p0, p1, ..` -> extent.
    AddPoints = 4,
    /// `SetMinMax(min, max)`: in `A, min, max` -> status, extent.
    SetMinMax = 5,
    /// `SetMinMax(x4)`: in `A, 4 coords` -> status, extent.
    SetMinMaxCoords = 6,
    /// Accessors and predicates: in `A, tol` -> see the shim (22 values).
    Queries = 7,
    /// `Evaluate`: in `A, nx, ny` -> point.
    Evaluate = 8,
    /// `Union`: in `A, B` -> extent.
    Union = 9,
    /// `Intersect`: in `A, B` -> status, extent (if ok).
    Intersect = 10,
    /// `AreDisjoint`, `AreEqual`, `IsContainedBy`, `operator==`,
    /// `GetMaxBoundaryDist2d`: in `A, B, tol` -> 4 flags, distance.
    Compare = 11,
    /// `ExpandRelative`: in `A, factor` -> extent.
    ExpandRelative = 12,
    /// `ExpandAbsolute(double)`: in `A, d` -> extent.
    ExpandAbsolute = 13,
    /// `ExpandAbsolute(SmVector2d)`: in `A, dx, dy` -> extent.
    ExpandAbsoluteVec = 14,
    /// `Transpose`: in `A` -> extent.
    Transpose = 15,
    /// Point predicates: in `A, p, tol, has_tol_v, tol_v` -> see the shim (16 values).
    PointOps = 16,
    /// `ContainsPeriodicPoint2d`: in `A, p, u period, v period, tol` -> flag,
    /// periodic point, flag (without the point output).
    ContainsPeriodic = 17,
    /// `Inversion`: in `A, p, tol` -> status, normalised point (if ok).
    Inversion = 18,
    /// `ClipLine2d`: in `A, p, v, t` -> parameter.
    ClipLine = 19,
    /// `IntersectWithInfiniteLine`: in `A, p, v, has_tol_u, tol_u, has_tol_v, tol_v`
    /// -> status, found, interval (if found).
    IntersectLine = 20,
    /// `Point2dMapToDomain`: in `A, p, B, switch` -> status, point (if ok).
    MapToDomain = 21,
    /// `GetUInterval`, `GetVInterval`, `SetUInterval`, `SetVInterval`: in
    /// `A, u ivl, v ivl` -> 4 interval ends, 2 statuses, extent.
    Intervals = 22,
    /// `SmTol::GetScaledZero(const SmExtent2d&)`: in `A` -> value.
    ScaledZero = 23,
}

impl Op {
    /// Number of input values (`None`: `AddPoints`, `4 + 2k`).
    #[must_use]
    pub fn input_size(self) -> Option<usize> {
        Some(match self {
            Op::AddPoints => return None,
            Op::New => 0,
            Op::FromPoint => 2,
            Op::FromPoints | Op::Transpose | Op::ScaledZero => 4,
            Op::Queries | Op::ExpandRelative | Op::ExpandAbsolute => 5,
            Op::FromExtent3d | Op::Evaluate | Op::ExpandAbsoluteVec => 6,
            Op::Inversion => 7,
            Op::SetMinMax | Op::SetMinMaxCoords | Op::Union | Op::Intersect | Op::Intervals => 8,
            Op::Compare | Op::PointOps | Op::ContainsPeriodic | Op::ClipLine => 9,
            Op::MapToDomain => 11,
            Op::IntersectLine => 12,
        })
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    match op.input_size() {
        Some(n) if input.len() != n => Err(FfiError::InvalidArgument(format!(
            "{op:?}: {} inputs, expected {n}",
            input.len()
        ))),
        None if input.len() < 4 || !(input.len() - 4).is_multiple_of(2) => Err(
            FfiError::InvalidArgument(format!("{op:?}: {} inputs, expected 4 + 2k", input.len())),
        ),
        _ => Ok(()),
    }
}

/// Runs `op` on `input` in upstream C++ and returns its flat output.
pub fn op(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    let n_in = i64::try_from(input.len()).map_err(|_| FfiError::InvalidArgument("input".into()))?;
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads (the shim reads at most its
        // op's input size, checked above and again in the shim); `buf` is valid for
        // `buf.len()` writes; `len` is a valid writable i64.
        unsafe {
            sys::sc_geom_extent2d_op(
                op as i32,
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
