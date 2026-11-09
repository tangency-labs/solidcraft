//! WR460 addendum: `fixes.fillet_refuse_crossing_rails` read the rails of the rounds
//! before they are trimmed, when their edge interval is still the inside-out
//! `Extent1d::new()`. Its samples then lay ~1e21 away, so the check either compared garbage
//! or failed in the closest-point query on a curved edge (status 1001), refusing rounds
//! that fit. SolidCraft oracle designs 36 and 34 are the repros: a box with rounded
//! (or chamfered) vertical edges, then its whole top loop rounded.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// A `w` x `d` x `h` box whose vertical edges get a section of size `s` (`xsect` 1 round,
/// 0 chamfer), then every edge of its top face a round of `r`: the body, its volume and
/// the second blend's result.
fn two_blends(fixed: bool, (w, d, h): (f64, f64, f64), xsect: u32, s: f64, r: f64) -> (Brep, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_crossing_rails = fixed;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), w, d, h).unwrap();
    let vertical = edges_where(&k, &b, |m| (m.z - 0.5 * h).abs() < 1e-6);
    fillet_edges(&mut k, &mut b, &vertical, s, xsect, 1, 1.0).unwrap();
    let top = edges_where(&k, &b, |m| (m.z - h).abs() < 1e-6);
    assert_eq!(top.len(), 8);
    fillet_edges(&mut k, &mut b, &top, r, 1, 1, 1.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    let v = volume(&k, &mut b);
    (b, v)
}

/// SolidCraft oracle design 36: 50 x 40 x 30, verticals R8, top loop R4.
#[test]
fn wr460_rounded_box_top_loop_is_kept() {
    let (_, v) = two_blends(true, (50.0, 40.0, 30.0), 1, 8.0, 4.0);
    let (_, upstream) = two_blends(false, (50.0, 40.0, 30.0), 1, 8.0, 4.0);
    assert_eq!(v.to_bits(), upstream.to_bits());
    // Pappus: the R4 section's centroid sits 0.2234 R4 inside both walls, so it runs
    // 34 + 24 twice on the flats and 4 quarter turns at 8 - 0.2234 * 4 on the corners.
    let section = 16.0 * (1.0 - 0.25 * PI);
    let c = 4.0 * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let path = 2.0 * (34.0 + 24.0) + 2.0 * PI * (8.0 - c);
    let base = (50.0 * 40.0 - (4.0 - PI) * 64.0) * 30.0;
    let want = base - section * path;
    assert!((v - want).abs() < 1e-5 * want, "{v} vs {want}");
}

/// SolidCraft oracle design 34: 40 x 30 x 20, verticals chamfered 5, top loop R2.
#[test]
fn wr460_chamfered_box_top_loop_is_kept() {
    let (_, v) = two_blends(true, (40.0, 30.0, 20.0), 0, 5.0, 2.0);
    let (_, upstream) = two_blends(false, (40.0, 30.0, 20.0), 0, 5.0, 2.0);
    assert_eq!(v.to_bits(), upstream.to_bits());
}
