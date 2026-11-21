//! Constraint grouping, connected curve-relation groups, intersection anchors, and curve anchor shifting.

use std::collections::{HashMap, HashSet, VecDeque};

use solidcore_record::Vec2;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{Constraint, ConstraintKind, CurveKind, Sketch};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AnchorKey {
    PointCluster(usize),
    PointPair(usize, usize),
    Curve(usize),
    CurveJoint(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CurveRelKind {
    Equal,
    Parallel,
    Symmetry,
}

impl CurveRelKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Equal => "Equal",
            Self::Parallel => "Parallel",
            Self::Symmetry => "Symmetry",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CurveRelGroup {
    pub(crate) kind: CurveRelKind,
    pub(crate) index: usize,
    pub(crate) multi: bool,
    pub(crate) curves: Vec<usize>,
    pub(crate) constraint_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HoveredGeom {
    Curve(usize),
    PointCluster(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SelectedGeomKey {
    Curve(String),
    Point(String),
}

pub(crate) struct ConstraintAnchorGroup {
    pub(crate) key: AnchorKey,
    pub(crate) at: Vec2,
    pub(crate) dir: Option<Vec2>,
    pub(crate) constraint_ids: Vec<String>,
}

/// The constraints one glyph stands for (shared non-driven MidPoint glyphs group together).
pub fn glyph_group(sk: &Sketch, id: &str) -> Vec<String> {
    use ConstraintKind::Midpoint;
    match sk.constraints.iter().find(|c| c.id == id).map(|c| (c.kind.clone(), c.driven)) {
        Some((Midpoint { p, .. }, false)) => {
            sk.constraints.iter().filter(|c| !c.driven && matches!(c.kind, Midpoint { p: q, .. } if q == p)).map(|c| c.id.clone()).collect()
        }
        _ => vec![id.to_string()],
    }
}

/// What deleting glyph `id` removes: the constraints it stands for, plus any orphaned midpoint helper.
pub fn glyph_deletion(sk: &Sketch, id: &str) -> Vec<Sel> {
    use ConstraintKind::Midpoint;
    let group = glyph_group(sk, id);
    let shared = match sk.constraints.iter().find(|c| c.id == id).map(|c| &c.kind) {
        Some(Midpoint { p, .. }) if group.len() > 1 => Some(*p),
        _ => None,
    };
    let orphan = shared.filter(|&p| {
        !sk.curves.iter().any(|c| c.kind.uses(p))
            && !sk.constraints.iter().any(|c| !group.contains(&c.id) && c.kind.indices().0.contains(&p))
            && sk.points.get(p).is_some_and(|q| !q.fixed && q.link.is_none() && q.exprs.is_none())
    });
    let mut out: Vec<Sel> = group.into_iter().map(|id| Sel::SketchConstraint { id }).collect();
    if let Some(q) = orphan.and_then(|p| sk.points.get(p)) {
        out.push(Sel::SketchPoint { id: q.id.clone() });
    }
    out
}

/// The curves and points a constraint ties together.
pub(crate) fn constraint_entities(k: &ConstraintKind) -> (Vec<usize>, Vec<usize>) {
    use ConstraintKind::*;
    match *k {
        Coincident { p, q } | HorizontalPoints { p, q } | VerticalPoints { p, q } => (vec![], vec![p, q]),
        PatternPoint { p, q, center, directions, .. } => {
            (directions.into_iter().flatten().map(|(l, _)| l).collect(), vec![p, q].into_iter().chain(center).collect())
        }
        PatternRadius { a, b } => (vec![a, b], vec![]),
        Fix { p } => (vec![], vec![p]),
        Midpoint { p, l } => (vec![l], vec![p]),
        PointOnCurve { p, c } | PointAlong { p, c, .. } => (vec![c], vec![p]),
        Symmetric { p, q, l } => (vec![l], vec![p, q]),
        Horizontal { l } | Vertical { l } => (vec![l], vec![]),
        Parallel { a, b }
        | Perpendicular { a, b }
        | Collinear { a, b }
        | Equal { a, b }
        | Concentric { a, b }
        | Tangent { a, b }
        | Smooth { a, b } => (vec![a, b], vec![]),
        _ => (vec![], vec![]),
    }
}

pub(crate) fn find_point_cluster(parent: &mut [usize], mut x: usize) -> usize {
    while let Some(&p) = parent.get(x) {
        if p == x {
            break;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(x) {
            *slot = gp;
        }
        x = p;
    }
    x
}

pub(crate) fn point_cluster_map(sk: &Sketch) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..sk.points.len()).collect();
    for c in &sk.constraints {
        if let ConstraintKind::Coincident { p, q } = c.kind {
            let rp = find_point_cluster(&mut parent, p);
            let rq = find_point_cluster(&mut parent, q);
            if rp != rq
                && let Some(slot) = parent.get_mut(rq)
            {
                *slot = rp;
            }
        }
    }
    parent
}

pub(crate) fn constraint_relates_to_geom(parent: &mut [usize], k: &ConstraintKind, geom: HoveredGeom) -> bool {
    let (cs, ps) = constraint_entities(k);
    match geom {
        HoveredGeom::Curve(ci) => cs.contains(&ci),
        HoveredGeom::PointCluster(root) => ps.into_iter().any(|p| find_point_cluster(parent, p) == root),
    }
}

/// Constraints that draw a glyph: no dimensions, one MidPoint per shared midpoint, no PointOnCurve
/// already subsumed by a Midpoint on the same point cluster & line, and pattern constraints once.
pub(crate) fn glyph_constraints(sk: &Sketch) -> impl Iterator<Item = &Constraint> {
    let mut midpoints = HashSet::new();
    let mut parent = point_cluster_map(sk);
    let mid_pairs: HashSet<(usize, usize)> = sk
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            ConstraintKind::Midpoint { p, l } if !c.driven => Some((find_point_cluster(&mut parent, p), l)),
            _ => None,
        })
        .collect();
    sk.constraints.iter().filter(move |c| {
        if c.kind.is_dimension() {
            return false;
        }
        if let ConstraintKind::PointOnCurve { p, c: curve } = c.kind
            && !mid_pairs.is_empty()
            && mid_pairs.contains(&(find_point_cluster(&mut parent, p), curve))
        {
            return false;
        }
        if let ConstraintKind::Midpoint { p, .. } = c.kind
            && !c.driven
            && !midpoints.insert(p)
        {
            return false;
        }
        !(matches!(c.kind, ConstraintKind::PatternPoint { .. } | ConstraintKind::PatternRadius { .. })
            && !sk.patterns.iter().any(|p| p.instances.first().and_then(|i| i.constraints.first()) == Some(&c.id)))
    })
}

pub(crate) fn poly_point_at_frac(poly: &[Vec2], total: f64, frac: f64) -> Option<(Vec2, Option<Vec2>)> {
    if total <= 1e-12 {
        return poly.first().map(|p| (*p, None));
    }
    let target = total * frac.clamp(0.0, 1.0);
    let mut acc = 0.0;
    for w in poly.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= target && l > 0.0 {
            return Some((w[0].lerp(w[1], ((target - acc) / l).clamp(0.0, 1.0)), (w[1] - w[0]).normalized()));
        }
        acc += l;
    }
    poly.last().map(|p| (*p, None))
}

fn poly_aabb_2d(poly: &[Vec2]) -> Option<(Vec2, Vec2)> {
    let first = *poly.first()?;
    let (mut min, mut max) = (first, first);
    for &p in &poly[1..] {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Some((min, max))
}

fn aabb_2d_overlaps(a: (Vec2, Vec2), b: (Vec2, Vec2), pad: f64) -> bool {
    a.0.x - pad <= b.1.x && a.1.x + pad >= b.0.x && a.0.y - pad <= b.1.y && a.1.y + pad >= b.0.y
}

fn dist_point_to_seg_2d(p: Vec2, a: Vec2, b: Vec2) -> (f64, Vec2) {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 <= 1e-12 {
        return (p.dist(a), a);
    }
    let proj = a + ab * ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p.dist(proj), proj)
}

fn dist_point_to_poly_2d(p: Vec2, poly: &[Vec2]) -> Option<(f64, Vec2)> {
    let mut best: Option<(f64, Vec2)> = None;
    for w in poly.windows(2) {
        let (d, proj) = dist_point_to_seg_2d(p, w[0], w[1]);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, proj));
        }
    }
    best.or_else(|| poly.first().map(|&q| (p.dist(q), q)))
}

