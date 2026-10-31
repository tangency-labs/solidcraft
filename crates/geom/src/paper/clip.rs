//! Clips (#4890 A8): an object's items cut to its [`Clip`], a rectangle or a circle, keeping what
//! lies inside. [`Scene::prepare_clipped`](super::Scene::prepare_clipped) does it once, before any
//! writer, so every writer draws the same clip-free groups.
//!
//! - Each piece of a stroke is cut where it crosses the clip's edge, and a part is kept when its
//!   middle is inside; on the edge, within [`EPS`], counts as inside. A line or an arc is cut where
//!   its equation says. Any other curve is halved ([`Seg2::part`]) while its control points lie
//!   either side of the edge (widened by [`EPS`]), until they span at most [`CUT_MM`], and is cut
//!   there: at most [`MAX_HALVINGS`] halvings a piece. Each part is the same kind of piece as the
//!   whole.
//! - Kept parts that follow one another stay one stroke, open. A stroke wholly inside is kept as it
//!   is, closed if it was. A closed stroke cut open rejoins across its seam, so the parts on either
//!   side of it are one line with one join.
//! - A dashed stroke keeps its pattern: each part's phase is where along the whole stroke it
//!   starts, so its dashes fall where they did. Across the seam of a closed one the pattern is
//!   continuous only when the stroke's length is a whole number of patterns from its phase, so the
//!   parts rejoin there only then.
//! - Parts no longer than [`EPS`], such as a touch at a tangent, are dropped.
//! - A fill wholly inside is kept and one wholly outside is dropped; one that crosses the edge, or
//!   lies round the whole clip, isn't supported yet.

use std::f64::consts::TAU;

use super::{Clip, Dashes, Group, Invalid, Item, Result};
use crate::{EPS, Region2, Seg2, Vec2};

/// How closely a curve's crossing of a clip's edge is found: the span of its last part's control
/// points, in millimetres.
pub const CUT_MM: f64 = 1e-9;
/// The most halvings one piece takes to find where it crosses a clip's edge: each crossing takes
/// about 50 (1e5 mm halved to 1e-9 mm), and a degree-9 piece crosses a rectangle at most 36 times.
pub const MAX_HALVINGS: usize = 1 << 12;

impl Clip {
    /// Whether `p` is inside, or on the edge within [`EPS`].
    fn holds(&self, p: Vec2) -> bool {
        match *self {
            Clip::Rect { min, max } => p.x >= min.x - EPS && p.x <= max.x + EPS && p.y >= min.y - EPS && p.y <= max.y + EPS,
            Clip::Circle { center, radius } => p.dist(center) <= radius + EPS,
        }
    }

    /// A point inside.
    fn middle(&self) -> Vec2 {
        match *self {
            Clip::Rect { min, max } => (min + max) / 2.0,
            Clip::Circle { center, .. } => center,
        }
    }

