//! Measurement on drawing views (W19a, #4896): what a dimension on a view reads, in model
//! millimetres or radians, from model points and edges and the view's basis. One rule for every
//! dimension: the dimension record (W19b) keeps what to measure, and W09 formats the number.
//!
//! A value is **projected** (as the view shows it) or **true** (as the model has it);
//! `foreshortened` says a measured line isn't parallel to the view's plane, so the two differ. The
//! view's scale and place on the sheet never change a measurement.

use serde::{Deserialize, Serialize};

use crate::drawing_view::{Invalid, PARALLEL, require};
use crate::section::{MAX_NUMBER, validate_plane};
use crate::{EPS, Plane, Vec2, Vec3};

type Result<T> = std::result::Result<T, Invalid>;

/// Which way a linear dimension measures on its view.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LinearDirection {
    /// Along the view's horizontal (paper x).
    Horizontal,
    /// Along the view's vertical (paper y).
    Vertical,
    /// Along the segment as the view shows it.
    Aligned,
    /// Along a direction on the view, in its plane's coordinates (any length but zero): a rotated
    /// dimension, or one along a picked edge.
    Along(Vec2),
}

/// What a dimension reads: the length or angle as the view shows it, or as the model has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reading {
    Projected,
    True,
}

/// The line an angle is measured from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AngleFrom {
    /// The view's horizontal, pointing right (paper +x).
    Horizontal,
    /// The view's vertical, pointing up (paper +y).
    Vertical,
    /// A model direction, such as the part's axis, as the view shows it.
    ModelAxis(Vec3),
    /// Another edge, from its first point to its second.
    Edge(Vec3, Vec3),
}

/// What a dimension reads, and whether a measured line is out of the view's plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    /// Model millimetres, or radians for an angle.
    pub value: f64,
    /// A measured line isn't parallel to the view's plane, so its projected value differs from
    /// the model's.
    pub foreshortened: bool,
}

/// The distance from `a` to `b` (model points) that a linear dimension measuring `direction` on
/// the view through `basis` reads. A true reading is the segment's own length, so it goes with
/// [`LinearDirection::Aligned`] only: a true horizontal has no one meaning in a tilted view.
pub fn linear(basis: &Plane, a: Vec3, b: Vec3, direction: LinearDirection, reading: Reading) -> Result<Measured> {
    let d = segment(basis, a, b)?;
    let shown = Vec2::new(d.dot(basis.x), d.dot(basis.y));
    let value = match (direction, reading) {
        (LinearDirection::Aligned, Reading::True) => d.len(),
        (_, Reading::True) => return Err(Invalid("a true length is along the segment: measure it aligned")),
        (LinearDirection::Horizontal, Reading::Projected) => shown.x.abs(),
        (LinearDirection::Vertical, Reading::Projected) => shown.y.abs(),
        (LinearDirection::Aligned, Reading::Projected) => shown.len(),
        (LinearDirection::Along(along), Reading::Projected) => {
            let along = along.normalized().ok_or(Invalid("the direction to measure along is zero or not finite"))?;
            shown.dot(along).abs()
        }
    };
    require(value > EPS, "the dimension would read 0: the points line up across the direction it measures")?;
    Ok(Measured { value, foreshortened: out_of_plane(basis, d) })
}

/// What a chain of linear dimensions reads: one member from each point to the next, and the
/// overall from the first to the last, which the members add up to.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub members: Vec<Measured>,
    pub overall: Measured,
}

