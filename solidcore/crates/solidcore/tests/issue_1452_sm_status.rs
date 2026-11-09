//! #1452: `SolidError::sm_status` tells an upstream `SmStatus` from a port-side
//! invariant failure. The #1440 misuse (a caller-keyed copy queried under its
//! naming-only identities) fails with `PortInvariant`, which stands for no upstream
//! status; `SmApiBrepCopy` of a null B-rep returns upstream `SM_ERR_INVALID_INPUT`.

use solidcore::error::SM_ERR_INVALID_INPUT;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::{ObjectRef, Shape};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::face_closest_point_in_shape;
use solidcore::query::brep::{BrepIdentity, LiveBrepObjects};
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::{p2, p3};
use solidcore::{Brep, BrepId, EntityRef, SolidError};

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

fn caller_keyed_copy(k: &mut Kernel, b: &Brep) -> solidcore::Result<Brep> {
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    let key_of = |e| named(e, 100_000_000);
    env.key_of = &key_of;
    p3::api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0))
}

#[test]
fn identity_misuse_is_a_port_invariant_without_status() {
    let mut k = Kernel::new(&KernelOptions::default());
    let b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let mut w = caller_keyed_copy(&mut k, &b).unwrap();
    // the copy's naming alone (arena keys) does not name its caller-keyed owners
    let identity = BrepIdentity::of_naming_with_id(ONE, &w);
    let p = Vec3::new(15.0, 10.0, 20.0);
    let f = faces(&w, None).unwrap()[0];
    let shape = Shape::new(&mut k, &(ONE, &w), &[], &[(ONE, f)], &[], &[]).unwrap();
    let objects = LiveBrepObjects::new(&mut w, &identity, None).unwrap();
    let e = face_closest_point_in_shape(&k, &shape, &objects, (ONE, f), p).unwrap_err();
    assert!(
        matches!(
            e,
            SolidError::PortInvariant {
                operation: "topology::brep_cache",
                ..
            }
        ),
        "{e:?}"
    );
    assert_eq!(e.sm_status(), None);
}

#[test]
fn upstream_invalid_input_has_its_status() {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    // SmApiBrepCopy(NULL): SM_ERR_INVALID_INPUT
    let e = p3::api_brep_copy(&mut k, &mut env, None, &|e| named(e, 0)).unwrap_err();
    assert!(matches!(e, SolidError::Upstream { .. }), "{e:?}");
    assert_eq!(e.sm_status(), Some(SM_ERR_INVALID_INPUT));
    assert_eq!(e.sm_status(), Some(1007));
}
