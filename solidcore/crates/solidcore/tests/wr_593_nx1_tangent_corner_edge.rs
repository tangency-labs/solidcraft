//! WR-593 (tangency-labs/smlib@1ff7353): a stadium bar 10 high whose top meets a
//! cylinder of the bar's end radius standing on its round end, the bar's flat sides
//! running tangent into the cylinder (SolidCraft's test-peg part). Rounding the concave
//! arc where the top meets the cylinder's wall with r 2 needs WR-457's tangent
//! side-edge skip (once `fillet_nx1_tangent_side_edge`, now unconditional): at each end
//! of the arc `SmFilletNx1Corner::MakeCornerTopology` skips the tangent side edge (the
//! cylinder's seam, here where its two half-circle walls meet) and takes the edge
//! reached, the bar's flat side meeting its round end, as the corner's edge when it has
//! the arc's convexity. That edge is G1, so its convexity is only the sign of rounding
//! residues: with a sketch residue of 1e-10 in the bar's outline one end reads
//! "opposite" and the round failed with status 1001. A tangential edge there is now
//! taken as the corner's edge, as the exact geometry already has it.
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
use solidcore::mass::enclosure::enclose_volume;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
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

fn fillet(residue: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
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
fn residue_round_with_the_fix() {
    let v = fillet(RESIDUE).unwrap();
    let want = expected_volume();
    assert!((v - want).abs() < 1e-3 * want, "{v} vs {want}");
}

#[test]
fn exact_round_builds() {
    // main's volume, bit-identical with and without the fix; the closed form holds
    // to the round's approximation, as for the residue round
    let v = fillet(0.0).unwrap();
    let pinned = 3_581.983_727_673_527_5_f64;
    assert_eq!(v.to_bits(), pinned.to_bits(), "{v} vs {pinned}");
    let want = expected_volume();
    assert!((v - want).abs() < 1e-3 * want, "{v} vs {want}");
}

/// Formerly `residue_round_fails_by_default`. The residue round upstream refused with
/// status 1001 (the case this file pins) builds by default since
/// `fillet_nx1_tangent_side_edge` was removed: the body is valid at Level 2 and one closed
/// manifold solid. Its certified volume enclosure lies above the peg's volume (a concave
/// round only adds material) and below [`expected_volume`], the full half-turn sweep of
/// the round's section (at the arc's ends the section would overhang the bar's flat
/// sides, so the built round is trimmed there and adds less; ARGUED). The residue of 1e-10
/// must not change the body: the exact round's pinned volume (`exact_round_builds`) lies
/// within 1e-6 of the enclosure (MEASURED: that volume is the integrator's estimate).
/// The body is that sweep trimmed by the bar's side planes y = +-R: at radius `rho` the
/// planes cut `2 acos(R / rho)` off the half turn, so the round adds
/// `int (pi - 2 acos(R / rho)) rho h(rho) d rho` over the section, whose height above the
/// top is `h(rho) = r - sqrt(r^2 - (rho - R - r)^2)`. The enclosure must hold the peg plus
/// that integral, evaluated by a fine midpoint rule (MEASURED: the rule's error is not
/// bounded here; it agrees with the enclosure to 4e-6).
#[test]
fn residue_round_builds_by_default() {
    let mut k = Kernel::default();
    let mut body = peg(&mut k, RESIDUE);
    let e = concave_arc(&body);
    fillet_edges(&mut k, &mut body, &[e], RADIUS, 1, 1, 1.0).unwrap();
    let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "Level 2: {lines:?}");
    assert!(is_manifold_solid(&body).unwrap());
    let pi = core::f64::consts::PI;
    let peg = H * (2.0 * R * L + pi * R * R) + H * pi * R * R;
    let sweep = expected_volume();
    let width = 1e-7 * sweep;
    let enc = enclose_volume(&k, &mut body, width).unwrap();
    assert!(enc.width() <= width, "{enc:?}");
    assert!(peg < enc.lo && enc.hi < sweep, "{peg} < {enc:?} < {sweep}");
    let n = 200_000;
    let step = RADIUS / f64::from(n);
    let trimmed: f64 = (0..n)
        .map(|i| {
            let rho = R + (f64::from(i) + 0.5) * step;
            let t = rho - R - RADIUS;
            let h = RADIUS - (RADIUS * RADIUS - t * t).max(0.0).sqrt();
            (pi - 2.0 * (R / rho).min(1.0).acos()) * rho * h * step
        })
        .sum();
    assert!(
        enc.lo - 1e-5 <= peg + trimmed && peg + trimmed <= enc.hi + 1e-5,
        "trimmed sweep {} vs [{}, {}]",
        peg + trimmed,
        enc.lo,
        enc.hi
    );
    let exact = 3_581.983_727_673_527_5_f64;
    assert!(
        enc.lo - 1e-6 <= exact && exact <= enc.hi + 1e-6,
        "exact round {exact} vs [{}, {}]",
        enc.lo,
        enc.hi
    );
}
