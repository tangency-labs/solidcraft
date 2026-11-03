//! Translating the engine's motion commands into this crate's [`Move`]s.
//!
//! Three stages, in one pass per command:
//! - **Sanitation** — non-finite coordinates and curves the engine must not
//!   produce are refused, zero-length moves are dropped, and the first
//!   command becomes the positioning move from the clearance plane.
//! - **Link conventions** — the engine links its passes with high-speed
//!   skims that rise just above the cut and knows from its own cleared-area
//!   state that they cross cleared material. This crate judges a path from
//!   the path alone, so a travel below the retract height is rewritten the
//!   way a machinist reads it: up to the retract plane, across, down at the
//!   plunge feed. Straight descents take the plunge feed.
//! - **Exact-region clamp** — the clearing planners work on flattened
//!   regions, so every move target is snapped into the exact valid
//!   tool-center region (this crate's own offsetting, the oracle here) and
//!   an arc that would cross a wall is flattened into cuts. The profile
//!   passes take `clamp: None`: the engine walks the exact offset itself,
//!   so their arcs pass through as arcs.

use raygeo::ops::container::Ops;
use raygeo::ops::{MoveCmd, OpCategory};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::offset::Shape;
use crate::params::Cut;
use crate::path::{Move, arc_sweep};
use crate::{CamError, Result};

/// What the translator clamps against: the exact valid tool-center region
/// for the polygon-approximate clearing paths, and nothing for the exact
/// profile passes.
pub(crate) enum Clamp {
    /// Snap move targets into the exact region, `keep_out` from the wall.
    Region { shape: Shape, keep_out: f64 },
    /// The engine already walked the exact offset: no clamping.
    Exact,
}

impl Clamp {
    fn clear(&self, p: Vec2) -> bool {
        match self {
            Clamp::Region { shape, keep_out } => shape.clear(p, *keep_out),
            Clamp::Exact => true,
        }
    }

    fn clamp_to(&self, from: Vec2, to: Vec2) -> Vec2 {
        match self {
            Clamp::Region { shape, keep_out } => {
                if shape.clear(to, *keep_out) || !shape.clear(from, *keep_out) {
                    return to;
                }
                let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
                for _ in 0..24 {
                    let mid = (lo + hi) * 0.5;
                    if shape.clear(from.lerp(to, mid), *keep_out) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                from.lerp(to, lo)
            }
            Clamp::Exact => to,
        }
    }
}

/// Appends the engine's motion commands as moves, keeping the tool's
/// position in `pos` across calls (one engine run per region and depth
/// level feeds the same move list).
pub(crate) fn append(ops: &Ops, cut: &Cut, moves: &mut Vec<Move>, pos: &mut Option<Vec3>, clamp: &Clamp) -> Result<()> {
    let h = &cut.heights;
    let feeds = &cut.feeds;
    for node in ops.commands.iter() {
        let OpCategory::Moving { end, cmd } = &node.category else { continue };
        let mut to = Vec3::new(end.x, end.y, end.z);
        if !to.is_finite() {
            return Err(CamError::Unsafe("the clearing engine produced a move that is not a finite number".into()));
        }
        let from = match *pos {
            Some(from) if from.dist(to) <= 1e-9 => continue,
            Some(from) => from,
            None if moves.is_empty() => {
                // The engine's first command positions at its own working
                // height; the path still begins at the clearance plane.
                let at = Vec3::new(to.x, to.y, h.clearance.max(to.z));
                moves.push(Move::Rapid { to: at });
                *pos = Some(at);
                continue;
            }
            None => continue,
        };
        let from_xy = Vec2::new(from.x, from.y);
        let to_xy = Vec2::new(to.x, to.y);
        match cmd {
            MoveCmd::MoveTo => {
                let clamped = clamp.clamp_to(from_xy, to_xy);
                to = clamped.extend(to.z);
                let same_xy = (from.x - to.x).abs() <= 1e-9 && (from.y - to.y).abs() <= 1e-9;
                if to.z >= h.retract - 1e-9 {
                    // Rising out of a cut: straight up before any sideways
                    // motion, the same rule the verifier holds every op to.
                    if !same_xy && from.z < h.retract - 1e-9 {
                        moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, h.retract) });
                    }
                    moves.push(Move::Rapid { to });
                } else if same_xy {
                    if from.z > h.retract {
                        moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, h.retract) });
                    }
                    moves.push(Move::Feed { to, feed: feeds.plunge });
                } else {
                    moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, h.retract) });
                    moves.push(Move::Rapid { to: Vec3::new(to.x, to.y, h.retract) });
                    moves.push(Move::Feed { to, feed: feeds.plunge });
                }
            }
            MoveCmd::LineTo => {
                let clamped = clamp.clamp_to(from_xy, to_xy);
                to = clamped.extend(to.z);
                let plunges = from.x == to.x && from.y == to.y && to.z < from.z - 1e-9;
                moves.push(Move::Feed { to, feed: if plunges { feeds.plunge } else { feeds.cut } });
            }
            MoveCmd::ArcTo(arc) => {
                // The engine's arc centre is relative to the move's start
                // (G-code I/J), like every other raygeo consumer treats it.
                let center = from_xy + Vec2::new(arc.center.x, arc.center.y);
                let start = from_xy - center;
                let sweep = arc_sweep(from, to, center, !arc.cw);
                // An arc that would cross a wall is flattened into clamped cuts.
                let on_arc = |t: f64| center + Vec2::from_angle(start.angle() + sweep * t) * start.len();
                let crossed = [0.25, 0.5, 0.75].iter().any(|t| !clamp.clear(on_arc(*t))) || !clamp.clear(to_xy);
                if crossed {
                    let steps = ((sweep.abs() / 0.2).ceil() as usize).clamp(2, 64);
                    for k in 1..=steps {
                        let t = k as f64 / steps as f64;
                        let q = clamp.clamp_to(from_xy, on_arc(t));
                        let z = from.z + (to.z - from.z) * t;
                        moves.push(Move::Feed { to: q.extend(z), feed: feeds.cut });
                        *pos = Some(q.extend(z));
                    }
                    continue;
                }
                let end = to_xy - center;
                if start.dist(end) <= 1e-9 || start.len() <= 1e-9 {
                    // A full turn as two halves, so no arc move comes near a
                    // full turn.
                    for half in [0.0, std::f64::consts::PI] {
                        let at = center + Vec2::from_angle(start.angle() + half) * start.len();
                        let z = from.z + (to.z - from.z) * half / std::f64::consts::TAU;
                        moves.push(Move::Arc { to: at.extend(z), center, ccw: !arc.cw, feed: feeds.cut });
                    }
                    continue;
                }
                let sweep = arc_sweep(from, to, center, !arc.cw);
                if sweep.abs() > std::f64::consts::PI {
                    let mid = center + Vec2::from_angle(start.angle() + sweep * 0.5) * start.len();
                    let z_mid = from.z + (to.z - from.z) * 0.5;
                    moves.push(Move::Arc { to: mid.extend(z_mid), center, ccw: !arc.cw, feed: feeds.cut });
                }
                moves.push(Move::Arc { to, center, ccw: !arc.cw, feed: feeds.cut });
            }
            // The clearing engine cuts lines and arcs; anything else would
            // have to be linearized first and is refused rather than guessed.
            MoveCmd::BezierTo(_) | MoveCmd::QuadraticBezierTo { .. } | MoveCmd::ScanLine { .. } => {
                return Err(CamError::Unsupported("the clearing engine produced a curve this operation does not handle".into()));
            }
        }
        *pos = Some(to);
    }
    Ok(())
}
