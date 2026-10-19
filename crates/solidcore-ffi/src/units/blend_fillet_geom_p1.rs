//! FFI wrappers for unit `blend.fillet_geom_p1`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_geom_p1.cpp` (`sc_blend_fillet_geom_p1_eval`).
//! Upstream: `source/SMLib/src/SmFilletGeom.cpp`, `source/SMLib/inc/SmFilletGeom.h`.
//!
//! A surface is `n m p q r s`, `(n+1)(m+1)` control points `x y z w` (v index fastest),
//! `r+1` u knots, `s+1` v knots (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`). A curve is
//! `dim n p m`, `n+1` control points `x y z w`, `m+1` knots (`SmBSplineCurve(gw_CURVE*,
//! dim, FALSE)`, `dim` 2 or 3). A point is `x y z`, a UV point `u v`. Enum values are
//! upstream's.
//!
//! Geometry ops (`op < 20`):
//!
//! | [`Op`]            | upstream                          | parameters                                   | outputs (after the status) |
//! |-------------------|-----------------------------------|----------------------------------------------|---------------------------|
//! | `CleanupNoise`    | `sm_CleanupNoise`                 | `u v`                                        | `u v` |
//! | `CheckSeamOut`, `CheckSeamOn` | `sm_CheckSeamOut`, `sm_CheckSeamOn` | `surface tol closure u v`        | `ret u v` |
//! | `SurfaceClosure`  | `sm_SetSurfaceClosure`            | `surface umin vmin umax vmax`                | `closure` |
//! | `IsOrderCorrect`  | `sm_IsOrderCorrect`               | `n` then `n` x `pos[3] deriv[3]`, `i1 i2`     | `ret` |
//! | `AdjustUvOnSeam`  | `sm_AdjustUVOnSeam`               | `surface ref[3] u v`                         | `u v` |
//! | `Interpolate`     | `sm_InterpolateEndptsAndTangents` | `start[3] end[3] svec[3] evec[3] dim`        | `svec[3] evec[3]`, then the curve (`has ...`) |
//! | `SurfaceDropPoint`| `sm_SurfaceDropPoint`             | `surface pnt[3] tol`                         | `n` then `n` x `u v value` |
//! | `TwoPnts`         | `sm_TwoPntsIntersection`          | `surface1 surface2 start[3] has_end end[3] dir[3] has_end_dir end_dir[3] tol` | three curves (`has ...`), `dir[3]` as left by the call |
//! | `EvaluateMate`    | `sm_EvaluateMate`                 | `surface u v` (a face on the surface)        | `pnt[3]` |
//! | `Capping`         | `JoinVertsByCapping`              | `tol surface uvcurve1 orient1 uvcurve2 orient2` | `n` then `n` x `uvcurve curve3d orient` |
//! | `ClassifyFacePoint` | `sm_ClassifyFacePoint`          | `surface1 surface2 pos[3]` (two faces)       | `which` |
//! | `EvaluateMateAnalytic` | `sm_EvaluateMate` on a real `SmPlane` / `SmSurfOfRevolution` | `0 O[3] X[3] Y[3] scale[2] umin vmin umax vmax make_nurb u v` (plane) or `1 a[3] b[3] axis_pnt[3] axis_dir[3] swap u v` (`CreateCanonical` of the line `a b`) | `construction_status`, then the evaluation's status and `pnt[3]` |
//!
//! Pseudo-B-rep ops (`op >= 20`) start with the block (the shim builds an `SmFilletBrep`
//! per call): `nt` target wire-edge curves (3D), `nf` target face surfaces, `nfb` fillet
//! B-rep wire-edge curves (3D) each followed by the edge tolerance `tol`; `nv` vertices `x y z type status corner pc_class pc_kind
//! pc_index pc_sub pc_face t_param u v` (`pc_kind` 0 none, 1 the start/end (`pc_sub`)
//! vertex of target wire edge `pc_index`, 2 its primary edge use, 3 target face
//! `pc_index`; `pc_face` a target face or -1; `corner` an opaque tag or -1); `ng`
//! geometries `type status has_center [curve] has_surface [surface]`; `ne` edges `vs ve
//! type status corner has_curve [curve] has_ivl t0 t1 fbe1 fbe2`, then per edge use
//! (primary, mate) `has_uv [uvcurve] geom`, then per edge use its vertex use's `u v`;
//! then per geometry `rail0 rail1 n1 edges.. n2 edges.. ns (edge eu)..`; then per vertex
//! `mate0 mate1`. Vertices are indexed in creation order, edges in edge-list order,
//! geometries in creation order; a vertex use or edge use is `(edge, 0 primary / 1 mate)`.
//!
//! | [`Op`]           | upstream                                | parameters after the block | outputs (after the status) |
//! |------------------|-----------------------------------------|----------------------------|---------------------------|
//! | `FbExport`       | the raw state                           |                            | the export (below) |
//! | `FbVertexGeoms`  | `SmFilletVertex::GetFilletGeoms`        | `v`                        | `n geoms..` |
//! | `FbVuAtRailEnd`  | `SmFilletVertex::GetVUAtRailEnd`        | `v geom excl_edge excl_eu rail` (-1 to ignore) | `found edge eu` |
//! | `FbRailVertexuse`| `SmFilletGeom::GetRailVertexuse`        | `g rail end`               | `found edge eu` |
//! | `FbEdgeName`     | `SmFilletEdge::GetName`                 | `e`                        | `code` (-1 untouched) |
//! | `FbFindEdge`     | `SmFilletEdge::FindFilletBrepEdge`      | `e multi n cands..`        | `fbe1 fbe2` |
//! | `FbClearEdges`   | `ClearFilletBrepEdges` + `GetFilletBrepEdges` | `e n edges..`        | `n edges.. fbe1` |
//! | `FbBrepEdges`    | `GetFilletBrepEdges`, `GetFilletBrepEdge` | `e`                      | `n edges.. fbe1` |
//! | `FbTrim`         | `SmFilletGeom::Trim`                    | `g t0 t1`                  | `t0 t1`, the export |
//! | `FbDeleteVertex` | `SmFilletVertex::~SmFilletVertex`       | `v` (no edges on it)       | the export |
//! | `FbDeleteEdge`   | `SmFilletEdge::~SmFilletEdge`           | `e`                        | the export |
//! | `FbVertOnVert`   | `SmFilletVertex::CalcVertOnVert`        | `v` (classified to a vertex) | `pnt[3] status` |
//! | `FbCliff`        | `SmFilletVertex::CalcCliffRailIntEdge`  | `v surface tol` (classified to an edge use) | `pnt[3] status` |
//! | `FbSplitFace`    | `SmFilletVertex::UpdateSplitFace`       | `v f1 f2`                  | `pc_class pc_kind pc_index pc_sub pc_face` |
//! | `FbExamine`      | `sm_ExamineNFixFilletGeoms`             | `g1 at1 g2 at2 sv ev tol`  | the export |
//! | `FbSetEuGeom`    | `SmFilletEdgeuse::SetFilletGeom`        | `e eu g`                   | the export |
//! | `FbSetUvCurve`   | `SmFilletEdgeuse::SetUVCurve`           | `e eu has [uvcurve] delete warn` | the export |
//! | `FbOrigUvCurve`  | `SmFilletEdge::GetOriginalUVCurve`      | `e`                        | the curve |
//! | `FbSetVuProperty`| `SmFilletVertexuse::SetProperty`        | `e eu te teu`              | the export |
//! | `FbRemoveEdge`   | `SmFilletBrep::RemoveEdge` (+ `AddEdge`)| `e readd`                  | the export |
//!
//! The export: `nv` then per vertex `x y z type status mate0 mate1 corner pc_class pc_kind
//! pc_index pc_sub pc_face t_param u v nvu` and per vertex use `edge eu u v`; `ne` then per
//! edge `vs ve type status corner fbe1 fbe2 curve t0 t1 neu` and per edge use `orient
//! vertex uvcurve geom`; `ng` then per geometry `type status rail0 rail1 n1 edges.. n2
//! edges.. center surface nref (edge eu).. nside (edge eu)..`. Curves and surfaces carry a
//! leading `has`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped B-splines (`1 <= p <= n`, at most [`MAX_INDEX`] control points per direction,
//! degrees at most [`MAX_DEGREE`], finite data, consistent rationality, non-decreasing
//! knots with end multiplicity `degree + 1` and interior multiplicity at most the degree),
//! 3D edge, centre-line and target curves and 2D UV curves, enum values in exactly their
//! upstream range (every code is refused rather than defaulted on either side), indices
//! inside the block, at most [`MAX_LIST`] objects per list, a curve on an edge whose
//! fillet B-rep edge is looked for, both rails on a trimmed or rail-queried geometry, a
//! vertex-classified vertex for `FbVertOnVert`, an edge-use-classified one for `FbCliff`,
//! a deleted vertex without edges (its edge uses would keep dangling vertex-use pointers
//! the export reads), finite tolerances and parameters. One refusal for UB-166 lives in
//! the shim, which reads back the knot vector the `SmBSplineSurface` constructor really
//! left (its `sm_FixupKnotVector` snaps knots within the scaled zero and spreads
//! distinct knots closer than `SM_EFF_ZERO_PARAM`, which can land a knot on the start
//! knot): for the six ops that build a surface cache (`SurfaceDropPoint`, `TwoPnts`,
//! `ClassifyFacePoint`, `FbVertOnVert`, `FbCliff`, `FbSplitFace`) it returns `SC_ERR_ARG`
//! for a surface whose constructed v knot vector has start multiplicity above
//! `degree + 1`, because `sm_DecomposeSrf` writes past the cache's v-patch array for it.
//! The u direction fails cleanly there (upstream's B262 guard), non-monotone constructed
//! vectors run (defined behaviour), and the other ops never decompose, so all of those
//! are compared. The refusal is the crash precondition, not an exact crash predicate:
//! some paths return before building the cache for the snapped surface and would
//! complete (the round-5 review measured 28 of 169 such programs, `TwoPnts` returning
//! 1001 and `FbVertOnVert`/`FbCliff`/`FbSplitFace` exits ahead of the cache fetch);
//! telling them apart would mean replicating upstream's control flow. For
//! `ClassifyFacePoint` and `FbSplitFace`, the
//! ops that reach `sm_ClassifyFacePoint`, every value (control points as Euclidean
//! `Pw / w`) is bounded by [`MAX_MAGNITUDE`], the overflow regime of HANG-29; inputs
//! under it are accepted even though those from about `1e15` can take seconds to about
//! a minute. For `SurfaceDropPoint`, `TwoPnts`, `FbVertOnVert` and `FbCliff`, whose
//! global solves never finish once a coordinate's own box arithmetic overflows
//! (HANG-20), every value is bounded by [`MAX_SOLVER_MAGNITUDE`]. Inverted intervals
//! are accepted (upstream `SetInterval` and `Trim` take them). Another refusal lives in the
//! shim: an `FbExamine` whose adjusting path would be reached returns `SC_ERR_ARG`,
//! because the shim's geometries have no `SmFilletSolver` and upstream dereferences it
//! there (the shim repeats upstream's own degree, kind and end-distance checks to
//! decide); the same geometry may be passed twice.

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 64;
/// Largest NURB degree.
pub const MAX_DEGREE: usize = 9;
/// Largest number of objects in any list (vertices, edges, geometries, candidates, ...).
pub const MAX_LIST: usize = 64;
/// Largest magnitude of any coordinate, knot, parameter or tolerance of a
/// [`Op::ClassifyFacePoint`] or [`Op::FbSplitFace`] input, the ops that reach
/// `sm_ClassifyFacePoint`; rational control points are bounded as `Pw / w`. The
/// boundary split in upstream `SmCurve::GlobalCoincidenceChecker` (reached through
/// `SmFace::CalculateUVDomainFromUVTrimCurves`) never terminates once squared
/// distances overflow to infinity (HANG-29: a control point of `1e153` returns, `1e154`
/// never), and that is the only non-termination established, so the bound sits at the
/// overflow regime. It does not bound the runtime: the split is slow in a
/// shape-dependent way from about `1e15` on (one control point of `-1e15` in every
/// coordinate: 54 s; a fixture scaled uniformly by `1e30`: 55 s; `1e120`..`1e150`:
/// 70-99 s; nothing at or under the bound ran past 99 s in any probe). Such inputs are
/// valid upstream inputs and are accepted; the other ops take any finite value.
pub const MAX_MAGNITUDE: f64 = 1e150;
/// Largest magnitude of any value (control points as Euclidean `Pw / w`) of an input to
/// the ops that run a global solver over a surface: [`Op::SurfaceDropPoint`] and
/// [`Op::FbVertOnVert`] (the point/surface solver of `SmSurface::GlobalPointSolve`),
/// [`Op::TwoPnts`] (that, then the surface/surface solver of
/// `SmSurface::PointBasedSurfaceIntersect`) and [`Op::FbCliff`] (the curve/surface
/// solver of `SmSurface::GlobalCurveIntersect`). Those solvers never finish once a
/// coordinate's own box arithmetic overflows (HANG-20; `SmGlobalSolver::SolveBranch`
/// branches forever): measured on the fuzz programs that found it, with one surface
/// coordinate substituted, the cliff returns at `7e307` and never at `8e307`, the point
/// drop returns at `3e307` and never at `7e307`, the two-point intersection returns at
/// a homogeneous `2e307` (Euclidean `1.3e307`) and never at `3e307` (`2e307`), and all
/// three return in milliseconds at `1e300` and below. The bound sits under every
/// measured hang and at or under every measured return; it is not the squared-distance
/// regime of HANG-29 (those three calls return at `1e150`..`1e300`).
pub const MAX_SOLVER_MAGNITUDE: f64 = 1e307;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    CleanupNoise = 0,
    CheckSeamOut = 1,
    CheckSeamOn = 2,
    SurfaceClosure = 3,
    IsOrderCorrect = 4,
    AdjustUvOnSeam = 5,
    Interpolate = 6,
    SurfaceDropPoint = 7,
    TwoPnts = 8,
    EvaluateMate = 9,
    Capping = 10,
    ClassifyFacePoint = 11,
    EvaluateMateAnalytic = 12,
    FbExport = 20,
    FbVertexGeoms = 21,
    FbVuAtRailEnd = 22,
    FbRailVertexuse = 23,
    FbEdgeName = 24,
    FbFindEdge = 25,
    FbClearEdges = 26,
    FbTrim = 27,
    FbDeleteVertex = 28,
    FbDeleteEdge = 29,
    FbVertOnVert = 30,
    FbCliff = 31,
    FbSplitFace = 32,
    FbExamine = 33,
    FbSetEuGeom = 34,
    FbSetUvCurve = 35,
    FbOrigUvCurve = 36,
    FbSetVuProperty = 37,
    FbBrepEdges = 38,
    FbRemoveEdge = 39,
}

