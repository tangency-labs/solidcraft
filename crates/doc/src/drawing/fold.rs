//! The bases of views made from a parent view off its sides: a pictorial view seen from one of the
//! parent's corners ([`axonometric`], [`pictorial_basis`]), and an auxiliary view folded from it
//! toward any side, square to an edge or a face it shows as a line ([`auxiliary_side`],
//! [`auxiliary_basis`]).
//!
//! This is drawing-only maths, so it lives in the drawing code and not in solidcore or geom (the
//! maintainers' ruling on #16038 and #14114: "solidcore holds what more than one client app needs").
//!
//! Sources: ISO 5456-3:1996, *Technical drawings, projection methods, part 3: axonometric
//! representations* (the isometric view's plane equally inclined to the three axes, the dimetric's
//! 1 : 1/2 : 1); ISO 5456-2:1996, *part 2: orthographic representations* (views turned about a
//! folding line onto the drawing plane); Higham, *Accuracy and Stability of Numerical Algorithms*
//! (2nd ed.), §27.8 (a length scaled by its largest component before squaring, so that it can't
//! overflow).

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::prelude::*;
use solidcraft_geom::section::validate_plane;

use crate::{DocError, Result};

/// The least length a direction may have and still give a unit: a length at or below it is refused.
pub const MIN_DIRECTION: f64 = 1e-9;

/// The view of `frame` from its corner `corner = (k_x, k_y, k_n)`, along its x, its y and its
/// normal `n = x × y`: looking from `k_x x + k_y y + k_n n` toward the origin, paper x horizontal
/// in the frame (square to its y) and paper y up, so the view's normal points back at the corner.
/// `(±1, 1, ±1)` is isometric and `(±1, 1, ±√7)` the standard dimetric (ISO 5456-3). With `k` the
/// corner's unit, paper x is the unit of `k_n x - k_x n`, the view looks along
/// `t = -(k_x x + k_y y + k_n n)`, and paper y is `x × t`; the origin is kept. Each unit is
/// [`scaled_unit3`]'s, the module's one length rule. For a corner of components ±1 and 0 on a frame
/// of unit axes, every step is exact or one correctly rounded operation. The axes are orthonormal
/// to rounding, and as orthonormal as the frame's.
///
/// # Errors
///
/// [`DocError::Invalid`] for a frame [`validate_plane`] refuses, a corner not finite or of length
/// at or below [`MIN_DIRECTION`], or a corner within [`MIN_DIRECTION`] radians of the frame's y,
/// which has no horizontal.
pub fn axonometric(frame: &Plane, corner: [f64; 3]) -> Result<Plane> {
    let refuse = |reason: &str| DocError::Invalid(format!("no view from the corner: {reason}"));
    validate_plane(frame).map_err(|_| refuse("a frame not finite or not orthonormal within 1e-8"))?;
    let k = scaled_unit3(Vec3::from(corner)).ok_or_else(|| refuse("a corner not finite, or of length at or below 1e-9: a longer one fits"))?;
    let (x, y, n) = axes(frame);
    // `k_n x - k_x n` is as long as the sine of the corner's angle from y
    let across =
        scaled_unit3(x * k.z - n * k.x).ok_or_else(|| refuse("a corner within 1e-9 rad of the frame's y has no horizontal: one further fits"))?;
    let toward = -(x * k.x + y * k.y + n * k.z);
    Ok(Plane { origin: frame.origin, x: across, y: across.cross(toward) })
}

/// The basis of the pictorial view on `parent`'s diagonal `(right, up)` (each ±1: which way along
/// the parent's paper x and y it sits), in `projection`: an isometric view through the parent's
/// origin, its paper x horizontal in the parent. As a projected view does
/// (`solidcraft_geom::drawing_view::projected_basis`), it looks from the corner it sits by in third
/// angle and from the opposite one in first: the corner `(right, up, 1)` along the parent's paper
/// x, paper y and normal, each paper sign times 1 in third angle and -1 in first.
///
/// # Errors
///
/// [`DocError::Invalid`] for a parent basis [`validate_plane`] refuses.
pub fn pictorial_basis(parent: &Plane, (right, up): (f64, f64), projection: Projection) -> Result<Plane> {
    let from = facing(projection);
    let view = axonometric(parent, [right * from, up * from, 1.0])?;
    Ok(Plane { origin: view.origin, x: positive_zeros(view.x), y: positive_zeros(view.y) })
}

