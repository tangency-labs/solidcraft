//! Exact line/arc stock paths: terminal tangents, traversal, section frames and bend clearance.
//!
//! Moved from SolidCraft's frame-member evaluator. The sweep itself remains
//! [`super::segments::sweep_along_segments`]; this module prepares its exact inputs.
//! Frames follow Bishop's parallel transport: constant on a line, Rodrigues rotation on an
//! arc, carrying the previous span's frame into the next (docs/procedural-sweep.md §3.2).
//! Section enclosures use the convex-hull property (Piegl and Tiller, The NURBS Book, §4.2).
//! Circular supports use Cauchy-Schwarz, with outward-rounded interval arithmetic
//! (docs/definitions-and-certificates.md §4.2).
//! Not upstream. Deviations: none. No recursion; at most 1000 path pieces.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::path::Segment;
use crate::eval::Frame;
use crate::math::{Point2, Point3, Vec3};
use crate::prism::profile::{Region2, Seg2};
use crate::surface::procedural_sweep::path::rodrigues as rotate;

/// Minimum stock length and radius (mm), as the frame-member command has always used.
pub const MIN_LENGTH: f64 = 1e-6;

/// Maximum lean of a vertical stock member before its section follows world up.
pub const VERTICAL_TOLERANCE: f64 = 1e-6;

/// How far a piece's given unit vectors (its tangents, an arc's axis) may lie from the ones its
/// constructor makes, for [`Piece::from_parts`]. Making them again from the piece's own ends
/// moves them by a few rounding units (about 1e-16); within 1e-13 of a unit vector, two such
/// tangents that run on exactly from each other still pass [`orient`]'s tangency test, whose
/// cosine margin is `1 - cos(1e-6)`, about 5e-13.
pub const PARTS_TOLERANCE: f64 = 1e-13;

/// Why an exact stock path or its section cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// A line has no finite direction or has coordinates outside the model's range.
    #[error("the line has no length or has invalid coordinates")]
    Line,
    /// The circle data do not define one finite open arc.
    #[error("the arc needs a finite, open circular path")]
    Arc,
    /// No finite frame can be laid square to the starting tangent.
    #[error("no plane square to the member")]
    Frame,
    /// Two consecutive curves do not meet; the index names the later curve.
    #[error(
        "curve {0} is not connected end to end in path order; pick the curves in order along the path"
    )]
    Connection(usize),
    /// Two consecutive curves do not have the same tangent; the index names the later curve.
    #[error("curve {0} is not tangent to the previous curve; draw a bend arc")]
    Tangency(usize),
    /// Stock must have two distinct ends.
    #[error("not supported yet: a continuous member must have two open ends")]
    Closed,
    /// The section reaches the bend axis; the index names that arc.
    #[error("arc {0}: the bend radius is too small for the anchored profile")]
    Bend(usize),
    /// The section has no finite certified directional enclosure.
    #[error("not supported yet: the profile has no certified bound for bend clearance")]
    Profile,
    /// The path is empty or exceeds the command's bound.
    #[error("a member path needs 1 to 1000 curves")]
    Count,
}

/// A validated exact segment and its terminal data, independent of display polylines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    /// Start point.
    pub a: Point3,
    /// End point.
    pub b: Point3,
    /// Unit starting tangent.
    pub t0: Vec3,
    /// Unit ending tangent.
    pub t1: Vec3,
    /// The line or circular arc passed to the exact sweep.
    pub segment: Segment,
}

