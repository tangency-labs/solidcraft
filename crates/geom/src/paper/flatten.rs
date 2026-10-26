//! A path on a writer's grid as straight lines (#4890, A4), for a writer that draws only lines
//! (PNG): every point of every curve lies within the tolerance of them, and a path that would need
//! more than [`MAX_POINTS`] points is refused, never drawn coarser. It is the one flattener for the
//! paper scene: a writer that needs a curve as lines (PDF or DXF for what they can't hold) uses it.
//!
//! Each piece becomes the [`Seg2`] it is and takes [`Seg2::chords`]' steps, the bound
//! [`Seg2::polyline`] uses too. `polyline` itself doesn't fit here: it floors an arc's tolerance at
//! a millionth of its radius and caps a piece at 4096 steps, both coarser than asked, without a
//! word. An arc comes from the walk as its ends, radius and direction, which fix its centre (well
//! conditioned up to the walk's quarter turns).

use super::{GridPath, GridPoint, Invalid, Piece, Result};
use crate::{Seg2, Vec2};

/// The most points one path may flatten to.
pub const MAX_POINTS: usize = 1 << 20;

/// `path`'s points, from its start, joined by lines within `tolerance` steps (above 0) of its
/// curves. A closed path's closing line is left to the reader, as on the grid.
pub fn flatten(path: &GridPath, tolerance: f64) -> Result<Vec<Vec2>> {
    if !(tolerance > 0.0 && tolerance.is_finite()) {
        return Err(Invalid("a flattening tolerance is above 0"));
    }
    let mut points = vec![v(path.start)];
    let mut at = v(path.start);
    for piece in &path.pieces {
        let to = v(piece.to());
        let seg = match *piece {
            Piece::Line { .. } => Seg2::Line { a: at, b: to },
            Piece::Arc { radius, clockwise, .. } => {
                let r = radius as f64;
                let half = at.dist(to) / 2.0;
                // The centre sits off the chord's middle, left of it for an anticlockwise turn.
                let off = (r * r - half * half).max(0.0).sqrt();
                let side = (to - at).normalized().map_or(Vec2::new(0.0, 0.0), Vec2::perp);
                let center = (at + to) / 2.0 + side * if clockwise { -off } else { off };
                let sweep = 2.0 * (half / r).min(1.0).asin() * if clockwise { -1.0 } else { 1.0 };
                Seg2::Arc { center, radius: r, start: (at - center).angle(), sweep }
            }
            Piece::Cubic { c1, c2, .. } => Seg2::Cubic { p0: at, p1: v(c1), p2: v(c2), p3: to },
            Piece::Quadratic { c, .. } => Seg2::Conic { a: at, apex: v(c), b: to, w: 1.0 },
        };
        let n = steps(seg.chords(tolerance), points.len())?;
        points.extend((1..n).map(|i| seg.point_at(i as f64 / n as f64)));
        points.push(to);
        at = to;
    }
    Ok(points)
}

fn v(p: GridPoint) -> Vec2 {
    Vec2::new(p.x as f64, p.y as f64)
}

