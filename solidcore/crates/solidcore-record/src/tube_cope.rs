//! Round-tube copes as data: a tube's straight end ([`Terminal`]), a mate's outside cylinder
//! ([`Mate`]), a box round a swept piece ([`Box3`]), a mate fitted to an end ([`Fit`]), a retained
//! cope ([`Cope`]) and the cope unwrapped as a 1:1 template ([`Template`]). Plain data: the cope is
//! the kernel's, through solidcore-ops' `tube_cope`.

use crate::Vec3;

/// A tube's straight end: the original path endpoint, the unit direction into the stock, the
/// section's unit `x` and `y` there (`y = axis × x`), and the straight length from the endpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Terminal {
    /// The original path endpoint.
    pub origin: Vec3,
    /// Unit direction from the endpoint into the stock.
    pub axis: Vec3,
    /// The section's unit x.
    pub x: Vec3,
    /// The section's unit y, `axis × x`.
    pub y: Vec3,
    /// The straight length from the endpoint (mm).
    pub length: f64,
}

/// A mate's outside cylinder and how far its own copes reach in from its start and end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mate {
    /// The cylinder's start, on its axis.
    pub origin: Vec3,
    /// Unit direction from the start.
    pub axis: Vec3,
    /// Outside radius (mm).
    pub radius: f64,
    /// Length (mm).
    pub length: f64,
    /// How far the mate's copes reach in from its start (mm).
    pub coped_start: f64,
    /// How far the mate's copes reach in from its end (mm).
    pub coped_end: f64,
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    /// The smallest corner.
    pub min: Vec3,
    /// The largest corner.
    pub max: Vec3,
}

/// A mate fitted to a tube's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The cylinder's start, snapped so its axis passes through the endpoint.
    pub origin: Vec3,
    /// How far from the endpoint into the tube the cut can reach (mm).
    pub depth: f64,
}

/// A retained cope: the tube's end and outside radius, its mates (each a unit axis and outside
/// radius, every axis through the endpoint) and how far the cut reaches into the tube.
#[derive(Clone, Copy, Debug)]
pub struct Cope<'a> {
    /// The coped end.
    pub end: &'a Terminal,
    /// The tube's outside radius (mm).
    pub radius: f64,
    /// Each mate's unit axis and outside radius.
    pub mates: &'a [(Vec3, f64)],
    /// How far the cut reaches into the tube from the endpoint (mm).
    pub depth: f64,
}

/// A cope unwrapped onto a tube's outside.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// `[arc length on the outside from the seam, station into the stock]`, both mm, from the
    /// seam once round to the seam again.
    pub points: Vec<[f64; 2]>,
    /// The unit radial direction of the seam.
    pub seam_direction: Vec3,
    /// The tube's outside circumference (mm).
    pub circumference: f64,
    /// The tube's outside diameter (mm).
    pub outer_diameter: f64,
}