impl Op {
    /// Whether the op starts with a pseudo-B-rep block.
    #[must_use]
    pub fn is_pseudo(self) -> bool {
        (self as i32) >= 20
    }

    /// The op with this code (the `op` of the shim entry point), if any.
    #[must_use]
    pub fn from_code(code: i64) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|&op| i64::from(op as i32) == code)
    }

    /// Every op, in code order.
    pub const ALL: [Self; 33] = [
        Self::CleanupNoise,
        Self::CheckSeamOut,
        Self::CheckSeamOn,
        Self::SurfaceClosure,
        Self::IsOrderCorrect,
        Self::AdjustUvOnSeam,
        Self::Interpolate,
        Self::SurfaceDropPoint,
        Self::TwoPnts,
        Self::EvaluateMate,
        Self::Capping,
        Self::ClassifyFacePoint,
        Self::EvaluateMateAnalytic,
        Self::FbExport,
        Self::FbVertexGeoms,
        Self::FbVuAtRailEnd,
        Self::FbRailVertexuse,
        Self::FbEdgeName,
        Self::FbFindEdge,
        Self::FbClearEdges,
        Self::FbTrim,
        Self::FbDeleteVertex,
        Self::FbDeleteEdge,
        Self::FbVertOnVert,
        Self::FbCliff,
        Self::FbSplitFace,
        Self::FbExamine,
        Self::FbSetEuGeom,
        Self::FbSetUvCurve,
        Self::FbOrigUvCurve,
        Self::FbSetVuProperty,
        Self::FbBrepEdges,
        Self::FbRemoveEdge,
    ];
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_geom_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input: the shared [`crate::reader::Cursor`] plus this
/// op's magnitude bound. `unit_reader!` takes a fixed bound, and this unit's bound
/// depends on the op, so the newtype is spelled out here.
struct Reader<'a> {
    cursor: crate::reader::Cursor<'a>,
    /// The magnitude bound of this op's values: [`MAX_MAGNITUDE`] for the ops reaching
    /// HANG-29, [`MAX_SOLVER_MAGNITUDE`] for the global-solver ops (HANG-20), infinite
    /// otherwise.
    bound: f64,
}

