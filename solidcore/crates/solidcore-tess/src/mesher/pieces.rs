//! A trimmed face cut into four-sided patches. The cuts are segments in (u, v): the squares
//! round the face's holes (each joined to its hole at four points, so the ring between them is
//! four collar patches) and the motorcycle traces that extend the squares' sides. Each cut is
//! sampled once and both patches beside it use those nodes (as faces share their edges' nodes),
//! so the patches meet exactly. A cut never adds a node to the face's boundary: it ends on a
//! boundary node, at a point inside the face (a square's corner, or where two traces met
//! head-on), or on another cut (a T-node, which that cut then passes through).

use std::collections::HashMap;

use solidcraft_geom::Ring;

/// Where a cut segment ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum End {
    /// A boundary node.
    Node(u32),
    /// A point inside the face; points at the same place are one node.
    Point([f64; 2]),
    /// A point on cut segment `.0` (a T-node: that segment passes through it).
    On(usize, [f64; 2]),
}

/// A cut from `a` to `b`, straight in (u, v).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Segment {
    pub a: End,
    pub b: End,
    /// Segments to sample it in (spread over its pieces between T-nodes by length), instead of
    /// the seed rule: a butterfly's square side takes the count of the hole arc facing it.
    pub count: Option<usize>,
}

/// A patch: its outer ring and any holes inside it, as indices into the boundary nodes followed
/// by the cut nodes, counter-clockwise (holes clockwise) in (u, v).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub ring: Ring<u32>,
    pub holes: Vec<Ring<u32>>,
}

