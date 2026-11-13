//! API copies retain real caller bindings through later public transforms.
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::context::NotifyCallback;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p1, p2, p3};
use solidcore::topology::proto_topology::p1::api_transform::{api_translate, TransformObject};
use solidcore::{Brep, EntityRef, Kernel};
use std::sync::{Arc, Mutex};

fn key(entity: EntityRef, domain: u64) -> ObjectRef {
    let reference = p3::topology_ref(0, entity);
    ObjectRef {
        type_code: reference.type_code,
        key: ObjectKey(
            domain + reference.type_code as u64 * 1_000_000 + (reference.key.0 & 0xffff_ffff),
        ),
    }
}

fn copy(k: &mut Kernel, source: &Brep, domain: u64) -> solidcore::Result<Brep> {
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    let target_key = |entity| key(entity, domain);
    env.key_of = &target_key;
    p3::api_brep_copy(k, &mut env, Some(source), &|entity| key(entity, 0))
}

fn samples(k: &Kernel, body: &Brep) -> solidcore::Result<(Point3, Point3)> {
    let missing = || solidcore::SolidError::invalid_input("copy_test", "missing geometry");
    let edges = p1::edges(body, None)?;
    let faces = p1::faces(body, None)?;
    let edge = body.edge(*edges.first().ok_or_else(missing)?)?;
    let face = body.face(*faces.first().ok_or_else(missing)?)?;
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
            self.0
                .lock()
                .map_err(|_| {
                    solidcore::SolidError::invalid_input("copy_test", "poisoned recorder")
                })?
                .push(caller);
        }
        Ok(())
    }
}

#[test]
fn copied_caller_bindings_move_geometry_with_distinct_canonical_notifications() {
    let mut k = Kernel::default();
    let source = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
    let source_before = samples(&k, &source).unwrap();
    let mut first = copy(&mut k, &source, 100_000_000).unwrap();
    let mut second = copy(&mut k, &source, 200_000_000).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    k.context
        .set_user_notify_callback(Some(Box::new(Recorder(Arc::clone(&seen)))));
    let delta = Vec3::new(7.0, 11.0, 13.0);
    let mut prior = Vec::new();
    for body in [&mut first, &mut second] {
        let edge = p1::edges(body, None).unwrap()[0];
        let face = p1::faces(body, None).unwrap()[0];
        let owners = (
            body.edge(edge).unwrap().curve.as_ref().unwrap().owner(),
            body.face(face).unwrap().surface.as_ref().unwrap().owner(),
        );
        api_translate(&mut k, Some(TransformObject::Brep(body)), delta).unwrap();
        let after = samples(&k, body).unwrap();
        for (actual, expected) in [
            (after.0, source_before.0 + delta),
            (after.1, source_before.1 + delta),
        ] {
            for (a, e) in [
                (actual.x, expected.x),
                (actual.y, expected.y),
                (actual.z, expected.z),
            ] {
                assert!((a - e).abs() <= 1e-9 * a.abs().max(e.abs()));
            }
        }
        assert_eq!(
            (
                body.edge(edge).unwrap().curve.as_ref().unwrap().owner(),
                body.face(face).unwrap().surface.as_ref().unwrap().owner()
            ),
            owners
        );
        let keys = std::mem::take(&mut *seen.lock().unwrap());
        assert!(!keys.is_empty());
        assert!(keys.iter().all(|key| !prior.contains(key)));
        prior.extend(keys);
    }
    assert_eq!(samples(&k, &source).unwrap(), source_before);
}

#[test]
fn copied_geometry_does_not_accept_foreign_or_absent_owners() {
    for foreign in [false, true] {
        let mut k = Kernel::default();
        let source = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
        let mut body = copy(&mut k, &source, 100_000_000).unwrap();
        let mut other = copy(&mut k, &source, 200_000_000).unwrap();
        let serial = p3::notify_serial(&mut k.context, &mut other).unwrap();
        let edge = p1::edges(&body, None).unwrap()[0];
        let face = p1::faces(&body, None).unwrap()[0];
        let edge_owner = foreign.then(|| {
            p3::topology_ref(serial, EntityRef::Edge(p1::edges(&other, None).unwrap()[0]))
        });
        let face_owner = foreign.then(|| {
            p3::topology_ref(serial, EntityRef::Face(p1::faces(&other, None).unwrap()[0]))
        });
        body.edge_mut(edge)
            .unwrap()
            .curve
            .as_mut()
            .unwrap()
            .set_owner(edge_owner, None);
        body.face_mut(face)
            .unwrap()
            .surface
            .as_mut()
            .unwrap()
            .set_owner(face_owner);
        let before = samples(&k, &body).unwrap();
        let vertex = p1::vertices(&body, None).unwrap()[0];
        let point = body.vertex(vertex).unwrap().point;
        let delta = Vec3::new(7.0, 11.0, 13.0);
        api_translate(&mut k, Some(TransformObject::Brep(&mut body)), delta).unwrap();
        assert_eq!(samples(&k, &body).unwrap(), before);
        assert_eq!(body.vertex(vertex).unwrap().point, point + delta);
        assert_eq!(
            body.edge(edge).unwrap().curve.as_ref().unwrap().owner(),
            edge_owner
        );
        assert_eq!(
            body.face(face).unwrap().surface.as_ref().unwrap().owner(),
            face_owner
        );
    }
}
