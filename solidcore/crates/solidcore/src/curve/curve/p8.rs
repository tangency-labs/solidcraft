//! `SmCurve`, part 8: the global curve/curve intersector `GlobalCurveIntersect`
//! (analytic shortcuts, the leaf box intersection test, the coincidence check, the
//! curve/curve global solve and the heuristics that combine, split, drop and convert
//! the solutions it finds).
//!
//! Upstream: `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! | Rust                                                    | Upstream                        |
//! |---------------------------------------------------------|---------------------------------|
//! | [`Curve::global_curve_intersect`], [`CurveIntersectOptions`] | `SmCurve::GlobalCurveIntersect` |
//! | [`curves_meet_at_most_once`], [`touch_is_alone`]         | not upstream (WR-647)           |
//!
//! Deviations:
//! - **Out-params.** The solutions are returned as a [`SolutionArray`].
//! - **Objects.** Solutions name the caller's handles: [`CurveIntersectOptions::handles`]
//!   for this curve and the other curve (upstream `this`, `&crOtherCurve`; also the
//!   owner objects of their cache trees) and
//!   [`CurveIntersectOptions::analytic_handles`] for the analytic replacements the
//!   curve caches make of non-analytic curves (upstream the caches' `SmLine` /
//!   `SmCircle`). Where upstream casts a solution object back to its curve
//!   (`sm_RefineCCSolutions`, the spurious point check) the handle is resolved by
//!   equality, in the order this curve, other curve, this analytic, other analytic;
//!   a handle naming none of them is `SM_ERR_NULL_POINTER`, as upstream's `NER` on a
//!   failed cast.
//!   A caller that gives no handles at all gets solutions without objects; the solve
//!   itself then names stand-ins for the four curves (`operand_handles`) reporting
//!   their natural intervals, so `IdenticalSolutions` merges within 1/10000th of
//!   them and `sm_RefineCCSolutions` resolves each curve, as upstream.
//! - **Caches.** The caches are built for the call with
//!   [`make_curve_cache`](crate::curve::curve::p5::make_curve_cache) (see unit
//!   `curve.curve_p5`), or kept across calls in slots the caller lends
//!   (`global_curve_intersect_cached`, as upstream keeps them on the curves; the
//!   composite curve offsetter uses it for its offset pairs, and
//!   `global_curve_intersect_unnamed_cached` serves callers that name no handles,
//!   such as the boundary curves of `SmSurface::CoincidenceCheck`); the same curve (by
//!   identity, [`core::ptr::eq`]) shares one cache and so one tree, which makes the
//!   intersection a self intersection, as upstream's shared object cache. A cache that cannot be built is
//!   `SM_ERR_NULL_POINTER` (upstream `NER`).
//!   Callers holding topology may supply each curve owner's zone tolerance to the
//!   lazy build without changing intersection tolerance or copying geometry.
//! - **Leaf pair solve.** Upstream calls `LocalSolve` on the intersection test
//!   solver directly, which appends to the solution array the solver still points
//!   at; the port lends the array to the solver for that call.
//! - **Ignored statuses.** `sm_RefineCCSolutions`, `ClassifyIntersectingRangeSolution`,
//!   `SetMinMax` and the `EvaluatePoint`s of the solution combination and spurious
//!   point checks ignore their status upstream; so does the port (a failed
//!   evaluation leaves `Vec3::UNDEF`, a failed classification is not degenerate),
//!   except [`SolidError::Unported`], which is always propagated.
//! - **Debug only code.** The `SM_DEBUG_CODE` blocks, the assertions and the loop
//!   that only warns about consecutive point solutions within tolerance
//!   (`SM_DBG_WARN`, no output) are not ported. The obsolete `while (!bDone2)` loop
//!   around the range solution check runs exactly once upstream (nothing clears the
//!   flag) and is a single pass here.
//! - **Upstream quirks kept.** Trimming the intervals at a coincident range ending
//!   at the other curve's interval end sets this curve's interval
//!   (`sIvl1.SetMinMax(sIvl2.GetMin(), ...)`), and the second gap drop of the
//!   solution combination evaluates the other curve inside its trimmed interval
//!   (`sIvl2.Evaluate(0.38)`), as upstream. `bIsDegenerate` keeps its value from one
//!   range solution to the next when the classification is skipped.
//! - **Statuses passed on (WR-663).** The statuses SMLib drops are passed on, as the
//!   fork (tangency-labs/smlib 8311f8e):`GlobalCurveIntersect`'s refinement, interval
//!   trims, combine and cull evaluations and range classification, and the evaluations of
//!   its consistency check (which the port otherwise leaves out: it only warns).
//! - **Contacts at curve ends (WR-455).** After the global solve's refinement, a point
//!   solution whose contact runs on to an end of either curve's interval moves to that
//!   end (`sm_MoveContactsToCurveEnds`, the fork's tangency-labs/smlib@51eefa5), so a
//!   tangential contact is not placed inside the curve. Upstream leaves it inside. Not for
//!   a self intersection, where the stretch test would compare the curve with itself.
//!   A failed evaluation or solve in the move leaves the solution where it is. The
//!   stretch test is certified, not sampled as the fork's: the curves stay within
//!   tolerance along the parameter path between the two contacts by a Lipschitz bound
//!   from the curves' own data ([`curves_stay_within`], [`Curve::speed_bound`]); a
//!   stretch it cannot certify (a curve kind with no speed bound, at most 4096 halved
//!   pieces) keeps the solution where upstream puts it, and so does a stretch a coincident
//!   range solution overlaps (the range already reports that contact; the point beside it
//!   is left for the later passes to combine with it). After a move the solutions are sorted
//!   again on the first parameter, which the later passes assume; a move landing exactly on a
//!   contact another solution names is dropped instead (the contact is not reported
//!   twice).
//! - **Contacts seen through the tolerance (WR-647).** Before the range classification
//!   (and at the single leaf pair exit), a stretch of range solutions (ranges meeting or
//!   overlapping) one of whose ends lies inside both curves (its points farther than the
//!   tolerance from both ends of each curve's interval: there the curves part) is taken
//!   for the one contact inside it: the crossing when the curves change sides across it
//!   (the separations at its ends, taken across this curve, both past the scaled zero
//!   with a negative dot product), part at both of its ends and are certified to meet at
//!   most once over it ([`curves_meet_at_most_once`]: their cones of derivative
//!   directions part, over the stretches or, nearly parallel, over the pieces left
//!   near the meeting once the pieces whose hulls are apart are dropped, or one is a
//!   conic along which the other moves monotonically through the conic's quadratic;
//!   every rounding counted), found by [`local_curve_intersect`] from where the
//!   separation, taken as linear, vanishes (or from the nearest range end) inside the
//!   stretch and nearer than every range end; otherwise, when the stretch is all the
//!   solutions, the touch: the least distance over it ([`Curve::local_curve_solve`],
//!   `Minimize`, from the range end where the curves are nearest, or that end, or a
//!   minimum found from another range end nearer by more than the scaled zero), nearer
//!   than where they part, placed where the separation across the curves is least
//!   (`SignedDirectedMinimize`), where the curves MEET (within the scaled zero, evaluated),
//!   certified the stretch's only contact ([`touch_is_alone`]: a straight curve against
//!   one whose second derivative across it, bounded over the whole stretch from its
//!   Bernstein-B-spline coefficients, keeps one sign: its meetings with the line lie in
//!   one interval of rounding size, and its run within the scaled zero of the line, bounded
//!   by the same curvature and the curve's speed, is within the tolerance; or
//!   [`curves_meet_at_most_once`]),
//!   and for a curve
//!   against itself not on a fold. A touch the bounds do not certify (two curved curves
//!   tangent to each other, an exact or near overlap with a conic over an arc) keeps its
//!   ranges (a
//!   flat touch, the curves meeting where their separation is at its least and that
//!   least is zero, is a touch only when so certified). Curves within the tolerance of each other that
//!   never meet (an edge against a fitted copy of a part of it, leaving it on one side),
//!   and curves lying on each other over a part of the stretch, keep their ranges as
//!   upstream. Curves that come
//!   back within the tolerance elsewhere are one coincidence whose fit error passes the
//!   tolerance here and there (#1036), or several touches, kept as upstream; so is a
//!   stretch whose crossing the solver does not find. Upstream keeps such a stretch,
//!   which becomes a tiny edge or a false overlap (#1979). A coincidence of two curves
//!   lying on each other ends where one of them stops, so a fitted copy of a curve, whose
//!   tolerance-scale separation can change sides, stays a range (#1036). The fork decided
//!   this per span pair (tangency-labs/smlib@c2e3d9d, 04b42b4), where the two cannot be
//!   told apart. The certificates do bounded work: at most 16 control points of a curve's
//!   stretch for the cones, 4096 piece pairs a level over 24 levels in the subdivision,
//!   and 64 pieces of each in the conic slope test; past that, or for curve kinds they do
//!   not cover, the ranges are kept. A face/face section tangent to a floor arc at
//!   their shared end vertex is made one point by the tangency witness, from the
//!   surfaces (`crate::intersect::curve_class::witness`, #18064).
//! - **Flank points of an ellipse's touch (#18828).** After the spurious points are
//!   culled, consecutive point solutions of a line and an ellipse that is no circle are
//!   replaced by the line's one touch with the ellipse when the line lies in the ellipse's
//!   plane and provably does not cross it twice there
//!   (`Curve::flank_points_of_an_ellipse_touch`, `Ellipse::touch_between_line_flanks`):
//!   at a tangency the local solves stop on both sides of the double root, where the
//!   curves are within the solver's zero of each other, and upstream returns those points.
//!   A touch at the ellipse's seam, whose flank points lie at the two ends of the
//!   parameter range, is left as upstream.

