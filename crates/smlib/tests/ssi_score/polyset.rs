//! Point-to-polyline distances and the topology signature: a port of `PolySet`,
//! `polyline_distance`, `components`, `free_ends` and `signature` of the corpus's
//! `scoring/score.py`, with the same segment hash (exact below the cell size, the nearest
//! decimated vertex above it), so the numbers the verdicts depend on are the same.

use std::collections::HashMap;

use super::geom::{V3, linspace, norm, py_max, py_min, sub};

/// numpy's `np.percentile(xs, q)` (linear interpolation).
fn percentile(xs: &[f64], q: f64) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    let Some(&last) = s.last() else { return f64::NAN };
    let vi = (s.len() - 1) as f64 * (q / 100.0);
    let lo = vi.floor();
    let i = lo as usize;
    let a = s.get(i).copied().unwrap_or(last);
    let b = s.get(i + 1).copied().unwrap_or(last);
    let t = vi - lo;
    let d = b - a;
    if t >= 0.5 { b - d * (1.0 - t) } else { a + d * t }
}

fn key(p: V3, cell: f64) -> [i64; 3] {
    [(p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64]
}

/// A set of polylines and points, hashed by segment for nearest-distance queries.
pub struct PolySet {
    a: Vec<V3>,
    b: Vec<V3>,
    cell: f64,
    table: HashMap<[i64; 3], Vec<u32>>,
    coarse: Vec<V3>,
}

impl PolySet {
    pub fn new(polylines: &[&[V3]], points: &[V3]) -> PolySet {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for p in polylines {
            if p.len() == 1 {
                a.push(p[0]);
                b.push(p[0]);
            }
            for w in p.windows(2) {
                a.push(w[0]);
                b.push(w[1]);
            }
        }
        for &p in points {
            a.push(p);
            b.push(p);
        }
        let mut set = PolySet { a, b, cell: 0.0, table: HashMap::new(), coarse: Vec::new() };
        if set.a.is_empty() {
            return set;
        }
        let lens: Vec<f64> = set.a.iter().zip(&set.b).map(|(&a, &b)| norm(sub(b, a))).collect();
        let lmax = lens.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut cell = percentile(&lens, 95.0) * 2.0;
        if 1e-300 > cell {
            cell = 1e-300;
        }
        let alt = if lmax > 0.0 { lmax / 50.0 } else { cell };
        if alt > cell {
            cell = alt;
        }
        set.cell = cell;
        for (i, (&a, &b)) in set.a.iter().zip(&set.b).enumerate() {
            let n = ((lens[i] / cell).ceil() as usize * 2).max(1);
            let d = sub(b, a);
            for t in linspace(0.0, 1.0, n + 1) {
                let k = key([a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]], cell);
                let e = set.table.entry(k).or_default();
                if e.last() != Some(&(i as u32)) {
                    e.push(i as u32);
                }
            }
        }
        let step = (set.a.len() / 4000).max(1);
        set.coarse = set.a.iter().step_by(step).copied().collect();
        set
    }

    pub fn is_empty(&self) -> bool {
        self.a.is_empty()
    }

    fn seg_dist(&self, q: V3, i: usize) -> f64 {
        let (a, b) = (self.a[i], self.b[i]);
        let d = sub(b, a);
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let qa = sub(q, a);
        let t = if dd > 0.0 { (qa[0] * d[0] + qa[1] * d[1] + qa[2] * d[2]) / dd } else { 0.0 };
        let t = super::geom::clip(t, 0.0, 1.0);
        norm(sub(q, [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]]))
    }

    /// Distance from one point to the set (infinite for an empty set).
    pub fn query1(&self, q: V3) -> f64 {
        if self.is_empty() {
            return f64::INFINITY;
        }
        let k = key(q, self.cell);
        let mut best = f64::INFINITY;
        let mut any = false;
        for i in -1..=1 {
            for j in -1..=1 {
                for l in -1..=1 {
                    if let Some(c) = self.table.get(&[k[0].wrapping_add(i), k[1].wrapping_add(j), k[2].wrapping_add(l)]) {
                        for &s in c {
                            let d = self.seg_dist(q, s as usize);
                            if !any || d < best {
                                best = d;
                            }
                            any = true;
                        }
                    }
                }
            }
        }
        if best > self.cell {
            let m = self.coarse.iter().map(|&c| norm(sub(c, q))).fold(f64::INFINITY, f64::min);
            best = py_min(best, m);
        }
        best
    }

    pub fn query(&self, qs: &[V3]) -> Vec<f64> {
        qs.iter().map(|&q| self.query1(q)).collect()
    }
}

