//! The largest size that builds a blend refused as too large for its faces: the ball
//! through a boundary point, the size at which rails stay on their faces, where two bands
//! on a face meet, and the size that leaves some of a face the blends take whole.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork (tangency-labs/smlib#79,
//! merged at 5e8c9c1) adds `sm_BallThrough`, `SmFilletSolver::BallThrough`,
//! `LargestSizeOnItsFaces` and `BandsMeetAt` (`SmFilletSolver.cpp`, `SmFilletSolver.h`) and
//! the executive's `NoteFitOfPolygonsTakenWhole` and the circle-top fit of
//! `CreateFilletBrep` (`SmFilletExecutive.cpp`) in commits a3d6d81 and 49f393d; 5e8c9c1
//! brackets each bound strictly inside its search (twice the blend's own size).
//!
//! | Rust                          | Upstream (fork)                                  |
//! |-------------------------------|--------------------------------------------------|
//! | [`ball_through`]              | `sm_BallThrough`, `SmFilletSolver::BallThrough`  |
//! | [`largest_size_on_its_faces`] | `SmFilletSolver::LargestSizeOnItsFaces`          |
//! | [`bands_meet_at`]             | `SmFilletSolver::BandsMeetAt`                    |
//! | [`polygon_fit`]               | one face of `SmFilletExecutive::NoteFitOfPolygonsTakenWhole` |
//! | [`circle_top_fit`]            | the circle-top fit after `FacesOutlastTheirBlends` in `CreateFilletBrep` |
//! | [`knife_chamfer_keeps_nothing`], [`knife_chamfer_distance_through`] | not the fork's: a knife edge's chamfer that keeps nothing |
//!
//! Deviations: the fork's out-params are returned (`None` for not found). The executive's
//! loops over solvers and faces run in the session (`creation_*` in `session.rs`) and hand
//! the executive its candidates in the fork's order; it notes them as the fork does. The
//! fork's map of faces is keyed by face pointer; the port visits faces in id order (the
//! least size is noted either way; ties keep the first face's solver). Every loop is bounded by fixed counts (32 steps, 60 bisections,
//! 60 or 40 golden-section steps, halvings down to a millionth of the radius); there is no
//! recursion. The knife chamfer's limit is a closed form over the body's vertices, line ends
//! and circle points, one pass over its faces and edges.

