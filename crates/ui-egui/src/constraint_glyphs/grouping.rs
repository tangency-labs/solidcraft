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
    pub(crate) first_for_curve: HashMap<usize, String>,
}

#[derive(Clone, Debug)]
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

/// Whether a constraint refers to the selected point or curve. Selecting a point also
/// exposes constraints on curves that use that point, so clicking a tangent endpoint can
/// reveal its line/circle constraint without turning on every other badge (#18817).
pub(crate) fn constraint_matches_selection(sk: &Sketch, kind: &ConstraintKind, selected: &[Sel]) -> bool {
    let (curves, points) = constraint_entities(kind);
    selected.iter().any(|item| match item {
        Sel::SketchCurve { id, .. } => curves.iter().any(|&index| sk.curves.get(index).is_some_and(|curve| curve.id == *id)),
        Sel::SketchPoint { id } => {
            points.iter().any(|&index| sk.points.get(index).is_some_and(|point| point.id == *id))
                || curves.iter().any(|&index| {
                    sk.curves
                        .get(index)
                        .is_some_and(|curve| curve.kind.point_ids().into_iter().any(|p| sk.points.get(p).is_some_and(|point| point.id == *id)))
                })
        }
        _ => false,
    })
}

/// A shared glyph is visible when any constraint represented by it qualifies. In
/// particular, a failing or selected second Midpoint constraint must not disappear
/// because glyph_constraints() chose the first as the displayed representative.
pub(crate) fn glyph_group_visible_when_selected(
    sk: &Sketch,
    id: &str,
    selected: &[Sel],
    picked: Option<&str>,
    is_failing: impl Fn(&String) -> bool,
) -> bool {
    let group = glyph_group(sk, id);
    sk.constraints.iter().filter(|c| group.contains(&c.id)).any(|c| {
        is_failing(&c.id)
            || picked == Some(c.id.as_str())
            || selected.iter().any(|sel| matches!(sel, Sel::SketchConstraint { id } if id == &c.id))
            || constraint_matches_selection(sk, &c.kind, selected)
    })
}

fn to_arr(p: Vec2) -> [f64; 2] {
    [p.x, p.y]
}

fn from_arr(p: [f64; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn polyline_3d_curve(poly: &[[f64; 2]]) -> Option<solidcore_record::brep::Curve> {
    let mut points: Vec<[f64; 3]> = poly.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| [p[0], p[1], 0.0]).collect();
    points.dedup();
    (points.len() >= 2).then_some(solidcore_record::brep::Curve::Polyline { points })
}

fn same_point_or_cluster(sk: &Sketch, parent: &mut [usize], p: usize, q: usize) -> bool {
    p == q
        || find_point_cluster(parent, p) == find_point_cluster(parent, q)
        || matches!((sk.point(p), sk.point(q)), (Some(a), Some(b)) if a.dist(b) <= 1e-5)
}

fn bounds_overlap(a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2]), tol: f64) -> bool {
    a.0[0] <= b.1[0] + tol && b.0[0] <= a.1[0] + tol && a.0[1] <= b.1[1] + tol && b.0[1] <= a.1[1] + tol
}

fn fast_curve_anchor_and_dir(poly: &[[f64; 2]]) -> Option<(Vec2, Option<Vec2>)> {
    if poly.len() == 2 {
        let (a, b) = (from_arr(poly[0]), from_arr(poly[1]));
        let d = b - a;
        let len = d.len();
        let dir = (len > 1e-9).then(|| d / len);
        return Some(((a + b) * 0.5, dir));
    }
    let total: f64 = poly.windows(2).map(|w| from_arr(w[0]).dist(from_arr(w[1]))).sum();
    let mut acc = 0.0;
    for w in poly.windows(2) {
        let (a, b) = (from_arr(w[0]), from_arr(w[1]));
        let l = a.dist(b);
        if acc + l >= total * 0.5 && l > 0.0 {
            return Some((a.lerp(b, (total * 0.5 - acc) / l), Some((b - a) / l)));
        }
        acc += l;
    }
    poly.first().copied().map(|p| (from_arr(p), None))
}

