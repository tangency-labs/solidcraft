//! Translating the planner's motion commands into this crate's [`Move`]s.
//!
//! The planner emits complete moves: feeds ride on its state commands, and
//! its SafeZ travels already rise to the clearance height, cross there and
//! re-enter at the plunge feed. What translation adds is what this crate
//! must answer for on its own terms:
//! - **Sanitation** — non-finite coordinates and curves the planner must
//!   not produce are refused, zero-length moves are dropped, and the first
//!   command becomes the positioning move from the clearance plane.
//! - **Feed tracking** — a move's feed is the planner's most recent feed
//!   state, falling back to the operation's cut feed.
//! - **Exact-region clamp** — the clearing planners work on flattened
//!   regions, so every move target is snapped into the exact valid
//!   tool-center region (this crate's own offsetting, the oracle here) and
//!   an arc that would cross a wall is flattened into cuts. The profile
//!   passes take `Clamp::Exact`: the planner walks the exact offset
//!   itself, so their arcs pass through as arcs.
//! - **Arc conventions** — arc centres arrive relative to the move's start
//!   (G-code I/J) and are made absolute; a full turn is written as two
//!   halves and an arc past half a turn in two pieces, so no arc move
//!   comes near a full circle.

use raygeo::ops::container::Ops;
use raygeo::ops::{MoveCmd, OpCategory, StateCmd};
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
    /// The planner already walked the exact offset: no clamping.
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

/// Appends the planner's motion commands as moves, keeping the tool's
/// position in `pos` across calls (one planner run per region and depth
/// level feeds the same move list), and refusing a list beyond the move
/// limit.
pub(crate) fn append(ops: &Ops, cut: &Cut, moves: &mut Vec<Move>, pos: &mut Option<Vec3>, clamp: &Clamp) -> Result<()> {
    let mut feed = cut.feeds.cut;
    for node in ops.commands.iter() {
        match &node.category {
            OpCategory::State(StateCmd::SetFeedRate(rate)) => {
                if *rate > 0 {
                    feed = f64::from(*rate);
                }
            }
            OpCategory::State(_) | OpCategory::Marker(_) => {
                if std::env::var("CAM_DEBUG_MOVES").is_ok() {
                    eprintln!("node: {:?}", node.category);
                }
            }
            OpCategory::Moving { end, cmd } => {
                let mut to = Vec3::new(end.x, end.y, end.z);
                if !to.is_finite() {
                    return Err(CamError::Unsafe("the clearing planner produced a move that is not a finite number".into()));
                }
                let from = match *pos {
                    Some(from) if from.dist(to) <= 1e-9 => continue,
                    Some(from) => from,
                    None if moves.is_empty() => {
                        // The planner's first command positions at its own
                        // working height; the path still begins at the
                        // clearance plane.
                        let at = Vec3::new(to.x, to.y, cut.heights.clearance.max(to.z));
                        moves.push(Move::Rapid { to: at });
                        *pos = Some(at);
                        continue;
                    }
                    None => continue,
                };
                let from_xy = Vec2::new(from.x, from.y);
                let to_xy = Vec2::new(to.x, to.y);
                match cmd {
                    // The planner links its passes on its own: a routed travel
                    // arrives as part of a rise-across-enter shape at the
                    // clearance height and goes out as the rapid it is. One
                    // upstream gap remains: a step's first positioning move
                    // travels sideways at the cut height, where the router
                    // never sees it. This crate judges a path from the path
                    // alone, so a travel below the retract height is still
                    // rewritten the way a machinist reads it: up to the
                    // retract plane, across, down at the plunge feed.
                    MoveCmd::MoveTo => {
                        let clamped = clamp.clamp_to(from_xy, to_xy);
                        to = clamped.extend(to.z);
                        let same_xy = (from.x - to.x).abs() <= 1e-9 && (from.y - to.y).abs() <= 1e-9;
                        if to.z >= cut.heights.retract - 1e-9 {
                            // Rising out of a cut: straight up before any sideways
                            // motion, the same rule the verifier holds every op to.
                            if !same_xy && from.z < cut.heights.retract - 1e-9 {
                                moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, cut.heights.retract) });
                            }
                            moves.push(Move::Rapid { to });
                        } else if same_xy {
                            if from.z > cut.heights.retract {
                                moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, cut.heights.retract) });
                            }
                            moves.push(Move::Feed { to, feed: cut.feeds.plunge });
                        } else {
                            moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, cut.heights.retract) });
                            moves.push(Move::Rapid { to: Vec3::new(to.x, to.y, cut.heights.retract) });
                            moves.push(Move::Feed { to, feed: cut.feeds.plunge });
                        }
                    }
                    MoveCmd::LineTo => {
                        let clamped = clamp.clamp_to(from_xy, to_xy);
                        to = clamped.extend(to.z);
                        moves.push(Move::Feed { to, feed });
                    }
                    MoveCmd::ArcTo(arc) => {
                        // The planner's arc centre is relative to the move's
                        // start (G-code I/J), like every other raygeo
                        // consumer treats it.
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
                                moves.push(Move::Feed { to: q.extend(z), feed });
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
                                moves.push(Move::Arc { to: at.extend(z), center, ccw: !arc.cw, feed });
                            }
                            continue;
                        }
                        let sweep = arc_sweep(from, to, center, !arc.cw);
                        if sweep.abs() > std::f64::consts::PI {
                            let mid = center + Vec2::from_angle(start.angle() + sweep * 0.5) * start.len();
                            let z_mid = from.z + (to.z - from.z) * 0.5;
                            moves.push(Move::Arc { to: mid.extend(z_mid), center, ccw: !arc.cw, feed });
                        }
                        moves.push(Move::Arc { to, center, ccw: !arc.cw, feed });
                    }
                    // The clearing planner cuts lines and arcs; anything else would
                    // have to be linearized first and is refused rather than guessed.
                    MoveCmd::BezierTo(_) | MoveCmd::QuadraticBezierTo { .. } | MoveCmd::ScanLine { .. } => {
                        return Err(CamError::Unsupported("the clearing planner produced a curve this operation does not handle".into()));
                    }
                }
                *pos = Some(to);
            }
        }
    }
    if moves.len() > crate::path::MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {} moves", crate::path::MAX_MOVES)));
    }
    Ok(())
}