// the fork's NaN-safe tests (`!(a > b)`) are kept as written
#![allow(clippy::neg_cmp_op_on_partial_ord)]
use crate::blend::fillet_executive::session::ball_regular::{
    ball_section_at, ops_of, rolls_a_ball, set_ball_radius,
};
use crate::blend::fillet_executive::session::SolverObject;
use crate::blend::fillet_executive::FitKind;
use crate::blend::fillet_solver::p1::{test_manifold_tangent_rollover, FilletSolverType};
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{edge, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "blend::fillet_executive::session::fit_size";

/// `lSteps`: the steps along an edge or the search.
const STEPS: usize = 32;

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

/// The state of `sm_BallThrough`'s `h`: the solver's base surfaces, their domains, the
/// guess for the other face's foot and that foot.
struct Through {
    bases: [Surface; 2],
    domains: [Extent2d; 2],
    radii: [f64; 2],
    guess: Option<Point2>,
    foot_at: Point3,
}

impl Through {
    /// `Toward`: the point of face `k`'s surface at `uv` (past the base's domain, on its
    /// extension) and the normal toward the ball there, `(offset - base) / r`.
    fn toward(
        &self,
        k: &Kernel,
        solver: &mut SolverObject,
        rail: usize,
        uv: Point2,
    ) -> Result<(Point3, Vec3)> {
        let base = self
            .bases
            .get(rail)
            .ok_or_else(|| SolidError::blend_missing(OP, "the rail's base surface"))?;
        let mut on = base.evaluate_point(k, uv)?;
        let slot = solver
            .base_mut()
            .surface_mut(rail)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?;
        let off = slot.surface.surface.evaluate_point_extending(k, uv)?;
        if let Some(extended) = slot.surface.surface.extended_base_surface() {
            if !base.natural_uv_domain_with_nurb(k)?.contains_point(uv, 0.0)
                && extended
                    .natural_uv_domain_with_nurb(k)?
                    .contains_point(uv, 0.0)
            {
                on = extended.evaluate_point(k, uv)?;
            }
        }
        let r = self.radii.get(rail).copied().unwrap_or(0.0);
        Ok((on, (off - on) / r))
    }
}

/// `sm_BallThrough(rSolver, lK, crQ, dMax, ..., pdRatio, pdChord)`: the radius `rho` of the
/// ball of this solver's kind that touches its face `lk` at `q` and its other face, the
/// least in `(0, max]` (the faces' offsets `rho` times `ratio`, both 1 for a ball), with the
/// distance between its two contacts; `None` where none does, or where its centre lies past
/// an end of the filleted edge. The solver's offsets stay at their own distances.
///
/// `target` holds the solver's edge use.
///
/// # Errors
///
/// Those of the drops and evaluations; `SM_ERR_NULL_POINTER` without both offset surfaces
/// or an edge curve.
pub fn ball_through(
    k: &Kernel,
    target: &Brep,
    solver: &mut SolverObject,
    lk: usize,
    q: Point3,
    max: f64,
    ratio: Option<[f64; 2]>,
) -> Result<Option<(f64, f64)>> {
    Ok(ball_through_at(k, target, solver, lk, q, max, ratio)?.map(|(rho, chord, _)| (rho, chord)))
}

/// [`ball_through`], with the filleted edge's parameter where the ball's centre lies square
/// to it.
fn ball_through_at(
    k: &Kernel,
    target: &Brep,
    solver: &mut SolverObject,
    lk: usize,
    q: Point3,
    max: f64,
    ratio: Option<[f64; 2]>,
) -> Result<Option<(f64, f64, f64)>> {
    let ratio = ratio.unwrap_or([1.0, 1.0]);
    let (bases, radii) = {
        let base = solver.base();
        let s0 = base
            .surface(0)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?;
        let s1 = base
            .surface(1)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?;
        (
            [
                s0.surface.base_surface().clone(),
                s1.surface.base_surface().clone(),
            ],
            [
                s0.surface.offset_distance().abs(),
                s1.surface.offset_distance().abs(),
            ],
        )
    };
    let mut domains = [
        bases[0].natural_uv_domain_with_nurb(k)?,
        bases[1].natural_uv_domain_with_nurb(k)?,
    ];
    {
        let ops = ops_of(solver);
        for (base, domain) in bases.iter().zip(domains.iter_mut()) {
            ops.setup_offset_extension(k, base, domain)?;
        }
    }
    let lj = 1 - lk;
    if !(radii[0] > EFF_ZERO) || !(radii[1] > EFF_ZERO) {
        return Ok(None);
    }
    let mut st = Through {
        bases,
        domains,
        radii,
        guess: None,
        foot_at: Point3::UNDEF,
    };
    let (Some(base_k), Some(domain_k)) = (st.bases.get(lk), st.domains.get(lk)) else {
        return Err(SolidError::blend_missing(
            OP,
            "the rail's base surface and domain",
        ));
    };
    let drop = base_k.drop_point::<NoObject>(k, q, domain_k, &DropPointOptions::default())?;
    if !drop.success {
        return Ok(None);
    }
    let (_, n) = st.toward(k, solver, lk, drop.result_uv)?;
    let (ratio_k, ratio_j) = (
        ratio.get(lk).copied().unwrap_or(1.0),
        ratio.get(lj).copied().unwrap_or(1.0),
    );
    // h(rho) = <c - f, n'> - rho: `None` where c drops on no point of the other face
    let h = |st: &mut Through, solver: &mut SolverObject, rho: f64| -> Result<Option<f64>> {
        let c = q + n * (rho * ratio_k);
        let (Some(base_j), Some(domain_j)) = (st.bases.get(lj), st.domains.get(lj)) else {
            return Err(SolidError::blend_missing(
                OP,
                "the other rail's base surface and domain",
            ));
        };
        let foot = base_j.drop_point::<NoObject>(
            k,
            c,
            domain_j,
            &DropPointOptions {
                guess_uv: st.guess,
                ..DropPointOptions::default()
            },
        )?;
        if !foot.success {
            return Ok(None);
        }
        st.guess = Some(foot.result_uv);
        let (f, nj) = st.toward(k, solver, lj, foot.result_uv)?;
        let value = (c - f).dot(nj) - rho * ratio_j;
        st.foot_at = f;
        Ok(Some(value))
    };
    let mut lo = 0.0;
    match h(&mut st, solver, 0.0)? {
        Some(h_lo) if h_lo > 0.0 => {}
        _ => return Ok(None), // q not on the ball's side of the other face
    }
    for i in 1..=STEPS {
        let hi = max * i as f64 / STEPS as f64;
        let Some(h_hi) = h(&mut st, solver, hi)? else {
            return Ok(None);
        };
        if h_hi > 0.0 {
            lo = hi;
            continue;
        }
        let (mut a, mut b) = (lo, hi);
        for _ in 0..60 {
            let mid = 0.5 * (a + b);
            let Some(h_mid) = h(&mut st, solver, mid)? else {
                return Ok(None);
            };
            if h_mid > 0.0 {
                a = mid;
            } else {
                b = mid;
            }
        }
        let rho = 0.5 * (a + b);
        // the fork's pdChord: h at rho again for its foot
        let _ = h(&mut st, solver, rho)?;
        let chord = q.distance(st.foot_at);
        // the ball's centre square to the filleted edge within it, not past an end
        let edgeuse = solver
            .base()
            .edgeuse(0)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the solver's edge use"))?;
        let e = eu::edge(target, edgeuse)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the edge use's edge"))?;
        let curve = edge::evaluation_curve(target, e)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
        let ivl = edge::interval(target, e)?;
        let dropped = curve.drop_point(
            k,
            &ivl,
            q + n * (rho * ratio_k),
            None,
            EFF_ZERO,
            None,
            SolverOperation::Minimize,
        )?;
        let ends = 1.0e-9 * (1.0 + ivl.length());
        let found = dropped.success
            && (edge::is_closed(target, e)?
                || (dropped.parameter > ivl.min() + ends && dropped.parameter < ivl.max() - ends));
        return Ok(found.then_some((rho, chord, dropped.parameter)));
    }
    Ok(None)
}

/// `SmFilletSolver::LargestSizeOnItsFaces(pOnly, ...)`: the largest size at which this
/// blend's rails stay on the faces it rolls on (`only`: on that face only): the least size
/// of a ball, or of the solver's two offsets, that touches one face at a point of that
/// face's boundary (an edge not the filleted one nor at its ends) and the other face, with
/// what it sizes. `None` where no boundary point bounds it, or for a radius law.
///
/// Not the fork's (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): a rail rolled
/// over a smooth far edge of its face, the whole length, rolls on the face beyond, and its
/// face's boundary is that face's: less the edges it rolled over, and with the edges at their
/// ends taken as the side edges the rail ends on (as those at the filleted edge's ends). The
/// face it rolled off lies inside the band and does not bound it. Each boundary point of a
/// solver with a rolled rail sizes it only where the solver's own ball of that size touches
/// that point, or where that cannot be told ([`rolls_through`]): the ball through it may lie
/// on another sheet of the rolled-onto face's surface (beyond a boss, against the extension
/// of the other face). Such a solver of another kind than a constant ball is not sized.
///
/// # Errors
///
/// Those of [`ball_through`], [`rolls_through`] and the topology lookups.
pub fn largest_size_on_its_faces(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    only: Option<FaceId>,
) -> Result<Option<(f64, FitKind)>> {
    let mut found: Option<f64> = None;
    let mut kind = FitKind::Radius;
    let base = solver.base();
    let (Some(eu0), Some(eu1)) = (base.edgeuse(0)?, base.edgeuse(1)?) else {
        return Ok(None);
    };
    let off = match (base.surface(0)?, base.surface(1)?) {
        (Some(a), Some(b)) => [
            a.surface.offset_distance().abs(),
            b.surface.offset_distance().abs(),
        ],
        _ => return Ok(None),
    };
    let mut ratio = [1.0, 1.0];
    // the bound is looked for up to twice the blend's own size (5e8c9c1)
    let mut max = 2.0 * off[0];
    let solver_type = solver.solver_type()?;
    let (rolls, on_edge) = rolls_a_ball(solver);
    if rolls {
        if on_edge {
            return Ok(None); // a radius law: no single size
        }
    } else if solver_type == FilletSolverType::ConstDist {
        kind = FitKind::Chord; // a ball of any radius, sized by its chord
        max = 20.0 * smos_max(off[0], off[1]);
    } else if solver_type == FilletSolverType::ConstRadius && off[0] > 0.0 && off[1] > 0.0 {
        kind = FitKind::Scale; // both offsets scaled together
        ratio = off;
        max = 2.0; // the factor, twice the blend's own (1), as for a ball
    } else {
        return Ok(None);
    }
    let blended = eu::edge(target, eu0)?
        .ok_or_else(|| SolidError::blend_missing(OP, "the edge use's edge"))?;
    let v0 = edge::vertex(target, blended)?;
    let ends = [v0, edge::other_vertex(target, blended, v0)?];
    let rolled = solver.base().rolled_faces;
    let tangency = solver.base().tangency_tolerance;
    let confirm = rolled.iter().any(Option::is_some);
    if confirm && kind != FitKind::Radius {
        // the confirmation rolls the solver's ball: a rolled rail of another kind is not
        // sized (the rollover rolls only constant balls)
        return Ok(None);
    }
    for (rail, edgeuse) in [eu0, eu1].into_iter().enumerate() {
        let Some(own) = eu::face(target, edgeuse)? else {
            continue;
        };
        // the face the rail rolls on: its own, or the face beyond the smooth edge it rolled
        // over; that edge, and the vertices it ends the rail at
        let onto = rolled.get(rail).copied().flatten();
        let face = onto.unwrap_or(own);
        if only.is_some_and(|f| f != face) {
            continue;
        }
        let mut over: Vec<EdgeId> = Vec::new();
        let mut rail_ends: Vec<Option<VertexId>> = ends.to_vec();
        if onto.is_some() {
            for e in crate::topology::face::p4::edges(target, face, None, false, None)? {
                if edge::faces(target, e, None)?.contains(&own)
                    && test_manifold_tangent_rollover(k, target, e, tangency)?.0
                {
                    let p0 = edge::vertex(target, e)?;
                    let p1 = edge::other_vertex(target, e, p0)?;
                    rail_ends.extend([p0, p1].into_iter().flatten().map(Some));
                    over.push(e);
                }
            }
        }
        for e in crate::topology::face::p4::edges(target, face, None, false, None)? {
            if e == blended || over.contains(&e) {
                continue;
            }
            let Some(curve) = edge::evaluation_curve(target, e)? else {
                continue;
            };
            let p0 = edge::vertex(target, e)?;
            let p1 = edge::other_vertex(target, e, p0)?;
            if rail_ends.contains(&p0) || rail_ends.contains(&p1) {
                continue; // a side edge at an end of the rail: the rail ends on it
            }
            let curve = curve.clone();
            let ivl = edge::interval(target, e)?;
            let mut rho_at = |solver: &mut SolverObject, t: f64| -> Result<f64> {
                let q = curve.evaluate_point(k, ivl.evaluate(smos_max(0.0, smos_min(1.0, t))))?;
                if confirm {
                    // a ball of a solver with a rolled rail (a constant ball): no bound where
                    // the solver does not roll it, kept where its section is not found
                    let through = ball_through_at(k, &*target, solver, rail, q, max, Some(ratio))?;
                    let Some((rho, _, at)) = through else {
                        return Ok(BIG_DOUBLE);
                    };
                    return Ok(match rolls_through(k, target, solver, rail, q, rho, at)? {
                        Some(false) => BIG_DOUBLE,
                        Some(true) | None => rho,
                    });
                }
                Ok(
                    match ball_through(k, &*target, solver, rail, q, max, Some(ratio))? {
                        Some((rho, chord)) => {
                            if kind == FitKind::Chord {
                                chord
                            } else {
                                rho
                            }
                        }
                        None => BIG_DOUBLE,
                    },
                )
            };
            let mut least = BIG_DOUBLE;
            let mut at = 0.0;
            for j in 0..=STEPS {
                let rho = rho_at(solver, j as f64 / STEPS as f64)?;
                if rho < least {
                    least = rho;
                    at = j as f64 / STEPS as f64;
                }
            }
            if !(least < BIG_DOUBLE) {
                continue;
            }
            let step = 1.0 / STEPS as f64;
            let (mut a, mut b) = (smos_max(0.0, at - step), smos_min(1.0, at + step));
            let golden = 0.5 * (5.0_f64.sqrt() - 1.0);
            let mut x1 = b - golden * (b - a);
            let mut x2 = a + golden * (b - a);
            let mut f1 = rho_at(solver, x1)?;
            let mut f2 = rho_at(solver, x2)?;
            for _ in 0..60 {
                if f1 < f2 {
                    b = x2;
                    x2 = x1;
                    f2 = f1;
                    x1 = b - golden * (b - a);
                    f1 = rho_at(solver, x1)?;
                } else {
                    a = x1;
                    x1 = x2;
                    f1 = f2;
                    x2 = a + golden * (b - a);
                    f2 = rho_at(solver, x2)?;
                }
            }
            least = smos_min(least, smos_min(f1, f2));
            if found.is_none_or(|r| least < r) {
                found = Some(least);
            }
        }
    }
    Ok(found.map(|r| (r, kind)))
}

/// Not the fork's (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): whether the
/// ball of radius `rho` that `solver` rolls, in its section at its filleted edge's `t`,
/// touches its face `rail` at `q`, within the solver's 3D tolerance; `None` where that cannot
/// be told (no single ball, or no section found). A ball through a boundary
/// point that touches the other face's surface ([`ball_through`]) is not always the solver's:
/// past a boss a rail rolled onto, it may touch that surface on the boss's far side and the
/// other face's surface extended beyond the face, where the solver's ball never rolls. The
/// solver's radius is its own again after.
///
/// # Errors
///
/// Those of the section solve and the evaluations.
fn rolls_through(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    rail: usize,
    q: Point3,
    rho: f64,
    t: f64,
) -> Result<Option<bool>> {
    let base = solver.base();
    let tol = base.approx_tol_3d;
    // without its offset surface, edge use or edge curve, it cannot be told
    let Some(own) = base.surface(0)?.map(|s| s.surface.offset_distance().abs()) else {
        return Ok(None);
    };
    let Some(edgeuse) = base.edgeuse(0)? else {
        return Ok(None);
    };
    let Some(e) = eu::edge(target, edgeuse)? else {
        return Ok(None);
    };
    let Some(curve) = edge::evaluation_curve(target, e)? else {
        return Ok(None);
    };
    let side = if rail == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    };
    if !set_ball_radius(solver, rho)? {
        return Ok(None); // no single ball
    }
    let contact = (|| -> Result<Option<Point3>> {
        let Some(pnt) = ball_section_at(k, target, solver, &curve, t, None)? else {
            return Ok(None);
        };
        let Some(slot) = solver.base().surface(rail)? else {
            return Ok(None);
        };
        Ok(Some(
            slot.surface
                .base_surface()
                .evaluate_point(k, pnt.uv_pos(side))?,
        ))
    })();
    set_ball_radius(solver, own)?;
    let contact = contact?;
    Ok(contact.map(|on| on.distance(q) <= tol))
}

