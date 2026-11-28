//! Run the kernel's separate Ends from a complete live-session fillet.

use super::{FilletBody, FilletObject, FilletSession, NetScope};
use crate::blend::blend_corner::separate_ends::{plan, SectionKind, SeparateEndPlan};
use crate::blend::blend_corner::TurnChoice;
use crate::blend::fillet_executive::p3::{FitKind, FitSize, NoRoomReason};
use crate::blend::fillet_geom::p1::FilletErrorType;
use crate::blend::fillet_solver::p1::{FilletSurfaceGenerator, FilletSurfaceGeneratorType};
use crate::error::blend::BlendFailure;
use crate::error::topology::TopologyFailure;
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::kernel::Kernel;
use crate::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use crate::topology::{edgeuse::p1 as eu1, Brep, EdgeId};

pub(crate) const OP: &str = "blend::fillet_executive::session::separate_ends";

struct Selection {
    kind: SectionKind,
    edges: Vec<EdgeId>,
    sizes: Vec<f64>,
}

impl FilletSession<'_> {
    /// A complete regular blend can finish independent Ends in two groups.
    /// Staged sessions keep their caller's control of the corner and merge steps.
    pub(super) fn finish_separate_ends(&mut self, k: &mut Kernel) -> Result<bool> {
        let Some(Selection { kind, edges, sizes }) = self.separate_end_selection(k)? else {
            return Ok(false);
        };
        let groups = match plan(k, self.objects.target, &edges, &sizes, kind)? {
            SeparateEndPlan::NotApplicable => return Ok(false),
            SeparateEndPlan::Limited { size, edge } => {
                let at = EntityRef::Edge(edge);
                self.executive.error.code = FilletErrorType::NoRoom as i32;
                self.executive.error.no_room = Some(NoRoomReason::RailLeavesFace);
                self.executive.note_fit_radius(
                    size,
                    Some(FilletObject::Topology(FilletBody::Target, at)),
                    FitKind::Radius,
                );
                return Err(SolidError::FilletRefused {
                    operation: OP,
                    code: crate::error::SM_ERR,
                    entity: Some(at),
                    reason: self.executive.error.no_room,
                    fit: FitSize {
                        size,
                        edge: Some(edge),
                        kind: FitKind::Radius,
                    },
                });
            }
            SeparateEndPlan::Ends(groups) => groups,
        };
        let section = match kind {
            SectionKind::Chamfer => FilletSurfaceGeneratorType::Linear,
            SectionKind::Round => FilletSurfaceGeneratorType::Circular,
        };
        let objects = &mut self.objects;
        let serial = objects.target_serial;
        let mut attrs = objects.attributes.clone();
        let mut body_attrs = attrs.clone();
        if let Some(FilletObject::Attribute(edge_mark)) = self.executive.edge_attribute {
            body_attrs.destroy(edge_mark)?;
        }
        let body_attrs = super::super::body_attributes(&body_attrs, objects.target, serial)?;
        attrs.retain_rekeyed(&|key| {
            crate::topology::brep::p3::topology_entity(serial, key)
                .is_none()
                .then_some(key)
        })?;

        let result = crate::blend::definition_check::keeping_blend_face_log(k, |k| {
            objects.target.with_rollback(
                |target| {
                    // Return the target's attributes to it while the two ordinary
                    // sessions run. Import what survives back into this session.
                    target.set_attributes(body_attrs);
                    groups.build(k, target, |k, b, edges, sizes| {
                        Ok(constant_group(k, b, edges, sizes, section))
                    })??;
                    super::super::seed_body_attributes(&mut attrs, target, serial)?;
                    target.take_attributes();
                    k.check_cancelled(OP)
                },
                Result::is_ok,
            )
        });
        result.map_err(without_group_fit)?;
        objects.attributes = attrs;
        Ok(true)
    }

    /// The edge and section of every loaded standard constant solver.
    /// Auto still permits these Ends: the plan qualifies six-sector fans, so
    /// none of these pairs is a three-edge Turn that could be swept.
    fn separate_end_selection(&self, k: &Kernel) -> Result<Option<Selection>> {
        let exec = &self.executive;
        let objects = &self.objects;
        let n = exec.solvers.len();
        let options = exec.options;
        if n < 2
            || !exec.initialized
            || n % 2 != 0
            || !options.topology_insertion
            || !options.classification
            || !options.trimming
            || options.piecewise_merge
            || options.global_merge
            || exec.mark_corner_patches
            || exec.cliff_overflow
            || exec.overflow_first
            || exec.classify_only
            || exec.roll_over_pass
            || !exec.earlier_pass_faces.is_empty()
            || !exec.fan_split_points.is_empty()
            || objects.second_base.is_some()
            || objects.turns == TurnChoice::Swept
            || objects.roll_over_classify
            || !objects.roll_over_ends.is_empty()
            || objects.chamfer_corner.is_some()
            || !objects.set_back_vertices.is_empty()
            || !objects.set_back_fills.is_empty()
            || objects.self_intersection_handler.is_some()
            || objects.curve_blend_self_intersection_handler.is_some()
            || !objects.corners.is_empty()
        {
            return Ok(None);
        }
        let mut edges = Vec::new();
        let mut sizes = Vec::new();
        edges
            .try_reserve_exact(n)
            .and_then(|()| sizes.try_reserve_exact(n))
            .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
        let mut common = None;
        for &object in &exec.solvers {
            k.check_cancelled(OP)?;
            let FilletObject::Solver(id) = object else {
                return Ok(None);
            };
            let super::super::SolverObject::ConstantRadius(solver) = self.solver(id)? else {
                return Ok(None);
            };
            let base = &solver.solver;
            let kind = match base.fsg {
                Some(FilletSurfaceGenerator::Linear) if base.chamfer.is_none() => {
                    SectionKind::Chamfer
                }
                // The sweep certificate and the group runner use exact arcs.
                // The percentage tolerance is read only for approximated arcs.
                Some(FilletSurfaceGenerator::Circular(circle))
                    if !circle.approximate_arcs && !circle.mirror && !circle.complement =>
                {
                    SectionKind::Round
                }
                _ => return Ok(None),
            };
            if common.is_some_and(|previous| previous != kind) {
                return Ok(None);
            }
            let [Some(eu), _] = base.edgeuses else {
                return Ok(None);
            };
            let Some(edge) = eu1::edge(objects.target, eu)? else {
                return Ok(None);
            };
            let size = solver.fillet_radius(0)?;
            if size != solver.fillet_radius(1)? {
                return Ok(None);
            }
            common = Some(kind);
            edges.push(edge);
            sizes.push(size);
        }
        Ok(common.map(|kind| Selection { kind, edges, sizes }))
    }
}

