//! tangency-labs/solidcore#1980: the cliff overflow's subtraction keeps the attributes of
//! the target edges it replaces. The session keeps the target's attributes in its own
//! store, and `subtract_removal_region` ran its Boolean on the target's empty store, so
//! a shortened edge lost them (the fork's ride on the objects, `SM_AB_COPY`).

#![allow(clippy::unwrap_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, vertex, Brep, EdgeId};
use solidcore::{EntityRef, Kernel, Point3};

const ID: u64 = 0x7000_0001;

fn between(b: &Brep, a: Point3, q: Point3) -> Option<EdgeId> {
    b.edges().ids().find(|&e| {
        let (s, t) = edge::start_end_vertices(b, e).unwrap();
        let (s, t) = (
            vertex::point(b, s.unwrap()).unwrap(),
            vertex::point(b, t.unwrap()).unwrap(),
        );
        (s.distance(a) < 1e-6 && t.distance(q) < 1e-6)
            || (s.distance(q) < 1e-6 && t.distance(a) < 1e-6)
    })
}

fn edge_at(b: &Brep, a: Point3, q: Point3) -> EdgeId {
    between(b, a, q).unwrap_or_else(|| panic!("no edge {a:?}-{q:?}"))
}

#[test]
fn an_edge_tag_crosses_a_cliff_overflow() {
    let p = Point3::new;
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    let block = create_box(&mut k, Point3::ZERO, 40.0, 30.0, 10.0).unwrap();
    let notch = create_box(&mut k, p(35.0, -1.0, -1.0), 6.0, 6.0, 4.0).unwrap();
    let mut b = boolean_difference(&mut k, block, notch).unwrap();
    for (a, q, v) in [
        (p(0.0, 30.0, 10.0), p(40.0, 30.0, 10.0), 1),
        (p(0.0, 0.0, 10.0), p(0.0, 30.0, 10.0), 2),
    ] {
        let e = edge_at(&b, a, q);
        b.add_attribute(EntityRef::Edge(e), Attribute::long(ID, v, Behavior::Copy))
            .unwrap();
    }
    let rounds = [
        edge_at(&b, p(0.0, 0.0, 10.0), p(40.0, 0.0, 10.0)),
        edge_at(&b, p(40.0, 0.0, 10.0), p(40.0, 30.0, 10.0)),
    ];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &rounds, 8.0, 1, 1, 1.0)
        .unwrap();
    s.do_fillet(&mut k).unwrap();
    drop(s);
    b.refresh_cached_topology();
    for (a, q, which) in [
        (p(0.0, 30.0, 10.0), p(32.0, 30.0, 10.0), "back"),
        (p(0.0, 8.0, 10.0), p(0.0, 30.0, 10.0), "left"),
    ] {
        let e = edge_at(&b, a, q);
        let n = b
            .entity_attributes(EntityRef::Edge(e))
            .iter()
            .filter(|(_, x)| x.attribute_id() == ID)
            .count();
        assert_eq!(n, 1, "the {which} edge's attributes");
    }
}
