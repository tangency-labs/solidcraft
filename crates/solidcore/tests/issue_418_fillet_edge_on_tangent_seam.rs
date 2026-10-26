//! tangency-labs/solidcore#418 (WR-457): an r 2 round on a straight edge whose ends sit
//! on a tangent seam fails with status 1001. A stadium bar (a box and two cylinders along
//! Z) has a 6 mm slot cut across one end; the slot floor meets the flat top in the edge
//! to round, and each end of that edge is also on the seam where the top runs tangent
//! into the rounded end. `SmFilletNx1Corner::MakeCornerTopology` takes the tangent
//! top/round edge as the side edge, so it picks the round as the end face, which the
//! fillet never meets. The opt-in `Kernel::fixes.fillet_nx1_tangent_side_edge` skips it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::get_edges;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::{Brep, Kernel};

const R: f64 = 9.5;
const RADIUS: f64 = 2.0;

/// The issue's bar, with its coordinates turned (x, y, z) -> (y, z, x) so the end
/// cylinders run along Z: 25 x 19 x 14, the slot 6 wide at Z 3.5..9.5 cut in from X < 0
/// across the top (Y = 9.5). `slot_floor` is the slot floor's X (0 on the seam).
fn bar_with_slot(k: &mut Kernel, slot_floor: f64) -> Brep {
    let block = create_box(k, Point3::new(0.0, -R, 0.0), 25.0, 19.0, 14.0).unwrap();
    let end0 = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), R, 14.0).unwrap();
    let end1 = create_cylinder(k, Point3::new(25.0, 0.0, 0.0), R, 14.0).unwrap();
    let bar = boolean_union(k, block, end0).unwrap();
    let bar = boolean_union(k, bar, end1).unwrap();
    let slot = create_box(
        k,
        Point3::new(slot_floor - 12.0, -10.0, 3.5),
        12.0,
        20.0,
        6.0,
    )
    .unwrap();
    boolean_difference(k, bar, slot).unwrap()
}

/// The straight edge along the slot floor at the top (Y = 9.5), Z 3.5..9.5.
fn slot_top_edge(b: &Brep, slot_floor: f64) -> solidcore::topology::EdgeId {
    let near = |p: Point3, z: f64| {
        (p.x - slot_floor).abs() < 1e-6 && (p.y - R).abs() < 1e-6 && (p.z - z).abs() < 1e-6
    };
    get_edges(b)
        .unwrap()
        .into_iter()
        .find(|&e| {
            let (Some(s), Some(t)) = edge::start_end_vertices(b, e).unwrap() else {
                return false;
            };
            let (s, t) = (b.vertex(s).unwrap().point, b.vertex(t).unwrap().point);
            (near(s, 3.5) && near(t, 9.5)) || (near(s, 9.5) && near(t, 3.5))
        })
        .expect("slot top edge")
}

fn fillet(flag: bool, slot_floor: f64) -> solidcore::Result<(f64, Brep)> {
    let mut k = Kernel::default();
    k.fixes.fillet_nx1_tangent_side_edge = flag;
    let mut body = bar_with_slot(&mut k, slot_floor);
    let e = slot_top_edge(&body, slot_floor);
    fillet_edges(&mut k, &mut body, &[e], RADIUS, 1, 1, 1.0)?;
    assert!(is_manifold_solid(&body).unwrap());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut body, 1e-6, &attrs).unwrap();
    Ok((v, body))
}

/// The bar less the slot, less the round's section (4 - pi) over the slot's 6 mm.
fn expected_volume() -> f64 {
    let pi = core::f64::consts::PI;
    14.0 * (25.0 * 19.0 + pi * R * R) - 6.0 * pi * R * R / 2.0 - 6.0 * (4.0 - pi)
}

#[test]
fn seam_edge_round_is_refused_by_default() {
    assert!(fillet(false, 0.0).is_err());
}

#[test]
fn seam_edge_round_with_the_fix() {
    let (v, _) = fillet(true, 0.0).unwrap();
    assert!(
        (v - expected_volume()).abs() < 1e-3 * expected_volume(),
        "{v}"
    );
}

#[test]
fn off_seam_round_is_unchanged_by_the_fix() {
    let (off, _) = fillet(false, 0.5).unwrap();
    let (on, _) = fillet(true, 0.5).unwrap();
    assert_eq!(off.to_bits(), on.to_bits());
}
