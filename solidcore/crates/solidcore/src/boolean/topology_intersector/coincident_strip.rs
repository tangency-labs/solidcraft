//! Not upstream (#15718, replacing WR-614's removal, tangency-labs/solidcore#1794): the
//! crossing line of two planar faces that lie within the intersection tolerance of each
//! other wherever both exist beside it is no event, decided from the operands' geometry
//! before the face pair is intersected.
//!
//! Two planes crossing at a small angle θ meet in a line fixed only to `tol / sin θ`.
//! The signed distance to a plane is affine in the point: zero on the crossing line,
//! growing linearly to either side. The rule is for a crossing through both faces: each
//! has an edge end strictly on either side of it, beyond the end's slack (below). Two faces that meet only along their
//! boundaries (a shared edge, from opposite sides, at any angle) have no pieces beside the
//! line to pair, and are intersected as upstream does. Take each side of the crossing.
//! When on both sides one of the two faces lies, over the whole face, within the
//! intersection tolerance of the other face's plane, then every point where both faces
//! exist beside the crossing lies within that tolerance of the other face. The two face
//! pieces form one coincident pair and the crossing is a line where two coincident pieces
//! cross within their tolerance, not a feature: the face pair makes no curve, no edge
//! definition and no vertex definitions for it ([`crossing_inside_coincident_pair`]).
//! Where both faces go on past the tolerance on one side, the crossing is an exit and
//! stays.
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
//! the faces' boundaries, removes all three.
//!
//! The decision is a certificate over the whole face, not a sample and not an assumption
//! about its shape. A face is compact, so the supremum of an affine function over it is
//! reached on its boundary (inner loops included: they are checked too, and need not be).
//! Each boundary edge lies in the convex hull of its hull points (a line's ends, a
//! B-spline's control points; tangency-labs/solidcraft#16146's enclosures, with their
//! coordinate errors). So when every hull point of every edge of the face is closer to the
//! other plane than the tolerance on that side, with its error and the distance's rounding
//! added, every point of the face is. Holes, reentrant notches and bowed edges cannot break
//! this: it bounds the face, not one edge beside the line. A face with an edge whose curve
//! has no certain hull never qualifies, and a NaN anywhere refuses. The face itself is the
//! region of its own plane its trims bound, which the edge curves follow only to the edge's
//! tolerance and their offset from that plane: both are added to each hull point's error
//! (`boundary_slack`), so a grown edge tolerance or an edge off its plane cannot hide a
//! crossing.
//!
//! The ends of a crossing that stays (not upstream; own design): the crossing is fixed
//! only to `tol / sin θ` across a strip bounded by edges of the two faces, so at each end
//! of it the crossing's end and the ends of the edges bounding the strip are one contact
//! point (docs/booleans.md, "One point or two"), though they lie farther apart than their
//! tolerances. [`strip_contacts`] reads the strip from the face pairs' curve intervals
//! before any of their proto vertices is made and declares each end a contact: the segment
//! across the strip, widened by the tolerance within which its edges cover the crossing.
//! Where an edge bounds the crossing on one side only (the other face goes on out of
//! tolerance), the half strip between them is such a contact too, and the crossing stays
//! an edge. A crossing inside a coincident pair never gets that far (it makes no curve);
//! the strip is read from edge-on-face curve intervals whose offsets from the crossing
//! line, and coverage of it, are bounded over each curve's certified hull
//! (`Curve::hull_points_over`), never from samples.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `crossing_inside_coincident_pair` (crate) | none (own design) |
//! | [`strip_contacts`] (super) | none (own design) |
//!
//! Deviations: not upstream at all; always on. Planes only (two planar faces whose
//! boundary edges are lines or B-splines); every other pair is intersected as upstream
//! does.

