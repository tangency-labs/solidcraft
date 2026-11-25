//! Curve queries as data: where a curve is nearest a point or a ray ([`CurvePoint`],
//! [`CurveRayPoint`]), where two curves
//! cross ([`Crossing`]), where a curve meets a plane ([`PlaneSection`], [`PlanePart`]), what an
//! imported cubic Bézier stands as ([`CubicStanding`], [`CubicPiece`]) and what a fit spline's
//! bounds certify ([`FitSplineBounds`]). Plain data: the queries themselves are the kernel's,
//! through solidcore-ops' `curve` and `curve_fit`.
use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3};

/// The point of a free curve nearest a given point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    /// The curve's parameter there, the record's.
    pub parameter: f64,
    /// The curve's point there.
    pub point: [f64; 3],
    /// Its distance from the given point: the kernel solve's (not certified). A solution
    /// within the tolerance of an end is snapped to that end and keeps the solve's distance, so
    /// it is the distance to `point` within the tolerance.
    pub distance: f64,
}

/// The point of a free curve nearest a ray.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurveRayPoint {
    /// The curve's parameter there, the record's.
    pub parameter: f64,
    /// The curve's point there.
    pub point: [f64; 3],
    /// The ray's point nearest it.
    pub on_ray: [f64; 3],
    /// The distance between the two: the kernel solve's (not certified).
    pub distance: f64,
}

/// Where two free curves cross.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crossing {
    /// The first curve's parameter there, the record's.
    pub a: f64,
    /// The second curve's parameter there, the record's.
    pub b: f64,
    /// The first curve's point there.
    pub on_a: [f64; 3],
    /// The second curve's point there: `on_a` within the tolerance in space; as seen along a
    /// direction, the point the second curve passes behind or in front of it.
    pub on_b: [f64; 3],
}

/// Where a curve meets a plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneSection {
    /// The curve lies in the plane over the whole interval.
    InPlane,
    /// The curve meets the plane at isolated points and over parts of its interval, or
    /// misses it.
    Meets {
        /// The isolated intersections, in curve order and in the plane's local coordinates.
        /// A point at an end of a part in `in_plane` is not among them.
        points: Vec<Vec2>,
        /// The parts of the curve lying in the plane, in curve order. Together they never
        /// cover the whole interval.
        in_plane: Vec<PlanePart>,
    },
}

/// A part of a curve lying in a plane.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanePart {
    /// The part's interval in the record's parameter (radians on a circle or ellipse).
    pub interval: [f64; 2],
    /// The curve's points at the two ends of `interval`.
    pub ends: [Vec3; 2],
}

/// What a cubic Bézier curve on `[0, 1]`, the curve a profile cubic is built as, stands as in
/// the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CubicStanding {
    /// The whole curve lies within the model's zone tolerance of the segment between its ends:
    /// the model cannot tell it from that segment.
    Chord,
    /// A curve that passes the B-spline curve check.
    Curve,
    /// A curve the B-spline curve check refuses (a speed or a radius of curvature below its
    /// tolerance, for example), which no body could keep.
    Refused,
}

/// One piece of an imported cubic Bézier curve: the cubic Bézier curve through `points` on
/// `[0, 1]` and what it stands as.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CubicPiece {
    /// Its four control points.
    pub points: [[f64; 3]; 4],
    /// What it stands as in the model.
    pub standing: CubicStanding,
}

/// What a fit spline's bounds certify, in the points' units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FitSplineBounds {
    /// How far, at most, a point given with a fit point is from the curve there; 0 without
    /// such points.
    pub deviation: f64,
    /// How far, at most, the curve is from the polygon through its fit points, each span
    /// against its chord at the same share of the span.
    pub bulge: f64,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn curve_query_data_keeps_its_serde_names() {
        assert_eq!(
            serde_json::to_string(&CubicStanding::Chord).unwrap(),
            "\"chord\""
        );
        let s = PlaneSection::Meets {
            points: vec![Vec2::new(1.0, 2.0)],
            in_plane: Vec::new(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.starts_with("{\"meets\":"), "{json}");
        assert_eq!(serde_json::from_str::<PlaneSection>(&json).unwrap(), s);
        assert_eq!(
            serde_json::to_string(&PlaneSection::InPlane).unwrap(),
            "\"in_plane\""
        );
    }
}
