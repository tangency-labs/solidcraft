//! Trim: a face cut along curves that lie on it into the regions they bound, each with a point
//! inside it, and faces taken out of a body (the regions an application discards).
//!
//! A face is cut by the kernel's merge of curves on a surface (`MergeCurvesOnSurface`, as
//! `SmBrep::ProjectAndTrim` merges its projections): the curves, each within the tolerance of
//! the face's surface (as [`crate::sweep::project_curves`] makes them), become edges of the face,
//! and each run of them that closes, on itself or on the face's boundary, splits off a face. A
//! curve the merge leaves hanging (an open curve whose end reaches neither the boundary nor
//! another curve) is refused, as is a cut that splits nothing. Each region's point is the
//! kernel's interior point of its face (`SmApiFaceInternalPoint`). Faces are taken out by the
//! kernel's `SmBrep::RemoveFaces`, with the edges and vertices only they used.
//!
//! Faces are record indices ([`Order`]). Ops sequences the kernel's calls and compares what they
//! return; it computes nothing. Every operation works on a copy: its operands are unchanged.

use std::collections::HashSet;

use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_internal_point;
use solidcore::topology::brep::p10::merge_curves_on_surface;
use solidcore::topology::brep::p3::{brep_copy_mapped, edit_brep, with_kernel_callbacks};
use solidcore::topology::brep::p5::remove_faces_notified;
use solidcore::topology::face::p4 as face_p4;
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::topology::{edge, vertex, Brep, FaceId, NamingScheme, VertexId};
use solidcore::EntityRef;

use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::order::{self, Order};
use crate::session::run_on;
use crate::split::{copies_of, curve_ref, MAX_FACES};
use crate::sweep::{kernel_curves, tolerance_ok, Curve};
use crate::{Error, Result};

/// The most curves one cut takes.
pub const MAX_CURVES: usize = 4096;

/// A region a face was cut into: its face, as an index into the cut body's record (the
/// [`Order`] [`Body::to_record`] gives), and a point inside it, on the face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    /// The region's face, a record index of the cut body.
    pub face: usize,
    /// A point of the face inside its boundary.
    pub inside: [f64; 3],
}

/// The faces of a copy that are `ids` (a body's own faces), through the copy's map; `None`
/// when one has no copy.
fn face_copies(map: &CopyBrepMap<EntityRef>, ids: &[FaceId]) -> Option<Vec<FaceId>> {
    copies_of(map, ids, EntityRef::Face, |e| match e {
        EntityRef::Face(f) => Some(f),
        _ => None,
    })
}