/// `SmFilletSolver::BandsMeetAt(rOther, lMine, lTheirs, ...)`: the radius at which this
/// constant ball's rail on its face `mine` first reaches into the band `other` takes off
/// the same face (its face `theirs`), both blends at that radius. `None` where the two do
/// not roll a constant ball of one radius, or no radius below theirs keeps them apart.
/// Both solvers' radii are their own again after.
///
/// # Errors
///
/// Those of the section solves, [`ball_through`] and the evaluations.
pub fn bands_meet_at(
    k: &Kernel,
    target: &mut Brep,
    this: &mut SolverObject,
    other: &mut SolverObject,
    mine: usize,
    theirs: usize,
) -> Result<Option<f64>> {
    let (rolls, law) = rolls_a_ball(this);
    let (other_rolls, other_law) = rolls_a_ball(other);
    if !rolls || law || !other_rolls || other_law {
        return Ok(None);
    }
    let Some(edgeuse) = this.base().edgeuse(0)? else {
        return Ok(None);
    };
    let own = this
        .base()
        .surface(0)?
        .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
        .surface
        .offset_distance()
        .abs();
    let other_own = other
        .base()
        .surface(0)?
        .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
        .surface
        .offset_distance()
        .abs();
    if !((other_own - own).abs() <= EFF_ZERO * (1.0 + own)) {
        return Ok(None); // no single size: their radii differ
    }
    let e = eu::edge(target, edgeuse)?
        .ok_or_else(|| SolidError::blend_missing(OP, "the edge use's edge"))?;
    let curve = edge::evaluation_curve(target, e)?
        .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?
        .clone();
    let ivl = edge::interval(target, e)?;
    let side = if mine == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    };
    // g at the edge's t, both balls of radius rho (BIG where no point of the rail is in reach)
    let g = |target: &mut Brep,
             this: &mut SolverObject,
             other: &mut SolverObject,
             t: f64,
             rho: f64,
             near: &mut Option<TsectPnt>|
     -> Result<f64> {
        let _ = set_ball_radius(this, rho);
        let _ = set_ball_radius(other, rho);
        let result = (|| -> Result<SectionThrough> {
            let Some(pnt) = ball_section_at(k, target, this, &curve, t, near.as_ref())? else {
                return Ok((None, None));
            };
            let q = this
                .base()
                .surface(mine)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
                .surface
                .base_surface()
                .evaluate_point(k, pnt.uv_pos(side))?;
            let through = ball_through(k, target, other, theirs, q, rho, None)?;
            Ok((Some(pnt), through))
        })();
        let _ = set_ball_radius(this, own);
        let _ = set_ball_radius(other, own);
        let (found, through) = result?;
        if let Some(pnt) = found {
            *near = Some(pnt);
        }
        Ok(through.map_or(BIG_DOUBLE, |(through, _)| through - rho))
    };
    // the least of g over the rail at rho
    let least_at = |target: &mut Brep,
                    this: &mut SolverObject,
                    other: &mut SolverObject,
                    rho: f64|
     -> Result<f64> {
        let mut near: Option<TsectPnt> = None;
        let mut least = BIG_DOUBLE;
        let mut at = 0.0;
        for j in 0..STEPS {
            let t = ivl.evaluate((j as f64 + 0.5) / STEPS as f64);
            let value = g(target, this, other, t, rho, &mut near)?;
            if value < least {
                least = value;
                at = t;
            }
        }
        if !(least < BIG_DOUBLE) {
            return Ok(least);
        }
        let step = ivl.length() / STEPS as f64;
        let (mut a, mut b) = (
            smos_max(ivl.min(), at - step),
            smos_min(ivl.max(), at + step),
        );
        let golden = 0.5 * (5.0_f64.sqrt() - 1.0);
        let mut x1 = b - golden * (b - a);
        let mut x2 = a + golden * (b - a);
        let mut f1 = g(target, this, other, x1, rho, &mut near)?;
        let mut f2 = g(target, this, other, x2, rho, &mut near)?;
        for _ in 0..40 {
            if f1 < f2 {
                b = x2;
                x2 = x1;
                f2 = f1;
                x1 = b - golden * (b - a);
                f1 = g(target, this, other, x1, rho, &mut near)?;
            } else {
                a = x1;
                x1 = x2;
                f1 = f2;
                x2 = a + golden * (b - a);
                f2 = g(target, this, other, x2, rho, &mut near)?;
            }
        }
        Ok(smos_min(least, smos_min(f1, f2)))
    };
    let (mut hi, mut lo) = (own, 0.0);
    let least = least_at(target, this, other, hi)?;
    if !(least <= 0.0) {
        return Ok(None); // the bands do not overlap at the blends' radius
    }
    let mut tr = 0.5 * own;
    while tr > 1.0e-6 * own {
        if least_at(target, this, other, tr)? > 0.0 {
            lo = tr;
            break;
        }
        hi = tr;
        tr *= 0.5;
    }
    if !(lo > 0.0) {
        return Ok(None);
    }
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if least_at(target, this, other, mid)? > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(Some(0.5 * (lo + hi)))
}

