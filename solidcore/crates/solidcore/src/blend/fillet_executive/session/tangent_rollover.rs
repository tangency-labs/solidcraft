//! A lone round rolls over a smooth far edge of its face, the whole length (not upstream;
//! solidcore-native): before its trace, a lone round's solver
//! rolls its ball on the face beyond on each side where the ball reaches past the face's smooth
//! far edge ([`crate::blend::fillet_tangent_rollover::tangent_rollover_supports`]).
//!
//! | Rust                                     | Upstream |
//! |------------------------------------------|----------|
//! | `FilletSession::roll_over_tangent_edges` | none     |
//!
//! Deviations: none of upstream's; there is no recursion.

use super::{FilletObject, FilletSession, SolverObject};
use crate::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use crate::blend::fillet_solver::p1::{
    FilletSurfaceGenerator, FilletSurfaceGeneratorType, OffsetSurfaceSlot,
};
use crate::blend::fillet_tangent_rollover::tangent_rollover_supports;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::surface::offset_surface::SS_UNKNOWN;
use crate::topology::edgeuse::p1 as eu1;

const OP: &str = "blend::fillet_executive::session::tangent_rollover";

impl FilletSession<'_> {
    /// A lone constant round of circular section whose
    /// ball reaches past a smooth far edge of a face beside it rolls on the face beyond:
    /// that side's solver offset surface is replaced by one of the same offset (its sign
    /// following the faces' normals across the edge) on the face beyond's surface, owned by
    /// that face, in the solver and in its fillet geometries, so the rail is traced, classified
    /// and trimmed on the face it lies on. A round built together with others meets them in
    /// corners this construction does not make yet, so only a round on its own rolls over.
    ///
    /// # Errors
    /// Those of the topology queries and of [`tangent_rollover_supports`].
    pub(super) fn roll_over_tangent_edges(&mut self, k: &Kernel) -> Result<()> {
        if self.executive.solvers.len() != 1 {
            return Ok(());
        }
        let Some(&FilletObject::Solver(id)) = self.executive.solvers.first() else {
            return Ok(());
        };
        let object = self.objects.solver(id)?;
        let base = object.base();
        let circular = base
            .fsg
            .as_ref()
            .map(FilletSurfaceGenerator::fillet_surface_generator_type)
            == Some(FilletSurfaceGeneratorType::Circular);
        if !matches!(object, SolverObject::ConstantRadius(_)) || !circular {
            return Ok(());
        }
        let tangency = base.tangency_tolerance;
        let uses = base.edgeuses;
        let mut offsets = [0.0; 2];
        for (offset, rail) in offsets.iter_mut().zip(0..2) {
            *offset = base
                .surface(rail)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
                .surface
                .offset_distance();
        }
        let geoms = base.fillet_geoms();
        let target = &mut *self.objects.target;
        let [Some(u0), Some(u1)] = uses else {
            return Ok(());
        };
        let Some(edge) = eu1::edge(target, u0)? else {
            return Ok(());
        };
        let mut faces = Vec::with_capacity(2);
        for u in [u0, u1] {
            let face = eu1::faceuse(target, u)?
                .map(|fu| eu1::faceuse_face(target, fu))
                .transpose()?
                .flatten()
                .ok_or_else(|| SolidError::blend_missing(OP, "the face use's face"))?;
            faces.push(face);
        }
        let faces = <[_; 2]>::try_from(faces)
            .map_err(|_| SolidError::blend_missing(OP, "a face on each side of the edge"))?;
        let rollovers = tangent_rollover_supports(k, target, edge, faces, offsets, tangency)?;
        for (rail, rollover) in rollovers.into_iter().enumerate() {
            let Some(rollover) = rollover else {
                continue;
            };
            let support = crate::topology::face::p2::surface(self.objects.target, rollover.face)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?
                .clone_for_evaluation()?;
            let sign = if rollover.opposite_normals { -1.0 } else { 1.0 };
            let solver = self.objects.solver_mut(id)?.base_mut();
            let old = solver
                .surface(rail)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?;
            let (distance, extends) = (
                old.surface.offset_distance() * sign,
                old.surface.surface.allow_extension(),
            );
            let key = solver
                .surfaces
                .iter()
                .flatten()
                .chain(solver.detached.iter())
                .map(|s| s.key)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| SolidError::invalid_input(OP, "offset identity exhausted"))?;
            let mut surface = OffsetSurface::new(
                k,
                distance,
                support.clone_for_evaluation()?,
                SurfaceId::Face(rollover.face),
                SS_UNKNOWN,
            )?;
            surface.surface.set_allow_extension(extends);
            solver.set_surface(rail, Some(OffsetSurfaceSlot { key, surface }))?;
            if let Some(slot) = solver.rolled_faces.get_mut(rail) {
                *slot = Some(rollover.face);
            }
            if rollover.opposite_normals {
                if let Some(o) = solver.orientations.get_mut(rail) {
                    *o *= -1.0;
                }
            }
            for &g in &geoms {
                let Some(slot) = self.objects.pseudo.geom_mut(g)?.off_surfs.get_mut(rail) else {
                    continue;
                };
                let Some(own) = slot.as_ref() else {
                    continue;
                };
                let (distance, extends) = (
                    own.surface.offset_distance() * sign,
                    own.surface.allow_extension(),
                );
                let mut copy = OffsetSurface::new(
                    k,
                    distance,
                    support.clone_for_evaluation()?,
                    SurfaceId::Face(rollover.face),
                    SS_UNKNOWN,
                )?;
                copy.surface.set_allow_extension(extends);
                *slot = Some(copy);
            }
        }
        Ok(())
    }
}