use crate::error::{Result, SolidError};
use crate::geom::solution_array::SolutionObject;
use crate::geom::tol::{xsect_tol3d, xsect_tol_3d_from_zones, zone_tol3d};
use crate::intersect::point_class::{object_zone_tol_3d, ClassObject, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::math::gamma;
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::proto_topology::DeclaredContact;

use super::p1::{can_imprint, ignore_soft, FaceIntersection};
use super::p2::IntersectorBrepP2;

const OP_STRIP: &str =
    "boolean::topology_intersector::coincident_strip::crossing_inside_coincident_pair";
const OP_STRIP_CONTACTS: &str = "boolean::topology_intersector::coincident_strip::strip_contacts";

/// An edge definition (or a curve interval of a face pair that becomes one) as the strip
/// test reads it: what it is (`id`: its index), its
/// source objects and their classification types in B-reps 1 and 2, the start and end of its
/// curve over its interval, the points whose convex hull holds the curve there (`None`: no
/// certified hull), and its tolerance.
struct Def<O, I> {
    id: I,
    src: [(PointClassType, Option<O>); 2],
    ends: [Point3; 2],
    hull: Option<Vec<(Point3, f64)>>,
    tol: f64,
}

/// A face pair's intersection as `intersect_topology` keeps it until the proto topology is
/// made: the two faces, whether their surfaces coincide, the intersection and its single
/// points.
pub(super) type FacePairIntersection<O> = (O, O, bool, FaceIntersection<O>, Vec<Point3>);

/// One side of a strip: the offset of its bounding edge from the crossing line, and the
/// tolerance within which that edge covers the crossing's length.
#[derive(Clone, Copy)]
struct Side {
    off: Vec3,
    end_tol: f64,
}

/// The strip about a crossing: the sides edge-on-face definitions give it, and the two sides
/// that close it when it is bounded on both.
struct Strip {
    sides: Vec<Side>,
    closed: Option<[Side; 2]>,
}

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

    /// An upper bound of the distance from the plane of the point `q` whose coordinates are
    /// within `e` of it: the difference rounds by `γ_1` and the dot by `γ_3` of `|q - origin|`
    /// per component, the normal is unit within `γ_8` (as `curve::plane_of`).
    fn distance_bound(self, q: Point3, e: f64) -> f64 {
        let w = q - self.origin;
        (w.dot(self.unit_normal).abs() * (1.0 + gamma(8))
            + 2.0 * (gamma(1) + gamma(3)) * w.length() * (1.0 + gamma(5))
            + 2.0 * e)
            * (1.0 + gamma(2))
    }
}

/// Whether the crossing line of planar faces `fa` and `fb` (of the two operands) lies
/// inside a coincident pair of their pieces (module docs): each face crosses the line, and
/// on each side of the line one of the two faces lies, over its whole boundary, closer to
/// the other face's plane than their intersection tolerance. `false` for any face that is
/// not planar, for planes closer to parallel than `EFF_ZERO_SQRT`, and for a face that does
/// not reach both sides of the line.
///
/// # Errors
///
/// Those of the curve evaluations and the zone tolerances; cancellation.
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
    let sine = pa.unit_normal.cross(pb.unit_normal).length();
    let crossing = sine > EFF_ZERO_SQRT;
    if !crossing {
        return Ok(false);
    }
    // Each face crosses the line: a point of the face near one of its edge ends lies
    // strictly above the other face's plane and one strictly below (beyond the end's
    // `boundary_slack` and the distance's rounding). Faces that only meet the line
    // along their boundary, as two faces meeting along a shared edge from opposite sides,
    // have no piece on both sides of it: there is no coincident pair, whatever the angle.
    if !crosses_plane(k, brep, fa, pa, pb, sine)? || !crosses_plane(k, brep, fb, pb, pa, sine)? {
        return Ok(false);
    }
    let tol = xsect_tol_3d_from_zones(
        k,
        object_zone_tol_3d(k, &*brep, Some(fa))?,
        object_zone_tol_3d(k, &*brep, Some(fb))?,
    );
    // On fa, the side of the crossing a point lies on is the sign of its distance above
    // fb's plane; on fb, the sign of its distance below fa's (fa rises above fb on one
    // side of the line exactly where fb lies below fa). `flip` turns fb's into fa's sense.
    let (mut above, mut below) = (false, false);
    for (face, own_plane, other_plane, flip) in [(fa, pa, pb, 1.0), (fb, pb, pa, -1.0)] {
        k.check_cancelled(OP_STRIP)?;
        let Some(hulls) = boundary_hulls(k, brep, face, own_plane, sine)? else {
            continue;
        };
        let within = |sigma: f64| {
            hulls
                .iter()
                .all(|hull| hull_within_band_on_side(hull, other_plane, flip * sigma, tol))
        };
        above |= within(1.0);
        below |= within(-1.0);
    }
    Ok(above && below)
}

/// One edge's hull points, each with the error bound of its coordinates.
type Hull = Vec<(Point3, f64)>;