impl<'a> core::ops::Deref for Reader<'a> {
    type Target = crate::reader::Cursor<'a>;
    fn deref(&self) -> &Self::Target {
        &self.cursor
    }
}

impl core::ops::DerefMut for Reader<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cursor
    }
}

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > self.bound {
            return Err(bad("non-finite or oversized value"));
        }
        Ok(v)
    }

    /// Exactly `marker` (`NL_NOZ` / `NL_NOW`), or a finite (bounded) value.
    fn marker_or_finite(&mut self, marker: f64) -> Result<f64, FfiError> {
        let v = self.num()?;
        if v == marker {
            return Ok(v);
        }
        if !v.is_finite() || v.abs() > self.bound {
            return Err(bad("non-finite or oversized value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    /// An integer in `-1..max` as `None` / `Some`.
    fn opt_index(&mut self, len: usize) -> Result<Option<usize>, FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(None);
        }
        if v.fract() != 0.0 || v < 0.0 || v >= len as f64 {
            return Err(bad(format!("bad optional index {v} (len {len})")));
        }
        Ok(Some(v as usize))
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok(())
}

/// Checks that all points share their rational marker and, for `dim == 3`, are 3D.
fn check_points(pts: &[[f64; 4]], dim: usize, what: &str) -> Result<(), FfiError> {
    let [_, _, _, w0] = *pts
        .first()
        .ok_or_else(|| bad(format!("{what}: no points")))?;
    for &[_, _, z, w] in pts {
        if dim == 3 && z == NOZ {
            return Err(bad(format!("{what}: 2D data")));
        }
        if (w == NOW) != (w0 == NOW) {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if w != NOW && w <= 0.0 {
            return Err(bad(format!("{what}: non-positive weight")));
        }
    }
    Ok(())
}

fn points(rd: &mut Reader<'_>, k: usize) -> Result<Vec<[f64; 4]>, FfiError> {
    (0..k)
        .map(|_| {
            let pt = [
                rd.finite()?,
                rd.finite()?,
                rd.marker_or_finite(NOZ)?,
                rd.marker_or_finite(NOW)?,
            ];
            // the bound is on the Euclidean point: a tiny weight under a bounded
            // homogeneous coordinate reaches the HANG-29 regime just the same
            let w = pt[3];
            if rd.bound.is_finite() && w != NOW {
                let oversized = |c: f64| {
                    let e = (c / w).abs();
                    e.is_nan() || e > rd.bound
                };
                if oversized(pt[0]) || oversized(pt[1]) || (pt[2] != NOZ && oversized(pt[2])) {
                    return Err(bad("oversized Euclidean control point"));
                }
            }
            Ok(pt)
        })
        .collect()
}

/// Reads and validates a curve `dim n p m cpts knots` of dimension `want_dim`.
fn curve(rd: &mut Reader<'_>, want_dim: usize, what: &str) -> Result<(), FfiError> {
    let dim = rd.index(3)?;
    if dim != want_dim {
        return Err(bad(format!("{what}: need a {want_dim}D curve")));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad(format!("{what}: need 1 <= p <= n and m = n + p + 1")));
    }
    let pts = points(rd, n + 1)?;
    let knots = rd.finites(m + 1)?;
    check_points(&pts, dim, what)?;
    check_knots(&knots, p, what)?;
    Ok(())
}

/// Reads and validates a surface `n m p q r s cpts u v`.
fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    let pts = points(rd, (n + 1) * (m + 1))?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    check_points(&pts, 3, "surface")?;
    check_knots(&u, p, "surface u")?;
    check_knots(&v, q, "surface v")?;
    Ok(())
}

/// `SmSurfParamType` values: U, V, UNKNOWN, BOTH, NEITHER, UMIN, VMIN, UMAX, VMAX, OUT
/// (the whole enum; the seam checks act on U, V and BOTH and return for the rest).
fn closure(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.index(9).map(|_| ())
}

/// `SmOrientType` values: UNKNOWN, SAME, OPPOSITE.
fn orient(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.index(2).map(|_| ())
}

fn validate_geometry(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::CleanupNoise => {
            rd.finites(2)?;
        }
        Op::CheckSeamOut | Op::CheckSeamOn => {
            surface(rd)?;
            rd.finite()?;
            closure(rd)?;
            rd.finites(2)?;
        }
        Op::SurfaceClosure | Op::SurfaceDropPoint => {
            surface(rd)?;
            rd.finites(4)?;
        }
        Op::IsOrderCorrect => {
            let n = rd.index(MAX_LIST)?;
            rd.finites(6 * n)?;
            let i1 = rd.index(MAX_LIST)?;
            let i2 = rd.index(MAX_LIST)?;
            if i1 >= n || i2 >= n {
                return Err(bad("order: index out of range"));
            }
        }
        Op::AdjustUvOnSeam => {
            surface(rd)?;
            rd.finites(5)?;
        }
        Op::Interpolate => {
            rd.finites(12)?;
            rd.index(16)?;
        }
        Op::TwoPnts => {
            surface(rd)?;
            surface(rd)?;
            rd.finites(3)?;
            rd.flag()?;
            rd.finites(6)?;
            rd.flag()?;
            rd.finites(4)?;
        }
        Op::EvaluateMate => {
            surface(rd)?;
            rd.finites(2)?;
        }
        Op::Capping => {
            rd.finite()?;
            surface(rd)?;
            curve(rd, 2, "uv curve 1")?;
            orient(rd)?;
            curve(rd, 2, "uv curve 2")?;
            orient(rd)?;
        }
        Op::ClassifyFacePoint => {
            surface(rd)?;
            surface(rd)?;
            rd.finites(3)?;
        }
        Op::EvaluateMateAnalytic => {
            if rd.flag()? {
                // surface of revolution: line ends, axis point and direction, swap
                rd.finites(12)?;
            } else {
                // plane: origin, axes, scales, domain, then make_nurb
                rd.finites(15)?;
            }
            rd.flag()?;
            rd.finites(2)?;
        }
        _ => return Err(bad("not a geometry op")),
    }
    Ok(())
}

