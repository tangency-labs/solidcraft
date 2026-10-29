//! Rebuilding a link whose shape changed ([`Sketch::update_link`]) without losing what the
//! sketch's own geometry held on it. The new entities are made first; the sketch's curves and
//! constraints on the old ones move to the new ones that match them (the same corner, the same
//! circle, the same side or the piece of it that is left); only then do the old ones go.
use std::collections::{BTreeMap, BTreeSet};

use solidcraft_geom::{Aabb2, Vec2};

use super::{LCurve, Layout, curve_points, point_roles, same_shape};
use crate::model::{ConstraintKind, Result, Sketch};

/// Positions closer than this, relative to their distance from the origin, are one place.
const TOL: f64 = 1e-6;

/// Most (old point, new point) pairs compared to match points; past it nothing is matched.
const MAX_MATCH_WORK: usize = 4_000_000;

/// Coarsely, what a point is to the curves that use it ([`point_roles`]): a centre, an end,
/// another point of a curve (a conic's apex, an ellipse's major point, a spline's inner point)
/// or a point on its own. A point is only matched with one of its kind.
fn kind_of(roles: &[usize; 10]) -> u8 {
    let any = |of: &[usize]| of.iter().any(|i| roles.get(*i).is_some_and(|n| *n > 0));
    if any(&[1, 2, 6]) {
        0
    } else if any(&[0, 3, 4, 8]) {
        1
    } else if any(&[5, 7, 9]) {
        2
    } else {
        3
    }
}

/// The new point (an index into `new`) each `needed` old point becomes: the one of its kind
/// where it was (it did not move), else the one of its kind nearest to where it was relative
/// to the whole link (the link grew or shrank around it), when that one is clearly the nearest.
/// Two old points never become one new point.
fn match_points(old: &[Vec2], old_kind: &[u8], new: &[Vec2], new_kind: &[u8], needed: &[usize]) -> Vec<Option<usize>> {
    let mut map = vec![None; old.len()];
    if needed.len().saturating_mul(new.len()) > MAX_MATCH_WORK {
        return map;
    }
    let (old_box, new_box): (Aabb2, Aabb2) = (old.iter().copied().collect(), new.iter().copied().collect());
    // (old, new, found where it was)
    let mut claims: Vec<(usize, usize, bool)> = Vec::new();
    for &k in needed {
        let (Some(p), Some(kind)) = (old.get(k), old_kind.get(k)) else { continue };
        let candidates = || new.iter().enumerate().filter(|(i, _)| new_kind.get(*i) == Some(kind));
        if let Some((i, _)) = candidates().find(|(_, q)| q.dist(*p) <= TOL * (1.0 + p.len())) {
            claims.push((k, i, true));
            continue;
        }
        let at = old_box.relative(*p);
        let (mut best, mut second): (Option<(usize, f64)>, f64) = (None, f64::INFINITY);
        for (i, q) in candidates() {
            let d = new_box.relative(*q).dist(at);
            match best {
                Some((_, b)) if d >= b => second = second.min(d),
                _ => {
                    second = best.map_or(second, |(_, b)| b);
                    best = Some((i, d));
                }
            }
        }
        if let Some((i, d)) = best
            && d < 0.1
            && d < second * 0.5
        {
            claims.push((k, i, false));
        }
    }
    for &(k, i, exact) in &claims {
        // A point found where it was wins over one found by its place in the link.
        let rivals = claims.iter().filter(|(o, j, e)| *j == i && *o != k && (*e || !exact)).count();
        if rivals == 0
            && let Some(slot) = map.get_mut(k)
        {
            *slot = Some(i);
        }
    }
    map
}

/// How long the part of segment `u`–`w` along segment `p`–`q` is, when `u`–`w` lies on the line
/// through `p` and `q`; else 0.
fn overlap(p: Vec2, q: Vec2, u: Vec2, w: Vec2) -> f64 {
    let Some(dir) = (q - p).normalized() else { return 0.0 };
    let tol = TOL * (1.0 + p.len().max(q.len()));
    if (u - p).cross(dir).abs() > tol || (w - p).cross(dir).abs() > tol {
        return 0.0;
    }
    let (tu, tw) = ((u - p).dot(dir), (w - p).dot(dir));
    let (lo, hi) = (tu.min(tw).max(0.0), tu.max(tw).min(p.dist(q)));
    if hi - lo > tol { hi - lo } else { 0.0 }
}

