//! Footprints: the outline a body covers when it is seen straight down (along -Z), for laying
//! bodies out on a sheet or a build plate ([`crate::arrange`]).
//!
//! The outline comes from the body's exact edges and face silhouettes, projected from their exact
//! curves ([`crate::hlr::projected_curves`]); the display mesh plays no part. Visibility is not
//! worked out at all: every projected curve is kept, seen or hidden. The pieces are
//! flattened into chords with a known error, welded where they meet, split where they cross or
//! where one ends on another, and the outer boundary of that drawing is walked keeping the
//! outside on the right. What is inside the outline is covered, so a hole through the body is
//! not offered to other parts: a footprint has no holes.
//!
//! A body whose shadow is several pieces apart, or whose outline pinches to a point, gets the
//! convex hull of its drawing instead: it covers more than the body, never less.
//!
//! The outline is the true one to within [`Footprint::error`] (0 for an outline of straight lines;
//! points closer than 1e-9 of the size are taken as one): a layout keeps true outlines apart when
//! it widens its spacing by twice that and its margin by it.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::brep::Brep;
use solidcore_record::{Seg2, Vec2, Vec3};

use crate::arrange::{MAX_CORNERS, Shape2};
use crate::hlr::{self, HlrError, Options, Placement, View};
use crate::profile::Seg2Geom;

/// How closely curves are chorded before the outline is walked, relative to the body's size: a
/// fifth of the outline's own chord ([`outline_of`]).
const CURVE_TOLERANCE: f64 = 1e-4;

/// Most pieces of drawing an outline is walked from; past it the body is too detailed.
pub const MAX_PIECES: usize = 200_000;

