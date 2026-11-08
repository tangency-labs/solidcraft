//! Regression test for WR-611 (#1809; the fork's tangency-labs/smlib@de4dae6, capi test
//! `knife_edge_chamfers` at d 0.5 and 0.8): `SmFilletGeom::TrimOriginalSurfaces` takes the
//! piece of a face 'inside' a fillet's rail as one whose binormal dot with the fillet's is
//! above a fixed 0.2, which refuses a fillet meeting its face at more than 78 degrees.
//! With `fillet_trim_inside_by_best_dot` the inside piece is the one with the largest
//! positive dot.
//!
//! The part is WR-570's knife edge: a 62 1/6 x 21 plate whose corner is cut by a circle of
//! radius 21 tangent to its back face y = 0 at x = TIP, extruded 10 (knife edges need
//! `fillet_knife_edge_rounds`, WR-570). Its edge is chamfered d (a linear section on the
//! ball of radius d, as the fork's capi chamfers a knife edge): the chord between the
//! ball's contacts takes off the triangle flat contact / tip / wall contact less the
//! wall's circular segment `R^2 / 2 (b - sin b)`, `b = acos((R - d) / (R + d))`, all the
//! way up. At d 0.5 the chord meets the flat face at 81 degrees (dot 0.15): without the
//! fix nothing is trimmed and the chamfer face is left apart from the body.
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
/// as WR-570's test makes it.
fn part(k: &mut Kernel, s: f64) -> Brep {
    let curves = if s > 0.0 {
        vec![
            line((0.0, 0.0), (TIP, 0.0)),
            Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
            line((TIP - R, -R), (0.0, -R)),
            line((0.0, -R), (0.0, 0.0)),
        ]
    } else {
        vec![
            line((0.0, 0.0), (0.0, -R)),
            line((0.0, -R), (-TIP + R, -R)),
            Curve::from(create_arc(Vec3::new(-TIP, -R, 0.0), R, 0.0, 90.0).unwrap()),
            line((-TIP, 0.0), (0.0, 0.0)),
        ]
    };
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

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

/// The volume the chamfer of size `d` takes off (the fork's closed form).
fn removed(d: f64) -> f64 {
    let flat = TIP - (4.0 * R * d).sqrt();
    let wy = -R + (R - d) * R / (R + d);
    let triangle = 0.5 * ((TIP - flat) * wy).abs();
    let b = ((R - d) / (R + d)).acos();
    (triangle - 0.5 * R * R * (b - b.sin())) * H
}

/// The knife edge of the part (mirrored when `s` is -1) chamfered `d`: its volume, or why
/// it failed (the chamfer's error, or a body that is not a solid).
fn chamfer(best_dot: bool, s: f64, d: f64) -> Result<f64, String> {
    let mut k = Kernel::default();
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_trim_inside_by_best_dot = best_dot;
    let mut b = part(&mut k, s);
    let e = knife_edge(&k, &b, s);
    // the linear section (`xsect_type` 0) on the ball of radius d
    fillet_edges(&mut k, &mut b, &[e], d, 0, 1, 1.0).map_err(|e| format!("{e:?}"))?;
    if !is_manifold_solid(&b).unwrap() {
        return Err("not a solid".to_string());
    }
    Ok(volume(&k, &mut b))
}

#[test]
fn wr_611_steep_knife_edge_chamfers_trim_their_face() {
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    // the fork's check of its closed form at d 2
    assert!((removed(2.0) - 159.236_450_742_901_18).abs() <= 1e-12 * 159.236_450_742_901_18);
    for d in [0.5, 0.8, 1.0, 2.0, 5.0] {
        for s in [1.0, -1.0] {
            let v = chamfer(true, s, d)
                .unwrap_or_else(|e| panic!("d {d}, side {s}: the chamfer failed: {e}"));
            let want = before - removed(d);
            assert!(
                (v - want).abs() <= 1e-9 * want,
                "d {d}, side {s}: volume {v}, expected {want}"
            );
        }
    }
}

/// Without the opt-in the 0.5 and 0.8 chamfers (meeting the flat face past 78 degrees)
/// trim nothing: the chamfer fails or leaves a body that is not a solid. From d 1 they
/// build either way.
#[test]
fn wr_611_without_the_opt_in_steep_chamfers_fail() {
    for s in [1.0, -1.0] {
        for d in [0.5, 0.8] {
            assert!(chamfer(false, s, d).is_err(), "d {d}, side {s}");
        }
        if let Err(e) = chamfer(false, s, 2.0) {
            panic!("d 2, side {s}: {e}");
        }
    }
}
