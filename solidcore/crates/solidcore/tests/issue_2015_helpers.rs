//! Equivalence and analytic checks of the convenience APIs from issue #2015.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test assertions

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletSolverId;
use solidcore::blend::fillet_solver::FilletSurfaceGenerator;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{ids, Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::heal::face_props::Tri as ExistingTri;
use solidcore::mass::integrator::properties::{
    brep_mass_properties, brep_mass_properties_default, brep_volume, brep_volume_default,
    PropertyAttributes,
};
use solidcore::primitive::api_primitives::{create_box, create_plane};
use solidcore::query::brep::BrepFaceServices;
use solidcore::query::face_surface_intersect;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceIntersection, SurfaceKind};
use solidcore::topology::brep::{edges, faces, stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::face;
use solidcore::topology::shell::{is_inner_shell_of, is_inner_shell_of_tri};
use solidcore::topology::{Region, ShellId};
use solidcore::{Brep, EdgeId, EntityRef, Kernel, Point2, Point3, Tri, Vec3};

#[test]
fn typed_predicates_preserve_all_three_states_and_reject_invalid_flags() {
    let mut region = Region::new();
    for (raw, expected) in [(0, Tri::False), (1, Tri::True), (2, Tri::Unsure)] {
        let typed = Tri::try_from(raw).unwrap();
        assert_eq!(typed, expected);
        assert_eq!(typed.code(), i64::from(raw));
        assert_eq!(typed.is_true(), raw == 1);
        assert_eq!(typed.is_false(), raw == 0);
        region.set_is_void(raw);
        assert_eq!(region.is_void(), raw);
        assert_eq!(region.void_state().unwrap(), expected);
    }
    for raw in [-1, 3, i32::MIN, i32::MAX] {
        assert!(Tri::try_from(raw).is_err());
        region.set_is_void(raw);
        assert!(region.void_state().is_err());
    }
    // The public alias is the existing type, not a separate enum.
    let _: ExistingTri = Tri::Unsure;
}

#[test]
fn typed_shell_query_keeps_the_raw_querys_result_and_rng_state() {
    let mut k_raw = Kernel::default();
    let mut k_typed = Kernel::default();
    let mut raw = create_box(&mut k_raw, Point3::ZERO, 2.0, 3.0, 4.0).unwrap();
    let mut typed = create_box(&mut k_typed, Point3::ZERO, 2.0, 3.0, 4.0).unwrap();
    let shell = raw.shells().ids().next().unwrap();
    let expected = is_inner_shell_of(&mut k_raw, &mut raw, shell, None).unwrap();
    let actual = is_inner_shell_of_tri(&mut k_typed, &mut typed, shell, None).unwrap();
    assert_eq!(actual, Tri::try_from(expected).unwrap());
    assert_eq!(k_raw.rng.next_u64(), k_typed.rng.next_u64());
    assert!(is_inner_shell_of_tri(
        &mut k_typed,
        &mut typed,
        ShellId::from_index(u32::MAX),
        None
    )
    .is_err());
}

#[test]
fn default_mass_helpers_match_empty_attributes_and_the_boxs_closed_form() {
    let mut k = Kernel::default();
    let mut body = create_box(&mut k, Point3::ZERO, 2.0, 3.0, 4.0).unwrap();
    let face = faces(&body, None).unwrap()[0];
    body.add_attribute(
        EntityRef::Face(face),
        Attribute::vector3d(
            ids::MASS_PROPERTIES,
            Vec3::new(100.0, 0.0, 0.0),
            Behavior::Copy,
        ),
    )
    .unwrap();
    let before_attributes = body.entity_attributes(EntityRef::Face(face)).len();
    let store = AttributeStore::default();
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    assert_eq!(
        brep_volume_default(&k, &mut body, 1e-8).unwrap(),
        brep_volume(&k, &mut body, 1e-8, &attributes).unwrap()
    );
    assert!((brep_volume_default(&k, &mut body, 1e-8).unwrap() - 24.0).abs() < 24.0e-9);
    for origin in [Point3::ZERO, Point3::new(123.0, -45.0, 6.0)] {
        let actual = brep_mass_properties_default(&k, &mut body, 1e-8, 2.0, origin).unwrap();
        let expected = brep_mass_properties(&k, &mut body, 1e-8, 2.0, origin, &attributes).unwrap();
        assert_eq!(actual, expected);
        assert!((actual.properties.mass - 48.0).abs() < 48.0e-9);
        assert!((actual.centroid - Point3::new(1.0, 1.5, 2.0)).length() < 1e-9);
    }
    assert_eq!(
        body.entity_attributes(EntityRef::Face(face)).len(),
        before_attributes
    );
    assert!(brep_volume_default(&k, &mut body, f64::NAN).is_err());
    assert!(brep_mass_properties_default(&k, &mut body, 1e-8, -1.0, Point3::ZERO).is_err());
}

fn curve_samples(k: &Kernel, curve: &Curve) -> Vec<Point3> {
    let interval = curve.natural_interval();
    [interval.min(), interval.mid(), interval.max()]
        .into_iter()
        .map(|t| curve.evaluate_point(k, t).unwrap())
        .collect()
}

fn assert_intersections_equal(k: &Kernel, a: &SurfaceIntersection, b: &SurfaceIntersection) {
    assert_eq!(a.curve_types, b.curve_types);
    assert_eq!(a.deviations, b.deviations);
    assert_eq!(a.curves_3d.len(), b.curves_3d.len());
    for (a, b) in a.curves_3d.iter().zip(&b.curves_3d) {
        assert_eq!(curve_samples(k, a), curve_samples(k, b));
    }
    for (a, b) in [
        (&a.surface1_uv_curves, &b.surface1_uv_curves),
        (&a.surface2_uv_curves, &b.surface2_uv_curves),
    ] {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(
                a.as_ref().map(|c| curve_samples(k, c)),
                b.as_ref().map(|c| curve_samples(k, c))
            );
        }
    }
}