/// The outline a body covers seen from above.
#[derive(Clone, Debug, PartialEq)]
pub struct Footprint {
    /// The outline, counter-clockwise, in the XY coordinates of the turned body; no holes.
    pub shape: Shape2,
    /// How far the outline may be from the true one (mm).
    pub error: f64,
    /// Whether this is the convex hull of the drawing rather than its outline.
    pub hull: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FootprintError {
    #[error("{0}")]
    Hlr(#[from] HlrError),
    #[error("the body has no edges to outline")]
    Empty,
    #[error("the body is too detailed to outline ({0} pieces of drawing)")]
    TooDetailed(usize),
    #[error("cancelled")]
    Cancelled,
}

/// The footprint of `body` turned by `rotation` (its columns: where the body's X, Y and Z axes
/// go), seen along -Z.
pub fn footprint(body: &Brep, rotation: [Vec3; 3], cancel: &dyn Fn() -> bool) -> Result<Footprint, FootprintError> {
    Shadow::new(body, cancel)?.footprint(rotation, cancel)
}

/// A body made ready for its footprints: the costly part, done once however it is turned.
pub struct Shadow {
    def: hlr::Prepared,
}

impl Shadow {
    pub fn new(body: &Brep, cancel: &dyn Fn() -> bool) -> Result<Self, FootprintError> {
        // Whether a piece would be seen does not matter to a shadow: the curves are projected
        // with no hidden-line work, freeform faces included.
        let opts = Options { freeform: true, ..Options::default() };
        Ok(Self { def: hlr::prepare_cancellable(body, &opts, cancel)? })
    }

    /// The footprint of the body turned by `rotation`, as [`footprint`] finds it.
    pub fn footprint(&self, rotation: [Vec3; 3], cancel: &dyn Fn() -> bool) -> Result<Footprint, FootprintError> {
        let view = View::new(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 1.0, 0.0))?;
        let placed = Placement { body: 0, rotation, translation: Vec3::ZERO };
        let (curves, sag) = hlr::projected_curves(&self.def, &placed, &view, CURVE_TOLERANCE, cancel)?;
        if cancel() {
            return Err(FootprintError::Cancelled);
        }
        let mut fp = outline_of(&curves)?;
        if sag.is_finite() {
            fp.error += sag;
        }
        Ok(fp)
    }
}

/// The outer outline of a drawing made of `curves`, with the error of its flattening.
pub fn outline_of(curves: &[Seg2]) -> Result<Footprint, FootprintError> {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut usable = Vec::new();
    for c in curves {
        if c.check(hlr::MAX_COORDINATE).is_err() {
            continue;
        }
        for p in [c.start(), c.end(), c.mid()] {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        usable.push(*c);
    }
    let size = (hi.x - lo.x).max(hi.y - lo.y);
    if usable.is_empty() || !size.is_finite() || size <= 0.0 {
        return Err(FootprintError::Empty);
    }
    // Chords within 5e-4 of the size: 0.05 mm on a 100 mm part, well inside a printer's or a
    // cutter's own error, and few enough corners to keep a layout quick. A detailed outline that
    // needs more corners than a layout takes is flattened more coarsely.
    let mut chord = 5e-4 * size;
    for _ in 0..8 {
        // Ends of pieces meant to meet are exact projections of one point: they agree far closer.
        let weld = 1e-9 * size;
        let mut segs = Vec::new();
        for c in &usable {
            let pts = c.polyline(chord);
            segs.extend(pts.windows(2).filter_map(|w| match w {
                [a, b] => Some((*a, *b)),
                _ => None,
            }));
            if segs.len() > MAX_PIECES {
                return Err(FootprintError::TooDetailed(segs.len()));
            }
        }
        // Edges that nearly coincide in the view (a ring lying flat: its top and bottom circles)
        // can leave the walked outline touching itself; welding more coarsely joins them, and
        // the hull is the last resort. The weld used is part of the error.
        let mut found = None;
        for w in [weld, weld * 1e2, weld * 1e4] {
            if let Some(r) = outer_ring(&segs, w).filter(|r| crate::arrange::is_simple(r)) {
                found = Some((r, w));
                break;
            }
        }
        // Corners a hair apart, or a hair off the line through their neighbours, go: the layout
        // would take them for edges touching.
        let fine = 1e-7 * size;
        let tidied = |r: Vec<Vec2>| {
            let n = r.len();
            let t = tidy(r, fine);
            let moved = if t.len() < n { fine } else { 0.0 };
            (t, moved)
        };
        let (ring, hull, welded) = match found.map(|(r, w)| (tidied(r), w)).filter(|((r, _), _)| crate::arrange::is_simple(r)) {
            Some(((r, moved), w)) => (r, false, moved + if w > weld { w } else { 0.0 }),
            None => {
                let (r, moved) = tidied(convex_hull(segs.iter().flat_map(|&(a, b)| [a, b]).collect()));
                (r, true, moved)
            }
        };
        if ring.len() < 3 {
            return Err(FootprintError::Empty);
        }
        if ring.len() <= MAX_CORNERS {
            // Lines are kept exactly; only curves are flattened.
            let error = if usable.iter().all(|c| matches!(c, Seg2::Line { .. })) { 0.0 } else { chord } + welded;
            return Ok(Footprint { shape: Shape2 { outer: ring, holes: Vec::new() }, error, hull });
        }
        chord *= 2.0;
    }
    Err(FootprintError::TooDetailed(usable.len()))
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Point ids for points welded within `tol`.
struct Welder {
    tol: f64,
    pts: Vec<Vec2>,
    grid: BTreeMap<(i64, i64), Vec<usize>>,
}

impl Welder {
    fn cell(&self, p: Vec2) -> (i64, i64) {
        // Coordinates are bounded (hlr::MAX_COORDINATE) and tol is a fraction of their spread.
        ((p.x / self.tol).floor() as i64, (p.y / self.tol).floor() as i64)
    }
    fn id(&mut self, p: Vec2) -> usize {
        let (x, y) = self.cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(ids) = self.grid.get(&(x + dx, y + dy)) {
                    for &i in ids {
                        if self.pts.get(i).is_some_and(|q| (q.x - p.x).hypot(q.y - p.y) <= self.tol) {
                            return i;
                        }
                    }
                }
            }
        }
        self.pts.push(p);
        self.grid.entry((x, y)).or_default().push(self.pts.len() - 1);
        self.pts.len() - 1
    }
}

/// The outer boundary of the drawing `segs`, counter-clockwise; `None` when it is not one simple
/// ring around every point of the drawing.
fn outer_ring(segs: &[(Vec2, Vec2)], tol: f64) -> Option<Vec<Vec2>> {
    let mut w = Welder { tol, pts: Vec::new(), grid: BTreeMap::new() };
    let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
    for &(a, b) in segs {
        let (i, j) = (w.id(a), w.id(b));
        if i != j {
            edges.insert((i.min(j), i.max(j)));
        }
    }
    // Split where an edge crosses another or a point lies on one, until nothing changes.
    for _ in 0..4 {
        let list: Vec<(usize, usize)> = edges.iter().copied().collect();
        let pt = |i: usize| w.pts.get(i).copied().unwrap_or(Vec2::ZERO);
        let mut order: Vec<usize> = (0..list.len()).collect();
        let lox = |e: (usize, usize)| pt(e.0).x.min(pt(e.1).x);
        order.sort_by(|&a, &b| lox(list[a]).total_cmp(&lox(list[b])));
        let mut cuts: BTreeMap<usize, Vec<Vec2>> = BTreeMap::new();
        for (k, &ia) in order.iter().enumerate() {
            let (a0, a1) = (pt(list[ia].0), pt(list[ia].1));
            let hix = a0.x.max(a1.x) + tol;
            for &ib in order.iter().skip(k + 1) {
                let (b0, b1) = (pt(list[ib].0), pt(list[ib].1));
                if b0.x.min(b1.x) > hix {
                    break;
                }
                if a0.y.max(a1.y) + tol < b0.y.min(b1.y) || b0.y.max(b1.y) + tol < a0.y.min(a1.y) {
                    continue;
                }
                for (e, p) in [(ia, b0), (ia, b1), (ib, a0), (ib, a1)] {
                    let (c, d) = (pt(list[e].0), pt(list[e].1));
                    if on_inside(p, c, d, tol) {
                        cuts.entry(e).or_default().push(p);
                    }
                }
                if let Some(x) = crossing(a0, a1, b0, b1, tol) {
                    cuts.entry(ia).or_default().push(x);
                    cuts.entry(ib).or_default().push(x);
                }
            }
        }
        if cuts.is_empty() {
            break;
        }
        let mut plans = Vec::new();
        for (e, mut at) in cuts {
            let Some(&(i, j)) = list.get(e) else { continue };
            let (a, b) = (pt(i), pt(j));
            let d = Vec2::new(b.x - a.x, b.y - a.y);
            at.sort_by(|p, q| ((p.x - a.x) * d.x + (p.y - a.y) * d.y).total_cmp(&((q.x - a.x) * d.x + (q.y - a.y) * d.y)));
            plans.push((i, j, at));
        }
        for (i, j, at) in plans {
            edges.remove(&(i, j));
            let mut prev = i;
            for id in at.into_iter().map(|p| w.id(p)).chain([j]) {
                if id != prev {
                    edges.insert((prev.min(id), prev.max(id)));
                }
                prev = id;
            }
        }
    }
    // Drop dangling edges: they are no part of an outline.
    let mut adj: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for &(i, j) in &edges {
        adj.entry(i).or_default().insert(j);
        adj.entry(j).or_default().insert(i);
    }
    let mut ends: Vec<usize> = adj.iter().filter(|(_, n)| n.len() < 2).map(|(&i, _)| i).collect();
    while let Some(i) = ends.pop() {
        let Some(ns) = adj.remove(&i) else { continue };
        for n in ns {
            if let Some(m) = adj.get_mut(&n) {
                m.remove(&i);
                if m.len() < 2 {
                    ends.push(n);
                }
            }
        }
    }
    let pt = |i: usize| w.pts.get(i).copied().unwrap_or(Vec2::ZERO);
    let start = *adj.keys().min_by(|&&a, &&b| pt(a).x.total_cmp(&pt(b).x).then(pt(a).y.total_cmp(&pt(b).y)))?;
    // Walk with the outside on the right: at each point leave by the first edge met turning
    // counter-clockwise from the way back. At the start the way back points straight up.
    let angle = |from: usize, to: usize| {
        let (a, b) = (pt(from), pt(to));
        (b.y - a.y).atan2(b.x - a.x)
    };
    let tau = std::f64::consts::TAU;
    let (mut prev_dir, mut at) = (std::f64::consts::FRAC_PI_2, start);
    let mut ring = vec![start];
    let mut first = None;
    for _ in 0..=2 * edges.len() + 2 {
        let next = adj.get(&at)?.iter().copied().min_by(|&a, &b| {
            let turn = |n: usize| {
                let t = (angle(at, n) - prev_dir).rem_euclid(tau);
                if t <= 1e-12 { tau } else { t }
            };
            turn(a).total_cmp(&turn(b))
        })?;
        match first {
            None => first = Some((at, next)),
            Some(f) if f == (at, next) => break,
            Some(_) => {}
        }
        prev_dir = angle(next, at);
        at = next;
        ring.push(at);
    }
    // The ring is closed on its start; a point met twice is a pinch.
    if ring.last() != Some(&start) {
        return None;
    }
    ring.pop();
    let unique: BTreeSet<usize> = ring.iter().copied().collect();
    if unique.len() != ring.len() || ring.len() < 3 {
        return None;
    }
    let poly: Vec<Vec2> = ring.iter().map(|&i| pt(i)).collect();
    let area: f64 = (0..poly.len()).map(|k| cross(Vec2::ZERO, poly[k], poly[(k + 1) % poly.len()])).sum();
    if area <= 0.0 {
        return None;
    }
    // Every point of the drawing must be inside or on the ring: else part of the shadow is apart.
    let inside = |p: Vec2| {
        let mut c = false;
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            if on_segment(p, a, b, tol) {
                return true;
            }
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                c = !c;
            }
        }
        c
    };
    if !adj.keys().chain(edges.iter().map(|e| &e.0)).all(|&i| inside(pt(i))) {
        return None;
    }
    Some(drop_straight(poly, tol))
}

