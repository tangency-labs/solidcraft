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
    for r in [2.0, 1.0] {
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