/// What an auxiliary view is square to ([`auxiliary_side`]): points and directions in the design's
/// frame, read exactly from the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FoldFor {
    /// The straight line from `start` to `end`: the view square to it shows it at its true length.
    Line { start: Vec3, end: Vec3 },
    /// The flat face through `point` with normal `normal`, which the parent shows edge-on: the view
    /// along its normal shows it at its true shape.
    Face { point: Vec3, normal: Vec3 },
}

/// Which side of its fold line an auxiliary view sits on ([`auxiliary_side`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FoldSide {
    /// The side this direction on the parent's paper points to.
    Toward(Vec2),
    /// The side away from this point of the parent's plane, in its coordinates (the middle of what
    /// the parent shows).
    AwayFrom(Vec2),
}

/// Why no auxiliary view looks square to a line or a face ([`auxiliary_side`]), each with the size
/// that fits. The command words each one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FoldRefusal {
    /// A parent basis [`validate_plane`] refuses.
    Frame,
    /// A point, a direction or the side not finite, or a line, a normal or a side of length at or
    /// below [`MIN_DIRECTION`] (a longer one fits).
    Input,
    /// The parent sees the line end-on, within [`MIN_DIRECTION`] radians of its normal: a view that
    /// shows it as a line fits.
    EndOn,
    /// The parent doesn't see the face edge-on: its normal is `angle` radians out of the parent's
    /// plane, where at most [`EDGE_ON`] fits.
    NotEdgeOn { angle: f64 },
    /// The side runs along the fold line, within [`MIN_DIRECTION`] radians: a direction across it fits.
    AlongFoldLine,
}

/// The most a face's normal may lean out of the parent's plane, as a sine, for the parent to see it
/// edge-on: `validate_plane`'s own 1e-8, so a face the parent's rounding tilts still counts.
pub const EDGE_ON: f64 = 1e-8;

/// The basis of the auxiliary view on the side `toward` of `parent` (a direction on its paper), in
/// `projection`: the parent folded toward that side about a fold line square to it (ISO 5456-2),
/// through the parent's origin, so every point keeps its place along the fold line and the two views
/// line up across it. As a projected view does, it looks from the side it sits on in third angle and
/// from the side away in first; toward a paper axis it is that side's projected view, the same
/// doubles once -0 is written as 0.
///
/// With `w` the side's unit (times -1 in first angle) and `n = x × y`, the fold line runs along
/// `f = (-w_y, w_x)`, in the world `F = f_x x + f_y y`; paper x is `f_x F - w_x n` and paper y
/// `f_y F - w_y n`, so the view's normal is `w_x x + w_y y` and `F` shows as `f` in both views.
///
/// # Errors
///
/// [`DocError::Invalid`] for a parent [`validate_plane`] refuses, or a side not finite or of length
/// at or below [`MIN_DIRECTION`].
pub fn auxiliary_basis(parent: &Plane, toward: Vec2, projection: Projection) -> Result<Plane> {
    let refuse = |reason: &str| DocError::Invalid(format!("no auxiliary view: {reason}"));
    validate_plane(parent).map_err(|_| refuse("a parent basis not finite or not orthonormal within 1e-8"))?;
    let w = scaled_unit(toward * facing(projection)).ok_or_else(|| refuse("a side not finite, or of length at or below 1e-9: a longer one fits"))?;
    let (x, y, n) = axes(parent);
    let f = left_of(w);
    let fold_line = x * f.x + y * f.y;
    let across = fold_line * f.x - n * w.x;
    let up = fold_line * f.y - n * w.y;
    Ok(Plane { origin: parent.origin, x: positive_zeros(across), y: positive_zeros(up) })
}

