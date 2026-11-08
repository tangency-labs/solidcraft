//! tangency-labs/solidcore#2115: an edit run through `with_kernel_callbacks` on a
//! `NamingScheme::Notify` body names its objects by the kernel's edit identities
//! (`env.key_of`, `topology_ref(serial, ..)`), so the geometry it binds to new edges is
//! owned under those keys. The body stays `Notify` and `check_brep` looks owners up by
//! the body's scheme, so every new edge failed `SmEdge` test #61
//! (`pE->ValidateGeometry(FALSE)`). The edit now rebinds that geometry to the body's
//! own identities when it ends.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::cutting::cut_notified;
use solidcore::topology::brep::p3::with_kernel_callbacks;
use solidcore::topology::brep::p9;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{vertex, Brep, NamingScheme};
use solidcore::{EntityRef, Kernel, Point3, Vec3};

/// The edges whose curve is not owned by the identity the check compares.
fn misowned_edges(b: &Brep) -> Vec<String> {
    b.edge_list()
        .iter()
        .filter_map(|&e| {
            let owner = b.edge(e).unwrap().curve.as_ref().and_then(Curve::owner);
            let identity = b.owner_identity(EntityRef::Edge(e));
            (owner != Some(identity)).then(|| format!("{e:?}: {owner:?} vs {identity:?}"))
        })
        .collect()
}

fn volume(k: &mut Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The issue's repro: a 10 x 10 x 10 box cut at z = 5. `SmApiCut` keeps the half below
/// the plane as an open sheet (no cap): the bottom and four 10 x 5 sides, area 300.
#[test]
fn cut_under_kernel_callbacks_owns_new_edges_by_the_body_scheme() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    assert_eq!(b.naming(), NamingScheme::Notify);
    with_kernel_callbacks(&mut k, &mut b, |k, b, env| {
        let key_of = env.key_of;
        let cr = move |e, _t: i64| key_of(EntityRef::Edge(e));
        cut_notified(
            k,
            b,
            env,
            &cr,
            Point3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, 1.0),
        )
    })
    .unwrap();
    assert_eq!(b.naming(), NamingScheme::Notify);
    assert_eq!(misowned_edges(&b), Vec::<String>::new());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert_eq!(check.assert_valid, Some(true), "{check:?}");
    assert_eq!(b.faces().ids().count(), 5);
    assert_eq!(b.edge_list().len(), 12);
    for &v in b.vertex_list() {
        let z = vertex::point(&b, v).unwrap().z;
        assert!(z.abs() < 1e-9 || (z - 5.0).abs() < 1e-9, "vertex at z {z}");
    }
    let area = brep_area(&k, &mut b, 1e-9).unwrap();
    assert!((area - 300.0).abs() <= 1e-9 * 300.0, "area {area}");
}

/// Another kernel-callback edit that makes an edge: splitting a box edge at its middle,
/// the new edge's curve named by the edit's identity.
#[test]
fn split_edge_under_kernel_callbacks_owns_the_new_edge_by_the_body_scheme() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    assert_eq!(b.naming(), NamingScheme::Notify);
    let e = b.edge_list()[0];
    b.set_editing_enabled(true);
    let pieces = with_kernel_callbacks(&mut k, &mut b, |k, b, env| {
        let key_of = env.key_of;
        let cr = move |edge, kind: i64| ObjectRef {
            key: key_of(EntityRef::Edge(edge)).key,
            type_code: kind,
        };
        p9::make_vertex_split_edge(k, b, e, 0.5, env, &cr)
    })
    .unwrap()
    .edges
    .unwrap();
    assert_eq!(pieces.len(), 2);
    b.refresh_cached_topology();
    assert_eq!(b.naming(), NamingScheme::Notify);
    assert_eq!(misowned_edges(&b), Vec::<String>::new());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert_eq!(check.assert_valid, Some(true), "{check:?}");
    let v = volume(&mut k, &mut b);
    assert!((v - 1000.0).abs() <= 1e-9 * 1000.0, "volume {v}");
}
