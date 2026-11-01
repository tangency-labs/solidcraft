//! Closed-profile detection.
//!
//! Non-construction lines and arcs form a planar graph (end points within a small tolerance are
//! merged). Dangling edges are pruned, then faces are traced with the "first clockwise turn"
//! rule, which yields every bounded region counter-clockwise. Circles are loops of their own.
//! A loop that lies inside another loop of a different connected component becomes a hole of
//! the innermost loop containing it, so a circle inside a rectangle gives two profiles: the
//! rectangle with a hole, and the disc.

use std::f64::consts::TAU;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb2, Loop2, Region2, RegionOutline, Ring, Seg2, Vec2};

use crate::model::{CurveKind, Sketch};
use crate::shape::{Shape, intersections_within};
use crate::union_find::{find, union};

const MERGE_TOL: f64 = 1e-6;
const ENDPOINT_JOIN_TOL: f64 = MERGE_TOL * 100.0;
const MAX_EDGES: usize = 50_000;

/// A closed region usable by features, with the ids of the curves bounding it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub region: Region2,
    /// Curve ids of the outer loop.
    pub outer_curves: Vec<String>,
    /// Curve ids of each hole loop.
    pub hole_curves: Vec<Vec<String>>,
    pub area: f64,
    pub centroid: Vec2,
    // Computed before exact-segment restoration; never trusted from serialized input.
    #[serde(skip)]
    complete_outer: Option<Vec<String>>,
    #[serde(skip)]
    complete_holes: Option<Vec<Vec<String>>>,
    // [`Profile::contains`]'s outer box and sampled loops, made on first use from `region` (a
    // profile is made once and not edited): hovering asks every profile on each mouse move.
    #[serde(skip)]
    hull: OnceLock<Option<Aabb2>>,
    #[serde(skip)]
    outline: OnceLock<RegionOutline>,
}

impl Profile {
    /// Whole source boundaries, certified by complete graph-piece coverage. Intersections,
    /// missing spans and unknown sampled geometry cannot acquire this persistent identity.
    pub fn whole_curve_boundaries(&self) -> Option<(Vec<String>, Vec<Vec<String>>)> {
        Some((self.complete_outer.clone()?, self.complete_holes.clone()?))
    }

    /// Whether `p` is in the profile: what `region.contains(p)` answers, from the loops sampled
    /// once. A point clearly outside the outer loop's box ([`Loop2::hull_box`]) is answered
    /// without sampling anything, so the profiles away from the cursor cost a box test.
    pub fn contains(&self, p: Vec2) -> bool {
        if self.hull.get_or_init(|| self.region.outer.hull_box()).is_some_and(|b| b.clearly_excludes(p)) {
            return false;
        }
        self.outline.get_or_init(|| self.region.outline()).contains(p)
    }

    /// All curve ids bounding the profile.
    pub fn curve_ids(&self) -> Vec<String> {
        let mut v = self.outer_curves.clone();
        for h in &self.hole_curves {
            v.extend(h.iter().cloned());
        }
        v
    }
}

struct Edge {
    seg: Seg2,
    u: usize,
    v: usize,
    id: String,
    /// A piece of an exact free-form segment: (curve, segment, piece, pieces).
    tag: Option<Piece>,
    coverage: Option<Piece>,
    /// A piece of a line, arc or circle (not one sampled from a free-form curve).
    analytic: bool,
    /// Index of the sketch curve the piece is part of.
    curve: usize,
}

/// Do two circles lie within `tol` of each other all the way round?
fn same_circle(c: Vec2, r: f64, c2: Vec2, r2: f64, tol: f64) -> bool {
    c.dist(c2) + (r - r2).abs() <= tol
}

/// Which edges stay in the graph: of pieces lying on top of each other between the same two
/// nodes (a side drawn onto a projected edge, which the solver puts there only up to round-off),
/// the first, so the earliest curve in sketch order names the shared edge, as when the pieces
/// lie exactly on each other. Kept both, they would bound a sliver of no area, and the
/// round-off in their directions would make the turns at their ends disagree, so the regions on
/// both sides would be lost (#635).
fn without_overlaps(edges: &[Edge], tol: f64) -> Vec<bool> {
    // Between the same nodes, two pieces of lines run along one path, and so do two pieces of
    // arcs or circles on one circle through the same middle (not the two halves of a circle). A
    // line and an arc never do, nor does a sampled piece of a free-form curve: a chord and its
    // curve bound a region, however thin.
    let same_path = |a: &Edge, b: &Edge| match (a.seg, b.seg) {
        _ if !(a.analytic && b.analytic) => false,
        (Seg2::Line { .. }, Seg2::Line { .. }) => true,
        (Seg2::Arc { center, radius, .. }, Seg2::Arc { center: c2, radius: r2, .. }) => {
            same_circle(center, radius, c2, r2, tol) && a.seg.mid().dist(b.seg.mid()) <= 2.0 * tol
        }
        _ => false,
    };
    let mut kept: std::collections::HashMap<(usize, usize), Vec<usize>> = std::collections::HashMap::new();
    edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let between = kept.entry((e.u.min(e.v), e.u.max(e.v))).or_default();
            let first = !between.iter().any(|k| edges.get(*k).is_some_and(|f| same_path(f, e)));
            if first {
                between.push(i);
            }
            first
        })
        .collect()
}

type Piece = (usize, usize, usize, usize);

/// Prove each represented source uses all its original spans exactly once and consistently
/// in one direction. Work and storage are bounded by the existing graph-edge budget.
fn complete_loop(tags: &[Option<(Piece, bool)>], expected: &std::collections::HashMap<usize, Vec<usize>>, sk: &Sketch) -> Option<Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    if tags.is_empty() || tags.len() > MAX_EDGES {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut directions = BTreeMap::new();
    let mut counts = BTreeMap::<usize, Vec<(usize, usize)>>::new();
    let mut previous_curve = tags.last().copied().flatten()?.0.0;
    let mut runs = BTreeSet::new();
    for tag in tags {
        let ((ci, segment, piece, pieces), forward) = (*tag)?;
        let expected_pieces = *expected.get(&ci)?.get(segment)?;
        if pieces == 0 || pieces != expected_pieces || piece >= pieces || !seen.insert((ci, segment, piece)) {
            return None;
        }
        if directions.insert(ci, forward).is_some_and(|old| old != forward) {
            return None;
        }
        if previous_curve != ci && !runs.insert(ci) {
            return None;
        }
        previous_curve = ci;
        counts.entry(ci).or_default().push((segment, piece));
    }
    let mut ids = Vec::with_capacity(counts.len());
    for (ci, order) in counts {
        let spans = expected.get(&ci)?;
        let total = spans.iter().try_fold(0usize, |sum, n| sum.checked_add(*n))?;
        if spans.is_empty() || spans.contains(&0) || total > MAX_EDGES || total != order.len() {
            return None;
        }
        let mut offsets = Vec::with_capacity(spans.len());
        let mut offset = 0;
        for count in spans {
            offsets.push(offset);
            offset += count;
        }
        let forward = *directions.get(&ci)?;
        let indices = order.iter().map(|(segment, piece)| offsets[*segment] + piece).collect::<Vec<_>>();
        if indices.windows(2).any(|w| w[1] != if forward { (w[0] + 1) % total } else { (w[0] + total - 1) % total }) {
            return None;
        }
        ids.push(sk.curves.get(ci)?.id.clone());
    }
    ids.sort();
    if ids.windows(2).any(|w| w[0] == w[1]) {
        return None;
    }
    Some(ids)
}

/// Do two shapes of lines, arcs or circles lie on one line or circle (within `tol` all along)?
/// Where they overlap they don't cross: the points where one ends on the other cut them (each
/// end lies within `tol` of the other). Their crossing, computed from round-off alone, would
/// land anywhere along them and cut both apart there. Two whole circles have no ends to cut
/// them, so they keep their crossings.
fn overlapping(x: &Shape, y: &Shape, tol: f64) -> bool {
    let near_line = |a: Vec2, b: Vec2, ps: [Vec2; 2]| (b - a).normalized().is_some_and(|d| ps.iter().all(|p| (*p - a).cross(d).abs() <= tol));
    match (*x, *y) {
        (Shape::Line { a, b }, Shape::Line { a: c, b: d }) => near_line(a, b, [c, d]) && near_line(c, d, [a, b]),
        (Shape::Round { c, r, .. }, Shape::Round { c: c2, r: r2, .. }) => !(x.is_full() && y.is_full()) && same_circle(c, r, c2, r2, tol),
        _ => false,
    }
}

/// Parameter of `p` at an end of the shape (lines: 0 or 1), for intersections at end points.
fn end_param(sh: &Shape, p: Vec2, tol: f64) -> Option<f64> {
    match *sh {
        Shape::Line { a, b } => {
            if p.dist(a) <= tol {
                Some(0.0)
            } else if p.dist(b) <= tol {
                Some(1.0)
            } else {
                None
            }
        }
        Shape::Round { c, r, start, sweep } => {
            let s0 = c + Vec2::from_angle(start) * r;
            let s1 = c + Vec2::from_angle(start + sweep) * r;
            (p.dist(s0) <= tol || p.dist(s1) <= tol).then_some(0.0)
        }
    }
}

/// Put exact free-form segments back. A loop that runs over all the pieces of one, in order,
/// gets the whole segment; one that runs over part of it (cut by a crossing) gets the exact
/// sub-segment, its ends moved to the true crossing with the neighbouring segment. Where that
/// crossing cannot be found the loop keeps the polyline pieces.
fn restore_exact(segs: Vec<Seg2>, tags: &[Option<(Piece, bool)>], exact: &std::collections::HashMap<usize, Vec<Seg2>>) -> Vec<Seg2> {
    let n = segs.len();
    if n == 0 || tags.len() != n || tags.iter().all(Option::is_none) {
        return segs;
    }
    let key = |i: usize| tags.get(i % n).copied().flatten().map(|((ci, k, _, _), fwd)| (ci, k, fwd));
    // Start where a run begins so no run wraps around.
    let r = (0..n).find(|i| key(*i) != key(*i + n - 1)).unwrap_or(0);
    let mut runs: Vec<Run> = Vec::new();
    let mut i = 0;
    while i < n {
        let k0 = key(r + i);
        let mut j = i + 1;
        while j < n && key(r + j) == k0 && k0.is_some() {
            j += 1;
        }
        let run: Vec<usize> = (i..j).map(|x| (r + x) % n).collect();
        let plain: Vec<Seg2> = run.iter().filter_map(|x| segs.get(*x).copied()).collect();
        let full = k0.and_then(|(ci, k, fwd)| exact.get(&ci).and_then(|e| e.get(k)).map(|s| if fwd { *s } else { s.reversed() }));
        let whole = match (full, tags.get(run[0]).copied().flatten()) {
            (Some(_), Some(((_, _, _, cnt), fwd))) if run.len() == cnt => {
                let order: Vec<usize> = run.iter().filter_map(|x| tags.get(*x).copied().flatten().map(|((_, _, p, _), _)| p)).collect();
                let want: Vec<usize> = if fwd { (0..cnt).collect() } else { (0..cnt).rev().collect() };
                order == want
            }
            _ => false,
        };
        runs.push(match full {
            Some(seg) if whole => Run::Exact { seg, t0: 0.0, t1: 1.0, fixed: (true, true), plain },
            Some(seg) => {
                let (a, b) = (plain.first().map(Seg2::start).unwrap_or_default(), plain.last().map(Seg2::end).unwrap_or_default());
                let (t0, t1) = (seg.closest_param(a), seg.closest_param(b));
                // Ends that are the segment's own ends need no search.
                let snap = |t: f64, p: Vec2, at: f64| (t - at).abs() < 1e-6 && seg.point_at(at).dist(p) < 1e-6;
                let fixed = (snap(t0, a, 0.0), snap(t1, b, 1.0));
                let (t0, t1) = (if fixed.0 { 0.0 } else { t0 }, if fixed.1 { 1.0 } else { t1 });
                if t1 - t0 > 1e-9 { Run::Exact { seg, t0, t1, fixed, plain } } else { Run::Plain(plain) }
            }
            None => Run::Plain(plain),
        });
        i = j;
    }
    // Junctions next to a cut exact run: find the true crossing.
    let m = runs.len();
    for a in 0..m {
        let b = (a + 1) % m;
        let need = match (runs.get(a), runs.get(b)) {
            (Some(Run::Exact { fixed, .. }), _) if !fixed.1 => true,
            (_, Some(Run::Exact { fixed, .. })) if !fixed.0 => true,
            _ => false,
        };
        if !need {
            continue;
        }
        let (Some(ra), Some(rb)) = (runs.get(a), runs.get(b)) else { continue };
        let p0 = ra.end();
        let crossing = refine_crossing(ra, rb, p0);
        match crossing {
            Some((p, ta, tb)) if p.dist(p0) < 1e-2 => {
                if let Some(x) = runs.get_mut(a) {
                    x.set_end(p, ta);
                }
                if let Some(x) = runs.get_mut(b) {
                    x.set_start(p, tb);
                }
            }
            _ => {
                for k in [a, b] {
                    if let Some(x) = runs.get_mut(k) {
                        x.give_up();
                    }
                }
            }
        }
    }
    runs.into_iter().flat_map(Run::into_segs).collect()
}

