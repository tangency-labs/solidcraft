//! A planar region with holes cut into a few convex polygons through its boundary points (for
//! programs that take polygons but no holes, such as a live link's n-gons).
//!
//! Points that only continue a straight side are set aside, each hole is bridged to the outer
//! loop (towards the nearest edge to its right that it can see), the one polygon that makes is
//! cut into triangles by clipping ears, and triangles are joined across their shared sides
//! wherever the join stays convex, longest side first (Hertel and Mehlhorn: at most four times
//! the fewest convex parts). The points set aside go back into the sides they lay on, so every
//! boundary point is a corner of the part along it.

use std::collections::{HashMap, HashSet};

use crate::Vec2;

/// Most boundary points [`convex_parts`] takes (clipping ears is quadratic in them).
pub const MAX_CONVEX_POINTS: usize = 10_000;

/// Corners closer to a straight line than this, relative to its length, are on it.
const STRAIGHT: f64 = 1e-7;

/// Convex polygons covering the region inside `loops[0]` (counter-clockwise) and outside every
/// later loop (the holes, clockwise). Each part lists its corners as (loop, index) pairs,
/// counter-clockwise. `None` when the loops cannot be cut: wound the wrong way, not finite,
/// crossing, too many points.
pub fn convex_parts(loops: &[Vec<Vec2>]) -> Option<Vec<Vec<(usize, usize)>>> {
    let total: usize = loops.iter().map(Vec::len).sum();
    if loops.is_empty() || total > MAX_CONVEX_POINTS || loops.iter().any(|l| l.len() < 3) {
        return None;
    }
    let mut pts: Vec<Vec2> = Vec::with_capacity(total);
    let mut ids: Vec<(usize, usize)> = Vec::with_capacity(total);
    let mut ranges = Vec::with_capacity(loops.len());
    for (l, points) in loops.iter().enumerate() {
        let start = pts.len();
        for (i, p) in points.iter().enumerate() {
            if !(p.x.is_finite() && p.y.is_finite()) {
                return None;
            }
            pts.push(*p);
            ids.push((l, i));
        }
        ranges.push(start..pts.len());
    }
    let at = |i: usize| pts.get(i).copied().unwrap_or(Vec2::ZERO);
    let areas: Vec<f64> = ranges.iter().map(|r| area(&r.clone().collect::<Vec<_>>(), &at)).collect();
    let (Some(&outer_area), Some(holes)) = (areas.first(), areas.get(1..)) else { return None };
    if !(outer_area > 0.0) || holes.iter().any(|a| !(*a < 0.0)) {
        return None;
    }

    // Each loop without its straight-through points; those points, by the side they lie on.
    let mut kept: Vec<Vec<usize>> = Vec::with_capacity(ranges.len());
    let mut between: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for r in &ranges {
        let ring: Vec<usize> = r.clone().collect();
        let n = ring.len();
        let straight = |k: usize| {
            let (a, v, b) = (ring.get((k + n - 1) % n), ring.get(k), ring.get((k + 1) % n));
            let (Some(&a), Some(&v), Some(&b)) = (a, v, b) else { return false };
            on_segment(at(a), at(v), at(b))
        };
        let keep: Vec<usize> = (0..n).filter(|&k| !straight(k)).filter_map(|k| ring.get(k).copied()).collect();
        if keep.len() < 3 {
            return None;
        }
        // Walk from a kept point, gathering the dropped ones until the next kept point.
        let first = (0..n).find(|&k| !straight(k))?;
        let mut from = ring.get(first).copied().unwrap_or(0);
        let mut run = Vec::new();
        for s in 1..=n {
            let k = (first + s) % n;
            let Some(&v) = ring.get(k) else { continue };
            if straight(k) {
                run.push(v);
            } else {
                if !run.is_empty() {
                    between.insert((from, v), std::mem::take(&mut run));
                }
                from = v;
            }
        }
        kept.push(keep);
    }

    let poly = bridge_holes(kept, &at)?;
    let tris = clip_ears(&poly, &at)?;
    let parts = merge_convex(tris, &at)?;

    // The straight-through points back into the sides they lay on.
    let mut out: Vec<Vec<usize>> = Vec::with_capacity(parts.len());
    for part in parts {
        let mut corners = Vec::with_capacity(part.len());
        for (k, &a) in part.iter().enumerate() {
            corners.push(a);
            let b = part.get((k + 1) % part.len()).copied().unwrap_or(a);
            if let Some(run) = between.get(&(a, b)) {
                corners.extend(run);
            }
        }
        out.push(corners);
    }

    // The parts must cover the region exactly and use every boundary point.
    let covered: f64 = out.iter().map(|p| area(p, &at)).sum();
    let region: f64 = areas.iter().sum();
    let used: HashSet<usize> = out.iter().flatten().copied().collect();
    if !(region > 0.0) || (covered - region).abs() > 1e-9 * outer_area || used.len() != total || out.iter().any(|p| !(area(p, &at) > 0.0)) {
        return None;
    }
    out.into_iter().map(|p| p.into_iter().map(|i| ids.get(i).copied()).collect()).collect()
}

