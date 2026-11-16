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

use super::near::{clean_crossing, merge, near_set, near_union, NearEdge};
use super::rows::RowSpan;
use super::{G1_TOL, ON_EDGE};

/// Whether `p` lies above the row `v` (a point on it counts as below).
fn above(p: (f64, f64), v: f64) -> bool {
    p.1 > v
}

/// The crossings of one edge with the row `v`, by the half-open rule, pushed to `out`.
fn edge_crossings(edge: &NearEdge, v: f64, out: &mut Vec<f64>) {
    match *edge {
        NearEdge::Line(a, b) | NearEdge::Chord(a, b) => {
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

/// The unit tangent of `edge` at its first end (`at_end` false) or its last end, in the direction
/// of travel.
fn tangent(edge: &NearEdge, at_end: bool) -> (f64, f64) {
    match *edge {
        NearEdge::Line(a, b) | NearEdge::Chord(a, b) => {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let n = dx.hypot(dy);
            (dx / n, dy / n)
        }
        NearEdge::Arc { start, sweep, .. } => {
            let a = if at_end { start + sweep } else { start };
            let s = sweep.signum();
            (-a.sin() * s, a.cos() * s)
        }
    }
}

/// Whether `next` continues `prev` with a turn of at most `G1_TOL` (one chain, design 2).
fn smooth(prev: &NearEdge, next: &NearEdge) -> bool {
    let (a, b) = (tangent(prev, true), tangent(next, false));
    let turn = (a.0 * b.1 - a.1 * b.0).atan2(a.0 * b.0 + a.1 * b.1).abs();
    turn <= G1_TOL
}

/// A chain id for every edge of `loops` (design 2: a maximal run of consecutive edges of one loop
/// meeting with a turn of at most `G1_TOL`), numbered over all loops in order. A loop that is
/// smooth all round is one chain.
pub fn chain_ids(loops: &[Vec<NearEdge>]) -> Vec<Vec<usize>> {
    let mut next = 0;
    let mut out = Vec::with_capacity(loops.len());
    for lp in loops {
        let first = next;
        let mut ids = Vec::with_capacity(lp.len());
        let mut prev: Option<&NearEdge> = None;
        for e in lp {
            if let Some(p) = prev {
                if !smooth(p, e) {
                    next += 1;
                }
            }
            ids.push(next);
            prev = Some(e);
        }
        if !lp.is_empty() {
            next += 1;
        }
        // The loop closes: a smooth join of its last edge to its first merges their chains.
        if let (Some(last), Some(head), Some(&last_id)) = (lp.last(), lp.first(), ids.last()) {
            if last_id != first && smooth(last, head) {
                for id in &mut ids {
                    if *id == last_id {
                        *id = first;
                    }
                }
            }
        }
        out.push(ids);
    }
    out
}

/// Every crossing of the loops' edges with the row `v` with its edge's chain id, sorted.
fn chain_crossings(loops: &[Vec<NearEdge>], ids: &[Vec<usize>], v: f64) -> Vec<(f64, usize)> {
    let mut out = Vec::new();
    let mut one = Vec::new();
    for (lp, lp_ids) in loops.iter().zip(ids) {
        for (edge, &id) in lp.iter().zip(lp_ids) {
            one.clear();
            edge_crossings(edge, v, &mut one);
            out.extend(one.iter().map(|&x| (x, id)));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
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
    let ids = chain_ids(plate);
    let mc = chain_crossings(plate, &ids, v);
    let m: Vec<f64> = mc.iter().map(|c| c.0).collect();
    let mut cuts: Vec<f64> = f.iter().chain(&m).copied().collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    // The chain of the plate crossing an end lies on (within ON_EDGE), the nearest one.
    let on_plate_edge = |x: f64| {
        mc.iter()
            .filter(|c| (c.0 - x).abs() <= ON_EDGE)
            .min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs()))
            .map(|c| c.1)
    };
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
                lo_chain: None,
                hi_chain: None,
            }),
        }
    }
    for s in &mut spans {
        s.lo_chain = on_plate_edge(s.lo);
        s.hi_chain = on_plate_edge(s.hi);
        s.lo_open = s.lo_chain.is_some();
        s.hi_open = s.hi_chain.is_some();
    }
    spans
}

/// Drops the open tag of every end of `spans` (on the row `v`) whose crossing is not clean
/// (design 6.6 rule 1): the near set of its own chain with reach `rho` must reach at most
/// `rho / sin(MIN_CROSSING)` into the material, hold no other crossing, and meet no other near set,
/// the plate's other chains with reach `rho` and the area's closed edges `closed` with reach
/// `closed_reach` (`r + m`). An end of unknown chain is left as it is.
pub fn clean_ends(
    spans: &mut [RowSpan],
    plate: &[Vec<NearEdge>],
    closed: &[NearEdge],
    v: f64,
    rho: f64,
    closed_reach: f64,
) {
    let ids = chain_ids(plate);
    let crossings = row_crossings(plate, v);
    let edges = || plate.iter().flatten().zip(ids.iter().flatten());
    let closed_near = near_union(closed, v, closed_reach);
    let clean = |x: f64, chain: usize, air_below: bool| {
        let own = near_union(edges().filter(|e| *e.1 == chain).map(|e| e.0), v, rho);
        let mut others = near_union(edges().filter(|e| *e.1 != chain).map(|e| e.0), v, rho);
        others.extend_from_slice(&closed_near);
        clean_crossing(x, air_below, rho, &own, &crossings, &others)
    };
    for s in spans {
        if let Some(c) = s.lo_chain.filter(|_| s.lo_open) {
            if !clean(s.lo, c, true) {
                s.lo_open = false;
                s.lo_chain = None;
            }
        }
        if let Some(c) = s.hi_chain.filter(|_| s.hi_open) {
            if !clean(s.hi, c, false) {
                s.hi_open = false;
                s.hi_chain = None;
            }
        }
    }
}

