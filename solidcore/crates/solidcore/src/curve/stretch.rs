//! A curve's control points stretched from a pivot: the picked points moved by a drag of one of
//! them (the handle), each by the share of the drag its place along the control polygon gives.
//!
//! Not upstream. With [`Stretch::Linear`], point `i` moves by `delta * s_i`, `s_i` the length of
//! the control polygon from the pivot to `i` over its length from the pivot to the handle (each
//! the sum of the legs between them, whichever way along the polygon they lie): the pivot stays,
//! the handle moves by `delta` whole, a point halfway along the polygon to the handle by half of
//! it, a point past the handle by more. With [`Stretch::None`], every picked point moves by
//! `delta`. With a plane normal, `delta` is first taken into the plane (its part along the unit
//! normal removed), so points in that plane stay in it.
//!
//! Refused: an index past the points, the handle at the pivot, a polygon of no length from the
//! pivot to the handle, a `delta` or normal not finite, a normal of no length. Bounded: one pass
//! over the points.

use crate::error::{Result, SolidError};
use crate::math::{Point3, Vec3};

const OP: &str = "curve::stretch::stretch_points";

/// How the picked points share a stretch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stretch {
    /// In proportion to the control polygon's length from the pivot.
    Linear,
    /// All alike: each by the whole drag.
    None,
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// The control polygon's length from point `from` to point `to` (the legs between them).
fn polygon_length(points: &[Point3], from: usize, to: usize) -> f64 {
    let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
    points.get(lo..=hi).map_or(0.0, |run| {
        run.windows(2)
            .map(|w| match w {
                [a, b] => a.distance(*b),
                _ => 0.0,
            })
            .sum()
    })
}

/// `points` with those `picked` moved: each by `delta` (taken into the plane of unit `normal`
/// first, when given) times its share by `how` (module notes); the others as they were.
///
/// # Errors
/// `InvalidInput` for the refusals the module names.
pub fn stretch_points(
    points: &[Point3],
    picked: &[usize],
    pivot: usize,
    handle: usize,
    delta: Vec3,
    how: Stretch,
    normal: Option<Vec3>,
) -> Result<Vec<Point3>> {
    let n = points.len();
    if pivot >= n || handle >= n || picked.iter().any(|&i| i >= n) {
        return Err(invalid("an index past the control points"));
    }
    if pivot == handle {
        return Err(invalid("the handle is the pivot"));
    }
    if !finite(delta) {
        return Err(invalid("a drag that is not finite"));
    }
    let delta = match normal {
        Some(normal) => {
            if !finite(normal) {
                return Err(invalid("a plane normal that is not finite"));
            }
            let unit = normal
                .unitize()
                .map_err(|_| invalid("a plane normal of no length"))?;
            delta - unit * delta.dot(unit)
        }
        None => delta,
    };
    let reach = polygon_length(points, pivot, handle);
    if how == Stretch::Linear && !(reach > 0.0 && reach.is_finite()) {
        return Err(invalid("no control polygon from the pivot to the handle"));
    }
    let mut out = points.to_vec();
    for &i in picked {
        let share = match how {
            Stretch::Linear => polygon_length(points, pivot, i) / reach,
            Stretch::None => 1.0,
        };
        if let (Some(slot), Some(&from)) = (out.get_mut(i), points.get(i)) {
            *slot = from + delta * share;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    /// Five points 1 apart along x: pivot 0, handle 4.
    fn row() -> Vec<Point3> {
        (0..5).map(|i| p(f64::from(i), 0.0, 0.0)).collect()
    }

    #[test]
    fn the_pivot_stays_the_handle_moves_whole_and_halfway_moves_half() {
        let d = Vec3::new(0.0, 2.0, 0.0);
        let out = stretch_points(&row(), &[0, 1, 2, 3, 4], 0, 4, d, Stretch::Linear, None).unwrap();
        for (i, q) in out.iter().enumerate() {
            let want = p(i as f64, 2.0 * i as f64 / 4.0, 0.0);
            assert!(q.distance(want) < 1e-12, "{i}: {q:?}");
        }
    }

    #[test]
    fn without_falloff_every_picked_point_moves_whole() {
        let d = Vec3::new(1.0, 1.0, 1.0);
        let out = stretch_points(&row(), &[1, 3], 0, 4, d, Stretch::None, None).unwrap();
        assert!(out[1].distance(p(2.0, 1.0, 1.0)) < 1e-12);
        assert!(out[3].distance(p(4.0, 1.0, 1.0)) < 1e-12);
        assert_eq!((out[0], out[2], out[4]), (row()[0], row()[2], row()[4]));
    }

    #[test]
    fn kept_in_a_plane_an_off_plane_drag_stays_in_it() {
        let d = Vec3::new(0.0, 1.0, 3.0);
        let out = stretch_points(
            &row(),
            &[1, 2, 3, 4],
            0,
            4,
            d,
            Stretch::Linear,
            Some(Vec3::new(0.0, 0.0, 2.0)),
        )
        .unwrap();
        assert!(out.iter().all(|q| q.z.abs() < 1e-12), "{out:?}");
        assert!(out[4].distance(p(4.0, 1.0, 0.0)) < 1e-12, "{out:?}");
    }

    #[test]
    fn bad_picks_are_refused() {
        let d = Vec3::new(0.0, 1.0, 0.0);
        assert!(stretch_points(&row(), &[9], 0, 4, d, Stretch::Linear, None).is_err());
        assert!(stretch_points(&row(), &[1], 2, 2, d, Stretch::Linear, None).is_err());
        let same = vec![p(0.0, 0.0, 0.0); 3];
        assert!(stretch_points(&same, &[1], 0, 2, d, Stretch::Linear, None).is_err());
        assert!(stretch_points(&row(), &[1], 0, 4, d, Stretch::Linear, Some(Vec3::ZERO)).is_err());
    }
}
