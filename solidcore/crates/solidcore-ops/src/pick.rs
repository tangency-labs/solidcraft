//! Entities picked by point: for an application that names the edges and vertices of an
//! operation by points near them (a click, a saved design's pick points) and needs the index
//! each point means before it calls the operation.
//!
//! - [`edges`]: per point, the body's edge nearest it.
//! - [`vertices`]: per point, the body's vertex nearest it.
//!
//! The rule is one for both. A point picks the entity nearest it. It picks none
//! ([`Miss::None`]) when that entity is farther than the tolerance asked. It is ambiguous
//! ([`Miss::Ambiguous`]) when the second nearest is within the tolerance too and farther than
//! the nearest by at most the body's own tolerance: two entities the body itself holds as one
//! place. The points are taken in order and the first that misses ends the pick.
//!
//! Every distance is the kernel's (`query::api_queries::edge_closest_point`,
//! `query::api_queries::vertex_distance`); ops compares them. An edge whose closest-point solve
//! answers with a status (no point found) is passed over, as an edge the point cannot be near.
//! Indices are the body's native lists', as [`crate::blend`] takes them; a copy keeps the order
//! ([`Body::copy`]).

use solidcore::query::api_queries as q;
use solidcore::{Brep, Kernel, Point3};

use crate::body::Body;
use crate::session::{answer, read_body, Answer};
use crate::{Error, Result};

/// The largest tolerance a pick takes.
pub const MAX_TOLERANCE: f64 = 1e6;

/// Why a pick chose no entity, and the index of the point that chose none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// The point is farther than the tolerance from every entity.
    None(usize),
    /// The point is equally near to two entities: both within the tolerance, their distances
    /// apart by at most the body's own tolerance.
    Ambiguous(usize),
}

/// What a pick chose: per point, the index of its entity in the body's native list, or the
/// first point that chose none.
pub type Picked = std::result::Result<Vec<usize>, Miss>;

impl Answer for Picked {}

/// The points as the kernel's, and the tolerance, checked.
fn checked(points: &[[f64; 3]], tolerance: f64) -> Result<Vec<Point3>> {
    let tolerance_ok = tolerance.is_finite() && tolerance > 0.0 && tolerance < MAX_TOLERANCE;
    let points_ok = points.iter().flatten().all(|x| x.is_finite());
    if !(tolerance_ok && points_ok) {
        return Err(Error::Argument);
    }
    Ok(points
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect())
}

/// The rule of the module on the distances `distance` gives from each point to each of `count`
/// entities (`None`: an entity passed over).
fn nearest(
    count: usize,
    points: &[Point3],
    tolerance: f64,
    body_tolerance: f64,
    mut distance: impl FnMut(usize, Point3) -> solidcore::Result<Option<f64>>,
) -> solidcore::Result<Picked> {
    let mut picked = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        let (mut best, mut d0, mut d1) = (None, f64::INFINITY, f64::INFINITY);
        for x in 0..count {
            let Some(d) = distance(x, p)? else { continue };
            if !d.is_finite() {
                continue;
            }
            if d < d0 {
                d1 = d0;
                d0 = d;
                best = Some(x);
            } else if d < d1 {
                d1 = d;
            }
        }
        let Some(best) = best.filter(|_| d0 <= tolerance) else {
            return Ok(Err(Miss::None(i)));
        };
        if d1 <= tolerance && apart(d0, d1) <= body_tolerance {
            return Ok(Err(Miss::Ambiguous(i)));
        }
        picked.push(best);
    }
    Ok(Ok(picked))
}

/// How much farther the second nearest entity is than the nearest.
#[allow(
    clippy::float_arithmetic,
    reason = "two of the kernel's distances compared with a tolerance, not a geometric quantity (docs/layers.md)"
)]
fn apart(nearest: f64, second: f64) -> f64 {
    second - nearest
}

/// Per point of `points`, the index in `body`'s native edge list of the edge nearest it within
/// `tolerance`, by the module's rule; the first point that picks no edge otherwise.
///
/// # Errors
/// [`Error::Argument`] for a coordinate that is not finite or a tolerance not finite, positive
/// and below [`MAX_TOLERANCE`]; the kernel's error from a closest-point solve that is no
/// status; [`Error::Internal`] for a body a panic left unfinished.
pub fn edges(body: &Body, points: &[[f64; 3]], tolerance: f64) -> Result<Picked> {
    let points = checked(points, tolerance)?;
    read_body("pick::edges", body, |k: &mut Kernel, b: &Brep| {
        let list = b.edge_list();
        nearest(list.len(), &points, tolerance, b.tolerance(), |x, p| {
            let Some(&e) = list.get(x) else {
                return Ok(None);
            };
            Ok(answer(q::edge_closest_point(k, b, e, p))?.map(|c| c.closest.distance))
        })
    })
}

