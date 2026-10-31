//! WR-609 / WR-610 (#960; the fork's tangency-labs/smlib@a9277fd and @6e77631, rev
//! 68fb9162): with `fillet_trace_reaches_rails`, `CreateFilletBrep` lets each solver's
//! trace reach as far as its rails can lie from the edge before the corners
//! (`sm_ExtendTraceToItsRails`, `GetDomainExtension`), and `RailEdgeuseIntersect` runs a
//! straight side edge on along its line.
//!
//! - A variable round 2 -> 10 on the 20 long top edge of a 10 x 20 x 30 box: its law
//!   reaches 30 where the trace ends (two and a half edge lengths past the end), so the
//!   trace's reach is `(2 / tan(45 deg) + 1) * 30 = 90` across the edge; the fork's
//!   commit message gives the volume 5822.58649 "either way".
//! - A round r10 on a 5 x 5 bar's long edge: the pinned C++ fails in `CalcFilletGeom`
//!   with no rail ends (fillet error 4); with the flag the rails' ends are found and it
//!   fails where r9 does, at the corner edges (error 3), which the cliff overflow
//!   (#960's later PRs) builds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::variable_radius_fillet;
use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession};
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_trace_reaches_rails = fix;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge whose ends both satisfy `on`.
fn edge_where(k: &Kernel, b: &Brep, on: impl Fn(Point3) -> bool) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            on(c.evaluate_point(k, ivl.min()).unwrap())
                && on(c.evaluate_point(k, ivl.max()).unwrap())
        })
        .expect("edge")
}

/// The 10 x 20 x 30 box and its top edge at x = 0 (20 long, the top face 10 wide).
fn box_and_top_edge(k: &mut Kernel) -> (Brep, EdgeId) {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let e = edge_where(k, &b, |p| p.z == 30.0 && p.x == 0.0);
    (b, e)
}

fn variable_round_volume(fix: bool) -> f64 {
    let mut k = kernel(fix);
    let (mut b, e) = box_and_top_edge(&mut k);
    variable_radius_fillet(&mut k, &mut b, &[e], 2.0, 10.0, 1, 1).unwrap();
    volume(&k, &mut b)
}

/// The trace reach and across flags of each solver after the variable round.
fn variable_round_reach(fix: bool) -> Vec<(f64, [[bool; 2]; 2])> {
    let mut k = kernel(fix);
    let (mut b, e) = box_and_top_edge(&mut k);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_variable_radius_edges(&mut k, &[e], 2.0, FilletLaw::linear(2.0, 10.0, None), 1, 1)
        .unwrap();
    session.do_fillet(&mut k).unwrap();
    let mut out = Vec::new();
    for s in session.executive().solvers.clone() {
        let FilletObject::Solver(id) = s else {
            continue;
        };
        let base = session.solver(id).unwrap().base();
        out.push((base.trace_reach, base.trace_across));
    }
    out
}

/// The fillet error code of a round of radius `r` on a 5 x 5 x 30 bar's long edge.
fn bar_round_error(fix: bool, r: f64) -> i32 {
    let mut k = kernel(fix);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 5.0, 30.0).unwrap();
    let e = edge_where(&k, &b, |p| p.x == 5.0 && p.y == 5.0);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
        .unwrap();
    assert!(
        session.do_fillet(&mut k).is_err(),
        "r{r} has no room without the cliff overflow"
    );
    session.error_info().code
}

#[test]
fn variable_round_traced_farther_comes_out_the_same() {
    for fix in [false, true] {
        let v = variable_round_volume(fix);
        assert!((v - 5822.58649).abs() < 5e-6, "flag {fix}: volume {v}");
    }
}

#[test]
fn variable_round_trace_reaches_its_rails_only_with_the_flag() {
    assert_eq!(variable_round_reach(false), vec![(0.0, [[true; 2]; 2])]);
    let on = variable_round_reach(true);
    let expect = (2.0 / (PI / 4.0).tan() + 1.0) * 30.0;
    assert_eq!(on.len(), 1);
    let (reach, across) = on[0];
    assert!((reach - expect).abs() <= 1e-12 * expect, "reach {reach}");
    // each face's u runs across the edge, its v along it
    assert_eq!(across, [[true, false], [true, false]]);
}

#[test]
fn bar_round_wider_than_its_faces_finds_its_rail_ends() {
    // pinned behaviour: no rail ends, the fillet surface fails
    assert_eq!(bar_round_error(false, 10.0), 4);
    // with the flag: as far as r9 gets, the corner edges
    assert_eq!(bar_round_error(true, 9.0), 3);
    assert_eq!(bar_round_error(true, 10.0), 3);
}

#[test]
fn variable_round_with_the_flag_is_deterministic() {
    assert_eq!(
        variable_round_volume(true).to_bits(),
        variable_round_volume(true).to_bits()
    );
}
