//! tangency-labs/solidcore#418 (WR-457): an r 2 round on a straight edge whose ends sit
//! on a tangent seam fails with status 1001. A stadium bar (a box and two cylinders along
//! Z) has a 6 mm slot cut across one end; the slot floor meets the flat top in the edge
//! to round, and each end of that edge is also on the seam where the top runs tangent
//! into the rounded end. `SmFilletNx1Corner::MakeCornerTopology` takes the tangent
//! top/round edge as the side edge, so it picks the round as the end face, which the
//! fillet never meets. The tangent side-edge skip (once the opt-in
//! `fillet_nx1_tangent_side_edge`, now the only path) skips it.
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
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
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

fn fillet(slot_floor: f64) -> solidcore::Result<(f64, Brep)> {
    let mut k = Kernel::default();
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

/// The bar less the slot (its half disc of the round end, and the strip of `slot_floor`
/// x 19 beyond the seam), less the round's section `4 - pi` (r 2, convex) over the slot's
/// 6 mm: the round's ends lie in the slot walls Z = 3.5 and 9.5, square to the edge.
fn expected_volume(slot_floor: f64) -> f64 {
    let pi = core::f64::consts::PI;
    14.0 * (25.0 * 19.0 + pi * R * R)
        - 6.0 * (pi * R * R / 2.0 + slot_floor * 2.0 * R)
        - 6.0 * (4.0 - pi)
}

#[test]
fn seam_edge_round_with_the_fix() {
    let (v, _) = fillet(0.0).unwrap();
    let want = expected_volume(0.0);
    eprintln!("seam volume {v} vs {want}");
    assert!((v - want).abs() < 1e-3, "{v} vs {want}");
}

/// Formerly `off_seam_round_builds_a_manifold_solid`; `fillet` still asserts the
/// manifold solid. The tolerance, 1e-3 absolute (MEASURED), is far below the round's own
/// 6 (4 - pi) = 5.15, so an unrounded or partly rounded body fails.
#[test]
fn off_seam_round_holds_its_closed_form() {
    let (v, _) = fillet(0.5).unwrap();
    let want = expected_volume(0.5);
    // pins the arithmetic of expected_volume
    assert!((want - 9706.665662772806).abs() < 1e-9);
    eprintln!("off-seam volume {v} vs {want}");
    assert!((v - want).abs() < 1e-3, "{v} vs {want}");
}

/// #17605's bar: 30 high, its one round end at X < 0, the slot 6 wide at Z 12..18 down to the
/// seam, and a notch X < -5 across the round end at its foot (`notch_at_foot`, Z < 4) or its
/// head (Z > 26). The notch's concave edge ends on the round end's face beside the slot.
fn bar_with_slot_and_notch(k: &mut Kernel, notch_at_foot: bool) -> Brep {
    let block = create_box(k, Point3::new(0.0, -R, 0.0), 25.0, 19.0, 30.0).unwrap();
    let end = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), R, 30.0).unwrap();
    let bar = boolean_union(k, block, end).unwrap();
    let slot = create_box(k, Point3::new(-12.0, -10.0, 12.0), 12.0, 20.0, 6.0).unwrap();
    let bar = boolean_difference(k, bar, slot).unwrap();
    let z = if notch_at_foot { -1.0 } else { 26.0 };
    let notch = create_box(k, Point3::new(-12.0, -10.0, z), 7.0, 20.0, 5.0).unwrap();
    boolean_difference(k, bar, notch).unwrap()
}

fn edge_with_midpoint(k: &Kernel, b: &Brep, p: Point3) -> solidcore::topology::EdgeId {
    get_edges(b)
        .unwrap()
        .into_iter()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let curve = edge::curve(b, e).unwrap().unwrap();
            let mid = curve
                .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
                .unwrap()[0];
            (mid - p).length() < 1e-6
        })
        .expect("edge")
}

