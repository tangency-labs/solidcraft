//! Rebuilding a link whose shape changed ([`Sketch::update_link`]) without losing what the
//! sketch's own geometry held on it. The new entities are made first; the sketch's curves and
//! constraints on the old ones move to the new ones that match them (the same corner, the same
//! circle, the same side or the piece of it that is left); only then do the old ones go.
use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec2;
use solidcraft_geom::Aabb2;
use solidcraft_geom::prelude::*;

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
    // Per new point: (claims found where they were, all claims).
    let mut count: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for &(_, i, exact) in &claims {
        let n = count.entry(i).or_default();
        n.0 += usize::from(exact);
        n.1 += 1;
    }
    for &(k, i, exact) in &claims {
        // A point found where it was wins over one found by its place in the link.
        let (found, all) = count.get(&i).copied().unwrap_or_default();
        let rivals = if exact { found } else { all }.saturating_sub(1);
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

/// The new whole circle (an index into `lay.curves`) old arc `c` lies on: the one circle about
/// the point `c`'s centre became, of `c`'s radius. A closed circular edge was once read as its
/// two halves and is one circle now; both halves lie on it.
fn circle_of_arc(c: &LCurve, old: &[Vec2], map: &[Option<usize>], lay: &Layout) -> Option<usize> {
    let LCurve::Arc(centre, a, _) = *c else { return None };
    let new_centre = map.get(centre).copied().flatten()?;
    let radius = old.get(centre)?.dist(*old.get(a)?);
    let tol = TOL * (1.0 + radius);
    let mut hits = lay.curves.iter().enumerate().filter(|(_, n)| matches!(n, LCurve::Circle(x, r) if *x == new_centre && (r - radius).abs() <= tol));
    match (hits.next(), hits.next()) {
        (Some((j, _)), None) => Some(j),
        _ => None,
    }
}

/// The new arc (an index into `lay.curves`) old arc `c` is when its centre became a different
/// point: the one arc between the points `c`'s ends became whose centre is where `c`'s was (or
/// where the point `c`'s centre became is). Two concentric arcs saved on one centre point can be
/// read again with centres apart by more than the layout merges but within [`TOL`] (#20280: a
/// shelled wall's rim and its outer rim); the old centre becomes only one of the two, and the
/// other arc is still the same arc.
fn arc_about_same_centre(c: &LCurve, old: &[Vec2], map: &[Option<usize>], lay: &Layout) -> Option<usize> {
    let LCurve::Arc(centre, a, b) = *c else { return None };
    let (ma, mb) = (map.get(a).copied().flatten()?, map.get(b).copied().flatten()?);
    let was = *old.get(centre)?;
    let became = map.get(centre).copied().flatten().and_then(|i| lay.pts.get(i)).copied();
    let near = |p: Vec2, q: Vec2| p.dist(q) <= TOL * (1.0 + p.len().max(q.len()));
    let mut hits = lay.curves.iter().enumerate().filter(|(_, n)| {
        matches!(**n, LCurve::Arc(x, y, z) if y == ma && z == mb
            && lay.pts.get(x).is_some_and(|p| near(*p, was) || became.is_some_and(|q| near(*p, q))))
    });
    match (hits.next(), hits.next()) {
        (Some((j, _)), None) => Some(j),
        _ => None,
    }
}

/// The new curve (an index into `lay.curves`) old curve `c` becomes, and whether it is only a
/// piece of the line `c` was: the curve of the same kind on the points `c`'s points became, for
/// an arc without one the arc between the same ends about the same centre
/// ([`arc_about_same_centre`]) or else the whole circle it lies on ([`circle_of_arc`], a piece too), or for a
/// line without one, the longest new line along it (where it was, or between the points its
/// ends became).
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
    if let Some(j) = arc_about_same_centre(c, old, map, lay) {
        return Some((j, false));
    }
    if let Some(j) = circle_of_arc(c, old, map, lay) {
        return Some((j, true));
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

/// Does `k` depend on where arc `c` ends, not only on the circle it lies on? It cannot move to
/// that whole circle then.
fn needs_arc_ends(k: &ConstraintKind, c: usize) -> bool {
    use ConstraintKind::*;
    match *k {
        Midpoint { l, .. } => l == c,
        ArcSweep { c: x, .. } | ArcLength { c: x, .. } | PointAlong { c: x, .. } => x == c,
        Smooth { a, b } | SlotOverallLength { a, b, .. } => a == c || b == c,
        _ => false,
    }
}

/// Names have already fixed a one-to-one whole-edge correspondence. Only
/// witness its extent and direction here; endpoints never choose another edge.
/// Closed or changed extents without a unique witness keep the ordinary refusal.
fn named_curve_identities(cur: &[LCurve], old: &[Vec2], lay: &Layout, names: &[(usize, usize)]) -> BTreeMap<usize, (usize, bool)> {
    let ends = |c: &LCurve| match *c {
        LCurve::Line(a, b) | LCurve::Arc(_, a, b) | LCurve::Conic(a, b, ..) | LCurve::Spline(a, b, ..) => Some([a, b]),
        _ => None,
    };
    names
        .iter()
        .filter_map(|&(i, j)| {
            let (a, b) = (cur.get(i)?, lay.curves.get(j)?);
            let [a0, a1] = ends(a)?;
            let [b0, b1] = ends(b)?;
            let old = [*old.get(a0)?, *old.get(a1)?];
            let fresh = [*lay.pts.get(b0)?, *lay.pts.get(b1)?];
            let agrees = |points: &[Vec2]| solidcraft_kernel::curves::ordered_nearby_points(&old, points, TOL, 16).unwrap_or(false);
            match (agrees(&fresh), agrees(&[fresh[1], fresh[0]])) {
                (true, false) => Some((i, (j, false))),
                (false, true) => Some((i, (j, true))),
                _ => None,
            }
        })
        .collect()
}

impl Sketch {
    /// Rebuild link `id`, whose entities are `points` and `curves` (laid out as `cur`, when they
    /// still are a layout), for its new shape `lay`. A curve of the sketch's own on an old point
    /// moves to the new point that matches it, or else to a free point of its own where it was;
    /// a constraint moves to the new points and curves that match, and goes only when one of
    /// them has no match. A new point or curve that matches an old one takes its id; one that
    /// nothing in the sketch holds is matched afterwards, to what the held ones left, and not
    /// at all past [`MAX_MATCH_WORK`]. All or nothing: on an error the sketch is as it was.
    pub(super) fn rebuild_link(
        &mut self,
        id: &str,
        lay: &Layout,
        points: &[usize],
        curves: &[usize],
        cur: Option<&[LCurve]>,
        named_edges: &[(usize, usize)],
    ) -> Result<()> {
        let saved = self.clone();
        let rebuilt = self.rebuild_link_in_place(id, lay, points, curves, cur, named_edges);
        if rebuilt.is_err() {
            *self = saved;
        }
        rebuilt
    }

    fn rebuild_link_in_place(
        &mut self,
        id: &str,
        lay: &Layout,
        points: &[usize],
        curves: &[usize],
        cur: Option<&[LCurve]>,
        named_edges: &[(usize, usize)],
    ) -> Result<()> {
        let keep = self.links.clone();
        let old_point: BTreeMap<usize, usize> = points.iter().enumerate().map(|(k, p)| (*p, k)).collect();
        let old_curve: BTreeMap<usize, usize> = curves.iter().enumerate().map(|(k, c)| (*c, k)).collect();
        let old_ids: BTreeSet<String> = points.iter().filter_map(|p| self.points.get(*p)).map(|p| p.id.clone()).collect();
        let old_pos: Vec<Vec2> = points.iter().map(|p| self.points.get(*p).map_or(Vec2::ZERO, |q| q.pos)).collect();
        let identities = named_curve_identities(cur.unwrap_or_default(), &old_pos, lay, named_edges);
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
        // An arc's centre, to find the whole circle the arc may be now ([`circle_of_arc`]).
        needed.extend(cur.unwrap_or_default().iter().filter_map(|c| match *c {
            LCurve::Arc(centre, ..) => Some(centre),
            _ => None,
        }));
        let first_curve = self.curves.len();
        let made = self.build_link(id, lay)?;
        // Old local point -> new layout point, old local curve -> (new layout curve, a piece).
        // `id_points` and `id_curves` add the entities nothing in the sketch holds: features
        // outside it pick a profile by the ids of its curves (#14249). They are matched after the
        // held ones, only to new entities no held one took, so what the sketch holds moves as
        // before.
        let (point_map, curve_map, id_points, id_curves) = match cur {
            Some(cur) => {
                let kinds = |roles: Vec<[usize; 10]>| roles.iter().map(kind_of).collect::<Vec<_>>();
                let (old_kind, mut new_kind) = (kinds(point_roles(cur, points.len())), kinds(point_roles(&lay.curves, lay.pts.len())));
                let held: Vec<usize> = needed.iter().copied().collect();
                let map = match_points(&old_pos, &old_kind, &lay.pts, &new_kind, &held);
                let curve_map: BTreeMap<usize, (usize, bool)> =
                    needed_curves.iter().filter_map(|k| Some((*k, match_curve(cur.get(*k)?, &old_pos, &map, lay)?))).collect();
                // A new point a held one became is no candidate any more.
                for i in map.iter().flatten() {
                    if let Some(kind) = new_kind.get_mut(*i) {
                        *kind = u8::MAX;
                    }
                }
                let rest: Vec<usize> = (0..points.len()).filter(|k| !needed.contains(k)).collect();
                let mut id_points = match_points(&old_pos, &old_kind, &lay.pts, &new_kind, &rest);
                for (slot, held) in id_points.iter_mut().zip(&map) {
                    if held.is_some() {
                        *slot = *held;
                    }
                }
                // Past the work bound no point was matched, and the curves are not either.
                let within = rest.len().saturating_mul(lay.pts.len()) <= MAX_MATCH_WORK;
                // (a piece, how far the radius moved, old curve, new curve): the closest matches
                // take their curve first, so a circle that kept its radius keeps its id.
                let mut found: Vec<(bool, f64, usize, usize)> = (0..curves.len())
                    .filter(|k| within && !needed_curves.contains(k))
                    .filter_map(|k| {
                        let c = cur.get(k)?;
                        let (j, piece) = match_curve(c, &old_pos, &id_points, lay)?;
                        let moved = match (c, lay.curves.get(j)) {
                            (LCurve::Circle(_, r), Some(LCurve::Circle(_, s))) => (r - s).abs(),
                            _ => 0.0,
                        };
                        Some((piece, moved, k, j))
                    })
                    .collect();
                found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                let mut taken: BTreeSet<usize> = curve_map.values().map(|(j, _)| *j).collect();
                let mut id_curves: BTreeMap<usize, usize> = BTreeMap::new();
                for (_, _, k, j) in found {
                    if taken.insert(j) {
                        id_curves.insert(k, j);
                    }
                }
                (map, curve_map, id_points, id_curves)
            }
            None => (vec![None; points.len()], BTreeMap::new(), vec![None; points.len()], BTreeMap::new()),
        };
        // Arcs that are one whole circle now (old local curve -> new layout curve), and their
        // ends, which no new point stands for: the circle each of those lay on. What the sketch
        // held on such an end keeps its place, on the circle.
        let arcs_on_circles: BTreeMap<usize, usize> =
            cur.unwrap_or_default().iter().enumerate().filter_map(|(k, c)| Some((k, circle_of_arc(c, &old_pos, &point_map, lay)?))).collect();
        let mut end_on_circle: BTreeMap<usize, usize> = BTreeMap::new();
        for (k, j) in &arcs_on_circles {
            if let Some(LCurve::Arc(_, a, b)) = cur.and_then(|cur| cur.get(*k)) {
                for e in [*a, *b] {
                    if point_map.get(e).copied().flatten().is_none()
                        && let Some(p) = points.get(e)
                    {
                        end_on_circle.entry(*p).or_insert(first_curve + j);
                    }
                }
            }
        }
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
        let mut held_on: Vec<(usize, usize)> = Vec::new();
        for p in unmatched {
            let pos = self.points.get(p).map_or(Vec2::ZERO, |q| q.pos);
            let point = self.add_point(pos, None)?;
            free.insert(p, point);
            if let Some(c) = end_on_circle.get(&p) {
                held_on.push((point, *c));
            }
        }
        let to_point = |p: usize| if old_point.contains_key(&p) { new_point(p).or_else(|| free.get(&p).copied()) } else { Some(p) };
        // A curve whose kind changed (a spline that is an arc again) has no geometric match in
        // `curve_map`, but a named identity says which new curve it is: constraints on it follow.
        let to_curve = |c: usize| match old_curve.get(&c) {
            Some(k) => curve_map.get(k).or_else(|| identities.get(k)).map(|(j, _)| first_curve + j),
            None => Some(c),
        };
        for c in self.curves.iter_mut().filter(|c| c.link.as_deref() != Some(id)) {
            c.kind.map_points(&|p| to_point(p).unwrap_or(p));
        }
        let pieces: Vec<usize> = curve_map
            .iter()
            .filter(|(k, (_, piece))| *piece && !arcs_on_circles.contains_key(k))
            .filter_map(|(k, _)| curves.get(*k).copied())
            .collect();
        let arcs: Vec<usize> = arcs_on_circles.keys().filter_map(|k| curves.get(*k).copied()).collect();
        // A point of the sketch's own made coincident with such an arc end lies on the circle.
        for k in &mut self.constraints {
            if let ConstraintKind::Coincident { p, q } = k.kind {
                let on = |end: usize, other: usize| if old_point.contains_key(&other) { None } else { end_on_circle.get(&end) };
                if let Some(c) = on(q, p) {
                    k.kind = ConstraintKind::PointOnCurve { p, c: *c };
                } else if let Some(c) = on(p, q) {
                    k.kind = ConstraintKind::PointOnCurve { p: q, c: *c };
                }
            }
        }
        self.constraints.retain_mut(|k| {
            !pieces.iter().any(|c| needs_whole_line(&k.kind, *c))
                && !arcs.iter().any(|c| needs_arc_ends(&k.kind, *c))
                && k.kind.remap(&to_point, &to_curve)
        });
        for (p, c) in held_on {
            self.add_constraint(ConstraintKind::PointOnCurve { p, c }, None)?;
        }
        // A new entity that matches an old one is that entity, moved: it takes the old id, so
        // the solve that follows sees where it was (`positions_in`) and names still find it.
        let id_point = |k: usize| id_points.get(k).copied().flatten().and_then(|li| made.get(li).copied().flatten());
        let point_ids: BTreeMap<String, String> = points
            .iter()
            .enumerate()
            .filter_map(|(k, p)| Some((self.points.get(id_point(k)?)?.id.clone(), self.points.get(*p)?.id.clone())))
            .collect();
        let mut curve_ids: BTreeMap<String, String> = BTreeMap::new();
        // A new curve that is an old one keeps how the sketch used it: a projected outline set to
        // construction stays out of profiles (#17732).
        let mut curve_flags: BTreeMap<String, (bool, bool)> = BTreeMap::new();
        let mut reversed = BTreeSet::new();
        for (k, (j, reverse)) in &identities {
            if let (Some(new), Some(old)) = (self.curves.get(first_curve + j), curves.get(*k).and_then(|c| self.curves.get(*c))) {
                curve_ids.insert(new.id.clone(), old.id.clone());
                curve_flags.insert(new.id.clone(), (old.construction, old.centerline));
                if *reverse {
                    reversed.insert(old.id.clone());
                }
            }
        }
        // The halves of a circle that is one curve now: the first half's id names the circle.
        let matched = curve_map.iter().map(|(k, (j, _))| (k, j));
        let mut stable_ids: BTreeSet<String> = curve_ids.values().cloned().collect();
        for (k, j) in arcs_on_circles.iter().chain(matched).chain(&id_curves) {
            if let (Some(new), Some(old)) = (self.curves.get(first_curve + j), curves.get(*k).and_then(|c| self.curves.get(*c))) {
                if curve_ids.contains_key(&new.id) || !stable_ids.insert(old.id.clone()) {
                    continue;
                }
                curve_flags.insert(new.id.clone(), (old.construction, old.centerline));
                curve_ids.insert(new.id.clone(), old.id.clone());
            }
        }
        // Transfer ids before deletion. Pruning must see the retired generated ids,
        // not stable ids still named by offsets, equations, patterns or features.
        let retired: BTreeMap<String, String> = curve_ids.iter().map(|(new, old)| (old.clone(), new.clone())).collect();
        let old_curves: BTreeSet<usize> = curves.iter().copied().collect();
        for (i, curve) in self.curves.iter_mut().enumerate() {
            if old_curves.contains(&i) {
                if let Some(temporary) = retired.get(&curve.id) {
                    curve.id.clone_from(temporary);
                }
            } else {
                if let Some((construction, centerline)) = curve_flags.get(&curve.id) {
                    (curve.construction, curve.centerline) = (*construction, *centerline);
                }
                if let Some(stable) = curve_ids.get(&curve.id) {
                    curve.id.clone_from(stable);
                }
            }
        }
        for offset in &mut self.offsets {
            for (source, forward) in &mut offset.sources {
                *forward ^= reversed.contains(source);
            }
        }
        for offset in &mut self.freeform_offsets {
            offset.forward ^= reversed.contains(&offset.source);
        }
        for offset in &mut self.freeform_chain_offsets {
            for (source, forward) in &mut offset.sources {
                *forward ^= reversed.contains(source);
            }
        }
        // Nothing refers to the old entities any more: they go without taking anything along.
        let chain_offsets = self.freeform_chain_offsets.clone();
        self.remove_curves(curves);
        // A rebuild is not a user deletion. An unresolved source must still
        // report "offset source vanished", rather than silently break its offset.
        let remaining: BTreeSet<&str> = self.curves.iter().map(|c| c.id.as_str()).collect();
        self.freeform_chain_offsets = chain_offsets.into_iter().filter(|o| o.outputs.iter().all(|id| remaining.contains(id.as_str()))).collect();
        let rest: Vec<usize> = self.points.iter().enumerate().filter(|(_, p)| old_ids.contains(&p.id)).map(|(i, _)| i).collect();
        self.remove_points(&rest);
        for p in &mut self.points {
            if let Some(old) = point_ids.get(&p.id) {
                p.id.clone_from(old);
            }
        }
        // Removal prunes empty links; this one stays.
        self.links = keep;
        Ok(())
    }
}
