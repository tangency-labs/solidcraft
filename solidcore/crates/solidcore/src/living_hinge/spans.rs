//! A row's spans of material (design 6.3), from the area's loops and the plate's loops.
//!
//! For row `k`, `J_k = (row ∩ F) ∩ (row ∩ M)`: from the sorted crossings of the row line with F's
//! loops and with M's loops, each sub-interval is kept when its midpoint is inside F and inside M
//! (by the parity of the crossings below it), and kept intervals sharing an end are joined. An end
//! is open when it is a crossing with M's boundary (within `ON_EDGE`), closed otherwise.
//!
//! Loops come in the hinge frame as [`NearEdge`]s; a conic or a Bézier as its chords. A crossing is
//! counted by the half-open rule (a point on the row counts as below it), so a vertex on the row is
//! counted once where the boundary passes through it and twice or not at all where it only
//! touches: the parity stays right. Neighbouring edges must share their end points exactly (the
//! caller snaps an arc's ends to the lines it meets).

use super::near::NearEdge;
use super::rows::RowSpan;
use super::ON_EDGE;

/// Whether `p` lies above the row `v` (a point on it counts as below).
fn above(p: (f64, f64), v: f64) -> bool {
    p.1 > v
}

/// The crossings of one edge with the row `v`, by the half-open rule, pushed to `out`.
fn edge_crossings(edge: &NearEdge, v: f64, out: &mut Vec<f64>) {
    match *edge {
        NearEdge::Line(a, b) => {
            if above(a, v) != above(b, v) {
                let t = (v - a.1) / (b.1 - a.1);
                out.push(a.0 + t * (b.0 - a.0));
            }
        }
        NearEdge::Arc {
            center,
            radius,
            start,
            sweep,
        } => arc_crossings(center, radius, start, sweep, v, out),
    }
}

/// The arc's crossings with the row `v`. The arc is cut at the circle's top and bottom into pieces
/// monotone in `v`, each counted by the half-open rule from its end points (worked out with one
/// formula, so two pieces meeting at an angle agree on which side their common point lies).
fn arc_crossings(
    center: (f64, f64),
    radius: f64,
    start: f64,
    sweep: f64,
    v: f64,
    out: &mut Vec<f64>,
) {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let point = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
    // At most a full turn: more would only trace the circle again.
    let sweep = sweep.clamp(-TAU, TAU);
    let end = start + sweep;
    let (lo, hi) = (start.min(end), start.max(end));
    let mut cuts = vec![lo];
    let mut n = ((lo - FRAC_PI_2) / PI).ceil();
    // A full turn holds at most three of the circle's top and bottom points strictly inside it.
    for _ in 0..4 {
        let a = FRAC_PI_2 + n * PI;
        if a >= hi {
            break;
        }
        if a > lo {
            cuts.push(a);
        }
        n += 1.0;
    }
    cuts.push(hi);
    let dy = v - center.1;
    let dx = (radius * radius - dy * dy).max(0.0).sqrt();
    for w in cuts.windows(2) {
        let &[a, b] = w else { continue };
        if above(point(a), v) != above(point(b), v) {
            let side = ((a + b) / 2.0).cos().signum();
            out.push(center.0 + side * dx);
        }
    }
}

/// Every crossing of the loops' edges with the row `v`, sorted.
pub fn row_crossings(loops: &[Vec<NearEdge>], v: f64) -> Vec<f64> {
    let mut out = Vec::new();
    for edge in loops.iter().flatten() {
        edge_crossings(edge, v, &mut out);
    }
    out.sort_by(f64::total_cmp);
    out
}

/// Whether `x` is inside the region whose sorted crossings on the row are `crossings`: an odd
/// number of them below it.
fn inside(crossings: &[f64], x: f64) -> bool {
    crossings.iter().filter(|&&c| c < x).count() % 2 == 1
}

