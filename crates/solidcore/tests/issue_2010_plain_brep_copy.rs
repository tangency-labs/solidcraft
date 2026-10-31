//! tangency-labs/solidcore#2010: a plain owning copy of a B-rep (`SmApiBrepCopy`)
//! without namings or a notification environment of the caller's.
//!
//! `topology::brep::p3::brep_copy(k, b)` copies a 10 x 20 x 30 box with a `Copy`
//! attribute on each of its faces and edges into a body that owns its geometry: 6
//! faces, 12 edges, 8 vertices, volume 6000, all 18 attributes, a clean Level2
//! `check_brep`, and a union with another such copy that matches the union of two
//! `api_brep_copy` copies under explicit, disjoint namings (SolidCraft's `copy_of`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{Attribute, AttributeStore, AttributeValue, Behavior, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::{api_brep_copy, brep_copy, topology_ref};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, NamingScheme};
use solidcore::{EntityRef, Kernel};

const ID: u64 = 2_010_001;

/// The faces and edges of `b` carrying attribute [`ID`], with its values.
fn tagged(b: &Brep) -> (Vec<i64>, Vec<i64>) {
    let value = |e| {
        b.entity_attributes(e)
            .iter()
            .find(|(_, a)| a.attribute_id() == ID)
            .map(|(_, a)| match a.value() {
                AttributeValue::Long(v) => *v,
                other => panic!("not a long attribute: {other:?}"),
            })
    };
    let mut faces: Vec<i64> = b
        .face_list()
        .iter()
        .filter_map(|&f| value(EntityRef::Face(f)))
        .collect();
    let mut edges: Vec<i64> = b
        .edge_list()
        .iter()
        .filter_map(|&e| value(EntityRef::Edge(e)))
        .collect();
    faces.sort_unstable();
    edges.sort_unstable();
    (faces, edges)
}

/// A 10 x 20 x 30 box at `origin` with a `Copy` attribute (value: its index) on each of
/// its faces and edges.
fn tagged_box(k: &mut Kernel, origin: Point3) -> Brep {
    let mut b = create_box(k, origin, 10.0, 20.0, 30.0).unwrap();
    let entities: Vec<_> = b
        .face_list()
        .iter()
        .copied()
        .map(EntityRef::Face)
        .chain(b.edge_list().iter().copied().map(EntityRef::Edge))
        .collect();
    for (i, e) in entities.into_iter().enumerate() {
        b.add_attribute(e, Attribute::long(ID, i as i64, Behavior::Copy))
            .unwrap();
    }
    b
}

fn all_tags() -> (Vec<i64>, Vec<i64>) {
    ((0..6).collect(), (6..18).collect())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    )
}

fn assert_checks_valid(k: &mut Kernel, b: &mut Brep, what: &str) {
    let c = check_brep(k, b, CheckLevel::Level2)
        .unwrap_or_else(|e| panic!("{what}: check_brep refused the body: {e}"));
    assert!(c.is_valid(), "{what}: {:#?}", c.lines());
    assert!(c.lines().is_empty(), "{what}: {:#?}", c.lines());
}

fn assert_close(v: f64, analytic: f64, what: &str) {
    assert!(
        (v - analytic).abs() <= 1e-9 * analytic,
        "{what}: volume {v}, analytic {analytic}"
    );
}

/// SolidCraft's `copy_of`: `api_brep_copy` with copy `n` named in a key domain of its own.
fn named_copy(k: &mut Kernel, b: &Brep, n: u64) -> Brep {
    fn named(e: EntityRef, domain: u64) -> ObjectRef {
        let r = topology_ref(0, e);
        let (kind, index) = ((r.key.0 >> 32) & 0xf, r.key.0 & 0xffff_ffff);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(domain + (((kind + 1) << 32) | index)),
        }
    }
    let target = move |e| named(e, n << 40);
    let mut state = ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target;
    api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0)).unwrap()
}

#[test]
fn plain_copy_of_tagged_box() {
    let mut k = Kernel::default();
    let b = tagged_box(&mut k, Point3::new(0.0, 0.0, 0.0));
    let mut c = brep_copy(&mut k, &b).unwrap();
    assert_eq!(counts(&c), (6, 12, 8));
    assert_eq!(c.naming(), NamingScheme::Notify);
    assert_close(volume(&k, &mut c), 6000.0, "copy");
    assert_eq!(tagged(&c), all_tags(), "the copy carries every attribute");
    assert_eq!(tagged(&b), all_tags(), "the source keeps its attributes");
    // #1584 / #1585: every geometry owner of the copy is its entity
    assert_checks_valid(&mut k, &mut c, "copy");
    // the copy's geometry names its faces and edges (a `Clone`'s has no owner)
    assert_eq!(p1::surfaces(&c, None).unwrap().len(), 6);
    assert_eq!(p1::curves(&c, None).unwrap().len(), 12);
    let clone = b.clone();
    assert_eq!(p1::surfaces(&clone, None).unwrap().len(), 0);
    // a copy of the copy is a copy too
    let mut cc = brep_copy(&mut k, &c).unwrap();
    assert_eq!(counts(&cc), (6, 12, 8));
    assert_eq!(tagged(&cc), all_tags());
    assert_checks_valid(&mut k, &mut cc, "copy of the copy");
}

#[test]
fn union_of_plain_copies_matches_named_copies() {
    let mut k = Kernel::default();
    let a = tagged_box(&mut k, Point3::new(0.0, 0.0, 0.0));
    let b = tagged_box(&mut k, Point3::new(5.0, 5.0, 5.0));
    // 2 * 6000 minus the 5 x 15 x 25 overlap
    let analytic = 12000.0 - 1875.0;

    let (pa, pb) = (
        brep_copy(&mut k, &a).unwrap(),
        brep_copy(&mut k, &b).unwrap(),
    );
    let mut plain = boolean(&mut k, pa, pb, BooleanOperation::Union)
        .unwrap()
        .result;
    let (na, nb) = (named_copy(&mut k, &a, 1), named_copy(&mut k, &b, 2));
    let mut named = boolean(&mut k, na, nb, BooleanOperation::Union)
        .unwrap()
        .result;

    assert_eq!(counts(&plain), counts(&named));
    assert_close(volume(&k, &mut plain), analytic, "union of plain copies");
    assert_close(volume(&k, &mut named), analytic, "union of named copies");
    assert_eq!(
        tagged(&plain),
        tagged(&named),
        "the unions keep the same attributes"
    );
    assert_checks_valid(&mut k, &mut plain, "union of plain copies");
    // the operands' sources are untouched
    assert_eq!(tagged(&a), all_tags());
    assert_eq!(tagged(&b), all_tags());
}