/// The material a round of radius `r` adds along the notch's edge (X = -5): at X = -5 - r + r
/// sin(t) it is r (1 - cos(t)) thick (its ball's centre is at X = -5 - r) and reaches the round
/// end at Y = +-sqrt(R^2 - X^2). Simpson's rule on t, whose integrand is smooth over 0..pi/2
/// (|X| <= 7 < R): 2000 intervals are far past the tolerance below.
fn notch_round_volume(r: f64) -> f64 {
    let n = 2000_u32;
    let h = core::f64::consts::FRAC_PI_2 / f64::from(n);
    let f = |t: f64| {
        let x = -5.0 - r + r * t.sin();
        r * (1.0 - t.cos()) * 2.0 * (R * R - x * x).sqrt() * r * t.cos()
    };
    let sum: f64 = (0..=n)
        .map(|i| {
            let w = match i {
                0 => 1.0,
                _ if i == n => 1.0,
                _ if i % 2 == 1 => 4.0,
                _ => 2.0,
            };
            w * f(f64::from(i) * h)
        })
        .sum();
    sum * h / 3.0
}

/// #17605: the slot's round ends at V on the slot's wall W, past the tangent seam: the cap's
/// extended edge is the arc where W meets the round end C. In one call with the notch's edge,
/// whose Nx1 corners end on C, both of the arc's faces have an extended surface. The cap's
/// edge took the first of them round the arc, C where the notch is at the bar's head, whose
/// surface only touches the top's: the fillet failed. With the notch at its foot W came first.
#[test]
fn seam_edge_round_beside_a_corner_ending_on_the_arcs_other_face() {
    for notch_at_foot in [true, false] {
        for r in [1.0, 2.0] {
            for slot_first in [true, false] {
                let case = format!("notch at foot {notch_at_foot}, r {r}, slot first {slot_first}");
                let mut k = Kernel::default();
                let mut b = bar_with_slot_and_notch(&mut k, notch_at_foot);
                let volume = |k: &Kernel, b: &mut Brep| {
                    let store = AttributeStore::default();
                    let attrs = PropertyAttributes {
                        store: &store,
                        key_of: &|_| ObjectKey(0),
                    };
                    brep_volume(k, b, 1e-6, &attrs).unwrap()
                };
                let notch_z = if notch_at_foot { 4.0 } else { 26.0 };
                let slot = edge_with_midpoint(&k, &b, Point3::new(0.0, R, 15.0));
                let notch = edge_with_midpoint(&k, &b, Point3::new(-5.0, 0.0, notch_z));
                let edges = if slot_first {
                    [slot, notch]
                } else {
                    [notch, slot]
                };
                // the bar's closed form: the block, the half disc over 30 - 6 of its height, less
                // the notch's circular segment (X < -5) over 4
                let pi = core::f64::consts::PI;
                let segment = R * R * (5.0 / R).acos() - 5.0 * (R * R - 25.0).sqrt();
                let bar = 25.0 * 19.0 * 30.0 + 24.0 * pi * R * R / 2.0 - 4.0 * segment;
                let sharp = volume(&k, &mut b);
                assert!((sharp - bar).abs() < 1e-3, "{case}: {sharp} vs {bar}");
                fillet_edges(&mut k, &mut b, &edges, r, 1, 1, 1.0)
                    .unwrap_or_else(|e| panic!("{case}: {e:?}"));
                assert!(is_manifold_solid(&b).unwrap(), "{case}");
                let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                    .unwrap()
                    .lines();
                assert!(lines.is_empty(), "{case}: Level 2: {lines:?}");
                // convex over the slot's 6 mm, its ends square to it in W and its mate
                let want = bar - 6.0 * r * r * (1.0 - pi / 4.0) + notch_round_volume(r);
                let got = volume(&k, &mut b);
                assert!((got - want).abs() < 1e-3, "{case}: {got} vs {want}");
            }
        }
    }
}
