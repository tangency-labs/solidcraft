//! The spacing of a set of round items that share one axis direction (not upstream, #10170):
//! holes along a row, or round a bolt circle. Each item is the centre and the axis of a circle
//! or a cylinder. The centres are projected onto the plane through the first centre square to
//! its axis, and the pattern is read there: on one line (ordered along it), on one circle
//! (ordered round it), or neither (kept in the given order). The gaps are the centre-to-centre
//! distances of neighbours in that order.
//!
//! The circle through three points is their circumcircle (Euclid, Elements IV.5), taken in the
//! plane's own frame with one point at the origin and one on the x axis.
//!
//! Coordinates are finite within ±1e9; a call takes 2..=[`MAX_ITEMS`] items. Distances are
//! compared to the kernel's zone tolerance for a model of the pattern's size
//! ([`size_estimate_to_zone_tol_3d`]).

use std::f64::consts::TAU;

use crate::error::{Result, SolidError};
use crate::geom::tol::size_estimate_to_zone_tol_3d;
use crate::geom::vector3d::Vec3;

/// The most items one call takes.
pub const MAX_ITEMS: usize = 64;
const LIMIT: f64 = 1.0e9;
/// How far from parallel two unit axes may be: the length of their cross product.
const PARALLEL: f64 = 1.0e-6;

/// What the projected centres lie on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// One straight line.
    Line,
    /// One circle (a bolt circle), and not one line.
    Circle,
    /// Neither.
    None,
}

/// The circle a [`Pattern::Circle`] lies on.
#[derive(Clone, Debug, PartialEq)]
pub struct PitchCircle {
    /// Its diameter (the pitch-circle diameter).
    pub diameter: f64,
    /// Its centre, in the plane of the projected centres.
    pub centre: [f64; 3],
    /// The angle at the centre between neighbours, in degrees, one for each gap.
    pub angles_deg: Vec<f64>,
    /// Where the gaps are even: the angle between neighbours, a whole turn shared among them.
    pub angle_pitch_deg: Option<f64>,
}

/// The spacing of the items, see [`spacing`].
#[derive(Clone, Debug, PartialEq)]
pub struct Spacing {
    /// What the projected centres lie on.
    pub pattern: Pattern,
    /// The items' indices in order along the line or round the circle; the given order where
    /// the pattern is [`Pattern::None`].
    pub order: Vec<usize>,
    /// The centre-to-centre distance of each neighbouring pair in `order`. A circle has one
    /// more, from the last item back to the first.
    pub gaps: Vec<f64>,
    /// The two projected centres each gap lies between.
    pub gap_points: Vec<[[f64; 3]; 2]>,
    /// The distance from the first centre in `order` to the last.
    pub span: f64,
    /// Whether every gap is the same, to within the zone tolerance.
    pub even: bool,
    /// The gaps' mean where they are even; otherwise the gap most of the others equal (the
    /// smallest such where several tie), and none where no two gaps are the same.
    pub pitch: Option<f64>,
    /// The gaps (indices into `gaps`) that differ from `pitch`.
    pub odd_gaps: Vec<usize>,
    /// The circle of a [`Pattern::Circle`].
    pub circle: Option<PitchCircle>,
}

fn refused(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "geom::hole_spacing::spacing",
        reason,
    }
}

fn valid(p: [f64; 3]) -> bool {
    p.into_iter().all(|x| x.is_finite() && x.abs() <= LIMIT)
}

/// The two points farthest apart, as indices into `points` (the lower first), and how far.
fn farthest(points: &[Vec3]) -> Option<(usize, usize, f64)> {
    let mut best: Option<(usize, usize, f64)> = None;
    for (i, a) in points.iter().enumerate() {
        for (j, b) in points.iter().enumerate().skip(i + 1) {
            let d = a.distance(*b);
            if best.is_none_or(|(_, _, far)| d > far) {
                best = Some((i, j, d));
            }
        }
    }
    best
}

