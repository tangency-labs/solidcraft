//! Frames as data: a frame-stock path's pieces ([`PieceData`]), a straight member laid out for
//! trimming ([`MemberData`]) with its ends ([`End`], [`Side`], [`Axis`], [`Cut`], [`Extent`]), and
//! a gusset's supports and plate ([`SupportFrames`], [`SupportData`], [`Dimensions`],
//! [`Placement`], [`Prepared`]).
//!
//! These are the plain values of the kernel's frame geometry (`solidcore::sweep::member`,
//! `solidcore::prism::member` and `solidcore::prism::gusset`), so an application can keep and pass
//! them without holding the kernel's types. Plain data: making, checking and evaluating them are
//! the kernel's, and `solidcore-ops` turns each back into the kernel's value with its checks
//! (`frame_member::Piece::from_data`, `frame_trim::Member::from_data`,
//! `frame_gusset::Support::from_data`).
use serde::{Deserialize, Serialize};

use crate::{Plane, Region2, Vec2, Vec3};

/// One exact segment of a stock path, as the sweep takes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Segment {
    /// A straight run between its two ends.
    Line {
        /// Start.
        a: Vec3,
        /// End.
        b: Vec3,
    },
    /// An open circular arc.
    Arc {
        /// Start.
        a: Vec3,
        /// Centre.
        center: Vec3,
        /// Unit normal of the positive turn.
        axis: Vec3,
        /// Positive turn, radians.
        angle: f64,
    },
}

/// One validated piece of a stock path: its ends, its unit tangents there and its exact segment.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PieceData {
    /// Start.
    pub a: Vec3,
    /// End.
    pub b: Vec3,
    /// Unit start tangent.
    pub t0: Vec3,
    /// Unit end tangent.
    pub t1: Vec3,
    /// Its exact sweep segment.
    pub segment: Segment,
}

/// A straight member laid out: its section's plane at the line's start (the profile's own
/// origin at the plane's origin, `x × y` along the line), the line's unit direction and length,
/// and the section's box in the plane's coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberData {
    /// The section's plane at the line's start.
    pub frame: Plane,
    /// The line's unit direction.
    pub dir: Vec3,
    /// The line's length.
    pub length: f64,
    /// The section box's low corner, in the plane's coordinates.
    pub lo: Vec2,
    /// Its high corner.
    pub hi: Vec2,
}

/// Which end of a member moves: the one nearer the target, or the start or the end of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// The end nearer the target.
    Nearer,
    /// The line's start.
    Start,
    /// The line's end.
    End,
}

/// A side of a member's section box, in its plane's x and y (`Right` is +x, `Top` +y).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// +x.
    Right,
    /// -x.
    Left,
    /// +y.
    Top,
    /// -y.
    Bottom,
}

/// A member's middle line (through the middle of its section box): its point at the line's
/// start, how far along the line from there each end crosses it, and the length between them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    /// The middle line's point at the line's start.
    pub centre: Vec3,
    /// Where the start crosses it, along the line from `centre`.
    pub t0: f64,
    /// Where the end crosses it.
    pub t1: f64,
    /// The length between them, `t1 - t0`.
    pub length: f64,
}

/// Where an end is cut: a point on its plane and the plane's unit normal, out of the member. The
/// plane's in-plane axes are the application's to choose.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    /// A point on the plane.
    pub origin: Vec3,
    /// The unit normal, out of the member.
    pub normal: Vec3,
}

/// The range along the line a member's section is extruded over, and which ends (start, end)
/// are then cut at a slant in their planes. A square end is the range's own end.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    /// Where the extrusion starts, along the line from the section's plane.
    pub from: f64,
    /// Where it ends.
    pub to: f64,
    /// Whether the start and the end are cut at a slant.
    pub cut: [bool; 2],
}

/// A gusset's evaluated dimensions, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimensions {
    /// Length on the first support.
    pub leg1: f64,
    /// Length on the second support.
    pub leg2: f64,
    /// Plate thickness.
    pub thickness: f64,
    /// Inner corner relief.
    pub relief: f64,
    /// Outer tip clipping.
    pub clip: f64,
    /// Signed offset along the joint.
    pub offset: f64,
}

/// A gusset plate's side of the joint plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// Half the thickness on either side.
    Center,
    /// All thickness on the positive side.
    Positive,
    /// All thickness on the negative side.
    Negative,
}

/// The frames and tolerance of a gusset support's faces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportFrames {
    /// Support plane in the plate's author frame.
    pub plane: Plane,
    /// Common plane in the source body's frame.
    pub source: Plane,
    /// Author coordinates to source coordinates, a row-major 4 × 4 matrix.
    pub to_source: [[f64; 4]; 4],
    /// Source coordinates to author coordinates, a row-major 4 × 4 matrix.
    pub to_author: [[f64; 4]; 4],
    /// Planar boolean tolerance in millimetres.
    pub tolerance: f64,
}

/// A gusset support's material: its frames and its trimmed regions (holes included) in the
/// source plane's coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportData {
    /// Its frames and tolerance.
    pub frames: SupportFrames,
    /// Its material, one region per piece.
    pub regions: Vec<Region2>,
}

/// A gusset plate's exact planar region and extrusion levels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prepared {
    /// Profile frame in author coordinates.
    pub plane: Plane,
    /// Closed plate outline.
    pub profile: Region2,
    /// Lower extrusion level.
    pub start: f64,
    /// Upper extrusion level.
    pub end: f64,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn frame_data_keeps_its_serde_names() {
        let s = Segment::Line {
            a: Vec3::ZERO,
            b: Vec3::X,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.starts_with("{\"line\":"), "{json}");
        assert_eq!(serde_json::from_str::<Segment>(&json).unwrap(), s);
        assert_eq!(serde_json::to_string(&End::Nearer).unwrap(), "\"nearer\"");
        assert_eq!(serde_json::to_string(&Side::Bottom).unwrap(), "\"bottom\"");
        assert_eq!(
            serde_json::to_string(&Placement::Center).unwrap(),
            "\"center\""
        );
    }
}
