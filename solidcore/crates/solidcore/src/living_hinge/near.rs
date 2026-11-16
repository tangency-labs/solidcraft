//! Near sets on a row line (design 6.4): the one distance primitive of the edge rules.
//!
//! For an edge `e` and a reach `q`, its near set on the row line `v = v_k` is the open set
//! `{u : dist((u, v_k), e) < q}`, worked out in closed form for lines and arcs. A conic or a Bézier
//! comes here as its chords, each a line with reach `q + CHORD` (the caller widens the reach).
//! Sets are returned as sorted, disjoint open intervals `(lo, hi)`.

use std::f64::consts::TAU;

use super::{GRAZE, MIN_CROSSING};

/// An edge in the hinge frame, `(u, v)` coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NearEdge {
    /// The segment from the first point to the second.
    Line((f64, f64), (f64, f64)),
    /// An arc of the circle `center`, `radius`, from the angle `start` sweeping `sweep` (radians,
    /// counter-clockwise when positive).
    Arc {
        /// The circle's centre.
        center: (f64, f64),
        /// The circle's radius.
        radius: f64,
        /// The angle of the arc's first end.
        start: f64,
        /// The signed sweep from `start`.
        sweep: f64,
    },
}

/// The interval of `u` within `q` of the point `p` on the row `v`, if any.
fn disc(p: (f64, f64), v: f64, q: f64) -> Option<(f64, f64)> {
    let dy = (p.1 - v).abs();
    if dy.is_nan() || dy >= q {
        return None;
    }
    let h = ((q - dy) * (q + dy)).sqrt();
    Some((p.0 - h, p.0 + h))
}

/// The near set of the segment `ab` on the row `v`: its stadium is convex, so one interval or
/// none. The band part is the row cut by the parallelogram `a ± q n, b ± q n`, worked out with `a`
/// as the origin so that large coordinates keep their clearance; the end discs are joined to it.
fn near_line(a: (f64, f64), b: (f64, f64), v: f64, q: f64) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut take = |iv: Option<(f64, f64)>| {
        if let Some((l, h)) = iv {
            lo = lo.min(l);
            hi = hi.max(h);
        }
    };
    take(disc(a, v, q));
    take(disc(b, v, q));
    if len > 0.0 {
        // The parallelogram's corners relative to `a`, in order around it.
        let (nx, ny) = (-dy / len * q, dx / len * q);
        let corners = [(nx, ny), (dx + nx, dy + ny), (dx - nx, dy - ny), (-nx, -ny)];
        let row = v - a.1;
        for (i, &p) in corners.iter().enumerate() {
            let r = corners[(i + 1) % corners.len()];
            let (y0, y1) = (p.1 - row, r.1 - row);
            if y0 == y1 {
                if y0 == 0.0 {
                    // A side lying on the row: both its ends count.
                    take(Some((a.0 + p.0.min(r.0), a.0 + p.0.max(r.0))));
                }
            } else if (y0 <= 0.0) != (y1 <= 0.0) || y0 == 0.0 || y1 == 0.0 {
                let u = p.0 + (r.0 - p.0) * (y0 / (y0 - y1));
                take(Some((a.0 + u, a.0 + u)));
            }
        }
    }
    (lo < hi).then_some((lo, hi))
}

/// Whether the direction at `angle` lies on the arc from `start` sweeping `sweep`.
fn sweeps(start: f64, sweep: f64, angle: f64) -> bool {
    if sweep.abs() >= TAU {
        return true;
    }
    let (from, span) = if sweep >= 0.0 {
        (start, sweep)
    } else {
        (start + sweep, -sweep)
    };
    (angle - from).rem_euclid(TAU) <= span
}