/// `BandsMeetAt`'s `G`: the section found at a station, and the other's ball through its
/// rail point (radius, chord).
type SectionThrough = (Option<TsectPnt>, Option<(f64, f64)>);

/// A blend on a face, for [`polygon_fit`]: the solver and which of its faces this is.
pub struct OnFace<'s> {
    /// The solver.
    pub solver: &'s mut SolverObject,
    /// Its face index (0 or 1).
    pub rail: usize,
}

/// One face of `SmFilletExecutive::NoteFitOfPolygonsTakenWhole`: where `face` is a plane
/// bounded by straight edges each blended with a constant ball (`on`, the blends on it in
/// the executive's order), the largest radius that leaves some of it, its point deepest in
/// the balls' terms (a linear programme over each triple of edges). `None` where the face
/// is not such a polygon or no vertex of the programme is feasible.
///
/// # Errors
///
/// Those of the evaluations, [`ball_through`] and the topology lookups.
pub fn polygon_fit(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    on: &mut [OnFace<'_>],
) -> Result<Option<f64>> {
    let Some(surface) = crate::topology::face::p2::surface(target, face)? else {
        return Ok(None);
    };
    let planar = surface.is_planar(k, EFF_ZERO)?;
    if !planar.is_planar {
        return Ok(None);
    }
    let at = planar.point;
    let edges = crate::topology::face::p4::edges(target, face, None, false, None)?;
    if edges.len() < 3 || edges.len() != on.len() {
        return Ok(None); // not every edge of the face blended
    }
    // the face's plane frame, and each edge's line: a point and its unit direction
    let normal = planar.normal.unitize()?;
    let x = if normal.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0).cross(normal)
    } else {
        Vec3::new(0.0, 1.0, 0.0).cross(normal)
    };
    let x = x.unitize()?;
    let y = normal.cross(x);
    let mut middle = Point3::new(0.0, 0.0, 0.0);
    let mut ends: Vec<(Point3, Point3)> = Vec::new();
    for &e in &edges {
        let curve = edge::evaluation_curve(target, e)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
        let ivl = edge::interval(target, e)?;
        let a = curve.evaluate_point(k, ivl.min())?;
        let b = curve.evaluate_point(k, ivl.max())?;
        ends.push((a, b));
        middle += (a + b) * (0.5 / edges.len() as f64);
    }
    // t (ax x + ay y + b) is edge i's ball radius at (x, y)
    let mut lines: Vec<[f64; 4]> = Vec::new();
    for (&e, &(pa, pb)) in edges.iter().zip(&ends) {
        let curve = edge::evaluation_curve(target, e)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
        let half = curve.evaluate_point(k, edge::interval(target, e)?.evaluate(0.5))?;
        // straight: its middle on the chord
        let chord = pb - pa;
        let length = chord.length();
        if !(length > EFF_ZERO)
            || (half - pa).cross(chord).length() / length > edge::tolerance(target, e)?
        {
            return Ok(None);
        }
        // the in-plane unit normal of the line, toward the face's middle
        let dx = chord.dot(x) / length;
        let dy = chord.dot(y) / length;
        let (mut ax, mut ay) = (-dy, dx);
        let px = (pa - at).dot(x);
        let py = (pa - at).dot(y);
        let mut b = -(ax * px + ay * py);
        let mx = (middle - at).dot(x);
        let my = (middle - at).dot(y);
        if ax * mx + ay * my + b < 0.0 {
            ax = -ax;
            ay = -ay;
            b = -b;
        }
        // t: the blend's ball touching the face a little way in from the edge's middle
        let mut blend: Option<usize> = None;
        for (i, o) in on.iter().enumerate() {
            let Some(edgeuse) = o.solver.base().edgeuse(0)? else {
                continue;
            };
            if eu::edge(target, edgeuse)? == Some(e) {
                blend = Some(i);
            }
        }
        let Some(o) = blend.and_then(|i| on.get_mut(i)) else {
            return Ok(None);
        };
        let d_in = 1.0e-3 * length;
        let q = half + (x * ax + y * ay) * d_in;
        let max = o
            .solver
            .base()
            .surface(0)?
            .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
            .surface
            .offset_distance()
            .abs();
        let Some((rho, _)) = ball_through(k, target, o.solver, o.rail, q, max, None)? else {
            return Ok(None);
        };
        lines.push([ax, ay, b, rho / d_in]);
    }
    // max rho with rho <= t_i (a_i . q + b_i): the best vertex of the programme
    let det3 = |m: &[[f64; 3]; 3]| -> f64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let mut best = -1.0;
    let n = lines.len();
    for i in 0..n {
        for j in i + 1..n {
            for l in j + 1..n {
                let (Some(li), Some(lj), Some(ll)) = (lines.get(i), lines.get(j), lines.get(l))
                else {
                    continue;
                };
                let three = [li, lj, ll];
                let mut m = [[0.0_f64; 3]; 3];
                let mut r = [0.0_f64; 3];
                for (row, (mr, rr)) in three.iter().zip(m.iter_mut().zip(r.iter_mut())) {
                    let [ax, ay, b, t] = **row;
                    *mr = [t * ax, t * ay, -1.0];
                    *rr = -t * b;
                }
                let det = det3(&m);
                if !(det.abs() > EFF_ZERO) {
                    continue;
                }
                let mut sol = [0.0_f64; 3];
                for (c, s) in sol.iter_mut().enumerate() {
                    let mut cm = m;
                    for (row, rr) in cm.iter_mut().zip(r) {
                        if let Some(cell) = row.get_mut(c) {
                            *cell = rr;
                        }
                    }
                    *s = det3(&cm) / det;
                }
                let [qx, qy, rho] = sol;
                let mut feasible = rho > 0.0;
                for &[ax, ay, b, t] in &lines {
                    if !feasible {
                        break;
                    }
                    if rho > t * (ax * qx + ay * qy + b) * (1.0 + 1.0e-12) + 1.0e-12 {
                        feasible = false;
                    }
                }
                if feasible && rho > best {
                    best = rho;
                }
            }
        }
    }
    Ok((best > 0.0).then_some(best))
}

