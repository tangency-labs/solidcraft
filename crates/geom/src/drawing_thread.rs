//! A thread's lines in a drawing view, as ISO 6410 draws them (W46, #13050): where they lie on the
//! view's plane. Nothing here finds a body's threads or decides how a line is drawn; the view does.
//!
//! - The **crest** is the thread's outside for a screw (the major diameter) and its inside for a
//!   tapped hole (the minor); it is drawn as the part's edges are. The **root** is the other one,
//!   drawn thin: seen from the side, two lines along the thread; seen end on, three quarters of a
//!   circle, its gap in the upper right quadrant. So for an M8 tapped hole the crest is the Ø6.8
//!   drilled hole and the root a thin arc of Ø8; for an M8 screw the crest is Ø8 and the root a thin
//!   arc at the minor diameter.
//! - A **limit line** goes across the thread where its full form ends, as long as the major diameter.
//! - A thread seen at a slant draws no thread lines.
//!
//! Lines are in the view's plane coordinates ([`Plane::to_local`]), as a view's other lines, for
//! [`crate::drawing_view::ViewFrame::seg_to_paper`]. A view's paper map turns nothing, so an angle in
//! the plane is the same angle on paper.

use crate::prelude::*;

use crate::UnitVec3;
use crate::drawing_view::PARALLEL;
use crate::section::{MAX_NUMBER, validate_plane};
use solidcore_record::{Plane, Seg2, Vec3};

/// A thread as a view needs it: its axis, the threaded span along it, and its two radii.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thread {
    pub axis_point: Vec3,
    pub axis: UnitVec3,
    /// The threaded span, along the axis from `axis_point`.
    pub start: f64,
    pub end: f64,
    /// The crest's and the root's radii: an internal thread's root is outside its crest, an
    /// external one's inside.
    pub crest_radius: f64,
    pub root_radius: f64,
    pub internal: bool,
    /// Whether a limit line goes across at `start` and at `end`.
    pub limits: [bool; 2],
}

/// A thread's lines on a view's plane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadLines {
    pub crest: Vec<Seg2>,
    pub root: Vec<Seg2>,
    pub limits: Vec<Seg2>,
}

impl Thread {
    /// Every number finite and within 1e12 mm, a span that runs forward, radii above zero, and the
    /// root on its kind's side of the crest.
    fn is_drawable(&self) -> bool {
        let within = |v: f64| v.is_finite() && v.abs() <= MAX_NUMBER;
        let p = self.axis_point;
        let numbers = [p.x, p.y, p.z, self.start, self.end, self.crest_radius, self.root_radius].into_iter().all(within);
        let root_side = if self.internal { self.root_radius > self.crest_radius } else { self.root_radius < self.crest_radius };
        numbers && self.start < self.end && self.crest_radius > 0.0 && self.root_radius > 0.0 && root_side
    }
}

/// `thread`'s lines in the view through `basis`: end on (its axis within [`PARALLEL`] of the view's
/// normal, by the sine of their angle), the crest's circle and the root's three-quarter arc; side
/// on (within [`PARALLEL`] of square to it, by the cosine), the crest's and the root's lines along
/// the span and the limit lines asked for. Seen at a slant, a thread that can't be drawn (any number
/// not finite, a span that doesn't run forward, a radius not above zero, the root on the wrong side
/// of the crest) or a basis that isn't one: none.
pub fn thread_lines(basis: &Plane, thread: &Thread) -> Option<ThreadLines> {
    validate_plane(basis).ok()?;
    if !thread.is_drawable() {
        return None;
    }
    let axis = thread.axis.vec().normalized()?;
    let normal = basis.x.cross(basis.y);
    let on_plane = |along: f64| basis.to_local(thread.axis_point + axis * along);
    if axis.cross(normal).len() <= PARALLEL {
        let centre = on_plane(thread.start);
        let circle = |radius: f64, start: f64, sweep: f64| Seg2::Arc { center: centre, radius, start, sweep };
        let quarter = std::f64::consts::FRAC_PI_2;
        return Some(ThreadLines {
            crest: vec![circle(thread.crest_radius, 0.0, 4.0 * quarter)],
            root: vec![circle(thread.root_radius, quarter, 3.0 * quarter)],
            limits: Vec::new(),
        });
    }
    if axis.dot(normal).abs() > PARALLEL {
        return None;
    }
    let (from, to) = (on_plane(thread.start), on_plane(thread.end));
    let across = (to - from).normalized()?.perp();
    let pair = |radius: f64| [-1.0, 1.0].map(|side| Seg2::Line { a: from + across * (side * radius), b: to + across * (side * radius) }).to_vec();
    let major = thread.crest_radius.max(thread.root_radius);
    let limits = [(thread.limits[0], from), (thread.limits[1], to)];
    Some(ThreadLines {
        crest: pair(thread.crest_radius),
        root: pair(thread.root_radius),
        limits: limits.into_iter().filter(|(asked, _)| *asked).map(|(_, at)| Seg2::Line { a: at - across * major, b: at + across * major }).collect(),
    })
}

#[cfg(test)]
#[path = "drawing_thread_tests.rs"]
mod tests;
