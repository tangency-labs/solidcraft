//! Not upstream (#15718, replacing WR-614's removal, tangency-labs/solidcore#1794): the
//! crossing line of two planar faces that lie within the intersection tolerance of each
//! other wherever both exist beside it is no event, decided from the operands' geometry
//! before the face pair is intersected.
//!
//! Two planes crossing at a small angle θ meet in a line fixed only to `tol / sin θ`.
//! The signed distance to a plane is affine in the point: zero on the crossing line,
//! growing linearly to either side. The rule is for a crossing through both faces: each
//! has a vertex strictly on either side of it. Two faces that meet only along their
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
//! | Rust | Upstream |
//! |------|----------|
//! | `crossing_inside_coincident_pair` (crate) | none (own design) |
//!
//! Deviations: not upstream at all; always on. Planes only (two planar faces whose
//! boundary edges are lines or B-splines); every other pair is intersected as upstream
//! does.

use crate::error::Result;
use crate::geom::tol::xsect_tol_3d_from_zones;
use crate::intersect::point_class::{object_zone_tol_3d, ClassObject};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::math::gamma;
use crate::surface::surface::p2::SurfaceKind;

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
    // Each face crosses the line: one of its vertices lies strictly above the other face's
    // plane and one strictly below (beyond their rounding). Faces that only meet the line
    // along their boundary, as two faces meeting along a shared edge from opposite sides,
    // have no piece on both sides of it: there is no coincident pair, whatever the angle.
    if !crosses_plane(k, brep, fa, pb)? || !crosses_plane(k, brep, fb, pa)? {
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

/// Whether `face` has an edge end strictly above `plane` and one strictly below, each
/// beyond the rounding of its signed distance. The ends are points of the face, so its
/// interior, which is connected, reaches both sides and the crossing line runs through it.
fn crosses_plane<O: ClassObject>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    face: O,
    plane: PlanePos,
) -> Result<bool> {
    let (mut above, mut below) = (false, false);
    for edge in brep.face_edges(face)? {
        let (Some(curve), Some(ivl)) = (
            brep.edge_curve(edge).flatten().and_then(|c| brep.curve(c)),
            brep.edge_interval(edge),
        ) else {
            continue;
        };
        for t in [ivl.min(), ivl.max()] {
            let (g, rounding) = plane.signed_distance(curve.evaluate_point(k, t)?);
            above |= g > rounding;
            below |= g < -rounding;
        }
        if above && below {
            return Ok(true);
        }
    }
    Ok(false)
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
}
