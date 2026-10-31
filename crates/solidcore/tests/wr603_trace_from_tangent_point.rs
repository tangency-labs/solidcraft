//! WR-603 (the fork's tangency-labs/smlib@68fb916): `SmFilletSolver::CalcFilletGeom`
//! starts a trace whose first through point is a tangent point along the filleted
//! edge's end tangent, which can be the line along which the offsets touch to second
//! order. `fixes.fillet_trace_from_tangent_point_toward_next` starts it toward the next
//! through point instead.
//!
//! The case follows the fork's input: a bar into a cylinder tangent to its front face,
//! the bar's front top edge rounded r 1, then the concave bar-top/cylinder arc rounded
//! r 1 with its tangent chain (the first round's curve on the cylinder, whose end is a
//! tangent point). On solidcore's cylinder the trace from that point reaches the next
//! through point either way (the fork's failure was on a one-face cylinder wall), so the
//! fix leaves the result as it was: this test pins that, with the flag on and off. The
//! changed start direction itself is tested by
//! `trace_from_a_tangent_first_point_starts_toward_the_next_with_the_fix` in
//! `blend/fillet_solver/p4.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// The edge whose midpoint is nearest `p`.
fn nearest_edge(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    b.edges()
        .ids()
        .min_by(|&x, &y| {
            midpoint(k, b, x)
                .distance(p)
                .total_cmp(&midpoint(k, b, y).distance(p))
        })
        .unwrap()
}

/// The volume the arc round adds, and the face count after it.
fn peg(fix: bool) -> (f64, usize) {
    let mut k = Kernel::default();
    k.fixes.fillet_trace_from_tangent_point_toward_next = fix;
    let bar = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 16.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(40.0, 8.0, 0.0), 8.0, 20.0).unwrap();
    let mut b = boolean_union(&mut k, bar, boss).unwrap();
    let front_top = nearest_edge(&k, &b, Point3::new(20.0, 0.0, 10.0));
    fillet_edges(&mut k, &mut b, &[front_top], 1.0, 1, 1, 1.0).unwrap();
    let before = volume(&k, &mut b);
    let arc = nearest_edge(&k, &b, Point3::new(32.0, 8.0, 10.0));
    let chain = nearest_edge(&k, &b, Point3::new(38.0, 0.3, 9.5));
    fillet_edges(&mut k, &mut b, &[arc, chain], 1.0, 1, 1, 1.0).unwrap();
    (volume(&k, &mut b) - before, b.faces().ids().count())
}

#[test]
fn arc_round_from_a_tangent_point_is_unchanged_by_the_flag() {
    let (added_off, faces_off) = peg(false);
    let (added_on, faces_on) = peg(true);
    assert_eq!(faces_on, faces_off);
    // the concave round adds about 4.2996 (r 1)
    assert!((added_off - 4.2996).abs() < 1e-3, "{added_off}");
    assert!(
        (added_on - added_off).abs() < 1e-8 * added_off,
        "{added_on} {added_off}"
    );
}
