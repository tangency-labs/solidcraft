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
//! the same 35 steps, and ends no farther than the nearest point such a move reached;
//! upstream reports the saddle as the minimum (`plan/upstream-issues/WR-23428--*`). Not upstream
//! (#23057): [`Surface::global_point_solve_1_answer`] also says why an empty answer is
//! empty ([`EmptyAnswer`]): proved, when the tree solve answered with none and every
//! leaf of its tree was excluded by its box's distance, or not.

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
    /// off a saddle (or a maximum) of the distance and returns whether it moved. The Newton
    /// step solves for a zero of the distance's gradient, so it converges to a saddle as
    /// readily as to a minimum (Nocedal and Wright, *Numerical Optimization*, 2nd ed., §3.4).
    /// Where the Hessian of `|S - P|² / 2`,
    /// `[[Su·Su + g·Suu, Su·Sv + g·Suv], [Su·Sv + g·Suv, Sv·Sv + g·Svv]]` with `g = S - P`,
    /// has a negative eigenvalue `λ`, the distance falls along its eigenvector `e`, a
    /// direction of negative curvature (Moré and Sorensen, *Math. Programming* 16 (1979)
    /// 1–20), unless the domain's boundary stops it (a point the boundary search converged
    /// to). The point moves to the least distance along the line `uv + s e`, in whichever
    /// direction is lower: a golden-section search over `s` inside the domain, its window
    /// ending where the line's tangent image `|Su e_u + Sv e_v| s` reaches twice the
    /// distance, and, as that search loses a fall close to the point where the distance
    /// rises again beyond it, a second search between the neighbours of the lowest of the
    /// points `0.618^k` of the window toward the point when that one is lower still. Where
    /// the Hessian is a multiple of the identity, `e` is the `u` direction. It moves only when that point is nearer by more than the solve's tight
    /// tolerance taken at the scale of the surface point too, `EFF_ZERO (1 + |P| + |S - P|)`
    /// (`|S| <= |P| + |S - P|`), so the rounding of the distances compared is no fall and a
    /// minimum never moves (a point on the axis of a cylinder of radius 1e5 has its nearest
    /// points on a whole circle, 1e5 away, whose distances differ by an ulp, 1.5e-11, where
    /// `EFF_ZERO (1 + |P|)` is 1e-12); and a direction is searched only where the
    /// quadratic model, a fall of `|λ| s² / 2` in `|S - P|² / 2` over the window, can give
    /// that much (so a degenerate minimum, on a surface of revolution's axis, costs no
    /// search). A distance whose evaluation fails is no candidate. The iso-direction tests
    /// of [`SurfPtGlobalSolver::is_local_max`] miss a saddle whose descent runs across the
    /// parameter lines: a coil's wall where the profile circle runs almost along the screw
    /// motion (its partials 3° apart), a saddle 0.0137 from a point on the wall's start
    /// station.
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
        let target = self.base.test_point();
        // the least fall a move must show
        let fall = tight.max(EFF_ZERO * (1.0 + target.max_dimension() + distance));
        let g = self.gap()?;
        let (su, sv) = (self.su()?, self.sv()?);
        let (suu, suv, svv) = (self.suu()?, self.suv()?, self.svv()?);
        let a = su.dot(su) + g.dot(suu);
        let b = su.dot(sv) + g.dot(suv);
        let c = sv.dot(sv) + g.dot(svv);
        // the smaller eigenvalue of [[a, b], [b, c]] and its eigenvector, the longer of the
        // two rows of the matrix less it, turned a quarter
        let low = 0.5 * (a + c) - (0.5 * (a - c)).hypot(b);
        if low.is_nan() || low >= 0.0 {
            return Ok(false);
        }
        let (e1, e2) = (Point2::new(b, low - a), Point2::new(low - c, b));
        let e = if e1.x.hypot(e1.y) >= e2.x.hypot(e2.y) {
            e1
        } else {
            e2
        };
        let len = e.x.hypot(e.y);
        if len.is_nan() {
            return Ok(false);
        }
        // both rows vanish only where the matrix is `low` times the identity (`b = 0`,
        // `a = c = low`), where every direction is an eigenvector
        let e = if len > 0.0 {
            Point2::new(e.x / len, e.y / len)
        } else {
            Point2::new(1.0, 0.0)
        };
        let reach = (su * e.x + sv * e.y).length();
        if reach.is_nan() || reach <= 0.0 {
            return Ok(false);
        }
        let domain = *self.base.domain();
        let uv = self.uv();
        let surface = self.base.surface();
        // the distance at a parameter of the line; a failed evaluation is no candidate
        let value = |at: Point2| -> Result<f64> {
            match surface.evaluate_point(k, at) {
                Ok(q) => Ok((q - target).length()),
                Err(error) if error.is_unported_or_cancelled() => Err(error),
                Err(_) => Ok(f64::INFINITY),
            }
        };
        let mut best: Option<(f64, Point2)> = None;
        for sign in [1.0, -1.0] {
            let d = Point2::new(sign * e.x, sign * e.y);
            // the line's last parameter in the domain, and in the window
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
            // the model's fall in |S - P|² / 2 over the window, against the least fall a
            // move needs there, (distance - (distance - fall))(2 distance) / 2
            if far.is_nan() || -low * far * far <= 2.0 * fall * distance {
                continue;
            }
            let at = |s: f64| domain.clamp_point(Point2::new(uv.x + s * d.x, uv.y + s * d.y));
            let ratio = 0.5 * (5.0_f64.sqrt() - 1.0);
            // golden-section search on [lo, hi], each step a fixed fraction of the bracket,
            // until the bracket is below the rounding of its ends: the lower of its last two
            // points
            let search = |mut lo: f64, mut hi: f64| -> Result<(f64, f64)> {
                let mut x1 = hi - ratio * (hi - lo);
                let mut x2 = lo + ratio * (hi - lo);
                let mut f1 = value(at(x1))?;
                let mut f2 = value(at(x2))?;
                for _ in 0..GOLDEN_STEPS {
                    if f1 <= f2 {
                        hi = x2;
                        (x2, f2) = (x1, f1);
                        x1 = hi - ratio * (hi - lo);
                        f1 = value(at(x1))?;
                    } else {
                        lo = x1;
                        (x1, f1) = (x2, f2);
                        x2 = lo + ratio * (hi - lo);
                        f2 = value(at(x2))?;
                    }
                }
                Ok(if f1 <= f2 { (x1, f1) } else { (x2, f2) })
            };
            let (mut s, mut f) = search(0.0, far)?;
            // the search keeps one part of its window at each step, so it loses a fall
            // close to the point where the distance rises again beyond it; the points
            // `far 0.618^k` toward the point, down to where the model's fall is below the
            // least fall, show such a fall, and one lower by the least fall than the
            // search's answer brackets a search between its neighbours
            let least = (2.0 * fall * distance / -low).sqrt();
            let mut lowest: Option<(f64, f64)> = None;
            let mut probe = far * ratio;
            for _ in 0..GOLDEN_STEPS {
                if probe.is_nan() || probe <= least {
                    break;
                }
                let at_probe = value(at(probe))?;
                if at_probe < f - fall && lowest.is_none_or(|(_, b)| at_probe < b) {
                    lowest = Some((probe, at_probe));
                }
                probe *= ratio;
            }
            if let Some((p, at_p)) = lowest {
                let (q, at_q) = search(p * ratio, (p / ratio).min(far))?;
                (s, f) = if at_q <= at_p { (q, at_q) } else { (p, at_p) };
            }
            if f < distance - fall && best.is_none_or(|(b, _)| f < b) {
                best = Some((f, at(s)));
            }
        }
        Ok(match best {
            Some((_, to)) => {
                self.set_uv(to);
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

/// Not upstream (#23057): why a global point solve answered with no solution, for a
/// caller that must tell "no surface point within the target" from a solve that found
/// nothing without proving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyAnswer {
    /// Any answer that is not proved empty: solutions, no candidate, or a tree solve
    /// that failed as upstream can (the global solver's drop stands unproved).
    Unproved,
    /// The global solver's minimum lay beyond the target distance by more than the
    /// tolerance, and the tree solve answered with none, every leaf of its tree
    /// excluded by its box's distance (none reached the leaf solve, whose one guess may
    /// find nothing): no point of the surface lies within the target.
    NoPointWithinTarget,
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
        Ok(self
            .global_point_solve_1_answer(
                k,
                domain,
                op,
                point,
                distance_tolerance,
                target_distance,
                requested,
                surface_object,
            )?
            .0)
    }

    /// [`Surface::global_point_solve_1`] with why an empty answer is empty (#23057):
    /// [`EmptyAnswer::NoPointWithinTarget`] only when the solver dropped its minimum
    /// beyond the target and the tree solve answered with none, every leaf excluded by
    /// its enclosure; [`EmptyAnswer::Unproved`] for any other answer (solutions, no
    /// candidate, a tree whose leaf solve ran, or a tree solve that failed as upstream
    /// can, whose drop stands).
    ///
    /// # Errors
    ///
    /// As [`Surface::global_point_solve_1`].
    pub(crate) fn global_point_solve_1_answer<O: SolutionObject>(
        &self,
        k: &Kernel,
        domain: &Extent2d,
        op: SolverOperation,
        point: Point3,
        distance_tolerance: f64,
        target_distance: Option<f64>,
        requested: SolutionRequested,
        surface_object: Option<O>,
    ) -> Result<(SolutionArray<O>, EmptyAnswer)> {
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
            return Ok((found, EmptyAnswer::Unproved));
        }
        Ok(
            match self.tree_solve_within_target(
                k,
                domain,
                op,
                point,
                distance_tolerance,
                target_distance,
                requested,
                surface_object,
            )? {
                Some((tree, true)) if tree.is_empty() => (tree, EmptyAnswer::NoPointWithinTarget),
                Some((tree, _)) => (tree, EmptyAnswer::Unproved),
                None => (found, EmptyAnswer::Unproved),
            },
        )
    }

    /// The tree solve [`Surface::global_point_solve_1`] answers with when the global
    /// solver drops a minimum beyond the target: `None` when it fails as upstream can,
    /// so the drop stands. Not upstream (#23057): with the answer, whether every leaf
    /// of the tree was excluded by its enclosure
    /// ([`Surface::global_point_solve_0_answer_in`]).
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
    ) -> Result<Option<(SolutionArray<O>, bool)>> {
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
            Some(object) => tree_on.global_point_solve_0_answer_in(
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
                .global_point_solve_0_answer_in(
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
                .map(|(t, excluded)| (t.without_objects(), excluded)),
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
        // Not upstream (tangency-labs/solidcore#3446): a minimize converged to a saddle goes on
        // from below it (`leave_saddle`), each move counted as a step, and the solve ends no
        // farther than the nearest point a move reached (the Newton steps after a move need
        // not lower the distance)
        let mut moved: Option<(f64, Point2)> = None;
        let mut leave = |at: &mut SurfPtSolveStepper<'_>| -> Result<bool> {
            if op != SolverOperation::Minimize || !at.leave_saddle(k)? {
                return Ok(false);
            }
            let reached = at.gap_length()?;
            if moved.is_none_or(|(nearest, _)| reached < nearest) {
                moved = Some((reached, at.uv()));
            }
            Ok(true)
        };
        if curr.converged(k, EFF_ZERO / 1000.0)?
            && (!curr.is_singular()?.0 || curr.gap_length()? < tight)
            && !leave(&mut curr)?
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
            if next.converged_with(k, true)? {
                if leave(&mut next)? {
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

            if !step.failed && reason == Converged && leave(&mut next)? {
                reason = StillIterating;
            }
            if step.failed || reason != StillIterating {
                break;
            }
            curr.assign(&next);
            curr.set_iter_count(iterations + 1);
            iterations += 1;
        }
        if let Some((nearest, at)) = moved {
            if next.gap_length()? > nearest {
                next.set_uv(at);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};

    /// tangency-labs/solidcore#3446: the saddle `z = 2 x y` over `[-1, 1]²` (an exact
    /// twisted bilinear patch, `S(u, v) = (2u - 1, 2v - 1, 2 (2u - 1)(2v - 1))`) and the
    /// point `P = (0, 0, 1)` above its centre. The centre is a stationary point of the
    /// distance, 1 away: the gradient's `g·Su` and `g·Sv` vanish there, and the Hessian of
    /// `|S - P|² / 2` is `[[4, -8], [-8, 4]]`, of eigenvalues 12 and -4. The nearest points
    /// are on the diagonal `x = y`, where `|S - P|² = 2x² + (2x² - 1)²` is least at
    /// `x² = 1/4`: `(±1/2, ±1/2, 1/2)`, `√3 / 2` away, at `(u, v) = (3/4, 3/4)` or
    /// `(1/4, 1/4)`. A minimize started at the centre stopped there (the shortcut for a
    /// guess already converged); it goes on to a nearest point. The same far from the origin
    /// (moved by `1e6` in each coordinate). The bounds are a thousand times the solve's tight
    /// tolerance, `EFF_ZERO (1 + |P|)`, which its convergence meets, and far below what tells
    /// the answers apart (the saddle's excess `1 - √3/2 = 0.134`, the parameters a quarter
    /// apart).
    #[test]
    fn a_minimize_started_at_a_saddle_goes_on_to_the_nearest_point() {
        let k = Kernel::default();
        for shift in [0.0, 1e6] {
            let at = |x: f64, y: f64, z: f64| Point3::new(x + shift, y + shift, z + shift);
            let s = Surface::from(
                BSplineSurface::create_bilinear_surface(
                    at(-1.0, -1.0, 2.0),
                    at(1.0, -1.0, -2.0),
                    at(-1.0, 1.0, -2.0),
                    at(1.0, 1.0, 2.0),
                )
                .unwrap(),
            );
            let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
            let r = s
                .local_point_solve_1::<()>(
                    &k,
                    &domain,
                    SolverOperation::Minimize,
                    at(0.0, 0.0, 1.0),
                    Point2::new(0.5, 0.5),
                    None,
                )
                .unwrap();
            let (u, v) = (
                r.solution.start.param(0).unwrap(),
                r.solution.start.param(1).unwrap(),
            );
            let d = r.solution.start.solution_value;
            let bound = 1000.0 * EFF_ZERO * (1.0 + at(0.0, 0.0, 1.0).max_dimension());
            assert!(r.found, "shift {shift}");
            assert!(
                (d - 0.75_f64.sqrt()).abs() < bound,
                "shift {shift}: {d} at ({u}, {v})"
            );
            assert!(
                (u - v).abs() < bound && ((u - 0.75).abs() < bound || (u - 0.25).abs() < bound),
                "shift {shift}: ({u}, {v})"
            );
        }
    }

    /// The minimize from `guess` to `point` on the polynomial patch of Bézier control net
    /// `net` (rows along `u`) and knots `u`, `v` over `domain`: the distance and parameters.
    fn minimize_on_patch(
        net: &[Vec<CPoint>],
        (u, v): (&[f64], &[f64]),
        domain: Extent2d,
        point: Point3,
        guess: Point2,
    ) -> (bool, f64, f64, f64) {
        let k = Kernel::default();
        let (n, m) = (net.len() - 1, net[0].len() - 1);
        let s = Surface::from(
            BSplineSurface::from_data(n, m, n, m, u.len() - 1, v.len() - 1, net, u, v).unwrap(),
        );
        let r = s
            .local_point_solve_1::<()>(&k, &domain, SolverOperation::Minimize, point, guess, None)
            .unwrap();
        (
            r.found,
            r.solution.start.solution_value,
            r.solution.start.param(0).unwrap(),
            r.solution.start.param(1).unwrap(),
        )
    }

    /// tangency-labs/solidcore#3446: a minimize started at a maximum of the distance whose
    /// Hessian is a multiple of the identity, where every direction is an eigenvector. The
    /// paraboloid `S(u, v) = (u, v, u² + v²)` over `[-1, 1]²` (Bézier net: `x` and `y` at
    /// -1, 0, 1, the `z` of each of `u²`, `v²` at 1, -1, 1), the point `P = (0, 0, 1)` and the
    /// guess `(0, 0)`: there `g = S - P = (0, 0, -1)`, the gradient vanishes and the Hessian
    /// of `|S - P|² / 2` is `[[1 - 2, 0], [0, 1 - 2]]`. With `w = u² + v²`, `|S - P|² =
    /// w + (w - 1)²` is 1 at the guess and least at `w = 1/2`, `√3 / 2` away. Bounds as in
    /// the saddle test above.
    #[test]
    fn a_minimize_started_at_a_round_maximum_goes_on_to_the_nearest_point() {
        let line = [-1.0, 0.0, 1.0];
        let square = [1.0, -1.0, 1.0];
        let net: Vec<Vec<CPoint>> = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| CPoint::new(line[i], line[j], square[i] + square[j], NOW))
                    .collect()
            })
            .collect();
        let knots = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        let point = Point3::new(0.0, 0.0, 1.0);
        let (found, d, u, v) = minimize_on_patch(
            &net,
            (&knots, &knots),
            Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0),
            point,
            Point2::new(0.0, 0.0),
        );
        let bound = 1000.0 * EFF_ZERO * (1.0 + point.max_dimension());
        assert!(found);
        assert!((d - 0.75_f64.sqrt()).abs() < bound, "{d} at ({u}, {v})");
        assert!((u * u + v * v - 0.5).abs() < bound, "({u}, {v})");
    }

    /// tangency-labs/solidcore#3446: a saddle whose distance falls along the descent
    /// direction only close to it and rises beyond, so a golden-section search over the
    /// whole window drops the fall. The patch `S(u, v) = (u, v, 150 u² - 100 u⁴)` over
    /// `[-2, 2] × [-1, 1]` (Bézier net along `u`: `x` at -2, -1, 0, 1, 2, `z` at -1000, 1600,
    /// -1800, 1600, -1000; linear along `v`), the point `P = (0, 0, 1)` and the guess
    /// `(0, 0)`: the Hessian of `|S - P|² / 2` there is `[[1 - 300, 0], [0, 1]]`, and the
    /// window along `u` is the whole `[0, 2]`, where the distance at 0.764 and 1.236 (the
    /// search's first two points) is over 5. The nearest points are on `v = 0`, where
    /// `|S - P|² = u² + (150 u² - 100 u⁴ - 1)²` is stationary when
    /// `1 + (150 w - 100 w² - 1)(300 - 400 w) = 0` (`w = u²`); its root below 0.01 (bisected
    /// to 50 digits) is `u = ±0.0816941811425684873`, 0.0817633827650059730 away (the
    /// other stationary points, near `|u| = 1.22`, are farther than 1). Bounds as in the
    /// saddle test above.
    #[test]
    fn a_saddle_left_finds_a_fall_close_to_it() {
        let x = [-2.0, -1.0, 0.0, 1.0, 2.0];
        let z = [-1000.0, 1600.0, -1800.0, 1600.0, -1000.0];
        let net: Vec<Vec<CPoint>> = (0..5)
            .map(|i| {
                [-1.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(x[i], y, z[i], NOW))
                    .collect()
            })
            .collect();
        let point = Point3::new(0.0, 0.0, 1.0);
        let (found, d, u, v) = minimize_on_patch(
            &net,
            (
                &[-2.0, -2.0, -2.0, -2.0, -2.0, 2.0, 2.0, 2.0, 2.0, 2.0],
                &[-1.0, -1.0, 1.0, 1.0],
            ),
            Extent2d::from_bounds(-2.0, -1.0, 2.0, 1.0),
            point,
            Point2::new(0.0, 0.0),
        );
        let bound = 1000.0 * EFF_ZERO * (1.0 + point.max_dimension());
        assert!(found);
        assert!(
            (d - 0.081_763_382_765_005_973).abs() < bound,
            "{d} at ({u}, {v})"
        );
        assert!(
            (u.abs() - 0.081_694_181_142_568_487).abs() < bound && v.abs() < bound,
            "({u}, {v})"
        );
    }
}