/// The chain of linear dimensions through `points` (model points, in order) on the view through
/// `basis`, every member measured along one line as the view shows it: `direction` as [`linear`]
/// takes it, aligned being the line from the first point to the last. (A chain reads projected:
/// the true lengths between points off one line don't add up.) The line runs the way the first
/// point to the last goes, and each member must be more than 0 along it, so a point out of order,
/// or lined up across the line with the one before, is refused, never moved.
pub fn chain(basis: &Plane, points: &[Vec3], direction: LinearDirection) -> Result<Chain> {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return Err(Invalid("a chain has no points")) };
    require(points.len() >= 3, "a chain is at least 3 points: 2 make one linear dimension")?;
    let whole = segment(basis, *first, *last)?;
    let shown = |d: Vec3| Vec2::new(d.dot(basis.x), d.dot(basis.y));
    let line = match direction {
        LinearDirection::Horizontal => Vec2::new(1.0, 0.0),
        LinearDirection::Vertical => Vec2::new(0.0, 1.0),
        LinearDirection::Aligned => shown(whole).normalized().ok_or(Invalid("the chain's first and last points are one point in this view"))?,
        LinearDirection::Along(along) => along.normalized().ok_or(Invalid("the direction to measure along is zero or not finite"))?,
    };
    let line = if shown(whole).dot(line) < 0.0 { -line } else { line };
    let members = points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| {
            let d = segment(basis, *a, *b)?;
            let value = shown(d).dot(line);
            require(value > EPS, "a chain's points run one way along its line: each one past the one before")?;
            Ok(Measured { value, foreshortened: out_of_plane(basis, d) })
        })
        .collect::<Result<Vec<_>>>()?;
    let overall = Measured { value: shown(whole).dot(line), foreshortened: members.iter().any(|m| m.foreshortened) };
    Ok(Chain { members, overall })
}

/// The way from a cylinder's axis to its silhouette on one side, on the view through `basis` (the
/// other side's is the opposite way): square to the axis and to the view's normal. Seen any way but
/// along its axis, a cylinder shows as a strip as wide as its diameter, so the diameter between
/// its silhouettes reads true.
pub fn silhouette_side(basis: &Plane, axis: Vec3) -> Result<Vec3> {
    validate_plane(basis).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
    let axis = unit_axis(axis)?;
    // A view along the axis makes this zero, which `normalized` refuses.
    basis.normal().cross(axis).normalized().ok_or(Invalid("the cylinder is seen end-on in this view: its rim is a circle"))
}

/// A cylinder as a measurement takes it (model): a point on its axis, its axis, and its radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    pub origin: Vec3,
    pub axis: Vec3,
    pub radius: f64,
}

/// Which of a cylinder's two silhouettes on a view, by where another cylinder is there: the one
/// nearer to the other's axis, or the one farther from it. Named so, a dimension between two
/// cylinders keeps to the silhouettes it was given whichever way round the view shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilhouetteSide {
    Near,
    Far,
}

/// The distance between silhouette `a_side` of cylinder `a` and silhouette `b_side` of cylinder
/// `b` as the view through `basis` shows them, along `direction` as [`linear`] takes it (aligned:
/// square to the silhouettes). The silhouettes must be parallel on the view, and the axes apart
/// there, or near and far would mean nothing. Also the ways from each axis to its silhouette, for
/// where the dimension is drawn. Foreshortened when the axes aren't parallel in the model or the
/// silhouettes lie at different depths.
pub fn between_silhouettes(
    basis: &Plane,
    a: &Cylinder,
    a_side: SilhouetteSide,
    b: &Cylinder,
    b_side: SilhouetteSide,
    direction: LinearDirection,
) -> Result<(Measured, [Vec3; 2])> {
    let round = |c: &Cylinder| model_point(c.origin) && c.radius > EPS && c.radius <= MAX_NUMBER;
    require(round(a) && round(b), "a cylinder has a point on its axis within 1e12 mm of the origin, and a radius over 0")?;
    let (wa, wb) = (silhouette_side(basis, a.axis)?, silhouette_side(basis, b.axis)?);
    require(wa.cross(wb).len() <= PARALLEL, "the cylinders' silhouettes aren't parallel in this view")?;
    // How far b's axis is from a's on the view, square to them.
    let gap = (b.origin - a.origin).dot(wa);
    require(gap.abs() > EPS, "the cylinders' axes are one line in this view: give each one's diameter instead")?;
    let toward = wa * gap.signum();
    let way = |side, toward: Vec3| if side == SilhouetteSide::Near { toward } else { -toward };
    let (a_way, b_way) = (way(a_side, toward), way(b_side, -toward));
    let (pa, pb) = (a.origin + a_way * a.radius, b.origin + b_way * b.radius);
    let across = (pb - pa).dot(toward);
    let along = match direction {
        LinearDirection::Aligned => None,
        LinearDirection::Horizontal => Some(Vec2::X),
        LinearDirection::Vertical => Some(Vec2::Y),
        LinearDirection::Along(v) => Some(v.normalized().ok_or(Invalid("the direction to measure along is zero or not finite"))?),
    };
    let value = match along {
        None => across.abs(),
        // Along a direction, as far as a line that way runs from one silhouette to the other.
        Some(v) => {
            let slant = Vec2::new(toward.dot(basis.x), toward.dot(basis.y)).dot(v);
            require(slant.abs() > PARALLEL, "the silhouettes run along the direction to measure: it never crosses from one to the other")?;
            (across / slant).abs()
        }
    };
    require(value > EPS, "the dimension would read 0: the silhouettes are one line in this view")?;
    // Parallel in the model, the silhouettes are as far apart as the view shows when the line square
    // to both lies in the view's plane.
    let (ua, ub) = (unit_axis(a.axis)?, unit_axis(b.axis)?);
    let square = (pb - pa) - ua * (pb - pa).dot(ua);
    let foreshortened = ua.cross(ub).len() > PARALLEL || out_of_plane(basis, square);
    Ok((Measured { value, foreshortened }, [a_way, b_way]))
}

