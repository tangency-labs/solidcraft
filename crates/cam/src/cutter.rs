//! The shape of a cutter's end. A milling cutter here is either rounded or tapered:
//!
//! - rounded: a flat bottom of radius `radius - corner` with a quarter-round corner of radius
//!   `corner` around it. A flat end mill has no corner, a ball's corner is its radius, and a
//!   bull nose is in between;
//! - tapered ([`Cone`]): a flat tip of radius `tip_radius`, then a cone opening at its half
//!   angle up to the cutting radius, as a chamfer mill's end.
//!
//! Above its end every cutter is a cylinder of its radius. Heights are measured up the tool
//! axis from the tip, the lowest point of the cutter.

use serde::{Deserialize, Serialize};

use crate::tool::MAX_TOOL_DIAMETER;
use crate::{Result, invalid};

/// Smallest and largest half angle of a tapered cutter (radians): 0.5° to 89.5°.
pub const CONE_HALF_ANGLE_RANGE: (f64, f64) = (0.5 * std::f64::consts::PI / 180.0, 89.5 * std::f64::consts::PI / 180.0);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cutter {
    /// Cutting radius (mm).
    pub radius: f64,
    /// Corner radius (mm), from 0 (flat) to `radius` (ball); 0 for a tapered cutter.
    pub corner: f64,
    /// The taper of a tapered cutter; `None` for a rounded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cone: Option<Cone>,
}

/// The end of a tapered cutter: a flat tip, then a cone up to the cutting radius.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cone {
    /// Radius of the flat tip (mm), from 0 (pointed) to below the cutting radius.
    pub tip_radius: f64,
    /// The angle between the tool axis and the cone's side (radians): half the included angle.
    pub half_angle: f64,
}

impl Cutter {
    pub fn flat(radius: f64) -> Result<Cutter> {
        Cutter { radius, corner: 0.0, cone: None }.checked()
    }

    pub fn ball(radius: f64) -> Result<Cutter> {
        Cutter { radius, corner: radius, cone: None }.checked()
    }

    pub fn bull(radius: f64, corner: f64) -> Result<Cutter> {
        Cutter { radius, corner, cone: None }.checked()
    }

    /// A tapered cutter: a flat tip of `tip_radius`, a cone at `half_angle` (radians) from the
    /// axis up to `radius`.
    pub fn tapered(radius: f64, tip_radius: f64, half_angle: f64) -> Result<Cutter> {
        Cutter { radius, corner: 0.0, cone: Some(Cone { tip_radius, half_angle }) }.checked()
    }

    fn checked(self) -> Result<Cutter> {
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<()> {
        if !(self.radius > 0.0) || !(self.radius <= MAX_TOOL_DIAMETER * 0.5) {
            return invalid("a cutter's radius must be above 0 and at most 500 mm");
        }
        if !(self.corner >= 0.0) || !(self.corner <= self.radius) {
            return invalid("a cutter's corner radius must be from 0 to its radius");
        }
        if let Some(c) = self.cone {
            if self.corner != 0.0 {
                return invalid("a tapered cutter has no corner radius");
            }
            if !(c.tip_radius >= 0.0) || !(c.tip_radius < self.radius) {
                return invalid("a tapered cutter's tip radius must be from 0 to below its radius");
            }
            if !(c.half_angle >= CONE_HALF_ANGLE_RANGE.0) || !(c.half_angle <= CONE_HALF_ANGLE_RANGE.1) {
                return invalid("a tapered cutter's included angle must be from 1 to 179 degrees");
            }
        }
        Ok(())
    }

    /// Radius of the flat part of the bottom.
    pub fn flat_radius(&self) -> f64 {
        match self.cone {
            Some(c) => c.tip_radius,
            None => (self.radius - self.corner).max(0.0),
        }
    }

    pub fn is_ball(&self) -> bool {
        self.cone.is_none() && self.radius - self.corner <= 1e-12
    }

    /// Height of the cutter's surface above the tip at distance `d` from the axis; `None`
    /// beyond the cutting radius. Convex and rising in `d`, which the drop-cutter relies on.
    pub fn height(&self, d: f64) -> Option<f64> {
        if !(d <= self.radius + 1e-12) {
            return None;
        }
        let a = self.flat_radius();
        if d <= a {
            return Some(0.0);
        }
        if let Some(c) = self.cone {
            return Some((d.min(self.radius) - a) / c.half_angle.tan());
        }
        let e = (d - a).min(self.corner);
        Some(self.corner - (self.corner * self.corner - e * e).max(0.0).sqrt())
    }

    /// Height of the end's top edge above the tip, where it meets the cylinder above: the
    /// corner radius of a rounded cutter, the cone's height of a tapered one.
    pub fn rim_height(&self) -> f64 {
        self.height(self.radius).unwrap_or(self.corner)
    }

    /// The cutter grown by `by` all round (its tip `by` lower): the shape that keeps the real
    /// cutter `by` away from a surface, used to leave stock. A tapered cutter grows to the
    /// cone `by` out from its side and the plane `by` below its tip, which holds the rounded
    /// offset of the tip's rim inside it.
    pub fn grown(&self, by: f64) -> Result<Cutter> {
        if !(by >= 0.0) || !by.is_finite() {
            return invalid("the stock to leave must be 0 or more");
        }
        match self.cone {
            Some(c) => {
                let (s, co) = c.half_angle.sin_cos();
                Cutter::tapered(self.radius + by, c.tip_radius + by * (1.0 - s) / co, c.half_angle)
            }
            None => Cutter { radius: self.radius + by, corner: self.corner + by, cone: None }.checked(),
        }
    }

    /// The cutter shrunk by `by` (sideways by at most half its radius) and how far up its axis
    /// the shrunk cutter's tip is from this one's: a shape inside this cutter that a surface
    /// meets only where it is more than `by` inside this cutter (the gouge test). A rounded
    /// cutter shrinks by `side` sideways and is raised by `by`; a tapered one keeps its angle,
    /// its side moved `by` in and its tip raised `by`.
    pub(crate) fn shrunk(&self, by: f64) -> Result<(Cutter, f64)> {
        if !(by >= 0.0) || !by.is_finite() {
            return invalid("the amount to shrink a cutter by must be 0 or more");
        }
        let side = by.min(self.radius * 0.5);
        let Some(c) = self.cone else {
            return Ok((Cutter::bull(self.radius - side, (self.corner - side).max(0.0))?, by));
        };
        let radius = self.radius - side;
        let (s, co) = c.half_angle.sin_cos();
        // The side moved `by` in along its normal is the cone `by / sin` higher; it meets the
        // tip raised by `by` at this radius.
        let tip = c.tip_radius - by * (1.0 - s) / co;
        if tip >= radius {
            // Flat as far out as the shrunk cutter reaches.
            return Ok((Cutter::flat(radius)?, by));
        }
        if tip >= 0.0 {
            return Ok((Cutter::tapered(radius, tip, c.half_angle)?, by));
        }
        // The side moved in comes to a point above the raised tip.
        Ok((Cutter::tapered(radius, 0.0, c.half_angle)?, by / s - c.tip_radius * co / s))
    }
}