/// The side of `parent` (a unit on its paper) an auxiliary view of `what` sits on to see it square
/// on: across a line's image `(e·x, e·y)`, so the line shows at its true length, or along the image of
/// the normal of a face the parent sees edge-on, so the face shows at its true shape. Of the two
/// senses, the one `side` points to; else the one away from its point, judged by where the parent
/// shows the line's start or the face's point (every point of either shows as far along the side);
/// one exactly through it sits on the side of positive x, or of positive y when the fold line runs
/// along x.
///
/// # Errors
///
/// The [`FoldRefusal`] that applies first.
pub fn auxiliary_side(parent: &Plane, what: FoldFor, side: FoldSide) -> std::result::Result<Vec2, FoldRefusal> {
    validate_plane(parent).map_err(|_| FoldRefusal::Frame)?;
    let (x, y, n) = axes(parent);
    let image = |v: Vec3| Vec2::new(v.dot(x), v.dot(y));
    // The unit square to the fold line, either way, and a point the parent shows on the fold line.
    let (q, at) = match what {
        FoldFor::Line { start, end } => {
            let e = scaled_unit3(end - start).ok_or(FoldRefusal::Input)?;
            (left_of(scaled_unit(image(e)).ok_or(FoldRefusal::EndOn)?), start)
        }
        FoldFor::Face { point, normal } => {
            let m = scaled_unit3(normal).ok_or(FoldRefusal::Input)?;
            let out = m.dot(n).abs();
            if !(out <= EDGE_ON) {
                return Err(FoldRefusal::NotEdgeOn { angle: out.min(1.0).asin() });
            }
            (scaled_unit(image(m)).ok_or(FoldRefusal::Input)?, point)
        }
    };
    if !at.is_finite() {
        return Err(FoldRefusal::Input);
    }
    // How far along `q` the chosen side is: positive keeps `q`, negative turns it round.
    let s = match side {
        FoldSide::Toward(t) => {
            let s = scaled_unit(t).ok_or(FoldRefusal::Input)?.dot(q);
            if !(s.abs() > MIN_DIRECTION) {
                return Err(FoldRefusal::AlongFoldLine);
            }
            s
        }
        FoldSide::AwayFrom(middle) => (image(at - parent.origin) - middle).dot(q),
    };
    if !s.is_finite() {
        return Err(FoldRefusal::Input);
    }
    let keep = s > 0.0 || (s == 0.0 && (q.x > 0.0 || (q.x == 0.0 && q.y > 0.0)));
    let q = if keep { q } else { -q };
    Ok(Vec2::new(positive_zero(q.x), positive_zero(q.y)))
}

/// 1 when a view made from its parent in `projection` looks from the side it sits on (third angle),
/// -1 when from the side away from it (first angle), as
/// `solidcraft_geom::drawing_view::projected_basis` turns a side view.
fn facing(projection: Projection) -> f64 {
    match projection {
        Projection::Third => 1.0,
        Projection::First => -1.0,
    }
}

/// A frame's x and y axes and normal (`x × y`).
fn axes(frame: &Plane) -> (Vec3, Vec3, Vec3) {
    (frame.x, frame.y, frame.x.cross(frame.y))
}

/// `v` with any -0 made 0 (turning by a sign makes a zero component -0), so a view's axes write out
/// as `[0.0, 1.0, 0.0]`, never `[-0.0, 1.0, -0.0]`.
fn positive_zeros(v: Vec3) -> Vec3 {
    Vec3::new(positive_zero(v.x), positive_zero(v.y), positive_zero(v.z))
}

/// `c`, or 0 for -0.
fn positive_zero(c: f64) -> f64 {
    if c == 0.0 { 0.0 } else { c }
}

/// `v`'s length and unit, the length scaled by its largest component before the squares are summed
/// (Higham §27.8), so that no finite vector overflows to a zero "unit" or underflows to a length it
/// doesn't have: `m·sqrt(Σ(v/m)²)`. `None` for a component not finite or a zero vector. `sqrt` and
/// each division are correctly rounded, so the result is the same on every target. The length may
/// round to infinity for a finite vector near `f64::MAX`: still a length.
fn scaled3(v: Vec3) -> Option<(f64, Vec3)> {
    if !v.is_finite() {
        return None;
    }
    let m = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if m == 0.0 {
        return None;
    }
    let (x, y, z) = (v.x / m, v.y / m, v.z / m);
    let s = (x * x + y * y + z * z).sqrt();
    Some((m * s, Vec3::new(x / s, y / s, z / s)))
}

/// `v`'s unit as [`scaled3`] gives it, or `None` for a length at or below [`MIN_DIRECTION`].
fn scaled_unit3(v: Vec3) -> Option<Vec3> {
    scaled3(v).and_then(|(len, unit)| (len > MIN_DIRECTION).then_some(unit))
}

/// `w` turned a quarter turn counter-clockwise: the fold line's direction for a fold toward `w`.
fn left_of(w: Vec2) -> Vec2 {
    Vec2::new(-w.y, w.x)
}

/// `v`'s unit as [`scaled_unit3`] gives it in the plane `z = 0`: the module's one length rule.
fn scaled_unit(v: Vec2) -> Option<Vec2> {
    scaled_unit3(v.extend(0.0)).map(|u| Vec2::new(u.x, u.y))
}

#[cfg(test)]
#[path = "fold_tests.rs"]
mod tests;