/// A stretch of a profile loop: plain segments, or (part of) an exact free-form segment.
enum Run {
    Plain(Vec<Seg2>),
    /// `seg` (in loop direction) between parameters `t0` and `t1`; `fixed` ends are exact;
    /// `plain` is the polyline fallback.
    Exact {
        seg: Seg2,
        t0: f64,
        t1: f64,
        fixed: (bool, bool),
        plain: Vec<Seg2>,
    },
}

impl Run {
    fn end(&self) -> Vec2 {
        match self {
            Run::Plain(v) => v.last().map(Seg2::end).unwrap_or_default(),
            Run::Exact { seg, t1, .. } => seg.point_at(*t1),
        }
    }
    /// The segment at the run's end and the parameter of the end on it.
    fn end_geom(&self) -> Option<(Seg2, f64)> {
        match self {
            Run::Plain(v) => v.last().map(|s| (*s, 1.0)),
            Run::Exact { seg, t1, .. } => Some((*seg, *t1)),
        }
    }
    /// The segment at the run's start and the parameter of the start on it.
    fn start_geom(&self) -> Option<(Seg2, f64)> {
        match self {
            Run::Plain(v) => v.first().map(|s| (*s, 0.0)),
            Run::Exact { seg, t0, .. } => Some((*seg, *t0)),
        }
    }
    fn set_end(&mut self, p: Vec2, t: f64) {
        match self {
            Run::Plain(v) => {
                if let Some(s) = v.last_mut() {
                    *s = with_end(*s, p);
                }
            }
            Run::Exact { t1, .. } => *t1 = t,
        }
    }
    fn set_start(&mut self, p: Vec2, t: f64) {
        match self {
            Run::Plain(v) => {
                if let Some(s) = v.first_mut() {
                    *s = with_end(s.reversed(), p).reversed();
                }
            }
            Run::Exact { t0, .. } => *t0 = t,
        }
    }
    fn give_up(&mut self) {
        if let Run::Exact { fixed: (false, _) | (_, false), plain, .. } = self {
            *self = Run::Plain(std::mem::take(plain));
        }
    }
    fn into_segs(self) -> Vec<Seg2> {
        match self {
            Run::Plain(v) => v,
            Run::Exact { seg, t0, t1, .. } => {
                if t0 <= 1e-12 && t1 >= 1.0 - 1e-12 {
                    return vec![seg];
                }
                let s = if t1 < 1.0 - 1e-12 { seg.split_at(t1).0 } else { seg };
                if t0 <= 1e-12 {
                    return vec![s];
                }
                // Conic halves are reparametrised: find t0's point again on the left part.
                let u = local_param(&s, seg.point_at(t0), if t1 > 1e-12 { t0 / t1 } else { 0.0 });
                vec![s.split_at(u.clamp(1e-9, 1.0 - 1e-9)).1]
            }
        }
    }
}

/// The segment with its end moved to `p` (lines; arcs keep their circle).
fn with_end(s: Seg2, p: Vec2) -> Seg2 {
    match s {
        Seg2::Line { a, .. } => Seg2::Line { a, b: p },
        Seg2::Arc { center, radius, start, sweep } => {
            let target = (p - center).angle();
            let end = start + sweep;
            // The same end angle, wound the same way.
            let delta = (target - end + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
            Seg2::Arc { center, radius, start, sweep: sweep + delta }
        }
        other => other,
    }
}

fn set_start(s: Seg2, p: Vec2) -> Seg2 {
    with_end(s.reversed(), p).reversed()
}

fn join_point(a: Seg2, b: Seg2, tol: f64) -> Option<Vec2> {
    match (a, b) {
        (Seg2::Line { .. }, Seg2::Line { .. }) => Some((a.end() + b.start()) * 0.5),
        (Seg2::Line { .. }, Seg2::Arc { center, radius, .. }) | (Seg2::Arc { center, radius, .. }, Seg2::Line { .. }) => {
            let near = if matches!(a, Seg2::Arc { .. }) { a.end() } else { b.start() };
            Some(center + Vec2::from_angle((near - center).angle()) * radius)
        }
        (Seg2::Arc { center, radius, .. }, Seg2::Arc { center: c2, radius: r2, .. }) if same_circle(center, radius, c2, r2, MERGE_TOL) => {
            let near = (a.end() + b.start()) * 0.5;
            Some(center + Vec2::from_angle((near - center).angle()) * radius)
        }
        (Seg2::Arc { center, radius, .. }, Seg2::Arc { center: c2, radius: r2, .. }) => intersections_within(
            &Shape::Round { c: center, r: radius, start: 0.0, sweep: TAU },
            &Shape::Round { c: c2, r: r2, start: 0.0, sweep: TAU },
            tol,
        )
        .into_iter()
        .filter(|p| p.dist((a.end() + b.start()) * 0.5) <= tol * 2.0)
        .min_by(|p, q| p.dist((a.end() + b.start()) * 0.5).total_cmp(&q.dist((a.end() + b.start()) * 0.5))),
        _ => None,
    }
}

fn close_graph_joints(mut segs: Vec<Seg2>, edge_nodes: &[(usize, usize)], edge_analytic: &[bool], tol: f64) -> Vec<Seg2> {
    let n = segs.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (Some((_, end_node)), Some((start_node, _))) = (edge_nodes.get(i), edge_nodes.get(j)) else { continue };
        if end_node != start_node || !edge_analytic.get(i).copied().unwrap_or(false) || !edge_analytic.get(j).copied().unwrap_or(false) {
            continue;
        }
        let (Some(a), Some(b)) = (segs.get(i).copied(), segs.get(j).copied()) else { continue };
        if a.end().dist(b.start()) <= 1e-12 || a.end().dist(b.start()) > tol {
            continue;
        }
        let Some(point) = join_point(a, b, tol) else { continue };
        let (a, b) = match (a, b) {
            (Seg2::Line { .. }, Seg2::Line { .. }) => (with_end(a, point), set_start(b, point)),
            (Seg2::Line { .. }, Seg2::Arc { .. }) => (with_end(a, b.start()), b),
            (Seg2::Arc { .. }, Seg2::Line { .. }) => (a, with_end(b.reversed(), a.end()).reversed()),
            (Seg2::Arc { .. }, Seg2::Arc { .. }) => (with_end(a, point), set_start(b, point)),
            _ => (a, b),
        };
        if let Some(slot) = segs.get_mut(i) {
            *slot = a;
        }
        if let Some(slot) = segs.get_mut(j) {
            *slot = b;
        }
    }
    segs
}

/// Close the joints a line leaves open: the graph joins ends within `tol` into one node, but a
/// free-form curve's polyline pieces keep their own ends, so two near-coincident curves can leave
/// a loop open by more than the closing tolerance. A line meeting a line moves both ends to their
/// middle; a line meeting another segment moves its end onto that segment's.
fn close_line_joints(mut segs: Vec<Seg2>, tol: f64) -> Vec<Seg2> {
    let n = segs.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (Some(a), Some(b)) = (segs.get(i).copied(), segs.get(j).copied()) else { continue };
        let gap = a.end().dist(b.start());
        if gap <= 1e-12 || gap > tol {
            continue;
        }
        let (a, b) = match (a, b) {
            (Seg2::Line { .. }, Seg2::Line { .. }) => {
                let p = (a.end() + b.start()) * 0.5;
                (with_end(a, p), set_start(b, p))
            }
            (Seg2::Line { .. }, _) => (with_end(a, b.start()), b),
            (_, Seg2::Line { .. }) => (a, set_start(b, a.end())),
            _ => continue,
        };
        if let Some(slot) = segs.get_mut(i) {
            *slot = a;
        }
        if let Some(slot) = segs.get_mut(j) {
            *slot = b;
        }
    }
    segs
}

/// Joins line and arc ends that lie close together. Returns false when there were too many ends
/// close together to look at them all (nothing is joined then).
fn merge_nearby_endpoints(sk: &Sketch, endpoints: &[(usize, usize)], groups: &mut [usize], max_edges: usize) -> bool {
    let tol = ENDPOINT_JOIN_TOL;
    let cell = |p: Vec2| ((p.x / tol).floor() as i64, (p.y / tol).floor() as i64);
    let mut curve_endpoints = std::collections::HashMap::<usize, Vec<usize>>::new();
    for &(point_index, curve_index) in endpoints {
        curve_endpoints.entry(curve_index).or_default().push(point_index);
    }
    let connected_endpoints: std::collections::HashSet<(usize, usize)> = curve_endpoints
        .values()
        .filter_map(|points| {
            let [a, b] = points.as_slice() else { return None };
            Some(((*a).min(*b), (*a).max(*b)))
        })
        .collect();
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    let mut joins = std::collections::HashMap::<(usize, usize), f64>::new();
    let mut checks = 0usize;
    let check_limit = max_edges.saturating_mul(16).max(16);
    let mut exceeded_limit = false;
    'points: for &(point_index, _) in endpoints {
        let Some(point) = sk.point(point_index).filter(|point| point.is_finite()) else { continue };
        let (cx, cy) = cell(point);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let (Some(x), Some(y)) = (cx.checked_add(dx), cy.checked_add(dy)) else { continue };
                if let Some(candidates) = grid.get(&(x, y)) {
                    for &other_index in candidates {
                        if other_index == point_index {
                            continue;
                        }
                        checks += 1;
                        if checks > check_limit {
                            exceeded_limit = true;
                            break 'points;
                        }
                        if let Some(distance) = sk.point(other_index).map(|other| point.dist(other)).filter(|distance| *distance <= tol) {
                            let pair = (point_index.min(other_index), point_index.max(other_index));
                            joins.entry(pair).and_modify(|best| *best = best.min(distance)).or_insert(distance);
                        }
                    }
                }
            }
        }
        grid.entry((cx, cy)).or_default().push(point_index);
    }
    if !exceeded_limit {
        let mut nearest = std::collections::HashMap::<usize, (f64, Option<usize>)>::new();
        let tie_tolerance = MERGE_TOL * 1e-6;
        for (&(a, b), &distance) in &joins {
            for (point, other) in [(a, b), (b, a)] {
                match nearest.entry(point).or_insert((f64::INFINITY, None)) {
                    (best, closest) if distance < *best - tie_tolerance => {
                        *best = distance;
                        *closest = Some(other);
                    }
                    (best, closest) if (distance - *best).abs() <= tie_tolerance => *closest = None,
                    _ => {}
                }
            }
        }
        for &(a, b) in joins.keys() {
            if connected_endpoints.contains(&(a, b)) {
                continue;
            }
            let a_is_unique_nearest = nearest.get(&a).is_some_and(|(_, closest)| *closest == Some(b));
            let b_is_unique_nearest = nearest.get(&b).is_some_and(|(_, closest)| *closest == Some(a));
            if a_is_unique_nearest && b_is_unique_nearest {
                union(groups, a, b);
            }
        }
    }
    !exceeded_limit
}

