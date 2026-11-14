//! Not upstream (WR-614, tangency-labs/solidcore#1794): the crossing line of two
//! planes that stay within tolerance of each other across a strip bounded by their edges
//! is not an edge.
//!
//! Two planar faces `fa` (B-rep 1) and `fb` (B-rep 2) crossing at a small angle meet in a
//! line fixed only to `tol / sin(angle)`. When one face has an edge lying on the other
//! (an edge-on-face edge definition, within tolerance by construction), parallel to the
//! crossing, covering it, farther from it than the tolerance and with the face's interior
//! toward the crossing, and on the opposite side of the crossing the other face ends at an
//! edge likewise parallel to, covering and facing the crossing within the edge/face
//! intersection tolerance (either as a second edge-on-face definition or as a boundary
//! edge within `GetXSectTol3d(edge, plane_face)`), the two planes stay within tolerance of
//! each other across the whole strip where both faces exist (the gap between two planes is
//! linear across it). Upstream still makes the crossing an edge definition: its end
//! vertices are picked per B-rep (`SmProtoVertex::GetFinalVertex`) and never related by
//! `SetRelationships`, so `SmMerge::MakeFaceuse` fails on the edge's inconsistent vertex
//! mates (`SM_ERR`). Removing the crossing's edge definition before `Resolve` (and its
//! proto edge when that is left without one) leaves the strip bounded by the existing
//! edges, while keeping crossings where either face continues out of tolerance past the
//! crossing.
//!
//! Every geometric condition is decided over the whole curve, never from samples: a curve
//! enters the test only as a line segment or a B-spline, whose points over its interval lie in
//! the convex hull of its two ends or of the control points of the knot spans that meet the
//! interval (`Curve::hull_points_over`). The offset from the crossing line and the distance from
//! a plane are affine in the point (up to the absolute value or norm, which are convex), so
//! their bound over those points, widened by the rounding of the points and of the arithmetic,
//! bounds them over the curve. A curve of any other kind, or a hull that leaves the band,
//! keeps the crossing.
//!
//! The strip's ends: the crossing is fixed only to the strip, so at each end of it the
//! crossing's end and the ends of the edges bounding the strip are one contact point
//! (docs/booleans.md, "One point or two"), though they lie farther apart than their
//! tolerances. [`strip_contacts`] reads the strip from the face pairs' curve intervals before
//! any of their proto vertices is made and declares each end a contact: the segment across the strip,
//! widened by the tolerance within which its edges cover the crossing. Where an edge bounds
//! the crossing on one side only (the other face goes on out of tolerance), the half strip
//! between them is such a contact too, and the crossing stays an edge.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `TopologyIntersector::drop_crossings_in_coincident_strips` (crate) | none (own design) |
//! | [`strip_contacts`] (super) | none (own design) |
//!
//! Deviations: not upstream at all; always on (its opt-in flag was retired).

