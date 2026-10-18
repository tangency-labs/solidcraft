//! FFI wrappers for unit `nlib.geom`.
//!
//! Shim: `harness/cpp/shim/nlib_geom.cpp` (`sc_nlib_geom_eval`).
//! Upstream: `source/SMLib/src/NL_Geom.cpp`, `source/SMLib/src/NL_FrameBasic.cpp`.
//!
//! | [`Op`]          | upstream                                                     |
//! |-----------------|--------------------------------------------------------------|
//! | `VectorOps`     | `N_Vector*`, `N_DistPtPt`, `N_DistSqPtPt`, `N_PtsAreEqual`, `N_DistCptCpt`, `N_DistCptCptHomo`, `N_DistSignedPtPlane`, interval / rectangle / box / line / plane accessors |
//! | `IsectLineLine` | `N_IsectLineLine`                                            |
//! | `IsectLinePlane`| `N_IsectLinePlane`                                           |
//! | `ProjectPtLine` | `N_ProjectPtLine`                                            |
//! | `PlaneOps`      | `N_ProjectPtPlane`, `N_DistPtInfLine`, `N_PlanePtNormalToImplicit` |
//! | `DistSeg`       | `N_DistPtLineSeg`, `N_DistPerpPtLineSeg`                     |
//! | `Polygon`       | `N_EPolygonFromPts`, `N_EPolygonGetPts`, `N_PolygonGetBBox`, `N_DistPolygon` |
//! | `CPolygon`      | `N_CPolygonFromCPts`, `N_CPolygonGetCPts`, `N_DistCPolygon`  |
//! | `Net`           | `N_ENetFromPts`, `N_ENetGetPts`, `N_ENetGetBBox`, `N_CNetFromCPts` |
//! | `Rotation`      | `N_CreateRotationMatrixAboutAxis`                            |
//! | `Transform`     | `N_CreateTransformMatrixFromAxes`                            |
//! | `CptOps`        | `N_CptReflect`, `N_CPtsAreColinear`                          |
//!
//! Output records (flat doubles, upstream statuses inlined as `NL_ERROR.eno`) are
//! described in `crates/difftest/src/units/nlib_geom.rs`. Inputs are checked here
//! (lengths, at least one polygon point, a positive integral net shape that matches
//! the point count) so upstream only reads inside the arrays it is given.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest net side accepted by [`eval`].
pub const MAX_NET_SIDE: usize = 4096;

/// A group of upstream calls run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Vector arithmetic and structure accessors (16 inputs).
    VectorOps,
    /// `N_IsectLineLine` (14 inputs).
    IsectLineLine,
    /// `N_IsectLinePlane` (13 inputs).
    IsectLinePlane,
    /// `N_ProjectPtLine` (10 inputs).
    ProjectPtLine,
    /// Plane projections and implicit form (10 inputs).
    PlaneOps,
    /// Point to segment distances (9 inputs).
    DistSeg,
    /// Point polygon (3 per point, at least one point).
    Polygon,
    /// Control polygon (4 per point).
    CPolygon,
    /// Point net (`rows, cols`, then `3*rows*cols`).
    Net,
    /// Rotation matrix (7 inputs).
    Rotation,
    /// Frame-to-frame matrix (24 inputs).
    Transform,
    /// Control-point reflection and collinearity (18 inputs).
    CptOps,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::VectorOps => 0,
            Op::IsectLineLine => 1,
            Op::IsectLinePlane => 2,
            Op::ProjectPtLine => 3,
            Op::PlaneOps => 4,
            Op::DistSeg => 5,
            Op::Polygon => 6,
            Op::CPolygon => 7,
            Op::Net => 8,
            Op::Rotation => 9,
            Op::Transform => 10,
            Op::CptOps => 11,
        }
    }

    fn fixed_inputs(self) -> Option<usize> {
        match self {
            Op::VectorOps => Some(16),
            Op::IsectLineLine => Some(14),
            Op::IsectLinePlane => Some(13),
            Op::ProjectPtLine | Op::PlaneOps => Some(10),
            Op::DistSeg => Some(9),
            Op::Rotation => Some(7),
            Op::Transform => Some(24),
            Op::CptOps => Some(18),
            Op::Polygon | Op::CPolygon | Op::Net => None,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_geom_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    if let Some(n) = op.fixed_inputs() {
        if input.len() != n {
            return Err(bad(format!("{op:?}: {} inputs, expected {n}", input.len())));
        }
        return Ok(());
    }
    match op {
        Op::Polygon => {
            if input.len() < 3 || !input.len().is_multiple_of(3) {
                return Err(bad("polygon: need at least one xyz point"));
            }
        }
        Op::CPolygon => {
            if !input.len().is_multiple_of(4) {
                return Err(bad("cpolygon: length not a multiple of 4"));
            }
        }
        Op::Net => {
            let side = |k: usize| -> Result<usize, FfiError> {
                let v = input
                    .get(k)
                    .copied()
                    .ok_or_else(|| bad("net: missing shape"))?;
                if v.fract() != 0.0 || v < 1.0 || v > MAX_NET_SIDE as f64 {
                    return Err(bad("net: bad shape"));
                }
                Ok(v as usize)
            };
            let (rows, cols) = (side(0)?, side(1)?);
            if input.len() != 2 + 3 * rows * cols {
                return Err(bad("net: point count does not match shape"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: input is valid for input.len() reads and its length matches what
        // the shim reads for `op` (validated above); buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_geom_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
