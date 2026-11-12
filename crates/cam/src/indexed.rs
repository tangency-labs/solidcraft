//! Indexed (3+1 and 3+2) machining: a 3-axis operation run in a tilted work plane, the rotary
//! axes positioned once before it.
//!
//! Two ways in. [`IndexedOperation`] holds the rotary angles and a 3-axis [`Toolpath`] given in
//! the frame those angles bring under the tool ([`frame_for_index`]): in that frame the
//! toolpath's coordinates are the machine's own linear positions, so it is written with the
//! 3-axis rules, arcs kept (see [`crate::gcode5`]). [`index`] instead takes a 3-axis toolpath
//! computed in any tilted frame (its Z the tool axis) and gives the multi-axis toolpath and the
//! machine angles, checked reachable; [`frame_for_axis`] makes such a frame whose X and Y are
//! what the machine's X and Y become after the rotaries turn, so a 2D operation's X/Y line up
//! with the machine's.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::frame::{Frame, unit};
use crate::kinematics::{Joints, Machine, check_limits, head_dir, head_point, inverse, orient, table_dir_inv, table_point_inv, tool_axis};
use crate::multiaxis::{Move5, Toolpath5};
use crate::path::{MAX_MOVES, Toolpath};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Chord error (mm) of arcs [`index`] turns into lines.
pub const ARC_TOLERANCE: f64 = 0.001;
/// Largest angle accepted for an index (radians).
const MAX_ANGLE: f64 = 1.0e4;

/// One operation of a program run with the rotaries held still at `rotary` (radians, joint
/// order, as written): a 3-axis toolpath in the frame of [`frame_for_index`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexedOperation {
    pub name: String,
    pub tool: Tool,
    /// Spindle speed (rev/min), clockwise.
    pub spindle_rpm: f64,
    pub coolant: bool,
    pub rotary: Vec<f64>,
    pub path: Toolpath,
}

impl IndexedOperation {
    /// The operation's moves in work coordinates with the tool axis, arcs followed within `tol`:
    /// the tips from the frame of [`frame_for_index`], the axis where the rotaries point the
    /// tool (the frame's Z only while the head is upright).
    pub fn poses(&self, machine: &Machine, tol: f64) -> Result<Toolpath5> {
        self.poses_mapped(machine, tol).map(|(p, _)| p)
    }

    /// [`IndexedOperation::poses`], with the index in [`IndexedOperation::path`] of the move
    /// each pose's move comes from.
    pub fn poses_mapped(&self, machine: &Machine, tol: f64) -> Result<(Toolpath5, Vec<usize>)> {
        let (mut path, from) = Toolpath5::from_frame_mapped(&self.path, &frame_for_index(machine, &self.rotary)?, tol)?;
        let axis = tool_axis(machine, &self.rotary);
        for m in &mut path.moves {
            match m {
                Move5::Rapid { to } | Move5::Feed { to, .. } => to.axis = axis,
                Move5::Dwell { .. } => {}
            }
        }
        Ok((path, from))
    }
}

fn check_angles(machine: &Machine, rotary: &[f64]) -> Result<()> {
    if rotary.len() != machine.rotaries.len() {
        return invalid(format!("{} rotary angles are given; the machine has {}", rotary.len(), machine.rotaries.len()));
    }
    if rotary.iter().any(|a| !(a.abs() <= MAX_ANGLE)) {
        return invalid("the rotary angles must be finite");
    }
    Ok(())
}

/// The frame (in work coordinates) in which the machine's linear position is the tool tip while
/// the rotaries are at `rotary`: a point `p` of it is where the tip is against the work when
/// the machine is at `X = p`. Its Z is the tool axis when the head is not tilted.
pub fn frame_for_index(machine: &Machine, rotary: &[f64]) -> Result<Frame> {
    machine.validate()?;
    check_angles(machine, rotary)?;
    let back = |d: Vec3| table_dir_inv(machine, rotary, d);
    let origin = table_point_inv(machine, rotary, head_point(machine, rotary, Vec3::ZERO)) - machine.work_origin;
    let frame = Frame { origin, x: back(Vec3::X), y: back(Vec3::Y), z: back(Vec3::Z) };
    frame.validate()?;
    Ok(frame)
}

/// Whether the head is upright at `rotary`, so the frame of [`frame_for_index`] has the tool
/// along its Z and arcs in its XY plane are square to the tool.
pub fn head_upright(machine: &Machine, rotary: &[f64]) -> bool {
    head_dir(machine, rotary, Vec3::Z).dist(Vec3::Z) <= 1e-9
}

/// The rotary angles (radians, joint order) that point the tool along `axis` (work
/// coordinates): the solution within the limits nearest zero.
pub fn index_for_axis(machine: &Machine, axis: Vec3) -> Result<Vec<f64>> {
    orient(machine, axis, None)
}

/// The tilted work plane at `origin` (work coordinates) for the tool direction `axis`: its Z is
/// `axis`, and its X and Y are the directions the machine's X and Y (turned with the head, on a
/// machine with a tilting head) take against the work once the rotaries are at
/// [`index_for_axis`].
pub fn frame_for_axis(machine: &Machine, axis: Vec3, origin: Vec3) -> Result<Frame> {
    if !origin.is_finite() {
        return invalid("the work plane's origin must be finite");
    }
    let a = unit(axis, "the tool axis")?;
    let rotary = index_for_axis(machine, a)?;
    let to_work = |d: Vec3| table_dir_inv(machine, &rotary, head_dir(machine, &rotary, d));
    let frame = Frame { origin, x: to_work(Vec3::X), y: to_work(Vec3::Y), z: to_work(Vec3::Z) };
    frame.validate()?;
    if frame.z.dist(a) > 1e-5 {
        return Err(CamError::Unsafe("the machine's rotaries do not reach the tool axis asked for".into()));
    }
    Ok(frame)
}

/// A 3-axis toolpath and the machine angles that run it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Indexed {
    /// The toolpath in work coordinates, the tool axis the frame's Z throughout.
    pub path: Toolpath5,
    /// The rotary angles (radians, joint order) held while it runs.
    pub rotary: Vec<f64>,
}

/// `path3`, computed in the tilted `frame` (its Z the tool axis, in work coordinates), as a
/// multi-axis toolpath with the machine angles that point the tool along `frame.z`: the
/// solution within the limits nearest `previous` (or zero). Every pose is checked within the
/// machine's travel; arcs become lines within [`ARC_TOLERANCE`].
pub fn index(path3: &Toolpath, frame: &Frame, machine: &Machine, previous: Option<&Joints>) -> Result<Indexed> {
    machine.validate()?;
    frame.validate()?;
    if path3.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let path = Toolpath5::from_frame(path3, frame, ARC_TOLERANCE)?;
    let rotary = orient(machine, frame.z, previous)?;
    if tool_axis(machine, &rotary).dist(frame.z) > 1e-5 {
        return Err(CamError::Unsafe("the machine's rotaries do not reach the frame's tool axis".into()));
    }
    let held = Joints { linear: Vec3::ZERO, rotary: rotary.clone() };
    for (i, pose) in path.poses().enumerate() {
        let j = inverse(machine, &pose, Some(&held))?;
        if j.rotary_change(&held) > 1e-9 {
            return Err(CamError::Unsafe(format!("pose {i}: the rotaries would have to move during an indexed operation")));
        }
        check_limits(machine, &j).map_err(|e| match e {
            CamError::Unsafe(s) => CamError::Unsafe(format!("pose {i}: {s}")),
            other => other,
        })?;
    }
    Ok(Indexed { path, rotary })
}
