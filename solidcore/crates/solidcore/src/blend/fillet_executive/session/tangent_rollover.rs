//! A round rolls over a smooth far edge of its face, the whole length (not upstream;
//! solidcore-native): before its trace, a round's solver rolls its ball on the face beyond on
//! each side where the ball's contact lies past the face's smooth far edge all along the
//! round's edge. A lone round between planes along a straight edge is decided in closed form
//! ([`crate::blend::fillet_tangent_rollover::tangent_rollover_supports`]); any other round,
//! lone or one of a chain, from its ball's sections along its edge
//! (`rails_wholly_past`, upstream's unimplemented tangent case of
//! `SmFilletSolver::ProcessFullRollOver`).
//!
//! | Rust                                     | Upstream |
//! |------------------------------------------|----------|
//! | `FilletSession::roll_over_tangent_edges` | none     |
//! | `rails_wholly_past` (private)            | none (the tangent case `ProcessFullRollOver` refuses as "Unimplemented Tangent Rollover Case") |
//!
//! Deviations: none of upstream's; there is no recursion.

use super::ball_regular::ball_section_at;
use super::{FilletObject, FilletSession, SolverObject};
use crate::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use crate::blend::fillet_geom::p1::FilletSolverId;
use crate::blend::fillet_solver::p1::{
    test_manifold_tangent_rollover, FilletSolver, FilletSurfaceGenerator,
    FilletSurfaceGeneratorType, OffsetSurfaceSlot,
};
use crate::blend::fillet_tangent_rollover::{offset_folds, tangent_rollover_supports, Rollover};
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::surface::offset_surface::SS_UNKNOWN;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_executive::session::tangent_rollover";

/// The intervals a round's edge is cut into to tell where its ball's contacts lie: the ball's
/// section is solved at each cut, the edge's ends among them.
const SAMPLES: u32 = 16;

/// What a solver rolls over with: its filleted edge, the face of each rail's edge use, each
/// rail's signed offset and the tangency tolerance.
struct Inputs {
    edge: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
    tangency: f64,
}

