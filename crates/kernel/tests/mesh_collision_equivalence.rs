//! Equivalence checks for SolidCraft's changes to the vendored truck crates.
//!
//! The vendored crates are outside the workspace, so their own tests never run in CI. Each
//! change that replaces an algorithm keeps the old one behind the crate's `test-hooks` feature
//! (on only for these tests) and these tests check the new one gives the same answers.

use truck_meshalgo::analyzers::collision_test_hooks as collision;
use truck_modeling::*;

/// Mesh collision candidate search (sweep with per-triangle boxes): same collisions, same order.
#[test]
fn mesh_collision_same_as_before() {
    match collision::same_collisions_in_the_same_order(0..200) {
        Ok(n) => assert!(n > 0, "the random meshes produced no collisions to compare"),
        Err(e) => panic!("{e}"),
    }
}

/// NaN coordinates in a mesh: the collision search goes on without a panic.
#[test]
fn mesh_collision_nan_triangles_do_not_panic() {
    if let Err(e) = collision::nan_triangles_do_not_panic() {
        panic!("{e}");
    }
}

// Nearest-point searches without a hint start from a grid search of the surface. The vendored
// crates search the grid of a revolved surface with `Processor::<RevolutedCurve<_>, _>::presearch`
// (each grid line's curve point and rotation made once) and of a B-spline or NURBS surface with
// `BSplineSurface::grid_presearch` (each grid line's basis functions made once). They must pick
// exactly the parameters truck's own `algo::surface::presearch` picks (#4431, #4542).

/// Small seeded generator (no extra dependency).
struct Lcg(u64);
impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn coord(&mut self) -> f64 {
        self.unit() * 20.0 - 10.0
    }
    fn point(&mut self) -> Point3 {
        Point3::new(self.coord(), self.coord(), self.coord())
    }
}

fn surfaces(r: &mut Lcg) -> Vec<Processor<RevolutedCurve<Curve>, Matrix4>> {
    let line = Curve::Line(Line(r.point(), r.point()));
    let spline = Curve::BSplineCurve(BSplineCurve::new(KnotVec::bezier_knot(3), vec![r.point(), r.point(), r.point(), r.point()]));
    let axis = Vector3::new(r.coord(), r.coord(), r.coord() + 0.5);
    let origin = r.point();
    let turn = Matrix4::from_translation(Vector3::new(r.coord(), r.coord(), r.coord()))
        * Matrix4::from_axis_angle(Vector3::new(r.coord(), r.coord(), 1.0).normalize(), Rad(r.unit() * 6.0))
        * Matrix4::from_nonuniform_scale(0.5 + r.unit(), 0.5 + r.unit(), 0.5 + r.unit());
    let mut out = Vec::new();
    for curve in [line, spline] {
        let plain = Processor::new(RevolutedCurve::by_revolution(curve, origin, axis));
        let mut moved = plain.clone();
        moved.transform_by(turn);
        let mut flipped = moved.clone();
        flipped.invert();
        out.extend([plain, moved, flipped]);
    }
    out
}

#[test]
fn revolved_presearch_is_truck_presearch() {
    let mut compared = 0;
    for seed in 0..40u64 {
        let mut r = Lcg(seed * 7919 + 11);
        for s in surfaces(&mut r) {
            let full = s.range_tuple();
            let ((u0, u1), (v0, v1)) = full;
            let part = ((u0 + (u1 - u0) * 0.2, u0 + (u1 - u0) * 0.7), (v0 + (v1 - v0) * 0.1, v0 + (v1 - v0) * 0.6));
            for range in [full, part] {
                for division in [7, 100] {
                    for _ in 0..4 {
                        let p = r.point();
                        let fast = s.presearch(p, range, division);
                        let truck = algo::surface::presearch(&s, p, range, division);
                        assert!(fast.0.to_bits() == truck.0.to_bits() && fast.1.to_bits() == truck.1.to_bits(), "seed {seed}: {fast:?} vs {truck:?}");
                        compared += 1;
                    }
                }
            }
        }
    }
    assert!(compared > 0);
}

