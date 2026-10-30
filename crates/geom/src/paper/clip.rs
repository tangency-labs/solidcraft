//! Clips (#4890 A8a): an object's strokes cut to its [`Clip`], a rectangle or a circle, keeping what
//! lies inside. [`Scene::prepare_clipped`](super::Scene::prepare_clipped) does it once, before any
//! writer, so every writer draws the same clip-free groups.
//!
//! - Each piece, a line or an arc, is cut where it crosses the clip's edge, and a part is kept when
//!   its middle is inside; on the edge, within [`EPS`], counts as inside.
//! - Kept parts that follow one another stay one stroke, open. A stroke wholly inside is kept as it
//!   is, closed if it was. A closed stroke cut open rejoins across its seam, so the parts on either
//!   side of it are one line with one join.
//! - A dashed stroke keeps its pattern: each part's phase is where along the whole stroke it
//!   starts, so its dashes fall where they did. Across the seam of a closed one the pattern is
//!   continuous only when the stroke's length is a whole number of patterns from its phase, so the
//!   parts rejoin there only then.
//! - Parts no longer than [`EPS`], such as a touch at a tangent, are dropped.

use std::f64::consts::TAU;

use super::{Clip, Dashes, Group, Invalid, Item, Result};
use crate::{EPS, Seg2, Vec2};

impl Clip {
    /// Whether `p` is inside, or on the edge within [`EPS`].
    fn holds(&self, p: Vec2) -> bool {
        match *self {
            Clip::Rect { min, max } => p.x >= min.x - EPS && p.x <= max.x + EPS && p.y >= min.y - EPS && p.y <= max.y + EPS,
            Clip::Circle { center, radius } => p.dist(center) <= radius + EPS,
        }
    }

    /// Where `seg` crosses the clip's edge, as parameters in (0, 1) in order. Pieces other than
    /// lines and arcs have none: a clipped group holds no others.
    fn cuts(&self, seg: &Seg2) -> Vec<f64> {
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
                // |a + t (b - a) - center|² = radius², a quadratic in t.
                let (d, f) = (*b - *a, *a - center);
                let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - radius * radius);
                let disc = qb * qb - 4.0 * qa * qc;
                if qa > 0.0 && disc >= 0.0 { [-1.0, 1.0].map(|s| (-qb + s * disc.sqrt()) / (2.0 * qa)).to_vec() } else { Vec::new() }
            }
            (clip, &Seg2::Arc { center, radius, start, sweep }) => {
                let along = |angle: f64| (angle - start).rem_euclid(TAU) / sweep;
                let backwards = |angle: f64| (start - angle).rem_euclid(TAU) / -sweep;
                let angles = arc_angles(clip, center, radius);
                angles.into_iter().map(|angle| if sweep > 0.0 { along(angle) } else { backwards(angle) }).collect()
            }
            _ => Vec::new(),
        };
        cuts.retain(|t: &f64| t.is_finite() && *t > 0.0 && *t < 1.0);
        cuts.sort_by(f64::total_cmp);
        cuts
    }
}

/// The angles round `center` where the circle of `radius` there meets `clip`'s edge.
fn arc_angles(clip: Clip, center: Vec2, radius: f64) -> Vec<f64> {
    match clip {
        Clip::Rect { min, max } => {
            let mut angles = Vec::new();
            for x in [min.x, max.x] {
                let c = (x - center.x) / radius;
                if c.abs() <= 1.0 {
                    angles.extend([c.acos(), -c.acos()]);
                }
            }
            for y in [min.y, max.y] {
                let s = (y - center.y) / radius;
                if s.abs() <= 1.0 {
                    angles.extend([s.asin(), std::f64::consts::PI - s.asin()]);
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
            // The cosine rule: the angle at `center` between `other` and either crossing.
            let half = ((radius * radius + d * d - r * r) / (2.0 * radius * d)).clamp(-1.0, 1.0).acos();
            vec![apart.angle() - half, apart.angle() + half]
        }
    }
}

/// The part of `seg` from parameter `t0` to `t1`, its ends exactly the piece's own where it has them.
fn part(seg: &Seg2, t0: f64, t1: f64) -> Seg2 {
    match *seg {
        Seg2::Line { a, b } => {
            let at = |t: f64| {
                if t == 0.0 {
                    a
                } else if t == 1.0 {
                    b
                } else {
                    a.lerp(b, t)
                }
            };
            Seg2::Line { a: at(t0), b: at(t1) }
        }
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep * t0, sweep: sweep * (t1 - t0) },
        _ => *seg,
    }
}

/// `group` with its items cut to `clip`: strokes of lines and arcs only, as yet.
pub(super) fn clipped(group: &Group, clip: &Clip) -> Result<Group> {
    let mut items = Vec::new();
    for item in &group.items {
        match item {
            Item::Stroke { path, closed, width_mm, pattern } if path.iter().all(|seg| matches!(seg, Seg2::Line { .. } | Seg2::Arc { .. })) => {
                items.extend(stroke(clip, path, *closed, *width_mm, pattern.as_ref()));
            }
            _ => return Err(Invalid("a clipped object's items are strokes of lines and arcs")),
        }
    }
    Ok(Group { object: group.object, kind: group.kind, items })
}

/// A run of kept parts: how far along the stroke it starts, and its pieces.
struct Run {
    from_mm: f64,
    path: Vec<Seg2>,
}

/// The parts of a stroke inside `clip`, as strokes in order.
fn stroke(clip: &Clip, path: &[Seg2], closed: bool, width_mm: f64, pattern: Option<&Dashes>) -> Vec<Item> {
    let mut runs: Vec<Run> = Vec::new();
    // `going`: the last part kept reached the end of its piece, or only parts too short to keep
    // came after it, so the next kept part carries its run on.
    let (mut along, mut going, mut whole) = (0.0, false, true);
    for seg in path {
        let length = seg.length();
        let mut bounds = vec![0.0];
        bounds.extend(clip.cuts(seg));
        bounds.push(1.0);
        for pair in bounds.windows(2) {
            let [t0, t1] = [pair[0], pair[1]];
            let piece = part(seg, t0, t1);
            if !clip.holds(piece.point_at(0.5)) {
                (going, whole) = (false, false);
                continue;
            }
            if length * (t1 - t0) <= EPS {
                continue;
            }
            match runs.last_mut() {
                Some(run) if going => run.path.push(piece),
                _ => runs.push(Run { from_mm: along + length * t0, path: vec![piece] }),
            }
            going = true;
        }
        along += length;
    }
    let dashes = |from_mm: f64| {
        pattern.map(|d| {
            let period: f64 = d.lengths_mm.iter().sum();
            Dashes { lengths_mm: d.lengths_mm.clone(), phase_mm: (d.phase_mm + from_mm).rem_euclid(period) }
        })
    };
    if whole {
        return vec![Item::Stroke { path: path.to_vec(), closed, width_mm, pattern: dashes(0.0) }];
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
    runs.into_iter().map(|run| Item::Stroke { path: run.path, closed: false, width_mm, pattern: dashes(run.from_mm) }).collect()
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