    /// Where `seg` crosses the clip's edge, as parameters in (0, 1) in order.
    fn cuts(&self, seg: &Seg2) -> Result<Vec<f64>> {
        let mut cuts = match (*self, seg) {
            (Clip::Rect { min, max }, Seg2::Line { a, b }) => {
                let d = *b - *a;
                [(a.x, d.x, min.x), (a.x, d.x, max.x), (a.y, d.y, min.y), (a.y, d.y, max.y)]
                    .into_iter()
                    .filter(|&(_, d, _)| d != 0.0)
                    .map(|(from, d, to)| (to - from) / d)
                    .collect()
            }
            (Clip::Circle { center, radius }, Seg2::Line { a, b }) => {
                // From the line's point nearest the centre, half the chord either way. A line within
                // EPS of touching the circle only touches it: one cut there, never a sliver of chord.
                let (d, f) = (*b - *a, *a - center);
                let t = -f.dot(d) / d.dot(d);
                let gap = radius - (f + d * t).len();
                if gap.is_nan() || gap < 0.0 {
                    Vec::new()
                } else if gap <= EPS {
                    vec![t]
                } else {
                    let half = (radius * radius - (f + d * t).len2()).max(0.0).sqrt() / d.len();
                    vec![t - half, t + half]
                }
            }
            (clip, &Seg2::Arc { center, radius, start, sweep }) => {
                let along = |angle: f64| (angle - start).rem_euclid(TAU) / sweep;
                let backwards = |angle: f64| (start - angle).rem_euclid(TAU) / -sweep;
                let angles = arc_angles(clip, center, radius);
                angles.into_iter().map(|angle| if sweep > 0.0 { along(angle) } else { backwards(angle) }).collect()
            }
            _ => self.halved(seg)?,
        };
        // A piece is split no nearer its ends, or another cut, than `MIN_SPLIT`: cuts closer than
        // that are one cut.
        cuts.retain(|t: &f64| t.is_finite() && *t > Seg2::MIN_SPLIT && *t < 1.0 - Seg2::MIN_SPLIT);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|later, kept| *later - *kept <= Seg2::MIN_SPLIT);
        Ok(cuts)
    }

    /// Where a curve crosses the edge, by halving it while its control points lie either side.
    fn halved(&self, seg: &Seg2) -> Result<Vec<f64>> {
        let (mut cuts, mut todo, mut halvings) = (Vec::new(), vec![(*seg, 0.0, 1.0)], 0);
        while let Some((piece, t0, t1)) = todo.pop() {
            let (lo, hi) = span(&piece);
            if !self.straddles(&piece, lo, hi) {
                continue;
            }
            if lo.dist(hi) <= CUT_MM {
                // A conic's halves are re-normalised (`Seg2::split_at`), so their parameters aren't the
                // whole's: its cut is found by the point. Every other piece's is the range's middle.
                let t = if matches!(seg, Seg2::Conic { .. }) { seg.closest_param(piece.point_at(0.5)) } else { (t0 + t1) / 2.0 };
                cuts.push(t);
                continue;
            }
            halvings += 1;
            if halvings > MAX_HALVINGS {
                return Err(Invalid("not supported yet: a curve this close along a clip's edge"));
            }
            let (left, right) = piece.split_at(0.5);
            let t = (t0 + t1) / 2.0;
            todo.extend([(right, t, t1), (left, t0, t)]);
        }
        Ok(cuts)
    }

    /// Whether the control points of `piece`, spanning `lo` to `hi`, may lie either side of the
    /// edge widened by [`EPS`]: the piece lies within its control points.
    fn straddles(&self, piece: &Seg2, lo: Vec2, hi: Vec2) -> bool {
        match *self {
            Clip::Rect { min, max } => {
                let (min, max) = (min - Vec2::new(EPS, EPS), max + Vec2::new(EPS, EPS));
                let across = |at: f64, lo: f64, hi: f64| lo <= at && at <= hi;
                let beside = |lo: f64, hi: f64, min: f64, max: f64| lo <= max && hi >= min;
                ((across(min.x, lo.x, hi.x) || across(max.x, lo.x, hi.x)) && beside(lo.y, hi.y, min.y, max.y))
                    || ((across(min.y, lo.y, hi.y) || across(max.y, lo.y, hi.y)) && beside(lo.x, hi.x, min.x, max.x))
            }
            Clip::Circle { center, radius } => {
                let r = radius + EPS;
                let nearest = Vec2::new(center.x.clamp(lo.x, hi.x), center.y.clamp(lo.y, hi.y));
                let farthest = controls(piece).iter().map(|p| p.dist(center)).fold(0.0, f64::max);
                nearest.dist(center) <= r && farthest >= r
            }
        }
    }
}

/// A curve's control points: it lies within them (a conic's weight is above 0).
fn controls(seg: &Seg2) -> Vec<Vec2> {
    match *seg {
        Seg2::Line { a, b } => vec![a, b],
        Seg2::Cubic { p0, p1, p2, p3 } => vec![p0, p1, p2, p3],
        Seg2::Conic { a, apex, b, .. } => vec![a, apex, b],
        Seg2::Bezier { n, ref p } => p.iter().take(usize::from(n) + 1).copied().collect(),
        // Never halved: cut where its equation says.
        Seg2::Arc { .. } => Vec::new(),
    }
}

/// The corners of the box round a curve's control points.
fn span(seg: &Seg2) -> (Vec2, Vec2) {
    let points = controls(seg);
    let lo = points.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |m, p| Vec2::new(m.x.min(p.x), m.y.min(p.y)));
    let hi = points.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |m, p| Vec2::new(m.x.max(p.x), m.y.max(p.y)));
    (lo, hi)
}

/// The angles round `center` where the circle of `radius` there meets `clip`'s edge.
fn arc_angles(clip: Clip, center: Vec2, radius: f64) -> Vec<f64> {
    match clip {
        Clip::Rect { min, max } => {
            let mut angles = Vec::new();
            // An edge within EPS of touching the circle only touches it: one angle there.
            let touches = |c: f64| radius - c.abs() * radius <= EPS;
            for x in [min.x, max.x] {
                let c = (x - center.x) / radius;
                if c.abs() <= 1.0 {
                    angles.extend(if touches(c) { vec![c.signum().acos()] } else { vec![c.acos(), -c.acos()] });
                }
            }
            for y in [min.y, max.y] {
                let s = (y - center.y) / radius;
                if s.abs() <= 1.0 {
                    angles.extend(if touches(s) { vec![s.signum().asin()] } else { vec![s.asin(), std::f64::consts::PI - s.asin()] });
                }
            }
            angles
        }
        Clip::Circle { center: other, radius: r } => {
            let apart = other - center;
            let d = apart.len();
            if d == 0.0 || d > radius + r || d < (radius - r).abs() {
                return Vec::new();
            }
            // The cosine rule: the angle at `center` between `other` and either crossing. Circles within
            // EPS of touching only touch: one angle there.
            let half = ((radius * radius + d * d - r * r) / (2.0 * radius * d)).clamp(-1.0, 1.0).acos();
            if (radius + r - d).min(d - (radius - r).abs()) <= EPS {
                return vec![apart.angle() + half];
            }
            vec![apart.angle() - half, apart.angle() + half]
        }
    }
}

