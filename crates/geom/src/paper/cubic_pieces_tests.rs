//! Conics and Béziers as cubics: each piece is checked against the curve itself, by the distance
//! from points along it to their closest points on the curve, so the proof's bound is tested too.

use std::f64::consts::FRAC_1_SQRT_2;

use super::*;
use crate::MAX_BEZIER_CTRL;

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn bezier(points: &[Vec2]) -> Seg2 {
    let mut p = [Vec2::ZERO; MAX_BEZIER_CTRL];
    p[..points.len()].copy_from_slice(points);
    Seg2::Bezier { n: (points.len() - 1) as u8, p }
}

/// How far `q` is from `seg`, or a distance from it no more than `enough`: by `closest_param` when
/// that is within `enough` (it never finds a point nearer than the nearest), else from the samples
/// no more than 0.1 apart along the curve, the closest point between the samples either side of each one within 0.1 of the
/// nearest (where the curve crosses itself, another branch's sample may be the nearest), found by
/// thirds. `closest_param` starts from 64 samples even in parameter, which miss where a lopsided
/// conic (a weight near 0, or far above 1) spends little of its parameter.
fn distance(seg: &Seg2, q: Vec2, enough: f64) -> f64 {
    let d = |t: f64| seg.point_at(t).dist(q);
    let near = d(seg.closest_param(q));
    if near <= enough {
        return near;
    }
    let (mut samples, mut todo) = (vec![1.0], vec![(0.0, 1.0)]);
    while let Some((t0, t1)) = todo.pop() {
        let mid = (t0 + t1) / 2.0;
        if seg.point_at(t0).dist(seg.point_at(t1)) > 0.1 && t1 - t0 > 1e-12 {
            todo.extend([(mid, t1), (t0, mid)]);
        } else {
            samples.push(t0);
        }
    }
    samples.sort_by(f64::total_cmp);
    let nearest = samples.iter().map(|t| d(*t)).fold(f64::INFINITY, f64::min);
    let refined = (0..samples.len()).filter(|k| d(samples[*k]) <= nearest + 0.1).map(|k| {
        let (mut lo, mut hi) = (samples[k.saturating_sub(1)], samples[(k + 1).min(samples.len() - 1)]);
        for _ in 0..200 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if d(a) < d(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        d((lo + hi) / 2.0)
    });
    refined.fold(near, f64::min)
}

/// The pieces join end to start from the curve's start to its end, and points along each, `samples`
/// to a piece, are within the bound reported of the curve (a little over, for where the closest point
/// is found), which is within `error`.
fn within_by(seg: &Seg2, error: f64, samples: u32) {
    let (pieces, bound) = cubic_pieces(seg, error).unwrap();
    assert!(bound <= error, "{seg:?}: bound {bound} over {error}");
    assert_eq!(pieces.first().unwrap().start(), seg.start());
    assert_eq!(pieces.last().unwrap().end(), seg.end());
    for pair in pieces.windows(2) {
        assert_eq!(pair[0].end(), pair[1].start());
    }
    for piece in &pieces {
        assert!(matches!(piece, Seg2::Cubic { .. }));
        for i in 0..=samples {
            let off = distance(seg, piece.point_at(f64::from(i) / f64::from(samples)), bound);
            assert!(off <= bound * 1.001 + 1e-9, "{seg:?}: {off} from the curve, bound {bound}, asked {error}");
        }
    }
}

/// A parabola raised to a cubic is exact: one piece, bound 0 to rounding.
#[test]
fn a_parabola_is_one_exact_cubic() {
    let parabola = Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 30.0), b: p(20.0, 0.0), w: 1.0 };
    let (pieces, bound) = cubic_pieces(&parabola, 1e-9).unwrap();
    let [Seg2::Cubic { p0, p1, p2, p3 }] = pieces.as_slice() else { panic!("one cubic: {pieces:?}") };
    for (got, want) in [(p0, p(0.0, 0.0)), (p1, p(20.0 / 3.0, 20.0)), (p2, p(40.0 / 3.0, 20.0)), (p3, p(20.0, 0.0))] {
        assert!(got.dist(want) < 1e-12, "{got:?} {want:?}");
    }
    assert!(bound < 1e-12, "{bound}");
}

/// A quarter of a circle of radius 100 as a conic: its cubics keep within the error of the circle,
/// and a finer error takes more of them.
#[test]
fn a_circular_conic_keeps_within_the_error_of_the_circle() {
    let quarter = Seg2::Conic { a: p(100.0, 0.0), apex: p(100.0, 100.0), b: p(0.0, 100.0), w: FRAC_1_SQRT_2 };
    let mut counts = Vec::new();
    for error in [1e-2, 1e-4, 1e-6] {
        let (pieces, _) = cubic_pieces(&quarter, error).unwrap();
        for piece in &pieces {
            for i in 0..=64 {
                let off = (piece.point_at(f64::from(i) / 64.0).len() - 100.0).abs();
                assert!(off <= error, "{error}: {off}");
            }
        }
        counts.push(pieces.len());
        within(&quarter, error);
    }
    assert!(counts.windows(2).all(|c| c[0] < c[1]), "{counts:?}");
}

