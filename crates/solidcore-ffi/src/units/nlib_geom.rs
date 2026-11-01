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
//! the point count) so upstream only reads inside the arrays it is given. `CptOps`
//! also refuses control points whose 3D collinearity test would compare a distance
//! `N_DistPtLineSeg` never wrote (UB-31, [`colinear_3d_reads_unset_distance`]).

use crate::validate::{bad, NOW, NOZ};
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

/// `NL_MTOL` as `NL_Globals.cpp` initialises it.
const MTOL: f64 = 1.0e-7;

/// `N_CPtToPtEuclid`: the Euclidean point of `x y z w`, through the reciprocal weight.
fn cpt_to_euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    if w == NOW {
        [x, y, if z == NOZ { 0.0 } else { z }]
    } else {
        let r = 1.0 / w;
        [x * r, y * r, if z == NOZ { 0.0 } else { z * r }]
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `N_FloatOpIsBad(x, y, NL_DIVISION)`.
fn division_is_bad(mut x: f64, mut y: f64) -> bool {
    if y == 0.0 {
        return true;
    }
    if x == 0.0 {
        return false;
    }
    if x < 0.0 {
        x = -x;
    }
    if y < 0.0 {
        y = -y;
    }
    if y <= 1.0 && x < f64::MAX * y {
        return false;
    }
    if y >= 1.0 && x > f64::MIN_POSITIVE * y {
        return false;
    }
    true
}

/// Whether `N_CPtsAreColinear(Pw, Qw, Rw, NL_THREED)` compares an uninitialised
/// distance (UB-31). It calls `N_DistPtLineSeg(Q, P, R, &d)`, ignores its status and
/// tests `d`, which that routine leaves unwritten when the segment `PR` is not
/// shorter than `NL_MTOL` and either `Q` projects inside it and the distance
/// quotient `|PQ x PR| / |PR|` is bad (an overflowing segment: nightly fuzz00091,
/// fuzz00228), or neither end test holds (a NaN dot product). The steps are
/// upstream's, in its operation order.
pub fn colinear_3d_reads_unset_distance(pw: [f64; 4], qw: [f64; 4], rw: [f64; 4]) -> bool {
    let (p, q, r) = (cpt_to_euclid(pw), cpt_to_euclid(qw), cpt_to_euclid(rw));
    let dotab = dot(sub(p, r), sub(p, r)).sqrt();
    if dotab < MTOL {
        return false;
    }
    let (ba, pa, bp) = (sub(r, p), sub(q, p), sub(r, q));
    let dotap = dot(pa, ba);
    let dotbp = dot(bp, ba);
    if dotap >= 0.0 && dotbp >= 0.0 {
        let t = [
            pa[1] * ba[2] - pa[2] * ba[1],
            pa[2] * ba[0] - pa[0] * ba[2],
            pa[0] * ba[1] - pa[1] * ba[0],
        ];
        division_is_bad(dot(t, t).sqrt(), dotab)
    } else {
        !(dotap < 0.0 || dotbp < 0.0)
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    if let Some(n) = op.fixed_inputs() {
        if input.len() != n {
            return Err(bad(format!("{op:?}: {} inputs, expected {n}", input.len())));
        }
        if op == Op::CptOps {
            let cpt = |i: usize| -> [f64; 4] {
                std::array::from_fn(|j| input.get(4 * i + j).copied().unwrap_or(0.0))
            };
            if colinear_3d_reads_unset_distance(cpt(0), cpt(1), cpt(2)) {
                return Err(bad(
                    "cpt ops: N_DistPtLineSeg leaves the collinearity distance unset (UB-31)",
                ));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The control points of nightly fuzz00091 and fuzz00228 (run 41190675541): `PR`
    /// overflows, so `|PR|` and the cross product are infinite and the quotient is bad.
    #[test]
    fn overflowing_segments_leave_the_distance_unset() {
        let fuzz00091 = (
            [-4.124403313553504, 4.708448260921347, 0.0, NOW],
            [-6.150727254333686, 6.955579686355636, 2.4415251694577087, NOW],
            [-1e300, -5.8094379909720715, -11.42776240091433, NOW],
        );
        let fuzz00228 = (
            [-5.089222669522229, -0.6613716250442554, 4.0, NOW],
            [-2.1955459535204156, 8993.558357017024, 10.470035774101095, NOW],
            [-10.584408709953527, -17080.976675779522, -8.28680801497978, -1e-300],
        );
        for (p, q, r) in [fuzz00091, fuzz00228] {
            assert!(colinear_3d_reads_unset_distance(p, q, r));
            let mut input = [p, q, r].concat();
            input.extend([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
            let e = validate(Op::CptOps, &input).unwrap_err().to_string();
            assert!(e.contains("UB-31"), "{e}");
        }
    }

    /// Every branch that writes the distance is accepted: a segment shorter than
    /// `NL_MTOL`, `Q` before `P`, past `R` or between them, and a large finite segment.
    #[test]
    fn written_distances_are_accepted() {
        let w = |x: f64, y: f64, z: f64| [x, y, z, NOW];
        let cases = [
            (w(0.0, 0.0, 0.0), w(1.0, 1.0, 0.0), w(5e-8, 0.0, 0.0)),
            (w(0.0, 0.0, 0.0), w(-1.0, 1.0, 0.0), w(2.0, 0.0, 0.0)),
            (w(0.0, 0.0, 0.0), w(3.0, 1.0, 0.0), w(2.0, 0.0, 0.0)),
            (w(0.0, 0.0, 0.0), w(1.0, 1.0, 0.0), w(2.0, 0.0, 0.0)),
            (w(0.0, 0.0, 0.0), w(1.0, 0.0, NOZ), [4.0, 0.0, NOZ, 2.0]),
            (w(-1e150, 0.0, 0.0), w(0.0, 1.0, 0.0), w(1e150, 0.0, 0.0)),
        ];
        for (p, q, r) in cases {
            assert!(!colinear_3d_reads_unset_distance(p, q, r), "{p:?} {q:?} {r:?}");
        }
    }
}
