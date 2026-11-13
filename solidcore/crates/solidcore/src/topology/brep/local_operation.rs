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
//! public operation leaves them. (not upstream) A plane given to a lone lamina face
//! that is exactly parallel and the same way round as the face's own plane takes the
//! face's own surface moved along its normal onto it
//! ([`moved_surface_on_own_frame`]): such a face's boundary is its own UV trims lifted
//! onto the surface, so keeping the face's own parameterisation keeps that lift the
//! pointwise normal translation the move is, instead of turning the boundary by the
//! given plane's frame. This holds for bare surfaces only (neither carries a procedural
//! definition): a definition's net alone does not prove its plane, and moving the net
//! would leave the definition behind. (not upstream, WR-777) A given surface is turned by
//! its normal where it passes nearest the old face's middle, not at its own domain's
//! middle, which for a full cylinder given to a round lies across the axis.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::curve::Curve;
use crate::error::offset::{BoundaryFault, OffsetFailure, ReconstructionRefusal};
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::geom::attribute::{Attribute, Behavior, ObjectKey};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::expansion as exact;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::mass::enclosure::VolumeSign;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface::p1::{BSplineSurface, SurfParamType};
use crate::nurbs::knots::Side;
use crate::nurbs::math::{CPoint, NOW};
use crate::offset::offset_executive::clearance::{self, KeptEdge, MovedFace};
use crate::offset::offset_executive::deferred_edge_vanish;
use crate::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use crate::offset::offset_executive::p2::OffsetNotifications;
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::surface::implicit::{self, AnalyticSupport};
use crate::surface::plane::Plane;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p2;
use crate::topology::brep::p5;
use crate::topology::brep::validation::{self, FailureClass};
use crate::topology::face::p2 as face_ops;
use crate::topology::face::p4 as face4;
use crate::topology::{Brep, FaceId};

pub use crate::offset::offset_executive::p2::local_operation_simple;

const OP: &str = "topology::brep::local_operation::local_operation";

