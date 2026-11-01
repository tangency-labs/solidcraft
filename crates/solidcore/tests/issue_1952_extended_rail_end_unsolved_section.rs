//! tangency-labs/solidcore#1952 (WR-641): with `Kernel::fixes.fillet_through_extended_rail_end`
//! (WR-468, the fork's tangency-labs/smlib@d33890f) on, the filleted edge's end at a rail
//! that ends on an extended edge is a through point of the trace. Where no section can
//! be solved there, `CalcFilletGeom` failed and the round with it (1001, "Fillet Surface
//! Computation Failure"), on rounds that build without the flag. The knot is now skipped
//! when its section is not found, so the trace goes on as it does without the flag.
//!
//! The case is a box 40 long, 10 wide and 15 high joined with `create_cylinder` r 10, h 30
//! at the origin (bases coplanar). The concave arc where the box's top meets the cylinder
//! is rounded r 2, with the box along +X, -X, +Y and -Y, so the cylinder's seam (+X)
//! falls at the arc's middle, opposite it, and a quarter turn either way. At the arc's
//! ends the section's rail on the top face lies past the box's side face (y 6 against
//! the face's y 5), off the top plane's domain, so the flag's through point there
//! cannot be solved.
//!
//! - Flag off: the +X and +Y boxes round. The -X and -Y boxes fail with 1001 at their
//!   corner, and so does the pinned C++ (`blend.fillet_executive_p3`,
//!   `attributed-box-cylinder-arc-round-*`). That is WR-568, which
//!   `fillet_torus_span_from_start_section` fixes.
//! - `fillet_through_extended_rail_end` alone: every box failed with 1001. Now it rounds
//!   as with the flag off.
//! - With `fillet_torus_span_from_start_section` as well, every box rounds.
//!
//! The rounded volume is the box-cylinder union plus the material under the round:
//! `6000 + 3000 pi - 15 (2 (2.5 sqrt 75 + 50 asin 0.5))` plus
//! `int_10^12 2 r asin(5 / r) (2 - sqrt(4 - (12 - r)^2)) dr` (the region between the
//! top plane, the cylinder and the ball's circle, swept round the axis between the
//! box's side planes y = +-5), 13998.81358766597.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges_per_edge;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::{Brep, Kernel, SolidError};

/// The union and the round's material, in closed form (see the module docs).
const ROUNDED_VOLUME: f64 = 13_998.813_587_665_97;

/// The boxes: origin and size, and whether the arc rounds with no flag on.
const BOXES: [([f64; 3], [f64; 3], bool); 4] = [
    ([0.0, -5.0, 0.0], [40.0, 10.0, 15.0], true),
    ([-40.0, -5.0, 0.0], [40.0, 10.0, 15.0], false),
    ([-5.0, 0.0, 0.0], [10.0, 40.0, 15.0], true),
    ([-5.0, -40.0, 0.0], [10.0, 40.0, 15.0], false),
];

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// Rounds the box's top arc r 2; the body and the round's result.
fn round(k: &mut Kernel, origin: [f64; 3], size: [f64; 3]) -> (Brep, solidcore::Result<()>) {
    let block = create_box(k, Point3::from(origin), size[0], size[1], size[2]).unwrap();
    let cylinder = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
    let mut b = boolean_union(k, block, cylinder).unwrap();
    // the arc: the edges at z 15 on the cylinder, between two faces, over the box
    let inside = |p: Point3| {
        (p.x - origin[0]) * (p.x - origin[0] - size[0]) <= 0.0
            && (p.y - origin[1]) * (p.y - origin[1] - size[1]) <= 0.0
    };
    let arc: Vec<_> = p1::edges(&b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let ivl = edge::interval(&b, e).unwrap();
            let curve = edge::curve(&b, e).unwrap().unwrap();
            let m = curve
                .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
                .unwrap()[0];
            (m.z - 15.0).abs() < 1e-6
                && ((m.x * m.x + m.y * m.y).sqrt() - 10.0).abs() < 1e-6
                && edge::faces(&b, e, None).unwrap().len() == 2
                && inside(m)
        })
        .collect();
    assert!(!arc.is_empty() && arc.len() <= 2, "{arc:?}");
    let r = fillet_edges_per_edge(
        k,
        &mut b,
        &arc,
        &vec![2.0; arc.len()],
        &vec![FilletSurfaceGeneratorType::Circular; arc.len()],
        1,
        1.0,
    );
    (b, r)
}

fn check(torus_span: bool, extended_rail_end: bool) {
    for (origin, size, rounds_unflagged) in BOXES {
        let mut k = Kernel::default();
        k.fixes.fillet_torus_span_from_start_section = torus_span;
        k.fixes.fillet_through_extended_rail_end = extended_rail_end;
        let (mut b, r) = round(&mut k, origin, size);
        let case =
            format!("box at {origin:?}, torus span {torus_span}, rail end {extended_rail_end}");
        if rounds_unflagged || torus_span {
            r.unwrap_or_else(|e| panic!("{case}: {e:?}"));
            assert!(is_manifold_solid(&b).unwrap(), "{case}");
            let v = volume(&k, &mut b);
            assert!((v - ROUNDED_VOLUME).abs() < 1e-5, "{case}: {v}");
        } else {
            assert!(
                matches!(r, Err(SolidError::Upstream { code: 1001, .. })),
                "{case}: {r:?}"
            );
        }
    }
}

/// No flag: the pinned behaviour (the C++ fails the -X and -Y boxes too).
#[test]
fn issue_1952_flags_off_round_as_the_pinned_cpp() {
    check(false, false);
}

/// The extended rail end alone: was 1001 for every box, now as with the flag off.
#[test]
fn issue_1952_extended_rail_end_skips_an_unsolved_end_section() {
    check(false, true);
}

/// With the torus span fix as well, every box rounds.
#[test]
fn issue_1952_extended_rail_end_with_torus_span_rounds_every_box() {
    check(true, true);
}

/// The torus span fix alone rounds every box (WR-568).
#[test]
fn issue_1952_torus_span_alone_rounds_every_box() {
    check(true, false);
}
