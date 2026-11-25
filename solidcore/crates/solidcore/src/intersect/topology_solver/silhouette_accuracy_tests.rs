//! Projected face measurements must use the surface at the returned UV point.

use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::tol::USE_DEFAULT;
use crate::geom::vector2d::Vec2;
use crate::intersect::topology_solver::p1::{
    LocalSolver, LocalSolverKind, TopologyObjects, TopologySolver,
};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::surface::sphere::{Sphere, SphereOptions};
use crate::surface::surface::Surface;
use crate::surface::torus::{Torus, TorusOptions};
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use crate::topology::brep_cache::{arena_identity, BrepObjects, CacheObject};
use crate::topology::topology_traverser::Marks;
use crate::topology::BrepId;

/// A prepopulated cache must evaluate its UV curve on S, not copy the 3D fit.
#[test]
fn cached_silhouette_measurements_use_the_source_surface() {
    use crate::curve::curve::Curve;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::surface::plane::Plane;
    use crate::surface::surface_cache::p1::{
        AltCacheKey, AltSrfCacheType, SurfaceCache, SurfaceCacheOptions,
    };

    struct CachedContext<'a>(&'a SurfaceCache<CacheObject>);
    impl crate::surface::surface::p1::SurfaceSolveContext<CacheObject> for CachedContext<'_> {
        fn build_surface_cache(
            &mut self,
            _k: &Kernel,
            _surface: &Surface,
            _handle: Option<CacheObject>,
        ) -> crate::Result<SurfaceCache<CacheObject>> {
            self.0.map_handles(Ok)
        }
        fn face_queries(
            &self,
        ) -> Option<&dyn crate::surface::surface_cache::face_context::FaceQueries<CacheObject>>
        {
            None
        }
    }
    struct CachedObjects<'a> {
        base: BrepObjects<'a>,
        cache: &'a SurfaceCache<CacheObject>,
    }
    macro_rules! delegate {
        ($name:ident, $ret:ty) => {
            fn $name(&self, object: CacheObject) -> $ret {
                self.base.$name(object)
            }
        };
    }
    impl TopologyObjects<CacheObject> for CachedObjects<'_> {
        fn surface_solve_context(
            &self,
        ) -> Option<Box<dyn crate::surface::surface::p1::SurfaceSolveContext<CacheObject> + '_>>
        {
            Some(Box::new(CachedContext(self.cache)))
        }
        delegate!(curve, crate::Result<&Curve>);
        delegate!(analytic_curve_handle, Option<CacheObject>);
        delegate!(vertex_point, crate::Result<Vec3>);
        delegate!(vertex_tolerance, crate::Result<f64>);
        delegate!(curve_edge, crate::Result<Option<CacheObject>>);
        delegate!(edge_curve, crate::Result<Option<CacheObject>>);
        delegate!(
            edge_interval,
            crate::Result<crate::geom::extent1d::Extent1d>
        );
        delegate!(edge_tolerance, crate::Result<f64>);
        delegate!(surface_face, crate::Result<Option<CacheObject>>);
        delegate!(face_surface, crate::Result<CacheObject>);
        delegate!(surface, crate::Result<&Surface>);
        delegate!(face_uv_domain, crate::Result<Extent2d>);
        delegate!(face_tolerance, crate::Result<f64>);
        delegate!(surface_owner, crate::Result<Option<CacheObject>>);
        delegate!(shape_member, crate::geom::object::ShapeMember);
        delegate!(
            face_entity,
            Option<(&crate::topology::Brep, crate::topology::FaceId)>
        );
        fn object_zone_tol_3d(&self, k: &Kernel, object: CacheObject) -> crate::Result<f64> {
            self.base.object_zone_tol_3d(k, object)
        }
    }

    let mut k = Kernel::default();
    let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    let surface = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(1.0, 1.0),
        &domain,
    ));
    let uv = Curve::from(
        BSplineCurve::create_line_segment(2, Vec3::new(0.0, 0.25, 0.0), Vec3::new(1.0, 0.25, 0.0))
            .unwrap(),
    );
    let fit = Curve::from(
        BSplineCurve::create_line_segment(
            3,
            Vec3::new(10.0, 0.25, 10.0),
            Vec3::new(11.0, 0.25, 10.0),
        )
        .unwrap(),
    );
    let mut cache: SurfaceCache<CacheObject> =
        SurfaceCache::new(surface.clone(), domain, SurfaceCacheOptions::default());
    let key = AltCacheKey {
        vec: Some(Vec3::new(0.0, 0.0, 1.0)),
        boolean: Some(false),
        ..AltCacheKey::default()
    };
    cache
        .add_curves_to_cache(
            AltSrfCacheType::TrimmedSilhouette,
            1e-5,
            20.0 * std::f64::consts::PI / 180.0,
            &key,
            Some(vec![fit]),
            Some(vec![uv]),
        )
        .unwrap();
    let copies = cache
        .has_cached_curves(
            AltSrfCacheType::TrimmedSilhouette,
            1e-5,
            20.0 * std::f64::consts::PI / 180.0,
            &key,
            true,
            true,
        )
        .unwrap()
        .unwrap();
    let uv_curves = copies.curves_uv.unwrap();
    let net = crate::blend::fillet_section::station::Nurbs::of(
        &crate::blend::fillet_section::station::as_b_spline(&k, &surface)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let proposals = super::projected_bound::native_uv_proposals(
        &k,
        &surface,
        &net,
        &domain,
        Vec3::ZERO,
        Vec3::X,
        &uv_curves,
    )
    .unwrap();
    assert!(!proposals.is_empty());
    for (uv, value) in proposals {
        let point = surface.evaluate_point(&k, uv).unwrap();
        assert_eq!(point.z, 0.0);
        assert_eq!(value, Vec3::X.cross(point).cross(Vec3::X).length());
        // The cached fit has z=10. Native proposals use z=0 even when that
        // coordinate contributes to the projection metric.
        assert!(value <= 1.0);
    }
    // No matching tolerance is a cache miss, not an empty successful query.
    assert!(cache
        .has_cached_curves(
            AltSrfCacheType::TrimmedSilhouette,
            1e-6,
            20.0 * std::f64::consts::PI / 180.0,
            &key,
            true,
            true,
        )
        .unwrap()
        .is_none());

    // Exercise the production PointFace path with the same poisoned cache.
    // The fit is displaced in X as well as Z, so measuring it under normal Z
    // would produce a value greater than 10 instead of the native sqrt(2).
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let keys = |f| arena_identity(CacheObject::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &keys,
    };
    let face = create_face_from_surface(&mut k, &mut b, Some(surface.clone()), &domain, &mut fenv)
        .unwrap();
    // Prepare the real trim curves before substituting only the silhouette
    // cache; bypassing preparation would leave no defining UV boundary.
    let mut preparation = crate::query::brep::RayPreparation::default();
    {
        let objects = BrepObjects {
            preparation: Some(&preparation),
            brep: &b,
            brep_id: BrepId::from_index(0),
            cache: None,
            keys: &arena_identity,
            owner_of: &|_| None,
        };
        let mut context = objects.surface_solve_context().unwrap();
        assert!(matches!(
            context.build_surface_cache(
                &k,
                b.face(face).unwrap().surface.as_ref().unwrap(),
                Some(CacheObject::Surface(face))
            ),
            Err(crate::SolidError::PreparationRequired { .. })
        ));
    }
    let request = preparation.take_request().unwrap();
    preparation.prepare(&k, &mut b, request).unwrap();
    assert!(super::projected_bound::is_exact_rectangle(&k, &b, face, &domain).unwrap());
    let objects = CachedObjects {
        base: BrepObjects {
            preparation: None,
            brep: &b,
            brep_id: BrepId::from_index(0),
            cache: None,
            keys: &arena_identity,
            owner_of: &|_| None,
        },
        cache: &cache,
    };
    let mut solver = TopologySolver::new(
        &k,
        &objects,
        SolverOperation::ProjectedMaximize,
        SolutionRequested::Single,
        1e-5,
        0.0,
        &[Vec3::Z],
    );
    let mut local = LocalSolver::new(LocalSolverKind::PointFace, false, None);
    local.set_point(Vec3::ZERO);
    local
        .solve_it(&k, &mut solver, None, Some(CacheObject::Surface(face)))
        .unwrap();
    assert_eq!(solver.base.solutions.len(), 1);
    let chosen = &solver.base.solutions.as_slice()[0];
    assert_eq!(chosen.start.solution_value, 2.0_f64.sqrt());
    assert_eq!(
        (
            chosen.start.param(0).unwrap(),
            chosen.start.param(1).unwrap()
        ),
        (1.0, 1.0)
    );
    super::projected_bound::certify(
        &k,
        &objects,
        CacheObject::Face(face),
        Vec3::ZERO,
        &solver.base,
    )
    .unwrap();
}

