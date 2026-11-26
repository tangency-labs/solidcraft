//! What a body has on and past a round face of it, as data: the answers the kernel reads off the
//! exact body (`solidcore::teeth::round_face`). Plain data: every reading, every tolerance and
//! every number in it is the kernel's.
use serde::{Deserialize, Serialize};

use crate::Plane;

/// How far a body reaches in a seat's frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    /// The least it reaches into the face from the free end: below zero where it runs on past the
    /// free end.
    pub low: f64,
    /// The most it reaches into the face from the free end.
    pub high: f64,
    /// The most it reaches from the axis along the frame's two cross directions (a round body's
    /// radius).
    pub across: f64,
}

/// One end of a round face, as the body has it: what lies past it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // independent readings of one end
pub struct End {
    /// The body ends where the face ends: nothing of it lies past the end.
    pub free: bool,
    /// Nothing stands in the way past the end: no shoulder larger than a shaft, no narrowing of a
    /// bore. A free end is open.
    pub open: bool,
    /// The cuts come out of the body past this end, as a cutter runs out of the work.
    pub out: bool,
    /// How far the cuts must run on past the end to come out.
    pub through: f64,
    /// The face stops at a step: a shaft's neck, a bore's counterbore.
    pub step: bool,
    /// At a shaft's step, the radius of the body just past the end.
    pub neck: f64,
    /// How far past the face's end the body's own end lies, where what lies between belongs to
    /// the face's end (a chamfer, a round); zero otherwise.
    pub end: f64,
    /// The radius at the body's own end: the rim of a shaft's end face, the mouth of a bore.
    pub rim: f64,
    /// What lies between is a plain chamfer: straight from the rim to the face.
    pub cone: bool,
}

/// The block a round face is grown on its own stretch of a body with: a round slab about the
/// seat's axis over the stretch the face runs along.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrowBlock {
    /// The plane the slab lies in: the seat's plane.
    pub plane: Plane,
    /// The slab's radius.
    pub radius: f64,
    /// Where it starts along the plane's normal, from the plane.
    pub from: f64,
    /// Where it ends along the plane's normal, from the plane: above `from`.
    pub to: f64,
}

/// What could not be read, or the number that rules a reading out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundFaceRefusal {
    /// A number given is not finite, or a radius or a stretch is not above zero.
    Number,
    /// The seat gives no frame: its plane's axes are not finite or do not span a plane.
    Seat,
    /// The body's exact box in the seat's frame could not be read.
    Extent,
    /// What a probe over a stretch of the body met could not be read.
    Probe,
    /// No face of the body lies on the seat's cylinder.
    NoBand,
    /// The body's faces and edges could not be read.
    Faces,
}
