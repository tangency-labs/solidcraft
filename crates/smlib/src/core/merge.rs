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

enum Keep<'a> {
    Points(&'a [[f64; 3]], f64),
    Indices(&'a [usize]),
}

/// Shared body of [`merge_faces`] and [`merge_faces_keeping_edges`]: runs
/// `SmBrep::RemoveTopologicalEdgesAndVertices` on a copy of `body`, keeping the edges `keep`
/// selects; then `finish` (faces left, and a solid in gives a manifold solid).
fn merge_faces_with(body: &Body, keep: Keep<'_>) -> Result<Body> {
    on_copy("SmBrep::RemoveTopologicalEdgesAndVertices", body, |k, mut b| {
        let kept = match keep {
            Keep::Points(pts, tolerance) => {
                if pts.is_empty() {
                    Vec::new()
                } else {
                    match pick_edges(k, &b, pts, tolerance)? {
                        Ok((edges, _)) => edges,
                        Err(m) => return Ok(Err(edge_miss(m))),
                    }
                }
            }
            Keep::Indices(indices) => {
                if indices.is_empty() {
                    Vec::new()
                } else {
                    let all = b.edge_list();
                    let mut seen = vec![false; all.len()];
                    let mut kept = Vec::new();
                    for &idx in indices {
                        let Some(&edge_id) = all.get(idx) else {
                            return Ok(Err(Error::Argument));
                        };
                        if let Some(flag) = seen.get_mut(idx)
                            && !*flag
                        {
                            *flag = true;
                            kept.push(edge_id);
                        }
                    }
                    kept
                }
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

/// `smlib_merge_faces`: `SmBrep::RemoveTopologicalEdgesAndVertices` on a copy of `body`, keeping
/// the edges nearest `keep` (each within `tolerance`); then `finish` (faces left, and a solid in
/// gives a manifold solid).
pub(crate) fn merge_faces(body: &Body, keep: &[[f64; 3]], tolerance: f64) -> Result<Body> {
    if !points_ok(keep, true) || !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    merge_faces_with(body, Keep::Points(keep, tolerance))
}

/// `SmBrep::RemoveTopologicalEdgesAndVertices` on a copy of `body`, keeping the edges at
/// indices `keep` in SMLib's edge order (`Brep::edge_list`). Unlike [`merge_faces`], this names
/// edges by topology index rather than 3D proximity, so it is not capped at `MAX_POINTS`, cannot
/// be ambiguous when another edge passes near a midpoint, and avoids the `O(|keep| * |edges|)`
/// closest-point search.
///
/// Refuses any out-of-range index with [`Error::Argument`].
pub(crate) fn merge_faces_keeping_edges(body: &Body, keep: &[usize]) -> Result<Body> {
    merge_faces_with(body, Keep::Indices(keep))
}