use crate::error::{Result, SolidError};
use crate::geom::solution_array::SolutionObject;
use crate::geom::tol::{xsect_tol3d, zone_tol3d};
use crate::intersect::point_class::{ClassObject, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::math::gamma;
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::proto_topology::{DeclaredContact, EdgeDefId, ProtoEdgeId};

use super::p1::{can_imprint, ignore_soft, FaceIntersection, TopologyIntersector};
use super::p2::IntersectorBrepP2;

const OP_DROP_STRIP: &str =
    "boolean::topology_intersector::coincident_strip::drop_crossings_in_coincident_strips";
const OP_STRIP_CONTACTS: &str = "boolean::topology_intersector::coincident_strip::strip_contacts";

/// An edge definition (or a curve interval of a face pair that becomes one) as the strip
/// test reads it: what it is (`id`: its proto edge and edge definition, or its index), its
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

impl<O: ClassObject + SolutionObject + Ord> TopologyIntersector<O> {
    /// Removes every face/face edge definition of two planes whose crossing lies in a
    /// strip bounded on both sides by edges of the two faces within tolerance of the
    /// opposite plane (module docs), and every proto edge that is left without an edge
    /// definition, and returns the curves deleted with them.
    ///
    /// # Errors
    ///
    /// Those of the curve and binormal evaluations and the face/edge connection tests.
    pub(crate) fn drop_crossings_in_coincident_strips(
        &mut self,
        k: &Kernel,
        brep: &mut impl IntersectorBrepP2<O>,
    ) -> Result<Vec<O>> {
        k.check_cancelled(OP_DROP_STRIP)?;
        let Some(mgr) = self.proto_topo_mgr.as_mut() else {
            return Ok(Vec::new());
        };
        let mut defs = Vec::new();
        for &pe in mgr.proto_edges() {
            let Some(edge) = mgr.proto_edge(pe) else {
                continue;
            };
            for &ed in edge.edge_definitions() {
                let Some(def) = mgr.edge_def(ed) else {
                    continue;
                };
                let Some(curve) = def.xsect_curve_3d().and_then(|c| brep.curve(c)) else {
                    continue;
                };
                let ivl = def.xsect_interval();
                defs.push(Def {
                    id: (pe, ed),
                    src: [
                        (def.src_topo_type(1), def.src_topo(1)),
                        (def.src_topo_type(2), def.src_topo(2)),
                    ],
                    ends: [
                        curve.evaluate_point(k, ivl.min())?,
                        curve.evaluate_point(k, ivl.max())?,
                    ],
                    hull: curve.hull_points_over(&ivl),
                    tol: def.tolerance(k),
                });
            }
        }
        let mut doomed: Vec<(ProtoEdgeId, EdgeDefId)> = Vec::new();
        for crossing in &defs {
            if in_coincident_strip(k, brep, crossing, &defs)?.is_some_and(|s| s.closed.is_some()) {
                doomed.push(crossing.id);
            }
        }
        let mut deleted = Vec::new();
        for (pe, ed) in doomed {
            if let Some(edge) = mgr.proto_edge_mut(pe) {
                ignore_soft(edge.remove_edge_definition(ed))?;
            }
            deleted.extend(mgr.delete_edge_definition(ed));
            if mgr
                .proto_edge(pe)
                .is_some_and(|e| e.num_edge_definitions() == 0)
            {
                ignore_soft(mgr.remove_proto_edge(k, pe))?;
                mgr.forget_proto_edge(pe);
                deleted.extend(mgr.delete_proto_edge_object(pe));
            }
        }
        Ok(deleted)
    }
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
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::geom::extent1d::Extent1d;
    use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

    const TOL: f64 = 1e-5;
    const LEN: f64 = 10.0;
    const OFF: f64 = 1e-3;

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

    /// A boundary whose start, midpoint and end lie on the band `y = OFF` but which bows a
    /// hundred tolerances off it in between: the three samples the test once read pass, the
    /// enclosure over its control points does not, so the crossing is kept.
    #[test]
    fn a_bowed_boundary_keeps_the_crossing() {
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
            assert!((q.y - OFF).abs() <= TOL, "sample {t} is on the band");
        }
        // the bulge at t = (3 - sqrt 3) / 6 is h / (6 sqrt 3) off the band
        let t = (3.0 - 3.0_f64.sqrt()) / 6.0;
        let bulge = curve.evaluate_point(&k, t).unwrap().y - OFF;
        assert!(bulge > 25.0 * TOL, "bulge {bulge}");
        assert_eq!(offset_of(&k, &curve), None);
    }

    /// A B-spline whose control points lie on the band, and a line segment, bound the strip.
    #[test]
    fn a_straight_spline_or_line_bounds_the_strip() {
        let k = Kernel::default();
        let straight = cubic(&[
            Point3::new(0.0, OFF, 0.0),
            Point3::new(LEN / 4.0, OFF, 0.0),
            Point3::new(LEN / 2.0, OFF, 0.0),
            Point3::new(LEN, OFF, 0.0),
        ]);
        let off = offset_of(&k, &straight).unwrap();
        assert!((off.y - OFF).abs() <= TOL && off.x.abs() <= TOL);
        let line = Curve::from(
            Line::create_line_segment(
                3,
                Point3::new(0.0, OFF, 0.0),
                Point3::new(LEN, OFF, 0.0),
                None,
            )
            .unwrap(),
        );
        let off = offset_of(&k, &line).unwrap();
        assert!((off.y - OFF).abs() <= TOL);
    }
}
