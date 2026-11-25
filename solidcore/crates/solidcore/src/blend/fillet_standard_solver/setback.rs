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
//! **A clipped step** (a foot on its face's boundary iso-line) solves that foot on the line
//! and s together, then the other foot at that s ([`SetbackFs::clipped_feet`]).
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
use crate::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
use crate::blend::fillet_section::setback_station::{
    setback_foot, setback_start, setback_tangent, EdgeJet,
};
use crate::blend::fillet_solver::p1::{ExtensionCorners, FilletSolver, FilletSolverOps};
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::blend::{SolverFailure, TraceFailure};
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

    /// The edge's point and first two derivatives at `s`, on the extended edge
    /// ([`CurveBasedFs::evaluate_base_curve`], which extends it further when `s` is past it).
    ///
    /// # Errors
    ///
    /// Those of the evaluation.
    pub fn jet(&mut self, k: &Kernel, s: f64) -> Result<EdgeJet> {
        let pv = self.curve_based.evaluate_base_curve(k, s, 2)?;
        Ok(EdgeJet {
            point: at3(&pv, 0, OP)?,
            d1: at3(&pv, 1, OP)?,
            d2: at3(&pv, 2, OP)?,
        })
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
        let solver = &self.curve_based.solver;
        let mut supports: Vec<Surface> = Vec::new();
        let mut edge_uv = guesses;
        let mut normals = [Vec3::ZERO; 2];
        for rail in 0..2 {
            let offset = solver.rail(rail)?;
            let support = Surface::from(offset.surface.clone());
            let guess = guesses.get(rail).copied().unwrap_or(Point2::new(0.0, 0.0));
            let domain = domains
                .get(rail)
                .copied()
                .unwrap_or_else(|| offset.surface.natural_uv_domain());
            let uv = offset_local_point_solve(k, offset, &domain, jet.point, guess)?
                .map_or(guess, |(uv, _)| uv);
            if let Some(slot) = edge_uv.get_mut(rail) {
                *slot = uv;
            }
            let n = support.evaluate_normal(k, uv, Side::Left, Side::Left)?;
            if let Some(slot) = normals.get_mut(rail) {
                *slot = n;
            }
            supports.push(support);
        }
        let mut feet = [None, None];
        for (rail, foot) in feet.iter_mut().enumerate() {
            let other = 1 - rail;
            let (Some(support), Some(&uv), Some(&d)) = (
                supports.get(rail),
                edge_uv.get(rail),
                self.setbacks.get(rail),
            ) else {
                continue;
            };
            let into =
                normals.get(other).copied().unwrap_or(Vec3::ZERO) * solver.orientation(other)?;
            let Some(t) = setback_tangent(k, support, uv, jet, into)? else {
                continue;
            };
            let Some(start) = setback_start(k, support, uv, t, d)? else {
                continue;
            };
            *foot = setback_foot(k, support, jet, d, start)?.map(|f| f.uv);
        }
        Ok(feet)
    }

    /// The base surface domains the solver's feet are sought over: each rail's natural
    /// domain, extended as the trace extends it ([`FilletSolver::domain_extension`]).
    ///
    /// # Errors
    ///
    /// Those of the domains and the extension.
    pub fn trace_domains(&self, k: &Kernel) -> Result<[Extent2d; 2]> {
        let solver = &self.curve_based.solver;
        let mut out = [Extent2d::default(); 2];
        for (rail, slot) in out.iter_mut().enumerate() {
            let surface = solver.rail(rail)?;
            let domain = surface.base_surface().natural_uv_domain_with_nurb(k)?;
            let size = solver.domain_extension(k, Some(surface.base_surface()), &domain)?;
            *slot = Extent2d::from_points(
                Point2::new(domain.min().x - size.x, domain.min().y - size.y),
                Point2::new(domain.max().x + size.x, domain.max().y + size.y),
            );
        }
        Ok(out)
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
        Ok(FilletSolverType::Setbacks)
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
        let w = 20.0;
        let plate = Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 1,
                v_degree: 1,
                control_points: &[
                    Point3::new(-w, -w, 0.0),
                    Point3::new(-w, w, 0.0),
                    Point3::new(w, -w, 0.0),
                    Point3::new(w, w, 0.0),
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
            (0.0, -WALL),
            (WALL, -WALL),
            (WALL, 0.0),
            (WALL, WALL),
            (0.0, WALL),
            (-WALL, WALL),
            (-WALL, 0.0),
        ]
        .into_iter()
        .zip([1.0, h, 1.0, h, 1.0, h, 1.0])
        {
            for z in [0.0, HEIGHT] {
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
                u_multiplicities: &[3, 2, 2, 3],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
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
}