/// The circle-top fit of `CreateFilletBrep` after `FacesOutlastTheirBlends` for one
/// solver: where its closed edge bounds alone a planar face it rolls on (a cylinder's top),
/// the radius of the ball touching that face at the circle's centre, looked for up to twice
/// the blend's radius (5e8c9c1); one per such face, in face-index order, with the edge.
///
/// # Errors
///
/// Those of the evaluations, [`ball_through`] and the topology lookups.
pub fn circle_top_fit(
    k: &Kernel,
    target: &Brep,
    solver: &mut SolverObject,
) -> Result<Vec<(f64, EdgeId)>> {
    let mut out = Vec::new();
    let Some(eu0) = solver.base().edgeuse(0)? else {
        return Ok(out);
    };
    let Some(e) = eu::edge(target, eu0)? else {
        return Ok(out);
    };
    let Some(curve) = edge::evaluation_curve(target, e)? else {
        return Ok(out);
    };
    if !edge::is_closed(target, e)? {
        return Ok(out);
    }
    let curve = curve.clone();
    for rail in 0..2 {
        let Some(edgeuse) = solver.base().edgeuse(rail)? else {
            continue;
        };
        let Some(face) = eu::face(target, edgeuse)? else {
            continue;
        };
        let is_plane = crate::topology::face::p2::surface(target, face)?
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
        if !is_plane {
            continue;
        }
        let own = crate::topology::face::p4::edges(target, face, None, false, None)?;
        if own.len() != 1 || own.first() != Some(&e) {
            continue;
        }
        // the circle's centre from three of its points, a fourth on the same circle
        let ivl = edge::interval(target, e)?;
        let mut p = [Point3::UNDEF; 4];
        for (j, pj) in p.iter_mut().enumerate() {
            *pj = curve.evaluate_point(k, ivl.evaluate(0.125 + 0.25 * j as f64))?;
        }
        let a = p[1] - p[0];
        let b = p[2] - p[0];
        let nrm = a.cross(b);
        let n2 = nrm.length_squared();
        if !(n2 > EFF_ZERO) {
            continue;
        }
        let centre = p[0]
            + (nrm.cross(a) * b.length_squared() + b.cross(nrm) * a.length_squared()) / (2.0 * n2);
        let rad = centre.distance(p[0]);
        if (centre.distance(p[3]) - rad).abs() > edge::tolerance(target, e)? {
            continue; // not a circle
        }
        let max = 2.0
            * solver
                .base()
                .surface(0)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
                .surface
                .offset_distance()
                .abs();
        if let Some((rho, _)) = ball_through(k, target, solver, rail, centre, max, None)? {
            out.push((rho, e));
        }
    }
    Ok(out)
}

