//! The chord tolerance over a face's finished triangulation, whatever layout made it. A grid
//! divides each parameter direction within the tolerance, but on a doubly curved surface the
//! two sags add up in the middle of a cell, along its diagonal; a patch blended from its sides
//! has no lines along the surface's own directions at all. Every triangle is checked against
//! the surface (at its centroid and at the middle of each side inside the face) and the sides
//! that leave it are split in two, both triangles on them together, so the face stays
//! conforming. Sides on the face's boundary are never split: they are the edges' own nodes,
//! which the neighbouring face shares.

use crate::vec::VecExt as _;
use crate::hash::BitsMap;

use solidcore::Vec3;

use super::rot3;
use solidcore::math::det;

pub(crate) struct Refinement {
    pub(crate) capped: bool,
    pub(crate) pos: Vec<Vec3>,
    displaced: Vec<bool>,
}

impl Refinement {
    /// Whether actual samples on triangles affected by a displaced held node miss the chord
    /// tolerance. Only those triangles need probing before deciding to measure the whole mesh.
    pub(crate) fn boundary_has_residual(&self, s: &super::poles::Face, tol: f64, poles: &Poles, fixed: &[Vec3], uv: &[[f64; 2]], tris: &[[u32; 3]]) -> bool {
        if !self.displaced.contains(&true) {
            return false;
        }
        let at = |i: u32| fixed.get(i as usize).copied().or_else(|| point_at(s, *uv.get(i as usize)?));
        tris.iter().filter(|t| t.iter().any(|&i| self.displaced.get(i as usize) == Some(&true))).any(|t| {
            triangle_sag(s, poles, uv, t, &at).into_iter().any(|d| d.is_none_or(|d| d > tol))
        })
    }
}

