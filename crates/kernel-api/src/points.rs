//! Points and picks ([`Points`]): distances, displacements and the centroid of points, a point's
//! nearest place on a polyline, whether a stored pick still lies on its face or edge, and where a
//! tool's flat faces sit back to back on a target. The values are plain numbers and the record's
//! bodies, so the application holds no kernel type.

use solidcore_record::brep::Brep;

use crate::Backend;

/// A distance between two points, with the proved bound on its rounding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    /// The distance.
    pub value: f64,
    /// The proved bound on its rounding.
    pub bound: f64,
}

/// A planar point set's centroid and mean distance from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSetSummary {
    /// The arithmetic mean of the points.
    pub centroid: [f64; 2],
    /// The arithmetic mean of each point's distance from `centroid`.
    pub mean_distance: f64,
}

/// A point query the kernel refuses, in its words: a coordinate not finite, an empty set, a
/// distance that overflows, a record it cannot read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PointsRefused(pub String);

/// A unit normal with its proved bound: the exact unit normal is within `bound` of `value`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitNormal {
    /// The unit normal.
    pub value: [f64; 3],
    /// The proved bound on its distance from the exact one.
    pub bound: f64,
}

/// The face a point classification reads the normal of, when one is picked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointNormalFace {
    /// A record face index.
    Index(usize),
    /// A point on the face; the first containing face in record order is chosen.
    Point([f64; 3]),
}

/// A point's material class and, on the boundary, its face (by record index) and normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClassifiedPoint {
    /// Inside a material region.
    Inside,
    /// Outside material, including holes and cavities.
    Outside,
    /// On a trimmed face within the body's modelling tolerance.
    On {
        /// Face index in the record's order.
        face: usize,
        /// Unit normal facing out of material.
        normal: UnitNormal,
    },
}

/// The points and picks of one kernel ([`crate::OpKind::Points`]).
pub trait Points: Backend {
    /// The distance from `a` to `b`, with its proved rounding bound.
    fn point_distance(&self, a: [f64; 3], b: [f64; 3]) -> Result<Distance, PointsRefused>;
    /// Whether `a` and `b` meet: their rounded distance is not more than `tol`.
    fn points_meet(&self, a: [f64; 3], b: [f64; 3], tol: f64) -> Result<bool, PointsRefused>;
    /// The vector from `from` to `to`.
    fn point_displacement(&self, from: [f64; 2], to: [f64; 2]) -> Result<[f64; 2], PointsRefused>;
    /// The centroid and mean radial distance of a non-empty planar point set.
    fn centroid_and_mean_radius(&self, points: &[[f64; 2]]) -> Result<PointSetSummary, PointsRefused>;
    /// The distance from `point` to the polyline through `points`, and its closest point; `None`
    /// for a polyline of no points.
    fn polyline_2d_closest_point(&self, point: [f64; 2], points: &[[f64; 2]]) -> Result<Option<(f64, [f64; 2])>, PointsRefused>;
    /// Whether `point` still lies on face `face` of `record`, on its surface and within its trims,
    /// to a thousandth of the body's box diagonal.
    fn stored_face(&self, record: &Brep, face: usize, point: [f64; 3]) -> Result<bool, PointsRefused>;
    /// Whether `point` still lies on edge `edge` of `record`, its closest-point distance within a
    /// thousandth of the body's box diagonal.
    fn stored_edge(&self, record: &Brep, edge: usize, point: [f64; 3]) -> Result<bool, PointsRefused>;
    /// Centroids of `tool`'s planar faces that lie back to back on a planar face of `target`; `None`
    /// where either record doesn't convert or the kernel fails.
    fn flush_contact_points(&self, target: &Brep, tool: &Brep) -> Option<Vec<[f64; 3]>>;
    /// `direction` made unit, however short it is: only a direction that is exactly zero or not
    /// finite has none.
    fn unit_direction(&self, direction: [f64; 3]) -> Option<[f64; 3]>;
}
