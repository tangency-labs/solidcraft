//! #1038: a small hole on a large curved face divides its intersection edges from each point's
//! neighbour (the vendored `IntersectionCurve::parameter_division`) instead of searching both
//! surfaces from nothing for every point. These pin the hinted division to the search from
//! nothing, so a refresh of the vendored crate can't quietly lose the speedup or change a body.

use std::time::{Duration, Instant};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;
use truck_modeling::{BoundedCurve, ParameterDivision1D};

use crate::{Body, BoolOp, build::extrude, cylinder, ops::boolean};

/// A 200 × 200 block whose top is one cubic wave across x (a single B-spline face), with a
/// Ø1 mm hole through it at the middle.
fn holed() -> (Body, Body) {
    let plane = Plane::new(Vec3::new(0.0, 200.0, 0.0), Vec3::X, Vec3::Z).unwrap();
    let (a, b, c, d) = (Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(200.0, 40.0), Vec2::new(0.0, 40.0));
    let top = Seg2::Cubic { p0: c, p1: Vec2::new(133.0, 70.0), p2: Vec2::new(67.0, 10.0), p3: d };
    let outer = Loop2 { segs: vec![Seg2::Line { a, b }, Seg2::Line { a: b, b: c }, top, Seg2::Line { a: d, b: a }] };
    let block = extrude(&plane, &[Region2 { outer, holes: vec![] }], 0.0, 200.0).unwrap().pop().unwrap();
    let tool = cylinder(Vec3::new(100.0, 100.0, -10.0), Vec3::Z, 0.5, 100.0).unwrap();
    let cut = boolean(&block, &tool, BoolOp::Cut).unwrap().unwrap();
    (block, cut)
}

fn intersection_curves(body: &Body) -> Vec<mt::Curve> {
    body.solid.edge_iter().map(|e| e.curve()).filter(|c| matches!(c, mt::Curve::IntersectionCurve(_))).collect()
}

fn timed<T>(f: impl Fn() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed())
}

#[test]
fn a_small_hole_in_a_large_spline_face_cuts_cleanly() {
    let (block, cut) = holed();
    assert!(cut.validity().is_empty(), "{:?}", cut.validity());
    // The hole adds its wall (two half cylinders) and splits nothing: the bottom and the wave
    // each gain a loop.
    assert_eq!(cut.face_count(), block.face_count() + 2);
    let curves = intersection_curves(&cut);
    assert!(!curves.is_empty(), "the hole's rim on the wave is an intersection edge");
    // The wave is within a few tenths of a millimetre of flat across a Ø1 hole, so the hole
    // removes a cylinder as tall as the wave at its axis (the cubic's middle: z = 40).
    let (v0, v1) = (block.volume_at(0.001).unwrap(), cut.volume_at(0.001).unwrap());
    let removed = std::f64::consts::PI * 0.25 * 40.0;
    assert!((v0 - v1 - removed).abs() < 0.05 * removed, "removed {} of {removed}", v0 - v1);
}

#[test]
fn hinted_division_matches_the_search_from_nothing_and_is_faster() {
    let (_, cut) = holed();
    let (mut hinted_time, mut scratch_time) = (Duration::ZERO, Duration::ZERO);
    for curve in intersection_curves(&cut) {
        let mt::Curve::IntersectionCurve(ic) = &curve else { continue };
        let range = ic.range_tuple();
        for tol in [0.01, 0.001] {
            let ((hp, hq), ht) = timed(|| ic.parameter_division(range, tol));
            let ((sp, _), st) = timed(|| mt::algo::curve::parameter_division(ic, range, tol));
            hinted_time += ht;
            scratch_time += st;
            // The same division up to rounding: the hinted projections converge to within the
            // Newton tolerance of the searched ones, so a split the chord test only just asks for
            // can come out one way or the other, but no more than that.
            assert_eq!((hp.first(), hp.last()), (sp.first(), sp.last()), "tol {tol}");
            assert!(hp.len().abs_diff(sp.len()) <= 2, "tol {tol}: {} points hinted, {} from nothing", hp.len(), sp.len());
            // Every hinted point is the curve's own point at its parameter.
            for (t, h) in hp.iter().zip(&hq) {
                let s = ic.search_triple(*t).unwrap().0;
                assert!(mt::MetricSpace::distance(*h, s) < 1e-6, "tol {tol}, t {t}: {h:?} vs {s:?}");
            }
        }
    }
    eprintln!("hinted {hinted_time:?}, from nothing {scratch_time:?}");
    // The hinted path skips each point's grid search of the B-spline face; a loose bound, so a
    // loaded runner doesn't trip it, but one a fallback to the search for every point would.
    assert!(hinted_time * 2 < scratch_time, "hinted {hinted_time:?}, from nothing {scratch_time:?}");
}

/// #4623: dividing an intersection edge at a tolerance finer than truck's 1e-6 (or one that is
/// not a number) panicked in truck's tolerance assertion, and the join that asked for it failed.
/// It divides at 1e-6 instead.
#[test]
fn dividing_an_intersection_edge_finer_than_truck_allows_divides_at_its_tolerance() {
    let (_, cut) = holed();
    let curves = intersection_curves(&cut);
    assert!(!curves.is_empty());
    for curve in curves {
        let mt::Curve::IntersectionCurve(ic) = &curve else { continue };
        let range = ic.range_tuple();
        let at_truck = ic.parameter_division(range, 1e-6);
        for tol in [5e-7, 1e-12, 0.0, -1.0, f64::NAN] {
            let got = std::panic::catch_unwind(|| ic.parameter_division(range, tol));
            let Ok(got) = got else { panic!("dividing at {tol:e} panicked") };
            assert_eq!(got, at_truck, "tolerance {tol:e}");
        }
    }
}
