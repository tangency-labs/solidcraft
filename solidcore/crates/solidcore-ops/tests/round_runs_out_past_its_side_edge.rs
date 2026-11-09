//! SolidCraft's `smooth_edges_and_unfinished_tangent_chains_are_refused_saying_why`: the box
//! 60 x 40 x 20, its top back edge rounded 5, its top right edge rounded 3 alone, then the
//! upright (60, 40) rounded r alone. The upright runs on tangentially into the r 5 round's end
//! arc, and a blend run on there leaves the part, so it has no cap (#2656). Above
//! r = 5 - sqrt 21, where the rail reaches the arc's far end (60, 39.58, 17), the rail runs past
//! that end: the round goes on until it leaves the material.
//!
//! The body is convex, so the result is the rounded box cut by the upright's cylinder: at height
//! z the section's corner is X(z) = 60 up to z = 17, then on the r 3 round's arc, by Y(z) = 40 up
//! to z = 15, then on the r 5 round's arc, less the quarter disc about (60 - r, 40 - r).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test: the closed form uses roots

mod common;

use common::{edge_at, kernel_copy, rel, volume};
use solidcore::kernel::UpstreamFixes;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{make_box, session, Body, CheckLevel};

/// The box with its two top edges rounded.
fn rounded() -> Body {
    let mut f = UpstreamFixes::all();
    f.fillet_refuse_rounds_breaking_through = false;
    f.fillet_round_ends_on_outline = false;
    let on: Vec<&str> = f.flags().filter(|(_, on)| *on).map(|(n, _)| n).collect();
    session::set_thread_fixes(on.iter().copied()).unwrap();
    let b = make_box([0.0; 3], [60.0, 40.0, 20.0]).unwrap();
    let e = edge_at(&b, [30.0, 40.0, 20.0]);
    blend(&b, Blend::Round, &[e], &[5.0], &[], Corners::Filled).unwrap();
    let e = edge_at(&b, [60.0, 20.0, 20.0]);
    blend(&b, Blend::Round, &[e], &[3.0], &[], Corners::Filled).unwrap();
    b
}

/// What the upright's round of radius `r` takes from the section at height `z`.
fn layer(z: f64, r: f64) -> f64 {
    let s = |h: f64, c: f64| (h * h - c * c).max(0.0).sqrt();
    let x = if z <= 17.0 {
        60.0
    } else {
        57.0 + s(3.0, z - 17.0)
    };
    let y = if z <= 15.0 {
        40.0
    } else {
        35.0 + s(5.0, z - 15.0)
    };
    let (a, b) = (x - (60.0 - r), y - (40.0 - r));
    if a <= 0.0 || b <= 0.0 || a * a + b * b <= r * r {
        return 0.0;
    }
    // the part of the corner [0, a] x [0, b] inside the disc: b to where the circle comes down
    // to height b, then under the circle
    let under = |t: f64| 0.5 * (t * s(r, t) + r * r * (t / r).min(1.0).asin());
    let x0 = s(r, b);
    a * b - (b * x0.min(a) + (under(a) - under(x0)).max(0.0))
}

/// What the round takes in all: constant to z = 15, Simpson's rule above.
fn taken(r: f64) -> f64 {
    let n = 100_000;
    let mut sum = 15.0 * layer(7.5, r);
    for (lo, hi) in [(15.0, 17.0), (17.0, 20.0)] {
        let h = (hi - lo) / f64::from(n);
        let mut s = layer(lo, r) + layer(hi, r);
        for i in 1..n {
            s += f64::from(if i % 2 == 1 { 4 } else { 2 }) * layer(lo + f64::from(i) * h, r);
        }
        sum += s * h / 3.0;
    }
    sum
}

#[test]
fn the_upright_round_runs_on_until_it_leaves_the_material() {
    // r 0.48 to 0.65 (#3062): where the round crosses the seam of the r 5 and r 3 rounds,
    // near (60, 40 - r, 17), the r 5 round's curve with the upright meets the r 3 round at a
    // grazing angle, so the vertex there is the three faces' common point
    for r in [2.0, 1.0, 0.7, 0.65, 0.6, 0.55, 0.5, 0.48, 0.45, 0.42] {
        let b = rounded();
        let v0 = volume(&b);
        let e = edge_at(&b, [60.0, 40.0, 8.0]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {}", report.text);
        let (_, brep) = kernel_copy(&b);
        assert_eq!(brep.face_list().len(), 9, "r {r}");
        let want = v0 - taken(r);
        assert!(rel(volume(&b), want) < 1e-8, "r {r}: {} {want}", volume(&b));
    }
}

/// #3062: at r 0.5 the edges at the vertex where the upright crosses the seam of the r 5 and
/// r 3 rounds lie on both their faces' surfaces along their last stretch (the continued
/// span, 1.2e-4 long, and on), within each edge's tolerance: measured at 200 points of the
/// last 1e-3 of each edge's length, by dropping each point on each face's surface.
#[test]
fn the_edges_at_the_grazing_crossing_lie_on_their_faces() {
    use solidcore::surface::surface::p4::NoObject;
    use solidcore::surface::surface::p5::DropPointOptions;
    use solidcore::topology::{edge, edgeuse, face, vertex};
    let r = 0.5;
    let b = rounded();
    let e = edge_at(&b, [60.0, 40.0, 8.0]);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    let (k, brep) = kernel_copy(&b);
    let near = solidcore::math::Point3::new(60.0, 40.0 - r, 17.0);
    let mut seen = 0;
    for &e in brep.edge_list() {
        let curve = edge::curve(&brep, e).unwrap().unwrap();
        let ivl = edge::interval(&brep, e).unwrap();
        let ends = [
            (edge::start_vertex(&brep, e).unwrap(), ivl.min()),
            (edge::end_vertex(&brep, e).unwrap(), ivl.max()),
        ];
        for (v, at) in ends {
            let Some(v) = v else { continue };
            let p = vertex::point(&brep, v).unwrap();
            if p.distance(near) > 0.2 || (p.z - 17.0).abs() < 1e-6 {
                continue;
            }
            seen += 1;
            let tol = edge::tolerance(&brep, e).unwrap();
            let span = 1e-3 * ivl.length();
            for eu in edge::edgeuses(&brep, e, None, None).unwrap() {
                let f = edgeuse::p1::face(&brep, eu).unwrap().unwrap();
                let s = face::p2::surface(&brep, f).unwrap().unwrap();
                let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
                let mut worst = 0.0_f64;
                for i in 0..=200 {
                    let t = if at == ivl.min() {
                        at + span * f64::from(i) / 200.0
                    } else {
                        at - span * f64::from(i) / 200.0
                    };
                    let x = curve.evaluate_point(&k, t).unwrap();
                    let d = s
                        .drop_point::<NoObject>(&k, x, &dom, &DropPointOptions::default())
                        .unwrap();
                    worst = worst.max(d.gap);
                }
                assert!(
                    worst <= tol,
                    "{e:?} on {f:?}: {worst:e} off, tolerance {tol:e}"
                );
            }
        }
    }
    assert!(seen >= 3, "the crossing's three edges, found {seen}");
}
