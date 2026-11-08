//! A section view's cutting line as data (W34, #13046): the line a drafter draws on a parent view,
//! and what the section view takes from it. Nothing here cuts or draws; the section producers do.
//!
//! - A [`CuttingLine`] is a chain of points in the parent view's plane coordinates
//!   ([`Plane::to_local`] of its basis, model millimetres), so it stays put when the parent's scale
//!   or place changes. Its [`SectionKind`] says its shape, and its [`ArrowSide`] which side of the
//!   chain, walked from its first point, its arrows point to: the side that is kept and seen.
//! - The section view looks the arrows' way across the first segment ([`CuttingLine::basis`]).
//! - Each segment cuts along a plane through it and the parent's normal ([`CuttingLine::planes`]).
//! - The kept side is a region of the parent's plane ([`CuttingLine::kept`]), the shape a cutting
//!   tool is extruded from.

use crate::prelude::*;

use serde::{Deserialize, Serialize};

use crate::Aabb2;
use crate::drawing_view::{Invalid, PARALLEL, require};
use crate::section::{MAX_NUMBER, validate_plane};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};

type Result<T> = std::result::Result<T, Invalid>;

/// The most points a cutting line has.
pub const MAX_POINTS: usize = 32;
/// The shortest segment of a cutting line, in millimetres.
pub const MIN_LENGTH: f64 = 1e-6;

/// A cutting line's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// One straight cut: two points.
    Full,
    /// A cut to the middle that turns a quarter turn, taking a quarter away: three points.
    Half,
    /// A stepped cut: legs that all run the first one's way, joined by square steps, four or more
    /// points, a leg first and last.
    Offset,
    /// Two arms about a centre, the middle of three points; the view turns the second arm into the
    /// first's plane ([`CuttingLine::turn`]).
    Aligned,
}

/// The side of a cutting line, walked from its first point, its arrows point to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowSide {
    Left,
    Right,
}

/// A section view's cutting line, checked: only [`CuttingLine::new`] makes one, and reading one
/// goes through it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Stored", into = "Stored")]
pub struct CuttingLine {
    kind: SectionKind,
    points: Vec<Vec2>,
    arrows: ArrowSide,
}

/// A cutting line as it is saved, before its checks.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    kind: SectionKind,
    points: Vec<Vec2>,
    arrows: ArrowSide,
}

impl TryFrom<Stored> for CuttingLine {
    type Error = Invalid;

    fn try_from(s: Stored) -> Result<CuttingLine> {
        CuttingLine::new(s.kind, s.points, s.arrows)
    }
}

impl From<CuttingLine> for Stored {
    fn from(line: CuttingLine) -> Stored {
        Stored { kind: line.kind, points: line.points, arrows: line.arrows }
    }
}

/// The sine of the angle from `a` to `b`, and its cosine. Neither is zero length (each segment is
/// at least [`MIN_LENGTH`]).
fn sin_cos(a: Vec2, b: Vec2) -> (f64, f64) {
    let lengths = a.len() * b.len();
    (a.cross(b) / lengths, a.dot(b) / lengths)
}

impl CuttingLine {
    /// The line through `points` of shape `kind`, its arrows to `arrows`. Every point is finite and
    /// within 1e12 mm, there are 2 to [`MAX_POINTS`] of them, and each segment is at least
    /// [`MIN_LENGTH`] long. A square turn and parallel legs are within [`PARALLEL`] (the cosine or
    /// sine of their angle): the same tolerance as a view's directions. Within those rules no kind's
    /// line can cross itself, so none is checked for it.
    pub fn new(kind: SectionKind, points: Vec<Vec2>, arrows: ArrowSide) -> Result<CuttingLine> {
        require((2..=MAX_POINTS).contains(&points.len()), "a cutting line has 2 to 32 points")?;
        let finite = points.iter().all(|p| p.is_finite() && p.x.abs() <= MAX_NUMBER && p.y.abs() <= MAX_NUMBER);
        require(finite, "a cutting line's points are finite and within 1e12 mm")?;
        let segments: Vec<Vec2> = points.windows(2).map(|w| w[1] - w[0]).collect();
        require(segments.iter().all(|s| s.len() >= MIN_LENGTH), "a cutting line's points are at least 0.000001 mm apart")?;
        let turns = || segments.windows(2).map(|w| sin_cos(w[0], w[1]));
        match kind {
            SectionKind::Full => require(points.len() == 2, "a full section's cutting line has 2 points")?,
            SectionKind::Half => {
                require(points.len() == 3, "a half section's cutting line has 3 points")?;
                require(turns().all(|(_, cos)| cos.abs() <= PARALLEL), "a half section's cutting line turns a square corner")?;
            }
            SectionKind::Offset => {
                require(
                    points.len() >= 4 && points.len().is_multiple_of(2),
                    "an offset section's cutting line has an even number of points, 4 or more",
                )?;
                require(turns().all(|(_, cos)| cos.abs() <= PARALLEL), "an offset section's cutting line turns only square corners")?;
                // Every leg (segments 0, 2, 4, …) runs the first one's way, so the line steps one way
                // along it and its ends, carried on, never meet it.
                let first = segments.first().copied().unwrap_or(Vec2::ZERO);
                let along = segments.iter().step_by(2).map(|leg| sin_cos(first, *leg)).all(|(sin, cos)| sin.abs() <= PARALLEL && cos > 0.0);
                require(along, "an offset section's legs all run the same way as its first")?;
            }
            SectionKind::Aligned => {
                require(points.len() == 3, "an aligned section's cutting line has 3 points, the centre in the middle")?;
                require(turns().all(|(sin, _)| sin.abs() >= PARALLEL), "an aligned section's arms meet at an angle, not straight on or folded back")?;
            }
        }
        Ok(CuttingLine { kind, points, arrows })
    }