/// `sm_CornerTakesPerRadius(crV, crEdge, crBlended, ...)` (the fork's, a3d6d81): how far
/// along the blended edge `e`, per unit of radius, the corner at its vertex `v` takes it.
/// A convex corner of three planar faces whose three edges are all in `blended` (blended
/// at one radius) ends its rounds on the plane through the corner's ball centre
/// `c = V + r u` square to each edge, so the edge loses `r <u, e>` there; 0 at any other
/// vertex.
///
/// # Errors
///
/// Those of the evaluations and topology lookups.
pub fn corner_takes_per_radius(
    k: &Kernel,
    target: &Brep,
    v: VertexId,
    e: EdgeId,
    blended: &std::collections::BTreeSet<EdgeId>,
) -> Result<f64> {
    let edges = crate::topology::vertex::edges(target, v, None)?;
    if edges.len() != 3 {
        return Ok(0.0);
    }
    let mut dir = [Vec3::UNDEF; 3];
    let mut this = 3;
    for (i, (&ei, d)) in edges.iter().zip(dir.iter_mut()).enumerate() {
        let Some(curve) = edge::evaluation_curve(target, ei)? else {
            return Ok(0.0);
        };
        if !blended.contains(&ei) {
            return Ok(0.0);
        }
        if ei == e {
            this = i;
        }
        // the edge's direction leaving V
        let ivl = edge::interval(target, ei)?;
        let at_start = edge::vertex(target, ei)? == Some(v);
        let pv = curve.evaluate(
            k,
            if at_start { ivl.min() } else { ivl.max() },
            1,
            at_start,
            false,
        )?;
        let tangent = pv
            .get(1)
            .copied()
            .ok_or_else(|| SolidError::blend_missing(OP, "the curve's tangent"))?;
        *d = (if at_start { tangent } else { -tangent }).unitize()?;
    }
    let Some(&along) = dir.get(this) else {
        return Ok(0.0);
    };
    // each face between two of the edges: its plane's normal, outward (away from the third)
    let mut n = [Vec3::UNDEF; 3];
    for (i, ni) in n.iter_mut().enumerate() {
        let (Some(&a), Some(&b), Some(&c)) =
            (dir.get((i + 1) % 3), dir.get((i + 2) % 3), dir.get(i))
        else {
            return Ok(0.0);
        };
        let s = a.cross(b);
        if !(s.length() > EFF_ZERO) {
            return Ok(0.0);
        }
        let mut s = s.unitize()?;
        if s.dot(c) > 0.0 {
            s = -s;
        }
        *ni = s;
    }
    // the faces at V are planes: each face holding two of the edges
    let faces = crate::topology::vertex::faces(target, v)?;
    if faces.len() != 3 {
        return Ok(0.0);
    }
    for f in faces {
        let plane = crate::topology::face::p2::surface(target, f)?
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
        if !plane {
            return Ok(0.0);
        }
    }
    // <n_f, u> = -1 for the three planes (Cramer's rule)
    let [a, b, c] = n;
    let det = a.dot(b.cross(c));
    if !(det.abs() > EFF_ZERO) {
        return Ok(0.0);
    }
    let u = (b.cross(c) + c.cross(a) + a.cross(b)) * (-1.0 / det);
    Ok(smos_max(0.0, u.dot(along)))
}

/// The distance `d` of a knife edge's equal chamfer whose chord passes through the profile
/// point `(x, y)` (not the fork's, #15250). The profile frame puts the tip at the origin, the
/// plane along `y = 0` with the body on `x < 0`, and the tangent circle of radius `r` about
/// `(0, -r)`. The chamfer of distance `d` is the chord between the contacts of the ball of
/// radius `d` that touches both faces: with `s = sqrt(r d)` they are `(-2s, 0)` on the plane
/// and `(-2r²s / (r² + s²), -2rs² / (r² + s²))` on the circle. Every such chord passes through
/// `A = (0, -2r)`, the circle's point opposite the tip, so the chord through `(x, y)` (with
/// `y > -2r`) has `s = -r x / (2r + y)`, and `d = s² / r = r (x / (2r + y))²`.
#[must_use]
pub fn knife_chamfer_distance_through(r: f64, x: f64, y: f64) -> f64 {
    let s = x / (2.0 * r + y);
    r * s * s
}

/// A face's surface, as [`knife_chamfer_keeps_nothing`] needs it: a plane (a point and its
/// normal) or a circular cylinder (a point of its axis, the axis and the radius).
#[derive(Clone, Copy)]
enum Support {
    Plane(Point3, Vec3),
    Cylinder(Point3, Vec3, f64),
}

/// A circle's centre, unit normal and radius when `curve` is one (an `SmCircle`, or an
/// ellipse whose radii agree); `None` for any other kind.
fn circle_of(curve: &crate::curve::curve::Curve) -> Option<(Point3, Vec3, f64)> {
    match curve.kind() {
        crate::curve::curve::CurveKind::Ellipse(c) if c.is_circle() => {
            let at = c.position();
            Some((at.origin(), at.z_axis(), c.x_radius()))
        }
        _ => None,
    }
}

/// The face's surface when it is a plane or a circular cylinder (a cone that is a cylinder,
/// or a circle extruded along its normal); `None` for any other kind or no surface.
fn support_of(target: &Brep, f: FaceId) -> Result<Option<Support>> {
    let Some(surface) = crate::topology::face::p2::surface(target, f)? else {
        return Ok(None);
    };
    Ok(match surface.kind() {
        SurfaceKind::Plane(p) => {
            let (point, normal, _) = p.is_planar();
            Some(Support::Plane(point, normal))
        }
        SurfaceKind::Cone(c) if c.is_cylinder() => {
            let canonical = c.canonical();
            Some(Support::Cylinder(
                canonical.origin.origin(),
                canonical.origin.z_axis(),
                canonical.radius,
            ))
        }
        SurfaceKind::SurfOfExtrusion(s) => {
            s.gen_curve
                .as_ref()
                .and_then(circle_of)
                .and_then(|(centre, normal, radius)| {
                    let v = s.extrusion_vector();
                    let n = v.length();
                    (n > EFF_ZERO && v.cross(normal).length() <= EFF_ZERO * n)
                        .then_some(Support::Cylinder(centre, normal, radius))
                })
        }
        _ => None,
    })
}

/// The profile frame of a knife edge between a plane and a circular cylinder tangent to it
/// along the edge ([`knife_chamfer_keeps_nothing`]): the tip, the edge's direction and
/// length, the unit vector from the edge toward the cylinder's axis, the radius and the
/// edge's tolerance.
struct KnifeFrame {
    tip: Point3,
    along: Vec3,
    length: f64,
    toward_axis: Vec3,
    radius: f64,
    tol: f64,
}

impl KnifeFrame {
    /// A point's height along the edge from the tip and its two profile components: along
    /// the plane (square to the edge, one way) and up out of the plane, away from the axis.
    fn profile(&self, p: Point3) -> (f64, f64, f64) {
        let q = p - self.tip;
        let up = -self.toward_axis;
        (q.dot(self.along), q.dot(self.along.cross(up)), q.dot(up))
    }

    /// Whether a direction is parallel to the edge, to the edge's tolerance over its length.
    fn parallel(&self, v: Vec3) -> bool {
        let n = v.length();
        n > EFF_ZERO && v.cross(self.along).length() <= n * self.tol / self.length
    }

    /// Whether a direction is square to the edge, to the edge's tolerance over its length.
    fn square(&self, v: Vec3) -> bool {
        let n = v.length();
        n > EFF_ZERO && v.dot(self.along).abs() <= n * self.tol / self.length
    }
}