/// Split triangles whose curvature takes them farther than `tol` from the surface, adding
/// at most `max_points` points. For this comparison use the surface point at each corner's
/// parameters where it evaluates. Held positions still determine side lengths and shape.
pub(crate) fn refine(s: &super::poles::Face, tol: f64, poles: &Poles, fixed: &[Vec3], uv: &mut Vec<[f64; 2]>, tris: &mut Vec<[u32; 3]>, max_points: usize) -> Refinement {
    let at = |q: [f64; 2]| s.derivs(q[0], q[1]).map_or(Vec3::ZERO, |d| d.0);
    if !(tol.is_finite() && tol > 0.0) || tris.is_empty() {
        let pos = uv.iter().enumerate().map(|(i, q)| fixed.get(i).copied().unwrap_or_else(|| at(*q))).collect();
        return Refinement { capped: false, pos, displaced: Vec::new() };
    }
    let mut displaced = Vec::with_capacity(fixed.len());
    let surface_boundary: Vec<Vec3> = uv.iter().zip(fixed).map(|(&q, &held)| {
        // A changed chart or a degenerate node need not evaluate here. Its held position
        // still gives the existing comparison and the mesh is not refused for this probe.
        let point = point_at(s, q).unwrap_or(held);
        displaced.push(point != held);
        point
    }).collect();
    let surface_corner = |i: u32, actual| surface_boundary.get(i as usize).copied().unwrap_or(actual);
    let mean = |qs: &[[f64; 2]]| poles.mean(qs);
    // How far the flat point `c` lies from the surface, measured along the normal at the surface
    // point of the same parameters: those parameters need not be the nearest (a straight line in
    // the parameters is no geodesic near a pole), and only the departure across the surface
    // counts. Where the normal is undefined (a pole), the whole distance.
    let off = |q: [f64; 2], c: Vec3| match s.derivs(q[0], q[1]) {
        Some((p, su, sv)) => su.cross(sv).normalized().map_or_else(|| p.dist(c), |n| (p - c).dot(n).abs()),
        None => Vec3::ZERO.dist(c),
    };
    let mut pos: Vec<Vec3> = uv.iter().enumerate().map(|(i, q)| fixed.get(i).copied().unwrap_or_else(|| at(*q))).collect();
    // No side is split below this length: a surface whose parameters bend sharply (at a pole) is
    // not chased to the last bit.
    let shortest = tol * 1e-3;
    let mut sides: BitsMap<(u32, u32), Vec<u32>> = BitsMap::with_capacity_and_hasher(tris.len() * 2, std::hash::BuildHasherDefault::default());
    for (ti, t) in tris.iter().enumerate() {
        for [a, b, _] in (0..3).map(|k| rot3(*t, k)) {
            sides.entry(key(a, b)).or_default().push(ti as u32);
        }
    }
    let mut sag_of: BitsMap<(u32, u32), f64> = BitsMap::default();
    let limit = uv.len().saturating_add(max_points);
    let mut queue: Vec<u32> = (0..tris.len() as u32).rev().collect();
    while let Some(ti) = queue.pop() {
        if uv.len() >= limit {
            return Refinement { capped: true, pos, displaced };
        }
        let Some(&t) = tris.get(ti as usize) else { continue };
        let p = |i: u32| pos.get(i as usize).copied();
        let q = |i: u32| uv.get(i as usize).copied();
        let (Some(pa), Some(pb), Some(pc), Some(qa), Some(qb), Some(qc)) = (p(t[0]), p(t[1]), p(t[2]), q(t[0]), q(t[1]), q(t[2])) else {
            continue;
        };
        // Sides inside the face (two triangles on them), long enough to split, with their sag.
        let mut inner: Vec<(f64, f64, (u32, u32))> = Vec::with_capacity(3);
        for [a, b, _] in (0..3).map(|k| rot3(t, k)) {
            let e = key(a, b);
            if sides.get(&e).is_none_or(|ts| ts.len() != 2) {
                continue;
            }
            let (Some(xa), Some(xb), Some(ua), Some(ub)) = (p(a), p(b), q(a), q(b)) else { continue };
            let len = xa.dist(xb);
            if len <= shortest {
                continue;
            }
            let sag = *sag_of.entry(e).or_insert_with(|| {
                off(mean(&[ua, ub]), (surface_corner(a, xa) + surface_corner(b, xb)) * 0.5)
            });
            inner.push((sag, len, e));
        }
        // The surface's departure from a flat triangle is about quadratic over it: at the
        // centroid it is 4/9 of the sum of the sags at the sides' middles, which splitting a side
        // inside the face divides by four. A triangle whose middle is off splits its side inside
        // the face that sags most, while that side still contributes; what is left comes from
        // its boundary sides, which the edges' own sampling keeps within the tolerance.
        let worst = inner.iter().max_by(|x, y| x.0.total_cmp(&y.0)).copied();
        let split = match worst {
            Some((sag, _, e)) if sag > tol => e,
            Some((sag, _, e)) if sag > 0.25 * tol => {
                let centroid = mean(&[qa, qb, qc]);
                let [a, b, c] = t;
                let center = (surface_corner(a, pa) + surface_corner(b, pb) + surface_corner(c, pc)) * (1.0 / 3.0);
                if off(centroid, center) > tol {
                    e
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        // Splitting a triangle's shorter side leaves a child with its long side and a wider
        // angle, and where the long side's middle lies on the surface (a section with an
        // inflection) the children flatten toward it without end. So a triangle flattened onto its
        // longest side (its widest angle past 150 degrees) splits that side, and a split never
        // goes across an obtuse triangle's shorter side: that triangle's longest side is split
        // first, and so on along the path (Rivara's longest-edge propagation, its lengths growing,
        // so it ends). Only sides inside the face are split.
        let mut split = flat_long_side(&t, &pos, &sides, shortest, 150.0).unwrap_or(split);
        let mut from = ti;
        while let Some(&across) = sides.get(&split).and_then(|ts| ts.iter().find(|&&o| o != from)) {
            match tris.get(across as usize).and_then(|o| flat_long_side(o, &pos, &sides, shortest, 90.0)) {
                Some(long) if long != split => (split, from) = (long, across),
                _ => break,
            }
        }
        // The triangle is looked at again when the split was another's, on its way.
        if from != ti {
            queue.push(ti);
        }
        let (Some(ua), Some(ub)) = (q(split.0), q(split.1)) else { continue };
        let m = mean(&[ua, ub]);
        let mi = uv.len() as u32;
        uv.push(m);
        pos.push(at(m));
        sag_of.remove(&split);
        queue.extend(split_side(tris, &mut sides, split, mi));
    }
    Refinement { capped: false, pos, displaced }
}

/// Finite surface point at `q`, which may lack a regular normal at a pole.
fn point_at(s: &super::poles::Face, q: [f64; 2]) -> Option<Vec3> {
    s.derivs(q[0], q[1]).map(|(p, _, _)| p).filter(|p| p.is_finite())
}

/// The farthest the finished triangles lie from the surface (mm) at their middles and their
/// sides' middles, along the surface normal there, using the held boundary positions. Also
/// counts how many of those middles could not be measured (a shared side counts twice).
pub(crate) fn worst(s: &super::poles::Face, poles: &Poles, fixed: &[Vec3], uv: &[[f64; 2]], tris: &[[u32; 3]]) -> (f64, usize) {
    let pos: Vec<Option<Vec3>> = uv.iter().enumerate().map(|(i, q)| fixed.get(i).copied().or_else(|| point_at(s, *q))).collect();
    let at = |i: u32| pos.get(i as usize).copied().flatten();
    let (mut worst, mut unmeasured) = (0.0f64, 0usize);
    for t in tris {
        for d in triangle_sag(s, poles, uv, t, &at) {
            match d {
                Some(d) => worst = worst.max(d),
                None => unmeasured += 1,
            }
        }
    }
    (worst, unmeasured)
}

fn triangle_sag(s: &super::poles::Face, poles: &Poles, uv: &[[f64; 2]], t: &[u32; 3], at: &impl Fn(u32) -> Option<Vec3>) -> [Option<f64>; 4] {
    let corner = |i: u32| Some((at(i)?, *uv.get(i as usize)?));
    let [Some((a, qa)), Some((b, qb)), Some((c, qc))] = t.map(corner) else { return [None; 4] };
    let off = |q: [f64; 2], x: Vec3| {
        let (p, su, sv) = s.derivs(q[0], q[1])?;
        let d = su.cross(sv).normalized().map_or_else(|| p.dist(x), |n| (p - x).dot(n).abs());
        d.is_finite().then_some(d)
    };
    [
        off(poles.mean(&[qa, qb, qc]), (a + b + c) * (1.0 / 3.0)),
        off(poles.mean(&[qa, qb]), (a + b) * 0.5),
        off(poles.mean(&[qb, qc]), (b + c) * 0.5),
        off(poles.mean(&[qc, qa]), (c + a) * 0.5),
    ]
}

/// Which parameter directions the surface is collapsed along at each node of a face's
/// triangulation (within `reach`): a property of the node's parameters, so it is asked once per
/// node however many triangles, sides and rounds of refinement and flipping look at it. Ported
/// from #13559, which did the same for solidcore-tess's own planner; the question itself is the
/// kernel's classifier's, asked once per iso-line ([`super::poles::Face::pole`]).
pub(crate) struct Poles<'a> {
    s: &'a super::poles::Face,
    reach: f64,
    /// The answers so far, by the node's parameters bit for bit.
    flags: std::cell::RefCell<BitsMap<[u64; 2], [bool; 2]>>,
    /// Pole questions asked of the surface, for the tests.
    #[cfg(test)]
    pub(crate) asked: std::cell::Cell<usize>,
}

impl<'a> Poles<'a> {
    /// The poles of `s`, a node within `reach` mm of one taken as one.
    pub(crate) fn new(s: &'a super::poles::Face, reach: f64) -> Self {
        Poles { s, reach, flags: std::cell::RefCell::new(BitsMap::default()), #[cfg(test)] asked: std::cell::Cell::new(0) }
    }

    /// Whether each direction is collapsed at the node of parameters `q`.
    fn at(&self, q: [f64; 2]) -> [bool; 2] {
        let key = [q[0].to_bits(), q[1].to_bits()];
        if let Some(f) = self.flags.borrow().get(&key) {
            return *f;
        }
        #[cfg(test)]
        self.asked.set(self.asked.get() + 2);
        let f = [0, 1].map(|d| self.s.pole(q, d, self.reach));
        self.flags.borrow_mut().insert(key, f);
        f
    }

    /// The mean of parameter points, as the surface point between them: at a pole the parameter
    /// along the degenerate direction is free (the pole copy carries a neighbour's), so there the
    /// mean takes it from the other points alone.
    fn mean(&self, qs: &[[f64; 2]]) -> [f64; 2] {
        let mut free_sum = [0.0; 2];
        let mut free_cnt = [0usize; 2];
        let mut all_sum = [0.0; 2];
        for &q in qs {
            let flags = self.at(q);
            for (((all, sum), cnt), (x, pole)) in all_sum.iter_mut().zip(&mut free_sum).zip(&mut free_cnt).zip(q.into_iter().zip(flags)) {
                *all += x;
                if !pole {
                    *sum += x;
                    *cnt += 1;
                }
            }
        }
        let n = qs.len().max(1) as f64;
        [
            if free_cnt[0] > 0 { free_sum[0] / free_cnt[0] as f64 } else { all_sum[0] / n },
            if free_cnt[1] > 0 { free_sum[1] / free_cnt[1] as f64 } else { all_sum[1] / n },
        ]
    }
}

/// The longest side of triangle `t` when the angle opposite it is past `degrees` and the side lies
/// inside the face (two triangles on it) and is long enough to split.
fn flat_long_side(t: &[u32; 3], pos: &[Vec3], sides: &BitsMap<(u32, u32), Vec<u32>>, shortest: f64, degrees: f64) -> Option<(u32, u32)> {
    let p = |i: u32| pos.get(i as usize).copied();
    let len = |i: u32, j: u32| p(i).zip(p(j)).map(|(a, b)| a.dist(b));
    let [t0, t1, t2] = *t;
    let lengths = [len(t0, t1)?, len(t1, t2)?, len(t2, t0)?];
    let k = lengths.iter().enumerate().max_by(|i, j| i.1.total_cmp(j.1))?.0;
    let [l, a, b] = rot3(lengths, k);
    let [ta, tb, _] = rot3(*t, k);
    let side = key(ta, tb);
    let inside = sides.get(&side).is_some_and(|ts| ts.len() == 2) && l > shortest;
    let flat = a > 0.0 && b > 0.0 && (a * a + b * b - l * l) / (2.0 * a * b) < det::cos(degrees.to_radians());
    (inside && flat).then_some(side)
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// Move triangle `from` to `to` in the list of side `(a, b)`.
fn retag(sides: &mut BitsMap<(u32, u32), Vec<u32>>, a: u32, b: u32, from: u32, to: u32) {
    if let Some(ts) = sides.get_mut(&key(a, b)) {
        for t in ts.iter_mut().filter(|t| **t == from) {
            *t = to;
        }
    }
}

/// Split side `e` at the new point `m`: each triangle on it becomes two, in its own winding.
/// The triangles touched, old and new.
fn split_side(tris: &mut Vec<[u32; 3]>, sides: &mut BitsMap<(u32, u32), Vec<u32>>, e: (u32, u32), m: u32) -> Vec<u32> {
    let on = sides.remove(&e).unwrap_or_default();
    let mut touched = Vec::with_capacity(4);
    for ti in on {
        let Some(&t) = tris.get(ti as usize) else { continue };
        // Turn the triangle so the side comes first, as it winds: [a, b, c].
        let Some([a, b, c]) = (0..3).map(|k| rot3(t, k)).find(|[a, b, _]| key(*a, *b) == e) else { continue };
        let new = tris.len() as u32;
        if let Some(slot) = tris.get_mut(ti as usize) {
            *slot = [a, m, c];
        }
        tris.push([m, b, c]);
        retag(sides, b, c, ti, new);
        sides.entry(key(a, m)).or_default().push(ti);
        sides.entry(key(m, b)).or_default().push(new);
        let mc = sides.entry(key(m, c)).or_default();
        mc.push(ti);
        mc.push(new);
        touched.extend([ti, new]);
    }
    touched
}

/// The sharpest corner (radians) a flip may make.
const MIN_FLIP_ANGLE: f64 = 1e-3;

/// Flip the sides inside a face's triangulation (as [`refine`] takes it) where the two triangles
/// on a side lie closer along the surface the other way: their facets turn less from the surface
/// normal. A layout can leave
/// slivers standing on their long side across the surface (their corners on it, their plane across
/// it), which count several times the area under them and which no split repairs; the flip lays
/// them down. A side is flipped only when both new triangles keep the orientation of the old ones
/// in (u, v) and against the surface normal (nothing folds), and the new side and the new
/// triangles' middles stay within `tol` of the surface, or no farther than the old side's middle.
/// Sides with one triangle (the face's boundary) are never flipped.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn flip(s: &super::poles::Face, tol: f64, poles: &Poles, fixed: &[Vec3], uv: &[[f64; 2]], tris: &mut [[u32; 3]]) {
    if !(tol.is_finite() && tol > 0.0) || tris.len() < 2 {
        return;
    }
    let pos: Vec<Vec3> = uv
        .iter()
        .enumerate()
        .map(|(i, q)| fixed.get(i).copied().unwrap_or_else(|| s.derivs(q[0], q[1]).map_or(Vec3::ZERO, |d| d.0)))
        .collect();
    flip_with_pos(s, tol, poles, &pos, uv, tris);
}

/// [`flip`] reusing the evaluated 3D vertex positions `pos` already computed by [`refine`].
pub(crate) fn flip_with_pos(s: &super::poles::Face, tol: f64, poles: &Poles, pos: &[Vec3], uv: &[[f64; 2]], tris: &mut [[u32; 3]]) {
    if !(tol.is_finite() && tol > 0.0) || tris.len() < 2 {
        return;
    }
    let mean = |qs: &[[f64; 2]]| poles.mean(qs);
    let p = |i: u32| pos.get(i as usize).copied();
    let q = |i: u32| uv.get(i as usize).copied();
    let off = |m: [f64; 2], c: Vec3| match s.derivs(m[0], m[1]) {
        Some((x, su, sv)) => su.cross(sv).normalized().map_or_else(|| x.dist(c), |n| (x - c).dot(n).abs()),
        None => Vec3::ZERO.dist(c),
    };
    // Twice the signed area in (u, v).
    let area_uv = |t: [u32; 3]| match (q(t[0]), q(t[1]), q(t[2])) {
        (Some(a), Some(b), Some(c)) => (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
        _ => 0.0,
    };
    // The triangle's smallest angle in 3D.
    let min_angle = |t: [u32; 3]| -> f64 {
        let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) else { return 0.0 };
        let angle = |o: Vec3, x: Vec3, y: Vec3| match ((x - o).normalized(), (y - o).normalized()) {
            (Some(u), Some(v)) => det::acos(u.dot(v).clamp(-1.0, 1.0)),
            _ => 0.0,
        };
        angle(a, b, c).min(angle(b, c, a)).min(angle(c, a, b))
    };
    // Centroid surface evaluation computed once per triangle: facet normal deviation, alignment
    // with surface normal (for upright check), and normal sag at the centroid.
    #[derive(Clone, Copy)]
    struct TriMetrics {
        dev: f64,
        dot_n: Option<f64>,
        off: f64,
    }
    let metrics = |t: [u32; 3]| -> TriMetrics {
        let (Some(a), Some(b), Some(c), Some(qa), Some(qb), Some(qc)) = (p(t[0]), p(t[1]), p(t[2]), q(t[0]), q(t[1]), q(t[2])) else {
            return TriMetrics { dev: 0.0, dot_n: Some(0.0), off: f64::INFINITY };
        };
        let m = mean(&[qa, qb, qc]);
        let cross_f = (b - a).cross(c - a);
        let center = (a + b + c) * (1.0 / 3.0);
        match s.derivs(m[0], m[1]) {
            Some((x, su, sv)) => {
                let sn = su.cross(sv);
                let dot_n = Some(cross_f.dot(sn));
                match sn.normalized() {
                    Some(n) => {
                        let dev = cross_f.normalized().map_or(0.0, |f| det::acos(f.dot(n).abs().clamp(0.0, 1.0)));
                        TriMetrics { dev, dot_n, off: (x - center).dot(n).abs() }
                    }
                    None => TriMetrics { dev: 0.0, dot_n, off: x.dist(center) },
                }
            }
            None => TriMetrics { dev: 0.0, dot_n: None, off: Vec3::ZERO.dist(center) },
        }
    };
    let mut sides: BitsMap<(u32, u32), Vec<u32>> = BitsMap::with_capacity_and_hasher(tris.len() * 2, std::hash::BuildHasherDefault::default());
    for (ti, t) in tris.iter().enumerate() {
        for [a, b, _] in (0..3).map(|k| rot3(*t, k)) {
            sides.entry(key(a, b)).or_default().push(ti as u32);
        }
    }
    let mut queue: Vec<(u32, u32)> = sides.iter().filter(|(_, ts)| ts.len() == 2).map(|(e, _)| *e).collect();
    queue.sort_unstable();
    let mut budget = tris.len().saturating_mul(8);
    // Each triangle's centroid metrics, until it is flipped.
    let mut metrics_of: Vec<Option<TriMetrics>> = vec![None; tris.len()];
    while let Some(e) = queue.pop() {
        if budget == 0 {
            break;
        }
        let Some(&[t1, t2]) = sides.get(&e).map(|v| v.as_slice()) else { continue };
        let (Some(&ta), Some(&tb)) = (tris.get(t1 as usize), tris.get(t2 as usize)) else { continue };
        // [a, b, c] on one side of a -> b, [b, a, d] on the other.
        let Some([a, b, c]) = (0..3).map(|k| rot3(ta, k)).find(|[a, b, _]| key(*a, *b) == e) else { continue };
        let Some(d) = tb.iter().copied().find(|&v| v != a && v != b) else { continue };
        if c == d || sides.contains_key(&key(c, d)) {
            continue;
        }
        let (n1, n2) = ([a, d, c], [d, b, c]);
        let sign = area_uv(ta).signum();
        if sign == 0.0 || area_uv(tb).signum() != sign || area_uv(n1) * sign <= 0.0 || area_uv(n2) * sign <= 0.0 {
            continue;
        }
        // The pair the other way must lie closer along the surface (its facets turn less from
        // the surface normal), by more than a degree: a pair within a degree of the surface is
        // left as the layout made it.
        let mut cached = |ti: u32, t: [u32; 3]| match metrics_of.get(ti as usize).copied().flatten() {
            Some(m) => m,
            None => {
                let m = metrics(t);
                if let Some(slot) = metrics_of.get_mut(ti as usize) {
                    *slot = Some(m);
                }
                m
            }
        };
        let (ma, mb) = (cached(t1, ta), cached(t2, tb));
        let worse = ma.dev.max(mb.dev);
        let degree = 1f64.to_radians();
        if worse < degree {
            continue;
        }
        let m1 = metrics(n1);
        if m1.dev >= worse - degree {
            continue;
        }
        let m2 = metrics(n2);
        if m2.dev >= worse - degree {
            continue;
        }
        // ... and have area in 3D (three corners on one line of a cone have none, however they
        // lie in (u, v)).
        if min_angle(n1).min(min_angle(n2)) < MIN_FLIP_ANGLE {
            continue;
        }
        if !(m1.dot_n.is_none_or(|v| v * sign > 0.0) && m2.dot_n.is_none_or(|v| v * sign > 0.0)) {
            continue;
        }
        let (Some(pa), Some(pb), Some(pc), Some(pd), Some(qa), Some(qb), Some(qc), Some(qd)) = (p(a), p(b), p(c), p(d), q(a), q(b), q(c), q(d))
        else {
            continue;
        };
        let allowed = tol.max(off(mean(&[qa, qb]), (pa + pb) * 0.5)).max(ma.off).max(mb.off);
        if off(mean(&[qc, qd]), (pc + pd) * 0.5) > allowed || m1.off > allowed || m2.off > allowed {
            continue;
        }
        budget -= 1;
        if let Some(slot) = metrics_of.get_mut(t1 as usize) {
            *slot = Some(m1);
        }
        if let Some(slot) = metrics_of.get_mut(t2 as usize) {
            *slot = Some(m2);
        }
        if let Some(slot) = tris.get_mut(t1 as usize) {
            *slot = n1;
        }
        if let Some(slot) = tris.get_mut(t2 as usize) {
            *slot = n2;
        }
        sides.remove(&e);
        sides.insert(key(c, d), vec![t1, t2]);
        // a-d moves from t2 to t1, b-c from t1 to t2.
        retag(&mut sides, a, d, t2, t1);
        retag(&mut sides, b, c, t1, t2);
        queue.extend([key(a, d), key(d, b), key(b, c), key(c, a)].into_iter().filter(|s| sides.get(s).is_some_and(|ts| ts.len() == 2)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::eval::plan::EvalSurface;

    /// Refining and flipping a coarse patch of a cylinder (every sag over the tolerance, so the
    /// nodes are looked at by many triangles, sides and rounds) asks the pole question once per
    /// node and direction, not once per look. Ported from #13559.
    #[test]
    fn the_pole_question_is_asked_once_per_node() {
        use solidcore::eval::plan::EvalCurve;
        let s = EvalSurface::Revolved {
            profile: EvalCurve { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[3.0, 0.0, 0.0, 1.0], [3.0, 0.0, 4.0, 1.0]] },
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            swap: false,
        };
        let n = 6u32;
        let mut uv: Vec<[f64; 2]> = Vec::new();
        for i in 0..=n {
            for j in 0..=n {
                uv.push([f64::from(i) / f64::from(n), 3.0 * f64::from(j) / f64::from(n)]);
            }
        }
        let mut tris: Vec<[u32; 3]> = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let a = i * (n + 1) + j;
                let (b, c, d) = (a + 1, a + n + 1, a + n + 2);
                tris.push([a, c, d]);
                tris.push([a, d, b]);
            }
        }
        let s = super::super::poles::Face::new(s, 0.0);
        let poles = Poles::new(&s, 1e-9);
        let start = uv.len();
        refine(&s, 5e-2, &poles, &[], &mut uv, &mut tris, 300);
        flip(&s, 5e-2, &poles, &[], &uv, &mut tris);
        assert!(uv.len() > 2 * start, "{} nodes: not refined", uv.len());
        assert!(poles.asked.get() > 0);
        assert!(poles.asked.get() <= 2 * uv.len(), "{} questions for {} nodes", poles.asked.get(), uv.len());
    }
}