/// What the block declares, for validating the op's arguments.
struct Block {
    nt: usize,
    nf: usize,
    nfb: usize,
    /// Per vertex: the classification object kind (`pc_kind`).
    pc_kinds: Vec<usize>,
    /// Per edge: whether it has a curve.
    edge_curves: Vec<bool>,
    /// Per edge: its two vertices.
    edge_vertices: Vec<[usize; 2]>,
    /// Per geometry: whether it has a surface.
    geom_surfaces: Vec<bool>,
    /// Per geometry: whether both rails are set.
    geom_rails: Vec<bool>,
}

impl Block {
    fn nv(&self) -> usize {
        self.pc_kinds.len()
    }
    fn ne(&self) -> usize {
        self.edge_curves.len()
    }
    fn ng(&self) -> usize {
        self.geom_surfaces.len()
    }
}

fn validate_block(rd: &mut Reader<'_>) -> Result<Block, FfiError> {
    let nt = rd.index(MAX_LIST)?;
    for _ in 0..nt {
        curve(rd, 3, "target curve")?;
    }
    let nf = rd.index(MAX_LIST)?;
    for _ in 0..nf {
        surface(rd)?;
    }
    let nfb = rd.index(MAX_LIST)?;
    for _ in 0..nfb {
        curve(rd, 3, "fillet brep curve")?;
        if rd.finite()? < 0.0 {
            return Err(bad("fillet brep edge: negative tolerance"));
        }
    }

    let nv = rd.index(MAX_LIST)?;
    let mut pc_kinds = Vec::with_capacity(nv);
    for _ in 0..nv {
        rd.finites(3)?;
        rd.index(13)?; // SmFilletVertexType
        rd.index(12)?; // SmFilStatus
        rd.opt_index(MAX_LIST)?; // corner tag
                                 // SmPointClassificationType: SM_PC_UNKNOWN (100) ..= SM_PC_NOTHING (110)
        let pc_class = rd.index(110)?;
        if pc_class < 100 {
            return Err(bad("vertex: point class below SM_PC_UNKNOWN"));
        }
        let pc_kind = rd.index(3)?;
        let pc_index = rd.opt_index(MAX_LIST)?;
        let pc_sub = rd.index(1)?;
        match (pc_kind, pc_index) {
            (0, _) => {}
            (1 | 2, Some(i)) if i < nt => {}
            (3, Some(i)) if i < nf && pc_sub == 0 => {}
            _ => return Err(bad("vertex: classification object out of range")),
        }
        rd.opt_index(nf)?; // pc_face
        rd.finites(3)?; // t_param u v
        pc_kinds.push(pc_kind);
    }

    let ng = rd.index(MAX_LIST)?;
    let mut geom_surfaces = Vec::with_capacity(ng);
    for _ in 0..ng {
        rd.index(6)?; // SmFilletGeomType
        rd.index(12)?; // SmFilStatus
        if rd.flag()? {
            curve(rd, 3, "centre line")?;
        }
        let has_surface = rd.flag()?;
        if has_surface {
            surface(rd)?;
        }
        geom_surfaces.push(has_surface);
    }

    let ne = rd.index(MAX_LIST)?;
    let mut edge_curves = Vec::with_capacity(ne);
    let mut edge_vertices = Vec::with_capacity(ne);
    for _ in 0..ne {
        let vs = rd.index(MAX_LIST)?;
        let ve = rd.index(MAX_LIST)?;
        if vs >= nv || ve >= nv {
            return Err(bad("edge: vertex out of range"));
        }
        rd.index(14)?; // SmFilletEdgeType
        rd.index(12)?; // SmFilStatus
        rd.opt_index(MAX_LIST)?; // corner tag
        let has_curve = rd.flag()?;
        if has_curve {
            curve(rd, 3, "edge curve")?;
        }
        if rd.flag()? {
            rd.finites(2)?; // any interval, as SetInterval
        }
        rd.opt_index(nfb)?;
        rd.opt_index(nfb)?;
        for _ in 0..2 {
            if rd.flag()? {
                curve(rd, 2, "uv curve")?;
            }
            rd.opt_index(ng)?;
        }
        rd.finites(4)?;
        edge_curves.push(has_curve);
        edge_vertices.push([vs, ve]);
    }

    let mut geom_rails = Vec::with_capacity(ng);
    for _ in 0..ng {
        let r0 = rd.opt_index(ne)?;
        let r1 = rd.opt_index(ne)?;
        geom_rails.push(r0.is_some() && r1.is_some());
        for _ in 0..2 {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.opt_index(ne)?
                    .ok_or_else(|| bad("geometry: other rail -1"))?;
            }
        }
        let ns = rd.index(MAX_LIST)?;
        for _ in 0..ns {
            rd.opt_index(ne)?
                .ok_or_else(|| bad("geometry: side edge use -1"))?;
            rd.index(1)?;
        }
    }

    for _ in 0..nv {
        rd.opt_index(nv)?;
        rd.opt_index(nv)?;
    }

    Ok(Block {
        nt,
        nf,
        nfb,
        pc_kinds,
        edge_curves,
        edge_vertices,
        geom_surfaces,
        geom_rails,
    })
}

