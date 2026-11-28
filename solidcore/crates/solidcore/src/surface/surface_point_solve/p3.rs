//! Surface-point solving, part 3: boundary refinement and the local iteration driver.
//!
//! Upstream: `source/SMLib/src/SmSurfacePointSolve.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`Surface::global_point_solve_1`] | `SmSurface::GlobalPointSolve_1` |
//! | [`Surface::local_point_solve_1`] | `SmSurface::LocalPointSolve_1` |
//! | [`SurfPtSolveStepper::find_boundary_solution`] | `FindBoundarySolution` |
//! | [`SurfPtSolveStepper::check_crease`] | `CheckCrease` |
//! | [`SurfPtSolveStepper::find_next_step`] | `FindNextStep` |
//!
//! Deviations: out-parameters become result records; object pointers become caller
//! handles. Direct local solutions retain the default variable count; the existing
//! `LocalPointSolve` dispatcher sets it to two, as upstream does. Out-of-bounds
//! evaluation is enabled in place on the surface and restored after, as upstream's
//! `SmTemporaryChangeValue`. The nesting check of the former evaluation copy still
//! runs: it checks every owned geometry branch against the `MAX_GEOMETRY_NESTING` (32)
//! depth bound. `CheckCrease` returns immediately because upstream disables it with
//! `constexpr sbDoCreaseSubdomains = FALSE`. The local driver is iterative with
//! upstream's fixed bound of 35. Not upstream (#20438): when the global solver drops a
//! minimum beyond the target distance, `GlobalPointSolve_1` answers with the tree
//! solve (`GlobalPointSolve_0`, point testing off, on a curve-bounded surface's basis
//! up to `MAX_GEOMETRY_NESTING` levels, on an ownerless copy with a cache of its own,
//! as the face's trims play no part), which bounds every candidate by the target, so a
//! nearer point within it is not lost (#20483); when the tree solve fails as upstream
//! can (no tree for the surface, a failed evaluation) the drop stands. Not upstream
//! (tangency-labs/solidcore#3446): a minimize that converges to a saddle of the distance (a
//! point off the surface whose Hessian has a negative eigenvalue) goes on from the least
//! distance along that eigenvalue's direction ([`SurfPtSolveStepper::leave_saddle`]), within
//! the same 35 steps; upstream reports the saddle as the minimum.

use super::p1::ErrorNormType;
use super::p2::with_out_of_bounds_enabled_in_place;
use super::{SurfPtGlobalSolver, SurfPtSolveStepper};
use crate::curve::curve::p2::CurvePropertyType;
use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::p5::local_property_analysis;
use crate::curve::curve::Curve;
use crate::curve::iso_curve::IsoCurve;
use crate::error::{native_failure, Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::local_solve_nd::TerminationReason;
use crate::geom::solution_array::{Solution, SolutionArray, SolutionObject, SolutionType};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::surface::surface::p1::zone_tol_3d;
use crate::surface::surface::p2::MAX_GEOMETRY_NESTING;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceKind};

/// The steps of [`SurfPtSolveStepper::leave_saddle`]'s golden-section search: each keeps
/// the golden ratio's 0.618 of the bracket, and 0.618^80 < 2e-17, so the bracket ends below
/// the rounding of a parameter as large as the line's length.
const GOLDEN_STEPS: usize = 80;

/// Whether the iteration's step failed, and its termination reason (`FindNextStep`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextStep {
    /// The step failed: [`TerminationReason::UnableToConverge`] (upstream's
    /// `SM_ERR_NOT_CONVERGING`) or [`TerminationReason::OutOfBounds`] (its
    /// `SM_ERR_OUTSIDE_OF_DOMAIN`). [`TerminationReason::OutOfBounds`] is also the reason of
    /// a step that did not fail, so the reason alone does not say which.
    pub failed: bool,
    /// The reason, including when the step failed.
    pub reason: TerminationReason,
}

impl NextStep {
    const fn success(reason: TerminationReason) -> Self {
        Self {
            failed: false,
            reason,
        }
    }

    const fn failure(reason: TerminationReason) -> Self {
        Self {
            failed: true,
            reason,
        }
    }
}

