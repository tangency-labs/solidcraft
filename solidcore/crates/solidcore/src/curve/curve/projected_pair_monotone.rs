//! Complete projected maxima for identical coordinate-monotone NURBS definitions.
//!
//! Not upstream. If each projected coordinate is monotone with a fixed sign,
//! increasing the separation of ordered parameters increases every coordinate
//! difference. Thus only opposite rectangle corners can be constrained maxima.
//! The proof uses whole-interval derivative enclosures, never sampled signs.
//! Deviations: one continuous union knot span; at most 4,096 interval visits and
//! depth 20 for sign certification. Uncertain signs or nonisolated sets refuse.

use super::projected_pair::{bounded_domain, project, Definition, Piece, Query};
use super::{Curve, CurveKind};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{SolutionArray, SolutionObject};
use crate::kernel::Kernel;
use crate::math::Vec3;

const OP: &str = "curve::curve::projected_pair_monotone";
const MAX_VISITS: usize = 4_096;
const MAX_DEPTH: usize = 20;

fn refused(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// Select identical native definitions and a single continuous union span.
/// This is a structural boundary, not a fallback after a failed sign proof.
pub(super) fn supports(curves: [&Curve; 2], intervals: [Extent1d; 2]) -> bool {
    let [a, b] = curves;
    let (CurveKind::BSpline(a), CurveKind::BSpline(b)) = (a.kind(), b.kind()) else {
        return false;
    };
    let (Some(a), Some(b)) = (a.gw_nurb(), b.gw_nurb()) else {
        return false;
    };
    if a.degree() == 0 || a.degree() != b.degree() || a.knots() != b.knots() || a.cpts() != b.cpts()
    {
        return false;
    }
    let p = a.degree();
    let n = a.cpts().len();
    let Some(clamp_count) = p.checked_add(1) else {
        return false;
    };
    let Some(active) = n.checked_sub(p) else {
        return false;
    };
    let Some(&start) = a.knots().get(p) else {
        return false;
    };
    let Some(&end) = a.knots().get(n) else {
        return false;
    };
    if !a.knots().iter().take(clamp_count).all(|t| *t == start)
        || !a.knots().iter().skip(n).all(|t| *t == end)
    {
        return false;
    }
    let spans = || a.knots().windows(2).skip(p).take(active);
    if !spans().all(|ends| ends.first().zip(ends.last()).is_some_and(|(a, b)| a < b)) {
        return false;
    }
    let clipped = intervals.map(|ivl| (start.max(ivl.min()), end.min(ivl.max())));
    if clipped
        .iter()
        .any(|(lo, hi)| !(lo.is_finite() && hi.is_finite() && lo < hi))
    {
        return false;
    }
    let [(alo, ahi), (blo, bhi)] = clipped;
    let lo = alo.min(blo);
    let hi = ahi.max(bhi);
    spans()
        .filter(|ends| {
            ends.first()
                .zip(ends.last())
                .is_some_and(|(a, b)| lo.max(*a) <= hi.min(*b))
        })
        .count()
        == 1
}

/// Require one positive-length native knot piece for the interval.
fn one_piece(definition: &Definition<'_>, interval: Extent1d) -> Result<Piece> {
    let pieces = definition.pieces(interval)?;
    match pieces.as_slice() {
        [piece] if piece.range.lo < piece.range.hi => Ok(*piece),
        _ => Err(refused(
            "monotone enumeration needs one continuous union knot span",
        )),
    }
}

/// Prove identical native NURBS definitions and fixed nonzero derivative signs
/// for every nonvanishing projected coordinate over the interval union.
/// Unresolved signs refuse when the subdivision depth or visit budget is spent.
fn certify(query: &Query<'_>, domains: [Extent1d; 2]) -> Result<[Piece; 2]> {
    let [a, b] = &query.definitions;
    let (Definition::Nurbs(na), Definition::Nurbs(nb)) = (a, b) else {
        return Err(refused(
            "monotone enumeration needs identical NURBS definitions",
        ));
    };
    if na.p != nb.p || na.n != nb.n || na.u != nb.u || na.pw != nb.pw {
        return Err(refused("the two exact NURBS definitions differ"));
    }
    let [da, db] = domains;
    let union = Extent1d::from_min_max(da.min().min(db.min()), da.max().max(db.max()));
    let union_piece = one_piece(a, union)?;
    let normal = query.normal;
    if normal == Vec3::ZERO {
        return Err(refused("zero projection has a nonisolated maximum set"));
    }
    // A row of n cross (v cross n) is identically zero when n is parallel to
    // that coordinate axis. This exact algebraic zero needs no numerical sign.
    let zero_rows = [
        normal.y == 0.0 && normal.z == 0.0,
        normal.x == 0.0 && normal.z == 0.0,
        normal.x == 0.0 && normal.y == 0.0,
    ];
    let mut signs = [0_i8; 3];
    let mut pending = vec![(union_piece.range, 0_usize)];
    let mut visits = 0;
    while let Some((range, depth)) = pending.pop() {
        query.kernel.check_cancelled(OP)?;
        visits += 1;
        if visits > MAX_VISITS {
            return Err(refused("monotone sign proof exhausted its interval budget"));
        }
        let (_, derivative) = a.jet(range, union_piece.knot)?;
        let projected = project(derivative, V3::from_vec3(normal));
        let mut unresolved = false;
        for ((enclosure, zero), sign) in [projected.x, projected.y, projected.z]
            .into_iter()
            .zip(zero_rows)
            .zip(signs.iter_mut())
        {
            if zero {
                continue;
            }
            if !(enclosure.lo.is_finite() && enclosure.hi.is_finite()) {
                return Err(refused("projected derivative has no finite enclosure"));
            }
            let direction = if enclosure.lo > 0.0 {
                1
            } else if enclosure.hi < 0.0 {
                -1
            } else {
                0
            };
            if direction == 0 {
                unresolved = true;
            } else if *sign != 0 && *sign != direction {
                return Err(refused("a projected coordinate derivative changes sign"));
            } else {
                *sign = direction;
            }
        }
        if unresolved {
            let mid = range.mid();
            if depth == MAX_DEPTH || !(range.lo < mid && mid < range.hi) {
                return Err(refused("projected coordinate signs remain unresolved"));
            }
            pending.push((Ivl::new(mid, range.hi), depth + 1));
            pending.push((Ivl::new(range.lo, mid), depth + 1));
        }
    }
    Ok([one_piece(a, da)?, one_piece(b, db)?])
}

/// Select opposite interval endpoints whose ordered parameters move apart
/// under the previously certified projected-coordinate signs.
fn corner_maximum([a, b]: [Piece; 2], s: f64, t: f64) -> bool {
    // With the certified fixed coordinate signs, distance strictly grows as
    // ordered parameters move apart. Equal parameters have zero distance and
    // cannot maximize two positive-length intervals of this nonconstant curve.
    (s < t && s == a.range.lo && t == b.range.hi) || (s > t && s == a.range.hi && t == b.range.lo)
}

/// Construct complete finite All maxima after certifying coordinate monotonicity.
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
    if !supports(curves, intervals) {
        return Err(refused(
            "query is outside the identical single-span native NURBS class",
        ));
    }
    if !(tolerance.is_finite()
        && tolerance >= 0.0
        && normal.x.is_finite()
        && normal.y.is_finite()
        && normal.z.is_finite())
        || target.is_some_and(|value| !value.is_finite())
    {
        return Err(refused("monotone maximum needs finite options"));
    }
    let [ca, cb] = curves;
    let [ia, ib] = intervals;
    let da = bounded_domain(ca, ia)?;
    let db = bounded_domain(cb, ib)?;
    let query = Query {
        kernel,
        curves,
        normal,
        definitions: [Definition::of(ca, da)?, Definition::of(cb, db)?],
    };
    let pieces = certify(&query, [da, db])?;
    super::projected_pair_all::construct(
        &query,
        pieces,
        tolerance,
        handles,
        target,
        |_, pieces, s, t| Ok(corner_maximum(pieces, s, t)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::solution_array::ObjectKind;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::math::{CPoint, NOW};

    #[derive(Clone, Copy, PartialEq)]
    struct Handle;
    impl SolutionObject for Handle {
        fn kind(&self) -> ObjectKind {
            ObjectKind::Curve
        }
    }
    fn quadratic(points: [CPoint; 3]) -> Curve {
        Curve::from(
            BSplineCurve::from_data(3, 2, 5, 2, &points, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap(),
        )
    }
    fn run(a: &Curve, b: &Curve, normal: Vec3) -> Result<SolutionArray<Handle>> {
        solve(
            &Kernel::default(),
            [a, b],
            [a.natural_interval(), b.natural_interval()],
            normal,
            0.0,
            [None, None],
            None,
        )
    }

    fn original_gen1839() -> (Curve, [Extent1d; 2]) {
        // Exact original homogeneous controls; independent Fraction oracle in
        // plan/findings/diagnostics/projected_pair_monotone_witness_3060.py.
        let a = quadratic([
            CPoint::new(11.842545634155265, 7.344713691606567, 5.46251826429733, 1.0),
            CPoint::new(
                6.776096256477461,
                10.257732173030126,
                5.881478943491939,
                0.829018189211171,
            ),
            CPoint::new(
                1.8974778199934033,
                11.021078891759203,
                6.655645433105123,
                1.0,
            ),
        ]);
        let domains = [
            Extent1d::from_min_max(0.11409671825899509, 0.43561427937496744),
            Extent1d::from_min_max(0.03474215986040488, 0.4670833692125368),
        ];
        (a, domains)
    }

    #[test]
    fn original_gen1839_identical_rational_all_has_the_opposite_corner_maximum() {
        let (a, domains) = original_gen1839();
        let b = a.clone();
        assert!(supports([&a, &b], domains));
        let result = solve::<Handle>(
            &Kernel::default(),
            [&a, &b],
            domains,
            Vec3::Z,
            1e-4,
            [None, None],
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let answer = &result.as_slice()[0];
        assert!((answer.start.solution_value - 4.408015599603469).abs() < 1e-12);
        assert_eq!(answer.start.parameters[0], domains[0].max());
        assert_eq!(answer.start.parameters[1], domains[1].min());
    }

    #[test]
    fn public_api_original_gen1839_all_is_green_and_selected_refusals_propagate() {
        use crate::curve::curve::p9::CurveSolveOptions;
        use crate::geom::global_solver::{SolutionRequested, SolverOperation};
        let k = Kernel::default();
        let normal = [Vec3::Z];
        let opts = CurveSolveOptions::<Handle> {
            opt_vectors: Some(&normal),
            ..Default::default()
        };
        let (a, [ia, ib]) = original_gen1839();
        let b = a.clone();
        let answers = a
            .global_curve_solve(
                &k,
                &ia,
                &b,
                &ib,
                SolverOperation::ProjectedMaximize,
                1e-4,
                &opts,
                SolutionRequested::All,
            )
            .unwrap();
        assert_eq!(answers.len(), 1);
        let answer = &answers.as_slice()[0];
        assert!((answer.start.solution_value - 4.408015599603469).abs() < 1e-12);
        assert!(ia.contains_value(answer.start.parameters[0], 0.0));
        assert!(ib.contains_value(answer.start.parameters[1], 0.0));
        assert!(a
            .natural_interval()
            .contains_value(answer.start.parameters[0], 0.0));
        assert!(b
            .natural_interval()
            .contains_value(answer.start.parameters[1], 0.0));

        let nonmonotone = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 1.0, 0.0, NOW),
            CPoint::new(1.0, 0.0, 0.0, NOW),
        ]);
        let domain = nonmonotone.natural_interval();
        assert!(matches!(
            nonmonotone.global_curve_solve(
                &k,
                &domain,
                &nonmonotone,
                &domain,
                SolverOperation::ProjectedMaximize,
                1e-4,
                &opts,
                SolutionRequested::All
            ),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn offset_circle_all_fixture_stays_on_its_unchanged_path() {
        use crate::curve::curve::p9::CurveSolveOptions;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::geom::global_solver::{SolutionRequested, SolverOperation};
        use crate::nurbs::b_spline_curve::NurbCircleParam;
        let circle = |center, radius| {
            Curve::from(
                BSplineCurve::create_circle_segment(
                    3,
                    &Axis2Placement::new(center, Vec3::X, Vec3::Y),
                    radius,
                    0.0,
                    360.0,
                    NurbCircleParam::Quadratic,
                )
                .unwrap(),
            )
        };
        let a = circle(Vec3::ZERO, 1.0);
        let b = circle(Vec3::new(3.0, 0.0, 2.0), 0.5);
        let intervals = [a.natural_interval(), b.natural_interval()];
        assert!(!supports([&a, &b], intervals));
        assert!(!super::super::projected_pair_all::supports(
            [&a, &b],
            intervals
        ));
        let normal = [Vec3::Z];
        let opts = CurveSolveOptions::<Handle> {
            opt_vectors: Some(&normal),
            ..Default::default()
        };
        let answers = a
            .global_curve_solve(
                &Kernel::default(),
                &intervals[0],
                &b,
                &intervals[1],
                SolverOperation::ProjectedMaximize,
                1e-5,
                &opts,
                SolutionRequested::All,
            )
            .unwrap();
        assert!(!answers.is_empty());
        assert!(answers
            .as_slice()
            .iter()
            .all(|answer| (answer.start.solution_value - 4.5).abs() <= 1e-5));
    }

    #[test]
    fn monotone_certificate_is_not_limited_to_axis_projection() {
        let a = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 5.0, 0.0, NOW),
            CPoint::new(1.0, 12.0, 0.0, NOW),
        ]);
        let n = Vec3::new(1.0, 1.0, 1.0);
        let result = run(&a, &a, n).unwrap();
        assert_eq!(result.len(), 2);
        let expected = n.cross(Vec3::new(1.0, 12.0, 0.0)).cross(n).length();
        for answer in result.as_slice() {
            assert!((answer.start.solution_value - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn selected_nonmonotone_definition_refuses_instead_of_falling_back() {
        let a = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 1.0, 0.0, NOW),
            CPoint::new(1.0, 0.0, 0.0, NOW),
        ]);
        assert!(supports(
            [&a, &a],
            [a.natural_interval(), a.natural_interval()]
        ));
        assert!(matches!(
            run(&a, &a, Vec3::Z),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn distinct_definitions_are_outside_the_structural_class() {
        let a = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 1.0, 0.0, NOW),
            CPoint::new(1.0, 3.0, 0.0, NOW),
        ]);
        let b = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 1.0, 0.0, NOW),
            CPoint::new(1.0, 3.00000001, 0.0, NOW),
        ]);
        assert!(!supports(
            [&a, &b],
            [a.natural_interval(), b.natural_interval()]
        ));
    }

    #[test]
    fn ordered_parameter_extrema_cover_disjoint_overlap_and_touching_intervals() {
        let a = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 0.5, 0.0, NOW),
            CPoint::new(1.0, 2.0, 0.0, NOW),
        ]);
        for (first, second, expected) in [
            ([0.0, 0.2], [0.7, 1.0], vec![[0.0, 1.0]]),
            ([0.7, 1.0], [0.0, 0.2], vec![[1.0, 0.0]]),
            ([0.0, 0.7], [0.3, 1.0], vec![[0.0, 1.0], [0.7, 0.3]]),
            ([0.0, 0.5], [0.5, 1.0], vec![[0.0, 1.0]]),
        ] {
            let domains = [
                Extent1d::from_min_max(first[0], first[1]),
                Extent1d::from_min_max(second[0], second[1]),
            ];
            assert!(supports([&a, &a], domains));
            // Large retention makes the classifier, rather than scalar pruning,
            // responsible for excluding all the nonmaximal corners.
            let result = solve::<Handle>(
                &Kernel::default(),
                [&a, &a],
                domains,
                Vec3::Z,
                100.0,
                [None, None],
                None,
            )
            .unwrap();
            assert_eq!(result.len(), expected.len(), "{first:?} x {second:?}");
            for wanted in expected {
                assert!(result
                    .as_slice()
                    .iter()
                    .any(
                        |answer| [answer.start.parameters[0], answer.start.parameters[1]] == wanted
                    ));
            }
        }
    }

    #[test]
    fn zero_normal_refuses_a_nonisolated_projected_solution_set() {
        let a = quadratic([
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5, 0.5, 0.0, NOW),
            CPoint::new(1.0, 2.0, 0.0, NOW),
        ]);
        assert!(matches!(
            run(&a, &a, Vec3::ZERO),
            Err(SolidError::Unsupported { .. })
        ));
    }
}