/// The hull points of every boundary edge of `face`, inner loops included, one list per
/// edge, each with a bound on how far the face's own boundary near it can be in the
/// distance to the other plane ([`boundary_slack`]: the coordinates' error, the point's
/// offset from the face's own plane `own`, and the edge's zone tolerance across the
/// crossing, `sine` being the sine of the planes' angle); `None` when the face has no edges
/// or an edge has no curve, no interval or no certain hull (a curve kind whose evaluation
/// is not its control polygon's): the face then never qualifies.
///
/// # Errors
///
/// Those of the topology walk and the zone tolerances.
fn boundary_hulls<O: ClassObject>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    face: O,
    own: PlanePos,
    sine: f64,
) -> Result<Option<Vec<Hull>>> {
    let mut out = Vec::new();
    for edge in brep.face_edges(face)? {
        let edge_zone = object_zone_tol_3d(k, brep, Some(edge))?;
        let Some(ivl) = brep.edge_interval(edge) else {
            return Ok(None);
        };
        let Some(curve) = brep.edge_curve(edge).flatten().and_then(|c| brep.curve(c)) else {
            return Ok(None);
        };
        let Some(hull) = curve.hull_points_over(&ivl) else {
            return Ok(None);
        };
        out.push(
            hull.into_iter()
                .map(|(q, e)| (q, boundary_slack(own, sine, edge_zone, q, e)))
                .collect(),
        );
    }
    Ok((!out.is_empty()).then_some(out))
}

/// How far, in the distance to the other plane, the face's own boundary can be from the hull
/// point `q` of one of its edges (coordinates within `e`): the face is the region of its own
/// plane `own` its trims bound, and the edge's 3D curve lies within the edge's zone
/// tolerance `edge_zone` of them (validity invariant: an edge is within its tolerance of
/// its faces' trims). A boundary point `p` near `q` lies in `own`, so `p - q` has the
/// component `-h` along `own`'s normal (`h` the signed distance of `q` from `own`, with its
/// rounding) and an in-plane part no longer than `edge_zone + e`. The other plane's unit
/// normal has a component of length `sine` in `own`'s plane and one of at most 1 along its
/// normal, so the distance changes by at most `|h| + sine (edge_zone + e)`; with `q`'s own
/// error `e` that is the slack. A grown edge tolerance, or an edge off its face's plane,
/// widens it. NaN propagates (and refuses where it is compared).
fn boundary_slack(own: PlanePos, sine: f64, edge_zone: f64, q: Point3, e: f64) -> f64 {
    let (h, h_rounding) = own.signed_distance(q);
    (e + h.abs() + h_rounding + sine * (edge_zone + e)) * (1.0 + gamma(4))
}

/// Whether every hull point of a boundary edge (`hull`, each with the error bound of its
/// coordinates) lies strictly below `tol` in the signed distance `sense · g` from `plane`,
/// with the point's error and the distance's rounding added: the edge lies in the convex
/// hull of these points and `g` is affine in the point, so the edge's points do too. Each
/// point is compared on its own and the comparison is written so that a NaN distance or
/// error refuses (no `f64::max`, which drops a NaN).
fn hull_within_band_on_side(hull: &[(Point3, f64)], plane: PlanePos, sense: f64, tol: f64) -> bool {
    hull.iter().all(|&(q, e)| {
        let (g, rounding) = plane.signed_distance(q);
        sense * g + e + rounding < tol
    })
}

