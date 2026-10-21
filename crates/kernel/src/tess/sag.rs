//! The chord tolerance over a face's finished triangulation, whatever layout made it. A grid
//! divides each parameter direction within the tolerance, but on a doubly curved surface the
//! two sags add up in the middle of a cell, along its diagonal; a patch blended from its sides
//! has no lines along the surface's own directions at all. Every triangle is checked against
//! the surface (at its centroid and at the middle of each side inside the face) and the sides
//! that leave it are split in two, both triangles on them together, so the face stays
//! conforming. Sides on the face's boundary are never split: they are the edges' own nodes,
//! which the neighbouring face shares.

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::tess::EvalSurface;

/// Split the triangles `tris` of a face (indices into `uv`; the first `fixed.len()` points sit
/// at `fixed`, the rest on the surface at their parameters) until none of them lies farther
/// than `tol` from the surface `s`, adding at most `max_points` points to `uv`.
pub(crate) fn refine(s: &EvalSurface, tol: f64, fixed: &[Vec3], uv: &mut Vec<[f64; 2]>, tris: &mut Vec<[u32; 3]>, max_points: usize) {
    if !(tol.is_finite() && tol > 0.0) || tris.is_empty() {
        return;
    }
    let at = |q: [f64; 2]| s.eval(q[0], q[1]).0;
    // How far the flat point `c` lies from the surface, measured along the normal at the surface
    // point of the same parameters: those parameters need not be the nearest (a straight line in
    // the parameters is no geodesic near a pole), and only the departure across the surface
    // counts. Where the normal is undefined (a pole), the whole distance.
    let off = |q: [f64; 2], c: Vec3| match s.derivs(q[0], q[1]) {
        Some((p, su, sv)) => su.cross(sv).normalized().map_or_else(|| p.dist(c), |n| (p - c).dot(n).abs()),
        None => at(q).dist(c),
    };
    let mut pos: Vec<Vec3> = uv.iter().enumerate().map(|(i, q)| fixed.get(i).copied().unwrap_or_else(|| at(*q))).collect();
    // No side is split below this length: a surface whose parameters bend sharply (at a pole) is
    // not chased to the last bit.
    let shortest = tol * 1e-3;
    let mut sides: HashMap<(u32, u32), Vec<u32>> = HashMap::with_capacity(tris.len() * 2);
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            sides.entry(key(t[k], t[(k + 1) % 3])).or_default().push(ti as u32);
        }
    }
    let mut sag_of: HashMap<(u32, u32), f64> = HashMap::new();
    let limit = uv.len().saturating_add(max_points);
    let mut queue: Vec<u32> = (0..tris.len() as u32).rev().collect();
    while let Some(ti) = queue.pop() {
        if uv.len() >= limit {
            break;
        }
        let Some(&t) = tris.get(ti as usize) else { continue };
        let p = |i: u32| pos.get(i as usize).copied();
        let q = |i: u32| uv.get(i as usize).copied();
        let (Some(pa), Some(pb), Some(pc), Some(qa), Some(qb), Some(qc)) = (p(t[0]), p(t[1]), p(t[2]), q(t[0]), q(t[1]), q(t[2])) else {
            continue;
        };
        // Sides inside the face (two triangles on them), long enough to split, with their sag.
        let mut inner: Vec<(f64, f64, (u32, u32))> = Vec::with_capacity(3);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let e = key(a, b);
            if sides.get(&e).is_none_or(|ts| ts.len() != 2) {
                continue;
            }
            let (Some(xa), Some(xb), Some(ua), Some(ub)) = (p(a), p(b), q(a), q(b)) else { continue };
            let len = xa.dist(xb);
            if len <= shortest {
                continue;
            }
            let sag = *sag_of.entry(e).or_insert_with(|| off(mean(s, &[ua, ub]), (xa + xb) * 0.5));
            inner.push((sag, len, e));
        }
        // The surface's departure from a flat triangle is about quadratic over it: at the
        // centroid it is 4/9 of the sum of the sags at the sides' middles, which splitting a side
        // inside the face divides by four. A triangle whose middle is off splits its side inside
        // the face that sags most, while that side still contributes; what is left comes from
        // its boundary sides, which the edges' own sampling keeps within the tolerance.
        let centroid = mean(s, &[qa, qb, qc]);
        let worst = inner.iter().max_by(|x, y| x.0.total_cmp(&y.0)).copied();
        let split = match worst {
            Some((sag, _, e)) if sag > tol => e,
            Some((sag, _, e)) if sag > 0.25 * tol && off(centroid, (pa + pb + pc) * (1.0 / 3.0)) > tol => e,
            _ => continue,
        };
        let (Some(ua), Some(ub)) = (q(split.0), q(split.1)) else { continue };
        let m = mean(s, &[ua, ub]);
        let mi = uv.len() as u32;
        uv.push(m);
        pos.push(at(m));
        sag_of.remove(&split);
        queue.extend(split_side(tris, &mut sides, split, mi));
    }
}

/// The mean of parameter points, as the surface point between them: at a pole the parameter
/// along the degenerate direction is free (the pole copy carries a neighbour's), so there the
/// mean takes it from the other points alone.
fn mean(s: &EvalSurface, qs: &[[f64; 2]]) -> [f64; 2] {
    let mut out = [0.0; 2];
    for (d, slot) in out.iter_mut().enumerate() {
        let free: Vec<f64> = qs.iter().filter(|q| !super::uv::degenerate(s, **q, d)).map(|q| q[d]).collect();
        let pick: Vec<f64> = if free.is_empty() { qs.iter().map(|q| q[d]).collect() } else { free };
        *slot = pick.iter().sum::<f64>() / pick.len().max(1) as f64;
    }
    out
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// Move triangle `from` to `to` in the list of side `(a, b)`.
fn retag(sides: &mut HashMap<(u32, u32), Vec<u32>>, a: u32, b: u32, from: u32, to: u32) {
    if let Some(ts) = sides.get_mut(&key(a, b)) {
        for t in ts.iter_mut().filter(|t| **t == from) {
            *t = to;
        }
    }
}

/// Split side `e` at the new point `m`: each triangle on it becomes two, in its own winding.
/// The triangles touched, old and new.
fn split_side(tris: &mut Vec<[u32; 3]>, sides: &mut HashMap<(u32, u32), Vec<u32>>, e: (u32, u32), m: u32) -> Vec<u32> {
    let on = sides.remove(&e).unwrap_or_default();
    let mut touched = Vec::with_capacity(4);
    for ti in on {
        let Some(&t) = tris.get(ti as usize) else { continue };
        // Turn the triangle so the side comes first, as it winds: [a, b, c].
        let Some(k) = (0..3).find(|&k| key(t[k], t[(k + 1) % 3]) == e) else { continue };
        let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
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
