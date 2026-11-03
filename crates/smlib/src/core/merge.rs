//! Sewing and merging on solidcore: the C API's `smlib_stitch` (the fork's
//! `capi/src/smlib_sheets.cpp`) and `smlib_merge_faces` (`capi/src/smlib_ops.cpp`, with
//! `capi/src/smlib_pick.h`'s `pick_edges`).
//!
//! As there, each works on a copy of its body (`SmApiBrepCopy`). A stitch is `SmBrep::StitchFaces`
//! at the caller's tolerance, once, then `SmBrep::OrientTrimmedSurfaces`: outward where the faces
//! close into a solid, else only made to agree across their shared edges. `StitchAndOrient` is not
//! used, as the capi does not use it: it caps the tolerance and stitches a second time, so the
//! caller's tolerance would not be the one that decides.
//!
//! `StitchFaces` glues an edge only to an edge one face uses: a sheet laid on edges two faces of
//! a solid already share (a cover over a box's cut top) is left beside the solid. As the fork's
//! e5d9bba, edges still lamina after that pass are glued at the same tolerance to the edges they
//! lie on whatever uses those (`bAllowSpineEdges`), on a copy ([`close_onto_shared_edges`]).

use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p2::{ArenaNotifyState, is_manifold_solid};
use solidcore::topology::brep::p3::{DeleteFaceOptions, delete_face, orient_trimmed_surfaces, with_kernel_callbacks};
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::p5::remove_topological_edges_and_vertices;
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId, RegionId, edge};

use super::Body;
use super::blend::{edge_miss, finish, on_copy, pick_edges, points_ok, status, tolerance_ok};
use crate::{Error, Result};
use solidcore::topology::brep::p3::brep_copy;

type Out<T> = std::result::Result<T, Error>;

/// `smlib_stitch`: the faces of a copy of `body` sewn along the vertices and edges that coincide
/// within `tolerance` (finite, positive, at most 1e3), then oriented; with whether the result is a
/// closed solid (no lamina edge left and `IsManifoldSolid`).
pub(crate) fn stitch(body: &Body, tolerance: f64) -> Result<(Body, bool)> {
    if !tolerance.is_finite() || !(tolerance > 0.0) || tolerance > 1e3 {
        return Err(Error::Argument);
    }
    let mut closed = false;
    let r = on_copy("SmBrep::StitchFaces", body, |k, mut b| {
        let mut lamina = match stitch_faces(k, &mut b, tolerance, None, false) {
            Ok(o) => o.lamina_edges,
            Err(e) => return Ok(Err(status(e))),
        };
        if lamina != 0 {
            b = match close_onto_shared_edges(k, b, tolerance, &mut lamina)? {
                Ok(b) => b,
                Err(e) => return Ok(Err(e)),
            };
        }
        let oriented = with_kernel_callbacks(k, &mut b, |k, b, env| {
            let mut marks = Marks::default();
            orient_trimmed_surfaces(k, b, env, &mut marks, true, lamina != 0, None)
        });
        if let Err(e) = oriented {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        closed = lamina == 0 && is_manifold_solid(&b)?;
        Ok(Ok(b))
    })?;
    Ok((r, closed))
}

/// How many regions of `b` are material: neither the infinite region nor a void (`IsVoid`'s
/// `UNSURE` counts as void, as the capi's `!IsVoid()`).
fn material_regions(b: &Brep) -> solidcore::Result<usize> {
    let infinite = b.infinite_region();
    let mut n = 0;
    for r in bl::regions(b, None)? {
        n += usize::from(Some(r) != infinite && b.region(r)?.is_void() == 0);
    }
    Ok(n)
}

/// The region on the side of `f`'s faceuse `fu` (its shell's), if any.
fn region_of(b: &Brep, fu: Option<solidcore::topology::FaceuseId>) -> solidcore::Result<Option<RegionId>> {
    let Some(fu) = fu else { return Ok(None) };
    match b.faceuse(fu)?.shell {
        Some(sh) => Ok(b.shell(sh)?.region),
        None => Ok(None),
    }
}

/// `remove_faces_inside`: every face of `b` lying between two material regions is inside the
/// part and removed (`DeleteFace(f, TRUE, TRUE)` with the B-rep open for editing): its regions
/// are joined, and its edges and vertices nothing else uses go with it.
fn remove_faces_inside(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<Out<()>> {
    let infinite = b.infinite_region();
    let mut inside: Vec<FaceId> = Vec::new();
    for f in bl::faces(b, None)? {
        let (u1, u2) = face_p2::faceuses(b, f)?;
        let (Some(r1), Some(r2)) = (region_of(b, u1)?, region_of(b, u2)?) else { continue };
        let material = |r: RegionId| -> solidcore::Result<bool> { Ok(Some(r) != infinite && b.region(r)?.is_void() == 0) };
        if r1 != r2 && material(r1)? && material(r2)? {
            inside.push(f);
        }
    }
    let editing = b.editing_enabled();
    b.set_editing_enabled(true);
    let mut deleted = Ok(());
    {
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        for f in inside {
            let options = DeleteFaceOptions { delete_edges_vertices: true, do_region_nesting: true, ..DeleteFaceOptions::default() };
            if let Err(e) = delete_face(k, b, &mut env, &mut marks, f, options) {
                deleted = Err(e);
                break;
            }
        }
    }
    b.set_editing_enabled(editing);
    b.refresh_cached_topology();
    Ok(deleted.map_err(status))
}

/// `close_onto_shared_edges` (fork e5d9bba): `b`, stitched as a manifold with `lamina` edges
/// left free, those glued at `tolerance` to the edges they lie on whatever uses those
/// (`StitchFaces` with `bAllowSpineEdges`) on a copy. The copy is taken, with its count of
/// lamina edges, when the gluing closed a space (a new, material region) and, the faces then
/// between two material regions removed, no edge has more than two faces; a sheet that closes
/// nothing with the solid's faces stays as the manifold stitch left it. A gluing that leaves
/// fewer material regions than it found (a sheet with no sector between the faces at its rim:
/// glued elsewhere it joins the solid's inside to the outside) is refused, as the capi's
/// `SMLIB_ERR_UNSUPPORTED`.
fn close_onto_shared_edges(k: &mut Kernel, b: Brep, tolerance: f64, lamina: &mut u32) -> solidcore::Result<Out<Brep>> {
    let mut c = brep_copy(k, &b)?;
    let material = material_regions(&c)?;
    let outcome = match stitch_faces(k, &mut c, tolerance, None, true) {
        Ok(o) => o,
        Err(e) => return Ok(Err(status(e))),
    };
    if outcome.stitched_edges == 0 {
        return Ok(Ok(b));
    }
    if material_regions(&c)? < material {
        return Ok(Err(Error::Unsupported));
    }
    if let Err(e) = remove_faces_inside(k, &mut c)? {
        return Ok(Err(e));
    }
    for e in bl::edges(&c, None)? {
        if edge::is_spine(&c, e)? {
            return Ok(Ok(b));
        }
    }
    *lamina = outcome.lamina_edges;
    Ok(Ok(c))
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
        finish(b, solid_in)
    })
}
