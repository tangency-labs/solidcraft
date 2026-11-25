//! Entities picked by point: for an application that names the edges and vertices of an
//! operation by points near them (a click, a saved design's pick points) and needs the index
//! each point means before it calls the operation.
//!
//! - [`edges`]: per point, the body's edge nearest it.
//! - [`vertices`]: per point, the body's vertex nearest it.
//! - [`Body::face_at`]: the face a point lies on, by the kernel's classification.
//! - [`Body::faces_on_one_surface`]: picks grown across the edges where the kernel proves one
//!   surface continues.
//! - [`stored_face`] and [`stored_edge`]: whether a saved record index and approximate hit
//!   still name that exact face or edge, without choosing a replacement entity.
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
use std::collections::BTreeMap;

use solidcore::error::EntityRef;
use solidcore::intersect::point_class::PointClassType;
use solidcore::query::brep::BrepClassifier;
use solidcore::query::faces_continue::{faces_continue, Continues};
use solidcore::topology::{edge, face, EdgeId, FaceId};
use solidcore::{Brep, Kernel, Point3};
use solidcore_record::brep::Brep as Record;

use crate::body::Body;
use crate::measure::box_max_distance_to_point;
use crate::order::{self, Order};
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

impl Answer for Vec<FaceId> {}

impl Body {
    /// The record face (through `order`) point `p` lies on, within `reach` of it, or `None`:
    /// the kernel's classification of the point against the body at that tolerance
    /// (`SmBrep::Point3DClassify`, with boundary intersections). A point the kernel puts on an
    /// edge or a vertex is on more than one face and picks none; so does a point off the body or
    /// one the kernel does not classify. Never a face guessed by nearness.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate or a reach that is not finite and positive; the
    /// kernel's status (its classification may add trim curves to the body).
    pub fn face_at(&self, order: &Order, p: [f64; 3], reach: f64) -> Result<Option<usize>> {
        if !(p.iter().all(|x| x.is_finite()) && reach.is_finite() && reach > 0.0) {
            return Err(Error::Argument);
        }
        let point = Point3::new(p[0], p[1], p[2]);
        let found = read_body("face_at", self, |k, b| {
            let mut classifier = BrepClassifier::of_brep(b);
            let pc = match classifier.point_3d_classify(k, point, reach, true) {
                Ok(pc) => pc,
                Err(e) if e.sm_status().is_some() => return Ok(None),
                Err(e) => return Err(e),
            };
            Ok(match (pc.point_class(), pc.object()) {
                (PointClassType::Face, Some(EntityRef::Face(f))) => Some(f),
                _ => None,
            })
        })?;
        Ok(found.and_then(|f| order.faces.iter().position(|&x| x == f)))
    }

    /// Record faces `faces` (through `order`) and every face joined to them, across an edge not
    /// in `keep_edges` (record edge indices: the edges that stay apart, such as a split face's
    /// line), where the kernel proves the two faces continue one surface with the same outward
    /// side ([`faces_continue`]). An edge the kernel leaves undecided is not crossed. The picks
    /// first, in their order, then each face as it is reached.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or the edges; the kernel's (a side it is
    /// unsure is material, a dangling id).
    pub fn faces_on_one_surface(
        &self,
        order: &Order,
        faces: &[usize],
        keep_edges: &[usize],
    ) -> Result<Vec<usize>> {
        let seeds: Vec<FaceId> = faces
            .iter()
            .map(|&i| order::face(order, i))
            .collect::<Result<_>>()?;
        let keep: Vec<EdgeId> = keep_edges
            .iter()
            .map(|&i| order::edge(order, i))
            .collect::<Result<_>>()?;
        let index: BTreeMap<FaceId, usize> = order
            .faces
            .iter()
            .enumerate()
            .map(|(i, &f)| (f, i))
            .collect();
        let grown = read_body("faces_on_one_surface", self, |_, b| {
            let mut out: Vec<FaceId> = Vec::new();
            for f in seeds {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
            let mut next = 0;
            while let Some(&f) = out.get(next) {
                next += 1;
                for e in face::edges(b, f, None, false, None)? {
                    if keep.contains(&e) {
                        continue;
                    }
                    for g in edge::faces(b, e, None)? {
                        if g != f
                            && !out.contains(&g)
                            && faces_continue(b, f, g, e)? == Continues::Proved
                        {
                            out.push(g);
                        }
                    }
                }
            }
            Ok(out)
        })?;
        grown
            .into_iter()
            .map(|f| index.get(&f).copied().ok_or(Error::Argument))
            .collect()
    }
}

/// The allowance of an approximate stored hit: a thousandth of the exact box diagonal,
/// at least a thousandth of a unit and the record's tolerance. Measuring the farthest box
/// corner from its least corner gives the diagonal through an existing kernel query.
#[allow(
    clippy::float_arithmetic,
    reason = "scales the kernel's measured diagonal for a workflow tolerance, not a geometric quantity (docs/layers.md)"
)]
fn stored_tolerance(body: &Body, record_tolerance: f64) -> Result<f64> {
    let (lo, hi) = body.bounds()?;
    let diagonal = box_max_distance_to_point(lo, hi, lo)?;
    let tolerance = (diagonal * 1e-3).max(1e-3).max(record_tolerance);
    if !tolerance.is_finite() {
        return Err(Error::Argument);
    }
    Ok(tolerance)
}

