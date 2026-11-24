//! Trim: a face cut along curves that lie on it into the regions they bound, each with a point
//! inside it, faces taken out of a body (the regions an application discards), and a body cut
//! along curves dropped square onto its faces, keeping the regions points pick.
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
//! A body is cut along dropped curves face by face and chain by chain (a chain: curves that
//! meet end to end): the kernel's drop of a chain's curves square onto the face's surface
//! ([`crate::sweep::drop_curves`]), merged as above into the face and into each piece earlier
//! chains cut off it. A chain that bounds nothing on a face leaves the face as it was; one that
//! bounds nothing on any face is refused, and so is a curve the kernel drops onto no face. The cut
//! lines are the edges the merges made, whole or split by a later cut: the body's own edges
//! are tagged before the first cut, the kernel's splits carry the tag, and an edge without it
//! is a cut line. The cut body's faces fall into regions (faces that meet at an edge that is
//! not a cut line are in one region), a point picks the region of the face nearest it (the
//! kernel's distance to the face as it is trimmed), and the faces of the regions no point
//! picks are taken out.
//!
//! Faces are record indices ([`Order`]). Ops sequences the kernel's calls and compares what they
//! return; it computes nothing. Every operation works on a copy: its operands are unchanged.

use std::collections::{BTreeMap, HashSet};

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::curve::curve::Curve as KCurve;
use solidcore::geom::tol::are_same_to_tol;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_internal_point;
use solidcore::topology::brep::p10::merge_curves_on_surface;
use solidcore::topology::brep::p3::{
    brep_copy, brep_copy_mapped, edit_brep, with_kernel_callbacks,
};
use solidcore::topology::brep::p5::remove_faces_notified;
use solidcore::topology::face::p4 as face_p4;
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId, NamingScheme, VertexId};
use solidcore::{EntityRef, SolidError};

use crate::body::{finish, finish_proved, Body, Expect};
use crate::error::status;
use crate::history::{self, History, Made};
use crate::order::{self, Order};
use crate::session::{run_on, run_taking, with_body};
use crate::split::{copies_of, curve_ref, MAX_FACES};
use crate::sweep::{dropped_on, kernel_curves, tolerance_ok, Curve};
use crate::{Error, Result};

/// The most curves one cut takes.
pub const MAX_CURVES: usize = 4096;

/// The most points one trim picks the regions it keeps by.
pub const MAX_PICKS: usize = 10_000;

/// Why [`trim_by_dropped_curves`] refuses a curve the kernel's drop lands on no face.
const CURVE_LANDS_NOWHERE: &str =
    "a curve drops onto no face of the body (a curve across the seam of a closed face is not dropped yet)";

/// Why [`trim_by_dropped_curves`] refuses a point that picks no one region.
const PICK_ON_A_CUT: &str =
    "a point to keep is as near one region as another (it lies on a cut line): pick inside the region to keep";

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

/// What a merge makes of a face: the faces split off it, in the order the merge made them, and
/// the edges of the curves (the cut lines).
type Merged = (Vec<FaceId>, Vec<EdgeId>);