/// `what` is missing (upstream `SM_ERR_NULL_POINTER`).
fn null(what: &'static str) -> SolidError {
    SolidError::topology_missing(OP, what)
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
        return Err(null("a surface for every face"));
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

/// [`local_operation`] on the copy of `b` it edits; [`local_operation_recorded`] for
/// the reconstruction's records.
fn local_operation_on(
    k: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    surfaces: Vec<Surface>,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    local_operation_recorded(k, b, faces, surfaces, notifications).map(|_| ())
}

/// [`local_operation_on`] with the crossed-boundary reconstruction's records (issue
/// #13622) returned: the events it fired, the correspondence map and the
/// endpoint-contact proofs the kept-edge gate takes.
fn local_operation_recorded(
    k: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    surfaces: Vec<Surface>,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<Reconstruction> {
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
        let own = face_ops::surface(b, face)?.ok_or_else(|| null("the face's surface"))?;
        if let Some(new) = index.and_then(|i| given.get(i)).and_then(Option::as_ref) {
            if !is_own_surface(own, new) {
                moving.push(face);
            }
        }
    }
    let mut continued = continued_planes(k, b, &all_faces, &moving)?;
    for &face in &all_faces {
        let index = faces.iter().position(|&f| f == face);
        let own = face_ops::surface(b, face)?.ok_or_else(|| null("the face's surface"))?;
        let replacement = match index {
            Some(index) if moving.contains(&face) => {
                if let Some(slot) = placed.get_mut(index) {
                    *slot = Some(new_list.len());
                }
                Some(
                    given
                        .get_mut(index)
                        .and_then(Option::take)
                        .ok_or_else(|| null("the face's given surface"))?,
                )
            }
            _ => continued.remove(&face),
        };
        let surface = if let Some(mut new) = replacement {
            moved.push(face);
            // (not upstream) a lone lamina face's boundary is its own UV trims lifted onto
            // its surface (`build_contained_offset_faces`): a replacement plane exactly
            // parallel and the same way round as its own plane takes the face's own
            // surface moved along its normal onto that plane, so the lift is the pointwise
            // normal translation; the given plane's unrelated frame would turn the
            // boundary about its normal. Any other surface is used as it came.
            if a_lone_lamina_face(b, face)? {
                // not this construction (the given surface is no exactly parallel plane
                // of the face's own): it is used as it came
                if let Some(on_the_own_frame) = moved_surface_on_own_frame(k, own, &new)? {
                    new = on_the_own_frame;
                }
            }
            let domain = new.natural_uv_domain_if_built();
            let old = own;
            let old_uv = b.face(face)?.uv_domain.evaluate(0.5, 0.5);
            // (not upstream, which takes the middle of the new surface's domain) the new
            // normal where the new surface passes nearest the old face's middle: a full
            // cylinder's domain middle is across the axis from a round's middle, so the
            // two normals there oppose though the surfaces face the same way. Where there
            // is no nearest point, or no normal at it (a cone's apex), upstream's point is
            // kept: the new rule only replaces it where it applies.
            // (`EvaluateNormal(uv, TRUE, TRUE, ..)`: from the left)
            let nearest = match new.drop_point::<NoObject>(
                k,
                old.evaluate_point(k, old_uv)?,
                &domain,
                &DropPointOptions::default(),
            ) {
                Ok(dropped) if dropped.success => {
                    Some(new.evaluate_normal(k, dropped.result_uv, Side::Left, Side::Left))
                }
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            };
            let new_normal = match nearest {
                Some(Ok(n)) => n,
                Some(Err(e)) if e.is_unported_or_cancelled() => return Err(e),
                _ => new.evaluate_normal(k, domain.evaluate(0.5, 0.5), Side::Left, Side::Left)?,
            };
            let old_normal = old.evaluate_normal(k, old_uv, Side::Left, Side::Left)?;
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
                (None, Some(at)) => new_list
                    .get(*at)
                    .ok_or_else(|| null("the placed new surface"))?,
                (None, None) => continue,
            };
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            let surf_box = surface
                .calculate_bounding_box(k, &domain, [true, false, false], true)?
                .normal_box
                .ok_or_else(|| null("the new surface's bounding box"))?;
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
    // vertices' faces all keep their surfaces, and whose whole curve is certified clear of
    // every moved surface (a ball enclosure of each curve cell against the surface's
    // certified distance bound, rounding counted: `clearance::separated`, #2971), are
    // kept as they are, not intersected again (`OffsetExecutive::keeps_edge`).
    executive.set_local_surfaces(&all_faces, new_list, &moved);
    // the shell faces get copied without being offset: none
    executive.set_shell_faces(&[]);
    // extend/intersect to fill new convex edge gaps rather than adding a fillet
    executive.set_extend_convex_edges(true);

    // (not upstream) each moved face carries a private mark through the operation, as a
    // caller's mark does (`Behavior::Copy`, found on the result's faces below): the
    // faces of the result that lie where the operation moved, with their trimmed regions,
    // are what the kept edges must stay clear of (`refuse_kept_edges_not_clear`), and
    // each original edge carries its arena index the same way (issue #13622's
    // correspondence map, [`mark_original_edges`])
    for &face in &moved {
        b.add_attribute(
            EntityRef::Face(face),
            Attribute::long(MOVED_FACE_MARK, 1, Behavior::Copy),
        )?;
    }
    mark_original_edges(b)?;
    // (issue #13622) the edges whose faces all keep their surfaces: the ones a moved
    // face can cross (the defect), whose events the reconstruction classifies. An edge
    // beside a moved face is rebuilt by the executive and reaches the repair only
    // through the cut corner's gaps.
    let mut crossable: BTreeSet<crate::topology::EdgeId> = BTreeSet::new();
    for e in bp1::edges(b, None)? {
        let faces = crate::topology::edge::faces(b, e, None)?;
        if faces.iter().all(|f| !moved.contains(f)) {
            crossable.insert(e);
        }
    }
    // (issue #13622's hook) the eligibility provider: the gap pass defers the vanish of
    // an edge the reconstruction owns (its original in `crossable`, its correspondence
    // complete and unambiguous) instead of refusing it, and this operation consumes the
    // obligations below. A caller that provides no eligibility — an offset or a shell —
    // defers nothing.
    executive.set_reconstruction_eligibility(crossable.clone());

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
    // the edges it kept, as they were on `b` (the faces of `b` go with `move_faces`)
    let kept = match &offset {
        Ok(_) => executive.kept_edge_curves(b),
        Err(_) => Ok(Vec::new()),
    };
    // (issue #13622's hook) the gap pass's vanish obligations, collected with the kept
    // edges before the executive goes: the reconstruction discharges each one (its
    // removal certified, every corresponding result edge severed) or refuses typed
    let vanished = match &offset {
        Ok(_) => executive.pending_vanishes().clone(),
        Err(_) => BTreeSet::new(),
    };
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
    let kept = kept?;

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
    // (issue #13622) the crossed-boundary reconstruction: the correspondence map from
    // the marks the copy carried, then the repair of every boundary a moved face
    // crossed (before the kept-edge gate, whose kept list it shrinks to the edges it
    // left as they were)
    let map = capture_edge_correspondence(b)?;
    let reconstruction =
        reconstruct_crossed_boundaries(k, b, &map, &kept, &crossable, &vanished, notifications)?;
    refuse_kept_edges_not_clear(k, b, &kept, &reconstruction)?;
    refuse_unsound(k, b, &input)?;
    Ok(reconstruction)
}

/// The id of the private mark the moved faces carry through a local operation (below the
/// application range, which starts above 10000).
const MOVED_FACE_MARK: u64 = 9_999;

/// (not upstream) The kept edges are proved clear of the faces the operation moved, or the
/// operation is refused ([`Unsoundness::KeptEdgeNotClear`](crate::error::offset::Unsoundness)),
/// the input as it came in.
///
/// The body's check has no face-face intersection test, so a face that crosses an edge
/// the operation kept (an edge whose faces and vertices' faces all keep their surfaces: the
/// executive does not intersect it again) leaves a self-intersecting body it cannot see. The
/// certified bound (issue #2971): over the whole curve of each kept edge, a ball enclosure
/// of each cell, rounding counted, and a surface distance lower bound to the part of each
/// moved face's surface over the box of its trim curves in its parameters
/// ([`clearance::all_clear`], `surface_distance_lower_bound`), must exceed the sum of the
/// edge's and the face's tolerances. A moved face is found by its mark on the result; its
/// trimmed region is the surface's part over its loops' boxes (the whole surface when a
/// loop has none), not the surface's extension, which a plane's net makes wider than any
/// part. No edge is intersected again: an edge not proved clear refuses the operation.
///
/// A kept edge the crossed-boundary reconstruction rebuilt (issue #13622) is not kept
/// as it was: its crossing became a certified clip at its end
/// ([`Reconstruction::proofs`], the records the kept-edge gate is wired to next), so it
/// drops out of this list and only the edges left untouched are held clear.
fn refuse_kept_edges_not_clear(
    k: &mut Kernel,
    b: &mut Brep,
    kept: &[(crate::topology::EdgeId, KeptEdge)],
    reconstruction: &Reconstruction,
) -> Result<()> {
    use crate::error::offset::Unsoundness;
    use crate::topology::loopuse;
    // the marks come off the result whatever follows
    let mut moved_faces = Vec::new();
    for face in bp1::faces(b, None)? {
        let handles: Vec<_> = b
            .entity_attributes(EntityRef::Face(face))
            .into_iter()
            .filter(|(_, a)| a.attribute_id() == MOVED_FACE_MARK)
            .map(|(h, _)| h)
            .collect();
        for h in &handles {
            b.remove_attribute(EntityRef::Face(face), *h)?;
        }
        if !handles.is_empty() {
            moved_faces.push(face);
        }
    }
    let kept: Vec<KeptEdge> = kept
        .iter()
        .filter(|(e, _)| !reconstruction.reconstructed_kept().contains(e))
        .map(|(_, edge)| edge.clone())
        .collect();
    // a reconstructed kept edge's crossing is certified by its recorded endpoint
    // contacts (`Reconstruction::proofs`, the records the gate is wired to): every
    // SURVIVING rebuilt result edge of the group carries its OWN
    // `ContactProof::Proved` (lamina twins map one original to several result edges,
    // and one twin's proof never certifies another), while a severed or removed edge
    // needs no proof (it is not in the result). A group where one twin has a proof and
    // another surviving rebuilt twin does not refuses.
    for original in reconstruction.reconstructed_kept() {
        for result_edge in reconstruction
            .map()
            .get(original)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if !b.edges().iter().any(|(x, _)| x == *result_edge) {
                // severed or removed: gone from the result, no proof to carry
                continue;
            }
            let proved = reconstruction.proofs().iter().any(|(e, proof)| {
                e == result_edge
                    && matches!(proof, clearance::endpoint_contact::ContactProof::Proved(_))
            });
            if !proved {
                return Err(Failure::new(
                    OP,
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::ClipUncertified { edge: *original },
                    },
                )
                .into_error());
            }
        }
    }
    if kept.is_empty() || moved_faces.is_empty() {
        return Ok(());
    }
    // (the trim curves a face lacks are made on a private copy)
    let mut copy = b.clone_standing_for_self()?;
    let mut faces = Vec::new();
    for face in moved_faces {
        let surface = face_ops::surface(&copy, face)?
            .ok_or_else(|| null("the face's surface"))?
            .clone();
        let mut uv_box: Option<Extent2d> = None;
        for l in face_ops::loops(&copy, face, false, None)? {
            let Some(span) = loopuse::loop_uv_span(k, &mut copy, l)? else {
                uv_box = None;
                break;
            };
            uv_box = Some(uv_box.map_or(span, |u| u.union(&span)));
        }
        faces.push(MovedFace {
            surface,
            uv_box,
            tolerance: copy.face(face)?.tolerance.max(copy.tolerance()),
        });
    }
    if clearance::all_clear(k, &kept, &faces)? {
        Ok(())
    } else {
        Err(Failure::new(
            OP,
            OffsetFailure::Unsound {
                what: Unsoundness::KeptEdgeNotClear,
            },
        )
        .into_error())
    }
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
/// - the body is a manifold solid whose volume is not proved positive: its certified
///   enclosure lies below 0, or still holds 0 after its budget
///   ([`validation::volume_sign`]; the input's volume is not measured). A face whose net
///   is certified within ε > 0 of its definition (a Gⁿ or law round, a sweep, a Mixed
///   corner) is read on its net, so the gate decides on the body as stored; widening the
///   enclosure by ε needs bounds on the definition's `S_u` and `S_v` (#3170).
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
    } else if found.manifold {
        match validation::volume_sign(k, &mut copy)? {
            VolumeSign::Positive(_) => return Ok(()),
            VolumeSign::Negative(_) => Unsoundness::VolumeNotPositive,
            VolumeSign::Undecided(_) => Unsoundness::VolumeUndecided,
        }
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
fn uv_line(k: &Kernel, curve: &Curve) -> Result<Option<UvLine>> {
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
    domain: &Extent2d,
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
    domain: &Extent2d,
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
                    return Err(null("the scratch copy of the B-rep"));
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

/// Whether every edge of `face` is lamina (`SmEdge::IsLamina`): the face is a lone
/// sheet's face, its two face uses the same face, so no edge of it is rebuilt from
/// another face's surface; the face constructor lifts the face's own UV trims onto
/// whatever surface it is given.
fn a_lone_lamina_face(b: &Brep, face: FaceId) -> Result<bool> {
    let edges = face4::edges(b, face, None, false, None)?;
    if edges.is_empty() {
        return Ok(false);
    }
    for e in edges {
        if !crate::topology::edgeuse::p1::edge_is_lamina(b, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The origin of `p`'s plane moved along `q`'s rounded normal onto `q`'s plane: `None`
/// unless both frames span planes, `p`'s axes lie in `q`'s, their orientations agree, and
/// the distance and moved origin are finite. Containment and displacement use exact
/// determinants of `q`'s stored axes, not perpendicularity to its rounded normal.
fn own_plane_moved_origin(p: &Axis2Placement, q: &Axis2Placement) -> Option<Vec3> {
    let (a, b, o) = (p.x_axis(), p.y_axis(), p.origin());
    let (x, y, m, g) = (q.x_axis(), q.y_axis(), q.z_axis(), q.origin());
    if !spans(p)? || !spans(q)? {
        return None;
    }
    // parallel: own's axes lie in the given plane
    if !exact::is_zero(&exact::det3(x, y, a)?) || !exact::is_zero(&exact::det3(x, y, b)?) {
        return None;
    }
    // the same way round: (a × b) · given_z is positive, with no unresolved overflow
    let orientation = exact::det3(a, b, m)?;
    if !orientation.iter().all(|c| c.is_finite()) || exact::sign(&orientation) <= 0 {
        return None;
    }
    // det(x, y, o + m*d) = det(x, y, g), using separate determinants rather than
    // rounding g-o before the proof. m need not be exactly perpendicular to x and y.
    let numerator = exact::to_f64(&exact::sub(&exact::det3(x, y, g)?, &exact::det3(x, y, o)?));
    let denominator_expansion = exact::det3(x, y, m)?;
    if exact::sign(&denominator_expansion) <= 0 {
        return None;
    }
    let denominator = exact::to_f64(&denominator_expansion);
    if !(numerator.is_finite() && denominator.is_finite() && denominator > 0.0) {
        return None;
    }
    let distance = numerator / denominator;
    if !distance.is_finite() {
        return None;
    }
    let origin = o + m * distance;
    [origin.x, origin.y, origin.z]
        .iter()
        .all(|c| c.is_finite())
        .then_some(origin)
}

/// The translation along `q`'s rounded normal that moves `net`'s plane onto `q`'s plane.
/// Exact determinants of `q`'s stored axes prove the homogeneous control points have
/// the same plane coordinate per positive finite weight. The displacement uses the
/// matching determinant ratio; its doubles are estimates ([`exact::to_f64`]). The sense
/// is `own`'s evaluated normal against `q`'s rounded normal, finite and positive.
fn net_moved_translation(
    net: &BSplineSurface,
    k: &Kernel,
    own: &Surface,
    q: &Axis2Placement,
) -> Option<Vec3> {
    let (x, y, m, g) = (q.x_axis(), q.y_axis(), q.z_axis(), q.origin());
    // a degenerate given frame has no normal to be parallel to (`z_axis` is (0, 0, 1) for
    // parallel or zero axes)
    if !spans(q)? {
        return None;
    }
    // det(x, y, H_i)*w0 = det(x, y, H_0)*wi, not H_i dot m: unitizing the
    // cross product rounds m, so its exact perpendicular plane can differ from q's.
    let nurb = net.gw_nurb()?;
    let weight = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
    let first = nurb.rows().flatten().next()?;
    let (w0, a0) = (
        weight(first),
        exact::det3(x, y, Vec3::new(first.x, first.y, first.z))?,
    );
    if !(w0.is_finite() && w0 > 0.0) {
        return None;
    }
    for point in nurb.rows().flatten() {
        let wi = weight(point);
        if !(wi.is_finite() && wi > 0.0) {
            return None;
        }
        let ai = exact::det3(x, y, Vec3::new(point.x, point.y, point.z))?;
        let common = exact::sub(&exact::scale(&ai, w0)?, &exact::scale(&a0, wi)?);
        if !exact::is_zero(&common) {
            return None;
        }
    }
    // a net whose normal cannot be evaluated is not shown this construction
    let domain = own.natural_uv_domain_with_nurb(k).ok()?;
    let own_normal = own
        .evaluate_normal(k, domain.mid(), Side::Left, Side::Left)
        .ok()?;
    let sense = own_normal.dot(m);
    if !(sense.is_finite() && sense > 0.0) {
        return None;
    }
    // det(x,y,H_i + wi*m*d) = wi*det(x,y,g), so
    // d = (w0*det(x,y,g) - det(x,y,H_0)) / (w0*det(x,y,m)).
    let numerator = exact::to_f64(&exact::sub(&exact::scale(&exact::det3(x, y, g)?, w0)?, &a0));
    if !numerator.is_finite() {
        return None;
    }
    let denominator_expansion = exact::scale(&exact::det3(x, y, m)?, w0)?;
    if exact::sign(&denominator_expansion) <= 0 {
        return None;
    }
    let denominator = exact::to_f64(&denominator_expansion);
    if !(denominator.is_finite() && denominator > 0.0) {
        return None;
    }
    let distance = numerator / denominator;
    if !distance.is_finite() {
        return None;
    }
    let translation = m * distance;
    [translation.x, translation.y, translation.z]
        .iter()
        .all(|c| c.is_finite())
        .then_some(translation)
}

/// The face's own surface moved along its normal onto `given`'s plane, when `given` is a
/// plane exactly parallel and the same way round as the face's own surface's plane: a
/// lone lamina face's boundary is its own UV trims lifted onto its surface, so on this
/// surface the lift is the pointwise normal translation the two planes define, not a
/// boundary turned by `given`'s frame (a `Plane::from_normal`'s canonical axes are not
/// the face's).
///
/// Own is a plane, or a B-spline net lying in one:
/// - a plane keeps its placement — its origin moved along its normal onto the given
///   plane, its axes, UV scale and analytic domain;
/// - a net is an exact copy (no knot fix-up from a copy constructor) moved by that
///   translation in its stored homogeneous control points, `H_i + w_i t`, its knots,
///   weights and parameterisation unchanged.
///
/// Either is the given plane to the rounding of that translation distance (a double
/// estimate, see [`exact::to_f64`]), and the face's UV trims lift to its own boundary
/// translated.
///
/// Certified exactly ([`exact`]) on the stored values: for a plane, both frames span a
/// plane, the own axes lie in the given one and `(x × y) · given_z` is positive; for a
/// net, `det(q.x, q.y, H_i) * w_0 = det(q.x, q.y, H_0) * w_i` for all homogeneous
/// control points with positive finite weights, so its plane is parallel to the given
/// stored axes. Both move along the finite rounded normal `m = q.z`, using the matching
/// ratio `(w_0 * det(q.x,q.y,g) - det(q.x,q.y,H_0)) / (w_0 * det(q.x,q.y,m))`
/// (a plane takes `H_0 = origin`, `w_0 = 1`). The denominator is exactly positive;
/// the ratio and translated coordinates must be finite. The net's orientation is its
/// evaluated normal at the natural domain's middle against `m`, finite and positive.
/// `None` for every other surface: a non-plane given
/// surface, an own or given surface with a procedural definition (its net alone is not
/// proof of a bare plane, and moving the net would not move the definition), a plane not
/// exactly parallel, one the other way round, a net not in such a plane, a zero normal, a
/// non-finite numerator, denominator, distance, origin or translated control point, or a
/// product out of the exact range; the caller's surface is then used as it came.
///
/// # Errors
///
/// Those of the net's NURB access.
fn moved_surface_on_own_frame(
    k: &Kernel,
    own: &Surface,
    given: &Surface,
) -> Result<Option<Surface>> {
    // a surface that is the net of a kept definition is not proved a bare plane by that
    // net, and moving the net alone would not move its definition: only a bare own and
    // given surface take this construction
    if own.procedural_definition().is_some() || given.procedural_definition().is_some() {
        return Ok(None);
    }
    let SurfaceKind::Plane(given_plane) = given.kind() else {
        return Ok(None);
    };
    let q = given_plane.position();
    match own.kind() {
        SurfaceKind::Plane(own) => {
            let p = own.position();
            let Some(origin) = own_plane_moved_origin(p, q) else {
                return Ok(None);
            };
            Ok(Some(Surface::from(Plane::new(
                origin,
                p.x_axis(),
                p.y_axis(),
                own.uv_scale(),
                &own.step_uv_domain(),
            ))))
        }
        SurfaceKind::BSpline(net) => {
            let Some(translation) = net_moved_translation(net, k, own, q) else {
                return Ok(None);
            };
            // the copy is exact (no knot fix-up), so the face keeps its own
            // parameterisation; the move is on the stored homogeneous control points,
            // H_i + w_i t, which leaves knots and weights as they were
            let mut moved = net.clone_exact();
            {
                let nurb = moved.get_or_create_gw_nurb()?;
                let (nu, nv) = (nurb.n(), nurb.m());
                for i in 0..=nu {
                    for j in 0..=nv {
                        let point = nurb.cpt_mut(i, j)?;
                        let w = if point.w == NOW { 1.0 } else { point.w };
                        let (x, y, z) = (
                            point.x + w * translation.x,
                            point.y + w * translation.y,
                            point.z + w * translation.z,
                        );
                        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                            return Ok(None);
                        }
                        point.x = x;
                        point.y = y;
                        point.z = z;
                    }
                }
            }
            Ok(Some(Surface::from(moved)))
        }
        _ => Ok(None),
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

// ---- The crossed-boundary reconstruction (issue #13622) --------------------------
//
// A local operation whose moved face crosses an edge that keeps its surfaces is not
// refused and not returned with the crossing kept: the boundary around the crossing is
// reconstructed after `move_faces` (`reconstruct_crossed_boundaries`), from certified
// clips and completed trims only:
//
// 1. every edge whose faces all keep their surfaces (the issue's "an edge that keeps
//    its surfaces") is classified against every moved face (`classify_edge_crossing`):
//    clear, trimmed at a certified clip point on the solid's side of the cut, or
//    removed where the cut leaves nothing;
// 2. the removed edges' lamina twins resolve (`merge_lamina_twins`) and sever
//    (`sever_edge`), the cut corner's diverging ends are split, re-trimmed to their
//    intended points and glued there (`split_vertex`, `retrim_edge`,
//    `p5::glue_vertices`), and a bridge edge closes each corner gap (`insert_bridge`);
// 3. the region pair `move_faces` dropped is restored (`restore_region_pair`), the
//    affected tolerances are re-measured, never grown
//    (`tighten_reconstruction_tolerances`), and the completed boundary is validated
//    (`validate_completed_boundary`) before anything is returned.
//
// The pass records, per rebuilt edge, its endpoint-contact proof
// (`clearance::endpoint_contact::separated_ending_on_face`) for the kept-edge gate. The
// correspondence map names the result edges of each original (the private
// [`CORRESPONDENCE_MARK`] marks, removed before the operation returns). Anything the
// pattern cannot certify refuses typed (`OffsetFailure::Reconstruction`): nothing
// uncertain is ever built. The cut is certified on a plane only (the endpoint
// certificate's documented scope): where a non-planar moved face leaves a cut
// undecided, the reconstruction refuses `SurfaceUncertified` when it engages, and does
// not engage otherwise (the gates decide, as before).

/// The id of the private mark naming the original edge a result edge was rebuilt from
/// (below the application range, which starts above 10000). It is carried through the
/// operation as a caller's mark is ([`Behavior::Copy`], the [`MOVED_FACE_MARK`] pattern
/// on edges): added to every original edge before the offset, copied onto the offset
/// edges at the executive's edge match (`notify_edge_and_vertices`'s `Copy`), and copied
/// again by `move_faces`' merge onto the result's edges. Read and removed by
/// [`capture_edge_correspondence`].
const CORRESPONDENCE_MARK: u64 = 9_998;

/// The original-edge correspondence of one local operation (issue #13622): each
/// original edge to the result edges carrying its mark. One original may
/// map to several result edges (the lamina twins of a rebuilt edge); a result edge
/// names at most one original (one mark id per object).
pub(crate) type EdgeMap = BTreeMap<crate::topology::EdgeId, Vec<crate::topology::EdgeId>>;

/// Marks every original edge of `b` with its arena index ([`CORRESPONDENCE_MARK`]),
/// beside the moved faces' [`MOVED_FACE_MARK`]s.
fn mark_original_edges(b: &mut Brep) -> Result<()> {
    let edges: Vec<_> = b.edges().iter().map(|(e, _)| e).collect();
    for e in edges {
        b.add_attribute(
            EntityRef::Edge(e),
            Attribute::long(CORRESPONDENCE_MARK, i64::from(e.index()), Behavior::Copy),
        )?;
    }
    Ok(())
}

/// The correspondence map of the result's edges, read from their marks; the marks are
/// removed on the way (they are private to the operation and no mark outlives it: a
/// refusal rolls the body back with them, a success strips them here).
fn capture_edge_correspondence(b: &mut Brep) -> Result<EdgeMap> {
    let mut out: EdgeMap = BTreeMap::new();
    for e in b.edges().iter().map(|(e, _)| e).collect::<Vec<_>>() {
        let handles: Vec<_> = b
            .entity_attributes(EntityRef::Edge(e))
            .into_iter()
            .filter(|(_, a)| a.attribute_id() == CORRESPONDENCE_MARK)
            .map(|(h, a)| (h, a.long_value()))
            .collect();
        for (h, value) in handles {
            if let Some(index) = value.and_then(|i| u32::try_from(i).ok()) {
                out.entry(crate::topology::EdgeId::from_index(index))
                    .or_default()
                    .push(e);
            }
            b.remove_attribute(EntityRef::Edge(e), h)?;
        }
    }
    for es in out.values_mut() {
        es.sort_unstable();
        es.dedup();
    }
    Ok(out)
}

/// A moved result face and everything the reconstruction needs about it (issue #13622):
/// the surface over the box of its trim curves (the [`MOVED_FACE_MARK`] as
/// [`refuse_kept_edges_not_clear`] reads it), its trimmed region as the loops' UV trim
/// curves (the clip's region membership), and its cut: the plane's origin and OUTWARD
/// normal (the solid stays where `n · (p - origin) <= 0`), `None` for a face that is not
/// planar (the endpoint certificate's documented scope).
struct MovedFaceCut {
    face: FaceId,
    moved: MovedFace,
    region: clearance::endpoint_contact::TrimRegion,
    plane: Option<PlaneCut>,
}

/// A planar moved face's cut.
#[derive(Debug, Clone, Copy)]
struct PlaneCut {
    origin: Point3,
    normal: Vec3,
}

impl PlaneCut {
    /// The moved face's plane, oriented outward (the face's surface normal, turned as
    /// its primary face use is); `None` for a face that is not planar.
    fn of(surface: &Surface, outward_same: bool) -> Option<Self> {
        let SurfaceKind::Plane(p) = surface.kind() else {
            return None;
        };
        let normal = p.evaluate_normal();
        Some(Self {
            origin: p.position().origin(),
            normal: if outward_same { normal } else { normal * -1.0 },
        })
    }

    /// The signed side of `p` (`> 0` outside the solid: the moved face's outward side).
    fn side(&self, p: Point3) -> f64 {
        self.normal.dot(p - self.origin)
    }
}

/// The moved faces of the result (the [`MOVED_FACE_MARK`]s), with their regions and
/// cuts. The marks stay on the body: [`refuse_kept_edges_not_clear`] removes them after
/// the reconstruction. The UV trim curves a face lacks are made on it (the pass edits
/// the body anyway, and the region cannot be read without them).
fn moved_result_faces(k: &mut Kernel, b: &mut Brep) -> Result<Vec<MovedFaceCut>> {
    use crate::offset::offset_executive::clearance::endpoint_contact::{
        TrimLoop, TrimRegion, TrimSegment,
    };
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::edgeuse::p3 as eu3;
    let mut out = Vec::new();
    for face in bp1::faces(b, None)? {
        let has_mark = b
            .entity_attributes(EntityRef::Face(face))
            .into_iter()
            .any(|(_, a)| a.attribute_id() == MOVED_FACE_MARK);
        if !has_mark {
            continue;
        }
        let surface = face_ops::surface(b, face)?
            .ok_or_else(|| null("the face's surface"))?
            .clone();
        let mut uv_box: Option<Extent2d> = None;
        for l in face_ops::loops(b, face, false, None)? {
            let Some(span) = crate::topology::loopuse::loop_uv_span(k, b, l)? else {
                uv_box = None;
                break;
            };
            uv_box = Some(uv_box.map_or(span, |u| u.union(&span)));
        }
        // the trimmed region: the primary face use's loops, each its edge uses' UV trim
        // curves (the other face use's loops are the same region read the other way, and
        // parity over both would count every crossing twice)
        let mut region = TrimRegion { loops: Vec::new() };
        let primary = b.face(face)?.faceuse;
        let (fu_a, fu_z) = face_ops::faceuses(b, face)?;
        for fu in [fu_a, fu_z].into_iter().flatten() {
            if Some(fu) != primary {
                continue;
            }
            for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
                let mut segments = Vec::new();
                for eu in eu1::loopuse_edgeuses(b, lu)? {
                    if eu1::uv_trim_curve(b, eu)?.0.is_none() {
                        eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
                    }
                    let Some(curve) = eu1::uv_trim_curve(b, eu)?.0 else {
                        continue;
                    };
                    // the segment covers the trim curve's own parameter range (the UV
                    // trim is built over the edge use's span)
                    segments.push(TrimSegment {
                        interval: curve.natural_interval(),
                        curve: curve.clone(),
                    });
                }
                region.loops.push(TrimLoop { segments });
            }
        }
        let primary = primary.ok_or_else(|| null("the face's face use"))?;
        let outward_same = eu1::faceuse_orientation(b, primary)?
            == crate::nurbs::b_spline_surface_n_lib::OrientType::Same;
        let plane = PlaneCut::of(&surface, outward_same);
        out.push(MovedFaceCut {
            face,
            moved: MovedFace {
                surface,
                uv_box,
                tolerance: b.face(face)?.tolerance.max(b.tolerance()),
            },
            region,
            plane,
        });
    }
    Ok(out)
}

/// What a moved face does to one edge (issue #13622's events).
#[derive(Debug, Clone, PartialEq)]
pub enum CrossEvent {
    /// The edge is untouched: proved clear of the face, or the face's surface meets its
    /// curve only off the face's trimmed region (the surface's extension).
    Clear,
    /// The face's cut trims the edge: `clip` (at `clip_t` of the edge's interval) is the
    /// cut's certified point on the face, and the retained interval keeps the far side
    /// (`removes_low`: the interval's low end goes).
    Trimmed {
        /// The clip point, on the face's trimmed region.
        clip: Point3,
        /// The clip's parameter on the edge's interval.
        clip_t: f64,
        /// The retained interval drops the low end of the edge's interval.
        removes_low: bool,
    },
    /// The cut leaves nothing of the edge: its whole interval is on the moved face's
    /// outward side (`clip` is where its curve meets the face).
    Removed {
        /// The clip point, on the face's trimmed region.
        clip: Point3,
    },
}

/// The event of one edge against one moved face, with the endpoint-contact proof of the
/// retained part where the event rebuilt it ([`CrossEvent::Trimmed`]).
#[derive(Debug)]
struct EdgeCrossing {
    event: CrossEvent,
    proof: Option<clearance::endpoint_contact::ContactProof>,
}

/// The cut of `face` on the edge `curve` over `interval` (issue #13622's event):
///
/// - [`CrossEvent::Clear`] where [`clearance::all_clear`] proves the edge clear of the
///   face by their summed `tolerance`, or where the cut's point certifies off the
///   face's trimmed region (`ContactProof::Outside`), or where the edge bounds the
///   face itself (its curve lies in the face's surface) AND the certified strict
///   predicate confirms it clear of the trimmed region (`clearance::separated` over
///   the trim curves' UV box). A sampled tolerance band only suggests that class: it
///   is never accepted on its own, and where neither certificate holds and no
///   endpoint contact certifies below, the edge is not clear (the event logic takes
///   the clip/rebuild path or refuses);
/// - [`CrossEvent::Trimmed`] or [`CrossEvent::Removed`] where the cut certifies: the
///   clip is the line's meeting with the face's plane (the sides at the interval's end
///   vertices, exact data), the retained interval is the part on the solid's side of the
///   cut (`PlaneCut::side` at most 0), and the clip's only-contact certificate
///   (`separated_ending_on_face`, also the record the gate takes) proves the clip
///   in-or-on the face's trimmed region and the retained part free of further contact.
///
/// Nothing uncertain is decided: a moved face that is not planar refuses
/// `SurfaceUncertified`, and an uncertifiable clip refuses `ClipUncertified`
/// ([`OffsetFailure::Reconstruction`], issue #13622).
fn classify_edge_crossing(
    k: &Kernel,
    curve: &Curve,
    interval: &crate::geom::extent1d::Extent1d,
    face: &MovedFaceCut,
    tolerance: f64,
    edge: crate::topology::EdgeId,
) -> Result<EdgeCrossing> {
    use crate::geom::extent1d::Extent1d;
    let clear = EdgeCrossing {
        event: CrossEvent::Clear,
        proof: None,
    };
    let (t0, t1) = (interval.min(), interval.max());
    if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
        return Err(null("the edge curve's interval of distinct ends"));
    }
    // 1. proved clear of the face: no decision to make
    let kept = KeptEdge {
        curve: curve.clone(),
        interval: *interval,
        tolerance,
    };
    if clearance::all_clear(k, &[kept], std::slice::from_ref(&face.moved))? {
        return Ok(clear);
    }
    // 2. the cut is certified on a plane only (the endpoint certificate's scope)
    let Some(plane) = face.plane else {
        return Err(refusal(ReconstructionRefusal::SurfaceUncertified {
            face: face.face,
        }));
    };
    // 3. the clip: the line's meeting with the plane, its sides read at the interval's
    // ends
    let end_side = |t: f64| -> Result<f64> {
        let p = curve.evaluate_point(k, t)?;
        Ok(plane.side(p))
    };
    let (g0, g1) = (end_side(t0)?, end_side(t1)?);
    // the edge bounds the face itself when its whole curve lies in the face's surface:
    // the side stays within the tolerance band over the interval (its max for a line is
    // at an end; sampled at the ends and the thirds otherwise, as production's gap
    // measurements are, `docs/validity-and-tolerances.md` §5.2). The sampled band only
    // SUGGESTS the class: before it classifies the edge `Clear`, the certified strict
    // predicate must confirm it — `clearance::separated` against the moved face's
    // trimmed region (the kept-edge test over the trim curves' UV box), never the
    // samples alone. Without that certificate and without an endpoint contact to
    // certify below, the edge is not clear: the event logic decides (the clip/rebuild
    // path or a typed refusal). Anything past the band is a cut below.
    let mut band = g0.abs().max(g1.abs());
    for f in [0.25, 0.5, 0.75] {
        let t = t0 + (t1 - t0) * f;
        band = band.max(end_side(t)?.abs());
    }
    let bounds_face = band <= tolerance;
    if bounds_face
        && clearance::separated(
            k,
            curve,
            interval,
            &face.moved.surface,
            face.moved.uv_box.as_ref(),
            tolerance,
        )?
    {
        return Ok(clear);
    }
    // lambda: the fraction of the interval where the side is zero
    let lambda = if g0 == g1 {
        if g0 <= 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    } else {
        g0 / (g0 - g1)
    };
    let (lo, hi) = if g1 > g0 {
        // the side grows: the solid side is the interval's low part [0, lambda]
        (0.0_f64, lambda)
    } else {
        // the side falls: the solid side is the high part [lambda, 1]
        (lambda, 1.0_f64)
    };
    let width = unit_clamp(hi) - unit_clamp(lo);
    // the cut's effect on the edge, by the lengths it removes and keeps (a cut exactly
    // at an end rounds a hair inside the interval: the length model, not the sign of
    // one rounding, decides). A suggested bounds-the-face edge is not accepted here
    // either: the length model reads the same ends the band sampled.
    let chord = end_side_chord(k, curve, t0, t1)?;
    if !bounds_face && (1.0 - width) * chord <= tolerance {
        // the cut trims off no part of the edge the model resolves (the clip is at its
        // very end, or the whole edge is clear of it): the edge is untouched
        return Ok(clear);
    }
    // a clip is needed: its point is the curve's meeting with the plane, proposed by
    // the ends' affine model (exact for a line, and a bent curve's proposal fails the
    // certificate below: nothing uncertain is built)
    if width * chord <= tolerance {
        // the retained part is within the tolerance of the cut: nothing of the edge
        // stays. The clip is where the curve meets the plane; it certifies at the
        // interval's end
        if !(0.0..=1.0).contains(&lambda) {
            return Err(refusal(ReconstructionRefusal::ClipUncertified { edge }));
        }
        let clip_t = t0 + lambda * (t1 - t0);
        let clip = clip_point(k, curve, t0, t1, clip_t)?;
        let proof = contact_proof(
            k,
            &ContactProofRequest {
                curve,
                retained: interval,
                clip_t,
                clip,
                face,
                tolerance,
                edge,
            },
        )?;
        return Ok(EdgeCrossing {
            event: CrossEvent::Removed { clip },
            proof: Some(proof),
        });
    }
    // the trimmed case: the retained interval keeps one side of the clip
    let removes_low = unit_clamp(lo) > 0.0;
    let clip_fraction = if removes_low {
        unit_clamp(lo)
    } else {
        unit_clamp(hi)
    };
    let clip_t = t0 + clip_fraction * (t1 - t0);
    let clip = clip_point(k, curve, t0, t1, clip_t)?;
    let retained = if removes_low {
        Extent1d::from_min_max(clip_t, t1)
    } else {
        Extent1d::from_min_max(t0, clip_t)
    };
    let proof = contact_proof(
        k,
        &ContactProofRequest {
            curve,
            retained: &retained,
            clip_t,
            clip,
            face,
            tolerance,
            edge,
        },
    )?;
    Ok(EdgeCrossing {
        event: CrossEvent::Trimmed {
            clip,
            clip_t,
            removes_low,
        },
        proof: Some(proof),
    })
}

/// The refusal of an uncertifiable clip.
fn refusal(why: ReconstructionRefusal) -> SolidError {
    Failure::new(OP, OffsetFailure::Reconstruction { why }).into_error()
}

/// `x` clamped to `[0, 1]`.
fn unit_clamp(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// The chord of `curve` over `[t0, t1]`: the distance between its evaluated ends, the
/// scale a fraction of the interval is measured at.
fn end_side_chord(k: &Kernel, curve: &Curve, t0: f64, t1: f64) -> Result<f64> {
    let p0 = curve.evaluate_point(k, t0)?;
    let p1 = curve.evaluate_point(k, t1)?;
    Ok(p0.distance(p1))
}

/// The clip point at `clip_t`, by the interval ends' affine interpolation: a line is
/// affine in its parameter, and the fractions this pass computes are ratios of its
/// ends' sides, so the clips are exact where the ends and sides are (the named case's
/// A = (0, 1, 0) and B = (1, 0, 0) among them).
fn clip_point(k: &Kernel, curve: &Curve, t0: f64, t1: f64, clip_t: f64) -> Result<Point3> {
    let p0 = curve.evaluate_point(k, t0)?;
    let p1 = curve.evaluate_point(k, t1)?;
    Ok(p0 + (p1 - p0) * ((clip_t - t0) / (t1 - t0)))
}

/// The request of one endpoint-contact certificate for a rebuilt edge (issue #13622's
/// `contact_proof` parameter clump): the edge's curve over its retained interval, the
/// clip the retained part ends at (its parameter and intended point), the moved face
/// that cut it, and the summed tolerance bound. `edge` names the result edge for the
/// typed refusal where nothing certifies.
struct ContactProofRequest<'a> {
    /// The edge's curve over `retained`.
    curve: &'a Curve,
    /// The retained part of the edge's interval.
    retained: &'a crate::geom::extent1d::Extent1d,
    /// The clip's parameter on the edge's interval.
    clip_t: f64,
    /// The clip point (the intended end point).
    clip: Point3,
    /// The moved face that cut the edge.
    face: &'a MovedFaceCut,
    /// The summed tolerance bound (the edge's and the face's), as
    /// [`clearance::all_clear`] computes it.
    tolerance: f64,
    /// The result edge, named in the refusal where nothing certifies.
    edge: crate::topology::EdgeId,
}

/// The endpoint-contact certificate of the retained part at its clip (issue #13622's
/// record): `Proved` (the clip is the edge's only contact, in-or-on the face's trimmed
/// region) keeps the event; `Unproved` refuses (nothing uncertain is built).
fn contact_proof(
    k: &Kernel,
    request: &ContactProofRequest<'_>,
) -> Result<clearance::endpoint_contact::ContactProof> {
    use crate::error::offset::ReconstructionRefusal;
    use crate::offset::offset_executive::clearance::endpoint_contact::{
        separated_ending_on_face, ContactProof, ContactRequest,
    };
    let window = (request.retained.max() - request.retained.min()).max(f64::MIN_POSITIVE);
    let proof = separated_ending_on_face(
        k,
        ContactRequest {
            curve: request.curve,
            interval: request.retained,
            contact_t: request.clip_t,
            contact_point: request.clip,
            face: &request.face.moved,
            region: &request.face.region,
            tolerance: request.tolerance,
        },
        window,
        usize::MAX,
    )?;
    if matches!(proof, ContactProof::Unproved(..)) {
        return Err(refusal(ReconstructionRefusal::ClipUncertified {
            edge: request.edge,
        }));
    }
    Ok(proof)
}

/// One original edge's event in one reconstruction (issue #13622), named by the
/// original edge (the correspondence mark's id).
#[derive(Debug)]
pub struct EdgeEventRecord {
    /// The original edge (the correspondence mark's id).
    original: crate::topology::EdgeId,
    /// What the moved faces' cuts do to its result edges.
    event: CrossEvent,
}

/// A cut corner's edge and the clip point its curve takes (issue #13622).
#[derive(Debug)]
struct CornerEdge {
    /// The result edge.
    edge: crate::topology::EdgeId,
    /// The clip point (the intended end point).
    clip: Point3,
    /// The clip's parameter on the edge's interval.
    clip_t: f64,
    /// The retained interval drops the low end of the edge's interval.
    drop_min: bool,
}

/// A clip the reconstruction re-trimmed a result edge to (issue #13622), with the
/// endpoint-contact proof of the re-trimmed edge where the clip is its crossing with a
/// moved face (`None` for a clip the cut corner's gap gave an edge that runs along the
/// face's surface: the face is one of its own faces there, so no endpoint-contact
/// certificate applies to it).
#[derive(Debug)]
struct ClipRecord {
    /// The result edge.
    edge: crate::topology::EdgeId,
    /// The moved face whose cut gives the clip.
    face: FaceId,
    /// The clip point (the intended end point).
    clip: Point3,
    /// The clip's parameter on the edge's interval.
    clip_t: f64,
    /// The retained interval drops the low end of the edge's interval.
    removes_low: bool,
    /// The endpoint-contact proof where the clip is the edge's crossing.
    proof: Option<clearance::endpoint_contact::ContactProof>,
}

/// One tolerance the crossed-boundary reconstruction re-measured (issue #13622,
/// `docs/validity-and-tolerances.md` §5.2): the entity it was measured on, and the
/// tolerance before and after (a re-measured value never grows one).
#[derive(Debug)]
pub struct ToleranceChange {
    /// The entity, labelled as in the tolerance report (`E3`, `V7`).
    pub entity: String,
    /// The tolerance before the re-measurement.
    pub before: f64,
    /// The tolerance after it (never above `before`).
    pub after: f64,
}

/// What one crossed-boundary reconstruction did and proved (issue #13622): the events
/// it fired per original edge, the correspondence map of the result's edges, the
/// endpoint-contact proof per rebuilt edge (the kept-edge gate's records), the bridge
/// edges it inserted and the tolerances it re-measured. Returned by
/// [`reconstruct_crossed_boundaries`] (through [`local_operation_recorded`]); the gate
/// takes [`Self::reconstructed_kept`] and [`Self::proofs`] from it.
#[derive(Debug)]
pub struct Reconstruction {
    /// The events that fired (the originals a moved face crossed), in map order.
    events: Vec<EdgeEventRecord>,
    /// The endpoint-contact proof of each rebuilt edge.
    proofs: Vec<(
        crate::topology::EdgeId,
        clearance::endpoint_contact::ContactProof,
    )>,
    /// The correspondence map as captured after `move_faces`.
    map: EdgeMap,
    /// The kept edges (the original edges) the reconstruction rebuilt: no longer
    /// kept as they were, their crossing certified by [`Self::proofs`] instead.
    reconstructed_kept: BTreeSet<crate::topology::EdgeId>,
    /// The bridge edges inserted.
    bridges: Vec<crate::topology::EdgeId>,
    /// The tolerances re-measured.
    tolerances: Vec<ToleranceChange>,
    /// The vanish obligations the gap pass deferred and this pass discharged (issue
    /// #13622's hook): one per original edge (id order); an undischarged one refuses
    /// typed ([`ReconstructionRefusal::VanishUnresolved`]), so a returned record's set
    /// is exhausted.
    discharged_vanishes: BTreeSet<crate::topology::EdgeId>,
}

impl Reconstruction {
    /// The events that fired (an edge a moved face crossed: trimmed or removed).
    #[must_use]
    pub fn events(&self) -> &[EdgeEventRecord] {
        &self.events
    }

    /// The endpoint-contact proof of each rebuilt edge.
    #[must_use]
    pub(crate) fn proofs(
        &self,
    ) -> &[(
        crate::topology::EdgeId,
        clearance::endpoint_contact::ContactProof,
    )] {
        &self.proofs
    }

    /// The correspondence map as captured after `move_faces`: each original edge's
    /// arena index to the result edges that carried its mark then.
    #[must_use]
    pub fn map(&self) -> &EdgeMap {
        &self.map
    }

    /// The kept edges (the original edges) the reconstruction rebuilt.
    #[must_use]
    pub fn reconstructed_kept(&self) -> &BTreeSet<crate::topology::EdgeId> {
        &self.reconstructed_kept
    }

    /// The bridge edges inserted.
    #[must_use]
    pub fn bridges(&self) -> &[crate::topology::EdgeId] {
        &self.bridges
    }

    /// The tolerances re-measured.
    #[must_use]
    pub fn tolerances(&self) -> &[ToleranceChange] {
        &self.tolerances
    }

    /// The vanish obligations the gap pass deferred and this pass discharged (issue
    /// #13622's hook): every one is gone from the result (its removal certified, every
    /// corresponding result edge severed), or the pass refused instead of returning.
    #[must_use]
    pub fn discharged_vanishes(&self) -> &BTreeSet<crate::topology::EdgeId> {
        &self.discharged_vanishes
    }

    /// An empty record (a pass that found no crossing).
    fn empty(map: EdgeMap) -> Self {
        Self {
            events: Vec::new(),
            proofs: Vec::new(),
            map,
            reconstructed_kept: BTreeSet::new(),
            bridges: Vec::new(),
            tolerances: Vec::new(),
            discharged_vanishes: BTreeSet::new(),
        }
    }
}

/// The edge's use starts and its far ends (`(start vertex, other vertex)` per use), the
/// [`topology::edge::other_vertex`] read the loop walks use.
fn use_ends(
    b: &Brep,
    eu: crate::topology::EdgeuseId,
) -> Result<(
    Option<crate::topology::VertexId>,
    Option<crate::topology::VertexId>,
)> {
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::vertexuse as vu1;
    let e = eu1::edge(b, eu)?.ok_or_else(|| null("the edge use's edge"))?;
    let v = eu1::vertexuse(b, eu)?.and_then(|vu| vu1::vertex(b, vu).ok().flatten());
    Ok((v, edge::other_vertex(b, e, v)?))
}

/// Every corner gap of every face's loop-use chains: consecutive uses (walking the
/// chain) whose end and start vertices differ. Returns (loop use, use before the gap,
/// use after it), in body order.
fn loop_gaps(
    b: &Brep,
) -> Result<
    Vec<(
        crate::topology::LoopuseId,
        crate::topology::EdgeuseId,
        crate::topology::EdgeuseId,
    )>,
> {
    use crate::topology::edgeuse::p1 as eu1;
    let mut out = Vec::new();
    for (lu, _) in b.loopuses().iter() {
        let chain = eu1::loopuse_edgeuses(b, lu)?;
        let n = chain.len();
        for (prev, next) in chain
            .iter()
            .copied()
            .zip(chain.iter().copied().cycle().skip(1))
            .take(n)
        {
            let prev_end = use_ends(b, prev)?.1;
            let next_start = use_ends(b, next)?.0;
            if prev_end.is_none() || next_start.is_none() || prev_end != next_start {
                out.push((lu, prev, next));
            }
        }
    }
    Ok(out)
}

/// One loop-use chain's walk verdict (issue #13622's stage-2 check): closed (walking
/// CW returns to the start in exactly the chain's length) and shared (consecutive uses
/// meet at exactly one vertex).
fn walk_chain(b: &Brep, lu: crate::topology::LoopuseId) -> Result<(bool, bool, usize)> {
    use crate::topology::edgeuse::p1 as eu1;
    let chain = eu1::loopuse_edgeuses(b, lu)?;
    let n = chain.len();
    if n == 0 {
        return Ok((false, false, 0));
    }
    let Some(&first) = chain.first() else {
        return Ok((false, false, 0));
    };
    let mut eu = first;
    let mut steps = 0_usize;
    loop {
        eu = match eu1::cw_edgeuse(b, eu)? {
            Some(next) => next,
            None => return Ok((false, false, n)),
        };
        steps = steps.saturating_add(1);
        if eu == first {
            break;
        }
        if steps > n {
            return Ok((false, false, n));
        }
    }
    if steps != n {
        return Ok((false, false, n));
    }
    for (cur, next) in chain
        .iter()
        .copied()
        .zip(chain.iter().copied().cycle().skip(1))
        .take(n)
    {
        let (s, e) = use_ends(b, cur)?;
        let (s2, e2) = use_ends(b, next)?;
        let shared = [s, e]
            .into_iter()
            .flatten()
            .filter(|v| [s2, e2].contains(&Some(*v)))
            .count();
        if shared != 1 {
            return Ok((true, false, n));
        }
    }
    Ok((true, true, n))
}

/// SEVER (issue #13622): removes edge `e` from the body, its uses leaving their loop
/// chains (the `p3::squeeze_loops` unlink core) and their records going with the edge's
/// (`PreEdit` / `RmFromBrep` / `Destruction` and the faces' `PreEdit`, the events
/// `squeeze_edge_in` sends) — without `squeeze_edge`'s vertex merge: the two end
/// vertices keep their other uses.
fn sever_edge(
    k: &mut Kernel,
    b: &mut Brep,
    e: crate::topology::EdgeId,
    env: &mut crate::topology::edge::NotifyEnv<'_>,
) -> Result<()> {
    use crate::error::EntityRef;
    use crate::geom::object::NotifyOperation;
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::loopuse;
    use crate::topology::vertexuse as vu1;
    k.check_cancelled(OP)?;
    let uses = edge::edgeuses(b, e, None, None)?;
    let key_of = loopuse::notify_ref;
    let eref = key_of(EntityRef::Edge(e));
    edge::notify(
        b,
        e,
        env,
        NotifyOperation::PreEdit,
        [Some(eref), None, None],
    )?;
    edge::notify(
        b,
        e,
        env,
        NotifyOperation::RmFromBrep,
        [Some(eref), None, None],
    )?;
    for &eu in &uses {
        if let Some(f) = eu1::face(b, eu)? {
            let fref = key_of(EntityRef::Face(f));
            crate::topology::face::p3::notify(
                b,
                f,
                env,
                None,
                NotifyOperation::PreEdit,
                [Some(fref), None, None],
            )?;
        }
    }
    // the `squeeze_loops` unlink core: each use leaves its loop chain
    for &eu in &uses {
        b.edgeuse_mut(eu)?.uv_trim_curve = None;
        let eu_cw = eu1::cw_edgeuse(b, eu)?
            .ok_or_else(|| null("the edge use's clockwise loop neighbour"))?;
        let eu_ccw = eu1::ccw_edgeuse(b, eu)?
            .ok_or_else(|| null("the edge use's counter-clockwise loop neighbour"))?;
        let lu = eu1::loopuse(b, eu)?.ok_or_else(|| null("the edge use's loop use"))?;
        if b.loopuse(lu)?.start == crate::topology::LoopuseStart::Edgeuse(Some(eu)) {
            b.loopuse_mut(lu)?.start = crate::topology::LoopuseStart::Edgeuse(Some(eu_cw));
        }
        b.edgeuse_mut(eu_cw)?.ccw = Some(eu_ccw);
        b.edgeuse_mut(eu_ccw)?.cw = Some(eu_cw);
    }
    // delete the uses and their vertex uses (p3.rs:3959-3969)
    for &eu in &uses {
        let vu = eu1::vertexuse(b, eu)?.ok_or_else(|| null("the edge use's vertex use"))?;
        let v = vu1::vertex(b, vu)?.ok_or_else(|| null("the vertex use's vertex"))?;
        b.edge_mut(e)?.edgeuses.retain(|&x| x != eu);
        b.vertex_mut(v)?.vertexuses.retain(|&x| x != vu);
        b.remove_edgeuse_record(eu)?;
        b.remove_vertexuse_record(vu)?;
    }
    edge::notify(
        b,
        e,
        env,
        NotifyOperation::Destruction,
        [Some(eref), None, None],
    )?;
    b.remove_edge_record(e)?;
    Ok(())
}

/// MERGE LAMINA TWINS (issue #13622): two lamina edges of one original (`keep` and its
/// `twin`, two uses each) become the one manifold edge they stand for: the twin's uses
/// are re-pointed onto `keep` (the `squeeze_edge` use re-pointing, one level up), the
/// merged radial list in a manifold order ([face1 up, face1 down, face2 down, face2 up]
/// shape: mates adjacent, orientations alternating) and the twin record goes. A pair
/// whose uses admit no alternating radial order refuses: nothing uncertain is built.
fn merge_lamina_twins(
    b: &mut Brep,
    keep: crate::topology::EdgeId,
    twin: crate::topology::EdgeId,
) -> Result<()> {
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;
    // (use, its face, on the face's upward face use)
    let slot = |b: &Brep,
                eu: crate::topology::EdgeuseId|
     -> Result<(crate::topology::EdgeuseId, FaceId, bool)> {
        let f = eu1::face(b, eu)?.ok_or_else(|| null("the edge use's face"))?;
        let lu = eu1::loopuse(b, eu)?.ok_or_else(|| null("the edge use's loop use"))?;
        Ok((eu, f, b.loopuse(lu)?.faceuse == b.face(f)?.faceuse))
    };
    let keeps: Vec<_> = edge::edgeuses(b, keep, None, None)?
        .into_iter()
        .map(|eu| slot(b, eu))
        .collect::<Result<_>>()?;
    let twins: Vec<_> = edge::edgeuses(b, twin, None, None)?
        .into_iter()
        .map(|eu| slot(b, eu))
        .collect::<Result<_>>()?;
    if keeps.len() != 2 || twins.len() != 2 {
        return Err(refusal(ReconstructionRefusal::ClipUncertified {
            edge: keep,
        }));
    }
    let find = |list: &[(crate::topology::EdgeuseId, FaceId, bool)], up: bool| {
        list.iter().find(|(_, _, u)| *u == up).map(|(eu, _, _)| *eu)
    };
    let mut order: Option<[crate::topology::EdgeuseId; 4]> = None;
    for first_twins_first in [false, true] {
        for up_a in [true, false] {
            for up_c in [true, false] {
                let (a, c) = if first_twins_first {
                    (twins.as_slice(), keeps.as_slice())
                } else {
                    (keeps.as_slice(), twins.as_slice())
                };
                if let (Some(a1), Some(a2), Some(c1), Some(c2)) =
                    (find(a, up_a), find(a, !up_a), find(c, up_c), find(c, !up_c))
                {
                    let candidate = [a1, a2, c2, c1];
                    let orientations: Vec<_> = candidate
                        .iter()
                        .map(|eu| eu1::orientation(b, *eu))
                        .collect::<Result<_>>()?;
                    let alternating = orientations
                        .iter()
                        .zip(orientations.iter().skip(1))
                        .all(|(x, y)| x != y);
                    if alternating {
                        order = Some(candidate);
                        break;
                    }
                }
            }
            if order.is_some() {
                break;
            }
        }
        if order.is_some() {
            break;
        }
    }
    let Some(order) = order else {
        return Err(refusal(ReconstructionRefusal::ClipUncertified {
            edge: keep,
        }));
    };
    for &eu in order.iter().skip(2) {
        b.edgeuse_mut(eu)?.edge = Some(keep);
    }
    b.edge_mut(keep)?.edgeuses = order.to_vec();
    b.edge_mut(twin)?.edgeuses.clear();
    b.remove_edge_record(twin)?;
    Ok(())
}

/// VERTEX SPLIT (issue #13622): duplicates vertex `v` (same point and tolerance) and
/// moves the vertex uses of the edges `moved` onto the copy — the mirror of
/// `squeeze_edge`'s use re-pointing, to a new vertex instead of a survivor. Returns the
/// copy.
fn split_vertex(
    b: &mut Brep,
    v: crate::topology::VertexId,
    moved: &[crate::topology::EdgeId],
) -> Result<crate::topology::VertexId> {
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::vertex;
    use crate::topology::vertexuse as vu1;
    use crate::topology::Vertex;
    let point = vertex::point(b, v)?;
    let tolerance = vertex::tolerance(b, v)?;
    let copy = b.add_vertex_record(Vertex {
        point,
        tolerance,
        vertexuses: Vec::new(),
        poly_vertex: None,
    })?;
    for &e in moved {
        for eu in edge::edgeuses(b, e, None, None)? {
            let vu = eu1::vertexuse(b, eu)?.ok_or_else(|| null("the edge use's vertex use"))?;
            if vu1::vertex(b, vu)? == Some(v) {
                b.vertex_mut(v)?.vertexuses.retain(|&x| x != vu);
                b.vertex_mut(copy)?.vertexuses.push(vu);
                b.vertexuse_mut(vu)?.vertex = Some(copy);
            }
        }
    }
    Ok(copy)
}

/// The parameter on curve `c`'s `ivl` where `point` lies (the projection between the
/// interval's ends; exact for the lines this pass clips, whose curves are affine in
/// their parameters). A projection whose residual exceeds the caller's tolerance is
/// caught by the tolerance pass (`GapBeyondConstruction`), never built on.
fn clip_param(
    k: &Kernel,
    c: &Curve,
    ivl: crate::geom::extent1d::Extent1d,
    point: Point3,
) -> Result<f64> {
    let a = c.evaluate_point(k, ivl.min())?;
    let z = c.evaluate_point(k, ivl.max())?;
    let d = z - a;
    let dd = d.dot(d);
    if dd.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(null("the curve's span between distinct ends"));
    }
    Ok(ivl.min() + (point - a).dot(d) / dd * (ivl.max() - ivl.min()))
}

/// RETRIM (issue #13622): edge `e` keeps the interval end away from its clip and takes
/// `clip_t` as the cut end (`drop_min` selects the end), its vertex uses' UV trim
/// curves rebuilt over the new interval (`rebuild_uv_trim_curve`: the face regions they
/// bound are what the volume integrates).
fn retrim_edge(
    k: &mut Kernel,
    b: &mut Brep,
    e: crate::topology::EdgeId,
    clip_t: f64,
    drop_min: bool,
) -> Result<()> {
    use crate::topology::edge;
    use crate::topology::edgeuse::p2 as eu2;
    let ivl = edge::interval(b, e)?;
    let next = if drop_min {
        crate::geom::extent1d::Extent1d::from_min_max(clip_t, ivl.max())
    } else {
        crate::geom::extent1d::Extent1d::from_min_max(ivl.min(), clip_t)
    };
    edge::set_interval(b, e, &next)?;
    for eu in edge::edgeuses(b, e, None, None)? {
        eu2::rebuild_uv_trim_curve(k, b, eu)?;
    }
    Ok(())
}

/// BRIDGE (issue #13622): one new edge between the existing vertices `a` and `by`
/// closing the corner gaps listed in `slots` (a mated pair in each face's two loop-use
/// chains): a vertex use at each end (the end wiring of `topology::edge`), the uses
/// spliced into their chains' gaps (the `squeeze_loops` unlink in reverse), a UV trim
/// curve on each use, the manifold radial list [face1 up, face1 down, face2 down, face2
/// up] (orientations alternating, or refused), and an `AddToBrep` notification.
fn insert_bridge(
    k: &mut Kernel,
    b: &mut Brep,
    a: crate::topology::VertexId,
    by: crate::topology::VertexId,
    slots: &[(
        crate::topology::LoopuseId,
        crate::topology::EdgeuseId,
        crate::topology::EdgeuseId,
    )],
    env: &mut crate::topology::edge::NotifyEnv<'_>,
) -> Result<crate::topology::EdgeId> {
    use crate::error::EntityRef;
    use crate::geom::extent1d::Extent1d;
    use crate::geom::object::NotifyOperation;
    use crate::nurbs::b_spline_surface_n_lib::OrientType;
    use crate::topology::brep::p2 as brep_p2;
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::edgeuse::p3 as eu3;
    use crate::topology::{Edge, Edgeuse, EdgeuseOwner, Vertexuse, VertexuseOwner};
    k.check_cancelled(OP)?;
    if slots.len() != 4 {
        let loopuse = match slots.first() {
            Some((lu, _, _)) => *lu,
            None => crate::topology::LoopuseId::from_index(0),
        };
        return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
            fault: BoundaryFault::LoopDiverges { loopuse },
        }));
    }
    // the curve carries the intended geometry: `a` to `by`
    let p_a = crate::topology::vertex::point(b, a)?;
    let p_b = crate::topology::vertex::point(b, by)?;
    let curve = Curve::from(crate::curve::line::Line::from_points(p_a, p_b, 3));
    let tolerance = b.tolerance();
    let e = b.add_edge_record(Edge::new([a, by]))?;
    let owner = b.owner_identity(EntityRef::Edge(e));
    {
        let record = b.edge_mut(e)?;
        let mut owned = curve.clone();
        owned.set_owner(Some(owner), None);
        record.curve = Some(owned);
        record.interval = Extent1d::from_min_max(0.0, 1.0);
        record.tolerance = tolerance;
        b.set_geometry_owner_key(EntityRef::Edge(e), Some(owner));
    }
    // the radial list: [face1 up, face1 down, face2 down, face2 up]
    let up = |b: &Brep, lu: crate::topology::LoopuseId, f: FaceId| -> Result<bool> {
        Ok(b.loopuse(lu)?.faceuse == b.face(f)?.faceuse)
    };
    let lu_face = |b: &Brep, lu: crate::topology::LoopuseId| -> Result<FaceId> {
        let fu = b
            .loopuse(lu)?
            .faceuse
            .ok_or_else(|| null("the loop use's face use"))?;
        eu1::faceuse_face(b, fu)?.ok_or_else(|| null("the face use's face"))
    };
    let mut slots_by_side: Vec<(
        FaceId,
        bool,
        (
            crate::topology::LoopuseId,
            crate::topology::EdgeuseId,
            crate::topology::EdgeuseId,
        ),
    )> = Vec::new();
    for &g in slots {
        let f = lu_face(b, g.0)?;
        slots_by_side.push((f, up(b, g.0, f)?, g));
    }
    let mut chain_slots = Vec::new();
    let mut faces: Vec<FaceId> = slots_by_side.iter().map(|(f, _, _)| *f).collect();
    faces.sort_unstable();
    faces.dedup();
    let gap_loopuse = || -> Result<crate::topology::LoopuseId> {
        match slots.first() {
            Some((lu, _, _)) => Ok(*lu),
            None => Err(null("the gap's loop use")),
        }
    };
    let [face1, face2] = faces.as_slice() else {
        return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
            fault: BoundaryFault::LoopDiverges {
                loopuse: gap_loopuse()?,
            },
        }));
    };
    let pick = |want: (FaceId, bool)| -> Result<(
        crate::topology::LoopuseId,
        crate::topology::EdgeuseId,
        crate::topology::EdgeuseId,
    )> {
        slots_by_side
            .iter()
            .find(|(f, u, _)| (*f, *u) == want)
            .map(|(_, _, g)| *g)
            .ok_or_else(|| null("the gap's edge use on one side"))
    };
    chain_slots.push(pick((*face1, true))?);
    chain_slots.push(pick((*face1, false))?);
    chain_slots.push(pick((*face2, false))?);
    chain_slots.push(pick((*face2, true))?);
    let mut uses = Vec::new();
    for (lu, prev, next) in chain_slots {
        let start = use_ends(b, prev)?.1;
        let start = start.ok_or_else(|| null("the edge use's end"))?;
        if start != a && start != by {
            return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                fault: BoundaryFault::LoopDiverges { loopuse: lu },
            }));
        }
        let orientation = if start == a {
            OrientType::Same
        } else {
            OrientType::Opposite
        };
        let eu = b.add_edgeuse_record(Edgeuse::default())?;
        let vu = b.add_vertexuse_record(Vertexuse::default())?;
        b.edge_mut(e)?.edgeuses.push(eu);
        let record = b.edgeuse_mut(eu)?;
        record.edge = Some(e);
        record.owner = EdgeuseOwner::Loopuse(Some(lu));
        record.vertexuse = Some(vu);
        record.orientation = orientation;
        b.vertexuse_mut(vu)?.owner = VertexuseOwner::Edgeuse(Some(eu));
        b.vertexuse_mut(vu)?.vertex = Some(start);
        b.vertex_mut(start)?.vertexuses.push(vu);
        // splice into the chain's gap (the `squeeze_loops` unlink in reverse)
        b.edgeuse_mut(prev)?.ccw = Some(eu);
        b.edgeuse_mut(next)?.cw = Some(eu);
        b.edgeuse_mut(eu)?.cw = Some(prev);
        b.edgeuse_mut(eu)?.ccw = Some(next);
        uses.push((eu, start));
    }
    let orientations: Vec<_> = uses
        .iter()
        .map(|(eu, _)| eu1::orientation(b, *eu))
        .collect::<Result<_>>()?;
    let alternating = orientations
        .iter()
        .zip(orientations.iter().skip(1))
        .all(|(x, y)| x != y);
    if !alternating {
        return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
            fault: BoundaryFault::LoopDiverges {
                loopuse: gap_loopuse()?,
            },
        }));
    }
    for &(eu, _) in &uses {
        eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
    }
    brep_p2::notify(
        b,
        env,
        None,
        NotifyOperation::AddToBrep,
        [
            Some(brep_p2::NotifyArg::Entity(EntityRef::Edge(e))),
            None,
            None,
        ],
    )?;
    Ok(e)
}

