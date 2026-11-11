//! Multi-axis toolpaths: moves of the tool tip with the direction of the tool axis, in the
//! setup's work coordinates. The axis points from the tip up the tool towards the spindle, so
//! a 3-axis toolpath has the axis `+Z` throughout.
//!
//! These are machine-independent: [`crate::kinematics`] turns them into the positions of a
//! particular machine's linear and rotary axes.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::frame::Frame;
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::{CamError, Result, invalid};

/// How far a tool axis may be from unit length.
const UNIT_TOL: f64 = 1e-6;

/// Where the tool is: its tip and the unit direction from the tip up the tool.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub tip: Vec3,
    pub axis: Vec3,
}

impl Pose {
    pub fn new(tip: Vec3, axis: Vec3) -> Pose {
        Pose { tip, axis }
    }

    /// The pose with a vertical tool, as a 3-axis machine holds it.
    pub fn vertical(tip: Vec3) -> Pose {
        Pose { tip, axis: Vec3::new(0.0, 0.0, 1.0) }
    }

    pub fn is_valid(&self) -> bool {
        self.tip.is_finite() && self.axis.is_finite() && (self.axis.len() - 1.0).abs() <= UNIT_TOL
    }

    /// A point `h` up the tool axis from the tip.
    pub fn up(&self, h: f64) -> Vec3 {
        self.tip + self.axis * h
    }

    /// The angle between this pose's axis and `other`'s (radians).
    pub fn axis_angle(&self, other: &Pose) -> f64 {
        self.axis.cross(other.axis).len().atan2(self.axis.dot(other.axis))
    }

    /// The pose in the outer coordinates of `frame`, this one being given in the frame.
    pub fn to_world(&self, frame: &Frame) -> Pose {
        Pose { tip: frame.to_world(self.tip), axis: frame.dir_to_world(self.axis) }
    }

