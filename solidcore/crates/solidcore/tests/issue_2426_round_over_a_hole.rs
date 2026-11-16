//! #2426, #422: an R4 round on a block's top edge running over a through hole. Keep features
//! keeps the hole and trims the round round it: the case SMLib left as a body that is not a
//! closed solid.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.expect("an edge there")
}

/// 40 x 20 x 10 less the hole r 3 at (20, 14.5), less the round's corner, plus the part of that
/// corner inside the hole, which isn't taken twice:
/// `int_16^17.5 2 sqrt(9 - (y - 14.5)^2) (4 - sqrt(16 - (y - 16)^2)) dy = 0.3758296360`.
const VOLUME: f64 = 7580.287315387334;

/// The fix set the round needs: keep features trims it in place only once the rails and the
/// overflow are the blend's (the cliff overflow). The default kernel builds it the same way
/// since the cliff overflow is the only path (`issue_422_round_over_through_hole`).
#[test]
fn the_hole_stays_with_every_fix() {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let block = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let drill = create_cylinder(&mut k, Point3::new(20.0, 14.5, -1.0), 3.0, 12.0).unwrap();
    let mut b = boolean_difference(&mut k, block, drill).unwrap();
    let e = pick(&k, &b, Point3::new(10.0, 20.0, 10.0));
    fillet_edges(&mut k, &mut b, &[e], 4.0, 1, 1, 1.0).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    assert!((v - VOLUME).abs() <= 1e-7 * VOLUME, "volume {v}");
    assert_eq!(b.face_list().len(), 8);
}