/// A local point solve, including the best iterate when convergence was not attained.
#[derive(Debug, Clone)]
pub struct LocalPointSolve<O: Copy> {
    /// Upstream's `rbFoundSolution`, independent of the termination reason.
    pub found: bool,
    /// The best solution, with the caller's surface handle.
    pub solution: Solution<O>,
    /// Upstream's `cbi_Reason`.
    pub reason: TerminationReason,
    /// Upstream's `cbi_IterCount` (zero for the starting-guess shortcut).
    pub iterations: i32,
}

/// Ignore ordinary upstream statuses while always propagating missing dependencies.
fn boundary_or_false(value: Result<bool>) -> Result<bool> {
    match value {
        Ok(v) => Ok(v),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(false),
    }
}

impl SurfPtSolveStepper<'_> {
    /// Refines along an outward-pushed boundary (`FindBoundarySolution`).
    ///
    /// Returns whether the boundary point satisfies the outward optimality test.
    /// The updated UV and evaluation remain in this stepper even without a solution.
    ///
    /// # Errors
    ///
    /// Missing dependencies and surface evaluation failures.
    pub fn find_boundary_solution(&mut self, k: &Kernel) -> Result<bool> {
        let tight = self.base.tight_tol();
        let loose = self.base.loose_tol();
        let (pushing, mut bound) = self.pushing_boundary(-tight)?;
        if !pushing {
            return Ok(false);
        }
        self.snap_to_boundaries(loose)?;
        let domain = *self.base.domain();
        if bound == SurfParamType::Both {
            let (pushing, _) = self.pushing_boundary(tight)?;
            if pushing {
                return Ok(true);
            }
            bound = if self.uv().x < domain.u_interval().mid() {
                SurfParamType::UMin
            } else {
                SurfParamType::UMax
            };
        }

        let natural = self.base.surface().natural_uv_domain_with_nurb(k)?;
        let (param, iso, guess, iso_domain, cross_domain) = match bound {
            SurfParamType::UMin => (
                SurfParamType::U,
                natural.u_interval().clamp_value(domain.min().x),
                self.uv().y,
                domain.v_interval(),
                domain.u_interval(),
            ),
            SurfParamType::UMax => (
                SurfParamType::U,
                natural.u_interval().clamp_value(domain.max().x),
                self.uv().y,
                domain.v_interval(),
                domain.u_interval(),
            ),
            SurfParamType::VMin => (
                SurfParamType::V,
                natural.v_interval().clamp_value(domain.min().y),
                self.uv().x,
                domain.u_interval(),
                domain.v_interval(),
            ),
            SurfParamType::VMax => (
                SurfParamType::V,
                natural.v_interval().clamp_value(domain.max().y),
                self.uv().x,
                domain.u_interval(),
                domain.v_interval(),
            ),
            _ => return Ok(false),
        };
        let curve = Curve::from(IsoCurve::from_surface(
            k,
            self.base.surface().clone_for_evaluation()?,
            param,
            iso,
            iso < cross_domain.mid(),
            true,
        )?);
        let mut sol: Solution<()> = Solution::new();
        let found = if self.base.solver_op() == SolverOperation::Signed3dDirectedMaximize {
            boundary_or_false(local_property_analysis(
                k,
                &curve,
                &iso_domain,
                CurvePropertyType::PerpendicularToVector,
                guess,
                None,
                &[self.base.dir_vec()],
                &mut sol,
            ))?
        } else {
            match curve.local_point_solve(
                k,
                &iso_domain,
                SolverOperation::Minimize,
                self.base.test_point(),
                &PointSolveOptions::default(),
                guess,
            ) {
                Ok((found, solution)) => {
                    sol = solution;
                    found
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => false,
            }
        };
        if !found {
            return Ok(false);
        }
        let new_param = sol.start.param(0)?;
        let mut new_uv = self.uv();
        let mut boundary = Self::copy_of(k, self);
        if param == SurfParamType::U {
            new_uv.y = new_param;
        } else {
            new_uv.x = new_param;
        }
        boundary.set_uv(new_uv);
        let (deriv1, deriv2, norm) = if param == SurfParamType::U {
            (boundary.su()?, boundary.suu()?, ErrorNormType::GAbs)
        } else {
            (boundary.sv()?, boundary.svv()?, ErrorNormType::FAbs)
        };
        if boundary.error_norm(norm)? < self.error_norm(norm)? {
            self.assign(&boundary);
        }
        let flip = if self.base.solver_op() == SolverOperation::Maximize {
            true
        } else if self.base.solver_op() == SolverOperation::Normalize {
            let term1 = deriv1.dot(deriv1);
            let term2 = self.gap()?.dot(deriv2);
            term1 + term2 < 0.0
        } else {
            false
        };
        let mut error = self.gap()?.dot(deriv1);
        if flip {
            error = -error;
        }
        Ok(if iso < cross_domain.mid() {
            error > -tight
        } else {
            error < tight
        })
    }

    /// Not upstream (tangency-labs/solidcore#3446): moves a point the minimize converged to
    /// off a saddle of the distance and returns whether it moved. The Newton step solves
    /// for a zero of the distance's gradient, so it converges to a saddle as readily as to a
    /// minimum (Nocedal and Wright, *Numerical Optimization*, 2nd ed., §3.4); a point off the
    /// surface where the Hessian of `|S - P|² / 2`,
    /// `[[Su·Su + g·Suu, Su·Sv + g·Suv], [Su·Sv + g·Suv, Sv·Sv + g·Svv]]` with `g = S - P`,
    /// has a negative eigenvalue is no minimum, and the distance falls along that
    /// eigenvalue's eigenvector `e` (a direction of negative curvature: Moré and Sorensen,
    /// *Math. Programming* 16 (1979) 1–20). The point moves to the least distance along the
    /// line `uv + s e` in whichever direction is lower (a golden-section search over `s` in
    /// the domain, and no farther than the 3D reach `|Su e_u + Sv e_v| s` of twice the
    /// distance, past which no point of the line's tangent is nearer), when that is nearer
    /// by more than the solve's tight tolerance; the iteration goes on from there. The
    /// iso-direction tests of [`SurfPtGlobalSolver::is_local_max`] miss such a saddle where
    /// the surface's parameter lines meet at a small angle and the descent is across them
    /// (a coil's wall where the profile circle runs almost along the screw motion: its
    /// partials 3° apart, a saddle 0.0137 from a point on the wall's start station).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the evaluation.
    pub fn leave_saddle(&mut self, k: &Kernel) -> Result<bool> {
        let tight = self.base.tight_tol();
        let distance = self.gap_length()?;
        if distance.is_nan() || distance <= tight {
            return Ok(false);
        }
        let g = self.gap()?;
        let (su, sv) = (self.su()?, self.sv()?);
        let (suu, suv, svv) = (self.suu()?, self.suv()?, self.svv()?);
        let a = su.dot(su) + g.dot(suu);
        let b = su.dot(sv) + g.dot(suv);
        let c = sv.dot(sv) + g.dot(svv);
        // the smaller eigenvalue of [[a, b], [b, c]] and its eigenvector, the longer of the
        // two rows of the matrix less it, turned a quarter
        let low = 0.5 * (a + c) - (0.25 * (a - c) * (a - c) + b * b).sqrt();
        if low.is_nan() || low >= 0.0 {
            return Ok(false);
        }
        let (e1, e2) = (Point2::new(b, low - a), Point2::new(low - c, b));
        let e = if e1.x * e1.x + e1.y * e1.y >= e2.x * e2.x + e2.y * e2.y {
            e1
        } else {
            e2
        };
        let len = e.x.hypot(e.y);
        let reach = (su * (e.x / len) + sv * (e.y / len)).length();
        if len.is_nan() || reach.is_nan() || len <= 0.0 || reach <= 0.0 {
            return Ok(false);
        }
        let e = Point2::new(e.x / len, e.y / len);
        let domain = *self.base.domain();
        let uv = self.uv();
        let mut best: Option<(f64, Self)> = None;
        for sign in [1.0, -1.0] {
            let d = Point2::new(sign * e.x, sign * e.y);
            // the line's last parameter in the domain, and within the reach
            let mut far = 2.0 * distance / reach;
            for (x, step, lo, hi) in [
                (uv.x, d.x, domain.min().x, domain.max().x),
                (uv.y, d.y, domain.min().y, domain.max().y),
            ] {
                if step > 0.0 {
                    far = far.min((hi - x) / step);
                } else if step < 0.0 {
                    far = far.min((lo - x) / step);
                }
            }
            if far.is_nan() || far <= 0.0 {
                continue;
            }
            let at = |s: f64| Point2::new(uv.x + s * d.x, uv.y + s * d.y);
            let mut trial = Self::copy_of(k, self);
            let value = |s: f64, trial: &mut Self| -> Result<f64> {
                trial.set_uv(domain.clamp_point(at(s)));
                trial.gap_length()
            };
            // golden-section search on [0, far], each step a fixed fraction of the bracket,
            // until the bracket is below the rounding of its ends
            let ratio = 0.5 * (5.0_f64.sqrt() - 1.0);
            let (mut lo, mut hi) = (0.0, far);
            let mut x1 = hi - ratio * (hi - lo);
            let mut x2 = lo + ratio * (hi - lo);
            let mut f1 = value(x1, &mut trial)?;
            let mut f2 = value(x2, &mut trial)?;
            for _ in 0..GOLDEN_STEPS {
                if f1 <= f2 {
                    hi = x2;
                    (x2, f2) = (x1, f1);
                    x1 = hi - ratio * (hi - lo);
                    f1 = value(x1, &mut trial)?;
                } else {
                    lo = x1;
                    (x1, f1) = (x2, f2);
                    x2 = lo + ratio * (hi - lo);
                    f2 = value(x2, &mut trial)?;
                }
            }
            let s = if f1 <= f2 { x1 } else { x2 };
            let f = value(s, &mut trial)?;
            if f < distance - tight && best.as_ref().is_none_or(|(b, _)| f < *b) {
                best = Some((f, trial));
            }
        }
        Ok(match best {
            Some((_, point)) => {
                self.assign(&point);
                true
            }
            None => false,
        })
    }

    /// The disabled crease check (`CheckCrease`): `(processed, found)` is `(false, false)`.
    pub fn check_crease(&mut self, _previous: &mut Self) -> (bool, bool) {
        (false, false)
    }

    /// Chooses the next Newton, tangent-plane or line-search step (`FindNextStep`).
    ///
    /// # Errors
    ///
    /// Missing dependencies and evaluation errors. Ordinary iteration failure is
    /// represented by [`NextStep::failed`] so its reason is retained.
    pub fn find_next_step(&mut self, k: &Kernel, previous: &mut Self) -> Result<NextStep> {
        use TerminationReason::{
            FoundAnswerConverged as Converged, OutOfBounds, StillIterating, UnableToConverge,
        };
        let tight = self.base.tight_tol();
        let (_, bound) = self.pushing_boundary(tight)?;
        if self.iter_count() > 0 || bound != SurfParamType::Neither {
            if bound == SurfParamType::Both {
                return Ok(NextStep::success(OutOfBounds));
            }
            if boundary_or_false(self.find_boundary_solution(k))? {
                return Ok(NextStep::success(if self.pushing_boundary(tight)?.0 {
                    OutOfBounds
                } else {
                    Converged
                }));
            }
        }
        self.set_nr_suspect(false);
        let mut nr = Self::copy_of(k, previous);
        let mut reason = nr.step(k, previous, false)?;
        if reason == Converged {
            self.assign(&nr);
            return Ok(NextStep::success(reason));
        }
        let mut got_nr = reason == StillIterating;
        if got_nr {
            match nr.adjust_step(k, previous) {
                Ok((adjusted, scale)) => {
                    if nr.converged_with(k, true)? {
                        self.assign(&nr);
                        return Ok(NextStep::success(reason));
                    }
                    if adjusted && scale < EFF_ZERO_SQ {
                        self.set_nr_suspect(true);
                    }
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => {
                    self.set_nr_suspect(true);
                    got_nr = false;
                }
            }
        } else {
            self.set_nr_suspect(true);
        }
        let mut suspect = self.nr_step_suspect() || nr.nr_step_suspect();
        if !suspect {
            let step = nr.uv() - self.uv();
            let factor = self.max_step() / 2.0;
            let domain = self.base.domain();
            if step.x.abs() > factor * domain.u_interval().length()
                || step.y.abs() > factor * domain.v_interval().length()
            {
                suspect = true;
            }
        }
        if !suspect
            && nr.error_norm(ErrorNormType::SumAbs)?
                > previous.error_norm(ErrorNormType::SumAbs)? * 0.75
            && nr.error_norm(ErrorNormType::SumAbs)? > self.base.loose_tol()
        {
            suspect = true;
        }
        self.set_nr_suspect(suspect);
        if got_nr && !self.nr_step_suspect() {
            self.assign(&nr);
            return Ok(NextStep::success(reason));
        }
        self.set_nr_suspect(true);
        let mut plane = Self::copy_of(k, previous);
        reason = plane.step(k, previous, true)?;
        if reason == Converged {
            // Upstream deliberately assigns sNREval here, even after the plane step.
            self.assign(&nr);
            return Ok(NextStep::success(reason));
        }
        let mut got_plane = reason == StillIterating;
        if got_plane {
            match plane.adjust_step(k, previous) {
                Ok(_) => {
                    if plane.converged_with(k, true)? {
                        self.assign(&plane);
                        return Ok(NextStep::success(reason));
                    }
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => got_plane = false,
            }
        }
        if !got_nr && !got_plane {
            self.assign(previous);
            return Ok(if nr.bound_hits_u() != 0 || nr.bound_hits_v() != 0 {
                NextStep::success(OutOfBounds)
            } else {
                NextStep::failure(UnableToConverge)
            });
        }
        let norm = if self.base.solver_op() == SolverOperation::Normalize {
            ErrorNormType::SumAbs
        } else {
            ErrorNormType::Gap
        };
        if got_plane
            && boundary_or_false(plane.find_boundary_solution(k))?
            && plane.error_norm(norm)? <= previous.error_norm(norm)?
            && (!got_nr || plane.error_norm(norm)? <= nr.error_norm(norm)?)
        {
            self.assign(&plane);
            return Ok(if self.pushing_boundary(tight)?.0 {
                NextStep::failure(OutOfBounds)
            } else {
                NextStep::success(Converged)
            });
        }
        // Both CheckCrease calls in upstream are disabled unconditionally.
        if got_nr
            && boundary_or_false(nr.find_boundary_solution(k))?
            && nr.error_norm(norm)? < previous.error_norm(norm)? + EFF_ZERO
            && (!got_plane || nr.error_norm(norm)? < plane.error_norm(norm)? + EFF_ZERO)
        {
            self.assign(&nr);
            return Ok(if self.pushing_boundary(tight)?.0 {
                NextStep::failure(OutOfBounds)
            } else {
                NextStep::success(Converged)
            });
        }
        if got_nr {
            match nr.line_search(k, previous) {
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                _ => {}
            }
        }
        if got_plane {
            match plane.line_search(k, previous) {
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                _ => {}
            }
        }
        if got_nr && (!got_plane || nr.error_norm(norm)? < plane.error_norm(norm)?) {
            self.assign(&nr);
        } else {
            self.assign(&plane);
        }
        if self.bound_hits_u().unsigned_abs() > 3 || self.bound_hits_v().unsigned_abs() > 3 {
            return Ok(NextStep::success(OutOfBounds));
        }
        if self.error_norm(norm)? >= previous.error_norm(norm)?
            || (self.uv() - previous.uv()).length() < EFF_ZERO_SQ
        {
            return Ok(
                if self.bound_hits_u().unsigned_abs() > 1 || self.bound_hits_v().unsigned_abs() > 1
                {
                    NextStep::success(OutOfBounds)
                } else {
                    NextStep::failure(UnableToConverge)
                },
            );
        }
        Ok(NextStep::success(reason))
    }
}

impl Surface {
    /// The new global surface-point solve (`GlobalPointSolve_1`).
    ///
    /// Not upstream (#20438): when the solver drops its one candidate, a minimum
    /// farther than `target_distance` by more than the tolerance
    /// ([`SurfPtGlobalSolver::dropped_beyond_target`]), that candidate need not be the
    /// nearest point (#20483), so the tree solve answers instead
    /// ([`Surface::global_point_solve_0`], with the cache's point testing off as the
    /// new solver has none; on a curve-bounded surface's basis, which its evaluations
    /// pass to; on an ownerless copy with a cache of its own, so a face's surface needs
    /// no trimmed cache): it bounds every candidate by the target, so it finds a nearer
    /// point within the target and is empty only when there is none. Its solutions are
    /// shaped by `requested` as the tree solve shapes them. A tree solve that fails as
    /// upstream can (a STEP surface has no decomposition tree; a failed evaluation)
    /// keeps the drop.
    ///
    /// # Errors
    ///
    /// Those of [`SurfPtGlobalSolver::new`] and [`SurfPtGlobalSolver::solve`], and the
    /// unported, cancelled and port-defect errors of [`Surface::global_point_solve_0`]
    /// when the tree solve answers.
    pub fn global_point_solve_1<O: SolutionObject>(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        op: SolverOperation,
        point: Point3,
        distance_tolerance: f64,
        target_distance: Option<f64>,
        requested: SolutionRequested,
        surface_object: Option<O>,
    ) -> Result<SolutionArray<O>> {
        let mut solver = SurfPtGlobalSolver::new(
            k,
            self,
            domain,
            op,
            point,
            distance_tolerance,
            target_distance,
            requested,
        )?;
        let found = solver.solve(k, surface_object)?;
        if !solver.dropped_beyond_target() {
            return Ok(found);
        }
        Ok(self
            .tree_solve_within_target(
                k,
                domain,
                op,
                point,
                distance_tolerance,
                target_distance,
                requested,
                surface_object,
            )?
            .unwrap_or(found))
    }

    /// The tree solve [`Surface::global_point_solve_1`] answers with when the global
    /// solver drops a minimum beyond the target: `None` when it fails as upstream can,
    /// so the drop stands.
    pub(crate) fn tree_solve_within_target<O: SolutionObject>(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        op: SolverOperation,
        point: Point3,
        distance_tolerance: f64,
        target_distance: Option<f64>,
        requested: SolutionRequested,
        surface_object: Option<O>,
    ) -> Result<Option<SolutionArray<O>>> {
        // the tree solve on the surface the distance is evaluated on: a curve-bounded
        // surface passes its evaluations to its basis
        let mut tree_on = self;
        for _ in 0..MAX_GEOMETRY_NESTING {
            match tree_on.kind() {
                SurfaceKind::CurveBounded(s) => tree_on = s.basis(),
                _ => break,
            }
        }
        // on an ownerless copy with a cache slot of its own: point testing is off, so
        // the face's trims play no part, and a face-owned surface's trimmed cache cannot
        // be built without its face; the copy's untrimmed cache never reaches the
        // surface's own slot
        let mut unowned = tree_on.clone_for_evaluation()?;
        unowned.detach_for_edit();
        let _ = unowned.set_owner(None);
        let tree_on = &unowned;
        let tree = match surface_object {
            Some(object) => tree_on.global_point_solve_0_in(
                k,
                object,
                None,
                None,
                true,
                domain,
                op,
                point,
                distance_tolerance,
                target_distance,
                requested,
            ),
            // no handle for the solutions' objects: solve naming a stand-in and
            // return the solutions without objects
            None => tree_on
                .global_point_solve_0_in(
                    k,
                    NoObject,
                    None,
                    None,
                    true,
                    domain,
                    op,
                    point,
                    distance_tolerance,
                    target_distance,
                    requested,
                )
                .map(|t| t.without_objects()),
        };
        // a tree solve that fails as upstream can (a surface with no decomposition
        // tree, such as a STEP surface; a failed evaluation): the global solver's drop
        // stands. Port defects, unported paths and cancellation still propagate.
        native_failure(tree)
    }

    /// The new bounded local surface-point solve (`LocalPointSolve_1`).
    ///
    /// The initial guess is clamped and at most 35 steps are taken. Upstream reports
    /// a best point as found for minimization/maximization even without convergence.
    ///
    /// # Errors
    ///
    /// Missing callees and evaluation failures.
    pub fn local_point_solve_1<O: Copy>(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        op: SolverOperation,
        point: Point3,
        guess: Point2,
        surface_object: Option<O>,
    ) -> Result<LocalPointSolve<O>> {
        with_out_of_bounds_enabled_in_place(self, || {
            self.local_point_solve_1_body(k, domain, op, point, guess, surface_object)
        })
    }

    fn local_point_solve_1_body<O: Copy>(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        op: SolverOperation,
        point: Point3,
        guess: Point2,
        surface_object: Option<O>,
    ) -> Result<LocalPointSolve<O>> {
        use TerminationReason::{
            FoundAnswerConverged as Converged, OutOfBounds, StillIterating, UnableToConverge,
        };
        let guess = domain.clamp_point(guess);
        let tight = EFF_ZERO * (1.0 + point.max_dimension());
        let loose = if self.owner().is_some() {
            zone_tol_3d(k, self)?
        } else if op == SolverOperation::Intersect {
            0.01
        } else {
            EFF_ZERO_SQRT * 100.0
        };
        let mut curr = SurfPtSolveStepper::new(k, self, domain, point, guess, tight, loose, op)?;
        let mut solution = Solution::new();
        solution.solution_type = SolutionType::SingleValue;
        solution.num_objects = 1;
        if let Some(slot) = solution.objects.first_mut() {
            *slot = surface_object;
        }
        *solution.start.param_mut(0)? = guess.x;
        *solution.start.param_mut(1)? = guess.y;
        solution.start.solution_value = curr.gap_length()?;
        // (a guess at a saddle of a minimize goes on from below it: `leave_saddle`)
        if curr.converged(k, EFF_ZERO / 1000.0)?
            && (!curr.is_singular()?.0 || curr.gap_length()? < tight)
            && !(op == SolverOperation::Minimize && curr.leave_saddle(k)?)
        {
            return Ok(LocalPointSolve {
                found: true,
                solution,
                reason: Converged,
                iterations: 0,
            });
        }
        let mut next = SurfPtSolveStepper::copy_of(k, &curr);
        let mut reason = StillIterating;
        let mut iterations = 0;
        while iterations < 35 {
            let step = next.find_next_step(k, &mut curr)?;
            reason = step.reason;
            // Not upstream (tangency-labs/solidcore#3446): a minimize converged to a saddle
            // goes on from below it (`leave_saddle`), the move counted as a step
            let saddle = |next: &mut SurfPtSolveStepper<'_>| -> Result<bool> {
                Ok(op == SolverOperation::Minimize && next.leave_saddle(k)?)
            };
            if next.converged_with(k, true)? {
                if saddle(&mut next)? {
                    curr.assign(&next);
                    iterations += 1;
                    curr.set_iter_count(iterations);
                    continue;
                }
                reason = Converged;
                break;
            }
            let (boundary, mut which) = next.converged_on_boundary(tight)?;
            if boundary {
                next.snap_to_boundaries(loose)?;
                if next.gap_length()? > loose {
                    let uv = next.uv();
                    let seam = self.is_on_seam(k, uv, None)?;
                    which = match which {
                        SurfParamType::VMin | SurfParamType::VMax => SurfParamType::V,
                        SurfParamType::UMin | SurfParamType::UMax => SurfParamType::U,
                        _ => which,
                    };
                    let mut new_uv = uv;
                    if seam.on_seam {
                        let index = if seam.seam_dir != SurfParamType::Both
                            && (seam.seam_dir == which || which == SurfParamType::Both)
                        {
                            Some(0)
                        } else if seam.seam_dir != SurfParamType::Both && seam.seam_dir != which {
                            None
                        } else if which != SurfParamType::Both {
                            Some(which as usize)
                        } else {
                            Some(2)
                        };
                        if let Some(index) = index {
                            new_uv = seam.cross_seam_uvs.get(index).copied().ok_or(
                                SolidError::invalid_input(
                                    "surface::surface_point_solve::p3::local_point_solve_1",
                                    "missing across-seam point",
                                ),
                            )?;
                        }
                    }
                    // SmVector2d::operator!= scales its tolerance from the left operand.
                    if !new_uv.approx_ne(uv) {
                        reason = OutOfBounds;
                        break;
                    }
                    next.set_uv(new_uv);
                } else {
                    reason = OutOfBounds;
                    break;
                }
            }

            if !step.failed && reason == Converged && saddle(&mut next)? {
                reason = StillIterating;
            }
            if step.failed || reason != StillIterating {
                break;
            }
            curr.assign(&next);
            curr.set_iter_count(iterations + 1);
            iterations += 1;
        }
        *solution.start.param_mut(0)? = next.uv().x;
        *solution.start.param_mut(1)? = next.uv().y;
        solution.start.solution_value =
            next.error_norm(if op == SolverOperation::Signed3dDirectedMaximize {
                ErrorNormType::SumAbs
            } else {
                ErrorNormType::Gap
            })?;
        let mut found = next.converged_with(k, false)?;
        if !found {
            if matches!(
                op,
                SolverOperation::Minimize
                    | SolverOperation::Maximize
                    | SolverOperation::Signed3dDirectedMaximize
            ) {
                found = true;
            }
            if iterations >= 35 && reason != OutOfBounds {
                reason = UnableToConverge;
            }
        }
        Ok(LocalPointSolve {
            found,
            solution,
            reason,
            iterations,
        })
    }
}
