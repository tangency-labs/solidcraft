//! The half of a cylinder a point on it faces a travel from, as a sheet's section: the kernel's
//! [`solidcore::geom::facing_trough::facing_trough`] on records' numbers (#20389). Ops converts.

use solidcore::geom::facing_trough::{self as k, Trough as KTrough};
use solidcore::geom::vector3d::Vec3;

fn vec([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A trough's section and reach ([`k::Trough`]), in the frame `origin`, `x`, `y` whose normal
/// `x × y` is the cylinder's axis: the line from `wing_top` to `top`, the arc about the frame's
/// origin of `radius` from angle `start` sweeping `sweep` to `bottom`, and the line from `bottom`
/// to `wing_bottom`, extruded along the axis from `-reach` to `reach`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trough {
    /// The frame's origin, on the cylinder's axis.
    pub origin: [f64; 3],
    /// The travel across the axis (unit).
    pub x: [f64; 3],
    /// Across the axis, square to `x` (unit).
    pub y: [f64; 3],
    /// The cylinder's radius.
    pub radius: f64,
    /// The far end of the upper wing, in the frame.
    pub wing_top: [f64; 2],
    /// Where the upper wing meets the arc.
    pub top: [f64; 2],
    /// The arc's start angle in the frame.
    pub start: f64,
    /// The arc's sweep from `start`.
    pub sweep: f64,
    /// Where the arc ends and the lower wing starts.
    pub bottom: [f64; 2],
    /// The far end of the lower wing, in the frame.
    pub wing_bottom: [f64; 2],
    /// How far the extrusion runs either way along the axis, and how long the wings are.
    pub reach: f64,
}

fn trough(t: KTrough) -> Trough {
    Trough {
        origin: arr(t.origin),
        x: arr(t.x),
        y: arr(t.y),
        radius: t.radius,
        wing_top: t.wing_top,
        top: t.top,
        start: KTrough::START,
        sweep: t.sweep,
        bottom: t.bottom,
        wing_bottom: t.wing_bottom,
        reach: t.reach,
    }
}

/// The trough of the cylinder through `origin` with axis `axis` and `radius` on the half the
/// point `near` faces the travel `travel` from, reaching past the box `box_min`..`box_max` (the
/// kernel's [`k::facing_trough`]). `None` when no one half is faced.
#[must_use]
pub fn facing_trough(
    origin: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    near: [f64; 3],
    travel: [f64; 3],
    box_min: [f64; 3],
    box_max: [f64; 3],
) -> Option<Trough> {
    k::facing_trough(
        vec(origin),
        vec(axis),
        radius,
        vec(near),
        vec(travel),
        vec(box_min),
        vec(box_max),
    )
    .map(trough)
}
