//! 2D drawing exchange for sketches: DXF (ASCII, read and write) and SVG (read). Readers produce
//! plain 2D geometry in millimetres; the engine turns it into sketch curves.

use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

/// One imported 2D entity (millimetres, y up).
#[derive(Clone, Debug, PartialEq)]
pub enum Geom2 {
    Point(Vec2),
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Counter-clockwise arc around `c` from `a` to `b`.
    Arc {
        c: Vec2,
        a: Vec2,
        b: Vec2,
    },
    /// Full ellipse: centre, end of the major axis, minor radius.
    Ellipse {
        c: Vec2,
        major: Vec2,
        minor: f64,
    },
    /// Spline through (`control == false`) or over (`control`) the points.
    Spline {
        pts: Vec<Vec2>,
        control: bool,
        degree: u8,
    },
    /// Spline over the control points with its own clamped knots (`pts.len() + degree + 1`)
    /// and weights (empty: all 1), as a DXF SPLINE gives it (#9355).
    Nurbs {
        pts: Vec<Vec2>,
        degree: u8,
        knots: Vec<f64>,
        weights: Vec<f64>,
    },
    /// Quadratic Bézier / conic.
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        rho: f64,
    },
    Text {
        text: String,
        at: Vec2,
        height: f64,
        angle: f64,
    },
}

/// The most drawn entities a file may have: lines, arcs, circles, splines and texts. A dense
/// exported drawing (an auto-traced logo) can hold hundreds of thousands of short lines, which
/// the insert's `simplify` follows with a few thousand curves, so this is well above the largest
/// sketch: reading one such file peaks at about 80 MB of geometry, on top of the file's own
/// [`MAX_DRAWING_BYTES`].
pub const MAX_ENTITIES: usize = 1_000_000;
/// Largest drawing file we read.
pub const MAX_DRAWING_BYTES: usize = 64 << 20;

/// Counter-clockwise arc from `p0` to `p1` with DXF bulge `b` (tan of a quarter of the
/// included angle, negative = clockwise).
pub(crate) fn bulge_arc(p0: Vec2, p1: Vec2, b: f64) -> Option<Geom2> {
    if b.abs() < 1e-12 || !b.is_finite() {
        return Some(Geom2::Line(p0, p1));
    }
    let chord = p1 - p0;
    let l = chord.len();
    if l < 1e-12 {
        return None;
    }
    let theta = 4.0 * b.atan();
    let r = l / (2.0 * (theta / 2.0).sin()).abs();
    // Centre on the perpendicular bisector, to the left for counter-clockwise bulges.
    let mid = (p0 + p1) * 0.5;
    let h = r * (theta / 2.0).cos().abs();
    let left = chord.perp() / l;
    let sign = if theta.abs() > std::f64::consts::PI { -1.0 } else { 1.0 };
    let c = if b > 0.0 { mid + left * (h * sign) } else { mid - left * (h * sign) };
    Some(if b > 0.0 { Geom2::Arc { c, a: p0, b: p1 } } else { Geom2::Arc { c, a: p1, b: p0 } })
}
