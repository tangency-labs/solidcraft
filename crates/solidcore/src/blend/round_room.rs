//! Rounds refused for the room they leave or take: what the refusal found, as data
//! ([`RoundRefusal`], carried by [`SolidError::RoundRefused`]).
//!
//! Not upstream: SolidCraft's checks around its fillet commands (its `blend.rs` `closed`,
//! `blend_variable.rs` `taper`), made the kernel's so that every application gets them.
//! The room a variable round's band has along a straight edge between planes is
//! [`crate::blend::band_room`]'s.

use std::collections::BTreeSet;

use crate::curve::curve::p8::CurveIntersectOptions;
use crate::error::{SolidError, SM_ERR};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::math::Point3;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::{edge, face, EdgeId, FaceId, VertexId};
use crate::{Brep, Kernel, Result};

const OP: &str = "blend::round_room";

/// Why a round was refused, and what was found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RoundRefusal {
    /// The rounds left a face whose boundary crosses itself (rounds overlapping where a wall
    /// is thinner than both): the points where it does, and the largest size that leaves no
    /// crossing where it was found.
    RoundsCross {
        /// Points where a face's boundary crosses itself.
        at: Vec<Point3>,
        /// The largest size that leaves no crossing, when found.
        fit: Option<f64>,
    },
    /// An end of a straight edge between planes where a side of a face leans away from the
    /// edge, or is not straight: a variable round's band has no room there that is known.
    TaperEndUnsupported {
        /// The edge's vertex at that end.
        vertex: VertexId,
        /// How far the side's far end lies past the plane square to the edge at the vertex,
        /// along the edge's outward direction, when the side is straight.
        lean: Option<f64>,
    },
    /// The faces of a straight edge between planes close to a point at both its ends: a
    /// band of any radius takes the whole of a face there.
    FacesCloseAtBothEnds {
        /// The edge.
        edge: EdgeId,
    },
    /// A variable round's radius is larger than the room its band has at a station of the
    /// edge.
    RoomRunsOut {
        /// The edge.
        edge: EdgeId,
        /// The station: the fraction of the way from the edge's start vertex to its end.
        at: f64,
        /// A radius whose band is proved to fit there (the room's certified lower bound).
        largest: f64,
    },
}

/// [`SolidError::RoundRefused`] of `refusal`, by `operation`.
#[must_use]
pub fn refused(operation: &'static str, refusal: RoundRefusal) -> SolidError {
    SolidError::RoundRefused {
        operation,
        code: SM_ERR,
        refusal,
    }
}

/// The curve objects of the intersections' solutions: their natural intervals.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveHandle(Extent1d);

impl SolutionObject for CurveHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.0)
    }
}

/// The points where the boundary of the planar face `f` meets itself other than at a vertex
/// its two edges share: every pair of the face's edges (of all its loops) intersected by the
/// kernel's curve intersector (`GlobalCurveIntersect`, analytic for lines and conics) within
/// the larger of their tolerances, a meeting within that tolerance of a vertex both edges end
/// at being their joint. A face that is not planar gives none (its boundary is not judged).
///
/// # Errors
/// Those of the topology reads, the intersections and the evaluations.
pub fn boundary_crossings(k: &Kernel, b: &Brep, f: FaceId) -> Result<Vec<Point3>> {
    if !matches!(
        b.face(f)?.surface.as_ref().map(Surface::kind),
        Some(SurfaceKind::Plane(_))
    ) {
        return Ok(Vec::new());
    }
    let mut edges: Vec<EdgeId> = Vec::new();
    for e in face::p4::edges(b, f, None, false, None)? {
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    let mut out = Vec::new();
    for (i, &e1) in edges.iter().enumerate() {
        for &e2 in edges.iter().skip(i + 1) {
            let (a, c) = (b.edge(e1)?, b.edge(e2)?);
            let (Some(c1), Some(c2)) = (a.curve.as_ref(), c.curve.as_ref()) else {
                return Err(SolidError::null_pointer(OP));
            };
            let tol = a.tolerance.max(c.tolerance);
            let found = c1.global_curve_intersect::<CurveHandle>(
                k,
                &a.interval,
                c2,
                &c.interval,
                tol,
                &CurveIntersectOptions::default(),
            )?;
            let (s1, t1) = edge::start_end_vertices(b, e1)?;
            let (s2, t2) = edge::start_end_vertices(b, e2)?;
            let shared: BTreeSet<VertexId> = [s1, t1]
                .into_iter()
                .flatten()
                .filter(|v| [s2, t2].contains(&Some(*v)))
                .collect();
            for s in found.as_slice() {
                let Some(&t) = s.start.parameters.first() else {
                    continue;
                };
                let p = c1.evaluate_point(k, t)?;
                let mut joint = false;
                for &v in &shared {
                    joint |= (b.vertex(v)?.point - p).length() <= tol;
                }
                if !joint {
                    out.push(p);
                }
            }
        }
    }
    Ok(out)
}

/// Refuses rounds whose result has a face whose boundary crosses itself: the faces `made` (the
/// rounds' faces) and every face sharing an edge with one, each judged by
/// [`boundary_crossings`]. Rounds that overlap where a wall is thinner than both leave such a
/// face: closed as topology, but with no inside.
///
/// # Errors
/// [`SolidError::RoundRefused`] with [`RoundRefusal::RoundsCross`], the points where the
/// boundaries cross (no fit: the largest size that leaves none is not searched); those of
/// [`boundary_crossings`].
pub fn rounds_cross(k: &Kernel, b: &Brep, made: &[FaceId]) -> Result<()> {
    let mut faces = BTreeSet::new();
    for &f in made {
        faces.insert(f);
        for e in face::p4::edges(b, f, None, false, None)? {
            faces.extend(edge::faces(b, e, None)?);
        }
    }
    let mut at = Vec::new();
    for f in faces {
        at.extend(boundary_crossings(k, b, f)?);
    }
    if at.is_empty() {
        Ok(())
    } else {
        Err(refused(OP, RoundRefusal::RoundsCross { at, fit: None }))
    }
}

#[cfg(test)]
#[path = "round_room_tests.rs"]
mod tests;