/// The region pair `move_faces` dropped (issue #13622): the merge leaves the body a
/// sheet model (one region record holding both face uses of every face), so
/// `is_manifold_solid`'s two-regions clause fails however sound the boundary is.
/// [`p6::test_closure_split_region`](crate::topology::brep::p6) (upstream
/// `SmBrep::TestClosureSplitRegion`, the boolean merge's own primitive) splits the
/// region at a seed face use: the Opposite face uses' closed set moves to a new solid
/// region and `do_infinite_region_testing` finds the infinite region again.
fn restore_region_pair(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut crate::topology::edge::NotifyEnv<'_>,
) -> Result<()> {
    use crate::nurbs::b_spline_surface_n_lib::OrientType;
    use crate::topology::edgeuse::p1 as eu1;
    if bp1::regions(b, None)?.len() >= 2 {
        return Ok(());
    }
    k.check_cancelled(OP)?;
    // the seed: any face's Opposite (down) face use
    let mut seed = None;
    for (f, _) in b.faces().iter() {
        let (fu_a, fu_z) = face_ops::faceuses(b, f)?;
        for fu in [fu_a, fu_z].into_iter().flatten() {
            if eu1::faceuse_orientation(b, fu)? == OrientType::Opposite {
                seed = Some(fu);
            }
        }
    }
    let seed = seed.ok_or_else(|| null("a down face use"))?;
    let mut marks = crate::topology::topology_traverser::Marks::default();
    crate::topology::brep::p6::test_closure_split_region(k, b, &mut marks, env, true, seed)?;
    Ok(())
}

