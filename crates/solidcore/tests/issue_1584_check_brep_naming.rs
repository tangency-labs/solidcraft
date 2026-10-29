//! #1584: `check_brep` of a body whose geometry owners are a caller's keys (an array
//! import through `make_topology_from_data`, a copy through `api_brep_copy`). Upstream
//! owners are pointers set by `SetOwner` (`SmBrep::MergeBrep`, `SmBrepData`,
//! `SmEdgeuse::SetUVTrimCurve`), so C++ finds these bodies valid; the port names each
//! owner by the caller key it records per edge, face and edge use.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p10::{make_topology_from_data, MakeTopologyOptions, TopologyEnv};
use solidcore::topology::brep::validation::{check_brep, check_brep_with_keys, CheckLevel};
use solidcore::topology::brep::{p2, p3};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId, NamingScheme};
use solidcore::{Brep, EntityRef, SolidError};

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

const IMPORT_BASE: u64 = 500_000_000;
const COPY_BASE: u64 = 100_000_000;

/// Case A: a box's arrays without UV curves (as flat arrays carry none), imported under
/// the caller keys `named(_, IMPORT_BASE)` without making UV trim curves.
fn imported_box(k: &mut Kernel) -> Brep {
    let b = create_box(k, Vec3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let store = AttributeStore::new();
    let recs = NotifyRecords::new();
    let src_key = |e| named(e, 0);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &src_key,
        records: &recs,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let mut data = BrepData::from_brep(&b, &env, &mut Vec::new()).unwrap();
    for eu in &mut data.edgeuses {
        eu.uv_curve = 999_999;
    }
    data.uv_curves.clear();
    let mut out = Brep::new_with_infinite_region().unwrap();
    let mut store2 = AttributeStore::new();
    let mut recs2 = NotifyRecords::new();
    let key_of = |e| named(e, IMPORT_BASE);
    let mut tenv = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store2,
        key_of: &key_of,
        records: &mut recs2,
        next_key: 1,
    };
    let opts = MakeTopologyOptions {
        make_uv_trim_curves: false,
        ..MakeTopologyOptions::default()
    };
    make_topology_from_data(k, &mut out, &mut data, &mut tenv, opts).unwrap();
    assert_eq!(out.naming(), NamingScheme::Caller);
    out
}

/// Case B: `api_brep_copy` of a box under the caller keys `named(_, COPY_BASE)`.
fn copied_box(k: &mut Kernel) -> Brep {
    let b = create_box(k, Vec3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    let key_of = |e| named(e, COPY_BASE);
    env.key_of = &key_of;
    p3::api_brep_copy(k, &mut env, Some(&b), &|e| named(e, 0)).unwrap()
}

#[test]
fn array_import_without_uv_curves_checks_valid_with_keys() {
    let mut k = Kernel::default();
    let key_of = |e| named(e, IMPORT_BASE);
    for level in [CheckLevel::Level0, CheckLevel::Level2] {
        let mut b = imported_box(&mut k);
        let r = check_brep_with_keys(&mut k, &mut b, level, &key_of).unwrap();
        assert!(r.is_valid(), "{level:?}: {:?}", r.lines());
        assert_eq!(r.failure_count(), 0);
    }
}

#[test]
fn array_import_checks_valid_without_keys() {
    let mut k = Kernel::default();
    let mut b = imported_box(&mut k);
    let r = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(r.is_valid(), "{:?}", r.lines());
    assert!(r.lines().is_empty(), "{:?}", r.lines());
}

#[test]
fn caller_keyed_copy_checks_valid() {
    let mut k = Kernel::default();
    let mut b = copied_box(&mut k);
    assert_eq!(b.naming(), NamingScheme::Caller);
    let r = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(r.is_valid(), "{:?}", r.lines());
    assert!(r.lines().is_empty(), "{:?}", r.lines());

    let mut b = copied_box(&mut k);
    let key_of = |e| named(e, COPY_BASE);
    let r = check_brep_with_keys(&mut k, &mut b, CheckLevel::Level2, &key_of).unwrap();
    assert!(r.is_valid(), "{:?}", r.lines());
    assert_eq!(r.failure_count(), 0);
}

#[test]
fn copied_trims_are_owned_by_their_edge_use() {
    let mut k = Kernel::default();
    let b = copied_box(&mut k);
    let mut trims = 0;
    for (eu, record) in b.edgeuses().iter() {
        if let Some(c) = &record.uv_trim_curve {
            trims += 1;
            assert_eq!(c.owner(), Some(named(EntityRef::Edgeuse(eu), COPY_BASE)));
        }
    }
    // the box's 12 edges carry a trim on each of their two face-side edge uses
    assert_eq!(trims, 24);
}

#[test]
fn lazy_trim_on_imported_body_gets_owner() {
    let mut k = Kernel::default();
    let mut b = imported_box(&mut k);
    let untrimmed: Vec<EdgeuseId> = b
        .edgeuses()
        .iter()
        .filter(|(_, r)| {
            r.uv_trim_curve.is_none() && r.edge.is_some() && r.orientation == OrientType::Same
        })
        .map(|(id, _)| id)
        .collect();
    let eu = *untrimmed.first().unwrap();
    let fetch = get_or_create_uv_trim_curve(&k, &mut b, eu, false, false, true).unwrap();
    assert!(fetch.status.is_ok(), "{:?}", fetch.status);
    let mut made = 0;
    for (id, record) in b.edgeuses().iter() {
        if let Some(c) = &record.uv_trim_curve {
            assert_eq!(c.owner(), Some(named(EntityRef::Edgeuse(id), IMPORT_BASE)));
            if untrimmed.contains(&id) {
                made += 1;
            }
        }
    }
    assert!(made >= 1, "no trim was made");
}

#[test]
fn foreign_trim_owner_still_fails_test_6() {
    let mut k = Kernel::default();
    let mut b = copied_box(&mut k);
    let eu = b
        .edgeuses()
        .iter()
        .find(|(_, r)| r.uv_trim_curve.is_some())
        .map(|(id, _)| id)
        .unwrap();
    let foreign = named(EntityRef::Edgeuse(eu), 900_000_000);
    let _ = b
        .edgeuse_mut(eu)
        .unwrap()
        .uv_trim_curve
        .as_mut()
        .unwrap()
        .set_owner(Some(foreign), None);
    let key_of = |e| named(e, COPY_BASE);
    let r = check_brep_with_keys(&mut k, &mut b, CheckLevel::Level2, &key_of).unwrap();
    let bad: Vec<String> = r
        .lines()
        .into_iter()
        .filter(|l| l.contains("Bad UVTrimCurve Owner"))
        .collect();
    assert_eq!(bad.len(), 1, "{:?}", r.lines());
    assert_eq!(r.failure_count(), 1, "{:?}", r.lines());
}

#[test]
fn caller_body_without_keys_is_refused() {
    let mut k = Kernel::default();
    // a body built through the Euler operators records no caller keys
    let mut b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    b.set_naming(NamingScheme::Caller);
    let err = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap_err();
    assert!(matches!(err, SolidError::InvalidInput { .. }), "{err:?}");
}
