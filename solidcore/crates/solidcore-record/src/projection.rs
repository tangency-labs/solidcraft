//! Planar projections as data: an edge read exactly in a plane ([`PlanarPiece`], [`PlanarSpline`]),
//! a projected image ready for a sketch ([`Projection`]), how two sketch planes stand to each
//! other ([`PlaneRelation`]) and a piece of a projected outline ([`OutlinePiece`]). Plain data:
//! projecting, classifying and tidying are the kernel's, through solidcore-ops'
//! `conic_projection`, `spline_projection` and `outline`.
use serde::{Deserialize, Serialize};

use crate::{Seg2, Vec2};

/// A free-form curve of several spans in a plane, as the one B-spline it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanarSpline {
    /// Its degree.
    pub degree: usize,
    /// Its clamped knots.
    pub knots: Vec<f64>,
    /// Its control points in the plane's coordinates.
    pub points: Vec<Vec2>,
    /// Its weights (empty: all one).
    pub weights: Vec<f64>,
}

/// One piece of an edge read exactly in a plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanarPiece {
    /// A line, arc or single free-form segment.
    Seg(Seg2),
    /// A free-form curve of several spans.
    Spline(PlanarSpline),
}

/// A planar projected image, ready for an application's link state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    /// An already exact native segment.
    Segment(Seg2),
    /// A point image.
    Point(Vec2),
    /// A complete linear image.
    Line(Vec2, Vec2),
    /// A control spline.
    Spline {
        /// Its exact spline definition.
        spline: PlanarSpline,
        /// The requested source degree.
        degree: usize,
        /// Whether the source omitted uniform knots.
        implicit: bool,
    },
    /// A rational quadratic conic.
    Conic {
        /// Its first point.
        a: Vec2,
        /// Its middle control point.
        apex: Vec2,
        /// Its last point.
        b: Vec2,
        /// Its conic weight ratio.
        rho: f64,
    },
    /// A full ellipse.
    Ellipse {
        /// Its centre.
        center: Vec2,
        /// A point on its major axis.
        major: Vec2,
        /// Its minor radius.
        minor: f64,
    },
}

/// The relative orientation of two sketch planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaneRelation {
    /// Their normals are parallel.
    pub parallel: bool,
    /// Their projected plane has only roundoff width.
    pub edge_on: bool,
    /// Their projection reverses orientation.
    pub flip: bool,
}

/// One piece of a planar outline (the kernel's `solidcore::eval::outline::pieces::Piece`, on the record's vectors).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutlinePiece {
    /// A point.
    Point(Vec2),
    /// A line from the first point to the second.
    Line(Vec2, Vec2),
    /// A circle: its centre and radius.
    Circle(Vec2, f64),
    /// A counter-clockwise arc about `c` from `a` to `b`.
    Arc {
        /// Its centre.
        c: Vec2,
        /// Its start.
        a: Vec2,
        /// Its end.
        b: Vec2,
    },
    /// A rational quadratic conic from `a` to `b` with control `apex` and shape `rho`.
    Conic {
        /// Its start.
        a: Vec2,
        /// Its middle control point.
        apex: Vec2,
        /// Its end.
        b: Vec2,
        /// Its shape: the middle weight `w` over `1 + w`.
        rho: f64,
    },
    /// A full ellipse: its centre, the end of its major axis and its minor radius.
    Ellipse {
        /// Its centre.
        c: Vec2,
        /// The end of its major axis.
        major: Vec2,
        /// Its minor radius.
        minor: f64,
    },
    /// A spline through (`control` false) or over (`control`) its points, with its own knots and
    /// weights where it has them.
    Spline {
        /// Its points.
        pts: Vec<Vec2>,
        /// The points are control points.
        control: bool,
        /// Its degree.
        degree: u8,
        /// It closes on itself.
        periodic: bool,
        /// Its knots and weights, where they are not uniform.
        nurbs: Option<(Vec<f64>, Vec<f64>)>,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn projection_data_keeps_its_serde_names() {
        let p = PlanarPiece::Spline(PlanarSpline {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)],
            weights: Vec::new(),
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.starts_with("{\"spline\":"), "{json}");
        assert_eq!(serde_json::from_str::<PlanarPiece>(&json).unwrap(), p);
        let o = OutlinePiece::Line(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.starts_with("{\"line\":"), "{json}");
    }
}