/// The knife edge `e`'s frame when it lies between a plane and a circular cylinder tangent
/// to the plane along it (the plane parallel to the axis, at the radius from it, the edge on
/// both); `None` otherwise.
fn knife_frame(k: &Kernel, target: &Brep, e: EdgeId) -> Result<Option<KnifeFrame>> {
    let Some(curve) = edge::curve(target, e)? else {
        return Ok(None);
    };
    let ivl = edge::interval(target, e)?;
    let p0 = curve.evaluate_point(k, ivl.min())?;
    let p1 = curve.evaluate_point(k, ivl.max())?;
    let tol = edge::tolerance(target, e)?;
    let faces = edge::faces(target, e, None)?;
    let (Some(&f0), Some(&f1), 2) = (faces.first(), faces.get(1), faces.len()) else {
        return Ok(None);
    };
    let ((plane_point, normal), (axis_point, axis, radius)) =
        match (support_of(target, f0)?, support_of(target, f1)?) {
            (Some(Support::Plane(p, n)), Some(Support::Cylinder(a, t, r)))
            | (Some(Support::Cylinder(a, t, r)), Some(Support::Plane(p, n))) => ((p, n), (a, t, r)),
            _ => return Ok(None),
        };
    let (n, t) = (normal.length(), axis.length());
    if !(n > EFF_ZERO && t > EFF_ZERO && radius > tol) {
        return Ok(None);
    }
    let (normal, mut along) = (normal / n, axis / t);
    let length = (p1 - p0).dot(along);
    if length < 0.0 {
        along = -along;
    }
    let length = length.abs();
    if !(length > tol) {
        return Ok(None);
    }
    // the plane's side the axis lies on, at the radius
    let height = (axis_point - plane_point).dot(normal);
    if (height.abs() - radius).abs() > tol {
        return Ok(None);
    }
    let frame = KnifeFrame {
        tip: p0,
        along,
        length,
        toward_axis: if height > 0.0 { normal } else { -normal },
        radius,
        tol,
    };
    // the plane parallel to the axis; the edge's ends on the plane and on the cylinder
    let off_axis = |p: Point3| {
        let w = p - axis_point;
        (w - along * w.dot(along)).length()
    };
    if !frame.square(normal)
        || [p0, p1].iter().any(|&p| {
            (p - plane_point).dot(normal).abs() > tol || (off_axis(p) - radius).abs() > tol
        })
    {
        return Ok(None);
    }
    Ok(Some(frame))
}

/// The points of an arc of the circle about `(ox, oy)` of radius `radius` in the knife's
/// profile, from `ends[0]` through `ends[1]` to `ends[2]` (profile points), whose
/// components are extreme for the conditions of [`knife_chamfer_keeps_nothing`]: the tangent
/// points from `A` (where the chord's distance is extreme on the circle), the points
/// farthest each way along and out of the plane, and the point nearest the knife's axis;
/// those on the arc.
fn arc_candidates(
    frame: &KnifeFrame,
    (ox, oy): (f64, f64),
    radius: f64,
    ends: [(f64, f64); 3],
) -> Vec<(f64, f64)> {
    let angle = |(x, y): (f64, f64)| (y - oy).atan2(x - ox);
    let turn = |a: f64, from: f64| (a - from).rem_euclid(crate::math::TWO_PI);
    let [s, m, f] = ends.map(angle);
    let [(sx, sy), _, (fx, fy)] = ends;
    let closed = ((sx - fx) * (sx - fx) + (sy - fy) * (sy - fy)).sqrt() <= frame.tol;
    let (to_mid, to_end) = (turn(m, s), turn(f, s));
    let on_arc = |a: f64| {
        closed || {
            let t = turn(a, s);
            if to_mid <= to_end {
                t <= to_end
            } else {
                t >= to_end || t == 0.0
            }
        }
    };
    let r = frame.radius;
    let mut points = vec![
        (ox + radius, oy),
        (ox - radius, oy),
        (ox, oy + radius),
        (ox, oy - radius),
    ];
    // the point nearest the axis, at (0, -r)
    let (cx, cy) = (-ox, -r - oy);
    let to_axis = (cx * cx + cy * cy).sqrt();
    if to_axis > EFF_ZERO {
        points.push((ox + radius * cx / to_axis, oy + radius * cy / to_axis));
    }
    // the tangent points from A = (0, -2r)
    let (vx, vy) = (-ox, -2.0 * r - oy);
    let l = (vx * vx + vy * vy).sqrt();
    if l > radius {
        let phi = (radius / l).acos();
        let (ux, uy) = (vx / l, vy / l);
        for sign in [1.0, -1.0] {
            let (sn, cs) = (sign * phi).sin_cos();
            points.push((
                ox + radius * (ux * cs - uy * sn),
                oy + radius * (ux * sn + uy * cs),
            ));
        }
    }
    points.retain(|&p| on_arc(angle(p)));
    points
}