use crate::curve::curve::p1::refine_cc_solutions;
use crate::curve::curve::p2::{at, CciGlobalSolver, Curve, CurveKind};
use crate::curve::curve::p3::local_curve_intersect;
use crate::curve::curve::p4::{analytic_curve_intersect, AnalyticOperand};
use crate::curve::curve::p5::{make_curve_cache_with_zone_tol, memoized_cache_top_box};
use crate::curve::curve_cache::CurveCache;
use crate::error::curve::CurveFailure;
use crate::error::{native_failure, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{
    Branch, GlobalSolve, SolutionRequested, SolveRequest, SolverOperation, MAX_SOLVER_TREES,
};
use crate::geom::solution_array::{
    OperandObject, Solution, SolutionArray, SolutionEnd, SolutionObject, SolutionType,
};
use crate::geom::tol::{are_parallel, are_same, scaled_zero2, ScaledZero};
use crate::geom::tree::Tree;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::{max2, min2, min4, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::{BSplineCurve, ProjectionType};
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{gamma, NOZ};
use crate::nurbs::nurbs_crv::{crv_accel_bound, crv_point_error, crv_speed_bound};

const OP_GCI: &str = "curve::curve::p8::Curve::global_curve_intersect";

/// `SM_BIG_ULONG`: no solution index yet.
const NO_INDEX: usize = usize::MAX;

/// The optional inputs of [`Curve::global_curve_intersect`].
#[derive(Debug, Clone, Copy)]
pub struct CurveIntersectOptions<O> {
    /// `bSkipGlobalCoincidenceCheck` (upstream default `FALSE`).
    pub skip_global_coincidence_check: bool,
    /// The handles of this curve and the other curve, named by the solutions. With
    /// these and [`CurveIntersectOptions::analytic_handles`] all `None` the solutions
    /// name no objects, but are merged and refined as upstream's (which always name
    /// the curves).
    pub handles: [Option<O>; 2],
    /// The handles of the analytic replacements the curve caches make of
    /// non-analytic curves (an `SmLine` of a straight B-spline), named by the
    /// solutions of their analytic intersection.
    pub analytic_handles: [Option<O>; 2],
}

impl<O> Default for CurveIntersectOptions<O> {
    fn default() -> Self {
        Self {
            skip_global_coincidence_check: false,
            handles: [None, None],
            analytic_handles: [None, None],
        }
    }
}

/// `SM_IS_BETWEEN(c, a, b)`: `c` strictly inside the open interval between `a` and
/// `b` shrunk by their scaled zero.
fn is_between(c: f64, a: f64, b: f64) -> bool {
    let z = scaled_zero2(&a, &b);
    ((a + z) < c && c < (b - z)) || ((b + z) < c && c < (a - z))
}

/// Solution `i` (a copy), `InvalidInput` past the size.
fn sol_at<O: SolutionObject>(sols: &SolutionArray<O>, i: usize) -> Result<Solution<O>> {
    sols.get_at(i)
}

/// Replaces solution `i`.
fn put_at<O: SolutionObject>(sols: &mut SolutionArray<O>, i: usize, s: &Solution<O>) -> Result<()> {
    sols.set_at(i, s)
}

/// The curves a solution object can name (see the module deviations).
struct Named<'a, O> {
    handles: [Option<O>; 2],
    curves: [&'a Curve; 2],
    analytic_handles: [Option<O>; 2],
    analytic: [Option<&'a Curve>; 2],
}

impl<'a, O: SolutionObject> Named<'a, O> {
    /// The curve named by `obj`, if any.
    fn curve_of(&self, obj: Option<O>) -> Option<&'a Curve> {
        if obj == self.handles[0] {
            return Some(self.curves[0]);
        }
        if obj == self.handles[1] {
            return Some(self.curves[1]);
        }
        for (h, c) in self.analytic_handles.iter().zip(self.analytic) {
            if let Some(c) = c {
                if obj == *h {
                    return Some(c);
                }
            }
        }
        None
    }
}

/// `sm_MoveContactsToCurveEnds` (the fork's tangency-labs/smlib@51eefa5, WR-455): a
/// point solution whose contact runs on to an end of either curve's interval is at that
/// end. It moves there when that end lies within `dist_tol` of the other
/// curve and the two curves are certified to stay within `dist_tol` of each other over
/// the whole stretch between the solution and the end ([`curves_stay_within`]; the fork
/// samples the stretch instead), and no range solution overlaps that stretch
/// (`crosses_a_range`). After a move the solutions are sorted on the first parameter;
/// one that would land exactly on a contact another solution names (a point or a range
/// end) is dropped instead, so the contact is not reported twice.
fn move_contacts_to_curve_ends<O: SolutionObject>(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    other: &Curve,
    other_interval: &Extent1d,
    dist_tol: f64,
    excluded_target_ends: [bool; 2],
    sols: &mut SolutionArray<O>,
) -> Result<()> {
    let mut moved_any = Vec::new();
    let mut named_twice = Vec::new();
    for ii in 0..sols.len() {
        let mut sol = sol_at(sols, ii)?;
        if sol.solution_type != SolutionType::SingleValue {
            continue;
        }
        let t1 = sol.start.parameters[0];
        let t2 = sol.start.parameters[1];
        // set once the solution has moved, or is dropped as a contact already named
        let mut moved = false;
        let mut twice = false;
        // which 0: an end of this curve, dropped to the other; 1: an end of the other,
        // dropped to this
        for which in 0..2 {
            if moved {
                break;
            }
            let (end_curve, drop_curve, end_ivl, drop_ivl) = if which == 0 {
                (curve, other, interval, other_interval)
            } else {
                (other, curve, other_interval, interval)
            };
            let (sol_on_end, sol_on_drop) = if which == 0 { (t1, t2) } else { (t2, t1) };
            for end in 0..2 {
                if which == 1 && excluded_target_ends.get(end).copied() == Some(true) {
                    continue;
                }
                if moved {
                    break;
                }
                let end_t = if end == 0 {
                    end_ivl.min()
                } else {
                    end_ivl.max()
                };
                if end_t == sol_on_end {
                    continue; // already there
                }
                // A failure on the way (an end past an unclamped curve's knots, a
                // failed solve) leaves the solution where upstream leaves it: this
                // pass only refines solutions already found, so it never fails the
                // intersection. Errors with no upstream status still propagate.
                let Some(end_pt) = native_failure(end_curve.evaluate_point(k, end_t))? else {
                    continue;
                };
                let Some(dropped) = native_failure(drop_curve.drop_point(
                    k,
                    drop_ivl,
                    end_pt,
                    None,
                    dist_tol,
                    Some(sol_on_drop),
                    SolverOperation::Minimize,
                ))?
                else {
                    continue;
                };
                if !dropped.success || dropped.distance.is_nan() || dropped.distance > dist_tol {
                    continue;
                }
                let drop_t = dropped.parameter;
                // Dropping a source end can also land on an excluded target end.
                // Reject the move before it can replace or remove the interior root.
                let target_t = if which == 0 { drop_t } else { end_t };
                if excluded_target_ends
                    .iter()
                    .zip([other_interval.min(), other_interval.max()])
                    .any(|(&excluded, end)| excluded && target_t == end)
                {
                    continue;
                }
                // A range solution overlapping the stretch (in either curve's parameter)
                // already reports that contact as a coincidence. Moving the point across
                // it onto the range's far end would drop the point there as named twice,
                // losing where the contact goes on past the range; the solution stays
                // where upstream puts it, for the later passes to combine with the range.
                // Point solutions on the stretch do not block: the stretch is certified
                // within tolerance, so they are the same contact and move to the same end.
                if crosses_a_range(
                    sols,
                    ii,
                    &named_twice,
                    which,
                    [sol_on_end, end_t],
                    [sol_on_drop, drop_t],
                ) {
                    continue;
                }
                // The curves within tolerance of one another over the whole stretch from
                // the solution to the end, certified along a parameter path between the
                // two contacts ([`curves_stay_within`]; a path monotone in
                // both parameters covers both stretches, so each point of either stretch
                // has a point of the other within tolerance). Not certified (a curve kind
                // without a speed bound, a point too far, the piece budget spent): the
                // solution stays where upstream puts it.
                k.work_counters.add_contact_stretch_certification();
                let Some(within) = native_failure(curves_stay_within(
                    k,
                    end_curve,
                    drop_curve,
                    [sol_on_end, sol_on_drop],
                    [end_t, drop_t],
                    dist_tol,
                ))?
                else {
                    continue;
                };
                if !within {
                    continue;
                }
                let at = if which == 0 {
                    [end_t, drop_t]
                } else {
                    [drop_t, end_t]
                };
                // A contact another solution already names (a point, or an end of a
                // range): the moved solution is that contact again and is dropped, so
                // the later passes do not report it twice (a short range turned into
                // points at its ends beside a point moved onto one of them).
                let named = sols.as_slice().iter().enumerate().any(|(jj, o)| {
                    jj != ii
                        && !named_twice.contains(&jj)
                        && (o.start.parameters[..2] == at
                            || (o.solution_type == SolutionType::RangeOfValues
                                && o.end.parameters[..2] == at))
                });
                if named {
                    moved = true;
                    twice = true;
                    continue;
                }
                sol.start.parameters[0] = at[0];
                sol.start.parameters[1] = at[1];
                sol.start.solution_value = dropped.distance;
                put_at(sols, ii, &sol)?;
                moved = true;
            }
        }
        if twice {
            named_twice.push(ii);
        } else if moved {
            moved_any.push(ii);
        }
    }
    // The later passes expect the solutions in first-parameter order (the global solver
    // adds them so, `SM_SK_BY_FIRST_PARAMETER`; phase 1 merges each with the last kept
    // one and `cull_spurious_points` compares neighbours). After a move the solutions
    // are sorted again on the first parameter (stable, so equal ones keep their order).
    // Solutions dropped as a contact named twice are taken out. Without a move nothing
    // is reordered: the solutions stay as refined, as upstream leaves them.
    if moved_any.is_empty() && named_twice.is_empty() {
        return Ok(());
    }
    let order = sorted_without(sols.as_slice(), &named_twice, |s| s.start.parameters[0]);
    for (i, s) in order.iter().enumerate() {
        put_at(sols, i, s)?;
    }
    sols.set_size(order.len());
    Ok(())
}

/// Whether a range solution other than `ii` (and those in `gone`) overlaps the stretch of a
/// move of solution `ii`: its parameter interval on the end curve (parameter index `which`)
/// meets the open stretch `end_stretch`, or its interval on the drop curve (the other index)
/// meets `drop_stretch`. The open stretch is shrunk at both ends by their scaled zero, as
/// upstream's `SM_IS_BETWEEN`, so a range only touching the stretch's start or end (the same
/// parameter to the kernel's zero) does not overlap it. A NaN parameter overlaps (the move is
/// refused).
fn crosses_a_range<O: SolutionObject>(
    sols: &SolutionArray<O>,
    ii: usize,
    gone: &[usize],
    which: usize,
    end_stretch: [f64; 2],
    drop_stretch: [f64; 2],
) -> bool {
    let end_idx = which.min(1);
    let drop_idx = 1 - end_idx;
    // `[a, b]` meets the open stretch shrunk by its ends' scaled zero; a NaN meets
    // (`f64::min` and `max` would drop it)
    let meets = |[s, e]: [f64; 2], a: f64, b: f64| {
        if [s, e, a, b].iter().any(|v| v.is_nan()) {
            return true;
        }
        let z = scaled_zero2(&s, &e);
        let (lo, hi) = (s.min(e) + z, s.max(e) - z);
        let (a, b) = (a.min(b), a.max(b));
        !(b <= lo || a >= hi)
    };
    sols.as_slice().iter().enumerate().any(|(jj, o)| {
        if jj == ii || gone.contains(&jj) || o.solution_type != SolutionType::RangeOfValues {
            return false;
        }
        [(end_idx, end_stretch), (drop_idx, drop_stretch)]
            .into_iter()
            .any(|(i, stretch)| {
                let (Some(&a), Some(&b)) = (o.start.parameters.get(i), o.end.parameters.get(i))
                else {
                    return true;
                };
                meets(stretch, a, b)
            })
    })
}

/// `all` without the entries at `gone`, sorted on `key` (stable: entries with equal keys
/// keep their order; `f64::total_cmp`, so the order is total).
fn sorted_without<T: Copy>(all: &[T], gone: &[usize], key: impl Fn(&T) -> f64) -> Vec<T> {
    let mut order: Vec<T> = all
        .iter()
        .enumerate()
        .filter(|(i, _)| !gone.contains(i))
        .map(|(_, s)| *s)
        .collect();
    order.sort_by(|a, b| key(a).total_cmp(&key(b)));
    order
}

/// Most path pieces [`curves_stay_within`] bounds before it gives up (no certificate).
const PATH_PIECES: usize = 4096;

/// A point of the parameter path of [`curves_stay_within`]: the path fraction, the two
/// parameters, and bounds above and below on the exact distance of the curves there.
#[derive(Clone, Copy)]
struct PathNode {
    l: f64,
    t: [f64; 2],
    above: f64,
    below: f64,
}

/// Not upstream: whether curves `a` and `b` stay within `tol` of each other along a
/// parameter path from `(a, b)` parameters `from` to `to` that is continuous and monotone
/// in both parameters (so it covers both stretches), certified (no sampled acceptance):
///
/// * the path is piecewise straight between nodes. It starts as one piece from `from` to
///   `to`; a piece is halved at the midpoint of its path fraction, the new node putting
///   `a` at that fraction of its stretch and `b` at the point of the piece's `b` stretch
///   nearest to it ([`Curve::drop_point`], clamped between the neighbours' parameters, the
///   straight path's parameter when the solve fails). Any `b` parameter between the
///   neighbours' keeps the path monotone, so the solve only steers which path is bounded;
///   every acceptance is the bounds below;
/// * the speeds of both curves are bounded over the stretches the path runs on
///   ([`Curve::speed_bound`], from the curves' own data);
/// * on a piece of the path between nodes `n0` and `n1`, the distance is at most
///   `(f0 + f1) / 2 + L / 2` with `f0`, `f1` bounds above at the nodes and `L = s_a
///   |Δt_a| + s_b |Δt_b|` its Lipschitz bound over the piece (a function of Lipschitz
///   constant `L` on a unit interval with end values `f0`, `f1` stays below that tent);
/// * when both curves are C1 over the piece's stretches, with acceleration bounds `A_a`,
///   `A_b` there ([`Curve::accel_bound`]), also at most `max(f0, f1) + M2 / 8` with
///   `M2 = A_a Δt_a^2 + A_b Δt_b^2` (second order): on the piece, `D(λ) = a(t_a(λ)) - b(t_b(λ))` for `λ` in
///   `[0, 1]` has `D' ` continuous and `|D''| = |a'' Δt_a^2 - b'' Δt_b^2| <= M2` wherever
///   it exists, so `E = D - L` with `L` the chord from `D(0)` to `D(1)` vanishes at both
///   ends and `E(λ) = -∫ G(λ, μ) E''(μ) dμ` with the Green's function `G >= 0`, `∫ G dμ =
///   λ (1 - λ) / 2`; hence `|E| <= M2 / 8` (the norm of an integral is at most the
///   integral of the norm) and `|D| <= |L| + |E| <= max(|D(0)|, |D(1)|) + M2 / 8` (a norm
///   is convex). The piece is accepted when either bound is within `tol`; the second
///   shrinks four times per halving where the first shrinks twice, so near-tolerance
///   stretches certify in few pieces;
/// * a node's bound above is its computed distance widened by `γ_6` (the distance's own
///   roundings) plus twice the two points' coordinate error bounds
///   ([`Curve::point_error_bound`]; a norm is within `√3 < 2` times a coordinate's);
/// * pieces are halved (an explicit stack, left half first) until either bound, widened
///   by `γ_4`, is within `tol`. The nodes are evaluated at rounded parameters (clamped to
///   the stretches, `b`'s to its neighbours'); `a`'s rounding keeps them monotone along
///   the path, so the pieces between actual nodes still form a path monotone in both
///   parameters, from `from` to `to` exactly.
///
/// `Ok(false)`, meaning not certified, when a curve kind has no speed bound, a number is
/// not finite, a node is certainly farther than `tol` (its bound below exceeds it), or
/// [`PATH_PIECES`] pieces were halved without a certificate. A NaN never certifies (every
/// acceptance is a `<=` comparison).
///
/// # Errors
///
/// Those of the curves' point evaluations, and those of the nearest-point solves that
/// carry no upstream status.
pub fn curves_stay_within(
    k: &Kernel,
    a: &Curve,
    b: &Curve,
    from: [f64; 2],
    to: [f64; 2],
    tol: f64,
) -> Result<bool> {
    let [s0, s1] = from;
    let [e0, e1] = to;
    if !([s0, s1, e0, e1, tol].iter().all(|v| v.is_finite()) && tol >= 0.0) {
        return Ok(false);
    }
    let ivl = [
        Extent1d::from_min_max(s0.min(e0), s0.max(e0)),
        Extent1d::from_min_max(s1.min(e1), s1.max(e1)),
    ];
    let (Some(speed_a), Some(speed_b)) = (a.speed_bound(&ivl[0]), b.speed_bound(&ivl[1])) else {
        return Ok(false);
    };
    let along = |l: f64, s: f64, e: f64, i: &Extent1d| {
        if l >= 1.0 {
            e
        } else {
            (s + l * (e - s)).max(i.min()).min(i.max())
        }
    };
    let node = |l: f64, t: [f64; 2]| -> Result<Option<PathNode>> {
        let pa = a.evaluate_point(k, t[0])?;
        let pb = b.evaluate_point(k, t[1])?;
        let (Some(ea), Some(eb)) = (a.point_error_bound(t[0]), b.point_error_bound(t[1])) else {
            return Ok(None);
        };
        let d = pa.distance(pb);
        let err = 2.0 * (ea + eb);
        let above = d * (1.0 + gamma(6)) + err;
        let below = d * (1.0 - gamma(6)) - err;
        Ok((above.is_finite() && below.is_finite()).then_some(PathNode { l, t, above, below }))
    };
    let (Some(first), Some(last)) = (node(0.0, [s0, s1])?, node(1.0, [e0, e1])?) else {
        return Ok(false);
    };
    let mut stack = vec![(first, last)];
    let mut pieces = 0_usize;
    while let Some((n0, n1)) = stack.pop() {
        if n0.below > tol || n1.below > tol {
            return Ok(false);
        }
        let lip = (speed_a * (n1.t[0] - n0.t[0]).abs() + speed_b * (n1.t[1] - n0.t[1]).abs())
            * (1.0 + gamma(4));
        let bound = (0.5 * (n0.above + n1.above) + 0.5 * lip) * (1.0 + gamma(4));
        if bound <= tol {
            continue;
        }
        // second order, when both curves are C1 over the piece's own stretches (a piece
        // holding a knot where a curve is only C0 keeps the first-order bound; halving
        // shrinks such pieces around the knot)
        let span = |x: f64, y: f64| Extent1d::from_min_max(x.min(y), x.max(y));
        let [ta0, tb0] = n0.t;
        let [ta1, tb1] = n1.t;
        let second = a
            .accel_bound(&span(ta0, ta1))
            .zip(b.accel_bound(&span(tb0, tb1)))
            .map(|(acc_a, acc_b)| {
                // the squares, products and sum round five times; the max and sum twice more
                let (da, db) = (ta1 - ta0, tb1 - tb0);
                let m2 = (acc_a * da * da + acc_b * db * db) * (1.0 + gamma(8));
                (n0.above.max(n1.above) + 0.125 * m2) * (1.0 + gamma(4))
            });
        if second.is_some_and(|s| s <= tol) {
            continue;
        }
        pieces += 1;
        let mid = 0.5 * (n0.l + n1.l);
        if pieces > PATH_PIECES || !(n0.l < mid && mid < n1.l) {
            return Ok(false);
        }
        // the new node: `a` at the path fraction, `b` at the point of its piece stretch
        // nearest to it (any parameter between the neighbours' keeps the path monotone,
        // so the choice only steers the bounds; a failed solve takes the straight path's)
        let ta = along(mid, s0, e0, &ivl[0]);
        let piece_b = span(tb0, tb1);
        let straight = along(mid, s1, e1, &ivl[1])
            .max(piece_b.min())
            .min(piece_b.max());
        let tb = match native_failure(b.drop_point(
            k,
            &piece_b,
            a.evaluate_point(k, ta)?,
            None,
            tol,
            Some(straight),
            SolverOperation::Minimize,
        ))? {
            Some(d) if d.success && d.parameter.is_finite() => {
                d.parameter.max(piece_b.min()).min(piece_b.max())
            }
            _ => straight,
        };
        let Some(nm) = node(mid, [ta, tb])? else {
            return Ok(false);
        };
        stack.push((nm, n1));
        stack.push((n0, nm));
    }
    Ok(true)
}

/// The B-spline speed bound of [`Curve::speed_bound`]: `None` when evaluation past the
/// natural interval is enabled and `interval` leaves it (the curve is not clamped there).
fn b_spline_speed_bound(c: &BSplineCurve, interval: &Extent1d) -> Option<f64> {
    let cur = c.nurb_ref().ok()?;
    if c.is_out_of_bounds_enabled() {
        let (lo, hi) = cur.param_bounds();
        if !(lo <= interval.min() && interval.max() <= hi) {
            return None;
        }
    }
    crv_speed_bound(cur, interval)
}

impl Curve {
    /// Not upstream: an upper bound on the speed `|c'(t)|` of [`Curve::evaluate_point`]
    /// for every `t` in `interval`, from the curve's own data (no sampling): the control
    /// points of a B-spline and of the kinds evaluated through one (ellipse, circle,
    /// hyperbola, parabola; [`crv_speed_bound`](crate::nurbs::nurbs_crv)), the power basis
    /// of a Hermite curve, the affine map of a line. `None`, meaning no certified bound,
    /// for the other kinds and where those bounds give none.
    #[must_use]
    pub fn speed_bound(&self, interval: &Extent1d) -> Option<f64> {
        match self.kind() {
            CurveKind::BSpline(c) => b_spline_speed_bound(c, interval),
            CurveKind::Ellipse(c) => b_spline_speed_bound(c.base(), interval),
            CurveKind::Hyperbola(c) => b_spline_speed_bound(c.base(), interval),
            CurveKind::Parabola(c) => b_spline_speed_bound(c.base(), interval),
            CurveKind::Hermite(c) => c.speed_bound(interval),
            CurveKind::Line(c) => c.speed_bound(),
            _ => None,
        }
    }

    /// Not upstream: an upper bound on the acceleration `|c''(t)|` of
    /// [`Curve::evaluate_point`] wherever it exists in `interval`, for a curve whose
    /// derivative is continuous over `interval` (no sampling): the control points of a
    /// B-spline and of the kinds evaluated through one, polynomial or rational
    /// ([`crv_accel_bound`](crate::nurbs::nurbs_crv)), the power basis of a Hermite
    /// curve, and `0` for a line (affine in its parameter, see [`Line`]'s speed bound).
    /// `None`, meaning no certified bound (in particular a curve that is not C1 over
    /// `interval`), for the other kinds and where those bounds give none.
    #[must_use]
    pub fn accel_bound(&self, interval: &Extent1d) -> Option<f64> {
        let spline = |c: &BSplineCurve| crv_accel_bound(c.nurb_ref().ok()?, interval);
        match self.kind() {
            CurveKind::BSpline(c) => spline(c),
            CurveKind::Ellipse(c) => spline(c.base()),
            CurveKind::Hyperbola(c) => spline(c.base()),
            CurveKind::Parabola(c) => spline(c.base()),
            CurveKind::Hermite(c) => c.accel_bound(interval),
            CurveKind::Line(_) => Some(0.0),
            _ => None,
        }
    }

    /// Not upstream: a bound on the error of each coordinate of the point
    /// [`Curve::evaluate_point`] returns at `t`, for the kinds [`Curve::speed_bound`]
    /// bounds; `None` for the others.
    #[must_use]
    pub fn point_error_bound(&self, t: f64) -> Option<f64> {
        let spline = |c: &BSplineCurve| crv_point_error(c.nurb_ref().ok()?);
        match self.kind() {
            CurveKind::BSpline(c) => spline(c),
            CurveKind::Ellipse(c) => spline(c.base()),
            CurveKind::Hyperbola(c) => spline(c.base()),
            CurveKind::Parabola(c) => spline(c.base()),
            CurveKind::Hermite(c) => c.point_error(t),
            CurveKind::Line(c) => c.point_error(t),
            _ => None,
        }
    }
}

/// Runs `sm_RefineCCSolutions` on `sols` (status ignored upstream; passed on as the
/// fork, tangency-labs/smlib 8311f8e).
fn refine<O: SolutionObject>(
    k: &Kernel,
    sols: &mut SolutionArray<O>,
    named: &Named<'_, O>,
) -> Result<()> {
    refine_cc_solutions(k, sols, |o| named.curve_of(o))
}

/// The analytic curve of a cache (`GetAnalyticCurve`), copied out of the cache.
fn cache_analytic<O: SolutionObject>(
    k: &Kernel,
    cache: &mut CurveCache<O>,
) -> Result<Option<Curve>> {
    Ok(cache.analytic_curve(k)?.cloned())
}

/// The top box of the tree of the cache [`build_cache`] would return for `curve` at
/// `zone_tol_3d`, read from the curve's memo when it holds that build (not upstream,
/// #14453); `None` when it does not, or the tree has no top node. A memo hit copies the
/// tree with every box as it is ([`crate::geom::tree::Tree::retag_curve_tree`]), so this
/// is the box the tree of that cache has.
fn memoized_top_box(k: &Kernel, curve: &Curve, zone_tol_3d: Option<f64>) -> Option<Extent3d> {
    memoized_cache_top_box(k, curve, zone_tol_3d.or_else(|| curve.owner_zone_tol_3d()))
}

/// Why a cache in this module's slots has a tree with a top node: the slots are crate
/// private and filled only by [`build_cache`], whose build sets the tree, with its top
/// node, whenever it succeeds.
const BUILT_CACHE_HAS_TREE: &str = "the curve's cache, filled only by build_cache, has a \
     tree with its top node: the build sets both whenever it succeeds";

/// The kernel's defect for a built cache without its tree or top node, with the `NER`
/// status upstream reports.
fn built_cache_defect() -> SolidError {
    Failure::new(
        OP_GCI,
        CurveFailure::Defect {
            why: BUILT_CACHE_HAS_TREE,
        },
    )
    .with_status(SM_ERR_NULL_POINTER)
    .into_error()
}

/// The cache tree (`GetTree`, `NER`).
fn cache_tree<O: SolutionObject>(cache: &CurveCache<O>) -> Result<&Tree<O>> {
    cache.tree().ok_or_else(built_cache_defect)
}

/// `GetOrCreateObjectCache` for a curve (`NER` on failure). `zone_tol_3d` is the zone
/// tolerance the caller resolved for the curve's owner: an edge curve's cache
/// tessellates at half that tolerance ([`make_curve_cache_with_zone_tol`]), as
/// upstream's `SmTol::GetApproxTol3d(&m_crCurve)` reads it from the curve's edge
/// (`SmCurveCache.cpp:1496`). `None` preserves the curve's recorded owner tolerance,
/// falling back to the system tolerance when the curve has none.
fn build_cache<O: SolutionObject>(
    k: &Kernel,
    curve: &Curve,
    handle: Option<O>,
    zone_tol_3d: Option<f64>,
) -> Result<CurveCache<O>> {
    match make_curve_cache_with_zone_tol::<O>(
        k,
        curve,
        handle,
        zone_tol_3d.or_else(|| curve.owner_zone_tol_3d()),
    ) {
        Ok(c) => Ok(c),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(e) => Err(Failure::new(OP_GCI, CurveFailure::CacheNotBuilt)
            .caused_by(e)
            .into_error()),
    }
}

/// The stand-in objects of a curve/curve solve whose caller names none: this curve,
/// the other curve (the same object when `other` is this curve, as upstream's
/// pointers) and their caches' analytic replacements (reporting the natural
/// interval of the curve they replace).
pub(crate) fn operand_handles(this: &Curve, other: &Curve) -> [OperandObject; 4] {
    let this_ivl = this.natural_interval();
    let other_ivl = other.natural_interval();
    if core::ptr::eq(this, other) {
        [
            OperandObject::Curve(0, this_ivl),
            OperandObject::Curve(0, this_ivl),
            OperandObject::Curve(2, this_ivl),
            OperandObject::Curve(2, this_ivl),
        ]
    } else {
        [
            OperandObject::Curve(0, this_ivl),
            OperandObject::Curve(1, other_ivl),
            OperandObject::Curve(2, this_ivl),
            OperandObject::Curve(3, other_ivl),
        ]
    }
}

impl Curve {
    /// `GlobalCurveIntersect`: the intersection points (`start.parameters`: this
    /// curve's, then the other curve's parameter) and coincident ranges of this
    /// curve over `interval` with `other` over `other_interval` within `dist_tol`.
    /// Two analytic curves (or curves whose caches have analytic replacements) are
    /// intersected analytically when that settles the answer; curves with disjoint
    /// cache boxes have none. A single leaf pair whose intersection is far from
    /// tangency is solved alone. Otherwise a coincidence covering a whole curve is
    /// the answer, coincident ends trim the intervals, and the curve/curve global
    /// solver finds the intersections, which are then combined (touching or
    /// overlapping ranges, ranges whose gap stays within tolerance, close points),
    /// culled (short ranges next to point solutions, points next to long ranges,
    /// worse points converging to better ones) and classified (short or tangent
    /// ranges become points). The same curve object as `other` is a self
    /// intersection.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when a cache cannot be built or a solution names an
    /// unknown handle; those of the analytic intersections, the global solves, the
    /// coincidence check, the drops, the gap measure and the evaluations upstream
    /// checks; [`SolidError::Unported`] from unported callees.
    pub fn global_curve_intersect<O: SolutionObject>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        dist_tol: f64,
        opts: &CurveIntersectOptions<O>,
    ) -> Result<SolutionArray<O>> {
        // no handles: upstream's solutions still name this curve and the other one
        // (whose natural intervals IdenticalSolutions reads, and Refine resolves)
        if opts.handles == [None, None] && opts.analytic_handles == [None, None] {
            let own = operand_handles(self, other);
            let inner = CurveIntersectOptions {
                skip_global_coincidence_check: opts.skip_global_coincidence_check,
                handles: [Some(own[0]), Some(own[1])],
                analytic_handles: [Some(own[2]), Some(own[3])],
            };
            return Ok(self
                .global_curve_intersect(k, interval, other, other_interval, dist_tol, &inner)?
                .without_objects());
        }
        self.global_curve_intersect_body(
            k,
            interval,
            other,
            other_interval,
            dist_tol,
            opts,
            &mut None,
            &mut None,
            [None, None],
            0.0,
            true,
            [false; 2],
        )
    }

    /// [`Curve::global_curve_intersect`] keeping the curves' caches in `cache1` (this
    /// curve's) and `cache2` (the other curve's; unused when `other` is this curve)
    /// across calls, as upstream keeps them on the curves (`GetOrCreateObjectCache`):
    /// a cache is built where the intersection first needs it (with the same errors)
    /// and reused by later calls given the same slot. A slot must belong to its curve,
    /// unchanged since the cache was built, and to the same handle in `opts.handles`
    /// (the cache names it in its solutions); the build is deterministic and the
    /// intersection only adds the analytic replacement a fresh cache would make, so a
    /// reused cache gives the same solutions as a fresh one. Without handles the slots
    /// are not used.
    ///
    /// # Errors
    ///
    /// As [`Curve::global_curve_intersect`].
    pub(crate) fn global_curve_intersect_cached<O: SolutionObject>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        dist_tol: f64,
        opts: &CurveIntersectOptions<O>,
        cache1: &mut Option<CurveCache<O>>,
        cache2: &mut Option<CurveCache<O>>,
    ) -> Result<SolutionArray<O>> {
        self.global_curve_intersect_cached_with_zone_tols(
            k,
            interval,
            other,
            other_interval,
            dist_tol,
            opts,
            cache1,
            cache2,
            [None, None],
            0.0,
            [false; 2],
        )
    }

    /// [`Curve::global_curve_intersect_cached`] with the zone tolerances the caller
    /// resolved for the curves' owners. Upstream builds each curve's cache on the curve
    /// object itself, so an edge curve tessellates at half the edge's tolerance
    /// (`SmTol::GetApproxTol3d(&m_crCurve)`, `SmCurveCache.cpp:1496`); a caller whose
    /// handle geometry is not that owned object supplies the tolerance here. A slot is
    /// used exactly as in [`Curve::global_curve_intersect_cached`]: nothing is built
    /// before the body reaches it, and a `None` tolerance builds as that method does.
    /// A populated slot must also match its supplied owner tolerance. These hints
    /// govern the two intersection-tree slots only; nested coincidence and drop
    /// routines retain their existing curve-owner context. Coplanar circles, or a circle
    /// and a line in its plane, overlapping past touching by less than `contact_tol` touch
    /// at one point (WR-787, #15429, #14599; [`analytic_curve_intersect`]); zero keeps
    /// upstream's crossings.
    ///
    /// # Errors
    ///
    /// As [`Curve::global_curve_intersect`].
    pub(crate) fn global_curve_intersect_cached_with_zone_tols<O: SolutionObject>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        dist_tol: f64,
        opts: &CurveIntersectOptions<O>,
        cache1: &mut Option<CurveCache<O>>,
        cache2: &mut Option<CurveCache<O>>,
        zone_tols: [Option<f64>; 2],
        contact_tol: f64,
        excluded_target_ends: [bool; 2],
    ) -> Result<SolutionArray<O>> {
        if opts.handles == [None, None] && opts.analytic_handles == [None, None] {
            let own = operand_handles(self, other);
            let inner = CurveIntersectOptions {
                skip_global_coincidence_check: opts.skip_global_coincidence_check,
                handles: [Some(own[0]), Some(own[1])],
                analytic_handles: [Some(own[2]), Some(own[3])],
            };
            return Ok(self
                .global_curve_intersect_cached_with_zone_tols(
                    k,
                    interval,
                    other,
                    other_interval,
                    dist_tol,
                    &inner,
                    &mut None,
                    &mut None,
                    zone_tols,
                    contact_tol,
                    excluded_target_ends,
                )?
                .without_objects());
        }
        self.global_curve_intersect_body(
            k,
            interval,
            other,
            other_interval,
            dist_tol,
            opts,
            cache1,
            cache2,
            zone_tols,
            contact_tol,
            false,
            excluded_target_ends,
        )
    }

    /// [`Curve::global_curve_intersect`] for a caller that names no handles, keeping
    /// the curves' caches in `cache1` (this curve's) and `cache2` (the other curve's;
    /// unused when `other` is this curve) across calls, as
    /// [`Curve::global_curve_intersect_cached`] does. The caches name the stand-in
    /// handles of the unnamed solve (`operand_handles`), so a slot must belong to its
    /// curve, unchanged since the cache was built, and keep its role: `cache1` always
    /// holds a cache of a `self` curve and `cache2` one of an `other` curve.
    ///
    /// # Errors
    ///
    /// As [`Curve::global_curve_intersect`].
    pub(crate) fn global_curve_intersect_unnamed_cached<O: SolutionObject>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        dist_tol: f64,
        skip_global_coincidence_check: bool,
        cache1: &mut Option<CurveCache<OperandObject>>,
        cache2: &mut Option<CurveCache<OperandObject>>,
    ) -> Result<SolutionArray<O>> {
        let own = operand_handles(self, other);
        let inner = CurveIntersectOptions {
            skip_global_coincidence_check,
            handles: [Some(own[0]), Some(own[1])],
            analytic_handles: [Some(own[2]), Some(own[3])],
        };
        Ok(self
            .global_curve_intersect_body(
                k,
                interval,
                other,
                other_interval,
                dist_tol,
                &inner,
                cache1,
                cache2,
                [None, None],
                0.0,
                false,
                [false; 2],
            )?
            .without_objects())
    }

    /// The body of [`Curve::global_curve_intersect`] for a caller giving handles, with
    /// the cache slots of [`Curve::global_curve_intersect_cached`]. `zone_tols` are the
    /// zone tolerances the caller resolved for the curves' owners, used only where a
    /// slot's cache is built ([`build_cache`]); `None` preserves the corresponding
    /// curve's recorded owner tolerance. `own_slots`: true only when the slots are the
    /// callee's locals, dropped on return; the top-box return (#14453) skips filling
    /// them. A caller's kept slots (`false`) are filled as before, wherever the
    /// intersection needs the caches.
    fn global_curve_intersect_body<O: SolutionObject>(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        dist_tol: f64,
        opts: &CurveIntersectOptions<O>,
        cache1_slot: &mut Option<CurveCache<O>>,
        cache2_slot: &mut Option<CurveCache<O>>,
        zone_tols: [Option<f64>; 2],
        contact_tol: f64,
        own_slots: bool,
        excluded_target_ends: [bool; 2],
    ) -> Result<SolutionArray<O>> {
        let [h_this, h_other] = opts.handles;
        let same_curve = core::ptr::eq(self, other);

        // init outputs
        let mut sols: SolutionArray<O> = SolutionArray::new();

        // locals
        let mut needs_more_intersections = true;

        let base_named = Named {
            handles: opts.handles,
            curves: [self, other],
            analytic_handles: opts.analytic_handles,
            analytic: [None, None],
        };

        // special case: Line/Line, Line/Ellipse, Ellipse/Ellipse intersections
        if self.is_analytic() && other.is_analytic() {
            // get Line/Line intersection
            let r = analytic_curve_intersect(
                k,
                AnalyticOperand::Curve(self),
                interval,
                AnalyticOperand::Curve(other),
                other_interval,
                dist_tol,
                contact_tol,
                h_this,
                h_other,
            )?;
            sols.reset();
            for s in &r.solutions {
                sols.add(s);
            }
            needs_more_intersections = r.needs_more_intersections;

            // Refine Range solutions.
            refine(k, &mut sols, &base_named)?;

            // exit when Line/Line solutions are complete
            if !needs_more_intersections {
                return Ok(sols);
            }
        }

        // Not upstream (#14453): two curves whose memoised caches' top boxes are apart
        // have no intersection, the same test as below on the same boxes, read from the
        // memo without copying the caches into this call's own slots (a boolean asks
        // this for every pair of proto edges, nearly all apart); a caller's kept slots
        // are filled as before
        if own_slots && !same_curve && cache1_slot.is_none() && cache2_slot.is_none() {
            if let (Some(root1), Some(root2)) = (
                memoized_top_box(k, self, zone_tols[0]),
                memoized_top_box(k, other, zone_tols[1]),
            ) {
                let mut bbox = root1;
                bbox.expand_absolute(dist_tol);
                if bbox.are_disjoint(&root2, 0.0) {
                    return Ok(sols);
                }
            }
        }

        // get target curves' caches (kept in the caller's slots, see
        // `global_curve_intersect_cached`), each at its owner's zone tolerance when the
        // caller resolved one
        let cache1 = match cache1_slot {
            Some(c) => c,
            slot @ None => slot.insert(build_cache(k, self, h_this, zone_tols[0])?),
        };
        let mut cache2_own = if same_curve {
            None
        } else {
            Some(match cache2_slot {
                Some(c) => c,
                slot @ None => slot.insert(build_cache(k, other, h_other, zone_tols[1])?),
            })
        };

        // get curve spatial decomposition trees from each cache
        {
            let tree1 = cache_tree(cache1)?;
            let tree2 = cache_tree(cache2_own.as_deref().unwrap_or(cache1))?;

            // no work - topNode bounding boxes are disjoint
            let root1 = tree1.node(tree1.top_node().ok_or_else(built_cache_defect)?)?;
            let root2 = tree2.node(tree2.top_node().ok_or_else(built_cache_defect)?)?;
            let mut bbox = root1.bbox;
            bbox.expand_absolute(dist_tol);
            if bbox.are_disjoint(&root2.bbox, 0.0) {
                return Ok(sols);
            }
        }

        // special case: curves converted to Analytic/Analytic intersection by
        // SmCurve::CreateAnalyticCurve(). Note, GetAnalyticCurve() can be expensive,
        // so call it only as many times as necessary.
        let analy1 = cache_analytic(k, cache1)?;
        let mut analy2: Option<Curve> = None;
        if let Some(a1) = analy1.as_ref() {
            // (the same curve shares its cache, so its analytic curve is the same
            // object: a self intersection, which the line intersector skips)
            if let Some(c2) = cache2_own.as_deref_mut() {
                analy2 = cache_analytic(k, c2)?;
            }
            let a2_ref = if same_curve {
                Some(a1)
            } else {
                analy2.as_ref()
            };
            if let Some(a2) = a2_ref {
                // (the cache's analytic curve is the curve itself when it is analytic)
                let a1_handle = if self.is_analytic() {
                    h_this
                } else {
                    opts.analytic_handles[0]
                };
                let a2_handle = if other.is_analytic() {
                    h_other
                } else {
                    opts.analytic_handles[1]
                };

                // execute analytic/analytic intersection
                let r = analytic_curve_intersect(
                    k,
                    AnalyticOperand::Curve(a1),
                    interval,
                    AnalyticOperand::Curve(a2),
                    other_interval,
                    dist_tol,
                    contact_tol,
                    a1_handle,
                    a2_handle,
                )?;
                sols.reset();
                for s in &r.solutions {
                    sols.add(s);
                }
                needs_more_intersections = r.needs_more_intersections;

                // exit when analytic/analytic intersection is complete
                if !needs_more_intersections {
                    return Ok(sols);
                }
            }
        }

        if !needs_more_intersections {
            return Ok(sols);
        }

        let named = Named {
            analytic: [
                if self.is_analytic() {
                    None
                } else {
                    analy1.as_ref()
                },
                if other.is_analytic() {
                    None
                } else if same_curve {
                    analy1.as_ref()
                } else {
                    analy2.as_ref()
                },
            ],
            ..base_named
        };

        let tree1 = cache_tree(cache1)?;
        let tree2 = cache_tree(cache2_own.as_deref().unwrap_or(cache1))?;

        // arrive here after line/line and analytic/analytic special cases were skipped
        // execute general curve/curve intersection solver

        // Set up a global solver to find all the leafNode/leafNode possible
        // intersection cases (see SmCCIGlobalSolver::LocalSolve,
        // SM_SO_INTERSECTION_TEST).
        let mut gs_test = CciGlobalSolver::new(
            tree1,
            tree2,
            *interval,
            *other_interval,
            false,
            ProjectionType::Parallel,
        )
        .with_curves(self, other, k);
        gs_test.solve_it(
            &SolveRequest {
                operation: SolverOperation::IntersectionTest,
                solution_requested: SolutionRequested::All,
                tolerance_3d: dist_tol,
                best_answer_so_far_sq: BIG_DOUBLE,
                opt_vectors: &[],
            },
            &mut sols,
        )?;

        // exit condition: no leafNode/leafNode possible solutions
        if sols.is_empty() {
            return Ok(sols);
        }

        // when leafNode/leafNode possible solutions were found
        {
            let sol = sol_at(&sols, 0)?;

            // check for easy cases: No Solution or 1 Solution far from tangency
            // note: sol.start[6] = number of intersecting leafNode pairs
            if sol.start.param(6)? < 1.00001 {
                // only a single node found
                gs_test
                    .base
                    .set_solver_operation(SolverOperation::Intersect);
                let mut branch: Branch = [None; MAX_SOLVER_TREES];

                // load branch with single leaf/leaf intersection SmTreeNodes
                branch[0] = sol.nodes[0];
                branch[1] = sol.nodes[1];
                sols.reset();

                // find LeafNode/LeafNode intersection (into the solver's array)
                std::mem::swap(&mut gs_test.base.solutions, &mut sols);
                let solved = gs_test.local_solve(&branch);
                std::mem::swap(&mut gs_test.base.solutions, &mut sols);
                solved?;
                let mut finished = true;

                // when just one leafNode/leafNode possible solution was found
                if sols.len() == 1 {
                    let mut sol2 = sol_at(&sols, 0)?;
                    sol2.num_objects = 2;
                    sol2.objects[0] = h_this;
                    sol2.objects[1] = h_other;
                    put_at(&mut sols, 0, &sol2)?;

                    // when solution value is only close to dist_tol and not extremely tight
                    if sol2.solution_type == SolutionType::SingleValue
                        && sol2.start.solution_value > dist_tol / 1000.0
                    {
                        // get the curve/curve intersection point positions and tangents
                        let pv1 = self.evaluate(k, sol2.start.parameters[0], 1, true, true)?;
                        let pv2 = other.evaluate(k, sol2.start.parameters[1], 1, true, true)?;
                        let t1 = at(&pv1, 1);
                        let t2 = at(&pv2, 1);

                        // get angle between curve tangents
                        // If this or other are degenerate, the angle fails
                        let ang_degree = match t1.angle_between(t2) {
                            Ok(rad) => rad * 180.0 / PI,
                            Err(_) => 90.0,
                        };

                        // when the tangents are within 20 degrees of one another
                        // (possibly coincident)
                        if ang_degree < 20.0 || ang_degree > 160.0 {
                            // clear the Finished bit
                            finished = false;
                        }
                    }
                } else if sols.len() > 1 {
                    // else several solutions: clear the Finished bit
                    finished = false;
                }

                // exit criteria - No Solution or 1 intersection Solution far from tangency
                if finished {
                    // Not upstream (WR-647): the one leaf pair's range is the whole stretch
                    // where the curves are within tolerance (no other leaf boxes meet).
                    self.contacts_seen_through_tolerance(
                        k,
                        &mut sols,
                        other,
                        interval,
                        other_interval,
                        dist_tol,
                        same_curve,
                    )?;
                    return Ok(sols);
                }
            }

            // Arrive here if the IntersectionTest found more than one solution, or
            // just one solution that is within 20 degree of tangent. Prepare for the
            // general Intersection.
            sols.reset();
        }

        // See if all of one of the curves is coincident with the other
        if !same_curve {
            // don't look for global coincidence when looking for self-intersecting curves
            let mut needs_more_work = true; // GlobalCoincidenceChecker() might set it to False.
            if !opts.skip_global_coincidence_check {
                // execute search for coincidence
                let r = self.global_coincidence_checker(
                    k,
                    interval,
                    other,
                    other_interval,
                    dist_tol,
                    false,
                    opts.handles,
                )?;
                needs_more_work = r.additional_work_needed;
                sols.reset();
                for s in &r.solutions {
                    sols.add(s);
                }
            }

            if !needs_more_work {
                self.drop_ranges_of_tangencies(k, &mut sols, other, interval, other_interval)?;
                return Ok(sols);
            }
        }

        // If we found solutions in the coincidence checker - we can decrease the size
        // of the intervals being checked to those portions of the curve without
        // coincidence.

        // create intervals to be trimmed to coincident region
        let mut ivl1 = *interval;
        let mut ivl2 = *other_interval;
        let mut done = false;

        // for every coincident solution
        for ii in 0..sols.len() {
            let sol = sol_at(&sols, ii)?;
            if sol.solution_type != SolutionType::RangeOfValues {
                continue;
            }
            let (s0, s1) = (sol.start.parameters[0], sol.start.parameters[1]);
            let (e0, e1) = (sol.end.parameters[0], sol.end.parameters[1]);

            // when solution starts at ivl1.min and stops short of ivl1.max
            if are_same(ivl1.min(), s0) && e0 < ivl1.max() {
                // trim ivl1 to remove solution range (status ignored upstream)
                ivl1.set_min_max(e0, ivl1.max())?;

                // for zero length regions - curve is entirely coincident - no more work
                if ivl1.length().abs() < EFF_ZERO {
                    done = true;
                    break;
                }
            }

            // when solution ends at ivl1.max and starts after ivl1.min
            if are_same(ivl1.max(), e0) && ivl1.min() < s0 {
                // trim ivl1 to remove solution range (status ignored upstream)
                ivl1.set_min_max(ivl1.min(), s0)?;

                // for zero length regions - curve is entirely coincident - no more work
                if ivl1.length().abs() < EFF_ZERO {
                    done = true;
                    break;
                }
            }

            // let ivl_tmp = solution range, so that we know which is min/max.
            let mut ivl_tmp = Extent1d::from_value(s1);
            ivl_tmp.add_value(e1);

            // when solution starts at ivl2.min and stops short of ivl2.max
            if are_same(ivl2.min(), ivl_tmp.min()) && ivl_tmp.max() < ivl2.max() {
                // trim ivl2 to remove solution range (status ignored upstream)
                ivl2.set_min_max(ivl_tmp.max(), ivl2.max())?;

                // for zero length regions - curve is entirely coincident - no more work
                if ivl2.length().abs() < EFF_ZERO {
                    done = true;
                    break;
                }
            }

            // when solution ends at ivl2.max and starts after ivl2.min
            if are_same(ivl2.max(), ivl_tmp.max()) && ivl2.min() < ivl_tmp.min() {
                // trim (upstream: sIvl1) to remove solution range (status ignored
                // upstream)
                ivl1.set_min_max(ivl2.min(), ivl_tmp.min())?;

                // for zero length regions - curve is entirely coincident - no more work
                if ivl2.length().abs() < EFF_ZERO {
                    done = true;
                    break;
                }
            }
        }

        // exit condition - a coincidence was found that includes all of one curve or
        // the other
        if done {
            return Ok(sols);
        }

        // arrive here - need to do a general curve/curve intersection

        // Remove the GlobalCoincidenceChecker point solutions with a gap bigger than
        // SM_EFF_ZERO: the global solver finds the actual intersections next.
        for ii in (1..=sols.len()).rev() {
            let sol = sol_at(&sols, ii - 1)?;
            if sol.solution_type == SolutionType::SingleValue && sol.start.solution_value > EFF_ZERO
            {
                sols.remove_at(ii - 1, 1);
            }
        }

        // construct global solver with the curve trees
        let mut gs =
            CciGlobalSolver::new(tree1, tree2, ivl1, ivl2, false, ProjectionType::Parallel)
                .with_curves(self, other, k);

        // traverse curve and surface sub-division trees
        gs.solve_it(
            &SolveRequest {
                operation: SolverOperation::Intersect,
                solution_requested: SolutionRequested::All,
                tolerance_3d: dist_tol,
                best_answer_so_far_sq: BIG_DOUBLE,
                opt_vectors: &[],
            },
            &mut sols,
        )?;

        // Refine Range solutions.
        refine(k, &mut sols, &named)?;
        // Not upstream (#18064): a range with exact ends on the flank of a tangency is no
        // coincidence. Dropped before the contacts move to the curve ends, as a range
        // there would stop the flank's point solutions moving to the touch past it.
        self.drop_ranges_of_tangencies(k, &mut sols, other, interval, other_interval)?;
        let self_intersect = core::ptr::eq(tree1, tree2);
        // Not for a self intersection: there the "other" curve is this curve, so the
        // stretch test compares the curve with itself and always passes, and the move
        // would carry a real self intersection (a closed curve's seam) onto the trivial
        // diagonal, where it is dropped below.
        if !self_intersect {
            move_contacts_to_curve_ends(
                k,
                self,
                interval,
                other,
                other_interval,
                dist_tol,
                excluded_target_ends,
                &mut sols,
            )?;
        }

        // Now sort through the intersections looking for cases to combine, split, or
        // convert from 'coincident maybe' intervals to point intersections.

        if self_intersect {
            // Remove all self intersections from beginning of list
            while let Some(curr) = sols.get(0) {
                if curr.start.parameters[0] > curr.start.parameters[1]
                    || (curr.start.parameters[0] - curr.start.parameters[1]).abs() < EFF_ZERO
                {
                    sols.remove_at(0, 1);
                } else {
                    break;
                }
            }
        }

        // phase 1: merge coincident intervals and point solutions close enough to be
        // the same point, compressing the array in place; keep a last interval index
        // and a last point index.
        let mut last_interval_index: usize;
        let mut last_point_index: usize;
        if sols
            .get(0)
            .is_some_and(|s| s.solution_type == SolutionType::RangeOfValues)
        {
            last_interval_index = 0;
            last_point_index = NO_INDEX;
        } else {
            last_interval_index = NO_INDEX;
            last_point_index = 0;
        }

        // We'll use these for checking when solutions are across a seam:
        let this_whole_domain = self.natural_interval();
        let other_whole_domain = other.natural_interval();

        // for every solution after the first - see if it can be combined with the
        // last processed solution of the same type
        let mut ii = 1;
        while ii < sols.len() {
            let curr = sol_at(&sols, ii)?;
            let curr_start = curr.start;
            let curr_is_range = curr.solution_type == SolutionType::RangeOfValues;

            // get index of last solution of same type as curr
            let last_index = if curr_is_range {
                last_interval_index
            } else {
                last_point_index
            };

            // get last solution
            let last_slot = if last_index == NO_INDEX {
                0
            } else {
                last_index
            };
            let mut last = sol_at(&sols, last_slot)?;
            let last_end = if last.solution_type == SolutionType::RangeOfValues {
                last.end
            } else {
                last.start
            };

            // For self intersection we have duplicates; eliminate those.
            if self_intersect
                && (curr.start.parameters[0] > curr.start.parameters[1]
                    || (curr.start.parameters[0] - curr.start.parameters[1]).abs() < EFF_ZERO)
            {
                ii += 1;
                continue;
            }

            let do_combine = self.decide_combine(
                k,
                other,
                interval,
                other_interval,
                &ivl2,
                dist_tol,
                &this_whole_domain,
                &other_whole_domain,
                self_intersect,
                last_index == NO_INDEX,
                &last,
                &last_end,
                &curr,
            )?;

            // when pair of solutions are marked for combination
            if do_combine {
                // Expand last interval to cover curr interval
                if curr_is_range {
                    if last.end.parameters[0] < curr.end.parameters[0] {
                        last.end = curr.end;
                        put_at(&mut sols, last_slot, &last)?;
                    }
                } else if curr_start.solution_value < last_end.solution_value {
                    // working with point solutions: keep the better solution
                    put_at(&mut sols, last_slot, &curr)?;
                }
                // else: take no action - which throws away curr
            } else {
                // Keep the current solution but squeeze it down into the array to
                // keep the array packed
                let next = if last_interval_index == NO_INDEX {
                    last_point_index.wrapping_add(1)
                } else if last_point_index == NO_INDEX {
                    last_interval_index.wrapping_add(1)
                } else {
                    last_interval_index.max(last_point_index).wrapping_add(1)
                };
                if curr_is_range {
                    last_interval_index = next;
                } else {
                    last_point_index = next;
                }
                if next != ii {
                    put_at(&mut sols, next, &curr)?;
                }
            }
            ii += 1;
        }

        // Set the array size after all combining has been done.
        if !sols.is_empty() {
            let last_index = if last_interval_index == NO_INDEX {
                last_point_index
            } else if last_point_index == NO_INDEX {
                last_interval_index
            } else {
                last_interval_index.max(last_point_index)
            };
            sols.set_size(last_index.wrapping_add(1));
        }

        // Phase 2: remove coincident segments which include or are adjacent to point
        // intersections (nearly tangent intersecting curves often lie within a
        // coincident segment: keep the point, discard the segment).
        Self::cull_ranges_near_points(&mut sols, tree1, tree2, interval)?;

        // For SMLib intersection consistency upstream evaluates each pair of
        // consecutive point solutions (not across a seam) and only warns when they lie
        // within tolerance; the port leaves that out. The fork passes on the statuses
        // of those evaluations (tangency-labs/smlib 8311f8e), so they are made and a
        // failure is returned.
        self.consistency_evaluations(k, &sols, other)?;

        // Check adjacent point solutions: a 'precise' solution with a not-so-good
        // solution very close may be an artifact of the local intersector; when the
        // gap steadily decreases from the bad solution to the tight one, delete the
        // bad one. [B298] (Assumes the solutions are sorted on the first parameter.)
        Self::cull_spurious_points(
            k,
            &mut sols,
            &named,
            &this_whole_domain,
            &other_whole_domain,
        )?;

        // Not upstream (#18828): two point solutions on the flanks of a line's touch with an
        // ellipse are that one touch.
        self.flank_points_of_an_ellipse_touch(
            k,
            &mut sols,
            other,
            interval,
            other_interval,
            dist_tol,
            contact_tol,
        )?;

        // Not upstream (WR-647): a merged range that ends where the curves part is their
        // crossing or touch seen through the tolerance.
        self.contacts_seen_through_tolerance(
            k,
            &mut sols,
            other,
            interval,
            other_interval,
            dist_tol,
            same_curve,
        )?;

        // check each coincident interval solution: very short or diverging intervals
        // become point solutions.
        self.classify_ranges(k, &mut sols, other, interval, other_interval)?;

        Ok(sols)
    }

    /// Whether `curr` combines with `last` (the last kept solution of its type,
    /// ending at `last_end`) in [`Curve::global_curve_intersect`]: never across the
    /// ends of a closed curve; touching ends combine (with exceptions for ranges);
    /// close points combine unless across a seam; overlapping ranges combine; ranges
    /// separated by an opening combine when the curves stay within tolerance of each
    /// other through it.
    fn decide_combine<O: SolutionObject>(
        &self,
        k: &Kernel,
        other: &Self,
        interval: &Extent1d,
        other_interval: &Extent1d,
        ivl2: &Extent1d,
        dist_tol: f64,
        this_whole_domain: &Extent1d,
        other_whole_domain: &Extent1d,
        self_intersect: bool,
        no_previous: bool,
        last: &Solution<O>,
        last_end: &SolutionEnd,
        curr: &Solution<O>,
    ) -> Result<bool> {
        let curr_start = &curr.start;
        let le0 = last_end.parameters[0];
        let le1 = last_end.parameters[1];
        let cs0 = curr_start.parameters[0];
        let cs1 = curr_start.parameters[1];

        // no previous solution of same type - keep solution
        if no_previous {
            return Ok(false);
        }

        // handle start/end closed OtherCurve case
        if (le0 - cs0).abs() < EFF_ZERO_SQRT
            && (le1 - cs1).abs() * 10.0 > (other_interval.max() - other_interval.min())
        {
            // don't combine because OtherCurve is closed
            return Ok(false);
        }

        // handle start/end closed ThisCurve case
        if (le1 - cs1).abs() < EFF_ZERO_SQRT
            && (le0 - cs0).abs() * 10.0 > (interval.max() - interval.min())
        {
            // don't combine because ThisCurve is closed
            return Ok(false);
        }

        // When Solution endPoints touch on either curve - combine solutions.
        if are_same(le0, cs0) || are_same(le1, cs1) {
            let mut do_combine = true;

            // for interval solutions - make some exceptions
            if curr.solution_type == SolutionType::RangeOfValues {
                let start_dev1 = last.start.solution_value;
                let end_dev1 = last.end.solution_value;

                // don't combine when end of last is much better solution than start
                if start_dev1 > EFF_ZERO && start_dev1 > end_dev1 * 100000.0 {
                    do_combine = false;
                }
                let start_dev2 = curr.start.solution_value;
                let end_dev2 = curr.end.solution_value;

                // don't combine when start of curr is much better solution than end
                if end_dev2 > EFF_ZERO && end_dev2 > start_dev2 * 100000.0 {
                    do_combine = false;
                }

                // If the smaller segment is less than 1/1000 of the other then just
                // combine them anyhow.
                let last_leng = max2(EFF_ZERO, last.end.parameters[0] - last.start.parameters[0]);
                let curr_leng = max2(EFF_ZERO, curr.end.parameters[0] - curr.start.parameters[0]);

                if last_leng > curr_leng * 1000.00 || curr_leng > last_leng * 1000.00 {
                    do_combine = true;
                }
            }
            return Ok(do_combine);
        }

        // for point solutions only
        if curr.solution_type == SolutionType::SingleValue {
            // evaluate solution points (statuses ignored upstream; passed on as the
            // fork, tangency-labs/smlib 8311f8e)
            let pt1 = self.evaluate_point(k, cs0)?;
            let pt2 = self.evaluate_point(k, le0)?;
            let dist2 = pt1.distance_squared(pt2);

            // combine solutions when points are close enough together
            if dist2 < dist_tol * dist_tol {
                // Same position, check whether they're at opposite ends of the domain
                // (seam). [B88]
                let mut across_seam = false;
                if (cs0 - le0).abs() > 0.9 * this_whole_domain.length() {
                    across_seam = true;
                }
                if (cs1 - le1).abs() > 0.9 * other_whole_domain.length() {
                    across_seam = true;
                }
                if !across_seam {
                    return Ok(true);
                }
            }
        }

        // for range solutions only
        if curr.solution_type == SolutionType::RangeOfValues {
            // When SolutionIntervals overlap on both curves - combine solutions
            if le0 >= cs0 {
                return Ok(is_between(
                    cs1,
                    last.start.parameters[1],
                    last.end.parameters[1],
                ));
            }

            // arrive here when Solution Intervals are separated by an opening; see if
            // the curves are coincident throughout the solution opening

            // get SolutionOpening = curve parameter intervals from last end to curr start
            let mut interval1 = Extent1d::new();
            interval1.add_value(le0);
            interval1.add_value(cs0);
            let mut interval2 = Extent1d::new();
            interval2.add_value(le1);
            interval2.add_value(cs1);

            // Evaluate and Drop a Curve1 SolutionOpening MidPoint to Curve2
            let pt_on1 = self.evaluate_point(k, interval1.evaluate(0.45))?;
            let guess = interval2.evaluate(0.5);
            let dropped = other.drop_point(
                k,
                &interval2,
                pt_on1,
                None,
                dist_tol,
                Some(guess),
                SolverOperation::Minimize,
            )?;

            // don't combine solutions when SolutionGap MidPoint does not drop
            if !dropped.success {
                return Ok(false);
            }

            if self_intersect {
                // with self intersection the orientation must be opposite
                let pv1 = self.evaluate(k, interval1.evaluate(0.45), 1, true, true)?;
                let pv2 = other.evaluate(k, dropped.parameter, 1, true, true)?;
                let t1 = at(&pv1, 1);
                let t2 = at(&pv2, 1);
                if t1.dot(t2) > 0.0 {
                    return Ok(false); // same orientation can not be self intersection
                }
            }

            // Evaluate and drop a Curve2 SolutionOpening nearMidPoint to Curve1
            // (upstream evaluates the trimmed interval sIvl2)
            let pt_on2 = other.evaluate_point(k, ivl2.evaluate(0.38))?;
            let guess = interval1.evaluate(0.5);
            let dropped = self.drop_point(
                k,
                &interval1,
                pt_on2,
                None,
                dist_tol,
                Some(guess),
                SolverOperation::Minimize,
            )?;

            // don't combine solutions when nearMidPoint fails to drop
            if !dropped.success {
                return Ok(false);
            }

            // If SolutionOpening Interval is less than 1000th of curve intervals
            // assume the curves are coincident between solutions and combine
            if interval1.length() < interval.length() / 1000.0
                && interval2.length() < other_interval.length() / 1000.0
            {
                return Ok(true);
            }

            // look for curve/curve distance at any maxima point within the opening
            let max_degree = self.degree()?.max(other.degree()?);
            let gap = self.curve_max_distance_between(
                k,
                &interval1,
                other,
                le1,
                cs1,
                max_degree as u64 + 3,
                Some(dist_tol),
            )?;

            // when curve/curve gap distance is less than tolerance at maxima
            if gap.distance < dist_tol {
                return Ok(true);
            }
        }

        // If made it to here then we don't combine
        Ok(false)
    }

    /// Phase 2 of [`Curve::global_curve_intersect`]: marks every range solution next
    /// to or containing a point solution (by the point's subdivision spans) for
    /// removal when short (else marks the point when it is inside the range, or close
    /// to an end and twice worse), then compresses the array, turning culled ranges
    /// with exact ends into point solutions.
    fn cull_ranges_near_points<O: SolutionObject>(
        sols: &mut SolutionArray<O>,
        tree1: &Tree<O>,
        tree2: &Tree<O>,
        interval: &Extent1d,
    ) -> Result<()> {
        let num_sols = sols.len();
        let mut cull = vec![false; num_sols];
        let mut compress = false;

        // search for interval solutions
        for ii in 0..num_sols {
            let inv_sol = sol_at(sols, ii)?;

            // skip point-solutions solutions
            if inv_sol.solution_type != SolutionType::RangeOfValues {
                continue;
            }

            let inv_start = inv_sol.start;
            let inv_end = inv_sol.end;
            let mut inv_span1 = Extent1d::new();
            inv_span1.add_value(inv_start.parameters[0]);
            inv_span1.add_value(inv_end.parameters[0]);
            let mut inv_span2 = Extent1d::new();
            inv_span2.add_value(inv_start.parameters[1]);
            inv_span2.add_value(inv_end.parameters[1]);

            // search for point solutions within or adjacent to this interval solution
            for jj in 0..num_sols {
                if ii == jj {
                    continue;
                }

                let pt_sol = sol_at(sols, jj)?;

                // skip interval solutions
                if pt_sol.solution_type != SolutionType::SingleValue {
                    continue;
                }

                let pt_start = pt_sol.start;

                // Get the cache subdivision intervals containing this point if
                // available
                let pt_ivl1 = match pt_sol.nodes[0] {
                    Some(n) => tree1.curve_aux(tree1.node(n)?)?.interval,
                    None => Extent1d::from_min_max(pt_start.parameters[0], pt_start.parameters[0]),
                };
                let pt_ivl2 = match pt_sol.nodes[1] {
                    Some(n) => tree2.curve_aux(tree2.node(n)?)?.interval,
                    None => Extent1d::from_min_max(pt_start.parameters[1], pt_start.parameters[1]),
                };

                // check for overlap between Interval SolSpan and Point SubDivisionSpan
                // (a disjoint intersection is Init())
                let xsect1 = pt_ivl1
                    .intersect(&inv_span1)
                    .unwrap_or_else(|_| Extent1d::new());
                let xsect2 = pt_ivl2
                    .intersect(&inv_span2)
                    .unwrap_or_else(|_| Extent1d::new());

                if xsect1.is_init() || xsect2.is_init() {
                    continue;
                }

                // Now InvSpan is adjacent to or contains solution or its sub-division
                // end-points. Delete the range interval only if it's 'short': 1/1000
                // of the whole domain.
                let close_param = interval.length() / 1000.0;

                if inv_span1.length() < close_param {
                    // Delete the range interval.
                    if let Some(c) = cull.get_mut(ii) {
                        *c = true;
                    }
                    compress = true;
                } else {
                    // Range is long enough. See about deleting the point solution.
                    let pt_param1 = pt_sol.start.parameters[0];
                    let delete_point = if pt_param1 < inv_span1.min() {
                        let dist = inv_span1.min() - pt_param1;
                        dist < close_param
                            && pt_sol.start.solution_value > 2.0 * inv_start.solution_value
                    } else if pt_param1 > inv_span1.max() {
                        let dist = pt_param1 - inv_span1.max();
                        dist < close_param
                            && pt_sol.start.solution_value > 2.0 * inv_end.solution_value
                    } else {
                        // Point sol is inside the range sol: delete it.
                        true
                    };
                    if delete_point {
                        if let Some(c) = cull.get_mut(jj) {
                            *c = true;
                        }
                        compress = true;
                    }
                }

                if cull.get(ii).copied().unwrap_or(false) {
                    break;
                }

                // quit searching when point solutions get past interval solutions
                if pt_start.parameters[0] > inv_span1.max()
                    || pt_start.parameters[1] > inv_span2.max()
                {
                    break;
                }
            }
        }

        // compress the solutions array to remove all unwanted interval solutions;
        // culled ranges with very accurate ends become point solutions.
        if compress {
            let mut next_index = 0;
            let mut ii = 0;
            while ii < sols.len() {
                // omit solutions to drop
                if cull.get(ii).copied().unwrap_or(false) {
                    let mut sol = sol_at(sols, ii)?;
                    // except when endPoints are good enough to keep
                    if sol.solution_type == SolutionType::RangeOfValues {
                        let start_dev = sol.start.solution_value;
                        let end_dev = sol.end.solution_value;

                        if start_dev < EFF_ZERO && end_dev < EFF_ZERO {
                            // change range solution into 2 point solutions
                            let mut first = sol;
                            first.solution_type = SolutionType::SingleValue;
                            sol.solution_type = SolutionType::SingleValue;
                            sol.start = sol.end;
                            put_at(sols, ii, &sol)?;
                            sols.insert_at(ii + 1, &first, 1);
                            if let Some(c) = cull.get_mut(ii) {
                                *c = false;
                            }
                            let at = (ii + 1).min(cull.len());
                            cull.insert(at, false);
                        } else if start_dev < EFF_ZERO {
                            // change range solution into point solution
                            sol.solution_type = SolutionType::SingleValue;
                            put_at(sols, ii, &sol)?;
                            if let Some(c) = cull.get_mut(ii) {
                                *c = false;
                            }
                        } else if end_dev < EFF_ZERO {
                            // change range solution into point solution
                            sol.solution_type = SolutionType::SingleValue;
                            sol.start = sol.end;
                            put_at(sols, ii, &sol)?;
                            if let Some(c) = cull.get_mut(ii) {
                                *c = false;
                            }
                        } else {
                            // all done with this solution - omit it
                            ii += 1;
                            continue;
                        }
                    } else {
                        // all done with this solution - omit it
                        ii += 1;
                        continue;
                    }
                }

                // move current solution to next open array spot
                if ii != next_index {
                    let s = sol_at(sols, ii)?;
                    put_at(sols, next_index, &s)?;
                }
                next_index += 1;
                ii += 1;
            }

            // Set the array size after discarding unwanted interval solutions.
            if !sols.is_empty() {
                sols.set_size(next_index);
            }
        }
        Ok(())
    }

    /// The evaluations of the consistency check of [`Curve::global_curve_intersect`]
    /// (consecutive single-value solutions not across a seam of either curve), whose
    /// statuses the fork passes on (`SER`, tangency-labs/smlib 8311f8e); the check
    /// itself only warns upstream.
    fn consistency_evaluations<O: SolutionObject>(
        &self,
        k: &Kernel,
        sols: &SolutionArray<O>,
        other: &Self,
    ) -> Result<()> {
        let same_to_scaled_zero =
            |a: f64, b: f64, scale: f64| crate::geom::tol::in_tol(a - b, scale.scaled_zero());
        for i1 in 1..sols.len() {
            let sol0 = sol_at(sols, i1 - 1)?;
            let sol1 = sol_at(sols, i1)?;
            // compare single value solutions
            if sol0.solution_type != SolutionType::SingleValue
                || sol1.solution_type != SolutionType::SingleValue
            {
                continue;
            }
            let this_ivl = self.natural_interval();
            let other_ivl = other.natural_interval();

            // but skip solutions across seams
            if (same_to_scaled_zero(sol0.start.param(0)?, this_ivl.min(), this_ivl.length())
                && same_to_scaled_zero(sol1.start.param(0)?, this_ivl.max(), this_ivl.length())
                && !are_same(this_ivl.min(), this_ivl.max()))
                || (same_to_scaled_zero(sol0.start.param(1)?, other_ivl.min(), other_ivl.length())
                    && same_to_scaled_zero(
                        sol1.start.param(1)?,
                        other_ivl.max(),
                        other_ivl.length(),
                    )
                    && !are_same(other_ivl.min(), other_ivl.max()))
            {
                continue;
            }
            self.evaluate_point(k, sol0.start.param(0)?)?;
            self.evaluate_point(k, sol1.start.param(0)?)?;
        }
        Ok(())
    }

    /// The spurious point check of [`Curve::global_curve_intersect`]: of two nearby
    /// point solutions with very different values, the worse is removed when the
    /// curves' gap at the midpoint is smaller than at the worse one.
    fn cull_spurious_points<O: SolutionObject>(
        k: &Kernel,
        sols: &mut SolutionArray<O>,
        named: &Named<'_, O>,
        domain_a: &Extent1d,
        domain_b: &Extent1d,
    ) -> Result<()> {
        let max_param_delta_a = domain_a.length() / 100.0; // mainly to avoid seams.
        let max_param_delta_b = domain_b.length() / 100.0;
        let num_sols = sols.len();
        for ii in 0..num_sols.saturating_sub(1) {
            let sol_a = sol_at(sols, ii)?;

            // Check only single solutions.
            if sol_a.solution_type != SolutionType::SingleValue {
                continue;
            }

            // Skip already-marked solutions (see below).
            if sol_a.start.solution_value > BIG_DOUBLE / 2.0 {
                continue;
            }

            let param_a1 = sol_a.start.parameters[0];
            let param_b1 = sol_a.start.parameters[1];

            // Double-loop on all solution pairs.
            for jj in ii + 1..num_sols {
                // (re-read: an earlier pair may have marked it)
                let sol_a = sol_at(sols, ii)?;
                let sol_b = sol_at(sols, jj)?;

                // Check only single solutions.
                if sol_b.solution_type != SolutionType::SingleValue {
                    continue;
                }

                // Skip already-'deleted' solutions (see below).
                if sol_b.start.solution_value > BIG_DOUBLE / 2.0 {
                    continue;
                }

                let param_a2 = sol_b.start.parameters[0];
                let param_b2 = sol_b.start.parameters[1];

                if (param_a2 - param_a1).abs() > max_param_delta_a {
                    break; // go on to next outer-loop iteration.
                }
                if (param_b2 - param_b1).abs() > max_param_delta_b {
                    break;
                }

                // Ok, the sols are fairly close on both curves. Check solution
                // values: we're looking for the case where they are very different.
                let val_a = sol_a.start.solution_value;
                let val_b = sol_b.start.solution_value;
                if val_a < val_b * 100.0 && val_a > val_b / 100.0 {
                    continue;
                }

                // Ok, one sol is much better than the other. Check the distances at
                // both solutions and at the midpoint.
                let curve_a = named.curve_of(sol_a.objects[0]).ok_or_else(|| {
                    SolidError::curve(OP_GCI, CurveFailure::SolutionWithoutCurve { object: 0 })
                })?;
                let curve_b = named.curve_of(sol_a.objects[1]).ok_or_else(|| {
                    SolidError::curve(OP_GCI, CurveFailure::SolutionWithoutCurve { object: 1 })
                })?;
                // (statuses ignored upstream; passed on as the fork, tangency-labs/smlib
                // 8311f8e)
                let pt_a = curve_a.evaluate_point(k, param_a1)?;
                let pt_b = curve_b.evaluate_point(k, param_b1)?;
                let dist1 = pt_a.distance(pt_b);

                let pt_a = curve_a.evaluate_point(k, param_a2)?;
                let pt_b = curve_b.evaluate_point(k, param_b2)?;
                let dist2 = pt_a.distance(pt_b);

                let t = (param_a1 + param_a2) / 2.0;
                let pt_a = curve_a.evaluate_point(k, t)?;
                let t = (param_b1 + param_b2) / 2.0;
                let pt_b = curve_b.evaluate_point(k, t)?;
                let dist_mid = pt_a.distance(pt_b);

                if dist1 < dist2 && dist_mid < dist2 {
                    // Remove the second solution (marked; deleted afterwards).
                    let mut s = sol_b;
                    s.start.solution_value = BIG_DOUBLE;
                    put_at(sols, jj, &s)?;
                } else if dist2 < dist1 && dist_mid < dist1 {
                    // Remove the first solution.
                    let mut s = sol_a;
                    s.start.solution_value = BIG_DOUBLE;
                    put_at(sols, ii, &s)?;
                }
            }
        }

        // Now remove all marked solutions.
        let mut done = false;
        while !done {
            done = true;
            for ii in 0..sols.len() {
                if sol_at(sols, ii)?.start.solution_value > BIG_DOUBLE / 2.0 {
                    sols.remove_at(ii, 1);
                    done = false;
                    break;
                }
            }
        }
        Ok(())
    }

    /// Not upstream (#18064): removes from `sols` each range solution that is the flank
    /// of a tangency, not a stretch where the two curves coincide.
    ///
    /// Two curves in contact of high order (a curve ending on an arc it is tangent to,
    /// apart as the fourth power of the distance from the touch) stay within the scaled
    /// zero of each other for a while beside the touching point, so the solve can report
    /// a range there whose two ends are both exact intersections; upstream keeps such a
    /// range as a coincidence (`classify_ranges`), and the curves are then cut at its
    /// far end, which is no event on either curve. Phase 2 (`cull_ranges_near_points`)
    /// discards a range beside a point solution for the same reason, but only when it is
    /// shorter than a thousandth of the domain.
    ///
    /// A range is such a flank when both its ends are exact to the scaled zero and a
    /// touching point lies beyond one end on both curves (a point solution, or the
    /// point where an end of each curve's interval meets the other's within the scaled
    /// zero), where the two curves are tangent, and from which the gap grows faster
    /// than linearly across the range: the gap at the touch is less than at the range's
    /// near end, and the gap divided by the straight distance from the touch on this
    /// curve is less at the near end than at the far end. The gap of a tangency grows at
    /// least as the square of the distance from the touch, so that ratio grows with the
    /// distance, and growth that is linear or slower is no tangency. The gap is taken at the
    /// range's two ends, the exact intersections the solve found, and not between the
    /// range and the touch: an intersection curve fitted to its surfaces strays from the
    /// other curve there by up to its fit tolerance, which can be larger than the
    /// flank's own gap (#18064's insert with a flatter top).
    ///
    /// Three things tell such a flank from a coincidence, whose gap along the range is
    /// only the rounding of the evaluations and says nothing of growth:
    /// - the range stops short of the touch (its near end lies farther than the scaled
    ///   zero from the touch on this curve): a coincidence through a point where the
    ///   curves meet reaches that point, as the gap is zero all along it;
    /// - its far end lies off the ends of both curves' intervals (farther than the scaled
    ///   zero from each), so the range ends where the curves part, not where one of them
    ///   stops (a seam edge along its surface's seam curve, a cone's edge along its
    ///   ruling);
    /// - the far end's two parameters give points within the scaled zero of each other,
    ///   as an exact end does, and the gap at the near end is larger than at the touch.
    ///
    /// # Errors
    ///
    /// Those of the evaluations.
    fn drop_ranges_of_tangencies<O: SolutionObject>(
        &self,
        k: &Kernel,
        sols: &mut SolutionArray<O>,
        other: &Self,
        interval: &Extent1d,
        other_interval: &Extent1d,
    ) -> Result<()> {
        let gap_at = |a: f64, b: f64| -> Result<f64> {
            Ok(self
                .evaluate_point(k, a)?
                .distance(other.evaluate_point(k, b)?))
        };
        // where the ends of the two intervals meet: touching points the solve need not
        // report
        let mut touches: Vec<[f64; 2]> = Vec::new();
        for a in [interval.min(), interval.max()] {
            for b in [other_interval.min(), other_interval.max()] {
                let p = self.evaluate_point(k, a)?;
                if gap_at(a, b)? < EFF_ZERO * (1.0 + p.max_dimension()) {
                    touches.push([a, b]);
                }
            }
        }
        // the ends of each curve's interval, where a coincidence of the two can stop
        let self_ends = [
            self.evaluate_point(k, interval.min())?,
            self.evaluate_point(k, interval.max())?,
        ];
        let other_ends = [
            other.evaluate_point(k, other_interval.min())?,
            other.evaluate_point(k, other_interval.max())?,
        ];
        let tangent_at =
            |c: &Self, t: f64| -> Result<Vec3> { Ok(at(&c.evaluate(k, t, 1, true, true)?, 1)) };
        let mut ii = 0;
        while ii < sols.len() {
            let range = sol_at(sols, ii)?;
            if range.solution_type != SolutionType::RangeOfValues {
                ii += 1;
                continue;
            }
            let pnt = self.evaluate_point(k, range.start.parameters[0])?;
            let scaled_zero = EFF_ZERO * (1.0 + pnt.max_dimension());
            if range.start.solution_value >= scaled_zero || range.end.solution_value >= scaled_zero
            {
                ii += 1;
                continue;
            }
            let ends = [
                [range.start.parameters[0], range.start.parameters[1]],
                [range.end.parameters[0], range.end.parameters[1]],
            ];
            let mut candidates = touches.clone();
            for s in sols.as_slice() {
                if s.solution_type == SolutionType::SingleValue {
                    candidates.push([s.start.parameters[0], s.start.parameters[1]]);
                }
            }
            let mut flank = false;
            for touch in candidates {
                for (near, far) in [(ends[0], ends[1]), (ends[1], ends[0])] {
                    // the touch lies beyond the near end, away from the far end, on both
                    let beyond = touch
                        .iter()
                        .zip(near.iter())
                        .zip(far.iter())
                        .all(|((t, n), f)| (t - n) * (f - n) <= 0.0 && (t - f).abs() > 0.0);
                    if !beyond {
                        continue;
                    }
                    // the curves part at the far end within both of them
                    let far_self = self.evaluate_point(k, far[0])?;
                    let far_other = other.evaluate_point(k, far[1])?;
                    let parts_inside = self_ends
                        .iter()
                        .all(|e| e.distance(far_self) >= scaled_zero)
                        && other_ends
                            .iter()
                            .all(|e| e.distance(far_other) >= scaled_zero);
                    // the range stops short of the touch
                    let short_of_touch = self
                        .evaluate_point(k, touch[0])?
                        .distance(self.evaluate_point(k, near[0])?)
                        >= scaled_zero;
                    // and they are tangent at the touch
                    let tangent = are_parallel(
                        tangent_at(self, touch[0])?,
                        tangent_at(other, touch[1])?,
                        None,
                    );
                    if !parts_inside || !short_of_touch || !tangent {
                        continue;
                    }
                    let g_touch = gap_at(touch[0], touch[1])?;
                    let g_near = gap_at(near[0], near[1])?;
                    let g_far = gap_at(far[0], far[1])?;
                    // the distances of the range's ends from the touch along this curve
                    let at_touch = self.evaluate_point(k, touch[0])?;
                    let d_near = at_touch.distance(self.evaluate_point(k, near[0])?);
                    let d_far = at_touch.distance(far_self);
                    // the far end lies farther from the touch than the near end, which lies
                    // off it (short_of_touch), and g_near / d_near < g_far / d_far
                    if g_far < scaled_zero
                        && g_touch < g_near
                        && d_near < d_far
                        && g_near * d_far < g_far * d_near
                    {
                        flank = true;
                    }
                }
                if flank {
                    break;
                }
            }
            if flank {
                sols.remove_at(ii, 1);
            } else {
                ii += 1;
            }
        }
        Ok(())
    }

    /// Not upstream (WR-647; the fork's tangency-labs/smlib@c2e3d9d and 04b42b4, decided
    /// here on the merged ranges instead of on one span pair): turns each stretch of range
    /// solutions that is a contact of two distinct curves seen through the tolerance, not
    /// a stretch where they lie on each other, into that contact's point.
    ///
    /// The span pairs report a range wherever the curves stay within `dist_tol` of each
    /// other: two distinct curves crossing at a shallow angle (two equal circles a little
    /// apart) or touching (a curve tangent to another) do so over a stretch about the
    /// contact, and the stretch becomes a tiny edge or a false overlap. A stretch is the
    /// ranges that meet end to end or overlap (in this curve's parameter, or their ends
    /// within `dist_tol`), as the solver splits one at a span or at the crossing. Two curves
    /// lying on each other stay within the tolerance until one of them stops, so their
    /// stretch ends at an end of a curve. A stretch is a contact when one of its ends lies
    /// inside both curves (that end's point on each curve is farther than `dist_tol` from
    /// both ends of that curve's interval, and for a curve against itself not where the
    /// curve folds back onto itself): there the curves part. On one span pair this
    /// cannot be told: an edge and a fitted copy of it, whose separation is
    /// tolerance-scale fit error that can change sides anywhere, look like a crossing there
    /// (#1036), and two concentric arcs a constant distance apart like a touch.
    ///
    /// The contact is
    /// - a crossing when the separations at the stretch's ends, taken across this curve
    ///   (the tangent's component removed), are both longer than the scaled zero and point
    ///   to opposite sides (a negative dot product), the curves part at both of its
    ///   ends (a stretch that runs on to a curve end may be a crossing followed by the
    ///   curves together within the tolerance up to that end, and keeps its ranges), and
    ///   [`curves_meet_at_most_once`] certifies one meeting at most over the stretch (on
    ///   each curve, from its least to its greatest range-end parameter; three crossings
    ///   in one stretch keep their ranges): the
    ///   one [`local_curve_intersect`]
    ///   finds within `dist_tol` in the stretch, started where the separation, taken as
    ///   linear between the ends, vanishes, and when the point found there is no nearer
    ///   than the range end where the curves are nearest (the stretch's, or one where two
    ///   of its ranges meet), the nearer of it and the one found from that end, when that is
    ///   nearer than the end and inside the stretch (else the solver did not find the
    ///   crossing);
    /// - otherwise, when the stretch is all the curves' solutions, the point where they come
    ///   nearest in it: the least distance over the closed stretch, the solver's interior
    ///   minimum ([`Curve::local_curve_solve`], `Minimize`, started at the range end, of
    ///   the stretch or where two of its ranges meet, where the curves are nearest) when it
    ///   finds one nearer than that end, else that end; or, started at another range end
    ///   of the stretch, a minimum found nearer than that end and nearer than this one by
    ///   more than the scaled zero (the curves' nearer approach, which the first solve,
    ///   local, may miss). The curves
    ///   part at an end, so their separation is not constant and they touch where it is
    ///   least: a stretch whose parting ends are not farther apart than its touch (by
    ///   more than the scaled zero) keeps its ranges, a constant separation (concentric
    ///   arcs). It is a touch only where the curves MEET: from that minimum the separation
    ///   across the curves is solved for where it is least
    ///   ([`SolverOperation::SignedDirectedMinimize`], along its own direction, in the plane
    ///   of it and the tangent: it grows as the square of the distance from a touch, where
    ///   the distance's square grows as the fourth power and the `Minimize` solve stops
    ///   short), and the two points found must lie within the scaled zero of each other,
    ///   evaluated. Curves that come no nearer than that stay within the tolerance of each
    ///   other along the stretch without meeting, a coincidence at the tolerance (an edge
    ///   against a fitted copy of a part of it that leaves it on one side), and keep their
    ///   ranges as upstream. Nearest at an end of the stretch, they meet there (a round
    ///   leaving a line it is tangent to at their shared end touches it there). And the
    ///   touch must be certified the stretch's only contact, over the whole stretch
    ///   ([`touch_is_alone`]: one straight curve, the other's second derivative across it
    ///   of one sign; or [`curves_meet_at_most_once`]); else (two curved curves tangent to
    ///   each other, an exact or near overlap, also with a conic over an arc, two touches)
    ///   the stretch keeps its ranges: a tangency has no sign change, so at the scaled zero
    ///   it is a short coincidence and no bound tells it from one. `drop_ranges_of_tangencies`
    ///   drops the flank ranges beside a touch (a curve against itself whose touch is where
    ///   it folds back onto itself), and a face/face section tangent to a floor arc at their
    ///   shared end vertex is made one point by the tangency witness
    ///   (`crate::intersect::curve_class::witness`, #18064). Curves that come
    ///   back within the tolerance elsewhere without changing sides
    ///   are one coincidence whose separation passes the tolerance here and there (a fitted
    ///   copy of a closed section, #1036), and keep their ranges; so do two separate touches
    ///   of the same two curves, as upstream.
    ///
    /// `itself` says that `other` is this curve (its parameters are folded on the
    /// diagonal); a caller that hands in copies (the projected curves of
    /// `GlobalCurveSolve`) says so here.
    ///
    /// A NaN fails these tests, and a crossing the solver does not find, keep the ranges.
    ///
    /// # Errors
    ///
    /// Those of the evaluations, of [`local_curve_intersect`] and of
    /// [`Curve::local_curve_solve`].
    pub(crate) fn contacts_seen_through_tolerance<O: SolutionObject>(
        &self,
        k: &Kernel,
        sols: &mut SolutionArray<O>,
        other: &Self,
        interval: &Extent1d,
        other_interval: &Extent1d,
        dist_tol: f64,
        itself: bool,
    ) -> Result<()> {
        let ranges: Vec<usize> = (0..sols.len())
            .filter(|&i| {
                sols.get(i)
                    .is_some_and(|s| s.solution_type == SolutionType::RangeOfValues)
            })
            .collect();
        if ranges.is_empty() {
            return Ok(());
        }
        // each range's ends on this curve: parameters and points
        let mut ends = Vec::new();
        for &i in &ranges {
            let r = sol_at(sols, i)?;
            let (s, e) = (r.start.parameters, r.end.parameters);
            ends.push((
                [s[0], s[1]],
                [e[0], e[1]],
                self.evaluate_point(k, s[0])?,
                self.evaluate_point(k, e[0])?,
            ));
        }
        // the stretches: ranges that meet or overlap, by a union of their indices
        let mut stretch_of: Vec<usize> = (0..ranges.len()).collect();
        for i in 0..ranges.len() {
            for j in i + 1..ranges.len() {
                let (Some(ri), Some(rj)) = (ends.get(i), ends.get(j)) else {
                    continue;
                };
                let (lo_i, hi_i) = (min2(ri.0[0], ri.1[0]), max2(ri.0[0], ri.1[0]));
                let (lo_j, hi_j) = (min2(rj.0[0], rj.1[0]), max2(rj.0[0], rj.1[0]));
                let overlap = lo_i <= hi_j && lo_j <= hi_i;
                let meet = [ri.2, ri.3]
                    .iter()
                    .any(|p| [rj.2, rj.3].iter().any(|q| p.distance(*q) <= dist_tol));
                if overlap || meet {
                    let (Some(&si), Some(&sj)) = (stretch_of.get(i), stretch_of.get(j)) else {
                        continue;
                    };
                    let (keep, gone) = (si.min(sj), si.max(sj));
                    for s in &mut stretch_of {
                        if *s == gone {
                            *s = keep;
                        }
                    }
                }
            }
        }
        let self_ends = [
            self.evaluate_point(k, interval.min())?,
            self.evaluate_point(k, interval.max())?,
        ];
        let other_ends = [
            other.evaluate_point(k, other_interval.min())?,
            other.evaluate_point(k, other_interval.max())?,
        ];
        // farther than the tolerance from both ends (NaN is not)
        let inside = |p: Vec3, ends: &[Vec3; 2]| ends.iter().all(|e| p.distance(*e) > dist_tol);
        let mut gone: Vec<usize> = Vec::new();
        for stretch in 0..ranges.len() {
            let members: Vec<usize> = (0..ranges.len())
                .filter(|&m| stretch_of.get(m) == Some(&stretch))
                .collect();
            let Some(&first) = members.first() else {
                continue;
            };
            // the stretch's ends: its least and greatest parameters on this curve
            let mut s = [f64::INFINITY, 0.0];
            let mut e = [f64::NEG_INFINITY, 0.0];
            for &m in &members {
                let Some(r) = ends.get(m) else { continue };
                for end in [r.0, r.1] {
                    if end[0] < s[0] {
                        s = end;
                    }
                    if end[0] > e[0] {
                        e = end;
                    }
                }
            }
            if !(s[0].is_finite() && e[0].is_finite()) {
                continue;
            }
            let this_a = self.evaluate_point(k, s[0])?;
            let this_b = self.evaluate_point(k, e[0])?;
            let other_a = other.evaluate_point(k, s[1])?;
            let other_b = other.evaluate_point(k, e[1])?;
            // a curve against itself: an end where the stretch reaches the diagonal, the
            // curve between its two parameters within the tolerance, is a fold of the
            // curve back onto itself, not a parting
            let fold = |end: [f64; 2], p: Vec3| -> Result<bool> {
                Ok(itself
                    && self.evaluate_point(k, (end[0] + end[1]) * 0.5)?.distance(p) <= dist_tol)
            };
            let parts_a =
                inside(this_a, &self_ends) && inside(other_a, &other_ends) && !fold(s, this_a)?;
            let parts_b =
                inside(this_b, &self_ends) && inside(other_b, &other_ends) && !fold(e, this_b)?;
            if !(parts_a || parts_b) {
                continue;
            }
            let separation_a = self.across_tangent(k, s[0], other_a - this_a)?;
            let separation_b = self.across_tangent(k, e[0], other_b - this_b)?;
            let scaled_zero =
                EFF_ZERO * (1.0 + max2(this_a.max_dimension(), this_b.max_dimension()));
            let (da, db) = (separation_a.length(), separation_b.length());
            if da.is_nan() || db.is_nan() || separation_a.dot(separation_b).is_nan() {
                continue;
            }
            let Some(&first_index) = ranges.get(first) else {
                continue;
            };
            let mut sol = sol_at(sols, first_index)?;
            let mut ivl1 = Extent1d::from_value(s[0]);
            ivl1.add_value(e[0]);
            let mut ivl2 = Extent1d::from_value(s[1]);
            ivl2.add_value(e[1]);
            // the stretch on each curve: this curve's from its least to its greatest
            // parameter, the other's over every range end of its members
            let mut other_lo = f64::INFINITY;
            let mut other_hi = f64::NEG_INFINITY;
            for &m in &members {
                let Some(r) = ends.get(m) else { continue };
                for end in [r.0, r.1] {
                    other_lo = min2(other_lo, end[1]);
                    other_hi = max2(other_hi, end[1]);
                }
            }
            let stretches = ((s[0], e[0]), (other_lo, other_hi));
            // the range end (the stretch's, or one where two of its ranges meet) where the
            // curves are nearest
            let (gap_a, gap_b) = (this_a.distance(other_a), this_b.distance(other_b));
            let (mut guess, mut gap) = if gap_b < gap_a {
                (e, gap_b)
            } else {
                (s, gap_a)
            };
            for &m in &members {
                let Some(r) = ends.get(m) else { continue };
                for (end, p) in [(r.0, r.2), (r.1, r.3)] {
                    let end_gap = p.distance(other.evaluate_point(k, end[1])?);
                    if end_gap < gap {
                        (guess, gap) = (end, end_gap);
                    }
                }
            }
            if da > scaled_zero && db > scaled_zero && separation_a.dot(separation_b) < 0.0 {
                // the curves change sides in the stretch; it is their crossing only where
                // they part at both ends: a stretch running on to a curve end may hold a
                // crossing and then the two curves together within the tolerance up to
                // that end, a coincidence
                if !(parts_a && parts_b) {
                    continue;
                }
                // and only where it is the stretch's one meeting: the curves' cones of
                // derivative directions over it part (else three crossings, or a crossing
                // and a coincidence, could hide in one stretch, and it keeps its ranges)
                if !curves_meet_at_most_once(k, self, stretches.0, other, stretches.1) {
                    continue;
                }
                // start where the separation, taken as linear between the ends, vanishes;
                // a point the solver accepts there no nearer than the nearest range end is
                // not the crossing, so start again from that end
                let w = da / (da + db);
                let mut found = local_curve_intersect(
                    k,
                    self,
                    &ivl1,
                    other,
                    &ivl2,
                    dist_tol,
                    s[0] + w * (e[0] - s[0]),
                    s[1] + w * (e[1] - s[1]),
                )?;
                if found.as_ref().is_none_or(|c| {
                    c.deviation.partial_cmp(&gap) != Some(core::cmp::Ordering::Less)
                }) {
                    let again = local_curve_intersect(
                        k, self, &ivl1, other, &ivl2, dist_tol, guess[0], guess[1],
                    )?;
                    if let Some(c) = again {
                        if found.as_ref().is_none_or(|f| c.deviation < f.deviation) {
                            found = Some(c);
                        }
                    }
                }
                // the curves are apart at both ends, so they cross inside the stretch and
                // nearer than at any range end: a point at an end of the stretch, or no
                // nearer than the nearest range end, is not where they cross (the solver
                // did not find the crossing)
                let Some(cross) = found.filter(|c| {
                    c.deviation < gap && ivl1.min() < c.my_param && c.my_param < ivl1.max()
                }) else {
                    continue;
                };
                sol.start.parameters[0] = cross.my_param;
                sol.start.parameters[1] = cross.other_param;
                sol.start.solution_value = cross.deviation;
            } else if members.len() == sols.len() {
                // the least distance over the closed stretch: the solver's interior
                // minimum, started at that nearest range end, or that end
                let minimum = |seed: [f64; 2]| {
                    self.local_curve_solve(
                        k,
                        &ivl1,
                        other,
                        &ivl2,
                        SolverOperation::Minimize,
                        dist_tol,
                        None,
                        None,
                        seed,
                        [sol.objects[0], sol.objects[1]],
                    )
                };
                let (found, nearest) = minimum(guess)?;
                let mut touch = if found && nearest.start.solution_value < gap {
                    [
                        nearest.start.parameters[0],
                        nearest.start.parameters[1],
                        nearest.start.solution_value,
                    ]
                } else {
                    [guess[0], guess[1], gap]
                };
                // the least: started at every other range end of the stretch, the solver
                // may find another minimum (nearer than that end), the curves' nearer
                // approach when nearer by more than the scaled zero
                for &m in &members {
                    let Some(r) = ends.get(m) else { continue };
                    for (end, p) in [(r.0, r.2), (r.1, r.3)] {
                        if end == guess {
                            continue;
                        }
                        let end_gap = p.distance(other.evaluate_point(k, end[1])?);
                        let (found, at) = minimum(end)?;
                        let value = at.start.solution_value;
                        if found && value < end_gap && value + scaled_zero < touch[2] {
                            touch = [at.start.parameters[0], at.start.parameters[1], value];
                        }
                    }
                }
                // where the curves part they are apart: farther than at the touch (a
                // constant separation is a coincidence)
                let apart = |parts: bool, end_gap: f64| !parts || end_gap > touch[2] + scaled_zero;
                if !(apart(parts_a, gap_a) && apart(parts_b, gap_b)) {
                    continue;
                }
                // a touch is where the curves meet: their points there, evaluated, within
                // the scaled zero of each other. Curves that come no nearer than that over
                // the stretch only stay within the tolerance of each other there, a
                // coincidence at the tolerance (an edge against a fitted copy of a part of
                // it, which leaves it on one side), and keep their ranges as upstream. This
                // also places the touch: nearest at an end of the stretch, the curves meet
                // there (a shared end). The distance's minimum is flat where the curves are
                // tangent (it grows as the square of the distance from the touch, its square
                // as the fourth power), so the solver stops short of it; the separation
                // across the curves, taken along its own direction, grows as the square and
                // is solved for where it is least
                // ([`SolverOperation::SignedDirectedMinimize`], in the plane of the tangent
                // and that direction), from there
                let q1 = self.evaluate_point(k, touch[0])?;
                let q2 = other.evaluate_point(k, touch[1])?;
                let zero_here = EFF_ZERO * (1.0 + max2(q1.max_dimension(), q2.max_dimension()));
                if !matches!(
                    q1.distance(q2).partial_cmp(&zero_here),
                    Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
                ) {
                    let across = self.across_tangent(k, touch[0], q2 - q1)?;
                    let tangent = at(&self.evaluate(k, touch[0], 1, false, true)?, 1);
                    let side = across.length();
                    let plane = tangent.cross(across);
                    let plane_length = plane.length();
                    // (no direction across, a NaN included: no touch is placed)
                    if !(side > zero_here && plane_length > 0.0) {
                        continue;
                    }
                    let vectors = [plane * (1.0 / plane_length), across * (1.0 / side)];
                    let (found, met) = self.local_curve_solve(
                        k,
                        &ivl1,
                        other,
                        &ivl2,
                        SolverOperation::SignedDirectedMinimize,
                        dist_tol,
                        None,
                        Some(&vectors),
                        [touch[0], touch[1]],
                        [sol.objects[0], sol.objects[1]],
                    )?;
                    if !found {
                        continue;
                    }
                    let (t, u) = (met.start.parameters[0], met.start.parameters[1]);
                    let value = self
                        .evaluate_point(k, t)?
                        .distance(other.evaluate_point(k, u)?);
                    touch = [t, u, value];
                }
                let p1 = self.evaluate_point(k, touch[0])?;
                let p2 = other.evaluate_point(k, touch[1])?;
                let meet_zero = EFF_ZERO * (1.0 + max2(p1.max_dimension(), p2.max_dimension()));
                if !matches!(
                    p1.distance(p2).partial_cmp(&meet_zero),
                    Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
                ) {
                    continue;
                }
                // a curve against itself nearest where it folds back onto itself (the
                // diagonal) does not touch itself there
                if fold([touch[0], touch[1]], p1)? {
                    continue;
                }
                // the touch is the only place in the stretch where the curves meet (one
                // cluster of meetings, between which the curves do not part), certified
                // over the whole stretch ([`touch_is_alone`]: a straight curve against one
                // whose second derivative across it keeps one sign over the stretch;
                // [`curves_meet_at_most_once`], for curves that pass through each other in
                // space with both ends of the stretch on one side, as a line through a
                // space curve);
                // else the stretch may hold more than that one contact (an exact or near
                // overlap, a fitted copy touching its edge where the fit is exact, two
                // touches) and keeps its ranges: a tangency of two curved curves has no
                // sign change, so at the scaled zero it cannot be told from a short
                // coincidence (the face/face tangency witness makes the shared-end
                // tangency of a section and a floor arc one point, #18064)
                if !(touch_is_alone(
                    k,
                    self,
                    stretches.0,
                    other,
                    stretches.1,
                    [touch[0], touch[1]],
                    dist_tol,
                )? || curves_meet_at_most_once(k, self, stretches.0, other, stretches.1))
                {
                    continue;
                }
                sol.start.parameters[0] = touch[0];
                sol.start.parameters[1] = touch[1];
                sol.start.solution_value = touch[2];
            } else {
                // the curves come back within the tolerance elsewhere: one coincidence
                // whose tolerance-scale separation passes the tolerance here and there
                continue;
            }
            sol.solution_type = SolutionType::SingleValue;
            put_at(sols, first_index, &sol)?;
            gone.extend(
                members
                    .iter()
                    .skip(1)
                    .filter_map(|&m| ranges.get(m).copied()),
            );
        }
        gone.sort_unstable();
        for &i in gone.iter().rev() {
            sols.remove_at(i, 1);
        }
        Ok(())
    }

    /// Not upstream (#18828): replaces two consecutive point solutions of a line and an
    /// ellipse (either way round) that stand for one touch with that touch
    /// (`Ellipse::touch_between_line_flanks`). At a tangency the local solves of the
    /// global solve stop on both flanks of the double root, where the curves are within
    /// the solver's zero of each other, and [`Curve::contacts_seen_through_tolerance`] only
    /// reads range solutions. The proof here is the circle's contact rule read on the
    /// ellipse's support function: the line stays outside the ellipse or enters it by no
    /// more than the contact tolerance, so the two points are not two crossings to keep.
    /// The touch is then tried against the point solutions on both sides of it, and each
    /// joins it with no further test of where it lies: the line does not cross the ellipse
    /// twice, and the distance from a point of a line to a convex region is convex along
    /// the line, so the points of the line within the tolerance of the ellipse are one
    /// stretch about the touch, and a point solution is one of them. Range solutions, and
    /// pairs with no certified touch, are left as they are.
    ///
    /// # Errors
    ///
    /// Those of `Ellipse::touch_between_line_flanks`.
    fn flank_points_of_an_ellipse_touch<O: SolutionObject>(
        &self,
        k: &Kernel,
        sols: &mut SolutionArray<O>,
        other: &Self,
        interval: &Extent1d,
        other_interval: &Extent1d,
        dist_tol: f64,
        contact_tol: f64,
    ) -> Result<()> {
        // the ellipse, the line, their intervals and which parameter is the ellipse's
        let (ellipse, line, e_ivl, l_ivl, e_at) = match (self.kind(), other.kind()) {
            (CurveKind::Ellipse(e), CurveKind::Line(l)) => (e, l, interval, other_interval, 0),
            (CurveKind::Line(l), CurveKind::Ellipse(e)) => (e, l, other_interval, interval, 1),
            _ => return Ok(()),
        };
        let l_at = 1 - e_at;
        let mut ii = 0;
        while ii + 1 < sols.len() {
            let (a, b) = (sol_at(sols, ii)?, sol_at(sols, ii + 1)?);
            if a.solution_type != SolutionType::SingleValue
                || b.solution_type != SolutionType::SingleValue
            {
                ii += 1;
                continue;
            }
            let flank =
                |s: &Solution<O>| Ok::<_, SolidError>([s.start.param(e_at)?, s.start.param(l_at)?]);
            let Some([u, w, dist]) = ellipse.touch_between_line_flanks(
                k,
                e_ivl,
                line,
                l_ivl,
                dist_tol,
                contact_tol,
                [flank(&a)?, flank(&b)?],
            )?
            else {
                ii += 1;
                continue;
            };
            let mut touch = a;
            *touch.start.param_mut(e_at)? = u;
            *touch.start.param_mut(l_at)? = w;
            touch.start.solution_value = dist;
            put_at(sols, ii, &touch)?;
            sols.remove_at(ii + 1, 1);
            // (the touch against the point before it next, then the one after)
            ii = ii.saturating_sub(1);
        }
        Ok(())
    }

    /// The range solution check of [`Curve::global_curve_intersect`]: ranges with
    /// exact ends are kept; very short ranges and ranges classified as tangent points
    /// (`ClassifyIntersectingRangeSolution`) become point solutions; long ranges are
    /// kept.
    fn classify_ranges<O: SolutionObject>(
        &self,
        k: &Kernel,
        sols: &mut SolutionArray<O>,
        other: &Self,
        interval: &Extent1d,
        other_interval: &Extent1d,
    ) -> Result<()> {
        // locals: curve/solution metrics
        let mut this_length = BIG_DOUBLE;
        let mut other_length = BIG_DOUBLE;
        let mut is_degenerate = false;

        // for every SM_ST_RANGE_OF_VALUES solution
        for ii in 0..sols.len() {
            let mut curr = sol_at(sols, ii)?;

            // skip SM_ST_SINGLE_VALUE solutions
            if curr.solution_type != SolutionType::RangeOfValues {
                continue;
            }

            // locals - intersection StartPoint, Start/End Deviations, Interval
            // ParameterLengths
            let dev_start = curr.start.solution_value;
            let dev_end = curr.end.solution_value;
            let mut ivl = Extent1d::from_value(curr.start.parameters[0]);
            ivl.add_value(curr.end.parameters[0]);
            let mut o_ivl = Extent1d::from_value(curr.start.parameters[1]);
            o_ivl.add_value(curr.end.parameters[1]);
            let ivl_length = ivl.length();
            let o_ivl_length = o_ivl.length();
            let pnt = self.evaluate_point(k, curr.start.parameters[0])?;

            // locals: Curve 3d and solution range lengths
            if this_length == BIG_DOUBLE {
                this_length = self.approximate_length(k, interval, 5, false)?.0;
            }
            if other_length == BIG_DOUBLE {
                other_length = other.approximate_length(k, other_interval, 5, false)?.0;
            }
            let seg_leng = self.approximate_length(k, &ivl, 5, false)?.0;

            // Scaled Zeros in 3D and both Parameter spaces
            let scaled_zero = EFF_ZERO * (1.0 + pnt.max_dimension());
            let scaled_ps_tol = 10000.0 * EFF_ZERO * (1.0 + interval.max_dimension());
            let scaled_ps_tol2 = 10000.0 * EFF_ZERO * (1.0 + other_interval.max_dimension());

            // keep nonZero intervals whose ends are exact intersections to ScaledZero
            if dev_start < scaled_zero
                && dev_end < scaled_zero
                && ivl_length > scaled_ps_tol
                && o_ivl_length > scaled_ps_tol2
            {
                continue;
            }

            // See if solution range is a large interval: at least 1/10 the length of
            // the shorter curve.
            let large_interval = seg_leng > min2(this_length, other_length) / 10.0;

            // See if solution range is a small interval: less than ScaledZero in
            // either curve domain
            let small_interval = ivl_length < scaled_ps_tol || o_ivl_length < scaled_ps_tol2;

            // for nonDegenerate intervals
            if !small_interval {
                // if GapVector is diverging over length of solution range change
                // solution to a tangent point intersection (status ignored upstream;
                // passed on as the fork, tangency-labs/smlib 8311f8e)
                let (deg, point) = self.classify_intersecting_range_solution(k, other, &curr)?;
                is_degenerate = deg;
                if let Some(p) = point {
                    curr.start.solution_value = p.solution_value;
                    curr.start.parameters[0] = p.parameters[0];
                    curr.start.parameters[1] = p.parameters[1];
                }
                put_at(sols, ii, &curr)?;
            }

            // keep interval if it is at least 1/10 the length of the shorter curve.
            if large_interval && !is_degenerate {
                continue;
            }

            // Change very small and diverging intervals into a single pt solutions.
            if small_interval || is_degenerate {
                // When working with short solution intervals with good solutions on
                // both ends load the one closest to a curve interval boundary, else
                // load the solution with smallest deviation into start
                if small_interval
                    && curr.start.solution_value < EFF_ZERO
                    && curr.end.solution_value < EFF_ZERO
                {
                    let del_start0 = curr.start.parameters[0] - interval.min();
                    let del_end0 = interval.max() - curr.end.parameters[0];

                    let del_start_beg1 = curr.start.parameters[1] - other_interval.min();
                    let del_start_end1 = other_interval.max() - curr.start.parameters[1];
                    let del_end_beg1 = curr.end.parameters[1] - other_interval.min();
                    let del_end_end1 = other_interval.max() - curr.end.parameters[1];
                    let del1_min = min4(del_start_beg1, del_start_end1, del_end_beg1, del_end_end1);

                    let at_start_of_1st_curve = del_start0 < EFF_ZERO;
                    let at_end_of_1st_curve = del_end0 < EFF_ZERO;
                    let other_1st_closest_to_start = del_end_beg1 == del1_min;
                    let other_1st_closest_to_end = del_end_end1 == del1_min;

                    // use the end Solution when it does not start on the 1st curve
                    // beg param and ends on the 1st curve end param or on the 2nd
                    // curve beg/end param
                    let use_end_solution = !at_start_of_1st_curve
                        && (at_end_of_1st_curve
                            || other_1st_closest_to_start
                            || other_1st_closest_to_end);
                    if use_end_solution {
                        curr.start = curr.end;
                    }
                } else if curr.start.solution_value > curr.end.solution_value {
                    curr.start = curr.end;
                }

                // change the solutionType
                curr.solution_type = SolutionType::SingleValue;
                put_at(sols, ii, &curr)?;
            }
        }
        Ok(())
    }
}