fn on_segment(p: Vec2, a: Vec2, b: Vec2, tol: f64) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l = dx * dx + dy * dy;
    let t = if l > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
    (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy) <= tol
}

/// Whether `p` lies on segment c-d within `tol`, away from both its ends.
fn on_inside(p: Vec2, c: Vec2, d: Vec2, tol: f64) -> bool {
    (p.x - c.x).hypot(p.y - c.y) > tol && (p.x - d.x).hypot(p.y - d.y) > tol && on_segment(p, c, d, tol)
}

/// Where segments a and b cross, inside both and away from their ends.
fn crossing(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2, tol: f64) -> Option<Vec2> {
    let (d1, d2) = (cross(b0, b1, a0), cross(b0, b1, a1));
    let (d3, d4) = (cross(a0, a1, b0), cross(a0, a1, b1));
    if (d1 > 0.0) == (d2 > 0.0) || (d3 > 0.0) == (d4 > 0.0) || d1 == 0.0 || d2 == 0.0 || d3 == 0.0 || d4 == 0.0 {
        return None;
    }
    let t = d1 / (d1 - d2);
    let x = Vec2::new(a0.x + (a1.x - a0.x) * t, a0.y + (a1.y - a0.y) * t);
    let away = |p: Vec2| (x.x - p.x).hypot(x.y - p.y) > tol;
    (away(a0) && away(a1) && away(b0) && away(b1) && x.is_finite()).then_some(x)
}

