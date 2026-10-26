//! tangency-labs/solidcore#1144: a body copied for an edit (undo, history, preview) is
//! copied with `SmApiBrepCopy`, whose geometry is owned by the copy's topology, and
//! fillets as the body itself does. A plain `Clone` (geometry without owners) gives the
//! fillet's merge no face for its surfaces.
#![allow(clippy::unwrap_used)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p2, p3};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

fn copy(k: &mut Kernel, b: &Brep, n: u64) -> solidcore::Result<Brep> {
    let named = |entity, domain: u64| {
        let r = p3::topology_ref(0, entity);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
        }
    };
    let target_key = |entity| named(entity, n * 100_000_000);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target_key;
    p3::api_brep_copy(k, &mut env, Some(b), &|entity| named(entity, 0))
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The box's top edge along X at y = 0 (the copy numbers its edges itself).
fn top_front_edge(b: &Brep) -> EdgeId {
    let found: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let (p, q) = (
                vertex::point(b, s.unwrap()).unwrap(),
                vertex::point(b, t.unwrap()).unwrap(),
            );
            p.y.abs() < 1e-9
                && q.y.abs() < 1e-9
                && (p.z - 10.0).abs() < 1e-9
                && (q.z - 10.0).abs() < 1e-9
        })
        .collect();
    assert_eq!(found.len(), 1);
    *found.first().unwrap()
}

/// The #1144 repro: a 100 x 100 x 10 box, one edge rounded with r 1.
fn round(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<()> {
    let e = top_front_edge(b);
    fillet_edges(k, b, &[e], 1.0, 1, 1, 1.0)
}

#[test]
fn issue_1144_an_api_brep_copy_fillets_as_the_body_does() {
    let mut k = Kernel::default();
    let mut body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0, 10.0).unwrap();
    let mut copied = copy(&mut k, &body, 1).unwrap();
    round(&mut k, &mut body).unwrap();
    round(&mut k, &mut copied).unwrap();
    let (a, b) = (volume(&k, &mut body), volume(&k, &mut copied));
    assert!((a - b).abs() <= 1e-9 * a.abs().max(b.abs()), "{a} vs {b}");
    // the box less the round's 100 mm long corner, (1 - pi/4) r^2 in section
    let expected = 100_000.0 - 100.0 * (1.0 - PI / 4.0);
    assert!((a - expected).abs() < 1e-6 * expected, "{a} vs {expected}");
}

#[test]
fn issue_1144_a_plain_clone_does_not_fillet() {
    let mut k = Kernel::default();
    let body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0, 10.0).unwrap();
    let mut cloned = body.clone();
    assert!(round(&mut k, &mut cloned).is_err());
}
