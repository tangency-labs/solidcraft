//! Sewing and merging on solidcore: the C API's `smlib_stitch` (the fork's
//! `capi/src/smlib_sheets.cpp`) and `smlib_merge_faces` (`capi/src/smlib_ops.cpp`, with
//! `capi/src/smlib_pick.h`'s `pick_edges`).
//!
//! As there, each works on a copy of its body (`SmApiBrepCopy`). A stitch is solidcore-ops'
//! ([`solidcore_ops::stitch::stitch`]): `SmBrep::StitchFaces` at the caller's tolerance, once,
//! edges still lamina glued onto the edges they lie on as the fork's e5d9bba, then
//! `SmBrep::OrientTrimmedSurfaces`.

use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::remove_topological_edges_and_vertices;

use super::Body;
use super::blend::{edge_miss, finish, on_copy, pick_edges, points_ok, status, tolerance_ok};
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

/// `smlib_merge_faces`: `SmBrep::RemoveTopologicalEdgesAndVertices` on a copy of `body`, keeping
/// the edges nearest `keep` (each within `tolerance`); then `finish` (faces left, and a solid in
/// gives a manifold solid).
pub(crate) fn merge_faces(body: &Body, keep: &[[f64; 3]], tolerance: f64) -> Result<Body> {
    if !points_ok(keep, true) || !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    on_copy("SmBrep::RemoveTopologicalEdgesAndVertices", body, |k, mut b| {
        let kept = if keep.is_empty() {
            Vec::new()
        } else {
            match pick_edges(k, &b, keep, tolerance)? {
                Ok((edges, _)) => edges,
                Err(m) => return Ok(Err(edge_miss(m))),
            }
        };
        let solid_in = is_manifold_solid(&b)?;
        let keep_edges = (!kept.is_empty()).then_some(kept.as_slice());
        if let Err(e) = remove_topological_edges_and_vertices(k, &mut b, keep_edges, None) {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish(k, b, solid_in)
    })
}