#[test]
fn face_surface_helper_preserves_domains_tolerances_values_and_trim_side_effects() {
    let mut k = Kernel::default();
    for (low, high, tol) in [
        (-5.0, 5.0, None),
        (-0.25, 0.5, Some(1e-7)),
        (10.0, 11.0, Some(1e-5)),
    ] {
        let mut actual_body = create_plane(&mut k, Point3::ZERO, 2.0, 3.0).unwrap();
        let mut expected_body = create_plane(&mut k, Point3::ZERO, 2.0, 3.0).unwrap();
        // The helper must bind the live face even when its trims do not yet exist.
        for body in [&mut actual_body, &mut expected_body] {
            let uses: Vec<_> = body.edgeuses().ids().collect();
            for id in uses {
                body.edgeuse_mut(id).unwrap().uv_trim_curve = None;
            }
        }
        let face = faces(&actual_body, None).unwrap()[0];
        let tool = Surface::from(Plane::from_normal(Point3::new(1.0, 0.0, 0.0), Vec3::X));
        let tool_domain = Extent2d::from_points(Point2::new(low, low), Point2::new(high, high));
        let expected = {
            let surface = face::surface(&expected_body, face)
                .unwrap()
                .unwrap()
                .clone_for_evaluation()
                .unwrap();
            let domain = face::uv_domain(&expected_body, face).unwrap();
            let mut services = BrepFaceServices::new(&mut expected_body);
            surface
                .global_surface_intersect_with_context(
                    &k,
                    [Some(CacheObject::Surface(face)), None],
                    &mut services,
                    &domain,
                    &tool,
                    &tool_domain,
                    [true, true],
                    tol,
                    None,
                )
                .unwrap()
        };
        let actual =
            face_surface_intersect(&k, &mut actual_body, face, &tool, &tool_domain, tol).unwrap();
        assert_intersections_equal(&k, &actual, &expected);
        assert_eq!(
            actual_body
                .edgeuses()
                .iter()
                .filter(|(_, u)| u.uv_trim_curve.is_some())
                .count(),
            expected_body
                .edgeuses()
                .iter()
                .filter(|(_, u)| u.uv_trim_curve.is_some())
                .count()
        );
        if high == 5.0 {
            assert!(!actual.curves_3d.is_empty());
            for point in actual.curves_3d.iter().flat_map(|c| curve_samples(&k, c)) {
                assert!((point.x - 1.0).abs() < 1e-9);
                assert!(point.z.abs() < 1e-9);
            }
        }
        // Analytic intersections preserve the existing solver's domain behavior;
        // they do not promise an extra clipping pass against tool_domain.
    }
}

