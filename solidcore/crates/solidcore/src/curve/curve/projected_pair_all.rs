//! Complete isolated projected maxima for two bounded affine curve pieces.
//!
//! Not upstream. Convexity places every isolated constrained maximum at a corner;
//! rounded inward derivatives classify those corners and rounded values certify
//! the maximum to their own arithmetic enclosure. Existing `GlobalSolve` applies
//! All retention and ordered duplicate rules. Shares Single definition primitives.
//! Deviations: one affine piece per operand only; uncertain classification,
//! continuous maxima and curved/multi-piece queries refuse. Missing handles
//! adapt through exactly one recursive call with concrete operand stand-ins
//! (recursion depth 1: the stand-in call has every handle and does not recurse).

use super::projected_pair::{
    bounded_domain, dot, norm_squared, project, sub, Candidate, Definition, Piece, Query,
};
use super::{Curve, CurveKind};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{
    GlobalSolve, GlobalSolver, PreferredSolution, SolutionPairType, SolutionRequested,
    SolverOperation,
};
use crate::geom::solution_array::{
    OperandObject, ParameterDomains, Solution, SolutionArray, SolutionObject, SolutionType, SortKey,
};
use crate::kernel::Kernel;
use crate::math::Vec3;

const OP: &str = "curve::curve::projected_pair_all";

/// Select a structural class before attempting any numerical certificate.
/// Failure of a selected query must propagate, never invoke the legacy solve.
pub(super) fn supports(curves: [&Curve; 2], intervals: [Extent1d; 2]) -> bool {
    curves.into_iter().zip(intervals).all(|(curve, requested)| {
        let natural = curve.natural_interval();
        let lo = natural.min().max(requested.min());
        let hi = natural.max().min(requested.max());
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return false;
        }
        match curve.kind() {
            CurveKind::Line(_) => true,
            CurveKind::BSpline(bs) => {
                let Some(net) = bs.gw_nurb() else {
                    return false;
                };
                if net.degree() != 1 {
                    return false;
                }
                let mut weights = net.cpts().iter().map(|cp| {
                    if cp.w == crate::nurbs::math::NOW {
                        1.0
                    } else {
                        cp.w
                    }
                });
                if !weights
                    .next()
                    .is_some_and(|first| weights.all(|w| w == first))
                {
                    return false;
                }
                let Some(count) = net.cpts().len().checked_sub(1) else {
                    return false;
                };
                // Keep nonclamped endpoint conventions outside this initial
                // structural class, even if their open span is affine.
                if !net
                    .knots()
                    .first()
                    .zip(net.knots().get(1))
                    .is_some_and(|(a, b)| a == b)
                    || !net
                        .knots()
                        .last()
                        .zip(net.knots().get(net.cpts().len()))
                        .is_some_and(|(a, b)| a == b)
                {
                    return false;
                }
                let spans = || net.knots().windows(2).skip(1).take(count);
                if !spans().all(|ends| ends.first().zip(ends.last()).is_some_and(|(a, b)| a < b)) {
                    return false;
                }
                // Count even an endpoint-only adjacent span: default evaluation
                // at a knot must not silently choose a different piece.
                spans()
                    .filter(|ends| {
                        ends.first()
                            .zip(ends.last())
                            .is_some_and(|(a, b)| lo.max(*a) <= hi.min(*b))
                    })
                    .count()
                    == 1
            }
            _ => false,
        }
    })
}

fn refused(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

struct AllSolutions<'a, O> {
    base: GlobalSolver<'a, O>,
    curves: [&'a Curve; 2],
    handles: [Option<O>; 2],
}

impl<O: SolutionObject> ParameterDomains<O> for AllSolutions<'_, O> {
    fn natural_interval(&self, handle: &O) -> Option<Extent1d> {
        handle.natural_interval().or_else(|| {
            self.handles
                .iter()
                .zip(self.curves)
                .find_map(|(candidate, curve)| {
                    (*candidate == Some(*handle)).then(|| curve.natural_interval())
                })
        })
    }
}