/// The near set of an arc on the row `v`: the annulus `R - q < |x - c| < R + q` cut by the row,
/// kept where the direction from the centre lies on the arc, joined with the end discs.
fn near_arc(
    center: (f64, f64),
    radius: f64,
    start: f64,
    sweep: f64,
    v: f64,
    q: f64,
) -> Vec<(f64, f64)> {
    let dy = v - center.1;
    let mut out: Vec<(f64, f64)> = Vec::new();
    let outer = radius + q;
    if dy.abs() < outer {
        let ho = ((outer - dy.abs()) * (outer + dy.abs())).sqrt();
        let inner = radius - q;
        let hi_in = if inner > 0.0 && dy.abs() < inner {
            Some(((inner - dy.abs()) * (inner + dy.abs())).sqrt())
        } else {
            None
        };
        let bands: Vec<(f64, f64)> = match hi_in {
            Some(h) => vec![(-ho, -h), (h, ho)],
            None => vec![(-ho, ho)],
        };
        // Along the row the direction from the centre turns monotonically on each side of the
        // centre's foot, so the arc's set is cut only where an end's ray meets the row.
        let mut cuts = vec![0.0];
        for a in [start, start + sweep] {
            let (s, c) = a.sin_cos();
            if s != 0.0 && (dy / s) > 0.0 {
                cuts.push(dy * c / s);
            }
        }
        for (l, h) in bands {
            let mut points = vec![l, h];
            points.extend(cuts.iter().copied().filter(|&x| l < x && x < h));
            points.sort_by(f64::total_cmp);
            for w in points.windows(2) {
                let (x0, x1) = (w[0], w[1]);
                if x1 <= x0 {
                    continue;
                }
                let mid = (x0 + x1) / 2.0;
                if sweeps(start, sweep, dy.atan2(mid)) {
                    out.push((center.0 + x0, center.0 + x1));
                }
            }
        }
    }
    let end = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
    out.extend(disc(end(start), v, q));
    out.extend(disc(end(start + sweep), v, q));
    merge(out)
}

/// Sorts open intervals and joins those that overlap or touch.
fn merge(mut ivs: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ivs.retain(|iv| iv.0 < iv.1);
    ivs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(ivs.len());
    for iv in ivs {
        match out.last_mut() {
            Some(last) if iv.0 <= last.1 => last.1 = last.1.max(iv.1),
            _ => out.push(iv),
        }
    }
    out
}

/// The near set of `edge` with reach `q` on the row `v`.
pub fn near_set(edge: &NearEdge, v: f64, q: f64) -> Vec<(f64, f64)> {
    match *edge {
        NearEdge::Line(a, b) => near_line(a, b, v, q).into_iter().collect(),
        NearEdge::Arc {
            center,
            radius,
            start,
            sweep,
        } => near_arc(center, radius, start, sweep, v, q),
    }
}

/// The union of the near sets of `edges`, each with reach `q`, on the row `v`.
pub fn near_union<'a>(
    edges: impl IntoIterator<Item = &'a NearEdge>,
    v: f64,
    q: f64,
) -> Vec<(f64, f64)> {
    merge(edges.into_iter().flat_map(|e| near_set(e, v, q)).collect())
}

/// The pieces of the centre line `[a0, a1]` outside the open set `zone` (design 6.5): a piece
/// entering it is shortened to its boundary, one it splits gives two, one inside it gives none.
/// A point within `GRAZE` inside the set still counts as outside.
pub fn outside(a0: f64, a1: f64, zone: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut from = a0;
    for &(lo, hi) in zone {
        if hi - GRAZE <= from || hi <= lo + 2.0 * GRAZE {
            continue;
        }
        if lo + GRAZE >= a1 {
            break;
        }
        if lo + GRAZE > from {
            out.push((from, lo.min(a1)));
        }
        from = from.max(hi);
    }
    if from <= a1 {
        out.push((from, a1));
    }
    out
}

/// The component of the open set `zone` that holds `x`, if one does.
pub fn component(zone: &[(f64, f64)], x: f64) -> Option<(f64, f64)> {
    zone.iter().copied().find(|&(lo, hi)| lo < x && x < hi)
}