/// Whether `face` (on its own plane `own`) reaches strictly above `plane` and strictly
/// below it, judged at its edge ends ([`beside`]): each end is a point of the edge curve,
/// which the face's own boundary follows only to the edge's zone tolerance and the end's
/// offset from `own`, so an end counts on a side only past that slack ([`boundary_slack`],
/// `sine` being the sine of the planes' angle) and the distance's rounding. An edge end
/// that lies on the other plane within its own tolerance, as along an edge two faces
/// share, counts on neither side. The face points so found lie on both sides, and the
/// face's interior, which is connected, reaches both: the crossing line runs through it.
///
/// # Errors
///
/// Those of the topology walk, the zone tolerances and the end evaluations.
fn crosses_plane<O: ClassObject>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    face: O,
    own: PlanePos,
    plane: PlanePos,
    sine: f64,
) -> Result<bool> {
    let (mut above, mut below) = (false, false);
    for edge in brep.face_edges(face)? {
        let (Some(curve), Some(ivl)) = (
            brep.edge_curve(edge).flatten().and_then(|c| brep.curve(c)),
            brep.edge_interval(edge),
        ) else {
            continue;
        };
        let edge_zone = object_zone_tol_3d(k, brep, Some(edge))?;
        for t in [ivl.min(), ivl.max()] {
            let q = curve.evaluate_point(k, t)?;
            let (up, down) = beside(own, plane, sine, edge_zone, q);
            above |= up;
            below |= down;
        }
        if above && below {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the face near its edge end `q` (an edge of zone tolerance `edge_zone`, the face
/// on plane `own`) lies strictly above `plane`, and whether strictly below: the signed
/// distance must pass the end's [`boundary_slack`] (with the evaluated point's error,
/// a few units in the last place of `|q|`) and the distance's rounding. NaN counts on
/// neither side.
fn beside(own: PlanePos, plane: PlanePos, sine: f64, edge_zone: f64, q: Point3) -> (bool, bool) {
    let (g, rounding) = plane.signed_distance(q);
    let margin = boundary_slack(own, sine, edge_zone, q, gamma(8) * q.length()) + rounding;
    (g > margin, g < -margin)
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

/// The contact points of the coincident strips among the face pairs' curve intervals
/// `intersections` (each a face pair, whether its surfaces coincide, its intersection and its
/// single points), before any of their proto vertices is made (module docs, "The strip's
/// ends"). Each
/// interval that will become a proto edge (its ends and middle imprintable in both B-reps,
/// its middle on no vertex) is read as an edge definition of tolerance `tol` (the manager's,
/// which a new proto edge's definition gets); for each crossing in a strip, each end of it
/// is one contact: the segment across the strip at that end, from one side's bounding edge
/// to the other's, widened by the tolerance within which the sides cover the crossing.
///
/// # Errors
///
/// Those of the curve evaluations and of the strip test; [`SolidError::InvalidInput`] for a
/// classification whose curve cannot be resolved.
pub(super) fn strip_contacts<O: ClassObject + SolutionObject + Ord>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    intersections: &[FacePairIntersection<O>],
    tol: f64,
) -> Result<Vec<DeclaredContact>> {
    k.check_cancelled(OP_STRIP_CONTACTS)?;
    let tol = zone_tol3d(k, tol);
    let mut defs = Vec::new();
    for (_, _, _, fi, _) in intersections {
        for (cc1, cc2) in fi.cc1.iter().zip(&fi.cc2) {
            let curve = cc1.curve().and_then(|c| brep.curve(c)).ok_or_else(|| {
                SolidError::invalid_input(OP_STRIP_CONTACTS, "curve handle cannot be resolved")
            })?;
            for ll in 0..cc1.size() {
                let (ci1, ci2) = (cc1.at(ll)?, cc2.at(ll)?);
                let imprintable = [ci1, ci2].iter().all(|ci| {
                    can_imprint(ci.start.point_class())
                        && can_imprint(ci.mid.point_class())
                        && can_imprint(ci.end.point_class())
                        && ci.mid.point_class() != PointClassType::Vertex
                });
                if !imprintable {
                    continue;
                }
                let ivl = ci1.interval;
                defs.push(Def {
                    id: defs.len(),
                    src: [
                        (ci1.mid.point_class(), ci1.mid.object()),
                        (ci2.mid.point_class(), ci2.mid.object()),
                    ],
                    ends: [
                        curve.evaluate_point(k, ivl.min())?,
                        curve.evaluate_point(k, ivl.max())?,
                    ],
                    hull: curve.hull_points_over(&ivl),
                    tol,
                });
            }
        }
    }
    let mut contacts = Vec::new();
    for crossing in &defs {
        let Some(strip) = in_coincident_strip(k, brep, crossing, &defs)? else {
            continue;
        };
        for end in crossing.ends {
            if let Some([a, b]) = strip.closed {
                contacts.push(DeclaredContact {
                    a: end + a.off,
                    b: end + b.off,
                    radius: a.end_tol.max(b.end_tol),
                });
            } else {
                for side in &strip.sides {
                    contacts.push(DeclaredContact {
                        a: end,
                        b: end + side.off,
                        radius: side.end_tol,
                    });
                }
            }
        }
    }
    Ok(contacts)
}

/// Whether `face`'s interior along `edge` lies toward the crossing line (`off` points from
/// the crossing line to `edge`, so `face`'s inward binormal at `edge`'s midpoint must dot
/// negatively with `off`).
fn faces_inward<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    edge: O,
    face: O,
    off: Vec3,
) -> Result<bool> {
    let Some(eu) = brep.edge_edgeuse_of_face(edge, face)? else {
        return Ok(false);
    };
    let Some(ivl) = brep.edge_interval(edge) else {
        return Ok(false);
    };
    let Some(binorm) = ignore_soft(brep.edgeuse_evaluate_binormal(k, eu, ivl.mid()))? else {
        return Ok(false);
    };
    Ok(binorm.is_initialized() && binorm.dot(off) < 0.0)
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

/// The strip sides `face` closes, when it has a boundary edge within intersection tolerance
/// of `other_plane` that runs parallel to and covers the crossing line `(p0, dir, len)` on
/// the side opposite to one of `opposite_defs`, with `face`'s interior toward the
/// crossing: that edge's side and the opposite one.
#[allow(clippy::too_many_arguments)]
fn face_edge_bounds_strip<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    face: O,
    other_plane: PlanePos,
    other_face_tol: f64,
    p0: Point3,
    dir: Vec3,
    len: f64,
    crossing_tol: f64,
    opposite_defs: &[Side],
) -> Result<Option<[Side; 2]>> {
    for edge in brep.face_edges(face)? {
        let Some(ivl) = brep.edge_interval(edge) else {
            continue;
        };
        let (ends, hull) = {
            let Some(curve) = brep.edge_curve(edge).flatten().and_then(|c| brep.curve(c)) else {
                continue;
            };
            let Some(hull) = curve.hull_points_over(&ivl) else {
                continue;
            };
            (
                [
                    curve.evaluate_point(k, ivl.min())?,
                    curve.evaluate_point(k, ivl.max())?,
                ],
                hull,
            )
        };
        let edge_tol = zone_tol3d(k, brep.topology_tolerance(edge).unwrap_or(0.0));
        let plane_tol = xsect_tol3d(k, edge_tol + other_face_tol).max(crossing_tol);
        if !hull
            .iter()
            .all(|&(q, e)| other_plane.distance_bound(q, e) <= plane_tol)
        {
            continue;
        }
        let end_tol = plane_tol + edge_end_tol(k, brep, edge)?;
        let Some(off) = band_offset(p0, dir, len, ends, &hull, crossing_tol, end_tol) else {
            continue;
        };
        let Some(&opposite) = opposite_defs.iter().find(|other| off.dot(other.off) < 0.0) else {
            continue;
        };
        if faces_inward(k, brep, edge, face, off)? {
            return Ok(Some([Side { off, end_tol }, opposite]));
        }
    }
    Ok(None)
}

/// The strip about `crossing`, a face/face edge definition of two planes (module docs): the
/// sides given by edge-on-face definitions in `defs` (an edge of either face within
/// tolerance of the other plane, parallel to and covering the crossing, the face's interior
/// toward it), and the two sides that close it when edges of the two faces bound it on both
/// sides; `None` when there is no such side.
fn in_coincident_strip<O: ClassObject, I: PartialEq>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    crossing: &Def<O, I>,
    defs: &[Def<O, I>],
) -> Result<Option<Strip>> {
    let [(PointClassType::Face, Some(fa)), (PointClassType::Face, Some(fb))] = crossing.src else {
        return Ok(None);
    };
    let (Some(pa), Some(pb)) = (plane_pos(brep, fa), plane_pos(brep, fb)) else {
        return Ok(None);
    };
    let [p0, p1] = crossing.ends;
    let axis = p1 - p0;
    let len = axis.length();
    if len <= crossing.tol {
        return Ok(None);
    }
    let dir = axis / len;
    // the crossing itself runs within its tolerance of its chord over its whole interval
    let Some(crossing_hull) = crossing.hull.as_deref() else {
        return Ok(None);
    };
    if !crossing_hull
        .iter()
        .all(|&(q, e)| offset_bound(p0, dir, Vec3::ZERO, q, e) <= crossing.tol)
    {
        return Ok(None);
    }
    let mut def_on_fb: Vec<Side> = Vec::new();
    let mut def_on_fa: Vec<Side> = Vec::new();
    for def in defs.iter().filter(|d| d.id != crossing.id) {
        let edge_side = match def.src {
            [(PointClassType::Edge, Some(e)), (PointClassType::Face, Some(f))]
                if f == fb && brep.edge_is_connected_to_face(e, fa)? =>
            {
                Some((true, e, fa))
            }
            [(PointClassType::Face, Some(f)), (PointClassType::Edge, Some(e))]
                if f == fa && brep.edge_is_connected_to_face(e, fb)? =>
            {
                Some((false, e, fb))
            }
            _ => None,
        };
        let Some((is_a_on_fb, edge, owner_face)) = edge_side else {
            continue;
        };
        let Some(hull) = def.hull.as_deref() else {
            continue;
        };
        let end_tol = crossing.tol.max(def.tol) + edge_end_tol(k, brep, edge)?;
        let Some(off) = band_offset(p0, dir, len, def.ends, hull, crossing.tol, end_tol) else {
            continue;
        };
        if !faces_inward(k, brep, edge, owner_face, off)? {
            continue;
        }
        if is_a_on_fb {
            def_on_fb.push(Side { off, end_tol });
        } else {
            def_on_fa.push(Side { off, end_tol });
        }
    }
    if def_on_fb.is_empty() && def_on_fa.is_empty() {
        return Ok(None);
    }
    let mut strip = Strip {
        sides: def_on_fa.iter().chain(&def_on_fb).copied().collect(),
        closed: None,
    };
    for &a in &def_on_fb {
        if let Some(&b) = def_on_fa.iter().find(|b| a.off.dot(b.off) < 0.0) {
            strip.closed = Some([a, b]);
            return Ok(Some(strip));
        }
    }
    let fa_tol = zone_tol3d(k, brep.topology_tolerance(fa).unwrap_or(0.0));
    let fb_tol = zone_tol3d(k, brep.topology_tolerance(fb).unwrap_or(0.0));
    if def_on_fa.is_empty() {
        // no side to close
    } else if let Some(sides) = face_edge_bounds_strip(
        k,
        brep,
        fa,
        pb,
        fb_tol,
        p0,
        dir,
        len,
        crossing.tol,
        &def_on_fa,
    )? {
        strip.closed = Some(sides);
        return Ok(Some(strip));
    }
    if def_on_fb.is_empty() {
        // no side to close
    } else if let Some(sides) = face_edge_bounds_strip(
        k,
        brep,
        fb,
        pa,
        fa_tol,
        p0,
        dir,
        len,
        crossing.tol,
        &def_on_fb,
    )? {
        strip.closed = Some(sides);
        return Ok(Some(strip));
    }
    Ok(Some(strip))
}

/// The offset from the crossing line (through `p0` along unit `dir`, `len` long) of the
/// curve with ends `[q0, q1]` and certified hull `hull` (points with the error bounds of
/// their coordinates), when every point of the curve lies within `tol` of the offset of the
/// ends' midpoint (an enclosure over `hull`, [`offset_bound`]), the ends' offsets are within
/// `tol` of each other, the curve covers `[0, len]` within `end_tol` and lies farther from
/// the line than `tol`; else `None`.
fn band_offset(
    p0: Point3,
    dir: Vec3,
    len: f64,
    [q0, q1]: [Point3; 2],
    hull: &[(Point3, f64)],
    tol: f64,
    end_tol: f64,
) -> Option<Vec3> {
    let (s0, s1) = ((q0 - p0).dot(dir), (q1 - p0).dot(dir));
    let r0 = (q0 - p0) - dir * s0;
    let r1 = (q1 - p0) - dir * s1;
    let off = (r0 + r1) / 2.0;
    let parallel = (r0 - r1).length() <= tol
        && hull
            .iter()
            .all(|&(q, e)| offset_bound(p0, dir, off, q, e) <= tol);
    let covers = s0.min(s1) <= end_tol && s0.max(s1) >= len - end_tol;
    (parallel && covers && off.length() > tol).then_some(off)
}

/// An upper bound of `|r(q) - off|`, with `r(q) = (q - p0) - dir ((q - p0) . dir)` the
/// offset of `q` from the line through `p0` along `dir` (unit within rounding), for the
/// point `q` whose coordinates are within `e` of it. `r` is affine in `q` and the norm is
/// convex, so the bound over the points of a convex hull bounds it over the hull. Rounding:
/// the difference `w` rounds by `γ_1 |w|`, the dot by `γ_3 |w|`, the product and the
/// subtraction by `γ_2 |w|` more; a coordinate error `e` moves `r` by at most `2 √3 e`; the
/// final norm rounds by `γ_4`. `NaN` propagates, so a caller comparing with `<=` refuses it.
fn offset_bound(p0: Point3, dir: Vec3, off: Vec3, q: Point3, e: f64) -> f64 {
    let w = q - p0;
    let r = w - dir * w.dot(dir);
    ((r - off).length() + 4.0 * gamma(8) * w.length() + 4.0 * e) * (1.0 + gamma(4))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::geom::extent1d::Extent1d;
    use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

    const TOL: f64 = 1e-5;
    const LEN: f64 = 10.0;
    const OFF: f64 = 5e-6;

    fn cubic(points: &[Point3; 4]) -> Curve {
        Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 3,
                control_points: points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[4, 4],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        )
    }

    /// Whether `curve` over `[0, 1]` lies within the band of the plane y = 0 on its side
    /// above (`.0`) and below (`.1`), by its certified hull.
    fn sides_of(curve: &Curve) -> Option<(bool, bool)> {
        let hull = curve.hull_points_over(&Extent1d::from_min_max(0.0, 1.0))?;
        let plane = PlanePos {
            origin: Point3::ZERO,
            unit_normal: Vec3::Y,
        };
        Some((
            hull_within_band_on_side(&hull, plane, 1.0, TOL),
            hull_within_band_on_side(&hull, plane, -1.0, TOL),
        ))
    }

    /// From tangency-labs/solidcraft#16146: a boundary whose start, midpoint and end lie in
    /// the band but which bows far off it in between is no bound (the samples pass, the
    /// enclosure does not), so the crossing is kept.
    #[test]
    fn a_bowed_boundary_is_no_bound() {
        let k = Kernel::default();
        let h = 100.0 * TOL;
        let curve = cubic(&[
            Point3::new(0.0, OFF, 0.0),
            Point3::new(LEN / 3.0, OFF + h, 0.0),
            Point3::new(2.0 * LEN / 3.0, OFF - h, 0.0),
            Point3::new(LEN, OFF, 0.0),
        ]);
        for t in [0.0, 0.5, 1.0] {
            let q = curve.evaluate_point(&k, t).unwrap();
            assert!(q.y.abs() < TOL, "sample {t} is in the band");
        }
        let t = (3.0 - 3.0_f64.sqrt()) / 6.0;
        let bulge = curve.evaluate_point(&k, t).unwrap().y - OFF;
        assert!(bulge > 25.0 * TOL, "bulge {bulge}");
        assert_eq!(sides_of(&curve), Some((false, false)));
    }

    /// From tangency-labs/solidcraft#16146: a B-spline whose control points lie on one line in
    /// the band, and a line segment, bound the strip, each hull point within the band.
    #[test]
    fn a_straight_spline_or_line_bounds_the_strip() {
        let straight = cubic(&[
            Point3::new(0.0, OFF, 0.0),
            Point3::new(LEN / 4.0, OFF, 0.0),
            Point3::new(LEN / 2.0, OFF, 0.0),
            Point3::new(LEN, OFF, 0.0),
        ]);
        let line = Curve::from(
            Line::create_line_segment(
                3,
                Point3::new(0.0, OFF, 0.0),
                Point3::new(LEN, OFF, 0.0),
                None,
            )
            .unwrap(),
        );
        for curve in [straight, line] {
            assert_eq!(sides_of(&curve), Some((true, true)));
        }
    }

    /// A hull point with a NaN coordinate, or a NaN error bound, never lies in the band:
    /// the comparison refuses NaN instead of dropping it as `f64::max` would.
    #[test]
    fn a_nan_hull_point_is_no_bound() {
        let plane = PlanePos {
            origin: Point3::ZERO,
            unit_normal: Vec3::Y,
        };
        let good = (Point3::new(0.0, OFF, 0.0), 0.0);
        for bad in [
            (Point3::new(1.0, f64::NAN, 0.0), 0.0),
            (Point3::new(1.0, OFF, 0.0), f64::NAN),
        ] {
            for sense in [1.0, -1.0] {
                assert!(hull_within_band_on_side(&[good], plane, sense, TOL));
                assert!(!hull_within_band_on_side(&[good, bad], plane, sense, TOL));
            }
        }
    }

    /// The face's boundary is in its own plane, within the edge's tolerance of the edge
    /// curve: a hull point 5e-6 from the other plane is in the band of 1e-5 when the edge
    /// lies on its face's plane with the default tolerance, but not when the edge sits
    /// 1.5e-5 off its face's plane, nor when its tolerance has grown to 1e-4 across a
    /// crossing of sine 0.1. Either may hide a real crossing.
    /// An edge end on the other plane within the edge's own tolerance, as along an edge two
    /// faces share, counts on neither side; past the edge's tolerance (times the planes'
    /// sine) and its offset from the face's own plane, it counts on its side. The rounding
    /// alone, as before, counted the first.
    #[test]
    fn an_edge_end_within_its_tolerance_of_the_other_plane_is_on_neither_side() {
        let other = PlanePos {
            origin: Point3::new(0.0, 0.0, 0.0),
            unit_normal: Vec3::new(0.0, 1.0, 0.0),
        };
        let own = |off: f64| PlanePos {
            origin: Point3::new(0.0, 0.0, off),
            unit_normal: Vec3::new(0.0, 0.0, 1.0),
        };
        let q = |y: f64| Point3::new(1.0, y, 0.0);
        let (sine, zone) = (1.0, 1e-7);
        assert_eq!(beside(own(0.0), other, sine, zone, q(5e-8)), (false, false));
        assert_eq!(
            beside(own(0.0), other, sine, zone, q(-5e-8)),
            (false, false)
        );
        assert_eq!(beside(own(0.0), other, sine, zone, q(2e-7)), (true, false));
        assert_eq!(beside(own(0.0), other, sine, zone, q(-2e-7)), (false, true));
        // the end 2e-7 off the face's own plane: its 2e-7 above no longer counts
        assert_eq!(
            beside(own(2e-7), other, sine, zone, q(2e-7)),
            (false, false)
        );
        assert_eq!(
            beside(own(0.0), other, sine, f64::NAN, q(1.0)),
            (false, false)
        );
    }

    #[test]
    fn an_edge_off_its_plane_or_with_a_grown_tolerance_widens_the_bound() {
        let other = PlanePos {
            origin: Point3::ZERO,
            unit_normal: Vec3::Y,
        };
        let own = |off: f64| PlanePos {
            origin: Point3::new(0.0, 0.0, off),
            unit_normal: Vec3::Z,
        };
        let q = Point3::new(1.0, 5e-6, 0.0);
        let sine = 0.1;
        let slack = |off: f64, edge_zone: f64| boundary_slack(own(off), sine, edge_zone, q, 0.0);
        let inside = |s: f64| hull_within_band_on_side(&[(q, s)], other, 1.0, TOL);
        assert!(inside(slack(0.0, 1e-5)));
        assert!(!inside(slack(1.5e-5, 1e-5)));
        assert!(!inside(slack(0.0, 1e-4)));
        assert!(boundary_slack(own(0.0), sine, f64::NAN, q, 0.0).is_nan());
    }

    /// A strip side ([`band_offset`]) 1e-3 off the crossing along x, `[0, LEN]`.
    const SIDE_OFF: f64 = 1e-3;

    /// The band offset of `curve` over `[0, 1]` from the crossing along x, `[0, LEN]`.
    fn offset_of(k: &Kernel, curve: &Curve) -> Option<Vec3> {
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let ends = [
            curve.evaluate_point(k, 0.0).unwrap(),
            curve.evaluate_point(k, 1.0).unwrap(),
        ];
        let hull = curve.hull_points_over(&ivl)?;
        band_offset(Point3::ZERO, Vec3::X, LEN, ends, &hull, TOL, TOL)
    }

    /// A strip side whose start, midpoint and end lie on the band `y = SIDE_OFF` but which
    /// bows a hundred tolerances off it in between is no side: the samples pass, the
    /// enclosure over its control points does not.
    #[test]
    fn a_bowed_boundary_is_no_strip_side() {
        let k = Kernel::default();
        let h = 100.0 * TOL;
        let curve = cubic(&[
            Point3::new(0.0, SIDE_OFF, 0.0),
            Point3::new(LEN / 3.0, SIDE_OFF + h, 0.0),
            Point3::new(2.0 * LEN / 3.0, SIDE_OFF - h, 0.0),
            Point3::new(LEN, SIDE_OFF, 0.0),
        ]);
        for t in [0.0, 0.5, 1.0] {
            let q = curve.evaluate_point(&k, t).unwrap();
            assert!((q.y - SIDE_OFF).abs() <= TOL, "sample {t} is on the band");
        }
        assert_eq!(offset_of(&k, &curve), None);
    }

    /// A B-spline whose control points lie on the band, and a line segment, are strip sides.
    #[test]
    fn a_straight_spline_or_line_is_a_strip_side() {
        let k = Kernel::default();
        let straight = cubic(&[
            Point3::new(0.0, SIDE_OFF, 0.0),
            Point3::new(LEN / 4.0, SIDE_OFF, 0.0),
            Point3::new(LEN / 2.0, SIDE_OFF, 0.0),
            Point3::new(LEN, SIDE_OFF, 0.0),
        ]);
        let off = offset_of(&k, &straight).unwrap();
        assert!((off.y - SIDE_OFF).abs() <= TOL && off.x.abs() <= TOL);
        let line = Curve::from(
            Line::create_line_segment(
                3,
                Point3::new(0.0, SIDE_OFF, 0.0),
                Point3::new(LEN, SIDE_OFF, 0.0),
                None,
            )
            .unwrap(),
        );
        let off = offset_of(&k, &line).unwrap();
        assert!((off.y - SIDE_OFF).abs() <= TOL);
    }
}