/// The distance beyond which a knife edge's equal chamfer keeps nothing of the body (not
/// the fork's, #15250), computed from the geometry: `None` unless `e` lies between a plane
/// and a circular cylinder tangent to it along `e` ([`knife_chamfer_distance_through`]'s
/// profile), and the body is one the chord's sweep covers whole. The chord pivots about the
/// cylinder's line opposite the edge, so the largest distance is the largest
/// [`knife_chamfer_distance_through`] over the body, a quotient of linear functions of the
/// profile point: reached on the body's boundary, and on a planar face on its edges.
///
/// Every face must be a plane or a cylinder along the edge. Each edge then lies where its
/// faces' surfaces meet, whatever its curve's representation: between two planes, or a
/// plane along the axis and a cylinder, or two cylinders, a line, whose extremes are its
/// ends; between a plane square to the axis and a cylinder, an arc of the cylinder's
/// circle, whose extremes are its ends and its points where the chord through `A` is
/// tangent to it. Any other plane, face or edge gives `None`, never a guess.
///
/// The body must lie within the edge's length along it, on the material side of the plane,
/// outside the cylinder, above `A` and on one side of the edge: then the chord of the
/// largest distance takes all of it. Open: the distance itself keeps a point. The caller
/// notes it less the fit margin.
///
/// # Errors
///
/// Those of the evaluations and topology lookups.
pub fn knife_chamfer_keeps_nothing(k: &Kernel, target: &Brep, e: EdgeId) -> Result<Option<f64>> {
    let Some(frame) = knife_frame(k, target, e)? else {
        return Ok(None);
    };
    let mut supports = std::collections::BTreeMap::new();
    for &f in target.face_list() {
        let fits = match support_of(target, f)? {
            Some(p @ Support::Plane(..)) => Some(p),
            Some(c @ Support::Cylinder(_, axis, _)) => frame.parallel(axis).then_some(c),
            None => None,
        };
        let Some(support) = fits else {
            return Ok(None);
        };
        supports.insert(f, support);
    }
    let (r, tol) = (frame.radius, frame.tol);
    // profile points (height, x, y) and the profile segments of the line edges
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    let mut segments: Vec<[(f64, f64); 2]> = Vec::new();
    for &v in target.vertex_list() {
        points.push(frame.profile(crate::topology::vertex::point(target, v)?));
    }
    for &ei in target.edge_list() {
        let Some(curve) = edge::curve(target, ei)? else {
            return Ok(None);
        };
        let ivl = edge::interval(target, ei)?;
        let ends = [
            frame.profile(curve.evaluate_point(k, ivl.min())?),
            frame.profile(curve.evaluate_point(k, ivl.evaluate(0.5))?),
            frame.profile(curve.evaluate_point(k, ivl.max())?),
        ];
        points.extend([ends[0], ends[2]]);
        let mut planes = Vec::new();
        let mut cylinders = Vec::new();
        for f in edge::faces(target, ei, None)? {
            match supports.get(&f) {
                Some(Support::Plane(_, n)) => planes.push(*n),
                Some(&Support::Cylinder(a, _, rho)) => cylinders.push((a, rho)),
                None => return Ok(None),
            }
        }
        // a plane across the axis (its normal along it), or neither along nor across it
        let plane_across = planes.iter().any(|&n| frame.parallel(n));
        let plane_oblique = planes
            .iter()
            .any(|&n| !frame.parallel(n) && !frame.square(n));
        let line = || [(ends[0].1, ends[0].2), (ends[2].1, ends[2].2)];
        match (planes.len(), cylinders.as_slice()) {
            (2, []) => segments.push(line()),
            (1, [_]) if !plane_across && !plane_oblique => segments.push(line()),
            (0, [_, _] | [_]) => {
                let (a, rho) = cylinders.first().copied().unwrap_or((Point3::UNDEF, 0.0));
                let same = cylinders.iter().all(|&(b, s)| {
                    (s - rho).abs() <= tol && {
                        let w = b - a;
                        (w - frame.along * w.dot(frame.along)).length() <= tol
                    }
                });
                if same {
                    // on one cylinder: an arc of its circle
                    let (_, ox, oy) = frame.profile(a);
                    let arc = ends.map(|(_, x, y)| (x, y));
                    points.extend(
                        arc_candidates(&frame, (ox, oy), rho, arc)
                            .into_iter()
                            .map(|(x, y)| (ends[0].0, x, y)),
                    );
                } else {
                    // two cylinders along the axis meet in lines along it
                    segments.push(line());
                }
            }
            (1, [(a, rho)]) if plane_across => {
                let (_, ox, oy) = frame.profile(*a);
                let arc = ends.map(|(_, x, y)| (x, y));
                points.extend(
                    arc_candidates(&frame, (ox, oy), *rho, arc)
                        .into_iter()
                        .map(|(x, y)| (ends[0].0, x, y)),
                );
            }
            _ => return Ok(None),
        }
    }
    // the side of the edge the body lies on
    let sign = if points.iter().all(|&(_, x, _)| x <= tol) {
        1.0
    } else if points.iter().all(|&(_, x, _)| x >= -tol) {
        -1.0
    } else {
        return Ok(None);
    };
    // no line edge runs into the cylinder (an arc's nearest point is among its points)
    for [(x0, y0), (x1, y1)] in segments {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            smos_max(0.0, smos_min(1.0, (-x0 * dx + (-r - y0) * dy) / len2))
        } else {
            0.0
        };
        let (px, py) = (x0 + t * dx, y0 + t * dy);
        if (px * px + (py + r) * (py + r)).sqrt() < r - tol {
            return Ok(None);
        }
    }
    let mut most = 0.0_f64;
    for &(z, x, y) in &points {
        let off_axis = (x * x + (y + r) * (y + r)).sqrt();
        if z < -tol
            || z > frame.length + tol
            || y > tol
            || !(y + 2.0 * r > tol)
            || off_axis < r - tol
        {
            return Ok(None);
        }
        most = smos_max(most, knife_chamfer_distance_through(r, sign * x, y));
    }
    Ok((most > 0.0).then_some(most))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
    use super::knife_chamfer_distance_through;

    /// The contacts of the ball of radius `d` touching the plane `y = 0` from below and the
    /// circle of radius `r` about `(0, -r)` from outside: its centre `(-b, -d)` with
    /// `b² + (r - d)² = (r + d)²`, the plane contact above it, the circle contact on the
    /// line between the two centres. Built from the ball, not from the chord's closed form.
    fn contacts(r: f64, d: f64) -> ([f64; 2], [f64; 2]) {
        let b = ((r + d) * (r + d) - (r - d) * (r - d)).sqrt();
        let centre = [-b, -d];
        let to = [centre[0], centre[1] + r];
        let n = (to[0] * to[0] + to[1] * to[1]).sqrt();
        ([-b, 0.0], [r * to[0] / n, -r + r * to[1] / n])
    }

    /// Every chord passes through `A = (0, -2r)`, the circle's point opposite the tip, and
    /// the distance through any point of it, `r (x / (2r + y))²`, is the chord's own.
    #[test]
    fn the_chord_pivots_about_the_point_opposite_the_tip() {
        for r in [0.5, 21.0, 1000.0] {
            for d in [1e-3 * r, 0.3 * r, r, 4.0 * r, 9.0 * r] {
                let (flat, wall) = contacts(r, d);
                let a = [0.0, -2.0 * r];
                let u = [flat[0] - a[0], flat[1] - a[1]];
                let v = [wall[0] - a[0], wall[1] - a[1]];
                let cross = u[0] * v[1] - u[1] * v[0];
                let scale = (u[0] * u[0] + u[1] * u[1]).sqrt() * (v[0] * v[0] + v[1] * v[1]).sqrt();
                assert!(cross.abs() <= 1e-12 * scale, "r {r} d {d}: {cross}");
                // the closed form of the contacts, s = sqrt(r d)
                let s = (r * d).sqrt();
                assert!((flat[0] + 2.0 * s).abs() <= 1e-12 * s);
                let q = r * r + s * s;
                assert!((wall[0] + 2.0 * r * r * s / q).abs() <= 1e-12 * r);
                assert!((wall[1] + 2.0 * r * s * s / q).abs() <= 1e-12 * r);
                for t in [0.0, 0.25, 0.5, 1.0] {
                    let p = [
                        flat[0] + t * (wall[0] - flat[0]),
                        flat[1] + t * (wall[1] - flat[1]),
                    ];
                    let got = knife_chamfer_distance_through(r, p[0], p[1]);
                    assert!((got - d).abs() <= 1e-12 * d, "r {r} d {d} t {t}: {got}");
                }
            }
        }
    }

    /// The issue's part (#15250): R 21, tip 62 1/6 from the far side. Its far bottom corner
    /// is reached at TIP² / R, its far top corner at TIP² / 4R, the arc's foot at R.
    #[test]
    fn the_issue_parts_corners_are_reached_in_closed_form() {
        const R: f64 = 21.0;
        const TIP: f64 = 62.0 + 1.0 / 6.0;
        let at = |x: f64, y: f64| knife_chamfer_distance_through(R, x, y);
        assert!((at(-TIP, -R) - TIP * TIP / R).abs() <= 1e-12 * TIP * TIP / R);
        assert!((at(-TIP, 0.0) - TIP * TIP / (4.0 * R)).abs() <= 1e-12 * TIP * TIP);
        assert!((at(-R, -R) - R).abs() <= 1e-12 * R);
        assert!((at(-TIP, -R) - 184.033_068_783_068_8).abs() < 1e-9);
    }
}