impl<'a, O: SolutionObject + 'a> GlobalSolve<'a, O> for AllSolutions<'a, O> {
    fn solver(&self) -> &GlobalSolver<'a, O> {
        &self.base
    }
    fn solver_mut(&mut self) -> &mut GlobalSolver<'a, O> {
        &mut self.base
    }
    fn identical_solutions(
        &self,
        a: &Solution<O>,
        b: &Solution<O>,
    ) -> Result<(SolutionPairType, Option<PreferredSolution>)> {
        self.base.identical_solutions_in(a, b, self)
    }
}

/// Require one positive-length continuous affine span. NURBS inputs must
/// have degree one, equal weights and no repeated active knot boundary.
fn affine_piece(definition: &Definition<'_>, domain: Extent1d) -> Result<Piece> {
    let affine = match definition {
        Definition::Line(_) => true,
        Definition::Nurbs(net) if net.p == 1 => {
            // Repeated active knots can expose a different default endpoint
            // value from the affine one-sided limit. Convexity needs continuous
            // endpoint values, not just affine behavior in the open span.
            if !net
                .u
                .windows(2)
                .skip(net.p)
                .take(net.n - net.p)
                .all(|ends| ends.first().zip(ends.last()).is_some_and(|(a, b)| a < b))
            {
                return Err(refused(
                    "All affine enumeration requires continuous knot endpoints",
                ));
            }
            let mut weights = net.pw.chunks_exact(4).filter_map(|cp| cp.get(3));
            weights
                .next()
                .is_some_and(|first| weights.all(|w| w == first))
        }
        _ => false,
    };
    if !affine {
        return Err(refused(
            "All enumeration requires a proven affine definition",
        ));
    }
    let pieces = definition.pieces(domain)?;
    match pieces.as_slice() {
        [piece] if piece.range.lo < piece.range.hi => Ok(*piece),
        _ => Err(refused(
            "All enumeration requires one nonempty affine piece per operand",
        )),
    }
}

/// Classify a constrained corner by strict inward objective derivative signs.
/// An increasing inward direction excludes it; unresolved signs refuse.
fn corner_maximum(query: &Query<'_>, pieces: [Piece; 2], s: f64, t: f64) -> Result<bool> {
    let [a, b] = &query.definitions;
    let [pa, pb] = pieces;
    let (va, da) = a.jet(Ivl::from(s), pa.knot)?;
    let (vb, db) = b.jet(Ivl::from(t), pb.knot)?;
    let n = V3::from_vec3(query.normal);
    let delta = project(sub(va, vb), n);
    let ds = dot(delta, project(da, n)) * Ivl::from(if s == pa.range.lo { 2.0 } else { -2.0 });
    let dt = dot(delta, project(db, n)) * Ivl::from(if t == pb.range.lo { -2.0 } else { 2.0 });
    if ds.lo > 0.0 || dt.lo > 0.0 {
        return Ok(false);
    }
    if ds.hi < 0.0 && dt.hi < 0.0 {
        return Ok(true);
    }
    Err(refused(
        "corner inward derivatives do not isolate a constrained maximum",
    ))
}

/// Enumerate all isolated near-best maxima for a certified affine rectangle.
pub(super) fn solve<O: SolutionObject>(
    kernel: &Kernel,
    curves: [&Curve; 2],
    intervals: [Extent1d; 2],
    normal: Vec3,
    tolerance: f64,
    handles: [Option<O>; 2],
    target: Option<f64>,
) -> Result<SolutionArray<O>> {
    kernel.check_cancelled(OP)?;
    if !(tolerance.is_finite()
        && tolerance >= 0.0
        && normal.x.is_finite()
        && normal.y.is_finite()
        && normal.z.is_finite())
    {
        return Err(refused("All maximum has no finite error budget"));
    }
    if target.is_some_and(|value| !value.is_finite()) {
        return Err(refused("All maximum incumbent must be finite"));
    }
    let [ca, cb] = curves;
    let [ia, ib] = intervals;
    let da = bounded_domain(ca, ia)?;
    let db = bounded_domain(cb, ib)?;
    let a = Definition::of(ca, da)?;
    let b = Definition::of(cb, db)?;
    let pa = affine_piece(&a, da)?;
    let pb = affine_piece(&b, db)?;
    let query = Query {
        kernel,
        curves,
        definitions: [a, b],
        normal,
    };
    // Strict convexity separately in each active parameter excludes interior
    // and open-edge maxima. Zero projected derivatives can produce continua.
    for (definition, piece) in query.definitions.iter().zip([pa, pb]) {
        let (_, derivative) = definition.jet(piece.range, piece.knot)?;
        if norm_squared(project(derivative, V3::from_vec3(normal))).lo <= 0.0 {
            return Err(refused(
                "projected affine direction is not certified nonzero",
            ));
        }
    }
    construct(&query, [pa, pb], tolerance, handles, target, corner_maximum)
}