fn finite(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

fn point_ok(p: Vec3) -> bool {
    finite(p) && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= 1e9
}

// The same normalisation and operation order as the former evaluator (geom's EPS = 1e-9).
fn unit(v: Vec3) -> Option<Vec3> {
    let length = v.length();
    (length > 1e-9 && length.is_finite()).then(|| v / length)
}

impl Piece {
    /// The exact line from `a` to `b`.
    ///
    /// # Errors
    /// [`Refusal::Line`] for a collapsed line or invalid coordinates.
    pub fn line(a: Point3, b: Point3) -> Result<Self, Refusal> {
        let t = unit(b - a)
            .filter(|_| a.distance(b) > MIN_LENGTH)
            .ok_or(Refusal::Line)?;
        if !point_ok(a) || !point_ok(b) {
            return Err(Refusal::Line);
        }
        Ok(Self {
            a,
            b,
            t0: t,
            t1: t,
            segment: Segment::Line { start: a, end: b },
        })
    }

    /// An open arc through its two ends, swept positively about `axis` by `angle` radians.
    ///
    /// # Errors
    /// [`Refusal::Arc`] if its data disagree or are not finite.
    pub fn arc(
        a: Point3,
        b: Point3,
        center: Point3,
        axis: Vec3,
        angle: f64,
    ) -> Result<Self, Refusal> {
        let k = unit(axis).ok_or(Refusal::Arc)?;
        let radius = a.distance(center);
        if !point_ok(a)
            || !point_ok(b)
            || !point_ok(center)
            || !radius.is_finite()
            || radius <= MIN_LENGTH
            || !angle.is_finite()
            || angle <= MIN_LENGTH
            || angle >= std::f64::consts::TAU - 1e-9
            || (b.distance(center) - radius).abs() > MIN_LENGTH
            || (a - center).dot(k).abs() > 1e-6
            || (b - center).dot(k).abs() > 1e-6
            // The stored endpoint also drives joins, reversal and terminal planes. Its
            // distance from the swept endpoint must use the same absolute gap as joins,
            // regardless of the circle's radius.
            || (center + rotate(a - center, k, angle)).distance(b) > MIN_LENGTH
        {
            return Err(Refusal::Arc);
        }
        let t0 = unit(k.cross(a - center)).ok_or(Refusal::Arc)?;
        let t1 = unit(k.cross(b - center)).ok_or(Refusal::Arc)?;
        Ok(Self {
            a,
            b,
            t0,
            t1,
            segment: Segment::Arc {
                start: a,
                centre: center,
                axis: k,
                angle,
            },
        })
    }

    /// A spatial circle's open positive arc, with its angle derived from its exact ends.
    ///
    /// # Errors
    /// [`Refusal::Arc`] for inconsistent circle data or a closed arc.
    pub fn spatial_arc(
        a: Point3,
        b: Point3,
        center: Point3,
        normal: Vec3,
        radius: f64,
    ) -> Result<Self, Refusal> {
        if !radius.is_finite()
            || !finite(normal)
            || (normal.length() - 1.0).abs() > 1e-6
            || (a.distance(center) - radius).abs() > 1e-6
        {
            return Err(Refusal::Arc);
        }
        let u = a - center;
        let v = b - center;
        let angle = normal
            .dot(u.cross(v))
            .atan2(u.dot(v))
            .rem_euclid(std::f64::consts::TAU);
        Self::arc(a, b, center, normal, angle)
    }

    /// A line or arc in its sketch's plane, placed exactly in world coordinates.
    ///
    /// # Errors
    /// [`Refusal::Arc`] for an unsupported segment or invalid circle; [`Refusal::Line`]
    /// for a collapsed line.
    pub fn planar(plane: &Frame, segment: &Seg2) -> Result<Self, Refusal> {
        let x = Vec3::from(plane.x_axis);
        let y = Vec3::from(plane.y_axis);
        let at = |p: Point2| Vec3::from(plane.origin) + x * p.x + y * p.y;
        match *segment {
            Seg2::Line { a, b } => Self::line(at(a), at(b)),
            Seg2::Arc { center, sweep, .. } => Self::arc(
                at(segment.start()),
                at(segment.end()),
                at(center),
                x.cross(y) * sweep.signum(),
                sweep.abs(),
            ),
            _ => Err(Refusal::Arc),
        }
    }

    /// The line's retained parameter interval, without inspecting display samples.
    ///
    /// # Errors
    /// [`Refusal::Line`] for a non-finite or collapsed retained interval.
    pub fn retained_line(start: Point3, end: Point3, interval: [f64; 2]) -> Result<Self, Refusal> {
        let d = end - start;
        let [from, to] = interval;
        Self::line(start + d * from, start + d * to)
    }

    /// A piece from its fields as given (an application's copy of a piece's data), checked as
    /// the constructors check them: `segment` must be a line [`Piece::line`] accepts from `a` to
    /// `b`, or an arc from `a` that [`Piece::arc`] accepts ending at `b` whose axis is the unit
    /// axis that constructor stores, and `t0` and `t1` the unit tangents it gives, each to
    /// [`PARTS_TOLERANCE`]. The fields are kept exactly as given, so every piece's own fields
    /// make that piece again, reversed or not.
    ///
    /// # Errors
    /// [`Refusal::Line`] for a line, or [`Refusal::Arc`] for an arc, that the constructor
    /// refuses or whose ends, axis or tangents disagree with it.
    pub fn from_parts(
        a: Point3,
        b: Point3,
        t0: Vec3,
        t1: Vec3,
        segment: Segment,
    ) -> Result<Self, Refusal> {
        let (made, refusal) = match segment {
            Segment::Line { start, end } => (Self::line(start, end), Refusal::Line),
            Segment::Arc {
                start,
                centre,
                axis,
                angle,
            } => (Self::arc(start, b, centre, axis, angle), Refusal::Arc),
        };
        let made = made?;
        // NaN compares false, so a non-finite tangent or axis is refused here.
        let near = |u: Vec3, v: Vec3| (u - v).length() <= PARTS_TOLERANCE;
        let axis_kept = match (segment, made.segment) {
            (Segment::Arc { axis, .. }, Segment::Arc { axis: unit, .. }) => near(axis, unit),
            _ => true,
        };
        if made.a != a || made.b != b || !axis_kept || !near(made.t0, t0) || !near(made.t1, t1) {
            return Err(refusal);
        }
        Ok(Self {
            a,
            b,
            t0,
            t1,
            segment,
        })
    }

    /// Reverse the traversal while keeping the same exact geometric locus.
    pub fn reverse(&mut self) {
        self.segment = match self.segment {
            Segment::Line { start, end } => Segment::Line {
                start: end,
                end: start,
            },
            Segment::Arc {
                centre,
                axis,
                angle,
                ..
            } => Segment::Arc {
                start: self.b,
                centre,
                axis: -axis,
                angle,
            },
        };
        std::mem::swap(&mut self.a, &mut self.b);
        let old = self.t0;
        self.t0 = -self.t1;
        self.t1 = -old;
    }

    /// Its straight length, or `None` for an arc.
    #[must_use]
    pub fn line_length(&self) -> Option<f64> {
        matches!(self.segment, Segment::Line { .. }).then(|| self.a.distance(self.b))
    }
}

/// Orient the supplied traversal, reversing individual curves while preserving pick order.
///
/// # Errors
/// [`Refusal::Count`], [`Refusal::Connection`], [`Refusal::Tangency`] or [`Refusal::Closed`].
pub fn orient(pieces: &mut [Piece]) -> Result<(), Refusal> {
    if pieces.is_empty() || pieces.len() > 1000 {
        return Err(Refusal::Count);
    }
    if let (Some(first), Some(next)) = (pieces.first(), pieces.get(1)) {
        let flip = first.a.distance(next.a).min(first.a.distance(next.b))
            < first.b.distance(next.a).min(first.b.distance(next.b));
        if flip {
            if let Some(first) = pieces.first_mut() {
                first.reverse();
            }
        }
    }
    let mut end = None;
    for (i, p) in pieces.iter_mut().enumerate() {
        if let Some((b, tangent)) = end {
            if p.b.distance(b) < p.a.distance(b) {
                p.reverse();
            }
            if p.a.distance(b) > 1e-6 {
                return Err(Refusal::Connection(i));
            }
            if p.t0.dot(tangent) < (1e-6_f64).cos() {
                return Err(Refusal::Tangency(i));
            }
        }
        end = Some((p.b, p.t1));
    }
    if pieces
        .first()
        .zip(pieces.last())
        .is_some_and(|(a, b)| a.a.distance(b.b) <= 1e-6)
    {
        return Err(Refusal::Closed);
    }
    Ok(())
}

/// Stock section axes: y as near world up as possible, a vertical member's x along world X.
#[must_use]
pub fn member_axes(d: Vec3, angle: f64) -> Option<(Vec3, Vec3)> {
    if !angle.is_finite() {
        return None;
    }
    let d = unit(d)?;
    let up = Vec3::Z - d * Vec3::Z.dot(d);
    let (x, y) = match unit(up).filter(|_| up.length() > VERTICAL_TOLERANCE) {
        Some(y) => (y.cross(d), y),
        None => (Vec3::X, d.cross(Vec3::X)),
    };
    let (c, s) = (angle.cos(), angle.sin());
    Some((x * c + y * s, y * c - x * s))
}

/// The starting section plane, re-orthonormalised as the document's plane constructor was.
///
/// # Errors
/// [`Refusal::Frame`] for non-finite or degenerate input.
pub fn section_plane(first: &Piece, at: Point2, turn: f64) -> Result<Frame, Refusal> {
    let (x, y) = member_axes(first.t0, turn).ok_or(Refusal::Frame)?;
    let origin = first.a - x * at.x - y * at.y;
    let x = unit(x).ok_or(Refusal::Frame)?;
    let n = unit(x.cross(y)).ok_or(Refusal::Frame)?;
    let y = unit(n.cross(x)).ok_or(Refusal::Frame)?;
    if !finite(origin) {
        return Err(Refusal::Frame);
    }
    Ok(Frame {
        origin: origin.to_array(),
        x_axis: x.to_array(),
        y_axis: y.to_array(),
    })
}

/// Both terminal section planes. The same rigid arc rotations as the exact sweep carry
/// the axes; the final frame is squared to the final tangent to remove accumulated rounding.
/// Both normals point along travel. Each origin is the endpoint minus the transported anchor
/// offset; use a zero anchor for planes centred on the path endpoints.
///
/// # Errors
/// [`Refusal::Count`] for an empty path or [`Refusal::Frame`] for a degenerate frame.
pub fn terminal_planes(path: &[Piece], at: Point2, turn: f64) -> Result<[Frame; 2], Refusal> {
    if path.len() > 1000 {
        return Err(Refusal::Count);
    }
    let first = path.first().ok_or(Refusal::Count)?;
    let last = path.last().ok_or(Refusal::Count)?;
    let start = section_plane(first, at, turn)?;
    let mut x = Vec3::from(start.x_axis);
    let mut y = Vec3::from(start.y_axis);
    for p in path {
        if let Segment::Arc { axis, angle, .. } = p.segment {
            x = rotate(x, axis, angle);
            y = rotate(y, axis, angle);
        }
    }
    let axis = unit(last.t1).ok_or(Refusal::Frame)?;
    x = unit(x - axis * x.dot(axis)).ok_or(Refusal::Frame)?;
    // Retain the transported handedness, with the terminal normal along travel.
    let up = unit(axis.cross(x)).ok_or(Refusal::Frame)?;
    if up.dot(y) <= 0.0 {
        return Err(Refusal::Frame);
    }
    y = up;
    let origin = last.b - x * at.x - y * at.y;
    if !finite(origin) {
        return Err(Refusal::Frame);
    }
    let end = Frame {
        origin: origin.to_array(),
        x_axis: x.to_array(),
        y_axis: y.to_array(),
    };
    Ok([start, end])
}

fn section_inward(region: &Region2, at: Point2, dx: Ivl, dy: Ivl) -> Result<f64, Refusal> {
    let mut lo = f64::INFINITY;
    let project = |p: Point2| {
        (Ivl::from(p.x) - Ivl::from(at.x)) * dx + (Ivl::from(p.y) - Ivl::from(at.y)) * dy
    };
    let extend = |p: Point2| {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(Refusal::Profile);
        }
        let bound = project(p);
        if !bound.is_finite() {
            return Err(Refusal::Profile);
        }
        Ok(bound.lo)
    };
    for seg in &region.outer.segs {
        match *seg {
            Seg2::Line { a, b } => {
                lo = lo.min(extend(a)?).min(extend(b)?);
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                if !radius.is_finite() || !start.is_finite() || !sweep.is_finite() {
                    return Err(Refusal::Profile);
                }
                // The supporting circle's directional minimum is c.d - r|d|.
                // This encloses every angle of an arc, is tight for a full circle,
                // and does not grow when the section rotates as a box would.
                extend(center)?;
                let bound = project(center) - Ivl::from(radius.abs()) * (dx * dx + dy * dy).sqrt();
                if !bound.is_finite() {
                    return Err(Refusal::Profile);
                }
                lo = lo.min(bound.lo);
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                for p in [p0, p1, p2, p3] {
                    lo = lo.min(extend(p)?);
                }
            }
            Seg2::Conic { a, apex, b, w } if w.is_finite() && w > 0.0 => {
                for p in [a, apex, b] {
                    lo = lo.min(extend(p)?);
                }
            }
            Seg2::Bezier { n, ref p } => {
                for &p in p.get(..=usize::from(n)).ok_or(Refusal::Profile)? {
                    lo = lo.min(extend(p)?);
                }
            }
            Seg2::Conic { .. } => return Err(Refusal::Profile),
        }
    }
    if !lo.is_finite() {
        return Err(Refusal::Profile);
    }
    Ok(lo.min(0.0))
}

