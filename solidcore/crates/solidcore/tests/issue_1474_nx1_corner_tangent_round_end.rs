//! tangency-labs/solidcore#1474 (WR-499): a block 40 x 30 x 5 with a cylinder r 10.5
//! standing on its top, the cylinder's axis on the block's front edge so half of it
//! overhangs (SolidCraft rig case fillets/4174). Rounding the concave arc where the
//! cylinder meets the top with R 2 fails with status 1001: at each end of the arc the
//! overhang's rim continues it tangentially with the opposite convexity, and
//! `SmFilletNx1Corner::MakeCornerTopology` takes that rim as the corner's side edge.
//! The rail on the cylinder never meets it. The opt-in
//! `Kernel::fixes.fillet_nx1_opposite_continuation_side_edge` (with WR-457's
//! `fillet_nx1_tangent_side_edge`) passes it, so the round ends on the front face's
//! plane, as tangency-labs/smlib@420e47d does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const R_CYL: f64 = 10.5;
const H_CYL: f64 = 10.0;
const RADIUS: f64 = 2.0;

/// The block x -20..20, y 0..30, z 0..5 and the cylinder on its top, axis on the
/// front edge (x = y = 0).
fn block_with_overhanging_cylinder(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(-20.0, 0.0, 0.0), 40.0, 30.0, 5.0).unwrap();
    let cyl = create_cylinder(k, Point3::new(0.0, 0.0, 5.0), R_CYL, H_CYL).unwrap();
    boolean_union(k, block, cyl).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// The concave arc on the block's top (z = 5, radius 10.5, y > 0).
fn junction_arc(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edge_list().to_vec();
    edges
        .into_iter()
        .filter(|&e| {
            let m = midpoint(k, b, e);
            (m.z - 5.0).abs() < 1e-6
                && ((m.x * m.x + m.y * m.y).sqrt() - R_CYL).abs() < 1e-6
                && m.y > 1e-3
        })
        .collect()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

fn round(tangent_side_edge: bool, continuation: bool) -> (Brep, Kernel, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_nx1_tangent_side_edge = tangent_side_edge;
    k.fixes.fillet_nx1_opposite_continuation_side_edge = continuation;
    let mut body = block_with_overhanging_cylinder(&mut k);
    let arc = junction_arc(&k, &body);
    assert_eq!(arc.len(), 1);
    let r = fillet_edges(&mut k, &mut body, &arc, RADIUS, 1, 1, 1.0);
    (body, k, r)
}

/// The block and cylinder plus the round over the half turn (Pappus): the section
/// R^2 (1 - pi/4) with its centroid R (10 - 3 pi) / (12 - 3 pi) out from the wall.
fn expected_volume() -> f64 {
    let pi = core::f64::consts::PI;
    let section = RADIUS * RADIUS * (1.0 - pi / 4.0);
    let centroid = R_CYL + RADIUS * (10.0 - 3.0 * pi) / (12.0 - 3.0 * pi);
    40.0 * 30.0 * 5.0 + pi * R_CYL * R_CYL * H_CYL + pi * centroid * section
}

fn has_vertex(b: &Brep, p: Point3) -> bool {
    b.vertex_list().iter().any(|&v| {
        let q = b.vertex(v).unwrap().point;
        (q - p).length() < 1e-6
    })
}

#[test]
fn round_fails_with_upstream_behaviour() {
    for tangent_side_edge in [false, true] {
        let (_, _, r) = round(tangent_side_edge, false);
        assert!(
            matches!(r, Err(solidcore::SolidError::Upstream { code: 1001, .. })),
            "{r:?}"
        );
    }
    // The continuation skip lives inside WR-457's: alone it changes nothing.
    let (_, _, r) = round(false, true);
    assert!(
        matches!(r, Err(solidcore::SolidError::Upstream { code: 1001, .. })),
        "{r:?}"
    );
}

#[test]
fn round_ends_on_the_front_face_plane_with_the_fix() {
    let (mut b, k, r) = round(true, true);
    r.unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    // the B-rep's faces, edges and vertices (deleted ones leave their arenas)
    assert_eq!(b.face_list().len(), 10);
    assert_eq!(b.edge_list().len(), 23);
    assert_eq!(b.vertex_list().len(), 15);
    // Each end of the round lies in the front face's plane y = 0: its rail on the top
    // at radius 12.5, its rail on the cylinder 2 up.
    for x in [-1.0, 1.0] {
        assert!(has_vertex(&b, Point3::new(x * (R_CYL + RADIUS), 0.0, 5.0)));
        assert!(has_vertex(&b, Point3::new(x * R_CYL, 0.0, 5.0 + RADIUS)));
    }
    let v = volume(&k, &mut b);
    let expected = expected_volume();
    assert!((v - expected).abs() < 1e-6 * expected, "{v} vs {expected}");
}