fn curve_anchor_and_dir(
    sk: &Sketch,
    c: usize,
    polys: &[Vec<[f64; 2]>],
    poly_bounds: &[Option<([f64; 2], [f64; 2])>],
    visible: &[bool],
    pair_crossings: &mut HashMap<(usize, usize), Vec<Vec2>>,
) -> Option<(Vec2, Option<Vec2>)> {
    const FRACS: [f64; 9] = [0.5, 0.4, 0.6, 0.33, 0.67, 0.25, 0.75, 0.2, 0.8];
    const POINT_ON_CURVE_TOL: f64 = 1e-2;
    const CROSSING_TOL: f64 = 1e-4;

    let poly = polys.get(c)?;
    let (total, candidates): (f64, Vec<(Vec2, Option<Vec2>)>) = if poly.len() == 2 {
        let (a, b) = (from_arr(poly[0]), from_arr(poly[1]));
        let d = b - a;
        let len = d.len();
        let dir = (len > 1e-9).then(|| d / len);
        (len, FRACS.iter().map(|&f| (a.lerp(b, f), dir)).collect())
    } else {
        let Some(curve) = polyline_3d_curve(poly) else {
            return poly.first().copied().map(|p| (from_arr(p), None));
        };
        let Ok((total, samples)) = solidcraft_kernel::curves::samples_at_fractions(&curve, None, &FRACS) else {
            return poly.first().copied().map(|p| (from_arr(p), None));
        };
        (total, samples.into_iter().map(|s| (Vec2::new(s.point[0], s.point[1]), s.tangent.map(|t| Vec2::new(t[0], t[1])))).collect())
    };
    let first = *candidates.first()?;

    let mut avoid: Vec<Vec2> = Vec::new();
    let bounds = poly_bounds.get(c).copied().flatten();
    let curve_center = match sk.curves.get(c).map(|cv| &cv.kind) {
        Some(CurveKind::Circle { c: ctr, .. }) => Some(*ctr),
        _ => None,
    };
    for (pi, p) in sk.points.iter().enumerate() {
        if Some(pi) == curve_center || !(p.pos.x.is_finite() && p.pos.y.is_finite()) {
            continue;
        }
        let arr = to_arr(p.pos);
        if let Some((min_xy, max_xy)) = bounds
            && (arr[0] < min_xy[0] - POINT_ON_CURVE_TOL
                || arr[0] > max_xy[0] + POINT_ON_CURVE_TOL
                || arr[1] < min_xy[1] - POINT_ON_CURVE_TOL
                || arr[1] > max_xy[1] + POINT_ON_CURVE_TOL)
        {
            continue;
        }
        if solidcraft_kernel::points::polyline_2d_closest_point(arr, poly).ok().flatten().is_some_and(|(d, _)| d <= POINT_ON_CURVE_TOL) {
            avoid.push(p.pos);
        }
    }
    for (other, other_poly) in polys.iter().enumerate() {
        if other == c || !visible.get(other).copied().unwrap_or(false) {
            continue;
        }
        if let (Some(ba), Some(bb)) = (bounds, poly_bounds.get(other).copied().flatten())
            && !bounds_overlap(ba, bb, CROSSING_TOL)
        {
            continue;
        }
        let key = if c < other { (c, other) } else { (other, c) };
        let crossings = pair_crossings.entry(key).or_insert_with(|| {
            solidcraft_kernel::points::polyline_2d_crossings(poly, other_poly, CROSSING_TOL)
                .map(|pts| pts.into_iter().map(from_arr).collect())
                .unwrap_or_default()
        });
        avoid.extend(crossings.iter().copied());
    }
    if avoid.is_empty() {
        return Some(first);
    }

    let clear_dist = (total * 0.14).clamp(1.2, 4.0);
    let mut best = first;
    let mut best_clear = -1.0_f64;
    for (pt, dir) in candidates {
        let min_d = avoid.iter().map(|&a| pt.dist(a)).fold(f64::INFINITY, f64::min);
        if min_d >= clear_dist {
            return Some((pt, dir));
        }
        if min_d > best_clear {
            best_clear = min_d;
            best = (pt, dir);
        }
    }
    Some(best)
}

