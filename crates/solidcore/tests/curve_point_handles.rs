//! Duplicate point/curve solutions retain arena identities and use live geometry.

use solidcore::curve::curve::p2::CpGlobalSolver;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{GlobalSolve, SolutionPairType};
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::tree::Tree;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::Kernel;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ArenaCurve(u32);

impl SolutionObject for ArenaCurve {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

#[test]
fn opaque_curve_comparison_uses_natural_not_query_interval() {
    let k = Kernel::default();
    let curve = Curve::from(BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::X).unwrap());
    let point_tree = Tree::from_point(Vec3::ZERO, 0.0, true);
    let curve_tree = Tree::from_point(Vec3::ZERO, 0.0, true);
    let mut solver = CpGlobalSolver::new(
        &k,
        &point_tree,
        &curve_tree,
        Extent1d::from_min_max(0.1, 0.2),
    )
    .with_curve(&curve);
    let mut first = Solution::new();
    first.solution_type = SolutionType::SingleValue;
    first.num_objects = 1;
    first.num_variables = 1;
    first.objects[0] = Some(ArenaCurve(7));
    first.start.parameters[0] = 0.1;
    let mut second = first;
    second.start.parameters[0] = 0.10005;

    assert_eq!(
        solver.identical_solutions(&first, &second).unwrap().0,
        SolutionPairType::Identical
    );
    assert_eq!(first.objects[0], Some(ArenaCurve(7)));
    second.objects[0] = Some(ArenaCurve(8));
    assert_eq!(
        solver.identical_solutions(&first, &second).unwrap().0,
        SolutionPairType::DiffObjects
    );
    second.objects[0] = first.objects[0];
    solver.curve = None;
    assert!(solver
        .identical_solutions(&first, &second)
        .unwrap_err()
        .is_unported());
}
