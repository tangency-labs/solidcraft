//! Figures on a drawing sheet: a figure placed at its size and turn, a regular outline's corners,
//! and the tallest label box inside a convex outline. Drawings place a symbol, draw a revision
//! tag's outline and fit a label's text with these, so the drawing code computes no geometry.
//!
//! Not upstream: SolidCraft's drawing sheets (docs/layers.md §1). The limits are a profile's own
//! ([`MAX_SEGMENTS`], [`MAX_PROFILE_COORD`]), so a figure is a profile [`check_regions`](super::profile::check_regions) accepts.
//!
//! Deviations: none (no upstream counterpart).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use std::f64::consts::{PI, TAU};

use super::profile::{
    bezier_points, cross2, finite2, gamma, unit, within_profile, Seg2, MAX_PROFILE_COORD,
    MAX_SEGMENTS,
};
use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::math::{Point2, Transform, Vec2};

const OP_FIGURE: &str = "prism::figure::place_figure_2d";
const OP_POLYGON: &str = "prism::figure::regular_polygon_2d";
const OP_FIT: &str = "prism::figure::box_fit_in_convex_polygon";

/// Where a figure goes on its sheet ([`place_figure_2d`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FigurePlacement {
    /// Its size: the figure's units are `scale` long on the sheet.
    pub scale: f64,
    /// Its turn about its own origin, counter-clockwise, in radians.
    pub angle: f64,
    /// Where the figure's own origin goes.
    pub origin: Point2,
}

/// A figure placed on its sheet ([`place_figure_2d`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedFigure {
    /// The figure's segments, in its order.
    pub segments: Vec<Seg2>,
    /// The points asked for (a text's anchor, say), in their order.
    pub points: Vec<Point2>,
    /// The unit direction the figure's `+x` now points along (a text's baseline).
    pub direction: Vec2,
}

/// `figure` and `points` scaled by `placement.scale`, turned by `placement.angle` about their own
/// origin and moved to `placement.origin`: each point `p` goes to `origin + scale R p`, where `R`
/// is the turn (counter-clockwise about `+z`). An arc's centre moves, its radius scales, its start
/// turns by the angle and its sweep stays; a conic keeps its weight. A drawing symbol is a table
/// of segments and text anchors in units of its size, placed by this one call.
///
/// A mirror is out of scope: a negative scale would also turn each arc's sweep, so it is refused.
///
/// # Errors
///
/// `InvalidInput` for a scale not above 0 or not finite, an angle or an origin not finite, more
/// than [`MAX_SEGMENTS`] segments or points, a Bézier degree outside `1 ..=`
/// [`MAX_BEZIER_DEGREE`](super::profile::MAX_BEZIER_DEGREE), or a segment or point not finite before or after it is placed.
pub fn place_figure_2d(
    figure: &[Seg2],
    points: &[Point2],
    placement: FigurePlacement,
) -> Result<PlacedFigure> {
    let bad = |reason| SolidError::invalid_input(OP_FIGURE, reason);
    let FigurePlacement {
        scale,
        angle,
        origin,
    } = placement;
    if !(scale > 0.0 && scale.is_finite() && angle.is_finite() && finite2(origin)) {
        return Err(bad(
            "a scale not above 0 or not finite, or an angle or an origin not finite",
        ));
    }
    if figure.len() > MAX_SEGMENTS || points.len() > MAX_SEGMENTS {
        return Err(bad("more segments or points than MAX_SEGMENTS"));
    }
    // The turned axes are `x` and `x` turned a quarter more: one rounding per entry, so no turn is
    // exact. The kernel's affine map then scales them and adds the move.
    let x = unit(angle);
    let map = Transform::from_axes(
        Vec3::new(x.x, x.y, 0.0) * scale,
        Vec3::new(-x.y, x.x, 0.0) * scale,
        Vec3::Z,
        Point3::new(origin.x, origin.y, 0.0),
    );
    let at = |p: Point2| Vec2::from_xy(map.transform_point(Point3::new(p.x, p.y, 0.0)));
    let mut placed = Vec::with_capacity(figure.len());
    for seg in figure {
        let moved = match *seg {
            Seg2::Line { a, b } => Seg2::Line { a: at(a), b: at(b) },
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Seg2::Arc {
                center: at(center),
                radius: radius * scale,
                start: start + angle,
                sweep,
            },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
                p0: at(p0),
                p1: at(p1),
                p2: at(p2),
                p3: at(p3),
            },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                a: at(a),
                apex: at(apex),
                b: at(b),
                w,
            },
            Seg2::Bezier { n, mut p } => {
                let used = bezier_points(n, &p)
                    .ok_or_else(|| bad("a Bézier degree out of range"))?
                    .len();
                for q in p.iter_mut().take(used) {
                    *q = at(*q);
                }
                Seg2::Bezier { n, p }
            }
        };
        if !seg_finite(&moved) {
            return Err(bad("a segment not finite"));
        }
        placed.push(moved);
    }
    let points = points.iter().map(|&p| at(p)).collect::<Vec<_>>();
    if !points.iter().copied().all(finite2) {
        return Err(bad("a point not finite"));
    }
    Ok(PlacedFigure {
        segments: placed,
        points,
        direction: x,
    })
}