/// `group` with its items cut to `clip`.
pub(super) fn clipped(group: &Group, clip: &Clip) -> Result<Group> {
    let mut items = Vec::new();
    for item in &group.items {
        match item {
            Item::Stroke { path, closed, width_mm, pattern } => items.extend(stroke(clip, path, *closed, *width_mm, pattern.as_ref())?),
            Item::Fill { region } => {
                if inside(clip, region)? {
                    items.push(item.clone());
                }
            }
        }
    }
    Ok(Group { object: group.object, kind: group.kind, items })
}

/// Whether a fill is wholly inside `clip` (kept) or wholly outside (dropped): every part of its
/// edges between where they meet the clip's edge lies on one side.
fn inside(clip: &Clip, region: &Region2) -> Result<bool> {
    const CROSSING: Invalid = Invalid("not supported yet: a fill crossing a clip's edge");
    let mut all = None;
    for seg in std::iter::once(&region.outer).chain(&region.holes).flat_map(|l| &l.segs) {
        let mut bounds = vec![0.0];
        bounds.extend(clip.cuts(seg)?);
        bounds.push(1.0);
        for pair in bounds.windows(2) {
            let here = clip.holds(seg.part(pair[0], pair[1]).point_at(0.5));
            if all.is_some_and(|all| all != here) {
                return Err(CROSSING);
            }
            all = Some(here);
        }
    }
    // Wholly outside, its edge may still go round the clip.
    match all {
        Some(false) if region.contains(clip.middle()) => Err(CROSSING),
        all => Ok(all.unwrap_or(false)),
    }
}

/// A run of kept parts: how far along the stroke it starts, and its pieces.
struct Run {
    from_mm: f64,
    path: Vec<Seg2>,
}

/// The parts of a stroke inside `clip`, as strokes in order.
fn stroke(clip: &Clip, path: &[Seg2], closed: bool, width_mm: f64, pattern: Option<&Dashes>) -> Result<Vec<Item>> {
    let mut runs: Vec<Run> = Vec::new();
    // `going`: the last part kept reached the end of its piece, or only parts too short to keep
    // came after it, so the next kept part carries its run on.
    let (mut along, mut going, mut whole) = (0.0, false, true);
    for seg in path {
        let mut bounds = vec![0.0];
        bounds.extend(clip.cuts(seg)?);
        bounds.push(1.0);
        for pair in bounds.windows(2) {
            let piece = seg.part(pair[0], pair[1]);
            let length = piece.length();
            let from_mm = along;
            along += length;
            if !clip.holds(piece.point_at(0.5)) {
                (going, whole) = (false, false);
                continue;
            }
            if length <= EPS {
                continue;
            }
            match runs.last_mut() {
                Some(run) if going => run.path.push(piece),
                _ => runs.push(Run { from_mm, path: vec![piece] }),
            }
            going = true;
        }
    }
    let dashes = |from_mm: f64| {
        pattern.map(|d| {
            let period: f64 = d.lengths_mm.iter().sum();
            Dashes { lengths_mm: d.lengths_mm.clone(), phase_mm: (d.phase_mm + from_mm).rem_euclid(period) }
        })
    };
    if whole {
        return Ok(vec![Item::Stroke { path: path.to_vec(), closed, width_mm, pattern: dashes(0.0) }]);
    }
    // Across the seam: the run that reached the end, then the one that started at the start.
    let continuous = pattern.is_none_or(|d| {
        let period: f64 = d.lengths_mm.iter().sum();
        let drift = along.rem_euclid(period);
        drift <= EPS || drift >= period - EPS
    });
    if closed && going && continuous && runs.len() > 1 && runs.first().is_some_and(|run| run.from_mm <= EPS) {
        let first = runs.remove(0);
        if let Some(last) = runs.last_mut() {
            last.path.extend(first.path);
        }
    }
    Ok(runs.into_iter().map(|run| Item::Stroke { path: run.path, closed: false, width_mm, pattern: dashes(run.from_mm) }).collect())
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
