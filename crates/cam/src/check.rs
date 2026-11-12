//! Safety rules every toolpath must keep, checked before a path leaves an operation.

use std::collections::HashMap;

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::ops::Heights;
use crate::path::{MAX_MOVES, Move, PlaneArc, Toolpath};
use crate::surface_ops::ENTRY_GAP;
use crate::{CamError, Result};

const TOL: f64 = 1e-6;

fn fail<T>(i: usize, why: impl std::fmt::Display) -> Result<T> {
    Err(CamError::Unsafe(format!("move {i}: {why}")))
}

fn xy(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// Side of the cells cutting moves are filed under (mm).
const CELL: f64 = 2.0;
/// Most cells one cutting move is filed under; a longer one is not looked for.
const MAX_CELLS: i64 = 1024;

/// The straight cutting moves so far, filed by where they are seen from above, to find how
/// deep the tool has been at a point.
#[derive(Default)]
pub(crate) struct Swept {
    cells: HashMap<(i64, i64), Vec<(Vec3, Vec3)>>,
}

impl Swept {
    fn cell(v: f64) -> Option<i64> {
        let c = (v / CELL).floor();
        (c.abs() < 1e15).then_some(c as i64)
    }

    pub(crate) fn add(&mut self, a: Vec3, b: Vec3) {
        let (Some(x0), Some(x1), Some(y0), Some(y1)) =
            (Self::cell(a.x.min(b.x)), Self::cell(a.x.max(b.x)), Self::cell(a.y.min(b.y)), Self::cell(a.y.max(b.y)))
        else {
            return;
        };
        if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_CELLS {
            return;
        }
        for i in x0..=x1 {
            for j in y0..=y1 {
                self.cells.entry((i, j)).or_default().push((a, b));
            }
        }
    }

    /// Whether a cutting move so far went through `p` (seen from above) below `z`.
    pub(crate) fn below(&self, p: Vec2, z: f64) -> bool {
        let (Some(i), Some(j)) = (Self::cell(p.x), Self::cell(p.y)) else { return false };
        self.cells.get(&(i, j)).is_some_and(|segs| {
            segs.iter().any(|&(a, b)| {
                let (a2, b2) = (xy(a), xy(b));
                let d = b2 - a2;
                let len2 = d.dot(d);
                // A move straight down or up went through its point as low as its lower end.
                let (u, low) = if len2 > 1e-24 {
                    let u = ((p - a2).dot(d) / len2).clamp(0.0, 1.0);
                    (u, a.z + (b.z - a.z) * u)
                } else {
                    (0.0, a.z.min(b.z))
                };
                a2.lerp(b2, u).dist(p) <= 1e-6 && low < z - 1e-9
            })
        })
    }
}

/// Raises every rapid of `path` straight down that stops below `top` + [`ENTRY_GAP`] where
/// the path has not yet cut through that point lower down, to there (no higher than it
/// started): `top` is where the material may reach (the stock's top, which may be above the
/// operation's own `heights.top`), and the feed after it then comes down the rest of the way.
/// A rapid down into what the path has already cut (a level coming down through the levels
/// above it) is left as it is.
pub fn lift_entries(path: &mut Toolpath, top: f64) {
    if !top.is_finite() {
        return;
    }
    let air = top + ENTRY_GAP;
    let mut swept = Swept::default();
    let mut pos: Option<Vec3> = None;
    let mut keep = vec![true; path.moves.len()];
    for (i, m) in path.moves.iter_mut().enumerate() {
        match m {
            Move::Rapid { to } => {
                if let Some(from) = pos
                    && xy(from).dist(xy(*to)) <= 1e-9
                    && to.z < from.z
                    && to.z < air - 1e-9
                    && !swept.below(xy(*to), to.z)
                {
                    to.z = air.min(from.z);
                    if to.z >= from.z - 1e-12
                        && let Some(k) = keep.get_mut(i)
                    {
                        *k = false;
                    }
                }
            }
            Move::Feed { to, .. } => {
                if let Some(from) = pos {
                    swept.add(from, *to);
                }
            }
            Move::Arc { to, .. } | Move::PlaneArc { to, .. } => {
                if let Some(from) = pos {
                    swept.add(from, from);
                }
                swept.add(*to, *to);
            }
            Move::Dwell { .. } => {}
        }
        if let Some(t) = m.target() {
            pos = Some(t);
        }
    }
    let mut k = keep.iter();
    path.moves.retain(|_| k.next().copied().unwrap_or(true));
}

/// Checks the rules:
/// - every coordinate is finite and every feed is above zero;
/// - the path starts with a rapid to the clearance height or above;
/// - a rapid that moves in XY stays at the retract height or above;
/// - a rapid below the retract height is vertical and stops at or above the top of the
///   material, above the deepest point already cut at that position (a drill returning into
///   its hole), or above a straight cutting move that went through that position before (a
///   level coming down to the one it cut);
/// - no move goes below the bottom height;
/// - an arc starts and ends at the same distance from its centre.
pub fn verify(path: &Toolpath, h: &Heights) -> Result<()> {
    verify_against(path, h, None)
}

/// Where a cutter resting on the part at a point (seen from above) has its tip: `None` where
/// nothing is under it.
pub(crate) type RestAt<'a> = &'a dyn Fn(Vec2) -> Result<Option<f64>>;

/// [`verify`] for a finishing path whose `heights.top` says nothing of where the material is
/// (a parallel or waterline finish, whose top may be below the part): a rapid below the
/// retract height that is not back into a cut must stop at or above where the cutter rests on
/// the part there (`rest`), not merely at or above `heights.top`.
pub(crate) fn verify_over(path: &Toolpath, h: &Heights, rest: RestAt<'_>) -> Result<()> {
    verify_against(path, h, Some(rest))
}

fn verify_against(path: &Toolpath, h: &Heights, rest: Option<RestAt<'_>>) -> Result<()> {
    // Whether a rapid stopping at `p` stops in the material: below `heights.top`, or with
    // `rest`, below where the cutter rests on the part.
    let in_material = |p: Vec3| -> Result<bool> {
        Ok(match rest {
            None => p.z < h.top - TOL,
            Some(f) => f(xy(p))?.is_some_and(|z| p.z < z - TOL),
        })
    };
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let mut pos: Option<Vec3> = None;
    // Deepest Z cut at the current XY position since the tool arrived there.
    let mut cut_here: Option<f64> = None;
    let mut swept = Swept::default();
    for (i, m) in path.moves.iter().enumerate() {
        if let Some(to) = m.target() {
            if !to.is_finite() {
                return fail(i, "a coordinate is not a finite number");
            }
            if to.z < h.bottom - TOL {
                return fail(i, format!("Z {} is below the bottom height {}", to.z, h.bottom));
            }
        }
        match *m {
            Move::Rapid { to } => {
                let Some(from) = pos else {
                    if to.z < h.clearance - TOL {
                        return fail(i, "the first move must go to the clearance height");
                    }
                    pos = Some(to);
                    continue;
                };
                if xy(from).dist(xy(to)) > 1e-9 {
                    if from.z < h.retract - TOL || to.z < h.retract - TOL {
                        return fail(i, "a rapid moves sideways below the retract height");
                    }
                    cut_here = None;
                } else if to.z < h.retract - TOL && !cut_here.is_some_and(|z| to.z > z + 1e-9) && !swept.below(xy(to), to.z) && in_material(to)? {
                    return fail(i, "a rapid goes down into material that has not been cut");
                }
            }
            Move::Feed { to, feed } => {
                if pos.is_none() {
                    return fail(i, "the path must start with a rapid");
                }
                if !(feed > 0.0) || !feed.is_finite() {
                    return fail(i, "the feed must be above 0");
                }
                let moved = pos.is_some_and(|from| xy(from).dist(xy(to)) > 1e-9);
                cut_here = Some(if moved { to.z } else { cut_here.map_or(to.z, |z| z.min(to.z)) });
                if let Some(from) = pos {
                    swept.add(from, to);
                }
            }
            Move::Arc { to, center, feed, .. } => {
                let Some(from) = pos else { return fail(i, "the path must start with a rapid") };
                if !(feed > 0.0) || !feed.is_finite() {
                    return fail(i, "the feed must be above 0");
                }
                let (r0, r1) = (xy(from).dist(center), xy(to).dist(center));
                if !center.is_finite() || !(r0 > 0.0) || (r0 - r1).abs() > 1e-3 {
                    return fail(i, "an arc does not start and end at the same distance from its centre");
                }
                cut_here = Some(to.z);
                // An arc is filed by its ends: its chord is not where it cut.
                swept.add(from, from);
                swept.add(to, to);
            }
            Move::PlaneArc { to, center, plane, ccw, feed } => {
                let Some(from) = pos else { return fail(i, "the path must start with a rapid") };
                if !(feed > 0.0) || !feed.is_finite() {
                    return fail(i, "the feed must be above 0");
                }
                let arc = PlaneArc { from, to, center, plane, ccw };
                let (r0, r1) = (arc.radius(), (plane.split(to).0 - plane.split(center).0).len());
                if !center.is_finite() || !(r0 > 0.0) || (r0 - r1).abs() > 1e-3 {
                    return fail(i, "an arc does not start and end at the same distance from its centre");
                }
                // A vertical arc may dip below its ends.
                if arc.points(1e-4).iter().any(|q| q.z < h.bottom - TOL) {
                    return fail(i, format!("an arc goes below the bottom height {}", h.bottom));
                }
                cut_here = Some(to.z);
                swept.add(from, from);
                swept.add(to, to);
            }
            Move::Dwell { seconds } => {
                if !(seconds >= 0.0) || !seconds.is_finite() {
                    return fail(i, "a dwell must be 0 seconds or longer");
                }
            }
        }
        if let Some(to) = m.target() {
            pos = Some(to);
        }
    }
    Ok(())
}
