//! The half of a cylinder a point on it faces a travel from, as a sheet's section (not upstream,
//! #20389): a straight line along a travel across a cylinder's axis meets the cylinder, extended,
//! twice, once on each half either side of the axis. A tool run along the travel and cut at the
//! cylinder ends where it crosses the half a face of it lies on. That half, its long edges carried
//! on along the travel as flat wings tangent to it, is a smooth sheet such a line crosses once at
//! most: back the way the travel came where the line leaves the cylinder on that half, on ahead
//! where it goes in.
//!
//! [`facing_trough`] gives the sheet's section in a frame across the axis, and how far along the
//! axis and out along the wings it must reach to pass a box (a tool's): the caller extrudes the
//! section and cuts the tool at its round face.

use crate::geom::vector3d::{Point3, Vec3};
use crate::math::{HALF_PI, PI};

/// How near square to the travel the pick may lie, as the cosine between the travel and the
/// direction from the axis to the pick, both across the axis, before it lies on neither half:
/// the cylinder's edge as seen along the travel.
const ON_EDGE: f64 = 1.0e-9;

/// A trough's section and reach, in the frame `origin`, `x`, `y`: `x` the travel across the axis
/// (unit), `y` square to it across the axis (unit), `x × y` the cylinder's axis (unit). The
/// section is one open chain in that frame: the line from `wing_top` to `top`, the arc about the
/// frame's origin of `radius` from angle `HALF_PI` sweeping `sweep` (`-PI` or `PI`, the facing
/// half) to `bottom`, and the line from `bottom` to `wing_bottom`. Extruded along the axis from
/// `-reach` to `reach`, it passes the box it was made for every way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trough {
    /// The frame's origin: the cylinder's axis point.
    pub origin: Point3,
    /// The travel across the axis (unit).
    pub x: Vec3,
    /// Across the axis, square to `x` (unit): `axis × x`.
    pub y: Vec3,
    /// The cylinder's radius.
    pub radius: f64,
    /// The far end of the upper wing, in the frame.
    pub wing_top: [f64; 2],
    /// Where the upper wing meets the arc, `(0, radius)`.
    pub top: [f64; 2],
    /// The arc's sweep from `HALF_PI`: `-PI` on the half facing `x`, `PI` on the other.
    pub sweep: f64,
    /// Where the arc ends and the lower wing starts, `(0, -radius)`.
    pub bottom: [f64; 2],
    /// The far end of the lower wing, in the frame.
    pub wing_bottom: [f64; 2],
    /// How far the extrusion runs either way along the axis from the frame's origin, and how
    /// long the wings are.
    pub reach: f64,
}

impl Trough {
    /// The angle along the frame's arc where it starts, at `top`.
    pub const START: f64 = HALF_PI;
}