/// The signed area of a ring of points (positive counter-clockwise).
fn area(ring: &[usize], at: &impl Fn(usize) -> Vec2) -> f64 {
    let Some(&first) = ring.first() else { return 0.0 };
    let o = at(first);
    let mut s = 0.0;
    for (k, &i) in ring.iter().enumerate() {
        let j = ring.get((k + 1) % ring.len()).copied().unwrap_or(i);
        s += (at(i) - o).cross(at(j) - o);
    }
    s * 0.5
}

/// `v` lies on the segment from `a` to `b`, strictly between its ends.
fn on_segment(a: Vec2, v: Vec2, b: Vec2) -> bool {
    let ab = b - a;
    let l2 = ab.len2();
    if !(l2 > 0.0) {
        return false;
    }
    let t = (v - a).dot(ab) / l2;
    t > 0.0 && t < 1.0 && v.dist(a + ab * t) <= STRAIGHT * l2.sqrt()
}

/// The turn at `v` from `a` to `b`: positive to the left.
fn turn(a: Vec2, v: Vec2, b: Vec2) -> f64 {
    (v - a).cross(b - v)
}

/// The outer loop with every hole spliced in through a doubled bridge: one ring, holes in
/// order of their rightmost point, each bridged to a point of the ring it can see.
fn bridge_holes(mut loops: Vec<Vec<usize>>, at: &impl Fn(usize) -> Vec2) -> Option<Vec<usize>> {
    if loops.is_empty() {
        return None;
    }
    let mut ring = loops.remove(0);
    // The rightmost point of each hole (the lowest when level), right to left.
    let rightmost = |h: &Vec<usize>| -> Option<(usize, Vec2)> {
        h.iter().enumerate().map(|(k, &i)| (k, at(i))).max_by(|a, b| a.1.x.total_cmp(&b.1.x).then(b.1.y.total_cmp(&a.1.y)))
    };
    let mut holes: Vec<(usize, Vec2, Vec<usize>)> = loops.into_iter().filter_map(|h| rightmost(&h).map(|(k, p)| (k, p, h))).collect();
    holes.sort_by(|a, b| b.1.x.total_cmp(&a.1.x).then(a.1.y.total_cmp(&b.1.y)));
    for (k, m, hole) in holes {
        let pos = visible(&ring, m, at)?;
        let n = hole.len();
        let mut spliced = Vec::with_capacity(ring.len() + n + 2);
        spliced.extend(ring.get(..=pos)?);
        spliced.extend((0..=n).filter_map(|s| hole.get((k + s) % n)));
        spliced.push(*ring.get(pos)?);
        spliced.extend(ring.get(pos + 1..)?);
        ring = spliced;
    }
    Some(ring)
}

/// The position in `ring` of a point that `m` (inside the ring) sees: along a ray to the right
/// to the first side it meets, then the end of that side, or a reflex corner nearer the ray
/// when one hides it.
fn visible(ring: &[usize], m: Vec2, at: &impl Fn(usize) -> Vec2) -> Option<usize> {
    let n = ring.len();
    let p = |k: usize| ring.get(k % n).map(|&i| at(i));
    // The nearest side crossing the ray upwards (inside is to the left of the sides).
    let mut hit: Option<(f64, usize)> = None;
    for k in 0..n {
        let (a, b) = (p(k)?, p(k + 1)?);
        if !(a.y <= m.y && m.y < b.y) {
            continue;
        }
        let x = a.x + (m.y - a.y) / (b.y - a.y) * (b.x - a.x);
        if x >= m.x && hit.is_none_or(|(hx, _)| x < hx) {
            hit = Some((x, k));
        }
    }
    let (x, k) = hit?;
    let ray_hit = Vec2::new(x, m.y);
    let (a, b) = (p(k)?, p(k + 1)?);
    if a == ray_hit {
        return Some(k);
    }
    if b == ray_hit {
        return Some((k + 1) % n);
    }
    let end = if a.x > b.x { k } else { (k + 1) % n };
    let e = p(end)?;
    // A reflex corner inside the triangle (m, hit, end) hides the end: take the one nearest the
    // ray's direction.
    let mut best = end;
    let mut best_key = (f64::INFINITY, f64::INFINITY);
    let inside = |q: Vec2| {
        let (d1, d2, d3) = (turn(m, ray_hit, q), turn(ray_hit, e, q), turn(e, m, q));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    };
    for j in 0..n {
        let (prev, q, next) = (p(j + n - 1)?, p(j)?, p(j + 1)?);
        if j == end || q == m || turn(prev, q, next) >= 0.0 || !inside(q) {
            continue;
        }
        let d = q - m;
        let key = (d.y.abs().atan2(d.x), d.len2());
        if key.0 < best_key.0 || (key.0 == best_key.0 && key.1 < best_key.1) {
            best = j;
            best_key = key;
        }
    }
    // A point the ring passes twice (an earlier bridge): the pass whose corner opens towards m.
    let target = ring.get(best).copied()?;
    let passes: Vec<usize> = (0..n).filter(|&j| ring.get(j) == Some(&target)).collect();
    if passes.len() > 1 {
        for &j in &passes {
            let (prev, q, next) = (p(j + n - 1)?, p(j)?, p(j + 1)?);
            if opens_towards(prev, q, next, m) {
                return Some(j);
            }
        }
    }
    Some(best)
}