    pub fn to_local(&self, frame: &Frame) -> Pose {
        Pose { tip: frame.to_local(self.tip), axis: frame.dir_to_local(self.axis) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Move5 {
    /// Positioning at the machine's rapid rate; never through material.
    Rapid { to: Pose },
    /// A cut to `to` at `feed` mm/min of the tip, the tip moving on a straight line and the
    /// axis turning evenly (on the great circle between the two directions).
    Feed { to: Pose, feed: f64 },
    /// Pause with the spindle running.
    Dwell { seconds: f64 },
}

impl Move5 {
    pub fn target(&self) -> Option<Pose> {
        match *self {
            Move5::Rapid { to } | Move5::Feed { to, .. } => Some(to),
            Move5::Dwell { .. } => None,
        }
    }
}

/// The direction `t` of the way from `a` to `b` (unit vectors) along the great circle between
/// them; halfway round when they point opposite ways, which a planned path never asks for.
pub fn slerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    let dot = a.dot(b).clamp(-1.0, 1.0);
    let angle = dot.acos();
    if angle <= 1e-9 {
        return a.lerp(b, t).normalized().unwrap_or(a);
    }
    let s = angle.sin();
    if s <= 1e-9 {
        let p = a.any_perp().normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let (sa, ca) = (angle * t).sin_cos();
        return a * ca + p * sa;
    }
    (a * (((1.0 - t) * angle).sin() / s) + b * ((t * angle).sin() / s)).normalized().unwrap_or(a)
}

/// Lengths and time of a multi-axis toolpath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathStats5 {
    /// Distance the tip travels at feed (mm).
    pub cut_length: f64,
    /// Distance the tip travels at rapid (mm).
    pub rapid_length: f64,
    /// Time at feed, dwells included (minutes).
    pub cut_minutes: f64,
    /// Largest turn of the tool axis in one cutting move (radians).
    pub max_axis_step: f64,
    /// Largest angle between the tool axis and work `+Z` (radians).
    pub max_tilt: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath5 {
    pub moves: Vec<Move5>,
}

impl Toolpath5 {
    /// A 3-axis toolpath given in `frame` (its tool axis the frame's Z), in the frame's outer
    /// coordinates: the tool axis is the frame's Z throughout. Arcs become lines within `tol`.
    pub fn from_frame(path: &Toolpath, frame: &Frame, tol: f64) -> Result<Toolpath5> {
        Toolpath5::from_frame_mapped(path, frame, tol).map(|(p, _)| p)
    }

    /// [`Toolpath5::from_frame`], with the index in `path` of the move each move comes from.
    pub fn from_frame_mapped(path: &Toolpath, frame: &Frame, tol: f64) -> Result<(Toolpath5, Vec<usize>)> {
        frame.validate()?;
        if !(tol > 0.0) || !tol.is_finite() {
            return invalid("the arc tolerance must be above 0");
        }
        if path.moves.len() > MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        let axis = frame.z;
        let mut out = Vec::with_capacity(path.moves.len());
        let mut from = Vec::with_capacity(path.moves.len());
        let mut pos: Option<Vec3> = None;
        for (i, m) in path.moves.iter().enumerate() {
            match *m {
                Move::Rapid { to } => out.push(Move5::Rapid { to: Pose::new(frame.to_world(to), axis) }),
                Move::Feed { to, feed } => out.push(Move5::Feed { to: Pose::new(frame.to_world(to), axis), feed }),
                Move::Arc { feed, .. } => {
                    // Follow the arc as `Toolpath::points` does, from where the tool is.
                    let one = Toolpath { moves: pos.map(|p| Move::Rapid { to: p }).into_iter().chain([*m]).collect() };
                    for (p, cutting) in one.points(tol).into_iter().skip(usize::from(pos.is_some())) {
                        let to = Pose::new(frame.to_world(p), axis);
                        out.push(if cutting { Move5::Feed { to, feed } } else { Move5::Rapid { to } });
                    }
                }
                Move::Dwell { seconds } => out.push(Move5::Dwell { seconds }),
            }
            if let Some(to) = m.target() {
                pos = Some(to);
            }
            from.resize(out.len(), i);
            if out.len() > MAX_MOVES {
                return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
            }
        }
        Ok((Toolpath5 { moves: out }, from))
    }

    /// A 3-axis toolpath in work coordinates, with the tool vertical throughout.
    pub fn from_3axis(path: &Toolpath, tol: f64) -> Result<Toolpath5> {
        Toolpath5::from_frame(path, &Frame::IDENTITY, tol)
    }

    pub fn poses(&self) -> impl Iterator<Item = Pose> + '_ {
        self.moves.iter().filter_map(Move5::target)
    }

    pub fn stats(&self) -> PathStats5 {
        let mut s = PathStats5::default();
        let mut pos: Option<Pose> = None;
        let up = Vec3::new(0.0, 0.0, 1.0);
        for m in &self.moves {
            match *m {
                Move5::Rapid { to } => s.rapid_length += pos.map_or(0.0, |p| p.tip.dist(to.tip)),
                Move5::Feed { to, feed } => {
                    let len = pos.map_or(0.0, |p| p.tip.dist(to.tip));
                    s.cut_length += len;
                    s.cut_minutes += if feed > 0.0 { len / feed } else { 0.0 };
                    s.max_axis_step = s.max_axis_step.max(pos.map_or(0.0, |p| p.axis_angle(&to)));
                }
                Move5::Dwell { seconds } => s.cut_minutes += seconds.max(0.0) / 60.0,
            }
            if let Some(to) = m.target() {
                s.max_tilt = s.max_tilt.max(to.axis_angle(&Pose::new(to.tip, up)));
                pos = Some(to);
            }
        }
        s
    }

    /// The tip's way as points for display and checking: `(pose, cutting)`.
    pub fn points(&self) -> Vec<(Pose, bool)> {
        self.moves
            .iter()
            .filter_map(|m| match *m {
                Move5::Rapid { to } => Some((to, false)),
                Move5::Feed { to, .. } => Some((to, true)),
                Move5::Dwell { .. } => None,
            })
            .collect()
    }
}

/// Checks what every multi-axis toolpath must keep: at most 2,000,000 moves, every pose
/// finite with a unit axis, every feed above zero, a rapid first, and no cutting move turning
/// the tool axis by more than `max_axis_step` (radians) — a path that does is not the smooth
/// motion it was planned as.
pub fn verify5(path: &Toolpath5, max_axis_step: f64) -> Result<()> {
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let fail = |i: usize, why: &str| Err(CamError::Unsafe(format!("move {i}: {why}")));
    let mut pos: Option<Pose> = None;
    for (i, m) in path.moves.iter().enumerate() {
        match *m {
            Move5::Rapid { to } => {
                if !to.is_valid() {
                    return fail(i, "a pose is not finite or its tool axis is not a unit direction");
                }
            }
            Move5::Feed { to, feed } => {
                let Some(from) = pos else { return fail(i, "the path must start with a rapid") };
                if !to.is_valid() {
                    return fail(i, "a pose is not finite or its tool axis is not a unit direction");
                }
                if !(feed > 0.0) || !feed.is_finite() {
                    return fail(i, "the feed must be above 0");
                }
                if from.axis_angle(&to) > max_axis_step + 1e-9 {
                    return fail(i, "the tool axis turns too far in one move");
                }
            }
            Move5::Dwell { seconds } => {
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