/// Where the end of run `a` meets the start of run `b`, near `p`: the point and the parameters
/// on the two runs' segments (Newton on A(ta) = B(tb); lines and arcs extend past their ends).
fn refine_crossing(a: &Run, b: &Run, p: Vec2) -> Option<(Vec2, f64, f64)> {
    let (sa, mut ta) = a.end_geom()?;
    let (sb, mut tb) = b.start_geom()?;
    // Start from the parameters nearest the polyline crossing.
    ta = local_param(&sa, p, ta);
    tb = local_param(&sb, p, tb);
    for _ in 0..50 {
        let f = sa.point_at(ta) - sb.point_at(tb);
        if f.len() < 1e-11 {
            let pt = match (a, b) {
                (Run::Exact { .. }, _) | (Run::Plain(_), Run::Plain(_)) => sa.point_at(ta),
                (_, Run::Exact { .. }) => sb.point_at(tb),
            };
            return (ta.is_finite() && tb.is_finite()).then_some((pt, ta, tb));
        }
        let (da, db) = (sa.derivative(ta), -sb.derivative(tb));
        let det = da.cross(db);
        if det.abs() < 1e-14 * da.len().max(1e-300) * db.len().max(1e-300) {
            return None;
        }
        // Solve [da db] (dta, dtb)ᵀ = −f.
        ta -= f.cross(db) / det;
        tb -= da.cross(f) / det;
    }
    None
}

/// Parameter of the point of `s` nearest `p`, by Newton from `t`.
fn local_param(s: &Seg2, p: Vec2, mut t: f64) -> f64 {
    for _ in 0..30 {
        let (q, d) = (s.point_at(t), s.derivative(t));
        let dd = d.dot(d);
        if dd < 1e-300 {
            break;
        }
        let step = (p - q).dot(d) / dd;
        t += step;
        if step.abs() < 1e-15 {
            break;
        }
    }
    t
}

/// A uniform grid over a set of boxes (about one box per cell), for neighbour queries.
struct Grid {
    lo: Vec2,
    size: f64,
}

impl Grid {
    fn new(boxes: &[solidcraft_geom::Aabb2]) -> Grid {
        let all = boxes.iter().fold(solidcraft_geom::Aabb2::EMPTY, |a, b| a.union(b));
        let (lo, hi) = (all.min, all.max);
        if !(lo.is_finite() && hi.is_finite()) {
            return Grid { lo: Vec2::ZERO, size: 1.0 };
        }
        let ext = (hi.x - lo.x).max(hi.y - lo.y).max(1e-9);
        let per_side = (boxes.len() as f64).sqrt().ceil().clamp(1.0, 512.0);
        Grid { lo, size: ext / per_side }
    }
    fn cell(&self, p: Vec2) -> (i64, i64) {
        (((p.x - self.lo.x) / self.size).floor() as i64, ((p.y - self.lo.y) / self.size).floor() as i64)
    }
    /// Cells a box covers; None for boxes spanning too many (they are checked against all),
    /// including infinite or huge boxes whose cell span does not fit in an `i64`.
    fn cells(&self, b: &solidcraft_geom::Aabb2) -> Option<Vec<(i64, i64)>> {
        if !(b.min.is_finite() && b.max.is_finite()) {
            return None;
        }
        let (c0, c1) = (self.cell(b.min), self.cell(b.max));
        let span = |lo: i64, hi: i64| hi.checked_sub(lo)?.checked_add(1);
        let count = span(c0.0, c1.0)?.checked_mul(span(c0.1, c1.1)?)?;
        if count > 2048 {
            return None;
        }
        let mut out = Vec::new();
        for x in c0.0..=c1.0 {
            for y in c0.1..=c1.1 {
                out.push((x, y));
            }
        }
        Some(out)
    }
}

/// Signed curvature at the start of a segment (left turns positive).
fn curvature(s: &Seg2) -> f64 {
    match *s {
        Seg2::Line { .. } => 0.0,
        Seg2::Arc { radius, sweep, .. } => sweep.signum() / radius.max(1e-12),
        Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
            let (d1, h) = (s.derivative(0.0), 1e-5);
            let d2 = (s.derivative(h) - d1) / h;
            d1.cross(d2) / d1.len().powi(3).max(1e-300)
        }
    }
}

/// Profiles of the drawn geometry only: linked reference curves (projections, intersections)
/// are left out; text outlines count as drawn.
pub fn find_drawn_profiles(sk: &Sketch) -> Vec<Profile> {
    if sk.links.iter().all(|l| l.kind == crate::LinkKind::Text) {
        return find_profiles(sk);
    }
    let intersect_links: std::collections::HashSet<&str> =
        sk.links.iter().filter(|link| link.kind == crate::LinkKind::Intersect).map(|link| link.id.as_str()).collect();
    let mut d = sk.clone();
    for (ci, c) in d.curves.iter_mut().enumerate() {
        if c.link.as_ref().is_some_and(|link| !intersect_links.contains(link.as_str())) && !sk.is_text_curve(ci) {
            c.construction = true;
        }
    }
    find_profiles(&d)
}

/// The profiles without text counters: a profile bounded by text outlines alone that lies inside
/// an odd number of other such profiles is the counter of a letter (the hole of A, B, 8...),
/// which the letter's own profile already has as a hole. What "all profiles" means for text:
/// the letters, not the letters with their counters filled in.
pub fn without_text_counters(sk: &Sketch, profiles: Vec<Profile>) -> Vec<Profile> {
    let index: std::collections::HashMap<&str, usize> = sk.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let text_only: Vec<bool> = profiles
        .iter()
        .map(|p| !p.outer_curves.is_empty() && p.outer_curves.iter().all(|id| index.get(id.as_str()).is_some_and(|ci| sk.is_text_curve(*ci))))
        .collect();
    if !text_only.iter().any(|t| *t) {
        return profiles;
    }
    // Outer polylines and their boxes, once.
    let polys: Vec<Ring<Vec2>> = profiles.iter().map(|p| p.region.outer.polyline(1e-3)).collect();
    let boxes: Vec<solidcraft_geom::Aabb2> = polys.iter().map(|pl| pl.bounds()).collect();
    let areas: Vec<f64> = profiles.iter().map(|p| p.region.outer.signed_area().abs()).collect();
    let keep: Vec<bool> = (0..profiles.len())
        .map(|i| {
            if !text_only.get(i).copied().unwrap_or(false) {
                return true;
            }
            let Some(probe) = profiles.get(i).and_then(|p| p.region.outer.segs.first()).map(Seg2::mid) else { return true };
            let ai = areas.get(i).copied().unwrap_or(0.0);
            let depth = (0..profiles.len())
                .filter(|&j| {
                    j != i
                        && text_only.get(j).copied().unwrap_or(false)
                        && areas.get(j).copied().unwrap_or(0.0) > ai
                        && boxes.get(j).is_some_and(|b| b.contains(probe))
                        && polys.get(j).is_some_and(|pl| pl.contains(probe))
                })
                .count();
            depth % 2 == 0
        })
        .collect();
    profiles.into_iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| p).collect()
}

/// Find all closed profiles of a sketch. Curves are split where other curves end on them or
/// cross them, like a drawing would be read.
pub fn find_profiles(sk: &Sketch) -> Vec<Profile> {
    find_profiles_checked(sk).profiles
}

/// The profiles of a sketch, and whether some were possibly left out.
#[derive(Clone, Debug, Default)]
pub struct ProfileSearch {
    pub profiles: Vec<Profile>,
    /// A group of touching curves was too large to search completely (more than
    /// [`MAX_PROFILE_EDGES`] pieces), so some of its profiles may be missing or not split where
    /// curves cross. The caller should say so rather than drop profiles silently.
    pub incomplete: bool,
}

/// Pieces one group of touching curves may be cut into before its profile search stops.
pub const MAX_PROFILE_EDGES: usize = MAX_EDGES;

/// [`find_profiles`], also telling whether the search was cut short.
///
/// The search runs island by island: curves whose (slightly enlarged) bounds do not touch any
/// curve of another group cannot cut, join or overlap it, so each group's loops are found on
/// their own, and remembered by a fingerprint of the group's geometry. Dragging one letter of a
/// large imported logo then traces only that letter again (#6896). The loops are nested into
/// profiles over the whole sketch afterwards, so a contour inside another one without touching
/// it still becomes its hole, and the profiles, their order and their curve ids are the ones a
/// single search over the whole sketch finds.
pub fn find_profiles_checked(sk: &Sketch) -> ProfileSearch {
    search(sk, MAX_EDGES, true, true)
}

#[cfg(test)]
fn find_profiles_bounded(sk: &Sketch, max_edges: usize) -> Vec<Profile> {
    search(sk, max_edges, true, true).profiles
}

fn search(sk: &Sketch, max_edges: usize, split: bool, cached: bool) -> ProfileSearch {
    let curves: Vec<usize> = sk.curves.iter().enumerate().filter(|(_, c)| !c.construction && !c.centerline && !c.infinite).map(|(i, _)| i).collect();
    let groups = if split { islands(sk, &curves) } else { vec![curves] };
    let found: Vec<Arc<Island>> = groups
        .iter()
        .map(|group| {
            if !cached {
                return Arc::new(island_loops(sk, group, max_edges));
            }
            let key = fingerprint(sk, group, max_edges);
            if let Some(hit) = island_cache::get(key) {
                return hit;
            }
            let island = Arc::new(island_loops(sk, group, max_edges));
            island_cache::put(key, &island);
            island
        })
        .collect();
    ProfileSearch { incomplete: found.iter().any(|i| i.incomplete), profiles: assemble(sk, &found) }
}

/// One island's closed loops, and whether its search was cut short.
struct Island {
    loops: Vec<IslandLoop>,
    incomplete: bool,
}

struct IslandLoop {
    lp: Loop2,
    ids: Vec<String>,
    /// Connected component within the island (loops of one component never nest).
    comp: usize,
    complete: Option<Vec<String>>,
    /// Place in the order a search over the whole sketch finds loops: (0, circle curve, 0) for a
    /// whole circle, (1, curve of the first edge, first half-edge) for a traced face.
    key: (u8, usize, usize),
    bounds: solidcraft_geom::Aabb2,
    area: f64,
}

impl IslandLoop {
    fn new(lp: Loop2, ids: Vec<String>, comp: usize, complete: Option<Vec<String>>, key: (u8, usize, usize)) -> IslandLoop {
        let bounds = lp.polyline(1e-3).bounds();
        let area = lp.signed_area().abs();
        IslandLoop { lp, ids, comp, complete, key, bounds, area }
    }

    /// Rough size, for bounding the cache.
    fn weight(&self) -> usize {
        self.lp.segs.len() + self.ids.len() + 1
    }
}

/// How far apart two curves' pieces must be to search them separately: well past every
/// tolerance the search uses (end joins at `ENDPOINT_JOIN_TOL`, crossings next to an end within
/// `100 * tol` = 1e-3, node merging at `tol`).
const ISLAND_MARGIN: f64 = 2e-3;
/// Box pairs looked at when splitting into islands before giving up and searching the sketch
/// as one island (many long curves lying across each other).
const ISLAND_PAIR_BUDGET: usize = 4_000_000;

/// Groups of profile curves (each in sketch order, groups by their first curve) that can be
/// searched separately: the boxes of their pieces, enlarged by [`ISLAND_MARGIN`], touch no box
/// of another group. Every box holds everything the search derives from its curve (the end
/// points, and the sampled points of free-form segments, which lie in their control polygon).
fn islands(sk: &Sketch, curves: &[usize]) -> Vec<Vec<usize>> {
    let mut boxes: Vec<(usize, solidcraft_geom::Aabb2)> = Vec::new();
    for (k, &ci) in curves.iter().enumerate() {
        for b in piece_boxes(sk, ci) {
            if !(b.min.is_finite() && b.max.is_finite()) {
                return vec![curves.to_vec()];
            }
            boxes.push((k, b.inflated(ISLAND_MARGIN)));
        }
    }
    // Sweep along x: a box meets only boxes still open when it starts.
    boxes.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
    let mut parent: Vec<usize> = (0..curves.len()).collect();
    let mut open: Vec<usize> = Vec::new();
    let mut pairs = 0usize;
    for (i, (k, b)) in boxes.iter().enumerate() {
        open.retain(|&j| boxes.get(j).is_some_and(|(_, o)| o.max.x >= b.min.x));
        pairs += open.len();
        if pairs > ISLAND_PAIR_BUDGET {
            return vec![curves.to_vec()];
        }
        for &j in &open {
            if let Some((kj, o)) = boxes.get(j)
                && o.meets(b)
            {
                union(&mut parent, *k, *kj);
            }
        }
        open.push(i);
    }
    let mut group_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (k, &ci) in curves.iter().enumerate() {
        let root = find(&mut parent, k);
        let g = *group_of.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        if let Some(group) = groups.get_mut(g) {
            group.push(ci);
        }
    }
    groups
}

