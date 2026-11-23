//! SolidCraft's VR viewer (docs/vr.md): the design as a thing in the room, held, turned and
//! scaled with the controllers.
//!
//! - [`room`]: where the model is in the room ([`room::Transform`]: one turn, one scale, one
//!   move) and where it first appears.
//! - [`grab`]: the world grab. One hand's grip moves the model; both grips move, turn and scale
//!   it. This is the only way to get around: there is no stick movement, so nobody gets sick.
//! - [`flick`]: a thumbstick pushed out and let go, counted once.
//! - [`press`]: a button clicked or held, told apart (a short press when let go, a long one once
//!   held long enough).
//! - [`menu`]: where the ring menu's items stand (a ring or a fan facing the head) and which one
//!   the hand is nearest; the menu itself is the app's.
//! - [`model`]: a controller's model as the VR software hands it out (glTF), ready to draw.
//! - [`marks`]: what is drawn for each controller (its body and its ray), as triangles in the
//!   room.
//! - [`section`]: the section plane held at the controller's tip, in the room and in the
//!   model's units, and the square drawn where it is.
//!
//! The crate is plain maths on the xr crate's poses: the app reads the headset and the
//! controllers, draws the pictures, and feeds the results through here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod flick;
pub mod grab;
pub mod marks;
pub mod menu;
pub mod model;
pub mod press;
pub mod room;
pub mod section;

/// Small vector helpers for the room's metres.
pub(crate) mod vector {
    /// The dot product.
    pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// The cross product, right-handed.
    pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    /// `a` + `b`.
    pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    /// `a` − `b`.
    pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    /// `a` made `s` times as long.
    pub fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
        [a[0] * s, a[1] * s, a[2] * s]
    }

    /// The length of `a`.
    pub fn length(a: [f64; 3]) -> f64 {
        dot(a, a).sqrt()
    }

    /// Whether every component of `a` is a finite number.
    pub fn finite(a: [f64; 3]) -> bool {
        a.iter().all(|x| x.is_finite())
    }

    /// `a` at length 1, or `None` when it has no direction.
    pub fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
        let l = length(a);
        (l.is_finite() && l > 1e-9).then(|| scale(a, 1.0 / l))
    }

    /// The 3 × 3 matrix with these `columns` applied to `v`.
    pub fn turn(columns: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
        add(add(scale(columns[0], v[0]), scale(columns[1], v[1])), scale(columns[2], v[2]))
    }
}