/// Prove the section stays clear of every bend axis using circular support bounds and
/// projected control hulls, transporting its axes through preceding arcs. No display samples.
///
/// # Errors
/// [`Refusal::Count`], [`Refusal::Frame`], [`Refusal::Profile`] or [`Refusal::Bend`].
pub fn check_bend_clearance(
    path: &[Piece],
    region: &Region2,
    at: Point2,
    turn: f64,
) -> Result<(), Refusal> {
    if path.len() > 1000 {
        return Err(Refusal::Count);
    }
    let first = path.first().ok_or(Refusal::Count)?;
    if !at.x.is_finite() || !at.y.is_finite() {
        return Err(Refusal::Frame);
    }
    let (mut x, mut y) = member_axes(first.t0, turn).ok_or(Refusal::Frame)?;
    // Validate the section even for a path without bends.
    section_inward(region, at, Ivl::from(1.0), Ivl::from(0.0))?;
    for (i, p) in path.iter().enumerate() {
        if let Segment::Arc {
            centre,
            axis,
            angle,
            ..
        } = p.segment
        {
            let radial = unit(p.a - centre).ok_or(Refusal::Arc)?;
            let dot = |v: Vec3| {
                Ivl::from(v.x) * Ivl::from(radial.x)
                    + Ivl::from(v.y) * Ivl::from(radial.y)
                    + Ivl::from(v.z) * Ivl::from(radial.z)
            };
            let inward = section_inward(region, at, dot(x), dot(y))?;
            if p.a.distance(centre) + inward <= MIN_LENGTH + 2e-3 {
                return Err(Refusal::Bend(i));
            }
            x = rotate(x, axis, angle);
            y = rotate(y, axis, angle);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::prism::profile::Loop2;
    use std::f64::consts::FRAC_PI_2;

    fn compound_path() -> Vec<Piece> {
        vec![
            Piece::line(Vec3::new(100.0, -200.0, 0.0), Vec3::new(100.0, 0.0, 0.0)).unwrap(),
            Piece::arc(
                Vec3::new(100.0, 0.0, 0.0),
                Vec3::new(0.0, 100.0, 0.0),
                Vec3::ZERO,
                Vec3::Z,
                FRAC_PI_2,
            )
            .unwrap(),
            Piece::arc(
                Vec3::new(0.0, 100.0, 0.0),
                Vec3::new(-100.0, 100.0, 100.0),
                Vec3::new(0.0, 100.0, 100.0),
                Vec3::Y,
                FRAC_PI_2,
            )
            .unwrap(),
            Piece::line(
                Vec3::new(-100.0, 100.0, 100.0),
                Vec3::new(-100.0, 100.0, 300.0),
            )
            .unwrap(),
        ]
    }

    fn near(actual: Vec3, expected: Vec3) {
        assert!(
            actual.distance(expected) < 1e-10,
            "{actual:?} vs {expected:?}"
        );
    }

    #[test]
    fn a_pieces_own_fields_make_it_again_and_others_are_refused() {
        let mut pieces = compound_path();
        let mut reversed = pieces.clone();
        for p in &mut reversed {
            p.reverse();
        }
        pieces.extend(reversed);
        for p in pieces {
            assert_eq!(Piece::from_parts(p.a, p.b, p.t0, p.t1, p.segment), Ok(p));
        }
        let line = Piece::line(Vec3::ZERO, Vec3::X).unwrap();
        let parts = |a, b, t0, t1| Piece::from_parts(a, b, t0, t1, line.segment);
        assert_eq!(parts(Vec3::X, line.b, line.t0, line.t1), Err(Refusal::Line));
        assert_eq!(parts(line.a, Vec3::Y, line.t0, line.t1), Err(Refusal::Line));
        assert_eq!(parts(line.a, line.b, Vec3::Y, line.t1), Err(Refusal::Line));
        let nan = Vec3::new(f64::NAN, 0.0, 0.0);
        assert_eq!(parts(line.a, line.b, line.t0, nan), Err(Refusal::Line));
        let arc = compound_path()[1];
        let Segment::Arc {
            start,
            centre,
            axis,
            ..
        } = arc.segment
        else {
            panic!("an arc")
        };
        let turned = Segment::Arc {
            start,
            centre,
            axis,
            angle: 1.0,
        };
        assert_eq!(
            Piece::from_parts(arc.a, arc.b, arc.t0, arc.t1, turned),
            Err(Refusal::Arc)
        );
        assert_eq!(
            Piece::from_parts(arc.a, arc.b, arc.t1, arc.t1, arc.segment),
            Err(Refusal::Arc)
        );
        // an arc's axis is the unit axis the constructor keeps: twice it is refused
        let doubled = Segment::Arc {
            start,
            centre,
            axis: axis * 2.0,
            angle: FRAC_PI_2,
        };
        assert_eq!(
            Piece::from_parts(arc.a, arc.b, arc.t0, arc.t1, doubled),
            Err(Refusal::Arc)
        );
        // a tangent off its unit length by more than the tolerance is refused, so a piece taken
        // back always passes orient's tangency test
        let short = line.t1 * (1.0 - 5e-10);
        assert_eq!(parts(line.a, line.b, line.t0, short), Err(Refusal::Line));
    }

    #[test]
    fn traversal_reverses_drawn_curves_but_does_not_sort_picks() {
        let expected = compound_path();
        let mut path = expected.clone();
        path[0].reverse();
        path[2].reverse();
        orient(&mut path).unwrap();
        assert_eq!(path, expected);
        path.swap(1, 2);
        assert_eq!(orient(&mut path), Err(Refusal::Connection(1)));

        let mut corner = [
            Piece::line(Vec3::ZERO, Vec3::X).unwrap(),
            Piece::line(Vec3::X, Vec3::X + Vec3::Y).unwrap(),
        ];
        assert_eq!(orient(&mut corner), Err(Refusal::Tangency(1)));
    }

    #[test]
    fn compound_terminal_frames_carry_the_section_and_its_anchor() {
        let path = compound_path();
        let [start, end] = terminal_planes(&path, Vec2::new(3.0, 7.0), 0.0).unwrap();
        near(Vec3::from(start.origin), Vec3::new(103.0, -200.0, -7.0));
        near(Vec3::from(start.x_axis), -Vec3::X);
        near(Vec3::from(start.y_axis), Vec3::Z);
        near(Vec3::from(end.origin), Vec3::new(-107.0, 103.0, 300.0));
        near(Vec3::from(end.x_axis), -Vec3::Y);
        near(Vec3::from(end.y_axis), Vec3::X);
        near(
            Vec3::from(start.x_axis).cross(Vec3::from(start.y_axis)),
            Vec3::Y,
        );
        near(
            Vec3::from(end.x_axis).cross(Vec3::from(end.y_axis)),
            Vec3::Z,
        );

        let [start, end] = terminal_planes(&path, Vec2::ZERO, 0.0).unwrap();
        near(Vec3::from(start.origin), path[0].a);
        near(Vec3::from(end.origin), path[3].b);
    }

    #[test]
    fn reversing_a_compound_path_keeps_its_endpoints_and_terminal_normals() {
        let mut path = compound_path();
        path.reverse();
        orient(&mut path).unwrap();
        let [start, end] = terminal_planes(&path, Vec2::ZERO, 0.0).unwrap();
        near(Vec3::from(start.origin), Vec3::new(-100.0, 100.0, 300.0));
        near(Vec3::from(end.origin), Vec3::new(100.0, -200.0, 0.0));
        near(
            Vec3::from(start.x_axis).cross(Vec3::from(start.y_axis)),
            -Vec3::Z,
        );
        near(
            Vec3::from(end.x_axis).cross(Vec3::from(end.y_axis)),
            -Vec3::Y,
        );
    }

    fn rectangle(half_width: f64) -> Region2 {
        let a = Vec2::new(-half_width, -2.0);
        let b = Vec2::new(half_width, -2.0);
        let c = Vec2::new(half_width, 2.0);
        let d = Vec2::new(-half_width, 2.0);
        Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Line { a, b },
                    Seg2::Line { a: b, b: c },
                    Seg2::Line { a: c, b: d },
                    Seg2::Line { a: d, b: a },
                ],
            },
            holes: vec![],
        }
    }

    #[test]
    fn bend_clearance_accounts_for_the_anchored_section_enclosure() {
        let path = compound_path();
        check_bend_clearance(&path, &rectangle(10.0), Vec2::ZERO, 0.0).unwrap();
        assert_eq!(
            check_bend_clearance(&path, &rectangle(101.0), Vec2::ZERO, 0.0),
            Err(Refusal::Bend(1))
        );
        assert_eq!(
            check_bend_clearance(&path, &rectangle(10.0), Vec2::new(-91.0, 0.0), 0.0),
            Err(Refusal::Bend(1))
        );
    }

    #[test]
    fn arc_endpoint_agreement_uses_the_absolute_join_gap_at_large_radii() {
        let radius = 1e6;
        let a = Vec3::X * radius;
        let b = Vec3::Y * radius;
        // A relative 1e-6 radius check admits this 0.5 mm gap. The following
        // tangent line would meet the stored endpoint but not the swept circle.
        let displaced = b + Vec3::Y * 0.5;
        assert_eq!(
            Piece::spatial_arc(a, displaced, Vec3::ZERO, Vec3::Z, radius),
            Err(Refusal::Arc)
        );
        for displaced in [displaced, b + Vec3::X * 0.5] {
            assert_eq!(
                Piece::arc(a, displaced, Vec3::ZERO, Vec3::Z, FRAC_PI_2),
                Err(Refusal::Arc)
            );
        }

        // The exact quarter circle still connects to its tangent line. Its
        // terminal plane and reversed traversal keep the analytic endpoints.
        let arc = Piece::spatial_arc(a, b, Vec3::ZERO, Vec3::Z, radius).unwrap();
        let mut path = [arc, Piece::line(b, b - Vec3::X * 100.0).unwrap()];
        orient(&mut path).unwrap();
        let [_, end] = terminal_planes(&[arc], Vec2::ZERO, 0.0).unwrap();
        near(Vec3::from(end.origin), b);
        near(
            Vec3::from(end.x_axis).cross(Vec3::from(end.y_axis)),
            -Vec3::X,
        );
        let mut reversed = arc;
        reversed.reverse();
        let [_, end] = terminal_planes(&[reversed], Vec2::ZERO, 0.0).unwrap();
        near(Vec3::from(end.origin), a);
        near(
            Vec3::from(end.x_axis).cross(Vec3::from(end.y_axis)),
            -Vec3::Y,
        );
    }

    #[test]
    fn round_section_clearance_is_independent_of_section_rotation() {
        let path = [compound_path()[1]];
        let circle = |radius| Region2 {
            outer: Loop2 {
                segs: vec![Seg2::Arc {
                    center: Vec2::ZERO,
                    radius,
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                }],
            },
            holes: vec![],
        };
        // The true inward reach is 80 for every rotation: the bend clears
        // its axis by 100 - 80 = 20 mm. A rotated box reaches 80 sqrt(2).
        for turn in [0.0, std::f64::consts::FRAC_PI_4, FRAC_PI_2] {
            assert_eq!(
                check_bend_clearance(&path, &circle(80.0), Vec2::ZERO, turn),
                Ok(())
            );
            // Actual axis contact remains a refusal, including after rotation.
            assert_eq!(
                check_bend_clearance(&path, &circle(100.0), Vec2::ZERO, turn),
                Err(Refusal::Bend(0))
            );
        }
    }

    #[test]
    fn nonfinite_circle_and_retained_line_data_are_refused() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                Piece::spatial_arc(Vec3::X, Vec3::Y, Vec3::ZERO, Vec3::Z, value),
                Err(Refusal::Arc)
            );
            assert_eq!(
                Piece::arc(Vec3::X, Vec3::Y, Vec3::ZERO, Vec3::Z, value),
                Err(Refusal::Arc)
            );
            assert_eq!(
                Piece::retained_line(Vec3::ZERO, Vec3::X, [0.0, value]),
                Err(Refusal::Line)
            );
            assert_eq!(
                check_bend_clearance(
                    &compound_path(),
                    &rectangle(10.0),
                    Vec2::new(value, 0.0),
                    0.0
                ),
                Err(Refusal::Frame)
            );
        }
        assert_eq!(
            Piece::spatial_arc(Vec3::X, Vec3::Y, Vec3::ZERO, Vec3::Z * 2.0, 1.0),
            Err(Refusal::Arc)
        );
    }
}
