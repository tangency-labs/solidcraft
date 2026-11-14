//! Regression test for WR-621 (#1808; the fork's tangency-labs/smlib@2e8758b, capi test
//! `crease_rounds_from_half_a_degree`): `SmEdge::GetBlendEdgeuse` takes two faces for
//! tangent within 2 degrees, so a crease of 0.5 to 2 degrees cannot be rounded. With
//! `fillet_half_degree_crease` the limit is half a degree.
//!
//! The part is a block 40 x 20, 20 deep, whose top rises to a crease at x = 20, each half
//! sloping `turn / 2` degrees, so the faces turn by `turn` across it. A round of radius r
//! along the crease takes off `20 r^2 (tan(d/2) - d/2)`, d the turn. The fork asserts
//! this to 1e-6.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The rise of the crease above y = 20 for a turn of `turn` degrees.
fn rise(turn: f64) -> f64 {
    20.0 * (0.5 * turn * PI / 180.0).tan()
}

/// The creased block, extruded 20 along z.
fn block(k: &mut Kernel, turn: f64) -> Brep {
    let h = rise(turn);
    let curves = [
        line((0.0, 0.0), (40.0, 0.0)),
        line((40.0, 0.0), (40.0, 20.0)),
        line((40.0, 20.0), (20.0, 20.0 + h)),
        line((20.0, 20.0 + h), (0.0, 20.0)),
        line((0.0, 20.0), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, 20.0, 0.0, 3, false).unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The crease: the edge along z at (20, 20 + rise).
fn crease(k: &Kernel, b: &Brep, turn: f64) -> EdgeId {
    let h = rise(turn);
    let on = |p: Vec3| (p.x - 20.0).abs() < 1e-9 && (p.y - 20.0 - h).abs() < 1e-9;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            on(p) && on(q) && (p.z - q.z).abs() > 10.0
        })
        .expect("the crease edge")
}

/// The crease of the block turning `turn` degrees rounded `r`: the volume removed, or
/// the round's failure.
fn round(half_degree: bool, turn: f64, r: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    k.fixes.fillet_half_degree_crease = half_degree;
    let mut b = block(&mut k, turn);
    let before = volume(&k, &mut b);
    let e = crease(&k, &b, turn);
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0)?;
    assert!(
        is_manifold_solid(&b).unwrap(),
        "turn {turn}, r {r}: not a solid"
    );
    Ok(before - volume(&k, &mut b))
}

fn removed(turn: f64, r: f64) -> f64 {
    let d = turn * PI / 180.0;
    20.0 * r * r * ((0.5 * d).tan() - 0.5 * d)
}

#[test]
fn wr_621_creases_from_half_a_degree_round() {
    for turn in [1.0, 3.0] {
        for r in [2.0, 20.0] {
            let got = round(true, turn, r)
                .unwrap_or_else(|e| panic!("turn {turn}, r {r}: the round failed: {e:?}"));
            let want = removed(turn, r);
            assert!(
                (got - want).abs() < 1e-6,
                "turn {turn}, r {r}: removed {got}, expected {want}"
            );
        }
    }
    // within half a degree the faces are tangent: nothing to round
    for r in [2.0, 20.0] {
        assert!(round(true, 0.3, r).is_err(), "0.3 degrees, r {r}");
    }
}

/// Without the opt-in a 1 degree crease is taken for a smooth edge (within 2 degrees),
/// as upstream does; a 3 degree one rounds either way.
#[test]
fn wr_621_without_the_opt_in_a_one_degree_crease_is_refused() {
    for r in [2.0, 20.0] {
        assert!(round(false, 1.0, r).is_err(), "1 degree, r {r}");
        let got = round(false, 3.0, r).unwrap();
        assert!(
            (got - removed(3.0, r)).abs() < 1e-6,
            "3 degrees, r {r}: {got}"
        );
    }
}