#[test]
fn nurbs_face_intersection_builds_missing_trims_through_the_live_face() {
    let mut k = Kernel::default();
    let mut body = create_plane(&mut k, Point3::ZERO, 2.0, 3.0).unwrap();
    let face = faces(&body, None).unwrap()[0];
    // Preserve the plane and its parameterization while taking the general NURBS
    // solver path, which requires the owning face's cache service.
    let nurbs = match face::surface(&body, face).unwrap().unwrap().kind() {
        SurfaceKind::Plane(plane) => plane.base_with_nurb().unwrap().into_owned(),
        _ => panic!("create_plane must produce a plane"),
    };
    let owner = body.owner_identity(EntityRef::Face(face));
    face::p1::set_surface(&k, &mut body, face, owner, Surface::from(nurbs), true).unwrap();
    let uses: Vec<_> = body.edgeuses().ids().collect();
    for id in uses {
        body.edgeuse_mut(id).unwrap().uv_trim_curve = None;
    }
    let surface = face::surface(&body, face)
        .unwrap()
        .unwrap()
        .clone_for_evaluation()
        .unwrap();
    let domain = face::uv_domain(&body, face).unwrap();
    let tool = Surface::from(Plane::from_normal(Point3::new(1.0, 0.0, 0.0), Vec3::X));
    let tool_domain = Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0);
    assert!(surface
        .global_surface_intersect(
            &k,
            &domain,
            &tool,
            &tool_domain,
            [true, true],
            Some(1e-7),
            None
        )
        .is_err());
    let actual =
        face_surface_intersect(&k, &mut body, face, &tool, &tool_domain, Some(1e-7)).unwrap();
    assert!(!actual.curves_3d.is_empty());
    for point in actual.curves_3d.iter().flat_map(|c| curve_samples(&k, c)) {
        assert!((point.x - 1.0).abs() < 1e-9);
        assert!(point.z.abs() < 1e-9);
    }
    assert!(body
        .edgeuses()
        .iter()
        .any(|(_, u)| u.uv_trim_curve.is_some()));
}

fn offsets(session: &FilletSession<'_>, index: u32) -> [f64; 2] {
    session
        .solver(FilletSolverId(index))
        .unwrap()
        .base()
        .offsets_data()
        .unwrap()
        .surfaces
        .map(|s| s.unwrap().surface.offset_distance().abs())
}

#[test]
fn two_offsets_preserve_independent_distances_tolerances_and_generators() {
    let mut k = Kernel::default();
    let mut body = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edges = edges(&body, None).unwrap();
    let mut session = FilletSession::new(&mut k, &mut body).unwrap();
    let generators = [
        FilletSurfaceGenerator::Linear,
        FilletSurfaceGenerator::circular_default(),
    ];
    session
        .prepare_per_edge_two_offsets(&k, &edges[..2], &[0.5, 0.75], &[1.0, 1.25], &generators)
        .unwrap();
    assert_eq!(offsets(&session, 0), [0.5, 1.0]);
    assert_eq!(offsets(&session, 1), [0.75, 1.25]);
    assert!(session.executive().initialized);
    assert!(!session.executive().options.global_merge);
    assert_eq!(session.executive().solvers.len(), 2);
    assert_eq!(session.executive().surface_generators.len(), 2);
    for (index, generator) in generators.iter().enumerate() {
        let base = session.solver(FilletSolverId(index as u32)).unwrap().base();
        assert_eq!(base.fillet_surface_generator(), Some(generator));
        assert_eq!(
            base.this_approx_tol_3d(),
            4.0 * solidcore::topology::edge::tolerance(session.target(), edges[index]).unwrap()
        );
        assert_eq!(
            base.this_ang_tol_rad(),
            20.0 * (solidcore::math::PI / 180.0)
        );
        assert_eq!(
            base.tangency_tolerance(),
            10.0 * (solidcore::math::PI / 180.0)
        );
    }
}

