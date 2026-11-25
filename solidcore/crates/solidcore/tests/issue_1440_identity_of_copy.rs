//! #1440: `BrepIdentity::of_brep` of a copy made by `api_brep_copy` under caller keys
//! resolves the copy's geometry owners (its recorded `geometry_owner_keys`), so a face
//! closest-point query over the copy finds the face, not only its vertices.

use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::{ObjectRef, Shape};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::face_closest_point_in_shape;
use solidcore::query::brep::{BrepIdentity, LiveBrepObjects};
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::{p2, p3};
use solidcore::{Brep, BrepId, EntityRef};

/// A caller naming: the notification layout shifted by `base`.
fn named(e: EntityRef, base: u64) -> ObjectRef {
    let (type_code, kind, index) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(base + 1 + (kind << 32) + u64::from(index)),
    }
}

const ONE: BrepId = BrepId::from_index(0);

#[test]
fn face_closest_point_on_a_caller_keyed_copy() {
    let mut k = Kernel::new(&KernelOptions::default());
    let b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let mut w = {
        let mut state = p2::ArenaNotifyState::new(&Brep::new());
        let mut env = state.env(None);
        let key_of = |e| named(e, 100_000_000);
        env.key_of = &key_of;
        p3::api_brep_copy(&mut k, &mut env, Some(&b), &|e| named(e, 0)).unwrap()
    };
    let ids = BrepIdentity::of_brep(&w);
    let identity = ids.identity();
    // 10 above the centre of the top face (z = 10)
    let p = Vec3::new(15.0, 10.0, 20.0);
    let mut found = Vec::new();
    for f in faces(&w, None).unwrap() {
        let shape = Shape::new(&mut k, &(ONE, &w), &[], &[(ONE, f)], &[], &[]).unwrap();
        let objects = LiveBrepObjects::new(&mut w, &identity, None).unwrap();
        let d = face_closest_point_in_shape(&k, &shape, &objects, (ONE, f), p).unwrap();
        found.push((d.distance, d.point));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let side_y = 200.0_f64.sqrt(); // y = 0 / y = 20 faces: their top edge
    let side_x = 325.0_f64.sqrt(); // x = 0 / x = 30 faces: their top edge
    let expected = [10.0, side_y, side_y, side_x, side_x, 20.0];
    for ((d, _), e) in found.iter().zip(expected) {
        assert!((d - e).abs() <= 1e-9 * e, "distance {d}, expected {e}");
    }
    let (top, point) = found[0];
    assert_eq!(top, 10.0);
    assert_eq!(point, Vec3::new(15.0, 10.0, 10.0));
    assert_eq!(found[5].1, Vec3::new(15.0, 10.0, 0.0));
}