/// Whether a stored face index and approximate hit still name that exact trimmed face.
/// The allowance is a thousandth of the exact body box diagonal, at least a thousandth
/// of a unit and the record's tolerance. The point must
/// lie on the named face's surface and within its own trims, as [`Body::point_on_face`]
/// decides. No nearest face or display mesh substitutes for the supplied index.
///
/// The record must be a B-rep; callers holding mesh bodies must reject them before this call.
///
/// # Errors
/// Those of [`Body::from_record`], [`Body::bounds`] and [`Body::point_on_face`];
/// [`Error::Argument`] for an allowance that is not finite.
pub fn stored_face(record: &Record, face: usize, point: [f64; 3]) -> Result<bool> {
    let (body, order) = Body::from_record(record, true)?;
    let tolerance = stored_tolerance(&body, record.tolerance)?;
    Ok(body
        .point_on_face(&order, face, point, tolerance)?
        .is_some())
}

/// Whether a stored edge index and approximate hit still name that exact trimmed curve.
/// The allowance is a thousandth of the exact body box diagonal, at least a thousandth
/// of a unit and the record's tolerance. The named
/// edge's closest-point distance must be within it. No nearest edge or display mesh
/// substitutes for the supplied index.
///
/// The record must be a B-rep; callers holding mesh bodies must reject them before this call.
///
/// # Errors
/// Those of [`Body::from_record`], [`Body::bounds`] and [`Body::edge_closest_point`];
/// [`Error::Argument`] for an allowance that is not finite.
pub fn stored_edge(record: &Record, edge: usize, point: [f64; 3]) -> Result<bool> {
    let (body, order) = Body::from_record(record, true)?;
    let tolerance = stored_tolerance(&body, record.tolerance)?;
    Ok(body
        .edge_closest_point(&order, edge, point)?
        .is_some_and(|p| p.distance <= tolerance))
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

    /// The 3 x 4 x 12 box has diagonal 13, so saved hits may be 0.013 away. Its bottom
    /// face and bottom front edge accept a 0.01 offset, refuse 0.02 and never substitute a
    /// different face or edge. A point on their untrimmed support beyond the box is refused.
    #[test]
    fn stored_hits_validate_the_named_trimmed_entity_with_exact_scale() {
        let body = make_box([0.0; 3], [3.0, 4.0, 12.0]).unwrap();
        let (record, order) = body.to_record().unwrap();
        assert_eq!(
            stored_tolerance(&body, record.tolerance).unwrap(),
            13.0 * 1e-3
        );
        let face_point = [1.5, 2.0, 0.0];
        let face = (0..record.faces.len())
            .find(|&face| {
                body.point_on_face(&order, face, face_point, 1e-6)
                    .unwrap()
                    .is_some()
            })
            .unwrap();
        assert!(stored_face(&record, face, face_point).unwrap());
        assert!(stored_face(&record, face, [1.5, 2.0, 0.01]).unwrap());
        assert!(!stored_face(&record, face, [1.5, 2.0, 0.02]).unwrap());
        assert!(!stored_face(&record, face, [4.0, 2.0, 0.0]).unwrap());
        assert!((0..record.faces.len())
            .filter(|&other| other != face)
            .all(|other| !stored_face(&record, other, face_point).unwrap()));

        let edge_point = [1.5, 0.0, 0.0];
        let edge = (0..record.edges.len())
            .find(|&edge| {
                body.edge_closest_point(&order, edge, edge_point)
                    .unwrap()
                    .is_some_and(|p| p.distance < 1e-6)
            })
            .unwrap();
        assert!(stored_edge(&record, edge, edge_point).unwrap());
        assert!(stored_edge(&record, edge, [1.5, 0.01, 0.0]).unwrap());
        assert!(!stored_edge(&record, edge, [1.5, 0.02, 0.0]).unwrap());
        assert!(!stored_edge(&record, edge, [4.0, 0.0, 0.0]).unwrap());
        assert!((0..record.edges.len())
            .filter(|&other| other != edge)
            .all(|other| !stored_edge(&record, other, edge_point).unwrap()));
        assert_eq!(
            stored_face(&record, record.faces.len(), face_point),
            Err(Error::Argument)
        );
        assert_eq!(
            stored_edge(&record, record.edges.len(), edge_point),
            Err(Error::Argument)
        );
        assert_eq!(
            stored_face(&record, face, [f64::NAN, 0.0, 0.0]),
            Err(Error::Argument)
        );
        assert_eq!(
            stored_edge(&record, edge, [0.0, f64::INFINITY, 0.0]),
            Err(Error::Argument)
        );
    }

    #[test]
    fn stored_hit_allowance_keeps_absolute_and_record_tolerance_floors() {
        let body = make_box([0.0; 3], [0.1; 3]).unwrap();
        assert_eq!(stored_tolerance(&body, 1e-6).unwrap(), 1e-3);
        assert_eq!(stored_tolerance(&body, 0.02).unwrap(), 0.02);
        assert_eq!(stored_tolerance(&body, f64::INFINITY), Err(Error::Argument));
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
