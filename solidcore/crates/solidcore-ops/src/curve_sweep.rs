//! A curve in space swept along one direction, as the sheet a face is split along: the kernel's
//! [`solidcore::geom::curve_sweep`] on records' numbers. Ops converts; the kernel computes.

use solidcore::geom::curve_sweep as k;
use solidcore::geom::vector3d::Vec3;

pub use k::{SweepRefusal, ALONG_FACE};

fn vec([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A sample of a curve and the points of the faces nearest it, each with the face's normal there
/// ([`k::Sample`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    /// The point of the curve.
    pub at: [f64; 3],
    /// Each face's nearest point and its normal there.
    pub feet: Vec<([f64; 3], [f64; 3])>,
}

/// The unit direction to sweep a curve along: `given`, or the mean face normal at its samples
/// ([`k::sweep_direction`]).
///
/// # Errors
///
/// [`SweepRefusal`]: no foot on a face, no direction, or a direction along the face everywhere.
pub fn sweep_direction(
    samples: &[Sample],
    given: Option<[f64; 3]>,
) -> Result<[f64; 3], SweepRefusal> {
    let samples: Vec<k::Sample> = samples
        .iter()
        .map(|s| k::Sample {
            at: vec(s.at),
            feet: s.feet.iter().map(|(p, n)| (vec(*p), vec(*n))).collect(),
        })
        .collect();
    k::sweep_direction(&samples, given.map(vec)).map(arr)
}

/// A line swept along a direction: the plane strip through `origin` spanned by the unit `u`
/// (the sweep) and `v` (the line), `length` along `v` ([`k::Strip`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// The line's start.
    pub origin: [f64; 3],
    /// The unit sweep direction.
    pub u: [f64; 3],
    /// The line's unit direction.
    pub v: [f64; 3],
    /// The line's length.
    pub length: f64,
}

/// The line from `a` to `b` swept along the unit `dir` ([`k::line_strip`]); `None` for no length
/// or a line along `dir`.
#[must_use]
pub fn line_strip(a: [f64; 3], b: [f64; 3], dir: [f64; 3]) -> Option<Strip> {
    k::line_strip(vec(a), vec(b), vec(dir)).map(|s| Strip {
        origin: arr(s.origin),
        u: arr(s.u),
        v: arr(s.v),
        length: s.length,
    })
}

/// An arc about an axis along the sweep, swept: a cylinder strip ([`k::ArcStrip`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcStrip {
    /// The arc's centre.
    pub origin: [f64; 3],
    /// Unit, from the centre to the arc's start.
    pub x: [f64; 3],
    /// Unit, the axis crossed with `x`.
    pub y: [f64; 3],
    /// The arc's radius.
    pub radius: f64,
    /// The arc's angles from `x` toward `y`, low then high.
    pub angles: [f64; 2],
    /// The curve's left, as the sweep sees it, is inside the cylinder.
    pub left_inside: bool,
}

/// The arc from `start` about `center` turning `angle` about `axis`, swept along the unit `dir`,
/// when its axis runs along `dir` ([`k::arc_strip`]).
#[must_use]
pub fn arc_strip(
    start: [f64; 3],
    center: [f64; 3],
    axis: [f64; 3],
    angle: f64,
    dir: [f64; 3],
) -> Option<ArcStrip> {
    k::arc_strip(vec(start), vec(center), vec(axis), angle, vec(dir)).map(|a| ArcStrip {
        origin: arr(a.origin),
        x: arr(a.x),
        y: arr(a.y),
        radius: a.radius,
        angles: a.angles,
        left_inside: a.left_inside,
    })
}

/// The arc from `start` about `center` turning `angle` about `axis` as homogeneous rational
/// quadratic pieces ([`k::arc_beziers`]).
#[must_use]
pub fn arc_beziers(
    start: [f64; 3],
    center: [f64; 3],
    axis: [f64; 3],
    angle: f64,
) -> Option<Vec<[[f64; 4]; 3]>> {
    k::arc_beziers(vec(start), vec(center), vec(axis), angle)
}

/// A homogeneous rational Bézier translated along the unit `dir` across a box: its two control
/// rows ([`k::extruded_rows`]).
#[must_use]
pub fn extruded_rows(
    ctrl: &[[f64; 4]],
    dir: [f64; 3],
    box_min: [f64; 3],
    box_max: [f64; 3],
) -> Option<[Vec<[f64; 4]>; 2]> {
    k::extruded_rows(ctrl, vec(dir), vec(box_min), vec(box_max))
}

/// How far an open curve's end at `from` must run on to pass the box ([`k::reach_past`]).
#[must_use]
pub fn reach_past(box_min: [f64; 3], box_max: [f64; 3], from: [f64; 3]) -> f64 {
    k::reach_past(vec(box_min), vec(box_max), vec(from))
}

/// The point `far` on from `at` along `along` ([`k::run_on`]).
#[must_use]
pub fn run_on(at: [f64; 3], along: [f64; 3], far: f64) -> Option<[f64; 3]> {
    k::run_on(vec(at), vec(along), far).map(arr)
}

/// Whether a curve from `start` to `end` closes on itself ([`k::closes`]).
#[must_use]
pub fn closes(start: [f64; 3], end: [f64; 3]) -> bool {
    k::closes(vec(start), vec(end))
}

/// How a closed polyline turns about `dir`: positive counter-clockwise ([`k::turn_about`]).
#[must_use]
pub fn turn_about(points: &[[f64; 3]], dir: [f64; 3]) -> f64 {
    let points: Vec<Vec3> = points.iter().map(|p| vec(*p)).collect();
    k::turn_about(&points, vec(dir))
}
