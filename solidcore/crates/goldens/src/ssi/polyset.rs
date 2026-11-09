//! Distances from points to a set of polylines and the topology helpers of the SSI corpus
//! scorer.
//!
//! Ported from jyviko's ssi-corpus `scoring/score.py` (`PolySet`, `diameter`, `seg_seg`,
//! `polyline_distance`, `components`, `free_ends`, `signature`),
//! <https://github.com/jyviko/ssi-corpus> at commit `b04c8a6`, licensed MIT OR Apache-2.0.
//!
//! Deviations: the segment hash is a `HashMap` of cell -> segment indices (only looked up,
//! never iterated, so results do not depend on its order); the per-cell sets become lists.

use std::collections::HashMap;

use super::geom::{dist, dot, linspace, sub, P3};

/// `PolySet`: segment hash, exact below the cell size, approximate (nearest decimated
/// vertex) above it.
#[derive(Debug, Clone)]
pub struct PolySet {
    a: Vec<P3>,
    b: Vec<P3>,
    cell: f64,
    table: HashMap<[i64; 3], Vec<usize>>,
    coarse: Vec<P3>,
}

/// numpy `percentile(x, q)` with linear interpolation (`x` non-empty).
fn percentile(x: &[f64], q: f64) -> f64 {
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n == 0 {
        return f64::NAN;
    }
    // numpy `_compute_virtual_index(n, q, 1, 1)`
    let qq = q / 100.0;
    // alpha = beta = 1: n*q + (alpha + q*(1 - alpha - beta)) - 1
    let (alpha, beta) = (1.0, 1.0);
    let idx = (n as f64 * qq + (alpha + qq * (1.0 - alpha - beta))) - 1.0;
    let lo = (idx.floor().max(0.0) as usize).min(n - 1);
    let hi = (lo + 1).min(n - 1);
    let frac = idx - lo as f64;
    let (Some(&vlo), Some(&vhi)) = (s.get(lo), s.get(hi)) else {
        return f64::NAN;
    };
    // numpy `_lerp`: a + (b - a) * t, and b - (b - a) * (1 - t) for t >= 0.5
    let diff = vhi - vlo;
    if frac >= 0.5 {
        vhi - diff * (1.0 - frac)
    } else {
        vlo + diff * frac
    }
}

/// numpy `floor(p / cell).astype(int64)` on x86-64: out-of-range and NaN values become
/// `i64::MIN` (`cvttsd2si`), not a saturated value.
fn key(p: P3, cell: f64) -> [i64; 3] {
    let k = |x: f64| {
        let f = (x / cell).floor();
        if (-9.223372036854775808e18..9.223372036854775808e18).contains(&f) {
            f as i64
        } else {
            i64::MIN
        }
    };
    [k(p[0]), k(p[1]), k(p[2])]
}

/// Python `max(a, b)`: `b` only when `b > a`.
pub fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// Python `min(a, b)`: `b` only when `b < a`.
pub fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// numpy `max` of an array (NaN propagates; `-inf` for an empty array, never used so).
pub fn np_max(v: &[f64]) -> f64 {
    let mut m = f64::NEG_INFINITY;
    for &x in v {
        if x.is_nan() {
            return f64::NAN;
        }
        if x > m {
            m = x;
        }
    }
    m
}

/// numpy `min` of an array (NaN propagates).
pub fn np_min(v: &[f64]) -> f64 {
    let mut m = f64::INFINITY;
    for &x in v {
        if x.is_nan() {
            return f64::NAN;
        }
        if x < m {
            m = x;
        }
    }
    m
}

