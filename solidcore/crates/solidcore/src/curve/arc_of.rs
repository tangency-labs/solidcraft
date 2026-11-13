//! A record circle over an interval of its angle as an arc's definition ([`ArcDefinition`]):
//! the circle `origin + radius (cos t x + sin t y)` from `t = start` to `t = end` is the arc about
//! `origin` of `radius`, counter-clockwise about `x × y` from `x`, over the same angles. The
//! definition is the record's numbers, the axis their one product: the arc's circle
//! ([`ArcDefinition::placement`]) is the record's, within [`crate::curve::arc::frame_error`].
//!
//! Not upstream: the modelling layer's record curves taken as sweep arcs, kept in the kernel so
//! the layer computes no geometry (docs/layers.md §1). Deviations: none (no upstream
//! counterpart). No recursion.

use std::f64::consts::TAU;

use crate::curve::arc::{frame_error, ArcDefinition, MIN_ARC};
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::geom::certified::{difference, distance, dot, Bounded};
use crate::geom::vector3d::Vec3;

const OP: &str = "curve::arc_of::arc_of_circle";

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The arc of the record circle on `frame` of `radius` from angle `interval[0]` to
/// `interval[1]` (radians from the frame's x axis, counter-clockwise about `x × y`): centre the
/// frame's origin, reference its x axis, axis `x × y`, the same radius and angles.
///
/// # Errors
///
/// `InvalidInput` for numbers not finite, a radius below [`MIN_ARC`], an interval that does not
/// increase by at least [`MIN_ARC`] or turns more than once round, or axes not proved of unit
/// length and square to each other to the arc's own frame error ([`frame_error`]): a frame
/// farther off would make the arc's circle (its placement made orthonormal) another circle
/// than the record's, by that much times the radius.
pub fn arc_of_circle(frame: &Frame, radius: f64, interval: [f64; 2]) -> Result<ArcDefinition> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let [start, end] = interval;
    let finite = [frame.origin, frame.x_axis, frame.y_axis]
        .iter()
        .flatten()
        .chain(&[radius, start, end])
        .all(|x| x.is_finite());
    if !finite {
        return Err(bad("not finite"));
    }
    if radius < MIN_ARC {
        return Err(bad("a radius below the smallest arc's"));
    }
    if end - start < MIN_ARC {
        return Err(bad("an interval that does not increase"));
    }
    if end - start > TAU {
        return Err(bad("an interval past a full turn"));
    }
    let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
    // the record's frame: unit axes square to each other, each proved to the arc's own frame
    // error, so the arc's orthonormal placement is the record's circle to within the error its
    // placement has anyway, at any radius (a relative bound)
    let within = frame_error();
    let unit_long = |v: Vec3| {
        let length = distance(Vec3::ZERO, v);
        let off = difference(length.value, 1.0);
        Bounded {
            value: off.value,
            bound: off.bound + length.bound,
        }
        .at_most(within)
        .is_yes()
    };
    if !unit_long(x) || !unit_long(y) {
        return Err(bad("axes not of unit length"));
    }
    let across = dot(x, y, 0.0, 0.0);
    let across = Bounded {
        value: across.value.abs(),
        ..across
    };
    if !across.at_most(within).is_yes() {
        return Err(bad("axes not square to each other"));
    }
    let arc = ArcDefinition {
        centre: v3(frame.origin),
        axis: x.cross(y),
        reference: x,
        radius,
        start,
        end,
    };
    // the placement's own checks: an axis with a length, the reference square to it
    arc.placement()?;
    Ok(arc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: [f64; 3], y: [f64; 3]) -> Frame {
        Frame {
            origin: [1.0, 2.0, 3.0],
            x_axis: x,
            y_axis: y,
        }
    }

    #[test]
    fn a_circle_is_the_arc_about_x_cross_y_from_x() {
        let f = frame([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let arc = arc_of_circle(&f, 5.0, [0.25, 2.0]).unwrap();
        assert_eq!(arc.centre, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(arc.axis, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(arc.reference, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!((arc.radius, arc.start, arc.end), (5.0, 0.25, 2.0));
        // the arc's points are the record circle's: origin + r (cos t x + sin t y)
        for t in [0.25, 1.0, 2.0] {
            let p = arc.point_at(t).unwrap();
            let want = Vec3::new(1.0, 2.0 + 5.0 * t.cos(), 3.0 + 5.0 * t.sin());
            assert!((p - want).length() < 1e-12, "{p:?} at {t}");
        }
        // a whole turn is an arc
        assert!(arc_of_circle(&f, 5.0, [0.0, TAU]).is_ok());
    }

    #[test]
    fn bad_circles_and_intervals_are_refused() {
        let f = frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let refused = |f: &Frame, r: f64, i: [f64; 2]| {
            matches!(arc_of_circle(f, r, i), Err(SolidError::InvalidInput { .. }))
        };
        assert!(refused(&f, 0.0, [0.0, 1.0]));
        assert!(refused(&f, f64::NAN, [0.0, 1.0]));
        assert!(refused(&f, 1.0, [1.0, 1.0]));
        assert!(refused(&f, 1.0, [1.0, 0.0]));
        assert!(refused(&f, 1.0, [0.0, TAU + 1e-9]));
        assert!(refused(&f, 1.0, [0.0, f64::INFINITY]));
        // parallel axes: no axis
        assert!(refused(
            &frame([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            1.0,
            [0.0, 1.0]
        ));
        // axes not square, or not unit
        let w = std::f64::consts::FRAC_1_SQRT_2;
        assert!(refused(
            &frame([1.0, 0.0, 0.0], [w, w, 0.0]),
            1.0,
            [0.0, 1.0]
        ));
        assert!(refused(
            &frame([2.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            1.0,
            [0.0, 1.0]
        ));
        // nearly square (within 1e-9) is not the record's circle at a large radius: at 1e6 its
        // point at a quarter turn is 5e-4 off the arc's
        assert!(refused(
            &frame([1.0, 0.0, 0.0], [5e-10, 1.0, 0.0]),
            1e6,
            [0.0, 2.0]
        ));
    }
}
