//! The cliff overflow trimmed at its creases on the live session (not upstream): the
//! fork's `SmFilletExecutive::TrimAtCreases` (`source/SMLib/src/SmFilletOverflow.cpp`,
//! tangency-labs/smlib#79 step 6 at 5e8c9c1: 99e57b8, 668fe9f, bacd0bb, 2d4b154, bc8c44d,
//! de5df14, 8832ead; `fixes.fillet_overflow_trim_at_creases`, WR-695). The geometry steps
//! are in [`crate::blend::fillet_overflow`].
//!
//! | Rust                                | Fork                                 |
//! |-------------------------------------|--------------------------------------|
//! | `FilletSession::trim_at_creases`    | `SmFilletExecutive::TrimAtCreases`   |
//! | `is_tangent_edge` (private)         | `SmFilletSolver::IsTangentEdge` (bacd0bb) |
//!
//! Deviations:
//! - **The sheet leaves the session.** As in `subtract_removal_region`, the sheet (the
//!   intermediate body) is taken out of the session first and edited on its own; the
//!   target is edited in place through the session's notifications, as the fork edits
//!   it, and put back as it was when the trim fails (the fork leaves whatever it reached).
//! - **Sets.** The fork's pointer-keyed `std::set` / `std::map` are `BTreeSet` /
//!   `BTreeMap` keyed by ids, for membership; every loop follows a vector in the bodies'
//!   own orders. The faces taken off are deleted in the order they were reached (the
//!   fork's in address order).
//! - **Fit sizes.** The three refusals step 5 sizes (`NoteFitOnFace` after the tangent
//!   edge and the extension, `NoteFitRadius` from `sm_CornerTakesPerRadius` after the
//!   whole edge; tangency-labs/smlib a3d6d81, 49f393d) note them with
//!   `fixes.fillet_gn_section_generator` (WR-696, #2278).
//! - **Section ends (not the fork's).** A round with WR-674's section ends is trimmed here
//!   only with `fixes.fillet_section_end_trim_at_creases` (WR-706, #2223): each such end is
//!   cut by its section's plane and closed by the section's flat face, and a side of that
//!   face that lies along an edge of the body is sewn to the edge's piece the rail's
//!   imprint cuts off, not imprinted. Without it the caller subtracts the round, as before.

use std::collections::{BTreeMap, BTreeSet};

use super::{invalid, FilletBody, FilletObject, FilletSession, SessionObjects};
use crate::blend::fillet_cliff_overflow::{
    cut_fillet_at_open_end, section_edge, section_end, section_end_face, Refusal, SectionEnd,
};
use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::{FilletErrorType, FilletGeomId};
use crate::blend::fillet_overflow::{
    creases, edge_curve, edge_on_surface, extend, in_material, make_extension_face, pieces_in_face,
    ExtendOutcome, Extension, Rail,
};
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::ids;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, BIG_DOUBLE};
use crate::query::brep::BrepIdentity;
use crate::surface::plane::Plane;
use crate::surface::surface::Surface;
use crate::topology::brep::p3::{self, delete_face, with_kernel_callbacks, DeleteFaceOptions};
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge::NotifyEnv;
use crate::topology::topology_traverser::Marks;
use crate::topology::{edge, edgeuse::p1 as eu1, face, vertex, Brep, BrepId, EdgeId, FaceId};