    pub fn kind(&self) -> SectionKind {
        self.kind
    }

    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    pub fn arrows(&self) -> ArrowSide {
        self.arrows
    }

    /// The first segment's direction, and the arrows' direction square to it, in the parent's plane.
    fn first_way(&self) -> (Vec2, Vec2) {
        let (a, b) = match self.points.as_slice() {
            [a, b, ..] => (*a, *b),
            _ => (Vec2::ZERO, Vec2::X),
        };
        let along = (b - a).normalized().unwrap_or(Vec2::X);
        let arrows = match self.arrows {
            ArrowSide::Left => along.perp(),
            ArrowSide::Right => -along.perp(),
        };
        (along, arrows)
    }

    /// The section view's basis, looking the arrows' way across the first segment: the parent's
    /// basis turned a quarter turn about the segment, through the parent's origin, as a projected
    /// view is turned about the edge it shares with its parent ([`crate::drawing_view::projected_basis`]).
    /// The turn keeps the segment's direction and takes the arrows' direction to the parent's normal
    /// (and the normal to against the arrows, the new view's viewer behind them). On a front view, a
    /// horizontal line with its arrows down gives the top view, one with its arrows up the bottom
    /// view, a vertical one with its arrows right the left view; which way the line was drawn
    /// doesn't matter.
    pub fn basis(&self, parent: &Plane) -> Result<Plane> {
        validate_plane(parent).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
        let (along, arrows) = self.first_way();
        let (line, normal) = (parent.x * along.x + parent.y * along.y, parent.x.cross(parent.y));
        // Each paper axis of the parent is `along` and `arrows` in its parts; the turn moves the part
        // along the arrows onto the normal.
        let turned = |axis: Vec2| line * axis.dot(along) + normal * axis.dot(arrows);
        let basis = Plane { origin: parent.origin, x: turned(Vec2::X), y: turned(Vec2::Y) };
        validate_plane(&basis).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
        Ok(basis)
    }

    /// Each segment's cutting plane in the model: through the segment and the parent's normal, its x
    /// along the segment and its normal toward the arrows. An offset line's steps have planes too.
    pub fn planes(&self, parent: &Plane) -> Result<Vec<Plane>> {
        validate_plane(parent).map_err(|_| Invalid("a view's basis is finite and orthonormal"))?;
        let normal = parent.x.cross(parent.y);
        // A segment's direction × the parent's normal points to its right.
        let side = match self.arrows {
            ArrowSide::Left => -normal,
            ArrowSide::Right => normal,
        };
        let world = |p: Vec2| parent.x * p.x + parent.y * p.y;
        let plane = |a: Vec2, b: Vec2| {
            let x = world(b - a).normalized().ok_or(Invalid("a cutting line's segment is too short"))?;
            Ok(Plane { origin: parent.origin + world(a), x, y: side })
        };
        self.points.windows(2).map(|w| plane(w[0], w[1])).collect()
    }

