//! WR-698 (smlib#79 step 7, the fork's bb9f45e and 729f893): a surface cache is built on a
//! blend's net: every entry point of its tree's construction holds a `NetScope`, so a
//! blend by its definition evaluates as its net there (no point of the definition is
//! evaluated), and the boxes of the nodes it splits off a blend's subdivision surfaces
//! are grown by the net's certified bound.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use solidcore::blend::fillet_section::section::GnSectionFsg;
use solidcore::geom::vector3d::Point3;
use solidcore::intersect::adv_surface_intersector::NoSurfaceHandle;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::surface::procedural_blend::{BlendDefinitionInput, NetScope, ProceduralBlend};
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{SurfaceCache, SurfaceCacheOptions};

const L: f64 = 10.0;
const R: f64 = 2.0;

/// A round of radius R between the planes y = 0 and z = 0 along the x axis, by its
/// definition, on a bilinear net over the unit square.
fn blend(k: &Kernel) -> Surface {
    let a = Surface::from(
        BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(L, 0.0, 0.0),
            Point3::new(0.0, 0.0, L),
            Point3::new(L, 0.0, L),
        )
        .unwrap(),
    );
    let b = Surface::from(
        BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(L, 0.0, 0.0),
            Point3::new(0.0, L, 0.0),
            Point3::new(L, L, 0.0),
        )
        .unwrap(),
    );
    let net = BSplineSurface::create_bilinear_surface(
        Point3::new(0.0, 0.0, R),
        Point3::new(L, 0.0, R),
        Point3::new(0.0, R, 0.0),
        Point3::new(L, R, 0.0),
    )
    .unwrap();
    let leader =
        BSplineCurve::create_line_segment(3, Point3::new(0.0, R, R), Point3::new(L, R, R)).unwrap();
    let seed1 = BSplineCurve::create_line_segment(
        2,
        Point3::new(0.0, R / L, 0.0),
        Point3::new(1.0, R / L, 0.0),
    )
    .unwrap();
    let seed2 = seed1.clone();
    let input = BlendDefinitionInput {
        supports: [&a, &b],
        sides: [-1.0, 1.0],
        offsets: [R, R],
        law: None,
        order: 1,
        thumbweight: 0.0,
        leader: &leader,
        seeds: [&seed1, &seed2],
        bound: 1e-3,
        chamfer: None,
    };
    Surface::from(
        ProceduralBlend::create(k, &net, &input, |n, w| {
            GnSectionFsg::is_valid(n as usize, w)
        })
        .unwrap(),
    )
}

#[test]
fn a_surface_cache_is_built_on_a_blend_s_net() {
    let k = Kernel::default();
    let s = blend(&k);
    // outside a scope the blend is its definition: a point evaluates it
    let before = k.blend_counts.get().evaluations;
    s.evaluate_point(&k, Point2::new(0.5, 0.5)).unwrap();
    assert!(k.blend_counts.get().evaluations > before);
    let d = s.natural_uv_domain_if_built();
    let mut cache = SurfaceCache::<NoSurfaceHandle>::new(s, d, SurfaceCacheOptions::default());
    let before = k.blend_counts.get().evaluations;
    cache.build_tree(&k, false).unwrap();
    assert!(cache.tree().is_some());
    assert_eq!(
        k.blend_counts.get().evaluations,
        before,
        "the cache evaluated the definition"
    );
    assert!(!NetScope::is_active(&k), "a scope outlived the build");
}

/// A plane `x = x0`, u along y and v along z over [0, L].
fn plane_x(x0: f64) -> Surface {
    Surface::from(
        BSplineSurface::create_bilinear_surface(
            Point3::new(x0, 0.0, 0.0),
            Point3::new(x0, L, 0.0),
            Point3::new(x0, 0.0, L),
            Point3::new(x0, L, L),
        )
        .unwrap(),
    )
}

/// The rational quadratic arc of radius `rad` about `(x, c, c)` in the plane of that x,
/// from angle `a0` to `a1` (radians, in (y, z)), over [0, 1].
fn arc(x: f64, c: f64, rad: f64, a0: f64, a1: f64) -> BSplineCurve {
    use solidcore::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};
    let h = 0.5 * (a1 - a0);
    let at = |a: f64, r: f64| Point3::new(x, c + r * a.cos(), c + r * a.sin());
    let cps = [at(a0, rad), at(a0 + h, rad / h.cos()), at(a1, rad)];
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 2,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&[1.0, h.cos(), 1.0]),
        trim: None,
    })
    .unwrap()
}