/// `bound` rounded up to whole steps, at least one, refused when a path that has `so_far` points
/// would go over [`MAX_POINTS`].
fn steps(bound: f64, so_far: usize) -> Result<usize> {
    let n = bound.ceil().max(1.0);
    if n.is_finite() && so_far as f64 + n <= MAX_POINTS as f64 {
        Ok(n as usize)
    } else {
        Err(Invalid("a curve needs over 1,048,576 points at this resolution"))
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    use super::*;
    use crate::Seg2;
    use crate::paper::{Grid, GridItem, Group, Item, ObjectKind, Scene};

    /// The scene's one stroke along `seg` on the nanometre grid.
    fn on_grid(seg: Seg2) -> GridPath {
        let scene = Scene::new(
            Vec2::new(420.0, 297.0),
            vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items: vec![Item::Stroke { path: vec![seg], closed: false, width_mm: 0.35 }] }],
        );
        let on = scene.prepare().unwrap().on_grid(Grid::NANOMETRE).unwrap();
        let GridItem::Stroke { path, .. } = &on.groups[0].items[0] else { panic!("a stroke") };
        path.clone()
    }

    fn distance_to_lines(p: Vec2, points: &[Vec2]) -> f64 {
        points.windows(2).map(|w| p.extend(0.0).dist_to_segment(w[0].extend(0.0), w[1].extend(0.0))).fold(f64::INFINITY, f64::min)
    }

    /// Every curve point within the tolerance of the lines, every line point on (or, for a chord,
    /// within the tolerance of) the curve, in nanometres: arcs either way round of every size,
    /// cubics and parabolas.
    #[test]
    fn every_curve_lies_within_the_tolerance_of_its_lines() {
        let p = Vec2::new;
        let mm = |x: f64| x * 1e6;
        for tolerance in [1.0, 132.3, 1323.0, 1e5] {
            for (radius, start, sweep) in [(10.0, 0.0, FRAC_PI_2), (10.0, 1.0, -FRAC_PI_2), (0.01, 0.3, TAU), (1e4, -2.0, -PI), (250.0, 0.2, 1e-4)] {
                let center = p(100.0, 100.0);
                let points = flatten(&on_grid(Seg2::Arc { center, radius, start, sweep }), tolerance).unwrap();
                let c = p(mm(center.x), mm(center.y));
                for q in &points {
                    assert!((q.dist(c) - mm(radius)).abs() < 4.0, "a point on the circle: r {radius}, {:?}", q);
                }
                for k in 0..=1000 {
                    let on = c + Vec2::from_angle(start + sweep * f64::from(k) / 1000.0) * mm(radius);
                    let d = distance_to_lines(on, &points);
                    assert!(d <= tolerance + 2.0, "arc r {radius} sweep {sweep}, tolerance {tolerance}: {d}");
                }
            }
            let cubic = [p(0.0, 0.0), p(10.0, 40.0), p(30.0, -20.0), p(40.0, 0.0)];
            let points = flatten(&on_grid(Seg2::Cubic { p0: cubic[0], p1: cubic[1], p2: cubic[2], p3: cubic[3] }), tolerance).unwrap();
            for k in 0..=1000 {
                let (t, s) = (f64::from(k) / 1000.0, 1.0 - f64::from(k) / 1000.0);
                let q = cubic[0] * (s * s * s) + cubic[1] * (3.0 * s * s * t) + cubic[2] * (3.0 * s * t * t) + cubic[3] * (t * t * t);
                assert!(distance_to_lines(p(mm(q.x), mm(q.y)), &points) <= tolerance + 2.0, "cubic at {t}, tolerance {tolerance}");
            }
            let (a, c, b) = (p(0.0, 0.0), p(10.0, 30.0), p(20.0, 0.0));
            let points = flatten(&on_grid(Seg2::Conic { a, apex: c, b, w: 1.0 }), tolerance).unwrap();
            for k in 0..=1000 {
                let (t, s) = (f64::from(k) / 1000.0, 1.0 - f64::from(k) / 1000.0);
                let q = a * (s * s) + c * (2.0 * s * t) + b * (t * t);
                assert!(distance_to_lines(p(mm(q.x), mm(q.y)), &points) <= tolerance + 2.0, "parabola at {t}, tolerance {tolerance}");
            }
        }
    }

    /// As few points as the bounds allow, the last each piece's own end; lines as they are.
    #[test]
    fn the_point_counts_are_the_bounds() {
        let p = Vec2::new;
        // A quarter turn of 10 mm within 1 µm: 2·acos(1 − 1e3 / 1e7) a step, 56 steps.
        let quarter = on_grid(Seg2::Arc { center: p(50.0, 50.0), radius: 10.0, start: 0.0, sweep: FRAC_PI_2 });
        let points = flatten(&quarter, 1e3).unwrap();
        assert_eq!(points.len(), 57);
        assert_eq!(points.last().copied(), Some(v(quarter.pieces[0].to())));
        // A cubic whose largest second difference is √1700 ≈ 41.2 mm, within 1 µm: √(0.75 · 4.12e7 / 1e3) → 176.
        let cubic = on_grid(Seg2::Cubic { p0: p(0.0, 0.0), p1: p(10.0, 40.0), p2: p(30.0, 40.0), p3: p(40.0, 0.0) });
        assert_eq!(flatten(&cubic, 1e3).unwrap().len(), 1 + 176);
        // A parabola whose second difference is 60 mm, within 1 µm: √(0.25 · 6e7 / 1e3) → 123.
        let parabola = on_grid(Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 30.0), b: p(20.0, 0.0), w: 1.0 });
        assert_eq!(flatten(&parabola, 1e3).unwrap().len(), 1 + 123);
        let line = on_grid(Seg2::Line { a: p(1.0, 2.0), b: p(3.0, 4.0) });
        assert_eq!(flatten(&line, 1e3).unwrap(), [p(1e6, 2e6), p(3e6, 4e6)]);
        // A tolerance wider than the radius: eighth turns, so one chord for half a radian.
        let tiny = on_grid(Seg2::Arc { center: p(50.0, 50.0), radius: 1e-3, start: 0.0, sweep: 0.5 });
        assert_eq!(flatten(&tiny, 1e4).unwrap().len(), 2);
    }

    /// Over the cap is refused, never coarser; a tolerance must be above 0.
    #[test]
    fn too_many_points_or_no_tolerance_is_refused() {
        let circle = on_grid(Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 1e5, start: 0.0, sweep: TAU });
        assert!(flatten(&circle, 1e3).is_ok());
        let e = flatten(&circle, 0.1).unwrap_err();
        assert!(e.0.contains("1,048,576"), "{e}");
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(flatten(&circle, tolerance).is_err(), "{tolerance}");
        }
        // The cap counts the points a path already has: it may reach the cap, never pass it.
        assert_eq!(steps(10.0, MAX_POINTS - 10).ok(), Some(10));
        assert!(steps(10.0, MAX_POINTS - 9).is_err());
        assert!(steps(0.5, MAX_POINTS).is_err(), "a curve is at least one step");
    }
}