#[test]
fn two_offsets_broadcast_lists_and_reject_bad_inputs_before_mutation() {
    let mut k = Kernel::default();
    let mut body = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edges = edges(&body, None).unwrap();
    let mut session = FilletSession::new(&mut k, &mut body).unwrap();
    for (selected, first, second, generators) in [
        (
            &[][..],
            &[0.5][..],
            &[1.0][..],
            &[FilletSurfaceGenerator::Linear][..],
        ),
        (
            &edges[..2],
            &[][..],
            &[1.0][..],
            &[FilletSurfaceGenerator::Linear][..],
        ),
        (
            &edges[..2],
            &[0.5][..],
            &[1.0, 2.0, 3.0][..],
            &[FilletSurfaceGenerator::Linear][..],
        ),
        (&edges[..2], &[0.5][..], &[1.0][..], &[][..]),
        (
            &[EdgeId::from_index(u32::MAX)][..],
            &[0.5][..],
            &[1.0][..],
            &[FilletSurfaceGenerator::Linear][..],
        ),
    ] {
        assert!(session
            .prepare_per_edge_two_offsets(&k, selected, first, second, generators)
            .is_err());
        assert!(session.executive().solvers.is_empty());
        assert!(session.executive().surface_generators.is_empty());
        assert!(!session.executive().initialized);
    }
    session
        .prepare_per_edge_two_offsets(
            &k,
            &edges[..2],
            &[0.5],
            &[1.0, 1.25],
            &[FilletSurfaceGenerator::Linear],
        )
        .unwrap();
    assert_eq!(offsets(&session, 0), [0.5, 1.0]);
    assert_eq!(offsets(&session, 1), [0.5, 1.25]);
    assert_eq!(session.executive().surface_generators.len(), 1);
}

fn chamfered_box(manual: bool) -> (Brep, f64) {
    let mut k = Kernel::default();
    let mut body = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = edges(&body, None).unwrap()[0];
    let curve = solidcore::topology::edge::curve(&body, edge)
        .unwrap()
        .unwrap();
    let interval = solidcore::topology::edge::interval(&body, edge).unwrap();
    let length = (curve.evaluate_point(&k, interval.max()).unwrap()
        - curve.evaluate_point(&k, interval.min()).unwrap())
    .length();
    {
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        if manual {
            let generator = session
                .add_surface_generator(FilletSurfaceGenerator::Linear)
                .unwrap();
            let eu = session.blend_edgeuse(&k, edge).unwrap().unwrap();
            let tol = 4.0 * solidcore::topology::edge::tolerance(session.target(), edge).unwrap();
            let solver = session
                .new_constant_radius_solver_with_second(
                    &k,
                    eu,
                    0.5,
                    1.0,
                    tol,
                    20.0 * (solidcore::math::PI / 180.0),
                    10.0 * (solidcore::math::PI / 180.0),
                )
                .unwrap();
            session.load_solver(&k, solver).unwrap();
            session.set_solver_generator(solver, generator).unwrap();
            session.create_corners(&k, None).unwrap();
            session.do_filleting(&mut k, None).unwrap();
        } else {
            session
                .prepare_per_edge_two_offsets(
                    &k,
                    &[edge],
                    &[0.5],
                    &[1.0],
                    &[FilletSurfaceGenerator::Linear],
                )
                .unwrap();
            session.do_fillet(&mut k).unwrap();
        }
    }
    stitch_and_orient(&mut k, &mut body, &StitchAndOrientOptions::default()).unwrap();
    body.refresh_cached_topology();
    let volume = brep_volume_default(&k, &mut body, 1e-8).unwrap();
    assert!((volume - (6000.0 - 0.5 * 0.5 * 1.0 * length)).abs() < 6000.0e-9);
    (body, volume)
}

