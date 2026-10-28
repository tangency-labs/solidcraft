//! Public transforms retain producer ownership without admitting foreign geometry.
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p1, p3};
use solidcore::topology::proto_topology::p1::api_transform::{api_translate, TransformObject};
use solidcore::{Brep, EntityRef, Kernel};

fn samples(k: &Kernel, b: &Brep) -> solidcore::Result<(Point3, Point3)> {
    let missing = || solidcore::SolidError::invalid_input("transform_test", "missing geometry");
    let edges = p1::edges(b, None)?;
    let faces = p1::faces(b, None)?;
    let edge = b.edge(*edges.first().ok_or_else(missing)?)?;
    let face = b.face(*faces.first().ok_or_else(missing)?)?;
    Ok((
        edge.curve
            .as_ref()
            .ok_or_else(missing)?
            .evaluate_point(k, edge.interval.mid())?,
        face.surface
            .as_ref()
            .ok_or_else(missing)?
            .evaluate_point(k, face.uv_domain.evaluate(0.37, 0.61))?,
    ))
}

#[test]
fn clone_does_not_rebind_unowned_geometry() {
    let mut k = Kernel::default();
    let original = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
    let mut cloned = original.clone();
    let before = samples(&k, &cloned).unwrap();
    let delta = Vec3::new(7.0, -2.0, 3.0);
    api_translate(&mut k, Some(TransformObject::Brep(&mut cloned)), delta).unwrap();
    let after = samples(&k, &cloned).unwrap();
    assert_eq!(after, before);
    let vertex = p1::vertices(&cloned, None).unwrap()[0];
    assert_eq!(
        cloned.vertex(vertex).unwrap().point,
        original.vertex(vertex).unwrap().point + delta
    );
    assert_eq!(samples(&k, &original).unwrap(), before);
}

#[test]
fn foreign_and_unowned_geometry_remains_filtered() {
    for foreign in [false, true] {
        let mut k = Kernel::default();
        let mut body = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
        let mut other = create_box(&mut k, Point3::new(20.0, 30.0, 50.0), 2.0, 3.0, 4.0).unwrap();
        let serial = p3::notify_serial(&mut k.context, &mut other).unwrap();
        let other_edge = p1::edges(&other, None).unwrap()[0];
        let other_face = p1::faces(&other, None).unwrap()[0];
        let curve_owner = foreign.then(|| p3::topology_ref(serial, EntityRef::Edge(other_edge)));
        let surface_owner = foreign.then(|| p3::topology_ref(serial, EntityRef::Face(other_face)));
        let edge = p1::edges(&body, None).unwrap()[0];
        let face = p1::faces(&body, None).unwrap()[0];
        body.edge_mut(edge)
            .unwrap()
            .curve
            .as_mut()
            .unwrap()
            .set_owner(curve_owner, None);
        body.face_mut(face)
            .unwrap()
            .surface
            .as_mut()
            .unwrap()
            .set_owner(surface_owner);
        let before = samples(&k, &body).unwrap();
        let vertex = p1::vertices(&body, None).unwrap()[0];
        let vertex_before = body.vertex(vertex).unwrap().point;
        let delta = Vec3::new(7.0, -2.0, 3.0);
        api_translate(&mut k, Some(TransformObject::Brep(&mut body)), delta).unwrap();
        assert_eq!(samples(&k, &body).unwrap(), before);
        assert_eq!(body.vertex(vertex).unwrap().point, vertex_before + delta);
        assert_eq!(
            body.edge(edge).unwrap().curve.as_ref().unwrap().owner(),
            curve_owner
        );
        assert_eq!(
            body.face(face).unwrap().surface.as_ref().unwrap().owner(),
            surface_owner
        );
    }
}