/// Cut the face bounded by `loops` (ranges of `boundary`) along `segments`, measured in the
/// metric `scale`. `seeds(a, b, sa, sb)`: where to put nodes between two nodes of a cut (in
/// (u, v)), as fractions strictly between 0 and 1, given the spacing wanted at each (in the
/// metric): the boundary's own spacing at a boundary node, unbounded inside the face (the seed
/// rule then sets it). Returns the cut nodes (numbered after the boundary) and the patches;
/// `None` when the cuts do not make a planar subdivision (the face is then meshed whole).
pub(crate) fn cut(
    boundary: &[[f64; 2]],
    loops: &[std::ops::Range<usize>],
    segments: &[Segment],
    scale: [f64; 2],
    seeds: &dyn Fn([f64; 2], [f64; 2], f64, f64) -> Vec<f64>,
) -> Option<(Vec<[f64; 2]>, Vec<Piece>)> {
    let n = boundary.len();
    let metric = |q: [f64; 2]| [q[0] * scale[0], q[1] * scale[1]];
    let dist = |a: [f64; 2], b: [f64; 2]| {
        let (a, b) = (metric(a), metric(b));
        (a[0] - b[0]).hypot(a[1] - b[1])
    };
    let mut cut_uv: Vec<[f64; 2]> = Vec::new();
    let mut points: HashMap<[u64; 2], u32> = HashMap::new();
    // Each segment's end nodes, and the T-nodes other segments put on it.
    let mut ends: Vec<(u32, u32)> = Vec::with_capacity(segments.len());
    let mut along: Vec<Vec<u32>> = vec![Vec::new(); segments.len()];
    for s in segments {
        let mut key = |e: End| -> Option<u32> {
            let mut point = |q: [f64; 2]| -> Option<u32> {
                let bits = [q[0].to_bits(), q[1].to_bits()];
                if let Some(&k) = points.get(&bits) {
                    return Some(k);
                }
                let k = u32::try_from(n + cut_uv.len()).ok()?;
                cut_uv.push(q);
                points.insert(bits, k);
                Some(k)
            };
            match e {
                End::Node(k) => ((k as usize) < n).then_some(k),
                End::Point(q) => point(q),
                End::On(on, q) => {
                    let k = point(q)?;
                    along.get_mut(on)?.push(k);
                    Some(k)
                }
            }
        };
        let (a, b) = (key(s.a)?, key(s.b)?);
        ends.push((a, b));
    }
    let at_in = |cut_uv: &[[f64; 2]], k: u32| -> Option<[f64; 2]> {
        let k = k as usize;
        if k < n { boundary.get(k).copied() } else { cut_uv.get(k - n).copied() }
    };
    // The spacing wanted at a node: at a boundary node the mean of its two boundary segments.
    let spacing = |k: u32| -> f64 {
        let k = k as usize;
        let Some(r) = loops.iter().find(|r| r.contains(&k)) else { return f64::INFINITY };
        let m = r.len();
        let i = k - r.start;
        let (prev, next) = (r.start + (i + m - 1) % m, r.start + (i + 1) % m);
        match (boundary.get(prev), boundary.get(k), boundary.get(next)) {
            (Some(&p), Some(&q), Some(&x)) => 0.5 * (dist(p, q) + dist(q, x)),
            _ => f64::INFINITY,
        }
    };
    // Boundary sides, so a cut that is one of them is left out.
    let mut sides: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for r in loops {
        let m = r.len();
        for k in 0..m {
            let (a, b) = (u32::try_from(r.start + k).ok()?, u32::try_from(r.start + (k + 1) % m).ok()?);
            sides.insert((a.min(b), a.max(b)));
        }
    }
    let mut cuts: Vec<(u32, u32)> = Vec::new();
    let mut taken: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for (i, &(s0, s1)) in ends.iter().enumerate() {
        if s0 == s1 {
            continue;
        }
        let p0 = at_in(&cut_uv, s0)?;
        let mut nodes: Vec<(f64, u32)> = vec![(0.0, s0)];
        for &k in along.get(i)? {
            nodes.push((dist(p0, at_in(&cut_uv, k)?), k));
        }
        let total = dist(p0, at_in(&cut_uv, s1)?);
        nodes.push((total, s1));
        nodes.sort_by(|a, b| a.0.total_cmp(&b.0));
        nodes.dedup_by_key(|x| x.1);
        let count = segments.get(i).and_then(|s| s.count);
        for w in nodes.windows(2) {
            let (Some(&(_, a)), Some(&(_, b))) = (w.first(), w.get(1)) else { continue };
            if a == b {
                continue;
            }
            // The same cut twice (two segments between the same nodes) is one cut.
            let key = (a.min(b), a.max(b));
            if sides.contains(&key) || !taken.insert(key) {
                continue;
            }
            let (pa, pb) = (at_in(&cut_uv, a)?, at_in(&cut_uv, b)?);
            let fractions: Vec<f64> = match count {
                Some(k) if total > 0.0 => {
                    let share = dist(pa, pb) / total;
                    let k = ((k as f64 * share).round() as usize).clamp(1, 4096);
                    (1..k).map(|j| j as f64 / k as f64).collect()
                }
                _ => seeds(pa, pb, spacing(a), spacing(b)),
            };
            let mut prev = a;
            for f in fractions.into_iter().take(4096) {
                let k = u32::try_from(n + cut_uv.len()).ok()?;
                cut_uv.push([pa[0] + (pb[0] - pa[0]) * f, pa[1] + (pb[1] - pa[1]) * f]);
                cuts.push((prev, k));
                prev = k;
            }
            cuts.push((prev, b));
        }
    }
    if cuts.is_empty() {
        return None;
    }
    let at = |k: u32| at_in(&cut_uv, k);
    let pos = |k: u32| at(k).map(metric);
    // Half-edges: boundary sides with the face on their left, cut sides both ways.
    let inside = |q: [f64; 2]| {
        let mut odd = false;
        for r in loops {
            let m = r.len();
            for k in 0..m {
                let (Some(a), Some(b)) = (boundary.get(r.start + k).map(|p| metric(*p)), boundary.get(r.start + (k + 1) % m).map(|p| metric(*p)))
                else {
                    continue;
                };
                if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    odd = !odd;
                }
            }
        }
        odd
    };
    let mut half: Vec<(u32, u32)> = Vec::new();
    for r in loops {
        let m = r.len();
        let side = |k: usize| -> Option<(u32, u32)> { Some((u32::try_from(r.start + k).ok()?, u32::try_from(r.start + (k + 1) % m).ok()?)) };
        // Which way round the face lies to the left: test beside the loop's longest side.
        let longest = (0..m).max_by(|&x, &y| {
            let l = |k: usize| side(k).and_then(|(a, b)| Some(dist(at(a)?, at(b)?))).unwrap_or(0.0);
            l(x).total_cmp(&l(y))
        })?;
        let (a, b) = side(longest)?;
        let (pa, pb) = (pos(a)?, pos(b)?);
        let len = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
        if !(len > 0.0) {
            return None;
        }
        let left = [0.5 * (pa[0] + pb[0]) - (pb[1] - pa[1]) * 1e-4, 0.5 * (pa[1] + pb[1]) + (pb[0] - pa[0]) * 1e-4];
        let forward = inside(left);
        for k in 0..m {
            let (a, b) = side(k)?;
            if a == b {
                continue;
            }
            half.push(if forward { (a, b) } else { (b, a) });
        }
    }
    for &(a, b) in &cuts {
        half.push((a, b));
        half.push((b, a));
    }
    // Outgoing half-edges per node, by angle.
    let angle = |a: u32, b: u32| -> Option<f64> {
        let (p, q) = (pos(a)?, pos(b)?);
        Some((q[1] - p[1]).atan2(q[0] - p[0]))
    };
    let mut out: HashMap<u32, Vec<(f64, usize)>> = HashMap::new();
    for (h, &(a, b)) in half.iter().enumerate() {
        out.entry(a).or_default().push((angle(a, b)?, h));
    }
    for v in out.values_mut() {
        v.sort_by(|x, y| x.0.total_cmp(&y.0));
    }
    // The face on the left of u→v continues at v with the first outgoing side clockwise from
    // v→u.
    let next = |h: usize| -> Option<usize> {
        let &(u, v) = half.get(h)?;
        let back = angle(v, u)?;
        let list = out.get(&v)?;
        let before = list
            .iter()
            .rev()
            .find(|(a, e)| *a < back - 1e-12 && half.get(*e).is_some_and(|x| x.1 != u))
            .or_else(|| list.iter().rev().find(|(_, e)| half.get(*e).is_some_and(|x| x.1 != u)));
        before.map(|(_, e)| *e).or_else(|| list.iter().find(|(_, e)| half.get(*e).is_some_and(|x| x.1 == u)).map(|(_, e)| *e))
    };
    let mut seen = vec![false; half.len()];
    let mut rings: Vec<Ring<u32>> = Vec::new();
    for h0 in 0..half.len() {
        if *seen.get(h0)? {
            continue;
        }
        let mut ring = Vec::new();
        let mut h = h0;
        loop {
            *seen.get_mut(h)? = true;
            ring.push(half.get(h)?.0);
            h = next(h)?;
            if h == h0 {
                break;
            }
            if ring.len() > half.len() || *seen.get(h)? {
                return None;
            }
        }
        rings.push(Ring::new(ring));
    }
    let area = |ring: &Ring<u32>| -> Option<f64> { Some(ring.iter().map(|&k| pos(k)).collect::<Option<Ring<_>>>()?.signed_area()) };
    let mut pieces: Vec<Piece> = Vec::new();
    let mut holes: Vec<Ring<u32>> = Vec::new();
    for r in rings {
        if area(&r)? > 0.0 {
            pieces.push(Piece { ring: r, holes: Vec::new() });
        } else {
            holes.push(r);
        }
    }
    // A loop no cut reached is a hole of the patch around it.
    for h in holes {
        let p = pos(*h.first()?)?;
        let contains = |ring: &Ring<u32>| {
            let mut odd = false;
            for (&a, &b) in ring.edges() {
                let (Some(a), Some(b)) = (pos(a), pos(b)) else { continue };
                if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    odd = !odd;
                }
            }
            odd
        };
        let host = pieces.iter_mut().filter(|pc| contains(&pc.ring)).min_by(|a, b| {
            let s = |pc: &Piece| area(&pc.ring).unwrap_or(f64::MAX);
            s(a).total_cmp(&s(b))
        })?;
        host.holes.push(h);
    }
    Some((cut_uv, pieces))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(all: &[[f64; 2]], ring: &Ring<u32>) -> f64 {
        ring.iter().map(|&k| all[k as usize]).collect::<Ring<_>>().signed_area()
    }

    /// A square cut by four segments meeting at its centre (one point inside the face, shared)
    /// gives four quarters covering it.
    #[test]
    fn a_cross_cuts_a_square_into_four() {
        let b = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [0.0, 2.0], [0.0, 1.0]];
        let loops = [std::ops::Range { start: 0, end: 8 }];
        let c = End::Point([1.0, 1.0]);
        let segs = [
            Segment { a: End::Node(1), b: c, count: None },
            Segment { a: End::Node(3), b: c, count: None },
            Segment { a: End::Node(5), b: c, count: None },
            Segment { a: End::Node(7), b: c, count: None },
        ];
        let Some((cut_uv, pieces)) = cut(&b, &loops, &segs, [1.0, 1.0], &|_, _, _, _| vec![0.5]) else { panic!("cuts") };
        assert_eq!(pieces.len(), 4);
        let mut all = b.clone();
        all.extend(cut_uv);
        let total: f64 = pieces.iter().map(|p| area(&all, &p.ring)).sum();
        assert!((total - 4.0).abs() < 1e-12, "{total}");
    }

    /// A segment ending on another (a T-node) splits it there: three patches.
    #[test]
    fn a_t_node_splits_the_segment_it_lands_on() {
        let b = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [2.0, 2.0], [0.0, 2.0], [0.0, 1.0]];
        let loops = [std::ops::Range { start: 0, end: 7 }];
        let segs = [Segment { a: End::Node(6), b: End::Node(3), count: None }, Segment { a: End::Node(1), b: End::On(0, [1.0, 1.0]), count: None }];
        let Some((cut_uv, pieces)) = cut(&b, &loops, &segs, [1.0, 1.0], &|_, _, _, _| Vec::new()) else { panic!("cuts") };
        assert_eq!(pieces.len(), 3, "{pieces:?}");
        let mut all = b.clone();
        all.extend(cut_uv);
        let total: f64 = pieces.iter().map(|p| area(&all, &p.ring)).sum();
        assert!((total - 4.0).abs() < 1e-12, "{total}");
    }

    #[test]
    fn no_segments_no_cut() {
        let b = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(cut(&b, &[std::ops::Range { start: 0, end: 4 }], &[], [1.0, 1.0], &|_, _, _, _| Vec::new()).is_none());
    }
}
