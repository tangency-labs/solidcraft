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
//! | `SessionObjects::rolled_rails_on_their_faces`, `rail_within_face`, `rail_clear_of` (private) | none |
//!
//! The sections only propose such a rollover: it is accepted after the trace, once each rolled
//! rail, as trimmed, is proved on the face beyond (`rail_within_face`: a point of it on the
//! face, and the whole rail farther than the solver's fit tolerance and twice the face's
//! largest edge tolerance (each at least the body's tolerance) from every other face across an
//! edge of the face, by the certified distance bounds of [`crate::query::distance_bound`] on
//! the rail's Bézier pieces), or the blend is refused.
//!
//! Deviations: none of upstream's; there is no recursion (the rail's pieces wait on a stack, at
//! most `RAIL_PIECES` of them for each face, each halved at most `RAIL_DEPTH` times).

use super::ball_regular::ball_section_at;
use super::{FilletObject, FilletSession, SessionObjects, SolverObject};
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
use crate::math::{Point2, Point3, EFF_ZERO};
use crate::surface::offset_surface::SS_UNKNOWN;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_executive::session::tangent_rollover";

/// A stage of the rollover that failed with no reason the kernel names (a defect,
/// [`crate::error::blend::BlendFailure::StageFailed`]): a solver the session does not hold.
/// Its origin is the caller's line.
#[track_caller]
fn stage_failed() -> SolidError {
    SolidError::blend(OP, crate::error::blend::BlendFailure::StageFailed)
}

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
                let Some(solver) = self
                    .objects
                    .solvers
                    .get_mut(id.0 as usize)
                    .and_then(Option::as_mut)
                else {
                    return Err(stage_failed());
                };
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
/// (each solved from the one before as its fallback guess), or `None` where one is not found
/// or is not the ball of radii `offsets` there ([`is_ball_at`]).
fn sections(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    offsets: &[f64; 2],
    curve: &Curve,
    ts: &[f64],
) -> Result<Option<Vec<TsectPnt>>> {
    let mut out: Vec<TsectPnt> = Vec::new();
    for &t in ts {
        let Some(section) = ball_section_at(k, target, solver, curve, t, out.last())? else {
            return Ok(None);
        };
        if !is_ball_at(k, target, solver, offsets, curve, t, &section)? {
            return Ok(None);
        }
        out.push(section);
    }
    Ok(Some(out))
}