impl PolySet {
    /// `PolySet(polylines, points)` with the automatic cell size.
    pub fn new(polylines: &[&[P3]], points: &[P3]) -> Self {
        let mut a = Vec::new();
        let mut b = Vec::new();
        for p in polylines {
            if let [only] = p {
                a.push(*only);
                b.push(*only);
                continue;
            }
            for w in p.windows(2) {
                if let [x, y] = w {
                    a.push(*x);
                    b.push(*y);
                }
            }
        }
        for p in points {
            a.push(*p);
            b.push(*p);
        }
        let mut s = Self {
            a,
            b,
            cell: 0.0,
            table: HashMap::new(),
            coarse: Vec::new(),
        };
        if s.a.is_empty() {
            return s;
        }
        let l: Vec<f64> = s.a.iter().zip(&s.b).map(|(x, y)| dist(*y, *x)).collect();
        let lmax = np_max(&l);
        let mut cell = py_max(percentile(&l, 95.0) * 2.0, 1e-300);
        cell = py_max(cell, if lmax > 0.0 { lmax / 50.0 } else { cell });
        s.cell = cell;
        for (i, (&pa, &pb)) in s.a.iter().zip(&s.b).enumerate() {
            let li = l.get(i).copied().unwrap_or(0.0);
            let n = ((li / cell).ceil() as usize).max(1) * 2;
            let d = sub(pb, pa);
            for t in linspace(0.0, 1.0, n + 1) {
                let q = [pa[0] + t * d[0], pa[1] + t * d[1], pa[2] + t * d[2]];
                let cell_list = s.table.entry(key(q, cell)).or_default();
                if cell_list.last() != Some(&i) {
                    cell_list.push(i);
                }
            }
        }
        let step = (s.a.len() / 4000).max(1);
        s.coarse = s.a.iter().step_by(step).copied().collect();
        s
    }

    /// Whether the set has no segment.
    pub fn is_empty(&self) -> bool {
        self.a.is_empty()
    }

    /// `seg_dist` of `q` to segment `i`.
    fn seg_dist(&self, q: P3, i: usize) -> f64 {
        let (Some(&a), Some(&b)) = (self.a.get(i), self.b.get(i)) else {
            return f64::INFINITY;
        };
        let d = sub(b, a);
        let dd = dot(d, d);
        let mut t = if dd > 0.0 {
            dot(sub(q, a), d) / dd
        } else {
            0.0
        };
        t = t.clamp(0.0, 1.0);
        let c = [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]];
        dist(q, c)
    }

    /// `query`: the distance of each point to the set (`inf` when empty).
    pub fn query(&self, pts: &[P3]) -> Vec<f64> {
        pts.iter().map(|&q| self.query1(q)).collect()
    }

    /// `query` of one point.
    pub fn query1(&self, q: P3) -> f64 {
        if self.is_empty() {
            return f64::INFINITY;
        }
        let k = key(q, self.cell);
        let mut cand: Vec<f64> = Vec::new();
        for i in -1..=1 {
            for j in -1..=1 {
                for l in -1..=1 {
                    if let Some(c) = self.table.get(&[
                        k[0].wrapping_add(i),
                        k[1].wrapping_add(j),
                        k[2].wrapping_add(l),
                    ]) {
                        cand.extend(c.iter().map(|&s| self.seg_dist(q, s)));
                    }
                }
            }
        }
        let mut best = if cand.is_empty() {
            f64::INFINITY
        } else {
            np_min(&cand)
        };
        if best > self.cell {
            let m = np_min(&self.coarse.iter().map(|&c| dist(c, q)).collect::<Vec<_>>());
            best = py_min(best, m);
        }
        best
    }
}

/// `diameter`: the largest coordinate extent.
pub fn diameter(p: &[P3]) -> f64 {
    if p.is_empty() {
        return 0.0;
    }
    let mut ext = [0.0f64; 3];
    for (k, e) in ext.iter_mut().enumerate() {
        let lo = p
            .iter()
            .filter_map(|x| x.get(k).copied())
            .fold(f64::INFINITY, f64::min);
        let hi = p
            .iter()
            .filter_map(|x| x.get(k).copied())
            .fold(f64::NEG_INFINITY, f64::max);
        *e = hi - lo;
    }
    np_max(&ext)
}

/// `seg_seg`: the exact distance between segments `p1 q1` and `p2 q2`.
fn seg_seg(p1: P3, q1: P3, p2: P3, q2: P3) -> f64 {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);
    let (s, t);
    if a <= 1e-300 && e <= 1e-300 {
        return dot(r, r).sqrt();
    }
    if a <= 1e-300 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= 1e-300 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let s0 = if den > 1e-300 {
                ((b * f - c * e) / den).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            } else {
                t = t0;
                s = s0;
            }
        }
    }
    let x = [p1[0] + d1[0] * s, p1[1] + d1[1] * s, p1[2] + d1[2] * s];
    let y = [p2[0] + d2[0] * t, p2[1] + d2[1] * t, p2[2] + d2[2] * t];
    dist(x, y)
}

fn seg_lengths_max(p: &[P3]) -> f64 {
    // numpy `norm(diff(P)).max(initial=0)`
    p.windows(2)
        .map(|w| match w {
            [x, y] => dist(*y, *x),
            _ => 0.0,
        })
        .fold(0.0, |m, x| if x > m || x.is_nan() { x } else { m })
}