/// The ring without corners that lie on the line through their neighbours.
fn drop_straight(mut ring: Vec<Vec2>, tol: f64) -> Vec<Vec2> {
    let mut k = 0;
    while ring.len() > 3 && k < ring.len() {
        let n = ring.len();
        let (a, b, c) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
        if on_segment(b, a, c, tol * 1e-3) {
            ring.remove(k);
            k = k.saturating_sub(1);
        } else {
            k += 1;
        }
    }
    ring
}

/// The convex hull of `pts`, counter-clockwise.
/// `ring` without corners closer than `tol` to the one before, or within `tol` of the line
/// through their neighbours: the outline moves by at most `tol`.
fn tidy(mut ring: Vec<Vec2>, tol: f64) -> Vec<Vec2> {
    loop {
        let n = ring.len();
        if n <= 3 {
            return ring;
        }
        let drop = (0..n).find(|&i| {
            let (a, p, b) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            let ab = (b.x - a.x).hypot(b.y - a.y);
            (p.x - a.x).hypot(p.y - a.y) <= tol || (ab > tol && cross(a, b, p).abs() / ab <= tol)
        });
        match drop {
            Some(i) => {
                ring.remove(i);
            }
            None => return ring,
        }
    }
}

fn convex_hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.retain(|p| p.is_finite());
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<Vec2> = Vec::new();
    for &p in &pts {
        while let [.., a, b] = lower[..] {
            if cross(a, b, p) <= 0.0 {
                lower.pop();
            } else {
                break;
            }
        }
        lower.push(p);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &p in pts.iter().rev() {
        while let [.., a, b] = upper[..] {
            if cross(a, b, p) <= 0.0 {
                upper.pop();
            } else {
                break;
            }
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
#[path = "footprint_tests.rs"]
mod tests;