#[test]
fn separate_bodies_emit_distinct_notification_identities() {
    use solidcore::geom::context::NotifyCallback;
    use solidcore::geom::object::{NotifyOperation, ObjectRef};
    use std::sync::{Arc, Mutex};
    #[derive(Debug)]
    struct Recorder(Arc<Mutex<Vec<ObjectRef>>>);
    impl NotifyCallback for Recorder {
        fn execute(
            &mut self,
            operation: NotifyOperation,
            caller: ObjectRef,
            _: Option<ObjectRef>,
            _: Option<ObjectRef>,
            _: Option<ObjectRef>,
        ) -> solidcore::Result<()> {
            if operation == NotifyOperation::PreEdit {
                self.0.lock().unwrap().push(caller);
            }
            Ok(())
        }
    }
    let mut k = Kernel::default();
    let mut first = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
    let mut second = create_box(&mut k, Point3::new(20.0, 30.0, 50.0), 2.0, 3.0, 4.0).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    k.context
        .set_user_notify_callback(Some(Box::new(Recorder(Arc::clone(&seen)))));
    let delta = Vec3::new(7.0, -2.0, 3.0);
    api_translate(&mut k, Some(TransformObject::Brep(&mut first)), delta).unwrap();
    let first_keys = std::mem::take(&mut *seen.lock().unwrap());
    api_translate(&mut k, Some(TransformObject::Brep(&mut second)), delta).unwrap();
    let second_keys = seen.lock().unwrap();
    assert!(!first_keys.is_empty());
    assert!(!second_keys.is_empty());
    assert!(first_keys.iter().all(|key| !second_keys.contains(key)));
}

#[test]
fn notified_transform_and_brep_query_respect_proven_owners_and_filter_foreign() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::object::ObjectRef;
    use solidcore::intersect::point_class::PointClassType;
    use solidcore::query::brep::BrepQuery;
    use solidcore::topology::brep::p5::transform_with_notifications;
    use solidcore::topology::edge::NotifyEnv;
    use solidcore::topology::NamingScheme;

    let mut k = Kernel::default();
    let origin = Point3::new(120.0, -210.0, 5.0);
    let mut body = create_box(&mut k, origin, 20.0, 30.0, 40.0).unwrap();
    body.set_naming(NamingScheme::Unknown);

    // `BrepQuery::of_brep` resolves proven `create_box` owners even when `NamingScheme`
    // is `Unknown`, classifying an interior point to the solid region.
    let solid = body
        .regions()
        .ids()
        .find(|&r| Some(r) != body.infinite_region())
        .map(EntityRef::Region);
    let inside = origin + Vec3::new(10.0, 15.0, 20.0);
    let pc = BrepQuery::of_brep(&body)
        .point_3d_classify(&k, &mut body, inside, 1e-6, false)
        .unwrap();
    assert_eq!(pc.point_class(), PointClassType::Region);
    assert_eq!(pc.object(), solid);

    // `transform_with_notifications(..., Some(&mut env))` under a distinct caller
    // notification namespace transforms proven-owned geometry while leaving foreign
    // geometry untouched.
    let mut other = create_box(&mut k, Point3::ZERO, 2.0, 2.0, 2.0).unwrap();
    let other_serial = p3::notify_serial(&mut k.context, &mut other).unwrap();
    let foreign_face = p1::faces(&other, None).unwrap()[0];
    let foreign_owner = p3::topology_ref(other_serial, EntityRef::Face(foreign_face));

    let faces = p1::faces(&body, None).unwrap();
    let hijacked_face = faces[0];
    let owned_face = faces[1];
    let hijacked_before = body
        .face(hijacked_face)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .evaluate_point(&k, body.face(hijacked_face).unwrap().uv_domain.mid())
        .unwrap();
    let owned_before = body
        .face(owned_face)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .evaluate_point(&k, body.face(owned_face).unwrap().uv_domain.mid())
        .unwrap();
    body.face_mut(hijacked_face)
        .unwrap()
        .surface
        .as_mut()
        .unwrap()
        .set_owner(Some(foreign_owner));

    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let custom_key = |entity: EntityRef| ObjectRef {
        type_code: p3::topology_ref(1, entity).type_code,
        key: ObjectKey((9u64 << 60) | p3::topology_ref(1, entity).key.0),
    };
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &custom_key,
    };
    let delta = Vec3::new(-120.0, 210.0, -5.0);
    let placement = Axis2Placement::new(delta, Vec3::X, Vec3::Y);
    transform_with_notifications(&mut k, &mut body, &placement, None, false, Some(&mut env))
        .unwrap();

    let hijacked_after = body
        .face(hijacked_face)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .evaluate_point(&k, body.face(hijacked_face).unwrap().uv_domain.mid())
        .unwrap();
    let owned_after = body
        .face(owned_face)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .evaluate_point(&k, body.face(owned_face).unwrap().uv_domain.mid())
        .unwrap();
    assert_eq!(hijacked_after, hijacked_before);
    assert!((owned_after - (owned_before + delta)).length() < 1e-12);
}
