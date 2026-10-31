//! WR-630 (#960, #1467; the fork's tangency-labs/smlib@3f22e97, rev 68fb9162): with
//! `fillet_rail_end_on_continued_surface`, `CalcRailIntEdgeuse` places a rail's end on
//! its base surface continued, as the solve found it, not clamped to the surface's domain.
//!
//! A 40 x 20 x 10 box's top front edge (at y = 0, z = 10) rounded r10.05: on the 10 high
//! front face the rail lies 10.05 below the top, 0.05 past the face's bottom edge (a cliff).
//! With `fillet_trace_reaches_rails` the rail meets each end's vertical side edge's line
//! there, 0.05 past the edge's end. Clamped, the rail's end lands on the side edge's
//! bottom vertex (z = 0); continued, it stays on the rail (z = -0.05).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, vertex};
use solidcore::Kernel;

/// The lowest z of the fillet's corner vertices on the front face (y = 0).
fn lowest_rail_end(fix: bool) -> f64 {
    let mut k = Kernel::default();
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = fix;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 20.0, 10.0).unwrap();
    let e = b
        .edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(&b, e).unwrap();
            let c = edge::curve(&b, e).unwrap().unwrap();
            let on = |p: Point3| p.y == 0.0 && p.z == 10.0;
            on(c.evaluate_point(&k, ivl.min()).unwrap())
                && on(c.evaluate_point(&k, ivl.max()).unwrap())
        })
        .unwrap();
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], 10.05, 1, 1, 1.0)
        .unwrap();
    // too large for the front face: it fails later, without the cliff overflow
    let _ = session.do_fillet(&mut k);
    let mut lowest = f64::INFINITY;
    for c in session.executive().corners.clone() {
        let FilletObject::Corner(id) = c else {
            continue;
        };
        for &v in &session.corner(id).unwrap().vertices {
            let p = vertex::point(session.pseudo_brep().brep(), v).unwrap();
            if p.y.abs() < 1e-6 {
                lowest = lowest.min(p.z);
            }
        }
    }
    lowest
}

#[test]
fn rail_end_past_its_side_edge_stays_on_the_rail() {
    let off = lowest_rail_end(false);
    let on = lowest_rail_end(true);
    assert!(off.abs() < 1e-6, "clamped onto the side edge's end: {off}");
    assert!(
        (on + 0.05).abs() < 1e-6,
        "on the rail, 0.05 past the edge's end: {on}"
    );
}