#[test]
fn a_round_on_a_round_reads_the_inner_round_by_its_definition() {
    // the inner round's exact surface: the cylinder |(y, z) - (R, R)| = R between its
    // feet on y = 0 (angle 180 degrees) and z = 0 (270)
    let k = Kernel::default();
    let inner = blend(&k);
    let (x0, rho) = (5.0, 0.5);
    let wall = plane_x(x0);
    let (a0, a1) = (200_f64.to_radians(), 250_f64.to_radians());
    // the outer ball of radius rho rolls inside the cylinder against the wall x = x0: its
    // centre at x0 - rho, R - rho from the cylinder's axis; the leader that circle
    let leader = arc(x0 - rho, R, R - rho, a0, a1);
    let seg = |a: Point2, b: Point2| {
        BSplineCurve::create_line_segment(2, Point3::new(a.x, a.y, 0.0), Point3::new(b.x, b.y, 0.0))
            .unwrap()
    };
    // the seeds: on the inner round (u along x, v across its arc, roughly by angle) and on
    // the wall (y / L, z / L)
    let ui = (x0 - rho) / L;
    let vi = |a: f64| (a.to_degrees() - 180.0) / 90.0;
    let seed_inner = seg(Point2::new(ui, vi(a0)), Point2::new(ui, vi(a1)));
    let wall_uv =
        |a: f64| Point2::new((R + (R - rho) * a.cos()) / L, (R + (R - rho) * a.sin()) / L);
    let seed_wall = seg(wall_uv(a0), wall_uv(a1));
    // the sides: the ball's centre against each support's normal at its seed's start
    let m0 = leader.evaluate_point(0.0).unwrap();
    let side = |s: &Surface, uv: Point2| {
        let p = s.evaluate_point(&k, uv).unwrap();
        let n = s
            .evaluate_normal(
                &k,
                uv,
                solidcore::nurbs::knots::Side::Left,
                solidcore::nurbs::knots::Side::Left,
            )
            .unwrap();
        if (m0 - p).dot(n) >= 0.0 {
            1.0
        } else {
            -1.0
        }
    };
    let sides = [
        side(&inner, Point2::new(ui, vi(a0))),
        side(&wall, wall_uv(a0)),
    ];
    let net = BSplineSurface::create_bilinear_surface(
        Point3::new(x0 - rho, 0.0, R),
        Point3::new(x0 - rho, R, 0.0),
        Point3::new(x0, 0.0, R),
        Point3::new(x0, R, 0.0),
    )
    .unwrap();
    let input = BlendDefinitionInput {
        supports: [&inner, &wall],
        sides,
        offsets: [rho, rho],
        law: None,
        order: 1,
        thumbweight: 0.0,
        leader: &leader,
        seeds: [&seed_inner, &seed_wall],
        bound: 1e-3,
        chamfer: None,
    };
    let outer = ProceduralBlend::create(&k, &net, &input, |n, w| {
        GnSectionFsg::is_valid(n as usize, w)
    })
    .unwrap();
    assert_eq!(outer.depth(), 2);
    let outer = Surface::from(outer);
    for i in 0..=4 {
        let u = f64::from(i) / 4.0;
        // its first rail on the inner round's definition, the exact cylinder (its net, a
        // plane, is up to 0.4 off it), its second on the wall
        let p0 = outer.evaluate_point(&k, Point2::new(u, 0.0)).unwrap();
        let on_cylinder = ((p0.y - R).powi(2) + (p0.z - R).powi(2)).sqrt() - R;
        assert!(
            on_cylinder.abs() < 1e-9,
            "rail 1 at {u}: {on_cylinder:e} ({p0:?})"
        );
        let p1 = outer.evaluate_point(&k, Point2::new(u, 1.0)).unwrap();
        assert!((p1.x - x0).abs() < 1e-9, "rail 2 at {u}: {p1:?}");
    }
}
