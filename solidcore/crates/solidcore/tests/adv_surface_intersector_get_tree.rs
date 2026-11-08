//! `AdvSurfaceIntersector::set_surface_caches` on caches without a tree builds the trees
//! (upstream `SmSurfaceIntersector::Intersect`: `pSC->GetTree()` builds a missing tree),
//! instead of returning `Unported(SmSurfaceCache::GetTree)`.

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::{
    GlobalSolve, SolutionRequested, SolveRequest, SolverOperation,
};
use solidcore::geom::solution_array::SolutionArray;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::adv_surface_intersector::{AdvSurfaceIntersector, NoSurfaceHandle};
use solidcore::kernel::Kernel;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{SurfaceCache, SurfaceCacheOptions};

fn plane(o: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Surface {
    let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let d = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    Surface::from(Plane::new(v(o), v(x), v(y), Vec2::new(2.0, 2.0), &d))
}

fn cache(s: Surface) -> SurfaceCache<NoSurfaceHandle> {
    let d = s.natural_uv_domain_if_built();
    SurfaceCache::new(s, d, SurfaceCacheOptions::default())
}

#[test]
fn set_surface_caches_builds_missing_trees() {
    let k = Kernel::default();
    let a = || plane([-1.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let b = || plane([-1.0, 0.1, -1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let (sa, sb) = (a(), b());
    let mut caches = vec![cache(a()), cache(b())];
    assert!(caches.iter().all(|c| c.tree().is_none()));
    let da = sa.natural_uv_domain_if_built();
    let db = sb.natural_uv_domain_if_built();
    let mut si = AdvSurfaceIntersector::new(&k, &sa, &da, &sb, &db, false).unwrap();
    si.set_surface_caches(&k, &mut caches)
        .expect("GetTree builds the missing trees");
    for t in 0..2 {
        let tree = si.solver.tree(t).unwrap();
        assert!(!tree.nodes().is_empty());
        assert!(tree.top_node().is_some());
    }
    // the planes z = 0 and y = 0.1 meet along a line inside both squares
    let mut sols = SolutionArray::new();
    let req = SolveRequest {
        operation: SolverOperation::Intersect,
        solution_requested: SolutionRequested::from_code(1).unwrap(),
        tolerance_3d: 1e-3,
        best_answer_so_far_sq: 1e20,
        opt_vectors: &[],
    };
    si.solve_it(&req, &mut sols).unwrap();
    assert!(!sols.as_slice().is_empty());
}

#[test]
fn set_surface_caches_same_surface_one_cache() {
    let k = Kernel::default();
    let s = plane([-1.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let d = s.natural_uv_domain_if_built();
    let mut caches = vec![cache(s.clone())];
    let mut si = AdvSurfaceIntersector::new(&k, &s, &d, &s, &d, false).unwrap();
    si.set_surface_caches(&k, &mut caches).unwrap();
    let n0 = si.solver.tree(0).unwrap().nodes().len();
    assert!(n0 > 0);
    assert_eq!(si.solver.tree(1).unwrap().nodes().len(), n0);
}

#[test]
fn set_surface_caches_without_caches_is_null_pointer() {
    let k = Kernel::default();
    let s = plane([-1.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let d = s.natural_uv_domain_if_built();
    let mut none: Vec<SurfaceCache<NoSurfaceHandle>> = Vec::new();
    let mut si = AdvSurfaceIntersector::new(&k, &s, &d, &s, &d, false).unwrap();
    let e = si.set_surface_caches(&k, &mut none).unwrap_err();
    assert_eq!(e.sm_status(), Some(solidcore::error::SM_ERR_NULL_POINTER));
}