impl FilletSession<'_> {
    /// A constant round of circular section whose ball's contact lies past a smooth far edge
    /// of a face beside it, all along its edge, rolls on the face beyond: that side's solver
    /// offset surface is replaced by one of the same offset (its sign following the faces'
    /// normals across the edge) on the face beyond's surface, owned by that face, in the solver
    /// and in its fillet geometries, so the rail is traced, classified and trimmed on the face it
    /// lies on. A lone round between planes along a straight edge is decided in closed form
    /// ([`tangent_rollover_supports`]); every other round of the blend, lone or one of a chain,
    /// from its ball's sections ([`rails_wholly_past`]). A rail that lies past the edge only
    /// partway is left to the rollover recovery after the trace (`ProcessRollOver`).
    ///
    /// # Errors
    /// Those of the topology queries, of [`tangent_rollover_supports`] and of
    /// [`rails_wholly_past`].
    pub(super) fn roll_over_tangent_edges(&mut self, k: &Kernel) -> Result<()> {
        let lone = self.executive.solvers.len() == 1;
        let ids: Vec<FilletSolverId> = self
            .executive
            .solvers
            .iter()
            .filter_map(|object| match object {
                FilletObject::Solver(id) => Some(*id),
                _ => None,
            })
            .collect();
        for id in ids {
            let Some(inputs) = self.rollover_inputs(id)? else {
                continue;
            };
            let target = &mut *self.objects.target;
            let mut rollovers = if lone {
                tangent_rollover_supports(
                    k,
                    target,
                    inputs.edge,
                    inputs.faces,
                    inputs.offsets,
                    inputs.tangency,
                )?
            } else {
                [None, None]
            };
            if rollovers.iter().all(Option::is_none) {
                let solver = self
                    .objects
                    .solvers
                    .get_mut(id.0 as usize)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| SolidError::blend_missing(OP, "the live solver"))?;
                rollovers = rails_wholly_past(k, &mut *self.objects.target, solver, &inputs)?;
            }
            for (rail, rollover) in rollovers.into_iter().enumerate() {
                if let Some(rollover) = rollover {
                    self.roll_rail_over(k, id, rail, rollover)?;
                }
            }
        }
        Ok(())
    }

    /// The edge, faces, offsets and tangency of solver `id` when it is a constant round of
    /// circular section with an edge use on each side, else `None`.
    fn rollover_inputs(&self, id: FilletSolverId) -> Result<Option<Inputs>> {
        let object = self.objects.solver(id)?;
        let base = object.base();
        let circular = base
            .fsg
            .as_ref()
            .map(FilletSurfaceGenerator::fillet_surface_generator_type)
            == Some(FilletSurfaceGeneratorType::Circular);
        if !matches!(object, SolverObject::ConstantRadius(_)) || !circular {
            return Ok(None);
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
        let target = &*self.objects.target;
        let [Some(u0), Some(u1)] = uses else {
            return Ok(None);
        };
        let Some(edge) = eu1::edge(target, u0)? else {
            return Ok(None);
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
        Ok(Some(Inputs {
            edge,
            faces,
            offsets,
            tangency,
        }))
    }

    /// Solver `id`'s rail `rail` rolls on `rollover.face`: its offset surface there, in the
    /// solver and in its fillet geometries, of the same offset, its sign (and the solver's
    /// orientation on that side) following the faces' normals across the edge.
    fn roll_rail_over(
        &mut self,
        k: &Kernel,
        id: FilletSolverId,
        rail: usize,
        rollover: Rollover,
    ) -> Result<()> {
        let geoms = self.objects.solver(id)?.base().fillet_geoms();
        let support = crate::topology::face::p2::surface(self.objects.target, rollover.face)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?
            .clone_for_evaluation()?;
        let solver = self.objects.solver_mut(id)?.base_mut();
        roll_solver_surface(k, solver, rail, &support, rollover)?;
        let sign = if rollover.opposite_normals { -1.0 } else { 1.0 };
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
        Ok(())
    }
}

/// `solver`'s offset surface on side `rail` replaced by one of the same offset on `support`,
/// the surface of `rollover.face`, owned by that face (its sign, and the solver's orientation on
/// that side, turned where the faces' normals are opposite across the edge), and that face
/// noted as the one the rail rolls on.
fn roll_solver_surface(
    k: &Kernel,
    solver: &mut FilletSolver,
    rail: usize,
    support: &crate::surface::surface::Surface,
    rollover: Rollover,
) -> Result<()> {
    let sign = if rollover.opposite_normals { -1.0 } else { 1.0 };
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
    Ok(())
}

/// The ball's sections of `solver` at each of `ts` along `curve`, its filleted edge's curve
/// (each solved from the one before as its fallback guess), or `None` where one is not found.
fn sections(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    curve: &Curve,
    ts: &[f64],
) -> Result<Option<Vec<TsectPnt>>> {
    let mut out: Vec<TsectPnt> = Vec::new();
    for &t in ts {
        let Some(section) = ball_section_at(k, target, solver, curve, t, out.last())? else {
            return Ok(None);
        };
        out.push(section);
    }
    Ok(Some(out))
}

/// How `uv` on `face`'s surface classifies against the face, within `tolerance`: in it, on its
/// boundary, or off it (`Unknown` or `Nothing`).
fn classify(
    k: &Kernel,
    target: &mut Brep,
    face: FaceId,
    uv: Point2,
    tolerance: f64,
) -> Result<PointClassType> {
    let mut class = PointClassification::<EntityRef>::new(tolerance);
    crate::query::face::p2::point_classify(
        k,
        target,
        face,
        uv,
        &mut class,
        crate::query::face::p2::PointClassifyOptions {
            src_zone_tol_3d: tolerance,
            do_boundary_intersections: true,
            use_uv_classification: true,
            ..crate::query::face::p2::PointClassifyOptions::default()
        },
    )?;
    Ok(class.point_class())
}

/// The section side of rail `rail` (0 the first).
fn side_of(rail: usize) -> SrfIndex {
    if rail == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    }
}

/// Whether a point classified `class` lies off the face.
fn off(class: PointClassType) -> bool {
    matches!(class, PointClassType::Unknown | PointClassType::Nothing)
}

