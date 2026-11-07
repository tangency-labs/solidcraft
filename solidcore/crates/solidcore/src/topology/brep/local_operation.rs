//! `SmBrep::LocalOperation`: replaces the surfaces of some faces of a B-rep by given
//! ones, extending or trimming the faces around them to meet the new surfaces.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp` (`SmBrep::LocalOperation`),
//! `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust                | Upstream                  |
//! |---------------------|---------------------------|
//! | [`local_operation`] | `SmBrep::LocalOperation`  |
//! | [`local_operation_simple`] | (not upstream) `SmBrep::LocalOperation` with its own notification identities |
//!
//! Deviations: the offset executive's local-operation geometry creator
//! (`SmLocalOperationGeometryCreation`) is the face and surface lists the executive
//! holds ([`OffsetExecutive::set_local_surfaces`]). The executive's notifications are
//! the caller's ([`OffsetNotifications`]). Upstream removes every face of the B-rep and
//! copies the faces of the offset B-rep in with a merge (`RemoveFaces`, `CopyFaces`), as
//! here; the copy notifies through an arena-only store. (not upstream) The B-rep's own
//! attributes ([`Brep::add_attribute`]) are lent to the offset under the caller's
//! original identities, the offset B-rep takes what the offset leaves on its objects,
//! and the copy back gives them to the B-rep (SMLib's Copy handler), so each face keeps
//! its attributes as upstream's do. `SmTemporaryChangeValue` of
//! `m_bEditingEnabled` is a set and restore of the flag. The debug drawing code is omitted. No
//! recursion is introduced here. Opt-in (WR-534,
//! `Kernel::fixes.local_operation_box_holds_new_surfaces`, the fork's
//! tangency-labs/smlib@a1ee273): the box that sizes the executive's offset distance also
//! holds the replacement surfaces. (not upstream) The B-rep's cached topology fields are
//! refreshed before the operation returns ([`Brep::refresh_cached_topology`]), as every
//! public operation leaves them.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::offset::OffsetFailure;
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::knots::Side;
use crate::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use crate::offset::offset_executive::p2::OffsetNotifications;
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::surface::implicit::{self, AnalyticSupport};
use crate::surface::plane::Plane;
use crate::surface::surface::Surface;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p2;
use crate::topology::brep::p5;
use crate::topology::brep::validation::{self, FailureClass};
use crate::topology::face::p2 as face_ops;
use crate::topology::{Brep, FaceId};

pub use crate::offset::offset_executive::p2::local_operation_simple;

const OP: &str = "topology::brep::local_operation::local_operation";

fn null() -> SolidError {
    SolidError::null_pointer(OP)
}

/// `SmBrep::LocalOperation(rFaces, rNewSurfaces)`: `surfaces[i]` becomes the surface of
/// `faces[i]` (turned about if its normal opposes the old one's), every other face
/// keeping a copy of its surface (as an analytic one when possible), and the faces are
/// rebuilt by an extended offset that extends the surfaces to intersect. (Not upstream) a
/// face whose B-spline surface has a pole at a vertex of a moved face, and lies on a plane,
/// continues as that plane (`continued_planes`).
///
/// To find where `surfaces[i]` went, mark `faces[i]` before the call
/// ([`Brep::add_attribute`], [`Behavior::Copy`](crate::geom::attribute::Behavior::Copy))
/// and look for the mark on the result's faces: upstream callers mark the `SmSurface`
/// itself, but the port names a surface by its owning face, and `faces[i]`'s offset face
/// takes `surfaces[i]`, so the mark travels with it as upstream's does with the surface
/// (tangency-labs/solidcore#1875; the one difference, a merge keeping an unpicked
/// survivor, is `plan/findings/topology.brep_local_operation--1875-face-mark-for-surface-mark.md`).
///
/// # Errors
///
/// The extended offset's own error, its status and reason, when it fails (not upstream,
/// which returns `SM_ERR`); `InvalidInput` where upstream dereferences
/// null (a surface list shorter than the face list, a face without surface);
/// [`OffsetFailure::SeamOpened`] (not upstream) when a face that meets itself across
/// edges (seams) is given a surface its seams' two UV lines do not lift to one curve on
/// (not closed in the seam's direction, or the lines not on its seam: `keeps_seams`),
/// before anything changes; [`OffsetFailure::Unsound`] (not upstream) when the body built
/// fails what `b` did not (`refuse_unsound`: a check failure class `b`'s check has fewer
/// lines of, a geometry the check could not reach, a manifold solid lost, a volume not
/// positive); those of the callees ([`crate::offset::offset_executive::p2`], the surfaces'
/// normals, reversal and analytic copy, the bounding box). On every error `b` is as it
/// came in, the caches its surfaces had included (the operation edits a copy of it), and
/// so are `notifications.records` and `notifications.attributes`; a panic puts them all
/// back before it goes on.
pub fn local_operation(
    k: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    surfaces: Vec<Surface>,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
    if surfaces.len() < faces.len() {
        return Err(null());
    }
    // (not upstream) a refusal or a failure leaves `b` as it came in, the caches its
    // surfaces had included (the operation runs on its copy), and the caller's
    // notification stores as they were; a panic puts them back too before it goes on, as
    // `with_rollback_untouched` puts the body back
    let records = notifications.records.clone();
    let attributes = notifications.attributes.clone();
    let done = catch_unwind(AssertUnwindSafe(|| {
        b.with_rollback_untouched(
            |b| local_operation_on(k, b, faces, surfaces, &mut *notifications),
            Result::is_ok,
        )
    }));
    match done {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            *notifications.records = records;
            *notifications.attributes = attributes;
            Err(e)
        }
        Err(panic) => {
            *notifications.records = records;
            *notifications.attributes = attributes;
            resume_unwind(panic)
        }
    }
}

/// [`local_operation`] on the copy of `b` it edits.
fn local_operation_on(
    k: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    surfaces: Vec<Surface>,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    // (not upstream) the input's standing, measured once before anything changes: the
    // final gate (`refuse_unsound`) holds the operation to what it adds
    let input = validation::standing(k, &mut b.clone_standing_for_self()?)?;
    // Make a new list of all surfaces in this Brep: for every face, the new surface if it
    // is in the list to be replaced (turned to the original's side), else a copy of its own.
    let all_faces = bp1::faces(b, None)?;
    let mut given: Vec<Option<Surface>> = surfaces.into_iter().map(Some).collect();
    let mut new_list = Vec::new();
    // (WR-534) where each given surface went in the new list
    let mut placed: Vec<Option<usize>> = vec![None; given.len()];
    // (not upstream) the faces whose surfaces move: a face given its own surface again keeps
    // a copy of its own surface, as the faces not given one do, and so keeps its edges
    let mut moved = Vec::new();
    // (not upstream) the faces given another surface, and the planes faces that keep
    // theirs continue as beside them (`continued_planes`)
    let mut moving = Vec::new();
    for &face in &all_faces {
        let index = faces.iter().position(|&f| f == face);
        let own = face_ops::surface(b, face)?.ok_or_else(null)?;
        if let Some(new) = index.and_then(|i| given.get(i)).and_then(Option::as_ref) {
            if !is_own_surface(own, new) {
                moving.push(face);
            }
        }
    }
    let mut continued = continued_planes(k, b, &all_faces, &moving)?;
    for &face in &all_faces {
        let index = faces.iter().position(|&f| f == face);
        let own = face_ops::surface(b, face)?.ok_or_else(null)?;
        let replacement = match index {
            Some(index) if moving.contains(&face) => {
                if let Some(slot) = placed.get_mut(index) {
                    *slot = Some(new_list.len());
                }
                Some(
                    given
                        .get_mut(index)
                        .and_then(Option::take)
                        .ok_or_else(null)?,
                )
            }
            _ => continued.remove(&face),
        };
        let surface = if let Some(mut new) = replacement {
            moved.push(face);
            let domain = new.natural_uv_domain_if_built();
            // (`EvaluateNormal(uv, TRUE, TRUE, ..)`: from the left)
            let new_normal =
                new.evaluate_normal(k, domain.evaluate(0.5, 0.5), Side::Left, Side::Left)?;
            let old = own;
            let old_normal = old.evaluate_normal(
                k,
                b.face(face)?.uv_domain.evaluate(0.5, 0.5),
                Side::Left,
                Side::Left,
            )?;
            // when surface normals oppose - reverse the input surface's orientation
            if old_normal.dot(new_normal) < 0.0 {
                new.reverse(k, SurfParamType::U)?;
            }
            // (not upstream) a face that meets itself across seams keeps its UV loops on
            // the surface it takes (turned as it will be): each seam's two lines must lift
            // to one curve there, or it has no edge: refused before anything is changed
            if !keeps_seams(k, b, face, &new)? {
                return Err(Failure::new(OP, OffsetFailure::SeamOpened { face }).into_error());
            }
            new
        } else {
            // copy the face's current surface, when possible as an analytic one
            own.copy_and_add_analytics(k)?
        };
        new_list.push(surface);
    }

    // the Brep's bounding sphere gives the executive an offset distance it does not use
    let mut bbox = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
    // Not upstream (WR-534), opt-in `local_operation_box_holds_new_surfaces` (the fork's
    // tangency-labs/smlib@a1ee273): the box also holds the replacement surfaces. Their
    // faces' edges are found by extending the neighbouring surfaces to meet them, by a
    // distance sized from this box: sized from the original body alone, a face moved
    // further than about its size away left the extended neighbours short of it.
    if k.fixes.local_operation_box_holds_new_surfaces {
        for (j, slot) in placed.iter().enumerate() {
            let surface = match (given.get(j).and_then(Option::as_ref), slot) {
                (Some(surface), _) => surface,
                (None, Some(at)) => new_list.get(*at).ok_or_else(null)?,
                (None, None) => continue,
            };
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            let surf_box = surface
                .calculate_bounding_box(k, &domain, [true, false, false], true)?
                .normal_box
                .ok_or_else(null)?;
            bbox = bbox.union(&surf_box);
        }
    }
    let radius = bbox.sphere_bound().radius;
    let tolerance = b.tolerance();
    let mut executive = OffsetExecutive::new(
        OffsetOperation::LocalOperation,
        radius / 2.0,
        tolerance / 2.0,
    );
    // (the geometry creator's constructor tolerance, half a degree under WR-666's opt-in)
    executive.use_creator_tangency_tol(k);
    // A face's surface is extended as far as its neighbours' new surfaces meet it, and as
    // far again as they moved beside it (not upstream, whose margin is five half-radii of
    // the sphere bounding the box: `local_extension_reach`). The edges whose faces and
    // vertices' faces all keep their surfaces are kept as they are, not intersected again
    // (`OffsetExecutive::keeps_edge`).
    executive.set_local_surfaces(&all_faces, new_list, &moved);
    // the shell faces get copied without being offset: none
    executive.set_shell_faces(&[]);
    // extend/intersect to fill new convex edge gaps rather than adding a fillet
    executive.set_extend_convex_edges(true);

    // (not upstream) `b`'s own attributes go to the offset under the original
    // identities, as upstream's live on the faces it copies (`SM_NO_COPY` of each
    // source face); the caller's store does not keep them afterwards
    let seeded: BTreeSet<ObjectKey> = b
        .attributed_entities()
        .into_iter()
        .map(|e| (notifications.original_key_of)(e).key)
        .collect();
    p2::seed_brep_attributes(
        notifications.attributes,
        b.attribute_store().clone(),
        notifications.original_key_of,
    )?;

    // Build a new Brep by merging in all the faces with their new surfaces; (not upstream,
    // whose status is then `SM_ERR`) the offset's error is the operation's, with its own
    // status and reason, as the reach's and the final gate's refusals are
    let offset = executive.do_extended_offset(k, Some(&mut *b), notifications);
    drop(executive);
    let new_brep = offset.and_then(|mut new_brep| {
        // the offset B-rep's objects carry what the offset gave them
        p2::restore_keyed_attributes(
            &mut new_brep,
            notifications.attributes,
            notifications.result_key_of,
        )?;
        Ok(new_brep)
    });
    if !seeded.is_empty() {
        notifications
            .attributes
            .retain_rekeyed(&|key| (!seeded.contains(&key)).then_some(key))?;
    }
    let mut new_brep = new_brep?;

    // remove all faces from this Brep, then move all faces of the new Brep into it
    // (`CopyFaces` with a merge, which also grows this Brep's tolerance to the new one's);
    // (not upstream) a body that is not sound, or a failed move, is refused
    // (`local_operation` puts `b` back)
    let editing = b.editing_enabled();
    b.set_editing_enabled(true);
    let moved = move_faces(k, b, &mut new_brep);
    b.set_editing_enabled(editing);
    // (not upstream: the port's cached topology fields, true again for the caller)
    b.refresh_cached_topology();
    moved?;
    refuse_unsound(k, b, &input)
}