/// Whether `section`, solved by `solver` at `t` along `curve` (its filleted edge's curve), is
/// the ball there: the two sides' offset points at its contacts (the ball's centre as each
/// side's offset gives it) agree, the centre lies on the plane through the edge's point square
/// to the edge, and each contact lies the ball's radius `|offsets[rail]|` from the centre, all
/// within the solver's fit tolerance (at least the body's). A solve can stop where the faces'
/// offsets, extended as the trace extends them, are no longer the offsets by the ball (a side
/// round about as wide as the ball, as on #2652's twisted disc); the section it returns then
/// misses the plane and the radius, and nothing is judged from it.
///
/// # Errors
/// Those of the evaluations.
fn is_ball_at(
    k: &Kernel,
    target: &Brep,
    solver: &mut SolverObject,
    offsets: &[f64; 2],
    curve: &Curve,
    t: f64,
    section: &TsectPnt,
) -> Result<bool> {
    let pv = curve.evaluate(k, t, 1, true, false)?;
    let (Some(&point), Some(&tangent)) = (pv.first(), pv.get(1)) else {
        return Ok(false);
    };
    let length = tangent.length();
    if length.is_nan() || length <= EFF_ZERO {
        return Ok(false);
    }
    let normal = tangent.unitize()?;
    let tol = solver.base().approx_tol_3d.max(target.tolerance());
    if !tol.is_finite() {
        return Ok(false);
    }
    let mut centres = [Point3::default(); 2];
    for (rail, centre) in centres.iter_mut().enumerate() {
        let uv = section.uv_pos(side_of(rail));
        let Some(contact) = solver
            .base()
            .surface(rail)?
            .map(|o| o.surface.base_surface().evaluate_point(k, uv))
            .transpose()?
        else {
            return Ok(false);
        };
        *centre = crate::blend::fillet_solver::p2::eval_offset(k, solver.base_mut(), rail, uv)?;
        let radius = offsets.get(rail).map_or(f64::NAN, |o| o.abs());
        let miss = ((*centre - contact).length() - radius).abs();
        if miss.is_nan() || miss > tol {
            return Ok(false);
        }
    }
    let [c0, c1] = centres;
    Ok((c0 - c1).length() <= tol && (c0 - point).dot(normal).abs() <= tol)
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
/// has no curve, or a section is not found or is not the ball there, [`is_ball_at`]).
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
        if !is_ball_at(k, target, solver, &inputs.offsets, &curve, t, &section)? {
            return Ok(None);
        }
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

/// [`carried`]'s trial, which leaves the solver rolled on the faces beyond. A face beyond
/// without a surface, or an edge without a tangent, carries nothing.
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
        let Some(surface) = crate::topology::face::p2::surface(target, rollover.face)? else {
            return Ok(false);
        };
        let surface = surface.clone_for_evaluation()?;
        roll_solver_surface(k, solver.base_mut(), rail, &surface, *rollover)?;
        *support = Some(surface);
    }
    let Some(rolled) = sections(k, target, solver, &inputs.offsets, &sides.curve, &sides.ts)?
    else {
        return Ok(false);
    };
    let tolerance = target.tolerance();
    for (section, &t) in rolled.iter().zip(&sides.ts) {
        let Some(along) = sides.curve.evaluate(k, t, 1, true, false)?.get(1).copied() else {
            return Ok(false);
        };
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

/// The most pieces one rolled rail is cut into for one face's bound ([`rail_clear_of`]).
const RAIL_PIECES: usize = 512;

/// The most times one rolled rail's piece is halved ([`rail_clear_of`]).
const RAIL_DEPTH: u32 = 24;

impl SessionObjects<'_> {
    /// Whether every rail of solver `id` rolled over a smooth far edge of its face before the
    /// trace ([`rails_wholly_past`]) lies on the face beyond, proved on the rail as traced and
    /// trimmed ([`rail_within_face`]): the acceptance of that rollover, which the sections
    /// [`rails_wholly_past`] solves only propose. `false` where a rail is not proved so, also
    /// where its proof fails ([`probe`]). A lone round's rollover, decided in closed form
    /// ([`tangent_rollover_supports`]), is not taken again here.
    ///
    /// # Errors
    /// [`SolidError::Unported`] and a cancellation from the proofs; those of the lookups.
    pub(super) fn rolled_rails_on_their_faces(
        &mut self,
        k: &Kernel,
        id: FilletSolverId,
        lone: bool,
    ) -> Result<bool> {
        if lone {
            return Ok(true);
        }
        let (rolled, geoms, fit) = {
            let base = self.solver(id)?.base();
            (base.rolled_faces, base.fillet_geoms(), base.approx_tol_3d)
        };
        if rolled.iter().all(Option::is_none) {
            return Ok(true);
        }
        for g in geoms {
            for (which, onto) in rolled.iter().enumerate() {
                let Some(onto) = *onto else {
                    continue;
                };
                let Some(rail) = self.pseudo.geom(g)?.rail(which) else {
                    return Ok(false);
                };
                let within = rail_within_face(k, self.target, self.pseudo.brep(), rail, onto, fit);
                if probe(within)? != Some(true) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

/// Whether the rail edge `rail` of the fillet B-rep `fillets` lies on face `face` of `target`:
/// a point of it (its middle, dropped on the face's surface) classifies on the face, and the
/// rail is farther than `fit + 2 zone` from every other face across an edge of the face
/// ([`rail_clear_of`]). `fit` is the solver's fit tolerance, how far the traced rail may lie
/// from the face's surface; `zone` the largest tolerance of the face's edges, how far an
/// edge's curve may lie from either face it bounds; each is at least `target`'s tolerance. A
/// rail that left the face would cross its boundary in the surface: there the rail is within
/// `fit` of a point of the face's boundary, which is within `zone` of an edge's curve, which
/// is within `zone` of the face across it. So the rail crosses no edge of the face: it lies
/// on the face, all of it. A rail without a curve or whose curve has no B-spline form this
/// proof reads, a face without a surface, or a rail that comes as near another face (one it ends on, a face continued past its
/// boundary by its surface over the face's UV box), is not proved.
///
/// # Errors
/// Those of the lookups, the evaluations, the drop, the classification and the bounds.
fn rail_within_face(
    k: &Kernel,
    target: &mut Brep,
    fillets: &Brep,
    rail: EdgeId,
    face: FaceId,
    fit: f64,
) -> Result<bool> {
    let Some(curve) = edge::curve(fillets, rail)?.cloned() else {
        return Ok(false);
    };
    let ivl = edge::interval(fillets, rail)?;
    let tolerance = target.tolerance();
    let middle = curve.evaluate_point(k, ivl.evaluate(0.5))?;
    let Some(surface) = crate::topology::face::p2::surface(target, face)? else {
        return Ok(false);
    };
    let surface = surface.clone_for_evaluation()?;
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped = surface.drop_point::<crate::surface::surface::p4::NoObject>(
        k,
        middle,
        &domain,
        &crate::surface::surface::p5::DropPointOptions::default(),
    )?;
    if !dropped.success
        || classify(k, target, face, dropped.result_uv, tolerance)? != PointClassType::Face
    {
        return Ok(false);
    }
    let Some(pieces) = rail_pieces(&curve, ivl.min(), ivl.max())? else {
        return Ok(false);
    };
    let mut others: Vec<FaceId> = Vec::new();
    let mut zone = tolerance;
    for e in face_p4::edges(target, face, None, false, None)? {
        let edge_zone = edge::tolerance(target, e)?;
        if !edge_zone.is_finite() {
            return Ok(false);
        }
        zone = zone.max(edge_zone);
        for f in edge::faces(target, e, None)? {
            if f != face && !others.contains(&f) {
                others.push(f);
            }
        }
    }
    if !fit.is_finite() {
        return Ok(false);
    }
    // rounded up: the sum and the product, gamma_2
    let gap = (fit.max(tolerance) + 2.0 * zone) * (1.0 + crate::nurbs::math::gamma(2));
    for other in others {
        if !rail_clear_of(k, target, other, &pieces, gap)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A Bézier piece of a rail: its control points in homogeneous coordinates `(w x, w y, w z,
/// w)`, each coordinate within `err` of the exact curve's, and how often it was halved.
#[derive(Debug, Clone)]
struct RailPiece {
    /// The homogeneous control points.
    h: Vec<[f64; 4]>,
    /// The bound on every coordinate's error.
    err: f64,
    /// How often it was halved.
    depth: u32,
}

/// The Bézier pieces of `curve` between its parameters `lo` and `hi`, from its NURB (a
/// B-spline curve's parameter is its NURB's): the segment extracted and decomposed by knot
/// insertion. Each coefficient goes through at most `p` insertions at each end of its piece,
/// each `alpha P + (1 - alpha) Q` within `8 u M` of exact for `|P|, |Q| <= M` (as
/// [`crate::query::distance_bound`] counts them), so every coordinate is within `gamma_16(p +
/// 1)` of the NURB's largest. `None` for a curve of another kind, without its NURB, in 2D, or
/// over an interval outside its knots.
///
/// # Errors
/// Those of the extraction and the decomposition.
fn rail_pieces(curve: &Curve, lo: f64, hi: f64) -> Result<Option<Vec<RailPiece>>> {
    let crate::curve::curve::CurveKind::BSpline(base) = curve.kind() else {
        return Ok(None);
    };
    let Some(nurb) = base.nurb.as_ref() else {
        return Ok(None);
    };
    let (Some(&first), Some(&last)) = (nurb.knots().first(), nurb.knots().last()) else {
        return Ok(None);
    };
    if !(first <= lo && lo < hi && hi <= last) {
        return Ok(None);
    }
    let segment = if first < lo || hi < last {
        nurb.extract_seg(lo, hi)?
    } else {
        nurb.clone()
    };
    let largest = nurb
        .cpts()
        .iter()
        .flat_map(|c| {
            let w = if c.w == crate::nurbs::math::NOW {
                1.0
            } else {
                c.w
            };
            [c.x.abs(), c.y.abs(), c.z.abs(), w.abs()]
        })
        .fold(0.0_f64, f64::max);
    let err = crate::nurbs::math::gamma(16 * (nurb.degree() + 1)) * largest;
    let mut pieces = Vec::new();
    for bezier in segment.decompose_bez()? {
        let mut h = Vec::new();
        for c in bezier.cpts() {
            if c.z == crate::nurbs::math::NOZ {
                return Ok(None);
            }
            let w = if c.w == crate::nurbs::math::NOW {
                1.0
            } else {
                c.w
            };
            h.push([c.x, c.y, c.z, w]);
        }
        pieces.push(RailPiece { h, err, depth: 0 });
    }
    Ok(Some(pieces))
}

impl RailPiece {
    /// A ball holding the piece: a centre, and a radius at least the distance of every point of
    /// the piece from it. With positive weights the piece lies in the hull of its Euclidean
    /// control points; each computed point is within `widen` of the exact one in each
    /// coordinate (the quotient of coefficients within `err`, and its own rounding, as
    /// [`crate::query::distance_bound`] widens its boxes), so within `2 widen` in length. The
    /// centre is the middle of the points' box, the radius their farthest distance from it,
    /// rounded up, plus that. `None` where the weights are not proved positive or a value is
    /// not finite.
    fn ball(&self) -> Option<(Point3, f64)> {
        use crate::nurbs::math::gamma;
        let w_min = self.h.iter().map(|c| c[3]).fold(f64::INFINITY, f64::min);
        if !crate::geom::certified::above(w_min - self.err, 0.0) {
            return None;
        }
        let points: Vec<[f64; 3]> = self
            .h
            .iter()
            .map(|c| [c[0] / c[3], c[1] / c[3], c[2] / c[3]])
            .collect();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut size = 0.0_f64;
        for e in &points {
            for (axis, &v) in e.iter().enumerate() {
                size = size.max(v.abs());
                if let (Some(l), Some(u)) = (lo.get_mut(axis), hi.get_mut(axis)) {
                    *l = l.min(v);
                    *u = u.max(v);
                }
            }
        }
        let centre = Point3::new(
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        );
        let mut far = 0.0_f64;
        for e in &points {
            let d = [e[0] - centre.x, e[1] - centre.y, e[2] - centre.z];
            far = far.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
        }
        let widen = self.err * (1.0 + size) / (w_min - self.err) + gamma(2) * size;
        // the differences, squares, sum and root: within gamma_5 of the size, twice
        let radius = far * (1.0 + gamma(5)) + gamma(5) * 2.0 * size + 2.0 * widen;
        let finite = radius.is_finite() && centre.x.is_finite() && centre.y.is_finite();
        (finite && centre.z.is_finite()).then_some((centre, radius))
    }

    /// Its halves, by one de Casteljau level at the middle: each coordinate one more convex
    /// combination of its neighbours per level, within `gamma` of twice the degree plus two of
    /// the largest (as [`crate::query::distance_bound`] counts a subdivision).
    fn halves(&self) -> (Self, Self) {
        let degree = self.h.len().saturating_sub(1);
        let largest = self
            .h
            .iter()
            .flat_map(|c| c.iter().map(|v| v.abs()))
            .fold(0.0_f64, f64::max);
        let err = self.err + crate::nurbs::math::gamma(2 * degree + 2) * (largest + self.err);
        let mut work = self.h.clone();
        let mut left = Vec::new();
        let mut right = Vec::new();
        if let Some(&first) = work.first() {
            left.push(first);
        }
        if let Some(&last) = work.last() {
            right.push(last);
        }
        for level in 1..=degree {
            for i in 0..=(degree - level) {
                if let (Some(&a), Some(&b)) = (work.get(i), work.get(i + 1)) {
                    if let Some(slot) = work.get_mut(i) {
                        *slot = [
                            0.5 * (a[0] + b[0]),
                            0.5 * (a[1] + b[1]),
                            0.5 * (a[2] + b[2]),
                            0.5 * (a[3] + b[3]),
                        ];
                    }
                }
            }
            if let Some(&first) = work.first() {
                left.push(first);
            }
            if let Some(&last) = work.get(degree - level) {
                right.push(last);
            }
        }
        right.reverse();
        let depth = self.depth + 1;
        (
            Self {
                h: left,
                err,
                depth,
            },
            Self {
                h: right,
                err,
                depth,
            },
        )
    }
}

/// Whether every point of the rail's `pieces` is farther than `gap` from face `face` of
/// `target`: for a piece in the ball of centre `c` and radius `r` ([`RailPiece::ball`]), the
/// face's certified lower bound of its distance from `c`
/// ([`crate::query::distance_bound::face_distance_lower_bound`]) less `r`, rounded down, is
/// over `gap`, or the piece's halves are. At most [`RAIL_PIECES`] pieces are bounded and a
/// piece is halved at most [`RAIL_DEPTH`] times; past either the rail is not proved clear.
///
/// # Errors
/// Those of the bound; a cancellation.
fn rail_clear_of(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    pieces: &[RailPiece],
    gap: f64,
) -> Result<bool> {
    use crate::nurbs::math::gamma;
    let mut stack: Vec<RailPiece> = pieces.iter().rev().cloned().collect();
    let mut bounded = 0;
    while let Some(piece) = stack.pop() {
        k.check_cancelled(OP)?;
        bounded += 1;
        if bounded > RAIL_PIECES {
            return Ok(false);
        }
        let Some((centre, radius)) = piece.ball() else {
            return Ok(false);
        };
        let want = (radius + gap) * (1.0 + gamma(2));
        let lower =
            crate::query::distance_bound::face_distance_lower_bound(k, target, face, centre, want)?;
        let clear = (lower - radius) * (1.0 - gamma(1));
        if clear > gap {
            continue;
        }
        if piece.depth >= RAIL_DEPTH {
            return Ok(false);
        }
        let (left, right) = piece.halves();
        stack.push(right);
        stack.push(left);
    }
    Ok(true)
}