/// A surface's nearest-point search without a hint goes through the cached presearch and
/// finds the same parameters as a search started from truck's own presearch.
#[test]
fn unhinted_search_on_a_revolved_surface_matches_truck() {
    let mut r = Lcg(5);
    for s in surfaces(&mut r) {
        let surface = Surface::RevolutedCurve(s.clone());
        for _ in 0..8 {
            let p = r.point();
            let hint = algo::surface::presearch(&s, p, s.range_tuple(), 100);
            let expected = algo::surface::search_nearest_parameter(&s, p, hint, 30);
            assert_eq!(surface.search_nearest_parameter(p, None, 30), expected);
        }
    }
}

fn splines(r: &mut Lcg) -> (BSplineSurface<Point3>, NurbsSurface<Vector4>) {
    let (n0, n1) = (2 + (r.unit() * 4.0) as usize, 2 + (r.unit() * 4.0) as usize);
    let knots = |n: usize, r: &mut Lcg| {
        let degree = (1 + (r.unit() * 3.0) as usize).min(n - 1);
        let inner = n - degree - 1;
        let mut ks: Vec<f64> = (0..inner).map(|_| r.unit()).collect();
        ks.sort_by(f64::total_cmp);
        let mut all = vec![0.0; degree + 1];
        all.extend(ks);
        all.extend(vec![1.0; degree + 1]);
        KnotVec::from(all)
    };
    let (k0, k1) = (knots(n0, r), knots(n1, r));
    let pts: Vec<Vec<Point3>> = (0..n0).map(|_| (0..n1).map(|_| r.point()).collect()).collect();
    let weighted: Vec<Vec<Vector4>> =
        pts.iter().map(|row| row.iter().map(|p| Vector4::new(p.x, p.y, p.z, 1.0) * (0.3 + r.unit() * 2.0)).collect()).collect();
    (BSplineSurface::new((k0.clone(), k1.clone()), pts), NurbsSurface::new(BSplineSurface::new((k0, k1), weighted)))
}

#[test]
fn spline_presearch_is_truck_presearch() {
    let mut compared = 0;
    for seed in 0..60u64 {
        let mut r = Lcg(seed * 7919 + 17);
        let (b, n) = splines(&mut r);
        let full = b.range_tuple();
        let part = ((0.1, 0.8), (0.3, 0.6));
        for range in [full, part] {
            for division in [5, 50] {
                for _ in 0..4 {
                    let p = r.point();
                    let fast = b.grid_presearch(range, division, |q| q.distance2(p));
                    let truck = algo::surface::presearch(&b, p, range, division);
                    assert!(fast.0.to_bits() == truck.0.to_bits() && fast.1.to_bits() == truck.1.to_bits(), "seed {seed}: {fast:?} vs {truck:?}");
                    let fast = n.non_rationalized().grid_presearch(range, division, |q| q.to_point().distance2(p));
                    let truck = algo::surface::presearch(&n, p, range, division);
                    assert!(
                        fast.0.to_bits() == truck.0.to_bits() && fast.1.to_bits() == truck.1.to_bits(),
                        "seed {seed} (NURBS): {fast:?} vs {truck:?}"
                    );
                    compared += 2;
                }
            }
        }
        // And the searches that use them.
        for _ in 0..4 {
            let p = r.point();
            let hint = algo::surface::presearch(&b, p, b.range_tuple(), 50);
            assert_eq!(b.search_nearest_parameter(p, None, 30), algo::surface::search_nearest_parameter(&b, p, hint, 30));
            let hint = algo::surface::presearch(&n, p, n.range_tuple(), 50);
            assert_eq!(n.search_nearest_parameter(p, None, 30), algo::surface::search_nearest_parameter(&n, p, hint, 30));
        }
    }
    assert!(compared > 0);
}
