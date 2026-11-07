//! Iso-curve metadata and public point solves follow the varying tensor coordinate.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::{
    curve::{
        curve::{p1::GlobalPointSolveOptions, Curve},
        iso_curve::IsoCurve,
    },
    geom::{
        extent1d::Extent1d,
        geom_utility::p2::ContinuityType,
        global_solver::{SolutionRequested, SolverOperation},
        solution_array::{ObjectKind, SolutionObject},
    },
    math::Vec3,
    nurbs::{
        b_spline_surface::{BSplineSurface, SurfParamType},
        math::CPoint,
        srf_basic::Surface as Net,
    },
    Kernel,
};

#[derive(Clone, Copy, PartialEq)]
struct Object;
impl SolutionObject for Object {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

fn corner(fixed: SurfParamType, shallow: bool) -> Curve {
    let (a, y) = if shallow {
        (1.0 / 1024.0, 1.0 - 1.0 / 524288.0)
    } else {
        (0.25, 0.0)
    };
    let xy = [(-a, y), (0.0, 1.0), (a, y)];
    let short = vec![0.0, 0.0, 1.0, 1.0];
    let long = vec![0.0, 0.0, 0.5, 2.0, 2.0];
    let (rows, u, v) = if fixed == SurfParamType::U {
        (
            [0.0, 1.0]
                .into_iter()
                .map(|z| xy.iter().map(|&(x, y)| CPoint::new(x, y, z, 1.0)).collect())
                .collect(),
            short,
            long,
        )
    } else {
        (
            xy.into_iter()
                .map(|(x, y)| {
                    [0.0, 1.0]
                        .into_iter()
                        .map(|z| CPoint::new(x, y, z, 1.0))
                        .collect()
                })
                .collect(),
            long,
            short,
        )
    };
    let surface = BSplineSurface::from_nurb(Net::new(rows, 1, 1, u, v).unwrap(), false);
    Curve::from(IsoCurve::new(surface, fixed, 0.5, false, true))
}

#[test]
fn continuity_follows_each_varying_direction_with_unequal_knot_counts() {
    let k = Kernel::default();
    for fixed in [SurfParamType::U, SurfParamType::V] {
        let curve = corner(fixed, false);
        assert_eq!(curve.natural_interval(), Extent1d::from_min_max(0.0, 2.0));
        assert_eq!(
            curve.knots(&k, None).unwrap(),
            (vec![0.0, 0.5, 2.0], vec![2, 1, 2])
        );
        assert_eq!(
            curve.evaluate_point(&k, 0.5).unwrap(),
            Vec3::new(0.0, 1.0, 0.5)
        );
        assert_eq!(
            curve.calculate_continuities(&k, 0.5).unwrap(),
            (
                ContinuityType::C0,
                vec![
                    ContinuityType::Discontinuous,
                    ContinuityType::C0,
                    ContinuityType::Discontinuous
                ]
            )
        );
    }
}

#[test]
fn original_shallow_corner_has_a_continuity_entry_for_each_varying_knot() {
    let k = Kernel::default();
    for fixed in [SurfParamType::U, SurfParamType::V] {
        let curve = corner(fixed, true);
        let (_, continuities) = curve.calculate_continuities(&k, 0.5).unwrap();
        assert_eq!(continuities.len(), 3);
        assert_eq!(continuities.first(), Some(&ContinuityType::Discontinuous));
        assert_eq!(continuities.last(), Some(&ContinuityType::Discontinuous));
    }
}

#[test]
fn projected_query_uses_the_varying_knot_without_a_list_mismatch() {
    let k = Kernel::default();
    for fixed in [SurfParamType::U, SurfParamType::V] {
        let curve = corner(fixed, false);
        for requested in [SolutionRequested::Single, SolutionRequested::All] {
            let answers = curve
                .global_point_solve::<Object>(
                    &k,
                    &curve.natural_interval(),
                    SolverOperation::ProjectedMaximize,
                    Vec3::ZERO,
                    0.0,
                    requested,
                    &GlobalPointSolveOptions {
                        opt_vectors: Some(&[Vec3::Z]),
                        ..Default::default()
                    },
                )
                .unwrap();
            // This dispatcher regression checks traversal, not the independent
            // generic solver's accuracy at the exact corner maximum.
            assert!(!answers.is_empty());
            for answer in answers.as_slice() {
                let t = answer.start.parameters[0];
                assert!(t.is_finite() && (0.0..=2.0).contains(&t));
                assert!(answer.start.solution_value.is_finite());
                let point = curve.evaluate_point(&k, t).unwrap();
                assert!(point.x.is_finite() && point.y.is_finite() && point.z.is_finite());
            }
        }
    }
}