/// The inside of the ring's corner at `v` (between `a` before it and `b` after) holds the
/// direction to `q`.
fn opens_towards(a: Vec2, v: Vec2, b: Vec2, q: Vec2) -> bool {
    let (to_next, to_prev, d) = (b - v, a - v, q - v);
    if to_next.cross(to_prev) > 0.0 { to_next.cross(d) >= 0.0 && d.cross(to_prev) >= 0.0 } else { to_next.cross(d) >= 0.0 || d.cross(to_prev) >= 0.0 }
}

/// Triangles covering a counter-clockwise ring (bridges allowed), by clipping ears.
fn clip_ears(ring: &[usize], at: &impl Fn(usize) -> Vec2) -> Option<Vec<[usize; 3]>> {
    let n = ring.len();
    if n < 3 {
        return None;
    }
    let p = |k: usize| ring.get(k).map_or(Vec2::ZERO, |&i| at(i));
    let mut prev: Vec<usize> = (0..n).map(|k| (k + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|k| (k + 1) % n).collect();
    let link = |prev: &mut Vec<usize>, next: &mut Vec<usize>, k: usize| {
        let (a, b) = (prev.get(k).copied().unwrap_or(k), next.get(k).copied().unwrap_or(k));
        if let Some(s) = next.get_mut(a) {
            *s = b;
        }
        if let Some(s) = prev.get_mut(b) {
            *s = a;
        }
    };
    let mut left = n;
    let mut tris = Vec::with_capacity(n);
    let mut k = 0usize;
    let mut since_clip = 0usize;
    while left > 3 {
        if since_clip > left {
            // A full turn without an ear: what is left must have no area.
            let mut rest = vec![k];
            let mut j = next.get(k).copied().unwrap_or(k);
            while j != k && rest.len() <= left {
                rest.push(j);
                j = next.get(j).copied().unwrap_or(k);
            }
            let rest: Vec<usize> = rest.iter().filter_map(|&j| ring.get(j).copied()).collect();
            return (area(&rest, at).abs() <= 1e-12 * area(ring, at).abs()).then_some(tris);
        }
        let (a, b) = (prev.get(k).copied().unwrap_or(k), next.get(k).copied().unwrap_or(k));
        let (pa, pk, pb) = (p(a), p(k), p(b));
        // A spike back along a bridge, or a point repeated: drop it, it holds no area.
        if pa == pb || pk == pb {
            link(&mut prev, &mut next, k);
            left -= 1;
            k = a;
            since_clip = 0;
            continue;
        }
        if turn(pa, pk, pb) > 0.0 && !blocks_ear(&next, &prev, b, a, (pa, pk, pb), &p) {
            if let (Some(&ia), Some(&ik), Some(&ib)) = (ring.get(a), ring.get(k), ring.get(b)) {
                tris.push([ia, ik, ib]);
            }
            link(&mut prev, &mut next, k);
            left -= 1;
            k = a;
            since_clip = 0;
            continue;
        }
        k = b;
        since_clip += 1;
    }
    let a = prev.get(k).copied().unwrap_or(k);
    let b = next.get(k).copied().unwrap_or(k);
    if turn(p(a), p(k), p(b)) > 0.0
        && let (Some(&ia), Some(&ik), Some(&ib)) = (ring.get(a), ring.get(k), ring.get(b))
    {
        tris.push([ia, ik, ib]);
    }
    Some(tris)
}

/// Some reflex corner of the ring, other than the ear's own, lies in the ear `t` (inclusive).
fn blocks_ear(next: &[usize], prev: &[usize], from: usize, to: usize, t: (Vec2, Vec2, Vec2), p: &impl Fn(usize) -> Vec2) -> bool {
    let (a, k, b) = t;
    let (lo, hi) = (Vec2::new(a.x.min(k.x).min(b.x), a.y.min(k.y).min(b.y)), Vec2::new(a.x.max(k.x).max(b.x), a.y.max(k.y).max(b.y)));
    // The corners after the ear, up to the one before it.
    let mut j = next.get(from).copied().unwrap_or(to);
    let mut steps = 0usize;
    while j != to && steps <= next.len() {
        steps += 1;
        let q = p(j);
        let after = next.get(j).copied().unwrap_or(j);
        let before = prev.get(j).copied().unwrap_or(j);
        j = after;
        if q == a || q == k || q == b || q.x < lo.x || q.x > hi.x || q.y < lo.y || q.y > hi.y {
            continue;
        }
        if turn(p(before), q, p(after)) > 0.0 {
            continue;
        }
        if turn(a, k, q) >= 0.0 && turn(k, b, q) >= 0.0 && turn(b, a, q) >= 0.0 {
            return true;
        }
    }
    false
}

/// Triangles joined across shared sides wherever the join stays convex, longest side first.
fn merge_convex(tris: Vec<[usize; 3]>, at: &impl Fn(usize) -> Vec2) -> Option<Vec<Vec<usize>>> {
    let mut owner: HashMap<(usize, usize), usize> = HashMap::new();
    for (k, t) in tris.iter().enumerate() {
        for i in 0..3 {
            let e = (t[i], t[(i + 1) % 3]);
            if owner.insert(e, k).is_some() {
                return None;
            }
        }
    }
    let mut sides: Vec<(f64, usize, usize)> =
        owner.keys().filter(|&&(a, b)| a < b && owner.contains_key(&(b, a))).map(|&(a, b)| (at(a).dist(at(b)), a, b)).collect();
    sides.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    let mut parts: Vec<Option<Vec<usize>>> = tris.iter().map(|t| Some(t.to_vec())).collect();
    let convex = |a: usize, v: usize, b: usize| {
        let (pa, pv, pb) = (at(a), at(v), at(b));
        turn(pa, pv, pb) >= -1e-9 * pa.dist(pv) * pv.dist(pb)
    };
    for (_, a, b) in sides {
        let (Some(&p), Some(&q)) = (owner.get(&(a, b)), owner.get(&(b, a))) else { continue };
        if p == q {
            continue;
        }
        let (Some(Some(pp)), Some(Some(qq))) = (parts.get(p), parts.get(q)) else { continue };
        // pp from b round to a, qq from a round to b.
        let (Some(pb), Some(qa)) = (pp.iter().position(|&v| v == b), qq.iter().position(|&v| v == a)) else { continue };
        let pr: Vec<usize> = (0..pp.len()).filter_map(|s| pp.get((pb + s) % pp.len()).copied()).collect();
        let qr: Vec<usize> = (0..qq.len()).filter_map(|s| qq.get((qa + s) % qq.len()).copied()).collect();
        let (Some(&before_a), Some(&after_a), Some(&before_b), Some(&after_b)) =
            (pr.get(pr.len().wrapping_sub(2)), qr.get(1), qr.get(qr.len().wrapping_sub(2)), pr.get(1))
        else {
            continue;
        };
        if pr.last() != Some(&a) || qr.last() != Some(&b) || !convex(before_a, a, after_a) || !convex(before_b, b, after_b) {
            continue;
        }
        let mut merged = pr;
        merged.extend(qr.get(1..qr.len() - 1).unwrap_or(&[]));
        let mut seen = merged.clone();
        seen.sort_unstable();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            continue;
        }
        owner.remove(&(a, b));
        owner.remove(&(b, a));
        for (i, &v) in merged.iter().enumerate() {
            let w = merged.get((i + 1) % merged.len()).copied().unwrap_or(v);
            if let Some(o) = owner.get_mut(&(v, w)) {
                *o = p;
            }
        }
        if let Some(slot) = parts.get_mut(p) {
            *slot = Some(merged);
        }
        if let Some(slot) = parts.get_mut(q) {
            *slot = None;
        }
    }
    Some(parts.into_iter().flatten().collect())
}

#[cfg(test)]
#[path = "convex_parts_tests.rs"]
mod tests;