/// The faces `solver`'s ball rolls onto across a smooth far edge of the face on each side, the
/// whole length of its edge, or `None` on a side where it touches its own face.
///
/// The ball's section is solved at the ends of the edge and [`SAMPLES`] - 1 points between
/// ([`ball_section_at`], on the faces' surfaces extended as the trace extends them). A side
/// rolls over when its contact lies off its face (`inputs.faces`) at every one, and some edge of
/// that face, tangent to the face beyond within `inputs.tangency`
/// (`sm_TestManifoldTangentRollover`), has a face beyond that carries the ball at every one:
/// solved with that side's offset surface on the face beyond's surface (its sign following the
/// normals across the edge), the contact lies on that face, and the face's offset by the ball
/// does not fold there ([`offset_folds`]). The first such edge in the face's edge order is taken.
/// The ball is solved only while a side with a tangent edge may still roll over (none where
/// neither face has one), and the solver is as before after.
///
/// # Errors
/// Those of the section solves, the classifications, the topology queries and the tangency test.
fn rails_wholly_past(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    inputs: &Inputs,
) -> Result<[Option<Rollover>; 2]> {
    let none = [None, None];
    let Some(curve) = edge::evaluation_curve(target, inputs.edge)? else {
        return Ok(none);
    };
    let ivl = edge::interval(target, inputs.edge)?;
    let ts: Vec<f64> = (0..=SAMPLES)
        .map(|i| ivl.evaluate(f64::from(i) / f64::from(SAMPLES)))
        .collect();
    // each side's candidates: the edges of its face but the filleted one, tangent to the face
    // beyond within the tangency tolerance (`sm_TestManifoldTangentRollover`), in the face's
    // edge order
    let mut candidates: [Vec<Rollover>; 2] = [Vec::new(), Vec::new()];
    for (found, &face) in candidates.iter_mut().zip(&inputs.faces) {
        for e in face_p4::edges(target, face, None, false, None)? {
            if e == inputs.edge {
                continue;
            }
            let (tangent, opposite_normals) =
                test_manifold_tangent_rollover(k, target, e, inputs.tangency)?;
            if !tangent {
                continue;
            }
            let Some(&beyond) = edge::faces(target, e, None)?.iter().find(|&&f| f != face) else {
                continue;
            };
            found.push(Rollover {
                edge: e,
                face: beyond,
                opposite_normals,
            });
        }
    }
    if candidates.iter().all(Vec::is_empty) {
        return Ok(none);
    }
    let tolerance = target.tolerance();
    // the ball's own sections, solved one at a time while a side with candidates may still
    // have its contact off its face at every one
    let mut wholly_off = candidates.each_ref().map(|found| !found.is_empty());
    let mut own: Vec<TsectPnt> = Vec::new();
    for &t in &ts {
        let Some(section) = ball_section_at(k, target, solver, &curve, t, own.last())? else {
            return Ok(none);
        };
        for (rail, (off_face, &face)) in wholly_off.iter_mut().zip(&inputs.faces).enumerate() {
            if *off_face {
                *off_face = off(classify(
                    k,
                    target,
                    face,
                    section.uv_pos(side_of(rail)),
                    tolerance,
                )?);
            }
        }
        if wholly_off.iter().all(|&w| !w) {
            return Ok(none);
        }
        own.push(section);
    }
    let mut out = none;
    for (rail, slot) in out.iter_mut().enumerate() {
        let side = side_of(rail);
        let (Some(&offset), Some(true), Some(found)) = (
            inputs.offsets.get(rail),
            wholly_off.get(rail).copied(),
            candidates.get(rail),
        ) else {
            continue;
        };
        for &rollover in found {
            let beyond = rollover.face;
            let opposite_normals = rollover.opposite_normals;
            let support = crate::topology::face::p2::surface(target, beyond)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?
                .clone_for_evaluation()?;
            // the ball rolled on the face beyond, then the solver as it was
            let saved = solver.base().offsets_data()?;
            let saved_rolled = solver.base().rolled_faces;
            roll_solver_surface(k, solver.base_mut(), rail, &support, rollover)?;
            let rolled = sections(k, target, solver, &curve, &ts);
            solver.base_mut().set_offsets_data(&saved)?;
            solver.base_mut().rolled_faces = saved_rolled;
            let Some(rolled) = rolled? else {
                continue;
            };
            let offset_beyond = if opposite_normals { -offset } else { offset };
            let mut carried = true;
            for (section, &t) in rolled.iter().zip(&ts) {
                if off(classify(
                    k,
                    target,
                    beyond,
                    section.uv_pos(side),
                    tolerance,
                )?) {
                    carried = false;
                    break;
                }
                let contact = support.evaluate_point(k, section.uv_pos(side))?;
                let along = curve
                    .evaluate(k, t, 1, true, false)?
                    .get(1)
                    .copied()
                    .ok_or_else(|| SolidError::blend_missing(OP, "the edge's tangent"))?;
                if offset_folds(k, target, beyond, contact, along, offset_beyond)? {
                    carried = false;
                    break;
                }
            }
            if carried {
                *slot = Some(rollover);
                break;
            }
        }
    }
    Ok(out)
}