/// Build and retain the complete corner set after the caller has proved that
/// no non-corner maximum exists. The classifier must be sound for that class.
pub(super) fn construct<O: SolutionObject>(
    query: &Query<'_>,
    pieces: [Piece; 2],
    tolerance: f64,
    handles: [Option<O>; 2],
    target: Option<f64>,
    classify: fn(&Query<'_>, [Piece; 2], f64, f64) -> Result<bool>,
) -> Result<SolutionArray<O>> {
    let [pa, pb] = pieces;
    let mut corners: Vec<Candidate> = Vec::new();
    let mut lower = 0.0_f64;
    for s in [pa.range.lo, pa.range.hi] {
        for t in [pb.range.lo, pb.range.hi] {
            let candidate = query.candidate(s, t)?;
            lower = lower.max(candidate.lower);
            corners.push(candidate);
        }
    }
    // The optional target seeds the incumbent just as in GlobalSolve; it is
    // a cutoff, not a claim that this rectangle attains that value.
    let incumbent = target.map_or(0.0, f64::abs);
    let lower = lower.max(incumbent);

    let exclude = (Ivl::from(lower) - Ivl::from(tolerance)).lo;
    let mut retained = Vec::new();
    for candidate in corners {
        if candidate.upper < exclude {
            continue;
        }
        let [s, t] = candidate.parameters;
        if !classify(query, [pa, pb], s, t)? {
            continue;
        }
        // Classification proves this exact parameter corner is a constrained
        // maximum. Construction has no geometric optimization error to absorb;
        // the existing accumulator below applies reported-value All retention.
        if let Some(target) = target {
            let cutoff = Ivl::from(target.abs()) - Ivl::from(tolerance);
            if candidate.upper < cutoff.lo {
                continue;
            }
            if candidate.lower < cutoff.hi {
                return Err(refused(
                    "maximum enclosure straddles the external incumbent cutoff",
                ));
            }
        }
        retained.push(candidate);
    }

    accumulate(query, retained, tolerance, handles)
}

/// Preserve public All retention and ordered-parameter duplicate semantics.
/// Every input must already be a certified isolated maximum representative.
pub(super) fn accumulate<O: SolutionObject>(
    query: &Query<'_>,
    mut retained: Vec<Candidate>,
    tolerance: f64,
    handles: [Option<O>; 2],
) -> Result<SolutionArray<O>> {
    // Exactly one adaptation call supplies domains for duplicate comparison.
    if handles == [None, None] {
        let [a, b] = query.curves;
        return Ok(accumulate(
            query,
            retained,
            tolerance,
            [
                Some(OperandObject::Curve(0, a.natural_interval())),
                Some(OperandObject::Curve(1, b.natural_interval())),
            ],
        )?
        .without_objects());
    }
    let required_upper = retained
        .iter()
        .map(|candidate| candidate.upper)
        .fold(0.0_f64, f64::max);
    let best_lower = retained
        .iter()
        .map(|candidate| candidate.lower)
        .fold(0.0_f64, f64::max);
    let exclusion_cutoff = (Ivl::from(best_lower) - Ivl::from(tolerance)).lo;
    retained.sort_by(|a, b| b.reported.total_cmp(&a.reported));
    let mut accumulation = AllSolutions {
        base: GlobalSolver::new(),
        curves: query.curves,
        handles,
    };
    accumulation.base.operation = SolverOperation::ProjectedMaximize;
    accumulation.base.solution_requested = SolutionRequested::All;
    accumulation.base.tolerance_3d = tolerance;
    accumulation.base.best_answer_so_far_sq = 0.0;
    accumulation.base.fixes = query.kernel.fixes;
    let mut constructed = Vec::new();
    for candidate in retained {
        let mut answer = Solution::new();
        answer.num_objects = 2;
        for (slot, handle) in answer.objects.iter_mut().zip(handles.iter()) {
            *slot = *handle;
        }
        answer.num_variables = 2;
        answer.solution_type = SolutionType::SingleValue;
        let [s, t] = candidate.parameters;
        *answer.start.param_mut(0)? = s;
        *answer.start.param_mut(1)? = t;
        answer.start.solution_value = candidate.reported;
        accumulation.add_sorted_solution(&answer, SortKey::BySolutionValue, false)?;
        constructed.push((candidate, answer));
    }
    let mut returned_upper = 0.0_f64;
    for answer in accumulation.base.solutions.as_slice() {
        let candidate = query.candidate(answer.start.param(0)?, answer.start.param(1)?)?;
        returned_upper = returned_upper.max(candidate.upper);
    }
    // Reported-value sorting may discard the candidate with the largest true
    // objective enclosure. A discarded point's rounding cannot certify output.
    if !constructed.is_empty() && returned_upper < required_upper {
        return Err(refused(
            "All retention discarded the global objective enclosure",
        ));
    }
    for (candidate, answer) in constructed {
        if candidate.upper < exclusion_cutoff {
            continue;
        }
        let mut represented = false;
        for returned in accumulation.base.solutions.as_slice() {
            if accumulation.identical_solutions(&answer, returned)?.0 == SolutionPairType::Identical
            {
                represented = true;
                break;
            }
        }
        // Global coverage alone cannot hide a different qualifying extremum.
        // Preserve existing duplicate equivalence; otherwise a dropped candidate
        // must be proved outside the band, not merely lower after f64 sorting.
        if !represented {
            return Err(refused(
                "All retention has an unresolved maximum-band ordering",
            ));
        }
    }
    Ok(accumulation.base.solutions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::solution_array::ObjectKind;
    use crate::nurbs::b_spline_curve::BSplineCurve;

    #[derive(Clone, Copy, PartialEq)]
    struct Handle;
    impl SolutionObject for Handle {
        fn kind(&self) -> ObjectKind {
            ObjectKind::Curve
        }
    }
    fn segment(a: [f64; 3], b: [f64; 3]) -> Curve {
        Curve::from(
            BSplineCurve::create_line_segment(3, Vec3::from_array(a), Vec3::from_array(b)).unwrap(),
        )
    }
    fn run(a: &Curve, b: &Curve, tolerance: f64) -> Result<SolutionArray<Handle>> {
        solve(
            &Kernel::default(),
            [a, b],
            [a.natural_interval(), b.natural_interval()],
            Vec3::new(0.0, 0.0, 1.0),
            tolerance,
            [None, None],
            None,
        )
    }
    #[test]
    fn all_retains_both_isolated_equal_maxima() {
        let a = segment([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let b = segment([-0.5, 0.5, 0.0], [0.5, 0.5, 0.0]);
        let answers = run(&a, &b, 1e-8).unwrap();
        assert_eq!(answers.len(), 2);
        for answer in answers.as_slice() {
            assert!((answer.start.solution_value - 2.5_f64.sqrt()).abs() < 1e-8);
        }
        assert_ne!(
            answers.as_slice()[0].start.parameters,
            answers.as_slice()[1].start.parameters
        );
    }
    #[test]
    fn all_refuses_unresolved_reported_value_ordering_without_borrowing_rounding() {
        let a = segment([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let b = segment([-0.5, 0.5, 0.0], [0.5 + 4.0 * f64::EPSILON, 0.5, 0.0]);
        // The two exact constrained extrema differ by only a few ulps. A
        // reported-value winner does not prove the other is outside zero slack
        // when their definition intervals still overlap.
        assert!(matches!(
            run(&a, &b, 0.0),
            Err(SolidError::Unsupported { .. })
        ));
        assert_eq!(run(&a, &b, 1e-8).unwrap().len(), 2);
    }

    #[test]
    fn all_zero_retention_slack_accepts_its_own_arithmetic_enclosure() {
        let a = segment([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let b = segment([-0.5, 0.5, 0.0], [0.5, 0.5, 0.0]);
        let result = run(&a, &b, 0.0).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.as_slice()[0].start.solution_value, 2.5_f64.sqrt());
    }
    #[test]
    fn structural_selector_excludes_unowned_geometric_classes() {
        use crate::nurbs::math::{CPoint, NOW};
        let line = segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let points = [
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 1.0, 0.0, NOW),
            CPoint::new(2.0, 0.0, 0.0, NOW),
        ];
        let polyline = Curve::from(
            BSplineCurve::from_data(3, 2, 4, 1, &points, &[0.0, 0.0, 0.5, 1.0, 1.0]).unwrap(),
        );
        let quadratic = Curve::from(
            BSplineCurve::from_data(3, 2, 5, 2, &points, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap(),
        );
        let weighted = Curve::from(
            BSplineCurve::from_data(
                3,
                1,
                3,
                1,
                &[points[0], CPoint::new(1.0, 1.0, 0.0, 2.0)],
                &[0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
        );
        let whole = Extent1d::from_min_max(0.0, 1.0);
        assert!(supports([&line, &line], [whole, whole]));
        assert!(!supports([&polyline, &line], [whole, whole]));
        assert!(supports(
            [&polyline, &line],
            [Extent1d::from_min_max(0.1, 0.4), whole]
        ));
        assert!(!supports([&weighted, &line], [whole, whole]));
        assert!(!supports([&quadratic, &line], [whole, whole]));
    }

    #[test]
    fn all_preserves_existing_ordered_parameter_duplicate_rule() {
        let a = segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let small = Extent1d::from_min_max(0.3, 0.30001);
        let result = solve::<Handle>(
            &Kernel::default(),
            [&a, &a],
            [small, small],
            Vec3::new(0.0, 0.0, 1.0),
            1e-8,
            [None, None],
            None,
        )
        .unwrap();
        // Ordered endpoints differ, but by less than the existing 1e-4 of
        // the natural parameter domain. This is a duplicate, not a new root.
        assert_eq!(result.len(), 1);
    }
    #[test]
    fn all_band_retains_or_excludes_second_maximum_by_caller_tolerance() {
        let a = segment([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let b = segment([-0.4, 0.5, 0.0], [0.6, 0.5, 0.0]);
        assert_eq!(run(&a, &b, 0.01).unwrap().len(), 1);
        assert_eq!(run(&a, &b, 0.25).unwrap().len(), 2);
    }
    #[test]
    fn all_refuses_a_continuous_projected_maximum_set() {
        let a = segment([0.0, 0.0, -1.0], [0.0, 0.0, 1.0]);
        let b = segment([1.0, 0.0, -1.0], [1.0, 0.0, 1.0]);
        assert!(matches!(
            run(&a, &b, 1e-8),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn all_applies_an_external_incumbent_without_blanket_refusal() {
        let a = segment([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let b = segment([-0.4, 0.5, 0.0], [0.6, 0.5, 0.0]);
        let solve_target = |target| {
            solve::<Handle>(
                &Kernel::default(),
                [&a, &b],
                [a.natural_interval(), b.natural_interval()],
                Vec3::new(0.0, 0.0, 1.0),
                0.25,
                [None, None],
                Some(target),
            )
        };
        assert_eq!(solve_target(1.8).unwrap().len(), 1);
        assert!(solve_target(2.0).unwrap().is_empty());
        assert_eq!(solve_target(1.0).unwrap().len(), 2);
    }
}

#[cfg(test)]
#[path = "projected_pair_all_fixtures.rs"]
mod original_fixtures;
