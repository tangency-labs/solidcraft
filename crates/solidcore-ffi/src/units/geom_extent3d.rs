//! FFI wrappers for unit `geom.extent3d`.
//!
//! Shim: `harness/cpp/shim/geom_extent3d.cpp`.
//! Upstream: `source/SMLib/src/SmExtent3d.cpp` (`SmExtent3d`).
//!
//! | Rust   | shim                  | upstream                         |
//! |--------|-----------------------|----------------------------------|
//! | [`op`] | `sc_geom_extent3d_op` | `SmExtent3d` methods, per [`Op`] |
//!
//! Every operation takes a flat `f64` input (extents are
//! `[min.x, min.y, min.z, max.x, max.y, max.z]`, built without swapping) and returns
//! a flat `f64` output; the layouts are documented on [`Op`] and in the shim.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_extent3d_op(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Operation selector of `sc_geom_extent3d_op` (`A`, `B`: extents of 6 values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmExtent3d(a, b)`: in `a, b` -> extent.
    FromPoints = 0,
    /// `SmExtent3d(p)`: in `p` -> extent.
    FromPoint = 1,
    /// `SmExtent3d()`: -> extent, `IsInit`, `HasNegativeVolume`.
    New = 2,
    /// `AddPoint3d` repeatedly: in `A, p0, p1, ..` -> extent.
    AddPoints = 3,
    /// `SetMinMax(min, max)`: in `A, min, max` -> status, extent.
    SetMinMax = 4,
    /// `SetMinMax(x6)`: in `A, 6 coords` -> status, extent.
    SetMinMaxCoords = 5,
    /// `SetUMin`/`SetUMax`/`SetVMin`/`SetVMax` (`which` 0..=3): in `A, which, v`
    /// -> status, extent.
    SetBound = 6,
    /// Accessors and predicates: in `A, tol` -> see the shim (34 values).
    Queries = 7,
    /// `Evaluate`: in `A, nx, ny, nz` -> point.
    Evaluate = 8,
    /// `Union`: in `A, B` -> extent.
    Union = 9,
    /// `Intersect`: in `A, B` -> status, extent (if ok).
    Intersect = 10,
    /// Max/min distance (squared): in `A, B` -> 4 values.
    Distances = 11,
    /// `ExpandRelative`: in `A, factor` -> extent.
    ExpandRelative = 12,
    /// `ExpandAbsolute`: in `A, amount` -> extent.
    ExpandAbsolute = 13,
    /// `GetCorners`, `GetEdges`, `GetPlanes`: in `A` -> 8 + 24 + 6 + 6 points.
    CornersEdgesPlanes = 14,
    /// `ComputeSphereBound`: in `A` -> centre, radius, min radius.
    SphereBound = 15,
    /// `ComputeConeBoundTo`: in `A, B` -> status, axis, angle (if ok).
    ConeBoundTo = 16,
    /// `AreDisjoint`, `IsContainedBy`: in `A, B, tol` -> 2 flags.
    DisjointContained = 17,
    /// `ContainsPoint3d`, `IsPoint3dOnBoundary`: in `A, p, tol` -> 2 flags, binormal.
    PointPredicates = 18,
    /// `ContainsLineSeg3d`: in `A, p, q, tol` -> flag.
    ContainsLineSeg = 19,
    /// `IntersectLine`: in `A, p, v` -> status, count, t enter, t exit (if ok;
    /// NaN where upstream sets nothing).
    IntersectLine = 20,
    /// `SetUnbounded`: in `A` -> extent.
    SetUnbounded = 21,
    /// `ApproximateUnbounded`: in `A, has_center, center, half_size` -> extent,
    /// expanded extent (`SmExtent3d()` if upstream rejected it).
    ApproximateUnbounded = 22,
    /// `SmExtent3d(SmPseudoBox)` (`Circumscribe`): in basis x, y, z, intervals
    /// x, y, z -> extent.
    Circumscribe = 23,
    /// `GetU/V/WInterval`, `SetUInterval`, `SetVInterval`: in `A, u ivl, v ivl`
    /// -> 6 interval ends, 2 statuses, extent.
    Intervals = 24,
}

impl Op {
    /// Number of input values (`None`: `AddPoints`, `6 + 3k`).
    #[must_use]
    pub fn input_size(self) -> Option<usize> {
        Some(match self {
            Op::AddPoints => return None,
            Op::New => 0,
            Op::FromPoint => 3,
            Op::FromPoints | Op::CornersEdgesPlanes | Op::SphereBound | Op::SetUnbounded => 6,
            Op::Queries | Op::ExpandRelative | Op::ExpandAbsolute => 7,
            Op::SetBound => 8,
            Op::Evaluate => 9,
            Op::PointPredicates | Op::Intervals => 10,
            Op::ApproximateUnbounded => 11,
            Op::SetMinMax
            | Op::SetMinMaxCoords
            | Op::Union
            | Op::Intersect
            | Op::Distances
            | Op::ConeBoundTo
            | Op::IntersectLine => 12,
            Op::DisjointContained | Op::ContainsLineSeg => 13,
            Op::Circumscribe => 15,
        })
    }
}

fn det3(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

fn len3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let bad = |m: String| Err(FfiError::InvalidArgument(m));
    match op.input_size() {
        Some(n) if input.len() != n => {
            return bad(format!("{op:?}: {} inputs, expected {n}", input.len()))
        }
        None if input.len() < 6 || !(input.len() - 6).is_multiple_of(3) => {
            return bad(format!("{op:?}: {} inputs, expected 6 + 3k", input.len()))
        }
        _ => {}
    }
    match op {
        Op::SetBound => {
            let w = input.get(6).copied().unwrap_or(f64::NAN);
            if ![0.0, 1.0, 2.0, 3.0].contains(&w) {
                return bad(format!("SetBound: which = {w}"));
            }
        }
        Op::Circumscribe => {
            // SmPseudoBox::SetBasis returns early (leaving members uninitialised) for a
            // near-singular basis: only pass clearly independent, finite bases.
            if input.iter().any(|v| !v.is_finite()) {
                return bad("Circumscribe: non-finite input".into());
            }
            let v = |i: usize| -> [f64; 3] {
                match input.get(i..i + 3) {
                    Some(&[x, y, z]) => [x, y, z],
                    _ => [0.0; 3],
                }
            };
            let (a, b, c) = (v(0), v(3), v(6));
            let scale = len3(a) * len3(b) * len3(c);
            if !(det3(a, b, c).abs() >= 1e-3 * scale && scale > 1e-6) {
                return bad("Circumscribe: basis not clearly independent".into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Runs `op` on `input` in upstream C++ and returns its flat output.
pub fn op(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    let n_in = i64::try_from(input.len()).map_err(|_| FfiError::InvalidArgument("input".into()))?;
    call_with_buffer(128, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads (the shim reads at most its
        // op's input size, checked above and again in the shim); `buf` is valid for
        // `buf.len()` writes; `len` is a valid writable i64.
        unsafe {
            sys::sc_geom_extent3d_op(
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
