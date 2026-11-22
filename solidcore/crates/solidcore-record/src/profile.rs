//! Planar profiles as data: closed loops of lines, circular arcs, Béziers and conics, and
//! regions (an outer loop minus holes). Sketches produce them; the kernel evaluates them and
//! turns them into faces.
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::vec::Vec2;

/// One boundary segment in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Seg2 {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Arc from `start` angle sweeping `sweep` radians (positive = counter-clockwise).
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// Cubic Bézier.
    Cubic {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    /// Rational quadratic Bézier from `a` to `b` with control `apex` of weight `w` (end
    /// weights 1): exact conic arcs (ellipses, parabolas, hyperbolas).
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        w: f64,
    },
    /// Bézier of degree `n` (4…[`MAX_BEZIER_DEGREE`]) over `p[0..=n]` (the rest unused): pieces of
    /// higher-degree splines.
    Bezier {
        n: u8,
        p: [Vec2; MAX_BEZIER_CTRL],
    },
}

/// Highest degree of [`Seg2::Bezier`]: a degree-9 piece is the single Bézier a G4 blend curve
/// (`sketch.blend_curve` with `continuity: "g4"`) is made of.
pub const MAX_BEZIER_DEGREE: usize = 9;

/// Control points a [`Seg2::Bezier`] carries: its highest degree plus one (the unused tail is
/// zero).
pub const MAX_BEZIER_CTRL: usize = MAX_BEZIER_DEGREE + 1;

/// A closed loop of segments, each starting where the previous one ends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loop2 {
    pub segs: Vec<Seg2>,
}

impl Loop2 {
    /// Full circle as one arc from angle 0 (counter-clockwise): one closed curve, so a solid
    /// swept from it has one wall, and an edge round it is one edge.
    pub fn circle(center: Vec2, radius: f64) -> Loop2 {
        Loop2::circle_from(center, radius, 0.0)
    }
    /// Full circle as one arc starting (and ending) at angle `start`, counter-clockwise.
    pub fn circle_from(center: Vec2, radius: f64, start: f64) -> Loop2 {
        Loop2 {
            segs: vec![Seg2::Arc {
                center,
                radius,
                start,
                sweep: TAU,
            }],
        }
    }
    pub fn polygon(pts: &[Vec2]) -> Loop2 {
        let n = pts.len();
        Loop2 {
            segs: (0..n)
                .filter_map(|i| {
                    Some(Seg2::Line {
                        a: *pts.get(i)?,
                        b: *pts.get((i + 1) % n)?,
                    })
                })
                .collect(),
        }
    }
}

/// A planar region: outer loop (counter-clockwise) minus holes (clockwise).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Region2 {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

impl Region2 {
    /// The outer loop, then the holes.
    pub fn loops(&self) -> impl Iterator<Item = &Loop2> + Clone {
        std::iter::once(&self.outer).chain(&self.holes)
    }
}