/// Boxes covering a profile curve: its end points, and each of its pieces (lines and arcs in a
/// few parts, so a long diagonal line or a large circle around other curves does not cover them).
fn piece_boxes(sk: &Sketch, ci: usize) -> Vec<solidcraft_geom::Aabb2> {
    let Some(c) = sk.curves.get(ci) else { return Vec::new() };
    let mut out = Vec::new();
    let point_box = |p: Vec2| solidcraft_geom::Aabb2 { min: p, max: p };
    if let Some((a, b)) = c.kind.ends() {
        out.extend([a, b].into_iter().filter_map(|i| sk.point(i)).map(point_box));
    }
    match c.kind {
        CurveKind::Line { a, b } => {
            if let (Some(a), Some(b)) = (sk.point(a), sk.point(b)) {
                seg_boxes(&Seg2::Line { a, b }, &mut out);
            }
        }
        CurveKind::Circle { c, r } => {
            if let Some(c) = sk.point(c) {
                seg_boxes(&Seg2::Arc { center: c, radius: r, start: 0.0, sweep: TAU }, &mut out);
            }
        }
        CurveKind::Arc { .. } => {
            for s in sk.segs(ci).iter().take(1) {
                seg_boxes(s, &mut out);
            }
        }
        _ => match sk.exact_segs(ci) {
            Some(ex) => ex.iter().for_each(|s| seg_boxes(s, &mut out)),
            None => sk.segs(ci).iter().for_each(|s| seg_boxes(s, &mut out)),
        },
    }
    out
}

/// Boxes covering a segment and everything the search samples from it.
fn seg_boxes(s: &Seg2, out: &mut Vec<solidcraft_geom::Aabb2>) {
    const PARTS: usize = 16;
    let hull = |pts: &[Vec2]| {
        pts.iter().fold(solidcraft_geom::Aabb2::EMPTY, |mut b, p| {
            b.add(*p);
            b
        })
    };
    match *s {
        Seg2::Line { a, b } => {
            for k in 0..PARTS {
                let (t0, t1) = (k as f64 / PARTS as f64, (k + 1) as f64 / PARTS as f64);
                out.push(hull(&[a + (b - a) * t0, a + (b - a) * t1]));
            }
        }
        Seg2::Arc { center, radius, start, sweep } => {
            // Parts of at most TAU / PARTS; each lies within its chord's box grown by its sagitta.
            let r = radius.abs();
            let n = ((sweep.abs() / (TAU / PARTS as f64)).ceil() as usize).clamp(1, PARTS);
            let step = sweep / n as f64;
            let sag = r * (1.0 - (step.abs() * 0.5).cos()) + r * 1e-12;
            let at = |t: f64| center + Vec2::new(t.cos(), t.sin()) * r;
            for k in 0..n {
                let (t0, t1) = (start + step * k as f64, start + step * (k + 1) as f64);
                out.push(hull(&[at(t0), at(t1)]).inflated(sag));
            }
        }
        Seg2::Cubic { p0, p1, p2, p3 } => out.push(hull(&[p0, p1, p2, p3])),
        Seg2::Conic { a, apex, b, w } if w.is_finite() && w > 0.0 => out.push(hull(&[a, apex, b])),
        Seg2::Bezier { n, ref p } if (1..=solidcraft_geom::MAX_BEZIER_DEGREE).contains(&(n as usize)) => {
            out.push(hull(p.get(..=n as usize).unwrap_or(p)));
        }
        // Anything else: the very points the search samples.
        _ => out.push(hull(&s.polyline(1e-3))),
    }
}

/// A fingerprint of everything [`island_loops`] reads for a group of curves: their places in
/// the sketch, ids, kinds, end points and geometry.
fn fingerprint(sk: &Sketch, curves: &[usize], max_edges: usize) -> u128 {
    use std::hash::{Hash, Hasher};
    let mut h = [std::collections::hash_map::DefaultHasher::new(), std::collections::hash_map::DefaultHasher::new()];
    1u8.hash(&mut h[1]);
    let mut put = |f: &dyn Fn(&mut std::collections::hash_map::DefaultHasher)| h.iter_mut().for_each(f);
    put(&|x| (max_edges, curves.len()).hash(x));
    let pt = |x: &mut std::collections::hash_map::DefaultHasher, p: Option<Vec2>| match p {
        Some(p) => (1u8, p.x.to_bits(), p.y.to_bits()).hash(x),
        None => 0u8.hash(x),
    };
    for &ci in curves {
        let Some(c) = sk.curves.get(ci) else {
            put(&|x| (ci, u8::MAX).hash(x));
            continue;
        };
        put(&|x| (ci, &c.id, c.construction, c.centerline, c.infinite, std::mem::discriminant(&c.kind)).hash(x));
        if let Some((a, b)) = c.kind.ends() {
            put(&|x| {
                (a, b).hash(x);
                pt(x, sk.point(a));
                pt(x, sk.point(b));
            });
        }
        let segs: Vec<Seg2> = match c.kind {
            CurveKind::Line { .. } => Vec::new(),
            CurveKind::Circle { c, r } => {
                put(&|x| {
                    pt(x, sk.point(c));
                    r.to_bits().hash(x);
                });
                Vec::new()
            }
            CurveKind::Arc { .. } => sk.segs(ci).into_iter().take(1).collect(),
            _ => match sk.exact_segs(ci) {
                Some(ex) => {
                    put(&|x| 1u8.hash(x));
                    ex
                }
                None => {
                    put(&|x| 0u8.hash(x));
                    sk.segs(ci)
                }
            },
        };
        for s in &segs {
            put(&|x| hash_seg(s, x));
        }
    }
    let [a, b] = h;
    (u128::from(a.finish()) << 64) | u128::from(b.finish())
}

fn hash_seg(s: &Seg2, x: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    let v = |x: &mut dyn std::hash::Hasher, p: Vec2| {
        x.write_u64(p.x.to_bits());
        x.write_u64(p.y.to_bits());
    };
    match *s {
        Seg2::Line { a, b } => {
            0u8.hash(x);
            v(x, a);
            v(x, b);
        }
        Seg2::Arc { center, radius, start, sweep } => {
            1u8.hash(x);
            v(x, center);
            (radius.to_bits(), start.to_bits(), sweep.to_bits()).hash(x);
        }
        Seg2::Cubic { p0, p1, p2, p3 } => {
            2u8.hash(x);
            [p0, p1, p2, p3].into_iter().for_each(|p| v(x, p));
        }
        Seg2::Conic { a, apex, b, w } => {
            3u8.hash(x);
            [a, apex, b].into_iter().for_each(|p| v(x, p));
            w.to_bits().hash(x);
        }
        Seg2::Bezier { n, ref p } => {
            4u8.hash(x);
            n.hash(x);
            p.iter().for_each(|q| v(x, *q));
        }
    }
}

/// Island results by fingerprint, shared by every search (live drag frames, evaluation, the
/// profile pickers). Bounded: it is emptied when it grows past its limits.
mod island_cache {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use super::Island;

    const MAX_ENTRIES: usize = 4096;
    const MAX_WEIGHT: usize = 250_000;

    #[derive(Default)]
    struct Cache {
        map: HashMap<u128, Arc<Island>>,
        weight: usize,
    }

    fn cache() -> &'static Mutex<Cache> {
        static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
        CACHE.get_or_init(Mutex::default)
    }

    pub(super) fn get(key: u128) -> Option<Arc<Island>> {
        cache().lock().ok()?.map.get(&key).cloned()
    }

    pub(super) fn put(key: u128, island: &Arc<Island>) {
        let Ok(mut c) = cache().lock() else { return };
        let weight: usize = island.loops.iter().map(super::IslandLoop::weight).sum::<usize>() + 1;
        if weight > MAX_WEIGHT {
            return;
        }
        if c.map.len() >= MAX_ENTRIES || c.weight.saturating_add(weight) > MAX_WEIGHT {
            c.map.clear();
            c.weight = 0;
        }
        if c.map.insert(key, Arc::clone(island)).is_none() {
            c.weight += weight;
        }
    }
}