/// Whether one of `faces` has a vertex not in `before` that the cut left hanging: one edge
/// meets it, and that edge's other end is elsewhere (a closed curve's one edge starts and ends
/// at its vertex).
fn hangs(b: &Brep, faces: &[FaceId], before: &HashSet<VertexId>) -> solidcore::Result<bool> {
    for &f in faces {
        for v in face_p4::vertices(b, f, None, false, None)? {
            if before.contains(&v) {
                continue;
            }
            let edges = vertex::edges(b, v, None)?;
            let [e] = edges.as_slice() else {
                continue;
            };
            let (start, end) = edge::start_end_vertices(b, *e)?;
            if start != end {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A face cut in a copy of its body: the copy, and each region's face with the kernel's point
/// inside it.
type Cut = (Brep, Vec<(FaceId, [f64; 3])>);

/// The face cut along `curves` in a copy of the body ([`Cut`]), the face first, then those split
/// off it in the order the merge made them.
fn cut(
    k: &mut Kernel,
    body: &Body,
    src: FaceId,
    curves: &[Curve],
    tolerance: f64,
) -> solidcore::Result<Result<Cut>> {
    let curves = match kernel_curves(k, curves) {
        Ok(c) => c,
        Err(e) => return Ok(Err(e)),
    };
    // the record's face is the body's own: looked up in it, then mapped to its copy (the copy
    // numbers its entities as it builds them)
    let (mut a, f) = {
        let g = body.lock();
        if !g.face_list().contains(&src) {
            return Ok(Err(Error::Argument));
        }
        let (a, map) = brep_copy_mapped(k, &g)?;
        let Some([f]) = face_copies(&map, &[src]).and_then(|v| <[FaceId; 1]>::try_from(v).ok())
        else {
            return Ok(Err(Error::Argument));
        };
        (a, f)
    };
    // the face's own vertices: any other is one the curves made
    let before: HashSet<VertexId> = face_p4::vertices(&a, f, None, false, None)?
        .into_iter()
        .collect();
    // the merge's tolerance: the body's, the face's or the curves', whichever is largest (as
    // the kernel's project and trim takes it)
    let mut tol = a.tolerance();
    let face_tol = a.face(f)?.tolerance;
    if face_tol > tol {
        tol = face_tol;
    }
    if tolerance > tol {
        tol = tolerance;
    }
    let editing = a.editing_enabled();
    let doing = k.context.doing_boolean();
    a.set_editing_enabled(true);
    k.context.set_doing_boolean(true);
    // the edits notify through the kernel's callbacks with the body's own attributes, so what
    // an application attached to the face follows each piece
    let merged = with_kernel_callbacks(k, &mut a, |k, a, env| {
        merge_curves_on_surface(k, a, env, &curve_ref, f, tol, &curves, None, false)
    });
    a.set_editing_enabled(editing);
    k.context.set_doing_boolean(doing);
    a.refresh_cached_topology();
    // the cut's new edges own their curves under the kernel's identities (it records them),
    // and the check reads the owners by the body's naming
    a.set_naming(NamingScheme::Caller);
    let split_off = match merged {
        Ok((made, _)) => made,
        Err(e) => return Ok(Err(status(e))),
    };
    let mut regions = vec![f];
    regions.extend(split_off);
    if regions.len() < 2 || hangs(&a, &regions, &before)? {
        return Ok(Err(Error::CurveDoesNotCut));
    }
    let mut out = Vec::new();
    for r in regions {
        match face_internal_point(k, &mut a, r) {
            Ok(p) => out.push((r, [p.x, p.y, p.z])),
            Err(e) => return Ok(Err(status(e))),
        }
    }
    Ok(Ok((a, out)))
}

/// A copy of `body` with record face `face` (an index into `order`) cut along `curves`, and
/// the regions the face became, at least two, in record order of the copy, each with a point
/// inside it (for an application to show the regions and pick those to keep). The curves lie
/// on the face's surface within `tolerance` (as [`crate::sweep::project_curves`] makes them,
/// each its curve over its whole parameter range); each closed run of them, on itself or ending
/// on the face's boundary, bounds a region. `body` is unchanged.
///
/// # Errors
/// - [`Error::Argument`]: no curve, more than [`MAX_CURVES`], a curve refused, a tolerance not
///   finite or outside `[1e-9, 1e3)`, or a face past the record's list or not the body's;
/// - [`Error::CurveDoesNotCut`]: a curve left hanging in the face (an open curve whose end
///   reaches neither the face's boundary nor another curve), or curves that split nothing;
/// - [`Error::Exception`]: a region the cut body's record does not list;
/// - the kernel's errors, and those of [`Body::to_record`].
pub fn split_face_by_curves(
    body: &Body,
    order: &Order,
    face: usize,
    curves: &[Curve],
    tolerance: f64,
) -> Result<(Body, Vec<Region>)> {
    if curves.is_empty() || curves.len() > MAX_CURVES || !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    let src = order::face(order, face)?;
    let (b, inside) = run_on("split_face_by_curves", &[("brep", body)], |k| {
        cut(k, body, src, curves, tolerance)
    })??;
    let b = Body::new(b);
    // the regions by their record indices, as the application names faces
    let (_, named) = b.to_record()?;
    let mut regions = Vec::new();
    for (f, p) in inside {
        let Some(i) = named.faces.iter().position(|&x| x == f) else {
            return Err(Error::Exception);
        };
        regions.push(Region { face: i, inside: p });
    }
    regions.sort_by_key(|r| r.face);
    Ok((b, regions))
}

/// A copy of `body` without record faces `faces` (indices into `order`, distinct), and without
/// the edges and vertices only they used: what is left of a solid is a sheet. `body` is
/// unchanged.
///
/// # Errors
/// - [`Error::Argument`]: no face, more than [`MAX_FACES`], a face twice or past the record's
///   list or not the body's;
/// - [`Error::Empty`]: every face taken out;
/// - the kernel's errors.
pub fn remove_faces(body: &Body, order: &Order, faces: &[usize]) -> Result<Body> {
    if faces.is_empty() || faces.len() > MAX_FACES {
        return Err(Error::Argument);
    }
    let mut seen = HashSet::new();
    let mut picked = Vec::new();
    for &f in faces {
        if !seen.insert(f) {
            return Err(Error::Argument);
        }
        picked.push(order::face(order, f)?);
    }
    let left = run_on("remove_faces", &[("brep", body)], |k| {
        // the record's faces are the body's own: looked up in it, then mapped to their copies
        let (mut a, picked) = {
            let g = body.lock();
            if picked.iter().any(|f| !g.face_list().contains(f)) {
                return Ok(Err(Error::Argument));
            }
            let (a, map) = brep_copy_mapped(k, &g)?;
            let Some(picked) = face_copies(&map, &picked) else {
                return Ok(Err(Error::Argument));
            };
            (a, picked)
        };
        // an edit of the body, notified through the kernel's callbacks with its own attributes
        let removed = edit_brep(k, &mut a, |k, b, scope| {
            let editing = b.editing_enabled();
            remove_faces_notified(k, b, editing, &picked, scope.env)
        });
        if let Err(e) = removed {
            return Ok(Err(status(e)));
        }
        finish(k, a, Expect::FACES)
    })??;
    Ok(Body::new(left))
}