/// (not upstream) The local operation's final gate: the body it built is refused when it
/// fails what its input did not ([`validation::soundness`] of both, `input` measured
/// before the edit, the body on a private copy):
///
/// - the check at `SM_LEVEL_2` has more failure lines of some class (the failed test and
///   the type of the object it is about, [`FailureClass`]) than the input's check had
///   ([`BrepCheck::failures_beyond`](validation::BrepCheck::failures_beyond));
/// - the body's pointer check failed, so its geometry was not checked (`AssertValid` is
///   skipped then), even where the input's pointer check failed alike;
/// - the input was a manifold solid and the body is not;
/// - the body is a manifold solid whose volume is not positive (the input's volume is
///   not measured).
///
/// The owners the check tests the geometry against are the identities the body's naming
/// scheme names its entities by ([`validation::standing`]), as `move_faces` sets them.
///
/// Failures are compared by class, not by entity: the operation rebuilds every face (the
/// extended offset builds them all, unmoved ones on copies of their surfaces, and they
/// are copied into `b`), so no identity persists to compare by. A failure the input had
/// and the body still has is the input's; one the input had on a face the operation
/// rebuilt and the body no longer has is gone; a class the body has more of is the
/// operation's. (A failure gone from one entity while one of its class appears on
/// another is counted as the input's.)
///
/// Each moved edge is found again where extended surfaces meet; where that cannot close
/// the body (a surface that meets its neighbours nowhere near the old edges, a vertex
/// whose faces no longer meet in one point), the merge closes what is left by growing
/// tolerances, and the body it leaves is no solid: refused, not returned.
///
/// # Errors
///
/// [`OffsetFailure::Unsound`] naming what the body fails; those of the copy and the
/// measurement.
fn refuse_unsound(k: &mut Kernel, b: &Brep, input: &validation::Standing) -> Result<()> {
    use crate::error::offset::Unsoundness;
    let mut copy = b.clone_standing_for_self()?;
    let found = validation::standing(k, &mut copy)?;
    let added = found.check.failures_beyond(&input.check);
    let what = if let Some(&(class, _)) = added.first() {
        Unsoundness::Check {
            failures: added.iter().fold(0, |n, &(_, more)| n.saturating_add(more)),
            first: match class {
                FailureClass::Report {
                    reporting_type,
                    test_index,
                    ..
                } => Some((reporting_type, test_index)),
                _ => None,
            },
        }
    } else if found.check.assert_valid.is_none() {
        // the pointer check failed, so `AssertValid` did not run: the body's geometry is
        // not checked at all, whatever the input's pointer failures were
        Unsoundness::GeometryUnchecked
    } else if let Some(lost) = validation::lost_manifold(found.manifold, || Ok(input.manifold))? {
        lost
    } else if found.manifold
        && validation::volume(k, &mut copy)?.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
    {
        Unsoundness::VolumeNotPositive
    } else {
        return Ok(());
    };
    Err(Failure::new(OP, OffsetFailure::Unsound { what }).into_error())
}

/// (not upstream) The planes faces that keep their surfaces continue as: a face whose
/// B-spline surface has a pole (a side collapsed to a point) at a vertex of a moved face
/// (one of `moving`), and whose net lies on a plane ([`implicit::plane_of_net`],
/// certified to the rounding of its stored points), takes that plane, and moves with
/// the moved faces (its edges are intersected again, as those of a face given its own
/// plane are). The patch cannot be extended across its pole: a side's extension is a
/// strip of parameters whose width in space goes to nothing at the pole, so where the
/// moved faces split the vertex there (four faces at a pyramid's apex no longer meet in
/// one point, and two of them meet in a new edge), the split's vertices next to the pole
/// lie on no extension of it, and its edges to them stopped where they left the strip:
/// a vertex too many, and edges on one face alone. The plane holds them. A face that
/// meets itself across an edge (a seam), whose surface is closed, is left as it is.
///
/// # Errors
///
/// Those of the topology queries, [`crate::topology::vertexuse::is_on_pole`] and
/// [`implicit::plane_of_net`].
fn continued_planes(
    k: &Kernel,
    b: &mut Brep,
    all_faces: &[FaceId],
    moving: &[FaceId],
) -> Result<BTreeMap<FaceId, Surface>> {
    use crate::topology::{face::p4 as face4, vertex, vertexuse};
    let mut out = BTreeMap::new();
    for &face in all_faces {
        if moving.contains(&face)
            || !matches!(
                face_ops::surface(b, face)?.map(Surface::kind),
                Some(SurfaceKind::BSpline(_))
            )
            || meets_itself(b, face)?
        {
            continue;
        }
        let mut pole = false;
        for v in face4::vertices(b, face, None, false, None)? {
            if pole || !vertex::faces(b, v)?.iter().any(|f| moving.contains(f)) {
                continue;
            }
            for vu in vertex::vertexuses_of_face(b, v, Some(face), None)? {
                pole |= vertexuse::is_on_pole(k, b, vu)?;
            }
        }
        if !pole {
            continue;
        }
        let Some(SurfaceKind::BSpline(form)) = face_ops::surface(b, face)?.map(Surface::kind)
        else {
            continue;
        };
        if let Some(implicit::Implicit::Plane { origin, normal }) = implicit::plane_of_net(form)? {
            out.insert(face, Surface::from(Plane::from_normal(origin, normal)));
        }
    }
    Ok(out)
}

