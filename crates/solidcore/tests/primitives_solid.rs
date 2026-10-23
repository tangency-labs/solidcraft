//! The `primitives` example's checks as a test: box, cylinder, cone, pyramid, sphere and
//! torus are closed manifold solids with the analytic Euler characteristic, volume, area
//! and bounding box.

#[path = "../examples/primitives/checks.rs"]
#[allow(dead_code)] // the example also uses its per-check detail (--json)
mod checks;

#[test]
fn complete_spheres_and_tori_have_analytic_properties() {
    let specs = checks::curved_specs();
    for name in ["sphere r3", "sphere r3 (no pole)", "torus R5 r1.5"] {
        let spec = specs.iter().find(|spec| spec.name == name).unwrap();
        checks::check(spec).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn primitives_are_closed_solids_with_analytic_properties() {
    let failures: Vec<String> = checks::specs()
        .iter()
        .filter_map(|s| checks::check(s).err().map(|e| format!("{}: {e}", s.name)))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn precise_sphere_properties_scale_and_translate() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{
        brep_properties, MassPropertyBehavior, PropertyAttributes,
    };
    use solidcore::primitive::api_primitives::{
        create_sphere, create_sphere_no_pole_with_tolerance,
    };
    use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
    use solidcore::{math::Vec3, Kernel};

    // At radius 3 the analytic area and volume are equal, so also test radii
    // that distinguish the two properties and their different scaling laws.
    for (center, radius) in [(Vec3::ZERO, 2.0), (Vec3::new(1.0, -2.0, 0.5), 5.0)] {
        for no_pole in [false, true] {
            let mut k = Kernel::default();
            let b = if no_pole {
                create_sphere_no_pole_with_tolerance(&mut k, center, radius, 1e-9)
            } else {
                create_sphere(&mut k, center, radius)
            }
            .unwrap();
            let bbox = calculate_bounding_box(&k, &b, BoundingBoxOptions::default()).unwrap();
            let store = AttributeStore::default();
            let key_of = |_| ObjectKey(0);
            let properties = brep_properties(
                &k,
                &b,
                1e-10,
                bbox.mid(),
                MassPropertyBehavior::default(),
                &PropertyAttributes {
                    store: &store,
                    key_of: &key_of,
                },
            )
            .unwrap();
            let area = 4.0 * std::f64::consts::PI * radius * radius;
            let volume = area * radius / 3.0;
            assert!((properties.area - area).abs() <= checks::REL_TOL * area);
            assert!((properties.volume - volume).abs() <= checks::REL_TOL * volume);
        }
    }
}

#[test]
#[ignore = "blocked: partial sphere/torus construction needs SmFace::CurveOnClassify"]
fn spheres_and_tori_are_closed_solids_with_analytic_properties() {
    let failures: Vec<String> = checks::curved_specs()
        .iter()
        .filter_map(|s| checks::check(s).err().map(|e| format!("{}: {e}", s.name)))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn pole_free_sphere_tolerance_is_explicit_and_validated() {
    use solidcore::primitive::api_primitives::{
        create_sphere_no_pole, create_sphere_no_pole_with_tolerance,
    };
    use solidcore::{Kernel, SolidError};
    let center = solidcore::math::Vec3::ZERO;
    let mut k = Kernel::default();
    for tolerance in [
        0.0,
        -1e-9,
        solidcore::math::EFF_ZERO,
        f64::NAN,
        f64::INFINITY,
    ] {
        assert!(matches!(
            create_sphere_no_pole_with_tolerance(&mut k, center, 3.0, tolerance),
            Err(SolidError::InvalidInput { .. })
        ));
    }
    let default = create_sphere_no_pole(&mut k, center, 3.0).unwrap();
    let explicit = create_sphere_no_pole_with_tolerance(&mut k, center, 3.0, 1e-9).unwrap();
    assert_eq!(default.tolerance(), 1e-5);
    assert_eq!(explicit.tolerance(), 1e-9);
    assert_eq!(default.face_list().len(), explicit.face_list().len());
    assert_eq!(default.edge_list().len(), explicit.edge_list().len());
    assert_eq!(default.vertex_list().len(), explicit.vertex_list().len());
    for (id, edgeuse) in explicit.edgeuses().iter() {
        let Some(face) = solidcore::topology::edgeuse::p1::face(&explicit, id).unwrap() else {
            continue;
        };
        let edge = explicit.edge(edgeuse.edge.unwrap()).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let trim = solidcore::topology::edgeuse::p1::uv_trim_curve(&explicit, id)
            .unwrap()
            .0
            .unwrap();
        let surface = explicit.face(face).unwrap().surface.as_ref().unwrap();
        for sample in 0..=32 {
            let t = edge.interval.evaluate(f64::from(sample) / 32.0);
            let uv = trim.evaluate_point(&k, t).unwrap();
            let on_surface = surface
                .evaluate_point(&k, solidcore::math::Point2::new(uv.x, uv.y))
                .unwrap();
            let on_edge = curve.evaluate_point(&k, t).unwrap();
            assert!(on_surface.distance(on_edge) <= 1e-9, "{id:?} at {t}");
        }
    }
    let next_default = create_sphere_no_pole(&mut k, center, 3.0).unwrap();
    assert_eq!(next_default.tolerance(), default.tolerance());
}