    /// A region of the parent's plane holding every point of `around` (in plane coordinates) on the
    /// arrows' side of the line and none on the other: the line with its ends carried straight on
    /// to a square around both it and `around`, closed along the square on the arrows' side. One
    /// anticlockwise outer loop of lines, no holes. A line that misses `around` keeps all of it or
    /// none.
    pub fn kept(&self, around: Aabb2) -> Result<Region2> {
        let finite = [around.min, around.max].iter().all(|p| p.is_finite() && p.x.abs() <= MAX_NUMBER && p.y.abs() <= MAX_NUMBER);
        require(!around.is_empty() && finite, "the box a cutting line keeps from is finite, within 1e12 mm and not inside out")?;
        // Walked so the kept side is on the left: an anticlockwise loop has its inside there.
        let mut chain = self.points.clone();
        if self.arrows == ArrowSide::Right {
            chain.reverse();
        }
        let mut all = around;
        chain.iter().for_each(|p| all.add(*p));
        let centre = (all.min + all.max) * 0.5;
        // Half the square's side: past every point of the line and the box, by 1 mm at least.
        let half = (all.max - all.min).len() + 1.0;
        // Each end carried on along its own segment (for two points, the one segment both ways).
        let end_pair = |at: usize, next: usize| Some((*chain.get(at)?, *chain.get(next)?));
        let last = chain.len().saturating_sub(1);
        let (Some((first, second)), Some((end_point, before_end))) = (end_pair(0, 1), end_pair(last, last.saturating_sub(1))) else {
            return Err(Invalid("a cutting line has 2 to 32 points"));
        };
        let (start, start_round) = exit(first, first - second, centre, half)?;
        let (end, end_round) = exit(end_point, end_point - before_end, centre, half)?;
        let mut points = vec![start];
        points.extend(chain);
        points.push(end);
        points.extend(corners_between(end_round, start_round, centre, half));
        let segs = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: *a, b: *b }).collect();
        Ok(Region2 { outer: Loop2 { segs }, holes: Vec::new() })
    }

    /// For an aligned section, the turn (radians, anticlockwise about the parent's normal through the
    /// centre) that lays the second arm onto the first arm carried on past the centre; for the
    /// other kinds, none.
    pub fn turn(&self) -> Option<f64> {
        match (self.kind, self.points.as_slice()) {
            (SectionKind::Aligned, [first, centre, second]) => {
                let (onto, arm) = (*centre - *first, *second - *centre);
                Some(arm.cross(onto).atan2(arm.dot(onto)))
            }
            _ => None,
        }
    }
}

/// Where the ray from `from` along `way` leaves the square of half side `half` about `centre`, and
/// how far round the square's edge that is, anticlockwise from its lower right corner (0 to 8 ×
/// `half`). `from` is inside the square.
fn exit(from: Vec2, way: Vec2, centre: Vec2, half: f64) -> Result<(Vec2, f64)> {
    let way = way.normalized().ok_or(Invalid("a cutting line's segment is too short"))?;
    let reach = |p: f64, w: f64, c: f64| match w {
        w if w > 0.0 => (c + half - p) / w,
        w if w < 0.0 => (c - half - p) / w,
        _ => f64::INFINITY,
    };
    let (across, up) = (reach(from.x, way.x, centre.x), reach(from.y, way.y, centre.y));
    let at = from + way * across.min(up) - centre;
    // Which edge it leaves by, its coordinate set exactly on it.
    let (point, round) = if across <= up {
        if way.x > 0.0 { (Vec2::new(half, at.y), half + at.y) } else { (Vec2::new(-half, at.y), 5.0 * half - at.y) }
    } else if way.y > 0.0 {
        (Vec2::new(at.x, half), 3.0 * half - at.x)
    } else {
        (Vec2::new(at.x, -half), 7.0 * half + at.x)
    };
    require(point.is_finite(), "a cutting line's region is finite")?;
    Ok((centre + point, round))
}

/// The square's corners passed going anticlockwise round its edge from `from` to `to` (each how far
/// round, as [`exit`] gives it), in order.
fn corners_between(from: f64, to: f64, centre: Vec2, half: f64) -> Vec<Vec2> {
    let perimeter = 8.0 * half;
    let ahead = |at: f64| (at - from).rem_euclid(perimeter);
    let span = ahead(to);
    // The corners anticlockwise from the upper right, how far round each is.
    let corners = [(2.0, Vec2::new(half, half)), (4.0, Vec2::new(-half, half)), (6.0, Vec2::new(-half, -half)), (8.0, Vec2::new(half, -half))];
    let mut passed: Vec<(f64, Vec2)> = corners.iter().map(|(at, c)| (ahead(at * half), centre + *c)).filter(|(d, _)| *d > 0.0 && *d < span).collect();
    passed.sort_by(|a, b| a.0.total_cmp(&b.0));
    passed.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
#[path = "drawing_section_tests.rs"]
mod tests;