#[test]
fn projected_sphere_face_maximum_matches_surface_and_closed_form() {
    check_sphere_projection(0.0, 1.0);
}

#[test]
fn projected_tilted_sphere_face_maximum_uses_projection_metric() {
    check_sphere_projection(45.0, 0.001);
    check_sphere_projection(45.0, 1.0);
    check_sphere_projection(45.0, 1000.0);
}

fn check_sphere_projection(tilt: f64, scale: f64) {
    let k = Kernel::default();
    let mut sphere = Sphere::new(
        &k,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Extent2d::from_bounds(0.0, -60.0, 360.0, 60.0),
        0.5,
        SphereOptions::default(),
    )
    .unwrap();
    sphere.make_nurb(&k).unwrap();
    let angle = tilt.to_radians();
    let normal = Vec3::new(angle.sin(), 0.0, angle.cos()) * scale;
    let query = Vec3::new(0.2, 0.3, 2.0);
    let expected = 0.5 * normal.length_squared()
        + normal
            .cross(query - Vec3::new(0.0, 0.0, -1.0))
            .cross(normal)
            .length();
    check_face_projection(k, &Surface::from(sphere), normal, expected);
}

#[test]
fn projected_trimmed_sphere_maximum_keeps_the_trim_endpoint() {
    let k = Kernel::default();
    let mut sphere = Sphere::new(
        &k,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Extent2d::from_bounds(0.0, -45.0, 180.0, 45.0),
        0.5,
        SphereOptions::default(),
    )
    .unwrap();
    sphere.make_nurb(&k).unwrap();
    // The unconstrained farthest equator point is outside this half-longitude
    // face. Its farthest included point is the trim endpoint (-0.5, 0, -1).
    let expected = (0.7_f64 * 0.7 + 0.3 * 0.3).sqrt();
    check_face_projection(
        k,
        &Surface::from(sphere),
        Vec3::new(0.0, 0.0, 1.0),
        expected,
    );
}