fn edge_use(rd: &mut Reader<'_>, b: &Block) -> Result<usize, FfiError> {
    let e = rd
        .opt_index(b.ne())?
        .ok_or_else(|| bad("edge use: edge -1"))?;
    rd.index(1)?;
    Ok(e)
}

fn validate_pseudo(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let b = validate_block(rd)?;
    let _ = (b.nt, b.nf);
    match op {
        Op::FbExport => {}
        Op::FbVertexGeoms => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
        }
        Op::FbVuAtRailEnd => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            rd.opt_index(b.ng())?;
            let xe = rd.opt_index(b.ne())?;
            let xw = rd.num()?;
            if xe.is_some() && xw != 0.0 && xw != 1.0 {
                return Err(bad("exclude: edge use not 0/1"));
            }
            rd.opt_index(b.ne())?;
        }
        Op::FbRailVertexuse => {
            let g = rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.index(1)?;
            rd.index(1)?;
            if !b.geom_rails.get(g).copied().unwrap_or(false) {
                return Err(bad("rail vertex use: geometry without both rails"));
            }
        }
        Op::FbEdgeName | Op::FbBrepEdges | Op::FbOrigUvCurve => {
            rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
        }
        Op::FbFindEdge => {
            let e = rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
            if !b.edge_curves.get(e).copied().unwrap_or(false) {
                return Err(bad("find edge: edge without a curve"));
            }
            rd.flag()?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.opt_index(b.nfb)?.ok_or_else(|| bad("candidate -1"))?;
            }
        }
        Op::FbClearEdges => {
            rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.opt_index(b.nfb)?.ok_or_else(|| bad("edge -1"))?;
            }
        }
        Op::FbTrim => {
            let g = rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.finites(2)?;
            if !b.geom_rails.get(g).copied().unwrap_or(false) {
                return Err(bad("trim: geometry without both rails"));
            }
        }
        Op::FbDeleteVertex => {
            let v = rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            if b.edge_vertices.iter().any(|&[s, e]| s == v || e == v) {
                return Err(bad("delete vertex: it still has edges"));
            }
        }
        Op::FbDeleteEdge | Op::FbRemoveEdge => {
            rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
            if op == Op::FbRemoveEdge {
                rd.flag()?;
            }
        }
        Op::FbVertOnVert => {
            let v = rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            if b.pc_kinds.get(v).copied() != Some(1) {
                return Err(bad("vert on vert: not classified to a vertex"));
            }
        }
        Op::FbCliff => {
            let v = rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            surface(rd)?;
            rd.finite()?;
            if b.pc_kinds.get(v).copied() != Some(2) {
                return Err(bad("cliff: not classified to an edge use"));
            }
        }
        Op::FbSplitFace => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            rd.opt_index(b.nf)?.ok_or_else(|| bad("face -1"))?;
            rd.opt_index(b.nf)?.ok_or_else(|| bad("face -1"))?;
        }
        Op::FbExamine => {
            let g1 = rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.flag()?;
            let g2 = rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.flag()?;
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            rd.finite()?;
            // the same geometry twice is allowed (SmFilletEdge::CalcCrossSection does
            // it); the shim refuses an input that reaches the adjusting path, where
            // upstream dereferences the geometry's (here NULL) SmFilletSolver
            let _ = (g1, g2);
        }
        Op::FbSetEuGeom => {
            edge_use(rd, &b)?;
            rd.opt_index(b.ng())?;
        }
        Op::FbSetUvCurve => {
            edge_use(rd, &b)?;
            if rd.flag()? {
                curve(rd, 2, "uv curve")?;
            }
            rd.flag()?;
            rd.flag()?;
        }
        Op::FbSetVuProperty => {
            edge_use(rd, &b)?;
            edge_use(rd, &b)?;
        }
        _ => return Err(bad("not a pseudo-B-rep op")),
    }
    Ok(())
}

/// Checks `input` for `op` exactly as [`eval`] does before calling the shim: the
/// layout, counts and indices, and the magnitude bounds of the hang regimes (HANG-29,
/// HANG-20). The difftest runs the Rust side only on inputs this accepts, so an input
/// in a hang regime never reaches the Rust port either.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: match op {
            Op::ClassifyFacePoint | Op::FbSplitFace => MAX_MAGNITUDE,
            Op::SurfaceDropPoint | Op::TwoPnts | Op::FbVertOnVert | Op::FbCliff => {
                MAX_SOLVER_MAGNITUDE
            }
            _ => f64::INFINITY,
        },
    };
    if op.is_pseudo() {
        validate_pseudo(op, &mut rd)?;
    } else {
        validate_geometry(op, &mut rd)?;
    }
    rd.done()
}

/// Runs `op` on the flat `input` (layouts above) and returns the shim's record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely;
/// [`FfiError::Status`] for a shim error (`SC_ERR_*`).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads (validated above); buf is valid
        // for buf.len() writes; len is a valid pointer.
        unsafe {
            sys::sc_blend_fillet_geom_p1_eval(
                op as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