/// The new curve (an index into `lay.curves`) old curve `c` becomes, and whether it is only a
/// piece of the line `c` was: the curve of the same kind on the points `c`'s points became, or
/// for a line without one, the longest new line along it (where it was, or between the points
/// its ends became).
fn match_curve(c: &LCurve, old: &[Vec2], map: &[Option<usize>], lay: &Layout) -> Option<(usize, bool)> {
    let mapped: Option<Vec<usize>> = curve_points(c).iter().map(|p| map.get(*p).copied().flatten()).collect();
    if let Some(m) = &mapped {
        let hits: Vec<usize> = lay
            .curves
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                let at = curve_points(n);
                same_shape(c, n) && (at == *m || matches!(c, LCurve::Line(..)) && at.iter().rev().eq(m.iter()))
            })
            .map(|(j, _)| j)
            .collect();
        match (hits.as_slice(), c) {
            ([j], _) => return Some((*j, false)),
            // Concentric circles: the one with the nearest radius, when one is clearly nearest.
            ([_, _, ..], LCurve::Circle(_, r)) => {
                let mut by: Vec<(usize, f64)> = hits
                    .iter()
                    .filter_map(|j| match lay.curves.get(*j) {
                        Some(LCurve::Circle(_, s)) => Some((*j, (s - r).abs())),
                        _ => None,
                    })
                    .collect();
                by.sort_by(|x, y| x.1.total_cmp(&y.1));
                return match by.as_slice() {
                    [(j, d), (_, e), ..] if *d < *e => Some((*j, false)),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    let LCurve::Line(a, b) = *c else { return None };
    let (p, q) = (*old.get(a)?, *old.get(b)?);
    let ends = match mapped.as_deref() {
        Some([x, y]) => lay.pts.get(*x).zip(lay.pts.get(*y)),
        _ => None,
    };
    let mut best: Option<(usize, f64)> = None;
    for (j, n) in lay.curves.iter().enumerate() {
        let LCurve::Line(x, y) = *n else { continue };
        let (Some(u), Some(w)) = (lay.pts.get(x), lay.pts.get(y)) else { continue };
        let along = overlap(p, q, *u, *w).max(ends.map_or(0.0, |(p2, q2)| overlap(*p2, *q2, *u, *w)));
        if along > 0.0 && best.is_none_or(|(_, o)| along > o) {
            best = Some((j, along));
        }
    }
    best.map(|(j, _)| (j, true))
}

/// Does `k` depend on where line `c` ends, not only on the line it lies on? It cannot move to a
/// piece of `c` then.
fn needs_whole_line(k: &ConstraintKind, c: usize) -> bool {
    use ConstraintKind::*;
    match *k {
        Midpoint { l, .. } | Length { l, .. } => l == c,
        Equal { a, b } | Smooth { a, b } | SlotOverallLength { a, b, .. } => a == c || b == c,
        _ => false,
    }
}

impl Sketch {
    /// Rebuild link `id`, whose entities are `points` and `curves` (laid out as `cur`, when they
    /// still are a layout), for its new shape `lay`. A curve of the sketch's own on an old point
    /// moves to the new point that matches it, or else to a free point of its own where it was;
    /// a constraint moves to the new points and curves that match, and goes only when one of
    /// them has no match. All or nothing: on an error the sketch is as it was.
    pub(super) fn rebuild_link(&mut self, id: &str, lay: &Layout, points: &[usize], curves: &[usize], cur: Option<&[LCurve]>) -> Result<()> {
        let saved = self.clone();
        let rebuilt = self.rebuild_link_in_place(id, lay, points, curves, cur);
        if rebuilt.is_err() {
            *self = saved;
        }
        rebuilt
    }

    fn rebuild_link_in_place(&mut self, id: &str, lay: &Layout, points: &[usize], curves: &[usize], cur: Option<&[LCurve]>) -> Result<()> {
        let keep = self.links.clone();
        let old_point: BTreeMap<usize, usize> = points.iter().enumerate().map(|(k, p)| (*p, k)).collect();
        let old_curve: BTreeMap<usize, usize> = curves.iter().enumerate().map(|(k, c)| (*c, k)).collect();
        let old_ids: BTreeSet<String> = points.iter().filter_map(|p| self.points.get(*p)).map(|p| p.id.clone()).collect();
        let old_pos: Vec<Vec2> = points.iter().map(|p| self.points.get(*p).map_or(Vec2::ZERO, |q| q.pos)).collect();
        // What refers to the old entities: the sketch's own curves and its constraints.
        let mut needed: BTreeSet<usize> = BTreeSet::new();
        let mut needed_curves: BTreeSet<usize> = BTreeSet::new();
        for c in self.curves.iter().filter(|c| c.link.as_deref() != Some(id)) {
            needed.extend(c.kind.point_ids().iter().filter_map(|p| old_point.get(p)));
        }
        for k in &self.constraints {
            let (ps, cs) = k.kind.indices();
            needed.extend(ps.iter().filter_map(|p| old_point.get(p)));
            needed_curves.extend(cs.iter().filter_map(|c| old_curve.get(c)));
        }
        for c in needed_curves.iter().filter_map(|k| cur.and_then(|cur| cur.get(*k))) {
            needed.extend(curve_points(c));
        }
        let first_curve = self.curves.len();
        let made = self.build_link(id, lay)?;
        // Old local point -> new layout point, old local curve -> (new layout curve, a piece).
        let (point_map, curve_map) = match cur {
            Some(cur) => {
                let kinds = |roles: Vec<[usize; 10]>| roles.iter().map(kind_of).collect::<Vec<_>>();
                let needed: Vec<usize> = needed.into_iter().collect();
                let map = match_points(
                    &old_pos,
                    &kinds(point_roles(cur, points.len())),
                    &lay.pts,
                    &kinds(point_roles(&lay.curves, lay.pts.len())),
                    &needed,
                );
                let curve_map: BTreeMap<usize, (usize, bool)> =
                    needed_curves.iter().filter_map(|k| Some((*k, match_curve(cur.get(*k)?, &old_pos, &map, lay)?))).collect();
                (map, curve_map)
            }
            None => (vec![None; points.len()], BTreeMap::new()),
        };
        let new_point =
            |p: usize| old_point.get(&p).and_then(|k| point_map.get(*k).copied().flatten()).and_then(|li| made.get(li).copied().flatten());
        // A point of the sketch's own curves with no match stays where it was, as a free point.
        let mut free: BTreeMap<usize, usize> = BTreeMap::new();
        let unmatched: BTreeSet<usize> = self
            .curves
            .iter()
            .filter(|c| c.link.as_deref() != Some(id))
            .flat_map(|c| c.kind.point_ids())
            .filter(|p| old_point.contains_key(p) && new_point(*p).is_none())
            .collect();
        for p in unmatched {
            let pos = self.points.get(p).map_or(Vec2::ZERO, |q| q.pos);
            free.insert(p, self.add_point(pos, None)?);
        }
        let to_point = |p: usize| if old_point.contains_key(&p) { new_point(p).or_else(|| free.get(&p).copied()) } else { Some(p) };
        let to_curve = |c: usize| match old_curve.get(&c) {
            Some(k) => curve_map.get(k).map(|(j, _)| first_curve + j),
            None => Some(c),
        };
        for c in self.curves.iter_mut().filter(|c| c.link.as_deref() != Some(id)) {
            c.kind.map_points(&|p| to_point(p).unwrap_or(p));
        }
        let pieces: Vec<usize> = curve_map.iter().filter(|(_, (_, piece))| *piece).filter_map(|(k, _)| curves.get(*k).copied()).collect();
        self.constraints.retain_mut(|k| !pieces.iter().any(|c| needs_whole_line(&k.kind, *c)) && k.kind.remap(&to_point, &to_curve));
        // A new entity that matches an old one is that entity, moved: it takes the old id, so
        // the solve that follows sees where it was (`positions_in`) and names still find it.
        let point_ids: BTreeMap<String, String> =
            points.iter().filter_map(|p| Some((self.points.get(new_point(*p)?)?.id.clone(), self.points.get(*p)?.id.clone()))).collect();
        let mut curve_ids: BTreeMap<String, String> = BTreeMap::new();
        for (k, (j, _)) in &curve_map {
            if let (Some(new), Some(old)) = (self.curves.get(first_curve + j), curves.get(*k).and_then(|c| self.curves.get(*c))) {
                curve_ids.entry(new.id.clone()).or_insert_with(|| old.id.clone());
            }
        }
        // Nothing refers to the old entities any more: they go without taking anything along.
        self.remove_curves(curves);
        let rest: Vec<usize> = self.points.iter().enumerate().filter(|(_, p)| old_ids.contains(&p.id)).map(|(i, _)| i).collect();
        self.remove_points(&rest);
        for p in &mut self.points {
            if let Some(old) = point_ids.get(&p.id) {
                p.id.clone_from(old);
            }
        }
        for c in &mut self.curves {
            if let Some(old) = curve_ids.get(&c.id) {
                c.id.clone_from(old);
            }
        }
        // Removal prunes empty links; this one stays.
        self.links = keep;
        Ok(())
    }
}