/// The closed loops of one island of profile curves (`curves`, in sketch order), before they
/// are nested into profiles. Everything here only looks at curves of the island: curves of other
/// islands are too far away to cut, join or overlap any of them (see [`islands`]).
fn island_loops(sk: &Sketch, curves: &[usize], max_edges: usize) -> Island {
    let tol = MERGE_TOL * 10.0;
    let mut incomplete = false;
    let mut endpoint_groups: Vec<usize> = (0..sk.points.len()).collect();
    let mut curve_endpoints = Vec::new();
    for &curve_index in curves {
        if let Some(curve) = sk.curves.get(curve_index)
            && !curve.construction
            && !curve.centerline
            && !curve.infinite
            && matches!(curve.kind, CurveKind::Line { .. } | CurveKind::Arc { .. })
            && let Some((a, b)) = curve.kind.ends()
        {
            curve_endpoints.extend([(a, curve_index), (b, curve_index)]);
        }
    }
    incomplete |= !merge_nearby_endpoints(sk, &curve_endpoints, &mut endpoint_groups, max_edges);
    let endpoint_groups: Vec<usize> = (0..endpoint_groups.len()).map(|p| find(&mut endpoint_groups, p)).collect();
    // Lines, arcs and circles (free-form curves take part as sampled pieces).
    let analytic =
        |ci: usize| sk.curves.get(ci).is_some_and(|c| matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. } | CurveKind::Circle { .. }));
    // Analytic shapes of the profile curves.
    let mut shapes: Vec<(usize, Shape)> = Vec::new();
    let mut tags: Vec<Option<Piece>> = Vec::new();
    let mut exact: std::collections::HashMap<usize, Vec<Seg2>> = std::collections::HashMap::new();
    let mut whole_graph_complete = true;
    let mut expected = std::collections::HashMap::<usize, Vec<usize>>::new();
    for &ci in curves {
        let Some(c) = sk.curves.get(ci) else { continue };
        if c.construction || c.centerline || c.infinite {
            continue;
        }
        if matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. } | CurveKind::Circle { .. }) {
            expected.insert(ci, vec![1]);
        }
        match c.kind {
            CurveKind::Line { a, b } => {
                if let (Some(a), Some(b)) = (sk.point(a), sk.point(b))
                    && a.dist(b) > MERGE_TOL
                {
                    shapes.push((ci, Shape::Line { a, b }));
                }
            }
            CurveKind::Circle { c, r } => {
                if let Some(c) = sk.point(c) {
                    shapes.push((ci, Shape::Round { c, r, start: 0.0, sweep: TAU }));
                }
            }
            CurveKind::Arc { .. } => {
                if let Some(Seg2::Arc { center, radius, start, sweep }) = sk.segs(ci).into_iter().next() {
                    shapes.push((ci, Shape::Round { c: center, r: radius, start, sweep }));
                }
            }
            // Free-form curves take part as polylines of their exact segments; loops that use
            // a whole segment get the exact one back.
            _ => match sk.exact_segs(ci) {
                Some(ex) => {
                    let mut spans = Vec::with_capacity(ex.len());
                    for (k, seg) in ex.iter().enumerate() {
                        let poly = seg.polyline(1e-3);
                        let n = poly.len().saturating_sub(1);
                        spans.push(n);
                        for (j, w) in poly.windows(2).enumerate() {
                            if w[0].dist(w[1]) > MERGE_TOL {
                                shapes.push((ci, Shape::Line { a: w[0], b: w[1] }));
                                tags.resize(shapes.len() - 1, None);
                                tags.push(Some((ci, k, j, n)));
                            }
                        }
                    }
                    expected.insert(ci, spans);
                    exact.insert(ci, ex);
                }
                None => {
                    for s in sk.segs(ci) {
                        if let Seg2::Line { a, b } = s {
                            shapes.push((ci, Shape::Line { a, b }));
                        }
                    }
                }
            },
        }
        tags.resize(shapes.len(), None);
        if shapes.len() > max_edges {
            whole_graph_complete = false;
            incomplete = true;
            break;
        }
    }
    // Past this many shapes the crossings below are not looked for.
    let crossings = shapes.len() <= MAX_EDGES;
    incomplete |= !crossings;
    // Split parameters per shape: end points of other curves on it, and crossings.
    let mut ends: Vec<Vec2> = Vec::new();
    for (_, sh) in &shapes {
        match *sh {
            Shape::Line { a, b } => ends.extend([a, b]),
            Shape::Round { sweep, .. } if sweep < TAU - 1e-12 => ends.extend([sh.point(0.0), sh.point(sweep)]),
            Shape::Round { .. } => {}
        }
    }
    // Bounding boxes, and a uniform grid over them: only shapes sharing a cell can meet.
    let boxes: Vec<solidcraft_geom::Aabb2> = shapes
        .iter()
        .map(|(_, sh)| match *sh {
            Shape::Line { a, b } => {
                solidcraft_geom::Aabb2 { min: Vec2::new(a.x.min(b.x), a.y.min(b.y)), max: Vec2::new(a.x.max(b.x), a.y.max(b.y)) }.inflated(tol)
            }
            Shape::Round { c, r, .. } => {
                solidcraft_geom::Aabb2 { min: Vec2::new(c.x - r - tol, c.y - r - tol), max: Vec2::new(c.x + r + tol, c.y + r + tol) }
            }
        })
        .collect();
    let overlap = |i: usize, j: usize| match (boxes.get(i), boxes.get(j)) {
        (Some(a), Some(b)) => a.meets(b),
        _ => false,
    };
    let grid = Grid::new(&boxes);
    // Each cut's parameter, and whether it is weak: a crossing next to an end that misses one of
    // the two curves (below), which gives way to any other cut it is merged with.
    let mut cuts: Vec<Vec<(f64, bool)>> = vec![Vec::new(); shapes.len()];
    // End points of other curves lying on each shape.
    let mut end_cells: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (k, p) in ends.iter().enumerate() {
        end_cells.entry(grid.cell(*p)).or_default().push(k);
    }
    let all_ends: Vec<usize> = (0..ends.len()).collect();
    for (i, (_, sh)) in shapes.iter().enumerate() {
        let Some(b) = boxes.get(i) else { continue };
        let cand: Vec<usize> = match grid.cells(b) {
            Some(cs) => cs.iter().flat_map(|c| end_cells.get(c).map(Vec::as_slice).unwrap_or(&[])).copied().collect(),
            None => all_ends.clone(),
        };
        for k in cand {
            if let Some(p) = ends.get(k)
                && let Some(t) = sh.param_on(*p, tol)
                && let Some(cu) = cuts.get_mut(i)
            {
                cu.push((t, false));
            }
        }
    }
    let mut shape_cells: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    let mut global: Vec<usize> = Vec::new();
    for (i, b) in boxes.iter().enumerate() {
        match grid.cells(b) {
            Some(cs) => {
                for c in cs {
                    shape_cells.entry(c).or_default().push(i);
                }
            }
            None => global.push(i),
        }
    }
    let mut partners: Vec<Vec<usize>> = vec![Vec::new(); shapes.len()];
    for v in shape_cells.values() {
        for (k, &i) in v.iter().enumerate() {
            for &j in v.iter().skip(k + 1) {
                let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                if let Some(p) = partners.get_mut(lo) {
                    p.push(hi);
                }
            }
        }
    }
    for &g in &global {
        for j in 0..shapes.len() {
            if j != g {
                let (lo, hi) = if g < j { (g, j) } else { (j, g) };
                if let Some(p) = partners.get_mut(lo) {
                    p.push(hi);
                }
            }
        }
    }
    for p in &mut partners {
        p.sort_unstable();
        p.dedup();
    }
    // Lines (and arcs) lying on a line (or an arc) are one path with it, carried by it: another
    // curve meets them all where it meets the carrier, so they are cut at the same points. Met
    // one by one, a curve touching them, or crossing them at a shallow angle, would meet each a
    // little apart, and pieces cut at different points would not be one edge. Their crossings
    // with each other, computed from round-off alone, are no cuts. The carrier is the first of
    // them, and each lies on it itself (not just on one that lies on it); a carrier takes one
    // whole circle at most, as two whole circles keep their crossings.
    let mut carrier: Vec<usize> = (0..shapes.len()).collect();
    let mut anchors: Vec<Vec2> = Vec::new();
    if crossings {
        let mut whole: Vec<bool> = shapes.iter().map(|(_, s)| s.is_full()).collect();
        for (i, (ci, x)) in shapes.iter().enumerate() {
            if carrier.get(i) != Some(&i) || !analytic(*ci) {
                continue;
            }
            for &j in partners.get(i).map(Vec::as_slice).unwrap_or(&[]) {
                let Some((cj, y)) = shapes.get(j) else { continue };
                let taken = y.is_full() && whole.get(i).copied().unwrap_or(true);
                if carrier.get(j) == Some(&j) && !taken && overlap(i, j) && analytic(*cj) && overlapping(x, y, tol) {
                    if let Some(c) = carrier.get_mut(j) {
                        *c = i;
                    }
                    if let Some(w) = whole.get_mut(i) {
                        *w |= y.is_full();
                    }
                }
            }
        }
    }
    if crossings {
        for i in 0..shapes.len() {
            for &j in partners.get(i).map(Vec::as_slice).unwrap_or(&[]) {
                if !overlap(i, j) {
                    continue;
                }
                let (gi, gj) = (carrier.get(i).copied().unwrap_or(i), carrier.get(j).copied().unwrap_or(j));
                let (Some((_, x)), Some((_, y)), Some((_, gx)), Some((_, gy))) = (shapes.get(i), shapes.get(j), shapes.get(gi), shapes.get(gj))
                else {
                    continue;
                };
                if gi == gj {
                    continue;
                }
                let mut pts = intersections_within(gx, gy, tol);
                // A tangent touch gives two nearly equal roots: one point.
                if let [p, q] = pts[..]
                    && p.dist(q) < tol * 100.0
                {
                    pts = vec![(p + q) * 0.5];
                }
                for p in pts {
                    let on_x = x.param_on(p, tol).or_else(|| end_param(x, p, tol));
                    let on_y = y.param_on(p, tol).or_else(|| end_param(y, p, tol));
                    // Crossings at (or next to) curve ends are already cuts. Not next to an end of
                    // one of the two curves that misses the other: nothing cut the other curve
                    // there, and it would cross without meeting it (a line 3e-5 from the
                    // junction of two curves of a loop, #755). Such a crossing is a weak cut.
                    let on = |sh: &Shape, e: Vec2| sh.param_on(e, tol).is_some() || end_param(sh, e, tol).is_some();
                    let misses = |e: Vec2| (end_param(x, e, tol).is_some() && !on(y, e)) || (end_param(y, e, tol).is_some() && !on(x, e));
                    // Every end within reach, whatever the grid's cell size (so an island's
                    // smaller grid finds the same ends as the whole sketch's).
                    let near_ends = || {
                        let reach = solidcraft_geom::Aabb2 { min: p, max: p }.inflated(tol * 100.0);
                        let cand: Vec<usize> = match grid.cells(&reach) {
                            Some(cs) => cs.iter().flat_map(|c| end_cells.get(c).map(Vec::as_slice).unwrap_or(&[])).copied().collect(),
                            None => all_ends.clone(),
                        };
                        cand.into_iter()
                            .filter_map(|k| ends.get(k).copied())
                            .filter(|e| e.dist(p) < tol * 100.0)
                            .fold((false, false), |(cut, weak), e| if misses(e) { (cut, true) } else { (true, weak) })
                    };
                    let (already, weak) = near_ends();
                    if on_x.is_none() || on_y.is_none() || already {
                        continue;
                    }
                    if let (Some(tx), Some(ty)) = (on_x, on_y) {
                        if x.param_on(p, tol).is_some()
                            && let Some(c) = cuts.get_mut(i)
                        {
                            c.push((tx, weak));
                        }
                        if y.param_on(p, tol).is_some()
                            && let Some(c) = cuts.get_mut(j)
                        {
                            c.push((ty, weak));
                        }
                        if (gi, gj) != (i, j) {
                            anchors.push(p);
                        }
                    }
                }
            }
        }
    }
    // Pieces → edges between merged nodes.
    let mut nodes: Vec<Vec2> = Vec::new();
    // Grid buckets of `tol`-sized cells: merging looks only at neighbouring cells.
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    let mut endpoint_nodes: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let cell = |p: Vec2| ((p.x / tol).floor() as i64, (p.y / tol).floor() as i64);
    let mut node = |p: Vec2, endpoint: Option<usize>| -> usize {
        let group = endpoint.and_then(|i| endpoint_groups.get(i).copied());
        if let Some(i) = group.and_then(|group| endpoint_nodes.get(&group).copied()) {
            return i;
        }
        let (cx, cy) = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = grid.get(&(cx + dx, cy + dy))
                    && let Some(i) = v.iter().copied().find(|i| nodes.get(*i).is_some_and(|q| q.dist(p) <= tol))
                {
                    if let Some(group) = group {
                        endpoint_nodes.insert(group, i);
                    }
                    return i;
                }
            }
        }
        nodes.push(p);
        let i = nodes.len() - 1;
        grid.entry((cx, cy)).or_default().push(i);
        if let Some(group) = group {
            endpoint_nodes.insert(group, i);
        }
        i
    };
    // Where curves meet at their carriers' crossing, the cuts on each lie within the tolerance of
    // that point, though not always of each other: they meet at a node there.
    for a in &anchors {
        node(*a, None);
    }
    let mut loops: Vec<IslandLoop> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    for (k, (ci, sh)) in shapes.iter().enumerate() {
        let mut ts = cuts.get(k).cloned().unwrap_or_default();
        ts.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Cuts closer than a few tolerances are one cut (tangent touches give near-double roots).
        // A weak cut gives way to the other: it only adds a cut where there was none.
        let scale = match *sh {
            Shape::Line { a, b } => a.dist(b),
            Shape::Round { r, .. } => r,
        };
        let close = (tol * 100.0 / scale.max(1e-12)).max(1e-9);
        ts.dedup_by(|a, b| {
            let same = (a.0 - b.0).abs() < close;
            if same && b.1 && !a.1 {
                *b = *a;
            }
            same
        });
        if let (Shape::Round { sweep, .. }, Some(&first), Some(&last)) = (*sh, ts.first(), ts.last())
            && sweep >= TAU - 1e-12
            && ts.len() > 1
            && (first.0 + TAU - last.0).abs() < close
        {
            if first.1 && !last.1 {
                ts.remove(0);
            } else {
                ts.pop();
            }
        }
        let ts: Vec<f64> = ts.into_iter().map(|(t, _)| t).collect();
        let id = sk.curves.get(*ci).map(|c| c.id.clone()).unwrap_or_default();
        let endpoints = sk.curves.get(*ci).filter(|c| matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. })).and_then(|c| c.kind.ends());
        let analytic = analytic(*ci);
        let coverage = tags.get(k).copied().flatten().or_else(|| {
            sk.curves
                .get(*ci)
                .filter(|c| matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. } | CurveKind::Circle { .. }))
                .map(|_| (*ci, 0, 0, 1))
        });
        let coverage = if ts.is_empty() { coverage } else { None };
        match *sh {
            Shape::Line { a, b } => {
                let mut ps = vec![a];
                ps.extend(ts.iter().map(|t| sh.point(*t)));
                ps.push(b);
                // Pieces split by other curves no longer stand for their whole piece.
                let tag = if ts.is_empty() { tags.get(k).copied().flatten() } else { None };
                for (piece, w) in ps.windows(2).enumerate() {
                    let (p, q) = (w[0], w[1]);
                    if p.dist(q) > MERGE_TOL {
                        let first = piece == 0;
                        let last = piece + 2 == ps.len();
                        let u = node(p, first.then(|| endpoints.map(|(a, _)| a)).flatten());
                        let v = node(q, last.then(|| endpoints.map(|(_, b)| b)).flatten());
                        edges.push(Edge { seg: Seg2::Line { a: p, b: q }, u, v, id: id.clone(), tag, coverage, analytic, curve: *ci });
                    }
                }
            }
            Shape::Round { c, r, start, sweep } => {
                let full = sweep >= TAU - 1e-12;
                if full && ts.is_empty() {
                    loops.push(IslandLoop::new(
                        Loop2::circle(c, r),
                        vec![id.clone()],
                        usize::MAX - *ci,
                        whole_graph_complete.then(|| vec![id.clone()]),
                        (0, *ci, 0),
                    ));
                    continue;
                }
                let mut bounds: Vec<f64> =
                    if full { ts.clone() } else { std::iter::once(0.0).chain(ts.iter().copied()).chain(std::iter::once(sweep)).collect() };
                if full {
                    let first = bounds.first().copied().unwrap_or(0.0);
                    bounds.push(first + TAU);
                }
                for (piece, w) in bounds.windows(2).enumerate() {
                    let (t0, t1) = (w[0], w[1]);
                    if (t1 - t0) * r <= MERGE_TOL {
                        continue;
                    }
                    let seg = Seg2::Arc { center: c, radius: r, start: start + t0, sweep: t1 - t0 };
                    let first = !full && piece == 0;
                    let last = !full && piece + 2 == bounds.len();
                    let u = node(seg.start(), first.then(|| endpoints.map(|(a, _)| a)).flatten());
                    let v = node(seg.end(), last.then(|| endpoints.map(|(_, b)| b)).flatten());
                    edges.push(Edge { seg, u, v, id: id.clone(), tag: None, coverage, analytic, curve: *ci });
                }
            }
        }
    }
    let n = nodes.len();
    // One edge where pieces overlap, then prune dangling edges.
    let mut alive = without_overlaps(&edges, tol);
    loop {
        let mut deg = vec![0usize; n];
        for (e, a) in edges.iter().zip(&alive) {
            if *a {
                if let Some(d) = deg.get_mut(e.u) {
                    *d += 1;
                }
                if let Some(d) = deg.get_mut(e.v) {
                    *d += 1;
                }
            }
        }
        let mut changed = false;
        for (e, a) in edges.iter().zip(alive.iter_mut()) {
            if *a && (deg.get(e.u).copied().unwrap_or(0) < 2 || deg.get(e.v).copied().unwrap_or(0) < 2) {
                *a = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Components over nodes for nesting.
    let mut comp: Vec<usize> = (0..n).collect();
    for (e, a) in edges.iter().zip(&alive) {
        if *a {
            union(&mut comp, e.v, e.u);
        }
    }

    // Half-edges: 2e = forward (u→v), 2e+1 = backward.
    let half = |h: usize| -> Option<(Seg2, usize, usize)> {
        let e = edges.get(h / 2)?;
        Some(if h.is_multiple_of(2) { (e.seg, e.u, e.v) } else { (e.seg.reversed(), e.v, e.u) })
    };
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (ei, a) in alive.iter().enumerate() {
        if !*a {
            continue;
        }
        for h in [2 * ei, 2 * ei + 1] {
            if let Some((_, from, _)) = half(h)
                && let Some(o) = outgoing.get_mut(from)
            {
                o.push(h);
            }
        }
    }
    let mut visited = vec![false; edges.len() * 2];
    for start in 0..edges.len() * 2 {
        if !alive.get(start / 2).copied().unwrap_or(false) || visited.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut segs = Vec::new();
        let mut edge_nodes = Vec::new();
        let mut edge_analytic = Vec::new();
        let mut ids = Vec::new();
        let mut ptags: Vec<Option<(Piece, bool)>> = Vec::new();
        let mut coverage_tags = Vec::new();
        let mut h = start;
        let mut ok = true;
        for _ in 0..=edges.len() * 2 {
            if let Some(v) = visited.get_mut(h) {
                *v = true;
            }
            let Some((seg, from, to)) = half(h) else {
                ok = false;
                break;
            };
            segs.push(seg);
            edge_nodes.push((from, to));
            if let Some(e) = edges.get(h / 2) {
                ids.push(e.id.clone());
                ptags.push(e.tag.map(|t| (t, h.is_multiple_of(2))));
                coverage_tags.push(e.coverage.map(|t| (t, h.is_multiple_of(2))));
                edge_analytic.push(e.analytic);
            } else {
                ptags.push(None);
                coverage_tags.push(None);
                edge_analytic.push(false);
            }
            let back = (-seg.end_tangent()).angle();
            let twin = h ^ 1;
            // First clockwise turn from the way back. Directions are compared a short way
            // along each segment (curvature decides between equal tangents, e.g. a line tangent
            // to an arc, or an arc leaving back along the incoming line).
            let kt = half(twin).map(|(ts, _, _)| curvature(&ts)).unwrap_or(0.0);
            let mut best: Option<(f64, f64, usize)> = None;
            for &o in outgoing.get(to).map(Vec::as_slice).unwrap_or(&[]) {
                if o == twin {
                    continue;
                }
                let Some((os, _, _)) = half(o) else { continue };
                let mut raw = (back - os.start_tangent().angle()).rem_euclid(TAU);
                if raw > TAU - 1e-9 {
                    raw = 0.0;
                }
                let mut d = raw + (kt - curvature(&os)) * 1e-7;
                if d <= 1e-12 {
                    d += TAU;
                }
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, 0.0, o));
                }
            }
            let best = best.map(|(d, _, o)| (d, o));
            let Some((_, next)) = best else {
                ok = false;
                break;
            };
            h = next;
            if h == start {
                break;
            }
            if visited.get(h).copied().unwrap_or(true) {
                ok = false;
                break;
            }
        }
        if !ok || h != start {
            continue;
        }
        let segs = close_graph_joints(segs, &edge_nodes, &edge_analytic, ENDPOINT_JOIN_TOL);
        let lp = Loop2 { segs: close_line_joints(restore_exact(segs, &ptags, &exact), ENDPOINT_JOIN_TOL) };
        if lp.signed_area() > 1e-9 {
            let c = edges.get(start / 2).map(|e| find(&mut comp, e.u)).unwrap_or(0);
            let curve = edges.get(start / 2).map(|e| e.curve).unwrap_or(usize::MAX);
            let complete = whole_graph_complete.then(|| complete_loop(&coverage_tags, &expected, sk)).flatten();
            loops.push(IslandLoop::new(lp, ids, c, complete, (1, curve, start)));
        }
    }
    Island { loops, incomplete }
}

