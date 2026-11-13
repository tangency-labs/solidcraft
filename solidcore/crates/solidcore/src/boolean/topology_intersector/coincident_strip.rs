//! Not upstream (#15718, replacing WR-614's removal, tangency-labs/solidcore#1794): the
//! crossing line of two planar faces that lie within the intersection tolerance of each
//! other wherever both exist beside it is no event, decided from the operands' geometry
//! before the face pair is intersected.
//!
//! Two planes crossing at a small angle θ meet in a line fixed only to `tol / sin θ`.
//! Their distance is affine across the line: zero on it, growing linearly to either side.
//! Take each side of the crossing. When on both sides a straight boundary edge of either
//! face spans the crossing's extent along the line, faces back toward the line (its face's
//! interior lies between it and the line), and lies within the intersection tolerance of
//! the other face's plane at both ends, then every point where both faces exist beside the
//! crossing lies within that tolerance of the other face (the distance is affine, so it is
//! largest at the bounds, and past a bound its face ends). The two face pieces form one
//! coincident pair and the crossing is a line where two coincident pieces cross within
//! their tolerance, not a feature: the face pair makes no curve, no edge definition and no
//! vertex definitions for it ([`crossing_inside_coincident_pair`]). Where either face
//! goes on past the tolerance on one side, the crossing is an exit and stays.
//!
//! Upstream makes the crossing an edge definition. Its end vertices, a tolerance or less
//! from the bounds' vertices, are picked per B-rep (`SmProtoVertex::GetFinalVertex`) and
//! never related by `SetRelationships`, so `SmMerge::MakeFaceuse` fails on the edge's
//! inconsistent vertex mates (`SM_ERR`); or its ends make a chain of points pairwise
//! within the tolerance that no vertex covers. WR-614 removed the crossing's edge
//! definition after the intersection, which left its end vertex definitions (a dangling
//! edge in each end face) and missed the crossing once the classification had read it as
//! running along the bounds; it also required the bounds to run parallel to the crossing
//! and farther from it than the tolerance. Deciding the rule up front, from the planes and
//! the bound edges, removes all three.
//!
//! The decision is a certificate, not a sample: each bound is a line segment, so its
//! distance to the other plane is affine along it and its two end distances bound it
//! (with their rounding counted); the side of each end and the direction its face extends
//! are signs of distances to the other plane; and the crossing's extent is over-estimated
//! by the projections of both faces onto the crossing's direction (from the end points of
//! their line edges and the boxes of the others), so a bound that spans it spans the
//! crossing wherever it lies in both faces.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `crossing_inside_coincident_pair` (crate) | none (own design) |
//!
//! Deviations: not upstream at all; always on. Planes only (two planar faces, bounds on
//! lines); every other pair is intersected as upstream does.

use crate::curve::curve::p2::CurveKind;
use crate::error::Result;
use crate::geom::tol::{xsect_tol_3d_from_zones, zone_tol3d};
use crate::intersect::point_class::{object_zone_tol_3d, ClassObject};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::math::gamma;
use crate::surface::surface::p2::SurfaceKind;

use super::p1::ignore_soft;
use super::p2::IntersectorBrepP2;

const OP_STRIP: &str =
    "boolean::topology_intersector::coincident_strip::crossing_inside_coincident_pair";

/// Origin and unit normal of a planar face's surface.
#[derive(Clone, Copy)]
struct PlanePos {
    origin: Point3,
    unit_normal: Vec3,
}

impl PlanePos {
    /// The signed distance of `pt` from the plane, and a bound on its rounding: the
    /// difference, the dot product and the normal's own normalisation, each a few
    /// rounded operations on coordinates no larger than `|pt| + |origin|`.
    fn signed_distance(self, pt: Point3) -> (f64, f64) {
        let rounding = gamma(16) * (pt.length() + self.origin.length());
        ((pt - self.origin).dot(self.unit_normal), rounding)
    }
}