fn analytic_tangent_contact(sk: &Sketch, a: usize, b: usize, poly_a: &[[f64; 2]], poly_b: &[[f64; 2]]) -> Option<Vec2> {
    use CurveKind::{Arc, Circle, Line};
    let (ka, kb) = (&sk.curves.get(a)?.kind, &sk.curves.get(b)?.kind);
    let line_round = |line_a: usize, line_b: usize, round_idx: usize, round_c: usize, round_poly: &[[f64; 2]]| -> Option<Vec2> {
        let (p0, p1, center, radius) = (sk.point(line_a)?, sk.point(line_b)?, sk.point(round_c)?, sk.radius(round_idx)?);
        let proj = solidcraft_kernel::points::tangent_contact_line_circle_2d(to_arr(p0), to_arr(p1), to_arr(center), radius, 1e-3).ok().flatten()?;
        solidcraft_kernel::points::polyline_2d_closest_point(proj, round_poly)
            .ok()
            .flatten()
            .is_some_and(|(d, _)| d <= 1e-2)
            .then_some(from_arr(proj))
    };
    match (ka, kb) {
        (Line { a: a0, b: a1 }, Circle { c, .. } | Arc { c, .. }) => line_round(*a0, *a1, b, *c, poly_b),
        (Circle { c, .. } | Arc { c, .. }, Line { a: b0, b: b1 }) => line_round(*b0, *b1, a, *c, poly_a),
        (Circle { c: c1, .. } | Arc { c: c1, .. }, Circle { c: c2, .. } | Arc { c: c2, .. }) => {
            let (p1, p2, r1, r2) = (sk.point(*c1)?, sk.point(*c2)?, sk.radius(a)?, sk.radius(b)?);
            let cand = solidcraft_kernel::points::tangent_contact_circle_circle_2d(to_arr(p1), r1, to_arr(p2), r2, 1e-3).ok().flatten()?;
            let on_a = solidcraft_kernel::points::polyline_2d_closest_point(cand, poly_a).ok().flatten().is_some_and(|(da, _)| da <= 1e-2);
            let on_b = solidcraft_kernel::points::polyline_2d_closest_point(cand, poly_b).ok().flatten().is_some_and(|(db, _)| db <= 1e-2);
            (on_a && on_b).then_some(from_arr(cand))
        }
        _ => None,
    }
}