/// Every number `seg` uses is finite (a Bézier's tail past its degree is not used).
fn seg_finite(seg: &Seg2) -> bool {
    match *seg {
        Seg2::Line { a, b } => finite2(a) && finite2(b),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => finite2(center) && radius.is_finite() && start.is_finite() && sweep.is_finite(),
        Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].into_iter().all(finite2),
        Seg2::Conic { a, apex, b, w } => finite2(a) && finite2(apex) && finite2(b) && w.is_finite(),
        Seg2::Bezier { n, p } => {
            bezier_points(n, &p).is_some_and(|q| q.iter().copied().all(finite2))
        }
    }
}

/// The corners of the regular polygon with `sides` sides, each `side` long, about `centre`:
/// counter-clockwise, the first at the angle `first_angle` from `+x` (a triangle point up has
/// `first_angle = π/2`). The circumradius is `side / (2 sin(π / sides))`, and the centroid is
/// `centre`.
///
/// # Errors
///
/// `InvalidInput` for fewer than 3 sides or more than [`MAX_SEGMENTS`], a side not above 0, a
/// centre, side or angle not finite, or a corner that comes out not finite or beyond
/// [`MAX_PROFILE_COORD`] (the coordinates [`check_regions`](super::profile::check_regions) allows).
pub fn regular_polygon_2d(
    centre: Point2,
    sides: usize,
    side: f64,
    first_angle: f64,
) -> Result<Vec<Point2>> {
    let bad = |reason| SolidError::invalid_input(OP_POLYGON, reason);
    if !(3..=MAX_SEGMENTS).contains(&sides) {
        return Err(bad("fewer than 3 sides or more than MAX_SEGMENTS"));
    }
    if !(side > 0.0 && side.is_finite() && finite2(centre) && first_angle.is_finite()) {
        return Err(bad("a side not above 0, or a value not finite"));
    }
    let n = sides as f64;
    let radius = side / (2.0 * (PI / n).sin());
    let corners: Vec<Point2> = (0..sides)
        .map(|i| centre + unit(first_angle + TAU * i as f64 / n) * radius)
        .collect();
    if corners.iter().copied().all(within_profile) {
        Ok(corners)
    } else {
        Err(bad("a corner not finite or beyond MAX_PROFILE_COORD"))
    }
}

/// A box fitted inside a convex outline ([`box_fit_in_convex_polygon`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxFit {
    /// Its height (its width is the height times the aspect asked for).
    pub height: f64,
    /// Its lower-left corner.
    pub min: Point2,
}