/// `polyline_distance`: the minimum distance between polylines `a` and `b` (`set_b` is
/// `b`'s [`PolySet`]): vertex query first, exact segment-segment distance around the
/// closest approach when it matters.
pub fn polyline_distance(a: &[P3], b: &[P3], set_b: &PolySet, tol: f64) -> f64 {
    let dv = set_b.query(a);
    // numpy argmin: first minimum (a NaN wins)
    let mut k = 0usize;
    let mut d = f64::INFINITY;
    for (i, &x) in dv.iter().enumerate() {
        if x.is_nan() {
            k = i;
            d = x;
            break;
        }
        if i == 0 || x < d {
            k = i;
            d = x;
        }
    }
    let _ = k;
    if d <= tol {
        return d;
    }
    let sa = seg_lengths_max(a);
    let sb = seg_lengths_max(b);
    if d > tol + py_max(sa, sb) {
        return d;
    }
    let mut best = d;
    let lim = tol + sa + sb;
    for (i, &di) in dv.iter().enumerate() {
        if di
            .partial_cmp(&lim)
            .is_none_or(|o| o == core::cmp::Ordering::Greater)
        {
            continue;
        }
        for ia in [i.checked_sub(1), Some(i)].into_iter().flatten() {
            let (Some(&a0), Some(&a1)) = (a.get(ia), a.get(ia + 1)) else {
                continue;
            };
            // argmin over the vertices of b (first minimum)
            let mut jb = 0usize;
            let mut dbest = f64::INFINITY;
            for (j, &q) in b.iter().enumerate() {
                let dj = dist(q, a0);
                if j == 0 || dj < dbest {
                    jb = j;
                    dbest = dj;
                }
            }
            let lo = jb.saturating_sub(3);
            let hi = (jb + 3).min(b.len().saturating_sub(1));
            for j in lo..hi {
                if let (Some(&b0), Some(&b1)) = (b.get(j), b.get(j + 1)) {
                    best = py_min(best, seg_seg(a0, a1, b0, b1));
                }
            }
        }
    }
    best
}

/// An item of a connected component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// A curve (polyline index).
    Curve(usize),
    /// A point (index).
    Point(usize),
}

/// Union-find root of `x`, halving the path.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    loop {
        let Some(&px) = parent.get(x) else {
            return x;
        };
        if px == x {
            return x;
        }
        let ppx = parent.get(px).copied().unwrap_or(px);
        if let Some(slot) = parent.get_mut(x) {
            *slot = ppx;
        }
        x = ppx;
    }
}

/// `components`: connected components of curves and points within `join_tol`.
pub fn components(curves: &[Vec<P3>], points: &[P3], join_tol: f64) -> Vec<Vec<Item>> {
    let items: Vec<Item> = (0..curves.len())
        .map(Item::Curve)
        .chain((0..points.len()).map(Item::Point))
        .collect();
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let sets: Vec<PolySet> = curves.iter().map(|c| PolySet::new(&[c], &[])).collect();
    for (i, &ii) in items.iter().enumerate() {
        for (j, &ij) in items.iter().enumerate().skip(i + 1) {
            let d = match (ii, ij) {
                (Item::Point(ki), Item::Point(kj)) => match (points.get(ki), points.get(kj)) {
                    (Some(&p), Some(&q)) => dist(p, q),
                    _ => f64::INFINITY,
                },
                (Item::Curve(ki), Item::Curve(kj)) => {
                    match (curves.get(ki), curves.get(kj), sets.get(kj)) {
                        (Some(ci), Some(cj), Some(sj)) => polyline_distance(ci, cj, sj, join_tol),
                        _ => f64::INFINITY,
                    }
                }
                (Item::Curve(c), Item::Point(p)) | (Item::Point(p), Item::Curve(c)) => {
                    match (sets.get(c), points.get(p)) {
                        (Some(s), Some(&q)) => s.query1(q),
                        _ => f64::INFINITY,
                    }
                }
            };
            if d <= join_tol {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if let Some(slot) = parent.get_mut(ri) {
                    *slot = rj;
                }
            }
        }
    }
    // groups in first-seen order of their root (Python dict order)
    let mut roots: Vec<usize> = Vec::new();
    let mut groups: Vec<Vec<Item>> = Vec::new();
    for (i, &it) in items.iter().enumerate() {
        let r = find(&mut parent, i);
        if let Some(g) = roots.iter().position(|&x| x == r) {
            if let Some(grp) = groups.get_mut(g) {
                grp.push(it);
            }
        } else {
            roots.push(r);
            groups.push(vec![it]);
        }
    }
    groups
}