// ---- WR-647 contact certificates ---------------------------------------------------------

/// The most control points of one curve's stretch the WR-647 contact certificates take
/// (bounded work: the cone test runs over every pair and triple of their differences); a
/// stretch over more gets no certificate, and its ranges are kept.
const MAX_STRETCH_POINTS: usize = 16;

/// A curve over a stretch written as a B-spline (WR-647 certificates): knots, degree and
/// homogeneous control points `(x w, w)` (`w = 1` without weights), each with a bound on
/// the distance of its computed `x w` from the exact one. With exact arithmetic the curve
/// over the stretch IS this B-spline: a B-spline kind is its own NURB (the spans meeting
/// the stretch, by local support), a line or a Hermite cubic is a polynomial written in
/// Bernstein form, a parallel projection an affine map of the control points.
struct StretchSpline {
    knots: Vec<f64>,
    degree: usize,
    /// `(x w, w, error of x w)`.
    points: Vec<(Vec3, f64, f64)>,
    rational: bool,
}

/// `a > b`, false for a NaN.
fn above(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) == Some(core::cmp::Ordering::Greater)
}

/// One generator of a cone of derivative directions: a vector and a bound on the
/// distance of the computed vector from an exact generator.
#[derive(Clone, Copy)]
struct ConeGenerator {
    v: Vec3,
    err: f64,
}