/// The gaps measured on one edge by the tolerance formula (issue #13622,
/// `docs/validity-and-tolerances.md` §5.2): the edge use's end to its loop neighbour's
/// start (`g_ee`), the whole-edge edge/face gap (`g_ef`), and the largest face
/// tolerance at the edge (the zone formula's floor).
struct EdgeGaps {
    /// The edge measured.
    edge: crate::topology::EdgeId,
    /// The largest `g_ee` over the edge's uses.
    g_ee: f64,
    /// The largest `g_ef` over the edge's uses.
    g_ef: f64,
    /// The largest face tolerance at the edge.
    max_face: f64,
}

/// The tolerance pass of the reconstruction (issue #13622, `docs/validity-and-tolerances.md`
/// §5.2, §7.2 steps 4-5, §8 and invariant 9): the gaps the production formula names are
/// measured on the affected entities (an edge's `g_ee` between an edge use's end and its
/// loop neighbour's start and its whole-edge `g_ef`; a vertex's `vertex/edge` and
/// `vertex/face` gaps, calculated separately) and the tolerances are set to the formula's
/// value without the only-grow guard: a stored tolerance larger than its gaps need
/// shrinks (the named case's corner vertices, 1.00001 -> 1e-5). An edge's tolerance is
/// left unchanged unless its whole-edge gaps justify changing it. A measured gap past
/// the construction bound (the body's zone tolerance: the model's resolution, below
/// which an exact construction leaves no gap) refuses
/// [`GapBeyondConstruction`](crate::error::offset::ReconstructionRefusal): the tolerance
/// is never grown to cover it (§8: "the construction is wrong: refuse").
fn tighten_reconstruction_tolerances(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[crate::topology::EdgeId],
    vertices: &[crate::topology::VertexId],
) -> Result<Vec<ToleranceChange>> {
    use crate::geom::tol::{calc_edge_zone_tol_3d, calc_vertex_zone_tol_3d};
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::vertex;
    use crate::topology::vertexuse::{calc_vertex_edge_gap, calc_vertex_face_gap};
    let bound = b.tolerance();
    let refuse_gap = |gap: f64| {
        Err(refusal(ReconstructionRefusal::GapBeyondConstruction {
            gap,
            bound,
        }))
    };
    let mut out = Vec::new();
    // the edge gaps the formula names: g_ee (an edge use's end to its loop neighbour's
    // start) and g_ef (the whole-edge edge/face gap, `calc_max_edge_face_gap` over the
    // edge's uses)
    let mut measured: Vec<EdgeGaps> = Vec::new();
    for &e in edges {
        k.check_cancelled(OP)?;
        let mut g_ee = 0.0_f64;
        let mut g_ef = 0.0_f64;
        for eu in edge::edgeuses(b, e, None, None)? {
            if let Some(g) = eu1::calc_edge_ccw_edge_gap(k, b, eu, true, true)? {
                if g.gap_3d.is_finite() && g.gap_3d > 0.0 {
                    g_ee = g_ee.max(g.gap_3d);
                }
            }
            let g = eu1::calc_max_edge_face_gap(k, b, eu, true)?;
            if g.gap_3d.is_finite() && g.gap_3d > 0.0 {
                g_ef = g_ef.max(g.gap_3d);
            }
        }
        if g_ee > bound || g_ef > bound {
            return refuse_gap(g_ee.max(g_ef));
        }
        let max_face = edge::faces(b, e, None)?
            .iter()
            .filter_map(|&f| b.face(f).ok().map(|r| r.tolerance))
            .fold(0.0_f64, f64::max);
        measured.push(EdgeGaps {
            edge: e,
            g_ee,
            g_ef,
            max_face,
        });
    }
    for gaps in measured {
        let e = gaps.edge;
        let old = edge::tolerance(b, e)?;
        let new = calc_edge_zone_tol_3d(k, gaps.g_ee, gaps.g_ef, Some(gaps.max_face), None);
        // §7.2 step 4, "set without the only-grow guard": a stored tolerance larger
        // than its gaps need shrinks; a larger one is never set (§8, invariant 9)
        if new < old {
            edge::set_tolerance(b, e, new, false, false)?;
            out.push(ToleranceChange {
                entity: format!("E{}", e.index()),
                before: old,
                after: new,
            });
        }
    }
    // the vertex bounds, calculated separately (§5.2): the vertex/edge and vertex/face
    // gaps and the largest edge tolerance at the vertex as the floor (§7.2 step 5)
    for &v in vertices {
        k.check_cancelled(OP)?;
        let mut g_ve = 0.0_f64;
        let mut g_vf = 0.0_f64;
        for vu in vertex::vertexuses(b, v)? {
            if let Some(g) = calc_vertex_edge_gap(k, b, vu, true, true)? {
                if g.gap_3d.is_finite() && g.gap_3d > 0.0 {
                    g_ve = g_ve.max(g.gap_3d);
                }
            }
            if let Some(g) = calc_vertex_face_gap(k, b, vu, true, true)? {
                if g.gap_3d.is_finite() && g.gap_3d > 0.0 {
                    g_vf = g_vf.max(g.gap_3d);
                }
            }
        }
        if g_ve > bound || g_vf > bound {
            return refuse_gap(g_ve.max(g_vf));
        }
        let max_edge = vertex::edges(b, v, None)?
            .iter()
            .filter_map(|&e| edge::tolerance(b, e).ok())
            .fold(0.0_f64, f64::max);
        let old = vertex::tolerance(b, v)?;
        let new = calc_vertex_zone_tol_3d(k, g_ve, g_vf, Some(max_edge), None);
        if new < old {
            vertex::set_tolerance(b, v, new, false, false)?;
            out.push(ToleranceChange {
                entity: format!("V{}", v.index()),
                before: old,
                after: new,
            });
        }
    }
    Ok(out)
}

/// The stage-2 validation of the completed boundary (issue #13622): every loop of every
/// affected face walks closed with consecutive uses meeting at one vertex, every bridge
/// the pass inserted is still a four-use manifold edge between its end vertices, and
/// every clip point still certifies in-or-on its moved face's COMPLETED trims (the
/// region the trims make after the re-trim, re-read here). Anything else refuses typed
/// ([`BoundaryFault`]): nothing uncertain is ever returned.
fn validate_completed_boundary(
    k: &mut Kernel,
    b: &mut Brep,
    clips: &[ClipRecord],
    bridges: &[crate::topology::EdgeId],
    affected_faces: &[FaceId],
) -> Result<()> {
    use crate::error::offset::{BoundaryFault, ReconstructionRefusal};
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;
    for &f in affected_faces {
        let (fu_a, fu_z) = face_ops::faceuses(b, f)?;
        for fu in [fu_a, fu_z].into_iter().flatten() {
            for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
                k.check_cancelled(OP)?;
                let (closed, shared, _) = walk_chain(b, lu)?;
                if !closed {
                    return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                        fault: BoundaryFault::LoopNotClosed { loopuse: lu },
                    }));
                }
                if !shared {
                    return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                        fault: BoundaryFault::LoopDiverges { loopuse: lu },
                    }));
                }
            }
        }
    }
    for &e in bridges {
        let alive = b.edges().iter().any(|(x, _)| x == e);
        let manifold = alive && edge::is_manifold(b, e)? && edge::edgeuse_count(b, e)? == 4;
        if !manifold {
            return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                fault: BoundaryFault::BridgeMissing { edge: e },
            }));
        }
    }
    // the clip points against the completed trims: each certified clip must still be
    // proved in-or-on its moved face's region as the re-trimmed loops make it now
    if !clips.is_empty() {
        let cuts = moved_result_faces(k, b)?;
        for clip in clips {
            k.check_cancelled(OP)?;
            if clip.proof.is_none() {
                continue;
            }
            let e = clip.edge;
            let curve = edge::curve(b, e)?
                .ok_or_else(|| null("the edge's curve"))?
                .clone();
            let interval = edge::interval(b, e)?;
            let tolerance = edge::tolerance(b, e)?.max(b.tolerance());
            let Some(face) = cuts.iter().find(|f| f.face == clip.face) else {
                return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                    fault: BoundaryFault::ClipOutsideRegion { edge: e },
                }));
            };
            let Ok(clearance::endpoint_contact::ContactProof::Proved(_)) = contact_proof(
                k,
                &ContactProofRequest {
                    curve: &curve,
                    retained: &interval,
                    clip_t: clip.clip_t,
                    clip: clip.clip,
                    face,
                    tolerance,
                    edge: e,
                },
            ) else {
                return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                    fault: BoundaryFault::ClipOutsideRegion { edge: e },
                }));
            };
        }
    }
    Ok(())
}