/// Per point of `points`, the index in `body`'s native vertex list of the vertex nearest it
/// within `tolerance`, by the module's rule; the first point that picks no vertex otherwise.
///
/// # Errors
/// [`Error::Argument`] for a coordinate that is not finite or a tolerance not finite, positive
/// and below [`MAX_TOLERANCE`]; the kernel's refusal of a distance that overflows;
/// [`Error::Internal`] for a body a panic left unfinished.
pub fn vertices(body: &Body, points: &[[f64; 3]], tolerance: f64) -> Result<Picked> {
    let points = checked(points, tolerance)?;
    read_body("pick::vertices", body, |_: &mut Kernel, b: &Brep| {
        let list = b.vertex_list();
        nearest(list.len(), &points, tolerance, b.tolerance(), |x, p| {
            let Some(&v) = list.get(x) else {
                return Ok(None);
            };
            Ok(Some(q::vertex_distance(b, v, p)?.value))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_box;

    /// The rule on plain numbers: entities at 0, 1 and 3 on a line, the points' x their place.
    #[test]
    fn the_nearest_within_tolerance_is_picked_unless_two_are_equally_near() {
        let at = [0.0f64, 1.0, 3.0];
        let d = |x: usize, p: Point3| Ok(Some((at[x] - p.x).abs()));
        let pts =
            |xs: &[f64]| -> Vec<Point3> { xs.iter().map(|&x| Point3::new(x, 0.0, 0.0)).collect() };
        assert_eq!(nearest(3, &pts(&[0.9]), 0.5, 1e-6, d).unwrap(), Ok(vec![1]));
        assert_eq!(
            nearest(3, &pts(&[2.0]), 0.5, 1e-6, d).unwrap(),
            Err(Miss::None(0))
        );
        // half way between 0 and 1, both within 1: ambiguous, at the second point
        assert_eq!(
            nearest(3, &pts(&[1.0, 0.5]), 1.0, 1e-6, d).unwrap(),
            Err(Miss::Ambiguous(1))
        );
        // each point has its own answer: the same entity twice is given twice
        assert_eq!(
            nearest(3, &pts(&[3.1, 2.9]), 0.5, 1e-6, d).unwrap(),
            Ok(vec![2, 2])
        );
        // an entity passed over is never the nearest
        let skip = |x: usize, p: Point3| Ok((x != 1).then(|| (at[x] - p.x).abs()));
        assert_eq!(
            nearest(3, &pts(&[0.9]), 0.5, 1e-6, skip).unwrap(),
            Err(Miss::None(0))
        );
    }

    /// On a 10 x 20 x 30 box: an edge's middle picks that edge (its closest point is the point,
    /// distance 0, and its two end vertices are the box's corners at x 0 and 10); a corner is
    /// equally near its three edges; a point 5 away from everything picks none at tolerance 1.
    #[test]
    fn a_box_s_edges_and_vertices_are_picked_by_point() {
        let b = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        let [_, n_edges, n_vertices] = b.counts().unwrap();
        assert_eq!((n_edges, n_vertices), (12, 8));
        let middle = [5.0, 0.0, 0.0];
        let picked = edges(&b, &[middle, middle], 0.1).unwrap().unwrap();
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0], picked[1]);
        assert!(picked[0] < n_edges);
        // every edge's middle picks a different edge
        let middles = [
            [5.0, 0.0, 0.0],
            [5.0, 20.0, 0.0],
            [5.0, 0.0, 30.0],
            [5.0, 20.0, 30.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 30.0],
            [10.0, 10.0, 30.0],
            [0.0, 0.0, 15.0],
            [10.0, 0.0, 15.0],
            [0.0, 20.0, 15.0],
            [10.0, 20.0, 15.0],
        ];
        let mut all = edges(&b, &middles, 0.1).unwrap().unwrap();
        all.sort_unstable();
        assert_eq!(all, (0..12).collect::<Vec<_>>());
        assert_eq!(
            edges(&b, &[middle, [10.0, 20.0, 30.0]], 0.1).unwrap(),
            Err(Miss::Ambiguous(1))
        );
        assert_eq!(
            edges(&b, &[[5.0, -5.0, 0.0]], 1.0).unwrap(),
            Err(Miss::None(0))
        );
        // the same point picks its edge at a tolerance that reaches it
        assert_eq!(
            edges(&b, &[[5.0, -5.0, 0.0]], 6.0).unwrap(),
            Ok(vec![picked[0]])
        );

        let corner = vertices(&b, &[[10.0, 20.0, 30.0], [0.0, 0.0, 0.0]], 0.1)
            .unwrap()
            .unwrap();
        assert_ne!(corner[0], corner[1]);
        assert!(corner.iter().all(|&v| v < n_vertices));
        // half way along an edge 10 long, its two ends are 5 away each: ambiguous at 6, none at 1
        assert_eq!(
            vertices(&b, &[middle], 6.0).unwrap(),
            Err(Miss::Ambiguous(0))
        );
        assert_eq!(vertices(&b, &[middle], 1.0).unwrap(), Err(Miss::None(0)));
        // no points pick nothing
        assert_eq!(edges(&b, &[], 0.1).unwrap(), Ok(Vec::new()));
    }

    #[test]
    fn hostile_picks_are_refused() {
        let b = make_box([0.0; 3], [1.0; 3]).unwrap();
        for t in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_TOLERANCE] {
            assert_eq!(edges(&b, &[[0.0; 3]], t), Err(Error::Argument), "{t}");
            assert_eq!(vertices(&b, &[[0.0; 3]], t), Err(Error::Argument), "{t}");
        }
        assert_eq!(
            edges(&b, &[[f64::NAN, 0.0, 0.0]], 0.1),
            Err(Error::Argument)
        );
        assert_eq!(
            vertices(&b, &[[0.0, f64::INFINITY, 0.0]], 0.1),
            Err(Error::Argument)
        );
    }
}
