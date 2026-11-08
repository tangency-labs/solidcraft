//! Representation-changing public scales retain real owner filtering.
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::topology::brep::{p1, p3};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_scale, api_translate, TransformObject,
};
use solidcore::{Brep, EntityRef, Kernel};

fn samples(k: &Kernel, b: &Brep) -> solidcore::Result<(Point3, Point3)> {
    let missing = || solidcore::SolidError::invalid_input("scale_test", "missing geometry");
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

fn close(a: Vec3, b: Vec3) -> bool {
    [(a.x, b.x), (a.y, b.y), (a.z, b.z)]
        .into_iter()
        .all(|(a, b)| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()))
}

#[test]
fn converted_geometry_moves_again_without_rebinding() {
    let mut k = Kernel::default();
    let box_ = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
    let sphere = create_sphere(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0).unwrap();
    let scale = Vec3::new(2.0, 3.0, 0.5);
    let delta = Vec3::new(7.0, -2.0, 3.0);
    for mut body in [box_, sphere] {
        let before = samples(&k, &body).unwrap();
        api_scale(&mut k, Some(TransformObject::Brep(&mut body)), scale).unwrap();
        let after = samples(&k, &body).unwrap();
        for (a, b) in [(after.0, before.0), (after.1, before.1)] {
            assert!(close(
                a,
                Vec3::new(b.x * scale.x, b.y * scale.y, b.z * scale.z)
            ));
        }
        let edge = p1::edges(&body, None).unwrap()[0];
        let face = p1::faces(&body, None).unwrap()[0];
        let owners = (
            body.edge(edge).unwrap().curve.as_ref().unwrap().owner(),
            body.face(face).unwrap().surface.as_ref().unwrap().owner(),
        );
        assert!(owners.0.is_some() && owners.1.is_some());
        api_translate(&mut k, Some(TransformObject::Brep(&mut body)), delta).unwrap();
        let moved = samples(&k, &body).unwrap();
        assert!(close(moved.0, after.0 + delta) && close(moved.1, after.1 + delta));
        assert_eq!(
            (
                body.edge(edge).unwrap().curve.as_ref().unwrap().owner(),
                body.face(face).unwrap().surface.as_ref().unwrap().owner()
            ),
            owners
        );
    }
}

#[test]
fn conversion_filters_foreign_and_absent_owners() {
    for foreign in [false, true] {
        let mut k = Kernel::default();
        let mut body = create_box(&mut k, Point3::new(2.0, 3.0, 5.0), 2.0, 3.0, 4.0).unwrap();
        let mut other = create_box(&mut k, Point3::new(20.0, 30.0, 50.0), 2.0, 3.0, 4.0).unwrap();
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
        api_scale(
            &mut k,
            Some(TransformObject::Brep(&mut body)),
            Vec3::new(2.0, 3.0, 0.5),
        )
        .unwrap();
        assert_eq!(samples(&k, &body).unwrap(), before);
        assert_eq!(
            body.vertex(vertex).unwrap().point,
            Point3::new(point.x * 2.0, point.y * 3.0, point.z * 0.5)
        );
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