/// Whether `face` meets itself across one of its edges: more edge uses of the edge on
/// the face than its two sides' (a seam of a closed surface, where the face continues
/// round onto itself). A moved face's edges are found again by intersecting the new
/// surfaces of the faces on their two sides; across such an edge both sides are the
/// face itself, so an open surface (a plane) has no curve there: before this was refused,
/// the edge closed onto one vertex grown to the gap (13 units on fc0016's cylinder face
/// given its tangent plane), the shell stayed open, and the body kept was the wrong side
/// (manifold, volume -877.6).
fn meets_itself(b: &Brep, face: FaceId) -> Result<bool> {
    use crate::topology::edgeuse::p1 as eu1;
    for e in crate::topology::face::p4::edges(b, face, None, false, None)? {
        let mut on_face = 0_usize;
        for u in crate::topology::edge::edgeuses(b, e, None, None)? {
            if eu1::face(b, u)? == Some(face) {
                on_face = on_face.saturating_add(1);
            }
        }
        // (each side of the face has its own edge use: two for an ordinary edge)
        if on_face > 2 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Which end of the new surface's domain, in the seam's direction, a seam use's UV line
/// lies on ([`keeps_seams`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeamEnd {
    /// The domain's low end (`u = umin` for a seam in u).
    Low,
    /// The domain's high end.
    High,
}

/// A seam use's UV trim curve read exactly ([`uv_line`]): its control points (Euclidean
/// UV) and its two ends.
struct UvLine {
    points: Vec<crate::geom::vector2d::Point2>,
    ends: [crate::geom::vector2d::Point2; 2],
}

/// The rounding a UV coordinate is compared with a domain end to, at the parameter scale
/// `scale`: the two are one value computed two ways (the old surface's UV loops, the new
/// surface's domain), each within a few roundings of the scale, and a rational line's
/// coordinate is a quotient, one rounding more. A cylinder one rounding wider (`2π r` one
/// ulp off `2π`) is within it; a line a period or a part of one away is not.
fn uv_rounding(scale: f64) -> f64 {
    crate::nurbs::math::gamma(4) * scale
}

/// The coordinate of `p` in the seam's direction (`in_u`), or across it (`!in_u`).
fn coordinate(p: crate::geom::vector2d::Point2, in_u: bool) -> f64 {
    if in_u {
        p.x
    } else {
        p.y
    }
}

/// `curve` (a UV trim curve) as its control polygon and ends, or `None` for a kind without
/// one (a B-spline's and a line's NURB have one) or a weight not positive: a curve with
/// positive weights lies in its control points' hull, so a coordinate all of them share
/// to rounding is the curve's own.
///
/// # Errors
///
/// Those of the line's NURB and of the end evaluations.
fn uv_line(k: &Kernel, curve: &crate::curve::curve::Curve) -> Result<Option<UvLine>> {
    use crate::curve::curve::CurveKind;
    use crate::geom::vector2d::Point2;
    use crate::nurbs::math::NOW;
    let line_base;
    let nurb = match curve.kind() {
        CurveKind::BSpline(c) => c.gw_nurb(),
        CurveKind::Line(c) => {
            line_base = c.base_with_nurb()?;
            line_base.gw_nurb()
        }
        _ => None,
    };
    let Some(nurb) = nurb else {
        return Ok(None);
    };
    let mut points = Vec::new();
    for c in nurb.cpts() {
        if c.w == NOW {
            points.push(Point2::new(c.x, c.y));
        } else if c.w > 0.0 {
            points.push(Point2::new(c.x / c.w, c.y / c.w));
        } else {
            return Ok(None);
        }
    }
    let interval = curve.natural_interval();
    let a = curve.evaluate_point(k, interval.min())?;
    let b = curve.evaluate_point(k, interval.max())?;
    Ok(Some(UvLine {
        points,
        ends: [Point2::new(a.x, a.y), Point2::new(b.x, b.y)],
    }))
}

/// The end of `[low, high]` every control point of `line` lies on in the seam's direction
/// (`in_u`), to [`uv_rounding`]; `None` when it is neither.
fn seam_end(line: &UvLine, in_u: bool, low: f64, high: f64) -> Option<SeamEnd> {
    let r = uv_rounding(low.abs().max(high.abs()).max((high - low).abs()));
    let on = |end: f64| {
        line.points
            .iter()
            .all(|&p| (coordinate(p, in_u) - end).abs() <= r)
    };
    if on(low) {
        Some(SeamEnd::Low)
    } else if on(high) {
        Some(SeamEnd::High)
    } else {
        None
    }
}

/// The interval `line` covers across the seam (`in_u` the seam's direction): its ends'
/// coordinates, when every control point lies between them to `r` (the hull then holds
/// the curve's image there, and it reaches both ends, so the image is that interval);
/// `None` otherwise.
fn line_extent(line: &UvLine, in_u: bool, r: f64) -> Option<(f64, f64)> {
    let [a, b] = line.ends.map(|p| coordinate(p, !in_u));
    let (lo, hi) = (a.min(b), a.max(b));
    line.points
        .iter()
        .all(|&p| {
            let c = coordinate(p, !in_u);
            c >= lo - r && c <= hi + r
        })
        .then_some((lo, hi))
}

/// A certified bound on the distance between the surface's two boundary iso curves in
/// the seam's direction (`in_u`), `u = umin` and `u = umax` at each `v` for a seam in u,
/// over `domain` (the surface's natural domain), or `None` when there is no certificate:
/// - a surface evaluated through a B-spline net (a B-spline, a surface of revolution,
///   sphere, torus, extrusion, ball patch; a procedural blend's net within its bound of
///   its definition): the net clamped in that direction at the domain's ends, so its
///   boundary isos are its first and last columns, each pair of those columns' points of
///   one positive weight. Then `C_lo(v) - C_hi(v) = sum_j R_j(v) (P_j - Q_j)` with `R_j`
///   the one rational basis of both, non-negative and summing to one, so the gap is at
///   most `max_j |P_j - Q_j|` (plus twice a blend's bound);
/// - a cone or cylinder ([`cone_seam_gap`]).
///
/// A plane is never closed; a curve-bounded, STEP, offset or in-volume surface has no
/// certificate here. Neither is accepted.
///
/// # Errors
///
/// Those of the surface's NURB form and evaluations.
fn seam_gap(
    k: &Kernel,
    surface: &Surface,
    in_u: bool,
    domain: &crate::geom::extent2d::Extent2d,
) -> Result<Option<crate::geom::certified::Bounded>> {
    use crate::geom::certified::{distance, gamma, Bounded};
    use crate::geom::vector3d::Point3;
    use crate::nurbs::math::{CPoint, NOW};
    let blend = match surface.kind_with_nurb(k)? {
        SurfaceKind::Plane(_)
        | SurfaceKind::CurveBounded(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::OffsetSurface(_)
        | SurfaceKind::SrfInVolume(_) => return Ok(None),
        SurfaceKind::Cone(c) => return cone_seam_gap(k, surface, c, in_u, domain),
        SurfaceKind::ProceduralBlend(pb) => pb.def.bound,
        SurfaceKind::BSpline(_)
        | SurfaceKind::BallPatch(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_) => 0.0,
    };
    let Some(form) = crate::blend::fillet_section::station::as_b_spline(k, surface)? else {
        return Ok(None);
    };
    let Some(net) = form.gw_nurb() else {
        return Ok(None);
    };
    let (p, q) = net.degrees();
    let (knots, degree, low, high) = if in_u {
        (net.u_knots(), p, domain.min.x, domain.max.x)
    } else {
        (net.v_knots(), q, domain.min.y, domain.max.y)
    };
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
        return Ok(None);
    };
    let clamped = knots.len() > degree.saturating_mul(2).saturating_add(1)
        && knots
            .iter()
            .take(degree.saturating_add(1))
            .all(|&t| t == first)
        && knots
            .iter()
            .rev()
            .take(degree.saturating_add(1))
            .all(|&t| t == last);
    if !clamped || first != low || last != high {
        return Ok(None);
    }
    let rows: Vec<&[CPoint]> = net.rows().collect();
    let pairs: Vec<(CPoint, CPoint)> = if in_u {
        let (Some(a), Some(b)) = (rows.first(), rows.last()) else {
            return Ok(None);
        };
        a.iter().copied().zip(b.iter().copied()).collect()
    } else {
        let mut pairs = Vec::new();
        for row in &rows {
            let (Some(&a), Some(&b)) = (row.first(), row.last()) else {
                return Ok(None);
            };
            pairs.push((a, b));
        }
        pairs
    };
    let euclid = |c: CPoint| {
        if c.w == NOW {
            Point3::new(c.x, c.y, c.z)
        } else {
            Point3::new(c.x / c.w, c.y / c.w, c.z / c.w)
        }
    };
    let (mut value, mut bound) = (0.0_f64, 0.0_f64);
    for (a, b) in pairs {
        let rational = a.w != NOW;
        if rational != (b.w != NOW) || (rational && !(a.w > 0.0 && a.w == b.w)) {
            return Ok(None);
        }
        let (pa, pb) = (euclid(a), euclid(b));
        let d = distance(pa, pb);
        // (a rational point is a quotient: each coordinate one rounding off)
        let quotient = if rational {
            gamma(1) * (pa.length() + pb.length())
        } else {
            0.0
        };
        value = value.max(d.value);
        bound = bound.max(d.bound + quotient);
    }
    Ok(Some(Bounded {
        value: value + 2.0 * blend,
        bound,
    }))
}

/// [`seam_gap`] of a cone or cylinder, evaluated by its STEP angle and height: the seam
/// must be its sweep coordinate (`u`, or `v` when its NURB's u is the linear direction).
/// Its point is `A + r(h) (cos θ X + sin θ Y) + h Z`, the angle `θ` a function of the
/// sweep parameter alone and the height `h` and radius `r(h)` affine in the other
/// (`cone_convert_uv_from_nurbs_to_step`), so the two rulings' gap at a height,
/// `|r(h)| 2 |sin(Δθ / 2)|`, is convex along them: its largest is at one of the domain's
/// two ends, the larger of the two corner distances. Each corner point is within
/// `gamma(16)` of its size (the angle's conversion, `cos` and `sin` at two roundings
/// each, the products and sums), added to the distance's own bound.
///
/// # Errors
///
/// Those of the evaluations.
fn cone_seam_gap(
    k: &Kernel,
    surface: &Surface,
    cone: &crate::surface::cone::Cone,
    in_u: bool,
    domain: &crate::geom::extent2d::Extent2d,
) -> Result<Option<crate::geom::certified::Bounded>> {
    use crate::geom::certified::{distance, gamma, Bounded};
    use crate::geom::vector2d::Point2;
    if in_u == cone.rev().swap_uv() {
        return Ok(None);
    }
    let (lo, hi) = (domain.min, domain.max);
    let corners = if in_u {
        [
            (Point2::new(lo.x, lo.y), Point2::new(hi.x, lo.y)),
            (Point2::new(lo.x, hi.y), Point2::new(hi.x, hi.y)),
        ]
    } else {
        [
            (Point2::new(lo.x, lo.y), Point2::new(lo.x, hi.y)),
            (Point2::new(hi.x, lo.y), Point2::new(hi.x, hi.y)),
        ]
    };
    let (mut value, mut bound) = (0.0_f64, 0.0_f64);
    for (a, b) in corners {
        let (p, q) = (surface.evaluate_point(k, a)?, surface.evaluate_point(k, b)?);
        let d = distance(p, q);
        value = value.max(d.value);
        bound = bound.max(d.bound + gamma(16) * (p.length() + q.length()));
    }
    Ok(Some(Bounded { value, bound }))
}

/// Whether `surface` keeps every seam `face` meets itself across (none for a face that
/// does not, [`meets_itself`]). The face's UV loops are copied onto the surface it takes,
/// so a seam's edge uses keep their UV lines, which on its own surface are the two ends
/// of the domain in the direction it is closed in (`u = 0` and `u = 2π` for a radius-1
/// cylinder's side). On the new surface the seam has an edge only where both lines lift
/// to one curve, decided exactly, without samples:
/// 1. every use's UV trim curve ([`uv_line`]) lies, by its control points, on one end of
///    the new surface's natural domain in one direction, to the parameters' rounding
///    ([`seam_end`]), and both ends are taken: the lines are the new surface's own seam
///    (a radius-0.5 cylinder's arc-length u of period π puts u = 2π off its domain, a
///    swapped cylinder puts the lines across its seam);
/// 2. the lines cover one interval across the seam ([`line_extent`]);
/// 3. the surface's two boundary isos there are within the edge's tolerance, by a
///    certified bound ([`seam_gap`]; no certificate refuses).
///
/// A use without a UV trim curve has one made on a scratch copy of `b` (as
/// `NormalizedEvaluate` makes it), so `b` is not changed. An edge whose uses all share one
/// UV line (a slit) lifts to one curve on any surface and is not a seam.
///
/// # Errors
///
/// Those of the topology queries, of the scratch copy and the UV trim curves, and of the
/// surface's NURB form and evaluations.
fn keeps_seams(k: &Kernel, b: &mut Brep, face: FaceId, surface: &Surface) -> Result<bool> {
    use crate::topology::edgeuse::{get_or_create_uv_trim_curve, p1 as eu1};
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let mut scratch: Option<Brep> = None;
    for e in crate::topology::face::p4::edges(b, face, None, false, None)? {
        let mut uses = Vec::new();
        for u in crate::topology::edge::edgeuses(b, e, None, None)? {
            if eu1::face(b, u)? == Some(face) {
                uses.push(u);
            }
        }
        // (each side of the face has its own edge use: two for an ordinary edge)
        if uses.len() <= 2 {
            continue;
        }
        let mut lines = Vec::new();
        for &u in &uses {
            let line = if let Some(c) = eu1::uv_trim_curve(b, u)?.0 {
                uv_line(k, c)?
            } else {
                if scratch.is_none() {
                    scratch = Some(b.clone_standing_for_self()?);
                }
                let Some(s) = scratch.as_mut() else {
                    return Err(null());
                };
                let _ = get_or_create_uv_trim_curve(k, s, u, false, false, true)?;
                match eu1::uv_trim_curve(s, u)?.0 {
                    Some(c) => uv_line(k, c)?,
                    None => None,
                }
            };
            let Some(line) = line else {
                return Ok(false);
            };
            lines.push(line);
        }
        let one_line = |a: &UvLine, c: &UvLine| {
            a.points == c.points || a.points.iter().rev().eq(c.points.iter())
        };
        if let Some(first) = lines.first() {
            if lines.iter().all(|l| one_line(first, l)) {
                continue;
            }
        }
        let tolerance = crate::topology::edge::tolerance(b, e)?;
        let mut kept = false;
        for in_u in [true, false] {
            let ((low, high), (across_lo, across_hi)) = if in_u {
                ((domain.min.x, domain.max.x), (domain.min.y, domain.max.y))
            } else {
                ((domain.min.y, domain.max.y), (domain.min.x, domain.max.x))
            };
            let Some(ends) = lines
                .iter()
                .map(|l| seam_end(l, in_u, low, high))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if !(ends.contains(&SeamEnd::Low) && ends.contains(&SeamEnd::High)) {
                continue;
            }
            let r = uv_rounding(
                across_lo
                    .abs()
                    .max(across_hi.abs())
                    .max((across_hi - across_lo).abs()),
            );
            let Some(extents) = lines
                .iter()
                .map(|l| line_extent(l, in_u, r))
                .collect::<Option<Vec<_>>>()
            else {
                return Ok(false);
            };
            let Some(&(lo, hi)) = extents.first() else {
                return Ok(false);
            };
            if !extents
                .iter()
                .all(|&(a, c)| (a - lo).abs() <= r && (c - hi).abs() <= r)
            {
                return Ok(false);
            }
            let Some(gap) = seam_gap(k, surface, in_u, &domain)? else {
                return Ok(false);
            };
            if !gap.at_most(tolerance).is_yes() {
                return Ok(false);
            }
            kept = true;
            break;
        }
        if !kept {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `given` is the surface `own` lies on, exactly: both of one analytic kind
/// ([`AnalyticSupport`]) with equal radii and semi-angle, and the same placement where it
/// matters, each comparison made without rounding (error-free products and sums,
/// [`exact`]):
/// - a plane: the plane `own` spans (its origin and its two axes) holds the given plane's
///   origin, and its two axes span the same plane;
/// - a cylinder: the same axis line (either sense);
/// - a cone: the same axis line and sense and the same origin (where its radius is the
///   base radius), or the opposite sense with the semi-angle negated;
/// - a sphere: the same centre;
/// - a torus: the same centre and axis line (either sense).
///
/// Not a tolerance: a surface off by a rounding is another surface, and its face moves.
/// A placement whose products fall out of the exact range (a subnormal coordinate, or a
/// product that could underflow: [`exact`]) is not shown the same either, and its face
/// moves to the given surface. The extent and parametrisation are not compared (a face's
/// surface is extended as its neighbours need, whatever its extent, and its own copy keeps
/// its own parameters).
fn is_own_surface(own: &Surface, given: &Surface) -> bool {
    use AnalyticSupport as A;
    let (Some(own), Some(given)) = (AnalyticSupport::of(own), AnalyticSupport::of(given)) else {
        return false;
    };
    match (own, given) {
        (A::Plane { placement: p, .. }, A::Plane { placement: q, .. }) => same_plane(&p, &q),
        (
            A::Cylinder {
                placement: p,
                radius: r,
            },
            A::Cylinder {
                placement: q,
                radius: s,
            },
        ) => r == s && same_axis(&p, &q),
        (
            A::Cone {
                placement: p,
                base_radius: r,
                semi_angle_deg: a,
            },
            A::Cone {
                placement: q,
                base_radius: s,
                semi_angle_deg: b,
            },
        ) => {
            // (the axes are parallel exactly, so their unit z axes' dot is +-1 to
            // rounding: its sign is the sense)
            let same_sense = p.z_axis().dot(q.z_axis()) > 0.0;
            r == s
                && (if same_sense { a == b } else { a == -b })
                && same_point(p.origin(), q.origin())
                && same_axis(&p, &q)
        }
        (
            A::Sphere {
                placement: p,
                radius: r,
            },
            A::Sphere {
                placement: q,
                radius: s,
            },
        ) => r == s && same_point(p.origin(), q.origin()),
        (
            A::Torus {
                placement: p,
                major_radius: r1,
                minor_radius: r2,
            },
            A::Torus {
                placement: q,
                major_radius: s1,
                minor_radius: s2,
            },
        ) => r1 == s1 && r2 == s2 && same_point(p.origin(), q.origin()) && same_axis(&p, &q),
        _ => false,
    }
}

fn same_point(a: Vec3, b: Vec3) -> bool {
    a.x == b.x && a.y == b.y && a.z == b.z
}

/// Whether `p`'s axes span a plane, exactly; `None` when a product is out of the exact
/// range ([`exact`]).
fn spans(p: &Axis2Placement) -> Option<bool> {
    Some(
        exact::cross(p.x_axis(), p.y_axis())?
            .iter()
            .any(|c| !exact::is_zero(c)),
    )
}

/// Whether the plane `p` spans (its origin and its two axes) is the one `q` spans: `q`'s
/// axes lie in `p`'s and its origin on it, each determinant exact. A product out of the
/// exact range ([`exact`]: a subnormal coordinate, or one that underflows) is not shown
/// the same: false.
fn same_plane(p: &Axis2Placement, q: &Axis2Placement) -> bool {
    let (a, b, o) = (p.x_axis(), p.y_axis(), p.origin());
    let (x, y, g) = (q.x_axis(), q.y_axis(), q.origin());
    let same = || {
        Some(
            spans(p)?
                && spans(q)?
                && exact::is_zero(&exact::det3(a, b, x)?)
                && exact::is_zero(&exact::det3(a, b, y)?)
                && exact::is_zero(&exact::sub(&exact::det3(a, b, g)?, &exact::det3(a, b, o)?)),
        )
    };
    same().unwrap_or(false)
}

/// Whether `p`'s z axis line (through its origin along `x × y`) is `q`'s, either sense:
/// `q`'s axis `x' × y'` is normal to `p`'s x and y axes (so parallel to `x × y`), and the
/// origins' difference is too, each product exact. A product out of the exact range
/// ([`exact`]) is not shown the same: false.
fn same_axis(p: &Axis2Placement, q: &Axis2Placement) -> bool {
    let (a, b, o) = (p.x_axis(), p.y_axis(), p.origin());
    let (x, y, g) = (q.x_axis(), q.y_axis(), q.origin());
    let same = || {
        Some(
            spans(p)?
                && spans(q)?
                && exact::is_zero(&exact::det3(x, y, a)?)
                && exact::is_zero(&exact::det3(x, y, b)?)
                && exact::is_zero(&exact::sub(&exact::dot(a, g)?, &exact::dot(a, o)?))
                && exact::is_zero(&exact::sub(&exact::dot(b, g)?, &exact::dot(b, o)?)),
        )
    };
    same().unwrap_or(false)
}

/// Error-free arithmetic on doubles: a value is an expansion, a sum of doubles whose
/// components do not overlap (Shewchuk's grow-expansion), so sums and products of doubles
/// are held without rounding and a value's sign is read off exactly (an expansion is zero
/// exactly when every component is).
///
/// A sum (`two_sum`) is error-free at any magnitude short of overflow, which leaves an
/// infinite or NaN component, never a zero. A product (`product`) is error-free only in
/// range: a product that could underflow, or of a subnormal or non-finite factor, is
/// `None`, and so is every value built from it. A caller reads `None` as not shown zero.
mod exact {
    use crate::math::Vec3;

    /// `2^-968`: the least rounded product of two normal doubles whose rounding error
    /// `fma(a, b, -a b)` is exact. The error is a multiple of `ulp(a) ulp(b) =
    /// 2^(ea + eb - 104)` (`ea`, `eb` the exponents) of at most 53 bits, so exact when
    /// that unit is not below the least subnormal `2^-1074`, `ea + eb >= -970`; a rounded
    /// product of at least `2^-968` has `|a b| > 2^-969` and, as `|a b| < 2^(ea + eb + 2)`,
    /// `ea + eb >= -970`.
    const LEAST_EXACT_PRODUCT: f64 = f64::MIN_POSITIVE * 18_014_398_509_481_984.0;

    /// `a + b` as `(s, e)`: `s = fl(a + b)` and its rounding error (Knuth's `TwoSum`).
    fn two_sum(a: f64, b: f64) -> (f64, f64) {
        let s = a + b;
        let bb = s - a;
        (s, (a - (s - bb)) + (b - bb))
    }

    /// `e + b` as an expansion (Grow-Expansion).
    fn grow(e: &[f64], b: f64) -> Vec<f64> {
        let mut q = b;
        let mut out = Vec::new();
        for &c in e {
            let (s, err) = two_sum(q, c);
            out.push(err);
            q = s;
        }
        out.push(q);
        out
    }

    /// `e + f` as an expansion.
    fn add(e: &[f64], f: &[f64]) -> Vec<f64> {
        f.iter().fold(e.to_vec(), |acc, &c| grow(&acc, c))
    }

    /// `e - f` as an expansion.
    pub(super) fn sub(e: &[f64], f: &[f64]) -> Vec<f64> {
        f.iter().fold(e.to_vec(), |acc, &c| grow(&acc, -c))
    }

    /// `a b` as an expansion: the product and its rounding error, `fma(a, b, -p)`; zero
    /// exactly when `a` or `b` is. `None` out of range: `a` or `b` subnormal or not
    /// finite, or the rounded product not finite or below [`LEAST_EXACT_PRODUCT`] (where
    /// the error, or the product itself, could underflow).
    fn product(a: f64, b: f64) -> Option<Vec<f64>> {
        if a == 0.0 || b == 0.0 {
            return (a.is_finite() && b.is_finite()).then(|| vec![0.0]);
        }
        let p = a * b;
        let in_range =
            a.is_normal() && b.is_normal() && p.is_finite() && p.abs() >= LEAST_EXACT_PRODUCT;
        in_range.then(|| vec![a.mul_add(b, -p), p])
    }

    /// `e b` as an expansion: each component's product, summed.
    fn scale(e: &[f64], b: f64) -> Option<Vec<f64>> {
        e.iter()
            .try_fold(Vec::new(), |acc, &c| Some(add(&acc, &product(c, b)?)))
    }

    /// `u × v`, each component an expansion.
    pub(super) fn cross(u: Vec3, v: Vec3) -> Option<[Vec<f64>; 3]> {
        Some([
            sub(&product(u.y, v.z)?, &product(u.z, v.y)?),
            sub(&product(u.z, v.x)?, &product(u.x, v.z)?),
            sub(&product(u.x, v.y)?, &product(u.y, v.x)?),
        ])
    }

    /// `u · v` as an expansion.
    pub(super) fn dot(u: Vec3, v: Vec3) -> Option<Vec<f64>> {
        Some(add(
            &add(&product(u.x, v.x)?, &product(u.y, v.y)?),
            &product(u.z, v.z)?,
        ))
    }

    /// `det(a, b, c) = (a × b) · c` as an expansion.
    pub(super) fn det3(a: Vec3, b: Vec3, c: Vec3) -> Option<Vec<f64>> {
        let [x, y, z] = cross(a, b)?;
        Some(add(
            &add(&scale(&x, c.x)?, &scale(&y, c.y)?),
            &scale(&z, c.z)?,
        ))
    }

    /// Whether the expansion is zero exactly.
    pub(super) fn is_zero(e: &[f64]) -> bool {
        e.iter().all(|&c| c == 0.0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_rounded_sum_is_not_zero_but_its_exact_one_is() {
            // 0.1 + 0.2 - 0.3 rounds to 5.6e-17; exactly it is that too (the doubles are
            // not the decimals), while 1e16 + 1 - 1e16 is 1, not the 0 or 2 of rounding
            assert!(!is_zero(&sub(&add(&[0.1], &[0.2]), &[0.3])));
            assert_eq!(sub(&add(&[1e16], &[1.0]), &[1e16]).iter().sum::<f64>(), 1.0);
            let (p, q) = (product(3.0, 0.1).unwrap(), product(0.1, 3.0).unwrap());
            assert!(is_zero(&sub(&p, &q)));
        }

        #[test]
        fn a_determinant_is_zero_exactly_only_for_dependent_vectors() {
            let a = Vec3::new(0.1, 0.7, 0.3);
            let b = Vec3::new(0.9, 0.2, 0.4);
            assert!(is_zero(&det3(a, b, a).unwrap()));
            let c = Vec3::new(0.1, 0.7, 0.3 + 1e-16 * 0.3);
            assert!(!is_zero(&det3(a, b, c).unwrap()));
        }

        /// A product whose rounding error is not a double is out of range: one that
        /// underflows (rounded to zero, its `fma` error zero too, so it read as zero), one
        /// of a subnormal factor, one that overflows. A zero factor's product is zero, and
        /// one just in range is exact.
        #[test]
        fn a_product_that_could_underflow_is_out_of_range() {
            assert_eq!(LEAST_EXACT_PRODUCT, 2.0_f64.powi(-968));
            assert!(product(1e-200, 1e-200).is_none());
            assert!(product(1e-160, 1e-160).is_none());
            assert!(product(1e-310, 1.0).is_none());
            assert!(product(1.0, -5e-324).is_none());
            assert!(product(f64::MAX, 2.0).is_none());
            assert!(product(f64::NAN, 1.0).is_none());
            assert_eq!(product(0.0, 1e-310), Some(vec![0.0]));
            let h = 2.0_f64.powi(-484);
            assert_eq!(product(h, h), Some(vec![0.0, LEAST_EXACT_PRODUCT]));
            assert!(product(1e-140, 1e-140).is_some());
            let z = Vec3::new(0.0, 0.0, 1e-310);
            assert!(det3(Vec3::X, Vec3::Y, z).is_none());
        }
    }
}

/// `RemoveFaces(GetFaces)` of `b`, then `pNewBrep->CopyFaces(GetFaces, b)`.
fn move_faces(k: &mut Kernel, b: &mut Brep, new_brep: &mut Brep) -> Result<()> {
    let old_faces = bp1::faces(b, None)?;
    p5::remove_faces(k, b, true, &old_faces)?;
    let new_faces = bp1::faces(new_brep, None)?;
    // (not upstream: upstream's surface owner is the face itself) the copied faces'
    // surfaces are owned under the identity `b`'s naming scheme names a face by when it
    // records none: the notification identity of a `Notify` B-rep, else the arena
    // identity (tangency-labs/solidcore#2047)
    let naming = b.naming();
    let face_ref = |face| crate::topology::scheme_owner_identity(naming, EntityRef::Face(face));
    // the arena entry carries no attribute records: the copy notifies through an
    // arena-only store (a default record per object of both B-reps) whose source
    // identities are distinct from `b`'s, holding both B-reps' attributes; `b` gets the
    // attributes its objects end with (the merge branch of `p7::copy_faces`)
    crate::boolean::merge::p3::copy_component(k, new_brep, &new_faces, b, &face_ref)?;
    // (not upstream, as for the surfaces) the copied edges' curves and edge uses' UV trim
    // curves are owned under the copy's notification identities; a B-rep not named by
    // them names its edges and edge uses by the arena identities: rebound to those, so
    // the body's geometry names the entities it lies on (`SmBrep` test #61, Bad Edge)
    crate::topology::brep::p3::rebind_geometry_owners(
        b,
        crate::topology::loopuse::notify_ref,
        |e| crate::topology::scheme_owner_identity(naming, e),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_primitives::create_box;
    use crate::surface::plane::Plane;
    use crate::surface::surface::SurfaceKind;

    use crate::geom::attribute::{AttributeStore, ObjectKey};
    use crate::geom::object::ObjectRef;
    use crate::topology::topology::Topology;

    /// Runs `f` with throwaway notification stores keyed by entity id, as callers with no
    /// identities of their own do.
    fn with_notifications<R>(
        b: &mut Brep,
        f: impl FnOnce(&mut Brep, &mut OffsetNotifications<'_>) -> R,
    ) -> R {
        fn key(entity: EntityRef, namespace: u64) -> ObjectRef {
            let (index, type_code) = match entity {
                EntityRef::Brep(id) => (id.index(), crate::geom::a_object::BREP_TYPE),
                EntityRef::Region(id) => (id.index(), crate::topology::Region::type_code()),
                EntityRef::Shell(id) => (id.index(), crate::topology::Shell::type_code()),
                EntityRef::Face(id) => (id.index(), crate::topology::face::p2::TYPE),
                EntityRef::Faceuse(id) => (id.index(), 16022),
                EntityRef::Loopuse(id) => (id.index(), crate::topology::loopuse::TYPE),
                EntityRef::Edge(id) => (id.index(), crate::topology::edge::TYPE),
                EntityRef::Edgeuse(id) => (id.index(), crate::topology::edgeuse::p1::TYPE),
                EntityRef::Vertex(id) => (id.index(), crate::topology::vertex::TYPE),
                EntityRef::Vertexuse(id) => (id.index(), crate::topology::vertexuse::TYPE),
                EntityRef::Loop(id) => (id.index(), crate::topology::topology::TYPE),
            };
            ObjectRef {
                key: ObjectKey((namespace << 60) | ((type_code as u64) << 32) | u64::from(index)),
                type_code,
            }
        }
        let mut records = crate::topology::topology::NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let mut add = |entity| {
            records.insert(key(entity, 1).key, Topology::new());
        };
        add(EntityRef::Brep(crate::topology::BrepId::from_index(0)));
        b.regions()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Region(id)));
        b.shells()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Shell(id)));
        b.faces()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Face(id)));
        b.faceuses()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Faceuse(id)));
        b.loopuses()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Loopuse(id)));
        b.edges()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Edge(id)));
        b.edgeuses()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Edgeuse(id)));
        b.vertices()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Vertex(id)));
        b.vertexuses()
            .iter()
            .for_each(|(id, _)| add(EntityRef::Vertexuse(id)));
        let result_brep = key(EntityRef::Brep(crate::topology::BrepId::from_index(0)), 3);
        records.insert(result_brep.key, Topology::new());
        let original = |e| key(e, 1);
        let result = |e| key(e, 3);
        let original_curve = |e: crate::topology::EdgeId, t: i64| ObjectRef {
            key: ObjectKey((5_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
            type_code: t,
        };
        let result_curve = |e: crate::topology::EdgeId, t: i64| ObjectRef {
            key: ObjectKey((4_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
            type_code: t,
        };
        let lists = |_: bool, region: crate::topology::RegionId| {
            [0_u64, 1].map(|list| ObjectRef {
                key: ObjectKey((6_u64 << 60) | (list << 32) | u64::from(region.index())),
                type_code: 0,
            })
        };
        let mut notifications = OffsetNotifications {
            records: &mut records,
            attributes: &mut attributes,
            original_key_of: &original,
            result_key_of: &result,
            original_curve_ref: &original_curve,
            result_curve_ref: &result_curve,
            boolean_source_lists: &lists,
            result_brep,
            merge_identities: None,
        };
        f(b, &mut notifications)
    }

    /// A body named by its arena identities (as an imported one is): the edges copied in
    /// from the offset own their curves under those identities, not the copy's
    /// notification ones, so the body passes its check under its own naming, and the
    /// operation returns it (the 20 mm box, its top raised to z = 25: 10000 mm³).
    #[test]
    fn an_arena_named_body_keeps_its_naming_on_the_edges_copied_in() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
        b.use_arena_naming().unwrap();
        let top = box_top(&k, &b);
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 25.0), Vec3::Z));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[top], vec![raised]);
        assert_eq!(why, None);
        let lines = validation::check_brep(
            &mut k,
            &mut b.clone_standing_for_self().unwrap(),
            validation::CheckLevel::Level2,
        )
        .unwrap()
        .lines();
        assert!(lines.is_empty(), "{lines:?}");
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v - 10000.0).abs() <= 1e-9 * 10000.0, "{v}");
    }

    /// Issue 2976: z = x - 1 cuts the two bottom edges y = 0 and y = 2 at
    /// (1, y, 0), away from their vertices. Those edges bound the bottom and a
    /// side, and all faces at their vertices are bottom/sides, never the top:
    /// the old vertex-neighbourhood rule therefore keeps both crossed edges.
    /// The solid is the triangular prism 1 <= x <= 2, 0 <= z <= x - 1,
    /// 0 <= y <= 2, with volume 2 * integral_1^2 (x - 1) dx = 1.
    #[test]
    fn a_tilted_top_reintersects_the_unmoved_bottom_edges_it_crosses() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let top = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(&b, f).unwrap().unwrap();
                let uv = s
                    .natural_uv_domain_with_nurb(&k)
                    .unwrap()
                    .evaluate(0.5, 0.5);
                (s.evaluate_point(&k, uv).unwrap().z - 2.0).abs() < 1e-9
            })
            .unwrap();
        let tilted = Surface::from(Plane::from_normal(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 1.0),
        ));
        let result = with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[top], vec![tilted], notes)
        });
        let (volume, manifold) = volume_and_manifold(&k, &mut b);
        eprintln!("issue 2976: result={result:?}, volume={volume}, manifold={manifold}; exact=1");
        result.unwrap();
        crate::topology::integrity::check(&b).unwrap();
        let check = validation::check_brep(
            &mut k,
            &mut b.clone_standing_for_self().unwrap(),
            validation::CheckLevel::Level2,
        )
        .unwrap();
        assert!(check.lines().is_empty(), "{:?}", check.lines());
        assert!(manifold && (volume - 1.0).abs() <= 1e-8, "{volume}");
    }

    #[test]
    fn moves_the_top_of_a_box_and_keeps_it_a_solid() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 2.0, 2.0).unwrap();
        let top = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(&b, f).unwrap().unwrap();
                let p = s
                    .evaluate_point(
                        &k,
                        s.natural_uv_domain_with_nurb(&k)
                            .unwrap()
                            .evaluate(0.5, 0.5),
                    )
                    .unwrap();
                (p.z - 2.0).abs() < 1e-9
            })
            .unwrap();
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 3.0), Vec3::Z));
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[top], vec![raised], notes)
        })
        .unwrap();
        assert_eq!(bp1::faces(&b, None).unwrap().len(), 6);
        // the cached topology fields agree with the links (the public integrity check)
        crate::topology::integrity::check(&b).unwrap();
        let e = calculate_bounding_box(&k, &mut b, BoundingBoxOptions::default()).unwrap();
        // the box carries the body's tolerance
        let near = |a: f64, b: f64| (a - b).abs() < 1e-4;
        assert!(near(e.max_at(2), 3.0) && near(e.min_at(2), 0.0) && near(e.max_at(0), 2.0));
    }

    /// A cylinder's side meets itself across its seam: given its tangent plane (open), it
    /// is refused before anything changes (fc0016 of the difftest unit kept the wrong side
    /// of an open shell, volume -877.6).
    #[test]
    fn a_seamed_face_given_a_plane_is_refused_unchanged() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0)
                .unwrap();
        let side = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                !matches!(
                    face_ops::surface(&b, f).unwrap().unwrap().kind(),
                    SurfaceKind::Plane(_)
                )
            })
            .unwrap();
        assert!(meets_itself(&b, side).unwrap());
        let before = (b.faces().len(), b.edges().len(), b.vertices().len());
        let tangent = Surface::from(Plane::from_normal(Vec3::new(1.0, 0.0, 1.0), Vec3::X));
        let e = with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[side], vec![tangent], notes)
        })
        .unwrap_err();
        assert_eq!(e.kind(), crate::error::ErrorKind::Unsupported, "{e}");
        assert!(matches!(
            e.failure().map(|f| &f.reason),
            Some(crate::error::Reason::Offset(OffsetFailure::SeamOpened { face })) if *face == side
        ));
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            before
        );
    }

    #[test]
    fn a_short_surface_list_is_a_null_pointer() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let face = bp1::faces(&b, None).unwrap()[0];
        let r = with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[face], Vec::new(), notes)
        });
        assert!(r.is_err());
    }

    /// The top face of a 20 mm cube.
    fn box_top(k: &Kernel, b: &Brep) -> FaceId {
        bp1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(b, f).unwrap().unwrap();
                let p = s
                    .evaluate_point(
                        k,
                        s.natural_uv_domain_with_nurb(k).unwrap().evaluate(0.5, 0.5),
                    )
                    .unwrap();
                (p.z - 20.0).abs() < 1e-9
            })
            .unwrap()
    }

    /// A 20 mm cube's top given the tilted plane `z = 25 - 0.1 x - 0.2 y`, in closed form:
    /// each top vertex is the exact meet of the plane with its two side planes, `(x, y, 25 -
    /// 0.1 x - 0.2 y)`, to rounding at the part's scale; the bottom vertices, on kept faces
    /// only, stay bit for bit; the volume is the base times the height over its centre,
    /// `400 * 22 = 8800`.
    #[test]
    fn a_tilted_box_top_meets_its_sides_at_the_exact_plane_meets() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
        let top = box_top(&k, &b);
        let normal = Vec3::new(0.1, 0.2, 1.0);
        let tilted = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 25.0), normal));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[top], vec![tilted]);
        assert_eq!(why, None);
        let mut points: Vec<Point3> = b.vertices().iter().map(|(_, v)| v.point).collect();
        points.sort_by(|p, q| (p.z, p.x, p.y).partial_cmp(&(q.z, q.x, q.y)).unwrap());
        assert_eq!(points.len(), 8);
        for (p, (x, y)) in
            points[..4]
                .iter()
                .zip([(0.0, 0.0), (0.0, 20.0), (20.0, 0.0), (20.0, 20.0)])
        {
            assert_eq!(*p, Point3::new(x, y, 0.0), "a kept vertex moved");
        }
        for p in &points[4..] {
            let z = 25.0 - 0.1 * p.x - 0.2 * p.y;
            let on_sides = [p.x, p.y]
                .iter()
                .all(|c| c.abs() <= 1e-12 || (c - 20.0).abs() <= 1e-12);
            assert!(
                on_sides && (p.z - z).abs() <= 1e-12 * 25.0,
                "{p:?} vs z {z}"
            );
        }
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v - 8800.0).abs() <= 1e-9 * 8800.0, "{v}");
    }

    /// WR-534 (the fork's tangency-labs/smlib@a1ee273): a 20 mm cube's top moved up by
    /// 60, further than the cube's size, is a sound 20 x 20 x 80 box with the opt-in.
    #[test]
    fn moves_the_top_of_a_box_further_than_its_size() {
        let mut k = Kernel::default();
        k.fixes.local_operation_box_holds_new_surfaces = true;
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
        let top = box_top(&k, &b);
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 80.0), Vec3::Z));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[top], vec![raised]);
        assert_eq!(why, None);
        assert_eq!(bp1::faces(&b, None).unwrap().len(), 6);
        let (v, _) = volume_and_manifold(&k, &mut b);
        assert!((v - 32000.0).abs() <= 1e-9 * 32000.0, "{v}");
    }

    /// Second review, finding 11: with the exact analytic mate opt-in, the analytic copies
    /// of the faces' surfaces are extended by each face's derived reach (not five offset
    /// distances, 2.5 times the body's bounding radius in a local operation): a cylinder's
    /// top raised 0.5 meets its side on the exact circle there, a sound solid of the
    /// raised cylinder's volume.
    #[test]
    fn a_raised_cylinder_top_mates_on_its_exact_circle_within_the_derived_reach() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        k.fixes.offset_analytic_mate_edges = true;
        let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0).unwrap();
        let top = box_top(&k, &b);
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 20.5), Vec3::Z));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[top], vec![raised]);
        assert_eq!(why, None);
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        let want = crate::math::PI * 25.0 * 20.5;
        assert!((v - want).abs() <= 1e-6 * want, "{v} vs {want}");
    }

    /// The same top given a plane parametrised over the unit square, 20 mm a unit: the
    /// body built leaves the corner over (20, 20) unbuilt (the vertical edge there ends at
    /// the old top, z = 20, and the moved top's two edges into that corner end 60 from
    /// their vertex), so it is refused as not sound, the cube as it came in (a defect of
    /// its own, not of the gate: the plane through the same points parametrised by length
    /// gives the sound box above).
    #[test]
    fn a_far_top_on_a_unit_square_plane_is_refused_as_not_sound() {
        let mut k = Kernel::default();
        k.fixes.local_operation_box_holds_new_surfaces = true;
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
        let top = box_top(&k, &b);
        let raised = Surface::from(Plane::new(
            Vec3::new(0.0, 0.0, 80.0),
            Vec3::X,
            Vec3::Y,
            crate::math::Vec2::new(20.0, 20.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[top], vec![raised]);
        assert_eq!(why, Some(unsound_check(3, 61)));
    }

    /// The volume and whether the body is a manifold solid, with the cached topology
    /// fields agreeing with the links.
    fn volume_and_manifold(k: &Kernel, b: &mut Brep) -> (f64, bool) {
        crate::topology::integrity::check(b).unwrap();
        let volume = crate::mass::integrator::properties::brep_volume_default(k, b, 1e-6).unwrap();
        (
            volume,
            crate::query::api_queries::brep_is_manifold_solid(b).unwrap(),
        )
    }

    /// The face of `b` whose surface is not a plane (a primitive's side).
    fn curved_face(b: &Brep) -> FaceId {
        bp1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                !matches!(
                    face_ops::surface(b, f).unwrap().unwrap().kind(),
                    SurfaceKind::Plane(_)
                )
            })
            .unwrap()
    }

    /// `b`'s curved face given `surface`: Ok, a manifold solid with the input's volume to
    /// rounding (the closed form `volume` to the integrator's accuracy, 1e-6), faces, edges,
    /// vertices and tolerance.
    fn given_its_own(mut k: Kernel, mut b: Brep, surface: Surface, volume: f64) {
        let face = curved_face(&b);
        let before = (b.faces().len(), b.edges().len(), b.vertices().len());
        let tolerance = b.tolerance();
        let (input, _) = volume_and_manifold(&k, &mut b);
        assert!(
            (input - volume).abs() <= 1e-6 * volume,
            "{input} vs {volume}"
        );
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[face], vec![surface], notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        assert!((v - input).abs() <= 1e-12 * input, "{v} vs {input}");
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            before
        );
        assert_eq!(b.tolerance(), tolerance);
    }

    /// A cylinder's side given the same cylinder (another surface of
    /// it, over another extent) keeps its surface and edges: the body is unchanged.
    #[test]
    fn a_cylinder_given_its_own_cylinder_is_unchanged() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let s = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(0.0, 0.0, -1.0), 1.0, 4.0).unwrap(),
        );
        given_its_own(k, b, s, 2.0 * std::f64::consts::PI);
    }

    #[test]
    fn a_cone_given_its_own_cone_is_unchanged() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let b = ap::create_cone(&mut k, Point3::ZERO, 2.0, 1.0, 3.0).unwrap();
        let s = Surface::from(ap::create_cone_surface(&k, Point3::ZERO, 2.0, 1.0, 3.0).unwrap());
        given_its_own(
            k,
            b,
            s,
            std::f64::consts::PI * 3.0 / 3.0 * (4.0 + 2.0 + 1.0),
        );
    }

    #[test]
    fn a_sphere_given_its_own_sphere_is_unchanged() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let b = ap::create_sphere(&mut k, Point3::new(1.0, 2.0, 3.0), 1.5).unwrap();
        let other = ap::create_sphere(&mut k, Point3::new(1.0, 2.0, 3.0), 1.5).unwrap();
        let s = face_ops::surface(&other, curved_face(&other))
            .unwrap()
            .unwrap()
            .clone();
        given_its_own(k, b, s, 4.0 / 3.0 * std::f64::consts::PI * 1.5_f64.powi(3));
    }

    #[test]
    fn a_torus_given_its_own_torus_is_unchanged() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let b = ap::create_torus(&mut k, Point3::ZERO, 3.0, 1.0).unwrap();
        let s = Surface::from(ap::create_torus_surface(&k, Point3::ZERO, 3.0, 1.0).unwrap());
        given_its_own(k, b, s, 2.0 * std::f64::consts::PI.powi(2) * 3.0);
    }

    /// A plane through the origin with normal `(0, -t, 1)` (axes `x`, `(0, 1, t)`), and the
    /// parallel plane through `(0, s, 0)`, apart by `t s` along the normal. With `s`
    /// subnormal, or `t s` below the least normal, the products `t s` round to zero and
    /// so do their `fma` errors: rounded, the origin read as on the plane. Out of the
    /// exact range, the planes are not shown the same, so the face moves; the plane is
    /// still its own.
    #[test]
    fn a_plane_a_subnormal_coordinate_apart_is_not_its_own() {
        let plane = |t: f64, s: f64| {
            Axis2Placement::from_stored(
                Point3::new(0.0, s, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, t),
            )
        };
        for (t, s) in [(1e-14, 1e-310), (1e-14, 5e-324), (1e-30, 1e-300)] {
            let own = plane(t, 0.0);
            assert!(same_plane(&own, &own));
            assert!(!same_plane(&own, &plane(t, s)), "t = {t}, s = {s}");
            assert!(!same_plane(&plane(t, s), &own), "t = {t}, s = {s}");
        }
    }

    /// A cylinder's side given the coaxial cylinder one rounding wider moves, by a reach
    /// of rounding size: the extension of the neighbours by that tiny margin is built, and
    /// the body is the wider cylinder.
    #[test]
    fn a_cylinder_given_one_a_rounding_wider_moves_by_it() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let r = f64::from_bits(1.0_f64.to_bits() + 1);
        let s = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(0.0, 0.0, -1.0), r, 4.0).unwrap(),
        );
        let face = curved_face(&b);
        let tolerance = b.tolerance();
        let (input, _) = volume_and_manifold(&k, &mut b);
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[face], vec![s], notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        // no edge is kept (each touches the side), and the body keeps the input's tolerance
        // all the same
        assert_eq!(b.tolerance(), tolerance);
        // the new caps' edges are re-intersected B-splines within the body's tolerance of
        // the circles (as for any moved cylinder side), so the volume is within the
        // tolerance times the area (2 pi r h + 2 pi r^2) of the input's
        let bound = b.tolerance() * 6.0 * std::f64::consts::PI;
        assert!((v - input).abs() <= bound, "{v} vs {input}");
    }

    /// A box's top raised, its bottom's four edges kept (their
    /// faces and their vertices' faces all stay). Each is the original edge in the result:
    /// the same curve over the same interval, its tolerance and its vertices' not below
    /// the input's; the body's tolerance is the input's; the result is a manifold solid.
    #[test]
    fn a_raised_top_keeps_the_bottom_edges_as_they_were() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let input = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let top = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(&b, f).unwrap().unwrap();
                let p = s
                    .evaluate_point(
                        &k,
                        s.natural_uv_domain_with_nurb(&k)
                            .unwrap()
                            .evaluate(0.5, 0.5),
                    )
                    .unwrap();
                (p.z - 2.0).abs() < 1e-9
            })
            .unwrap();
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 3.0), Vec3::Z));
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[top], vec![raised], notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v - 12.0).abs() <= 1e-6 * 12.0, "{v}");
        assert_eq!(b.tolerance(), input.tolerance());
        // the curve's points at its interval's ends and quarters, bit for bit
        let points = |br: &Brep, e: crate::topology::EdgeId| {
            let c = crate::topology::edge::curve(br, e).unwrap().unwrap();
            let i = crate::topology::edge::interval(br, e).unwrap();
            [0.0, 0.25, 0.5, 0.75, 1.0].map(|t| {
                c.evaluate_point(&k, i.min() + t * (i.max() - i.min()))
                    .unwrap()
            })
        };
        let bottom: Vec<_> = input
            .edges()
            .iter()
            .map(|(e, _)| e)
            .filter(|&e| points(&input, e).iter().all(|p| p.z == 0.0))
            .collect();
        assert_eq!(bottom.len(), 4);
        for e in bottom {
            let want = points(&input, e);
            let interval = crate::topology::edge::interval(&input, e).unwrap();
            let kept: Vec<_> = b
                .edges()
                .iter()
                .map(|(f, _)| f)
                .filter(|&f| points(&b, f) == want)
                .collect();
            let &[f] = kept.as_slice() else {
                panic!("edge {e:?}: {} copies", kept.len());
            };
            let got = crate::topology::edge::interval(&b, f).unwrap();
            assert_eq!(
                (got.min().to_bits(), got.max().to_bits()),
                (interval.min().to_bits(), interval.max().to_bits())
            );
            assert!(b.edge(f).unwrap().tolerance >= input.edge(e).unwrap().tolerance);
            let (s0, s1) = crate::topology::edge::start_end_vertices(&input, e).unwrap();
            let (t0, t1) = crate::topology::edge::start_end_vertices(&b, f).unwrap();
            for (s, t) in [(s0, t0), (s1, t1)] {
                let (s, t) = (
                    input.vertex(s.unwrap()).unwrap(),
                    b.vertex(t.unwrap()).unwrap(),
                );
                assert_eq!(s.point, t.point);
                assert!(t.tolerance >= s.tolerance);
            }
        }
    }

    /// A pyramid as the difftest unit's `Solids/pyramid.smb` is made: a rectangular base
    /// and four triangular sides, each face a bicubic B-spline patch over `[0, 1]^2` (a
    /// side's far row collapsed onto the apex), at the file's B-rep tolerance.
    fn bicubic_pyramid(k: &mut Kernel) -> Brep {
        use crate::geom::extent2d::Extent2d;
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as Net;
        use crate::topology::brep::p2::ArenaNotifyState;
        use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
        let (x, y, h) = (
            83.555_557_250_976_56,
            87.111_114_501_953_12,
            101.333_335_876_464_84,
        );
        let base = [
            Point3::new(-x, -y, 0.0),
            Point3::new(x, -y, 0.0),
            Point3::new(x, y, 0.0),
            Point3::new(-x, y, 0.0),
        ];
        let apex = Point3::new(0.0, 0.0, h);
        let third = |a: Point3, b: Point3, i: u32| a + (b - a) * (f64::from(i) / 3.0);
        // rows along the first side, columns towards the second
        let patch = |row: &dyn Fn(u32) -> (Point3, Point3)| {
            let net: Vec<Vec<CPoint>> = (0..4)
                .map(|i| {
                    let (a, b) = row(i);
                    (0..4)
                        .map(|j| {
                            let p = third(a, b, j);
                            CPoint::new(p.x, p.y, p.z, NOW)
                        })
                        .collect()
                })
                .collect();
            let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
            let n = Net::new(net, 3, 3, knots.clone(), knots).unwrap();
            Surface::from(BSplineSurface::from_gw_nurb(&n))
        };
        let mut surfaces = vec![patch(&|i| {
            (third(base[0], base[1], i), third(base[3], base[2], i))
        })];
        for s in 0..4 {
            let (a, b) = (base[s], base[(s + 1) % 4]);
            surfaces.push(patch(&|i| (third(a, b, i), apex)));
        }
        let mut b = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        {
            let mut state = ArenaNotifyState::new(&b);
            let mut env = state.env(None);
            let mut marks = crate::topology::topology_traverser::Marks::default();
            let this_face = |f: FaceId| crate::topology::loopuse::notify_ref(EntityRef::Face(f));
            let mut fenv = FaceBuildEnv {
                env: &mut env,
                marks: &mut marks,
                this_face: &this_face,
            };
            let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
            for s in surfaces {
                create_face_from_surface(k, &mut b, Some(s), &domain, &mut fenv).unwrap();
            }
        }
        p2::set_tolerance(k, &mut b, 0.001_013_333_358_765, false, true).unwrap();
        p5::stitch_and_orient(k, &mut b, &p5::StitchAndOrientOptions::default()).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// gen0044 of the difftest unit: a pyramid's side replaced by the plane through its old
    /// edges (its middle point and normal there, so the same plane to rounding). Nothing
    /// moves, and the neighbours' new edges must still cross the extended surfaces'
    /// boundaries: the body is a sound manifold solid with the input's volume, the same
    /// faces, edges and vertices, and its B-spline surfaces' knot vectors are valid. The
    /// side's two edges end at the apex at two parameters (its collapsed row, a pole): the
    /// plane's face has no edge there, where it had one of no length on that face alone.
    #[test]
    fn a_pyramid_side_given_the_plane_through_its_edges_stays_sound() {
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        let mut k = Kernel::default();
        let mut b = bicubic_pyramid(&mut k);
        let (input, _) = volume_and_manifold(&k, &mut b);
        let before = (b.faces().len(), b.edges().len(), b.vertices().len());
        let side = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(&b, f).unwrap().unwrap();
                let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
                s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap().z > 0.0
            })
            .unwrap();
        let s = face_ops::surface(&b, side).unwrap().unwrap();
        let uv = b.face(side).unwrap().uv_domain.evaluate(0.5, 0.5);
        let p = s.evaluate_point(&k, uv).unwrap();
        let n = s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap();
        let plane = Surface::from(Plane::from_normal(Vec3::new(p.x, p.y, p.z), n));
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[side], vec![plane], notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        assert!((v - input).abs() <= 1e-9 * input, "{v} vs {input}");
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            before
        );
        b.use_arena_naming().unwrap();
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{lines:?}");
        for (_, f) in b.faces().iter() {
            let net = match f.surface.as_ref().map(Surface::kind) {
                Some(SurfaceKind::BSpline(s)) => s,
                Some(SurfaceKind::Plane(p)) => p.base(),
                _ => continue,
            };
            let n = net.gw_nurb().unwrap();
            for (knots, degree) in [(n.u_knots(), n.p()), (n.v_knots(), n.q())] {
                let ordered = knots.iter().zip(knots.iter().skip(1)).all(|(a, b)| a <= b);
                assert!(ordered, "{knots:?}");
                assert!(
                    knots
                        .chunk_by(|a, b| a == b)
                        .all(|run| run.len() <= degree + 1),
                    "{knots:?} degree {degree}"
                );
            }
        }
    }

    /// gen0091 of the difftest unit: a pyramid's side given the plane through its edges and
    /// the side across its apex from it moved out. The four sides no longer meet in one point:
    /// the apex splits into two vertices and a new edge between the two sides the moved one
    /// parts (with the four-face split on). The new vertex is next to the apex, a pole of the
    /// fourth side's B-spline patch, and on no extension of it (a side's extension narrows to
    /// nothing at the pole): the side, which lies on a plane, continues as that plane. The
    /// body is the convex solid of the five planes, a sound manifold of nine edges and six
    /// vertices.
    #[test]
    fn a_pyramid_side_moved_past_its_apex_splits_it_on_the_planes_beside() {
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        let mut k = Kernel::default();
        k.fixes.offset_closes_loop_gaps = true;
        k.fixes.offset_splits_corner_vertices = true;
        k.fixes.offset_four_face_vertex_split = true;
        let mut b = bicubic_pyramid(&mut k);
        let all = bp1::faces(&b, None).unwrap();
        // the planes of the faces, each as (point, outward unit normal)
        let centre = Vec3::new(0.0, 0.0, 25.0);
        let planes: Vec<(Vec3, Vec3)> = all
            .iter()
            .map(|&f| {
                let s = face_ops::surface(&b, f).unwrap().unwrap();
                let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
                let p = s.evaluate_point(&k, uv).unwrap();
                let n = s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap();
                (p, if n.dot(p - centre) > 0.0 { n } else { n * -1.0 })
            })
            .collect();
        // faces: the base, then the sides along y = -y0, x = x0, y = y0, x = -x0
        let (kept, moved, distance) = (2_usize, 1_usize, 0.407_738_046_172_282_6);
        let mut given = planes.clone();
        given[moved].0 = given[moved].0 + given[moved].1 * distance;
        let surfaces = [kept, moved]
            .map(|i| Surface::from(Plane::from_normal(given[i].0, given[i].1)))
            .to_vec();
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[all[kept], all[moved]], surfaces, notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            (5, 9, 6)
        );
        b.use_arena_naming().unwrap();
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{lines:?}");
        // the convex solid of the five planes: its corners, and its volume from its faces'
        // fans about the base's centre
        let meet = |[i, j, l]: [usize; 3]| {
            let [(p, a), (q, c), (r, e)] = [given[i], given[j], given[l]];
            let det = a.dot(c.cross(e));
            (c.cross(e) * a.dot(p) + e.cross(a) * c.dot(q) + a.cross(c) * e.dot(r)) * (1.0 / det)
        };
        let corner = |i: usize, j: usize| meet([0, i, j]);
        let (b0, b1, b2, b3) = (corner(4, 1), corner(1, 2), corner(2, 3), corner(3, 4));
        let (apex, top) = (meet([2, 3, 4]), meet([1, 2, 4]));
        let faces: [&[Vec3]; 5] = [
            &[b0, b3, b2, b1],
            &[b0, b1, top],
            &[b1, b2, apex, top],
            &[b2, b3, apex],
            &[b3, b0, top, apex],
        ];
        let o = (b0 + b2) * 0.5;
        let want: f64 = faces
            .iter()
            .map(|f| {
                f.iter()
                    .skip(1)
                    .zip(f.iter().skip(2))
                    .map(|(&p, &q)| (f[0] - o).dot((p - o).cross(q - o)) / 6.0)
                    .sum::<f64>()
            })
            .sum();
        assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    }

    /// `b`'s `faces` given `surfaces`: the offset refusal (`Some`), `b` then exactly as it
    /// came in (its `Debug` form, every surface's cache included, the same before and
    /// after), or `None` when the body is returned. What a returned body is, each test
    /// asserts itself.
    fn refused_unchanged_or_returned(
        k: &mut Kernel,
        b: &mut Brep,
        faces: &[FaceId],
        surfaces: Vec<Surface>,
    ) -> Option<OffsetFailure> {
        let before = format!("{b:?}");
        let r = with_notifications(b, |b, notes| local_operation(k, b, faces, surfaces, notes));
        let e = r.err()?;
        assert_eq!(e.kind(), crate::error::ErrorKind::Unsupported, "{e}");
        let Some(crate::error::Reason::Offset(why)) = e.failure().map(|f| &f.reason) else {
            panic!("not an offset refusal: {e}");
        };
        assert!(
            format!("{b:?}") == before,
            "refused ({why:?}), but the input changed"
        );
        Some(*why)
    }

    /// A 10 mm cube with its vertical edge at x = y = 10 rounded r 2, and the round.
    fn box_rounded_r2(k: &mut Kernel) -> (Brep, FaceId) {
        use crate::blend::fillet_executive::api::fillet_edges;
        use crate::topology::{edge, vertex};
        let mut b = create_box(k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let e = b
            .edges()
            .ids()
            .find(|&e| {
                let (s, t) = edge::start_end_vertices(&b, e).unwrap();
                [s, t].into_iter().flatten().all(|v| {
                    let p = vertex::point(&b, v).unwrap();
                    p.x == 10.0 && p.y == 10.0
                })
            })
            .unwrap();
        fillet_edges(k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
        let round = curved_face(&b);
        (b, round)
    }

    /// A refusal leaves everything as it came in: `b` in its `Debug` form, the caches its
    /// surfaces had built before the call included (none dropped, none added), and the
    /// caller's notification records and attributes. Refused by the gate after the offset
    /// ran, a cube's round given a wider coaxial cylinder, and a side moved off its round
    /// (the edge between them apart: `LocalReach::apart`, adding nothing to the reach);
    /// `b` carries an attribute of its own, lent to the offset, and the caller's store one
    /// of the caller's.
    #[test]
    fn a_refusal_leaves_the_input_its_caches_and_the_callers_stores_as_they_were() {
        use crate::geom::attribute::{Attribute, Behavior};
        use crate::primitive::api_primitives as ap;
        type Make = fn(&mut Kernel) -> (Brep, FaceId, Surface);
        let cases: [(&str, Make); 2] = [
            ("gate", |k| {
                let (b, round) = box_rounded_r2(k);
                let s = ap::create_cylinder_surface(k, Point3::new(8.0, 8.0, -1.0), 2.3, 12.0);
                (b, round, Surface::from(s.unwrap()))
            }),
            ("apart", |k| {
                let b = cube_rounded_r5(k);
                let side = face_on_x(k, &b, 20.0);
                let s = Plane::from_normal(Vec3::new(20.1, 0.0, 0.0), Vec3::X);
                (b, side, Surface::from(s))
            }),
        ];
        for (name, make) in cases {
            let mut k = Kernel::default();
            let (mut b, face, s) = make(&mut k);
            b.add_attribute(EntityRef::Face(face), Attribute::long(7, 1, Behavior::Copy))
                .unwrap();
            // caches built on `b`'s surfaces before the call (the copy shares their slots)
            validation::soundness(&mut k, &mut b.clone_standing_for_self().unwrap()).unwrap();
            let before = format!("{b:?}");
            let (r, stores) = with_notifications(&mut b, |b, notes| {
                let h = notes
                    .attributes
                    .insert(Attribute::long(9, 2, Behavior::Copy))
                    .unwrap();
                notes
                    .attributes
                    .add_attribute(ObjectKey(1 << 62), h)
                    .unwrap();
                let stores = (format!("{:?}", notes.records), notes.attributes.clone());
                let r = local_operation(&mut k, b, &[face], vec![s], notes);
                let after = (format!("{:?}", notes.records), notes.attributes.clone());
                (r, (stores, after))
            });
            let e = r.expect_err(name);
            assert!(
                matches!(
                    e.failure().map(|f| &f.reason),
                    Some(crate::error::Reason::Offset(OffsetFailure::Unsound { .. }))
                ),
                "{name}: {e}"
            );
            assert!(format!("{b:?}") == before, "{name}: the input changed");
            assert!(stores.0 .0 == stores.1 .0, "{name}: the records changed");
            assert!(stores.0 .1 == stores.1 .1, "{name}: the attributes changed");
        }
    }

    /// The gate checks the owners against the body's naming, not against the owners the
    /// geometry records itself: an edge's curve owned under an identity no edge has
    /// fails `standing`'s owner test, where `soundness` (each geometry's recorded owner
    /// taken as right) cannot see it.
    #[test]
    fn the_gate_checks_curve_owners_against_the_bodys_naming() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let measure = |k: &mut Kernel, b: &Brep| {
            let mut copy = b.clone_standing_for_self().unwrap();
            let standing = validation::standing(k, &mut copy).unwrap();
            let mut copy = b.clone_standing_for_self().unwrap();
            (
                standing.check,
                validation::soundness(k, &mut copy).unwrap().check,
            )
        };
        let (clean, clean_own) = measure(&mut k, &b);
        assert!(
            clean.is_valid() && clean_own.is_valid(),
            "{:?}",
            clean.lines()
        );
        // an edge's identity no edge of the body has
        let mut wrong = b.owner_identity(EntityRef::Edge(b.edges().ids().next().unwrap()));
        wrong.key = ObjectKey(wrong.key.0 ^ (1 << 40));
        let e0 = b.edges().ids().next().unwrap();
        let curve = b.edge_mut(e0).unwrap().curve.as_mut().unwrap();
        let _ = curve.set_owner(Some(wrong), None);
        let (named, own) = measure(&mut k, &b);
        assert!(
            !named.failures_beyond(&clean).is_empty(),
            "{:?}",
            named.lines()
        );
        assert!(
            own.failures_beyond(&clean_own).is_empty(),
            "{:?}",
            own.lines()
        );
    }

    /// A body whose pointer check fails is not checked further (`AssertValid` is skipped):
    /// the gate refuses it even where its input failed the pointer check alike, rather
    /// than finding nothing beyond the input's failures.
    #[test]
    fn the_gate_refuses_a_body_whose_geometry_it_cannot_check() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let e = b.edges().ids().next().unwrap();
        b.edge_mut(e).unwrap().curve = None;
        let input =
            validation::standing(&mut k, &mut b.clone_standing_for_self().unwrap()).unwrap();
        assert!(
            input.check.assert_valid.is_none(),
            "{:?}",
            input.check.lines()
        );
        let e = refuse_unsound(&mut k, &b, &input).unwrap_err();
        assert!(
            matches!(
                e.failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(OffsetFailure::Unsound {
                    what: crate::error::offset::Unsoundness::GeometryUnchecked
                }))
            ),
            "{e}"
        );
    }

    /// A torus face meets itself across seams both ways round; a cylinder
    /// is closed one way only, so one seam would have no edge: refused before anything
    /// changes.
    #[test]
    fn a_torus_given_a_cylinder_is_refused_as_a_seam_opened() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let mut b = ap::create_torus(&mut k, Point3::ZERO, 3.0, 1.0).unwrap();
        let face = curved_face(&b);
        let s = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(0.0, 0.0, -2.0), 3.5, 4.0).unwrap(),
        );
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[face], vec![s]);
        assert!(
            matches!(why, Some(OffsetFailure::SeamOpened { face: f }) if f == face),
            "{why:?}"
        );
    }

    /// A cylinder's side given a coaxial
    /// cylinder of another radius. A cylinder's u is its arc length, of period `2π r`: the
    /// side's UV loops, copied onto the new surface, run u over `[0, 2π]` (radius 1), which
    /// on radius 1.5 is two thirds of the way round and on 0.5 twice round, so its seam
    /// lines (u = 0 and u = 2π) lift to two different rulings and the seam has no edge:
    /// refused as a seam opened before anything is built (before the seams were matched,
    /// the body was built and the gate refused it: an edge's curve owner, #61, and a vertex
    /// an edge is not on, #82).
    #[test]
    fn a_cylinder_given_a_coaxial_cylinder_of_another_radius_is_refused_as_a_seam_opened() {
        use crate::primitive::api_primitives as ap;
        for radius in [1.5, 0.5] {
            let mut k = Kernel::default();
            let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
            let face = curved_face(&b);
            let s = Surface::from(
                ap::create_cylinder_surface(&k, Point3::new(0.0, 0.0, -1.0), radius, 4.0).unwrap(),
            );
            assert!(!keeps_seams(&k, &mut b, face, &s).unwrap(), "{radius}");
            let why = refused_unchanged_or_returned(&mut k, &mut b, &[face], vec![s]);
            assert!(
                matches!(why, Some(OffsetFailure::SeamOpened { face: f }) if f == face),
                "{radius}: {why:?}"
            );
        }
    }

    /// Second review, finding 6: a cylinder's side given a cylinder of its radius off its own
    /// axis (1e-12: any offset will do, as `is_own_surface` compares exactly; so not its
    /// own surface) with u and v swapped is closed in one direction, as the side is, but
    /// round its v: the side's seam lines (u = 0 and u = 2π) lift to two circles 2π apart
    /// along the axis, so the seam has no edge there:
    /// refused as a seam opened before anything is built. Unswapped, it keeps the seam.
    /// Its own cylinder swapped is its own surface (`SwapUV` keeps the definition, WR-701,
    /// and the parametrisation is not compared): the face keeps its own copy.
    #[test]
    fn a_cylinder_given_its_cylinder_swapped_is_refused_as_a_seam_opened() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let face = curved_face(&b);
        let mut s = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(1e-12, 0.0, -1.0), 1.0, 4.0).unwrap(),
        );
        assert!(keeps_seams(&k, &mut b, face, &s).unwrap());
        s.swap_uv(&k).unwrap();
        assert!(!keeps_seams(&k, &mut b, face, &s).unwrap());
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[face], vec![s]);
        assert!(
            matches!(why, Some(OffsetFailure::SeamOpened { face: f }) if f == face),
            "{why:?}"
        );
        let mut own = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(0.0, 0.0, -1.0), 1.0, 4.0).unwrap(),
        );
        own.swap_uv(&k).unwrap();
        assert!(is_own_surface(
            face_ops::surface(&b, face).unwrap().unwrap(),
            &own
        ));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[face], vec![own]);
        assert!(why.is_none(), "{why:?}");
    }

    /// The largest tolerance of `face`'s edges.
    fn largest_edge_tolerance(b: &Brep, face: FaceId) -> f64 {
        crate::topology::face::p4::edges(b, face, None, false, None)
            .unwrap()
            .into_iter()
            .map(|e| crate::topology::edge::tolerance(b, e).unwrap())
            .fold(0.0, f64::max)
    }

    /// A closed square tube net, degree 1 both ways, over `domain` with 4 spans round
    /// (u) and 64 along (v): its first and last columns are one, except the last column's
    /// point at the v knot 4/64, moved by `moved` along x, and the last column's weights,
    /// `last_weight` (a rational net when not 1, its points the same).
    fn tube(domain: &Extent2d, moved: f64, last_weight: f64) -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        let (u0, u1, v0, v1) = (domain.min.x, domain.max.x, domain.min.y, domain.max.y);
        let corners = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
        let rational = last_weight != 1.0;
        let net: Vec<Vec<CPoint>> = corners
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| {
                (0..=64_u32)
                    .map(|j| {
                        let z = 2.0 * f64::from(j) / 64.0;
                        let last = i == 4;
                        let x = if last && j == 4 { x + moved } else { x };
                        match (rational, last) {
                            (false, _) => CPoint::new(x, y, z, NOW),
                            (true, false) => CPoint::new(x, y, z, 1.0),
                            (true, true) => CPoint::new(
                                x * last_weight,
                                y * last_weight,
                                z * last_weight,
                                last_weight,
                            ),
                        }
                    })
                    .collect()
            })
            .collect();
        let du = (u1 - u0) / 4.0;
        let u = vec![u0, u0, u0 + du, u0 + 2.0 * du, u0 + 3.0 * du, u1, u1];
        let mut v = vec![v0];
        v.extend((0..64_u32).map(|j| v0 + (v1 - v0) * f64::from(j) / 64.0));
        v.push(v1);
        v.push(v1);
        let n = crate::nurbs::srf_basic::Surface::new(net, 1, 1, u, v).unwrap();
        Surface::from(BSplineSurface::from_nurb(n, false))
    }

    /// A cylinder side given a closed tube net over its own domain keeps its seam: the
    /// lines are the net's domain ends, and its first and last columns are one, so the
    /// certified gap is 0. One last-column point moved by 10 times the edge's tolerance, at
    /// the v knot 4/64, opens the seam only between the knots 3/64 and 5/64 (where the
    /// side's seam spans the domain, between the points t = i/8 the seam was once lifted at,
    /// which saw no gap): the column bound refuses it.
    #[test]
    fn a_seam_opening_between_the_old_samples_is_refused() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let face = curved_face(&b);
        let domain = face_ops::surface(&b, face)
            .unwrap()
            .unwrap()
            .natural_uv_domain_with_nurb(&k)
            .unwrap();
        let tolerance = largest_edge_tolerance(&b, face);
        assert!(keeps_seams(&k, &mut b, face, &tube(&domain, 0.0, 1.0)).unwrap());
        let opened = tube(&domain, 10.0 * tolerance, 1.0);
        assert!(!keeps_seams(&k, &mut b, face, &opened).unwrap());
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[face], vec![opened]);
        assert!(
            matches!(why, Some(OffsetFailure::SeamOpened { face: f }) if f == face),
            "{why:?}"
        );
    }

    /// A closed net whose last column carries other weights than its first (the same
    /// points) has no column certificate (the two boundary isos do not share one rational
    /// basis), so the seam is refused rather than decided by samples.
    #[test]
    fn a_closed_net_whose_end_columns_differ_in_weight_is_refused() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let face = curved_face(&b);
        let domain = face_ops::surface(&b, face)
            .unwrap()
            .unwrap()
            .natural_uv_domain_with_nurb(&k)
            .unwrap();
        assert!(!keeps_seams(&k, &mut b, face, &tube(&domain, 0.0, 2.0)).unwrap());
    }

    /// A cylinder's side given an offset of a coaxial cylinder of its own radius (its
    /// lines on the offset's domain ends): an offset surface has no seam certificate, so
    /// it is refused as a seam opened before anything changes.
    #[test]
    fn a_cylinder_given_an_offset_surface_is_refused_as_a_seam_opened() {
        use crate::primitive::api_primitives as ap;
        use crate::surface::offset_surface::OffsetSurface;
        let mut k = Kernel::default();
        let mut b = ap::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let face = curved_face(&b);
        let base = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(0.0, 0.0, -1.0), 1.0, 4.0).unwrap(),
        );
        let s = Surface::from(OffsetSurface::new(&k, 0.25, base, true, 0).unwrap());
        assert!(!keeps_seams(&k, &mut b, face, &s).unwrap());
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[face], vec![s]);
        assert!(
            matches!(why, Some(OffsetFailure::SeamOpened { face: f }) if f == face),
            "{why:?}"
        );
    }

    /// A 10 mm cube's vertical edge rounded r 2, the round given a
    /// coaxial cylinder of another radius: the wider r 2.3 and r 2.05 refused as not sound
    /// (the first new failure an edge's curve owner, #61); the narrower
    /// r 1.8 no longer reaches the side planes it was tangent to (it ends 0.2 short of
    /// them): the edges between them are apart (`LocalReach::apart`, adding nothing to the
    /// reach), and the body built is refused as not sound.
    #[test]
    fn a_round_given_a_coaxial_cylinder_is_refused_as_not_sound() {
        use crate::primitive::api_primitives as ap;
        for radius in [2.3, 2.05, 1.8] {
            let mut k = Kernel::default();
            let (mut b, round) = box_rounded_r2(&mut k);
            let s = Surface::from(
                ap::create_cylinder_surface(&k, Point3::new(8.0, 8.0, -1.0), radius, 12.0).unwrap(),
            );
            let why = refused_unchanged_or_returned(&mut k, &mut b, &[round], vec![s]);
            if radius < 2.0 {
                assert!(
                    matches!(why, Some(OffsetFailure::Unsound { .. })),
                    "{radius}: {why:?}"
                );
            } else {
                // refused as not sound, the first new failure an edge's curve owner (#61);
                // how many more lines the check gives depends on how the body was built
                assert!(
                    matches!(
                        why,
                        Some(OffsetFailure::Unsound {
                            what: crate::error::offset::Unsoundness::Check {
                                first: Some((_, 61)),
                                ..
                            }
                        })
                    ),
                    "{radius}: {why:?}"
                );
            }
        }
    }

    /// gen0091 of the difftest unit with the default fixes: the pyramid's side moved past
    /// its apex needs the four-face vertex split (opt-in), without which every end merges
    /// into one vertex grown to cover the split: not sound, refused.
    #[test]
    fn a_pyramid_side_moved_past_its_apex_is_refused_without_the_four_face_split() {
        let mut k = Kernel::default();
        let mut b = bicubic_pyramid(&mut k);
        let all = bp1::faces(&b, None).unwrap();
        let plane_of = |b: &Brep, k: &Kernel, f: FaceId, d: f64| {
            let s = face_ops::surface(b, f).unwrap().unwrap();
            let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
            let p = s.evaluate_point(k, uv).unwrap();
            let n = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
            let n = if n.dot(p - Vec3::new(0.0, 0.0, 25.0)) > 0.0 {
                n
            } else {
                n * -1.0
            };
            Surface::from(Plane::from_normal(p + n * d, n))
        };
        // faces: the base, then the sides along y = -y0, x = x0, y = y0, x = -x0
        let surfaces = vec![
            plane_of(&b, &k, all[2], 0.0),
            plane_of(&b, &k, all[1], 0.407_738_046_172_282_6),
        ];
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[all[2], all[1]], surfaces);
        assert_eq!(why, Some(unsound_check(2, 61)));
    }

    /// A 20 mm cube of bilinear B-spline faces whose bottom (z = 0) carries a defect of its
    /// own: its u knot 0.5 twice, over the degree, the middle control points doubled so the
    /// face is whole, and placed off the middle so that the patch is not a plane
    /// parametrised by length. Its two edges along x, its iso lines, keep the knot: they
    /// fail `SmBSplineCurve` test 2 (an internal knot of multiplicity above the degree).
    fn cube_with_a_defective_bottom(k: &mut Kernel) -> Brep {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as Net;
        use crate::topology::brep::p2::ArenaNotifyState;
        use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
        let c = |x: f64, y: f64, z: f64| CPoint::new(x, y, z, NOW);
        let bilinear = |a: Point3, b: Point3, d: Point3| {
            // corners a, a + (b - a), a + (d - a), rows along b - a
            let e = b + (d - a);
            let net = vec![
                vec![c(a.x, a.y, a.z), c(d.x, d.y, d.z)],
                vec![c(b.x, b.y, b.z), c(e.x, e.y, e.z)],
            ];
            let knots = vec![0.0, 0.0, 1.0, 1.0];
            let n = Net::new(net, 1, 1, knots.clone(), knots).unwrap();
            Surface::from(BSplineSurface::from_gw_nurb(&n))
        };
        let p = Point3::new;
        let s = 20.0;
        let bottom = {
            let row = |x: f64| vec![c(x, 0.0, 0.0), c(x, s, 0.0)];
            let net = vec![row(0.0), row(5.0), row(5.0), row(s)];
            let n = Net::new(
                net,
                1,
                1,
                vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            Surface::from(BSplineSurface::from_gw_nurb(&n))
        };
        let surfaces = vec![
            bottom,
            bilinear(p(0.0, 0.0, s), p(s, 0.0, s), p(0.0, s, s)),
            bilinear(p(0.0, 0.0, 0.0), p(s, 0.0, 0.0), p(0.0, 0.0, s)),
            bilinear(p(0.0, s, 0.0), p(s, s, 0.0), p(0.0, s, s)),
            bilinear(p(0.0, 0.0, 0.0), p(0.0, s, 0.0), p(0.0, 0.0, s)),
            bilinear(p(s, 0.0, 0.0), p(s, s, 0.0), p(s, 0.0, s)),
        ];
        let mut b = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        {
            let mut state = ArenaNotifyState::new(&b);
            let mut env = state.env(None);
            let mut marks = crate::topology::topology_traverser::Marks::default();
            let this_face = |f: FaceId| crate::topology::loopuse::notify_ref(EntityRef::Face(f));
            let mut fenv = FaceBuildEnv {
                env: &mut env,
                marks: &mut marks,
                this_face: &this_face,
            };
            let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
            for s in surfaces {
                create_face_from_surface(k, &mut b, Some(s), &domain, &mut fenv).unwrap();
            }
        }
        p5::stitch_and_orient(k, &mut b, &p5::StitchAndOrientOptions::default()).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// The failure classes of `b`'s check ([`validation::soundness`], on a copy).
    fn failure_classes(k: &mut Kernel, b: &Brep) -> BTreeMap<FailureClass, usize> {
        let mut copy = b.clone_standing_for_self().unwrap();
        validation::soundness(k, &mut copy)
            .unwrap()
            .check
            .failure_classes()
    }

    /// The gate holds the operation to what it adds: a cube whose bottom fails its check
    /// on its own, its top raised by 5, is returned (the bottom untouched, its failure
    /// the input's), a manifold solid of the raised volume whose check has no failure
    /// class beyond the input's.
    #[test]
    fn a_failure_the_input_has_on_an_untouched_face_is_not_the_operations() {
        let mut k = Kernel::default();
        let mut b = cube_with_a_defective_bottom(&mut k);
        let had = failure_classes(&mut k, &b);
        assert_eq!(
            had.get(&FailureClass::Report {
                reporting_type: crate::geom::assert_array::labels::B_SPLINE_CURVE_TYPE,
                list_index: 0,
                test_index: 2,
                owner_type: crate::geom::assert_array::labels::B_SPLINE_CURVE_TYPE,
            }),
            Some(&2),
            "{had:?}"
        );
        let (v0, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v0 - 8000.0).abs() <= 1e-9 * 8000.0, "{v0}");
        let top = box_top(&k, &b);
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 25.0), Vec3::Z));
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[top], vec![raised], notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v - 10000.0).abs() <= 1e-9 * 10000.0, "{v}");
        let has = failure_classes(&mut k, &b);
        assert!(
            has.iter().all(|(c, n)| had.get(c).is_some_and(|m| n <= m)),
            "{has:?} vs {had:?}"
        );
    }

    /// The same cube, its top given a roof with a ridge: a degree-1 B-spline patch whose
    /// interior knot is a crease. The roof has no exact class, so the reach of the moved
    /// top has no certified bound: refused as `ReachUncertified` at the top, before any
    /// body is built, the cube as it came in with the input's own failures.
    #[test]
    fn a_creased_roof_on_a_defective_input_is_refused_as_reach_uncertified() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as Net;
        let mut k = Kernel::default();
        let mut b = cube_with_a_defective_bottom(&mut k);
        let had = failure_classes(&mut k, &b);
        assert!(!had.is_empty());
        let top = box_top(&k, &b);
        let row = |x: f64, z: f64| vec![CPoint::new(x, 0.0, z, NOW), CPoint::new(x, 20.0, z, NOW)];
        let net = vec![row(0.0, 25.0), row(10.0, 27.0), row(20.0, 25.0)];
        let roof = Net::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let roof = Surface::from(BSplineSurface::from_gw_nurb(&roof));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[top], vec![roof]);
        assert_eq!(why, Some(OffsetFailure::ReachUncertified { face: top }));
        assert_eq!(failure_classes(&mut k, &b), had);
    }

    /// [`BrepCheck::failures_beyond`](validation::BrepCheck::failures_beyond) counts per
    /// class: a class the input has as many lines of is not listed, one more line of it
    /// is, and a class the input lacks is.
    #[test]
    fn failures_beyond_counts_lines_per_class() {
        use crate::geom::assert_array::labels::SURFACE_TYPE;
        use crate::geom::assert_array::{AssertArray, AssertReport, ObjectKey, ReportSpec};
        // (each report about its own surface: the array keeps one of equal reports)
        let report = |test_index: u64, owner: u64| {
            AssertReport::new(ReportSpec {
                test_index,
                test_class: SURFACE_TYPE,
                owner: Some(ObjectKey(owner)),
                owner_type: SURFACE_TYPE,
                ..ReportSpec::default()
            })
            .unwrap()
        };
        let check = |tests: &[u64]| {
            let mut reports = AssertArray::new();
            for (owner, &t) in (0..).zip(tests) {
                assert!(reports.add_unique_delete_duplicate(report(t, owner)));
            }
            validation::BrepCheck {
                pointers_status: crate::error::SM_SUCCESS,
                pointer_reports: AssertArray::new(),
                assert_valid: Some(tests.is_empty()),
                reports,
            }
        };
        let input = check(&[4, 4, 6]);
        assert!(check(&[4, 6]).failures_beyond(&input).is_empty());
        assert!(check(&[4, 4, 6]).failures_beyond(&input).is_empty());
        let more = check(&[4, 4, 4, 6]).failures_beyond(&input);
        assert_eq!(more.len(), 1);
        assert_eq!(more.first().map(|&(_, n)| n), Some(1));
        let other = check(&[5]).failures_beyond(&input);
        assert!(matches!(
            other.as_slice(),
            [(FailureClass::Report { test_index: 5, .. }, 1)]
        ));
    }

    /// A 20 mm cube with its vertical edge at x = y = 20 rounded r 5.
    fn cube_rounded_r5(k: &mut Kernel) -> Brep {
        use crate::blend::fillet_executive::api::fillet_edges;
        use crate::topology::{edge, vertex};
        let mut b = create_box(k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let e = b
            .edges()
            .ids()
            .find(|&e| {
                let (s, t) = edge::start_end_vertices(&b, e).unwrap();
                [s, t].into_iter().flatten().all(|v| {
                    let p = vertex::point(&b, v).unwrap();
                    p.x == 20.0 && p.y == 20.0
                })
            })
            .unwrap();
        fillet_edges(k, &mut b, &[e], 5.0, 1, 1, 1.0).unwrap();
        b
    }

    /// The face of `b` on the plane x = `x`.
    fn face_on_x(k: &Kernel, b: &Brep, x: f64) -> FaceId {
        bp1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(b, f).unwrap().unwrap();
                matches!(s.kind(), SurfaceKind::Plane(_)) && {
                    let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
                    let p = s.evaluate_point(k, uv).unwrap();
                    let n = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
                    (p.x - x).abs() < 1e-9 && n.x.abs() > 0.99
                }
            })
            .unwrap()
    }

    /// `∫ √(R² − u²) du` from 0 to `u`.
    fn quarter_disc_area(radius: f64, u: f64) -> f64 {
        0.5 * (u * (radius * radius - u * u).sqrt() + radius * radius * (u / radius).asin())
    }

    /// The refusal of a body not sound by its check, `failures` lines beyond the input's,
    /// the first test `test` of an `SmBrep` (16011).
    fn unsound_check(failures: usize, test: u64) -> OffsetFailure {
        OffsetFailure::Unsound {
            what: crate::error::offset::Unsoundness::Check {
                failures,
                first: Some((crate::geom::a_object::BREP_TYPE, test)),
            },
        }
    }

    /// The cube's round r 5 given the coaxial cylinder r 5.1 (moved out 0.1).
    fn round_moved_out(k: &mut Kernel) -> (Brep, Option<OffsetFailure>) {
        use crate::primitive::api_primitives as ap;
        let mut b = cube_rounded_r5(k);
        let round = curved_face(&b);
        let wider = Surface::from(
            ap::create_cylinder_surface(k, Point3::new(15.0, 15.0, -1.0), 5.1, 22.0).unwrap(),
        );
        let why = refused_unchanged_or_returned(k, &mut b, &[round], vec![wider]);
        (b, why)
    }

    /// The cube's side x = 20, tangent to the round, moved into the body by 0.1.
    fn side_moved_into_its_round(k: &mut Kernel) -> (Brep, Option<OffsetFailure>) {
        let mut b = cube_rounded_r5(k);
        let side = face_on_x(k, &b, 20.0);
        let moved = Surface::from(Plane::from_normal(Vec3::new(19.9, 0.0, 0.0), Vec3::X));
        let why = refused_unchanged_or_returned(k, &mut b, &[side], vec![moved]);
        (b, why)
    }

    /// The round moved out. The side planes, tangent to the round, are
    /// extended to meet it 1.005 past their old edges (`reach`'s tests), but the tangent
    /// edge across from the moved face is extended and trimmed, not met again: it keeps
    /// its old line, the merge closes the gap by growing tolerances, and the check fails
    /// (issue draft "a moved face next to a tangent edge (a round) is refused as
    /// unsound"): refused, the cube as it came in.
    #[test]
    fn a_round_moved_out_is_refused_as_not_sound_at_its_tangent_edges() {
        let mut k = Kernel::default();
        let (_, why) = round_moved_out(&mut k);
        assert_eq!(why, Some(unsound_check(7, 61)));
    }

    /// The closed form the round moved out should have, once the tangent edges are met
    /// again (issue draft "a moved face next to a tangent edge (a round) is refused as
    /// unsound"): the cube with its corner cut by the r 5.1 circle, `20 (375 + A)`, `A`
    /// the part of the corner square [15, 20]² inside the circle.
    #[test]
    #[ignore = "a moved face next to a tangent edge (a round) is refused as unsound \
                (notes: issue-draft-tangent-edges.md)"]
    fn tangent_edges_a_round_moved_out_has_its_closed_form_volume() {
        let mut k = Kernel::default();
        let (mut b, why) = round_moved_out(&mut k);
        assert_eq!(why, None);
        let r = 5.1_f64;
        let w = (r * r - 25.0).sqrt();
        let inside = 5.0 * w + quarter_disc_area(r, 5.0) - quarter_disc_area(r, w);
        let want = 20.0 * (375.0 + inside);
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    }

    /// The side moved into its round: it meets the round 1 from its old tangent edge
    /// (`reach`'s tests), and the same tangent edges leave the body unsound: refused, the
    /// first new failure a vertex out of tolerance (test #59).
    #[test]
    fn a_side_moved_into_its_round_is_refused_as_not_sound_at_its_tangent_edges() {
        let mut k = Kernel::default();
        let (_, why) = side_moved_into_its_round(&mut k);
        assert_eq!(why, Some(unsound_check(4, 59)));
    }

    /// The closed form the side moved into its round should have (issue draft "a moved
    /// face next to a tangent edge (a round) is refused as unsound"):
    /// `20 (19.9 · 20 − 4.9 · 5 + ∫₀^4.9 √(25 − u²) du)`.
    #[test]
    #[ignore = "a moved face next to a tangent edge (a round) is refused as unsound \
                (notes: issue-draft-tangent-edges.md)"]
    fn tangent_edges_a_side_moved_into_its_round_has_its_closed_form_volume() {
        let mut k = Kernel::default();
        let (mut b, why) = side_moved_into_its_round(&mut k);
        assert_eq!(why, None);
        let want = 20.0 * (19.9 * 20.0 - 4.9 * 5.0 + quarter_disc_area(5.0, 4.9));
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold && (v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    }

    /// The same side moved out by 0.1: the plane x = 20.1 passes the round, which bends
    /// away from it, so no extension makes them meet: their edge is apart
    /// (`LocalReach::apart`, adding nothing to the reach), and the body built is refused
    /// as not sound, the cube as it came in.
    #[test]
    fn a_side_moved_off_its_round_is_refused_as_not_sound() {
        let mut k = Kernel::default();
        let mut b = cube_rounded_r5(&mut k);
        let side = face_on_x(&k, &b, 20.0);
        let moved = Surface::from(Plane::from_normal(Vec3::new(20.1, 0.0, 0.0), Vec3::X));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[side], vec![moved]);
        assert!(
            matches!(why, Some(OffsetFailure::Unsound { .. })),
            "{why:?}"
        );
    }

    /// A tangent pair of equal curvature: a 60 x 40 x 20 block whose top is imprinted
    /// along x = 30, the piece x < 30 raised 1. The two pieces' planes stay parallel and
    /// 1 apart: they meet nowhere, so their edge is apart (`LocalReach::apart`, adding
    /// nothing to the reach), and without the step walls, which build the wall between
    /// them (`wr_530_local_operation_step_walls`), the body built is refused as not sound.
    #[test]
    fn a_coplanar_neighbour_raised_off_its_twin_is_refused_as_not_sound() {
        use crate::boolean::api_brep::boolean_with_options;
        use crate::boolean::merge::MergeOptions;
        use crate::geom::a_object::BooleanOperation;
        let mut k = Kernel::default();
        let block = create_box(&mut k, Point3::ZERO, 60.0, 40.0, 20.0).unwrap();
        let cutter = create_box(&mut k, Point3::new(30.0, -1.0, 10.0), 60.0, 42.0, 20.0).unwrap();
        let mut b = boolean_with_options(
            &mut k,
            block,
            cutter,
            BooleanOperation::Imprint,
            MergeOptions::default(),
        )
        .unwrap()
        .result;
        b.refresh_cached_topology();
        let piece = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let face = b.face(f).unwrap();
                let s = face.surface.as_ref().unwrap();
                let p = s
                    .evaluate_point(&k, face.uv_domain.evaluate(0.5, 0.5))
                    .unwrap();
                matches!(s.kind(), SurfaceKind::Plane(_)) && (p.z - 20.0).abs() < 1e-9 && p.x < 30.0
            })
            .unwrap();
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 21.0), Vec3::Z));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[piece], vec![raised]);
        assert!(
            matches!(why, Some(OffsetFailure::Unsound { .. })),
            "{why:?}"
        );
    }
}