/// The angle from the line `from` to `edge` (model points, from its first to its second) on the
/// view through `basis`, from 0 to π: each line goes the way its caller gives it (from the vertex
/// outwards), so the other side of the angle reads π minus it. A true reading is the angle
/// between the model lines, so it isn't given from the view's horizontal or vertical.
pub fn angle(basis: &Plane, edge: (Vec3, Vec3), from: AngleFrom, reading: Reading) -> Result<Measured> {
    let d = segment(basis, edge.0, edge.1)?;
    let (reference, model_line) = match from {
        AngleFrom::Horizontal => (basis.x, false),
        AngleFrom::Vertical => (basis.y, false),
        AngleFrom::ModelAxis(axis) => (axis.normalized().ok_or(Invalid("the model axis is zero or not finite"))?, true),
        AngleFrom::Edge(a, b) => (segment(basis, a, b)?, true),
    };
    let value = match reading {
        Reading::True if model_line => between(d, reference)?,
        Reading::True => return Err(Invalid("a true angle is between two model lines, not from the view's horizontal or vertical")),
        Reading::Projected => between(on_view(basis, d)?, on_view(basis, reference)?)?,
    };
    Ok(Measured { value, foreshortened: out_of_plane(basis, d) || (model_line && out_of_plane(basis, reference)) })
}

/// `b - a`, both finite model points within 1e12 mm of the origin, the segment not of zero length,
/// on a valid basis.
fn segment(basis: &Plane, a: Vec3, b: Vec3) -> Result<Vec3> {
    validate_plane(basis).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
    require(model_point(a) && model_point(b), "a model point is finite and within 1e12 mm of the origin")?;
    let d = b - a;
    require(d.len() > EPS, "a segment or edge has no length")?;
    Ok(d)
}

/// A cylinder's axis as a unit vector: refused when it is zero or not finite.
fn unit_axis(axis: Vec3) -> Result<Vec3> {
    axis.normalized().ok_or(Invalid("the cylinder's axis is zero or not finite"))
}

/// A model point a measurement takes: finite, and within 1e12 mm of the origin.
fn model_point(p: Vec3) -> bool {
    p.is_finite() && [p.x, p.y, p.z].iter().all(|v| v.abs() <= MAX_NUMBER)
}

/// `d` as the view shows it, in its plane's coordinates: refused when the view sees it end-on.
fn on_view(basis: &Plane, d: Vec3) -> Result<Vec3> {
    let shown = Vec3::new(d.dot(basis.x), d.dot(basis.y), 0.0);
    require(shown.len() > PARALLEL * d.len(), "a line is seen end-on in this view: it has no direction there")?;
    Ok(shown)
}

/// The angle between `u` and `v`, from 0 to π: refused when they are parallel either way.
fn between(u: Vec3, v: Vec3) -> Result<f64> {
    let sine = u.cross(v).len();
    require(sine > PARALLEL * u.len() * v.len(), "the lines are parallel: there is no angle between them")?;
    Ok(sine.atan2(u.dot(v)))
}

/// Whether `d` leaves the view's plane: the sine of its angle to the plane is over [`PARALLEL`].
fn out_of_plane(basis: &Plane, d: Vec3) -> bool {
    d.dot(basis.normal()).abs() > PARALLEL * d.len()
}

#[cfg(test)]
#[path = "drawing_measure_tests.rs"]
mod tests;
