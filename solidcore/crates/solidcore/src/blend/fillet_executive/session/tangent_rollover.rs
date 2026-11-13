//! A round rolls over a smooth far edge of its face, the whole length (not upstream;
//! solidcore-native): before its trace, a round's solver rolls its ball on the face beyond on
//! each side where the ball's contact lies past the face's smooth far edge all along the
//! round's edge. A lone round is decided in closed form, between planes along a straight edge
//! only ([`crate::blend::fillet_tangent_rollover::tangent_rollover_supports`]); a round of a
//! blend of several, such as one of a chain, from its ball's sections along its edge
//! (`rails_wholly_past`, upstream's unimplemented tangent case of
//! `SmFilletSolver::ProcessFullRollOver`).
//!
//! | Rust                                     | Upstream |
//! |------------------------------------------|----------|
//! | `FilletSession::roll_over_tangent_edges` | none     |
//! | `rails_wholly_past`, `sides_wholly_off`, `carried` (private) | none (the tangent case `ProcessFullRollOver` refuses as "Unimplemented Tangent Rollover Case") |
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
    /// lies on. A lone round is decided in closed form, between planes along a straight edge
    /// only ([`tangent_rollover_supports`]); each round of a blend of several, such as one of a
    /// chain, from its ball's sections ([`rails_wholly_past`]). A rail that lies past the edge
    /// only partway is left to the rollover recovery after the trace (`ProcessRollOver`).
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
            let rollovers = if lone {
                tangent_rollover_supports(
                    k,
                    &mut *self.objects.target,
                    inputs.edge,
                    inputs.faces,
                    inputs.offsets,
                    inputs.tangency,
                )?
            } else {
                let solver = self
                    .objects
                    .solvers
                    .get_mut(id.0 as usize)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| SolidError::blend_missing(OP, "the live solver"))?;
                rails_wholly_past(k, &mut *self.objects.target, solver, &inputs)?
            };
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

/// A probe's result: a failure (a solve, a classification or a query that errs) finds nothing,
/// and only [`SolidError::Unported`] and a cancellation are passed on.
fn probe<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// The faces `solver`'s ball rolls onto across a smooth far edge of the face on each side, the
/// whole length of its edge, or `None` on a side where it touches its own face.
///
/// The ball's section is solved at the ends of the edge and [`SAMPLES`] - 1 points between
/// ([`ball_section_at`], on the faces' surfaces extended as the trace extends them). A side
/// rolls over when its contact lies off its face (`inputs.faces`) at every one, and some edge of
/// that face, tangent to the face beyond within `inputs.tangency`
/// (`sm_TestManifoldTangentRollover`), has a face beyond that carries the ball at every one
/// ([`carried`]). The first such edge in the face's edge order is taken. Where both sides roll
/// over, the ball rolled on both faces beyond together must be carried too, or neither side
/// rolls. The ball is solved only while a side with a tangent edge may still roll over (none
/// where neither face has one), a probe that fails finds no rollover ([`probe`]), and the solver
/// is as before after.
///
/// # Errors
/// [`SolidError::Unported`] and a cancellation from the probes; those of restoring the solver
/// after a trial.
fn rails_wholly_past(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    inputs: &Inputs,
) -> Result<[Option<Rollover>; 2]> {
    let none = [None, None];
    let Some(Some(sides)) = probe(sides_wholly_off(k, target, solver, inputs))? else {
        return Ok(none);
    };
    let mut out = none;
    for (rail, (slot, found)) in out.iter_mut().zip(&sides.candidates).enumerate() {
        if sides.wholly_off.get(rail) != Some(&true) {
            continue;
        }
        for &rollover in found {
            let mut trial = none;
            if let Some(side) = trial.get_mut(rail) {
                *side = Some(rollover);
            }
            if carried(k, target, solver, inputs, &sides, trial)? {
                *slot = Some(rollover);
                break;
            }
        }
    }
    if out.iter().all(Option::is_some) && !carried(k, target, solver, inputs, &sides, out)? {
        return Ok(none);
    }
    Ok(out)
}

/// The sides of a round whose ball's contact lies off its face at every section, with each
/// side's candidate edges to roll over.
struct Sides {
    /// The filleted edge's curve.
    curve: Curve,
    /// The edge parameters of the sections.
    ts: Vec<f64>,
    /// Each side's edges of its face but the filleted one, tangent to the face beyond within
    /// the tangency tolerance, in the face's edge order.
    candidates: [Vec<Rollover>; 2],
    /// Whether each side's contact lies off its face at every section.
    wholly_off: [bool; 2],
}