/// The tallest box `width_per_height` times as wide as it is tall, centred across on the line
/// `x = centre_x`, kept `margin` clear of every edge of `polygon` (measured square to the edge)
/// and no taller than `max_height`; of the boxes of that height, the one with the lowest bottom.
/// `None` when no box fits, not even one of no height.
///
/// `polygon` is convex with its corners counter-clockwise. A box of height `h` with its bottom at
/// `y0` has its corners at `(centre_x ± width_per_height h / 2, y0)` and `(.., y0 + h)`. An edge
/// from `P` with inward unit normal `n` holds a corner `c` when `n · (c - P) ≥ margin`, and the
/// corner nearest the edge gives one condition, linear in `h` and `y0`:
/// `A h + B y0 ≥ C`, with `A = min(n.y, 0) - |n.x| width_per_height / 2`, `B = n.y` and
/// `C = margin + n · P - n.x centre_x`. As `A ≤ 0`, a box that fits still fits made shorter on the
/// same bottom, so the heights that fit run from 0 to the answer. Upright edges (`B = 0`) bound
/// the height alone; the others bound `y0` from below (`B > 0`) or above (`B < 0`) by lines in
/// `h`. From the tallest height allowed, each step goes down to where the highest lower line
/// meets the lowest upper line (the gap between the bounds is concave in `h`, so each step lands
/// on the next piece of it), until the lowest bottom meets every condition. The conditions are
/// distances, so they are met within a rounding allowance of `gamma(16)` times the problem's size
/// (Higham §3.1).
///
/// # Errors
///
/// `InvalidInput` for fewer than 3 corners or more than [`MAX_SEGMENTS`], an edge of no length,
/// an outline not convex and counter-clockwise, a negative aspect, margin or height, a value not
/// finite or beyond [`MAX_PROFILE_COORD`] (the coordinates [`check_regions`](super::profile::check_regions) allows), or a bound
/// that overflows (an edge too near upright to compare). `SM_ERR` if the steps do not settle
/// within one per edge (rounding only).
pub fn box_fit_in_convex_polygon(
    polygon: &[Point2],
    centre_x: f64,
    width_per_height: f64,
    margin: f64,
    max_height: f64,
) -> Result<Option<BoxFit>> {
    let bad = |reason| SolidError::invalid_input(OP_FIT, reason);
    if !(3..=MAX_SEGMENTS).contains(&polygon.len()) {
        return Err(bad("fewer than 3 corners or more than MAX_SEGMENTS"));
    }
    // Within the coordinates a profile may have (`check_regions`), so no product overflows.
    let fine = |v: f64| v.abs() < MAX_PROFILE_COORD;
    let values = [centre_x, width_per_height, margin, max_height];
    if !polygon.iter().all(|p| fine(p.x) && fine(p.y)) || !values.into_iter().all(fine) {
        return Err(bad("a value not finite or beyond MAX_PROFILE_COORD"));
    }
    if width_per_height < 0.0 || margin < 0.0 || max_height < 0.0 {
        return Err(bad("a negative aspect, margin or height"));
    }
    // Each edge's start and unit direction.
    let mut edges = Vec::with_capacity(polygon.len());
    for (p, q) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
        let d = (*q - *p)
            .unitize()
            .map_err(|_| bad("an edge of no length"))?;
        edges.push((*p, d));
    }
    // Convex and counter-clockwise: no turn to the right, and one turn in all.
    let mut turned = 0.0;
    for ((_, d), (_, e)) in edges.iter().zip(edges.iter().cycle().skip(1)) {
        let c = cross2(*d, *e);
        if c < 0.0 {
            return Err(bad("an outline not convex and counter-clockwise"));
        }
        turned += c.atan2(d.dot(*e));
    }
    if (turned - TAU).abs() > PI {
        return Err(bad("an outline not convex and counter-clockwise"));
    }
    let half = width_per_height / 2.0;
    let holds: Vec<Hold> = edges
        .iter()
        .map(|(p, d)| {
            let n = Vec2::new(-d.y, d.x);
            Hold {
                a: n.y.min(0.0) - n.x.abs() * half,
                b: n.y,
                c: margin + n.dot(*p) - n.x * centre_x,
            }
        })
        .collect();
    let reach = polygon
        .iter()
        .fold(0.0_f64, |m, p| m.max(p.x.abs()).max(p.y.abs()));
    let size = reach + centre_x.abs() + margin + max_height * (1.0 + width_per_height);
    let allowance = gamma(16) * size;
    // Upright edges bound the height alone; one the centre line is too near fits nothing.
    let mut h = max_height;
    for hold in holds.iter().filter(|e| e.b == 0.0) {
        if hold.c > allowance {
            return Ok(None);
        }
        if hold.a < 0.0 {
            h = h.min((hold.c / hold.a).max(0.0));
        }
    }
    let lowest = |h: f64| {
        let below = holds.iter().filter(|e| e.b > 0.0);
        below
            .map(|e| (e.bottom(h), *e))
            .max_by(|x, y| x.0.total_cmp(&y.0))
    };
    let highest = |h: f64| {
        let above = holds.iter().filter(|e| e.b < 0.0);
        above
            .map(|e| (e.bottom(h), *e))
            .min_by(|x, y| x.0.total_cmp(&y.0))
    };
    for _ in 0..=holds.len() {
        let (Some((y, low)), Some((_, high))) = (lowest(h), highest(h)) else {
            return Err(bad("an outline open above or below"));
        };
        if !y.is_finite() {
            return Err(bad("an edge too near upright to compare"));
        }
        if holds.iter().all(|e| e.inside(h, y) >= -allowance) {
            let min = Vec2::new(centre_x - half * h, y);
            return Ok(Some(BoxFit { height: h, min }));
        }
        if !(h > 0.0) {
            return Ok(None);
        }
        // At `h` the lowest bottom the edges below allow is above the highest one the edges above
        // allow. The gap between those bounds is concave in `h`, so no box taller than the height
        // where the two edges that set them meet fits: step down to it. If they meet at or above
        // `h`, or never, the gap stays shut all the way down and no box fits.
        let lower = (low.c * high.b - high.c * low.b) / (low.a * high.b - high.a * low.b);
        if !(lower < h) {
            return Ok(None);
        }
        h = lower.max(0.0);
    }
    Err(SolidError::sm_err(OP_FIT))
}

/// One edge's condition on a box of height `h` with its bottom at `y0`: `a h + b y0 ≥ c`
/// ([`box_fit_in_convex_polygon`]).
#[derive(Clone, Copy, Debug)]
struct Hold {
    a: f64,
    b: f64,
    c: f64,
}

impl Hold {
    /// The bottom at which a box of height `h` meets this condition exactly (`b` not 0).
    fn bottom(self, h: f64) -> f64 {
        (self.c - self.a * h) / self.b
    }

    /// How far the box of height `h` with its bottom at `y0` is inside this condition: its corner
    /// nearest the edge, measured square to the edge from the margin line.
    fn inside(self, h: f64, y0: f64) -> f64 {
        self.a * h + self.b * y0 - self.c
    }
}

#[cfg(test)]
mod tests;