/// Ellipses and hyperbolas of many weights, the curve's own Béziers of degree 4 to 9, and a
/// degree-4 curve that is a cubic raised, which is one piece.
#[test]
fn conics_and_higher_beziers_keep_within_the_error() {
    for w in [1e-6, 0.1, 0.5, 0.9, 1.5, 2.0, 10.0, 1e3] {
        within(&Seg2::Conic { a: p(0.0, 0.0), apex: p(30.0, 40.0), b: p(60.0, -5.0), w }, 1e-4);
    }
    let wavy: Vec<Vec2> = (0..10).map(|i| p(f64::from(i) * 10.0, if i % 2 == 0 { 0.0 } else { 25.0 })).collect();
    for degree in 4..=9 {
        within(&bezier(&wavy[..=degree]), 1e-4);
    }
    // The cubic (0,0) (10,30) (30,30) (40,0) raised to degree 4.
    let raised = bezier(&[p(0.0, 0.0), p(7.5, 22.5), p(20.0, 30.0), p(32.5, 22.5), p(40.0, 0.0)]);
    let (pieces, bound) = cubic_pieces(&raised, 1e-9).unwrap();
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    assert!(bound < 1e-12, "{bound}");
}

/// [`within_by`] at 64 points a piece.
fn within(seg: &Seg2, error: f64) {
    within_by(seg, error, 64);
}

/// A curve that needs more than [`MAX_CUBICS`] pieces is refused, never drawn coarser: a quarter of a
/// circle of radius 10 m takes exactly that many at 1e-8 mm, and twice as many at 1e-9 mm.
#[test]
fn a_curve_past_the_cap_is_refused() {
    let quarter = Seg2::Conic { a: p(1e4, 0.0), apex: p(1e4, 1e4), b: p(0.0, 1e4), w: FRAC_1_SQRT_2 };
    assert_eq!(cubic_pieces(&quarter, 1e-8).unwrap().0.len(), MAX_CUBICS);
    assert!(cubic_pieces(&quarter, 1e-9).unwrap_err().0.contains("4096"));
}

/// The walk's order: the pieces run from the curve's start, whichever half was split last.
#[test]
fn pieces_come_in_the_curves_order() {
    let s = Seg2::Conic { a: p(0.0, 0.0), apex: p(0.0, 50.0), b: p(50.0, 50.0), w: 0.3 };
    let (pieces, _) = cubic_pieces(&s, 1e-6).unwrap();
    let along: Vec<f64> = pieces.iter().map(|c| s.closest_param(c.end())).collect();
    assert!(along.windows(2).all(|t| t[0] < t[1]), "{along:?}");
}

/// A cubic is itself; an error that isn't finite and above 0 is refused, as are lines and arcs;
/// a curve too sharp for the cap is refused, never drawn coarser; the sharpest weight a scene
/// takes ends one way or the other, without panicking.
#[test]
fn what_cant_be_cubics_is_refused() {
    let cubic = Seg2::Cubic { p0: p(0.0, 0.0), p1: p(1.0, 2.0), p2: p(3.0, 2.0), p3: p(4.0, 0.0) };
    assert_eq!(cubic_pieces(&cubic, 1e-4).unwrap(), (vec![cubic], 0.0));
    let ellipse = Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 10.0), b: p(20.0, 0.0), w: 0.5 };
    for error in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(cubic_pieces(&ellipse, error).is_err(), "{error}");
    }
    assert!(cubic_pieces(&Seg2::Line { a: p(0.0, 0.0), b: p(1.0, 0.0) }, 1e-4).is_err());
    assert!(cubic_pieces(&Seg2::Arc { center: p(0.0, 0.0), radius: 1.0, start: 0.0, sweep: 1.0 }, 1e-4).is_err());
    let refused = cubic_pieces(&ellipse, 1e-300).unwrap_err();
    assert!(refused.0.contains("4096"), "{refused}");
    for w in [1e12, 1e-12] {
        let sharp = Seg2::Conic { a: p(0.0, 0.0), apex: p(1e4, 1e4), b: p(2e4, 0.0), w };
        match cubic_pieces(&sharp, 1e-4) {
            Ok((pieces, bound)) => assert!(pieces.len() <= MAX_CUBICS && bound <= 1e-4),
            Err(e) => assert!(e.0.contains("4096"), "{e}"),
        }
    }
}

/// Random conics and Béziers of degree 4 to 9 across a sheet: each becomes cubics within the error
/// (checked at 8 points a piece, as there are many),
/// or is refused at the cap.
#[test]
fn fuzz_random_curves_keep_within_the_error() {
    let mut seed = 0x0489_0a3b_u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for case in 0..120 {
        let mut point = || p(next() * 400.0, next() * 300.0);
        let seg = if case % 2 == 0 {
            let (a, apex, b) = (point(), point(), point());
            Seg2::Conic { a, apex, b, w: 10f64.powf(next() * 6.0 - 3.0) }
        } else {
            let degree = 4 + (case / 2) % 6;
            bezier(&(0..=degree).map(|_| point()).collect::<Vec<_>>())
        };
        match cubic_pieces(&seg, 1e-3) {
            Ok(_) => {
                within_by(&seg, 1e-3, 8);
            }
            Err(e) => assert!(e.0.contains("4096"), "{case}: {e}"),
        }
    }
}