impl StretchSpline {
    /// The spline of `curve` over `[lo, hi]`, or `None` (no certificate) for other curve
    /// kinds (offset, on-surface, composite, perspective or rotation projections, a
    /// projection of a projection), a stretch outside the curve's knots, more than
    /// [`MAX_STRETCH_POINTS`] points, a 2D curve, or anything not finite or a weight not
    /// positive.
    fn of(curve: &Curve, lo: f64, hi: f64, projected: bool) -> Option<Self> {
        Self::of_up_to(curve, lo, hi, projected, MAX_STRETCH_POINTS)
    }

    /// [`StretchSpline::of`] with at most `most` control points in place of
    /// [`MAX_STRETCH_POINTS`] (for a certificate whose work grows only with their count).
    fn of_up_to(curve: &Curve, lo: f64, hi: f64, projected: bool, most: usize) -> Option<Self> {
        if !(lo.is_finite() && hi.is_finite() && lo <= hi) {
            return None;
        }
        let spline = match curve.kind() {
            CurveKind::BSpline(c) => Self::of_nurb(c.nurb_ref().ok()?, lo, hi, most)?,
            CurveKind::Ellipse(c) => Self::of_nurb(c.base().nurb_ref().ok()?, lo, hi, most)?,
            CurveKind::Parabola(c) => Self::of_nurb(c.base().nurb_ref().ok()?, lo, hi, most)?,
            CurveKind::Hyperbola(c) => Self::of_nurb(c.base().nurb_ref().ok()?, lo, hi, most)?,
            CurveKind::Line(l) => {
                // affine in the parameter: the segment between its points at the ends of
                // the stretch, each computed from `point + s vector scale` (a few
                // rounded operations on each coordinate)
                if lo >= hi {
                    return None;
                }
                let anchor = l.line_point().length();
                let end = |t: f64| -> Option<(Vec3, f64, f64)> {
                    let p = *l.evaluate(t, 0).first()?;
                    Some((p, 1.0, 4.0 * gamma(16) * (anchor + p.length())))
                };
                Self {
                    knots: vec![lo, lo, hi, hi],
                    degree: 1,
                    points: vec![end(lo)?, end(hi)?],
                    rational: false,
                }
            }
            CurveKind::Hermite(h) => {
                // the power basis `P1 + t (D1 + t (C + t D))` in Bernstein form on [0, 1]
                if !(0.0 <= lo && hi <= 1.0) {
                    return None;
                }
                let (p, d1) = (h.start_point(), h.start_derivative());
                let (c, d) = h.power_basis();
                let err = 4.0 * gamma(6) * (p.length() + d1.length() + c.length() + d.length());
                let bezier = [
                    p,
                    p + d1 * (1.0 / 3.0),
                    p + (d1 * 2.0 + c) * (1.0 / 3.0),
                    p + d1 + c + d,
                ];
                Self {
                    knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    degree: 3,
                    points: bezier.iter().map(|&b| (b, 1.0, err)).collect(),
                    rational: false,
                }
            }
            CurveKind::Projected(pc)
                if pc.proj_type() == ProjectionType::Parallel && !projected =>
            {
                // `x - n ((x - o) . n)` is affine: the control points map by it (in
                // homogeneous form, `x w - n ((x w - o w) . n)`)
                let base = Self::of_up_to(pc.curve(), lo, hi, true, most)?;
                let (n, o) = (pc.proj_vec(), pc.proj_point());
                let stretch = 1.0 + n.dot(n);
                let points = base
                    .points
                    .iter()
                    .map(|&(x, w, err)| {
                        let y = x - n * (x - o * w).dot(n);
                        let size = x.length() + w * o.length();
                        (y, w, (2.0 * err + 8.0 * gamma(8) * size) * stretch)
                    })
                    .collect();
                Self { points, ..base }
            }
            _ => return None,
        };
        let finite = spline.points.iter().all(|&(x, w, err)| {
            x.x.is_finite()
                && x.y.is_finite()
                && x.z.is_finite()
                && w.is_finite()
                && w > 0.0
                && err.is_finite()
        });
        finite.then_some(spline)
    }