/// Nest the loops of every island into profiles: a loop inside a loop of another connected
/// component (from any island) becomes a hole of the innermost one containing it.
fn assemble(sk: &Sketch, islands: &[Arc<Island>]) -> Vec<Profile> {
    // The loops in the order one search over the whole sketch finds them, so ties in nesting and
    // ordering go the same way: whole circles in sketch order, then the traced faces in the order
    // of their first edge (edges follow the curves in sketch order).
    let mut all: Vec<((usize, usize), &IslandLoop)> =
        islands.iter().enumerate().flat_map(|(k, island)| island.loops.iter().map(move |l| ((k, l.comp), l))).collect();
    all.sort_by_key(|(_, l)| l.key);
    let loops: Vec<(&Loop2, &Vec<String>, (usize, usize), &Option<Vec<String>>)> =
        all.iter().map(|(c, l)| (&l.lp, &l.ids, *c, &l.complete)).collect();

    // Nesting: parent = smallest containing loop from another component. Bounding boxes rule
    // out most pairs before the point-in-loop test.
    let areas: Vec<f64> = all.iter().map(|(_, l)| l.area).collect();
    let boxes: Vec<solidcraft_geom::Aabb2> = all.iter().map(|(_, l)| l.bounds).collect();
    let inside_box = |j: usize, p: Vec2| boxes.get(j).is_some_and(|b| b.contains(p));
    let mut parent_of: Vec<Option<usize>> = vec![None; loops.len()];
    for (i, (li, _, ci, _)) in loops.iter().enumerate() {
        let probe = li.segs.first().map(Seg2::mid).unwrap_or_default();
        let ai = areas.get(i).copied().unwrap_or(0.0);
        let mut best: Option<(f64, usize)> = None;
        for (j, (lj, _, cj, _)) in loops.iter().enumerate() {
            let aj = areas.get(j).copied().unwrap_or(0.0);
            if i == j || ci == cj || aj <= ai || !inside_box(j, probe) || best.is_some_and(|(ba, _)| aj >= ba) {
                continue;
            }
            if lj.contains(probe) {
                best = Some((aj, j));
            }
        }
        if let Some(p) = parent_of.get_mut(i) {
            *p = best.map(|(_, j)| j);
        }
    }
    let mut kids_of: Vec<Vec<usize>> = vec![Vec::new(); loops.len()];
    for (k, p) in parent_of.iter().enumerate() {
        if let Some(p) = p
            && let Some(v) = kids_of.get_mut(*p)
        {
            v.push(k);
        }
    }
    let out: Vec<Profile> = loops
        .iter()
        .enumerate()
        .map(|(i, (l, ids, _, complete_outer))| {
            let kids: Vec<usize> = kids_of.get(i).cloned().unwrap_or_default();
            let holes: Vec<Loop2> = kids.iter().filter_map(|k| loops.get(*k).map(|(h, _, _, _)| h.ccw().reversed())).collect();
            let hole_curves: Vec<Vec<String>> = kids.iter().filter_map(|k| loops.get(*k).map(|(_, ids, _, _)| (*ids).clone())).collect();
            let region = Region2 { outer: l.ccw(), holes };
            let area = region.area();
            let centroid = region.centroid();
            let mut complete_holes = kids.iter().map(|k| loops.get(*k)?.3.clone()).collect::<Option<Vec<_>>>();
            if let Some(holes) = &mut complete_holes {
                holes.sort();
            }
            Profile {
                region,
                outer_curves: (*ids).clone(),
                hole_curves,
                area,
                centroid,
                complete_outer: (*complete_outer).clone(),
                complete_holes,
                hull: OnceLock::new(),
                outline: OnceLock::new(),
            }
        })
        .collect();
    let index: std::collections::HashMap<&str, usize> = sk.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let first_curve = |p: &Profile| p.outer_curves.iter().filter_map(|id| index.get(id.as_str()).copied()).min().unwrap_or(usize::MAX);
    let mut keyed: Vec<(usize, Profile)> = out.into_iter().map(|p| (first_curve(&p), p)).collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.area.total_cmp(&a.1.area)));
    let out: Vec<Profile> = keyed.into_iter().map(|x| x.1).collect();
    out
}

/// Why no single profile is bounded by a list of curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurvePick {
    /// No profile's outer loop is made of exactly those curves.
    Missing,
    /// This many profiles are (e.g. both halves of a circle cut by a line).
    Ambiguous(usize),
}

/// The profile picked by the curves around it, by index into `profiles`. A profile matches when
/// its outer loop is made of the listed curves and no others: every listed curve, or a piece of
/// it where other curves cut it, lies on the loop, and every piece of the loop belongs to a
/// listed curve. Order and repeats don't matter. Holes don't take part (islands inside a profile
/// are its holes, as for a profile picked by a point). When several profiles match, one whose
/// loop has exactly the listed pieces (a curve listed twice for two pieces) wins; otherwise the
/// pick is ambiguous, never a guess.
pub fn profile_by_curves(profiles: &[Profile], ids: &[String]) -> Result<usize, CurvePick> {
    use std::collections::BTreeSet;
    let want: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    if want.is_empty() {
        return Err(CurvePick::Missing);
    }
    let hits: Vec<usize> = profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| p.outer_curves.iter().map(String::as_str).collect::<BTreeSet<_>>() == want)
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [] => Err(CurvePick::Missing),
        [i] => Ok(*i),
        _ => {
            fn sorted(v: &[String]) -> Vec<&str> {
                let mut v: Vec<&str> = v.iter().map(String::as_str).collect();
                v.sort_unstable();
                v
            }
            let exact = sorted(ids);
            let same: Vec<usize> = hits.iter().copied().filter(|i| profiles.get(*i).is_some_and(|p| sorted(&p.outer_curves) == exact)).collect();
            match same.as_slice() {
                [i] => Ok(*i),
                _ => Err(CurvePick::Ambiguous(hits.len())),
            }
        }
    }
}

