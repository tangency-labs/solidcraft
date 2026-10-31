//! The cliff overflow's subtraction on the live session (not upstream): the fork's
//! `SmFilletExecutive::SubtractRemovalRegion` and `sm_HoldsItsLaw`
//! (`source/SMLib/src/SmFilletCliffOverflow.cpp`, tangency-labs/smlib `8ba4667`, `ae2a6de`,
//! rev `68fb9162`; `fixes.fillet_cliff_overflow`, WR-631, WR-638). The construction steps
//! are in [`crate::blend::fillet_cliff_overflow`].
//!
//! | Rust                                       | Fork                                      |
//! |--------------------------------------------|-------------------------------------------|
//! | `FilletSession::subtract_removal_region`   | `SmFilletExecutive::SubtractRemovalRegion` |
//! | `FilletSession::holds_its_law` (private)   | `sm_HoldsItsLaw`                          |
//!
//! Deviations:
//! - **The removal region leaves the session first.** Once each round's law is checked
//!   (which reads the fillet surfaces where the intermediate body owns them), the
//!   intermediate body is taken out of the session, as the fork sets `m_pFilletBrep` to
//!   `NULL` before the subtraction; the open ends are cut, the region closed, stitched and
//!   oriented on it as a stand-alone body. The faces of each solver's fillet geometries are
//!   followed through the cuts in a local map rather than by resetting the geometries'
//!   fillet surfaces (`SetFilletSurface`), which nothing reads afterwards.
//! - **A failed subtraction keeps the target.** The Boolean consumes its operands, so the
//!   target is copied before it and the copy put back when it fails (the fork's in-place
//!   merge leaves whatever it reached).
//! - **The target's application attributes go through the Boolean.** The session keeps
//!   the bodies' attributes in its own store, so the target's application attributes
//!   (`SM_AI_USER_1` and up) are handed to it for the Boolean and taken back from the
//!   result, as the fork's ride on the objects it splits and merges (#1980); the
//!   kernel's own ids stay in the session store, found by handle.

use std::collections::BTreeMap;