/// The crossed-boundary reconstruction (issue #13622): run after `move_faces` on the
/// result body, before the kept-edge gate. The correspondence `map` (the result edges
/// each original edge's mark reached) groups the result edges; every group whose
/// original's faces all keep their surfaces (`crossable`) is classified against every
/// moved face ([`classify_edge_crossing`]); the events fire for the originals a moved
/// face crosses and the cut corner's neighbourhood around them: [`sever_edge`] for the
/// removed ones (lamina twins resolved with [`merge_lamina_twins`] first where a twin
/// pair must resolve), the cut corner's diverging ends split, re-trimmed to their
/// intended points and glued there, a bridge edge per corner gap ([`insert_bridge`]),
/// the region pair restored ([`restore_region_pair`]), the affected tolerances
/// re-measured, never grown ([`tighten_reconstruction_tolerances`]) and the completed
/// boundary validated ([`validate_completed_boundary`]). A no-crossing pass is a no-op
/// (zero events).
///
/// `vanished` is the gap pass's vanish obligations (issue #13622's hook,
/// [`OffsetExecutive::pending_vanishes`](crate::offset::offset_executive::p1::OffsetExecutive::pending_vanishes),
/// one per original edge): each must discharge here — its classification certifies its
/// removal, its correspondence is complete and unambiguous, and every corresponding
/// result edge is severed — before the repair builds anything. Where the reconstruction
/// does not take the event (a `Clear` or `Trimmed` classification, a missing or
/// ambiguous correspondence, or a no-engagement return) ownership is unestablished and
/// the gap pass's refusal stands again ([`deferred_edge_vanish`]); a certified removal
/// whose descendants do not all go refuses
/// [`ReconstructionRefusal::VanishUnresolved`], and no no-engagement return suppresses
/// an obligation (a cut the endpoint certificate's scope cannot decide on a pending
/// obligation refuses its own typed refusal). The returned record carries the
/// discharged set ([`Reconstruction::discharged_vanishes`]).
fn reconstruct_crossed_boundaries(
    k: &mut Kernel,
    b: &mut Brep,
    map: &EdgeMap,
    kept: &[(crate::topology::EdgeId, KeptEdge)],
    crossable: &BTreeSet<crate::topology::EdgeId>,
    vanished: &BTreeSet<crate::topology::EdgeId>,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<Reconstruction> {
    use crate::blend::fillet_section::interval::Ivl;
    use crate::geom::context::Context;
    use crate::topology::edge;
    use crate::topology::edge::NotifyEnv;
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::vertexuse as vu1;
    let cuts = moved_result_faces(k, &mut b.clone_standing_for_self()?)?;
    if cuts.is_empty() {
        // (issue #13622's hook) a no-engagement return discharges nothing: where the gap
        // pass deferred a vanish, the reconstruction does not take the event and the gap
        // pass's refusal stands again ([`deferred_edge_vanish`])
        if let Some(&edge) = vanished.iter().next() {
            return Err(deferred_edge_vanish(edge));
        }
        return Ok(Reconstruction::empty(map.clone()));
    }
    // 1. the events per original edge (map order is deterministic) and the clips with
    //    their endpoint-contact proofs
    let mut events: Vec<EdgeEventRecord> = Vec::new();
    let mut clips: Vec<ClipRecord> = Vec::new();
    let mut reconstructed_kept: BTreeSet<crate::topology::EdgeId> = BTreeSet::new();
    // a cut the endpoint certificate's scope cannot decide (a non-planar moved face):
    // the reconstruction engages only with the events that fire, and refuses then
    let mut undecided: Option<SolidError> = None;
    // (issue #13622's hook) DISCHARGE the gap pass's vanish obligations first, before
    // any other cut gets in the way: per pending original, complete unambiguous
    // correspondence (its group is there, and no result edge is shared with another
    // original) and a classification certifying its REMOVAL (the only cut whose edge is
    // gone). A `Clear` or `Trimmed` cut, a missing or ambiguous correspondence, or a cut
    // that cannot be certified (an uncertifiable clip) leaves ownership unestablished —
    // the reconstruction does not take the vanish — and the gap pass's refusal stands
    // again ([`deferred_edge_vanish`]). A cut the endpoint certificate's scope cannot
    // decide (a non-planar moved surface) refuses its own typed refusal: obligations are
    // never suppressible.
    for &pending in vanished {
        let group = map.get(&pending).map(Vec::as_slice).unwrap_or_default();
        let unambiguous = !group.is_empty()
            && group
                .iter()
                .all(|e| map.values().filter(|other| other.contains(e)).count() == 1);
        if !unambiguous {
            return Err(deferred_edge_vanish(pending));
        }
        let mut removal = false;
        for &e in group {
            let Some(record) = b.edges().iter().find(|(x, _)| *x == e).map(|(_, r)| r) else {
                // a descendant the body does not have: nothing certifies its removal
                return Err(deferred_edge_vanish(pending));
            };
            let Some(curve) = record.curve.clone() else {
                return Err(null("the edge's curve"));
            };
            let interval = record.interval;
            for face in &cuts {
                let tolerance = (Ivl::from(record.tolerance) + Ivl::from(face.moved.tolerance)).hi;
                let crossing =
                    match classify_edge_crossing(k, &curve, &interval, face, tolerance, e) {
                        Ok(crossing) => crossing,
                        Err(err) => {
                            let undecided = matches!(
                                err.failure().map(|f| &f.reason),
                                Some(crate::error::Reason::Offset(
                                    OffsetFailure::Reconstruction {
                                        why: ReconstructionRefusal::SurfaceUncertified { .. }
                                    }
                                ))
                            );
                            if undecided {
                                return Err(err);
                            }
                            return Err(deferred_edge_vanish(pending));
                        }
                    };
                match crossing.event {
                    CrossEvent::Clear => {}
                    CrossEvent::Removed { .. } => removal = true,
                    CrossEvent::Trimmed { .. } => return Err(deferred_edge_vanish(pending)),
                }
            }
        }
        if !removal {
            return Err(deferred_edge_vanish(pending));
        }
    }
    for (&original, result_edges) in map {
        k.check_cancelled(OP)?;
        if !crossable.contains(&original) {
            // an edge beside a moved face is rebuilt around it: no cut of its own
            continue;
        }
        let mut group: Option<CrossEvent> = None;
        for &e in result_edges {
            let Some(record) = b.edges().iter().find(|(x, _)| *x == e).map(|(_, r)| r) else {
                continue;
            };
            let Some(curve) = record.curve.clone() else {
                return Err(null("the edge's curve"));
            };
            let interval = record.interval;
            let edge_tolerance = record.tolerance;
            let mut event: Option<(
                CrossEvent,
                Option<clearance::endpoint_contact::ContactProof>,
                FaceId,
            )> = None;
            for face in &cuts {
                let tolerance = (Ivl::from(edge_tolerance) + Ivl::from(face.moved.tolerance)).hi;
                let crossing =
                    match classify_edge_crossing(k, &curve, &interval, face, tolerance, e) {
                        Ok(crossing) => crossing,
                        Err(e) => {
                            let uncertified = matches!(
                                e.failure().map(|f| &f.reason),
                                Some(crate::error::Reason::Offset(
                                    OffsetFailure::Reconstruction {
                                        why: ReconstructionRefusal::SurfaceUncertified { .. }
                                    }
                                ))
                            );
                            if uncertified {
                                undecided = Some(e);
                                continue;
                            }
                            return Err(e);
                        }
                    };
                if crossing.event != CrossEvent::Clear {
                    // one cut per edge is the reconstruction's pattern: two moved faces
                    // cutting one edge is not certified here
                    if event.is_some() {
                        return Err(refusal(ReconstructionRefusal::ClipUncertified { edge: e }));
                    }
                    event = Some((crossing.event, crossing.proof, face.face));
                }
            }
            if let Some((ev, proof, clip_face)) = event {
                if let CrossEvent::Trimmed {
                    clip,
                    clip_t,
                    removes_low,
                } = ev
                {
                    clips.push(ClipRecord {
                        edge: e,
                        face: clip_face,
                        clip,
                        clip_t,
                        removes_low,
                        proof,
                    });
                }
                match &group {
                    None | Some(CrossEvent::Clear) => group = Some(ev),
                    Some(prev) if ev == CrossEvent::Clear => {
                        let _ = prev;
                    }
                    Some(prev) => {
                        if *prev != ev {
                            return Err(refusal(ReconstructionRefusal::ClipUncertified {
                                edge: e,
                            }));
                        }
                    }
                }
            }
        }
        let Some(event) = group else {
            continue;
        };
        if event != CrossEvent::Clear {
            if kept.iter().any(|(id, _)| *id == original) {
                reconstructed_kept.insert(original);
            }
            events.push(EdgeEventRecord { original, event });
        }
    }
    if events.is_empty() {
        // (issue #13622's hook) obligations are not suppressible: where a cut the
        // endpoint certificate's scope cannot decide left one undecided, its typed
        // refusal stands ([`ReconstructionRefusal::SurfaceUncertified`]); a
        // no-engagement return discharges nothing, and where no cut was undecided the
        // deferred vanish's refusal stands again ([`deferred_edge_vanish`])
        if let Some(&edge) = vanished.iter().next() {
            if let Some(e) = undecided {
                return Err(e);
            }
            return Err(deferred_edge_vanish(edge));
        }
        // no reconstruction is engaged: the operation behaves as it always did (the
        // gates decide), even where a non-planar cut left a group undecided
        return Ok(Reconstruction::empty(map.clone()));
    }
    if let Some(e) = undecided {
        // the reconstruction engaged, but a cut is outside the endpoint certificate's
        // scope (ARC 1's documented planar scope): nothing uncertain is built
        return Err(e);
    }
    // the cut corner's vertices: the ends of the removed edges' uses
    let mut corner_vertices: Vec<crate::topology::VertexId> = Vec::new();
    for record in &events {
        if let CrossEvent::Removed { .. } = record.event {
            for &e in map
                .get(&record.original)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                for eu in edge::edgeuses(b, e, None, None)? {
                    if let Some(v) =
                        eu1::vertexuse(b, eu)?.and_then(|vu| vu1::vertex(b, vu).ok().flatten())
                    {
                        corner_vertices.push(v);
                    }
                }
            }
        }
    }
    corner_vertices.sort_unstable();
    corner_vertices.dedup();
    // the repair's bookkeeping
    let mut bridges: Vec<crate::topology::EdgeId> = Vec::new();
    let mut touched_edges: Vec<crate::topology::EdgeId> = Vec::new();
    let mut touched_vertices: Vec<crate::topology::VertexId> = Vec::new();
    let mut affected_faces: Vec<FaceId> = Vec::new();
    let naming = b.naming();
    let key_of = move |e: EntityRef| crate::topology::scheme_owner_identity(naming, e);
    // 2. SEVER the removed edges and MERGE the lamina twins a surviving group carries,
    //    under the caller's notification identities
    Context::with_notification_context(k, |k, context| -> Result<()> {
        let mut env = NotifyEnv {
            records: notifications.records,
            attributes: notifications.attributes,
            context: Some(context),
            key_of: &key_of,
        };
        crate::topology::brep::p3::seed_records(env.records, b, &key_of);
        for record in &events {
            if let CrossEvent::Removed { .. } = record.event {
                for &e in map
                    .get(&record.original)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    sever_edge(k, b, e, &mut env)?;
                }
            }
        }
        for (&original, result_edges) in map {
            if result_edges.len() != 2 {
                continue;
            }
            let alive: Vec<_> = result_edges
                .iter()
                .copied()
                .filter(|e| b.edges().iter().any(|(x, _)| x == *e))
                .collect();
            if alive.len() != 2 {
                // a severed group's twins go with their edges
                continue;
            }
            let lamina = alive
                .iter()
                .map(|&e| Ok(eu1::edge_is_lamina(b, e)? && edge::edgeuse_count(b, e)? == 2))
                .collect::<Result<Vec<_>>>()?;
            if !lamina.iter().all(|x| *x) {
                continue;
            }
            let has_event = events.iter().any(|record| record.original == original);
            // the cut corner's neighbourhood: a lamina twin pair one of whose edges sits
            // at a cut corner vertex resolves the same way (the spike's measured extra
            // repair)
            let at_corner = alive.iter().any(|&e| {
                edge::edgeuses(b, e, None, None).is_ok_and(|uses| {
                    uses.iter().any(|&eu| {
                        eu1::vertexuse(b, eu).is_ok_and(|vu| {
                            vu.and_then(|vu| vu1::vertex(b, vu).ok().flatten())
                                .is_some_and(|v| corner_vertices.contains(&v))
                        })
                    })
                })
            });
            if has_event || at_corner {
                if let [first, second] = alive.as_slice() {
                    merge_lamina_twins(b, *first, *second)?;
                }
            }
        }
        Ok(())
    })??;
    // (issue #13622's hook) every corresponding descendant of a pending original is
    // severed: a surviving one cannot discharge the obligation
    for &pending in vanished {
        let survivors = map.get(&pending).map(Vec::as_slice).unwrap_or_default();
        if survivors
            .iter()
            .any(|e| b.edges().iter().any(|(x, _)| x == *e))
        {
            return Err(refusal(ReconstructionRefusal::VanishUnresolved {
                edge: pending,
            }));
        }
    }
    // 3. the cut corner: every edge incident to a corner vertex takes the clip point on
    //    its curve (its own certified clip, else the intended point its curve passes
    //    through: the projection of the clip onto the curve, exact for the affine
    //    curves this pass clips)
    let mut corner_edges: Vec<CornerEdge> = Vec::new();
    let mut clip_points: Vec<Point3> = clips.iter().map(|c| c.clip).collect();
    clip_points.sort_by(|p, q| {
        (p.x, p.y, p.z)
            .partial_cmp(&(q.x, q.y, q.z))
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    clip_points.dedup();
    let corner_points: Vec<Point3> = corner_vertices
        .iter()
        .map(|&cv| b.vertex(cv).map(|r| r.point))
        .collect::<Result<_>>()?;
    for &v in &corner_vertices {
        for e in crate::topology::vertex::edges(b, v, None)? {
            if corner_edges.iter().any(|c| c.edge == e) {
                continue;
            }
            let record = b.edge(e)?;
            let Some(curve) = record.curve.clone() else {
                return Err(null("the edge's curve"));
            };
            let interval = record.interval;
            let tolerance = record.tolerance.max(b.tolerance());
            let (clip, clip_t, drop_min) = if let Some(own) = clips.iter().find(|c| c.edge == e) {
                (own.clip, own.clip_t, own.removes_low)
            } else {
                let mut found: Option<(Point3, f64)> = None;
                for p in &clip_points {
                    let t = clip_param(k, &curve, interval, *p)?;
                    if t < interval.min() || t > interval.max() {
                        continue;
                    }
                    let at = curve.evaluate_point(k, t)?;
                    if at.distance(*p) <= tolerance {
                        if found.is_some() {
                            return Err(refusal(ReconstructionRefusal::ClipUncertified {
                                edge: e,
                            }));
                        }
                        found = Some((*p, t));
                    }
                }
                let Some((p, t)) = found else {
                    return Err(refusal(ReconstructionRefusal::ClipUncertified { edge: e }));
                };
                // the end that goes is the one at the cut corner
                let (p_min, p_max) = (
                    curve.evaluate_point(k, interval.min())?,
                    curve.evaluate_point(k, interval.max())?,
                );
                let drop_min = corner_points
                    .iter()
                    .any(|old| p_min.distance(*old) < p_max.distance(*old));
                (p, t, drop_min)
            };
            corner_edges.push(CornerEdge {
                edge: e,
                clip,
                clip_t,
                drop_min,
            });
            touched_edges.push(e);
            affected_faces.extend(edge::faces(b, e, None)?);
        }
    }
    if let Some(first_clip) = clips.first() {
        if corner_edges.is_empty() {
            return Err(refusal(ReconstructionRefusal::ClipUncertified {
                edge: first_clip.edge,
            }));
        }
    }
    // 4. VERTEX SPLIT at the diverging ends: one copy per distinct clip point beyond
    //    the first (deterministic: the clip points' order), then RETRIM every corner
    //    edge to its clip and move each clip vertex's point to its intended point
    let mut clip_vertices: Vec<(Point3, Vec<crate::topology::VertexId>)> = Vec::new();
    for p in &clip_points {
        clip_vertices.push((*p, Vec::new()));
    }
    for &v in &corner_vertices {
        k.check_cancelled(OP)?;
        let incident = crate::topology::vertex::edges(b, v, None)?;
        let mut groups: Vec<(Point3, Vec<crate::topology::EdgeId>)> = Vec::new();
        for ce in &corner_edges {
            if !incident.contains(&ce.edge) {
                continue;
            }
            match groups.iter_mut().find(|(p, _)| *p == ce.clip) {
                Some((_, es)) => es.push(ce.edge),
                None => groups.push((ce.clip, vec![ce.edge])),
            }
        }
        groups.sort_by(|(p, _), (q, _)| {
            (p.x, p.y, p.z)
                .partial_cmp(&(q.x, q.y, q.z))
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        for (index, (p, es)) in groups.iter().enumerate() {
            let holder = if index == 0 {
                v
            } else {
                split_vertex(b, v, es)?
            };
            if let Some(slot) = clip_vertices.iter_mut().find(|(q, _)| q == p) {
                slot.1.push(holder);
            }
            if index > 0 {
                touched_vertices.push(holder);
            }
        }
    }
    for ce in &corner_edges {
        retrim_edge(k, b, ce.edge, ce.clip_t, ce.drop_min)?;
    }
    for (p, holders) in &clip_vertices {
        for &v in holders {
            b.vertex_mut(v)?.point = *p;
            touched_vertices.push(v);
        }
    }
    // 5. MERGE the coincident intended points (`p5::glue_vertices`): the clip point's
    //    vertices become one
    for (_, holders) in &mut clip_vertices {
        holders.sort_unstable();
        holders.dedup();
        if let Some(&survivor) = holders.first() {
            let doomed: Vec<_> = holders.iter().skip(1).copied().collect();
            for &gone in &doomed {
                if gone != survivor {
                    p5::glue_vertices(k, b, Some(survivor), Some(gone), None)?;
                    touched_vertices.retain(|&x| x != gone);
                }
            }
            holders.clear();
            holders.push(survivor);
            touched_vertices.push(survivor);
        }
    }
    b.refresh_cached_topology();
    // 6. BRIDGE: the corner gaps left (the cut the clip removed from the faces that
    //    bounded it) close with one four-use edge per gap pair
    let mut gap_slots: BTreeMap<
        [u32; 2],
        Vec<(
            crate::topology::LoopuseId,
            crate::topology::EdgeuseId,
            crate::topology::EdgeuseId,
        )>,
    > = BTreeMap::new();
    for (lu, prev, next) in loop_gaps(b)? {
        let (prev_end, next_start) = (use_ends(b, prev)?.1, use_ends(b, next)?.0);
        let (Some(u), Some(w)) = (prev_end, next_start) else {
            return Err(refusal(ReconstructionRefusal::BoundaryInvalid {
                fault: BoundaryFault::LoopDiverges { loopuse: lu },
            }));
        };
        if u == w {
            continue;
        }
        let pair = [u.index(), w.index()];
        let pair = [pair[0].min(pair[1]), pair[0].max(pair[1])];
        gap_slots.entry(pair).or_default().push((lu, prev, next));
    }
    Context::with_notification_context(k, |k, context| -> Result<()> {
        let mut env = NotifyEnv {
            records: notifications.records,
            attributes: notifications.attributes,
            context: Some(context),
            key_of: &key_of,
        };
        for (pair, slots) in &gap_slots {
            let [low, high] = *pair;
            let (a, by) = (
                crate::topology::VertexId::from_index(low),
                crate::topology::VertexId::from_index(high),
            );
            let e = insert_bridge(k, b, a, by, slots, &mut env)?;
            bridges.push(e);
            touched_edges.push(e);
            for &(lu, _, _) in slots {
                let fu = b
                    .loopuse(lu)?
                    .faceuse
                    .ok_or_else(|| null("the loop use's face use"))?;
                if let Some(f) = eu1::faceuse_face(b, fu)? {
                    affected_faces.push(f);
                }
            }
        }
        Ok(())
    })??;
    // 7. the region pair `move_faces` dropped, the affected tolerances re-measured
    //    (never grown), and the completed boundary validated
    Context::with_notification_context(k, |k, context| -> Result<()> {
        let mut env = NotifyEnv {
            records: notifications.records,
            attributes: notifications.attributes,
            context: Some(context),
            key_of: &key_of,
        };
        restore_region_pair(k, b, &mut env)
    })??;
    b.refresh_cached_topology();
    touched_edges.sort_unstable();
    touched_edges.dedup();
    touched_vertices.sort_unstable();
    touched_vertices.dedup();
    affected_faces.sort_unstable();
    affected_faces.dedup();
    let tolerances = tighten_reconstruction_tolerances(k, b, &touched_edges, &touched_vertices)?;
    validate_completed_boundary(k, b, &clips, &bridges, &affected_faces)?;
    let proofs = clips
        .into_iter()
        .filter_map(|c| c.proof.map(|p| (c.edge, p)))
        .collect();
    Ok(Reconstruction {
        events,
        proofs,
        map: map.clone(),
        reconstructed_kept,
        bridges,
        tolerances,
        discharged_vanishes: vanished.clone(),
    })
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
    use crate::kernel::UpstreamFixes;
    use crate::topology::topology::Topology;

    #[test]
    fn parallel_plane_origin_uses_determinants_before_rounding_the_origin_difference() {
        let k = Kernel::default();
        let t = 2.0_f64.powi(-25) + 2.0_f64.powi(-77);
        let x = Vec3::new(1.0, t, 0.0);
        let y = Vec3::Z;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let scale = crate::math::Vec2::new(1.0, 1.0);
        let origin = x * 2.0_f64.powi(-53);
        let own = Surface::from(Plane::new(origin, x, y, scale, &domain));
        let coincident = Surface::from(Plane::new(x, x, y, scale, &domain));
        // Both origins lie exactly in the stored axis span, but rounding x-origin
        // first loses a low bit. Projecting that difference invents a normal move.
        assert!(exact::is_zero(&exact::det3(x, y, origin).unwrap()));
        assert!(exact::is_zero(&exact::det3(x, y, x).unwrap()));
        assert!(!exact::is_zero(
            &exact::dot(x.cross(y), x - origin).unwrap()
        ));
        let moved = moved_surface_on_own_frame(&k, &own, &coincident)
            .unwrap()
            .expect("exactly coincident planes");
        let SurfaceKind::Plane(plane) = moved.kind() else {
            panic!("a moved plane stays a plane");
        };
        assert_eq!(plane.position().origin(), origin);
        assert_eq!(plane.position().x_axis(), x);
        assert_eq!(plane.position().y_axis(), y);

        // A nonzero move must put the new origin on y=t*x-2, not merely copy it.
        let given = Surface::from(Plane::new(Vec3::new(0.0, -2.0, 0.0), x, y, scale, &domain));
        let moved = moved_surface_on_own_frame(&k, &own, &given)
            .unwrap()
            .expect("exactly parallel planes");
        let SurfaceKind::Plane(plane) = moved.kind() else {
            panic!("a moved plane stays a plane");
        };
        let p = plane.position().origin();
        assert!(p.y < -1.9, "{p:?}");
        assert!((t * p.x - p.y - 2.0).abs() <= 8.0 * f64::EPSILON, "{p:?}");
    }

    #[test]
    fn parallel_net_moves_every_control_point_using_the_given_stored_plane_axes() {
        let k = Kernel::default();
        let t = 2.0_f64.powi(-25) + 2.0_f64.powi(-77);
        let x = Vec3::new(1.0, t, 0.0);
        let y = Vec3::Z;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let plane = Plane::new(
            Vec3::new(0.0, -2.0, 0.0),
            x,
            y,
            crate::math::Vec2::new(1.0, 1.0),
            &domain,
        );
        assert_eq!(plane.position().x_axis(), x);
        assert_eq!(plane.position().y_axis(), y);
        // The rounded/unitized normal is not exactly perpendicular to its own
        // plane's stored x axis. Dot(normal,H) would reject this parallel net.
        assert!(!exact::is_zero(
            &exact::dot(plane.position().z_axis(), x).unwrap()
        ));
        let own = Surface::from(
            BSplineSurface::create_bilinear_surface(Vec3::ZERO, x, y, x + y).unwrap(),
        );
        let moved = moved_surface_on_own_frame(&k, &own, &Surface::from(plane))
            .unwrap()
            .expect("the whole net lies in the given stored axis span");
        let (SurfaceKind::BSpline(before), SurfaceKind::BSpline(after)) =
            (own.kind(), moved.kind())
        else {
            panic!("a moved net stays a net");
        };
        let (before, after) = (before.gw_nurb().unwrap(), after.gw_nurb().unwrap());
        assert_eq!(before.u_knots(), after.u_knots());
        assert_eq!(before.v_knots(), after.v_knots());
        let first = after.rows().flatten().next().unwrap();
        let translation = Vec3::new(first.x, first.y, first.z);
        assert!(translation.y < -1.9, "{translation:?}");
        for (a, b) in before.rows().flatten().zip(after.rows().flatten()) {
            assert_eq!(a.w, b.w);
            assert_eq!(
                Vec3::new(b.x, b.y, b.z),
                Vec3::new(a.x, a.y, a.z) + translation
            );
            assert!((t * b.x - b.y - 2.0).abs() <= 8.0 * f64::EPSILON, "{b:?}");
        }
        for [u, v] in [[0.0, 0.0], [0.25, 0.75], [0.5, 0.5], [1.0, 1.0]] {
            let uv = crate::math::Point2::new(u, v);
            let a = own.evaluate_point(&k, uv).unwrap();
            let b = moved.evaluate_point(&k, uv).unwrap();
            assert!((b - (a + translation)).length() <= 8.0 * f64::EPSILON);
            assert!((t * b.x - b.y - 2.0).abs() <= 8.0 * f64::EPSILON, "{b:?}");
        }
    }

    #[test]
    fn perpendicularity_to_the_rounded_normal_does_not_prove_parallel_planes() {
        let k = Kernel::default();
        let t = 2.0_f64.powi(-25) + 2.0_f64.powi(-77);
        let x = Vec3::new(1.0, t, 0.0);
        let y = Vec3::Z;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let scale = crate::math::Vec2::new(1.0, 1.0);
        let plane = Plane::new(Vec3::ZERO, x, y, scale, &domain);
        let m = plane.position().z_axis();
        let a = Vec3::new(-m.y, m.x, 0.0);
        assert!(exact::is_zero(&exact::dot(m, a).unwrap()));
        assert!(!exact::is_zero(&exact::det3(x, y, a).unwrap()));
        let given = Surface::from(plane);
        let own_plane = Surface::from(Plane::new(Vec3::ZERO, a, y, scale, &domain));
        assert!(moved_surface_on_own_frame(&k, &own_plane, &given)
            .unwrap()
            .is_none());
        // Scaling amplifies the mismatch in the control coordinates. It remains
        // exactly perpendicular to m, but not in the given plane's stored span.
        let a = a * 2.0_f64.powi(80);
        assert!(exact::is_zero(&exact::dot(m, a).unwrap()));
        assert!(!exact::is_zero(&exact::det3(x, y, a).unwrap()));
        let own_net = Surface::from(
            BSplineSurface::create_bilinear_surface(Vec3::ZERO, a, y, a + y).unwrap(),
        );
        assert!(moved_surface_on_own_frame(&k, &own_net, &given)
            .unwrap()
            .is_none());
    }

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

    // Issue 2976's named case (the top of a 2 x 2 x 2 box becomes the plane z = x + y -
    // 1) is now `a_tilted_top_that_crosses_unmoved_edges_reconstructs_the_boundary_around_the_crossings`
    // in the reconstruction section below: the boundary around the crossings is
    // reconstructed, not refused.

    /// The check of the kept edges against the faces moved (issue #2976, certified bound
    /// #2971), on the result of raising a 2-cube's top to the plane z = 3 given as
    /// `Plane::from_normal`, whose net is about 1.2e6 wide while the face on it is 2 x 2.
    /// The moved face carries the mark. A vertical edge through the face's region at
    /// (1, 1) refuses the operation (`KeptEdgeNotClear`); one through the plane's net far
    /// outside the face, at (50, 50), is clear of the face (tested against the whole
    /// surface instead of the face's trimmed box, it would be called crossed). The marks
    /// are off the body after either.
    #[test]
    fn a_kept_edge_through_a_moved_face_refuses_and_one_through_its_plane_beside_it_does_not() {
        use crate::curve::curve::Curve;
        use crate::curve::line::Line;
        use crate::error::offset::Unsoundness;
        use crate::geom::extent1d::Extent1d;
        let kept_through = |x: f64, y: f64| KeptEdge {
            curve: Curve::from(Line::from_points(
                Point3::new(x, y, 2.5),
                Point3::new(x, y, 3.5),
                3,
            )),
            interval: Extent1d::from_min_max(0.0, 1.0),
            tolerance: 1e-5,
        };
        for (x, y, refused) in [(1.0, 1.0, true), (50.0, 50.0, false)] {
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
            let raised = Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, 3.0), Vec3::Z));
            with_notifications(&mut b, |b, notes| {
                local_operation(&mut k, b, &[top], vec![raised], notes)
            })
            .unwrap();
            let moved = bp1::faces(&b, None)
                .unwrap()
                .into_iter()
                .find(|&f| {
                    let s = face_ops::surface(&b, f).unwrap().unwrap();
                    let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
                    (s.evaluate_point(&k, uv).unwrap().z - 3.0).abs() < 1e-9
                })
                .unwrap();
            b.add_attribute(
                EntityRef::Face(moved),
                Attribute::long(MOVED_FACE_MARK, 1, Behavior::Copy),
            )
            .unwrap();
            let r = refuse_kept_edges_not_clear(
                &mut k,
                &mut b,
                &[(crate::topology::EdgeId::from_index(0), kept_through(x, y))],
                &Reconstruction::empty(BTreeMap::new()),
            );
            if refused {
                let e = r.unwrap_err();
                assert!(
                    matches!(
                        e.failure().map(|f| &f.reason),
                        Some(crate::error::Reason::Offset(OffsetFailure::Unsound {
                            what: Unsoundness::KeptEdgeNotClear
                        }))
                    ),
                    "{e}"
                );
            } else {
                r.unwrap();
            }
            assert!(b.entity_attributes(EntityRef::Face(moved)).is_empty());
        }
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
        let e = r.unwrap_err();
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR_NULL_POINTER));
        assert_eq!(e.operation(), OP);
        assert_eq!(
            e.topology_failure(),
            Some(&crate::error::topology::TopologyFailure::Missing(
                "a surface for every face"
            ))
        );
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
        // refused as not sound by the check after the body was built; which lines it
        // reports, and where the first one is, are incidental to how the body was built,
        // so only the class is asserted (the input's own `Debug` form is checked above)
        assert!(
            matches!(
                why,
                Some(OffsetFailure::Unsound {
                    what: crate::error::offset::Unsoundness::Check { .. }
                })
            ),
            "{why:?}"
        );
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

    /// A whole torus's one face, meeting only itself across its seams, given the coaxial
    /// torus a half wider (its offset by 0.5): it has no neighbour, so no reach to bound,
    /// and the body grows to the new torus, `2 π² R r²`, with the same face, edges and
    /// vertex. (The reach used to be asked of the torus's class first, which has no
    /// certified bound, and the move was refused.)
    #[test]
    fn a_torus_given_a_wider_coaxial_torus_moves_to_it() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let mut b = ap::create_torus(&mut k, Point3::ZERO, 3.0, 1.0).unwrap();
        let face = curved_face(&b);
        let before = (b.faces().len(), b.edges().len(), b.vertices().len());
        let s = Surface::from(ap::create_torus_surface(&k, Point3::ZERO, 3.0, 1.5).unwrap());
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[face], vec![s], notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        let want = 2.0 * std::f64::consts::PI.powi(2) * 3.0 * 1.5 * 1.5;
        assert!((v - want).abs() <= 1e-6 * want, "{v} vs {want}");
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            before
        );
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
    /// parts on the default split path. The new vertex is next to the apex, a pole of the
    /// fourth side's B-spline patch, and on no extension of it (a side's extension narrows to
    /// nothing at the pole): the side, which lies on a plane, continues as that plane. The
    /// body is the convex solid of the five planes, a sound manifold of nine edges and six
    /// vertices.
    #[test]
    fn a_pyramid_side_moved_past_its_apex_splits_it_on_the_planes_beside() {
        assert_pyramid_split_on_its_five_planes([2, 1]);
    }

    /// Both selections describe the same five-plane convex solid. Check its
    /// independent plane-intersection vertices as well as its volume and validity.
    fn assert_pyramid_split_on_its_five_planes(selection: [usize; 2]) {
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        let mut k = Kernel::default();
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
        let (moved, distance) = (1_usize, 0.407_738_046_172_282_6);
        let mut given = planes.clone();
        given[moved].0 = given[moved].0 + given[moved].1 * distance;
        let surfaces = selection
            .map(|i| Surface::from(Plane::from_normal(given[i].0, given[i].1)))
            .to_vec();
        let selected_faces = selection.map(|i| all[i]);
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &selected_faces, surfaces, notes)
        })
        .unwrap();
        let (v, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold);
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            (5, 9, 6)
        );
        b.use_arena_naming().unwrap();
        for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
            let check = check_brep(&mut k, &mut b, level).unwrap();
            assert!(
                check.is_valid() && check.failure_count() == 0,
                "{selection:?} {level:?}: {:?}",
                check.lines()
            );
        }
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
        let expected_vertices = [b0, b1, b2, b3, apex, top];
        let actual_vertices: Vec<_> = b.vertices().iter().map(|(_, v)| v.point).collect();
        let scale = expected_vertices
            .iter()
            .map(|p| p.max_dimension())
            .fold(0.0, f64::max);
        let same_point = |p: Vec3, q: Vec3| p.distance(q) <= 1e-9 * scale;
        for &p in &actual_vertices {
            assert!(
                expected_vertices.iter().any(|&q| same_point(p, q)),
                "{selection:?}: unexpected vertex {p:?}"
            );
        }
        for &p in &expected_vertices {
            assert!(
                actual_vertices.iter().any(|&q| same_point(p, q)),
                "{selection:?}: missing plane intersection {p:?}"
            );
        }
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
    /// caller's notification records and attributes. Refused while the offset's loop gaps
    /// are closed — the straight bridge that would join a gap is not established on the
    /// face's own surface within the operation's tolerance — a cube's round given a narrower
    /// coaxial cylinder, and a side moved off its round (the edge between them apart:
    /// `LocalReach::apart`, adding nothing to the reach); `b` carries an attribute of its
    /// own, lent to the offset, and the caller's store one of the caller's.
    #[test]
    fn a_refusal_leaves_the_input_its_caches_and_the_callers_stores_as_they_were() {
        use crate::geom::attribute::{Attribute, Behavior};
        use crate::primitive::api_primitives as ap;
        type Make = fn(&mut Kernel) -> (Brep, FaceId, Surface, FaceId);
        let cases: [(&str, Make); 2] = [
            // the round is the face whose offset loop's gap the bridge would join
            ("round", |k| {
                let (b, round) = box_rounded_r2(k);
                let s = ap::create_cylinder_surface(k, Point3::new(8.0, 8.0, -1.0), 1.8, 12.0);
                (b, round, Surface::from(s.unwrap()), round)
            }),
            // the moved side is the face whose offset loop's gap the bridge would join
            ("side", |k| {
                let b = cube_rounded_r5(k);
                let side = face_on_x(k, &b, 20.0);
                let s = Plane::from_normal(Vec3::new(20.1, 0.0, 0.0), Vec3::X);
                (b, side, Surface::from(s), side)
            }),
        ];
        for (name, make) in cases {
            let mut k = Kernel::default();
            let (mut b, face, s, at) = make(&mut k);
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
            assert_eq!(
                e.kind(),
                crate::error::ErrorKind::Unsupported,
                "{name}: {e}"
            );
            assert!(
                matches!(
                    e.failure().map(|f| &f.reason),
                    Some(crate::error::Reason::Offset(
                        OffsetFailure::LoopGapUncertified { face: refused }
                    )) if *refused == Some(at)
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

    /// A body whose volume's sign is not proved is refused as such: the quarter shell R 10,
    /// t 1e-6 passes the check, but its faces' boundaries lie apart by about its thickness,
    /// so the strips between them hold 0 in its enclosure whatever its cells become.
    #[test]
    fn the_gate_refuses_a_body_whose_sign_it_cannot_prove() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylindrical_box(
            &mut k,
            Point3::ZERO,
            10.0,
            10.0,
            10.0 + 1e-6,
            0.0,
            90.0,
        )
        .unwrap();
        let input =
            validation::standing(&mut k, &mut b.clone_standing_for_self().unwrap()).unwrap();
        let e = refuse_unsound(&mut k, &b, &input).unwrap_err();
        assert!(
            matches!(
                e.failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(OffsetFailure::Unsound {
                    what: crate::error::offset::Unsoundness::VolumeUndecided
                }))
            ),
            "{e}"
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

    /// A 10 mm cube's vertical edge rounded r 2, the round given the narrower coaxial
    /// cylinder r 1.8. The sides x = 10 and y = 10 pass it 0.2 off, so their tangent edges
    /// are apart, and the construction cannot establish the gap's straight bridge on the
    /// round within tolerance. It names the original round and refuses without changing
    /// the input. (A wider cylinder is met on the corner's side:
    /// `tangent_edges_a_round_given_a_wider_coaxial_cylinder_meets_its_sides_on_the_corner_side`.)
    #[test]
    fn a_round_given_a_narrower_coaxial_cylinder_is_refused_at_its_loop_gap() {
        use crate::primitive::api_primitives as ap;
        let mut k = Kernel::default();
        let (mut b, round) = box_rounded_r2(&mut k);
        let s = Surface::from(
            ap::create_cylinder_surface(&k, Point3::new(8.0, 8.0, -1.0), 1.8, 12.0).unwrap(),
        );
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[round], vec![s]);
        assert_eq!(
            why,
            Some(OffsetFailure::LoopGapUncertified { face: Some(round) })
        );
    }

    /// The former flag-off refusal is no longer a default behaviour. With the same
    /// two replacement planes selected in reverse order, the unconditional split
    /// must still build the analytically checked five-plane solid, not just succeed.
    #[test]
    fn a_pyramid_side_split_is_independent_of_replacement_selection_order() {
        assert_pyramid_split_on_its_five_planes([1, 2]);
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
    /// interior knot is a crease. The roof is a bare net beside the sides, and the reach is
    /// bounded where it meets them (across the crease too: the net is continuous there), so
    /// a body is built; it fails more of the check than the cube did (the sides along the
    /// ridge meet the roof in a kinked line), so the gate refuses it as `Unsound`, and the
    /// cube is as it came in, with the input's own failures.
    #[test]
    fn a_creased_roof_on_a_defective_input_is_refused_as_unsound() {
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
        assert!(
            matches!(
                why,
                Some(OffsetFailure::Unsound {
                    what: crate::error::offset::Unsoundness::Check { .. }
                })
            ),
            "{why:?}"
        );
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

    /// The fix sets the tangent-edge cases run with: none (the kernel's default) and every
    /// one (as an application turns them on, `offset_analytic_mate_edges` among them).
    fn tangent_edge_fixes() -> [UpstreamFixes; 2] {
        [UpstreamFixes::default(), UpstreamFixes::all()]
    }

    /// The cube's round r 5 given the coaxial cylinder r 5.1 (moved out 0.1), the operation
    /// run with `fixes`.
    fn round_moved_out(k: &mut Kernel, fixes: UpstreamFixes) -> (Brep, Option<OffsetFailure>) {
        use crate::primitive::api_primitives as ap;
        let mut b = cube_rounded_r5(k);
        k.fixes = fixes;
        let round = curved_face(&b);
        let wider = Surface::from(
            ap::create_cylinder_surface(k, Point3::new(15.0, 15.0, -1.0), 5.1, 22.0).unwrap(),
        );
        let why = refused_unchanged_or_returned(k, &mut b, &[round], vec![wider]);
        (b, why)
    }

    /// The cube's side x = 20, tangent to the round, moved into the body by 0.1, the
    /// operation run with `fixes`.
    fn side_moved_into_its_round(
        k: &mut Kernel,
        fixes: UpstreamFixes,
    ) -> (Brep, Option<OffsetFailure>) {
        let mut b = cube_rounded_r5(k);
        k.fixes = fixes;
        let side = face_on_x(k, &b, 20.0);
        let moved = Surface::from(Plane::from_normal(Vec3::new(19.9, 0.0, 0.0), Vec3::X));
        let why = refused_unchanged_or_returned(k, &mut b, &[side], vec![moved]);
        (b, why)
    }

    /// Each of `wants` is a vertex of `b`, within the input's tolerance 1e-5 (the mated
    /// edges are intersections of the extended surfaces, within the B-rep's tolerance; a
    /// vertex's own tolerance grows with the fixes on, so it would let a stray vertex pass).
    fn assert_vertices_at(b: &Brep, wants: &[Point3]) {
        use crate::topology::vertex;
        let points: Vec<Point3> = b
            .vertices()
            .ids()
            .map(|v| vertex::point(b, v).unwrap())
            .collect();
        for want in wants {
            assert!(
                points.iter().any(|p| p.distance(*want) <= 1e-5),
                "no vertex at {want:?} in {points:?}"
            );
        }
    }

    /// The round moved out. The side planes, tangent to the round, cut the r 5.1 cylinder
    /// `w = √(5.1² − 5²) ≈ 1.005` either side of their old tangent lines; the tangent edges
    /// are met on the round's side, at (20, 15 + w) and (15 + w, 20), and the arcs end
    /// there. The body is the cube with its corner cut by the r 5.1 circle, `20 (375 + A)`,
    /// `A` the part of the corner square [15, 20]² inside the circle.
    #[test]
    fn tangent_edges_a_round_moved_out_has_its_closed_form_volume() {
        for (set, fixes) in tangent_edge_fixes().into_iter().enumerate() {
            let mut k = Kernel::default();
            let (mut b, why) = round_moved_out(&mut k, fixes);
            assert_eq!(why, None, "fix set {set}");
            let r = 5.1_f64;
            let w = (r * r - 25.0).sqrt();
            let p = Point3::new;
            let ends = [0.0, 20.0].map(|z| [p(20.0, 15.0 + w, z), p(15.0 + w, 20.0, z)]);
            assert_vertices_at(&b, ends.as_flattened());
            let inside = 5.0 * w + quarter_disc_area(r, 5.0) - quarter_disc_area(r, w);
            let want = 20.0 * (375.0 + inside);
            let (v, manifold) = volume_and_manifold(&k, &mut b);
            assert!(
                manifold && (v - want).abs() <= 1e-6 * want,
                "fix set {set}: {v} vs {want}"
            );
        }
    }

    /// The side moved into its round: the plane x = 19.9 cuts the round r 5 at
    /// `y = 15 ± √0.99`; the tangent edge is met on the round's own face, at
    /// (19.9, 15 + √0.99), and the round's arcs end there. The body:
    /// `20 (19.9 · 20 − 4.9 · 5 + ∫₀^4.9 √(25 − u²) du)`.
    #[test]
    fn tangent_edges_a_side_moved_into_its_round_has_its_closed_form_volume() {
        for (set, fixes) in tangent_edge_fixes().into_iter().enumerate() {
            let mut k = Kernel::default();
            let (mut b, why) = side_moved_into_its_round(&mut k, fixes);
            assert_eq!(why, None, "fix set {set}");
            let y = 15.0 + 0.99_f64.sqrt();
            assert_vertices_at(&b, &[0.0, 20.0].map(|z| Point3::new(19.9, y, z)));
            let want = 20.0 * (19.9 * 20.0 - 4.9 * 5.0 + quarter_disc_area(5.0, 4.9));
            let (v, manifold) = volume_and_manifold(&k, &mut b);
            assert!(
                manifold && (v - want).abs() <= 1e-6 * want,
                "fix set {set}: {v} vs {want}"
            );
        }
    }

    /// The 10 mm cube's round r 2 about (8, 8) given a wider coaxial cylinder `R`. The sides
    /// x = 10 and y = 10 cut it at 8 ± w, w = √(R² − 4); only 8 + w is on the corner's side
    /// (the corner square [8, 10]²). The tangent edges are met there, at (10, 8 + w) and
    /// (8 + w, 10), and the arcs that end the new round at the top and bottom end at those
    /// vertices: ten vertices in all. The body is the cube with its corner cut by the
    /// circle R: `10 (96 + A)`, `A` the part of the corner square inside the circle. The
    /// new round faces out as the old one did, its outer loop running the surface's way:
    /// the cylinder's domain middle is across the axis, where its normal opposes the old
    /// round's (with every fix on, the face's UV trims are checked and its volume read on
    /// them, so a round turned inside out is refused).
    #[test]
    fn tangent_edges_a_round_given_a_wider_coaxial_cylinder_meets_its_sides_on_the_corner_side() {
        use crate::primitive::api_primitives as ap;
        for (radius, fixes) in [2.3_f64, 2.05]
            .into_iter()
            .flat_map(|r| tangent_edge_fixes().map(|f| (r, f)))
        {
            let mut k = Kernel::default();
            let (mut b, round) = box_rounded_r2(&mut k);
            k.fixes = fixes;
            let s = Surface::from(
                ap::create_cylinder_surface(&k, Point3::new(8.0, 8.0, -1.0), radius, 12.0).unwrap(),
            );
            let why = refused_unchanged_or_returned(&mut k, &mut b, &[round], vec![s]);
            assert_eq!(why, None, "{radius}");
            let w = (radius * radius - 4.0).sqrt();
            let p = Point3::new;
            let ends = [0.0, 10.0].map(|z| [p(10.0, 8.0 + w, z), p(8.0 + w, 10.0, z)]);
            assert_vertices_at(&b, ends.as_flattened());
            assert_eq!(b.vertices().ids().count(), 10, "{radius}");
            let inside = 2.0 * w + quarter_disc_area(radius, 2.0) - quarter_disc_area(radius, w);
            let want = 10.0 * (96.0 + inside);
            let (v, manifold) = volume_and_manifold(&k, &mut b);
            assert!(
                manifold && (v - want).abs() <= 1e-6 * want,
                "{radius}: {v} vs {want}"
            );
            let round = curved_face(&b);
            let outer = face_ops::loops(&b, round, false, None).unwrap()[0];
            let along = crate::topology::face::p3::compute_loop_orientation(
                &k, &mut b, round, outer, None, false,
            );
            let same = crate::nurbs::b_spline_surface_n_lib::p2::OrientType::Same;
            assert_eq!(along.unwrap(), same, "{radius}");
        }
    }

    /// The same side moved out by 0.1: the plane x = 20.1 passes the round, which bends
    /// away from it, so no extension makes them meet: their edge is apart
    /// (`LocalReach::apart`, adding nothing to the reach), and the straight bridge that
    /// would join the offset loop's gap is not established on the side's own surface
    /// within the operation's tolerance: refused before any body is checked, the cube as
    /// it came in.
    #[test]
    fn a_side_moved_off_its_round_is_refused_before_its_gap_bridge_is_established() {
        let mut k = Kernel::default();
        let mut b = cube_rounded_r5(&mut k);
        let side = face_on_x(&k, &b, 20.0);
        // the moved side is the face whose offset loop's gap the bridge would join
        let moved = Surface::from(Plane::from_normal(Vec3::new(20.1, 0.0, 0.0), Vec3::X));
        let why = refused_unchanged_or_returned(&mut k, &mut b, &[side], vec![moved]);
        assert_eq!(
            why,
            Some(OffsetFailure::LoopGapUncertified { face: Some(side) })
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

    // ---- the crossed-boundary reconstruction (issue #13622) -------------------------

    /// The named case of issue #13622: a 2 x 2 x 2 box at the origin, its top face, and
    /// the plane `z = x + y - 1` to move the top to (`Plane::from_normal((0, 0, -1),
    /// (-1, -1, 1))`).
    fn named_case(k: &mut Kernel) -> (Brep, FaceId, Surface) {
        let b = create_box(k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let top = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face_ops::surface(&b, f).unwrap().unwrap();
                let uv = s.natural_uv_domain_with_nurb(k).unwrap().evaluate(0.5, 0.5);
                (s.evaluate_point(k, uv).unwrap().z - 2.0).abs() < 1e-9
            })
            .unwrap();
        let tilted = Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
        ));
        (b, top, tilted)
    }

    /// The vertex at exactly `want`.
    fn vertex_at(b: &Brep, want: Point3) -> Option<crate::topology::VertexId> {
        b.vertices()
            .iter()
            .map(|(v, _)| v)
            .find(|&v| b.vertex(v).unwrap().point == want)
    }

    /// The edge whose curve covers exactly `p` to `q` (the rebuilt curves evaluate
    /// their exact geometry to a rounding, ~1e-16 at this part's scale).
    fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> Option<crate::topology::EdgeId> {
        use crate::topology::edge;
        b.edges().iter().map(|(e, _)| e).find(|&e| {
            let c = edge::curve(b, e).unwrap().unwrap();
            let i = edge::interval(b, e).unwrap();
            let a = c.evaluate_point(k, i.min()).unwrap();
            let z = c.evaluate_point(k, i.max()).unwrap();
            let near = |x: Point3, y: Point3| x.distance(y) <= 1.0e-9;
            (near(a, p) && near(z, q)) || (near(a, q) && near(z, p))
        })
    }

    /// A moved face of `surface` over `uv_box` for [`classify_edge_crossing`]'s refusal
    /// tests: `plane` is the cut of a planar face, `None` for any other.
    fn cut_face(face: FaceId, surface: Surface, uv_box: Extent2d) -> MovedFaceCut {
        use crate::offset::offset_executive::clearance::endpoint_contact::TrimRegion;
        let plane = PlaneCut::of(&surface, true);
        MovedFaceCut {
            face,
            moved: MovedFace {
                surface,
                uv_box: Some(uv_box),
                tolerance: 1e-5,
            },
            region: TrimRegion { loops: Vec::new() },
            plane,
        }
    }

    /// Issue #13622: the top of a 2 x 2 x 2 box becomes the plane z = x + y - 1. It
    /// cuts the bottom edges y = 0 and x = 0 at (1, 0, 0) and (0, 1, 0), away from
    /// their vertices, and leaves nothing of the vertical edge above the corner
    /// (0, 0, 0): the plane meets its line at (0, 0, -1), outside the edge. The
    /// boundary around the crossings is reconstructed
    /// (`reconstruct_crossed_boundaries`): the vertical vanishes (its lamina twins
    /// sever), the four boundary edges of the corner re-trim to the two clip points
    /// A = (0, 1, 0) and B = (1, 0, 0) (the diverging ends split and glue), and one
    /// bridge edge closes the cut between them. The body is a closed manifold solid of
    /// the closed-form volume 25/6 (the part of the box below the plane), its check
    /// reports nothing and nothing beyond the input's, and the crossed edges are not
    /// returned with the crossing kept.
    #[test]
    fn a_tilted_top_that_crosses_unmoved_edges_reconstructs_the_boundary_around_the_crossings() {
        use crate::topology::edge;
        use crate::topology::edgeuse::p1 as eu1;
        let mut k = Kernel::default();
        let (mut b, top, tilted) = named_case(&mut k);
        let input =
            validation::standing(&mut k, &mut b.clone_standing_for_self().unwrap()).unwrap();
        with_notifications(&mut b, |b, notes| {
            local_operation(&mut k, b, &[top], vec![tilted], notes)
        })
        .unwrap();
        // the sound body: the closed-form volume 25/6, a manifold solid in two regions
        let (volume, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold, "{volume}");
        assert!((volume - 25.0 / 6.0).abs() <= 1e-6, "{volume}");
        assert_eq!(bp1::regions(&b, None).unwrap().len(), 2);
        // the check reports nothing (the #82 tolerance findings and the input's #61
        // among what the repair removed) and adds none the input did not have
        let found =
            validation::standing(&mut k, &mut b.clone_standing_for_self().unwrap()).unwrap();
        let lines = found.check.lines();
        assert!(lines.is_empty(), "{lines:?}");
        let added = found.check.failures_beyond(&input.check);
        assert!(added.is_empty(), "{added:?}");
        // the intended corner points A = (0, 1, 0) and B = (1, 0, 0), exactly
        let a = vertex_at(&b, Point3::new(0.0, 1.0, 0.0)).expect("A = (0, 1, 0)");
        let by = vertex_at(&b, Point3::new(1.0, 0.0, 0.0)).expect("B = (1, 0, 0)");
        assert_ne!(a, by);
        // the (0, 0) verticals and their corner vertices are gone (the other uses of
        // the corner's edges intact: the four boundary edges end at the clip points)
        let verticals = b
            .edges()
            .iter()
            .map(|(e, _)| e)
            .filter(|&e| {
                let c = edge::curve(&b, e).unwrap().unwrap();
                let i = edge::interval(&b, e).unwrap();
                [i.min(), i.max()]
                    .into_iter()
                    .map(|t| c.evaluate_point(&k, t).unwrap())
                    .all(|p| p.x.abs() < 1e-6 && p.y.abs() < 1e-6)
            })
            .count();
        assert_eq!(verticals, 0, "the (0, 0) verticals survived");
        for gone in [Point3::ZERO, Point3::new(0.0, 0.0, -1.0)] {
            assert!(vertex_at(&b, gone).is_none(), "{gone:?}");
        }
        for (p, q) in [
            (Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 2.0, 0.0)),
            (Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)),
            (Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 2.0, 1.0)),
            (Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)),
        ] {
            assert!(edge_between(&k, &b, p, q).is_some(), "{p:?}..{q:?}");
        }
        // the (2, 2) vertical is extended to z = 3 (the moved top raised that corner)
        assert!(edge_between(
            &k,
            &b,
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(2.0, 2.0, 3.0)
        )
        .is_some());
        // the bridge: one four-use manifold edge between A and B, a mated pair in each
        // face's two loop-use chains
        let bridge = edge_between(
            &k,
            &b,
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        )
        .expect("the bridge edge A..B");
        assert_eq!(edge::edgeuse_count(&b, bridge).unwrap(), 4);
        assert!(edge::is_manifold(&b, bridge).unwrap());
        assert!(!eu1::edge_is_lamina(&b, bridge).unwrap());
        for f in edge::faces(&b, bridge, None).unwrap() {
            let on_face = edge::edgeuses(&b, bridge, None, None)
                .unwrap()
                .into_iter()
                .filter(|&eu| eu1::face(&b, eu).unwrap() == Some(f))
                .count();
            assert_eq!(on_face, 2, "F{}: {on_face} uses of the bridge", f.index());
        }
        // the body a public operation leaves: its counts and its loop walks closed
        assert_eq!(
            (
                b.faces().len(),
                b.edges().len(),
                b.edgeuses().len(),
                b.vertices().len(),
                b.vertexuses().len()
            ),
            (6, 12, 48, 8, 48)
        );
        for (lu, _) in b.loopuses().iter() {
            let (closed, shared, _) = walk_chain(&b, lu).unwrap();
            assert!(closed && shared, "LU{}", lu.index());
        }
    }

    /// A moved face that crosses no edge is a no-op for the reconstruction (issue
    /// #13622): the pass records zero events and touches nothing — the body is exactly
    /// what `moves_the_top_of_a_box_and_keeps_it_a_solid` and
    /// `a_raised_top_keeps_the_bottom_edges_as_they_were` measure without it.
    #[test]
    fn a_raised_top_that_crosses_no_edge_records_no_reconstruction_events() {
        let mut k = Kernel::default();
        let (mut b, top, _tilted) = named_case(&mut k);
        let raised = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, 3.0), Vec3::Z));
        let reconstruction = with_notifications(&mut b, |b, notes| {
            local_operation_recorded(&mut k, b, &[top], vec![raised], notes)
        })
        .unwrap();
        assert!(
            reconstruction.events().is_empty(),
            "{:?}",
            reconstruction.events()
        );
        assert!(reconstruction.bridges().is_empty());
        assert!(reconstruction.tolerances().is_empty());
        assert!(reconstruction.proofs().is_empty());
        // the correspondence was captured all the same (the marks flowed)
        assert_eq!(reconstruction.map().len(), 12);
    }

    /// The correspondence map of one reconstruction (issue #13622): each original
    /// edge's mark reaches the result edges rebuilt from it (one original to many for
    /// the lamina twins), the marks are private to the operation and none outlives a
    /// success, a refusal rolls them back with the body, the caller's notification
    /// callbacks stay installed throughout (the bridge's `AddToBrep` reaches them), and
    /// the map still names the same result edges after the welds.
    #[test]
    fn the_reconstruction_keeps_the_original_edge_correspondence() {
        use crate::geom::object::NotifyOperation;
        use crate::topology::edge;
        let mut k = Kernel::default();
        let events = edge::notification_test_support::install(&mut k.context);
        let (mut b, top, tilted) = named_case(&mut k);
        let originals: Vec<(crate::topology::EdgeId, Point3, Point3)> = b
            .edges()
            .iter()
            .map(|(e, _)| {
                let c = edge::curve(&b, e).unwrap().unwrap().clone();
                let i = edge::interval(&b, e).unwrap();
                (
                    e,
                    c.evaluate_point(&k, i.min()).unwrap(),
                    c.evaluate_point(&k, i.max()).unwrap(),
                )
            })
            .collect();
        assert_eq!(originals.len(), 12);
        let reconstruction = with_notifications(&mut b, |b, notes| {
            local_operation_recorded(&mut k, b, &[top], vec![tilted], notes)
        })
        .unwrap();
        let map = reconstruction.map();
        assert_eq!(map.len(), 12);
        assert_eq!(
            map.keys().copied().collect::<Vec<_>>(),
            originals.iter().map(|o| o.0).collect::<Vec<_>>()
        );
        // every result edge carried exactly one original's mark at capture: 14 result
        // edges for 12 originals (two originals map to their lamina twins)
        let marked: usize = map.values().map(Vec::len).sum();
        assert_eq!(marked, 14, "{map:?}");
        let find_original = |pred: &dyn Fn(Point3, Point3) -> bool| {
            originals.iter().find(|(_, p, q)| pred(*p, *q)).unwrap().0
        };
        // the severed corner: the vertical (0, 0) original maps to its two lamina
        // twins (one original, two result edges) and to nothing alive after
        let vertical = find_original(&|p, q| {
            [p, q]
                .into_iter()
                .all(|r| r.x.abs() < 1e-6 && r.y.abs() < 1e-6)
        });
        assert_eq!(map[&vertical].len(), 2, "{:?}", map[&vertical]);
        assert!(
            map[&vertical]
                .iter()
                .all(|e| !b.edges().iter().any(|(x, _)| x == *e)),
            "{:?}",
            map[&vertical]
        );
        // the top y = 0 twins map together and resolve to the one survivor
        let top_y0 = find_original(&|p, q| {
            [p, q]
                .into_iter()
                .all(|r| r.y.abs() < 1e-6 && (r.z - 2.0).abs() < 1e-6)
        });
        assert_eq!(map[&top_y0].len(), 2, "{:?}", map[&top_y0]);
        let alive: Vec<_> = map[&top_y0]
            .iter()
            .filter(|e| b.edges().iter().any(|(x, _)| x == **e))
            .copied()
            .collect();
        assert_eq!(alive.len(), 1, "{:?}", map[&top_y0]);
        // the marks are private to the operation: none outlives a success
        for (e, _) in b.edges().iter() {
            assert!(b
                .entity_attributes(EntityRef::Edge(e))
                .iter()
                .all(|(_, a)| a.attribute_id() != CORRESPONDENCE_MARK));
        }
        for (f, _) in b.faces().iter() {
            assert!(b
                .entity_attributes(EntityRef::Face(f))
                .iter()
                .all(|(_, a)| a.attribute_id() != MOVED_FACE_MARK));
        }
        // the callback the caller installed saw the pass's own notifications: the
        // bridge's `AddToBrep` proves the operation never replaced it
        let captured = events.lock().unwrap().clone();
        assert!(
            captured
                .iter()
                .any(|ev| ev.operation == NotifyOperation::AddToBrep),
            "{:?}",
            captured.iter().map(|ev| ev.operation).collect::<Vec<_>>()
        );
        // the map survives the welds: the same result edges for the same originals
        p5::stitch_and_orient(&mut k, &mut b, &p5::StitchAndOrientOptions::default()).unwrap();
        b.refresh_cached_topology();
        crate::heal::stitch::p2::stitch_into_solid(&mut k, &mut b).unwrap();
        b.refresh_cached_topology();
        for (original, result_edges) in map {
            for e in result_edges {
                if alive.contains(e) {
                    assert!(
                        b.edges().iter().any(|(x, _)| x == *e),
                        "original E{}: E{} died in the weld",
                        original.index(),
                        e.index()
                    );
                }
            }
        }
        assert_eq!(edge::edgeuse_count(&b, alive[0]).unwrap(), 4);
    }

    /// A refusal rolls the correspondence marks back with the body (issue #13622): the
    /// operation's private marks outlive nothing, not even a failed run.
    #[test]
    fn a_refusal_rolls_the_correspondence_marks_back_with_the_body() {
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
        assert!(
            matches!(why, Some(OffsetFailure::Unsound { .. })),
            "{why:?}"
        );
        for (e, _) in b.edges().iter() {
            assert!(b
                .entity_attributes(EntityRef::Edge(e))
                .iter()
                .all(|(_, a)| a.attribute_id() != CORRESPONDENCE_MARK));
        }
        for (f, _) in b.faces().iter() {
            assert!(b
                .entity_attributes(EntityRef::Face(f))
                .iter()
                .all(|(_, a)| a.attribute_id() != MOVED_FACE_MARK));
        }
    }

    /// The reconstruction's tolerance pass (issue #13622; `docs/validity-and-tolerances.md`
    /// §5.2's formula, §7.2 `Fix_TolSizes` steps 4-5, §8 and invariant 9): the two
    /// corner vertices shrink to what their measured gaps need (1.00001 -> 1e-5, the
    /// #82 findings gone), no edge tolerance changes where the whole-edge gaps do not
    /// justify it, and a measured gap past the construction bound refuses
    /// `GapBeyondConstruction`: the tolerance is never grown to cover it.
    #[test]
    fn the_reconstruction_tightens_the_corner_tolerances_and_never_grows_one() {
        use crate::topology::edge;
        let mut k = Kernel::default();
        let (mut b, top, tilted) = named_case(&mut k);
        let reconstruction = with_notifications(&mut b, |b, notes| {
            local_operation_recorded(&mut k, b, &[top], vec![tilted], notes)
        })
        .unwrap();
        // the rule shrinks what its gaps do not need and never grows
        let tolerances = reconstruction.tolerances();
        assert!(
            tolerances.iter().all(|t| t.after < t.before),
            "{tolerances:?}"
        );
        // only the two corner vertices move (1.00001 -> 1e-5): the edge tolerances are
        // left unchanged, their whole-edge bounds do not justify changing them
        assert!(
            tolerances.iter().all(|t| t.entity.starts_with('V')),
            "{tolerances:?}"
        );
        assert_eq!(tolerances.len(), 2, "{tolerances:?}");
        for t in tolerances {
            assert!(
                (t.before - 1.00001).abs() < 1.0e-5,
                "{} {}",
                t.entity,
                t.before
            );
            assert!(
                (t.after - 1.0e-5).abs() < 1.0e-12,
                "{} {}",
                t.entity,
                t.after
            );
        }
        // the corner edges keep the zone tolerance (re-measured: unchanged)
        for v in [
            vertex_at(&b, Point3::new(0.0, 1.0, 0.0)).unwrap(),
            vertex_at(&b, Point3::new(1.0, 0.0, 0.0)).unwrap(),
        ] {
            for e in crate::topology::vertex::edges(&b, v, None).unwrap() {
                assert_eq!(
                    edge::tolerance(&b, e).unwrap(),
                    b.tolerance(),
                    "E{}",
                    e.index()
                );
            }
            assert_eq!(crate::topology::vertex::tolerance(&b, v).unwrap(), 1e-5);
        }
        // the #82 tolerance findings are gone
        let found =
            validation::standing(&mut k, &mut b.clone_standing_for_self().unwrap()).unwrap();
        assert!(
            found.check.lines().iter().all(|l| !l.contains("#82")),
            "{:?}",
            found.check.lines()
        );
        // a gap past the construction bound (the body's zone tolerance: below it an
        // exact construction leaves no gap) refuses; the tolerance is never grown to    // cover it
        let a = vertex_at(&b, Point3::new(0.0, 1.0, 0.0)).unwrap();
        b.vertex_mut(a).unwrap().point = Point3::new(0.0, 1.0, 0.5);
        let edges_of_a = crate::topology::vertex::edges(&b, a, None).unwrap();
        let r = tighten_reconstruction_tolerances(&mut k, &mut b, &edges_of_a, &[a]);
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::GapBeyondConstruction { .. }
                    }
                ))
            ),
            "the gap past the construction bound was not refused"
        );
    }

    /// Stage-2 of the reconstruction (issue #13622): a bridge the pass inserted that is
    /// gone refuses the completed boundary typed (`BoundaryFault::BridgeMissing`) — the
    /// boundary is never returned half closed.
    #[test]
    fn the_completed_boundary_check_refuses_a_bridge_that_is_gone() {
        use crate::error::offset::BoundaryFault;
        let mut k = Kernel::default();
        let (mut b, top, tilted) = named_case(&mut k);
        let reconstruction = with_notifications(&mut b, |b, notes| {
            local_operation_recorded(&mut k, b, &[top], vec![tilted], notes)
        })
        .unwrap();
        assert_eq!(reconstruction.bridges().len(), 1);
        let bridge = reconstruction.bridges()[0];
        let faces = bp1::faces(&b, None).unwrap();
        // the recorded bridge passes the check
        validate_completed_boundary(&mut k, &mut b, &[], &[bridge], &faces).unwrap();
        // the same check with the bridge gone (a record for an edge the body does not
        // have): a typed refusal, never a half-closed boundary
        let gone = crate::topology::EdgeId::from_index(u32::MAX);
        let r = validate_completed_boundary(&mut k, &mut b, &[], &[gone], &faces);
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::BoundaryInvalid {
                            fault: BoundaryFault::BridgeMissing { .. }
                        }
                    }
                ))
            ),
            "a gone bridge was not refused"
        );
    }

    /// An uncertifiable clip refuses the reconstruction typed
    /// (`ReconstructionRefusal::ClipUncertified`): a curve whose kind has no exact clip
    /// on the plane is never guessed at (issue #13622).
    #[test]
    fn an_uncertifiable_clip_refuses_the_reconstruction() {
        use crate::geom::extent1d::Extent1d;
        use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let k = Kernel::default();
        let surface = Surface::from(Plane::from_normal(
            Point3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
        ));
        let face = cut_face(
            FaceId::from_index(0),
            surface,
            Extent2d::from_bounds(-2.0, -2.0, 2.0, 2.0),
        );
        // a bent (B-spline) curve touching the face's surface: no exact clip on a plane
        let bent = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &[
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(0.0, 1.5, 0.0),
                    Point3::new(0.0, 2.0, 2.0),
                ],
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        );
        let r = classify_edge_crossing(
            &k,
            &bent,
            &Extent1d::from_min_max(0.0, 1.0),
            &face,
            2.0e-5,
            crate::topology::EdgeId::from_index(0),
        );
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::ClipUncertified { .. }
                    }
                ))
            ),
            "the uncertifiable clip was not refused"
        );
    }

    /// A moved face whose surface is not planar refuses the cut typed
    /// (`ReconstructionRefusal::SurfaceUncertified`): the cut of the body, the clip    /// point and the endpoint contact are certified on a plane only (issue #13622, the
    /// endpoint certificate's documented scope).
    #[test]
    fn a_non_planar_moved_face_refuses_the_cut_as_uncertified() {
        use crate::geom::extent1d::Extent1d;
        let mut k = Kernel::default();
        let body =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0)
                .unwrap();
        let side = curved_face(&body);
        let surface = face_ops::surface(&body, side).unwrap().unwrap().clone();
        let face = cut_face(side, surface, Extent2d::from_bounds(-1.0, -0.1, 1.0, 2.1));
        assert!(face.plane.is_none());
        // a line through the face's region: not proved clear, and no plane to cut on
        let through = Curve::from(crate::curve::line::Line::from_points(
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(2.0, 0.0, 1.0),
            3,
        ));
        let r = classify_edge_crossing(
            &k,
            &through,
            &Extent1d::from_min_max(0.0, 1.0),
            &face,
            2.0e-5,
            crate::topology::EdgeId::from_index(0),
        );
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::SurfaceUncertified { .. }
                    }
                ))
            ),
            "the non-planar moved face was not refused"
        );
    }

    /// The kept-edge gate certifies the rebuilt group per result edge (issue #13622's
    /// review, S1): lamina twins map one original to several result edges, and every
    /// SURVIVING rebuilt result edge must carry its own `ContactProof::Proved` — a
    /// group where one twin has a proof and another rebuilt twin does not refuses typed
    /// (`ClipUncertified`), where one proof for the whole group used to pass. A severed
    /// edge needs no proof (it is not in the result).
    #[test]
    fn a_twin_group_with_one_proof_missing_refuses_the_gate() {
        use crate::error::offset::ReconstructionRefusal;
        use crate::offset::offset_executive::clearance::endpoint_contact::{
            ContactProof, ContactReport,
        };
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
        let mut ids = b.edges().iter().map(|(e, _)| e).collect::<Vec<_>>();
        ids.sort_unstable();
        let (twin_a, twin_b) = (ids[0], ids[1]);
        // the group of one original: the two live twins and a severed result edge
        let original = crate::topology::EdgeId::from_index(u32::MAX);
        let severed = crate::topology::EdgeId::from_index(u32::MAX - 1);
        let proved = || {
            ContactProof::Proved(ContactReport {
                incidence: 0.0,
                isolation_boxes: 0,
                remainder_boxes: 0,
                region_boxes: 0,
                root_t: None,
            })
        };
        let mut map = BTreeMap::new();
        map.insert(original, vec![twin_a, twin_b, severed]);
        let mut reconstructed_kept = BTreeSet::new();
        reconstructed_kept.insert(original);
        // one twin's proof alone: the other rebuilt twin carries none, refused typed
        let reconstruction = Reconstruction {
            events: Vec::new(),
            proofs: vec![(twin_a, proved())],
            map: map.clone(),
            reconstructed_kept: reconstructed_kept.clone(),
            bridges: Vec::new(),
            tolerances: Vec::new(),
            discharged_vanishes: BTreeSet::new(),
        };
        let r = refuse_kept_edges_not_clear(&mut k, &mut b, &[], &reconstruction);
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::ClipUncertified { .. }
                    }
                ))
            ),
            "a rebuilt twin without its own proof was not refused"
        );
        // each surviving twin its own proof: accepted (the severed edge needs none)
        let reconstruction = Reconstruction {
            events: Vec::new(),
            proofs: vec![(twin_a, proved()), (twin_b, proved())],
            map,
            reconstructed_kept,
            bridges: Vec::new(),
            tolerances: Vec::new(),
            discharged_vanishes: BTreeSet::new(),
        };
        refuse_kept_edges_not_clear(&mut k, &mut b, &[], &reconstruction).unwrap();
    }

    /// The "bounds the face" class (issue #13622's review, S2): an edge whose sampled
    /// tolerance band lies in the moved face's surface is never classified `Clear` on
    /// the samples alone. The certified strict predicate must confirm it
    /// (`clearance::separated` over the trimmed region); where that certificate fails
    /// and no endpoint contact certifies, the event logic refuses the uncertifiable
    /// clip typed (`ClipUncertified`) — where the sampled band used to return `Clear`.
    #[test]
    fn an_edge_that_bounds_the_moved_face_is_not_clear_on_the_sampled_band() {
        use crate::error::offset::ReconstructionRefusal;
        use crate::geom::extent1d::Extent1d;
        let k = Kernel::default();
        let surface = Surface::from(Plane::from_normal(
            Point3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
        ));
        let face = cut_face(
            FaceId::from_index(0),
            surface,
            Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        );
        // the line z = x + y - 1 from (0, 1, 0) to (2, 0, 1): its whole curve lies in
        // the face's plane (the sampled band is 0) within the face's trimmed region
        let in_plane = Curve::from(crate::curve::line::Line::from_points(
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 1.0),
            3,
        ));
        let r = classify_edge_crossing(
            &k,
            &in_plane,
            &Extent1d::from_min_max(0.0, 1.0),
            &face,
            2.0e-5,
            crate::topology::EdgeId::from_index(0),
        );
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::ClipUncertified { .. }
                    }
                ))
            ),
            "the sampled band was accepted without a certificate"
        );
    }

    /// The (0,0) vertical of the named case's box (both ends on x = y = 0).
    fn corner_vertical(k: &Kernel, b: &Brep) -> crate::topology::EdgeId {
        use crate::topology::edge;
        b.edges()
            .iter()
            .map(|(e, _)| e)
            .find(|&e| {
                let c = edge::curve(b, e).unwrap().unwrap();
                let i = edge::interval(b, e).unwrap();
                [i.min(), i.max()]
                    .into_iter()
                    .map(|t| c.evaluate_point(k, t).unwrap())
                    .all(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9)
            })
            .expect("the (0,0) vertical")
    }

    /// The named case's deferred vanish is the reconstruction's event, not a failure
    /// (issue #13622's hook): the gap pass records one obligation per original edge (the
    /// (0,0) vertical), the reconstruction certifies its REMOVE and severs every lamina
    /// twin — the obligations are exhausted.
    #[test]
    fn the_named_case_defers_the_vanish_as_an_obligation_and_the_reconstruction_discharges_it() {
        let mut k = Kernel::default();
        let (mut b, top, tilted) = named_case(&mut k);
        let vertical = corner_vertical(&k, &b);
        let reconstruction = with_notifications(&mut b, |b, notes| {
            local_operation_recorded(&mut k, b, &[top], vec![tilted], notes)
        })
        .unwrap();
        // the pending vanish is recorded (one obligation per original edge) and the
        // obligations are exhausted
        assert_eq!(
            reconstruction
                .discharged_vanishes()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![vertical],
            "{:?}",
            reconstruction.discharged_vanishes()
        );
        // its REMOVE is certified
        assert!(
            reconstruction.events().iter().any(|r| {
                r.original == vertical && matches!(r.event, CrossEvent::Removed { .. })
            }),
            "{:?}",
            reconstruction.events()
        );
        // all twins disappear
        let twins = &reconstruction.map()[&vertical];
        assert_eq!(twins.len(), 2, "{twins:?}");
        assert!(
            twins
                .iter()
                .all(|e| !b.edges().iter().any(|(x, _)| x == *e)),
            "{twins:?}"
        );
    }

    /// A deferred gap alongside a repairable one on the same face (issue #13622's hook):
    /// a 2 x 2 x 0.5 box with its top moved to the named case's plane z = x + y - 1 —
    /// the (0,0) vertical vanishes (its gap deferred) while the (0,2) corner rises to
    /// z = 1 and its gap closes by the extension, both on the face x = 0's offset loop.
    /// Processing completes without exhausting the gap pass's guard (`MAX_GAP_PASSES`,
    /// which refuses `SM_ERR`) and without skipping the repair: the deferred gap
    /// resolves as the recorded obligation, the extension is in the result (the (0,2)
    /// vertical runs 0 to 1), and the body is the closed-form volume 25/6, the part of
    /// the box's planes below the plane.
    #[test]
    fn a_deferred_gap_alongside_a_repairable_one_on_one_face_completes_without_the_gap_guard() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 0.5).unwrap();
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
                (p.z - 0.5).abs() < 1e-9
            })
            .expect("the top");
        let vertical = corner_vertical(&k, &b);
        let tilted = Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
        ));
        let reconstruction = with_notifications(&mut b, |b, notes| {
            local_operation_recorded(&mut k, b, &[top], vec![tilted], notes)
        })
        .unwrap();
        // the deferred gap resolved as the recorded obligation; the run completed — the
        // gap pass's MAX_GAP_PASSES guard refuses `SM_ERR`, so a scan that restarted on
        // the deferral or exhausted its passes could not have finished
        assert_eq!(
            reconstruction
                .discharged_vanishes()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![vertical]
        );
        // the repairable gap on the same face was not skipped: the (0,2) vertical is
        // extended to where the moved top meets its line, z = 1
        assert!(edge_between(
            &k,
            &b,
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 1.0)
        )
        .is_some());
        // the closed-form volume 25/6 (the part of the box's planes below the plane)
        let (volume, manifold) = volume_and_manifold(&k, &mut b);
        assert!(manifold, "{volume}");
        assert!((volume - 25.0 / 6.0).abs() <= 1e-6, "{volume}");
    }

    /// A pending vanish with a non-planar moved surface refuses typed, even where no
    /// other reconstruction event fires (issue #13622's hook's fail-closed rule and the
    /// no-engagement path): the cut the endpoint certificate's scope cannot decide
    /// refuses its own typed refusal (`SurfaceUncertified`) — a deferred obligation is
    /// never suppressible. (The uncertifiable-clip flavour of the same rule is
    /// `wr_566_local_operation_closes_a_wall`'s
    /// `wr_566_without_the_opt_in_the_closed_wall_is_refused`: its deferred wall edges'
    /// clips do not certify, and the gap pass's own `EdgeVanishes` stands again.)
    #[test]
    fn a_pending_vanish_with_a_non_planar_moved_face_refuses_typed_without_other_events() {
        use crate::geom::attribute::{Attribute, Behavior};
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0)
                .unwrap();
        let side = curved_face(&b);
        b.add_attribute(
            EntityRef::Face(side),
            Attribute::long(MOVED_FACE_MARK, 1, Behavior::Copy),
        )
        .unwrap();
        // a boundary edge of the curved face: the obligation's original and its result
        let e = face4::edges(&b, side, None, false, None).unwrap()[0];
        let map = BTreeMap::from([(e, vec![e])]);
        let crossable = BTreeSet::from([e]);
        let vanished = BTreeSet::from([e]);
        let r = with_notifications(&mut b, |b, notes| {
            reconstruct_crossed_boundaries(&mut k, b, &map, &[], &crossable, &vanished, notes)
        });
        assert!(
            matches!(
                r.unwrap_err().failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(
                    OffsetFailure::Reconstruction {
                        why: ReconstructionRefusal::SurfaceUncertified { .. }
                    }
                ))
            ),
            "the undecided cut was suppressed with an obligation pending"
        );
    }

    /// A deferred obligation the reconstruction does not take cannot discharge (issue
    /// #13622's hook): a `Clear` or `Trimmed` classification, a missing correspondence or
    /// an ambiguous one leaves ownership unestablished, and the gap pass's own
    /// `EdgeVanishes` refusal stands again — typed, never a silent success.
    #[test]
    fn a_pending_vanish_the_reconstruction_does_not_take_is_refused_again() {
        use crate::geom::attribute::{Attribute, Behavior};
        let tilted = |k: &mut Kernel| -> (Brep, FaceId) {
            let mut b = create_box(k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
            let top = bp1::faces(&b, None)
                .unwrap()
                .into_iter()
                .find(|&f| {
                    let s = face_ops::surface(&b, f).unwrap().unwrap();
                    let p = s
                        .evaluate_point(
                            k,
                            s.natural_uv_domain_with_nurb(k).unwrap().evaluate(0.5, 0.5),
                        )
                        .unwrap();
                    (p.z - 2.0).abs() < 1e-9
                })
                .expect("the top");
            b.face_mut(top).unwrap().surface = Some(Surface::from(Plane::from_normal(
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(-1.0, -1.0, 1.0),
            )));
            b.add_attribute(
                EntityRef::Face(top),
                Attribute::long(MOVED_FACE_MARK, 1, Behavior::Copy),
            )
            .unwrap();
            (b, top)
        };
        // (i) the (2,2) vertical is clear of the tilted top's cut, (ii) the bottom y = 0
        // edge is trimmed by it at (1, 0, 0), (iii) an obligation with no correspondence
        // entry, (iv) an obligation whose result edge another original also claims
        let mut k = Kernel::default();
        let (mut b, _top) = tilted(&mut k);
        let clear = edge_between(
            &k,
            &b,
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(2.0, 2.0, 2.0),
        )
        .expect("the (2,2) vertical");
        let trimmed = edge_between(&k, &b, Point3::ZERO, Point3::new(2.0, 0.0, 0.0))
            .expect("the bottom y = 0 edge");
        let cases = [
            (
                "clear",
                BTreeMap::from([(clear, vec![clear])]),
                BTreeSet::from([clear]),
            ),
            (
                "trimmed",
                BTreeMap::from([(trimmed, vec![trimmed])]),
                BTreeSet::from([trimmed]),
            ),
            ("missing", BTreeMap::new(), BTreeSet::from([clear])),
            (
                "ambiguous",
                BTreeMap::from([(clear, vec![clear]), (trimmed, vec![clear])]),
                BTreeSet::from([clear]),
            ),
        ];
        for (name, map, vanished) in cases {
            let crossable = vanished.clone();
            let r = with_notifications(&mut b, |b, notes| {
                reconstruct_crossed_boundaries(&mut k, b, &map, &[], &crossable, &vanished, notes)
            });
            let e = r.expect_err(name);
            assert!(
                matches!(
                    e.failure().map(|f| &f.reason),
                    Some(crate::error::Reason::Offset(OffsetFailure::EdgeVanishes {
                        edge: Some(_),
                        face: None
                    }))
                ),
                "{name}: {e}"
            );
        }
    }

    /// A refused run restores body, attributes and notification records (issue #13622's
    /// hook): a second correspondence mark on the (0,0) vertical leaves the
    /// correspondence ambiguous, so the deferred vanish's ownership is never established
    /// and the run refuses the gap pass's own `EdgeVanishes` — with the input body, its
    /// attributes and the caller's notification records and attributes exactly as they
    /// were (the obligations roll back with them).
    #[test]
    fn a_refused_run_with_obligations_pending_restores_the_body_attributes_and_stores() {
        use crate::geom::attribute::{Attribute, Behavior};
        let mut k = Kernel::default();
        let (mut b, top, tilted) = named_case(&mut k);
        let vertical = corner_vertical(&k, &b);
        // a second correspondence mark on the vertical: its result edges carry two
        // originals' marks at capture, so the correspondence is ambiguous
        b.add_attribute(
            EntityRef::Edge(vertical),
            Attribute::long(CORRESPONDENCE_MARK, 999, Behavior::Copy),
        )
        .unwrap();
        let before = format!("{b:?}");
        let (r, stores) = with_notifications(&mut b, |b, notes| {
            let stores = (format!("{:?}", notes.records), notes.attributes.clone());
            let r = local_operation(&mut k, b, &[top], vec![tilted], notes);
            let after = (format!("{:?}", notes.records), notes.attributes.clone());
            (r, (stores, after))
        });
        let e = r.expect_err("the ambiguous correspondence discharges nothing");
        assert!(
            matches!(
                e.failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(OffsetFailure::EdgeVanishes {
                    edge: Some(_),
                    face: None
                }))
            ),
            "{e}"
        );
        assert!(format!("{b:?}") == before, "the input changed");
        assert!(stores.0 .0 == stores.1 .0, "the records changed");
        assert!(stores.0 .1 == stores.1 .1, "the attributes changed");
    }
}
