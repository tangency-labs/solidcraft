//! Blend sections ([`Sections`]): the frames of a cross-section given by its points, and which
//! open sections of a blend to turn round so that each runs the way the one before it does.
//! Plain numbers in and out.

use crate::Backend;

/// One station of a blend's cross-section: its point and how the blend leaves it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendFrame {
    /// The point.
    pub p: [f64; 3],
    /// The direction the blend leaves in (at the last section, back into the blend).
    pub leave: [f64; 3],
    /// The curvature vector along `leave`.
    pub bend: [f64; 3],
    /// The arc-length derivative of `bend` along `leave`.
    pub bend_change: [f64; 3],
}

/// Why the kernel will not say which sections to turn.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TurnRefusal {
    /// A closed section (its ends within the kernel's zone tolerance), whose winding and seam a
    /// turn does not match; the kernel's words.
    #[error("{0}")]
    Closed(String),
    /// Anything else it refuses (an end not finite), in its words.
    #[error("{0}")]
    Refused(String),
}

/// A section the kernel cannot frame, in its words: stations out of range, a point or the
/// leaving direction not finite, a chain of no length.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FramesRefused(pub String);

/// The blend sections of one kernel ([`crate::OpKind::Sections`]).
pub trait Sections: Backend {
    /// The frames of a cross-section given by its points in order (the polyline through them),
    /// at `stations` stations of even arc length, leaving along `leave` with no bend.
    fn point_frames(&self, points: &[[f64; 3]], leave: [f64; 3], stations: usize) -> Result<Vec<BlendFrame>, FramesRefused>;
    /// For each open section, given its first and last point in the blend's order, whether to
    /// turn it round so that it runs the way the one before it does.
    fn open_section_turns(&self, ends: &[[[f64; 3]; 2]]) -> Result<Vec<bool>, TurnRefusal>;
}
