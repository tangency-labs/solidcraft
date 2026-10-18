//! Safety rules every toolpath must keep, checked before a path leaves an operation.

use solidcraft_geom::{Vec2, Vec3};

use crate::ops::Heights;
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::{CamError, Result};

const TOL: f64 = 1e-6;

fn fail<T>(i: usize, why: impl std::fmt::Display) -> Result<T> {
    Err(CamError::Unsafe(format!("move {i}: {why}")))
}

fn xy(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// Checks the rules:
/// - every coordinate is finite and every feed is above zero;
/// - the path starts with a rapid to the clearance height or above;
/// - a rapid that moves in XY stays at the retract height or above;
/// - a rapid below the retract height is vertical and stops above the deepest point already
///   cut at that position (a drill returning into its hole);
/// - no move goes below the bottom height;
/// - an arc starts and ends at the same distance from its centre.
pub fn verify(path: &Toolpath, h: &Heights) -> Result<()> {
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let mut pos: Option<Vec3> = None;
    // Deepest Z cut at the current XY position since the tool arrived there.
    let mut cut_here: Option<f64> = None;
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
                } else if to.z < h.retract - TOL && !cut_here.is_some_and(|z| to.z > z + 1e-9) {
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