/// The keep-out zone `Z_c ∪ Z_o` on the row `v` (design 6.5) as sorted, disjoint open intervals:
/// the near sets of the area's closed edges `closed` with reach `closed_reach` (`r + m`) and of
/// every edge of the plate's loops `plate` with reach `rho`, all of them, not only those inside the
/// area.
pub fn row_zone(
    plate: &[Vec<NearEdge>],
    closed: &[NearEdge],
    v: f64,
    rho: f64,
    closed_reach: f64,
) -> Vec<(f64, f64)> {
    merge(
        closed
            .iter()
            .flat_map(|e| near_set(e, v, closed_reach))
            .chain(plate.iter().flatten().flat_map(|e| near_set(e, v, rho)))
            .collect(),
    )
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
    fn the_zone_joins_closed_sides_and_every_plate_edge() {
        // Plate u 0..60 by v 0..100 with a hole u 20..23 (v 40..60), 3 mm wide so its sides' near
        // sets stay 0.2 apart; a closed side at u = 40.
        let plate = [rect(0.0, 0.0, 60.0, 100.0), rect(20.0, 40.0, 23.0, 60.0)];
        let closed = [NearEdge::Line((40.0, 30.0), (40.0, 70.0))];
        let zone = row_zone(&plate, &closed, 50.0, 1.4, 2.1);
        let want = [
            (-1.4, 1.4),
            (18.6, 21.4),
            (21.6, 24.4),
            (37.9, 42.1),
            (58.6, 61.4),
        ];
        assert_eq!(zone.len(), want.len());
        for (g, w) in zone.iter().zip(&want) {
            assert!(
                (g.0 - w.0).abs() < 1e-12 && (g.1 - w.1).abs() < 1e-12,
                "{g:?} != {w:?}"
            );
        }
        // Far from the hole the row keeps only the plate's sides.
        assert_eq!(row_zone(&plate, &[], 20.0, 1.4, 2.1).len(), 2);
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
                lo_chain: Some(3),
                hi_chain: Some(1),
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
                    lo_chain: None,
                    hi_chain: Some(7),
                },
                RowSpan {
                    lo: 35.0,
                    hi: 50.0,
                    lo_open: true,
                    hi_open: false,
                    lo_chain: Some(5),
                    hi_chain: None,
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
                lo_chain: None,
                hi_chain: None,
            }]
        );
    }

    #[test]
    fn ends_near_a_closed_side_are_not_clean() {
        // Design 17.1 in the frame: plate u 0..60 by v 0..100, band v 30..70 with closed sides.
        let plate = [rect(0.0, 0.0, 60.0, 100.0)];
        let field = [rect(0.0, 30.0, 60.0, 70.0)];
        let sides = [
            NearEdge::Line((0.0, 30.0), (60.0, 30.0)),
            NearEdge::Line((60.0, 70.0), (0.0, 70.0)),
        ];
        let open = |v: f64, reach: f64| {
            let mut s = row_spans(&field, &plate, v);
            clean_ends(&mut s, &plate, &sides, v, 1.4, reach);
            s.iter().map(|s| (s.lo_open, s.hi_open)).collect::<Vec<_>>()
        };
        // Case A: the outer row 0.5 inside a side, reach r = 0.1: clean.
        assert_eq!(open(30.5, 0.1), vec![(true, true)]);
        assert_eq!(open(50.0, 0.1), vec![(true, true)]);
        // Case J: reach r + m = 2.1 (rho 2.1 too), the row 1.25 inside: not clean; 2.75: clean.
        let mut s = row_spans(&field, &plate, 31.25);
        clean_ends(&mut s, &plate, &sides, 31.25, 2.1, 2.1);
        assert!(!s[0].lo_open && !s[0].hi_open && s[0].lo_chain.is_none());
        let mut s = row_spans(&field, &plate, 32.75);
        clean_ends(&mut s, &plate, &sides, 32.75, 2.1, 2.1);
        assert!(s[0].lo_open && s[0].hi_open);
    }

    #[test]
    fn chains_end_at_corners_and_run_through_smooth_joins() {
        use std::f64::consts::PI;
        // A rectangle: four corners, four chains; a second loop numbers on.
        let ids = chain_ids(&[rect(0.0, 0.0, 2.0, 1.0), rect(5.0, 5.0, 6.0, 6.0)]);
        assert_eq!(ids, vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]]);
        // A stadium: line, half circle, line, half circle, all tangent: one chain, and the next
        // loop is chain 1.
        let stadium = vec![
            NearEdge::Line((0.0, 0.0), (4.0, 0.0)),
            NearEdge::Arc {
                center: (4.0, 1.0),
                radius: 1.0,
                start: -PI / 2.0,
                sweep: PI,
            },
            NearEdge::Line((4.0, 2.0), (0.0, 2.0)),
            NearEdge::Arc {
                center: (0.0, 1.0),
                radius: 1.0,
                start: PI / 2.0,
                sweep: PI,
            },
        ];
        let ids = chain_ids(&[stadium, rect(9.0, 0.0, 10.0, 1.0)]);
        assert_eq!(ids, vec![vec![0, 0, 0, 0], vec![1, 2, 3, 4]]);
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
        let spans = row_spans(std::slice::from_ref(&diamond), &big, 0.0);
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
        let at = |v: f64| row_spans(std::slice::from_ref(&disc), &plate, v);
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