use super::{invalid, FilletBody, FilletObject, FilletSession, SolverObject};
use crate::blend::fillet_cliff_overflow::{
    close_removal_region, cut_fillet_at_open_end, Blended, Refusal,
};
use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::{FilletErrorType, FilletGeomId, FilletSurfaceStorage};
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::boolean::api_brep::boolean;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::geom::attribute::ids;
use crate::kernel::Kernel;
use crate::math::{Vec3, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::brep::p3::{orient_trimmed_surfaces, with_kernel_callbacks};
use crate::topology::topology_traverser::Marks;
use crate::topology::{edge, edgeuse::p1 as eu1, face, vertex, Brep, FaceId};

const OP: &str = "blend::fillet_executive::session::cliff_overflow";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// The `SmStatus` upstream's `SER` returns for a failed callee (a `double` function's
/// status as its value, as `SmVariableRadiusFS::GetFilletRadius` returns it).
fn status_of(e: &SolidError) -> i32 {
    match e {
        SolidError::Upstream { code, .. } => *code,
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => {
            crate::error::SM_ERR_INVALID_INPUT
        }
        _ => crate::error::SM_ERR,
    }
}

impl FilletSession<'_> {
    /// `GetFilletErrorInfo()->NoteError(code, comment)` of a [`Refusal`].
    fn note_refusal(&mut self, refusal: Refusal) {
        self.executive.error.code = refusal.code as i32;
        self.executive.error.comment = refusal.comment.into();
        self.executive.error.no_room = refusal.reason;
    }

    /// Note `note`'s refusal, if any, and pass `result` on.
    fn noted<T>(&mut self, note: Option<Refusal>, result: Result<T>) -> Result<T> {
        if let Some(refusal) = note {
            self.note_refusal(refusal);
        }
        result
    }

    /// The fork's `sm_HoldsItsLaw`: whether a variable round on planar faces holds its
    /// radius law between the filleted edge's ends.
    ///
    /// A rolling ball of radius r between two planes meeting at the interior angle a stands
    /// off the edge by `r (1 / sin(a/2) - 1)` where its section crosses the wedge's
    /// bisector. That point must lie on the fillet surface, at seven points along the edge,
    /// to twenty times the solver's tolerance and a ten-thousandth of the radius; of the two
    /// angles the planes make, and the bisector either way, the one that fits. Rounds of
    /// other solvers, with other than a circular section, on faces that are not both
    /// planes, with other than one fillet geometry, or along an edge that is not straight,
    /// pass.
    fn holds_its_law(&self, k: &Kernel, solver: FilletObject) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let object = self.objects.solver(id)?;
        let SolverObject::VariableRadius(vr) = object else {
            return Ok(true);
        };
        let base = object.base();
        if base.fsg.as_ref().map(
            crate::blend::fillet_solver::p1::FilletSurfaceGenerator::fillet_surface_generator_type,
        ) != Some(FilletSurfaceGeneratorType::Circular)
        {
            return Ok(true);
        }
        let mut normals = [Vec3::UNDEF; 2];
        for (rail, normal) in normals.iter_mut().enumerate() {
            let offset = base
                .surface(rail)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let planar = offset.surface.base_surface().is_planar(k, EFF_ZERO)?;
            if !planar.is_planar {
                return Ok(true);
            }
            *normal = planar.normal;
        }
        let [n0, n1] = normals;
        let phi = smos_max(-1.0, smos_min(1.0, n0.dot(n1))).acos();
        let [geom] = base.fillet_geoms[..] else {
            return Ok(true);
        };
        let Some(fillet) = self.objects.fillet_surface(geom)? else {
            return Ok(true);
        };
        let fillet = fillet.clone_for_evaluation()?;
        let eu = base
            .edgeuse(0)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let target = &*self.objects.target;
        let e = eu1::edge(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let curve = edge::curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        if !curve.is_linear(k, EFF_ZERO, false)?.linear {
            return Ok(true);
        }
        let interval = edge::interval(target, e)?;
        let domain = fillet.natural_uv_domain_with_nurb(k)?;
        let tol = 20.0 * base.approx_tol_3d;
        // (the law evaluates through its own caches: a copy, the solver stays shared)
        let mut law = vr.law.clone();
        let mut holds = true;
        let mut i = 1;
        while i < 8 && holds {
            let t = interval.evaluate(f64::from(i) / 8.0);
            i += 1;
            let pnt = curve.evaluate_point(k, t)?;
            // SmVariableRadiusFS::GetFilletRadius: SER inside a `double` function
            let radius = match law.evaluate(t, &vr.law_interval, vr.law_orientation) {
                Ok(values) => values.first().copied().unwrap_or(0.0),
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => f64::from(status_of(&e)),
            }
            .abs();
            // The section's middle: along the wedge's bisector, r (1 / sin(a/2) - 1) off the
            // edge. The planes' normals are both outward (or inward), the bisector along
            // their sum and the angle pi less theirs, or one of each, the bisector along
            // their difference.
            let mut fits = false;
            let mut c = 0;
            while c < 4 && !fits {
                let bisector = if c < 2 { n0 + n1 } else { n0 - n1 };
                let angle = if c < 2 { PI - phi } else { phi };
                let sign = if c % 2 == 1 { -1.0 } else { 1.0 };
                c += 1;
                let len = bisector.length();
                if len <= EFF_ZERO_SQRT || angle <= EFF_ZERO_SQRT {
                    continue;
                }
                let stand_off = radius * (1.0 / (angle / 2.0).sin() - 1.0);
                let middle = pnt + bisector * (sign * stand_off / len);
                let dropped = fillet.drop_point::<NoObject>(
                    k,
                    middle,
                    &domain,
                    &DropPointOptions::default(),
                )?;
                // the solver's tolerance, and its circular sections' relative accuracy (a
                // non-rational cubic across the round, within about 2e-5 of the radius)
                fits = dropped.success && dropped.gap <= tol + 1.0e-4 * radius;
            }
            holds = fits;
        }
        Ok(holds)
    }

    /// The face of the intermediate body each of `solver`'s fillet geometries' surfaces is
    /// on (`pFG->GetFilletSurface()->GetOwner()` in `m_pFilletBrep`), `None` for none.
    fn fillet_faces(&self, solver: FilletObject) -> Result<Vec<(FilletGeomId, Option<FaceId>)>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let body = self
            .objects
            .intermediate_identity(EntityRef::Brep(crate::topology::BrepId::from_index(0)))?;
        let mut out = Vec::new();
        for &g in &self.objects.solver(id)?.base().fillet_geoms {
            let face = match self.objects.pseudo.geom(g)?.fillet_surface {
                Some(FilletSurfaceStorage::ManagedFace { body: owner, face }) if owner == body => {
                    Some(face)
                }
                _ => None,
            };
            out.push((g, face));
        }
        Ok(out)
    }

    /// The fork's `SmFilletExecutive::SubtractRemovalRegion` (WR-631, `8ba4667`; its errors
    /// as `ae2a6de` keeps them, WR-638): in a cliff overflow, subtract the region the blends
    /// remove from the target: the blends built on their faces' continuations, then cut by
    /// what lies beyond the cliffs.
    ///
    /// Each variable round on planes must hold its law (`NoRoom` otherwise); each open end
    /// is cut ([`cut_fillet_at_open_end`]); the intermediate body is closed into the
    /// removal region ([`close_removal_region`]), stitched, and oriented as a solid; a
    /// region that is not a manifold solid without lamina edges is `BadMerge`. The fillet
    /// edges' attribute leaves the region's edges, and the region is subtracted from the
    /// target by the manifold Boolean (`BadMerge` when that fails). Each refusal is noted
    /// in the error info.
    ///
    /// # Errors
    /// The refusals as `SM_ERR`; those of the steps.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub(super) fn subtract_removal_region(&mut self, k: &mut Kernel) -> Result<()> {
        if self.objects.intermediate.is_none() {
            return Err(SolidError::null_pointer(OP));
        }
        let solvers = self.executive.solvers.clone();
        for &solver in &solvers {
            if !self.holds_its_law(k, solver)? {
                self.note_refusal(Refusal {
                    code: FilletErrorType::NoRoom,
                    reason: Some(NoRoomReason::CliffLawNotHeld),
                    comment: "Fillet Error: a variable round traced on its faces' continuations does not hold its radius law",
                });
                return Err(SolidError::sm_err(OP));
            }
        }

        // What the cuts and the closing read of the solvers, before the region leaves.
        let mut faces_of: BTreeMap<FilletObject, Vec<(FilletGeomId, Option<FaceId>)>> =
            BTreeMap::new();
        let mut tol = 0.0;
        let mut blended_edges = Vec::new();
        for &solver in &solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let base = self.objects.solver(id)?.base();
            tol = smos_max(tol, base.approx_tol_3d);
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            blended_edges.push(
                eu1::edge(self.objects.target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?,
            );
            faces_of.insert(solver, self.fillet_faces(solver)?);
        }
        let open_ends = self.executive.open_ends.clone();

        let mut region = self
            .objects
            .intermediate
            .take()
            .ok_or_else(|| SolidError::null_pointer(OP))?;

        let mut note = None;
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
            let end_surface = face::p2::surface(target, end_face)?
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .clone_for_evaluation()?;
            // the fillet's point nearest the middle of the filleted edge
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let fil_edge = eu1::edge(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let edge_mid = edge::curve(target, fil_edge)?
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .evaluate_point(k, edge::interval(target, fil_edge)?.evaluate(0.5))?;
            let vertex_point = vertex::point(target, v)?;
            let entry = faces_of
                .get_mut(&end.solver)
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let mut faces: Vec<Option<FaceId>> = entry.iter().map(|&(_, f)| f).collect();
            let cut = cut_fillet_at_open_end(
                k,
                &mut region,
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
        let mut blended = Blended::of_edges(self.objects.target, &blended_edges)?;
        // the filleted edge of each fillet face of the sheet (WR-636, `bba9914`), read
        // after the cuts, which can replace a geometry's face
        for (&solver, &edge) in solvers.iter().zip(&blended_edges) {
            for &(_, f) in faces_of.get(&solver).map_or(&[][..], Vec::as_slice) {
                if let Some(f) = f {
                    blended.edge_of_fillet.insert(f, edge);
                }
            }
        }
        let closed = close_removal_region(
            k,
            &mut region,
            self.objects.target,
            &blended,
            tol,
            &mut note,
        );
        self.noted(note.take(), closed)?;

        // stitch the closing faces to the sheet, and orient the closed shell as a solid
        let tolerance = region.tolerance();
        let stitched =
            crate::topology::brep::p4::stitch_faces(k, &mut region, tolerance, None, false)?;
        let mut maybe_open = false;
        if stitched.lamina_edges == 0 {
            let oriented = with_kernel_callbacks(k, &mut region, |k, b, env| {
                let mut marks = Marks::default();
                orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)
            })?;
            maybe_open = oriented.maybe_not_closed_solid;
        }
        if stitched.lamina_edges != 0
            || maybe_open
            || !crate::topology::brep::p2::is_manifold_solid(&region)?
        {
            self.note_refusal(Refusal {
                code: FilletErrorType::BadMerge,
                reason: None,
                comment:
                    "Fillet Error: a cliff overflow's removal region does not close into a solid",
            });
            return Err(SolidError::sm_err(OP));
        }

        // the fillet edges' attribute stays with the executive's own B-reps
        if let (Some(serial), Some(FilletObject::Attribute(handle))) =
            (region.notify_serial(), self.executive.edge_attribute)
        {
            for e in crate::topology::brep::p1::edges(&region, None)? {
                let key = crate::topology::brep::p3::topology_ref(serial, EntityRef::Edge(e)).key;
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

        // the region from the target, by the Boolean (the merge modifies the target in
        // place; the region is deleted after)
        let kept = (*self.objects.target).clone();
        let target_solid = crate::topology::brep::p2::is_manifold_solid(&kept)?;
        let serial = self.objects.target_serial;
        // (not upstream) the target's application attributes ride on its objects through
        // the Boolean, as the fork's live on them, so a split, merged or copied edge keeps
        // them (#1980); the session store takes the result's back once it succeeds
        let carried = super::body_application_attributes(&self.objects.attributes, &kept, serial)?;
        let mut taken: Brep = std::mem::take(&mut *self.objects.target);
        taken.set_attributes(carried);
        match boolean(k, taken, region, BooleanOperation::Difference) {
            Ok(outcome) => {
                // Not the fork's (WR-631 addendum): the subtraction from a manifold solid
                // must leave one. The Boolean can succeed with an empty or open body (a
                // region whose face meets a target face a hair from its edge, #1467's box
                // at r10.02); the fork checks only the Boolean's status. Refused here as
                // no room, the target as it was.
                if target_solid
                    && (outcome.result.faces().ids().next().is_none()
                        || !crate::topology::brep::p2::is_manifold_solid(&outcome.result)?)
                {
                    *self.objects.target = kept;
                    self.note_refusal(Refusal {
                        code: FilletErrorType::NoRoom,
                        reason: Some(NoRoomReason::CliffSubtractionLeavesNoSolid),
                        comment: "Fillet Error: subtracting a cliff overflow's removal region leaves no solid",
                    });
                    return Err(SolidError::sm_err(OP));
                }
                let mut result = outcome.result;
                super::replace_application_attributes(
                    &mut self.objects.attributes,
                    &mut result,
                    serial,
                )?;
                *self.objects.target = result;
                Ok(())
            }
            Err(e) => {
                *self.objects.target = kept;
                if e.is_unported() {
                    return Err(e);
                }
                self.note_refusal(Refusal {
                    code: FilletErrorType::BadMerge,
                    reason: None,
                    comment: "Fillet Error: subtracting a cliff overflow's removal region failed",
                });
                Err(SolidError::sm_err(OP))
            }
        }
    }
}