    /// The spans of a NURB meeting `[lo, hi]`, with the control points and knots they use.
    fn of_nurb(nurb: &NurbCurve, lo: f64, hi: f64, most: usize) -> Option<Self> {
        let p = nurb.degree();
        let (cpts, knots) = (nurb.cpts(), nurb.knots());
        let n = cpts.len().checked_sub(1)?;
        if p == 0 || knots.len() != n + p + 2 {
            return None;
        }
        if !knots
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.is_finite() && a <= b))
        {
            return None;
        }
        if !(*knots.get(p)? <= lo && hi <= *knots.get(n + 1)?) {
            return None;
        }
        let meets = |j: usize| -> bool {
            matches!(
                (knots.get(j), knots.get(j + 1)),
                (Some(&a), Some(&b)) if a < b && a <= hi && lo <= b
            )
        };
        let first = (p..=n).find(|&j| meets(j))?;
        let last = (p..=n).rev().find(|&j| meets(j))?;
        if last + 1 - first + p > most {
            return None;
        }
        let rational = nurb.is_rational();
        let mut points = Vec::new();
        for cp in cpts.get(first - p..=last)? {
            if cp.z == NOZ {
                return None;
            }
            let w = if rational { cp.w } else { 1.0 };
            points.push((Vec3::new(cp.x, cp.y, cp.z), w, 0.0));
        }
        Some(Self {
            knots: knots.get(first - p..=last + p + 1)?.to_vec(),
            degree: p,
            points,
            rational,
        })
    }

    /// The spans `j` (`knots[j] < knots[j + 1]`) meeting `[lo, hi]`.
    fn spans(&self, lo: f64, hi: f64) -> Vec<usize> {
        let p = self.degree;
        (p..self.points.len())
            .filter(|&j| {
                matches!(
                    (self.knots.get(j), self.knots.get(j + 1)),
                    (Some(&a), Some(&b)) if a < b && a <= hi && lo <= b
                )
            })
            .collect()
    }

    /// The point `i` (`x w / w`) and a bound on its error.
    fn point(&self, i: usize) -> Option<(Vec3, f64)> {
        let &(x, w, err) = self.points.get(i)?;
        if self.rational {
            let p = x * (1.0 / w);
            Some((
                p,
                (err / w) * (1.0 + gamma(2)) + 2.0 * gamma(2) * p.length(),
            ))
        } else {
            Some((x, err))
        }
    }

    /// Generators of a convex cone holding every first derivative over `[lo, hi]`: the
    /// differences of consecutive control points of the spans meeting it (the derivative
    /// over span `j` is a combination with non-negative coefficients of `P[i + 1] - P[i]`,
    /// `j - p <= i < j`, those whose basis has a positive support). With weights, only
    /// for degrees up to 2: a rational quadratic span with positive weights is a conic arc
    /// whose derivative is a non-negative combination of its two legs (`2 w0 w1 (1 - t)²
    /// (Q1 - Q0) + 2 w0 w2 t (1 - t) (Q2 - Q0) + 2 w1 w2 t² (Q2 - Q1)` over `w²`, and the
    /// legs of its Bezier form lie along these differences). `None` past that.
    fn derivative_cone(&self, lo: f64, hi: f64) -> Option<Vec<ConeGenerator>> {
        let p = self.degree;
        if self.rational && p > 2 {
            return None;
        }
        let mut used: Vec<usize> = Vec::new();
        for j in self.spans(lo, hi) {
            for i in j - p..j {
                let support = match (self.knots.get(i + 1), self.knots.get(i + p + 1)) {
                    (Some(&a), Some(&b)) => a < b,
                    _ => false,
                };
                if support && !used.contains(&i) {
                    used.push(i);
                }
            }
        }
        let mut gens = Vec::new();
        for i in used {
            let ((a, ea), (b, eb)) = (self.point(i)?, self.point(i + 1)?);
            let v = b - a;
            let err = ea + eb + 2.0 * gamma(1) * v.length();
            if v == Vec3::ZERO && err == 0.0 {
                continue;
            }
            gens.push(ConeGenerator { v, err });
        }
        (!gens.is_empty()).then_some(gens)
    }
}

