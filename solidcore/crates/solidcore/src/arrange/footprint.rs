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
//!
//! Moved from SolidCraft's `crates/geom/src/footprint.rs` as it was; curves are flattened by
//! the profile's own polyline (`develop::seg`) and checked as the cell grid checks them.

use std::collections::{BTreeMap, BTreeSet};

use crate::arrange::{Shape2, MAX_CORNERS};
use crate::cell_grid::check_seg;
use crate::develop::seg::SegMeasure;
use crate::hlr::{self, Brep, HlrError, Options, Placement, View};
use crate::math::{Vec2, Vec3};
use crate::prism::profile::Seg2;

/// The hidden-line options a shadow is prepared and drawn with: freeform faces included, as a
/// spline side casts a shadow like any other.
fn shadow_options() -> Options {
    Options {
        freeform: true,
        ..Options::default()
    }
}

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

/// Why a body has no footprint.
#[derive(Debug, thiserror::Error)]
pub enum FootprintError {
    /// Its curves could not be projected.
    #[error("{0}")]
    Hlr(#[from] HlrError),
    /// It has nothing to outline.
    #[error("the body has no edges to outline")]
    Empty,
    /// Its drawing has more pieces than [`MAX_PIECES`].
    #[error("the body is too detailed to outline ({0} pieces of drawing)")]
    TooDetailed(usize),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// The footprint of `body` turned by `rotation` (its columns: where the body's X, Y and Z axes
/// go), seen along -Z.
///
/// # Errors
/// As [`Shadow::new`] and [`footprint_of`].
pub fn footprint(
    body: &Brep,
    rotation: [Vec3; 3],
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Footprint, FootprintError> {
    Shadow::new(body, cancel)?.footprint(rotation, cancel)
}

/// A body made ready for its footprints: the costly part, done once however it is turned.
pub struct Shadow {
    def: hlr::Prepared,
}

impl Shadow {
    /// `body` made ready.
    ///
    /// # Errors
    /// [`FootprintError::Hlr`] when its geometry cannot be projected or the caller cancels.
    pub fn new(body: &Brep, cancel: &(dyn Fn() -> bool + Sync)) -> Result<Self, FootprintError> {
        // Whether a piece would be seen does not matter to a shadow: the curves are projected
        // with no hidden-line work, freeform faces included.
        Ok(Self {
            def: hlr::prepare_cancellable(body, &shadow_options(), cancel)?,
        })
    }

    /// The footprint of the body turned by `rotation`, as [`footprint`] finds it.
    ///
    /// # Errors
    /// As [`footprint_of`].
    pub fn footprint(
        &self,
        rotation: [Vec3; 3],
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<Footprint, FootprintError> {
        footprint_of(&[self], rotation, cancel)
    }
}

/// The footprint of several bodies moved as one (parts joined together, a component's bodies),
/// each turned by `rotation` about the one origin: the outline of all their drawings together.
/// Bodies that touch or overlap seen from above give the outline round them all; bodies apart
/// give the convex hull round them all, which covers more than they do, never less.
///
/// # Errors
/// [`FootprintError::Hlr`] when a curve cannot be projected, [`FootprintError::TooDetailed`]
/// past [`MAX_PIECES`], [`FootprintError::Cancelled`], and as [`outline_of`].
pub fn footprint_of(
    shadows: &[&Shadow],
    rotation: [Vec3; 3],
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Footprint, FootprintError> {
    let view = View::new(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 1.0, 0.0))?;
    let placed = Placement {
        body: 0,
        rotation,
        translation: Vec3::ZERO,
    };
    let mut curves = Vec::new();
    let mut most_sag: f64 = 0.0;
    for sh in shadows {
        let (more, sag) =
            hlr::projected_curves(&sh.def, &placed, &view, &shadow_options(), cancel)?;
        if cancel() {
            return Err(FootprintError::Cancelled);
        }
        if sag.is_finite() {
            most_sag = most_sag.max(sag);
        }
        curves.extend(more);
        if curves.len() > MAX_PIECES {
            return Err(FootprintError::TooDetailed(curves.len()));
        }
    }
    let mut fp = outline_of(&curves)?;
    fp.error += most_sag;
    Ok(fp)
}

/// The outer outline of a drawing made of `curves`, with the error of its flattening.
///
/// # Errors
/// [`FootprintError::Empty`] when no curve is usable or the outline has no area, and
/// [`FootprintError::TooDetailed`] when it cannot be flattened within [`MAX_CORNERS`].
pub fn outline_of(curves: &[Seg2]) -> Result<Footprint, FootprintError> {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut usable = Vec::new();
    for c in curves {
        if check_seg(c, hlr::MAX_COORDINATE).is_err() {
            continue;
        }
        for p in [c.start(), c.end(), c.point_at(0.5)] {
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
            segs.extend(pts.iter().zip(pts.iter().skip(1)).map(|(a, b)| (*a, *b)));
            if segs.len() > MAX_PIECES {
                return Err(FootprintError::TooDetailed(segs.len()));
            }
        }
        // Edges that nearly coincide in the view (a ring lying flat: its top and bottom circles)
        // can leave the walked outline touching itself; welding more coarsely joins them, and
        // the hull is the last resort. The weld used is part of the error.
        // A curve the drawing already gives as many short lines (a spline side chorded by the
        // hidden-line work) is flattened as a curve would be: corners within the chord of a
        // straight run go, and the chord is part of the error.
        let thinned = |r: Vec<Vec2>| {
            if r.len() > MAX_CORNERS {
                (thin(&r, chord), chord)
            } else {
                (r, 0.0)
            }
        };
        let mut found = None;
        for w in [weld, weld * 1e2, weld * 1e4] {
            if let Some((r, t)) = outer_ring(&segs, w)
                .map(thinned)
                .filter(|(r, _)| crate::arrange::is_simple(r))
            {
                found = Some((r, w, t));
                break;
            }
        }
        // Corners a hair apart, or a hair off the line through their neighbours, go: the layout
        // would take them for edges touching.
        let fine = 1e-7 * size;
        let tidied = |r: Vec<Vec2>| {
            let n = r.len();
            let t = tidy(&r, fine);
            let moved = if t.len() < n { fine } else { 0.0 };
            (t, moved)
        };
        let (ring, hull, welded) = if let Some(((r, moved), w, t)) = found
            .map(|(r, w, t)| (tidied(r), w, t))
            .filter(|((r, _), _, _)| crate::arrange::is_simple(r))
        {
            (r, false, moved + t + if w > weld { w } else { 0.0 })
        } else {
            let (r, t) = thinned(convex_hull(
                segs.iter().flat_map(|&(a, b)| [a, b]).collect(),
            ));
            let (r, moved) = tidied(r);
            (r, true, moved + t)
        };
        if ring.len() < 3 {
            return Err(FootprintError::Empty);
        }
        if ring.len() <= MAX_CORNERS {
            // Lines are kept exactly; only curves are flattened.
            let error = if usable.iter().all(|c| matches!(c, Seg2::Line { .. })) {
                0.0
            } else {
                chord
            } + welded;
            return Ok(Footprint {
                shape: Shape2 {
                    outer: ring,
                    holes: Vec::new(),
                },
                error,
                hull,
            });
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
        (
            (p.x / self.tol).floor() as i64,
            (p.y / self.tol).floor() as i64,
        )
    }
    fn id(&mut self, p: Vec2) -> usize {
        let (x, y) = self.cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(ids) = self.grid.get(&(x + dx, y + dy)) {
                    for &i in ids {
                        if self
                            .pts
                            .get(i)
                            .is_some_and(|q| (q.x - p.x).hypot(q.y - p.y) <= self.tol)
                        {
                            return i;
                        }
                    }
                }
            }
        }
        self.pts.push(p);
        self.grid
            .entry((x, y))
            .or_default()
            .push(self.pts.len() - 1);
        self.pts.len() - 1
    }
}

/// The outer boundary of the drawing `segs`, counter-clockwise; `None` when it is not one simple
/// ring around every point of the drawing.
fn outer_ring(segs: &[(Vec2, Vec2)], tol: f64) -> Option<Vec<Vec2>> {
    let mut w = Welder {
        tol,
        pts: Vec::new(),
        grid: BTreeMap::new(),
    };
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
        let ends = |e: usize| {
            list.get(e)
                .map_or((Vec2::ZERO, Vec2::ZERO), |&(i, j)| (pt(i), pt(j)))
        };
        let mut order: Vec<usize> = (0..list.len()).collect();
        let lox = |e: usize| {
            let (a, b) = ends(e);
            a.x.min(b.x)
        };
        order.sort_by(|&a, &b| lox(a).total_cmp(&lox(b)));
        let mut cuts: BTreeMap<usize, Vec<Vec2>> = BTreeMap::new();
        for (k, &ia) in order.iter().enumerate() {
            let (a0, a1) = ends(ia);
            let hix = a0.x.max(a1.x) + tol;
            for &ib in order.iter().skip(k + 1) {
                let (b0, b1) = ends(ib);
                if b0.x.min(b1.x) > hix {
                    break;
                }
                if a0.y.max(a1.y) + tol < b0.y.min(b1.y) || b0.y.max(b1.y) + tol < a0.y.min(a1.y) {
                    continue;
                }
                for (e, p) in [(ia, b0), (ia, b1), (ib, a0), (ib, a1)] {
                    let (c, d) = ends(e);
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
            at.sort_by(|p, q| {
                ((p.x - a.x) * d.x + (p.y - a.y) * d.y)
                    .total_cmp(&((q.x - a.x) * d.x + (q.y - a.y) * d.y))
            });
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
    let mut ends: Vec<usize> = adj
        .iter()
        .filter(|(_, n)| n.len() < 2)
        .map(|(&i, _)| i)
        .collect();
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
    let start = *adj.keys().min_by(|&&a, &&b| {
        pt(a)
            .x
            .total_cmp(&pt(b).x)
            .then(pt(a).y.total_cmp(&pt(b).y))
    })?;
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
                if t <= 1e-12 {
                    tau
                } else {
                    t
                }
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
    let sides = || {
        poly.iter()
            .zip(poly.iter().cycle().skip(1))
            .map(|(a, b)| (*a, *b))
    };
    let area: f64 = sides().map(|(a, b)| cross(Vec2::ZERO, a, b)).sum();
    if area <= 0.0 {
        return None;
    }
    // Every point of the drawing must be inside or on the ring: else part of the shadow is apart.
    let inside = |p: Vec2| {
        let mut c = false;
        for (a, b) in sides() {
            if on_segment(p, a, b, tol) {
                return true;
            }
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                c = !c;
            }
        }
        c
    };
    if !adj
        .keys()
        .chain(edges.iter().map(|e| &e.0))
        .all(|&i| inside(pt(i)))
    {
        return None;
    }
    Some(drop_straight(&poly, tol))
}

fn on_segment(p: Vec2, a: Vec2, b: Vec2, tol: f64) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l = dx * dx + dy * dy;
    let t = if l > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / l).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy) <= tol
}

/// Whether `p` lies on segment c-d within `tol`, away from both its ends.
fn on_inside(p: Vec2, c: Vec2, d: Vec2, tol: f64) -> bool {
    (p.x - c.x).hypot(p.y - c.y) > tol
        && (p.x - d.x).hypot(p.y - d.y) > tol
        && on_segment(p, c, d, tol)
}

/// Where segments a and b cross, inside both and away from their ends.
fn crossing(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2, tol: f64) -> Option<Vec2> {
    let (d1, d2) = (cross(b0, b1, a0), cross(b0, b1, a1));
    let (d3, d4) = (cross(a0, a1, b0), cross(a0, a1, b1));
    if (d1 > 0.0) == (d2 > 0.0)
        || (d3 > 0.0) == (d4 > 0.0)
        || d1 == 0.0
        || d2 == 0.0
        || d3 == 0.0
        || d4 == 0.0
    {
        return None;
    }
    let t = d1 / (d1 - d2);
    let x = Vec2::new(a0.x + (a1.x - a0.x) * t, a0.y + (a1.y - a0.y) * t);
    let away = |p: Vec2| (x.x - p.x).hypot(x.y - p.y) > tol;
    (away(a0) && away(a1) && away(b0) && away(b1) && x.x.is_finite() && x.y.is_finite())
        .then_some(x)
}

/// The ring without corners that lie on the line through their neighbours.
fn drop_straight(ring: &[Vec2], tol: f64) -> Vec<Vec2> {
    prune(ring, |a, b, c| on_segment(b, a, c, tol * 1e-3))
}

/// `ring` without corners closer than `tol` to the one before, or within `tol` of the line
/// through their neighbours: the outline moves by at most `tol`.
/// Most corners one straight run of [`thin`] takes in: it bounds the work at that many per
/// corner.
const THIN_RUN: usize = 256;

/// `ring` with the corners of each straight run taken off: walking from a kept corner, the run
/// grows while every corner it passes is within `tol` of the line from that corner to the run's
/// end, so no corner taken off is further than `tol` from the ring kept.
fn thin(ring: &[Vec2], tol: f64) -> Vec<Vec2> {
    let n = ring.len();
    let at = |i: usize| ring.get(i % n.max(1)).copied().unwrap_or(Vec2::ZERO);
    let near = |a: Vec2, b: Vec2, p: Vec2| {
        let ab = (b.x - a.x).hypot(b.y - a.y);
        if ab > tol {
            cross(a, b, p).abs() / ab <= tol
        } else {
            (p.x - a.x).hypot(p.y - a.y) <= tol
        }
    };
    let mut kept = Vec::new();
    let mut i = 0;
    while i < n {
        let a = at(i);
        kept.push(a);
        // The furthest end the run from `a` can reach, back round to the first corner at most.
        let mut end = i + 1;
        while end < n && end - i < THIN_RUN {
            let b = at(end + 1);
            if !(i + 1..=end).all(|k| near(a, b, at(k))) {
                break;
            }
            end += 1;
        }
        i = end;
    }
    kept
}

fn tidy(ring: &[Vec2], tol: f64) -> Vec<Vec2> {
    prune(ring, |a, p, b| {
        let ab = (b.x - a.x).hypot(b.y - a.y);
        (p.x - a.x).hypot(p.y - a.y) <= tol || (ab > tol && cross(a, b, p).abs() / ab <= tol)
    })
}

/// `ring` with corners taken off while `drop(before, corner, after)` says so, first corner first,
/// down to three: a corner is looked at again only once a neighbour of it goes.
fn prune(ring: &[Vec2], drop: impl Fn(Vec2, Vec2, Vec2) -> bool) -> Vec<Vec2> {
    let n = ring.len();
    let mut prev: Vec<usize> = (0..n)
        .map(|i| if i == 0 { n.saturating_sub(1) } else { i - 1 })
        .collect();
    let mut next: Vec<usize> = (0..n).map(|i| if i + 1 == n { 0 } else { i + 1 }).collect();
    let mut alive = vec![true; n];
    let mut left = n;
    let mut pending: BTreeSet<usize> = (0..n).collect();
    let at = |i: usize| ring.get(i).copied().unwrap_or(Vec2::ZERO);
    while left > 3 {
        let Some(i) = pending.pop_first() else { break };
        let (Some(&p), Some(&q)) = (prev.get(i), next.get(i)) else {
            break;
        };
        if !alive.get(i).copied().unwrap_or(false) || !drop(at(p), at(i), at(q)) {
            continue;
        }
        if let Some(a) = alive.get_mut(i) {
            *a = false;
        }
        if let Some(np) = next.get_mut(p) {
            *np = q;
        }
        if let Some(pq) = prev.get_mut(q) {
            *pq = p;
        }
        left -= 1;
        pending.insert(p);
        pending.insert(q);
    }
    ring.iter()
        .zip(&alive)
        .filter(|(_, a)| **a)
        .map(|(p, _)| *p)
        .collect()
}

fn convex_hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.retain(|p| p.x.is_finite() && p.y.is_finite());
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<Vec2> = Vec::new();
    for &p in &pts {
        while let [.., a, b] = *lower.as_slice() {
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
        while let [.., a, b] = *upper.as_slice() {
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
mod tests;