/// A group's fit is measured on only part of the selection, possibly after the
/// other group changed its supports. The joint plan alone can name a size for
/// the original selection. Rollback can also remove the group's reported edge.
fn without_group_fit(error: SolidError) -> SolidError {
    let SolidError::FilletRefused {
        operation,
        code,
        reason,
        ..
    } = error
    else {
        return error;
    };
    let failure = reason.map_or(BlendFailure::CornerRefused, BlendFailure::NoRoom);
    Failure::new(operation, failure)
        .with_status(code)
        .into_error()
}

fn constant_group(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    sizes: &[f64],
    section: FilletSurfaceGeneratorType,
) -> Result<()> {
    {
        let mut session = FilletSession::new(k, b)?;
        session.prepare_per_edge_constant_radius(k, edges, sizes, &[section], 1, 1.0)?;
        let tolerance = session.net_tolerance(k);
        NetScope::within_mut(k, tolerance, |k| session.do_fillet_within(k))?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_refusal_does_not_report_a_fit_on_a_reverted_edge() {
        let edge = EdgeId::from_index(42);
        let operation = "separate_ends::test_group";
        let code = crate::error::SM_ERR;
        let error = without_group_fit(SolidError::FilletRefused {
            operation,
            code,
            entity: Some(EntityRef::Edge(edge)),
            reason: Some(NoRoomReason::RailLeavesFace),
            fit: FitSize {
                size: 3.0,
                edge: Some(edge),
                kind: FitKind::Radius,
            },
        });
        assert!(matches!(
            error.blend_failure(),
            Some(BlendFailure::NoRoom(NoRoomReason::RailLeavesFace))
        ));
        assert_eq!(error.operation(), operation);
        assert_eq!(error.sm_status(), Some(code));
        assert_eq!(error.entity(), None);
        assert!(!matches!(error, SolidError::FilletRefused { .. }));
    }
}