/// Every `step`-th point (`P[::step]`).
pub fn stride(p: &[V3], step: usize) -> Vec<V3> {
    p.iter().step_by(step.max(1)).copied().collect()
}

/// The largest extent along x, y or z (`np.ptp(P, axis=0).max()`).
pub fn diameter(p: &[V3]) -> f64 {
    if p.is_empty() {
        return 0.0;
    }
    (0..3)
        .map(|k| {
            let lo = p.iter().map(|q| q[k]).fold(f64::INFINITY, f64::min);
            let hi = p.iter().map(|q| q[k]).fold(f64::NEG_INFINITY, f64::max);
            hi - lo
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Exact distance between segments p1q1 and p2q2.
pub fn seg_seg(p1: V3, q1: V3, p2: V3, q2: V3) -> f64 {
    let dt = |x: V3, y: V3| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
    let (a, e, f) = (dt(d1, d1), dt(d2, d2), dt(d2, r));
    if a <= 1e-300 && e <= 1e-300 {
        return norm(r);
    }
    let cl = |x: f64| py_min(py_max(x, 0.0), 1.0);
    let (s, t);
    if a <= 1e-300 {
        s = 0.0;
        t = cl(f / e);
    } else {
        let c = dt(d1, r);
        if e <= 1e-300 {
            t = 0.0;
            s = cl(-c / a);
        } else {
            let b = dt(d1, d2);
            let den = a * e - b * b;
            let s0 = if den > 1e-300 { cl((b * f - c * e) / den) } else { 0.0 };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = cl(-c / a);
            } else if t0 > 1.0 {
                t = 1.0;
                s = cl((b - c) / a);
            } else {
                t = t0;
                s = s0;
            }
        }
    }
    norm(sub([p1[0] + d1[0] * s, p1[1] + d1[1] * s, p1[2] + d1[2] * s], [p2[0] + d2[0] * t, p2[1] + d2[1] * t, p2[2] + d2[2] * t]))
}

fn max_seg(p: &[V3]) -> f64 {
    p.windows(2).map(|w| norm(sub(w[1], w[0]))).fold(0.0, |m, x| if x > m { x } else { m })
}

/// Min distance between polylines A and B: vertex query first, exact segment-segment distance
/// around the closest approach when it matters.
pub fn polyline_distance(a: &[V3], b: &[V3], set_b: &PolySet, tol: f64) -> f64 {
    let dv = set_b.query(a);
    let mut k = 0;
    for (i, &d) in dv.iter().enumerate() {
        if d.is_nan() {
            k = i;
            break;
        }
        if d < dv[k] {
            k = i;
        }
    }
    let Some(&d) = dv.get(k) else { return f64::INFINITY };
    if d <= tol {
        return d;
    }
    let (sa, sb) = (max_seg(a), max_seg(b));
    if d > tol + sa.max(sb) {
        return d;
    }
    let mut best = d;
    let lim = tol + sa + sb;
    // the nearest vertex of B to each vertex of A, computed once (np.argmin: the first minimum)
    let mut nearest: Vec<Option<usize>> = vec![None; a.len()];
    for (i, &di) in dv.iter().enumerate() {
        if !(di <= lim) {
            continue;
        }
        for ia in [i as isize - 1, i as isize] {
            if ia < 0 || ia as usize + 1 >= a.len() {
                continue;
            }
            let ia = ia as usize;
            let jb = *nearest[ia].get_or_insert_with(|| {
                let (mut jb, mut dj) = (0, f64::INFINITY);
                for (j, &q) in b.iter().enumerate() {
                    let x = norm(sub(q, a[ia]));
                    if x.is_nan() {
                        jb = j;
                        break;
                    }
                    if j == 0 || x < dj {
                        dj = x;
                        jb = j;
                    }
                }
                jb
            });
            for j in jb.saturating_sub(3)..(jb + 3).min(b.len().saturating_sub(1)) {
                best = py_min(best, seg_seg(a[ia], a[ia + 1], b[j], b[j + 1]));
            }
        }
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Curve(usize),
    Point(usize),
}

/// Connected components of curves and points joined within `join_tol`.
pub fn components(curves: &[Vec<V3>], points: &[V3], join_tol: f64) -> Vec<Vec<Item>> {
    let items: Vec<Item> = (0..curves.len()).map(Item::Curve).chain((0..points.len()).map(Item::Point)).collect();
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    let sets: Vec<PolySet> = curves.iter().map(|c| PolySet::new(&[c], &[])).collect();
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            let d = match (items[i], items[j]) {
                (Item::Point(a), Item::Point(b)) => norm(sub(points[a], points[b])),
                (Item::Curve(a), Item::Curve(b)) => polyline_distance(&curves[a], &curves[b], &sets[b], join_tol),
                (Item::Curve(c), Item::Point(p)) | (Item::Point(p), Item::Curve(c)) => sets[c].query1(points[p]),
            };
            if d <= join_tol {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<Item>)> = Vec::new();
    for (i, &it) in items.iter().enumerate() {
        let r = find(&mut parent, i);
        match groups.iter_mut().find(|(g, _)| *g == r) {
            Some((_, v)) => v.push(it),
            None => groups.push((r, vec![it])),
        }
    }
    groups.into_iter().map(|(_, v)| v).collect()
}

/// Number of free endpoints of each curve.
pub fn free_ends(curves: &[Vec<V3>], join_tol: f64) -> Vec<usize> {
    let sets: Vec<PolySet> = curves.iter().map(|c| PolySet::new(&[c], &[])).collect();
    let mut res = Vec::with_capacity(curves.len());
    for (i, c) in curves.iter().enumerate() {
        let (Some(&first), Some(&last)) = (c.first(), c.last()) else {
            res.push(0);
            continue;
        };
        if norm(sub(first, last)) <= join_tol && diameter(c) > 2.0 * join_tol {
            res.push(0);
            continue;
        }
        let mut s = Vec::with_capacity(c.len());
        let mut acc = 0.0;
        s.push(0.0);
        for w in c.windows(2) {
            acc += norm(sub(w[1], w[0]));
            s.push(acc);
        }
        let mut n = 0;
        for (end, si) in [(first, 0.0), (last, acc)] {
            let mut glued = (0..curves.len()).any(|j| j != i && sets[j].query1(end) <= join_tol);
            if !glued {
                let m = c
                    .iter()
                    .zip(&s)
                    .filter(|(_, sk)| (*sk - si).abs() > 3.0 * join_tol)
                    .map(|(&p, _)| norm(sub(p, end)))
                    .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))));
                glued = m.is_some_and(|m| m <= join_tol);
            }
            if !glued {
                n += 1;
            }
        }
        res.push(n);
    }
    res
}

/// The sorted component signature: `point` or `curve(free_ends=N)` per component.
pub fn signature(curves: &[Vec<V3>], points: &[V3], join_tol: f64) -> Vec<String> {
    let comps = components(curves, points, join_tol);
    let fe = if curves.is_empty() { Vec::new() } else { free_ends(curves, join_tol) };
    let mut sig: Vec<String> = comps
        .iter()
        .map(|comp| {
            let cs: Vec<usize> = comp.iter().filter_map(|it| if let Item::Curve(k) = it { Some(*k) } else { None }).collect();
            if cs.is_empty() { "point".to_string() } else { format!("curve(free_ends={})", cs.iter().map(|&k| fe[k]).sum::<usize>()) }
        })
        .collect();
    sig.sort();
    sig
}
