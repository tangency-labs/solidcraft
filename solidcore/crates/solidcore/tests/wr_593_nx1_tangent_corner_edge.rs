//! WR-593 (tangency-labs/smlib@1ff7353): a stadium bar 10 high whose top meets a
//! cylinder of the bar's end radius standing on its round end, the bar's flat sides
//! running tangent into the cylinder (SolidCraft's test-peg part). Rounding the concave
//! arc where the top meets the cylinder's wall with r 2 needs WR-457's
//! `fillet_nx1_tangent_side_edge`: at each end of the arc
//! `SmFilletNx1Corner::MakeCornerTopology` skips the tangent side edge (the cylinder's
//! seam, here where its two half-circle walls meet) and takes the edge reached, the
//! bar's flat side meeting its round end, as the corner's edge when it has the arc's
//! convexity. That edge is G1, so its convexity is only the sign of rounding residues:
//! with a sketch residue of 1e-10 in the bar's outline one end reads "opposite" and the
//! round fails with status 1001. The opt-in `fillet_nx1_tangent_corner_edge` takes a
//! tangential edge there as the corner's edge, as the exact geometry already has it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const R: f64 = 5.0;
const L: f64 = 20.0;
const H: f64 = 10.0;
const RADIUS: f64 = 2.0;
/// The sketch solver's residue in the start of the bar's first side line.
const RESIDUE: f64 = 1e-10;

fn sweep(k: &mut Kernel, cs: &[Curve], h: f64) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), h, true).unwrap()
}

/// The stadium bar (sides y = -R and y = R from x = 0 to L, round ends about x = 0 and
/// x = L), extruded 0..H, and the cylinder about x = L, H..2H, made of two half-circle
/// walls whose seams are at the bar's tangent points.
fn peg(k: &mut Kernel, residue: f64) -> Brep {
    let p = |x: f64, y: f64, z: f64| Point3::new(x, y, z);
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let arc = |c: Point3, s: f64, e: f64| Curve::from(curves::create_arc(c, R, s, e).unwrap());
    let bar = [
        line(p(0.0, -R + residue, 0.0), p(L, -R, 0.0)),
        arc(p(L, 0.0, 0.0), -90.0, 90.0),
        line(p(L, R, 0.0), p(0.0, R, 0.0)),
        arc(p(0.0, 0.0, 0.0), 90.0, 270.0),
    ];
    let bar = sweep(k, &bar, H);
    let post = [
        arc(p(L, 0.0, H), 90.0, 270.0),
        arc(p(L, 0.0, H), -90.0, 90.0),
    ];
    let post = sweep(k, &post, H);
    boolean_union(k, bar, post).unwrap()
}

/// The concave half circle at z = H from (L, R) to (L, -R) on the bar's side of the
/// cylinder.
fn concave_arc(b: &Brep) -> EdgeId {
    let arcs: Vec<EdgeId> = get_edges(b)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let (Some(s), Some(t)) = edge::start_end_vertices(b, e).unwrap() else {
                return false;
            };
            let (s, t) = (b.vertex(s).unwrap().point, b.vertex(t).unwrap().point);
            (s.z - H).abs() < 1e-6
                && (t.z - H).abs() < 1e-6
                && (s.x - L).abs() < 1e-6
                && (t.x - L).abs() < 1e-6
                && (s.y - t.y).abs() > R
        })
        .collect();
    assert_eq!(arcs.len(), 1, "one concave arc");
    arcs[0]
}

fn fillet(corner_edge_fix: bool, residue: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_nx1_tangent_corner_edge = corner_edge_fix;
    let mut body = peg(&mut k, residue);
    let e = concave_arc(&body);
    fillet_edges(&mut k, &mut body, &[e], RADIUS, 1, 1, 1.0)?;
    assert!(is_manifold_solid(&body).unwrap());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    Ok(brep_volume(&k, &mut body, 1e-6, &attrs).unwrap())
}

/// The peg and the round's section (4 - pi) swept half a turn about the cylinder's
/// axis at its centroid, `R + RADIUS (10 - 3 pi) / (3 (4 - pi))` from it.
fn expected_volume() -> f64 {
    let pi = core::f64::consts::PI;
    let peg = H * (2.0 * R * L + pi * R * R) + H * pi * R * R;
    let section = RADIUS * RADIUS * (1.0 - pi / 4.0);
    let centroid = R + RADIUS * (10.0 - 3.0 * pi) / (3.0 * (4.0 - pi));
    peg + section * centroid * pi
}

#[test]
fn residue_round_fails_without_the_fix() {
    let err = fillet(false, RESIDUE).unwrap_err();
    assert!(err.to_string().contains("1001"), "{err}");
}

#[test]
fn residue_round_with_the_fix() {
    let v = fillet(true, RESIDUE).unwrap();
    let want = expected_volume();
    assert!((v - want).abs() < 1e-3 * want, "{v} vs {want}");
}

#[test]
fn exact_round_is_unchanged_by_the_fix() {
    let off = fillet(false, 0.0).unwrap();
    let on = fillet(true, 0.0).unwrap();
    assert_eq!(off.to_bits(), on.to_bits());
}

#[test]
fn residue_round_fails_by_default() {
    let mut k = Kernel::default();
    let mut body = peg(&mut k, RESIDUE);
    let e = concave_arc(&body);
    assert!(fillet_edges(&mut k, &mut body, &[e], RADIUS, 1, 1, 1.0).is_err());
}