#[test]
fn two_offset_chamfer_completes_and_matches_the_existing_manual_sequence() {
    let (actual, volume) = chamfered_box(false);
    let (expected, expected_volume) = chamfered_box(true);
    assert_eq!(volume, expected_volume);
    assert_eq!(actual.face_list().len(), 7);
    assert_eq!(actual.face_list().len(), expected.face_list().len());
    assert_eq!(actual.edge_list().len(), expected.edge_list().len());
    assert_eq!(actual.vertices().len(), expected.vertices().len());
    assert_eq!(
        actual
            .vertices()
            .iter()
            .map(|(_, v)| v.point)
            .collect::<Vec<_>>(),
        expected
            .vertices()
            .iter()
            .map(|(_, v)| v.point)
            .collect::<Vec<_>>()
    );
}

#[test]
fn two_offset_chamfers_honor_the_vertex_fan_fix_and_cleanup() {
    for (per_fan, offset) in [(false, 1.0), (true, 1.0), (true, 30.0)] {
        let mut k = Kernel::default();
        k.fixes.fillet_corner_per_vertex_fan = per_fan;
        let cube = create_box(&mut k, Point3::new(-10.0, -10.0, -10.0), 20.0, 20.0, 20.0).unwrap();
        let up = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let down = create_box(&mut k, Point3::new(-10.0, -10.0, -10.0), 10.0, 10.0, 10.0).unwrap();
        let once = solidcore::boolean::api_brep::boolean_difference(&mut k, cube, up).unwrap();
        let mut body =
            solidcore::boolean::api_brep::boolean_difference(&mut k, once, down).unwrap();
        // WR-665's up/down squares: the two concave axis edges meet at a
        // self-touching vertex and belong to independent face fans.
        let selected: Vec<_> = body
            .edges()
            .ids()
            .filter(|&edge| {
                let (a, b) = solidcore::topology::edge::start_end_vertices(&body, edge).unwrap();
                let a = solidcore::topology::vertex::point(&body, a.unwrap()).unwrap();
                let b = solidcore::topology::vertex::point(&body, b.unwrap()).unwrap();
                a.x.abs() < 1e-9
                    && a.y.abs() < 1e-9
                    && b.x.abs() < 1e-9
                    && b.y.abs() < 1e-9
                    && a.distance(b) > 1e-9
            })
            .collect();
        assert_eq!(selected.len(), 2);
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_per_edge_two_offsets(
                &k,
                &selected,
                &[offset],
                &[offset],
                &[FilletSurfaceGenerator::Linear],
            )
            .unwrap();
        assert_eq!(
            session.executive().fan_split_points.len(),
            usize::from(per_fan)
        );
        let result = session.do_fillet(&mut k);
        if per_fan && offset == 1.0 {
            result.unwrap();
            drop(session);
            body.refresh_cached_topology();
            let volume = brep_volume_default(&k, &mut body, 1e-8).unwrap();
            assert!((volume - 6010.0).abs() < 6010.0e-9);
        } else {
            assert!(result.is_err());
            if !per_fan {
                assert_eq!(session.error_info().code, 3);
            }
            drop(session);
            // Failure with the fix enabled joins the split fans again; the
            // default path keeps the original shared vertex.
            assert_eq!(
                body.vertices()
                    .iter()
                    .filter(|(_, v)| v.point.distance(Point3::ZERO) < 1e-9)
                    .count(),
                1
            );
        }
    }
}