#[test]
fn projected_torus_maximum_matches_the_capsule_projection() {
    let k = Kernel::default();
    let mut torus = Torus::new(
        &k,
        Vec3::new(0.0, 0.0, -2.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
        1.0,
        0.25,
        TorusOptions::default(),
    )
    .unwrap();
    torus.make_nurb(&k).unwrap();
    // The projected torus is the radius-0.25 capsule around the y-axis segment
    // [-1, 1]. The farther cap is centered at (0, -1).
    let expected = (0.2_f64 * 0.2 + 1.3 * 1.3).sqrt() + 0.25;
    check_face_projection(k, &Surface::from(torus), Vec3::new(0.0, 0.0, 1.0), expected);
}

#[test]
fn projected_plane_maximum_uses_its_boundary_without_a_silhouette() {
    let k = Kernel::default();
    let domain = Extent2d::from_bounds(-0.5, -0.5, 0.5, 0.5);
    let surface = Surface::from(crate::surface::plane::Plane::new(
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(1.0, 1.0),
        &domain,
    ));
    check_face_projection(
        k,
        &surface,
        Vec3::new(0.0, 0.0, 1.0),
        (0.7_f64 * 0.7 + 0.8 * 0.8).sqrt(),
    );
}

#[test]
fn projected_sphere_band_maximum_uses_a_trim_away_from_the_silhouette() {
    let k = Kernel::default();
    let mut sphere = Sphere::new(
        &k,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Extent2d::from_bounds(0.0, 15.0, 360.0, 60.0),
        0.5,
        SphereOptions::default(),
    )
    .unwrap();
    sphere.make_nurb(&k).unwrap();
    let expected = 0.5 * 15.0_f64.to_radians().cos() + (0.2_f64 * 0.2 + 0.3 * 0.3).sqrt();
    check_face_projection(
        k,
        &Surface::from(sphere),
        Vec3::new(0.0, 0.0, 1.0),
        expected,
    );
}

#[test]
fn projected_sphere_band_original_maximum_is_not_certified() {
    // The analytic_face_sphere_band_5_15 fixture: band [-60, 60] degrees, a
    // vertex query, view Z. The maximum sqrt(0.13) + 0.5 is certified; the
    // value 0.8605419808705617, measured on a fitted silhouette, is not.
    let k = Kernel::default();
    let mut sphere = Sphere::new(
        &k,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::X,
        Vec3::Y,
        Extent2d::from_bounds(0.0, -60.0, 360.0, 60.0),
        0.5,
        SphereOptions::default(),
    )
    .unwrap();
    sphere.make_nurb(&k).unwrap();
    let expected = 0.5 + (0.2_f64 * 0.2 + 0.3 * 0.3).sqrt();
    let (k, net, domain) =
        check_face_projection_kind(k, &Surface::from(sphere), Vec3::Z, expected, true);
    assert!(!super::projected_bound::maximum_is_bounded(
        &k,
        &net,
        &domain,
        FACE_QUERY,
        Vec3::Z,
        0.8605419808705617 + 1e-5,
    )
    .unwrap());
}

#[test]
fn projected_brep_maximum_is_certified_in_either_face_order() {
    for (specs, query_x, expected) in [
        (vec![(0.0, 0.5), (0.1, 0.6)], 1e-6, 0.7 - 1e-6),
        (vec![(0.1, 0.6), (0.0, 0.5)], 1e-6, 0.7 - 1e-6),
        // All seam vertices project to (0.25, 0): a zero vertex answer
        // must not terminate a maximum query before its edges and face.
        (vec![(0.0, 0.5)], 0.25, 0.75),
    ] {
        let k = Kernel::default();
        let surfaces = specs
            .iter()
            .map(|&(x, radius)| {
                let mut sphere = Sphere::new(
                    &k,
                    Vec3::new(x, 0.0, -1.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Extent2d::from_bounds(0.0, -60.0, 360.0, 60.0),
                    radius,
                    SphereOptions::default(),
                )
                .unwrap();
                sphere.make_nurb(&k).unwrap();
                Surface::from(sphere)
            })
            .collect();
        let tol = 1e-8;
        let answer = brep_projected_maximum(k, surfaces, Vec3::new(query_x, 0.0, 2.0), tol)
            .unwrap_or_else(|error| panic!("specs={specs:?}, query_x={query_x}: {error:?}"));
        assert_eq!(answer.len(), 1);
        assert!((answer.as_slice()[0].start.solution_value - expected).abs() <= tol);
    }
}

/// Builds one face per surface in a new B-rep and asks its tree for the single
/// projected maximum from `query`, viewed along Z.
fn brep_projected_maximum(
    mut k: Kernel,
    surfaces: Vec<Surface>,
    query: Vec3,
    tol: f64,
) -> crate::Result<crate::geom::solution_array::SolutionArray<CacheObject>> {
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut faces = Vec::new();
    for surface in surfaces {
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let keys = |f| arena_identity(CacheObject::Face(f));
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &keys,
        };
        faces.push(
            create_face_from_surface(&mut k, &mut b, Some(surface), &domain, &mut fenv).unwrap(),
        );
    }
    let mut preparation = crate::query::brep::RayPreparation::default();
    for face in faces {
        {
            let objects = BrepObjects {
                preparation: Some(&preparation),
                brep: &b,
                brep_id: BrepId::from_index(0),
                cache: None,
                keys: &arena_identity,
                owner_of: &|_| None,
            };
            let mut context = objects.surface_solve_context().unwrap();
            assert!(matches!(
                context.build_surface_cache(
                    &k,
                    b.face(face).unwrap().surface.as_ref().unwrap(),
                    Some(CacheObject::Surface(face))
                ),
                Err(crate::SolidError::PreparationRequired { .. })
            ));
        }
        let request = preparation.take_request().unwrap();
        preparation.prepare(&k, &mut b, request).unwrap();
    }
    let mut cache =
        crate::topology::brep_cache::BrepCache::new(crate::geom::a_object::BREP_TYPE).unwrap();
    cache
        .build_trees(
            &k,
            crate::topology::brep_cache::CacheSource::Brep(&b),
            |o| o,
            &arena_identity,
        )
        .unwrap();
    let objects = BrepObjects {
        preparation: Some(&preparation),
        brep: &b,
        brep_id: BrepId::from_index(0),
        cache: Some(&cache),
        keys: &arena_identity,
        owner_of: &|_| None,
    };
    TopologySolver::brep_point_solve(
        &k,
        &objects,
        Some(CacheObject::Brep),
        query,
        crate::intersect::topology_solver::p2::TopologySolveOptions {
            operation: SolverOperation::ProjectedMaximize,
            requested: SolutionRequested::Single,
            tolerance_3d: tol,
            best_answer_so_far_sq: 0.0,
            opt_vectors: &[Vec3::new(0.0, 0.0, 1.0)],
        },
    )
}

#[test]
fn projected_full_sphere_pole_seeds_remain_only_proposals() {
    let mut k = Kernel::default();
    let mut sphere = Sphere::new(
        &k,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::X,
        Vec3::Y,
        Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
        0.5,
        SphereOptions::default(),
    )
    .unwrap();
    sphere.make_nurb(&k).unwrap();
    let surface = Surface::from(sphere);
    let net = crate::blend::fillet_section::station::Nurbs::of(
        &crate::blend::fillet_section::station::as_b_spline(&k, &surface)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let query = Vec3::new(0.2, 0.3, 2.0);
    let proposals = super::projected_bound::native_uv_proposals(
        &k,
        &surface,
        &net,
        &domain,
        query,
        Vec3::Z,
        &[],
    )
    .unwrap();
    let &(uv, value) = proposals.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let own = super::projected_bound::value_at(&net, uv, query, Vec3::Z).unwrap();
    assert!(own.lo <= value && value <= own.hi);
    assert!(
        super::projected_bound::maximum_is_bounded(&k, &net, &domain, query, Vec3::Z, own.hi)
            .unwrap()
    );
    // Native proposals at degenerate poles do not make a collapsed trim an
    // exact four-edge rectangle. The end-to-end feasibility gate still refuses.
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let keys = |f| arena_identity(CacheObject::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &keys,
    };
    let face = create_face_from_surface(&mut k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
    assert!(!super::projected_bound::is_exact_rectangle(&k, &b, face, &domain).unwrap());
    let objects = BrepObjects {
        preparation: None,
        brep: &b,
        brep_id: BrepId::from_index(0),
        cache: None,
        keys: &arena_identity,
        owner_of: &|_| None,
    };
    assert!(super::projected_bound::native_candidates(
        &k,
        &objects,
        CacheObject::Face(face),
        query,
        &[Vec3::Z],
        &[]
    )
    .is_err());
}

/// Exact native constructors and query values of static-analytic-faces.json's
/// seven entry=5, operation=15 regressions, including the `VertexFace` dispatch.
#[test]
fn native_vertex_face_proposals_cover_the_seven_analytic_fixtures() {
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::cone::Cone;
    for angle in [360.0, 180.0] {
        let k = Kernel::default();
        let mut cylinder = Cone::new_cylinder_bounded(
            Vec3::new(-0.5, 0.0, 0.0),
            Vec3::Y,
            Vec3::Z,
            0.5,
            0.0,
            angle,
            1.0,
            false,
            false,
            false,
        );
        cylinder.make_nurb(&k).unwrap();
        check_face_projection_kind(
            k,
            &Surface::from(cylinder),
            Vec3::Z,
            (0.7_f64 * 0.7 + 0.8 * 0.8).sqrt(),
            true,
        );
    }
    let k = Kernel::default();
    let tangent = (30.0 * std::f64::consts::PI / 180.0).tan();
    let placement = Axis2Placement::new(Vec3::new(-0.5, 0.0, 0.0), Vec3::Y, Vec3::Z);
    let cone = Cone::create_canonical(
        &k,
        &placement,
        0.5 * tangent,
        1.5 * tangent,
        1.0,
        false,
        false,
    )
    .unwrap();
    let y = 1.5 * tangent + 0.3;
    check_face_projection_kind(
        k,
        &Surface::from(cone),
        Vec3::Z,
        (0.3_f64 * 0.3 + y * y).sqrt(),
        true,
    );
    for (angle, latitude, expected) in [
        (360.0, 60.0, 0.5 + (0.2_f64 * 0.2 + 0.3 * 0.3).sqrt()),
        (180.0, 45.0, (0.7_f64 * 0.7 + 0.3 * 0.3).sqrt()),
    ] {
        let k = Kernel::default();
        let mut sphere = Sphere::new(
            &k,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, -latitude, angle, latitude),
            0.5,
            SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k).unwrap();
        check_face_projection_kind(k, &Surface::from(sphere), Vec3::Z, expected, true);
    }
    for angle in [360.0, 180.0] {
        let k = Kernel::default();
        let mut torus = Torus::new(
            &k,
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::Y,
            Vec3::Z,
            Extent2d::from_bounds(0.0, 0.0, angle, 360.0),
            1.0,
            0.25,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        check_face_projection_kind(
            k,
            &Surface::from(torus),
            Vec3::Z,
            0.25 + (0.2_f64 * 0.2 + 1.3 * 1.3).sqrt(),
            true,
        );
    }
}

fn check_face_projection(k: Kernel, surface: &Surface, normal: Vec3, expected: f64) {
    check_face_projection_kind(k, surface, normal, expected, false);
}

/// What a checked face projection leaves for further assertions: the kernel,
/// the face's NURBS net and its domain. The query point is `FACE_QUERY`.
type CheckedFace = (
    Kernel,
    crate::blend::fillet_section::station::Nurbs,
    Extent2d,
);

/// The query point of every `check_face_projection_kind` case.
const FACE_QUERY: Vec3 = Vec3::new(0.2, 0.3, 2.0);

fn check_face_projection_kind(
    mut k: Kernel,
    surface: &Surface,
    normal: Vec3,
    expected: f64,
    vertex_query: bool,
) -> CheckedFace {
    let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let query = FACE_QUERY;
    let vertex = vertex_query.then(|| b.add_vertex(query).unwrap());
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let keys = |f| arena_identity(CacheObject::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &keys,
    };
    let face = create_face_from_surface(&mut k, &mut b, Some(surface.clone()), &domain, &mut fenv)
        .unwrap();
    let mut preparation = crate::query::brep::RayPreparation::default();
    {
        let objects = BrepObjects {
            preparation: Some(&preparation),
            brep: &b,
            brep_id: BrepId::from_index(0),
            cache: None,
            keys: &arena_identity,
            owner_of: &|_| None,
        };
        let mut context = objects.surface_solve_context().unwrap();
        assert!(matches!(
            context.build_surface_cache(
                &k,
                b.face(face).unwrap().surface.as_ref().unwrap(),
                Some(CacheObject::Surface(face))
            ),
            Err(crate::SolidError::PreparationRequired { .. })
        ));
    }
    let request = preparation.take_request().unwrap();
    preparation.prepare(&k, &mut b, request).unwrap();
    assert!(super::projected_bound::is_exact_rectangle(&k, &b, face, &domain).unwrap());
    let objects = BrepObjects {
        preparation: Some(&preparation),
        brep: &b,
        brep_id: BrepId::from_index(0),
        cache: None,
        keys: &arena_identity,
        owner_of: &|_| None,
    };
    let tol = 1e-5;
    let mut solver = TopologySolver::new(
        &k,
        &objects,
        SolverOperation::ProjectedMaximize,
        SolutionRequested::Single,
        tol,
        0.0,
        &[normal],
    );
    let kind = if vertex_query {
        LocalSolverKind::VertexFace
    } else {
        LocalSolverKind::PointFace
    };
    let mut local = LocalSolver::new(kind, false, None);
    local.set_point(query);
    local
        .solve_it(
            &k,
            &mut solver,
            vertex.map(CacheObject::Vertex),
            Some(CacheObject::Surface(face)),
        )
        .unwrap();
    assert_eq!(solver.base.solutions.len(), 1);
    let solution = &solver.base.solutions.as_slice()[0];
    let uv = Vec2::new(
        solution.start.param(0).unwrap(),
        solution.start.param(1).unwrap(),
    );
    let point = surface.evaluate_point(&k, uv).unwrap();
    let measured = normal.cross(point - query).cross(normal).length();
    let net = crate::blend::fillet_section::station::Nurbs::of(
        &crate::blend::fillet_section::station::as_b_spline(&k, surface)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let enclosed = super::projected_bound::value_at(&net, uv, query, normal).unwrap();
    assert!(enclosed.lo <= measured && measured <= enclosed.hi);
    assert!(
        super::projected_bound::maximum_is_bounded(&k, &net, &domain, query, normal, enclosed.hi,)
            .unwrap(),
        "whole-domain bound did not close: normal={normal:?}"
    );
    assert!(
        (solution.start.solution_value - expected).abs() <= tol,
        "value={}, expected={expected}",
        solution.start.solution_value
    );
    assert!(
        (solution.start.solution_value - measured).abs() <= 8.0 * f64::EPSILON * measured.max(1.0),
        "value={}, surface measurement={measured}",
        solution.start.solution_value
    );
    // A plausible scalar is not enough: it must agree with the returned point.
    for delta in [-2.0 * tol, 2.0 * tol] {
        let mut incorrect = solver.base.clone();
        let mut changed = *solution;
        changed.start.solution_value += delta;
        incorrect.solutions.set_at(0, &changed).unwrap();
        assert!(super::projected_bound::certify(
            &k,
            &objects,
            CacheObject::Face(face),
            query,
            &incorrect,
        )
        .is_err());
    }
    // Exercise the public B-rep traversal as well as the reported local solve.
    let mut cache =
        crate::topology::brep_cache::BrepCache::new(crate::geom::a_object::BREP_TYPE).unwrap();
    cache
        .build_trees(
            &k,
            crate::topology::brep_cache::CacheSource::Brep(&b),
            |object| object,
            &arena_identity,
        )
        .unwrap();
    let global_objects = BrepObjects {
        cache: Some(&cache),
        ..objects
    };
    let global = TopologySolver::brep_point_solve(
        &k,
        &global_objects,
        Some(CacheObject::Brep),
        query,
        crate::intersect::topology_solver::p2::TopologySolveOptions {
            operation: SolverOperation::ProjectedMaximize,
            requested: SolutionRequested::Single,
            tolerance_3d: tol,
            best_answer_so_far_sq: 0.0,
            opt_vectors: &[normal],
        },
    )
    .unwrap();
    assert_eq!(global.len(), 1);
    assert!(
        (global.as_slice()[0].start.solution_value - expected).abs() <= tol,
        "global={:?}, normal={normal:?}, expected={expected}",
        global.as_slice()
    );
    (k, net, domain)
}

fn check_maximum_after_near_seam_vertex(operation: SolverOperation) {
    let mut k = Kernel::default();
    let mut torus = Torus::new(
        &k,
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
        1.0,
        0.25,
        TorusOptions::default(),
    )
    .unwrap();
    torus.make_nurb(&k).unwrap();
    let surface = Surface::from(torus);
    let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let keys = |f| arena_identity(CacheObject::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &keys,
    };
    let face = create_face_from_surface(&mut k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
    let vertices = crate::topology::brep::p1::vertices(&b, None).unwrap();
    assert_eq!(vertices.len(), 1);
    assert_eq!(
        b.vertex(vertices[0]).unwrap().point,
        Vec3::new(1.25, 0.0, 0.0)
    );
    // Face construction records notification identities for its seam edges;
    // arena-only keys would silently omit them from the cache tree.
    let proven = crate::query::brep::BrepIdentity::of_brep_with_id(BrepId::from_index(0), &b);
    let identity = proven.identity();
    let mut preparation = crate::query::brep::RayPreparation::default();
    {
        let objects = BrepObjects {
            preparation: Some(&preparation),
            brep: &b,
            brep_id: BrepId::from_index(0),
            cache: None,
            keys: identity.keys,
            owner_of: identity.owner_of,
        };
        let mut context = objects.surface_solve_context().unwrap();
        assert!(matches!(
            context.build_surface_cache(
                &k,
                b.face(face).unwrap().surface.as_ref().unwrap(),
                Some(CacheObject::Surface(face))
            ),
            Err(crate::SolidError::PreparationRequired { .. })
        ));
    }
    let request = preparation.take_request().unwrap();
    preparation.prepare(&k, &mut b, request).unwrap();
    let mut cache =
        crate::topology::brep_cache::BrepCache::new(crate::geom::a_object::BREP_TYPE).unwrap();
    cache
        .build_trees(
            &k,
            crate::topology::brep_cache::CacheSource::Brep(&b),
            |o| o,
            identity.keys,
        )
        .unwrap();
    assert_eq!(cache.curve_tree().unwrap().object_list().unwrap().len(), 2);
    assert_eq!(
        cache.surface_tree().unwrap().object_list().unwrap().len(),
        1
    );
    let objects = BrepObjects {
        preparation: Some(&preparation),
        brep: &b,
        brep_id: BrepId::from_index(0),
        cache: Some(&cache),
        keys: identity.keys,
        owner_of: identity.owner_of,
    };
    let tolerance = 1e-6;
    let offset = if operation == SolverOperation::DirectedMaximize {
        tolerance / 2.0
    } else {
        0.0
    };
    let query = Vec3::new(1.25 + offset, 0.0, 0.0);
    // Every torus point has norm at most R+r=1.25. The triangle bound
    // |C-query| <= 1.25+|query| is attained at (-1.25,0,0). That point
    // also lies on the directed X line in the Z projection, so the same
    // closed form applies to both operations.
    let expected = 2.5 + offset;
    let vectors = [Vec3::Z, Vec3::X];
    let answer = TopologySolver::brep_point_solve(
        &k,
        &objects,
        Some(CacheObject::Brep),
        query,
        crate::intersect::topology_solver::p2::TopologySolveOptions {
            operation,
            requested: SolutionRequested::Single,
            tolerance_3d: tolerance,
            best_answer_so_far_sq: 0.0,
            opt_vectors: &vectors,
        },
    )
    .unwrap();
    assert_eq!(answer.len(), 1);
    let actual = answer.as_slice()[0].start.solution_value;
    assert!(
        (actual - expected).abs() <= tolerance,
        "{operation:?}: actual={actual}, expected={expected}"
    );
}

#[test]
fn distance_maximum_does_not_stop_at_coincident_seam_vertex() {
    check_maximum_after_near_seam_vertex(SolverOperation::Maximize);
}

#[test]
fn directed_maximum_does_not_stop_at_nearby_seam_vertex() {
    check_maximum_after_near_seam_vertex(SolverOperation::DirectedMaximize);
}

fn check_zero_slack_plane_candidate(normal: Vec3) {
    let mut k = Kernel::default();
    let domain = Extent2d::from_bounds(-0.5, -0.5, 0.5, 0.5);
    let surface = &Surface::from(crate::surface::plane::Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &domain,
    ));
    let expected = 2.5_f64.sqrt() * normal.length_squared();
    let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let keys = |f| arena_identity(CacheObject::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &keys,
    };
    let face = create_face_from_surface(&mut k, &mut b, Some(surface.clone()), &domain, &mut fenv)
        .unwrap();
    let mut preparation = crate::query::brep::RayPreparation::default();
    {
        let objects = BrepObjects {
            preparation: Some(&preparation),
            brep: &b,
            brep_id: BrepId::from_index(0),
            cache: None,
            keys: &arena_identity,
            owner_of: &|_| None,
        };
        let mut context = objects.surface_solve_context().unwrap();
        assert!(matches!(
            context.build_surface_cache(
                &k,
                b.face(face).unwrap().surface.as_ref().unwrap(),
                Some(CacheObject::Surface(face))
            ),
            Err(crate::SolidError::PreparationRequired { .. })
        ));
    }
    let request = preparation.take_request().unwrap();
    preparation.prepare(&k, &mut b, request).unwrap();
    let objects = BrepObjects {
        preparation: Some(&preparation),
        brep: &b,
        brep_id: BrepId::from_index(0),
        cache: None,
        keys: &arena_identity,
        owner_of: &|_| None,
    };
    let query = Vec3::new(-1.0, 0.0, 2.0);
    let tol = 0.0;
    let mut solver = TopologySolver::new(
        &k,
        &objects,
        SolverOperation::ProjectedMaximize,
        SolutionRequested::Single,
        tol,
        0.0,
        &[normal],
    );
    let mut local = LocalSolver::new(LocalSolverKind::PointFace, false, None);
    local.set_point(query);
    local
        .solve_it(&k, &mut solver, None, Some(CacheObject::Surface(face)))
        .unwrap_or_else(|error| {
            panic!(
                "{error:?}; candidates={:?}",
                solver.base.solutions.as_slice()
            )
        });
    assert_eq!(solver.base.solutions.len(), 1);
    let solution = &solver.base.solutions.as_slice()[0];
    let uv = Vec2::new(
        solution.start.param(0).unwrap(),
        solution.start.param(1).unwrap(),
    );
    let point = surface.evaluate_point(&k, uv).unwrap();
    let measured = normal.cross(point - query).cross(normal).length();
    // Closed form: the farthest projected corners are (0.5,+/-0.5,0).
    // Candidate construction includes the exact trim endpoints. The explicit
    // caller slack is zero; all remaining room comes from arithmetic bounds.
    assert_eq!(solution.objects[0], Some(CacheObject::Face(face)));
    assert!(domain.contains_point(uv, 0.0));
    assert!((solution.start.solution_value - expected).abs() <= 8.0 * f64::EPSILON * expected);
    assert!((measured - expected).abs() <= 8.0 * f64::EPSILON * expected);
    let net = crate::blend::fillet_section::station::Nurbs::of(
        &crate::blend::fillet_section::station::as_b_spline(&k, surface)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let enclosure = super::projected_bound::value_at(&net, uv, query, normal).unwrap();
    let perturbation = 4.0 * (enclosure.hi - enclosure.lo);
    assert!(perturbation > 0.0);
    for delta in [-perturbation, perturbation] {
        let mut incorrect = solver.base.clone();
        let mut changed = *solution;
        changed.start.solution_value += delta;
        incorrect.solutions.set_at(0, &changed).unwrap();
        assert!(super::projected_bound::certify_candidate(
            &k,
            &objects,
            CacheObject::Face(face),
            query,
            &incorrect,
        )
        .is_err());
        incorrect.tolerance_3d = 10.0 * expected;
        assert!(super::projected_bound::certify_candidate(
            &k,
            &objects,
            CacheObject::Face(face),
            query,
            &incorrect,
        )
        .is_err());
    }
    // An edge incumbent must supply its own verified point metric. Merely
    // trusting its reported value could "certify" this face against an invented
    // larger answer on another object.
    let edge = crate::topology::brep::p1::edges(&b, None).unwrap()[0];
    let edge_object = CacheObject::Edge(edge);
    let curve = objects
        .curve(objects.edge_curve(edge_object).unwrap().unwrap())
        .unwrap();
    let t = objects.edge_interval(edge_object).unwrap().min();
    let edge_point = curve.evaluate_point(&k, t).unwrap();
    let mut incumbent = solver.base.clone();
    let mut edge_answer = *solution;
    edge_answer.objects.fill(None);
    edge_answer.objects[0] = Some(edge_object);
    edge_answer.num_objects = 1;
    edge_answer.num_variables = 1;
    edge_answer.start.parameters[0] = t;
    edge_answer.start.solution_value = normal.cross(edge_point - query).cross(normal).length();
    incumbent.solutions.set_at(0, &edge_answer).unwrap();
    super::projected_bound::certify_candidate(
        &k,
        &objects,
        CacheObject::Face(face),
        query,
        &incumbent,
    )
    .unwrap();
    edge_answer.start.solution_value = 10.0 * expected;
    incumbent.tolerance_3d = 100.0 * expected;
    incumbent.solutions.set_at(0, &edge_answer).unwrap();
    assert!(super::projected_bound::certify(
        &k,
        &objects,
        CacheObject::Face(face),
        query,
        &incumbent,
    )
    .is_err());

    // Retention slack must not make a feasible but suboptimal point a maximum.
    // For these normals parallel to Z, the center has projected distance
    // |normal|^2, below sqrt(2.5)|normal|^2.
    let mut suboptimal = solver.base.clone();
    suboptimal.tolerance_3d = 10.0 * expected;
    let mut changed = *solution;
    changed.start.parameters[0] = 0.0;
    changed.start.parameters[1] = 0.0;
    changed.start.solution_value = normal.length_squared();
    suboptimal.solutions.set_at(0, &changed).unwrap();
    super::projected_bound::certify_candidate(
        &k,
        &objects,
        CacheObject::Face(face),
        query,
        &suboptimal,
    )
    .unwrap();
    assert!(super::projected_bound::certify(
        &k,
        &objects,
        CacheObject::Face(face),
        query,
        &suboptimal,
    )
    .is_err());
}

#[test]
fn zero_slack_projected_face_maximum_accepts_plane_corner() {
    check_zero_slack_plane_candidate(Vec3::Z);
    check_zero_slack_plane_candidate(Vec3::Z * 2.0);
}

/// Each vertex-face leaf must be certified against its own point before global merging.
#[test]
fn projected_two_tree_vertex_face_keeps_each_query_point_separate() {
    for reverse in [false, true] {
        for swap in [false, true] {
            check_vertex_face_incumbent(reverse, swap, 0.0);
            check_vertex_face_incumbent(reverse, swap, 25.0);
        }
    }
}

/// Use conservative boxes to visit both vertices even after the farther one wins.
fn check_vertex_face_incumbent(reverse: bool, swap: bool, cutoff_squared: f64) {
    use crate::{
        geom::{
            extent2d::Extent2d,
            extent3d::Extent3d,
            global_solver::{SolutionRequested, SolverOperation},
            tol::USE_DEFAULT,
            tree::{NodeGeomType, Tree},
            vector2d::Vec2,
        },
        intersect::topology_solver::{
            p1::{TopologyObjects, TopologySolver},
            p2::TopologyTrees,
        },
        math::Vec3,
        surface::{plane::Plane, surface::Surface},
        topology::{
            brep::{
                p2::ArenaNotifyState,
                p3::new_brep,
                p8::{create_face_from_surface, FaceBuildEnv},
            },
            brep_cache::{arena_identity, BrepObjects, CacheObject},
            topology_traverser::Marks,
            BrepId,
        },
        Kernel,
    };

    let mut k = Kernel::default();
    let domain = Extent2d::from_bounds(0., 0., 1., 1.);
    let surface = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1., 1.),
        &domain,
    ));
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let keys = |f| arena_identity(CacheObject::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &keys,
    };
    let face = create_face_from_surface(&mut k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
    let far = b.add_vertex(Vec3::new(-3., 0., 0.)).unwrap();
    let near = b.add_vertex(Vec3::new(-1., 0., 0.)).unwrap();
    let mut prep = crate::query::brep::RayPreparation::default();
    {
        let objects = BrepObjects {
            preparation: Some(&prep),
            brep: &b,
            brep_id: BrepId::from_index(0),
            cache: None,
            keys: &arena_identity,
            owner_of: &|_| None,
        };
        let mut context = objects.surface_solve_context().unwrap();
        assert!(matches!(
            context.build_surface_cache(
                &k,
                b.face(face).unwrap().surface.as_ref().unwrap(),
                Some(CacheObject::Surface(face))
            ),
            Err(crate::SolidError::PreparationRequired { .. })
        ));
    }
    let req = prep.take_request().unwrap();
    prep.prepare(&k, &mut b, req).unwrap();
    let objects = BrepObjects {
        preparation: Some(&prep),
        brep: &b,
        brep_id: BrepId::from_index(0),
        cache: None,
        keys: &arena_identity,
        owner_of: &|_| None,
    };
    let extent = Extent3d::from_points(Vec3::new(-20., -20., -20.), Vec3::new(20., 20., 20.));
    let mut vertices = Tree::new_spatial(&extent);
    for v in if reverse { [near, far] } else { [far, near] } {
        vertices
            .add_to_spatial_tree(&extent, Some(CacheObject::Vertex(v)), NodeGeomType::Default)
            .unwrap();
    }
    let mut surfaces = Tree::new_spatial(&extent);
    surfaces
        .add_to_spatial_tree(
            &extent,
            Some(CacheObject::Surface(face)),
            NodeGeomType::Default,
        )
        .unwrap();
    let empty = Tree::from_point(Vec3::ZERO, 0.0, false);
    let first = TopologyTrees {
        vertices: &vertices,
        curves: &empty,
        surfaces: &empty,
    };
    let second = TopologyTrees {
        vertices: &empty,
        curves: &empty,
        surfaces: &surfaces,
    };
    let mut solver = TopologySolver::new(
        &k,
        &objects,
        SolverOperation::ProjectedMaximize,
        SolutionRequested::Single,
        0.,
        cutoff_squared,
        &[Vec3::Z],
    );
    let result = if swap {
        solver.solve_two_tree_sets(second, first)
    } else {
        solver.solve_two_tree_sets(first, second)
    };
    result.unwrap();
    if cutoff_squared == 25.0 {
        // The current vertex is certified locally, but its answer must still
        // respect the caller's stronger incumbent when merged globally.
        assert!(solver.base.solutions.is_empty());
        return;
    }
    let s = &solver.base.solutions.as_slice()[0];
    assert_eq!(s.start.solution_value, 17f64.sqrt());
    assert!(s.objects.contains(&Some(CacheObject::Vertex(far))));
}
