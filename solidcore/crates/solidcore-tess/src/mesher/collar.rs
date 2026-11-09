//! The cuts that make every patch of a trimmed face four-sided. Each hole gets a square around
//! it in (u, v) (sides on iso-lines, halfway to the nearest other feature), joined to the hole at
//! the hole's four 45° points: the ring between hole and square is four collar patches (a quarter
//! of the hole's edge, a straight cut, a side of the square, a straight cut), the O-grid round a
//! hole. The motorcycle graph then runs with the squares as the obstacles (Lyon et al. 2019 §4):
//! a square's corners are its concave peaks, so the traces extend the squares' sides, and every
//! patch outside the collars is a rectangle bounded by iso-lines.

use super::motorcycle::{self, End as Stop};
use super::pieces::{End, Segment};

/// The face's cuts, given its boundary loops (ranges of `uvs`), the metric `scale` and the face's
/// seed (its node spacing, mm; traces less than a quarter of it apart meet head-on). `flat`: the
/// face is a plane, whose shading does not depend on its interior: a butterfly's square sides
/// take half the facing arc's count there (each wing steps 2:1), instead of the whole count. `None` when the face has no hole, or a hole has no room
/// for a collar (it touches another feature).
pub(crate) fn segments(uvs: &[[f64; 2]], loops: &[std::ops::Range<usize>], scale: [f64; 2], seed: f64, flat: bool) -> Option<Vec<Segment>> {
    let snap = 0.25 * seed;
    let metric = |q: [f64; 2]| [q[0] * scale[0], q[1] * scale[1]];
    let unmetric = |q: [f64; 2]| [q[0] / scale[0], q[1] / scale[1]];
    let area = |r: &std::ops::Range<usize>| -> f64 {
        let m = r.len();
        (0..m)
            .filter_map(|k| {
                let (a, b) = (metric(*uvs.get(r.start + k)?), metric(*uvs.get(r.start + (k + 1) % m)?));
                Some(0.5 * (a[0] * b[1] - b[0] * a[1]))
            })
            .sum()
    };
    // The outer loop encloses the most area; the others are holes.
    let outer = loops.iter().enumerate().max_by(|a, b| area(a.1).abs().total_cmp(&area(b.1).abs()))?.0;
    let holes: Vec<&std::ops::Range<usize>> = loops.iter().enumerate().filter(|(i, _)| *i != outer).map(|(_, r)| r).collect();
    let outer = loops.get(outer)?;
    let bbox = |r: &std::ops::Range<usize>| -> Option<([f64; 2], [f64; 2])> {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for k in r.clone() {
            let q = metric(*uvs.get(k)?);
            for d in 0..2 {
                lo[d] = lo[d].min(q[d]);
                hi[d] = hi[d].max(q[d]);
            }
        }
        Some((lo, hi))
    };
    // The node of loop `r` furthest out towards each corner of its box, in the box's own
    // proportions (its 45° points on a round loop), corner by corner from (min, min).
    let supports = |r: &std::ops::Range<usize>| -> Option<[usize; 4]> {
        let (lo, hi) = bbox(r)?;
        let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        let mut out = [0usize; 4];
        for (slot, dir) in out.iter_mut().zip([[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]) {
            let reach = |i: usize| {
                uvs.get(i).map_or(f64::MIN, |q| {
                    let q = metric(*q);
                    dir[0] * (q[0] - lo[0]) / w + dir[1] * (q[1] - lo[1]) / h
                })
            };
            *slot = (r.start..r.end).max_by(|&x, &y| reach(x).total_cmp(&reach(y)))?;
        }
        let mut distinct = out.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        (distinct.len() == 4).then_some(out)
    };
    // The arc of loop `r` (in nodes) between its support points `a` and `b`, the way that passes
    // neither of the other two.
    let side_count = |n: usize| if flat { n.div_ceil(2).max(1) } else { n.max(1) };
    let arc = |r: &std::ops::Range<usize>, p: &[usize; 4], k: usize| -> Option<usize> {
        let m = r.len();
        let (a, b) = (*p.get(k)? - r.start, *p.get((k + 1) % 4)? - r.start);
        let fwd = (b + m - a) % m;
        let others: Vec<usize> = (0..4).filter(|&j| j != k && j != (k + 1) % 4).filter_map(|j| p.get(j).map(|x| (x - r.start + m - a) % m)).collect();
        Some(if others.iter().all(|&o| o > fwd) { fwd } else { m - fwd })
    };
    // A face of one round loop (no hole, no concave peak; a disc, an ellipse): the butterfly.
    // A square inside it, its corners halfway from the loop's centre to the loop's four 45°
    // points, each joined to its point; each square side in as many segments as the arc facing
    // it. Five four-sided patches: the square and four wings.
    if holes.is_empty() {
        let alone: Vec<[f64; 2]> = outer.clone().filter_map(|k| uvs.get(k).copied()).collect();
        if !motorcycle::traces(&alone, &[std::ops::Range { start: 0, end: alone.len() }], scale, snap).is_empty() {
            return None;
        }
        let p = supports(outer)?;
        let (lo, hi) = bbox(outer)?;
        let centre = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1])];
        let mut corners = [[0.0; 2]; 4];
        for (c, &i) in corners.iter_mut().zip(&p) {
            let q = metric(*uvs.get(i)?);
            *c = unmetric([centre[0] + 0.5 * (q[0] - centre[0]), centre[1] + 0.5 * (q[1] - centre[1])]);
        }
        let mut segs = Vec::with_capacity(8);
        for k in 0..4 {
            segs.push(Segment {
                a: End::Point(*corners.get(k)?),
                b: End::Point(*corners.get((k + 1) % 4)?),
                count: Some(side_count(arc(outer, &p, k)?)),
            });
        }
        for (k, &i) in p.iter().enumerate() {
            segs.push(Segment { a: End::Point(*corners.get(k)?), b: End::Node(u32::try_from(i).ok()?), count: None });
        }
        return Some(segs);
    }
    // A square needs a strip at least a seed wide between it and the outer loop on every side
    // (and a seed of collar inside it): a hole with less than two seeds to the outer loop on any
    // side has no room for one, so the outer loop is its square, and the collar cuts join the
    // hole's 45° points to the outer loop's.
    let (olo, ohi) = bbox(outer)?;
    for hole in &holes {
        let (lo, hi) = bbox(hole)?;
        let gaps = [lo[0] - olo[0], lo[1] - olo[1], ohi[0] - hi[0], ohi[1] - hi[1]];
        if holes.len() == 1 && gaps.iter().any(|g| *g < 2.0 * seed) {
            let (inner, out) = (supports(hole)?, supports(outer)?);
            return inner
                .iter()
                .zip(out)
                .map(|(&a, b)| Some(Segment { a: End::Node(u32::try_from(a).ok()?), b: End::Node(u32::try_from(b).ok()?), count: None }))
                .collect();
        }
    }
    let boxes: Vec<([f64; 2], [f64; 2])> = holes.iter().map(|r| bbox(r)).collect::<Option<_>>()?;
    // Distances in the metric: point to box, point to segment, box to box.
    let to_box = |p: [f64; 2], (lo, hi): ([f64; 2], [f64; 2])| {
        let dx = (lo[0] - p[0]).max(0.0).max(p[0] - hi[0]);
        let dy = (lo[1] - p[1]).max(0.0).max(p[1] - hi[1]);
        dx.hypot(dy)
    };
    let to_segment = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if len2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
        (p[0] - a[0] - t * ab[0]).hypot(p[1] - a[1] - t * ab[1])
    };
    let corners_of = |(lo, hi): ([f64; 2], [f64; 2])| [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]];
    // Each square: the hole's box grown by half its clearance to the outer loop and a quarter of
    // its gap to another hole's box (both squares grow into that gap), so squares never touch.
    let m = outer.len();
    let mut squares: Vec<([f64; 2], [f64; 2])> = Vec::with_capacity(boxes.len());
    for (i, &bx) in boxes.iter().enumerate() {
        let mut clear = f64::INFINITY;
        for k in 0..m {
            let (a, b) = (metric(*uvs.get(outer.start + k)?), metric(*uvs.get(outer.start + (k + 1) % m)?));
            clear = clear.min(to_box(a, bx));
            for c in corners_of(bx) {
                clear = clear.min(to_segment(c, a, b));
            }
        }
        for (j, &other) in boxes.iter().enumerate() {
            if j != i {
                // Box to box: the gap along each axis.
                let gx = (other.0[0] - bx.1[0]).max(bx.0[0] - other.1[0]).max(0.0);
                let gy = (other.0[1] - bx.1[1]).max(bx.0[1] - other.1[1]).max(0.0);
                // Both holes' squares grow into this gap: each takes half of it.
                clear = clear.min(0.5 * gx.hypot(gy));
            }
        }
        let margin = 0.5 * clear;
        if !(margin.is_finite() && margin > 0.0) {
            return None;
        }
        squares.push(([bx.0[0] - margin, bx.0[1] - margin], [bx.1[0] + margin, bx.1[1] + margin]));
    }
    let mut segs: Vec<Segment> = Vec::new();
    // Each hole's four 45° points (towards its square's corners, in order).
    let picks: Vec<[usize; 4]> = holes.iter().map(|r| supports(r)).collect::<Option<_>>()?;
    // Square sides (four per hole, corner k to corner k + 1), each in as many segments as the
    // hole's arc facing it (between its 45° points k and k + 1, the way that passes neither
    // other point): the wings of the butterfly then have matching opposite sides.
    for ((&sq, hole), p) in squares.iter().zip(&holes).zip(&picks) {
        let c = corners_of(sq).map(unmetric);
        for k in 0..4 {
            segs.push(Segment { a: End::Point(*c.get(k)?), b: End::Point(*c.get((k + 1) % 4)?), count: Some(side_count(arc(hole, p, k)?)) });
        }
    }
    for (p, &sq) in picks.iter().zip(&squares) {
        let c = corners_of(sq).map(unmetric);
        for (k, &i) in p.iter().enumerate() {
            segs.push(Segment { a: End::Point(*c.get(k)?), b: End::Node(u32::try_from(i).ok()?), count: None });
        }
    }
    // The motorcycles, with the squares as the obstacles: the outer loop, then each square as a
    // loop of its four corners.
    let mut domain: Vec<[f64; 2]> = outer.clone().filter_map(|k| uvs.get(k).copied()).collect();
    let mut dloops = vec![std::ops::Range { start: 0, end: domain.len() }];
    for &sq in &squares {
        let start = domain.len();
        domain.extend(corners_of(sq).map(unmetric));
        dloops.push(start..start + 4);
    }
    let traces = motorcycle::traces(&domain, &dloops, scale, snap);
    let base = segs.len();
    let on_outer = |i: usize| i < outer.len();
    for (ti, t) in traces.iter().enumerate() {
        let start = if on_outer(t.start) { End::Node(u32::try_from(outer.start + t.start).ok()?) } else { End::Point(*domain.get(t.start)?) };
        let end = match t.end {
            Stop::Boundary(a, b) if on_outer(a) && on_outer(b) => {
                // The nearer end of the boundary segment it reached (a cut never adds a node to
                // the boundary).
                let (pa, pb) = (metric(*domain.get(a)?), metric(*domain.get(b)?));
                let q = metric(t.b);
                let k = if (pa[0] - q[0]).hypot(pa[1] - q[1]) <= (pb[0] - q[0]).hypot(pb[1] - q[1]) { a } else { b };
                End::Node(u32::try_from(outer.start + k).ok()?)
            }
            Stop::Boundary(a, b) => {
                // A side of a square: corners `a` and `b` of square `s`.
                let (s, la, lb) = ((a - outer.len()) / 4, (a - outer.len()) % 4, (b - outer.len()) % 4);
                let k = if (la + 1) % 4 == lb { la } else { lb };
                End::On(4 * s + k, t.b)
            }
            Stop::Trace(c) => End::On(base + c, t.b),
            Stop::HeadOn(j) => {
                let o = traces.get(j)?;
                End::Point([0.5 * (t.b[0] + o.b[0]), 0.5 * (t.b[1] + o.b[1])])
            }
        };
        let _ = ti;
        segs.push(Segment { a: start, b: end, count: None });
    }
    Some(segs)
}