fn seg_intersect_2d(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Option<Vec2> {
    let (r, s) = (a2 - a1, b2 - b1);
    let rxs = r.x * s.y - r.y * s.x;
    if rxs.abs() <= 1e-9 {
        return None;
    }
    let qmp = b1 - a1;
    let t = (qmp.x * s.y - qmp.y * s.x) / rxs;
    let u = (qmp.x * r.y - qmp.y * r.x) / rxs;
    if (-1e-4..=1.0 + 1e-4).contains(&t) && (-1e-4..=1.0 + 1e-4).contains(&u) { Some(a1 + r * t.clamp(0.0, 1.0)) } else { None }
}

fn same_point_or_cluster(sk: &Sketch, parent: &mut [usize], p: usize, q: usize) -> bool {
    p == q
        || find_point_cluster(parent, p) == find_point_cluster(parent, q)
        || matches!((sk.point(p), sk.point(q)), (Some(a), Some(b)) if a.dist(b) <= 1e-5)
}

fn curve_anchor_and_dir(
    sk: &Sketch,
    c: usize,
    polys: &[Vec<Vec2>],
    aabbs: &[Option<(Vec2, Vec2)>],
    visible: &[bool],
) -> Option<(Vec2, Option<Vec2>)> {
    let poly = polys.get(c)?;
    let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
    if poly.len() < 2 || total <= 1e-12 {
        return poly.first().map(|p| (*p, None));
    }
    let Some(c_aabb) = aabbs.get(c).copied().flatten() else {
        return poly_point_at_frac(poly, total, 0.5);
    };

    let open_ends = poly.first().zip(poly.last()).filter(|(a, b)| a.dist(**b) > 1e-4);
    let end_tol = (total * 0.05).max(1e-3);
    let near_open_end = |q: Vec2| open_ends.is_some_and(|(a, b)| q.dist(*a) <= end_tol || q.dist(*b) <= end_tol);
    let curve_center = match sk.curves.get(c).map(|cv| &cv.kind) {
        Some(CurveKind::Circle { c: ctr, .. }) => Some(*ctr),
        _ => None,
    };

    let mut avoid_pts: Vec<Vec2> = Vec::new();
    let pt_tol = (total * 0.06).max(1e-2);
    for (pi, p) in sk.points.iter().enumerate() {
        if Some(pi) == curve_center || !p.pos.x.is_finite() || !p.pos.y.is_finite() || near_open_end(p.pos) {
            continue;
        }
        if p.pos.x < c_aabb.0.x - pt_tol || p.pos.x > c_aabb.1.x + pt_tol || p.pos.y < c_aabb.0.y - pt_tol || p.pos.y > c_aabb.1.y + pt_tol {
            continue;
        }
        if dist_point_to_poly_2d(p.pos, poly).is_some_and(|(d, _)| d <= pt_tol) {
            avoid_pts.push(p.pos);
        }
    }
    for other in 0..polys.len() {
        if other == c || !visible.get(other).copied().unwrap_or(false) {
            continue;
        }
        let Some(o_aabb) = aabbs.get(other).copied().flatten() else { continue };
        if !aabb_2d_overlaps(c_aabb, o_aabb, 1e-4) {
            continue;
        }
        let Some(opoly) = polys.get(other) else { continue };
        for wa in poly.windows(2) {
            let seg_aabb = (Vec2::new(wa[0].x.min(wa[1].x), wa[0].y.min(wa[1].y)), Vec2::new(wa[0].x.max(wa[1].x), wa[0].y.max(wa[1].y)));
            if !aabb_2d_overlaps(seg_aabb, o_aabb, 1e-4) {
                continue;
            }
            for wb in opoly.windows(2) {
                if let Some(ix) = seg_intersect_2d(wa[0], wa[1], wb[0], wb[1])
                    && !near_open_end(ix)
                {
                    avoid_pts.push(ix);
                }
            }
        }
    }

    if avoid_pts.is_empty() {
        return poly_point_at_frac(poly, total, 0.5);
    }
    const FRACS: [f64; 9] = [0.5, 0.4, 0.6, 0.33, 0.67, 0.25, 0.75, 0.2, 0.8];
    let safe = (total * 0.14).max(1e-3);
    let mut best: Option<(f64, Vec2, Option<Vec2>)> = None;
    for u in FRACS {
        let Some((pt, dir)) = poly_point_at_frac(poly, total, u) else { continue };
        let min_d = avoid_pts.iter().map(|a| pt.dist(*a)).fold(f64::INFINITY, f64::min);
        let prox_cost = if min_d < safe { ((safe - min_d) / safe) * 100.0 } else { 0.0 };
        let score = prox_cost + (u - 0.5).abs() * 0.5 - min_d.min(safe) * 0.01;
        if best.is_none_or(|(bs, _, _)| score < bs) {
            best = Some((score, pt, dir));
        }
    }
    best.map(|(_, pt, dir)| (pt, dir)).or_else(|| poly_point_at_frac(poly, total, 0.5))
}

fn analytic_tangent_contact(sk: &Sketch, a: usize, b: usize, poly_a: &[Vec2], poly_b: &[Vec2]) -> Option<Vec2> {
    use CurveKind::{Arc, Circle, Line};
    let (ka, kb) = (&sk.curves.get(a)?.kind, &sk.curves.get(b)?.kind);
    let line_round = |line_a: usize, line_b: usize, round_idx: usize, round_c: usize, round_poly: &[Vec2]| -> Option<Vec2> {
        let (p0, p1, center, radius) = (sk.point(line_a)?, sk.point(line_b)?, sk.point(round_c)?, sk.radius(round_idx)?);
        let (_, proj) = dist_point_to_seg_2d(center, p0, p1);
        ((proj.dist(center) - radius).abs() <= 1e-3 && dist_point_to_poly_2d(proj, round_poly).is_some_and(|(d, _)| d <= 1e-2)).then_some(proj)
    };
    match (ka, kb) {
        (Line { a: a0, b: a1 }, Circle { c, .. } | Arc { c, .. }) => line_round(*a0, *a1, b, *c, poly_b),
        (Circle { c, .. } | Arc { c, .. }, Line { a: b0, b: b1 }) => line_round(*b0, *b1, a, *c, poly_a),
        (Circle { c: c1, .. } | Arc { c: c1, .. }, Circle { c: c2, .. } | Arc { c: c2, .. }) => {
            let (p1, p2, r1, r2) = (sk.point(*c1)?, sk.point(*c2)?, sk.radius(a)?, sk.radius(b)?);
            let d = p1.dist(p2);
            if d <= 1e-9 {
                return None;
            }
            let dir = (p2 - p1) * (1.0 / d);
            let cand = if (d - (r1 + r2)).abs() <= 1e-3 {
                Some(p1 + dir * r1)
            } else if (d - (r1 - r2).abs()).abs() <= 1e-3 {
                Some(if r1 >= r2 { p1 + dir * r1 } else { p1 - dir * r1 })
            } else {
                None
            };
            cand.filter(|&pt| {
                dist_point_to_poly_2d(pt, poly_a).is_some_and(|(da, _)| da <= 1e-2)
                    && dist_point_to_poly_2d(pt, poly_b).is_some_and(|(db, _)| db <= 1e-2)
            })
        }
        _ => None,
    }
}

fn curve_intersection_point(sk: &Sketch, parent: &mut [usize], a: usize, b: usize, is_tangent: bool, polys: &[Vec<Vec2>]) -> Option<Vec2> {
    let (ca, cb) = (sk.curves.get(a)?, sk.curves.get(b)?);
    let (ends_a, ends_b) = (ca.kind.ends(), cb.kind.ends());
    if let (Some((a0, a1)), Some((b0, b1))) = (ends_a, ends_b) {
        for ea in [a0, a1] {
            for eb in [b0, b1] {
                if same_point_or_cluster(sk, parent, ea, eb)
                    && let Some(pt) = sk.point(ea)
                {
                    return Some(pt);
                }
            }
        }
    }
    for (ends, other_idx) in [(ends_a, b), (ends_b, a)] {
        if let Some((e0, e1)) = ends {
            for ep in [e0, e1] {
                let constrained_on = sk.constraints.iter().any(|c| {
                    matches!(c.kind, ConstraintKind::PointOnCurve { p, c: ci } | ConstraintKind::Midpoint { p, l: ci } if ci == other_idx && same_point_or_cluster(sk, parent, p, ep))
                });
                if constrained_on && let Some(pt) = sk.point(ep) {
                    return Some(pt);
                }
            }
        }
    }
    let (poly_a, poly_b) = (polys.get(a)?, polys.get(b)?);
    if is_tangent && let Some(pt) = analytic_tangent_contact(sk, a, b, poly_a, poly_b) {
        return Some(pt);
    }
    for wa in poly_a.windows(2) {
        for wb in poly_b.windows(2) {
            if let Some(inter) = seg_intersect_2d(wa[0], wa[1], wb[0], wb[1]) {
                return Some(inter);
            }
        }
    }
    if is_tangent {
        let mut best: Option<(f64, Vec2)> = None;
        for &pa in poly_a {
            if let Some((d, pb)) = dist_point_to_poly_2d(pa, poly_b)
                && d <= 1e-2
                && best.is_none_or(|(bd, _)| d < bd)
            {
                best = Some((d, (pa + pb) * 0.5));
            }
        }
        return best.map(|(_, pt)| pt);
    }
    None
}

fn compute_curve_rel_groups(sk: &Sketch, kind: CurveRelKind) -> Vec<CurveRelGroup> {
    let n = sk.curves.len();
    let mut adj: Vec<Vec<(usize, &str)>> = vec![Vec::new(); n];
    for c in glyph_constraints(sk) {
        let pair = match (kind, &c.kind) {
            (CurveRelKind::Equal, ConstraintKind::Equal { a, b }) => Some((*a, *b)),
            (CurveRelKind::Parallel, ConstraintKind::Parallel { a, b }) => Some((*a, *b)),
            _ => None,
        };
        if let Some((a, b)) = pair
            && a < n
            && b < n
        {
            adj[a].push((b, &c.id));
            adj[b].push((a, &c.id));
        }
    }
    let mut visited = vec![false; n];
    let mut groups: Vec<CurveRelGroup> = Vec::new();
    for start in 0..n {
        if visited.get(start).copied().unwrap_or(true) || adj.get(start).is_none_or(Vec::is_empty) {
            continue;
        }
        if let Some(slot) = visited.get_mut(start) {
            *slot = true;
        }
        let mut queue = VecDeque::from([start]);
        let (mut curves, mut constraint_ids) = (Vec::new(), Vec::new());
        while let Some(cur) = queue.pop_front() {
            curves.push(cur);
            if let Some(neighbors) = adj.get(cur) {
                for &(next, cid) in neighbors {
                    if !constraint_ids.iter().any(|x| x == cid) {
                        constraint_ids.push(cid.to_owned());
                    }
                    if !visited.get(next).copied().unwrap_or(true) {
                        if let Some(slot) = visited.get_mut(next) {
                            *slot = true;
                        }
                        queue.push_back(next);
                    }
                }
            }
        }
        curves.sort_unstable();
        groups.push(CurveRelGroup { kind, index: groups.len() + 1, multi: false, curves, constraint_ids });
    }
    let multi = groups.len() > 1;
    for g in &mut groups {
        g.multi = multi;
    }
    groups
}

fn compute_symmetry_axis_groups(sk: &Sketch) -> Vec<CurveRelGroup> {
    let mut by_axis: Vec<(usize, Vec<String>)> = Vec::new();
    for c in glyph_constraints(sk) {
        if let ConstraintKind::Symmetric { l, .. } = c.kind
            && l < sk.curves.len()
        {
            if let Some((_, ids)) = by_axis.iter_mut().find(|(ax, _)| *ax == l) {
                if !ids.iter().any(|x| x == &c.id) {
                    ids.push(c.id.clone());
                }
            } else {
                by_axis.push((l, vec![c.id.clone()]));
            }
        }
    }
    let multi = by_axis.len() > 1;
    by_axis
        .into_iter()
        .enumerate()
        .map(|(i, (axis, constraint_ids))| CurveRelGroup { kind: CurveRelKind::Symmetry, index: i + 1, multi, curves: vec![axis], constraint_ids })
        .collect()
}

pub(crate) fn anchor_groups_with_rel(sk: &Sketch) -> (Vec<ConstraintAnchorGroup>, Vec<CurveRelGroup>) {
    use ConstraintKind::*;
    let mut parent = point_cluster_map(sk);
    let curve_visible: Vec<bool> = (0..sk.curves.len()).map(|ci| solidcraft_engine::view::sketch_curve_visible(sk, ci)).collect();
    let curve_polys_2d: Vec<Vec<Vec2>> = (0..sk.curves.len()).map(|ci| sk.polyline(ci)).collect();
    let curve_aabbs_2d: Vec<Option<(Vec2, Vec2)>> = curve_polys_2d.iter().map(|p| poly_aabb_2d(p)).collect();
    let mut anchor_cache: HashMap<usize, Option<(Vec2, Option<Vec2>)>> = HashMap::new();
    let mut cached_anchor = |ci: usize| -> Option<(Vec2, Option<Vec2>)> {
        *anchor_cache.entry(ci).or_insert_with(|| curve_anchor_and_dir(sk, ci, &curve_polys_2d, &curve_aabbs_2d, &curve_visible))
    };

    let mut groups: Vec<ConstraintAnchorGroup> = Vec::new();
    let mut push_entry = |key: AnchorKey, at: Vec2, dir: Option<Vec2>, id: &str| {
        if let Some(existing) = groups.iter_mut().find(|g| g.key == key) {
            if !existing.constraint_ids.iter().any(|x| x == id) {
                existing.constraint_ids.push(id.to_owned());
            }
        } else {
            groups.push(ConstraintAnchorGroup { key, at, dir, constraint_ids: vec![id.to_owned()] });
        }
    };

    for c in glyph_constraints(sk) {
        match c.kind {
            Coincident { p, .. } | Fix { p } | Midpoint { p, .. } | PointOnCurve { p, .. } | PointAlong { p, .. } => {
                let root = find_point_cluster(&mut parent, p);
                if let Some(at) = sk.point(root).or_else(|| sk.point(p)) {
                    push_entry(AnchorKey::PointCluster(root), at, None, &c.id);
                }
            }
            PatternPoint { p, center, .. } => {
                let target = center.unwrap_or(p);
                let root = find_point_cluster(&mut parent, target);
                if let Some(at) = sk.point(root).or_else(|| sk.point(target)) {
                    push_entry(AnchorKey::PointCluster(root), at, None, &c.id);
                }
            }
            HorizontalPoints { p, q } | VerticalPoints { p, q } => {
                let (r1, r2) = (find_point_cluster(&mut parent, p), find_point_cluster(&mut parent, q));
                let (lo, hi) = if r1 <= r2 { (r1, r2) } else { (r2, r1) };
                if let Some((a, b)) = sk.point(p).zip(sk.point(q)) {
                    push_entry(AnchorKey::PointPair(lo, hi), (a + b) * 0.5, None, &c.id);
                }
            }
            Horizontal { l } | Vertical { l } => {
                if let Some((at, dir)) = cached_anchor(l) {
                    push_entry(AnchorKey::Curve(l), at, dir, &c.id);
                }
            }
            Perpendicular { a, b } | Tangent { a, b } => {
                if let Some(joint) = curve_intersection_point(sk, &mut parent, a, b, matches!(c.kind, Tangent { .. }), &curve_polys_2d) {
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    push_entry(AnchorKey::CurveJoint(lo, hi), joint, None, &c.id);
                } else {
                    for curve in [a, b] {
                        if let Some((at, dir)) = cached_anchor(curve) {
                            push_entry(AnchorKey::Curve(curve), at, dir, &c.id);
                        }
                    }
                }
            }
            Collinear { a, b } | Concentric { a, b } | Smooth { a, b } => {
                for curve in [a, b] {
                    if let Some((at, dir)) = cached_anchor(curve) {
                        push_entry(AnchorKey::Curve(curve), at, dir, &c.id);
                    }
                }
            }
            _ => {}
        }
    }

    let mut rel_groups = compute_curve_rel_groups(sk, CurveRelKind::Equal);
    rel_groups.extend(compute_curve_rel_groups(sk, CurveRelKind::Parallel));
    rel_groups.extend(compute_symmetry_axis_groups(sk));
    for rg in &rel_groups {
        let Some(fallback_id) = rg.constraint_ids.first() else { continue };
        for &ci in &rg.curves {
            let rep_id = rg
                .constraint_ids
                .iter()
                .find(|cid| {
                    sk.constraints.iter().find(|c| &c.id == *cid).is_some_and(|c| match c.kind {
                        Equal { a, b } | Parallel { a, b } => a == ci || b == ci,
                        Symmetric { l, .. } => l == ci,
                        _ => false,
                    })
                })
                .unwrap_or(fallback_id);
            if let Some((at, dir)) = cached_anchor(ci) {
                push_entry(AnchorKey::Curve(ci), at, dir, rep_id);
            }
        }
    }

    (groups, rel_groups)
}