/// The spans of material on the row `v`: where the area `field` and the plate `plate` overlap,
/// each end tagged open where it lies on the plate's boundary.
pub fn row_spans(field: &[Vec<NearEdge>], plate: &[Vec<NearEdge>], v: f64) -> Vec<RowSpan> {
    let f = row_crossings(field, v);
    let m = row_crossings(plate, v);
    let mut cuts: Vec<f64> = f.iter().chain(&m).copied().collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let on_plate_edge = |x: f64| m.iter().any(|&c| (c - x).abs() <= ON_EDGE);
    let mut spans: Vec<RowSpan> = Vec::new();
    for w in cuts.windows(2) {
        let &[lo, hi] = w else { continue };
        let mid = (lo + hi) / 2.0;
        if !(inside(&f, mid) && inside(&m, mid)) {
            continue;
        }
        match spans.last_mut() {
            Some(last) if last.hi == lo => last.hi = hi,
            _ => spans.push(RowSpan {
                lo,
                hi,
                lo_open: false,
                hi_open: false,
            }),
        }
    }
    for s in &mut spans {
        s.lo_open = on_plate_edge(s.lo);
        s.hi_open = on_plate_edge(s.hi);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(u0: f64, v0: f64, u1: f64, v1: f64) -> Vec<NearEdge> {
        let p = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
        p.iter()
            .zip(p.iter().cycle().skip(1))
            .map(|(&a, &b)| NearEdge::Line(a, b))
            .collect()
    }

    fn ends(spans: &[RowSpan]) -> Vec<(f64, f64)> {
        spans.iter().map(|s| (s.lo, s.hi)).collect()
    }

    fn near(got: &[RowSpan], want: &[(f64, f64)]) -> bool {
        got.len() == want.len()
            && got
                .iter()
                .zip(want)
                .all(|(g, w)| (g.lo - w.0).abs() < 1e-12 && (g.hi - w.1).abs() < 1e-12)
    }

    #[test]
    fn the_band_of_case_a_is_one_span_open_at_both_ends() {
        // Design 17.1 in the hinge frame: the plate is u 0..60 (its 60 mm side along the slots)
        // by v 0..100; the area is the band v 30..70, its ends on the plate's edges.
        let plate = [rect(0.0, 0.0, 60.0, 100.0)];
        let field = [rect(0.0, 30.0, 60.0, 70.0)];
        assert_eq!(
            row_spans(&field, &plate, 50.0),
            vec![RowSpan {
                lo: 0.0,
                hi: 60.0,
                lo_open: true,
                hi_open: true,
            }]
        );
    }

    #[test]
    fn an_area_inside_the_plate_has_closed_ends_and_a_hole_splits_the_row() {
        let plate = [rect(0.0, 0.0, 60.0, 100.0), rect(25.0, 40.0, 35.0, 60.0)];
        let field = [rect(10.0, 30.0, 50.0, 70.0)];
        assert_eq!(
            row_spans(&field, &plate, 50.0),
            vec![
                RowSpan {
                    lo: 10.0,
                    hi: 25.0,
                    lo_open: false,
                    hi_open: true,
                },
                RowSpan {
                    lo: 35.0,
                    hi: 50.0,
                    lo_open: true,
                    hi_open: false,
                },
            ]
        );
        // Past the hole's v range: one closed span.
        assert_eq!(
            row_spans(&field, &plate, 65.0),
            vec![RowSpan {
                lo: 10.0,
                hi: 50.0,
                lo_open: false,
                hi_open: false,
            }]
        );
    }

    #[test]
    fn a_vertex_on_the_row_keeps_the_parity() {
        // A diamond with its corners on the row v = 0 at u = -1 and u = 1: one span between them.
        let diamond = vec![
            NearEdge::Line((-1.0, 0.0), (0.0, -1.0)),
            NearEdge::Line((0.0, -1.0), (1.0, 0.0)),
            NearEdge::Line((1.0, 0.0), (0.0, 1.0)),
            NearEdge::Line((0.0, 1.0), (-1.0, 0.0)),
        ];
        let big = [rect(-5.0, -5.0, 5.0, 5.0)];
        let spans = row_spans(&[diamond.clone()], &big, 0.0);
        assert_eq!(ends(&spans), vec![(-1.0, 1.0)]);
        // Touching the row at a corner only (the row through the top corner): no span.
        assert!(row_spans(&[diamond], &big, 1.0).is_empty());
    }

    #[test]
    fn a_disc_area_gives_the_chord_and_its_ends_by_the_half_open_rule() {
        use std::f64::consts::{PI, TAU};
        let plate = [rect(-10.0, -10.0, 10.0, 10.0)];
        // A full circle of radius 5 as two half arcs, the first starting on the row v = 0.
        let disc = vec![
            NearEdge::Arc {
                center: (0.0, 0.0),
                radius: 5.0,
                start: 0.0,
                sweep: PI,
            },
            NearEdge::Arc {
                center: (0.0, 0.0),
                radius: 5.0,
                start: PI,
                sweep: PI,
            },
        ];
        let at = |v: f64| row_spans(&[disc.clone()], &plate, v);
        let s = at(3.0);
        assert!(near(&s, &[(-4.0, 4.0)]), "{s:?}");
        // The row through both arcs' ends: the one chord, closed at both ends.
        let s = at(0.0);
        assert!(near(&s, &[(-5.0, 5.0)]), "{s:?}");
        assert!(s.iter().all(|s| !s.lo_open && !s.hi_open));
        // Through the circle's top only: no span.
        assert!(at(5.0).is_empty());
        // The same circle clockwise, one arc of a full turn.
        let cw = vec![NearEdge::Arc {
            center: (0.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: -TAU,
        }];
        let s = row_spans(&[cw], &plate, 3.0);
        assert!(near(&s, &[(-4.0, 4.0)]), "{s:?}");
    }
}