/// [`rails_wholly_past`]'s first probe: each side's candidates, and whether its contact lies
/// off its face at every section; `None` where no side with a candidate has it so (or the edge
/// has no curve, or a section is not found).
///
/// # Errors
/// Those of the section solves, the classifications and the topology queries.
fn sides_wholly_off(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    inputs: &Inputs,
) -> Result<Option<Sides>> {
    let Some(curve) = edge::evaluation_curve(target, inputs.edge)? else {
        return Ok(None);
    };
    let ivl = edge::interval(target, inputs.edge)?;
    let ts: Vec<f64> = (0..=SAMPLES)
        .map(|i| ivl.evaluate(f64::from(i) / f64::from(SAMPLES)))
        .collect();
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
        return Ok(None);
    }
    let tolerance = target.tolerance();
    // the ball's own sections, solved one at a time while a side with candidates may still
    // have its contact off its face at every one
    let mut wholly_off = candidates.each_ref().map(|found| !found.is_empty());
    let mut previous: Option<TsectPnt> = None;
    for &t in &ts {
        let Some(section) = ball_section_at(k, target, solver, &curve, t, previous.as_ref())?
        else {
            return Ok(None);
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
            return Ok(None);
        }
        previous = Some(section);
    }
    Ok(Some(Sides {
        curve,
        ts,
        candidates,
        wholly_off,
    }))
}

/// Whether the faces beyond carry the ball rolled on them on the sides `rollovers` names, at
/// every one of `sides`'s sections: solved with each such side's offset surface on its face
/// beyond's surface (its sign following the normals across the edge), each such side's contact
/// lies on that face, and that face's offset by the ball does not fold there ([`offset_folds`]).
/// A trial that fails is not carried ([`probe`]). The solver is as before after.
///
/// # Errors
/// [`SolidError::Unported`] and a cancellation from the trial; those of saving and restoring
/// the solver's offset surfaces.
fn carried(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    inputs: &Inputs,
    sides: &Sides,
    rollovers: [Option<Rollover>; 2],
) -> Result<bool> {
    let saved = solver.base().offsets_data()?;
    let saved_rolled = solver.base().rolled_faces;
    let trial = probe(carried_rolled(k, target, solver, inputs, sides, rollovers));
    solver.base_mut().set_offsets_data(&saved)?;
    solver.base_mut().rolled_faces = saved_rolled;
    Ok(trial?.unwrap_or(false))
}

/// [`carried`]'s trial, which leaves the solver rolled on the faces beyond.
///
/// # Errors
/// Those of the rolls, the section solves, the classifications, the evaluations and
/// [`offset_folds`].
fn carried_rolled(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    inputs: &Inputs,
    sides: &Sides,
    rollovers: [Option<Rollover>; 2],
) -> Result<bool> {
    let mut supports = [None, None];
    for (rail, (rollover, support)) in rollovers.iter().zip(supports.iter_mut()).enumerate() {
        let Some(rollover) = rollover else {
            continue;
        };
        let surface = crate::topology::face::p2::surface(target, rollover.face)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?
            .clone_for_evaluation()?;
        roll_solver_surface(k, solver.base_mut(), rail, &surface, *rollover)?;
        *support = Some(surface);
    }
    let Some(rolled) = sections(k, target, solver, &sides.curve, &sides.ts)? else {
        return Ok(false);
    };
    let tolerance = target.tolerance();
    for (section, &t) in rolled.iter().zip(&sides.ts) {
        let along = sides
            .curve
            .evaluate(k, t, 1, true, false)?
            .get(1)
            .copied()
            .ok_or_else(|| SolidError::blend_missing(OP, "the edge's tangent"))?;
        for (rail, (rollover, support)) in rollovers.iter().zip(&supports).enumerate() {
            let (Some(rollover), Some(support), Some(&offset)) =
                (rollover, support, inputs.offsets.get(rail))
            else {
                continue;
            };
            let uv = section.uv_pos(side_of(rail));
            if off(classify(k, target, rollover.face, uv, tolerance)?) {
                return Ok(false);
            }
            let contact = support.evaluate_point(k, uv)?;
            let offset_beyond = if rollover.opposite_normals {
                -offset
            } else {
                offset
            };
            if offset_folds(k, target, rollover.face, contact, along, offset_beyond)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