/// The pitch of `gaps` and the gaps that differ from it, see [`Spacing::pitch`].
fn pitch_of(gaps: &[f64], tol: f64) -> (bool, Option<f64>, Vec<usize>) {
    let (lo, hi) = gaps
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), g| {
            (lo.min(*g), hi.max(*g))
        });
    if hi - lo <= tol {
        let mean = gaps.iter().sum::<f64>() / gaps.len().max(1) as f64;
        return (true, Some(mean), Vec::new());
    }
    // The gap with the most others like it; among equals the smallest.
    let mut best: Option<(usize, f64)> = None;
    for g in gaps {
        let like = gaps.iter().filter(|o| (*o - g).abs() <= tol).count();
        let better = match best {
            None => true,
            Some((n, v)) => like > n || (like == n && *g < v),
        };
        if better {
            best = Some((like, *g));
        }
    }
    match best {
        Some((like, pitch)) if like >= 2 => {
            let odd = gaps
                .iter()
                .enumerate()
                .filter(|(_, g)| (*g - pitch).abs() > tol)
                .map(|(i, _)| i)
                .collect();
            (false, Some(pitch), odd)
        }
        _ => (false, None, Vec::new()),
    }
}

/// The gaps of `points` taken in `order`, with the closing gap of a circle when `closed`.
fn gaps_of(points: &[Vec3], order: &[usize], closed: bool) -> (Vec<f64>, Vec<[[f64; 3]; 2]>) {
    let ordered: Vec<Vec3> = order
        .iter()
        .filter_map(|i| points.get(*i).copied())
        .collect();
    let mut pairs: Vec<(Vec3, Vec3)> = ordered
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .collect();
    if let (true, Some(first), Some(last)) = (closed, ordered.first(), ordered.last()) {
        pairs.push((*last, *first));
    }
    (
        pairs.iter().map(|(a, b)| a.distance(*b)).collect(),
        pairs
            .iter()
            .map(|(a, b)| [a.to_array(), b.to_array()])
            .collect(),
    )
}