fn curve_intersection_point(
    sk: &Sketch,
    parent: &mut [usize],
    a: usize,
    b: usize,
    is_tangent: bool,
    polys: &[Vec<[f64; 2]>],
    fast_drag: bool,
) -> Option<Vec2> {
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
    if fast_drag {
        return None;
    }
    let (poly_a, poly_b) = (polys.get(a)?, polys.get(b)?);
    if is_tangent && let Some(pt) = analytic_tangent_contact(sk, a, b, poly_a, poly_b) {
        return Some(pt);
    }
    for wa in poly_a.windows(2) {
        for wb in poly_b.windows(2) {
            if let Ok(Some(inter)) = solidcraft_kernel::points::segment_2d_intersection(wa[0], wa[1], wb[0], wb[1], 1e-4) {
                return Some(from_arr(inter));
            }
        }
    }
    if is_tangent {
        let mut best: Option<(f64, Vec2)> = None;
        for &pa in poly_a {
            if let Ok(Some((d, pb))) = solidcraft_kernel::points::polyline_2d_closest_point(pa, poly_b)
                && d <= 1e-2
                && best.is_none_or(|(bd, _)| d < bd)
            {
                best = Some((d, (from_arr(pa) + from_arr(pb)) * 0.5));
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
        let mut seen_cids: HashSet<&str> = HashSet::new();
        let mut first_for_curve: HashMap<usize, String> = HashMap::new();
        while let Some(cur) = queue.pop_front() {
            curves.push(cur);
            if let Some(neighbors) = adj.get(cur) {
                for &(next, cid) in neighbors {
                    if seen_cids.insert(cid) {
                        constraint_ids.push(cid.to_owned());
                    }
                    first_for_curve.entry(cur).or_insert_with(|| cid.to_owned());
                    first_for_curve.entry(next).or_insert_with(|| cid.to_owned());
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
        groups.push(CurveRelGroup { kind, index: groups.len() + 1, multi: false, curves, constraint_ids, first_for_curve });
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
        .map(|(i, (axis, constraint_ids))| {
            let mut first_for_curve = HashMap::new();
            if let Some(first) = constraint_ids.first() {
                first_for_curve.insert(axis, first.clone());
            }
            CurveRelGroup { kind: CurveRelKind::Symmetry, index: i + 1, multi, curves: vec![axis], constraint_ids, first_for_curve }
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn anchor_groups_with_rel(sk: &Sketch) -> (Vec<ConstraintAnchorGroup>, Vec<CurveRelGroup>) {
    anchor_groups_with_rel_mode(sk, false)
}

pub(crate) fn anchor_groups_with_rel_mode(sk: &Sketch, fast_drag: bool) -> (Vec<ConstraintAnchorGroup>, Vec<CurveRelGroup>) {
    use ConstraintKind::*;
    let mut parent = point_cluster_map(sk);
    let curve_visible: Vec<bool> = (0..sk.curves.len()).map(|ci| solidcraft_engine::view::sketch_curve_visible(sk, ci)).collect();
    let curve_polys_2d: Vec<Vec<[f64; 2]>> = (0..sk.curves.len()).map(|ci| sk.polyline(ci).into_iter().map(to_arr).collect()).collect();
    let curve_bounds: Vec<Option<([f64; 2], [f64; 2])>> = if fast_drag {
        Vec::new()
    } else {
        curve_polys_2d.iter().map(|poly| solidcraft_kernel::points::polyline_2d_bounds(poly).ok().flatten()).collect()
    };
    let mut pair_crossings: HashMap<(usize, usize), Vec<Vec2>> = HashMap::new();
    let mut anchor_cache: HashMap<usize, Option<(Vec2, Option<Vec2>)>> = HashMap::new();
    let mut cached_anchor = |ci: usize| -> Option<(Vec2, Option<Vec2>)> {
        *anchor_cache.entry(ci).or_insert_with(|| {
            if fast_drag {
                curve_polys_2d.get(ci).and_then(|p| fast_curve_anchor_and_dir(p))
            } else {
                curve_anchor_and_dir(sk, ci, &curve_polys_2d, &curve_bounds, &curve_visible, &mut pair_crossings)
            }
        })
    };

    let mut groups: Vec<ConstraintAnchorGroup> = Vec::new();
    let mut group_index: HashMap<AnchorKey, usize> = HashMap::new();
    let mut push_entry = |key: AnchorKey, at: Vec2, dir: Option<Vec2>, id: &str| {
        if let Some(&idx) = group_index.get(&key) {
            let existing = &mut groups[idx];
            if !existing.constraint_ids.iter().any(|x| x == id) {
                existing.constraint_ids.push(id.to_owned());
            }
        } else {
            let idx = groups.len();
            group_index.insert(key, idx);
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
                if let Some(joint) = curve_intersection_point(sk, &mut parent, a, b, matches!(c.kind, Tangent { .. }), &curve_polys_2d, fast_drag) {
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
            let rep_id = rg.first_for_curve.get(&ci).unwrap_or(fallback_id);
            if let Some((at, dir)) = cached_anchor(ci) {
                push_entry(AnchorKey::Curve(ci), at, dir, rep_id);
            }
        }
    }

    (groups, rel_groups)
}
