//! The session's refusals on its edges, typed ([`BlendRefusal`] in
//! [`SolidError::BlendRefused`]): an edge with no blend (from every preparation), a round
//! too small to change the part (constant circular rounds, before preparation), a round
//! that cannot end where its edge runs on tangentially and a setback asked for too short
//! or too long (from `DoFilleting`).
//!
//! Not upstream (solidcore's): upstream fails an edge with no blend edge use with
//! `SM_ERR_INVALID_INPUT`, and the others with `SM_ERR`; each refusal keeps that status
//! ([`SolidError::sm_status`]) and the error info noted. No recursion.

use super::{FilletBody, FilletObject, FilletSession};
use crate::blend::blend_refusal::{continuation_at, round_too_small, BlendRefusal};
use crate::blend::fillet_executive::p1::SmoothSequenceAccess;
use crate::blend::fillet_executive::NoRoomReason;
use crate::blend::fillet_geom::p1::FilletErrorType;
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::error::{SM_ERR, SM_ERR_INVALID_INPUT};
use crate::topology::{edge, EdgeId, EdgeuseId};
use crate::{EntityRef, Kernel, Result};

const OP: &str = "blend::fillet_executive::session::edge_refusals";

/// The tangency (radians) at which a corner takes two edges as running on, where the
/// corner of the vertex cannot be read: the executive's default (`SetFilletParameters`'
/// 10 degrees).
const DEFAULT_TANGENCY: f64 = 10.0 * (crate::math::PI / 180.0);

impl FilletSession<'_> {
    /// The blend edge use of `edge` ([`Self::blend_edgeuse`]), or the refusal of an edge with
    /// none: [`BlendRefusal::EdgeNotBlendable`] with its reason, status
    /// `SM_ERR_INVALID_INPUT`.
    ///
    /// # Errors
    /// The refusal; those of [`edge::blend_edgeuse_or_why`].
    pub fn blendable_edgeuse(&mut self, k: &Kernel, edge: EdgeId) -> Result<EdgeuseId> {
        match edge::blend_edgeuse_or_why(k, self.objects.target, edge)? {
            Ok(eu) => Ok(eu),
            Err(reason) => {
                Err(BlendRefusal::EdgeNotBlendable { edge, reason }.error(OP, SM_ERR_INVALID_INPUT))
            }
        }
    }

    /// A preparation's result (`SetFilletParameters`) with its refusal of an edge with no
    /// blend edge use typed: the edge its error info names, when it has none.
    pub(super) fn typed_unblendable(&mut self, k: &Kernel, result: Result<()>) -> Result<()> {
        let Err(e) = result else {
            return result;
        };
        if e.sm_status() != Some(SM_ERR_INVALID_INPUT)
            || self.executive.error.code != FilletErrorType::InvalidInput as i32
        {
            return Err(e);
        }
        let Some(FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge))) =
            self.executive.error.edge
        else {
            return Err(e);
        };
        match edge::blend_edgeuse_or_why(k, self.objects.target, edge)? {
            Err(reason) => {
                Err(BlendRefusal::EdgeNotBlendable { edge, reason }.error(OP, SM_ERR_INVALID_INPUT))
            }
            Ok(_) => Err(e),
        }
    }

    /// The first circular constant round of `edges` (edge i at `radii[i]`, or `radii[0]`
    /// for every edge; its section
    /// `sections[i]` or `sections[0]`) too small to change the part
    /// ([`round_too_small`]), refused as [`BlendRefusal::RoundTooSmall`] with status `SM_ERR`
    /// before the edges are prepared. Edges without a blend edge use are left to the
    /// preparation.
    ///
    /// The check reads a copy of the target, made once for the call: its queries build the
    /// UV trim curves an edge use lacks and refit the edge's tolerance to them
    /// (`GetBlendEdgeuse`, `EvaluateBinormal`), and on the target that would change the body
    /// before upstream's preparation reads it, a refused round included (an all-edges round
    /// of `Fillet/cyl_on_box_w_fillet.smb` left edges at 1.03e-5 where they stay at 1e-5).
    pub(super) fn refuse_rounds_too_small(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        radii: &[f64],
        sections: &[FilletSurfaceGeneratorType],
    ) -> Result<()> {
        let mut scratch: Option<crate::topology::Brep> = None;
        for (i, &e) in edges.iter().enumerate() {
            let radius = radii.get(i).or_else(|| radii.first()).copied();
            let section = sections.get(i).or_else(|| sections.first()).copied();
            let (Some(radius), Some(section)) = (radius, section) else {
                continue; // a size list the preparation refuses
            };
            if section != FilletSurfaceGeneratorType::Circular
                || !(radius.is_finite() && radius > 0.0)
            {
                continue;
            }
            let copy = scratch.get_or_insert_with(|| self.objects.target.clone());
            if let Some(refusal) = round_too_small(k, copy, e, radius)? {
                return Err(refusal.error(OP, SM_ERR));
            }
        }
        Ok(())
    }

    /// `DoFilleting`'s result with its refusal typed, its status unchanged: a Setback's
    /// refusal noted on the way (a setback asked for too short or too long:
    /// [`BlendRefusal::SetbackTooShort`], [`BlendRefusal::SetbackTooLong`]), which the
    /// later steps return as their own failure; else a round that ends where its edge runs
    /// on tangentially ([`NoRoomReason::TangentContinuationEndUnsupported`]): the first such
    /// end's edge and vertex and the edge not blended that it runs on into
    /// ([`BlendRefusal::ContinuesInto`]), at the tangency of the vertex's corner. A refusal
    /// that names no such data stays as it was.
    pub(crate) fn typed_filleting(&mut self, k: &Kernel, result: Result<()>) -> Result<()> {
        let noted = self.objects.setback_refusal.take();
        let Err(e) = result else {
            return result;
        };
        if let (Some(refusal), Some(code)) = (noted, e.sm_status()) {
            return Err(refusal.error(OP, code));
        }
        if self.executive.error.no_room_reason()
            != Some(NoRoomReason::TangentContinuationEndUnsupported)
        {
            return Err(e);
        }
        let Some(code) = e.sm_status() else {
            return Err(e);
        };
        let mut blended = Vec::new();
        for &solver in &self.executive.solvers {
            if let Ok(edge) = self.objects.smooth_solver_edge0_id(solver) {
                blended.push(edge);
            }
        }
        let ends: Vec<_> = self
            .executive
            .open_ends
            .iter()
            .filter(|end| end.section)
            .map(|end| (end.solver, end.vertex))
            .collect();
        for (solver, vertex) in ends {
            let FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v)) = vertex else {
                continue;
            };
            let Ok(edge) = self.objects.smooth_solver_edge0_id(solver) else {
                continue;
            };
            let tangency = match self.executive.fillet_corner_of_vertex(vertex)? {
                Some(corner) => self.objects.corner_object(corner)?.tangency_tol_radians,
                None => DEFAULT_TANGENCY,
            };
            let next = continuation_at(k, self.objects.target, edge, v, tangency, &blended)?;
            if let Some(continues_into) = next {
                return Err(BlendRefusal::ContinuesInto {
                    edge,
                    vertex: v,
                    continues_into,
                }
                .error(OP, code));
            }
        }
        Err(e)
    }
}