/// `curves` (the kernel's, each on the surface of face `f` within `tolerance`) merged into face
/// `f` of `a`, a body being edited: [`Merged`], or [`Error::CurveDoesNotCut`] when they split
/// nothing off the face or leave a curve hanging in it (`a` is then partly edited: the caller
/// drops it or puts it back).
fn merge_on(
    k: &mut Kernel,
    a: &mut Brep,
    f: FaceId,
    curves: &[KCurve],
    tolerance: f64,
) -> solidcore::Result<Result<Merged>> {
    // the face's own vertices: any other is one the curves made
    let before: HashSet<VertexId> = face_p4::vertices(a, f, None, false, None)?
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
    let merged = with_kernel_callbacks(k, a, |k, a, env| {
        merge_curves_on_surface(k, a, env, &curve_ref, f, tol, curves, None, false)
    });
    a.set_editing_enabled(editing);
    k.context.set_doing_boolean(doing);
    a.refresh_cached_topology();
    // the cut's new edges own their curves under the kernel's identities (it records them),
    // and the check reads the owners by the body's naming
    a.set_naming(NamingScheme::Caller);
    let (made, lines) = match merged {
        Ok(m) => m,
        Err(e) => return Ok(Err(status(e))),
    };
    let mut regions = vec![f];
    regions.extend(&made);
    if made.is_empty() || hangs(a, &regions, &before)? {
        return Ok(Err(Error::CurveDoesNotCut));
    }
    Ok(Ok((made, lines)))
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
    let made = match merge_on(k, &mut a, f, &curves, tolerance)? {
        Ok((made, _)) => made,
        Err(e) => return Ok(Err(e)),
    };
    let mut regions = vec![f];
    regions.extend(made);
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

/// A body cut along dropped curves in a copy of it: the copy, the cut lines' edges and the
/// copy's tolerance.
type DroppedCut = (Brep, Vec<EdgeId>, f64);

/// A copy of `body` cut along `chains` dropped square onto its faces, without the regions no
/// point of `points` picks. A chain is a list of curves that meet end to end: a closed loop,
/// or an open run.
///
/// Every face is cut on its own, one chain at a time: the chain's curves are dropped onto the
/// face's surface over the face's parameter box (each point of a curve to its foot on the
/// surface, as [`crate::sweep::drop_curves`] drops them, within `tolerance`) and merged into
/// the face and into each piece earlier chains cut off it, where each run of them that closes,
/// on itself, on the face's boundary or on an earlier cut line, splits off a face. A chain that
/// bounds nothing on a face leaves that face as it was (the chain is tried again once another
/// chain has cut the face, in case it ends on that chain's line). Faces that meet at an edge
/// that is not a cut line are in one region; each point picks the region of the face nearest it
/// (the kernel's distance to the face as it is trimmed), and the faces of the other regions are
/// taken out, with the edges and vertices only they used. `body` is unchanged.
///
/// # Errors
/// - [`Error::Argument`]: no chain, an empty chain, more than [`MAX_CURVES`] curves in all, a
///   curve refused, a tolerance not finite or outside `[1e-9, 1e3)`, no point, more than
///   [`MAX_PICKS`], or a coordinate that is not finite;
/// - [`Error::CurveDoesNotCut`]: a chain bounds no region on any face (an open run that ends
///   inside a face, or a chain whose drop lies off the faces as they are trimmed);
/// - [`Error::Unsupported`], with the reason: a curve the kernel drops onto no face (one beside
///   the body, or one across the seam of a closed face, which the kernel's drop does not follow
///   round), a point as near one region as another (on a cut line), or one whose distance to a
///   face the kernel does not answer;
/// - the kernel's errors, and those of [`Body::to_record`] and [`remove_faces`].
pub fn trim_by_dropped_curves(
    body: &Body,
    chains: &[Vec<Curve>],
    points: &[[f64; 3]],
    tolerance: f64,
) -> Result<Body> {
    let curves = chains
        .iter()
        .map(Vec::len)
        .fold(0usize, usize::saturating_add);
    if chains.is_empty()
        || chains.iter().any(Vec::is_empty)
        || curves > MAX_CURVES
        || !tolerance_ok(tolerance)
        || points.is_empty()
        || points.len() > MAX_PICKS
        || points.iter().flatten().any(|x| !x.is_finite())
    {
        return Err(Error::Argument);
    }
    let (cut, lines, pick_tolerance) = run_on("trim_by_dropped_curves", &[("brep", body)], |k| {
        cut_dropped(k, body, chains, tolerance)
    })??;
    let lines: HashSet<EdgeId> = lines.into_iter().collect();
    keep_picked(Body::new(cut), &lines, points, pick_tolerance)
}

/// [`trim_by_dropped_curves`]'s cut ([`DroppedCut`]): every face of a copy of `body` cut along
/// each chain dropped onto it, a face (or a piece of one) a chain bounds nothing on put back as
/// it was. The copy's own edges are tagged first ([`history::tag_edges`]); the cut lines are
/// the edges without the tag once every chain is merged, and the tags are then removed.
fn cut_dropped(
    k: &mut Kernel,
    body: &Body,
    chains: &[Vec<Curve>],
    tolerance: f64,
) -> solidcore::Result<Result<DroppedCut>> {
    let mut given = Vec::new();
    for chain in chains {
        match kernel_curves(k, chain) {
            Ok(c) => given.push(c),
            Err(e) => return Ok(Err(e)),
        }
    }
    let mut a = {
        let g = body.lock();
        brep_copy(k, &g)?
    };
    if let Err(e) = history::tag_edges(&mut a) {
        return Ok(Err(e));
    }
    // the copy's faces as they are before any cut
    let faces: Vec<FaceId> = a.face_list().to_vec();
    // whether each curve of each chain has landed on a face's parameter box, and whether each
    // chain has cut a face
    let mut landed: Vec<Vec<bool>> = given.iter().map(|chain| vec![false; chain.len()]).collect();
    let mut cuts = vec![false; given.len()];
    for f in faces {
        // every chain's drop onto the face, before a cut changes the face
        let mut drops = Vec::new();
        for (chain, seen) in given.iter().zip(&mut landed) {
            let pieces = match dropped_on(k, &a, f, chain, tolerance)? {
                Ok(pieces) => pieces,
                Err(e) => return Ok(Err(e)),
            };
            for (seen, of_curve) in seen.iter_mut().zip(&pieces) {
                *seen |= !of_curve.is_empty();
            }
            drops.push(pieces.into_iter().flatten().collect::<Vec<KCurve>>());
        }
        // the face and the pieces cut off it so far; the chains still to cut it
        let mut pieces = vec![f];
        let mut waiting: Vec<usize> = (0..drops.len())
            .filter(|&c| drops.get(c).is_some_and(|d| !d.is_empty()))
            .collect();
        // A chain that ends on another chain's line cuts only after that chain: the chains
        // that cut nothing are tried again while a pass still cuts with one of them.
        loop {
            let mut left = Vec::new();
            for &c in &waiting {
                let Some(dropped) = drops.get(c) else {
                    return Ok(Err(Error::Exception));
                };
                let mut cut_here = false;
                for g in pieces.clone() {
                    let merged = a.with_rollback(
                        |a| merge_on(k, a, g, dropped, tolerance),
                        |r| matches!(r, Ok(Ok(_))),
                    )?;
                    match merged {
                        Ok((made, _)) => {
                            pieces.extend(made);
                            cut_here = true;
                        }
                        Err(Error::CurveDoesNotCut) => {}
                        Err(e) => return Ok(Err(e)),
                    }
                }
                if cut_here {
                    if let Some(cut) = cuts.get_mut(c) {
                        *cut = true;
                    }
                } else {
                    left.push(c);
                }
            }
            let progress = left.len() < waiting.len();
            waiting = left;
            if !progress || waiting.is_empty() {
                break;
            }
        }
    }
    // A curve the kernel drops onto no face is no part of any cut. The kernel's drop gives no
    // curve for one that leaves a face's parameter box, which a curve across the seam of a
    // closed face does.
    if landed.iter().flatten().any(|seen| !seen) {
        return Ok(Err(Error::Unsupported(CURVE_LANDS_NOWHERE)));
    }
    // A chain that lands but bounds nothing anywhere: the cut asked for is not the one made.
    if cuts.contains(&false) {
        return Ok(Err(Error::CurveDoesNotCut));
    }
    let lines = history::untagged_edges(&a);
    if let Err(e) = history::clear(&mut a) {
        return Ok(Err(e));
    }
    let tol = a.tolerance();
    Ok(Ok((a, lines, tol)))
}

/// The region of record face `i`: the least face it is joined to (`joined` holds, for each
/// face, itself or a lesser face of its region).
fn region_of(joined: &[usize], mut i: usize) -> usize {
    while let Some(&j) = joined.get(i) {
        if j >= i {
            break;
        }
        i = j;
    }
    i
}

/// A copy of `cut` without the regions no point of `points` picks. Faces that meet at an edge
/// not in `lines` (the cut lines) are in one region; a point picks the region of the face
/// nearest it. A point whose two nearest regions are the same distance away within `tolerance`
/// picks neither: refused.
fn keep_picked(
    cut: Body,
    lines: &HashSet<EdgeId>,
    points: &[[f64; 3]],
    tolerance: f64,
) -> Result<Body> {
    let (record, order) = cut.to_record()?;
    let mut joined: Vec<usize> = (0..record.faces.len()).collect();
    for (e, id) in record.edges.iter().zip(&order.edges) {
        if lines.contains(id) {
            continue;
        }
        let mut uses = e.uses.iter().map(|u| u.face as usize);
        let Some(first) = uses.next() else {
            continue;
        };
        for other in uses {
            let (a, b) = (region_of(&joined, first), region_of(&joined, other));
            if let Some(slot) = joined.get_mut(a.max(b)) {
                *slot = a.min(b);
            }
        }
    }
    let regions: Vec<usize> = (0..joined.len()).map(|i| region_of(&joined, i)).collect();
    // for each point, its distance to each region: that of the region's nearest face
    let mut nearest: Vec<BTreeMap<usize, f64>> = points.iter().map(|_| BTreeMap::new()).collect();
    for (face, &region) in regions.iter().enumerate() {
        let distances = cut.face_distances(&order, face, points)?;
        for (row, d) in nearest.iter_mut().zip(distances) {
            let Some(d) = d.filter(|d| d.is_finite()) else {
                return Err(Error::Unsupported(
                    "the distance from a point to keep to a face of the cut body has no answer",
                ));
            };
            let least = row.entry(region).or_insert(d);
            if d < *least {
                *least = d;
            }
        }
    }
    let mut kept = HashSet::new();
    for row in &nearest {
        let mut by_distance: Vec<(f64, usize)> = row.iter().map(|(r, d)| (*d, *r)).collect();
        by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
        let Some(&(least, region)) = by_distance.first() else {
            return Err(Error::Exception);
        };
        if by_distance
            .get(1)
            .is_some_and(|(next, _)| are_same_to_tol(*next, least, tolerance))
        {
            return Err(Error::Unsupported(PICK_ON_A_CUT));
        }
        kept.insert(region);
    }
    let dropped: Vec<usize> = regions
        .iter()
        .enumerate()
        .filter_map(|(face, region)| (!kept.contains(region)).then_some(face))
        .collect();
    if dropped.is_empty() {
        // every region is picked: the cut body as it is
        return Ok(cut);
    }
    remove_faces(&cut, &order, &dropped)
}

/// Remove a connected feature whose two boundary loops need extensions that cross.
/// The returned solid and its history belong to a copy. `None` means the selection
/// did not use a patch trimmed at the intersection of its two extensions.
///
/// # Errors
/// Invalid face indices, a refused extension or a result that is not a closed solid.
pub fn remove_feature_with_crossing_patches(
    body: &Body,
    order: &Order,
    faces: &[usize],
) -> Result<Option<(Body, History)>> {
    if faces.is_empty() || faces.len() > MAX_FACES {
        return Err(Error::Argument);
    }
    let (result, mapped) = body.copy_with_order(order)?;
    let picked = faces
        .iter()
        .map(|&i| order::face(&mapped, i))
        .collect::<Result<Vec<_>>>()?;
    let selection: HashSet<_> = picked.iter().copied().collect();
    if selection.len() != picked.len() {
        return Err(Error::Argument);
    }
    let mut trimmed = false;
    let history = history::recorded(&result, &mapped, Made::Plain, || {
        trimmed = with_body("remove_feature_with_crossing_patches", &result, |k, b| {
            let mut executive = FeatureExecutive::new(k, b)?;
            let Some(i) = executive.add_feature(
                k,
                &picked,
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::FromOtherBrep,
            )?
            else {
                return Ok(false);
            };
            // add_feature starts at the first face. A second disconnected selection
            // must not be silently left behind.
            if !executive.features().get(i).is_some_and(|feature| {
                feature.loops.len() == 2
                    && feature.faces.len() == selection.len()
                    && feature.faces.iter().all(|face| selection.contains(face))
            }) {
                return Ok(false);
            }
            Ok(executive.do_defeature(k)?.is_empty()
                && executive
                    .features()
                    .get(i)
                    .is_some_and(|feature| feature.trimmed_crossing_base_patches))
        })?;
        Ok(())
    })?;
    if !trimmed {
        return Ok(None);
    }
    // Recording materializes UV trims. Check the solid the caller will receive.
    let (solid, proof) = run_taking("finish_crossing_patches", &[("brep", &result)], |k| {
        finish_proved(k, result.take_held().0, Expect::SOLID)
    })??;
    Ok(Some((Body::held(solid, proof), history)))
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
    validate_face_indices(order, faces)?;
    let picked = faces
        .iter()
        .map(|&i| order::face(order, i))
        .collect::<Result<Vec<_>>>()?;
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

fn validate_face_indices(order: &Order, faces: &[usize]) -> Result<()> {
    if faces.is_empty() || faces.len() > MAX_FACES {
        return Err(Error::Argument);
    }
    let mut seen = HashSet::new();
    if faces
        .iter()
        .any(|&i| i >= order.faces.len() || !seen.insert(i))
    {
        return Err(Error::Argument);
    }
    Ok(())
}

/// Remove a selected groove whose two rims the kernel can close together. Returns the new
/// solid and its face and edge history in record order. If the two annular supports cannot
/// be identified, the selection gives `None` so another removal can try it.
/// The operand is unchanged, including when the kernel refuses the construction.
///
/// # Errors
/// Invalid face indices or repeated picks, the kernel's construction errors, or a result
/// that is not a valid outward solid.
pub fn remove_ring_groove(
    body: &Body,
    order: &Order,
    faces: &[usize],
) -> Result<Option<(Body, History)>> {
    validate_face_indices(order, faces)?;
    let (result, mapped) = body.copy_with_order(order)?;
    let picked = faces
        .iter()
        .map(|&i| order::face(&mapped, i))
        .collect::<Result<Vec<_>>>()?;
    let history = history::recorded_if_changed(&result, &mapped, Made::Plain, || {
        with_body("remove_ring_groove", &result, |k, b| {
            remove_selected_ring(k, b, &picked)
        })
    })?;
    let Some(history) = history else {
        return Ok(None);
    };
    // Recording materializes missing UV trims and clears the history tags.
    // Prove the resulting B-rep after those edits and return it with that proof.
    let (solid, proof) = run_taking("finish_ring_groove", &[("brep", &result)], |k| {
        finish_proved(k, result.take_held().0, Expect::SOLID)
    })??;
    Ok(Some((Body::held(solid, proof), history)))
}

/// Try an annular closure on the selected faces of the copy.
pub(crate) fn remove_selected_ring(
    k: &mut Kernel,
    b: &mut Brep,
    picked: &[FaceId],
) -> solidcore::Result<bool> {
    match recognize_ring_groove(k, b, picked) {
        Ok(Some(mut groove)) => Ok(groove.do_defeature(k)?.is_empty()),
        Err(e)
            if e.is_unported_or_cancelled()
                || matches!(e, SolidError::PreparationRequired { .. }) =>
        {
            Err(e)
        }
        // A failed probe says nothing about whether an ordinary fillet chain can come off.
        Ok(None) | Err(_) => Ok(false),
    }
}

fn recognize_ring_groove<'a>(
    k: &mut Kernel,
    b: &'a mut Brep,
    picked: &[FaceId],
) -> solidcore::Result<Option<FeatureExecutive<'a>>> {
    let mut feature = FeatureExecutive::new(k, b)?;
    let Some(i) = feature.add_feature(
        k,
        picked,
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::FromOtherBrep,
    )?
    else {
        return Ok(None);
    };
    let selection: HashSet<_> = picked.iter().copied().collect();
    // add_feature partitions from the first selected face. Never accept a subset
    // of a disconnected selection as the requested groove.
    if !feature.features().get(i).is_some_and(|f| {
        f.faces.len() == selection.len() && f.faces.iter().all(|face| selection.contains(face))
    }) || feature.annular_supports(k, i)?.is_none()
    {
        return Ok(None);
    }
    Ok(Some(feature))
}