/// Whether the vectors lie in an open half-space through the origin: some `n` with
/// `n . v > 0` for each, the rounding of each vector and of the dot product counted
/// (`γ_3 |n| |v|`, Cauchy-Schwarz). The candidate normals are those of the least cap of
/// directions holding them all (its centre lies on the bisector of two of them or is
/// equally far from three), which is a separating `n` whenever one exists.
fn in_open_half_space(gens: &[ConeGenerator]) -> bool {
    let mut units = Vec::new();
    for g in gens {
        let length = g.v.length();
        if !above(length, g.err) {
            return false;
        }
        units.push(g.v * (1.0 / length));
    }
    let holds = |n: Vec3| {
        let size = n.length();
        gens.iter()
            .all(|g| n.dot(g.v) > size * (g.err + gamma(3) * g.v.length()) * (1.0 + gamma(16)))
    };
    for (i, &a) in units.iter().enumerate() {
        for &b in units.iter().skip(i) {
            if holds(a + b) {
                return true;
            }
        }
    }
    for (i, &a) in units.iter().enumerate() {
        for (j, &b) in units.iter().enumerate().skip(i + 1) {
            for &c in units.iter().skip(j + 1) {
                let n = (a - b).cross(a - c);
                if holds(n) || holds(-n) {
                    return true;
                }
            }
        }
    }
    false
}

/// Not upstream (WR-647): a certificate that two curves meet at most once over the
/// stretches `[a.0, a.1]` of `a` and `[b.0, b.1]` of `b` (parameters), and part linearly
/// from where they come nearest: their cones of derivative directions there, `K_a` and
/// `K_b`, meet only at the origin, and so do `K_a` and `-K_b`. Two meetings `(t, u)`,
/// `(t', u')` with `t < t'` would give a chord `c_a(t') - c_a(t)`, the integral of `c_a'`,
/// in `K_a \ {0}` and, the same chord, in `K_b` or `-K_b`. Each test is a separating
/// plane ([`in_open_half_space`] of the generators of one cone and those of the other,
/// negated or not), so the rounding of the generators and of the test is counted. The
/// cones part by an angle `θ > 0`, so the difference of the curves grows at least as
/// `sin θ` times the distance moved along them: points within the meeting zero `z` of each
/// other (the touch's test) lie in one cluster at most about `2 z / sin θ` long, where the
/// curves do not part.
///
/// Lines, B-splines (rational ones up to degree 2: conic arcs), Hermite cubics and their
/// parallel projections; any other curve, or a stretch over more than
/// [`MAX_STRETCH_POINTS`] control points, gets no certificate (`false`).
#[must_use]
pub fn curves_meet_at_most_once(
    k: &Kernel,
    a: &Curve,
    a_stretch: (f64, f64),
    b: &Curve,
    b_stretch: (f64, f64),
) -> bool {
    let cone = |c: &Curve, (lo, hi): (f64, f64)| {
        StretchSpline::of(c, lo, hi, false).and_then(|s| s.derivative_cone(lo, hi))
    };
    let (Some(ka), Some(kb)) = (cone(a, a_stretch), cone(b, b_stretch)) else {
        return false;
    };
    let with = |sign: f64| -> Vec<ConeGenerator> {
        ka.iter()
            .copied()
            .chain(kb.iter().map(|g| ConeGenerator {
                v: g.v * sign,
                err: g.err,
            }))
            .collect()
    };
    if in_open_half_space(&with(-1.0)) && in_open_half_space(&with(1.0)) {
        return true;
    }
    subdivision_parts(k, a, a_stretch, b, b_stretch).unwrap_or(false)
        || conic_parts(a, a_stretch, b, b_stretch)
        || conic_parts(b, b_stretch, a, a_stretch)
}

/// The quadratic `G(x) = ((x - c) . X)² + ((x - c) . Y)² - 1` that an ellipse (or its
/// parallel projection) lies on, as three linear forms of the homogeneous point
/// `(x w, w)`: `l1 = (x w - c w) . X`, `l2 = (x w - c w) . Y`, `l3 = w`, and
/// `w² G = l1² + l2² - l3²`. `X` and `Y` are the axes over the radii; for a projection along `n` the point
/// is first moved along `n` into the ellipse's plane (normal `N`), which folds into the
/// axes: `X - N (n . X) / (n . N)`. Any computed `c`, `X`, `Y` serve: the certificate
/// bounds `G` over the conic's own stretch rather than assuming it vanishes there.
fn conic_forms(curve: &Curve) -> Option<(Vec3, Vec3, Vec3)> {
    let (ellipse, along) = match curve.kind() {
        CurveKind::Ellipse(e) => (e, None),
        CurveKind::Projected(pc) if pc.proj_type() == ProjectionType::Parallel => {
            match pc.curve().kind() {
                CurveKind::Ellipse(e) => (e, Some(pc.proj_vec())),
                _ => return None,
            }
        }
        _ => return None,
    };
    let place = ellipse.position();
    let (c, x_axis, y_axis) = (place.origin(), place.x_axis(), place.y_axis());
    let (rx, ry) = (ellipse.x_radius(), ellipse.y_radius());
    if !(above(rx, 0.0) && above(ry, 0.0)) {
        return None;
    }
    let (mut x, mut y) = (x_axis * (1.0 / rx), y_axis * (1.0 / ry));
    if let Some(n) = along {
        let normal = x_axis.cross(y_axis);
        let d = n.dot(normal);
        if !above(d.abs(), 1e-3 * n.length() * normal.length()) {
            return None;
        }
        x = x - normal * (n.dot(x) / d);
        y = y - normal * (n.dot(y) / d);
    }
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    (finite(c) && finite(x) && finite(y)).then_some((c, x, y))
}