/// Whether the crossing of a row with the plate's edge at `x` is clean (design 6.6 rule 1).
/// `chain` is the near set of the crossing's chain with reach `rho`, `air_below` whether the air is
/// at `u < x`, `crossings` every crossing of the row with the plate's boundary (`x` among them),
/// `others` the union of the other near sets (closed edges and the plate's other chains). Clean
/// when the component `K` of `chain` holding `x` reaches at most `rho / sin(MIN_CROSSING)` into the
/// material, holds no other crossing, and meets no other near set.
pub fn clean_crossing(
    x: f64,
    air_below: bool,
    rho: f64,
    chain: &[(f64, f64)],
    crossings: &[f64],
    others: &[(f64, f64)],
) -> bool {
    let Some((lo, hi)) = component(chain, x) else {
        return false;
    };
    let into = if air_below { hi - x } else { x - lo };
    if into > rho / MIN_CROSSING.sin() + GRAZE {
        return false;
    }
    if crossings.iter().any(|&c| c != x && lo < c && c < hi) {
        return false;
    }
    !others.iter().any(|&(a, b)| a < hi && lo < b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1.0)
    }

    fn same(got: &[(f64, f64)], want: &[(f64, f64)]) {
        assert_eq!(got.len(), want.len(), "{got:?} against {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(
                close(g.0, w.0) && close(g.1, w.1),
                "{got:?} against {want:?}"
            );
        }
    }

    #[test]
    fn a_square_edge_reaches_q_either_side() {
        // The plate's edge u = 0 across the rows, reach rho = 1.4 (design 17.1).
        let e = NearEdge::Line((0.0, -10.0), (0.0, 80.0));
        same(&near_set(&e, 50.0, 1.4), &[(-1.4, 1.4)]);
    }

    #[test]
    fn an_edge_at_sixty_degrees_reaches_rho_over_sin() {
        let (s, c) = (60f64.to_radians()).sin_cos();
        let e = NearEdge::Line((-10.0 * c, -10.0 * s), (10.0 * c, 10.0 * s));
        let h = 1.4 / s;
        same(&near_set(&e, 0.0, 1.4), &[(-h, h)]);
    }

    #[test]
    fn a_side_parallel_to_the_row_covers_it_with_its_end_discs() {
        // The side v = 30 from u = 0 to u = 60, seen from the row 0.5 inside it with reach 1.4.
        let e = NearEdge::Line((0.0, 30.0), (60.0, 30.0));
        let h = (0.9f64 * 1.9).sqrt();
        same(&near_set(&e, 30.5, 1.4), &[(-h, 60.0 + h)]);
        assert!(near_set(&e, 31.5, 1.4).is_empty());
    }

    #[test]
    fn a_full_circle_gives_two_intervals_through_its_ring() {
        let e = NearEdge::Arc {
            center: (10.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: TAU,
        };
        same(&near_set(&e, 0.0, 1.0), &[(4.0, 6.0), (14.0, 16.0)]);
    }

    #[test]
    fn a_half_circle_keeps_its_own_side_and_its_end_discs() {
        // The upper half of the circle radius 5 at the origin, seen on the row v = 3.
        let e = NearEdge::Arc {
            center: (0.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: std::f64::consts::PI,
        };
        let ho = (36.0f64 - 9.0).sqrt();
        let hi = (16.0f64 - 9.0).sqrt();
        same(&near_set(&e, 3.0, 1.0), &[(-ho, -hi), (hi, ho)]);
        // Below the centre only the end discs at (+-5, 0) reach the row v = -0.5.
        let h = (0.5f64 * 1.5).sqrt();
        same(
            &near_set(&e, -0.5, 1.0),
            &[(-5.0 - h, -5.0 + h), (5.0 - h, 5.0 + h)],
        );
    }

    #[test]
    fn a_zone_shortens_splits_and_drops_a_centre_line() {
        same(
            &outside(20.1, 39.9, &[(24.9, 35.1)]),
            &[(20.1, 24.9), (35.1, 39.9)],
        );
        same(&outside(20.1, 39.9, &[(-1.0, 25.0)]), &[(25.0, 39.9)]);
        assert!(outside(20.1, 39.9, &[(10.0, 50.0)]).is_empty());
        same(&outside(20.1, 39.9, &[(40.0, 50.0)]), &[(20.1, 39.9)]);
    }

    #[test]
    fn a_square_crossing_is_clean_and_a_corner_or_a_slant_is_not() {
        // The edge u = 0, air below; rho = 1.4 (design 17.1 case A).
        let edge = NearEdge::Line((0.0, -10.0), (0.0, 80.0));
        let chain = near_set(&edge, 50.0, 1.4);
        assert!(clean_crossing(0.0, true, 1.4, &chain, &[0.0, 60.0], &[]));
        // A side edge alongside the row meets K: a plate corner.
        let side = NearEdge::Line((0.0, 50.5), (60.0, 50.5));
        let others = near_set(&side, 50.0, 1.4);
        assert!(!clean_crossing(
            0.0,
            true,
            1.4,
            &chain,
            &[0.0, 60.0],
            &others
        ));
        // An edge at 30 degrees to the row reaches rho / sin 30 = 2.8 > 1.617 into the material.
        let (s, c) = (30f64.to_radians()).sin_cos();
        let slant = NearEdge::Line((-10.0 * c, 50.0 - 10.0 * s), (10.0 * c, 50.0 + 10.0 * s));
        let chain = near_set(&slant, 50.0, 1.4);
        assert!(!clean_crossing(0.0, true, 1.4, &chain, &[0.0], &[]));
    }
}