const OP: &str = "blend::fillet_executive::session::trim_at_creases";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `SmFilletSolver::IsTangentEdge` (the fork's bacd0bb): whether the two faces at the
/// manifold edge `e` meet tangentially there (`sm_TestManifoldTangentRollover`).
fn is_tangent_edge(k: &Kernel, b: &mut Brep, e: EdgeId, tangency_tol_rad: f64) -> Result<bool> {
    if !edge::is_manifold(b, e)? {
        return Ok(false);
    }
    Ok(
        crate::blend::fillet_solver::p1::test_manifold_tangent_rollover(k, b, e, tangency_tol_rad)?
            .0,
    )
}

/// The key of entity `e` of `b` in a notification (its issued identity, or the default
/// scheme's).
fn notify_key(b: &Brep, e: EntityRef) -> ObjectRef {
    match b.notify_serial() {
        Some(s) => p3::topology_ref(s, e),
        None => crate::topology::loopuse::notify_ref(e),
    }
}

/// `MergeCurvesOnSurface(*face->GetSurface(), tol, curves, NULL, rNewFaces, rEdges, FALSE,
/// intersection)` on `b`, notified through `env`: the new faces and the curves' edges.
fn merge_curves(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    f: FaceId,
    tol: f64,
    mut curves: Vec<Curve>,
    intersection: Option<Surface>,
) -> Result<(Vec<FaceId>, Vec<EdgeId>)> {
    let key_of = env.key_of;
    let curve_ref = |e: EdgeId, kind: i64| ObjectRef {
        key: key_of(EntityRef::Edge(e)).key,
        type_code: kind,
    };
    match intersection {
        Some(surface) => crate::topology::brep::p10::merge_curves_on_surface_through_surface(
            k,
            b,
            env,
            &curve_ref,
            f,
            tol,
            &mut curves,
            None,
            Some(surface),
        ),
        None => crate::topology::brep::p10::merge_curves_on_surface(
            k, b, env, &curve_ref, f, tol, &curves, None, false,
        ),
    }
}

/// `DeleteFace(f, TRUE)` on `b`, notified through `env`.
fn delete(k: &mut Kernel, b: &mut Brep, env: &mut NotifyEnv<'_>, f: FaceId) -> Result<()> {
    let mut marks = Marks::default();
    delete_face(
        k,
        b,
        env,
        &mut marks,
        f,
        DeleteFaceOptions {
            delete_edges_vertices: true,
            ..DeleteFaceOptions::default()
        },
    )
    .map(drop)
}

/// The drop of `point` onto `curve` over `interval`: `Some(parameter, distance)` when it
/// succeeds.
fn drop_on(
    k: &Kernel,
    curve: &Curve,
    interval: &crate::geom::extent1d::Extent1d,
    point: Point3,
) -> Result<Option<(f64, f64)>> {
    let d = curve.drop_point(
        k,
        interval,
        point,
        None,
        BIG_DOUBLE,
        None,
        SolverOperation::Minimize,
    )?;
    Ok(d.success.then_some((d.parameter, d.distance)))
}

/// A refusal with its fillet error and typed reason.
const fn refusal(
    code: FilletErrorType,
    reason: Option<NoRoomReason>,
    comment: &'static str,
) -> Refusal {
    Refusal {
        code,
        reason,
        comment,
    }
}

impl FilletSession<'_> {
    /// The fork's fit for a blend whose corners take the whole of its edge (a3d6d81): for a
    /// constant ball, the edge's length over what its corners take per unit radius at both
    /// ends (`sm_CornerTakesPerRadius`, over the solvers of the same radius), less the fit
    /// margin, noted where some corner takes any.
    fn note_fit_of_corners_taking_the_edge(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        solvers: &[FilletObject],
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self.objects.solver(id)?;
        let Some(eu) = current.base().edgeuse(0)? else {
            return Ok(());
        };
        let Some(e) = eu1::edge(self.objects.target, eu)? else {
            return Ok(());
        };
        let (rolls, on_edge) = super::ball_regular::rolls_a_ball(current);
        if !rolls || on_edge {
            return Ok(());
        }
        let offset = |o: &super::SolverObject| -> Result<f64> {
            Ok(o.base()
                .surface(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .surface
                .offset_distance())
        };
        let own = offset(current)?;
        let mut blended = BTreeSet::new();
        for &other in solvers {
            let FilletObject::Solver(oid) = other else {
                return Err(invalid("object is not a solver"));
            };
            let o = self.objects.solver(oid)?;
            let (r, law) = super::ball_regular::rolls_a_ball(o);
            if r && !law && (offset(o)? - own).abs() <= crate::math::EFF_ZERO * (1.0 + own.abs()) {
                if let Some(oeu) = o.base().edgeuse(0)? {
                    if let Some(oe) = eu1::edge(self.objects.target, oeu)? {
                        blended.insert(oe);
                    }
                }
            }
        }
        let target: &Brep = self.objects.target;
        let v0 = edge::vertex(target, e)?;
        let v1 = edge::other_vertex(target, e, v0)?;
        let mut per = [0.0_f64; 2];
        for (slot, v) in per.iter_mut().zip([v0, v1]) {
            if let Some(v) = v {
                *slot = super::fit_size::corner_takes_per_radius(k, target, v, e, &blended)?;
            }
        }
        let curve =
            edge::evaluation_curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let length = curve.length(k, &edge::interval(target, e)?, crate::math::EFF_ZERO)?;
        if per[0] + per[1] > 0.0 {
            let margin = 1000.0 * edge::tolerance(target, e)?;
            let fit = smos_max(0.0, length / (per[0] + per[1]) - margin);
            self.executive.note_fit_radius(
                fit,
                Some(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edge(e),
                )),
                crate::blend::fillet_executive::FitKind::Radius,
            );
        }
        Ok(())
    }

    /// Runs `edit` on the target through the session's notifications (its records and
    /// attribute store, under the target's identities), with the identity its queries
    /// resolve the target's records by, as the session's own stitch of the target does.
    fn with_target_env<R>(
        &mut self,
        k: &mut Kernel,
        edit: impl FnOnce(&mut Kernel, &mut Brep, &mut NotifyEnv<'_>, BrepIdentity<'_>) -> Result<R>,
    ) -> Result<R> {
        let serial = self.objects.target_serial;
        let key_of = |entity| p3::topology_ref(serial, entity);
        let keys = |object| match object {
            CacheObject::Brep => key_of(EntityRef::Brep(BrepId::from_index(0))),
            CacheObject::Other(owner) => owner,
            object => crate::topology::brep::p2::caller_cache_key(&key_of, object),
        };
        let owner_of = |_: ObjectRef| None;
        let identity = BrepIdentity {
            brep_id: BrepId::from_index(
                u32::try_from(serial)
                    .map_err(|_| invalid("target body identity exceeds query domain"))?,
            ),
            keys: &keys,
            owner_of: &owner_of,
        };
        let SessionObjects {
            target,
            notification_records,
            attributes,
            ..
        } = &mut self.objects;
        if k.context.issued_brep_serial(target.notify_identity_slot()) != Some(serial) {
            return Err(invalid(
                "target body belongs to another notification context",
            ));
        }
        crate::geom::context::Context::with_notification_context(k, |k, context| {
            p3::seed_records(notification_records, target, &key_of);
            let mut env = NotifyEnv {
                records: notification_records,
                attributes,
                context: Some(context),
                key_of: &key_of,
            };
            edit(k, target, &mut env, identity)
        })?
    }

    /// WR-706: whether the lamina edge `e` of `sheet` is a side of a section's flat face
    /// (`section_ends`) that runs from the end's vertex along an edge of the target at that
    /// vertex (its samples on the section's plane and its middle on the edge, within ten
    /// times `tol`): its two end points if so.
    fn seam_side(
        &self,
        k: &Kernel,
        sheet: &Brep,
        e: EdgeId,
        section_ends: &[SectionEnd],
        tol: f64,
    ) -> Result<Option<(Point3, Point3)>> {
        let on_tol = 10.0 * tol;
        let (sv, ev) = (
            edge::start_vertex(sheet, e)?.ok_or_else(|| SolidError::null_pointer(OP))?,
            edge::end_vertex(sheet, e)?.ok_or_else(|| SolidError::null_pointer(OP))?,
        );
        let (a, b) = (vertex::point(sheet, sv)?, vertex::point(sheet, ev)?);
        let curve = edge_curve(k, sheet, e)?;
        let ivl = curve.natural_interval();
        let mid = curve.evaluate_point(k, ivl.evaluate(0.5))?;
        let target = &*self.objects.target;
        for end in section_ends {
            if a.distance(end.at) > on_tol && b.distance(end.at) > on_tol {
                continue;
            }
            let on_plane = |p: Point3| (p - end.at).dot(end.normal).abs() <= on_tol;
            if !(on_plane(a) && on_plane(b) && on_plane(mid)) {
                continue;
            }
            for te in crate::topology::brep::p1::edges(target, None)? {
                if edge::start_vertex(target, te)? != Some(end.vertex)
                    && edge::end_vertex(target, te)? != Some(end.vertex)
                {
                    continue;
                }
                let t_curve =
                    edge::curve(target, te)?.ok_or_else(|| SolidError::null_pointer(OP))?;
                if let Some((_, dist)) = drop_on(k, t_curve, &edge::interval(target, te)?, mid)? {
                    if dist <= on_tol {
                        return Ok(Some((a, b)));
                    }
                }
            }
        }
        Ok(None)
    }

    /// The fork's `SmFilletExecutive::TrimAtCreases` (WR-695): in a cliff overflow, trim
    /// the blends along their creases with the faces beyond the cliffs and sew them into
    /// the target: the blends built on their faces' continuations, the sharp edges moved
    /// to where they meet the body.
    ///
    /// Each variable round on planes must hold its law; each open end is cut by its end
    /// face's plane ([`cut_fillet_at_open_end`]). Each sheet face is sectioned by every
    /// face of the target near it whose surface holds none of its boundary ([`creases`]); a
    /// crease is kept where it lies inside the face it is on and the sheet face
    /// ([`pieces_in_face`]), and the sheet face is split along all of them at once; a
    /// piece whose inner point lies inside the target is kept, the others go. A support
    /// face whose kept rail runs past its boundary into the material is carried on along
    /// its plane ([`extend`]). The kept pieces' lamina edges are imprinted on the target
    /// faces they lie on; the target faces reached from the blended edges without crossing
    /// an imprinted edge (or a piece of an edge an extension runs along) are deleted; the
    /// sheet is merged in, the extensions made, all stitched, the sewn edges held to the
    /// sheet's accuracy, the body oriented, and the edges between sewn faces on one surface
    /// removed.
    ///
    /// Refused (`NoRoom`): a law not held; a crease on a face that runs on tangentially
    /// from a face the sheet rolls on; a blend that keeps no face of its edge; a rail into
    /// the material that [`extend`] does not build. `BadMerge`: a piece on the body's
    /// boundary, nothing or everything taken off, a sheet that does not sew into a
    /// manifold solid. Each refusal is noted in the error info.
    ///
    /// # Errors
    /// The refusals as `SM_ERR`; those of the steps.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub(super) fn trim_at_creases(&mut self, k: &mut Kernel) -> Result<()> {
        if self.objects.intermediate.is_none() {
            return Err(SolidError::null_pointer(OP));
        }
        let solvers = self.executive.solvers.clone();
        for &solver in &solvers {
            if !self.holds_its_law(k, solver)? {
                self.note_refusal(refusal(
                    FilletErrorType::NoRoom,
                    Some(NoRoomReason::CliffLawNotHeld),
                    "Fillet Error: a variable round traced on its faces' continuations does not hold its radius law",
                ));
                return Err(SolidError::sm_err(OP));
            }
        }

        // What the trim reads of the solvers, before the sheet leaves the session.
        let mut faces_of: BTreeMap<FilletObject, Vec<(FilletGeomId, Option<FaceId>)>> =
            BTreeMap::new();
        let mut approx = Vec::new();
        let mut tangency = 0.0;
        let mut blended_edges: Vec<EdgeId> = Vec::new();
        for &solver in &solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let base = self.objects.solver(id)?.base();
            approx.push(base.approx_tol_3d);
            tangency = smos_max(tangency, base.tangency_tolerance());
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            blended_edges.push(
                eu1::edge(self.objects.target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?,
            );
            faces_of.insert(solver, self.fillet_faces(solver)?);
        }
        let open_ends = self.executive.open_ends.clone();
        // WR-706: a section end (WR-674) is trimmed here only with its own fix
        let section_trim = k.fixes.fillet_section_end_trim_at_creases
            && k.fixes.fillet_overflow_trim_at_creases
            && k.fixes.fillet_round_ends_at_tangent_continuation;
        if !section_trim && open_ends.iter().any(|end| end.section) {
            return Err(invalid("a section end is not trimmed at its creases"));
        }

        let mut sheet = self
            .objects
            .intermediate
            .take()
            .ok_or_else(|| SolidError::null_pointer(OP))?;

        // the ends the corners left uncut, cut off by their end faces' surfaces continued
        // (an end's event: the end face cuts the blend whatever lies past it, a lip over a
        // wall's top)
        let mut note = None;
        let mut section_ends: Vec<SectionEnd> = Vec::new();
        for end in &open_ends {
            let FilletObject::Solver(id) = end.solver else {
                return Err(invalid("open end solver is not a solver"));
            };
            let (
                FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v)),
                FilletObject::Topology(FilletBody::Target, EntityRef::Face(end_face)),
            ) = (end.vertex, end.end_face)
            else {
                return Err(invalid("open end is not target topology"));
            };
            let base = self.objects.solver(id)?.base();
            let solver_tol = base.approx_tol_3d;
            let target = &*self.objects.target;
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let fil_edge = eu1::edge(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let edge_mid = edge::curve(target, fil_edge)?
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .evaluate_point(k, edge::interval(target, fil_edge)?.evaluate(0.5))?;
            let vertex_point = vertex::point(target, v)?;
            let end_surface = if end.section {
                // WR-706: a section end (WR-674) is cut by the section through the vertex,
                // normal to the filleted edge
                let section =
                    section_end(k, target, fil_edge, v)?.ok_or_else(|| SolidError::sm_err(OP))?;
                section_ends.push(section);
                Surface::from(Plane::from_normal(vertex_point, section.normal))
            } else {
                face::p2::surface(target, end_face)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?
                    .clone_for_evaluation()?
            };
            let entry = faces_of
                .get_mut(&end.solver)
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let mut faces: Vec<Option<FaceId>> = entry.iter().map(|&(_, f)| f).collect();
            let cut = cut_fillet_at_open_end(
                k,
                &mut sheet,
                &mut faces,
                &end_surface,
                vertex_point,
                edge_mid,
                solver_tol,
                &mut note,
            );
            for (slot, f) in entry.iter_mut().zip(faces) {
                slot.1 = f;
            }
            self.noted(note.take(), cut)?;
        }

        // WR-706: each section end closed by its flat face, from the round's section to the
        // vertex (WR-674's, as the subtraction closes it)
        if !section_ends.is_empty() {
            let mut tol = 0.0;
            for &a in &approx {
                tol = smos_max(tol, a);
            }
            for end in &section_ends {
                let Some(section) = section_edge(k, &sheet, end, 10.0 * tol)? else {
                    self.objects.intermediate = Some(sheet);
                    self.note_refusal(refusal(
                        FilletErrorType::NoRoom,
                        Some(NoRoomReason::TangentContinuationEndUnsupported),
                        "Fillet Error: a round ending where its edge runs on tangentially has no single section there",
                    ));
                    return Err(SolidError::sm_err(OP));
                };
                section_end_face(k, &mut sheet, end, section, 10.0 * tol)?;
            }
        }

        // the target is trimmed in place; a failed trim puts it back as it was
        let kept = (*self.objects.target).clone();
        let trimmed = self.trim_sheet_into(
            k,
            sheet,
            &solvers,
            &faces_of,
            &blended_edges,
            &approx,
            tangency,
            &section_ends,
            &mut note,
        );
        if let Some(refused) = note.take() {
            self.note_refusal(refused);
        }
        if trimmed.is_err() {
            *self.objects.target = kept;
        }
        trimmed
    }

    /// The fork's `TrimAtCreases` from its tolerances on: `sheet` (the open ends cut)
    /// trimmed and sewn into the target. A refusal is written to `note`.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one fork routine
    fn trim_sheet_into(
        &mut self,
        k: &mut Kernel,
        mut sheet: Brep,
        solvers: &[FilletObject],
        faces_of: &BTreeMap<FilletObject, Vec<(FilletGeomId, Option<FaceId>)>>,
        blended_edges: &[EdgeId],
        approx: &[f64],
        tangency: f64,
        section_ends: &[SectionEnd],
        note: &mut Option<Refusal>,
    ) -> Result<()> {
        let refuse = |note: &mut Option<Refusal>, r: Refusal| -> Result<()> {
            *note = Some(r);
            Err(SolidError::sm_err(OP))
        };
        let mut tol = sheet.tolerance();
        for &a in approx {
            tol = smos_max(tol, a);
        }
        let on_tol = 10.0 * tol;

        // the blended edges' curves, to find their pieces after the imprint
        let mut blended_curves = Vec::new();
        for &e in blended_edges {
            blended_curves.push(edge_curve(k, self.objects.target, e)?);
        }

        let target_faces = crate::topology::brep::p1::faces(self.objects.target, None)?;
        // the faces the blends roll on: those of the blended edges (not the end faces they
        // end on)
        let mut rolled_on: BTreeSet<FaceId> = BTreeSet::new();
        for &e in blended_edges {
            rolled_on.extend(edge::faces(self.objects.target, e, None)?);
        }
        let mut target_boxes = Vec::new();
        for &f in &target_faces {
            target_boxes.push(crate::query::face::p1::calculate_bounding_box(
                k,
                &*self.objects.target,
                f,
                false,
            )?);
        }

        // each blended edge's own fillet faces (not its corners')
        let mut round_of: BTreeMap<FaceId, usize> = BTreeMap::new();
        for (i, solver) in solvers.iter().enumerate() {
            for &(_, f) in faces_of.get(solver).map_or(&[][..], Vec::as_slice) {
                if let Some(f) = f {
                    round_of.insert(f, i);
                }
            }
        }
        let mut kept_rounds: BTreeSet<usize> = BTreeSet::new();

        // 1, 2: each sheet face split along its creases, the pieces outside the body
        // (beyond the cliffs, or past an end) deleted, whether a crease split it or not
        let body_tol = self.objects.target.tolerance();
        for s in crate::topology::brep::p1::faces(&sheet, None)? {
            let mut sheet_box =
                crate::query::face::p1::calculate_bounding_box(k, &sheet, s, false)?;
            sheet_box.expand_absolute(on_tol);
            let sheet_edges = face::p4::edges(&sheet, s, None, false, None)?;
            let mut pieces: Vec<Curve> = Vec::new();
            let mut only: Option<Surface> = None;
            let mut crossed = 0_usize;
            // the faces the sheet face rolls on or ends on: each holds one of its boundary
            // edges
            let mut supports: BTreeSet<FaceId> = BTreeSet::new();
            for (&f, bx) in target_faces.iter().zip(&target_boxes) {
                if bx.are_disjoint(&sheet_box, on_tol) {
                    continue;
                }
                let mut holds = false;
                for &e in &sheet_edges {
                    if holds {
                        break;
                    }
                    if !edge::is_lamina(&sheet, e)? {
                        continue;
                    }
                    holds = edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)?;
                }
                if holds {
                    supports.insert(f);
                }
            }
            for (&f, bx) in target_faces.iter().zip(&target_boxes) {
                if bx.are_disjoint(&sheet_box, on_tol) || supports.contains(&f) {
                    continue;
                }
                // fitted to the body's own accuracy, so that creases meeting at a corner of
                // the sheet (on two faces past a cliff) meet there within it; exact
                // sections stay exact
                let found = creases(k, &mut sheet, s, self.objects.target, f, body_tol)?;
                // each crease where it lies inside both faces
                let mut in_face = Vec::new();
                for c in &found {
                    pieces_in_face(k, c, self.objects.target, f, tol, &mut in_face)?;
                }
                let before = pieces.len();
                for c in &in_face {
                    pieces_in_face(k, c, &mut sheet, s, tol, &mut pieces)?;
                }
                if pieces.len() > before {
                    // A face past a cliff meets the face the rail leaves (one the blend
                    // rolls on) at a sharp edge. One that runs on from it tangentially (a
                    // round already on the face) is no cliff: the rail would roll over
                    // onto it, and the ball does not fit between.
                    for e in face::p4::edges(self.objects.target, f, None, false, None)? {
                        for g in edge::faces(self.objects.target, e, None)? {
                            if g != f
                                && supports.contains(&g)
                                && rolled_on.contains(&g)
                                && is_tangent_edge(k, self.objects.target, e, tangency)?
                            {
                                // the fork's NoteFitOnFace(g) (49f393d; with
                                // `fixes.fillet_gn_section_generator`, WR-696)
                                if k.fixes.fillet_gn_section_generator {
                                    let face = FilletObject::Topology(
                                        FilletBody::Target,
                                        EntityRef::Face(g),
                                    );
                                    self.executive
                                        .note_fit_on_face(k, &mut self.objects, face)?;
                                }
                                return refuse(
                                    note,
                                    refusal(
                                        FilletErrorType::NoRoom,
                                        Some(NoRoomReason::CreaseAcrossTangentEdge),
                                        "Fillet Error: a rail leaves the face it rolls on across a tangent edge, onto a face it would cut (the blend is larger than the face allows)",
                                    ),
                                );
                            }
                        }
                    }
                    crossed += 1;
                    only = Some(
                        face::p2::surface(self.objects.target, f)?
                            .ok_or_else(|| SolidError::null_pointer(OP))?
                            .clone_for_evaluation()?,
                    );
                }
            }
            let mut new_faces = Vec::new();
            if !pieces.is_empty() {
                let intersection = if crossed == 1 { only } else { None };
                new_faces = with_kernel_callbacks(k, &mut sheet, |k, b, env| {
                    merge_curves(k, b, env, s, tol, pieces, intersection)
                })?
                .0;
            }
            // keep the pieces inside the target
            let mut all = vec![s];
            all.extend(new_faces);
            for p in all {
                // a piece keeps the domain of the face it was split from: its own, from its
                // trims
                let domain = face::p1::calculate_uv_domain_from_uv_trim_curves(
                    k, &mut sheet, p, true, false, false,
                )?
                .domain;
                face::p2::set_uv_domain(&mut sheet, p, &domain)?;
                let inner = face::p2::calculate_an_internal_point(k, &mut sheet, p)?;
                let (inside, on_boundary) = in_material(k, self.objects.target, inner)?;
                if on_boundary {
                    return refuse(
                        note,
                        refusal(
                            FilletErrorType::BadMerge,
                            None,
                            "Fillet Error: a blend past its faces lies on the body's boundary away from its creases",
                        ),
                    );
                }
                if !inside {
                    with_kernel_callbacks(k, &mut sheet, |k, b, env| delete(k, b, env, p))?;
                } else if let Some(&i) = round_of.get(&s) {
                    kept_rounds.insert(i);
                }
            }
        }

        // A blended edge whose blend keeps no face in the body has no length left: its
        // corners take the whole of it (a 20 cube's corner rounded R20: the corner's ball
        // reaches the far faces, the rounds lie past them). Larger than the faces allow,
        // so a fit can reduce it.
        for i in 0..solvers.len() {
            let has = round_of.values().any(|&r| r == i);
            if has && !kept_rounds.contains(&i) {
                // the largest radius that keeps a length of the edge: its corners take
                // r <u, e> at each end (sm_CornerTakesPerRadius), the edge's length L in all
                // at r = L / (s0 + s1); open, so less the fit margin (the fork's a3d6d81;
                // with `fixes.fillet_gn_section_generator`, WR-696)
                if k.fixes.fillet_gn_section_generator {
                    if let Some(&solver) = solvers.get(i) {
                        self.note_fit_of_corners_taking_the_edge(k, solver, solvers)?;
                    }
                }
                return refuse(
                    note,
                    refusal(
                        FilletErrorType::NoRoom,
                        Some(NoRoomReason::CornersTakeWholeEdge),
                        "Fillet Error: a blend past its faces keeps no length of its edge: its corners take the whole of it (the blend is larger than the faces allow)",
                    ),
                );
            }
        }

        // the support faces continued to the kept rails that run past them into the
        // material
        let sheet_edges = crate::topology::brep::p1::edges(&sheet, None)?;
        let mut lamina_edges = Vec::new();
        for &e in &sheet_edges {
            if edge::is_lamina(&sheet, e)? {
                lamina_edges.push(e);
            }
        }
        // WR-706: a side of a section's flat face that runs from the end's vertex along an
        // edge of the body (the seam where the corner's cylinder runs on tangentially into
        // the flat side) is sewn to that edge's piece, not imprinted on its faces
        let mut seam_sides: Vec<(Point3, Point3)> = Vec::new();
        if !section_ends.is_empty() {
            let mut kept_lamina = Vec::new();
            for &e in &lamina_edges {
                let side = self.seam_side(k, &sheet, e, section_ends, tol)?;
                match side {
                    Some(ends) => seam_sides.push(ends),
                    None => kept_lamina.push(e),
                }
            }
            lamina_edges = kept_lamina;
        }
        let mut extensions: Vec<Extension> = Vec::new();
        let mut along: Vec<Curve> = Vec::new();
        for &f in &target_faces {
            let mut rails = Vec::new();
            for &e in &lamina_edges {
                if edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)? {
                    let start = edge::start_vertex(&sheet, e)?
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    let end =
                        edge::end_vertex(&sheet, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
                    rails.push(Rail {
                        curve: edge_curve(k, &sheet, e)?,
                        start,
                        end,
                        start_point: vertex::point(&sheet, start)?,
                        end_point: vertex::point(&sheet, end)?,
                    });
                }
            }
            if rails.is_empty() {
                continue;
            }
            let outcome = extend(k, self.objects.target, f, &rails, blended_edges, tol)?;
            match outcome {
                ExtendOutcome::Refused(why) => {
                    // the fork's NoteFitOnFace(f) (49f393d; with
                    // `fixes.fillet_gn_section_generator`, WR-696)
                    if k.fixes.fillet_gn_section_generator {
                        let face = FilletObject::Topology(FilletBody::Target, EntityRef::Face(f));
                        self.executive
                            .note_fit_on_face(k, &mut self.objects, face)?;
                    }
                    return refuse(
                        note,
                        refusal(
                            FilletErrorType::NoRoom,
                            Some(NoRoomReason::RailRunsPastFaceIntoMaterial),
                            why,
                        ),
                    );
                }
                ExtendOutcome::Extended(made) => {
                    for ext in &made {
                        for &e in &ext.along {
                            along.push(edge_curve(k, self.objects.target, e)?);
                        }
                    }
                    extensions.extend(made);
                }
            }
        }

        // 4: the kept sheet's boundary imprinted on the target faces it lies on
        let mut imprinted: BTreeSet<EdgeId> = BTreeSet::new();
        for &f in &target_faces {
            let mut on = Vec::new();
            for &e in &lamina_edges {
                if edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)? {
                    on.push(edge_curve(k, &sheet, e)?);
                }
            }
            if on.is_empty() {
                continue;
            }
            let (_, on_edges) =
                self.with_target_env(k, |k, b, env, _| merge_curves(k, b, env, f, tol, on, None))?;
            imprinted.extend(on_edges);
        }
        // WR-706: the pieces of the body's edges a section's side lies along, cut off at
        // the rails' ends by the imprint
        for &(a, b) in &seam_sides {
            let target = &*self.objects.target;
            let mut found = false;
            for e in crate::topology::brep::p1::edges(target, None)? {
                let (sv, ev) = (
                    edge::start_vertex(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?,
                    edge::end_vertex(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?,
                );
                let (s_pt, e_pt) = (vertex::point(target, sv)?, vertex::point(target, ev)?);
                if (s_pt.distance(a) <= on_tol && e_pt.distance(b) <= on_tol)
                    || (s_pt.distance(b) <= on_tol && e_pt.distance(a) <= on_tol)
                {
                    imprinted.insert(e);
                    found = true;
                }
            }
            if !found {
                return refuse(
                    note,
                    refusal(
                        FilletErrorType::BadMerge,
                        None,
                        "Fillet Error: a round's section end lies along an edge of the body that its rail does not cut",
                    ),
                );
            }
        }

        // the faces round the blended edges, up to the imprint and the extensions' edges
        // along the body: the material the blends take off
        let target = &*self.objects.target;
        let target_edges = crate::topology::brep::p1::edges(target, None)?;
        for &e in &target_edges {
            let (sv, ev) = (
                edge::start_vertex(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?,
                edge::end_vertex(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?,
            );
            let (s_pt, e_pt) = (vertex::point(target, sv)?, vertex::point(target, ev)?);
            // a piece of an edge an extension runs along, cut at its crossing by the imprint
            for ext in &extensions {
                for &(a, b) in &ext.along_pieces {
                    if (s_pt.distance(a) <= 10.0 * tol && e_pt.distance(b) <= 10.0 * tol)
                        || (s_pt.distance(b) <= 10.0 * tol && e_pt.distance(a) <= 10.0 * tol)
                    {
                        imprinted.insert(e);
                    }
                }
            }
            if along.is_empty() {
                continue;
            }
            let curve = edge::curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let mid = curve.evaluate_point(k, edge::interval(target, e)?.evaluate(0.5))?;
            for c in &along {
                if let Some((_, dist)) = drop_on(k, c, &c.natural_interval(), mid)? {
                    if dist <= tol {
                        imprinted.insert(e);
                        break;
                    }
                }
            }
        }
        let mut gone: BTreeSet<FaceId> = BTreeSet::new();
        let mut gone_order: Vec<FaceId> = Vec::new();
        let mut front: Vec<FaceId> = Vec::new();
        for &e in &target_edges {
            let curve = edge::curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let mid = curve.evaluate_point(k, edge::interval(target, e)?.evaluate(0.5))?;
            let mut on_blended = false;
            for c in &blended_curves {
                if let Some((_, dist)) = drop_on(k, c, &c.natural_interval(), mid)? {
                    if dist <= tol {
                        on_blended = true;
                        break;
                    }
                }
            }
            if !on_blended || imprinted.contains(&e) {
                continue;
            }
            for g in edge::faces(target, e, None)? {
                if gone.insert(g) {
                    gone_order.push(g);
                    front.push(g);
                }
            }
        }
        while let Some(f) = front.pop() {
            for e in face::p4::edges(target, f, None, false, None)? {
                if imprinted.contains(&e) {
                    continue;
                }
                for g in edge::faces(target, e, None)? {
                    if gone.insert(g) {
                        gone_order.push(g);
                        front.push(g);
                    }
                }
            }
        }
        if gone.is_empty() {
            return refuse(
                note,
                refusal(
                    FilletErrorType::BadMerge,
                    None,
                    "Fillet Error: a blend past its faces takes off nothing round its blended edges",
                ),
            );
        }
        if gone.len() >= crate::topology::brep::p1::num_faces(target) {
            return refuse(
                note,
                refusal(
                    FilletErrorType::BadMerge,
                    None,
                    "Fillet Error: a blend past its faces reaches every face of the body (its creases and rails enclose no part of it)",
                ),
            );
        }
        self.with_target_env(k, |k, b, env, _| {
            for f in gone_order {
                delete(k, b, env, f)?;
            }
            Ok(())
        })?;

        // the fillet edges' attribute stays with the executive's own B-reps
        if let (Some(serial), Some(FilletObject::Attribute(handle))) =
            (sheet.notify_serial(), self.executive.edge_attribute)
        {
            for e in crate::topology::brep::p1::edges(&sheet, None)? {
                let key = p3::topology_ref(serial, EntityRef::Edge(e)).key;
                if self
                    .objects
                    .attributes
                    .find_attribute(key, ids::ALL_FILLET_EDGES)
                    == Some(handle)
                {
                    self.objects
                        .attributes
                        .remove_attribute(key, handle, false)?;
                }
            }
        }

        // The sheet's boundary is cut where an extension's loop meets it: a rib that passes
        // under a lip runs on the end face, under the lip (the extension's edge) and on the
        // end face again, three edges of the body for one of the sheet's.
        for ext in &extensions {
            for c in &ext.lp {
                let ivl = c.curve.natural_interval();
                let ends = [
                    c.curve.evaluate_point(k, ivl.min())?,
                    c.curve.evaluate_point(k, ivl.max())?,
                ];
                for p in ends {
                    let mut found = None;
                    for e in crate::topology::brep::p1::edges(&sheet, None)? {
                        if found.is_some() {
                            break;
                        }
                        let (sv, ev) = (
                            edge::start_vertex(&sheet, e)?
                                .ok_or_else(|| SolidError::null_pointer(OP))?,
                            edge::end_vertex(&sheet, e)?
                                .ok_or_else(|| SolidError::null_pointer(OP))?,
                        );
                        if !edge::is_lamina(&sheet, e)?
                            || vertex::point(&sheet, sv)?.distance(p) <= 10.0 * tol
                            || vertex::point(&sheet, ev)?.distance(p) <= 10.0 * tol
                        {
                            continue;
                        }
                        let curve =
                            edge::curve(&sheet, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
                        if let Some((t, dist)) = drop_on(k, curve, &edge::interval(&sheet, e)?, p)?
                        {
                            if dist <= 10.0 * tol {
                                found = Some((e, t));
                            }
                        }
                    }
                    if let Some((e, t)) = found {
                        with_kernel_callbacks(k, &mut sheet, |k, b, env| {
                            let key_of = env.key_of;
                            let curve_ref = |e: EdgeId, kind: i64| ObjectRef {
                                key: key_of(EntityRef::Edge(e)).key,
                                type_code: kind,
                            };
                            crate::topology::brep::p9::make_vertex_split_edge(
                                k, b, e, t, env, &curve_ref,
                            )
                        })?;
                    }
                }
            }
        }

        // the sheet sewn in, and the body oriented. The sheet is merged in as the fillet's
        // normal path merges it (SmBrep::MergeBrep, surfaces kept as they are): its edges
        // keep their own tolerances and the body its own.
        self.with_target_env(k, |k, b, env, identity| {
            let before: BTreeSet<FaceId> =
                crate::topology::brep::p1::faces(b, None)?.into_iter().collect();
            let source_key = |e: EntityRef| notify_key(&sheet, e);
            p3::merge_brep(k, env, b, &sheet, &source_key, None, None, false)?;
            let mut copies: Vec<FaceId> = crate::topology::brep::p1::faces(b, None)?
                .into_iter()
                .filter(|f| !before.contains(f))
                .collect();
            for ext in extensions {
                copies.push(make_extension_face(k, b, env, ext, tol)?);
            }
            let listed: Vec<Option<FaceId>> = copies.iter().copied().map(Some).collect();
            let stitched = {
                let mut edits = crate::heal::stitch::p1::BorrowedStitchEdits {
                    env: &mut *env,
                    curve_identity: crate::topology::brep::p9::CurveIdentity::Actual(
                        &crate::topology::brep::p9::issue_curve_identity,
                    ),
                };
                crate::topology::brep::p4::stitch_faces_with_context(
                    k,
                    b,
                    tol,
                    Some(&listed),
                    false,
                    Some(identity),
                    Some(&mut edits),
                )?
            };
            // the edges the sheet is sewn along hold it to its own accuracy
            let sewn_tol = smos_max(
                tol,
                smos_max(
                    stitched.max_vertex_edge_gap,
                    stitched.max_edge_face_trim_curve_gap,
                ),
            );
            for &f in &copies {
                for e in face::p4::edges(b, f, None, false, None)? {
                    edge::set_tolerance(b, e, sewn_tol, true, true)?;
                }
            }
            let mut maybe_open = false;
            if stitched.lamina_edges == 0 {
                let mut marks = Marks::default();
                let oriented =
                    p3::orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)?;
                maybe_open = oriented.maybe_not_closed_solid;
            }
            if stitched.lamina_edges != 0
                || maybe_open
                || !crate::topology::brep::p2::is_manifold_solid(b)?
            {
                return refuse(
                    note,
                    refusal(
                        FilletErrorType::BadMerge,
                        None,
                        "Fillet Error: a blend past its faces does not sew into the body along its creases and rails",
                    ),
                );
            }

            // Faces sewn on one surface are one face, as a boolean's post-process leaves
            // them (only the edges it made are looked at, SmMerge::BooleanPostProcess): a
            // corner's end face on the end wall's plane carried on over a rim is that wall,
            // past the edges the blend sewed in. Every other edge and vertex of the body
            // stays.
            let mut sewn_edges: BTreeSet<EdgeId> = BTreeSet::new();
            let mut sewn_vertices = BTreeSet::new();
            for &f in &copies {
                for e in face::p4::edges(b, f, None, false, None)? {
                    sewn_edges.insert(e);
                    if let Some(v) = edge::start_vertex(b, e)? {
                        sewn_vertices.insert(v);
                    }
                    if let Some(v) = edge::end_vertex(b, e)? {
                        sewn_vertices.insert(v);
                    }
                }
            }
            let keep_edges: Vec<EdgeId> = crate::topology::brep::p1::edges(b, None)?
                .into_iter()
                .filter(|e| !sewn_edges.contains(e))
                .collect();
            let keep_vertices: Vec<_> = crate::topology::brep::p1::vertices(b, None)?
                .into_iter()
                .filter(|v| !sewn_vertices.contains(v))
                .collect();
            crate::topology::brep::p5::remove_topological_with_notifications(
                k,
                b,
                Some(&keep_edges),
                Some(&keep_vertices),
                Some(env),
            )
        })
    }
}
