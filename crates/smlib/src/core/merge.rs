//! Sewing and merging on solidcore: the C API's `smlib_stitch` (the fork's
//! `capi/src/smlib_sheets.cpp`) and `smlib_merge_faces` (`capi/src/smlib_ops.cpp`, with
//! `capi/src/smlib_pick.h`'s `pick_edges`).
//!
//! As there, each works on a copy of its body (`SmApiBrepCopy`), and both are solidcore-ops'. A
//! stitch ([`solidcore_ops::stitch::stitch`]) is `SmBrep::StitchFaces` at the caller's tolerance,
//! once, edges still lamina glued onto the edges they lie on as the fork's e5d9bba, then
//! `SmBrep::OrientTrimmedSurfaces`. A merge ([`solidcore_ops::stitch::merge_faces_with`]) is
//! `SmBrep::RemoveTopologicalEdgesAndVertices` keeping the edges picked by point or named by
//! index.

use solidcore_ops::Contract;
use solidcore_ops::pick::Miss;
use solidcore_ops::stitch::{KeptEdges, MergeOptions};

use super::Body;
use super::blend::{points_ok, tolerance_ok};
use super::call::ops_as;
use crate::{Error, Result};

/// `smlib_stitch`: the faces of a copy of `body` sewn along the vertices and edges that coincide
/// within `tolerance` (finite, positive, at most 1e3), then oriented; with whether the result is a
/// closed solid (no lamina edge left and `IsManifoldSolid`).
pub(crate) fn stitch(body: &Body, tolerance: f64) -> Result<(Body, bool)> {
    if !tolerance.is_finite() || !(tolerance > 0.0) || tolerance > 1e3 {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("SmBrep::StitchFaces: the body was left unfinished by an earlier panic".into()));
    }
    ops_as("SmBrep::StitchFaces", &[("brep", body)], super::blend::refused, || solidcore_ops::stitch::stitch(body.copy()?, tolerance))
}

/// The capi's merge contract, as solidcore-ops runs it ([`solidcore_ops::stitch::merge_faces_with`]):
/// a solid in gives a closed manifold solid by the topology test, the result's tolerances first
/// grown to cover marginal gaps (SolidCraft issue #10: fuzzer seeds 178 and others).
const CAPI_MERGE: MergeOptions = MergeOptions { contract: Contract::Closed, grow_tolerances: true };

/// Shared body of [`merge_faces`] and [`merge_faces_keeping_edges`]: solidcore-ops' merge
/// (`SmBrep::RemoveTopologicalEdgesAndVertices`) on a copy of `body`, keeping the edges `keep`
/// names, under the capi's contract ([`CAPI_MERGE`]: faces left, and a solid in gives a manifold
/// solid). A pick that chooses no edge is `pick_error`'s (ops.rs) [`Error::NoMatch`] or
/// [`Error::Ambiguous`].
fn merge_faces_with(body: &Body, keep: KeptEdges<'_>) -> Result<Body> {
    const CALL: &str = "SmBrep::RemoveTopologicalEdgesAndVertices";
    if body.kernel_left_unfinished() {
        return Err(Error::Internal(format!("{CALL}: the body was left unfinished by an earlier panic")));
    }
    let merged = ops_as(CALL, &[("brep", body)], super::blend::refused, || solidcore_ops::stitch::merge_faces_with(body.copy()?, keep, CAPI_MERGE))?;
    merged.map_err(|m| {
        let e = match m {
            Miss::None(point) => Error::NoMatch { point },
            Miss::Ambiguous(point) => Error::Ambiguous { point },
        };
        super::dump::failed_ops(CALL, &e.to_string(), &[("brep", body)]);
        e
    })
}

/// `smlib_merge_faces`: `SmBrep::RemoveTopologicalEdgesAndVertices` on a copy of `body`, keeping
/// the edges nearest `keep` (each within `tolerance`); then `finish` (faces left, and a solid in
/// gives a manifold solid).
pub(crate) fn merge_faces(body: &Body, keep: &[[f64; 3]], tolerance: f64) -> Result<Body> {
    if !points_ok(keep, true) || !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    merge_faces_with(body, KeptEdges::Nearest { points: keep, tolerance })
}

/// `SmBrep::RemoveTopologicalEdgesAndVertices` on a copy of `body`, keeping the edges at
/// indices `keep` in SMLib's edge order (`Brep::edge_list`). Unlike [`merge_faces`], this names
/// edges by topology index rather than 3D proximity, so it is not capped at `MAX_POINTS`, cannot
/// be ambiguous when another edge passes near a midpoint, and avoids the `O(|keep| * |edges|)`
/// closest-point search.
///
/// Refuses any out-of-range index with [`Error::Argument`].
pub(crate) fn merge_faces_keeping_edges(body: &Body, keep: &[usize]) -> Result<Body> {
    merge_faces_with(body, KeptEdges::Native(keep))
}
