//! Regression test for WR-570 (the fork's tangency-labs/smlib@b7ec55b, capi test
//! `knife_edge_rounds`): a knife edge (a cusp, two faces tangent along the edge that
//! fold back on each other) rounds to the ball touching both faces back from it.
//!
//! The part is a 62 1/6 x 21 plate whose corner is cut by a circle of radius 21 about
//! (TIP, -21), extruded 10: the arc wall meets the back face y = 0 tangentially at
//! x = TIP. Upstream `SmEdge::GetBlendEdgeuse` takes that edge for a smooth one and
//! the round fails. With `fillet_knife_edge_rounds` the ball of radius r, 2r wide,
//! touches the back face sqrt(4 R r) back from the tip and takes off the
//! quadrilateral tip / wall centre / ball centre / back contact, less the wall's
//! sector and the ball's, all the way up. The mirrored part (its edge uses reversed)
//! rounds the same. The volume is asserted to 1e-9 relative, as the fork asserts it.
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
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const R: f64 = 21.0;
const TIP: f64 = 62.0 + 1.0 / 6.0;
const H: f64 = 10.0;

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The knife-edge part, its tip at x = `s * TIP` (`s` = 1, or -1 for the mirror image),
/// the profile chained head to tail.
fn part(k: &mut Kernel, s: f64) -> Brep {
    let curves = if s > 0.0 {
        vec![
            line((0.0, 0.0), (TIP, 0.0)),
            // (TIP, 0) to (TIP - R, -R)
            Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
            line((TIP - R, -R), (0.0, -R)),
            line((0.0, -R), (0.0, 0.0)),
        ]
    } else {
        vec![
            line((0.0, 0.0), (0.0, -R)),
            line((0.0, -R), (-TIP + R, -R)),
            // (-TIP + R, -R) to (-TIP, 0)
            Curve::from(create_arc(Vec3::new(-TIP, -R, 0.0), R, 0.0, 90.0).unwrap()),
            line((-TIP, 0.0), (0.0, 0.0)),
        ]
    };
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

/// `SmApiBrepComputeVolume` at an integration accuracy well below the 1e-9 the volume
/// is checked to.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// The knife edge: the vertical edge at (`s * TIP`, 0).
fn knife_edge(k: &Kernel, b: &Brep, s: f64) -> EdgeId {
    let at_tip = |p: Point3| (p.x - s * TIP).abs() < 1e-6 && p.y.abs() < 1e-6;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            at_tip(p) && at_tip(q) && (p.z - q.z).abs() > H / 2.0
        })
        .expect("the knife edge")
}

/// The volume the round of radius `r` takes off.
fn removed(r: f64) -> f64 {
    let beta = ((R - r) / (R + r)).acos();
    let ox = TIP - (4.0 * R * r).sqrt();
    let quad = 0.5 * (TIP - ox) * (R + r);
    (quad - 0.5 * R * R * beta - 0.5 * r * r * (PI - beta)) * H
}

/// The knife edge of the part (mirrored when `s` is -1) rounded `r`: its face count and
/// volume, or the round's failure.
fn round(knife_edges: bool, s: f64, r: f64) -> solidcore::Result<(usize, f64, bool)> {
    let mut k = Kernel::default();
    k.fixes.fillet_knife_edge_rounds = knife_edges;
    let mut b = part(&mut k, s);
    let e = knife_edge(&k, &b, s);
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0)?;
    let faces = b.face_list().len();
    let manifold = is_manifold_solid(&b).unwrap();
    Ok((faces, volume(&k, &mut b), manifold))
}

#[test]
fn wr_570_knife_edge_rounds_to_the_ball_back_from_it() {
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    for r in [0.5, 2.0, 5.0] {
        for s in [1.0, -1.0] {
            let (faces, v, manifold) = round(true, s, r)
                .unwrap_or_else(|e| panic!("r {r}, side {s}: the round failed: {e:?}"));
            let want = before - removed(r);
            assert_eq!(faces, 7, "r {r}, side {s}: faces");
            assert!(manifold, "r {r}, side {s}: not a manifold solid");
            assert!(
                (v - want).abs() <= 1e-9 * want,
                "r {r}, side {s}: volume {v}, expected {want}"
            );
        }
    }
}

/// Without the opt-in the knife edge is taken for a smooth one, as upstream does.
#[test]
fn wr_570_without_the_opt_in_the_knife_edge_is_refused() {
    for s in [1.0, -1.0] {
        assert!(round(false, s, 2.0).is_err(), "side {s}");
    }
}