/// The trough of the cylinder through `origin` with axis `axis` (any length) and `radius`, on the
/// half the point `near` (on the cylinder) faces a travel `travel` (any length) from, reaching
/// past the box `box_min`..`box_max`. `None` when there is no one half: the travel runs along the
/// axis, `near` lies on the axis, or `near` lies on the cylinder's edge as seen along the travel
/// (square to it across the axis, within [`ON_EDGE`]); a radius or a value that is not finite or
/// not positive gives `None` too.
#[must_use]
pub fn facing_trough(
    origin: Point3,
    axis: Vec3,
    radius: f64,
    near: Point3,
    travel: Vec3,
    box_min: Point3,
    box_max: Point3,
) -> Option<Trough> {
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let axis = axis.unitize().ok()?;
    let across = |v: Vec3| v - axis * v.dot(axis);
    let x = across(travel).unitize().ok()?;
    let side = across(near - origin).unitize().ok()?;
    let facing = side.dot(x);
    if !facing.is_finite() || facing.abs() < ON_EDGE {
        return None;
    }
    // Far enough from the axis's origin to pass every corner of the box, and the arc.
    let corner = |i: usize| {
        let pick = |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
        Vec3::new(
            pick(1, box_min.x, box_max.x),
            pick(2, box_min.y, box_max.y),
            pick(4, box_min.z, box_max.z),
        )
    };
    let reach = (0..8)
        .map(|i| (corner(i) - origin).length())
        .fold(0.0_f64, f64::max)
        + radius
        + 1.0;
    if !reach.is_finite() {
        return None;
    }
    // The half facing +x: the line leaves the cylinder there, and the wings run back along -x.
    // The other half: it goes in, and they run on along +x.
    let sign = facing.signum();
    let back = -sign * reach;
    Some(Trough {
        origin,
        x,
        y: axis.cross(x),
        radius,
        wing_top: [back, radius],
        top: [0.0, radius],
        sweep: -sign * PI,
        bottom: [0.0, -radius],
        wing_bottom: [back, -radius],
        reach,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: Point3 = Vec3::new(30.0, 20.0, 0.0);
    const Z: Vec3 = Vec3::new(0.0, 0.0, 2.0);
    const LO: Point3 = Vec3::new(-10.0, 22.0, 0.0);
    const HI: Point3 = Vec3::new(60.0, 28.0, 4.0);

    #[test]
    fn a_pick_facing_the_travel_runs_its_wings_back() {
        // The pick at 45° on the +x, +y quarter, the travel +x: the line leaves the cylinder
        // there, so the trough is the +x half, its wings back along -x.
        let near = O + Vec3::new(10.0, 10.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let t = facing_trough(O, Z, 10.0, near, Vec3::new(3.0, 0.0, 0.0), LO, HI).unwrap();
        assert_eq!(t.x, Vec3::X);
        assert_eq!(t.y, Vec3::Y);
        assert_eq!(t.sweep, -PI);
        assert!(t.wing_top[0] < 0.0 && t.wing_bottom[0] < 0.0);
        assert_eq!(t.top, [0.0, 10.0]);
        assert_eq!(t.bottom, [0.0, -10.0]);
        // The farthest box corner from the axis point, (-10, 28, 4), plus the radius and 1.
        let far = (Vec3::new(-10.0, 28.0, 4.0) - O).length();
        assert!((t.reach - (far + 11.0)).abs() < 1e-12);
        assert_eq!(t.wing_top, [-t.reach, 10.0]);
    }

    #[test]
    fn coming_from_the_far_side_the_wings_run_on_ahead() {
        // The same pick, the travel -x: the line goes in there, so the wings run on ahead along
        // the travel, the frame's +x.
        let near = O + Vec3::new(10.0, 10.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let t = facing_trough(O, Z, 10.0, near, Vec3::new(-1.0, 0.0, 0.0), LO, HI).unwrap();
        assert_eq!(t.x, -Vec3::X);
        assert_eq!(t.sweep, PI);
        assert!(t.wing_top[0] > 0.0 && t.wing_bottom[0] > 0.0);
        // The frame is right handed about the axis: x × y is the axis.
        assert!((t.x.cross(t.y) - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-15);
    }

    #[test]
    fn no_one_half_gives_none() {
        let near = O + Vec3::new(0.0, 10.0, 0.0);
        // Along the axis.
        assert!(facing_trough(O, Z, 10.0, near, Z, LO, HI).is_none());
        // On the edge as seen along the travel.
        assert!(facing_trough(O, Z, 10.0, near, Vec3::X, LO, HI).is_none());
        // On the axis.
        assert!(facing_trough(O, Z, 10.0, O, Vec3::X, LO, HI).is_none());
        // No radius, no axis, a value that is not finite.
        let pick = O + Vec3::new(10.0, 0.0, 0.0);
        assert!(facing_trough(O, Z, 0.0, pick, Vec3::X, LO, HI).is_none());
        assert!(facing_trough(O, Vec3::ZERO, 10.0, pick, Vec3::X, LO, HI).is_none());
        assert!(facing_trough(O, Z, f64::NAN, pick, Vec3::X, LO, HI).is_none());
        assert!(facing_trough(
            O,
            Z,
            10.0,
            pick,
            Vec3::X,
            LO,
            Vec3::new(f64::INFINITY, 0.0, 0.0)
        )
        .is_none());
    }
}