/// `free_ends`: the number of free endpoints of each curve.
pub fn free_ends(curves: &[Vec<P3>], join_tol: f64) -> Vec<usize> {
    let sets: Vec<PolySet> = curves.iter().map(|c| PolySet::new(&[c], &[])).collect();
    let mut res = Vec::with_capacity(curves.len());
    for (i, c) in curves.iter().enumerate() {
        let (Some(&first), Some(&last)) = (c.first(), c.last()) else {
            res.push(0);
            continue;
        };
        if dist(first, last) <= join_tol && diameter(c) > 2.0 * join_tol {
            res.push(0);
            continue;
        }
        // cumulative arc length
        let mut s = Vec::with_capacity(c.len());
        let mut acc = 0.0;
        s.push(0.0);
        for w in c.windows(2) {
            if let [x, y] = w {
                acc += dist(*y, *x);
                s.push(acc);
            }
        }
        let total = s.last().copied().unwrap_or(0.0);
        let mut n = 0;
        for (end, si) in [(first, 0.0), (last, total)] {
            let mut glued = false;
            for (j, sj) in sets.iter().enumerate() {
                if j != i && sj.query1(end) <= join_tol {
                    glued = true;
                    break;
                }
            }
            if !glued {
                let far: Vec<f64> = c
                    .iter()
                    .zip(&s)
                    .filter(|(_, &sk)| (sk - si).abs() > 3.0 * join_tol)
                    .map(|(&p, _)| dist(p, end))
                    .collect();
                if !far.is_empty() && np_min(&far) <= join_tol {
                    glued = true;
                }
            }
            if !glued {
                n += 1;
            }
        }
        res.push(n);
    }
    res
}

/// `signature`: the sorted component kinds (`point`, `curve(free_ends=N)`).
pub fn signature(curves: &[Vec<P3>], points: &[P3], join_tol: f64) -> Vec<String> {
    let comps = components(curves, points, join_tol);
    let fe = if curves.is_empty() {
        Vec::new()
    } else {
        free_ends(curves, join_tol)
    };
    let mut sig: Vec<String> = comps
        .iter()
        .map(|comp| {
            let cs: Vec<usize> = comp
                .iter()
                .filter_map(|it| match it {
                    Item::Curve(k) => Some(*k),
                    Item::Point(_) => None,
                })
                .collect();
            if cs.is_empty() {
                "point".to_string()
            } else {
                let n: usize = cs.iter().filter_map(|k| fe.get(*k)).sum();
                format!("curve(free_ends={n})")
            }
        })
        .collect();
    sig.sort();
    sig
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(n: usize, r: f64, z: f64) -> Vec<P3> {
        (0..n)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / (n - 1) as f64;
                [r * t.cos(), r * t.sin(), z]
            })
            .collect()
    }

    #[test]
    fn polyset_distances() {
        let line = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let s = PolySet::new(&[&line], &[]);
        let d = s.query(&[[0.5, 0.25, 0.0], [2.0, 0.0, 0.0]]);
        assert!((d[0] - 0.25).abs() < 1e-15);
        assert!((d[1] - 1.0).abs() < 1e-15);
        assert_eq!(PolySet::new(&[], &[]).query1([0.0; 3]), f64::INFINITY);
    }

    #[test]
    fn signatures() {
        let c = circle(200, 1.0, 0.0);
        assert_eq!(
            signature(std::slice::from_ref(&c), &[], 1e-3),
            vec!["curve(free_ends=0)"]
        );
        let half: Vec<P3> = c.iter().take(100).copied().collect();
        assert_eq!(
            signature(&[half], &[[5.0, 0.0, 0.0]], 1e-3),
            vec!["curve(free_ends=2)", "point"]
        );
        let a = circle(200, 1.0, 0.0);
        let b = circle(200, 1.0, 0.5);
        assert_eq!(signature(&[a, b], &[], 1e-3).len(), 2);
    }

    #[test]
    fn numpy_percentile() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), 2.5);
        assert!((percentile(&[0.0, 10.0], 95.0) - 9.5).abs() < 1e-15);
    }
}
