//! The setback solver (not upstream, #13710): the tracer's solver for a chamfer whose sizes are
//! setbacks held at every station of its edge ([`ChamferDefinition::of_setbacks`]). It has no
//! counterpart in SMLib or the fork: upstream's solvers all trace the intersection of two
//! offset surfaces, a ball's centre; a chamfer of setbacks has no ball.
//!
//! | Rust                                  | Upstream |
//! |---------------------------------------|----------|
//! | [`SetbackFs`], [`SETBACK_FS_TYPE`]    | none: a new `FilletSolverOps` kind, modelled on `SmVariableRadiusFS` (curve based) |
//! | [`SetbackFs::from_edgeuse`]           | none (the edge-use part of `SmVariableRadiusFS::SmVariableRadiusFS`, offsets 0) |
//! | [`SetbackFs::load_jacobian`]          | none (`SmVariableRadiusFS::LoadJacobian`'s unknowns, the setbacks' equations) |
//! | `impl FilletSolverOps for SetbackFs`  | none |
//! | [`SetbackFs::station`], [`SetbackFs::station_through`], [`SetbackFs::station_point`], [`SetbackStation`] | none: the exact station at a given s |
//! | [`SetbackFs::compute_uv_vectors`]     | none (`SmFilletSolver::ComputeUVVectors`'s job, each foot's own tangent) |
//! | [`SetbackFs::check_not_folding`]      | none: the fold check |
//! | [`SetbackFs::station_leader`], [`SetbackFs::given_station`] | none: the station trace's leader and given points |
//!
//! **The station** is [`super::super::fillet_section::setback_station`]'s: at a parameter s of
//! the edge C (the extended edge curve, the solver's curve), foot i is the point `S_i` of
//! support i with `|S_i − C(s)| = d_i` and `(S_i − C(s)) · C′(s) = 0`. The solver is curve
//! based, as the variable-radius one: its unknowns are both feet's (u, v) and s
//! (`m_adUserDoubles[0]`, five in all), its offsets are 0 (so its offset surfaces are the
//! faces themselves, which every reader of `GetSurface(i)` takes as the supports), and the
//! traced curve, row 0 of the marcher's system (the plane through the stepped point), is
//! rail 0: foot 0 steps along its own track, the edge's s following. Rows 1 to 4
//! ([`SetbackFs::load_jacobian`]) are `(|S_i − C|² − d_i²) / 2` and `(S_i − C) · C′` for the
//! two feet. Newton does not start on the edge, where `|S − C|` has no derivative: a guess on
//! the edge (the marcher's guesses from the offsets, 0 here) is replaced by the station's
//! feet from `setback_start` and `setback_foot` ([`SetbackFs::load_initial_values`]).
//!
//! **The station trace** (`do_law_intersection_with` at the solver's own stations) takes the
//! extended edge as its leader ([`SetbackFs::station_leader`]), each given point at its own s
//! with its feet kept ([`SetbackFs::given_station`]) and every other station exact at its s
//! ([`SetbackFs::station_point`]).
//!
//! **A clipped step** (a foot on its face's boundary iso-line) solves that foot on the line
//! and s together, then the other foot at that s ([`SetbackFs::clipped_feet`]).
//!
//! **The exact station** at a given s ([`SetbackFs::station`]): both feet by `setback_foot`,
//! each with its motion in s from the implicit function theorem on its own system
//! (`SetbackFoot::per_s`, no difference quotient). A trace point's uv tangents are each
//! foot's own per unit length of foot 0's motion ([`SetbackFs::compute_uv_vectors`]), not the
//! base solver's drop of foot 0's tangent on both surfaces. Every station decides each foot's
//! branch (the setback's circle crosses the extended support twice, about two setbacks apart,
//! once beyond the edge): the foot lies on the face's side of the edge, `(S_i − C) · t_i > 0`
//! with `t_i` the support's tangent into the face that `setback_tangent` picks by the same
//! test against the other support's oriented normal `n_j` (on the wedge's side of it; tested
//! against `t_i`, not `n_j`, so that a wedge near flat, where `(S_i − C) · n_j` is small
//! beside the support's curvature, is not refused); a foot beyond the edge is solved once more
//! from the edge, else refused `SetbackFootMirrored` ([`SetbackFs::station`]). Every station is
//! then checked not to fold: `∂S_i/∂s · C′ > 0`, else `NoRoom(SetbacksCross)`
//! ([`SetbackFs::check_not_folding`]).
//!
//! Deviations: not upstream (no counterpart). Every loop has a fixed count
//! ([`NEWTON_STEPS`]), no recursion.
//!
//! [`ChamferDefinition::of_setbacks`]: crate::surface::procedural_blend::chamfer::ChamferDefinition::of_setbacks

use super::p1::{CurveBasedFs, FilletSolverType, SolverRails, CURVE_BASED_FS_TYPE};
use super::p2::{edgeuse_frame, extending_offset, face_base, rail_slots};
use crate::blend::fillet_custom_solver::{
    offset_evaluate_1st, offset_evaluate_point, offset_local_point_solve, InitialValues,
    JacobianLoad, OffsetSurface,
};
use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
use crate::blend::fillet_section::setback_station::{
    foot_at, on_wedge_side, setback_foot, setback_start, setback_tangent, EdgeJet, SetbackFoot,
};
use crate::blend::fillet_solver::p1::{ExtensionCorners, FilletSolver, FilletSolverOps};
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::blend::{BlendFailure, SolverFailure, TraceFailure};
use crate::error::{ignored, Result, SolidError};
use crate::geom::extent1d::{Extent1d, ExtentNd};
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::line_closest_point;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::krawczyk::NEWTON_STEPS;
use crate::geom::matrix::Matrix;
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::TsectPnt;
use crate::intersect::surface_intersector::p4::StationLeader;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::knots::Side;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as edgeuse;
use crate::topology::{edge, Brep, EdgeuseId, ObjectHandle};

const OP: &str = "blend::fillet_standard_solver::setback";
/// The tolerance a line is recognised within (the variable-radius solver's).
const ANALYTIC_TOL: f64 = 1.0e-8;
const OP_CTOR: &str = "blend::fillet_standard_solver::setback::SetbackFs::from_edgeuse";
const OP_JAC: &str = "blend::fillet_standard_solver::setback::SetbackFs::load_jacobian";
const OP_INIT: &str = "blend::fillet_standard_solver::setback::SetbackFs::load_initial_values";

/// The setback solver's run-time type (not upstream: a number SMLib's `TOPO_BASE_TYPE + n`
/// types do not use).
pub const SETBACK_FS_TYPE: i64 = CURVE_BASED_FS_TYPE + 1000;

/// The solver of a chamfer of setbacks held at every station (module docs): curve based, its
/// curve the edge, its offsets 0.
#[derive(Debug, Clone)]
pub struct SetbackFs {
    /// The `SmCurveBasedFS` part: the edge's curve and its extension.
    pub curve_based: CurveBasedFs,
    /// The setbacks `d_1`, `d_2` from the edge along supports 1 and 2 (rails 0 and 1).
    pub setbacks: [f64; 2],
    /// The edge's interval on its curve.
    pub edge_interval: Extent1d,
}

/// A station of the chamfer: the edge's parameter s, the edge's jet there and both feet, each
/// with its motion in s ([`SetbackFoot::per_s`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetbackStation {
    /// The edge's parameter.
    pub s: f64,
    /// The edge's point and derivatives at s.
    pub jet: EdgeJet,
    /// The feet on supports 1 and 2 (rails 0 and 1).
    pub feet: [SetbackFoot; 2],
}

impl SetbackStation {
    /// Whether both feet's motions in s are finite.
    fn motion_finite(&self) -> bool {
        self.feet.iter().all(|f| {
            f.uv_per_s.x.is_finite()
                && f.uv_per_s.y.is_finite()
                && f.per_s.x.is_finite()
                && f.per_s.y.is_finite()
                && f.per_s.z.is_finite()
        })
    }

    /// Each foot's uv tangent per unit length of foot 0's motion, `∂(u, v)_i/∂s / |∂S_0/∂s|`,
    /// negated where `direction` runs against foot 0's motion (`∂S_0/∂s · direction < 0`;
    /// a direction square to it, or not finite, keeps s's). Foot 0's motion is not zero at a
    /// station the fold check passed (`∂S_0/∂s · C′ > 0`).
    #[must_use]
    pub fn uv_tangents(&self, direction: Vec3) -> [Point2; 2] {
        let forward = self.feet[0].per_s;
        let sign = if forward.dot(direction) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let scale = sign / forward.length();
        self.feet.map(|f| f.uv_per_s * scale)
    }
}

/// The wedge at a station ([`SetbackFs::wedge`]): the edge point's parameters on each
/// support, and each support's tangent into its face on the wedge's side (the other
/// support's oriented normal there).
#[derive(Debug, Clone, Copy)]
struct Wedge {
    /// The edge point's (u, v) on supports 1 and 2.
    edge_uv: [Point2; 2],
    /// Support i's unit tangent into its face at the edge point, square to the edge, on the
    /// side of the other support's normal there times its orientation ([`setback_tangent`]);
    /// `None` where it has none.
    tangent: [Option<Vec3>; 2],
}

fn at(x: &[f64], i: usize, op: &'static str) -> Result<f64> {
    x.get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input(op, "array index out of range"))
}

fn put(f: &mut [f64], i: usize, v: f64, op: &'static str) -> Result<()> {
    let slot = f
        .get_mut(i)
        .ok_or_else(|| SolidError::invalid_input(op, "rF index out of range"))?;
    *slot = v;
    Ok(())
}

fn put_j(j: &mut Matrix, row: usize, col: usize, v: f64, op: &'static str) -> Result<()> {
    j.set_at(row, col, v)
        .map_err(|_| SolidError::invalid_input(op, "Jacobian index out of range"))
}

fn plus(a: usize, b: usize, op: &'static str) -> Result<usize> {
    a.checked_add(b)
        .ok_or_else(|| SolidError::invalid_input(op, "index overflow"))
}

fn minus(a: usize, b: usize, op: &'static str) -> Result<usize> {
    a.checked_sub(b)
        .ok_or_else(|| SolidError::invalid_input(op, "index underflow"))
}

fn interval_at<'a>(ivls: &'a mut ExtentNd, i: usize, op: &'static str) -> Result<&'a mut Extent1d> {
    ivls.get_mut(i)
        .ok_or_else(|| SolidError::invalid_input(op, "rIntervals index out of range"))
}

fn at3(v: &[Vec3], i: usize, op: &'static str) -> Result<Vec3> {
    v.get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input(op, "evaluation shorter than requested"))
}

/// The solution of the 2 × 2 system `[[a, b], [c, d]] x = f`, `None` where it is singular.
fn solve2(m: [[f64; 2]; 2], f: [f64; 2]) -> Option<[f64; 2]> {
    let [[a, b], [c, d]] = m;
    let det = a * d - b * c;
    let x = [(f[0] * d - b * f[1]) / det, (a * f[1] - c * f[0]) / det];
    (det != 0.0 && x[0].is_finite() && x[1].is_finite()).then_some(x)
}

impl SetbackFs {
    /// The solver's run-time type.
    pub const TYPE: i64 = SETBACK_FS_TYPE;

    /// The solver's kind: [`FilletSolverType::Setbacks`].
    #[must_use]
    pub const fn solver_type(&self) -> FilletSolverType {
        FilletSolverType::Setbacks
    }