/// The Bezier coefficients of `w² G` over a piece (degree `2 p`, from the homogeneous
/// Bezier points of degree `p` by the product of Bernstein polynomials), each with an
/// error bound.
fn conic_coefficients(
    forms: (Vec3, Vec3, Vec3),
    piece: &[(Vec3, f64, f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    let (c, x, y) = forms;
    let (sx, sy, sc) = (x.length(), y.length(), c.length());
    // l1, l2, l3 of each point, and their errors
    let mut forms_at = Vec::new();
    for &(xw, w, ex, ew) in piece {
        let d = xw - c * w;
        let size = xw.length() + sc * w;
        let err_d = ex + sc * ew + 2.0 * gamma(2) * size;
        let grow = 1.0 + gamma(4);
        let l1 = (d.dot(x), (sx * err_d + gamma(3) * sx * d.length()) * grow);
        let l2 = (d.dot(y), (sy * err_d + gamma(3) * sy * d.length()) * grow);
        forms_at.push([l1, l2, (w, ew)]);
    }
    let p = piece.len().checked_sub(1)?;
    let binomial = |n: usize, r: usize| -> f64 {
        let mut b = 1.0;
        for i in 0..r {
            #[allow(clippy::cast_precision_loss)]
            let step = (n - i) as f64 / (i + 1) as f64;
            b *= step;
        }
        b
    };
    let mut out = Vec::new();
    for kk in 0..=2 * p {
        let (mut value, mut err) = (0.0, 0.0);
        let whole = binomial(2 * p, kk);
        for i in kk.saturating_sub(p)..=kk.min(p) {
            let j = kk - i;
            let (Some(a), Some(b)) = (forms_at.get(i), forms_at.get(j)) else {
                return None;
            };
            let weight = binomial(p, i) * binomial(p, j) / whole;
            let mut term = 0.0;
            let mut term_err = 0.0;
            let mut size = 0.0;
            for (f, sign) in [(0, 1.0), (1, 1.0), (2, -1.0)] {
                let (Some(&(u, eu)), Some(&(v, ev))) = (a.get(f), b.get(f)) else {
                    return None;
                };
                term += sign * u * v;
                term_err += u.abs() * ev + v.abs() * eu + eu * ev;
                size += u.abs() * v.abs();
            }
            value += weight * term;
            err += weight * (term_err + gamma(4) * size);
        }
        // the weights (exact up to a few roundings each) and the sum
        let size: f64 = value.abs() + err;
        out.push((
            value,
            (err + gamma(4 * p + 8) * size * 4.0) * (1.0 + gamma(4)),
        ));
    }
    Some(out)
}

/// Not upstream (WR-647): the third certificate of [`curves_meet_at_most_once`], for a
/// conic `a` (an ellipse or its parallel projection) against `b` (two equal circles a
/// little apart cross at a small angle, too small against their turning over the stretch
/// for cones or slopes). Every point of `a` over its stretch has `|G| <= eps` (the bound
/// of `w² G`'s Bezier coefficients over its pieces, over the least weight squared); a
/// meeting is a point of `b` with `|G| <= eps`, so `|w² G(b(u))| <= eps w_max²`. When the
/// Bezier coefficients of `w² G(b)` over every piece of `b`'s stretch strictly increase (or
/// strictly decrease) in turn, `w² G(b)` is strictly monotone over it and the meetings lie
/// in one interval of `b`'s parameter where it is within `eps w_max²` of zero: one contact.
/// Each span's part of `b`'s stretch is cut into 1, 2, 4, ... pieces, up to
/// [`MAX_SLOPE_PIECES`] in all.
fn conic_parts(a: &Curve, a_stretch: (f64, f64), b: &Curve, b_stretch: (f64, f64)) -> bool {
    let Some(forms) = conic_forms(a) else {
        return false;
    };
    let (Some(sa), Some(sb)) = (
        StretchSpline::of(a, a_stretch.0, a_stretch.1, false),
        StretchSpline::of(b, b_stretch.0, b_stretch.1, false),
    ) else {
        return false;
    };
    // eps: |G| over a's stretch (its curve lies in the hull of its Bezier coefficients)
    let Some(pieces_a) = sa.homogeneous_pieces(a_stretch.0, a_stretch.1, 1) else {
        return false;
    };
    let mut bound: f64 = 0.0;
    let mut least_w = f64::INFINITY;
    for piece in &pieces_a {
        let Some(coef) = conic_coefficients(forms, piece) else {
            return false;
        };
        for (v, e) in coef {
            bound = max2(bound, v.abs() + e);
        }
        for &(_, w, _, ew) in piece {
            least_w = min2(least_w, w - ew);
        }
    }
    if !(above(least_w, 0.0) && bound.is_finite()) {
        return false;
    }
    let spans = sb.spans(b_stretch.0, b_stretch.1).len();
    let mut m = 1;
    while spans * m <= MAX_SLOPE_PIECES {
        let monotone = sb
            .homogeneous_pieces(b_stretch.0, b_stretch.1, m)
            .and_then(|pieces| {
                let mut side = 0.0;
                for piece in &pieces {
                    let coef = conic_coefficients(forms, piece)?;
                    for pair in coef.windows(2) {
                        let [(v0, e0), (v1, e1)] = pair else {
                            return None;
                        };
                        let step = v1 - v0;
                        let err = (e0 + e1 + gamma(1) * step.abs()) * (1.0 + gamma(1));
                        let sign = if above(step, err) {
                            1.0
                        } else if above(-step, err) {
                            -1.0
                        } else {
                            return None;
                        };
                        if side != 0.0 && sign != side {
                            return None;
                        }
                        side = sign;
                    }
                }
                Some(())
            })
            .is_some();
        if monotone {
            return true;
        }
        m *= 2;
    }
    false
}

/// The homogeneous Bezier points `(x w, w, error of x w, error of w)` of a piece.
type HomogeneousPiece = Vec<(Vec3, f64, f64, f64)>;

/// A piece of a curve over a parameter interval inside one span: its Bezier points
/// (Euclidean, each with an error bound), from the blossom of the span.
struct BezierPiece {
    points: Vec<(Vec3, f64)>,
}

impl StretchSpline {
    /// The blossom `f(args)` of span `span` (`args` in `[knots[span], knots[span + 1]]`):
    /// de Boor's recursion with the argument of each level, convex combinations, in
    /// homogeneous form; `(x w, w, error of x w, error of w)`.
    fn blossom(&self, span: usize, args: &[f64]) -> Option<(Vec3, f64, f64, f64)> {
        let p = self.degree;
        let first = span.checked_sub(p)?;
        let used = self.points.get(first..=span)?;
        let mut x: Vec<Vec3> = used.iter().map(|c| c.0).collect();
        let mut w: Vec<f64> = used.iter().map(|c| c.1).collect();
        let size = used.iter().map(|c| c.0.length() + c.2).fold(0.0, max2);
        let w_size = used.iter().map(|c| c.1).fold(0.0, max2);
        let input = used.iter().map(|c| c.2).fold(0.0, max2);
        for r in 1..=p {
            let arg = *args.get(r - 1)?;
            for j in (r..=p).rev() {
                let i = first + j;
                let (lo, hi) = (*self.knots.get(i)?, *self.knots.get(i + p + 1 - r)?);
                let alpha = (arg - lo) / (hi - lo);
                if !(0.0..=1.0).contains(&alpha) {
                    return None;
                }
                let xv = *x.get(j - 1)? * (1.0 - alpha) + *x.get(j)? * alpha;
                *x.get_mut(j)? = xv;
                if self.rational {
                    let wv = (1.0 - alpha) * *w.get(j - 1)? + alpha * *w.get(j)?;
                    *w.get_mut(j)? = wv;
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let levels = p as f64;
        let grow = 1.0 + gamma(4 * p + 4);
        // each coordinate as in [`de_boor`], the vector's error within twice the largest
        let err_x = 2.0 * (input + 4.0 * levels * gamma(4) * size) * grow;
        let err_w = if self.rational {
            4.0 * levels * gamma(4) * w_size * grow
        } else {
            0.0
        };
        Some((*x.get(p)?, *w.get(p)?, err_x, err_w))
    }

    /// The Bezier points of the curve over `[t0, t1]` inside span `span` (blossom values
    /// `f(t0^(p - r), t1^r)`), Euclidean, each with an error bound; `None` for a weight
    /// not above its error.
    fn piece(&self, span: usize, t0: f64, t1: f64) -> Option<BezierPiece> {
        let mut points = Vec::new();
        for (x, w, ex, ew) in self.homogeneous_piece(span, t0, t1)? {
            if self.rational {
                if !above(w, ew) {
                    return None;
                }
                let point = x * (1.0 / w);
                let err = (ex + point.length() * ew) / (w - ew) * (1.0 + gamma(4))
                    + 2.0 * gamma(2) * point.length();
                points.push((point, err));
            } else {
                points.push((x, ex));
            }
        }
        Some(BezierPiece { points })
    }

    /// The homogeneous Bezier points `(x w, w, error of x w, error of w)` of the curve over
    /// `[t0, t1]` inside span `span` (blossom values `f(t0^(p - r), t1^r)`).
    fn homogeneous_piece(&self, span: usize, t0: f64, t1: f64) -> Option<HomogeneousPiece> {
        let p = self.degree;
        (0..=p)
            .map(|r| {
                let args: Vec<f64> = (0..p).map(|i| if i < p - r { t0 } else { t1 }).collect();
                self.blossom(span, &args)
            })
            .collect()
    }

    /// The stretch `[lo, hi]` cut at the knots and each span's part into `m` equal pieces.
    fn pieces(&self, lo: f64, hi: f64, m: usize) -> Option<Vec<BezierPiece>> {
        self.cut(lo, hi, m, Self::piece)
    }

    /// [`StretchSpline::pieces`] in homogeneous form.
    fn homogeneous_pieces(&self, lo: f64, hi: f64, m: usize) -> Option<Vec<HomogeneousPiece>> {
        self.cut(lo, hi, m, Self::homogeneous_piece)
    }

    /// `make` over the stretch `[lo, hi]` cut at the knots and each span's part into `m`
    /// equal pieces.
    fn cut<T>(
        &self,
        lo: f64,
        hi: f64,
        m: usize,
        make: impl Fn(&Self, usize, f64, f64) -> Option<T>,
    ) -> Option<Vec<T>> {
        let mut out = Vec::new();
        #[allow(clippy::cast_precision_loss)]
        let mf = m as f64;
        for j in self.spans(lo, hi) {
            let (a, b) = (
                max2(lo, *self.knots.get(j)?),
                min2(hi, *self.knots.get(j + 1)?),
            );
            if a > b {
                continue;
            }
            let at = |i: usize| -> f64 {
                #[allow(clippy::cast_precision_loss)]
                let f = i as f64 / mf;
                if i == m {
                    b
                } else {
                    min2(b, a + (b - a) * f)
                }
            };
            for i in 0..m {
                out.push(make(self, j, at(i), at(i + 1))?);
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// The interval of `dir . x` over a piece (its hull: the curve lies in the convex hull of
/// its Bezier points), every rounding counted.
fn piece_extent(piece: &BezierPiece, dir: Vec3) -> (f64, f64) {
    let size = dir.length();
    let mut range = (f64::INFINITY, f64::NEG_INFINITY);
    for &(q, err) in &piece.points {
        let value = dir.dot(q);
        let slack = (size * err + gamma(3) * size * q.length()) * (1.0 + gamma(4));
        range = (min2(range.0, value - slack), max2(range.1, value + slack));
    }
    range
}

/// The most pieces each curve's stretch is cut into by [`conic_parts`] (bounded work).
const MAX_SLOPE_PIECES: usize = 64;

/// The most piece pairs [`subdivision_parts`] keeps at one level, and the most levels.
const MAX_SURVIVING_PAIRS: usize = 4096;
/// See [`MAX_SURVIVING_PAIRS`].
const MAX_SUBDIVISION_LEVELS: usize = 24;

/// A piece of a stretch in [`subdivision_parts`]: its span and its interval in that span.
type PieceSpan = (usize, f64, f64);

/// The generators of the cone of first derivatives of a stretch from the legs of its
/// Bezier pieces (each span's part of `[lo, hi]`), as [`StretchSpline::derivative_cone`]
/// but of the stretch alone.
fn stretch_cone(s: &StretchSpline, lo: f64, hi: f64) -> Option<Vec<ConeGenerator>> {
    if s.rational && s.degree > 2 {
        return None;
    }
    let mut gens = Vec::new();
    for piece in s.pieces(lo, hi, 1)? {
        for pair in piece.points.windows(2) {
            let [(a, ea), (b, eb)] = pair else {
                return None;
            };
            let v = *b - *a;
            let err = ea + eb + 2.0 * gamma(1) * v.length();
            if v == Vec3::ZERO && err == 0.0 {
                continue;
            }
            gens.push(ConeGenerator { v, err });
        }
    }
    (!gens.is_empty()).then_some(gens)
}

/// Whether the cones of derivative directions of `a` over `[a.0, a.1]` and `b` over `[b.0,
/// b.1]` part (as in [`curves_meet_at_most_once`]).
fn cones_part(sa: &StretchSpline, a: (f64, f64), sb: &StretchSpline, b: (f64, f64)) -> bool {
    let (Some(ka), Some(kb)) = (stretch_cone(sa, a.0, a.1), stretch_cone(sb, b.0, b.1)) else {
        return false;
    };
    let with = |sign: f64| -> Vec<ConeGenerator> {
        ka.iter()
            .copied()
            .chain(kb.iter().map(|g| ConeGenerator {
                v: g.v * sign,
                err: g.err,
            }))
            .collect()
    };
    in_open_half_space(&with(-1.0)) && in_open_half_space(&with(1.0))
}

/// Not upstream (WR-647): the second certificate of [`curves_meet_at_most_once`], for two
/// curves too close to parallel over their stretches for their cones to part (a curve
/// and its slightly rotated copy). Cut both stretches into pieces (one span each, halved
/// level by level) and drop every pair of pieces whose hulls (of their Bezier points,
/// from blossoms, rounding counted) are apart along one of eight directions (the axes, the
/// chord of `a`, the change of the separation across it, their cross product, and the two
/// normals of the chord of the pair's piece of `a`) by more than the meeting zero (the
/// scaled zero over both stretches, which the touch takes for a meeting, doubled for the
/// rounding of the touch's evaluated points, times the direction's length): no meeting,
/// even one within that zero, lies in a dropped pair. Two
/// curves within the zero of each other at two places keep both pairs (and their box). When the pairs left lie in a box (of each curve's
/// least and greatest parameter over them) over which the two cones part, the curves
/// meet at most once there, so at most once over the stretches; none left, they do not
/// meet. Near a crossing at a small angle the pairs left shrink with the pieces until
/// their box is short enough for the cones to part. At most [`MAX_SURVIVING_PAIRS`] pairs
/// are kept at a level, over at most [`MAX_SUBDIVISION_LEVELS`] levels; past either,
/// no certificate.
///
/// # Errors
///
/// Those of the evaluations of the stretches' ends.
fn subdivision_parts(
    k: &Kernel,
    a: &Curve,
    a_stretch: (f64, f64),
    b: &Curve,
    b_stretch: (f64, f64),
) -> Result<bool> {
    let (Some(sa), Some(sb)) = (
        StretchSpline::of(a, a_stretch.0, a_stretch.1, false),
        StretchSpline::of(b, b_stretch.0, b_stretch.1, false),
    ) else {
        return Ok(false);
    };
    if (sa.rational && sa.degree > 2) || (sb.rational && sb.degree > 2) {
        return Ok(false);
    }
    let (a0, a1) = (
        a.evaluate_point(k, a_stretch.0)?,
        a.evaluate_point(k, a_stretch.1)?,
    );
    let (mut b0, mut b1) = (
        b.evaluate_point(k, b_stretch.0)?,
        b.evaluate_point(k, b_stretch.1)?,
    );
    let v = a1 - a0;
    if (b1 - b0).dot(v) < 0.0 {
        core::mem::swap(&mut b0, &mut b1);
    }
    let w = (b1 - a1) - (b0 - a0);
    let directions = [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        v,
        w,
        v.cross(w),
    ];
    let plane = v.cross(w);
    // the meeting zero over both stretches, `z = EFF_ZERO (1 + size)` (`size` bounding every
    // coordinate of their control points, so of the curves, which lie in their hulls): the
    // touch's own test, which takes points within `z` of each other for a meeting
    let mut size: f64 = 0.0;
    for s in [&sa, &sb] {
        for i in 0..s.points.len() {
            let Some((x, err)) = s.point(i) else {
                return Ok(false);
            };
            size = max2(size, x.max_dimension() + err);
        }
    }
    // doubled: the touch tests evaluated points, each off its curve by its evaluation's
    // rounding (a few `γ` times `size` at the degrees a stretch takes, far below `z / 2`), so
    // a meeting by that test is within `2 z` of the curves exactly
    let zero = 2.0 * EFF_ZERO * (1.0 + size);
    if !zero.is_finite() {
        return Ok(false);
    }
    // whether the hulls of two pieces are apart along one of the six directions, or along
    // the two normals of the chord of `a`'s piece (across which the piece is thin, so near
    // a crossing at a small angle its pairs part once the pieces are thinner than the
    // curves' separation there), by more than the meeting zero: two points within `z` of
    // each other are within `|d| z` along `d`, so no meeting lies in a pair apart by more
    // (a NaN is not apart)
    let apart = |x: PieceSpan, y: PieceSpan| -> Option<bool> {
        let (px, py) = (sa.piece(x.0, x.1, x.2)?, sb.piece(y.0, y.1, y.2)?);
        let (first, last) = (px.points.first()?.0, px.points.last()?.0);
        let chord = last - first;
        let across = chord.cross(plane);
        let normals = [across, chord.cross(across)];
        Some(directions.iter().chain(&normals).any(|&d| {
            let (p, q) = (piece_extent(&px, d), piece_extent(&py, d));
            let margin = d.length() * zero * (1.0 + gamma(4));
            above(q.0 - p.1, margin) || above(p.0 - q.1, margin)
        }))
    };
    let first = |s: &StretchSpline, (lo, hi): (f64, f64)| -> Option<Vec<PieceSpan>> {
        let mut out = Vec::new();
        for j in s.spans(lo, hi) {
            let (a, b) = (max2(lo, *s.knots.get(j)?), min2(hi, *s.knots.get(j + 1)?));
            if a <= b {
                out.push((j, a, b));
            }
        }
        Some(out)
    };
    let (Some(pa), Some(pb)) = (first(&sa, a_stretch), first(&sb, b_stretch)) else {
        return Ok(false);
    };
    let mut pairs: Vec<(PieceSpan, PieceSpan)> = Vec::new();
    for &x in &pa {
        for &y in &pb {
            pairs.push((x, y));
        }
    }
    for _ in 0..MAX_SUBDIVISION_LEVELS {
        if pairs.len() > MAX_SURVIVING_PAIRS {
            return Ok(false);
        }
        let mut left = Vec::new();
        for &(x, y) in &pairs {
            let Some(parted) = apart(x, y) else {
                return Ok(false);
            };
            if !parted {
                left.push((x, y));
            }
        }
        if left.is_empty() {
            return Ok(true);
        }
        let mut box_a = (f64::INFINITY, f64::NEG_INFINITY);
        let mut box_b = (f64::INFINITY, f64::NEG_INFINITY);
        for &(x, y) in &left {
            box_a = (min2(box_a.0, x.1), max2(box_a.1, x.2));
            box_b = (min2(box_b.0, y.1), max2(box_b.1, y.2));
        }
        if cones_part(&sa, box_a, &sb, box_b) {
            return Ok(true);
        }
        let halves = |(j, lo, hi): PieceSpan| {
            let mid = lo + (hi - lo) * 0.5;
            [(j, lo, mid), (j, mid, hi)]
        };
        pairs = Vec::new();
        for (x, y) in left {
            for x2 in halves(x) {
                for y2 in halves(y) {
                    pairs.push((x2, y2));
                }
            }
        }
    }
    Ok(false)
}

/// The value at `x` of the scalar B-spline of degree `p` on `knots` with coefficients
/// `coef` (each `(value, error)`), by de Boor's recursion in span `span`
/// (`knots[span] <= x <= knots[span + 1]`), and a bound on its error: each level is a
/// convex combination (`alpha` in `[0, 1]`), which adds at most `4 γ_3` of the largest
/// magnitude `M` (the rounded `alpha` and the two products and the sum) and keeps the
/// input errors' bound (`|1 - alpha| + |alpha| = 1`, up to the same rounding).
fn de_boor(
    knots: &[f64],
    p: usize,
    coef: &[(f64, f64)],
    span: usize,
    x: f64,
) -> Option<(f64, f64)> {
    let first = span.checked_sub(p)?;
    let used = coef.get(first..=span)?;
    let mut d: Vec<f64> = used.iter().map(|c| c.0).collect();
    let size = used.iter().map(|c| c.0.abs() + c.1).fold(0.0, max2);
    let input = used.iter().map(|c| c.1).fold(0.0, max2);
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = first + j;
            let (lo, hi) = (*knots.get(i)?, *knots.get(i + p + 1 - r)?);
            let alpha = (x - lo) / (hi - lo);
            let value = (1.0 - alpha) * *d.get(j - 1)? + alpha * *d.get(j)?;
            *d.get_mut(j)? = value;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let levels = p as f64;
    let err = (input + 4.0 * levels * gamma(4) * size) * (1.0 + gamma(4 * p + 4));
    Some((*d.get(p)?, err))
}

/// Not upstream (WR-647): a certificate that a touch of two curves at `at` (parameters,
/// the curves meeting there) is their only contact over the stretches `[a.0, a.1]` and
/// `[b.0, b.1]`, for a straight curve (a line, or a degree 1 span) against a B-spline or
/// a Hermite cubic: all points where they meet lie in one interval of the curved one's
/// parameter, over which they stay within the rounding of the test of each other.
///
/// Take `n`, across the line `q + s v` (`q` its control point, exact), along the curved
/// curve's second derivative at the touch, and `g(u) = n . (x(u) - w(u) q)`, a scalar
/// B-spline with the coefficients `n . (x_i w_i - w_i q)` (`x w` its homogeneous form,
/// `g = w f`, `f = n . (c(u) - q)`). Where the curves meet, `c(u) = q + s v` and
/// `|f| <= |s| |n . v| = eps_f` (`n` is a rounded normal to `v`; `|s v|` is bounded by the
/// control points' distances from `q`), so `|g| <= eps_f w_max`.
/// The curved curve is C1 over its stretch and the second derivative coefficients of `g`
/// over it (Bernstein-B-spline, by local support) are at least `kappa > 0`, so
/// `g(u) >= g(u0) + g'(u0) (u - u0) + kappa (u - u0)² / 2` and `g >= g(u0) - g'(u0)² /
/// (2 kappa)` over the stretch. When that is at least `-eps_f w_max`, the meetings lie
/// where the convex `g` is at most `eps_f w_max`: one interval, over which `|g|` stays
/// within `eps_f w_max`. Every rounding (the coefficients, the de Boor values at the
/// touch, the second differences) is bounded and taken against acceptance.
///
/// The same lower bound bounds the run within the meeting zero: where the curves are
/// within `zero = EFF_ZERO (1 + size)` of each other, `g <= zero_g = 2 w_max zero +
/// eps_f w_max`, which lies in an interval of length at most `2 sqrt(2 (zero_g - least) /
/// kappa)` (`least` the bound's least value), and in space at most that times the curved
/// curve's speed bound. The touch is certified only when that length is within `tol`:
/// whatever the curves do inside the run (one exact meeting, or none, a near overlap at
/// the zero, for which the interval argument above holds vacuously), they are within the
/// zero of each other only over a length the tolerance does not resolve, and outside it
/// they are apart by more than the zero. A run the bound cannot put within `tol` keeps
/// the ranges, upstream's answer, whether or not the curves meet exactly.
///
/// `Ok(false)` (no certificate, the ranges are kept) for any other pair of curves, a
/// rational curve of degree above 2 or a weight not positive, a stretch over more than
/// [`MAX_STRETCH_POINTS`] points, a curved curve not C1 inside its stretch, or a NaN.
///
/// # Errors
///
/// Those of the second derivative's evaluation at the touch.
pub fn touch_is_alone(
    k: &Kernel,
    a: &Curve,
    a_stretch: (f64, f64),
    b: &Curve,
    b_stretch: (f64, f64),
    at: [f64; 2],
    tol: f64,
) -> Result<bool> {
    for (straight, s_stretch, curved, c_stretch, u0) in [
        (a, a_stretch, b, b_stretch, at[1]),
        (b, b_stretch, a, a_stretch, at[0]),
    ] {
        // the straight curve over its stretch: one degree 1 span
        let Some(line) = StretchSpline::of(straight, s_stretch.0, s_stretch.1, false) else {
            continue;
        };
        let spans = line.spans(s_stretch.0, s_stretch.1);
        let ([span], 1) = (spans.as_slice(), line.degree) else {
            continue;
        };
        let (Some((q, err_q)), Some((q1, err_q1))) = (line.point(span - 1), line.point(*span))
        else {
            continue;
        };
        let v = q1 - q;
        let err_v = err_q + err_q1 + 2.0 * gamma(1) * v.length();
        let v_length = v.length();
        if !above(v_length, 2.0 * err_v) {
            continue;
        }
        if touch_is_alone_against(k, &q, err_q, v, err_v, curved, c_stretch, u0, tol)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// [`touch_is_alone`] for the line `q + s v` (computed `q` exact, `v` within `err_v`)
/// against `curved` over `stretch`, touching at `u0`.
#[allow(clippy::too_many_arguments)]
fn touch_is_alone_against(
    k: &Kernel,
    q: &Vec3,
    err_q: f64,
    v: Vec3,
    err_v: f64,
    curved: &Curve,
    stretch: (f64, f64),
    u0: f64,
    tol: f64,
) -> Result<bool> {
    let (lo, hi) = stretch;
    let Some(spline) = StretchSpline::of(curved, lo, hi, false) else {
        return Ok(false);
    };
    let p = spline.degree;
    if p < 2 || !(lo <= u0 && u0 <= hi) {
        return Ok(false);
    }
    let knots = &spline.knots;
    // C1 inside the stretch: no interior knot of multiplicity p or more
    let c1 = knots
        .iter()
        .all(|&t| !(lo < t && t < hi) || knots.iter().filter(|&&s| s == t).count() < p);
    if !c1 {
        return Ok(false);
    }
    let derivs = curved.evaluate(k, u0, 2, false, false)?;
    let unit = v * (1.0 / v.length());
    // a unit normal to the line along the across-line part of `d`
    let across_of = |d: Vec3| -> Option<Vec3> {
        let across = d - unit * d.dot(unit);
        let length = across.length();
        above(length, 0.0).then(|| across * (1.0 / length))
    };
    let pf = int_to_f64(p);
    // the speed bound of a curve without weights over its control polygon, and its size
    let mut speed: f64 = 0.0;
    let mut size: f64 = 0.0;
    for i in 0..spline.points.len() {
        let Some(&(x, _, err)) = spline.points.get(i) else {
            return Ok(false);
        };
        size = max2(size, x.max_dimension() + err);
        let (Some(&(y, _, err_y)), Some(&u1), Some(&u2)) = (
            spline.points.get(i + 1),
            knots.get(i + 1),
            knots.get(i + p + 1),
        ) else {
            continue;
        };
        if u2 > u1 {
            speed = max2(speed, ((y - x).length() + err + err_y) * pf / (u2 - u1));
        }
    }
    // the meetings lie in an interval of parameter length `width`: a curve without
    // weights moves no farther over it than its speed bound times that, and when that is
    // within the scaled zero the meetings are one contact
    let one_contact = |width: f64| {
        !spline.rational
            && width.is_finite()
            && width * speed * (1.0 + gamma(16)) <= EFF_ZERO * (1.0 + size)
    };
    // a touch: n across the line, along the curved curve's second derivative at the touch
    let Some(n) = across_of(at(&derivs, 2)) else {
        return Ok(false);
    };
    let Some((coef, slope, eps)) = line_offsets(&spline, q, err_q, v, err_v, n) else {
        return Ok(false);
    };
    // kappa: the least second derivative coefficient (p - 1) (d[i + 1] - d[i]) / (U[i + p + 1]
    // - U[i + 2]) of the spans meeting the stretch, less its error
    let mut kappa = f64::INFINITY;
    let spans = spline.spans(lo, hi);
    for &j in &spans {
        for i in j - p..j - 1 {
            let (Some(&u1), Some(&u2)) = (knots.get(i + 2), knots.get(i + p + 1)) else {
                return Ok(false);
            };
            let Some((r, e)) = coefficient_difference(&slope, i, pf - 1.0, u2 - u1) else {
                return Ok(false);
            };
            kappa = min2(kappa, r - e);
        }
    }
    if !(kappa > 0.0 && kappa.is_finite()) {
        return Ok(false);
    }
    // g and g' at the touch, in the span holding it
    let Some(&span) = spans.iter().find(|&&j| {
        matches!((knots.get(j), knots.get(j + 1)), (Some(&a), Some(&b)) if a <= u0 && u0 <= b)
    }) else {
        return Ok(false);
    };
    let (Some((g0, e0)), Some((g1, e1))) = (
        de_boor(knots, p, &coef, span, u0),
        knots
            .get(1..)
            .and_then(|tail| de_boor(tail, p - 1, &slope, span - 1, u0)),
    ) else {
        return Ok(false);
    };
    let slack = g1.abs() + e1;
    let least = (g0 - e0) - slack * slack / (2.0 * kappa) * (1.0 + gamma(4));
    if !(least.is_finite() && eps.is_finite()) {
        return Ok(false);
    }
    // the run within the meeting zero: where the curves are within `zero` of each other
    // (the touch's meeting test, `EFF_ZERO (1 + size)` over both curves' points),
    // `c(u) = q + s v + d` with `|d| <= zero`, so `|f| <= |n| zero + |s| |n . v|` with `|s v|
    // <= reach + zero` and `|n . v| / |v| <= |n|` (`eps` holds the `reach` part): `g <=
    // zero_g = 2 w_max |n| zero + eps`, its rounding in the last factor; g lies above its
    // quadratic lower bound, whose least is at least `least`, so that run lies in an
    // interval of parameter length at most `2 sqrt(2 (zero_g - least) / kappa)`, and in
    // space no longer than that times the curve's speed bound (below). Curves within the
    // zero of each other over a length the tolerance resolves are an overlap at the zero
    // (they need not meet exactly, so the meetings below may be none) and keep their
    // ranges; the touch is certified only when that whole run is within the tolerance
    // `tol`, a contact the tolerance does not resolve into a length
    let Some((speed_c, size_c, w_max)) = curve_speed_bound(&spline) else {
        return Ok(false);
    };
    let line_size = max2(q.max_dimension(), (*q + v).max_dimension()) + err_q + err_v;
    let zero = EFF_ZERO * (1.0 + max2(size_c, line_size)) * (1.0 + gamma(4));
    let zero_g = (2.0 * w_max * n.length() * zero + eps) * (1.0 + gamma(4));
    let run = 2.0 * (2.0 * (zero_g - least) / kappa).sqrt() * (1.0 + gamma(8));
    let run_length = run * speed_c * (1.0 + gamma(4));
    if !(run_length.is_finite() && run_length <= tol) {
        return Ok(false);
    }
    if least >= -eps {
        return Ok(true);
    }
    // else the meetings lie where g <= eps, inside where its quadratic lower bound is:
    // an interval of length 2 sqrt(2 (eps - least) / kappa) (an exact touch whose least the
    // bounds place a rounding below zero)
    Ok(one_contact(2.0 * (2.0 * (eps - least) / kappa).sqrt()))
}

/// For [`touch_is_alone`]: a bound on the speed `|c'(u)|` of `spline` over its knots, a
/// bound on `|c(u)|` (each coordinate) and the largest weight, or `None` (a weight not
/// positive, or not finite). With `X = x w` the homogeneous points, `c = X / w` and
/// `c' = (X' - c w') / w`: `X'` and `w'` are B-splines with the coefficients
/// `p (a[i + 1] - a[i]) / (U[i + p + 1] - U[i + 1])`, so each is at most its largest
/// coefficient; `c` lies in the
/// convex hull of the points `X_i / w_i` (positive weights), and `w` is at least the least
/// weight. A curve without weights has `w = 1`, `w' = 0`.
fn curve_speed_bound(spline: &StretchSpline) -> Option<(f64, f64, f64)> {
    let p = spline.degree;
    let pf = int_to_f64(p);
    let knots = &spline.knots;
    let weight = |w: f64| if spline.rational { w } else { 1.0 };
    let (mut x_speed, mut w_speed, mut size, mut length): (f64, f64, f64, f64) =
        (0.0, 0.0, 0.0, 0.0);
    let (mut w_min, mut w_max) = (f64::INFINITY, 0.0_f64);
    for i in 0..spline.points.len() {
        let &(x, w, err) = spline.points.get(i)?;
        let w = weight(w);
        let (point, err_point) = spline.point(i)?;
        size = max2(size, point.max_dimension() + err_point);
        length = max2(length, point.length() + err_point);
        w_min = min2(w_min, w);
        w_max = max2(w_max, w);
        let (Some(&(y, wy, err_y)), Some(&u1), Some(&u2)) = (
            spline.points.get(i + 1),
            knots.get(i + 1),
            knots.get(i + p + 1),
        ) else {
            continue;
        };
        if u2 > u1 {
            x_speed = max2(x_speed, ((y - x).length() + err + err_y) * pf / (u2 - u1));
            w_speed = max2(w_speed, (weight(wy) - w).abs() * pf / (u2 - u1));
        }
    }
    if !(w_min > 0.0 && w_min.is_finite() && w_max.is_finite()) {
        return None;
    }
    // each coefficient bound takes about 9 roundings (the length, two error adds, the
    // product, the knot difference and the quotient) and the sum, product and quotient
    // here 4 more
    let speed = (x_speed + length * w_speed) / w_min * (1.0 + gamma(16));
    (speed.is_finite() && size.is_finite()).then_some((speed, size, w_max))
}

/// Scalar B-spline or Bezier coefficients, each `(value, error)`.
type Coefficients = Vec<(f64, f64)>;

/// The `f64` of a small count (a degree).
#[allow(clippy::cast_precision_loss)]
fn int_to_f64(n: usize) -> f64 {
    n as f64
}

/// The coefficient `scale (a[i + 1] - a[i]) / gap` of a B-spline's derivative and its
/// error (0 where the basis has no support, `gap <= 0`).
fn coefficient_difference(a: &[(f64, f64)], i: usize, scale: f64, gap: f64) -> Option<(f64, f64)> {
    let (&(a0, e0), &(a1, e1)) = (a.get(i)?, a.get(i + 1)?);
    if gap <= 0.0 {
        return Some((0.0, 0.0));
    }
    let value = scale * (a1 - a0) / gap;
    let e = (scale * (e0 + e1) / gap + 2.0 * gamma(4) * scale * (a0.abs() + a1.abs()) / gap)
        * (1.0 + gamma(4));
    Some((value, e))
}

/// For [`touch_is_alone`]: the signed offset `g(u) = n . (x(u) - w(u) q)` of `spline` across
/// the line `q + s v` (computed `q` exact, `v` within `err_v`) along the unit normal `n`, as
/// the scalar B-spline coefficients of `g` and of its derivative (each `(value, error)`),
/// and `eps`, the most `|g|` can be where the curve meets the line: there `c(u) = q + s v`,
/// so `|f| <= |s| |n . v|` (`n` a rounded normal to `v`; `|s v|` bounded by the control
/// points' distances from `q`) and `|g| <= eps`.
fn line_offsets(
    spline: &StretchSpline,
    q: &Vec3,
    err_q: f64,
    v: Vec3,
    err_v: f64,
    n: Vec3,
) -> Option<(Coefficients, Coefficients, f64)> {
    let p = spline.degree;
    let knots = &spline.knots;
    let n_length = n.length();
    // g's coefficients n . (x w - w q), and the distance of the control points from q
    let mut coef = Vec::new();
    let mut reach: f64 = 0.0;
    let mut w_max: f64 = 0.0;
    for i in 0..spline.points.len() {
        let &(x, w, err) = spline.points.get(i)?;
        // componentwise: each coordinate of x - q w within gamma_2 of its size, the dot
        // product within gamma_3 of the sum of its terms' sizes
        let d = x - *q * w;
        let comp = |nc: f64, xc: f64, qc: f64, dc: f64| {
            nc.abs() * (gamma(2) * (xc.abs() + qc.abs() * w) + gamma(3) * dc.abs())
        };
        let rounding =
            comp(n.x, x.x, q.x, d.x) + comp(n.y, x.y, q.y, d.y) + comp(n.z, x.z, q.z, d.z);
        let value = n.dot(d);
        let e = (n_length * err + rounding) * (1.0 + gamma(8));
        coef.push((value, e));
        let (point, err_point) = spline.point(i)?;
        reach = max2(reach, (point - *q).length() + err_point);
        w_max = max2(w_max, w);
    }
    // the derivative's coefficients p (a[i + 1] - a[i]) / (U[i + p + 1] - U[i + 1]) (degree
    // p - 1 on the knots without their first and last)
    let pf = int_to_f64(p);
    let mut slope = Vec::new();
    for i in 0..coef.len().saturating_sub(1) {
        let (&u1, &u2) = (knots.get(i + 1)?, knots.get(i + p + 1)?);
        slope.push(coefficient_difference(&coef, i, pf, u2 - u1)?);
    }
    // eps: |n . v| for the exact v and unit n, at most what the computed one gives plus
    // its rounding, over |v| (less its error)
    let terms = (n.x * v.x).abs() + (n.y * v.y).abs() + (n.z * v.z).abs();
    let n_dot_v = (n.dot(v).abs() + gamma(3) * terms + n_length * err_v) / (v.length() - err_v)
        * (1.0 + gamma(4));
    let eps = (n_length * err_q + (reach + err_q) * n_dot_v) * w_max * (1.0 + gamma(8));
    Some((coef, slope, eps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::hermite_curve::HermiteCurve;
    use crate::geom::solution_array::ObjectKind;
    use crate::geom::vector3d::Vec3;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::nurbs_crv::allocate_nurb_curve;

    /// After a move the solutions are in first-parameter order again, so the merge and
    /// cull passes after the move see each next to its neighbours, whether or not the
    /// refined solutions were sorted before.
    #[test]
    fn moved_contacts_go_back_into_first_parameter_order() {
        let key = |s: &(f64, u8)| s.0;
        // the solution at 0.2 moved to an end at 0.9, past the one at 0.5
        let all = [(0.1, 0), (0.9, 1), (0.5, 2), (1.0, 3)];
        let order = sorted_without(&all, &[], key);
        assert_eq!(order, vec![(0.1, 0), (0.5, 2), (0.9, 1), (1.0, 3)]);
        // equal keys keep their order
        let all = [(0.0, 0), (0.4, 1), (0.6, 2), (0.0, 3)];
        let order = sorted_without(&all, &[], key);
        assert_eq!(order, vec![(0.0, 0), (0.0, 3), (0.4, 1), (0.6, 2)]);
        // unmoved solutions out of order (refinement can swap close ones) are sorted too
        let all = [(0.3, 0), (0.2, 1), (2.0, 2), (0.6, 3)];
        let order = sorted_without(&all, &[], key);
        assert_eq!(order, vec![(0.2, 1), (0.3, 0), (0.6, 3), (2.0, 2)]);
        // a contact named twice is taken out; an index out of range removes nothing
        let order = sorted_without(&all, &[0, 9], key);
        assert_eq!(order, vec![(0.2, 1), (0.6, 3), (2.0, 2)]);
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(u8);

    impl SolutionObject for H {
        fn kind(&self) -> ObjectKind {
            ObjectKind::Curve
        }
    }

    fn polyline(pts: &[[f64; 3]]) -> Curve {
        let n = pts.len() - 1;
        let mut cur = allocate_nurb_curve(n, 1, n + 2).unwrap();
        for (d, s) in cur.cpts_mut().iter_mut().zip(pts) {
            d.x = s[0];
            d.y = s[1];
            d.z = s[2];
            d.w = -f64::MAX;
        }
        let mut u = vec![0.0, 0.0];
        for i in 1..n {
            u.push(i as f64 / n as f64);
        }
        u.extend([1.0, 1.0]);
        cur.knots_mut().copy_from_slice(&u);
        Curve::from(BSplineCurve::from_nurb(cur, 3, false))
    }

    fn opts() -> CurveIntersectOptions<H> {
        CurveIntersectOptions {
            handles: [Some(H(0)), Some(H(1))],
            analytic_handles: [Some(H(2)), Some(H(3))],
            ..CurveIntersectOptions::default()
        }
    }

    fn unit() -> Extent1d {
        Extent1d::from_min_max(0.0, 1.0)
    }

    /// A solution `[t0, t1]` (a point) or `[t0, t1]..[e0, e1]` (a range).
    fn sol(kind: SolutionType, start: [f64; 2], end: [f64; 2]) -> Solution<H> {
        let mut s = Solution::new();
        s.solution_type = kind;
        s.num_variables = 2;
        s.start.parameters[..2].copy_from_slice(&start);
        s.end.parameters[..2].copy_from_slice(&end);
        s
    }

    /// A point contact beside a coincident range is not moved across that range onto the
    /// curve end the range already reaches (where it would be dropped as named twice): the
    /// contact stays where the solve put it, for the later passes to combine with the range.
    /// The offset rows `overlap_pc_extrudeHoles1_f*_k0.37_out_rnd` lost such points (the
    /// curves coincide from the start to the point) and built unsound bodies reported as
    /// success. A lone contact with the same stretch still moves, and so does a chain of
    /// point contacts along one tangential stretch (b27's sliver round): they all land on the
    /// end, and the later ones are dropped as named twice.
    #[test]
    fn a_contact_beside_a_coincident_range_is_not_moved_across_it() {
        let k = Kernel::default();
        // identical over t in [0, 0.5], apart past it
        let a = polyline(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        let b = polyline(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 1.0, 0.0]]);
        let tol = 1e-3;
        let range = sol(SolutionType::RangeOfValues, [0.0, 0.0], [0.48, 0.48]);
        let point = sol(SolutionType::SingleValue, [0.5, 0.5], [0.0, 0.0]);
        let mut sols = SolutionArray::new();
        sols.add(&range);
        sols.add(&point);
        move_contacts_to_curve_ends(&k, &a, &unit(), &b, &unit(), tol, [false; 2], &mut sols)
            .unwrap();
        assert_eq!(sols.len(), 2, "the point is kept");
        let kept = sols.get(1).unwrap();
        assert_eq!(kept.solution_type, SolutionType::SingleValue);
        assert_eq!(kept.start.parameters[..2], [0.5, 0.5], "and not moved");
        assert_eq!(sols.get(0).unwrap().end.parameters[..2], [0.48, 0.48]);

        // the same point alone: its contact runs on to the start of both curves (certified),
        // so it moves there
        let mut alone = SolutionArray::new();
        alone.add(&point);
        move_contacts_to_curve_ends(&k, &a, &unit(), &b, &unit(), tol, [false; 2], &mut alone)
            .unwrap();
        assert_eq!(alone.len(), 1);
        assert_eq!(alone.get(0).unwrap().start.parameters[..2], [0.0, 0.0]);

        // a chain of points on one stretch: all move to the start, one is kept
        let mut chain = SolutionArray::new();
        for t in [0.1, 0.3, 0.5] {
            chain.add(&sol(SolutionType::SingleValue, [t, t], [0.0, 0.0]));
        }
        move_contacts_to_curve_ends(&k, &a, &unit(), &b, &unit(), tol, [false; 2], &mut chain)
            .unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.get(0).unwrap().start.parameters[..2], [0.0, 0.0]);

        // A source end drops onto the bounded target's start, even though the
        // actual oblique crossing is interior to both curves. Its mask applies
        // through this indirect route as well as when moving to a target end.
        let zone = 1e-5;
        let oblique = polyline(&[
            [-3.0 * zone, 0.0, 4.8 * zone],
            [0.0, 0.0, 1.8 * zone],
            [1.9 * zone, 0.0, -0.1 * zone],
        ]);
        let bounded = polyline(&[[0.0, 0.0, 0.0], [0.0, 0.0, 6.0 * zone]]);
        let crossing = sol(SolutionType::SingleValue, [0.5, 0.3], [0.0, 0.0]);
        for (mask, expected) in [([false; 2], [1.0, 0.0]), ([true, false], [0.5, 0.3])] {
            let mut contact = SolutionArray::new();
            contact.add(&crossing);
            move_contacts_to_curve_ends(
                &k,
                &oblique,
                &unit(),
                &bounded,
                &unit(),
                2.0 * zone,
                mask,
                &mut contact,
            )
            .unwrap();
            assert_eq!(contact.len(), 1);
            assert_eq!(contact.get(0).unwrap().start.parameters[..2], expected);
        }

        let crosses = |others: &[Solution<H>], gone: &[usize]| {
            let mut s = SolutionArray::new();
            s.add(&point);
            for o in others {
                s.add(o);
            }
            crosses_a_range(&s, 0, gone, 0, [0.5, 0.0], [0.5, 0.0])
        };
        // a range only touching the stretch's end, or to the kernel's zero, does not block
        let touching = sol(SolutionType::RangeOfValues, [0.5, 0.5], [0.52, 0.52]);
        let at_zero = sol(SolutionType::RangeOfValues, [0.5 - 1e-13, 0.6], [0.7, 0.7]);
        assert!(!crosses(&[touching, at_zero], &[]));
        // nor does a point inside it
        assert!(!crosses(
            &[sol(SolutionType::SingleValue, [0.2, 0.2], [0.0, 0.0])],
            &[]
        ));
        // a range inside the stretch in the drop curve's parameter only blocks it
        let inside_drop = sol(SolutionType::RangeOfValues, [0.9, 0.2], [0.95, 0.25]);
        assert!(crosses(&[inside_drop], &[]));
        // ... unless it is already dropped
        assert!(!crosses(&[inside_drop], &[1]));
        // NaN refuses
        let nan = sol(SolutionType::RangeOfValues, [f64::NAN, 0.9], [0.95, 0.95]);
        assert!(crosses(&[nan], &[]));
    }

    #[test]
    fn helpers() {
        assert!(is_between(0.5, 0.0, 1.0));
        assert!(is_between(0.5, 1.0, 0.0));
        assert!(!is_between(0.0, 0.0, 1.0));
        assert_eq!(min4(3.0, 2.0, 4.0, 1.0), 1.0);
    }

    #[test]
    fn far_apart_curves_have_no_solutions() {
        let k = Kernel::default();
        let a = polyline(&[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]]);
        let b = polyline(&[[0.0, 50.0, 0.0], [1.0, 51.0, 0.0], [2.0, 50.0, 0.0]]);
        match a.global_curve_intersect(&k, &unit(), &b, &unit(), 1e-6, &opts()) {
            Ok(s) => assert!(s.is_empty()),
            Err(e) => assert!(e.is_unported()),
        }
    }

    /// With the memo on, a pair whose caches the memo holds is answered from their trees'
    /// top boxes (#14453), and those are the boxes of fresh builds' trees, bit for bit: a
    /// pair apart gives no solutions, and a crossing pair the solutions of the memo off,
    /// call after call.
    #[test]
    fn memoised_top_boxes_answer_as_fresh_caches() {
        let on = Kernel {
            memoize_curve_caches: true,
            ..Kernel::default()
        };
        let off = Kernel {
            memoize_curve_caches: false,
            ..Kernel::default()
        };
        let a = polyline(&[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]]);
        let far = polyline(&[[0.0, 50.0, 0.0], [1.0, 51.0, 0.0], [2.0, 50.0, 0.0]]);
        let across = polyline(&[[0.0, 0.2, 0.0], [1.0, 0.1, 0.0], [2.0, 0.3, 0.0]]);
        assert!(
            memoized_top_box(&on, &a, None).is_none(),
            "nothing memoised yet"
        );
        for b in [&far, &across] {
            let fresh = a.global_curve_intersect(&off, &unit(), b, &unit(), 1e-6, &opts());
            for _ in 0..3 {
                let memo = a.global_curve_intersect(&on, &unit(), b, &unit(), 1e-6, &opts());
                assert_eq!(format!("{memo:?}"), format!("{fresh:?}"));
            }
            for c in [&a, b] {
                let built = build_cache::<H>(&off, c, Some(H(0)), None).unwrap();
                let tree = built.tree().unwrap();
                let top = tree.node(tree.top_node().unwrap()).unwrap().bbox;
                assert_eq!(memoized_top_box(&on, c, None), Some(top));
            }
        }
        let apart = a.global_curve_intersect(&on, &unit(), &far, &unit(), 1e-6, &opts());
        assert!(apart.unwrap().is_empty());
        assert!(
            memoized_top_box(&off, &a, None).is_none(),
            "the memo off reads nothing"
        );
    }

    /// The cached forms fill a kept slot when the curves' memoised top boxes are apart (the
    /// top-box return is only for the uncached form, whose slots are its own), and give the
    /// uncached solutions.
    #[test]
    fn kept_slots_are_filled_when_the_top_boxes_are_apart() {
        let k = Kernel {
            memoize_curve_caches: true,
            ..Kernel::default()
        };
        let a = polyline(&[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]]);
        let far = polyline(&[[0.0, 50.0, 0.0], [1.0, 51.0, 0.0], [2.0, 50.0, 0.0]]);
        // the uncached form builds and memoises both curves' caches
        let fresh = a.global_curve_intersect(&k, &unit(), &far, &unit(), 1e-6, &opts());
        assert!(memoized_top_box(&k, &a, None).is_some());
        assert!(memoized_top_box(&k, &far, None).is_some());
        let (mut slot_a, mut slot_b) = (None, None);
        let kept = a.global_curve_intersect_cached(
            &k,
            &unit(),
            &far,
            &unit(),
            1e-6,
            &opts(),
            &mut slot_a,
            &mut slot_b,
        );
        assert_eq!(format!("{kept:?}"), format!("{fresh:?}"));
        assert!(kept.unwrap().is_empty());
        assert!(
            slot_a.is_some() && slot_b.is_some(),
            "the kept slots are filled"
        );
    }

    #[test]
    fn crossing_hermites() {
        let k = Kernel::default();
        let a = Curve::from(HermiteCurve::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            3,
        ));
        let b = Curve::from(HermiteCurve::new(
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            3,
        ));
        // reaches the Newton intersection (unit curve.curve_p5) or finds the crossing
        match a.global_curve_intersect(&k, &unit(), &b, &unit(), 1e-6, &opts()) {
            Ok(s) => assert!(!s.is_empty()),
            Err(e) => assert!(e.is_unported()),
        }
    }

    /// Reused cache slots give the solutions of fresh caches, call after call, also
    /// for a self intersection (one slot).
    #[test]
    fn cached_slots_match_fresh_caches() {
        let k = Kernel::default();
        let a = Curve::from(HermiteCurve::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            3,
        ));
        let b = polyline(&[[0.0, 0.2, 0.0], [1.0, 0.1, 0.0], [2.0, 0.3, 0.0]]);
        let fresh = a.global_curve_intersect(&k, &unit(), &b, &unit(), 1e-6, &opts());
        let fresh_self = a.global_curve_intersect(&k, &unit(), &a, &unit(), 1e-6, &opts());
        let (mut slot_a, mut slot_b) = (None, None);
        for _ in 0..3 {
            let cached = a.global_curve_intersect_cached(
                &k,
                &unit(),
                &b,
                &unit(),
                1e-6,
                &opts(),
                &mut slot_a,
                &mut slot_b,
            );
            assert_eq!(format!("{cached:?}"), format!("{fresh:?}"));
            let cached_self = a.global_curve_intersect_cached(
                &k,
                &unit(),
                &a,
                &unit(),
                1e-6,
                &opts(),
                &mut slot_a,
                &mut None,
            );
            assert_eq!(format!("{cached_self:?}"), format!("{fresh_self:?}"));
        }
        if fresh.is_ok() {
            assert!(slot_a.is_some() && slot_b.is_some());
        }
    }

    /// The unnamed solve with reused cache slots gives the solutions of the
    /// uncached unnamed solve, call after call.
    #[test]
    fn unnamed_cached_slots_match_fresh_caches() {
        let k = Kernel::default();
        let a = Curve::from(HermiteCurve::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            3,
        ));
        let b = polyline(&[[0.0, 0.2, 0.0], [1.0, 0.1, 0.0], [2.0, 0.3, 0.0]]);
        let none = CurveIntersectOptions::<H>::default();
        let fresh = a.global_curve_intersect(&k, &unit(), &b, &unit(), 1e-6, &none);
        let (mut slot_a, mut slot_b) = (None, None);
        for _ in 0..3 {
            let cached: Result<SolutionArray<H>> = a.global_curve_intersect_unnamed_cached(
                &k,
                &unit(),
                &b,
                &unit(),
                1e-6,
                false,
                &mut slot_a,
                &mut slot_b,
            );
            assert_eq!(format!("{cached:?}"), format!("{fresh:?}"));
        }
        if fresh.is_ok() {
            assert!(slot_a.is_some() && slot_b.is_some());
        }
    }

    /// A Bezier curve over `[0, 1]`, without weights, from the power-basis coefficients
    /// of each coordinate (`power[j]` multiplies `s^j`): its Bernstein points are
    /// `b_i = sum_{j <= i} C(i, j) / C(n, j) a_j`.
    fn power_bezier(power: &[[f64; 3]]) -> Curve {
        let p = power.len() - 1;
        let choose = |n: usize, k: usize| -> f64 {
            (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
        };
        let mut cur = allocate_nurb_curve(p, p, 2 * p + 1).unwrap();
        for (i, d) in cur.cpts_mut().iter_mut().enumerate() {
            let mut b = [0.0; 3];
            for (j, a) in power.iter().enumerate().take(i + 1) {
                for c in 0..3 {
                    b[c] += choose(i, j) / choose(p, j) * a[c];
                }
            }
            d.x = b[0];
            d.y = b[1];
            d.z = b[2];
            d.w = -f64::MAX;
        }
        let mut u = vec![0.0; p + 1];
        u.extend(vec![1.0; p + 1]);
        cur.knots_mut().copy_from_slice(&u);
        Curve::from(BSplineCurve::from_nurb(cur, 3, false))
    }

    /// The planar parabola `(s, s², 0)` over `[0, 1]`.
    fn floor_parabola() -> Curve {
        power_bezier(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
    }

    /// Review r3 (M1): two meetings within the meeting zero, not exact. The curve
    /// `(s, s² + X² - δ², e)` (`X = s - 1/2`, `δ = 1/8`, `e = 5e-13`) crosses over the
    /// floor parabola `(s, s², 0)` in the plane at X = ±δ, `e` above it, within the
    /// scaled zero (`EFF_ZERO (1 + size)`, 2e-12 here) there: both are meetings to the
    /// touch's test. The subdivision drops a pair of pieces only when their hulls are
    /// apart by more than that zero, so the pairs at both meetings stay and the curves
    /// are not certified to meet at most once (dropping on any positive margin, every
    /// pair went along z, and "they do not meet" was certified).
    #[test]
    fn two_meetings_within_the_meeting_zero_are_not_at_most_once() {
        let (delta, e) = (0.125, 5e-13);
        // s² + (s² - s + 1/4) - δ²
        let above_it = power_bezier(&[
            [0.0, 0.25 - delta * delta, e],
            [1.0, -1.0, 0.0],
            [0.0, 2.0, 0.0],
        ]);
        let floor = floor_parabola();
        let k = Kernel::default();
        for s in [0.5 - delta, 0.5 + delta] {
            let (p, q) = (
                above_it.evaluate_point(&k, s).unwrap(),
                floor.evaluate_point(&k, s).unwrap(),
            );
            let zero = EFF_ZERO * (1.0 + max2(p.max_dimension(), q.max_dimension()));
            assert!(p.distance(q) <= zero, "{s}: {p:?} {q:?}");
            assert!(p.distance(q) >= 0.5 * e, "{s}: {p:?} {q:?}");
        }
        let whole = (0.0, 1.0);
        assert!(!curves_meet_at_most_once(
            &k, &above_it, whole, &floor, whole
        ));
        assert!(!curves_meet_at_most_once(
            &k, &floor, whole, &above_it, whole
        ));
    }

    /// Review r7 (M2): a line and a parabola `(s, dr + c (s - 1/2)², 0)` that never
    /// meet exactly (`dr` above the line) but stay within the scaled zero of it over the
    /// whole stretch (`c` tiny): a near overlap at the zero, not a touch. The convex
    /// offset puts the (empty) set of exact meetings in one interval, so without a bound
    /// on the run within the zero this was certified the stretch's only contact. The same
    /// offset with a parabola that does bend away (`c = 1`) is a touch within the zero
    /// and stays certified.
    #[test]
    fn a_run_within_the_zero_over_the_stretch_is_not_a_lone_touch() {
        let k = Kernel::default();
        let line = polyline(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        // (s, dr + c (s² - s + 1/4))
        let parabola = |dr: f64, c: f64| {
            power_bezier(&[[0.0, dr + 0.25 * c, 0.0], [1.0, -c, 0.0], [0.0, c, 0.0]])
        };
        let whole = (0.0, 1.0);
        let flat = parabola(5e-13, 1e-12);
        for s in [0.0, 0.5, 1.0] {
            let p = flat.evaluate_point(&k, s).unwrap();
            let q = line.evaluate_point(&k, s).unwrap();
            let zero = EFF_ZERO * (1.0 + max2(p.max_dimension(), q.max_dimension()));
            assert!(p.distance(q) <= zero, "{s}: {p:?} {q:?}");
            assert!(p.distance(q) > 0.0, "{s}: {p:?} {q:?}");
        }
        assert!(!touch_is_alone(&k, &line, whole, &flat, whole, [0.5, 0.5], 1e-5).unwrap());
        assert!(!touch_is_alone(&k, &flat, whole, &line, whole, [0.5, 0.5], 1e-5).unwrap());
        let bent = parabola(1e-13, 1.0);
        assert!(touch_is_alone(&k, &line, whole, &bent, whole, [0.5, 0.5], 1e-5).unwrap());
        assert!(touch_is_alone(&k, &bent, whole, &line, whole, [0.5, 0.5], 1e-5).unwrap());
    }

    /// #18828: the foot's inner side face cuts the plate's corner cylinder (radius 2 about
    /// x = -3, z = 8.3, along y) in an ellipse whose top, P = (-3, -15.817157, 10.3),
    /// touches the foot's top edge, the line along the side face at z = 10.3. In the
    /// Boolean the general solve stopped on both flanks of that double root, 2.68e-5 and
    /// 2.44e-5 from P along the line, where the curves are about 3e-11 apart, and the two
    /// point solutions became a 2.68e-5 edge from a vertex back to itself. Those two flank
    /// points are the one touch at P, either way round, and so are three points left on
    /// that stretch. A line 1e-3 below the top cuts the ellipse at two crossings 0.30
    /// apart, which stay two; so do the flank points without a contact tolerance when the
    /// line runs a hair inside the top.
    #[test]
    fn flank_points_of_a_line_tangent_to_an_ellipse_are_its_touch() {
        use crate::curve::ellipse::{Ellipse, EllipseOptions};
        use crate::curve::line::Line;
        let k = Kernel::default();
        let (a, b) = (
            Vec3::new(-3.0565447957777483, -15.938417685876269, 10.3),
            Vec3::new(-1.5555361814781503, -12.71949432476632, 10.3),
        );
        let d = (b - a).unitize_or_z();
        let centre = Vec3::new(-3.0, -15.817156980012088, 8.3);
        let ellipse = Curve::from(
            Ellipse::new(
                &k,
                centre,
                d,
                Vec3::Z,
                Extent1d::from_min_max(60.0, 120.0),
                2.0 / d.x,
                2.0,
                &EllipseOptions::default(),
            )
            .unwrap(),
        );
        let top = centre + 2.0 * Vec3::Z;
        let e_ivl = ellipse.natural_interval();
        // the points of the line `(a, b)` at `along` from P, dropped onto the ellipse: a
        // solution `[ellipse, line]` as the general solve gives it
        let solution = |line: &Curve, at: Vec3| {
            let dropped = ellipse
                .drop_point(&k, &e_ivl, at, None, 1e-3, None, SolverOperation::Minimize)
                .unwrap();
            let on_line = line
                .drop_point(
                    &k,
                    &line.natural_interval(),
                    at,
                    None,
                    1e-3,
                    None,
                    SolverOperation::Minimize,
                )
                .unwrap();
            let p = ellipse.evaluate_point(&k, dropped.parameter).unwrap();
            let q = line.evaluate_point(&k, on_line.parameter).unwrap();
            let mut s = sol(
                SolutionType::SingleValue,
                [dropped.parameter, on_line.parameter],
                [0.0, 0.0],
            );
            s.start.solution_value = p.distance(q);
            s
        };
        let run = |line: &Curve, flanks: &[Vec3], contact: f64, swapped: bool| {
            let mut sols = SolutionArray::new();
            for &f in flanks {
                let mut s = solution(line, f);
                if swapped {
                    s.start.parameters.swap(0, 1);
                }
                sols.add(&s);
            }
            let (this, other, ivl, other_ivl) = if swapped {
                (line, &ellipse, line.natural_interval(), e_ivl)
            } else {
                (&ellipse, line, e_ivl, line.natural_interval())
            };
            this.flank_points_of_an_ellipse_touch(
                &k, &mut sols, other, &ivl, &other_ivl, 2e-5, contact,
            )
            .unwrap();
            let e_at = usize::from(swapped);
            sols.as_slice()
                .iter()
                .map(|s| {
                    ellipse
                        .evaluate_point(&k, s.start.param(e_at).unwrap())
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        let tangent = Curve::from(Line::from_points(a, b, 3));
        let flanks = [top - 2.68e-5 * d, top + 2.44e-5 * d];
        // (a third point on the stretch, on either side of the touch, joins it as well,
        // with or without a contact tolerance: the tangent line does not enter the ellipse)
        let thirds = [
            [flanks[0], top - 1.1e-5 * d, flanks[1]],
            [flanks[0], top + 0.9e-5 * d, flanks[1]],
        ];
        for swapped in [false, true] {
            for contact in [1e-5, 0.0] {
                let mut runs = vec![flanks.to_vec()];
                runs.extend(thirds.iter().map(|t| t.to_vec()));
                for points_in in runs {
                    let points = run(&tangent, &points_in, contact, swapped);
                    assert_eq!(points.len(), 1, "swapped {swapped}: {points:?}");
                    assert!(points[0].distance(top) < 1e-9, "{points:?} vs {top:?}");
                }
            }
        }
        // two crossings of a line 1e-3 below the top: x - x0 = ±√(2 · 1e-3 · ρ) with the
        // radius of curvature ρ = (2 / d.x)² / 2 at the top
        let below = Vec3::new(0.0, 0.0, -1e-3);
        let cut = Curve::from(Line::from_points(a + below, b + below, 3));
        let half = (2.0 * 1e-3 * (2.0 / d.x).powi(2) / 2.0).sqrt();
        let crossings = [top + below - half * d, top + below + half * d];
        assert_eq!(run(&cut, &crossings, 1e-5, false).len(), 2);
        // a hair (1e-9) inside the top, with no contact tolerance: the circle rule's zero
        // keeps upstream's two points
        let inside = Vec3::new(0.0, 0.0, -1e-9);
        let hair = Curve::from(Line::from_points(a + inside, b + inside, 3));
        assert_eq!(run(&hair, &flanks.map(|f| f + inside), 0.0, false).len(), 2);
        // and through the intersector, with the Boolean's tolerances and the ellipse as the
        // join built it (the side face's section of the corner cylinder: 3.4 degrees about
        // the top, its y axis down), for which the general solve returns the two flank
        // points 2.68e-5 and 2.44e-5 from P: one touch at P, either way round
        let section = Curve::from(
            Ellipse::new(
                &k,
                centre,
                Vec3::new(0.42261826174069966, 0.9063077870366499, 0.0),
                -Vec3::Z,
                Extent1d::from_min_max(268.379895042679, 271.79999999999984),
                4.732403166304994,
                2.0,
                &EllipseOptions::default(),
            )
            .unwrap(),
        );
        for (this, other, e_at) in [(&section, &tangent, 0), (&tangent, &section, 1)] {
            let sols = this
                .global_curve_intersect_cached_with_zone_tols(
                    &k,
                    &this.natural_interval(),
                    other,
                    &other.natural_interval(),
                    2e-5,
                    &opts(),
                    &mut None,
                    &mut None,
                    [None, None],
                    1e-5,
                    [false; 2],
                )
                .unwrap();
            let points: Vec<Vec3> = sols
                .as_slice()
                .iter()
                .map(|s| {
                    section
                        .evaluate_point(&k, s.start.param(e_at).unwrap())
                        .unwrap()
                })
                .collect();
            assert_eq!(points.len(), 1, "ellipse at {e_at}: {points:?}");
            assert!(points[0].distance(top) < 1e-9, "{points:?} vs {top:?}");
        }
    }
}