/// The innermost profile containing a sketch point, by index into `profiles`.
pub fn profile_at_point(profiles: &[Profile], p: Vec2) -> Option<usize> {
    profiles.iter().enumerate().filter(|(_, q)| q.contains(p)).min_by(|a, b| a.1.area.total_cmp(&b.1.area)).map(|(i, _)| i)
}

/// The distinct curve ids of a loop, in loop order (a curve cut into pieces is listed once).
pub fn distinct_curves(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

fn same_seg(a: &Seg2, b: &Seg2, tol: f64) -> bool {
    match (*a, *b) {
        (Seg2::Line { a: p, b: q }, Seg2::Line { a: r, b: s }) => p.dist(r) <= tol && q.dist(s) <= tol,
        (Seg2::Arc { center: c1, radius: r1, .. }, Seg2::Arc { center: c2, radius: r2, .. }) => {
            c1.dist(c2) <= tol
                && (r1 - r2).abs() <= tol
                && a.start().dist(b.start()) <= tol
                && a.end().dist(b.end()) <= tol
                && a.mid().dist(b.mid()) <= tol
        }
        // Free-form segments (text outlines, ellipses, splines): the same curve when they run
        // through the same points.
        (Seg2::Line { .. } | Seg2::Arc { .. }, _) | (_, Seg2::Line { .. } | Seg2::Arc { .. }) => false,
        _ => [0.0, 0.25, 0.5, 0.75, 1.0].iter().all(|t| a.point_at(*t).dist(b.point_at(*t)) <= tol),
    }
}

/// A closed loop's segments with each run that `joins` says continue one another made one line.
fn rejoined(segs: Vec<Seg2>, joins: &impl Fn(&Seg2, &Seg2) -> bool) -> Vec<Seg2> {
    let n = segs.len();
    let join = |i: usize| segs.get((i + n - 1) % n).zip(segs.get(i)).is_some_and(|(s, t)| joins(s, t));
    // Start where a run starts; a loop that is all one run (it can't close) stays as it is.
    let Some(k) = (0..n).find(|&i| !join(i)) else { return segs };
    let mut out: Vec<Seg2> = Vec::with_capacity(n);
    for j in 0..n {
        let i = (k + j) % n;
        let Some(s) = segs.get(i).copied() else { continue };
        if j > 0
            && join(i)
            && let Some(Seg2::Line { b, .. }) = out.last_mut()
        {
            *b = s.end();
            continue;
        }
        out.push(s);
    }
    out
}

/// `regions` with each region that repeats an earlier one (the same boundary within a small
/// tolerance) left out.
fn without_repeats(regions: &[Region2]) -> Vec<Region2> {
    // Comparing every pair is quadratic: beyond this many the regions are kept as they are.
    const MAX_COMPARED: usize = 5000;
    if regions.len() < 2 || regions.len() > MAX_COMPARED {
        return regions.to_vec();
    }
    let keys: Vec<(f64, Vec2, f64)> =
        regions.iter().map(|r| (r.outer.signed_area().abs(), r.outer.centroid(), r.outer.segs.iter().map(Seg2::length).sum::<f64>())).collect();
    let mut kept: Vec<usize> = Vec::new();
    for (i, r) in regions.iter().enumerate() {
        let Some(&(area, centroid, size)) = keys.get(i) else { continue };
        let tol = 1e-7 * (1.0 + size);
        let repeat = kept.iter().any(|&k| {
            keys.get(k).is_some_and(|&(a, c, _)| (a - area).abs() <= tol * (1.0 + size) && c.dist(centroid) <= tol)
                && regions.get(k).is_some_and(|other| same_region(r, other, tol))
        });
        if !repeat {
            kept.push(i);
        }
    }
    kept.iter().filter_map(|&i| regions.get(i).cloned()).collect()
}

/// Whether two regions have the same boundary within `tol`: the same outer loop and the same
/// holes, whichever segment each loop starts with.
fn same_region(a: &Region2, b: &Region2, tol: f64) -> bool {
    let on = |p: Vec2, l: &Loop2| l.segs.iter().any(|s| s.point_at(s.closest_param(p)).dist(p) <= tol);
    // Each loop's corners and segment middles lie on the other loop.
    let covers = |x: &Loop2, y: &Loop2| x.segs.iter().all(|s| on(s.start(), y) && on(s.mid(), y));
    let same_loop = |x: &Loop2, y: &Loop2| {
        (x.signed_area().abs() - y.signed_area().abs()).abs() <= tol * (1.0 + x.segs.iter().map(Seg2::length).sum::<f64>())
            && covers(x, y)
            && covers(y, x)
    };
    a.holes.len() == b.holes.len() && same_loop(&a.outer, &b.outer) && a.holes.iter().all(|h| b.holes.iter().any(|g| same_loop(h, g)))
}

/// Union of regions that touch along shared boundary segments (one extrude of several adjacent
/// profiles makes one body). Disjoint regions stay separate.
pub fn merge_regions(regions: &[Region2]) -> Vec<Region2> {
    // A region drawn again on top of another is one region (#5543): a circle drawn twice at one
    // place is two profiles, and extruded as two coincident tools a cut through a grid of holes
    // with four such circles retried with the tool nudged and took fifty times as long.
    let distinct = without_repeats(regions);
    let regions = distinct.as_slice();
    let tol = MERGE_TOL * 10.0;
    // Every boundary segment oriented with the region on its left.
    let mut segs: Vec<Seg2> = Vec::new();
    for r in regions {
        segs.extend(r.outer.ccw().segs);
        for h in &r.holes {
            segs.extend(h.ccw().reversed().segs);
        }
    }
    if segs.len() > MAX_EDGES {
        return regions.to_vec();
    }
    // Drop pairs that run against each other (shared boundaries).
    let mut alive = vec![true; segs.len()];
    let mut shared = false;
    for i in 0..segs.len() {
        if !alive.get(i).copied().unwrap_or(false) {
            continue;
        }
        for j in (i + 1)..segs.len() {
            if alive.get(j).copied().unwrap_or(false)
                && let (Some(a), Some(b)) = (segs.get(i), segs.get(j))
                && same_seg(a, &b.reversed(), tol)
            {
                alive[i] = false;
                alive[j] = false;
                shared = true;
                break;
            }
        }
    }
    if !shared {
        return regions.to_vec();
    }
    // Where the shared boundaries ended, the outline is left split in two: the lines on either
    // side are one edge again (else they make split faces and vertices that booleans trip on).
    let ends: Vec<Vec2> = segs.iter().zip(&alive).filter(|(_, a)| !**a).flat_map(|(s, _)| [s.start(), s.end()]).collect();
    let rejoin = |s: &Seg2, t: &Seg2| match (*s, *t) {
        (Seg2::Line { a, b }, Seg2::Line { a: b2, b: c }) => {
            b.dist(b2) <= tol
                && ends.iter().any(|e| e.dist(b) <= tol)
                && (b - a).normalized().zip((c - b).normalized()).is_some_and(|(u, v)| u.dot(v) > 1.0 - 1e-12)
        }
        _ => false,
    };
    let rest: Vec<Seg2> = segs.iter().zip(&alive).filter(|(_, a)| **a).map(|(s, _)| *s).collect();
    // Chain into loops, taking the sharpest left turn where several continue.
    let mut used = vec![false; rest.len()];
    let mut loops: Vec<Loop2> = Vec::new();
    for start in 0..rest.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut lp = Vec::new();
        let mut cur = start;
        for _ in 0..=rest.len() {
            if let Some(u) = used.get_mut(cur) {
                *u = true;
            }
            let Some(seg) = rest.get(cur).copied() else { break };
            lp.push(seg);
            let end = seg.end();
            if let Some(first) = rest.get(start)
                && end.dist(first.start()) <= tol
                && lp.len() > 1
            {
                break;
            }
            let back = (-seg.end_tangent()).angle();
            let next = (0..rest.len())
                .filter(|k| !used.get(*k).copied().unwrap_or(true) && rest.get(*k).is_some_and(|s| s.start().dist(end) <= tol))
                .min_by(|x, y| {
                    let d = |k: usize| rest.get(k).map(|s| (back - s.start_tangent().angle()).rem_euclid(TAU)).unwrap_or(TAU);
                    d(*x).total_cmp(&d(*y))
                });
            match next {
                Some(k) => cur = k,
                None => break,
            }
        }
        // A loop that encloses nothing (pieces running back over themselves) is no boundary.
        if !lp.is_empty() && lp.first().zip(lp.last()).is_some_and(|(f, l)| l.end().dist(f.start()) <= tol) {
            let lp = Loop2 { segs: rejoined(lp, &rejoin) };
            if lp.signed_area().abs() > 1e-9 {
                loops.push(lp);
            }
        }
    }
    let outers: Vec<&Loop2> = loops.iter().filter(|l| l.signed_area() > 0.0).collect();
    let holes: Vec<&Loop2> = loops.iter().filter(|l| l.signed_area() < 0.0).collect();
    let mut out: Vec<Region2> = outers.iter().map(|o| Region2 { outer: (*o).clone(), holes: Vec::new() }).collect();
    for h in holes {
        let probe = h.segs.first().map(Seg2::mid).unwrap_or_default();
        // Innermost containing outer loop.
        let best = out.iter_mut().filter(|r| r.outer.contains(probe)).min_by(|a, b| a.outer.signed_area().total_cmp(&b.outer.signed_area()));
        if let Some(r) = best {
            r.holes.push(h.clone());
        }
    }
    if out.is_empty() { regions.to_vec() } else { out }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod whole_boundary_tests {
    use super::*;

    #[test]
    fn whole_boundary_coverage_requires_every_piece_once_in_one_direction() {
        let mut sk = Sketch::new();
        sk.add_circle(Vec2::ZERO, 5.0, None, Some("c1")).unwrap();
        let expected = std::collections::HashMap::from([(0, vec![2, 3])]);
        let full = vec![
            Some(((0, 0, 0, 2), true)),
            Some(((0, 0, 1, 2), true)),
            Some(((0, 1, 0, 3), true)),
            Some(((0, 1, 1, 3), true)),
            Some(((0, 1, 2, 3), true)),
        ];
        assert_eq!(complete_loop(&full, &expected, &sk), Some(vec!["c1".into()]));
        assert!(complete_loop(&full[..4], &expected, &sk).is_none());
        let mut duplicate = full.clone();
        duplicate[4] = duplicate[3];
        assert!(complete_loop(&duplicate, &expected, &sk).is_none());
        let mut cut = full.clone();
        cut[2] = None;
        assert!(complete_loop(&cut, &expected, &sk).is_none());
        let mut permuted = full.clone();
        permuted.swap(2, 3);
        assert!(complete_loop(&permuted, &expected, &sk).is_none());
        assert!(complete_loop(&full, &std::collections::HashMap::new(), &sk).is_none());
        // A skipped tiny graph span still belongs to the original curve and cannot be ignored.
        let omitted = std::collections::HashMap::from([(0, vec![2, 4])]);
        assert!(complete_loop(&full, &omitted, &sk).is_none());
        let mut reverse = full;
        reverse[2] = Some(((0, 1, 0, 3), false));
        assert!(complete_loop(&reverse, &expected, &sk).is_none());
    }

    #[test]
    fn whole_boundary_refuses_globally_truncated_graph_context() {
        let mut sk = Sketch::new();
        sk.add_circle(Vec2::ZERO, 10.0, None, Some("outline")).unwrap();
        sk.add_circle(Vec2::ZERO, 5.0, None, Some("hole")).unwrap();
        sk.add_line(Vec2::new(-12.0, 0.0), Vec2::new(12.0, 0.0), None, None, Some("later-crossing")).unwrap();
        // Exercise the production truncation branch with a small deterministic budget;
        // the old display still has outlines, but the omitted crossing forbids persistence.
        let displayed = find_profiles_bounded(&sk, 1);
        assert_eq!(displayed.len(), 2);
        assert!(displayed.iter().all(|p| p.whole_curve_boundaries().is_none()));
    }

    #[test]
    fn whole_boundary_records_holes_but_refuses_intersection_fragments() {
        let mut sk = Sketch::new();
        sk.add_circle(Vec2::ZERO, 10.0, None, Some("outer")).unwrap();
        sk.add_circle(Vec2::ZERO, 5.0, None, Some("hole")).unwrap();
        let profiles = find_profiles(&sk);
        let annulus = profiles.iter().find(|p| !p.region.holes.is_empty()).unwrap();
        assert_eq!(annulus.whole_curve_boundaries(), Some((vec!["outer".into()], vec![vec!["hole".into()]])));
        // A line through the ring creates bounded fragments of both original circles.
        sk.add_line(Vec2::new(-12.0, 0.0), Vec2::new(12.0, 0.0), None, None, Some("cut")).unwrap();
        let profiles = find_profiles(&sk);
        assert!(!profiles.is_empty());
        assert!(profiles.iter().all(|p| p.whole_curve_boundaries().is_none()));
        // Serialized profiles cannot forge this runtime certificate.
        let copy: Profile = serde_json::from_str(&serde_json::to_string(annulus).unwrap()).unwrap();
        assert!(copy.whole_curve_boundaries().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Aabb2;

    /// An infinite or huge box spans more cells than an `i64` holds; it is checked against all
    /// shapes instead of wrapping the count and filling memory (solidcraft#5020).
    #[test]
    fn grid_cells_refuse_infinite_and_huge_boxes() {
        let small = Aabb2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(10.0, 10.0) };
        let grid = Grid::new(&[small]);
        // A box spanning the whole grid touches the 2 x 2 cells at its corners.
        assert_eq!(grid.cells(&small).map(|c| c.len()), Some(4));
        let inf = Aabb2 { min: Vec2::new(-f64::INFINITY, -f64::INFINITY), max: Vec2::new(f64::INFINITY, f64::INFINITY) };
        assert!(grid.cells(&inf).is_none());
        let nan = Aabb2 { min: Vec2::new(f64::NAN, 0.0), max: Vec2::new(1.0, 1.0) };
        assert!(grid.cells(&nan).is_none());
        let huge = Aabb2 { min: Vec2::new(-1e300, -1e300), max: Vec2::new(1e300, 1e300) };
        assert!(grid.cells(&huge).is_none());
        // A grid built over an infinite box falls back to unit cells and still answers.
        let grid = Grid::new(&[small, inf]);
        assert!(grid.cells(&inf).is_none());
        assert!(grid.cells(&huge).is_none());
    }

    fn text_into(sk: &mut Sketch, text: &str, at: Vec2, height: f64) {
        let source =
            crate::LinkSource::Text { text: text.into(), at, height, angle: 0.0, font: None, style: Default::default(), exprs: Default::default() };
        sk.add_link(crate::LinkKind::Text, source, &crate::text_geometry(text, at, height, 0.0).unwrap()).unwrap();
    }

    fn rectangle(sk: &mut Sketch, x0: f64, y0: f64, x1: f64, y1: f64) {
        for (p, q) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
            sk.add_line(Vec2::new(p.0, p.1), Vec2::new(q.0, q.1), None, None, None).unwrap();
        }
    }

    /// The same profiles, in the same order, with the same curves, holes, geometry and ids.
    fn assert_same(got: &[Profile], want: &[Profile]) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert_eq!(serde_json::to_string(g).unwrap(), serde_json::to_string(w).unwrap());
            assert_eq!(g.whole_curve_boundaries(), w.whole_curve_boundaries());
        }
    }

    /// Searching island by island (and from the cache) finds exactly what one search over the
    /// whole sketch finds. Returns the number of islands.
    fn islands_match_whole(sk: &Sketch) -> usize {
        let whole = search(sk, MAX_EDGES, false, false);
        let split = search(sk, MAX_EDGES, true, false);
        assert!(!whole.incomplete && !split.incomplete);
        assert_same(&split.profiles, &whole.profiles);
        for _ in 0..2 {
            assert_same(&find_profiles(sk), &whole.profiles);
        }
        let curves: Vec<usize> = (0..sk.curves.len()).collect();
        islands(sk, &curves).len()
    }

    /// #6896: letters with counters (holes), a frame round them that touches none (its holes
    /// come from other islands), a disc in a counter, a circle crossing two letters, and curves
    /// whose ends join or cross next to another curve's end from just apart.
    #[test]
    fn profiles_found_island_by_island_match_one_whole_search() {
        let mut sk = Sketch::new();
        text_into(&mut sk, "B8O", Vec2::new(5.0, 0.0), 10.0);
        text_into(&mut sk, "AR", Vec2::new(5.0, -20.0), 10.0);
        rectangle(&mut sk, 0.0, -30.0, 60.0, 15.0);
        // A disc inside the O's counter, touching nothing: the counter's hole... and a profile.
        let disc = sk.add_circle(Vec2::new(28.5, 5.0), 1.0, None, None).unwrap();
        let disc = sk.curves[disc].id.clone();
        // A circle across the A and the R: one island with both.
        sk.add_circle(Vec2::new(14.0, -15.0), 4.0, None, None).unwrap();
        // A triangle whose corners miss by 5e-5 (joined), beside a line crossing next to a stray
        // end 5e-4 away (the stray end decides whether the crossing cuts).
        sk.add_line(Vec2::new(70.0, 0.0), Vec2::new(80.0, 0.0), None, None, None).unwrap();
        sk.add_line(Vec2::new(80.0 + 5e-5, 0.0), Vec2::new(75.0, 8.0), None, None, None).unwrap();
        sk.add_line(Vec2::new(75.0, 8.0 + 5e-5), Vec2::new(70.0, 0.0), None, None, None).unwrap();
        rectangle(&mut sk, 90.0, 0.0, 100.0, 10.0);
        sk.add_line(Vec2::new(95.0, -5.0), Vec2::new(95.0, 15.0), None, None, None).unwrap();
        sk.add_line(Vec2::new(95.0 + 5e-4, 10.0 + 5e-4), Vec2::new(105.0, 20.0), None, None, None).unwrap();
        // An arc and a ring far off on their own.
        sk.add_arc(Vec2::new(130.0, 0.0), Vec2::new(135.0, 0.0), Vec2::new(125.0, 0.0), [None; 3], None).unwrap();
        sk.add_line(Vec2::new(125.0, 0.0), Vec2::new(135.0, 0.0), None, None, None).unwrap();
        sk.add_circle(Vec2::new(150.0, 0.0), 5.0, None, None).unwrap();
        sk.add_circle(Vec2::new(150.0, 0.0), 3.0, None, None).unwrap();
        let islands = islands_match_whole(&sk);
        assert!(islands >= 8, "{islands} islands");
        let profiles = find_profiles(&sk);
        // The frame has the letters (and their crossing circle) as holes, across island borders.
        let frame = profiles.iter().find(|p| p.outer_curves.len() == 4 && p.region.outer.signed_area() > 2000.0).unwrap();
        assert!(frame.hole_curves.len() >= 4, "{:?}", frame.hole_curves.len());
        // The disc in the O's counter is a hole of the counter, which is its own profile.
        assert!(profiles.iter().any(|p| p.hole_curves.iter().any(|h| *h == [disc.clone()])));
    }

    /// #6896: past the edge limit, the old single search dropped profiles silently. The limit
    /// now holds per island, so many small islands are all found, and an island too large says
    /// so.
    #[test]
    fn profile_limit_holds_per_island_and_says_when_it_cuts_the_search_short() {
        let mut sk = Sketch::new();
        for k in 0..30 {
            let x = k as f64 * 20.0;
            rectangle(&mut sk, x, 0.0, x + 10.0, 10.0);
        }
        let whole = search(&sk, 50, false, false);
        assert!(whole.incomplete);
        assert!(whole.profiles.len() < 30);
        let split = search(&sk, 50, true, true);
        assert!(!split.incomplete);
        assert_eq!(split.profiles.len(), 30);
        assert_same(&split.profiles, &search(&sk, MAX_EDGES, false, false).profiles);
        // One island past the limit: reported.
        let mut big = Sketch::new();
        for k in 0..30 {
            let x = k as f64 * 10.0;
            rectangle(&mut big, x, 0.0, x + 10.0, 10.0);
        }
        assert!(search(&big, 50, true, true).incomplete);
        assert!(!find_profiles_checked(&big).incomplete);
    }

    /// #6896: moving one island's point changes only that island's fingerprint; the result from
    /// the cache is the one a fresh whole search finds.
    #[test]
    fn profiles_from_the_island_cache_follow_a_moved_point() {
        let mut sk = Sketch::new();
        text_into(&mut sk, "OB", Vec2::ZERO, 10.0);
        rectangle(&mut sk, 30.0, 0.0, 40.0, 10.0);
        let curves: Vec<usize> = (0..sk.curves.len()).collect();
        let before: Vec<u128> = islands(&sk, &curves).iter().map(|g| fingerprint(&sk, g, MAX_EDGES)).collect();
        assert_same(&find_profiles(&sk), &search(&sk, MAX_EDGES, false, false).profiles);
        let corner = sk.points.len() - 1;
        sk.points[corner].pos = Vec2::new(29.0, 11.0);
        let after: Vec<u128> = islands(&sk, &curves).iter().map(|g| fingerprint(&sk, g, MAX_EDGES)).collect();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
        assert_same(&find_profiles(&sk), &search(&sk, MAX_EDGES, false, false).profiles);
    }

    /// The profile under a point, from each profile's box and loops sampled once, is the one
    /// testing every region afresh found (the code before), over a sketch of text, circles in a
    /// plate, lines through them and arcs; asked twice, so the second answers from the samples.
    #[test]
    fn profile_at_point_from_the_sampled_loops_matches_the_regions() {
        let mut sk = Sketch::new();
        rectangle(&mut sk, -5.0, -5.0, 105.0, 85.0);
        text_into(&mut sk, "B8", Vec2::new(10.0, 62.0), 12.0);
        for i in 0..8 {
            for j in 0..5 {
                sk.add_circle(Vec2::new(8.0 + 12.0 * f64::from(i), 6.0 + 11.0 * f64::from(j)), 4.0, None, None).unwrap();
            }
        }
        for j in 0..3 {
            let y = 6.0 + 22.0 * f64::from(j);
            sk.add_line(Vec2::new(-5.0, y), Vec2::new(105.0, y), None, None, None).unwrap();
        }
        sk.add_arc(Vec2::new(80.0, 70.0), Vec2::new(90.0, 70.0), Vec2::new(70.0, 70.0), [None; 3], None).unwrap();
        sk.add_line(Vec2::new(70.0, 70.0), Vec2::new(90.0, 70.0), None, None, None).unwrap();
        let profiles = find_profiles(&sk);
        assert!(profiles.len() > 60, "{}", profiles.len());
        let old =
            |p: Vec2| profiles.iter().enumerate().filter(|(_, q)| q.region.contains(p)).min_by(|a, b| a.1.area.total_cmp(&b.1.area)).map(|(i, _)| i);
        let mut found = 0;
        for round in 0..2 {
            for j in 0..=91 {
                for i in 0..=127 {
                    let p = Vec2::new(-10.0 + 120.0 * f64::from(i) / 127.0, -10.0 + 100.0 * f64::from(j) / 91.0);
                    let want = old(p);
                    assert_eq!(profile_at_point(&profiles, p), want, "round {round} at {p:?}");
                    for q in &profiles {
                        assert_eq!(q.contains(p), q.region.contains(p));
                    }
                    found += usize::from(want.is_some());
                }
            }
        }
        assert!(found > 15_000, "{found}");
    }
}