/// The spacing of round items, each its centre and its axis direction (any length but zero).
/// The axes must be parallel (either way round) and no two centres may project to one point.
///
/// # Errors
/// [`SolidError::InvalidInput`] for fewer than two or more than [`MAX_ITEMS`] items, a value
/// that is not finite or out of range, a zero axis, axes that are not parallel, and two items
/// that share one centre.
pub fn spacing(items: &[([f64; 3], [f64; 3])]) -> Result<Spacing> {
    if items.len() < 2 || items.len() > MAX_ITEMS {
        return Err(refused("the spacing takes 2 to 64 round items"));
    }
    if items.iter().any(|(c, a)| !valid(*c) || !valid(*a)) {
        return Err(refused(
            "a centre or an axis is not finite or is out of range",
        ));
    }
    let mut axes = Vec::new();
    for (_, a) in items {
        // Any length but zero has a direction, however short.
        let (_, unit) = Vec3::from_array(*a)
            .length_and_unit()
            .ok_or_else(|| refused("an axis has no direction"))?;
        axes.push(unit);
    }
    let (Some(normal), Some((origin, _))) = (axes.first().copied(), items.first()) else {
        return Err(refused("the spacing takes 2 to 64 round items"));
    };
    if axes.iter().any(|a| a.cross(normal).length() > PARALLEL) {
        return Err(refused(
            "the round items must share one axis direction to measure their spacing",
        ));
    }
    let origin = Vec3::from_array(*origin);
    let points: Vec<Vec3> = items
        .iter()
        .map(|(c, _)| Vec3::from_array(*c).project_point_to_plane(origin, normal))
        .collect();
    let Some((i, j, size)) = farthest(&points) else {
        return Err(refused("the spacing takes 2 to 64 round items"));
    };
    // Two places are one where the kernel takes them for one in a model of the pattern's size:
    // its zone tolerance (1e-5 for sizes from 0.5 to 500, growing with the square root of the
    // size, not with the size). Centres that close share one centre, and a centre that far from
    // a line or a circle is off it.
    let tol = size_estimate_to_zone_tol_3d(size);
    let shared = points
        .iter()
        .enumerate()
        .any(|(k, a)| points.iter().skip(k + 1).any(|b| a.distance(*b) <= tol));
    if shared {
        return Err(refused("two of the round items share one centre"));
    }
    let (Some(a), Some(b)) = (points.get(i).copied(), points.get(j).copied()) else {
        return Err(refused("the spacing takes 2 to 64 round items"));
    };
    // The plane's frame: `u` along the farthest pair, from the one given first; `v` square to it.
    let u = (b - a) / size;
    let v = normal.cross(u);
    let planar: Vec<(f64, f64)> = points
        .iter()
        .map(|p| ((*p - a).dot(u), (*p - a).dot(v)))
        .collect();
    let off_line = planar
        .iter()
        .enumerate()
        .fold((0usize, 0.0f64), |(at, far), (k, (_, y))| {
            if y.abs() > far.abs() {
                (k, *y)
            } else {
                (at, far)
            }
        });

    let mut order: Vec<usize> = (0..points.len()).collect();
    let mut pattern = Pattern::None;
    let mut centre = None;
    if off_line.1.abs() <= tol {
        pattern = Pattern::Line;
        order.sort_by(|p, q| {
            let along = |k: &usize| planar.get(*k).map_or(0.0, |(x, _)| *x);
            along(p).total_cmp(&along(q))
        });
    } else if let Some((cx, cy)) = planar.get(off_line.0).copied() {
        // The circumcircle of (0, 0), (size, 0) and the point farthest off their line.
        let x0 = 0.5 * size;
        let y0 = (cx * cx + cy * cy - size * cx) / (2.0 * cy);
        let radius = x0.hypot(y0);
        // The circle of centres within the limit can be larger than the limit.
        let on_circle = radius.is_finite()
            && planar
                .iter()
                .all(|(x, y)| ((x - x0).hypot(y - y0) - radius).abs() <= tol);
        if on_circle {
            pattern = Pattern::Circle;
            // Round the axis from the item given first.
            let angle = |k: &usize| planar.get(*k).map_or(0.0, |(x, y)| (y - y0).atan2(x - x0));
            let start = angle(&0);
            let turn = |k: &usize| (angle(k) - start).rem_euclid(TAU);
            order.sort_by(|p, q| turn(p).total_cmp(&turn(q)));
            let angles: Vec<f64> = order.iter().map(turn).collect();
            let mut angles_deg: Vec<f64> = angles
                .windows(2)
                .filter_map(|w| match w {
                    [from, to] => Some((to - from).to_degrees()),
                    _ => None,
                })
                .collect();
            if let Some(last) = angles.last() {
                angles_deg.push((TAU - last).to_degrees());
            }
            centre = Some(PitchCircle {
                diameter: 2.0 * radius,
                centre: (a + u * x0 + v * y0).to_array(),
                angles_deg,
                angle_pitch_deg: None,
            });
        }
    }

    let (gaps, gap_points) = gaps_of(&points, &order, pattern == Pattern::Circle);
    let span = match (
        order.first().and_then(|k| points.get(*k)),
        order.last().and_then(|k| points.get(*k)),
    ) {
        (Some(first), Some(last)) => first.distance(*last),
        _ => 0.0,
    };
    let (even, pitch, odd_gaps) = pitch_of(&gaps, tol);
    if let (true, Some(circle)) = (even, centre.as_mut()) {
        circle.angle_pitch_deg = Some(360.0 / circle.angles_deg.len().max(1) as f64);
    }
    Ok(Spacing {
        pattern,
        order,
        gaps,
        gap_points,
        span,
        even,
        pitch,
        odd_gaps,
        circle: centre,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: [f64; 3] = [0.0, 0.0, 1.0];

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_row_of_five_holes_is_even() {
        // Five M3 holes at 88.75 along a panel edge, the cylinders' middles at varying depths.
        let items: Vec<_> = (0..5)
            .map(|k| ([22.5 + 88.75 * f64::from(k), 20.0, f64::from(k % 2)], Z))
            .collect();
        let s = spacing(&items).unwrap();
        assert_eq!(s.pattern, Pattern::Line);
        assert_eq!(s.order, vec![0, 1, 2, 3, 4]);
        assert_eq!(s.gaps.len(), 4);
        assert!(s.gaps.iter().all(|g| close(*g, 88.75)), "{:?}", s.gaps);
        assert!(close(s.span, 355.0));
        assert!(s.even);
        assert!(close(s.pitch.unwrap(), 88.75));
        assert!(s.odd_gaps.is_empty() && s.circle.is_none());
        // Each gap's ends are in the plane of the first centre.
        assert_eq!(s.gap_points[1], [[111.25, 20.0, 0.0], [200.0, 20.0, 0.0]]);
    }

    #[test]
    fn a_hole_left_out_of_a_row_is_the_odd_gap() {
        let items: Vec<_> = [0.0, 88.75, 266.25, 355.0]
            .iter()
            .map(|x| ([*x, 0.0, 0.0], Z))
            .collect();
        let s = spacing(&items).unwrap();
        assert_eq!(s.pattern, Pattern::Line);
        assert!(!s.even);
        assert!(close(s.pitch.unwrap(), 88.75));
        assert_eq!(s.odd_gaps, vec![1]);
        assert!(close(s.gaps[1], 177.5));
        assert!(close(s.span, 355.0));
    }

    #[test]
    fn picks_in_any_order_are_sorted_along_the_row() {
        // A slanted row, given out of order: it runs from the end item given first.
        let at = |t: f64| ([3.0 * t, 4.0 * t, 7.0], [0.0, 0.0, -2.0]);
        let s = spacing(&[at(2.0), at(3.0), at(0.0), at(1.0)]).unwrap();
        assert_eq!(s.pattern, Pattern::Line);
        assert_eq!(s.order, vec![1, 0, 3, 2]);
        assert!(s.gaps.iter().all(|g| close(*g, 5.0)));
        assert!(s.even && close(s.span, 15.0));
    }

    #[test]
    fn gaps_with_no_two_alike_have_no_pitch() {
        let items: Vec<_> = [0.0, 10.0, 25.0, 45.0]
            .iter()
            .map(|x| ([*x, 0.0, 0.0], Z))
            .collect();
        let s = spacing(&items).unwrap();
        assert!(!s.even && s.pitch.is_none() && s.odd_gaps.is_empty());
    }

    #[test]
    fn a_bolt_circle_gives_its_diameter_and_angles() {
        // Six holes on a 80 mm pitch circle about (10, -5), given out of order.
        let hole = |k: u32| {
            let t = (f64::from(k) * 60.0 + 15.0).to_radians();
            ([10.0 + 40.0 * t.cos(), -5.0 + 40.0 * t.sin(), 3.0], Z)
        };
        let s = spacing(&[hole(0), hole(3), hole(1), hole(5), hole(2), hole(4)]).unwrap();
        assert_eq!(s.pattern, Pattern::Circle);
        // Round the axis from the first: holes 0, 1, 2, 3, 4, 5.
        assert_eq!(s.order, vec![0, 2, 4, 1, 5, 3]);
        let c = s.circle.unwrap();
        assert!(close(c.diameter, 80.0));
        assert!(close(c.centre[0], 10.0) && close(c.centre[1], -5.0) && close(c.centre[2], 3.0));
        assert_eq!(c.angles_deg.len(), 6);
        assert!(
            c.angles_deg.iter().all(|a| close(*a, 60.0)),
            "{:?}",
            c.angles_deg
        );
        // The chord of 60° on radius 40 is 40.
        assert_eq!(s.gaps.len(), 6);
        assert!(s.gaps.iter().all(|g| close(*g, 40.0)));
        assert!(s.even && close(s.pitch.unwrap(), 40.0));
        assert_eq!(c.angle_pitch_deg, Some(60.0));
    }

    #[test]
    fn part_of_a_bolt_circle_has_an_odd_closing_gap() {
        let hole = |deg: f64| {
            let t = deg.to_radians();
            ([40.0 * t.cos(), 40.0 * t.sin(), 0.0], Z)
        };
        let s = spacing(&[hole(0.0), hole(60.0), hole(120.0), hole(180.0)]).unwrap();
        assert_eq!(s.pattern, Pattern::Circle);
        let c = s.circle.unwrap();
        assert!(close(c.angles_deg[3], 180.0) && close(c.diameter, 80.0));
        assert!(!s.even && close(s.pitch.unwrap(), 40.0));
        assert_eq!(s.odd_gaps, vec![3]);
        assert_eq!(c.angle_pitch_deg, None);
    }

    #[test]
    fn near_holes_on_a_long_row_are_not_one_centre() {
        // Holes 1 apart on a row 2 000 000 long: a tolerance that grew with the row's length
        // would take the first two for one hole.
        let items: Vec<_> = [0.0, 1.0, 2.0e6]
            .iter()
            .map(|x| ([*x, 0.0, 0.0], Z))
            .collect();
        let s = spacing(&items).unwrap();
        assert_eq!(s.pattern, Pattern::Line);
        assert!(close(s.gaps[0], 1.0) && close(s.gaps[1], 1_999_999.0) && close(s.span, 2.0e6));
        // The same centre twice is refused whatever the size.
        assert!(spacing(&[([0.0; 3], Z), ([2.0e6, 0.0, 0.0], Z), ([0.0; 3], Z)]).is_err());
    }

    #[test]
    fn a_short_axis_has_a_direction_and_a_circle_may_pass_the_coordinate_limit() {
        let row = |axis: [f64; 3]| {
            spacing(&[
                ([0.0; 3], axis),
                ([5.0, 0.0, 0.0], Z),
                ([10.0, 0.0, 0.0], Z),
            ])
            .unwrap()
        };
        assert_eq!(row([0.0, 0.0, 5.0e-13]), row(Z));
        // Four centres at the limit: their circle's radius is sqrt(2) times it.
        let far = 1.0e9;
        let corners = [[far, far], [-far, far], [-far, -far], [far, -far]];
        let items: Vec<_> = corners.iter().map(|[x, y]| ([*x, *y, 0.0], Z)).collect();
        let s = spacing(&items).unwrap();
        assert_eq!(s.pattern, Pattern::Circle);
        let c = s.circle.unwrap();
        assert!((c.diameter - 2.0 * far * 2f64.sqrt()).abs() < 1e-3);
        assert_eq!((s.even, c.angle_pitch_deg), (true, Some(90.0)));
    }

    #[test]
    fn centres_on_no_line_or_circle_keep_their_order() {
        let items = [
            ([0.0, 0.0, 0.0], Z),
            ([10.0, 0.0, 0.0], Z),
            ([10.0, 10.0, 0.0], Z),
            ([3.0, 4.0, 0.0], Z),
        ];
        let s = spacing(&items).unwrap();
        assert_eq!(s.pattern, Pattern::None);
        assert_eq!(s.order, vec![0, 1, 2, 3]);
        assert_eq!(s.gaps.len(), 3);
        assert!(close(s.span, 5.0) && s.circle.is_none());
    }

    #[test]
    fn coincident_non_parallel_and_hostile_items_are_refused() {
        let reason = |items: &[([f64; 3], [f64; 3])]| match spacing(items) {
            Err(SolidError::InvalidInput { reason, .. }) => reason,
            other => panic!("expected a refusal, got {other:?}"),
        };
        // Two picks on one hole's axis, at different depths.
        let shared = [([0.0; 3], Z), ([5.0, 0.0, 0.0], Z), ([5.0, 0.0, 9.0], Z)];
        assert_eq!(reason(&shared), "two of the round items share one centre");
        let skew = [
            ([0.0; 3], Z),
            ([5.0, 0.0, 0.0], Z),
            ([9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ];
        assert_eq!(
            reason(&skew),
            "the round items must share one axis direction to measure their spacing"
        );
        // Opposite axes are parallel.
        assert!(spacing(&[([0.0; 3], Z), ([5.0, 0.0, 0.0], [0.0, 0.0, -1.0])]).is_ok());
        assert!(spacing(&[([0.0; 3], Z)]).is_err());
        assert!(spacing(&vec![([0.0; 3], Z); 65]).is_err());
        assert!(spacing(&[([f64::NAN, 0.0, 0.0], Z), ([5.0, 0.0, 0.0], Z)]).is_err());
        assert!(spacing(&[([f64::INFINITY, 0.0, 0.0], Z), ([5.0, 0.0, 0.0], Z)]).is_err());
        assert!(spacing(&[([0.0; 3], [0.0; 3]), ([5.0, 0.0, 0.0], Z)]).is_err());
        assert!(spacing(&[([0.0; 3], Z), ([5.0, 0.0, 0.0], [f64::NAN, 0.0, 1.0])]).is_err());
    }
}
