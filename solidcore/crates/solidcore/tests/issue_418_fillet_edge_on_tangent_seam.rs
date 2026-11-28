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
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::det;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::get_edges;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
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

/// Simpson's rule over 0..`to`: 2000 intervals are far past the tolerances here for the smooth
/// integrands below.
fn simpson(to: f64, f: impl Fn(f64) -> f64) -> f64 {
    let n = 2000_u32;
    let h = to / f64::from(n);
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

/// The material a round of radius `r` adds along the notch's edge (X = -5): at X = -5 - r + r
/// sin(t) it is r (1 - cos(t)) thick (its ball's centre is at X = -5 - r) and reaches the round
/// end at Y = +-sqrt(R^2 - X^2), smooth over t = 0..pi/2 (|X| <= 7 < R).
fn notch_round_volume(r: f64) -> f64 {
    simpson(core::f64::consts::FRAC_PI_2, |t| {
        let x = -5.0 - r + r * det::sin(t);
        r * (1.0 - det::cos(t)) * 2.0 * (R * R - x * x).sqrt() * r * det::cos(t)
    })
}

/// The material a chamfer of `d` by `d` adds there: at X = -5 - u it is `d - u` thick.
fn notch_chamfer_volume(d: f64) -> f64 {
    simpson(d, |u| {
        let x = -5.0 - u;
        (d - u) * 2.0 * (R * R - x * x).sqrt()
    })
}

/// The sharp bar's closed form: the block, the half disc over 30 - 6 of its height, less the
/// notch's circular segment (X < -5) over 4.
fn bar_with_slot_and_notch_volume() -> f64 {
    let segment = R * R * det::acos(5.0 / R) - 5.0 * (R * R - 25.0).sqrt();
    25.0 * 19.0 * 30.0 + 24.0 * core::f64::consts::PI * R * R / 2.0 - 4.0 * segment
}

fn assert_volume(k: &Kernel, b: &mut Brep, want: f64, case: &str) {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let got = brep_volume(k, b, 1e-6, &attrs).unwrap();
    assert!((got - want).abs() < 1e-3, "{case}: {got} vs {want}");
}

/// `blend` on the slot's edge and the notch's in one call, for the notch at the foot and the
/// head, sizes 1 and 2 and both edge orders: the volume is the bar's and `change`'s, and the body
/// a manifold solid, clean at Level 2.
fn blends_beside_the_arcs_other_face(
    blend: impl Fn(&mut Kernel, &mut Brep, &[solidcore::topology::EdgeId], f64) -> solidcore::Result<()>,
    change: impl Fn(f64) -> f64,
) {
    for notch_at_foot in [true, false] {
        for size in [1.0, 2.0] {
            for slot_first in [true, false] {
                let case =
                    format!("notch at foot {notch_at_foot}, size {size}, slot first {slot_first}");
                let mut k = Kernel::default();
                let mut b = bar_with_slot_and_notch(&mut k, notch_at_foot);
                let notch_z = if notch_at_foot { 4.0 } else { 26.0 };
                let slot = edge_with_midpoint(&k, &b, Point3::new(0.0, R, 15.0));
                let notch = edge_with_midpoint(&k, &b, Point3::new(-5.0, 0.0, notch_z));
                let edges = if slot_first {
                    [slot, notch]
                } else {
                    [notch, slot]
                };
                let bar = bar_with_slot_and_notch_volume();
                assert_volume(&k, &mut b, bar, &case);
                blend(&mut k, &mut b, &edges, size).unwrap_or_else(|e| panic!("{case}: {e:?}"));
                assert!(is_manifold_solid(&b).unwrap(), "{case}");
                let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                    .unwrap()
                    .lines();
                assert!(lines.is_empty(), "{case}: Level 2: {lines:?}");
                assert_volume(&k, &mut b, bar + change(size), &case);
            }
        }
    }
}

/// #17605: the slot's round ends at V on the slot's wall W, past the tangent seam: the cap's
/// extended edge is the arc where W meets the round end C. In one call with the notch's edge,
/// whose Nx1 corners end on C, both of the arc's faces have an extended surface. The cap's
/// edge took the first of them round the arc, C where the notch is at the bar's head, whose
/// surface only touches the top's: the fillet failed. With the notch at its foot W came first.
#[test]
fn seam_edge_round_beside_a_corner_ending_on_the_arcs_other_face() {
    blends_beside_the_arcs_other_face(
        |k, b, edges, r| fillet_edges(k, b, edges, r, 1, 1, 1.0),
        // convex over the slot's 6 mm, its ends square to it in W and its mate
        |r| notch_round_volume(r) - 6.0 * r * r * (1.0 - core::f64::consts::PI / 4.0),
    );
}

/// The chamfers of the same edges, as `solidcore-ops` drives the session for them. Their bevelled
/// corners end on the same faces as the rounds'.
#[test]
fn seam_edge_chamfer_beside_a_corner_ending_on_the_arcs_other_face() {
    blends_beside_the_arcs_other_face(
        |k, b, edges, d| {
            {
                let mut s = FilletSession::new(k, b)?;
                s.prepare_chamfers_along(k, edges, &[ChamferSize::Distances([d, d])], &[])?;
                s.create_corners(k, None)?;
                for corner in s.fillet_corners()? {
                    s.set_corner_bevel(corner, true)?;
                }
                s.do_filleting(k, None)?;
            }
            stitch_and_orient(k, b, &StitchAndOrientOptions::default())
        },
        // a right triangle of legs `d` over the slot's 6 mm
        |d| notch_chamfer_volume(d) - 6.0 * d * d / 2.0,
    );
}