/// Whether the crossing line of planar faces `fa` and `fb` (of the two operands) lies
/// inside a coincident pair of their pieces (module docs): on both sides of the line a
/// straight boundary edge of either face spans the crossing's extent, faces back toward the
/// line and lies within the intersection tolerance of the other face's plane. `false` for
/// any face that is not planar, for planes closer to parallel than `EFF_ZERO_SQRT`, and
/// for faces whose projections onto the crossing's direction do not overlap.
///
/// # Errors
///
/// Those of the curve and binormal evaluations, the edge boxes and the zone tolerances.
pub(crate) fn crossing_inside_coincident_pair<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    fa: O,
    fb: O,
) -> Result<bool> {
    k.check_cancelled(OP_STRIP)?;
    let (Some(pa), Some(pb)) = (plane_pos(brep, fa), plane_pos(brep, fb)) else {
        return Ok(false);
    };
    // the other plane's normal turned toward fa's, so that both measure "above" alike
    let pb = PlanePos {
        origin: pb.origin,
        unit_normal: if pa.unit_normal.dot(pb.unit_normal) < 0.0 {
            -pb.unit_normal
        } else {
            pb.unit_normal
        },
    };
    let cross = pa.unit_normal.cross(pb.unit_normal);
    let sine = cross.length();
    let crossing = sine > EFF_ZERO_SQRT;
    if !crossing {
        return Ok(false);
    }
    let along = cross / sine;
    let (Some(span_a), Some(span_b)) = (
        face_span(k, brep, fa, along)?,
        face_span(k, brep, fb, along)?,
    ) else {
        return Ok(false);
    };
    let extent = (span_a.0.max(span_b.0), span_a.1.min(span_b.1));
    if extent.0 > extent.1 {
        return Ok(false);
    }
    // On fa, the side of the crossing a point lies on is the sign of its distance above
    // fb's plane; on fb, the sign of its distance below fa's (fa rises above fb on one
    // side of the line exactly where fb lies below fa). `flip` turns fb's into fa's sense.
    let (mut above, mut below) = (false, false);
    for (face, other, other_plane, flip) in [(fa, fb, pb, 1.0), (fb, fa, pa, -1.0)] {
        let other_zone = object_zone_tol_3d(k, &*brep, Some(other))?;
        for edge in brep.face_edges(face)? {
            match bound_side(
                k,
                brep,
                face,
                edge,
                (other_plane, flip, other_zone),
                along,
                extent,
            )? {
                Some(Side::Above) => above = true,
                Some(Side::Below) => below = true,
                None => {}
            }
            if above && below {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A side of the crossing line, in fa's sense: where fa lies above fb's plane, or below.
#[derive(Clone, Copy)]
enum Side {
    Above,
    Below,
}

/// The side of the crossing (in fa's sense) that `edge` of `face` bounds (module docs), or `None`: the edge is a line
/// segment; both its ends lie within the intersection tolerance of the other face's plane
/// (`other`: that plane, the sign `flip` that turns its distances into fa's sense, and the
/// other face's zone tolerance), with their rounding counted, so the whole edge does (the
/// distance is affine along it); both ends lie on that side or on the line (within their
/// rounding); its projection onto the crossing's direction `along` covers `extent`
/// (within the tolerance and its end vertices' zone tolerances); and its face's interior
/// lies toward the line from it (the inward binormal, constant along a line edge of a
/// planar face, takes the distance to the other plane toward zero, or across the line for
/// an edge on it).
fn bound_side<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    face: O,
    edge: O,
    (other, flip, other_zone): (PlanePos, f64, f64),
    along: Vec3,
    extent: (f64, f64),
) -> Result<Option<Side>> {
    let Some(ivl) = brep.edge_interval(edge) else {
        return Ok(None);
    };
    let ends = {
        let Some(curve) = brep.edge_curve(edge).flatten().and_then(|c| brep.curve(c)) else {
            return Ok(None);
        };
        if !matches!(curve.kind(), CurveKind::Line(_)) {
            return Ok(None);
        }
        [
            curve.evaluate_point(k, ivl.min())?,
            curve.evaluate_point(k, ivl.max())?,
        ]
    };
    let tol = xsect_tol_3d_from_zones(k, object_zone_tol_3d(k, &*brep, Some(edge))?, other_zone);
    let mut dist = [(0.0, 0.0); 2];
    for (slot, &p) in dist.iter_mut().zip(&ends) {
        let (g, rounding) = other.signed_distance(p);
        let within = g.abs() + rounding < tol;
        if !within {
            return Ok(None);
        }
        *slot = (flip * g, rounding);
    }
    let (s0, s1) = (ends[0].dot(along), ends[1].dot(along));
    let end_tol = tol + edge_end_tol(k, brep, edge)?;
    if !(s0.min(s1) <= extent.0 + end_tol && s0.max(s1) >= extent.1 - end_tol) {
        return Ok(None);
    }
    let Some(eu) = brep.edge_edgeuse_of_face(edge, face)? else {
        return Ok(None);
    };
    let Some(binorm) = ignore_soft(brep.edgeuse_evaluate_binormal(k, eu, ivl.mid()))? else {
        return Ok(None);
    };
    if !binorm.is_initialized() {
        return Ok(None);
    }
    let rise = flip * other.unit_normal.dot(binorm);
    for (side, sigma) in [(Side::Above, 1.0), (Side::Below, -1.0)] {
        let on_side = dist.iter().all(|&(g, rounding)| sigma * g >= -rounding);
        if on_side && sigma * rise < 0.0 {
            return Ok(Some(side));
        }
    }
    Ok(None)
}

/// The projection of `face` onto the unit direction `along`, over-estimated: the end
/// points of its line edges and the box corners of its other edges; `None` for a face
/// without edges.
fn face_span<O: ClassObject>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    face: O,
    along: Vec3,
) -> Result<Option<(f64, f64)>> {
    let mut span: Option<(f64, f64)> = None;
    let mut take = |p: Point3| {
        let s = p.dot(along);
        span = Some(span.map_or((s, s), |(lo, hi)| (lo.min(s), hi.max(s))));
    };
    for edge in brep.face_edges(face)? {
        let line = brep
            .edge_curve(edge)
            .flatten()
            .and_then(|c| brep.curve(c))
            .filter(|c| matches!(c.kind(), CurveKind::Line(_)));
        if let (Some(curve), Some(ivl)) = (line, brep.edge_interval(edge)) {
            take(curve.evaluate_point(k, ivl.min())?);
            take(curve.evaluate_point(k, ivl.max())?);
        } else {
            let b = brep.edge_bounding_box(k, edge)?;
            if b.has_negative_volume() {
                return Ok(None);
            }
            let (lo, hi) = (b.min(), b.max());
            for corner in 0..8 {
                take(Point3::new(
                    if corner & 1 == 0 { lo.x } else { hi.x },
                    if corner & 2 == 0 { lo.y } else { hi.y },
                    if corner & 4 == 0 { lo.z } else { hi.z },
                ));
            }
        }
    }
    Ok(span)
}

/// The plane of `face` when its surface is planar with a valid normal.
fn plane_pos<O: ClassObject>(brep: &impl IntersectorBrepP2<O>, face: O) -> Option<PlanePos> {
    let srf = brep
        .face_surface(face)
        .flatten()
        .and_then(|s| brep.surface(s))?;
    let SurfaceKind::Plane(plane) = srf.kind() else {
        return None;
    };
    let pos = plane.position();
    let n = pos.z_axis();
    if !n.is_initialized() {
        return None;
    }
    let len = n.length();
    (len > EFF_ZERO).then(|| PlanePos {
        origin: pos.origin(),
        unit_normal: n / len,
    })
}

/// Maximum zone tolerance of `edge`'s start and end vertices.
fn edge_end_tol<O: ClassObject>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    edge: O,
) -> Result<f64> {
    let v0 = brep.edge_start_vertex(edge)?;
    let v1 = brep.edge_other_vertex(edge, v0)?;
    let t0 = v0
        .and_then(|v| brep.topology_tolerance(v))
        .map_or(0.0, |t| zone_tol3d(k, t));
    let t1 = v1
        .and_then(|v| brep.topology_tolerance(v))
        .map_or(0.0, |t| zone_tol3d(k, t));
    Ok(t0.max(t1))
}