    /// The solver of the edge of `edgeuse` chamfered by `setbacks` (along the edge use's face
    /// and its radial's): the edge use and its radial, the edge's curve and interval, the
    /// offsets' orientations as the variable-radius solver finds them (`sm_SectorSides` at
    /// the larger setback with `fillet_knife_edge_rounds`, else the binormals at the middle
    /// of the edge), and offset surfaces of both faces at distance 0 (extension allowed). For
    /// faces tangent there the solver has no edge uses or surfaces, as the variable-radius
    /// one. The constructor's fillet geometry is stored in `pseudo` for solver `id` and,
    /// unless the faces are tangent, receives copies of the offsets.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for setbacks that are not finite and positive, or an edge
    /// use without radial, edge, curve, face use or face surface; those of the binormals,
    /// [`OffsetSurface::new`], [`CurveBasedFs::new_in`] and
    /// [`FilletSolver::finish_construction`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_edgeuse(
        k: &Kernel,
        brep: &mut Brep,
        pseudo: &mut FilletBrep,
        id: FilletSolverId,
        approx_tol_3d: f64,
        ang_tol_rad: f64,
        tangency_tolerance: f64,
        eu: EdgeuseId,
        setbacks: [f64; 2],
    ) -> Result<Self> {
        if !setbacks.iter().all(|d| d.is_finite() && *d > 0.0) {
            return Err(SolidError::invalid_input(
                OP_CTOR,
                "a chamfer's setbacks are finite and positive",
            ));
        }
        let mut curve_based = CurveBasedFs::new_in(
            k,
            pseudo,
            id,
            approx_tol_3d,
            ang_tol_rad,
            tangency_tolerance,
        )?;
        let radial = edgeuse::radial(brep, eu)?;
        curve_based.solver.edgeuses = [Some(eu), radial];
        let e = edgeuse::edge(brep, eu)?
            .ok_or_else(|| SolidError::invalid_input(OP_CTOR, "edge use without edge"))?;
        let edge_interval = edge::interval(brep, e)?;
        let original = brep
            .edge(e)?
            .curve
            .clone()
            .ok_or_else(|| SolidError::invalid_input(OP_CTOR, "edge without curve"))?;
        curve_based.original = Some(original);
        let mut fs = Self {
            curve_based,
            setbacks,
            edge_interval,
        };
        let size = setbacks[0].max(setbacks[1]);
        if k.fixes.fillet_knife_edge_rounds {
            let Some(sides) = super::p1::sector_sides(k, brep, [Some(eu), radial], size, OP_CTOR)?
            else {
                // smooth, or no sector: nothing to chamfer
                fs.curve_based.solver.edgeuses = [None, None];
                return Ok(fs);
            };
            fs.curve_based.solver.orientations = sides;
        } else {
            let fr = edgeuse_frame(k, brep, eu, OP_CTOR)?;
            fs.curve_based.solver.orientations = [1.0, 1.0];
            for (side, (bin, normal)) in [
                (fr.bin_vec2, fr.faceuse_normal),
                (fr.bin_vec, fr.faceuse_normal2),
            ]
            .into_iter()
            .enumerate()
            {
                let dot = bin.dot(normal);
                if dot.abs() < EFF_ZERO_SQRT {
                    fs.curve_based.solver.edgeuses = [None, None];
                    return Ok(fs);
                }
                if dot < 0.0 {
                    if let Some(o) = fs.curve_based.solver.orientations.get_mut(side) {
                        *o = -1.0;
                    }
                }
            }
        }
        let eu1 =
            radial.ok_or_else(|| SolidError::invalid_input(OP_CTOR, "edge use without radial"))?;
        let s0 = face_base(brep, eu, OP_CTOR)?;
        let s1 = face_base(brep, eu1, OP_CTOR)?;
        fs.curve_based.solver.surfaces =
            rail_slots(extending_offset(k, 0.0, s0)?, extending_offset(k, 0.0, s1)?);
        fs.curve_based.solver.finish_construction(k, pseudo)?;
        Ok(fs)
    }

    /// The solver's curve: the extended edge where there is one, else the edge's own.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without an edge curve.
    pub fn edge_curve(&self) -> Result<Curve> {
        match &self.curve_based.extended {
            Some(e) => Ok(Curve::from(e.clone())),
            None => self
                .curve_based
                .original
                .clone()
                .ok_or_else(|| SolidError::invalid_input(OP, "no edge curve")),
        }
    }

    /// The edge's point and first two derivatives at `s`, on the extended edge, continued
    /// first to reach `s` ([`Self::reach`]).
    ///
    /// # Errors
    ///
    /// Those of the continuation and the evaluation.
    pub fn jet(&mut self, k: &Kernel, s: f64) -> Result<EdgeJet> {
        self.reach(k, s)?;
        let pv = self.curve_based.evaluate_base_curve(k, s, 2)?;
        Ok(EdgeJet {
            point: at3(&pv, 0, OP)?,
            d1: at3(&pv, 1, OP)?,
            d2: at3(&pv, 2, OP)?,
        })
    }

    /// The extended edge continued to reach `s` past either end, keeping what it reached
    /// past the other: the stations a trace solved on one side stay on the leader when a
    /// later one lies past the other end. ([`CurveBasedFs::evaluate_base_curve`] extends the
    /// original afresh past the one end asked each time, so an extension past the start is
    /// dropped by one past the end: a given point before a line's start then lay before the
    /// leader the definition stores, which no proof covers.) The current curve is continued
    /// as its end span's polynomials (`C-infinity`, as [`Self::set_extension_flags`] extends
    /// a curved edge: the same polynomials however often it is continued, so continuing an
    /// extension never shortens or bends it), past `s` as far again as `s` lies past the end
    /// and at least a tenth of the curve's interval (`evaluate_base_curve`'s rule). Nothing
    /// is done for `s` inside the curve, within that routine's snapping tolerance, or for an
    /// edge that is not a B-spline (which that routine refuses).
    ///
    /// # Errors
    ///
    /// Those of a line's NURB and of the continuation.
    fn reach(&mut self, _k: &Kernel, s: f64) -> Result<()> {
        if !s.is_finite() {
            return Ok(());
        }
        let Some(current) = self.curve_based.base_b_spline_curve()? else {
            return Ok(());
        };
        let ivl = current.natural_interval();
        let snap = EFF_ZERO * (1.0 + ivl.min().abs() + ivl.max().abs());
        if s >= ivl.min() - snap && s <= ivl.max() + snap {
            return Ok(());
        }
        let tenth = ivl.length() / 10.0;
        let target = if s < ivl.min() {
            (s - (ivl.min() - s)).min(ivl.min() - tenth)
        } else {
            (s + (s - ivl.max())).max(ivl.max() + tenth)
        };
        if let Some(continued) =
            current.create_extended_curve(target, ContinuityType::CInfinity, true)?
        {
            self.curve_based.extended = Some(continued);
        }
        Ok(())
    }

    /// The edge's parameter nearest `pnt` (the foot's station: `(S − C) · C′ = 0` there),
    /// continued along the end tangent where it is past the curve's ends.
    ///
    /// # Errors
    ///
    /// `NoSingleCurveFoot` where the drop finds no foot; those of the drop.
    pub fn station_of(&self, k: &Kernel, pnt: Point3) -> Result<f64> {
        let curve = self.edge_curve()?;
        let ivl = curve.natural_interval();
        let sols = curve.global_point_solve(
            k,
            &ivl,
            SolverOperation::Minimize,
            pnt,
            self.curve_based.solver.approx_tol_3d,
            SolutionRequested::All,
            &GlobalPointSolveOptions::<ObjectHandle>::default(),
        )?;
        let mut best: Option<(f64, f64)> = None;
        for sol in sols.as_slice() {
            let t = sol.start.param(0)?;
            let d = curve.evaluate_point(k, t)?.distance(pnt);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((t, d));
            }
        }
        let Some((mut t, _)) = best else {
            return Err(SolidError::blend_solver(
                OP,
                SolverFailure::NoSingleCurveFoot { count: 0 },
            ));
        };
        if !ivl.contains_value(t, -EFF_ZERO) {
            let pv = curve.evaluate(k, t, 1, true, true)?;
            t += line_closest_point(at3(&pv, 0, OP)?, at3(&pv, 1, OP)?, pnt);
        }
        Ok(t)
    }

    /// The station's feet at the edge jet `jet`, each Newton's from `setback_start` off the
    /// edge point's parameters on its support (solved from `guesses`), into the face on the
    /// side of the other support's offset (the wedge's side: the ball of a round sits there);
    /// `None` for a foot not found.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the solves.
    pub fn feet_at(
        &self,
        k: &Kernel,
        jet: &EdgeJet,
        guesses: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<[Option<Point2>; 2]> {
        Ok(self
            .feet_from_edge(k, jet, guesses, domains)?
            .map(|f| f.map(|f| f.uv)))
    }

    /// [`Self::feet_at`]'s feet whole: each foot [`setback_foot`]'s, with its motions.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the solves.
    fn feet_from_edge(
        &self,
        k: &Kernel,
        jet: &EdgeJet,
        guesses: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<[Option<SetbackFoot>; 2]> {
        let wedge = self.wedge(k, jet, guesses, domains)?;
        Ok([
            self.foot_from_edge(k, jet, &wedge, 0)?,
            self.foot_from_edge(k, jet, &wedge, 1)?,
        ])
    }

    /// The wedge at the edge jet `jet`: the edge point's parameters on each support (its drop
    /// there from `guesses`, over `domains`, or the guess where the drop finds nothing),
    /// across each support the other support's normal at its edge point times that
    /// support's orientation (the side of it the wedge's interior lies on: a round's ball's),
    /// and each support's tangent into its face on that side ([`setback_tangent`]).
    ///
    /// # Errors
    ///
    /// Those of the drops, the evaluations and the orientations.
    fn wedge(
        &self,
        k: &Kernel,
        jet: &EdgeJet,
        guesses: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<Wedge> {
        let solver = &self.curve_based.solver;
        let mut edge_uv = guesses;
        let mut normals = [Vec3::ZERO; 2];
        for (rail, (slot, normal)) in edge_uv.iter_mut().zip(normals.iter_mut()).enumerate() {
            let offset = solver.rail(rail)?;
            let guess = *slot;
            let domain = domains
                .get(rail)
                .copied()
                .unwrap_or_else(|| offset.surface.natural_uv_domain());
            let uv = offset_local_point_solve(k, offset, &domain, jet.point, guess)?
                .map_or(guess, |(uv, _)| uv);
            *slot = uv;
            *normal = Surface::from(offset.surface.clone()).evaluate_normal(
                k,
                uv,
                Side::Left,
                Side::Left,
            )?;
        }
        let mut into = [Vec3::ZERO; 2];
        for (rail, slot) in into.iter_mut().enumerate() {
            let other = 1 - rail;
            *slot =
                normals.get(other).copied().unwrap_or(Vec3::ZERO) * solver.orientation(other)?;
        }
        let mut tangent = [None, None];
        for (rail, slot) in tangent.iter_mut().enumerate() {
            let (Some(&uv), Some(&side)) = (edge_uv.get(rail), into.get(rail)) else {
                continue;
            };
            *slot = setback_tangent(k, &self.support(rail)?, uv, jet, side)?;
        }
        Ok(Wedge { edge_uv, tangent })
    }

    /// The foot of rail `rail` from the edge: Newton ([`setback_foot`]) from
    /// [`setback_start`] off the edge point along [`setback_tangent`]'s tangent into the face,
    /// on the wedge's side (`wedge`); `None` where a step is not defined or the foot is not
    /// found.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the solve.
    fn foot_from_edge(
        &self,
        k: &Kernel,
        jet: &EdgeJet,
        wedge: &Wedge,
        rail: usize,
    ) -> Result<Option<SetbackFoot>> {
        let support = self.support(rail)?;
        let (Some(&uv), Some(&Some(t)), Some(&d)) = (
            wedge.edge_uv.get(rail),
            wedge.tangent.get(rail),
            self.setbacks.get(rail),
        ) else {
            return Ok(None);
        };
        let Some(start) = setback_start(k, &support, uv, t, d)? else {
            return Ok(None);
        };
        setback_foot(k, &support, jet, d, start)
    }

    /// Refuses foot `rail` at `point` of the station at `s` (edge jet `jet`) unless it lies
    /// on the face's side of the edge: [`on_wedge_side`] of `point − C(s)` against the
    /// support's tangent into the face (`wedge`'s, which [`setback_tangent`] picks by the
    /// same test against the other support's oriented normal, the side the wedge's interior
    /// lies on). The setback's circle crosses the extended support a second time beyond the
    /// edge, about two setbacks away, which no bound from a span's chord or tangents tells
    /// apart; the two crossings lie near `C ± d t`, so the test holds whatever the wedge's
    /// angle (against the other support's normal itself, `(S − C) · n_j ≈ d sin β`, the
    /// support's curvature would outweigh it in a wedge near flat).
    ///
    /// # Errors
    ///
    /// `SetbackFootMirrored` for a foot beyond the edge, or where the support has no tangent
    /// into its face there.
    fn check_branch(
        jet: &EdgeJet,
        wedge: &Wedge,
        rail: usize,
        point: Point3,
        s: f64,
    ) -> Result<()> {
        let into = wedge.tangent.get(rail).copied().flatten();
        if into.and_then(|t| on_wedge_side(point - jet.point, t)) == Some(true) {
            Ok(())
        } else {
            Err(SolidError::blend_solver(
                OP,
                SolverFailure::SetbackFootMirrored { rail, s },
            ))
        }
    }

    /// Support `rail` as a surface (the offsets are 0: the face's own).
    fn support(&self, rail: usize) -> Result<Surface> {
        Ok(Surface::from(
            self.curve_based.solver.rail(rail)?.surface.clone(),
        ))
    }

    /// The exact station at the edge's parameter `s`: both feet by [`setback_foot`] at the
    /// edge's jet there, each seeded from its guess in `guesses`, or, for a guess on the
    /// edge (nearer `C(s)` than half its setback, as [`Self::load_initial_values`] judges
    /// it, where `|S − C|` has no derivative), from the station's foot off the edge
    /// ([`Self::feet_at`]'s). Each foot's branch is decided at its station: it must lie on
    /// the wedge's side of the other support ([`Self::check_branch`]). A foot found beyond
    /// the edge from its guess (the setback circle's other crossing, which a guess between
    /// stations can reach) is solved once again from the edge, [`setback_start`]'s start
    /// into the face, and refused if that is not on the wedge's side either. Each foot
    /// carries its motion in s from the implicit function theorem on its own system
    /// ([`SetbackFoot::uv_per_s`], [`SetbackFoot::per_s`]), and the stations are checked not
    /// to fold there ([`Self::check_not_folding`]), after the branch. `None` where a foot is
    /// not found or its motion is not finite.
    ///
    /// # Errors
    ///
    /// `SetbackFootMirrored` for a foot on the wrong branch from its guess and from the edge;
    /// `NoRoom(SetbacksCross)` for a folding station; those of the evaluations and the
    /// solves.
    pub fn station(
        &mut self,
        k: &Kernel,
        s: f64,
        guesses: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<Option<SetbackStation>> {
        let jet = self.jet(k, s)?;
        let wedge = self.wedge(k, &jet, guesses, domains)?;
        let mut feet: [Option<SetbackFoot>; 2] = [None, None];
        for (rail, foot) in feet.iter_mut().enumerate() {
            let support = self.support(rail)?;
            let d = self.setbacks.get(rail).copied().unwrap_or(0.0);
            let guess = guesses.get(rail).copied().unwrap_or(Point2::new(0.0, 0.0));
            let on_edge = support.evaluate_point(k, guess)?.distance(jet.point) < 0.5 * d;
            let solved = if on_edge {
                self.foot_from_edge(k, &jet, &wedge, rail)?
            } else {
                setback_foot(k, &support, &jet, d, guess)?
            };
            let Some(mut found) = solved else {
                return Ok(None);
            };
            if !on_edge && Self::check_branch(&jet, &wedge, rail, found.point, s).is_err() {
                // the other crossing: once more from the edge, into the face
                match self.foot_from_edge(k, &jet, &wedge, rail)? {
                    Some(again) => found = again,
                    None => {
                        return Err(SolidError::blend_solver(
                            OP,
                            SolverFailure::SetbackFootMirrored { rail, s },
                        ))
                    }
                }
            }
            Self::check_branch(&jet, &wedge, rail, found.point, s)?;
            *foot = Some(found);
        }
        Self::checked_station(s, jet, feet)
    }

    /// The station at `s` through the feet `uvs` as they stand (no solve): each foot's
    /// motion in s at them ([`SetbackFoot::per_s`]), each foot's branch checked
    /// ([`Self::check_branch`], the wedge from the edge point's drops seeded at `uvs` over
    /// `domains`; feet given are never moved, so a foot beyond the edge is refused, not
    /// re-solved), then checked not to fold. `None` where a foot's system is singular there
    /// or its motion is not finite. The feet are taken as the station's roots; the motion is
    /// the root's only within the solve's tolerance.
    ///
    /// # Errors
    ///
    /// `SetbackFootMirrored` for a foot beyond the edge; `NoRoom(SetbacksCross)` for a
    /// folding station; those of the evaluations.
    pub fn station_through(
        &mut self,
        k: &Kernel,
        s: f64,
        uvs: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<Option<SetbackStation>> {
        let jet = self.jet(k, s)?;
        let wedge = self.wedge(k, &jet, uvs, domains)?;
        let mut feet: [Option<SetbackFoot>; 2] = [None, None];
        for (rail, foot) in feet.iter_mut().enumerate() {
            let support = self.support(rail)?;
            let d = self.setbacks.get(rail).copied().unwrap_or(0.0);
            let uv = uvs.get(rail).copied().unwrap_or(Point2::new(0.0, 0.0));
            *foot = foot_at(k, &support, &jet, d, uv)?;
            if let Some(f) = foot {
                Self::check_branch(&jet, &wedge, rail, f.point, s)?;
            }
        }
        Self::checked_station(s, jet, feet)
    }

    /// The station of `feet` at `s`: `None` without both feet or with a motion that is not
    /// finite, else the station once it passes [`Self::check_not_folding`].
    ///
    /// # Errors
    ///
    /// `NoRoom(SetbacksCross)` for a folding station.
    fn checked_station(
        s: f64,
        jet: EdgeJet,
        feet: [Option<SetbackFoot>; 2],
    ) -> Result<Option<SetbackStation>> {
        let [Some(f0), Some(f1)] = feet else {
            return Ok(None);
        };
        let station = SetbackStation {
            s,
            jet,
            feet: [f0, f1],
        };
        if !station.motion_finite() {
            return Ok(None);
        }
        Self::check_not_folding(&station)?;
        Ok(Some(station))
    }

    /// The fold check (#13710): at every station each foot moves
    /// forward along the edge, `∂S_i/∂s · C′ > 0`. At the root that is
    /// `|C′|² − (S_i − C) · C″ > 0`, the foot short of the edge's polar line
    /// ([`crate::blend::fillet_section::setback_station`]'s crossing); past it neighbouring
    /// stations' planes meet inside the chamfer and its feet run backwards.
    ///
    /// # Errors
    ///
    /// `NoRoom(SetbacksCross)` where a foot does not move forward.
    pub fn check_not_folding(station: &SetbackStation) -> Result<()> {
        for foot in &station.feet {
            // a NaN is refused too (motion_finite has rejected it first)
            let forward = foot.per_s.dot(station.jet.d1);
            if forward.is_nan() || forward <= 0.0 {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::NoRoom(NoRoomReason::SetbacksCross),
                ));
            }
        }
        Ok(())
    }

    /// Whether `foot0` is at foot 0's station at `s`: its reach `|S_0 − C(s)|` within the
    /// solver's 3D tolerance of its setback, and its square `(S_0 − C(s)) · C′(s) / |C′(s)|`
    /// within that tolerance of 0.
    ///
    /// # Errors
    ///
    /// Those of the edge's evaluation.
    fn foot0_at_station(&mut self, k: &Kernel, s: f64, foot0: Point3) -> Result<bool> {
        if !s.is_finite() {
            return Ok(false);
        }
        let tol = self.curve_based.solver.approx_tol_3d;
        let jet = self.jet(k, s)?;
        let g = foot0 - jet.point;
        let speed = jet.d1.length();
        let d0 = self.setbacks[0];
        Ok(speed > 0.0 && (g.length() - d0).abs() <= tol && (g.dot(jet.d1) / speed).abs() <= tol)
    }

    /// The edge's parameter of the trace point `point`: `user_doubles[0]`, where every
    /// solve of this solver leaves s, when foot 0 is at its station there
    /// ([`Self::foot0_at_station`]); else foot 0's nearest point on the edge
    /// ([`Self::station_of`]), as for a point no solve of this solver made, held to the same
    /// test. The drop minimises the distance, so it finds no station past the edge's polar
    /// line, where the station is a farthest point (`∂²|S − C|²/2∂s² = |C′|² − (S − C) · C″ < 0`):
    /// such a point is refused, never answered at the drop's s.
    ///
    /// # Errors
    ///
    /// `SetbackStationNotFound` where neither s puts foot 0 at its station; those of the
    /// evaluations and the drop.
    fn station_parameter(&mut self, k: &Kernel, point: &TsectPnt) -> Result<f64> {
        let foot0 = point.surface_pv[0][0][0];
        let s = point.user_doubles[0];
        if self.foot0_at_station(k, s, foot0)? {
            return Ok(s);
        }
        let dropped = self.station_of(k, foot0)?;
        if self.foot0_at_station(k, dropped, foot0)? {
            return Ok(dropped);
        }
        Err(SolidError::blend_solver(
            OP,
            SolverFailure::SetbackStationNotFound { s: dropped },
        ))
    }

    /// `ComputeUVVectors` of a chamfer of setbacks: each foot's own uv tangent, from the
    /// implicit derivative of its station system in s ([`Self::station_through`] at the
    /// point's feet `uv_curve_pv[i][0]` and its s, [`Self::station_parameter`]), per unit
    /// length of foot 0's motion (the trace's curve is rail 0, its tangent foot 0's unit
    /// chord), oriented as the point's curve tangent `curve_pv[1]` runs along foot 0's
    /// motion (with s where that tangent is not set or is square to it). Not the base
    /// solver's drop of foot 0's tangent on both surfaces, which gives foot 1 foot 0's
    /// speed (on a rim the feet run on circles of different radii). The point's surface
    /// values are the caller's (`ComputeSurfaceValues`, as for the base).
    ///
    /// # Errors
    ///
    /// `NoRoom(SetbacksCross)` for a folding station; `NoRoom(SetbackFootTangent)` where a
    /// foot's system is singular at the point or its motion is not finite (the support's
    /// section touches the setback's circle there: the foot's tangent is unbounded and the
    /// asked chamfer does not fit); `SetbackStationNotFound` where foot 0 is at no station
    /// ([`Self::station_parameter`]); those of the evaluations and the drop.
    pub fn compute_uv_vectors(&mut self, k: &Kernel, point: &mut TsectPnt) -> Result<()> {
        let s = self.station_parameter(k, point)?;
        let uvs = [point.uv_curve_pv[0][0], point.uv_curve_pv[1][0]];
        let domains = self.trace_domains(k)?;
        let Some(station) = self.station_through(k, s, uvs, &domains)? else {
            return Err(SolidError::blend(
                OP,
                BlendFailure::NoRoom(NoRoomReason::SetbackFootTangent),
            ));
        };
        let [t0, t1] = station.uv_tangents(point.curve_pv[1]);
        point.uv_curve_pv[0][1] = t0;
        point.uv_curve_pv[1][1] = t1;
        Ok(())
    }

    /// The trace point at the exact station at `s` ([`Self::station`], from `guesses`): the
    /// surface values at the feet (`ComputeSurfaceValues`, its curve point foot 0), its
    /// curve tangent foot 0's unit motion with s, its uv tangents each foot's own
    /// ([`SetbackStation::uv_tangents`]), and s in `user_doubles[0]` and `[1]`. `None` where
    /// the station is not found.
    ///
    /// # Errors
    ///
    /// `NoRoom(SetbacksCross)` for a folding station; those of the station and of
    /// `ComputeSurfaceValues`.
    pub fn station_point(
        &mut self,
        k: &Kernel,
        s: f64,
        guesses: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<Option<TsectPnt>> {
        let Some(station) = self.station(k, s, guesses, domains)? else {
            return Ok(None);
        };
        let mut point = TsectPnt::new();
        FilletSolver::compute_surface_values(
            k,
            self,
            [station.feet[0].uv, station.feet[1].uv],
            &mut point,
        )?;
        Self::put_station(&mut point, &station);
        Ok(Some(point))
    }

    /// The station's tangents and s in the trace point `point`: its curve tangent foot 0's
    /// unit motion with s, its uv tangents each foot's own ([`SetbackStation::uv_tangents`]),
    /// s in `user_doubles[0]` and `[1]`.
    fn put_station(point: &mut TsectPnt, station: &SetbackStation) {
        let forward = station.feet[0].per_s;
        point.curve_pv[1] = forward * (1.0 / forward.length());
        let [t0, t1] = station.uv_tangents(forward);
        point.uv_curve_pv[0][1] = t0;
        point.uv_curve_pv[1][1] = t1;
        point.user_doubles[0] = station.s;
        point.user_doubles[1] = station.s;
    }

    /// The leader of the solver's stations, for a station trace
    /// ([`crate::intersect::surface_intersector::p4::TraceStations::Solver`]): the extended
    /// edge's interval, whether the edge is closed (its ends within the 3D tolerance over
    /// [`Self::edge_interval`]; the extension of a closed edge is not), and the larger
    /// setback as the stand-off.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without an edge curve; those of the evaluations.
    pub fn station_leader(&self, k: &Kernel) -> Result<StationLeader> {
        let interval = self.edge_curve()?.natural_interval();
        let original = self
            .curve_based
            .original
            .as_ref()
            .ok_or_else(|| SolidError::invalid_input(OP, "no edge curve"))?;
        let start = original.evaluate_point(k, self.edge_interval.min())?;
        let end = original.evaluate_point(k, self.edge_interval.max())?;
        Ok(StationLeader {
            interval,
            closed: start.distance(end) <= self.curve_based.solver.approx_tol_3d,
            stand_off: self.setbacks[0].max(self.setbacks[1]),
        })
    }

    /// The edge parameter of the trace point `point` whatever made it (a rail end the corner
    /// code placed may carry a stale `user_doubles[0]`): its surface values at its own feet
    /// (`uv_curve_pv[i][0]`), then [`Self::station_parameter`] on them.
    ///
    /// # Errors
    ///
    /// Those of `ComputeSurfaceValues` and [`Self::station_parameter`]
    /// (`SetbackStationNotFound`).
    pub fn station_of_point(&mut self, k: &Kernel, point: &TsectPnt) -> Result<f64> {
        let uvs = [point.uv_curve_pv[0][0], point.uv_curve_pv[1][0]];
        let mut at = point.clone();
        FilletSolver::compute_surface_values(k, self, uvs, &mut at)?;
        self.station_parameter(k, &at)
    }

    /// A point given to a station trace (a rail end the corner code placed) as its station:
    /// its feet as given (`uv_curve_pv[i][0]`, never moved), its surface values there, its s
    /// ([`Self::station_parameter`]: `user_doubles[0]` where foot 0 is at its station there,
    /// else foot 0's drop on the edge, held to the same test), the tangents of the station
    /// through its feet ([`Self::station_through`], along increasing s) and s in
    /// `user_doubles[0]` and `[1]`. The station at that s solved from the feet
    /// ([`Self::station`]) must have each foot within `tol` of the given one: a given point
    /// off the chamfer is refused, not moved onto it.
    ///
    /// # Errors
    ///
    /// `SetbackFootMirrored` for a given foot beyond the edge; `SetbackFootOffStation` for a
    /// foot farther than `tol` from its station; `StationNotSolved` where the station is not
    /// found; `NoRoom(SetbackFootTangent)` where a foot's system is singular at the given
    /// feet; those of
    /// [`Self::station_parameter`] and [`Self::station`].
    pub fn given_station(
        &mut self,
        k: &Kernel,
        point: &mut TsectPnt,
        tol: f64,
        domains: &[Extent2d; 2],
    ) -> Result<()> {
        let uvs = [point.uv_curve_pv[0][0], point.uv_curve_pv[1][0]];
        let mut at = point.clone();
        FilletSolver::compute_surface_values(k, self, uvs, &mut at)?;
        let s = self.station_parameter(k, &at)?;
        let jet = self.jet(k, s)?;
        let wedge = self.wedge(k, &jet, uvs, domains)?;
        for (rail, pv) in at.surface_pv.iter().enumerate() {
            Self::check_branch(&jet, &wedge, rail, pv[0][0], s)?;
        }
        let solved = self
            .station(k, s, uvs, domains)?
            .ok_or_else(|| SolidError::blend_solver(OP, SolverFailure::StationNotSolved { s }))?;
        for (rail, foot) in solved.feet.iter().enumerate() {
            let given = at.surface_pv.get(rail).map_or(Vec3::UNDEF, |pv| pv[0][0]);
            let moved = foot.point.distance(given);
            if moved.is_nan() || moved > tol {
                return Err(SolidError::blend_solver(
                    OP,
                    SolverFailure::SetbackFootOffStation { rail, moved },
                ));
            }
        }
        let Some(through) = self.station_through(k, s, uvs, domains)? else {
            return Err(SolidError::blend(
                OP,
                BlendFailure::NoRoom(NoRoomReason::SetbackFootTangent),
            ));
        };
        Self::put_station(&mut at, &through);
        *point = at;
        Ok(())
    }

    /// The base surface domains the solver's feet are sought over: each rail's natural
    /// domain, extended as the trace extends it ([`FilletSolver::domain_extension`]).
    ///
    /// # Errors
    ///
    /// Those of the domains and the extension.
    pub fn trace_domains(&self, k: &Kernel) -> Result<[Extent2d; 2]> {
        self.curve_based.solver.trace_domains(k)
    }

    /// `LoadJacobian` of the setback solver: with `x = [.. u1 v1 u2 v2 s ..]` (the UV points
    /// located as `SmVariableRadiusFS::LoadJacobian` locates them, s the edge's parameter),
    /// `f[ne..ne+4]` = `(|S_0 − C|² − d_1²) / 2`, `(S_0 − C) · C′`, `(|S_1 − C|² − d_2²) / 2`,
    /// `(S_1 − C) · C′` (`S_i` the supports' points, the offsets being 0; C the extended
    /// edge), and the Jacobian rows `ne..ne+4`. Advances `num_equations` by 4 and
    /// `num_parameters` by 1 plus 2 per surface whose UV point is not already in `x`. Returns
    /// whether the point is a root (each residual, as a distance, within the conversion
    /// tolerance).
    ///
    /// # Errors
    ///
    /// `BaseDerivativeZero` for a support with a zero first derivative; those of the
    /// evaluations; [`SolidError::InvalidInput`] for an out-of-range index or a missing
    /// surface or curve.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn load_jacobian(
        &mut self,
        k: &Kernel,
        num_equations: &mut usize,
        num_parameters: &mut usize,
        load: &JacobianLoad<'_>,
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        const OP: &str = OP_JAC;
        let rail_index = load.rail_index;
        let x = load.x;
        let base0 = self.curve_based.solver.rail(0)?.base_id;
        let base1 = self.curve_based.solver.rail(1)?.base_id;

        let mut surf1_off = *num_parameters;
        *num_parameters = plus(*num_parameters, 2, OP)?;
        if rail_index == 0 && load.surface1 == Some(base0) {
            surf1_off = load.surf1_offset;
            *num_parameters = minus(*num_parameters, 2, OP)?;
        }
        if rail_index == 1 && load.surface2 == Some(base0) {
            surf1_off = load.surf2_offset;
            *num_parameters = minus(*num_parameters, 2, OP)?;
        }

        let mut surf2_off = *num_parameters;
        *num_parameters = plus(*num_parameters, 2, OP)?;
        if rail_index == 1 && load.surface1 == Some(base1) {
            surf2_off = load.surf1_offset;
            *num_parameters = minus(*num_parameters, 2, OP)?;
        }
        if rail_index == 0 && load.surface2 == Some(base1) {
            surf2_off = load.surf2_offset;
            *num_parameters = minus(*num_parameters, 2, OP)?;
        }

        let s = at(x, *num_parameters, OP)?;
        *num_parameters = plus(*num_parameters, 1, OP)?;
        let s_column = minus(*num_parameters, 1, OP)?;

        let uv0 = Point2::new(at(x, surf1_off, OP)?, at(x, plus(surf1_off, 1, OP)?, OP)?);
        let uv1 = Point2::new(at(x, surf2_off, OP)?, at(x, plus(surf2_off, 1, OP)?, OP)?);
        let (p0, p0u, p0v) = offset_evaluate_1st(k, self.curve_based.solver.rail_mut(0)?, uv0)?;
        let (p1, p1u, p1v) = offset_evaluate_1st(k, self.curve_based.solver.rail_mut(1)?, uv1)?;
        if [p0u, p0v, p1u, p1v]
            .iter()
            .any(|d| d.length_squared() < EFF_ZERO_SQ)
        {
            return Err(SolidError::blend_trace(
                OP,
                TraceFailure::BaseDerivativeZero,
            ));
        }
        let jet = self.jet(k, s)?;
        let (c, c1, c2) = (jet.point, jet.d1, jet.d2);
        let [d0, d1] = self.setbacks;
        let g0 = p0 - c;
        let g1 = p1 - c;
        let rows = [
            (0.5 * (g0.dot(g0) - d0 * d0), g0, p0u, p0v, surf1_off),
            (g0.dot(c1), g0, p0u, p0v, surf1_off),
            (0.5 * (g1.dot(g1) - d1 * d1), g1, p1u, p1v, surf2_off),
            (g1.dot(c1), g1, p1u, p1v, surf2_off),
        ];
        if let Some(j) = jacobian {
            let mut i = *num_equations;
            for (q, &(_, g, su, sv, col)) in rows.iter().enumerate() {
                let col1 = plus(col, 1, OP)?;
                if q % 2 == 0 {
                    // the reach: d/du = g . S_u, d/ds = -g . C'
                    put_j(j, i, col, g.dot(su), OP)?;
                    put_j(j, i, col1, g.dot(sv), OP)?;
                    put_j(j, i, s_column, -g.dot(c1), OP)?;
                } else {
                    // square to the edge: d/du = S_u . C', d/ds = -C' . C' + g . C''
                    put_j(j, i, col, su.dot(c1), OP)?;
                    put_j(j, i, col1, sv.dot(c1), OP)?;
                    put_j(j, i, s_column, g.dot(c2) - c1.dot(c1), OP)?;
                }
                i = plus(i, 1, OP)?;
            }
        }
        for &(v, ..) in &rows {
            put(f, *num_equations, v, OP)?;
            *num_equations = plus(*num_equations, 1, OP)?;
        }

        // each residual as a distance: the reach over its setback, the square over |C'|
        let speed = c1.length();
        let scaled_tol = self.curve_based.solver.conversion_tol * (1.0 + p0.max_dimension());
        let found = speed > 0.0
            && (rows[0].0 / d0).abs() < scaled_tol
            && (rows[1].0 / speed).abs() < scaled_tol
            && (rows[2].0 / d1).abs() < scaled_tol
            && (rows[3].0 / speed).abs() < scaled_tol;
        Ok(found)
    }

    /// `LoadInitialValues` of the setback solver, with `VariableRadiusFs::load_initial_values`'s
    /// arguments (`surface1` the solver's own rail `rail_index`): the point of that rail at
    /// the guess `guess_t[surface1_index..+2]` is dropped on the extended edge, its station
    /// s; the station's feet ([`Self::feet_at`]) replace a guess that lies on the edge (nearer
    /// it than half its setback, where the marcher's guesses from the offsets, 0 here, put
    /// it). With `do_surf2_calcs` the other rail's extended base domain, two `false`
    /// periodicities and its foot (or, where it is not found, the edge point's parameters on
    /// it) are appended. Then always: s's interval (the edge's extended by `5
    /// m_dSurfaceExtensionFactor` lengths), a `false` periodicity and s.
    ///
    /// # Errors
    ///
    /// Those of the drop, the evaluations and the solves; [`SolidError::InvalidInput`] for an
    /// out-of-range index or a rail index above 1.
    #[allow(clippy::too_many_arguments)]
    pub fn load_initial_values(
        &mut self,
        k: &Kernel,
        brep: Option<&Brep>,
        do_surf2_calcs: bool,
        rail_index: usize,
        surface1_index: usize,
        surface2_index: usize,
        intervals: &mut ExtentNd,
        periodicities: &mut Vec<bool>,
        guess_t: &mut Vec<f64>,
    ) -> Result<Option<InitialValues>> {
        const OP: &str = OP_INIT;
        if rail_index > 1 {
            return Err(SolidError::invalid_input(OP, "rail index out of range"));
        }
        let other = 1 - rail_index;
        let i1 = surface1_index;
        let uv = Point2::new(at(guess_t, i1, OP)?, at(guess_t, plus(i1, 1, OP)?, OP)?);
        let pnt = offset_evaluate_point(k, self.curve_based.solver.rail_mut(rail_index)?, uv)?;
        let s0 = self.station_of(k, pnt)?;
        let jet = self.jet(k, s0)?;
        let domains = self.trace_domains(k)?;

        // the other rail's guess: its own in guess_t, or the edge point on it (through the
        // other edge use's UV trim curve where it has one at the station)
        let other_guess = if do_surf2_calcs {
            let mut g = None;
            if let (Some(b), Some(eu)) = (
                brep,
                self.curve_based
                    .solver
                    .edgeuses
                    .get(other)
                    .copied()
                    .flatten(),
            ) {
                if let (Some(uv_curve), _) = edgeuse::uv_trim_curve(b, eu)? {
                    let inside = s0
                        .max(self.edge_interval.min())
                        .min(self.edge_interval.max());
                    let w = uv_curve.evaluate_point(k, inside)?;
                    g = Some(Point2::new(w.x, w.y));
                }
            }
            if let Some(g) = g {
                g
            } else {
                let surface = self.curve_based.solver.rail(other)?;
                let domain = domains.get(other).copied().unwrap_or_default();
                let sols = crate::blend::fillet_custom_solver::surface_global_point_solve(
                    k,
                    &Surface::from(surface.surface.clone()),
                    None,
                    &domain,
                    jet.point,
                    EFF_ZERO_SQ,
                )?;
                sols.first().copied().unwrap_or(Point2::new(0.0, 0.0))
            }
        } else {
            let i2 = surface2_index;
            Point2::new(at(guess_t, i2, OP)?, at(guess_t, plus(i2, 1, OP)?, OP)?)
        };
        let mut guesses = [uv, uv];
        if let Some(g) = guesses.get_mut(other) {
            *g = other_guess;
        }
        let feet = self.feet_at(k, &jet, guesses, &domains)?;

        // a guess on the edge is replaced by its foot
        let d_rail = self.setbacks.get(rail_index).copied().unwrap_or(0.0);
        if pnt.distance(jet.point) < 0.5 * d_rail {
            if let Some(Some(foot)) = feet.get(rail_index) {
                put(guess_t, i1, foot.x, OP)?;
                put(guess_t, plus(i1, 1, OP)?, foot.y, OP)?;
            }
        }
        let foot_other = feet.get(other).copied().flatten();

        let mut surface2_index = surface2_index;
        let mut out = None;
        if do_surf2_calcs {
            let n = guess_t.len();
            let domain = domains.get(other).copied().unwrap_or_default();
            *interval_at(intervals, n, OP)? =
                Extent1d::from_min_max(domain.min().x, domain.max().x);
            *interval_at(intervals, plus(n, 1, OP)?, OP)? =
                Extent1d::from_min_max(domain.min().y, domain.max().y);
            periodicities.push(false);
            periodicities.push(false);
            surface2_index = n;
            let g = foot_other.unwrap_or(other_guess);
            guess_t.push(g.x);
            guess_t.push(g.y);
            out = Some(InitialValues {
                surface2_rail: other,
                surface2_index,
            });
        } else {
            // the other guess on the edge: its foot
            let p =
                offset_evaluate_point(k, self.curve_based.solver.rail_mut(other)?, other_guess)?;
            let d_other = self.setbacks.get(other).copied().unwrap_or(0.0);
            if let (true, Some(foot)) = (p.distance(jet.point) < 0.5 * d_other, foot_other) {
                put(guess_t, surface2_index, foot.x, OP)?;
                put(guess_t, plus(surface2_index, 1, OP)?, foot.y, OP)?;
            }
        }

        let ivl = self.edge_curve()?.natural_interval();
        let edge_extension =
            5.0 * self.curve_based.solver.surface_extension_factor * (ivl.max() - ivl.min());
        *interval_at(intervals, plus(surface2_index, 2, OP)?, OP)? =
            Extent1d::from_min_max(ivl.min() - edge_extension, ivl.max() + edge_extension);
        periodicities.push(false);
        guess_t.push(s0);
        Ok(out)
    }

    /// A clipped step's next point: the foot of rail `si` on its boundary iso-line
    /// (`surf_param` held at `iso_param`) and its station s, solved together by Newton on
    /// `[(|S − C(s)|² − d²) / 2, (S − C(s)) · C′(s)]` in (the free parameter, s), from the
    /// guess `uv` and the station of its point; then the other foot at that s
    /// ([`Self::feet_at`], from its guess). `None` where a solve does not settle within
    /// [`NEWTON_STEPS`] steps or a foot is not found.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the drops.
    pub fn clipped_feet(
        &mut self,
        k: &Kernel,
        si: usize,
        surf_param: SurfParamType,
        iso_param: f64,
        uv: [Point2; 2],
        domains: &[Extent2d; 2],
    ) -> Result<Option<([Point2; 2], f64)>> {
        let oi = 1 - si;
        let guess = uv.get(si).copied().unwrap_or(Point2::new(0.0, 0.0));
        let on_line = |t: f64| {
            if surf_param == SurfParamType::U {
                Point2::new(iso_param, t)
            } else {
                Point2::new(t, iso_param)
            }
        };
        let mut t = if surf_param == SurfParamType::U {
            guess.y
        } else {
            guess.x
        };
        let p = offset_evaluate_point(k, self.curve_based.solver.rail_mut(si)?, on_line(t))?;
        let mut s = self.station_of(k, p)?;
        let d = self.setbacks.get(si).copied().unwrap_or(0.0);
        let mut settled = false;
        for _ in 0..NEWTON_STEPS {
            let (p, pu, pv) =
                offset_evaluate_1st(k, self.curve_based.solver.rail_mut(si)?, on_line(t))?;
            let pt = if surf_param == SurfParamType::U {
                pv
            } else {
                pu
            };
            let jet = self.jet(k, s)?;
            let g = p - jet.point;
            let f = [0.5 * (g.dot(g) - d * d), g.dot(jet.d1)];
            let m = [
                [g.dot(pt), -g.dot(jet.d1)],
                [pt.dot(jet.d1), g.dot(jet.d2) - jet.d1.dot(jet.d1)],
            ];
            let Some([dt, ds]) = solve2(m, f) else {
                return Ok(None);
            };
            t -= dt;
            s -= ds;
            if dt.abs() <= 1.0e-12 * (1.0 + t.abs()) && ds.abs() <= 1.0e-12 * (1.0 + s.abs()) {
                settled = true;
                break;
            }
        }
        if !settled {
            return Ok(None);
        }
        let jet = self.jet(k, s)?;
        let mut guesses = uv;
        if let Some(g) = guesses.get_mut(si) {
            *g = on_line(t);
        }
        let feet = self.feet_at(k, &jet, guesses, domains)?;
        let Some(Some(other)) = feet.get(oi).copied() else {
            return Ok(None);
        };
        let mut out = uv;
        if let Some(g) = out.get_mut(si) {
            *g = on_line(t);
        }
        if let Some(g) = out.get_mut(oi) {
            *g = other;
        }
        Ok(Some((out, s)))
    }

    /// `SetExtensionFlags`: the base's flags, then (as the variable-radius solver extends its
    /// law's edge) an edge of degree above 1 that is not a line and has no extension yet
    /// extended (`C-infinity`) by its interval length at both ends: the leader the chamfer's
    /// definition stores, as written.
    ///
    /// # Errors
    ///
    /// Those of the base flags; [`SolidError::InvalidInput`] without edge curve.
    pub fn set_extension_flags(
        &mut self,
        k: &Kernel,
        b: &Brep,
        corners: &dyn ExtensionCorners,
    ) -> Result<()> {
        self.curve_based.solver.set_extension_flags(b, corners)?;
        if self.curve_based.extended.is_some() {
            return Ok(());
        }
        let original = self
            .curve_based
            .original
            .as_ref()
            .ok_or_else(|| SolidError::invalid_input(OP, "no original curve"))?;
        let Some(bsc) = self.curve_based.base_b_spline_curve()? else {
            return Ok(());
        };
        if original.degree()? > 1 && original.is_line(k, 5, ANALYTIC_TOL)?.is_none() {
            let ivl = original.natural_interval();
            let first = ignored(bsc.create_extended_curve(
                ivl.min() - ivl.length(),
                ContinuityType::CInfinity,
                true,
            ))?
            .flatten();
            let Some(first) = first else {
                return Ok(());
            };
            let second = ignored(first.create_extended_curve(
                ivl.max() + ivl.length(),
                ContinuityType::CInfinity,
                true,
            ))?
            .flatten();
            self.curve_based.extended = second.or(Some(first));
        }
        Ok(())
    }

    /// The extended edge as a B-spline curve: the leader the definition stores.
    ///
    /// # Errors
    ///
    /// Those of a line's NURB.
    pub fn leader(&self) -> Result<Option<BSplineCurve>> {
        self.curve_based.base_b_spline_curve()
    }
}

impl FilletSolverOps for SetbackFs {
    fn solver(&self) -> &FilletSolver {
        &self.curve_based.solver
    }

    fn solver_mut(&mut self) -> &mut FilletSolver {
        &mut self.curve_based.solver
    }

    /// 5: both feet's (u, v) and s.
    fn jacobian_size(&self) -> Result<usize> {
        Ok(5)
    }

    /// The offsets stay 0 (no ball): the stand-off reported is 0.
    fn setup_offset_values(
        &mut self,
        _tsect: &TsectPnt,
        offset_dist: Option<&mut f64>,
    ) -> Result<()> {
        if let Some(out) = offset_dist {
            *out = 0.0;
        }
        Ok(())
    }

    /// The feet's stand-off from each other changes along the edge.
    fn offset_radii_can_change(&self) -> Result<bool> {
        Ok(true)
    }

    /// No analytic chamfer of setbacks is made by the tracer's analytic step.
    fn can_make_analytic(&self) -> Result<bool> {
        Ok(false)
    }

    /// [`SetbackFs::load_jacobian`].
    fn load_jacobian(
        &mut self,
        k: &Kernel,
        num_equations: &mut usize,
        num_parameters: &mut usize,
        load: &mut JacobianLoad<'_>,
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        SetbackFs::load_jacobian(self, k, num_equations, num_parameters, load, f, jacobian)
    }

    /// [`SetbackFs::load_initial_values`], on the solver's own rail surface (copied from
    /// `surface1` and back, as the variable-radius solver's); `rlSurface2Index` is 2 on entry.
    fn load_initial_values(
        &mut self,
        k: &Kernel,
        brep: Option<&mut Brep>,
        do_surf2_calcs: bool,
        rail_index: usize,
        surface1: &mut OffsetSurface,
        surface1_index: usize,
        intervals: &mut ExtentNd,
        periodicities: &mut Vec<bool>,
        guess_t: &mut Vec<f64>,
    ) -> Result<Option<InitialValues>> {
        self.curve_based
            .solver
            .rail_mut(rail_index)?
            .clone_from(surface1);
        let r = SetbackFs::load_initial_values(
            self,
            k,
            brep.as_deref(),
            do_surf2_calcs,
            rail_index,
            surface1_index,
            2,
            intervals,
            periodicities,
            guess_t,
        );
        surface1.clone_from(self.curve_based.solver.rail(rail_index)?);
        r
    }

    /// [`SetbackFs::set_extension_flags`].
    fn set_extension_flags(
        &mut self,
        k: &Kernel,
        b: &Brep,
        corners: &dyn ExtensionCorners,
    ) -> Result<()> {
        SetbackFs::set_extension_flags(self, k, b, corners)
    }

    fn type_code(&self) -> Result<i64> {
        Ok(SETBACK_FS_TYPE)
    }

    /// Its own type or a type a curve-based solver is.
    fn is_kind_of(&self, t: i64) -> Result<bool> {
        Ok(t == SETBACK_FS_TYPE || t == CURVE_BASED_FS_TYPE || FilletSolver::is_kind_of(t))
    }

    fn solver_type(&self) -> Result<FilletSolverType> {
        Ok(SetbackFs::solver_type(self))
    }

    fn setback_mut(&mut self) -> Option<&mut SetbackFs> {
        Some(self)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::blend::fillet_custom_solver::SurfaceId;
    use crate::intersect::surface_intersector::p1::SrfIndex;
    use crate::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};
    use crate::nurbs::b_spline_surface::p1::CanonicalSurfaceData;
    use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};

    const WALL: f64 = 8.0;
    const HEIGHT: f64 = 10.0;

    /// A quarter circle of radius `r` about the z axis at height `z`, from (r, 0) to (0, r):
    /// the rational quadratic arc, its parameter s in [0, 1].
    fn quarter(r: f64, z: f64) -> BSplineCurve {
        let w = 0.5_f64.sqrt();
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &[
                Point3::new(r, 0.0, z),
                Point3::new(r, r, z),
                Point3::new(0.0, r, z),
            ],
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[3, 3],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0, w, 1.0]),
            trim: None,
        })
        .unwrap()
    }

    /// The plate z = 0 over [-20, 20]^2 and three quarters of a wall of radius 8 standing on
    /// it, from -90 to 180 degrees (u = 1/3 + s/3 on the middle quarter, the edge's).
    fn supports() -> [Surface; 2] {
        plate_and_wall(20.0, 0.0, WALL, [0.0, HEIGHT])
    }

    /// The plate z = `plate_z` over [-w, w]^2 (u along x, v along y) and three quarters of
    /// a wall of radius `r` about the z axis over `wall_z` (v = 0 to 1), from -90 to 180
    /// degrees (u = 1/3 + s/3 on the middle quarter, [`quarter`]'s s).
    fn plate_and_wall(w: f64, plate_z: f64, r: f64, wall_z: [f64; 2]) -> [Surface; 2] {
        plate_and_wall_turning(w, plate_z, r, wall_z, false)
    }

    /// [`plate_and_wall`], the wall three quarters of a turn from -90 degrees, or the whole
    /// turn from -90 degrees (`whole`: closed in u, its seam at -90 degrees).
    fn plate_and_wall_turning(
        w: f64,
        plate_z: f64,
        r: f64,
        wall_z: [f64; 2],
        whole: bool,
    ) -> [Surface; 2] {
        let plate = Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 1,
                v_degree: 1,
                control_points: &[
                    Point3::new(-w, -w, plate_z),
                    Point3::new(-w, w, plate_z),
                    Point3::new(w, -w, plate_z),
                    Point3::new(w, w, plate_z),
                ],
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[2, 2],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                uv_domain: None,
            })
            .unwrap(),
        );
        let (mut cps, mut ws) = (Vec::new(), Vec::new());
        let h = 0.5_f64.sqrt();
        for (p, wi) in [
            (0.0, -r),
            (r, -r),
            (r, 0.0),
            (r, r),
            (0.0, r),
            (-r, r),
            (-r, 0.0),
            (-r, -r),
            (0.0, -r),
        ]
        .into_iter()
        .zip([1.0, h, 1.0, h, 1.0, h, 1.0, h, 1.0])
        .take(if whole { 9 } else { 7 })
        {
            for z in wall_z {
                cps.push(Point3::new(p.0, p.1, z));
                ws.push(wi);
            }
        }
        let wall = Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 2,
                v_degree: 1,
                control_points: &cps,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: if whole {
                    &[3, 2, 2, 2, 3]
                } else {
                    &[3, 2, 2, 3]
                },
                v_multiplicities: &[2, 2],
                u_knots: if whole {
                    &[0.0, 0.25, 0.5, 0.75, 1.0]
                } else {
                    &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
                },
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&ws),
                uv_domain: None,
            })
            .unwrap(),
        );
        [plate, wall]
    }

    /// The solver of the edge where the wall stands on the plate, `d` along the plate and the
    /// wall, its offsets' sides towards the wedge outside the wall above the plate.
    fn solver(k: &Kernel, d: [f64; 2]) -> SetbackFs {
        let [plate, wall] = supports();
        let mut curve_based = CurveBasedFs::new(1e-6, 0.1, 0.01);
        curve_based.original = Some(Curve::from(quarter(WALL, 0.0)));
        // the wall's normal at the edge's middle, against the outward radial: its side
        let n = wall
            .evaluate_normal(k, Point2::new(0.5, 0.0), Side::Left, Side::Left)
            .unwrap();
        let outward = Vec3::new(1.0, 1.0, 0.0);
        let wall_side = if n.dot(outward) > 0.0 { 1.0 } else { -1.0 };
        curve_based.solver.orientations = [1.0, wall_side];
        curve_based.solver.surfaces = rail_slots(
            extending_offset(k, 0.0, (plate, SurfaceId::Free(1))).unwrap(),
            extending_offset(k, 0.0, (wall, SurfaceId::Free(2))).unwrap(),
        );
        SetbackFs {
            curve_based,
            setbacks: d,
            edge_interval: Extent1d::from_min_max(0.0, 1.0),
        }
    }

    /// The closed-form feet at s: radius `8 + d_1` on the plate, height `d_2` on the wall, in the
    /// plane through the edge point square to it (a vertical half-plane through the axis).
    fn feet(k: &Kernel, s: f64, d: [f64; 2]) -> [Point3; 2] {
        let c = quarter(WALL, 0.0).evaluate_point(s).unwrap();
        let _ = k;
        let out = Vec3::new(c.x, c.y, 0.0) * (1.0 / WALL);
        [out * (WALL + d[0]), out * WALL + Vec3::Z * d[1]]
    }

    fn point(k: &Kernel, fs: &mut SetbackFs, rail: usize, uv: Point2) -> Point3 {
        offset_evaluate_point(k, fs.curve_based.solver.rail_mut(rail).unwrap(), uv).unwrap()
    }

    /// The station's feet from the edge, Newton's from the setback start: the closed form.
    #[test]
    fn the_feet_at_a_station_are_the_setbacks_square_to_the_edge() {
        let k = Kernel::default();
        let d = [2.0, 1.0];
        let mut fs = solver(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        for s in [0.0, 0.1, 0.37, 0.5, 0.82, 1.0] {
            let jet = fs.jet(&k, s).unwrap();
            let guesses = [Point2::new(0.6, 0.6), Point2::new(0.5, 0.01)];
            let found = fs.feet_at(&k, &jet, guesses, &domains).unwrap();
            let want = feet(&k, s, d);
            for rail in 0..2 {
                let uv = found[rail].expect("a foot");
                let p = point(&k, &mut fs, rail, uv);
                assert!(
                    p.distance(want[rail]) < 1e-9,
                    "s {s} rail {rail}: {p:?} vs {want:?}"
                );
            }
        }
    }

    /// A foot whose branch nothing decides is refused, after the re-solve from the edge finds
    /// no foot either. Both supports are the plate z = 0 (tangent along the edge, the quarter
    /// of radius 8 on it): the other support's oriented normal is `±z`, and the plate's tangent
    /// square to the edge, `n × C′` with `n = ±z` and `C′` horizontal, has no z, so its side
    /// `t · (±z)` is exactly 0 ([`on_wedge_side`]: `None`). The foot from a guess at its own
    /// root, radius `8 − 0.8 = 7.2` on the plate in the station's half-plane, is found but has
    /// no side; the re-solve from the edge has no tangent to start along (`setback_tangent`:
    /// `None`), so foot 0 is refused `SetbackFootMirrored` at that s, never answered.
    #[test]
    fn a_foot_with_no_side_from_its_guess_or_the_edge_is_refused_as_mirrored() {
        let k = Kernel::default();
        let d = [0.8, 0.8];
        let [plate, _] = supports();
        let mut fs = solver(&k, d);
        fs.curve_based.solver.surfaces = rail_slots(
            extending_offset(&k, 0.0, (plate.clone(), SurfaceId::Free(1))).unwrap(),
            extending_offset(&k, 0.0, (plate, SurfaceId::Free(2))).unwrap(),
        );
        let domains = fs.trace_domains(&k).unwrap();
        let s = 0.37;
        let c = quarter(WALL, 0.0).evaluate_point(s).unwrap();
        let inner = c * ((WALL - d[0]) / WALL);
        let uv = Point2::new((inner.x + 20.0) / 40.0, (inner.y + 20.0) / 40.0);
        // (the guess is the foot's root: on the plate, 0.8 from the edge, square to it)
        let jet = fs.jet(&k, s).unwrap();
        let g = point(&k, &mut fs, 0, uv) - jet.point;
        assert!((g.length() - d[0]).abs() < 1e-12 && g.dot(jet.d1).abs() < 1e-12);
        let err = fs
            .station(&k, s, [uv, uv], &domains)
            .expect_err("refused: no side");
        let Some(&BlendFailure::Solver(SolverFailure::SetbackFootMirrored { rail, s: at })) =
            err.blend_failure()
        else {
            panic!("{err:?}");
        };
        assert_eq!((rail, at), (0, s));
    }

    /// A straight edge from (0, 0, 20) to (10, 0, 20) over [0, 1], its jet asked past its start
    /// (s = -0.3) and then past its end (s = 1.3): the leader reaches both, and is the line
    /// continued, C(s) = (10 s, 0, 20) with C' = (10, 0, 0) and C'' = 0, at both (the jet past
    /// the start is the same after the end's continuation as before it). Before, the end's
    /// extension was made afresh from the edge and dropped the start's.
    #[test]
    fn the_leader_continued_past_both_ends_keeps_both() {
        let k = Kernel::default();
        let mut fs = solver(&k, [1.0, 0.5]);
        let line = BSplineCurve::create_line_segment(
            3,
            Point3::new(0.0, 0.0, 20.0),
            Point3::new(10.0, 0.0, 20.0),
        )
        .unwrap();
        fs.curve_based.original = Some(Curve::from(line));
        fs.curve_based.extended = None;
        let before = fs.jet(&k, -0.3).unwrap();
        let after = fs.jet(&k, 1.3).unwrap();
        let again = fs.jet(&k, -0.3).unwrap();
        let ivl = fs.edge_curve().unwrap().natural_interval();
        assert!(ivl.min() <= -0.3 && ivl.max() >= 1.3, "{ivl:?}");
        for (jet, s) in [(before, -0.3), (after, 1.3), (again, -0.3)] {
            assert!(
                jet.point.distance(Point3::new(10.0 * s, 0.0, 20.0)) < 1e-12,
                "{jet:?}"
            );
            assert!(
                (jet.d1 - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-12,
                "{jet:?}"
            );
            assert!(jet.d2.length() < 1e-12, "{jet:?}");
        }
        assert!(before.point.distance(again.point) < 1e-12);
    }

    /// The equations vanish at the closed-form feet, and the Jacobian is their derivative
    /// (central differences, step 1e-6, agree to 1e-6 of the entries' scale).
    #[test]
    fn the_equations_vanish_at_the_feet_and_the_jacobian_is_their_derivative() {
        let k = Kernel::default();
        let d = [2.0, 1.0];
        let mut fs = solver(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        let s = 0.37;
        let jet = fs.jet(&k, s).unwrap();
        let found = fs
            .feet_at(
                &k,
                &jet,
                [Point2::new(0.6, 0.6), Point2::new(0.5, 0.01)],
                &domains,
            )
            .unwrap();
        let (f0, f1) = (found[0].unwrap(), found[1].unwrap());
        let x = vec![f0.x, f0.y, f1.x, f1.y, s];
        let base0 = fs.curve_based.solver.rail(0).unwrap().base_id;
        let eval = |fs: &mut SetbackFs, x: &[f64], j: Option<&mut Matrix>| {
            let mut f = vec![0.0; 5];
            let (mut ne, mut np) = (1, 2);
            let load = JacobianLoad {
                rail_index: 0,
                surface1: Some(base0),
                surf1_offset: 0,
                surface2: None,
                surf2_offset: 0,
                x,
            };
            let found = fs
                .load_jacobian(&k, &mut ne, &mut np, &load, &mut f, j)
                .unwrap();
            assert_eq!((ne, np), (5, 5));
            (f, found)
        };
        let mut j = Matrix::new(5, 5);
        let (f, root) = eval(&mut fs, &x, Some(&mut j));
        assert!(root, "{f:?}");
        for v in &f[1..] {
            assert!(v.abs() < 1e-10, "{f:?}");
        }
        let h = 1e-6;
        for col in 0..5 {
            let mut xp = x.clone();
            let mut xm = x.clone();
            xp[col] += h;
            xm[col] -= h;
            let (fp, _) = eval(&mut fs, &xp, None);
            let (fm, _) = eval(&mut fs, &xm, None);
            for row in 1..5 {
                let fd = (fp[row] - fm[row]) / (2.0 * h);
                let an = j.at(row, col).unwrap();
                assert!(
                    (fd - an).abs() < 1e-6 * (1.0 + an.abs()),
                    "row {row} col {col}: {an} vs {fd}"
                );
            }
        }
    }

    /// A guess on the edge (where the marcher's guesses from the offsets, 0 here, put it) is
    /// replaced by the station's feet, s the edge point's parameter.
    #[test]
    fn initial_values_on_the_edge_start_on_the_feet() {
        let k = Kernel::default();
        let d = [2.0, 1.0];
        let mut fs = solver(&k, d);
        let s = 0.5;
        let c = quarter(WALL, 0.0).evaluate_point(s).unwrap();
        // the edge point's parameters on the plate, and the wall's at its foot of the edge
        let plate_uv = Point2::new((c.x + 20.0) / 40.0, (c.y + 20.0) / 40.0);
        let mut guess = vec![plate_uv.x, plate_uv.y, 0.5, 0.0];
        let mut intervals = ExtentNd::new(5);
        let mut periodicities = vec![false; 4];
        let out = SetbackFs::load_initial_values(
            &mut fs,
            &k,
            None,
            false,
            0,
            0,
            2,
            &mut intervals,
            &mut periodicities,
            &mut guess,
        )
        .unwrap();
        assert!(out.is_none());
        assert_eq!(guess.len(), 5);
        assert!((guess[4] - s).abs() < 1e-9, "{guess:?}");
        let want = feet(&k, s, d);
        let p0 = point(&k, &mut fs, 0, Point2::new(guess[0], guess[1]));
        let p1 = point(&k, &mut fs, 1, Point2::new(guess[2], guess[3]));
        assert!(p0.distance(want[0]) < 1e-9, "{p0:?}");
        assert!(p1.distance(want[1]) < 1e-9, "{p1:?}");
    }

    /// A clipped step: the plate's foot held on the iso-line u = const through the closed-form
    /// foot at s = 0.37 finds that foot and that s, and the wall's foot there.
    #[test]
    fn a_clipped_foot_is_solved_on_its_iso_line_with_its_station() {
        let k = Kernel::default();
        let d = [2.0, 1.0];
        let mut fs = solver(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        let s = 0.37;
        let want = feet(&k, s, d);
        let iso = (want[0].x + 20.0) / 40.0;
        // a guess a little off along the line and on the wall
        let guess = [
            Point2::new(iso, (want[0].y + 20.0) / 40.0 + 0.01),
            Point2::new(0.5, 0.2),
        ];
        let (uv, found_s) = fs
            .clipped_feet(&k, 0, SurfParamType::U, iso, guess, &domains)
            .unwrap()
            .expect("found");
        assert!((found_s - s).abs() < 1e-9, "{found_s}");
        assert_eq!(uv[0].x, iso);
        for rail in 0..2 {
            let p = point(&k, &mut fs, rail, uv[rail]);
            assert!(
                p.distance(want[rail]) < 1e-9,
                "rail {rail}: {p:?} vs {:?}",
                want[rail]
            );
        }
    }

    // ---- the rim: closed forms for the exact station (#13710, step 1 of the station plan) ----
    //
    // The rim of a cylinder of radius R = 10 about the z axis whose top face is z = 20: the
    // edge C(s) is the circle r = R at z = 20 ([`quarter`], s in [0, 1], not arc length). At s
    // let e be the unit radial (C − (0, 0, 20)) / R and τ = z × e the circle's unit tangent
    // (C′ = |C′| τ, the quarter running from (R, 0) to (0, R)). The plane through C(s) square
    // to C′(s) is the vertical plane through the axis containing e; its half-plane r > 0 is
    // spanned by e and z. Setbacks d = [1, 0.5]: foot 0 is on the top face d_0 from C towards
    // the axis, foot 1 on the wall d_1 below C:
    //   S_0 = (R − d_0) e + 20 z, (r, z) = (9, 20); S_1 = R e + (20 − d_1) z, (r, z) = (10, 19.5),
    // the only points of each face in that half-plane at distance d_i from C (the top face's
    // line through C along e meets the circle of radius d_0 at r = R ∓ d_0; into the face is
    // towards the axis; the wall's vertical line meets it at z = 20 ∓ d_1, into the face is
    // down). Both feet turn with e about the axis, at fixed (r, z): per unit arc length σ of the
    // edge (dσ = R dθ) a foot at radius r moves r dθ = (r / R) dσ along τ, so
    //   ∂S_0/∂σ = (9 / 10) τ = 0.9 τ, ∂S_1/∂σ = (10 / 10) τ = 1.0 τ,
    // and per unit length of foot 0's motion foot 1 moves 1 / 0.9 along τ. Fold check: in arc
    // length C′ = τ and C″ = −e / R; S_i − C = (r_i − R) e + (z_i − 20) z, so
    // (S_i − C) · C″ = −(r_i − R) / R and ∂S_i/∂σ · C′ = |C′|² − (S_i − C) · C″
    // = 1 + (r_i − R) / R = r_i / R: 0.9 and 1.0, positive. With d_0 = 12 > R foot 0 is past
    // the axis, r_0 = R − d_0 = −2 (the top face spans [−25, 25]² so it holds that foot), and
    // ∂S_0/∂σ · C′ = r_0 / R = −0.2 < 0: the stations fold (foot 0 runs backwards).

    const RIM_R: f64 = 10.0;
    const RIM_Z: f64 = 20.0;
    const RIM_W: f64 = 25.0;

    /// The solver of the rim (above), its offsets' sides from the faces' normals so the ball's
    /// side is inside the material: the top face's into the wall (down), the wall's into the
    /// top face (towards the axis).
    fn rim(k: &Kernel, d: [f64; 2]) -> SetbackFs {
        rim_on(k, d, quarter(RIM_R, RIM_Z), 1e-6)
    }

    /// [`rim`] on the edge `edge` (an arc of the rim's circle), the solver's 3D tolerance
    /// `tol`.
    fn rim_on(k: &Kernel, d: [f64; 2], edge: BSplineCurve, tol: f64) -> SetbackFs {
        rim_on_wall(k, d, edge, tol, false)
    }

    /// [`rim_on`], on the whole turn's wall where `whole`.
    fn rim_on_wall(
        k: &Kernel,
        d: [f64; 2],
        edge: BSplineCurve,
        tol: f64,
        whole: bool,
    ) -> SetbackFs {
        let [plate, wall] = plate_and_wall_turning(RIM_W, RIM_Z, RIM_R, [0.0, RIM_Z], whole);
        let mut curve_based = CurveBasedFs::new(tol, 0.1, 0.01);
        curve_based.original = Some(Curve::from(edge));
        let np = plate
            .evaluate_normal(k, Point2::new(0.5, 0.5), Side::Left, Side::Left)
            .unwrap();
        let plate_side = if np.z < 0.0 { 1.0 } else { -1.0 };
        let nw = wall
            .evaluate_normal(k, Point2::new(0.5, 0.5), Side::Left, Side::Left)
            .unwrap();
        let outward = Vec3::new(1.0, 1.0, 0.0);
        let wall_side = if nw.dot(outward) > 0.0 { -1.0 } else { 1.0 };
        curve_based.solver.orientations = [plate_side, wall_side];
        curve_based.solver.surfaces = rail_slots(
            extending_offset(k, 0.0, (plate, SurfaceId::Free(1))).unwrap(),
            extending_offset(k, 0.0, (wall, SurfaceId::Free(2))).unwrap(),
        );
        SetbackFs {
            curve_based,
            setbacks: d,
            edge_interval: Extent1d::from_min_max(0.0, 1.0),
        }
    }

    /// The rim's edge point, unit radial e and unit tangent τ at s.
    fn rim_frame(s: f64) -> (Point3, Vec3, Vec3) {
        let c = quarter(RIM_R, RIM_Z).evaluate_point(s).unwrap();
        let e = Vec3::new(c.x, c.y, 0.0) * (1.0 / RIM_R);
        (c, e, Vec3::new(-e.y, e.x, 0.0))
    }

    /// The closed-form feet at s (above).
    fn rim_feet(s: f64, d: [f64; 2]) -> [Point3; 2] {
        let (_, e, _) = rim_frame(s);
        [
            e * (RIM_R - d[0]) + Vec3::Z * RIM_Z,
            e * RIM_R + Vec3::Z * (RIM_Z - d[1]),
        ]
    }

    /// The top face's parameters of a point on it.
    fn plate_uv(p: Point3) -> Point2 {
        Point2::new((p.x + RIM_W) / (2.0 * RIM_W), (p.y + RIM_W) / (2.0 * RIM_W))
    }

    /// Guesses near the feet (off the edge), and guesses on the edge (C(s)'s parameters on
    /// both faces: the wall's middle quarter is [`quarter`]'s arc, u = (1 + s) / 3, v = 1).
    fn rim_guesses(s: f64, d: [f64; 2]) -> [[Point2; 2]; 2] {
        let (c, e, t) = rim_frame(s);
        let want = rim_feet(s, d);
        let near = [
            plate_uv(want[0] + e * 0.05 + t * 0.03),
            Point2::new((1.0 + s) / 3.0 + 0.002, (RIM_Z - d[1]) / RIM_Z - 0.01),
        ];
        let on_edge = [plate_uv(c), Point2::new((1.0 + s) / 3.0, 1.0)];
        [near, on_edge]
    }

    /// The exact station on the rim: the feet are at (r, z) = (9, 20) and (10, 19.5) in the
    /// station's half-plane through the axis (no component along τ), from guesses near them
    /// and from guesses on the edge.
    #[test]
    fn rim_station_feet_are_the_setbacks_in_the_half_plane_through_the_axis() {
        let k = Kernel::default();
        let d = [1.0, 0.5];
        let mut fs = rim(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        let want_rz = [(9.0, 20.0), (10.0, 19.5)];
        for s in [0.0, 0.37, 0.5, 0.82, 1.0] {
            let (_, e, t) = rim_frame(s);
            for guesses in rim_guesses(s, d) {
                let station = fs
                    .station(&k, s, guesses, &domains)
                    .unwrap()
                    .expect("a station");
                assert_eq!(station.s, s);
                for (rail, &(r, z)) in want_rz.iter().enumerate() {
                    let p = station.feet[rail].point;
                    assert!((p.dot(e) - r).abs() < 1e-9, "s {s} rail {rail}: {p:?}");
                    assert!((p.z - z).abs() < 1e-9, "s {s} rail {rail}: {p:?}");
                    assert!(p.dot(t).abs() < 1e-9, "s {s} rail {rail}: {p:?}");
                    let q = point(&k, &mut fs, rail, station.feet[rail].uv);
                    assert!(q.distance(p) < 1e-12, "s {s} rail {rail}: {q:?} vs {p:?}");
                }
            }
        }
    }

    /// Each foot's motion from the implicit derivative of its own system: per unit arc length
    /// of the edge (`∂S/∂s / |C′(s)|`, `|C′|` the edge's own speed, the quarter not being in
    /// arc length) 0.9 τ for foot 0 and 1.0 τ for foot 1, along the circles r = 9 and r = 10.
    #[test]
    fn rim_feet_move_along_their_circles_at_their_radius_over_the_rims() {
        let k = Kernel::default();
        let d = [1.0, 0.5];
        let mut fs = rim(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        for s in [0.0, 0.37, 0.82] {
            let (_, _, t) = rim_frame(s);
            let station = fs
                .station(&k, s, rim_guesses(s, d)[0], &domains)
                .unwrap()
                .expect("a station");
            let speed = station.jet.d1.length();
            for (rail, rate) in [(0, 0.9), (1, 1.0)] {
                let foot = station.feet[rail];
                let per_arc = foot.per_s * (1.0 / speed);
                assert!(
                    (per_arc - t * rate).length() < 1e-9,
                    "s {s} rail {rail}: {per_arc:?} vs {rate} {t:?}"
                );
            }
        }
    }

    /// The trace point at the exact station, and `ComputeUVVectors` on a point at the feet:
    /// the curve tangent is τ (foot 0's unit motion), foot 0's uv tangent maps to τ and foot
    /// 1's to τ / 0.9 (per unit foot-0 chord: foot 1 moves 1.0 per 0.9 of foot 0), so their
    /// ratio is 1 / 0.9; a reversed curve tangent reverses both; a point whose
    /// `user_doubles[0]` is not its station's s is answered at its foot 0's station.
    #[test]
    fn rim_uv_tangents_are_each_foots_own_per_unit_foot_0_chord() {
        let k = Kernel::default();
        let d = [1.0, 0.5];
        let mut fs = rim(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        let s = 0.37;
        let (_, _, t) = rim_frame(s);
        let point0 = fs
            .station_point(&k, s, rim_guesses(s, d)[1], &domains)
            .unwrap()
            .expect("a station point");
        assert_eq!((point0.user_doubles[0], point0.user_doubles[1]), (s, s));
        assert!(
            (point0.curve_pv[1] - t).length() < 1e-9,
            "{:?}",
            point0.curve_pv
        );
        let want = rim_feet(s, d);
        assert!(point0.curve_pv[0].distance(want[0]) < 1e-9);
        let images = |fs: &mut SetbackFs, p: &TsectPnt| {
            let mut out = [Vec3::ZERO; 2];
            for (rail, slot) in out.iter_mut().enumerate() {
                let uv = p.uv_curve_pv[rail];
                let (_, pu, pv) =
                    offset_evaluate_1st(&k, fs.curve_based.solver.rail_mut(rail).unwrap(), uv[0])
                        .unwrap();
                *slot = pu * uv[1].x + pv * uv[1].y;
            }
            out
        };
        let [i0, i1] = images(&mut fs, &point0);
        assert!((i0 - t).length() < 1e-9, "{i0:?}");
        assert!((i1 - t * (1.0 / 0.9)).length() < 1e-9, "{i1:?}");
        assert!((i1.length() / i0.length() - 1.0 / 0.9).abs() < 1e-9);

        // ComputeUVVectors at the same feet, the curve tangent reversed, s not left in the point
        let mut p = TsectPnt::new();
        FilletSolver::compute_surface_values(
            &k,
            &mut fs,
            [point0.uv_curve_pv[0][0], point0.uv_curve_pv[1][0]],
            &mut p,
        )
        .unwrap();
        p.curve_pv[1] = -t;
        p.user_doubles[0] = 0.0;
        fs.compute_uv_vectors(&k, &mut p).unwrap();
        let [j0, j1] = images(&mut fs, &p);
        assert!((j0 + t).length() < 1e-9, "{j0:?}");
        assert!((j1 + t * (1.0 / 0.9)).length() < 1e-9, "{j1:?}");

        // and with its s and the tangent forward: the station point's own tangents
        p.curve_pv[1] = t;
        p.user_doubles[0] = s;
        fs.compute_uv_vectors(&k, &mut p).unwrap();
        for rail in 0..2 {
            let (a, b) = (p.uv_curve_pv[rail][1], point0.uv_curve_pv[rail][1]);
            assert!((a - b).length() < 1e-12 * (1.0 + b.length()), "rail {rail}");
        }
    }

    /// The base solver's `PointOnPlaneSolve` on the plane through C(t) square to C′(t), from
    /// the marcher's guesses on the edge: foot 0 is on that plane and at its station, which
    /// on the rim is t's own (the plane is the station's, and foot 0 at r = 9 > 0 is on its
    /// half-plane), so `user_doubles[0]` = t and the feet are the closed form's. The solve
    /// reports found only with every residual within `conversion_tol (1 + |S_0|)`, about
    /// `1e-13 · 21` ([`SetbackFs::load_jacobian`]), so 1e-9 holds them with margin.
    #[test]
    fn rim_point_on_plane_solve_is_the_station_at_the_planes_parameter() {
        let k = Kernel::default();
        let d = [1.0, 0.5];
        let mut fs = rim(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        for t in [0.2, 0.37, 0.64] {
            let jet = fs.jet(&k, t).unwrap();
            let [_, on_edge] = rim_guesses(t, d);
            let mut tsect = TsectPnt::new();
            let found = FilletSolver::point_on_plane_solve(
                &mut fs,
                &k,
                None,
                jet.point,
                jet.d1,
                &domains[0],
                &domains[1],
                on_edge[0],
                on_edge[1],
                &mut tsect,
            )
            .unwrap();
            assert!(found, "t {t}");
            assert!(
                (tsect.user_doubles[0] - t).abs() < 1e-9,
                "t {t}: {:?}",
                tsect.user_doubles
            );
            let want = rim_feet(t, d);
            let p0 = point(&k, &mut fs, 0, tsect.uv_pos(SrfIndex::First));
            let p1 = point(&k, &mut fs, 1, tsect.uv_pos(SrfIndex::Second));
            assert!(
                p0.distance(want[0]) < 1e-9,
                "t {t}: {p0:?} vs {:?}",
                want[0]
            );
            assert!(
                p1.distance(want[1]) < 1e-9,
                "t {t}: {p1:?} vs {:?}",
                want[1]
            );
        }
    }

    /// The fold check: setbacks [12, 0.5] put foot 0 past the axis at r = −2, where
    /// `∂S_0/∂σ · C′ = r_0 / R = −0.2 < 0` (above): the exact station and the station
    /// through those feet are refused as crossing, not answered.
    #[test]
    fn rim_station_past_the_axis_is_refused_as_folding() {
        let k = Kernel::default();
        let d = [12.0, 0.5];
        let mut fs = rim(&k, d);
        let domains = fs.trace_domains(&k).unwrap();
        let s = 0.37;
        let want = rim_feet(s, d);
        let (_, e, _) = rim_frame(s);
        assert!((want[0].dot(e) + 2.0).abs() < 1e-12);
        let guesses = [
            plate_uv(want[0] + e * 0.05),
            Point2::new((1.0 + s) / 3.0 + 0.002, (RIM_Z - d[1]) / RIM_Z - 0.01),
        ];
        let refused = |r: Result<Option<SetbackStation>>| {
            let err = r.expect_err("refused as folding");
            assert_eq!(
                err.blend_failure(),
                Some(&BlendFailure::NoRoom(NoRoomReason::SetbacksCross)),
                "{err:?}"
            );
        };
        refused(fs.station(&k, s, guesses, &domains));
        let exact = [
            plate_uv(want[0]),
            Point2::new((1.0 + s) / 3.0, (RIM_Z - d[1]) / RIM_Z),
        ];
        refused(fs.station_through(&k, s, exact, &domains));
        // the same feet's motion, unchecked: foot 0 runs backwards at r_0 / R of the edge's speed
        let jet = fs.jet(&k, s).unwrap();
        let support = fs.support(0).unwrap();
        let foot = foot_at(&k, &support, &jet, d[0], exact[0])
            .unwrap()
            .unwrap();
        let ratio = foot.per_s.dot(jet.d1) / jet.d1.length_squared();
        assert!((ratio + 0.2).abs() < 1e-9, "{ratio}");
    }

    /// `ComputeUVVectors` at a point past the polar line whose s does not hold: setbacks
    /// [12, 0.5], the feet at s = 0.37 (foot 0 at r = −2, angle θ = θ(0.37) in (0, π/2)),
    /// `user_doubles[0]` = 0. Neither s puts foot 0 at its station, so the point is refused
    /// `SetbackStationNotFound`, not answered with tangents at a wrong s:
    /// - s = 0: C(0) = (10, 0, 20), and foot 0 − C(φ) is horizontal with
    ///   `|S_0 − C(φ)|² = 2² + 10² + 2·2·10 cos(φ − θ) = 104 + 40 cos(φ − θ)`; at φ = 0
    ///   that is `104 + 40 cos θ < 144`, so the reach is short of 12.
    /// - The drop minimises that distance over the arc φ ∈ [0, π/2]. Its minimum is at the
    ///   end farther in angle from θ, where `|φ − θ|` is largest. That gives
    ///   `104 + 40 cos(max(θ, π/2 − θ))`, which lies in [104, 144) since
    ///   `0 < max(θ, π/2 − θ) < π/2`, so the reach is again short of 12. Foot 0's true station
    ///   is φ = θ, the arc's farthest point from it (reach 12): `|C′|² − (S_0 − C) · C″ < 0` (the
    ///   fold test above).
    ///
    /// With its true s in `user_doubles[0]` the same point validates and is refused as
    /// folding instead.
    #[test]
    fn rim_uv_vectors_past_the_axis_without_its_station_are_refused() {
        let k = Kernel::default();
        let d = [12.0, 0.5];
        let mut fs = rim(&k, d);
        let s = 0.37;
        let want = rim_feet(s, d);
        let exact = [
            plate_uv(want[0]),
            Point2::new((1.0 + s) / 3.0, (RIM_Z - d[1]) / RIM_Z),
        ];
        let mut p = TsectPnt::new();
        FilletSolver::compute_surface_values(&k, &mut fs, exact, &mut p).unwrap();
        assert!(p.curve_pv[0].distance(want[0]) < 1e-12);
        p.curve_pv[1] = rim_frame(s).2;
        p.user_doubles[0] = 0.0;
        let err = fs.compute_uv_vectors(&k, &mut p).expect_err("refused");
        assert!(
            matches!(
                err.blend_failure(),
                Some(&BlendFailure::Solver(
                    SolverFailure::SetbackStationNotFound { .. }
                ))
            ),
            "{err:?}"
        );
        p.user_doubles[0] = s;
        let err = fs.compute_uv_vectors(&k, &mut p).expect_err("refused");
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::NoRoom(NoRoomReason::SetbacksCross)),
            "{err:?}"
        );
    }

    // ---- the station trace (`do_law_intersection_with` at the solver's stations) ----
    //
    // The rim above, on a half turn of its edge. Every station's plane is the half-plane
    // through the axis at the station's angle θ, where the top face's section is the line
    // z = 20 and the wall's the line r = 10: foot 0 is at (r, z) = (9, 20), foot 1 at
    // (10, 19.5), whatever θ. So the rails are the circles r = 9 at z = 20 and r = 10 at
    // z = 19.5, and the chord between them the line r = 9 + 2 (20 − z) (from (9, 20) to
    // (10, 19.5): slope −1/2 in z per unit r).
    //
    // An extension step from a station is `StepSolve`'s plane at the step h along foot 0's
    // unit tangent τ from foot 0, foot 0 on the circle r = 9: the next foot 0 is where
    // 9 sin Δθ = h, Δθ = asin(h / 9), and foot 0 moves the chord 2 · 9 sin(Δθ / 2). Each step
    // asks h = min(max(d), cap − moved) with max(d) = 1 and the cap 5 max(d) = 5, until foot
    // 0 has moved the cap ([`extension_reach`]).

    use crate::blend::fillet_geom::p1::{
        FilletEdgeType, FilletEdgeuse, FilletGeom, FilletGeomId, FilletGeomType, FilletVertex,
        FilletVertexuse,
    };
    use crate::blend::fillet_intersector::{FilletDispatch, FilletIntersector};
    use crate::blend::fillet_solver::p1::{FilletExecutiveRef, FilletSurfaceGenerator};
    use crate::blend::fillet_solver::p4::IntersectorSolver;
    use crate::blend::fillet_standard_solver::p1::FilletLaw;
    use crate::error::intersect::IntersectFailure;
    use crate::intersect::adv_surface_intersector::NoSurfaceHandle;
    use crate::intersect::surface_intersector::p1::{SpanAccuracy, SurfaceIntersector, TsectId};
    use crate::intersect::surface_intersector::p2::TsectCurves;
    use crate::intersect::surface_intersector::p3::NextPoint;
    use crate::intersect::surface_intersector::p4::{
        do_law_intersection_with, IntersectorVirtuals, StationStep, StationTraceOptions,
        TraceStations,
    };
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
    use crate::surface::surface::SurfaceKind;
    use crate::topology::{EdgeId, Edgeuse, Vertexuse, VertexuseOwner};

    /// The station trace's 3D tolerance, the solver's and the chord's.
    const TRACE_TOL: f64 = 1.0e-4;

    /// The rational quadratic arc of radius `r` about the z axis at height `z` from the
    /// angle `start_deg` (degrees) through `quarters` quarter turns (each span's middle
    /// control point at its corner `r √2` out, weight `√2 / 2`), its parameter s in [0, 1]
    /// with a knot at each quarter.
    fn arc(r: f64, z: f64, start_deg: f64, quarters: u32) -> BSplineCurve {
        let w = 0.5_f64.sqrt();
        let at = |deg: f64, rho: f64| {
            let a = deg.to_radians();
            Point3::new(rho * a.cos(), rho * a.sin(), z)
        };
        let (mut cps, mut ws) = (vec![at(start_deg, r)], vec![1.0]);
        let mut knots = vec![0.0];
        let mut mults = vec![3];
        for q in 0..quarters {
            let a = start_deg + 90.0 * f64::from(q);
            cps.push(at(a + 45.0, r / w));
            cps.push(at(a + 90.0, r));
            ws.push(w);
            ws.push(1.0);
            knots.push(f64::from(q + 1) / f64::from(quarters));
            mults.push(2);
        }
        if let Some(m) = mults.last_mut() {
            *m = 3;
        }
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &cps,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &knots,
            knot_type: KnotType::Unspecified,
            weights: Some(&ws),
            trim: None,
        })
        .unwrap()
    }

    /// A half turn of the rim's edge from −45 to 135 degrees, inside the wall's three
    /// quarters (−90 to 180) so that no foot is on a domain boundary.
    fn half() -> BSplineCurve {
        arc(RIM_R, RIM_Z, -45.0, 2)
    }

    /// A rail edge for the pseudo geometry (as `fillet_solver::p4`'s tests make one): two
    /// edge uses, the second the first's mate, and their vertex uses.
    fn add_rail(k: &Kernel, fb: &mut FilletBrep, y: f64) -> EdgeId {
        let p0 = Point3::new(0.0, y, 0.0);
        let p1 = Point3::new(1.0, y, 0.0);
        let v0 = fb.brep_mut().add_vertex(p0).unwrap();
        let v1 = fb.brep_mut().add_vertex(p1).unwrap();
        fb.insert_fillet_vertex(v0, FilletVertex::new(k, None));
        fb.insert_fillet_vertex(v1, FilletVertex::new(k, None));
        let e = fb.new_fillet_edge(v0, v1, None).unwrap();
        fb.fillet_edge_mut(e)
            .unwrap()
            .set_fillet_edge_type(FilletEdgeType::Rail);
        for (v, o) in [(v0, OrientType::Same), (v1, OrientType::Opposite)] {
            let b = fb.brep_mut();
            let e_u = b.add_edgeuse_record(Edgeuse::default()).unwrap();
            let vu = b.add_vertexuse_record(Vertexuse::default()).unwrap();
            b.edgeuse_mut(e_u).unwrap().edge = Some(e);
            b.edgeuse_mut(e_u).unwrap().orientation = o;
            b.edgeuse_mut(e_u).unwrap().vertexuse = Some(vu);
            b.vertexuse_mut(vu).unwrap().vertex = Some(v);
            b.vertexuse_mut(vu).unwrap().owner = VertexuseOwner::Edgeuse(Some(e_u));
            b.vertex_mut(v).unwrap().vertexuses.push(vu);
            b.edge_mut(e).unwrap().edgeuses.push(e_u);
            fb.insert_fillet_edgeuse(e_u, FilletEdgeuse::default());
            fb.insert_fillet_vertexuse(vu, FilletVertexuse::new());
        }
        e
    }

    /// The half rim's solver set up to trace and flush: its pseudo geometry with two rails,
    /// the linear generator and the chamfer of setbacks `d`, the trace surfaces (the
    /// solver's offsets at distance 0) and their domains.
    struct HalfRim {
        k: Kernel,
        fs: SetbackFs,
        pseudo: FilletBrep,
        geom: FilletGeomId,
        rails: [EdgeId; 2],
        surfaces: [Surface; 2],
        domains: [Extent2d; 2],
    }

    fn half_rim(d: [f64; 2]) -> HalfRim {
        half_rim_on(d, half())
    }

    fn half_rim_on(d: [f64; 2], edge: BSplineCurve) -> HalfRim {
        rim_trace_on(d, edge, false)
    }

    /// [`half_rim_on`], on the whole turn's wall where `whole`.
    fn rim_trace_on(d: [f64; 2], edge: BSplineCurve, whole: bool) -> HalfRim {
        let k = Kernel::default();
        let mut fs = rim_on_wall(&k, d, edge, TRACE_TOL, whole);
        let mut pseudo = FilletBrep::new();
        let rails = [
            add_rail(&k, &mut pseudo, 0.0),
            add_rail(&k, &mut pseudo, 1.0),
        ];
        let mut geom = FilletGeom::new(&k, None, FilletGeomType::Default).unwrap();
        geom.rails = [Some(rails[0]), Some(rails[1])];
        let g = pseudo.add_geom(geom);
        let solver = &mut fs.curve_based.solver;
        solver.fillet_geoms = vec![g];
        solver.executive = Some(FilletExecutiveRef);
        solver.set_fillet_surface_generator(Some(FilletSurfaceGenerator::Linear));
        solver.chamfer = Some(ChamferDefinition::of_setbacks(ChamferSize::Distances(d)).unwrap());
        let domains = fs.trace_domains(&k).unwrap();
        let surfaces =
            [0, 1].map(|r| Surface::from(fs.curve_based.solver.rail(r).unwrap().surface.clone()));
        HalfRim {
            k,
            fs,
            pseudo,
            geom: g,
            rails,
            surfaces,
            domains,
        }
    }

    /// The edge's angle at s.
    fn angle_at(edge: &BSplineCurve, s: f64) -> f64 {
        let c = edge.evaluate_point(s).unwrap();
        c.y.atan2(c.x)
    }

    /// A point's angle about the axis.
    fn angle_of(p: Point3) -> f64 {
        p.y.atan2(p.x)
    }

    /// The closed-form feet at the angle θ.
    fn feet_at_angle(theta: f64, d: [f64; 2]) -> [Point3; 2] {
        let e = Vec3::new(theta.cos(), theta.sin(), 0.0);
        [
            e * (RIM_R - d[0]) + Vec3::Z * RIM_Z,
            e * RIM_R + Vec3::Z * (RIM_Z - d[1]),
        ]
    }

    /// The wall's parameters of a point on it.
    fn wall_uv(h: &HalfRim, p: Point3) -> Point2 {
        let wall = h.fs.curve_based.solver.rail(1).unwrap();
        offset_local_point_solve(&h.k, wall, &h.domains[1], p, Point2::new(0.5, 0.9))
            .unwrap()
            .expect("on the wall")
            .0
    }

    /// The given point at the edge's s (a rail end as the corner code places one: the exact
    /// station, its s in `user_doubles[0]`), not yet processed (curve parameter −1).
    fn given_at(h: &mut HalfRim, s: f64, d: [f64; 2]) -> TsectPnt {
        let feet = feet_at_angle(angle_at(&half(), s), d);
        let guesses = [plate_uv(feet[0]), wall_uv(h, feet[1])];
        let mut p =
            h.fs.station_point(&h.k, s, guesses, &h.domains)
                .unwrap()
                .expect("a station");
        p.curve_parameter = -1.0;
        p
    }

    /// The driver on a recorder of the curve's points when it flushes them.
    struct Recorder<V> {
        inner: V,
        flushed: Vec<TsectPnt>,
    }

    impl<'s, V: IntersectorVirtuals<'s>> IntersectorVirtuals<'s> for Recorder<V> {
        fn intersector(&self) -> &SurfaceIntersector<'s> {
            self.inner.intersector()
        }
        fn intersector_mut(&mut self) -> &mut SurfaceIntersector<'s> {
            self.inner.intersector_mut()
        }
        fn compute_point_values(
            &mut self,
            k: &Kernel,
            uv_values: [Point2; 2],
            point: &mut TsectPnt,
            previous: Option<&TsectPnt>,
            step_size: Option<f64>,
        ) -> Result<()> {
            self.inner
                .compute_point_values(k, uv_values, point, previous, step_size)
        }
        fn compute_next_point(
            &mut self,
            k: &Kernel,
            prev: TsectId,
            step_size: f64,
            next: TsectId,
        ) -> Result<NextPoint> {
            self.inner.compute_next_point(k, prev, step_size, next)
        }
        fn flush_curve(&mut self, k: &Kernel, out: &mut TsectCurves) -> Result<()> {
            let base = self.inner.intersector();
            self.flushed = base
                .curve_points()
                .iter()
                .map(|&id| base.point(id).unwrap().clone())
                .collect();
            self.inner.flush_curve(k, out)
        }
        fn evaluate_law_point(
            &mut self,
            _k: &Kernel,
            _parameter: f64,
            _uv0: Point2,
            _uv1: Point2,
            _law: &mut FilletLaw,
            _law_orient: bool,
            _point_curve_interval: &Extent1d,
            _surface_orientations: [f64; 2],
            _plane_origin: Point3,
            _plane_normal: Vec3,
        ) -> Result<(Option<f64>, [Point2; 2])> {
            panic!("a station trace evaluates no law point");
        }
        fn test_span_accuracy(
            &mut self,
            k: &Kernel,
            point: TsectId,
            next: TsectId,
            mid: Option<TsectId>,
        ) -> Result<SpanAccuracy> {
            self.inner.test_span_accuracy(k, point, next, mid)
        }
        fn station_leader(&mut self, k: &Kernel) -> Result<Option<StationLeader>> {
            self.inner.station_leader(k)
        }
        fn given_station(&mut self, k: &Kernel, point: &mut TsectPnt, tol: f64) -> Result<()> {
            self.inner.given_station(k, point, tol)
        }
        fn station_point(
            &mut self,
            k: &Kernel,
            s: f64,
            guesses: [Point2; 2],
        ) -> Result<Option<TsectPnt>> {
            self.inner.station_point(k, s, guesses)
        }
        fn station_step(&mut self, k: &Kernel, from: &TsectPnt, step: f64) -> Result<StationStep> {
            self.inner.station_step(k, from, step)
        }
    }

    /// What a station trace of the half rim did: its result, the curve's points as flushed,
    /// and the given points as the trace left them.
    struct Traced {
        result: Result<()>,
        flushed: Vec<TsectPnt>,
        given: Vec<TsectPnt>,
    }

    fn trace(h: &mut HalfRim, given: &[TsectPnt], opts: &StationTraceOptions) -> Traced {
        let [s0, s1] = &h.surfaces;
        let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, FilletGeomId> =
            FilletIntersector::new(&h.k, s0, &h.domains[0], s1, &h.domains[1], None).unwrap();
        let ids: Vec<TsectId> = given
            .iter()
            .map(|p| fi.base.base.alloc_point(p.clone()))
            .collect();
        let surfaces = [
            fi.base.base.surface(SrfIndex::First),
            fi.base.base.surface(SrfIndex::Second),
        ];
        let mut solver = IntersectorSolver::new(&mut h.fs, surfaces).unwrap();
        let mut rec = Recorder {
            inner: FilletDispatch {
                intersector: &mut fi,
                solver: &mut solver,
                pseudo: &mut h.pseudo,
            },
            flushed: Vec::new(),
        };
        let result =
            do_law_intersection_with(&mut rec, &h.k, &ids, TraceStations::Solver(opts)).map(|_| ());
        let flushed = rec.flushed;
        let given = ids
            .iter()
            .map(|&id| fi.base.base.point(id).unwrap().clone())
            .collect();
        Traced {
            result,
            flushed,
            given,
        }
    }

    /// The flushed stations' feet are the closed form's at their own angle, and their s
    /// runs strictly one way (`up` for increasing).
    fn assert_stations(stations: &[TsectPnt], d: [f64; 2], up: bool) {
        assert!(stations.len() >= 2, "{} stations", stations.len());
        for (j, p) in stations.iter().enumerate() {
            let foot0 = p.surface_pv[0][0][0];
            let want = feet_at_angle(angle_of(foot0), d);
            for (rail, w) in want.iter().enumerate() {
                let got = p.surface_pv[rail][0][0];
                assert!(
                    got.distance(*w) < 1e-9,
                    "station {j} rail {rail}: {got:?} vs {w:?}"
                );
            }
            assert!(p.curve_parameter >= 0.0, "station {j}");
        }
        for pair in stations.windows(2) {
            let (a, b) = (pair[0].user_doubles[0], pair[1].user_doubles[0]);
            assert!(if up { b > a } else { b < a }, "s {a} then {b}");
            assert!(pair[1].curve_parameter > pair[0].curve_parameter);
        }
    }

    /// The flushed rails are the circles of the closed form within the chord's certified
    /// bound, itself within the asked tolerance, and the chord between them is the line
    /// r = 9 + 2 (20 − z).
    fn assert_flushed(h: &HalfRim, d: [f64; 2]) {
        let geom = h.pseudo.geom(h.geom).unwrap();
        let surface = geom
            .fillet_surface(None)
            .unwrap()
            .expect("a chamfer surface")
            .clone();
        let SurfaceKind::ProceduralBlend(pb) = surface.kind() else {
            panic!("a chamfer of setbacks by its definition: {surface:?}");
        };
        let bound = pb.def.bound;
        assert!(bound <= TRACE_TOL, "certified bound {bound}");
        let slack = bound + 1e-9;
        let want = [(RIM_R - d[0], RIM_Z), (RIM_R, RIM_Z - d[1])];
        for (rail, &(r, z)) in want.iter().enumerate() {
            let curve = h
                .pseudo
                .brep()
                .edge(h.rails[rail])
                .unwrap()
                .curve
                .clone()
                .unwrap();
            let ivl = curve.natural_interval();
            for i in 0..=40 {
                let p = curve
                    .evaluate_point(&h.k, ivl.evaluate(f64::from(i) / 40.0))
                    .unwrap();
                let rho = (p.x * p.x + p.y * p.y).sqrt();
                assert!((rho - r).abs() <= slack, "rail {rail}: r {rho} vs {r}");
                assert!((p.z - z).abs() <= slack, "rail {rail}: z {} vs {z}", p.z);
            }
        }
        let dom = surface.natural_uv_domain_with_nurb(&h.k).unwrap();
        for i in 0..=8 {
            for j in 1..4 {
                let uv = dom.evaluate(f64::from(i) / 8.0, f64::from(j) / 4.0);
                let p = surface.evaluate_point(&h.k, uv).unwrap();
                let rho = (p.x * p.x + p.y * p.y).sqrt();
                let on_line = RIM_R - d[0] + (d[0] / d[1]) * (RIM_Z - p.z);
                assert!(
                    (rho - on_line).abs() <= 3.0 * slack,
                    "({rho}, {}) off the chord",
                    p.z
                );
            }
        }
    }

    /// The station trace on the half rim through its two rail-end points (s = 0 and 1):
    /// the stations at the closed-form feet, s strictly increasing, the given points
    /// processed, and the flushed rails and chord the closed form's within the certified
    /// bound. The trace surfaces are the faces as offset surfaces at distance 0, which the
    /// driver's offset check takes.
    #[test]
    fn station_trace_of_a_half_rim_runs_on_the_closed_form_feet() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        assert!(h
            .surfaces
            .iter()
            .all(|s| matches!(s.kind(), SurfaceKind::OffsetSurface(_))));
        let given = [given_at(&mut h, 0.0, d), given_at(&mut h, 1.0, d)];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        t.result.unwrap();
        assert_stations(&t.flushed, d, true);
        assert_eq!(t.flushed.first().unwrap().user_doubles[0], 0.0);
        assert_eq!(t.flushed.last().unwrap().user_doubles[0], 1.0);
        for (p, g) in t.given.iter().zip(&given) {
            assert!(p.curve_parameter >= 0.0, "{p:?}");
            // (a rail end is never moved)
            for rail in 0..2 {
                assert_eq!(p.uv_curve_pv[rail][0], g.uv_curve_pv[rail][0]);
            }
        }
        assert_eq!(t.given[0].curve_parameter, 0.0);
        assert_flushed(&h, d);
    }

    /// The same trace with the rail ends given the other way round: s strictly decreasing,
    /// the same feet, rails and chord.
    #[test]
    fn station_trace_of_a_half_rim_runs_backwards_along_the_edge() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let given = [given_at(&mut h, 1.0, d), given_at(&mut h, 0.0, d)];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        t.result.unwrap();
        assert_stations(&t.flushed, d, false);
        assert!(t.given.iter().all(|p| p.curve_parameter >= 0.0));
        // (the tangents run along the trace: foot 0 from 135 degrees towards -45)
        let first = t.flushed.first().unwrap();
        let theta = angle_of(first.surface_pv[0][0][0]);
        let tau = Vec3::new(-theta.sin(), theta.cos(), 0.0);
        assert!(
            (first.curve_pv[1] + tau).length() < 1e-9,
            "{:?}",
            first.curve_pv[1]
        );
        assert_flushed(&h, d);
        // the geometry runs along the trace, as every generator hands it back (the corner
        // code takes its first row as the trace's start): rail 0 and the surface's first row
        // start at the trace's first station's foot 0 (on r = 9, z = 20 at the given s = 1's
        // end, 135 degrees, as assert_stations checks), not at the end s starts from
        let start = first.surface_pv[0][0][0];
        assert!(angle_of(start) > 1.5, "{start:?}");
        let geom = h.pseudo.geom(h.geom).unwrap();
        let surface = geom.fillet_surface(None).unwrap().unwrap().clone();
        let SurfaceKind::ProceduralBlend(pb) = surface.kind() else {
            panic!("a chamfer of setbacks by its definition");
        };
        // (the reversed definition proves its bound again)
        let proved = pb.recertify(&h.k).unwrap();
        assert!(proved <= TRACE_TOL, "re-certified bound {proved}");
        let slack = pb.def.bound + 1e-9;
        let rail0 = h
            .pseudo
            .brep()
            .edge(h.rails[0])
            .unwrap()
            .curve
            .clone()
            .unwrap();
        let p = rail0
            .evaluate_point(&h.k, rail0.natural_interval().min())
            .unwrap();
        assert!(
            p.distance(start) <= slack,
            "rail 0 starts at {p:?}, not {start:?}"
        );
        let dom = surface.natural_uv_domain_with_nurb(&h.k).unwrap();
        let p = surface
            .evaluate_point(&h.k, dom.evaluate(0.0, 0.0))
            .unwrap();
        assert!(
            p.distance(start) <= slack,
            "the surface starts at {p:?}, not {start:?}"
        );
        // and rail 1 and the surface's other side at foot 1, on the wall at r = 10, z = 19.5
        let start1 = first.surface_pv[1][0][0];
        let rail1 = h
            .pseudo
            .brep()
            .edge(h.rails[1])
            .unwrap()
            .curve
            .clone()
            .unwrap();
        let p = rail1
            .evaluate_point(&h.k, rail1.natural_interval().min())
            .unwrap();
        assert!(
            p.distance(start1) <= slack,
            "rail 1 starts at {p:?}, not {start1:?}"
        );
        let p = surface
            .evaluate_point(&h.k, dom.evaluate(0.0, 1.0))
            .unwrap();
        assert!(
            p.distance(start1) <= slack,
            "the surface's other side starts at {p:?}, not {start1:?}"
        );
        // the uv curves are reversed with their rails: each rail's mate uv curve, over the
        // rail's interval, starts at the trace's first foot on its support (135 degrees, as
        // above) and maps onto the rail at every parameter, within the bound
        for (rail, want) in [start, start1].into_iter().enumerate() {
            let b = h.pseudo.brep();
            let curve = b.edge(h.rails[rail]).unwrap().curve.clone().unwrap();
            let prim = edgeuse::primary_edgeuse(b, h.rails[rail]).unwrap().unwrap();
            let mate = edgeuse::mate(b, prim).unwrap().unwrap();
            let (Some(uv_curve), _) = edgeuse::uv_trim_curve(b, mate).unwrap() else {
                panic!("rail {rail} has no mate uv curve");
            };
            let ivl = curve.natural_interval();
            for i in 0..=8 {
                let t = ivl.evaluate(f64::from(i) / 8.0);
                let w = uv_curve.evaluate_point(&h.k, t).unwrap();
                let on = h.surfaces[rail]
                    .evaluate_point(&h.k, Point2::new(w.x, w.y))
                    .unwrap();
                let p = curve.evaluate_point(&h.k, t).unwrap();
                assert!(
                    on.distance(p) <= slack,
                    "rail {rail} at {t}: uv {on:?} vs rail {p:?}"
                );
                if i == 0 {
                    assert!(
                        on.distance(want) <= slack,
                        "rail {rail}: uv curve starts at {on:?}, not {want:?}"
                    );
                }
            }
        }
    }

    /// The seam both the solver's trace and its re-trace go through,
    /// [`FilletIntersector::do_point_intersection`], traces a chamfer of setbacks at its
    /// stations, never by marching its offsets (at 0, whose feet are apart by design): through
    /// the rail ends at s = 0 and 1 the stations, rails and chord are the closed form's as
    /// above, the given points processed, and the intersector says it traced stations. Given
    /// points out of order are refused as the station trace refuses them (not with a march's
    /// error), and the intersector says so there too: the solver's trace reads that to return
    /// the refusal, with no cusp recovery.
    #[test]
    fn point_intersection_of_a_chamfer_of_setbacks_traces_its_stations() {
        use crate::intersect::surface_intersector::p4::PointIntersectionOptions;
        let d = [1.0, 0.5];
        for given_s in [&[0.0, 1.0][..], &[0.2, 0.6, 0.4][..]] {
            let mut h = half_rim(d);
            let given: Vec<TsectPnt> = given_s.iter().map(|&s| given_at(&mut h, s, d)).collect();
            let [s0, s1] = &h.surfaces;
            let mut fi: FilletIntersector<'_, '_, NoSurfaceHandle, FilletGeomId> =
                FilletIntersector::new(&h.k, s0, &h.domains[0], s1, &h.domains[1], None).unwrap();
            let ids: Vec<TsectId> = given
                .iter()
                .map(|p| fi.base.base.alloc_point(p.clone()))
                .collect();
            let surfaces = [
                fi.base.base.surface(SrfIndex::First),
                fi.base.base.surface(SrfIndex::Second),
            ];
            let mut solver = IntersectorSolver::new(&mut h.fs, surfaces).unwrap();
            let opts = PointIntersectionOptions {
                extend_before_start: false,
                extend_after_end: false,
                start_direction: None,
                end_direction: None,
                approx_tol_3d: Some(1.0e-3),
                ang_tol_rad: None,
            };
            let r = fi.do_point_intersection(&mut solver, &mut h.pseudo, &h.k, &ids, &opts);
            assert!(fi.traced_stations());
            if given_s.len() == 2 {
                r.unwrap();
                for &id in &ids {
                    assert!(fi.base.base.point(id).unwrap().curve_parameter >= 0.0);
                }
                assert_flushed(&h, d);
            } else {
                let err = r.expect_err("refused");
                assert_eq!(
                    err.blend_failure(),
                    Some(&BlendFailure::Solver(SolverFailure::StationsNotMonotone {
                        index: 2
                    })),
                    "{err:?}"
                );
            }
        }
    }

    /// Extensions both ways from given points inside the edge (s = 0.3 and 0.6): steps of
    /// the larger setback until foot 0 has moved 5 setbacks, so the trace ends
    /// [`extension_reach`] past each given point's foot 0 (above), the given points
    /// processed, the first after the extension's stations.
    #[test]
    fn station_trace_extensions_step_five_setbacks_past_the_given_points() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let given = [given_at(&mut h, 0.3, d), given_at(&mut h, 0.6, d)];
        let opts = StationTraceOptions {
            extend_before_start: true,
            extend_after_end: true,
            ..StationTraceOptions::default()
        };
        let t = trace(&mut h, &given, &opts);
        t.result.unwrap();
        assert_stations(&t.flushed, d, true);
        let reach = extension_reach();
        let first = angle_of(t.flushed.first().unwrap().surface_pv[0][0][0]);
        let last = angle_of(t.flushed.last().unwrap().surface_pv[0][0][0]);
        let given_first = angle_of(given[0].surface_pv[0][0][0]);
        let given_last = angle_of(given[1].surface_pv[0][0][0]);
        assert!(
            (given_first - reach - first).abs() < 1e-9,
            "{first} vs {given_first} - {reach}"
        );
        assert!(
            (given_last + reach - last).abs() < 1e-9,
            "{last} vs {given_last} + {reach}"
        );
        assert!(t.given[0].curve_parameter > 0.0);
        assert!(t.given[1].curve_parameter > t.given[0].curve_parameter);
        assert_flushed(&h, d);
    }

    /// The angle an extension turns foot 0 through on the circle r = 9 (above): steps asking
    /// h = min(1, 5 − moved) while that exceeds the driver's floor `5 SM_EFF_ZERO`, each
    /// turning asin(h / 9) and moving foot 0 the chord 18 sin(Δθ / 2).
    fn extension_reach() -> f64 {
        let (cap, r0) = (5.0, RIM_R - 1.0);
        let (mut moved, mut turned) = (0.0_f64, 0.0_f64);
        for _ in 0..8 {
            let h = 1.0_f64.min(cap - moved);
            if h - EFF_ZERO * cap <= 0.0 {
                break;
            }
            let dt = (h / r0).asin();
            turned += dt;
            moved += 2.0 * r0 * (0.5 * dt).sin();
        }
        turned
    }

    /// A trace through three given points (s = 0, 0.5, 1): the stations at the closed-form
    /// feet, s strictly increasing, every given point processed in order, and the flushed
    /// rails and chord the closed form's.
    #[test]
    fn station_trace_through_three_given_points_runs_on_the_closed_form_feet() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let given = [
            given_at(&mut h, 0.0, d),
            given_at(&mut h, 0.5, d),
            given_at(&mut h, 1.0, d),
        ];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        t.result.unwrap();
        assert_stations(&t.flushed, d, true);
        assert_eq!(t.given[0].curve_parameter, 0.0);
        assert!(t.given[1].curve_parameter > 0.0);
        assert!(t.given[2].curve_parameter > t.given[1].curve_parameter);
        // the middle given point is one of the flushed stations, at its own s
        assert!(t
            .flushed
            .iter()
            .any(|p| p.user_doubles[0] == 0.5 && p.uv_curve_pv == t.given[1].uv_curve_pv));
        assert_flushed(&h, d);
    }

    /// A rail end whose `user_doubles[0]` is stale (0.37 on the point at s = 0, as a point no
    /// solve of this solver made may carry) is read at its own station: its foot 0 is not at
    /// the station at 0.37 (that station's foot is at another angle), so s is foot 0's drop
    /// on the edge, 0 (the edge's start, foot 0 at −45 degrees on r = 9); the trace runs and
    /// leaves s = 0 in the point. The solver's `station_of_point` gives the same.
    #[test]
    fn station_trace_reads_a_stale_edge_parameter_at_its_own_station() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let mut stale = given_at(&mut h, 0.0, d);
        stale.user_doubles[0] = 0.37;
        stale.user_doubles[1] = 0.37;
        let s = h.fs.station_of_point(&h.k, &stale).unwrap();
        assert!(s.abs() < 1e-9, "{s}");
        let given = [stale, given_at(&mut h, 1.0, d)];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        t.result.unwrap();
        assert!(
            t.given[0].user_doubles[0].abs() < 1e-9,
            "{:?}",
            t.given[0].user_doubles
        );
        assert_stations(&t.flushed, d, true);
        assert_flushed(&h, d);
    }

    /// Given points whose s does not run one way are refused, typed, before anything is
    /// traced.
    #[test]
    fn station_trace_refuses_given_points_out_of_order() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let given = [
            given_at(&mut h, 0.2, d),
            given_at(&mut h, 0.6, d),
            given_at(&mut h, 0.4, d),
        ];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        let err = t.result.expect_err("refused");
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::Solver(SolverFailure::StationsNotMonotone {
                index: 2
            })),
            "{err:?}"
        );
        assert!(t.flushed.is_empty());
    }

    /// A given point whose foot 1 is 0.01 above its station (z = 19.51 on the wall) is
    /// refused, typed, with its distance, not moved onto the chamfer.
    #[test]
    fn station_trace_refuses_a_given_foot_off_its_station() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let mut off = given_at(&mut h, 0.5, d);
        let foot1 = off.surface_pv[1][0][0] + Vec3::Z * 0.01;
        off.uv_curve_pv[1][0] = wall_uv(&h, foot1);
        let given = [given_at(&mut h, 0.0, d), off];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        let err = t.result.expect_err("refused");
        let Some(&BlendFailure::Solver(SolverFailure::SetbackFootOffStation { rail, moved })) =
            err.blend_failure()
        else {
            panic!("{err:?}");
        };
        assert_eq!(rail, 1);
        assert!((moved - 0.01).abs() < 1e-9, "{moved}");
    }

    /// A given point outside a trace domain (the top face's cut to x, y >= 0, foot 0 of the
    /// station at −45 degrees below y = 0) would clip the trace before its last given point:
    /// refused, typed.
    #[test]
    fn station_trace_refuses_a_given_point_outside_a_domain() {
        let d = [1.0, 0.5];
        let mut h = half_rim(d);
        let given = [given_at(&mut h, 0.0, d), given_at(&mut h, 1.0, d)];
        h.domains[0] = Extent2d::from_points(Point2::new(0.5, 0.5), Point2::new(1.0, 1.0));
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        let err = t.result.expect_err("refused");
        assert_eq!(
            err.intersect_failure(),
            Some(&IntersectFailure::PointOutsideDomain { surface: 0 }),
            "{err:?}"
        );
    }

    /// The rim's whole circle as the edge (from -90 degrees, the wall's seam), counterclockwise
    /// or, `reversed`, clockwise: a closed leader, its one vertex at the seam.
    fn whole_rim(d: [f64; 2], reversed: bool) -> (HalfRim, BSplineCurve) {
        let mut edge = arc(RIM_R, RIM_Z, -90.0, 4);
        if reversed {
            let ivl = edge.natural_interval();
            edge.reverse_parameterization(&ivl).unwrap();
        }
        (rim_trace_on(d, edge.copy(), true), edge)
    }

    /// The station of `h` at s, from guesses at the closed-form feet (its angle on `h`'s own
    /// edge), not yet processed.
    fn given_on(h: &mut HalfRim, edge: &BSplineCurve, s: f64, d: [f64; 2]) -> TsectPnt {
        let feet = feet_at_angle(angle_at(edge, s), d);
        let guesses = [plate_uv(feet[0]), whole_wall_uv(h, feet[1])];
        let mut p =
            h.fs.station_point(&h.k, s, guesses, &h.domains)
                .unwrap()
                .expect("a station");
        p.curve_parameter = -1.0;
        p
    }

    /// The whole wall's parameters of a point on it (u a quarter per 90 degrees from -90).
    fn whole_wall_uv(h: &HalfRim, p: Point3) -> Point2 {
        let wall = h.fs.curve_based.solver.rail(1).unwrap();
        let a = (p.y.atan2(p.x).to_degrees() + 90.0).rem_euclid(360.0) / 360.0;
        offset_local_point_solve(&h.k, wall, &h.domains[1], p, Point2::new(a, 0.9))
            .unwrap()
            .expect("on the wall")
            .0
    }

    /// A closed leader (#13710): the rim's whole circle as the edge, both given points at its
    /// seam (-90 degrees, where the solver finds s = 0 for both). The stations run one period,
    /// s from 0 up to 1 (the edge's own direction, no point between to say otherwise), each
    /// at the closed-form feet, the last given point re-solved at s = 1 on the same feet.
    #[test]
    fn a_closed_leader_s_seam_points_take_the_two_ends_of_its_interval() {
        let d = [1.0, 0.5];
        let (mut h, edge) = whole_rim(d, false);
        let seam = given_on(&mut h, &edge, 0.0, d);
        let t = trace(
            &mut h,
            &[seam.clone(), seam.clone()],
            &StationTraceOptions::default(),
        );
        t.result.unwrap();
        assert_stations(&t.flushed, d, true);
        assert_eq!(t.flushed.first().unwrap().user_doubles[0], 0.0);
        assert_eq!(t.flushed.last().unwrap().user_doubles[0], 1.0);
        let [first, last] = [&t.given[0], &t.given[1]];
        assert_eq!(first.user_doubles[0], 0.0);
        assert_eq!(last.user_doubles[0], 1.0);
        for rail in 0..2 {
            let moved = first.surface_pv[rail][0][0].distance(last.surface_pv[rail][0][0]);
            assert!(moved < 1e-9, "rail {rail}: the seam's feet {moved} apart");
        }
        assert_flushed(&h, d);
    }

    /// The same closed rim with the edge running clockwise (reversed: s = 0 and 1 still at the
    /// seam, s growing clockwise), traced the other way: given points at the seam, then at s =
    /// 0.75 and 0.25 (down the edge), then at the seam again. The first seam point takes s = 1,
    /// the last s = 0, and the stations run down, each at the closed-form feet.
    #[test]
    fn a_reversed_closed_leader_traced_down_its_edge_runs_from_1_to_0() {
        let d = [1.0, 0.5];
        let (mut h, edge) = whole_rim(d, true);
        let given = [
            given_on(&mut h, &edge, 0.0, d),
            given_on(&mut h, &edge, 0.75, d),
            given_on(&mut h, &edge, 0.25, d),
            given_on(&mut h, &edge, 0.0, d),
        ];
        let t = trace(&mut h, &given, &StationTraceOptions::default());
        t.result.unwrap();
        assert_stations(&t.flushed, d, false);
        assert_eq!(t.flushed.first().unwrap().user_doubles[0], 1.0);
        assert_eq!(t.flushed.last().unwrap().user_doubles[0], 0.0);
        assert_eq!(t.given[0].user_doubles[0], 1.0);
        assert_eq!(t.given[3].user_doubles[0], 0.0);
        assert_flushed(&h, d);
    }
}
