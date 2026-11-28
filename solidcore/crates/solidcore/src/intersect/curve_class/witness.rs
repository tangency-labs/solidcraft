//! Not upstream (WR-647 part A, #18064): the face/face tangency witness. A face/face
//! intersection curve that approaches a boundary edge without crossing it, ending at the
//! edge's end vertex, touches the edge at that vertex. The curve classifier's tolerance
//! band sees a range there instead. The witness proves the contact is one point from the
//! surfaces themselves, and replaces the range by that point.
//!
//! Upstream: none.
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | `tangency_point` (crate)   | none     |
//!
//! The argument: the curve C lies on F ∩ G, where F is the classifying face and G the
//! other face of the intersection, and the edge E lies on F ∩ Z for each other face Z
//! of E. So C ∩ E ⊆ G ∩ Z ∩ E = K ∩ E, with K the intersection curves of G and Z. Where K
//! and E are lines and circles, K ∩ E is found from their carriers in closed form, within
//! the tolerance. When every such point on E's stretch of the range lies at one end
//! vertex V of the range and of E, the range is the single contact at V. The range keeps
//! that end's parameters as the point, so no parameter is solved for.
//!
//! The range is kept unchanged (the classifier's own answer) whenever the witness does
//! not apply or cannot decide:
//! - the classification has no tangency partner (only
//!   [`crate::topology::face::p3::face_intersect_breps_cached`] sets one);
//! - the solution is not a range of the curve on an edge, or the edge is a seam;
//! - the range does not meet the case-3e predicate on its own ends
//!   ([`Curve::approaches_without_crossing`]);
//! - the range is not at exactly one end vertex of E that the classification met (a
//!   range with both ends at vertices is kept);
//! - E has no other face, or the classifying face lies on both sides of it;
//! - G or a Z is not an analytic surface, or their intersection fails (any error but a
//!   cancellation, unported paths included);
//! - E or a curve of K is not a line or a circle, or a K curve overlaps E;
//! - a line runs along a circle's plane but is neither within the tolerance of the plane
//!   nor apart from it on one side over the whole stretch where it can reach the circle;
//! - two circles are tilted or offset so far that the flattened one strays more than the
//!   tolerance from the true one (off + 2 r sin θ, see `meet`);
//! - a near-parallel pair changes side along the stretch, or a point is not finite;
//! - the stretch is no longer than the chord 2 sqrt(2 R tol) along which two carriers
//!   with relative radius of curvature R stay within the tolerance where they touch;
//! - for some Z, no point of K ∩ E lies on the stretch, or one lies away from V.
//!
//! Each Z of a non-manifold edge is tested on its own, within the larger of the range's
//! tolerance and the intersection tolerance of E's and Z's zones.
//!
//! Deviations: not upstream.

use super::p1::CurveClassification;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{Solution, SolutionEnd, SolutionType};
use crate::geom::tol::xsect_tol_3d_from_zones;
use crate::geom::vector3d::Point3;
use crate::geom::vector3d::Vec3;
use crate::intersect::curve_class::p9::BoundaryEdge;
use crate::intersect::point_class::{ClassGeometry, ClassObject};
use crate::kernel::Kernel;
use crate::math::det;
use crate::math::EFF_ZERO;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::{Brep, FaceId, VertexId};

/// A line or circle carrier of a curve.
#[derive(Debug, Clone, Copy)]
enum Carrier {
    /// Through `point` along the unit `direction`.
    Line { point: Point3, direction: Vec3 },
    /// Centre `origin`, unit in-plane axes `x` and `y`, unit `normal`, and `radius`.
    Circle {
        origin: Point3,
        x: Vec3,
        y: Vec3,
        normal: Vec3,
        radius: f64,
    },
}

/// How a curve of K meets E's carrier.
enum Meet {
    /// The carriers are one within the tolerance: the range may be a real overlap.
    Overlap,
    /// Not decided by these carriers.
    Undecided,
    /// The points of E's carrier where they meet.
    Points(Vec<Point3>),
}

/// The line or circle carrying `curve` (a line, a circle, or a degree-1 spline of two
/// control points), `None` for any other curve.
fn carrier(curve: &Curve) -> Option<Carrier> {
    match curve.kind() {
        CurveKind::Line(l) => {
            let (len, direction) = l.line_vector().length_and_unit()?;
            (len > EFF_ZERO).then_some(Carrier::Line {
                point: l.line_point(),
                direction,
            })
        }
        CurveKind::BSpline(b) => {
            // a degree-1 spline of two control points is the segment between them, whatever
            // their weights (an intersection of planes and extrusions comes back so)
            if b.degree().ok()? != 1 || b.number_control_points().ok()? != 2 {
                return None;
            }
            let (points, _) = b.control_polygon().ok()?;
            let (&start, &end) = (points.first()?, points.get(1)?);
            let (len, direction) = (end - start).length_and_unit()?;
            (len > EFF_ZERO).then_some(Carrier::Line {
                point: start,
                direction,
            })
        }
        CurveKind::Ellipse(e) if e.is_circle() && e.x_radius() > EFF_ZERO => {
            let p = e.position();
            let (_, x) = p.x_axis().length_and_unit()?;
            let (_, y) = p.y_axis().length_and_unit()?;
            let (_, normal) = x.cross(y).length_and_unit()?;
            Some(Carrier::Circle {
                origin: p.origin(),
                x,
                y,
                normal,
                radius: e.x_radius(),
            })
        }
        _ => None,
    }
}

/// The foot of `point` on the line through `origin` along unit `direction`.
fn foot(origin: Point3, direction: Vec3, point: Point3) -> Point3 {
    origin + direction * (point - origin).dot(direction)
}

/// `points` as met points, or undecided where one of them is not finite.
fn finite(points: Vec<Point3>) -> Meet {
    if points
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
    {
        Meet::Points(points)
    } else {
        Meet::Undecided
    }
}

/// How the line (`point`, unit `direction`) meets the circle; `line_is_e` says which
/// carrier's points are returned.
///
/// Within the tolerance of tangent the true crossings are returned, not the touching
/// point: a line just inside the circle crosses it twice, as far apart as the depth
/// allows, and the caller decides whether both lie at the vertex.
fn line_circle(
    point: Point3,
    direction: Vec3,
    origin: Point3,
    normal: Vec3,
    radius: f64,
    line_is_e: bool,
    tol: f64,
    sin_ang: f64,
) -> Meet {
    let dn = direction.dot(normal);
    let on_circle = |x: Point3| -> Option<Point3> {
        let (len, u) = (x - origin).length_and_unit()?;
        (len > EFF_ZERO).then_some(origin + u * radius)
    };
    let project = |points: Vec<Point3>| -> Meet {
        if line_is_e {
            return finite(points);
        }
        let mut out = Vec::new();
        for p in points {
            match on_circle(p) {
                Some(c) => out.push(c),
                None => return Meet::Undecided,
            }
        }
        finite(out)
    };
    if dn.abs() <= sin_ang {
        // the line runs along the circle's plane. Only its part whose foot on the line
        // through the centre lies within r + tol of the centre's foot can come within tol
        // of the circle (any other point is farther than r + tol from the centre); over
        // that part its height h(s) = h(s0) + dn (s - s0) above the plane is linear, so its
        // two ends bound it. Within tol of the plane over all of it, the line is in the
        // plane up to that height; beyond tol on one side over all of it, the line is apart
        // from the circle; otherwise it may cross the plane anywhere along the circle, and
        // the in-plane picture does not decide it
        let s0 = (origin - point).dot(direction);
        let reach = radius + tol;
        let h = |s: f64| (point + direction * s - origin).dot(normal);
        let (h1, h2) = (h(s0 - reach), h(s0 + reach));
        if !(h1.is_finite() && h2.is_finite()) {
            return Meet::Undecided;
        }
        if h1.abs() > tol && h2.abs() > tol && h1 * h2 > 0.0 {
            return Meet::Points(Vec::new());
        }
        let lift = h1.abs().max(h2.abs());
        if lift > tol {
            return Meet::Undecided;
        }
        let p = point + direction * s0;
        let in_plane = p - normal * (p - origin).dot(normal);
        let f = foot(in_plane, direction, origin);
        let delta = (f - origin).length();
        // the projection moves the line by at most `lift`: a crossing within tol of the
        // line itself lies within tol + lift of its projection
        if delta > radius + tol + lift {
            return Meet::Points(Vec::new());
        }
        let half_sq = radius * radius - delta * delta;
        if half_sq <= 0.0 {
            // tangent within the tolerance, outside: the one touching point
            return project(vec![f]);
        }
        let half = half_sq.sqrt();
        return project(vec![f - direction * half, f + direction * half]);
    }
    // the line crosses the circle's plane at one point x. A point of the line within tol
    // of the circle is within tol of the plane, so within tol / |dn| of x along the line,
    // and x then lies within tol + tol / |dn| of the circle in the plane: farther than
    // that the line is apart. In between, a shallow line may pass within tol of the
    // circle away from x, undecided here. x within tol of the circle is a crossing
    let x = point + direction * ((origin - point).dot(normal) / dn);
    let off = ((x - origin).length() - radius).abs();
    if !off.is_finite() {
        return Meet::Undecided;
    }
    if off > tol + tol / dn.abs() {
        return Meet::Points(Vec::new());
    }
    if off > tol {
        return Meet::Undecided;
    }
    // every point within tol of the circle lies in the window of half-width tol / |dn|
    // about x, and projects into the plane within tol of the circle. In the plane, take t
    // along the line's projection from x, the centre's foot at t = f and its distance dd
    // from the projected line: the points within tol of the circle have |t - f| in
    // [a, b], a = sqrt((r - tol)^2 - dd^2), two intervals mirrored about f, x in one. If
    // the window reaches the other, the line may meet the circle a second time there,
    // undecided here; otherwise x is the one place it meets
    let across = direction - normal * dn;
    let along = across.length();
    if along > 0.0 {
        let u = across * (1.0 / along);
        let window = tol / dn.abs() * along;
        let f = (origin - x).dot(u);
        let dd_sq = (x - origin).dot(x - origin) - f * f;
        let inner = radius - tol;
        let a_sq = inner * inner - dd_sq;
        if !(window.is_finite() && a_sq.is_finite()) {
            return Meet::Undecided;
        }
        if inner > 0.0 && a_sq > 0.0 && f.abs() + a_sq.sqrt() <= window {
            return Meet::Undecided;
        }
    }
    project(vec![x])
}

/// How the curve of K carried by `k_carrier` meets E's carrier `e`, within `tol` and
/// the angle whose sine is `sin_ang`. `stretch` holds E's points at the stretch's ends;
/// the offsets that decide whether the carriers share a plane or a line are measured
/// there.
fn meet(e: Carrier, k_carrier: Carrier, stretch: [Point3; 2], tol: f64, sin_ang: f64) -> Meet {
    let [a, b] = stretch;
    match (e, k_carrier) {
        (
            Carrier::Line {
                point: pe,
                direction: de,
            },
            Carrier::Line {
                point: pk,
                direction: dk,
            },
        ) => {
            if de.cross(dk).length() <= sin_ang {
                // parallel within the angle: one line where both stretch ends lie on K's
                // line, apart where both lie off it on one side; a near-parallel line that
                // passes from one side to the other along the stretch is not decided here
                let off = |x: Point3| x - foot(pk, dk, x);
                let (oa, ob) = (off(a), off(b));
                if oa.length() <= tol && ob.length() <= tol {
                    return Meet::Overlap;
                }
                if oa.length() > tol && ob.length() > tol && oa.dot(ob) > 0.0 {
                    return Meet::Points(Vec::new());
                }
                return Meet::Undecided;
            }
            let w0 = pe - pk;
            let bb = de.dot(dk);
            let d = de.dot(w0);
            let ee = dk.dot(w0);
            let den = 1.0 - bb * bb;
            let se = (bb * ee - d) / den;
            let sk = (ee - bb * d) / den;
            let on_e = pe + de * se;
            let on_k = pk + dk * sk;
            let gap = (on_e - on_k).length();
            if !gap.is_finite() {
                Meet::Undecided
            } else if gap <= tol {
                finite(vec![on_e])
            } else {
                Meet::Points(Vec::new())
            }
        }
        (
            Carrier::Line {
                point, direction, ..
            },
            Carrier::Circle {
                origin,
                normal,
                radius,
                ..
            },
        ) => line_circle(point, direction, origin, normal, radius, true, tol, sin_ang),
        (
            Carrier::Circle {
                origin,
                normal,
                radius,
                ..
            },
            Carrier::Line { point, direction },
        ) => line_circle(
            point, direction, origin, normal, radius, false, tol, sin_ang,
        ),
        (
            Carrier::Circle {
                origin: o1,
                normal: n1,
                radius: r1,
                ..
            },
            Carrier::Circle {
                origin: o2,
                normal: n2,
                radius: r2,
                ..
            },
        ) => {
            // circle 2 is flattened into E's plane. With sin θ = |n1 × n2| and off the
            // height of o2 above that plane, a point P of circle 2 lies within
            // off + r2 sin θ of the plane, and its projection within r2 (1 - cos θ)
            // <= r2 sin θ of the circle of radius r2 about o2's projection: P lies within
            // e = off + 2 r2 sin θ of the flattened circle. The flattening decides only
            // where e <= tol, and every band below is widened by e, so no crossing within
            // tol of circle 2 itself is dropped
            let sin_tilt = n1.cross(n2).length();
            if sin_tilt > sin_ang {
                return Meet::Undecided;
            }
            let off = (o2 - o1).dot(n1);
            let lift = off.abs() + 2.0 * r2 * sin_tilt;
            if !lift.is_finite() || lift > tol {
                return Meet::Undecided;
            }
            let band = tol + lift;
            let o2_in = o2 - n1 * off;
            let dvec = o2_in - o1;
            let d = dvec.length();
            if d <= band && (r1 - r2).abs() <= band {
                return Meet::Overlap;
            }
            if d <= EFF_ZERO {
                return Meet::Points(Vec::new());
            }
            if d > r1 + r2 + band || d < (r1 - r2).abs() - band {
                return Meet::Points(Vec::new());
            }
            // within the tolerance of crossing or touching: the true crossings where the
            // circles cross, the touching point on E where they do not quite reach
            let u = dvec * (1.0 / d);
            let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
            let half_sq = r1 * r1 - along * along;
            if half_sq <= 0.0 {
                let sense = if along >= 0.0 { 1.0 } else { -1.0 };
                return finite(vec![o1 + u * (r1 * sense)]);
            }
            let half = half_sq.sqrt();
            let q = o1 + u * along;
            let v = n1.cross(u);
            finite(vec![q - v * half, q + v * half])
        }
    }
}

/// `angle` reduced into `[0, 2π)`.
fn wrap(angle: f64) -> f64 {
    let full = 2.0 * crate::math::PI;
    let r = angle % full;
    if r < 0.0 {
        r + full
    } else {
        r
    }
}

/// Whether `p`, a point of E's carrier, lies on E's stretch from `a` through `m` to `b`,
/// extended by `tol` at each end.
fn on_stretch(e: Carrier, [a, m, b]: [Point3; 3], p: Point3, tol: f64) -> bool {
    match e {
        Carrier::Line { point, direction } => {
            let s = |x: Point3| (x - point).dot(direction);
            let (sa, sb, sp) = (s(a), s(b), s(p));
            sp >= sa.min(sb) - tol && sp <= sa.max(sb) + tol
        }
        Carrier::Circle {
            origin,
            x,
            y,
            radius,
            ..
        } => {
            let angle = |q: Point3| {
                let v = q - origin;
                det::atan2(v.dot(y), v.dot(x))
            };
            let full = 2.0 * crate::math::PI;
            let (ta, tm, tb, tp) = (angle(a), angle(m), angle(b), angle(p));
            let sweep_ab = wrap(tb - ta);
            // the arc from `a` to `b` that holds `m`, as a start and a counterclockwise sweep
            let (start, sweep) = if wrap(tm - ta) <= sweep_ab {
                (ta, sweep_ab)
            } else {
                (tb, full - sweep_ab)
            };
            let slack = tol / radius;
            let at = wrap(tp - start);
            at <= sweep + slack || at >= full - slack
        }
    }
}

/// Whether the surface is one whose intersections the closed-form routines make.
fn analytic(surface: &Surface) -> bool {
    matches!(
        surface.kind(),
        SurfaceKind::Plane(_)
            | SurfaceKind::Cone(_)
            | SurfaceKind::Sphere(_)
            | SurfaceKind::Torus(_)
            | SurfaceKind::SurfOfExtrusion(_)
            | SurfaceKind::SurfOfRevolution(_)
    )
}

/// An evaluation copy of face `f`'s surface, with its face tolerance, and its UV domain;
/// `None` for a face without a surface.
fn face_surface(b: &Brep, f: FaceId) -> Result<Option<(Surface, crate::geom::extent2d::Extent2d)>> {
    let Some(surface) = crate::topology::face::p2::surface(b, f)? else {
        return Ok(None);
    };
    let mut copy = surface.clone_for_evaluation()?;
    copy.set_owner_zone_tol_3d(Some(crate::topology::face::p2::tolerance(b, f)?));
    Ok(Some((copy, crate::topology::face::p2::uv_domain(b, f)?)))
}

/// The points of K ∩ E on E's stretch, for the curves `k_curves` of K; `None` where the
/// carriers do not decide them, a K curve overlaps E, or the stretch is within a circle's
/// tolerance band of touching (the sagitta bound).
fn stretch_points(
    e: Carrier,
    k_curves: &[Curve],
    stretch: [Point3; 3],
    tol: f64,
    sin_ang: f64,
) -> Option<Vec<Point3>> {
    let [a, _, b] = stretch;
    let mut points = Vec::new();
    for curve in k_curves {
        let k_carrier = carrier(curve)?;
        // two carriers that touch with relative radius of curvature R stay within `tol` of
        // each other along a chord of up to 2 sqrt(2 R tol) about where they touch: a
        // stretch no longer than that may be all coincident within the tolerance, and is
        // kept. R is r for a line and a circle, r1 r2 / |r1 - r2| for circles touching
        // inside each other (the larger, so the bound holds for either sense)
        let radius = match (e, k_carrier) {
            (Carrier::Line { .. }, Carrier::Line { .. }) => 0.0,
            (Carrier::Circle { radius, .. }, Carrier::Line { .. })
            | (Carrier::Line { .. }, Carrier::Circle { radius, .. }) => radius,
            (Carrier::Circle { radius: r1, .. }, Carrier::Circle { radius: r2, .. }) => {
                let gap = (r1 - r2).abs();
                if gap <= EFF_ZERO * r1.max(r2) {
                    return None;
                }
                (r1 * r2 / gap).max(r1.max(r2))
            }
        };
        if !radius.is_finite()
            || (radius > 0.0 && (b - a).length() <= 2.0 * (2.0 * radius * tol).sqrt())
        {
            return None;
        }
        match meet(e, k_carrier, [a, b], tol, sin_ang) {
            Meet::Overlap | Meet::Undecided => return None,
            Meet::Points(ps) => {
                points.extend(ps.into_iter().filter(|&p| on_stretch(e, stretch, p, tol)));
            }
        }
    }
    Some(points)
}

/// The tolerance K ∩ E is found within for a face Z of E: the larger of the range's
/// `tolerance` (found with the classifying face's zone) and the intersection tolerance of
/// E's and Z's zones, the gap the B-rep allows between E and Z.
fn z_tolerance(k: &Kernel, tolerance: f64, edge_zone: f64, z_zone: f64) -> f64 {
    tolerance.max(xsect_tol_3d_from_zones(k, edge_zone, z_zone))
}

/// An end vertex of edge `e` the range ends at: within the vertex's tolerance of both
/// curves' points at that end of the range, and met by the classification.
struct RangeEnd {
    /// The vertex.
    vertex: VertexId,
    /// Its point.
    point: Point3,
    /// Its tolerance.
    tolerance: f64,
    /// The range end at it.
    end: SolutionEnd,
}

/// The tangency witness (see the module docs): `Some` single-value solution at the
/// contact that replaces the range `solution` of the classified curve `source` (handle
/// `source_handle`) on `edge`, `None` to keep it. `tolerance` is the intersection
/// tolerance the range was found at.
///
/// # Errors
///
/// Those of the evaluations, the distance between the curves and the topology reads;
/// a cancellation of the surface intersection. Any other failure of the surface
/// intersection (an unported path among them: issue1517's tube ends and the shelled
/// cylinder's top reach one) keeps the range. The caller keeps the range on every error
/// but a cancellation: the witness only adds evidence, and where it cannot run the
/// classifier's own answer stands.
pub(crate) fn tangency_point<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    source: &Curve,
    source_handle: O,
    edge: &BoundaryEdge<O>,
    solution: &Solution<O>,
    tolerance: f64,
) -> Result<Option<Solution<O>>> {
    if solution.solution_type != SolutionType::RangeOfValues || edge.seam.is_some() {
        return Ok(None);
    }
    let (Some(partner), Some(face)) = (cc.tangency_partner, cc.face) else {
        return Ok(None);
    };
    // which parameter is the curve's and which the edge's
    let (ci, ei) = match (solution.objects.first(), solution.objects.get(1)) {
        (Some(&Some(s)), Some(&Some(t))) if s == source_handle && t == edge.curve => (0, 1),
        (Some(&Some(t)), Some(&Some(s))) if s == source_handle && t == edge.curve => (1, 0),
        _ => return Ok(None),
    };
    let Some(target) = geometry.curve(edge.curve) else {
        return Ok(None);
    };
    if core::ptr::eq(target, source) {
        return Ok(None);
    }

    // 1. the case-3e predicate on the range's own ends
    let mut ends = [solution.start, solution.end];
    if ends[0].param(ci)? > ends[1].param(ci)? {
        ends.swap(0, 1);
    }
    let [lo, hi] = ends;
    let first = (lo.param(ci)?, lo.param(ei)?);
    let last = (hi.param(ci)?, hi.param(ei)?);
    let ivl_c = Extent1d::from_min_max(first.0, last.0);
    let max_degree = source.degree()?.max(target.degree()?);
    let gap = source.curve_max_distance_between(
        k,
        &ivl_c,
        target,
        first.1,
        last.1,
        max_degree as u64 + 3,
        Some(tolerance),
    )?;
    if !source.approaches_without_crossing(k, target, first, last, gap.distance)? {
        return Ok(None);
    }

    // 2. the topology: the classifying face, the edge, the partner face
    let Some((brep, EntityRef::Face(f))) = geometry.topology_entity(face) else {
        return Ok(None);
    };
    let Some((edge_brep, EntityRef::Edge(e))) = geometry.topology_entity(edge.edge) else {
        return Ok(None);
    };
    if !core::ptr::eq(brep, edge_brep) {
        return Ok(None);
    }
    let Some((g_brep, EntityRef::Face(g))) = geometry.topology_entity(partner) else {
        return Ok(None);
    };
    // the partner is the other face of the intersection, never the classifying face
    if core::ptr::eq(g_brep, brep) && g == f {
        return Ok(None);
    }

    // 3. the range ends at an end vertex of the edge that the classification met
    let (v_start, v_end) = crate::topology::edge::start_end_vertices(brep, e)?;
    let mut at_vertex: Vec<RangeEnd> = Vec::new();
    let mut ends_at_vertices = 0;
    for end in [lo, hi] {
        let mut matched = false;
        let on_c = source.evaluate_point(k, end.param(ci)?)?;
        let on_e = target.evaluate_point(k, end.param(ei)?)?;
        for v in [v_start, v_end].into_iter().flatten() {
            let record = brep.vertex(v)?;
            let (point, tol_v) = (record.point, record.tolerance);
            if (on_c - point).length() > tol_v || (on_e - point).length() > tol_v {
                continue;
            }
            // an end at a vertex counts against a second contact whether or not the
            // classification met that vertex
            matched = true;
            let Some(handle) = geometry.topology_handle_in(brep, EntityRef::Vertex(v)) else {
                continue;
            };
            let met = cc.intervals().iter().any(|ivl| {
                [&ivl.start, &ivl.end]
                    .iter()
                    .any(|p| p.vertex_object() == Some(handle))
            });
            if met && !at_vertex.iter().any(|r| r.vertex == v) {
                at_vertex.push(RangeEnd {
                    vertex: v,
                    point,
                    tolerance: tol_v,
                    end,
                });
            }
        }
        if matched {
            ends_at_vertices += 1;
        }
    }
    // a range at one end vertex only: with both ends at vertices the range spans the
    // edge's stretch between them, and a contact at one does not account for the other
    if at_vertex.len() != 1 || ends_at_vertices != 1 {
        return Ok(None);
    }

    // 4. the edge's other faces Z, each tested on its own
    let faces = crate::topology::edge::faces(brep, e, None)?;
    if faces.iter().filter(|&&z| z == f).count() != 1 {
        return Ok(None);
    }
    let zs: Vec<FaceId> = faces.into_iter().filter(|&z| z != f).collect();
    if zs.is_empty() {
        return Ok(None);
    }
    let Some(e_carrier) = carrier(target) else {
        return Ok(None);
    };
    let Some((g_surface, g_domain)) = face_surface(g_brep, g)? else {
        return Ok(None);
    };
    if !analytic(&g_surface) {
        return Ok(None);
    }
    let stretch = [
        target.evaluate_point(k, first.1)?,
        target.evaluate_point(k, 0.5 * (first.1 + last.1))?,
        target.evaluate_point(k, last.1)?,
    ];
    let sin_ang = det::sin(k.tolerances.ang_tol_rad());
    let mut contact: Option<&RangeEnd> = None;
    for z in zs {
        // a Z that is G itself gives K = G and decides nothing
        if core::ptr::eq(g_brep, brep) && z == g {
            return Ok(None);
        }
        let Some((z_surface, z_domain)) = face_surface(brep, z)? else {
            return Ok(None);
        };
        if !analytic(&z_surface) {
            return Ok(None);
        }
        let k_curves = match g_surface.global_surface_intersect(
            k,
            &g_domain,
            &z_surface,
            &z_domain,
            [true, true],
            Some(k.tolerances.approx_tol_3d),
            Some(k.tolerances.ang_tol_rad()),
        ) {
            Ok(r) => r.curves_3d,
            Err(err) if err.is_cancelled() => return Err(err),
            Err(_) => return Ok(None),
        };
        // E lies within its own and Z's zone tolerances of Z, not the classifying face's: K
        // is met within the larger of the range's tolerance and that bound, so a crossing
        // E has with K through Z's wider zone is not dropped
        let tol_z = z_tolerance(
            k,
            tolerance,
            edge.zone_tol_3d,
            crate::topology::face::p2::tolerance(brep, z)?,
        );
        let Some(points) = stretch_points(e_carrier, &k_curves, stretch, tol_z, sin_ang) else {
            return Ok(None);
        };
        // K ∩ E on the stretch is not empty, and all of it is at one range-end vertex
        let Some(&p0) = points.first() else {
            return Ok(None);
        };
        let Some(v) = at_vertex
            .iter()
            .find(|r| (p0 - r.point).length() <= r.tolerance)
        else {
            return Ok(None);
        };
        if points.iter().any(|&p| (p - v.point).length() > v.tolerance) {
            return Ok(None);
        }
        match contact {
            Some(c) if c.vertex != v.vertex => return Ok(None),
            _ => contact = Some(v),
        }
    }
    let Some(contact) = contact else {
        return Ok(None);
    };
    let mut point = *solution;
    point.solution_type = SolutionType::SingleValue;
    point.start = contact.end;
    point.end = contact.end;
    Ok(Some(point))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-6;
    const SIN: f64 = 1.745_329_243_133_368e-4;

    /// The floor's arc of #18064's corner round: centre (4, 248, 7), radius 1.15, in z = 7.
    fn round() -> Carrier {
        Carrier::Circle {
            origin: Point3::new(4.0, 248.0, 7.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: 1.15,
        }
    }

    fn line(point: [f64; 3], direction: [f64; 3]) -> Carrier {
        let (_, direction) = Point3::from_array(direction).length_and_unit().unwrap();
        Carrier::Line {
            point: Point3::from_array(point),
            direction,
        }
    }

    /// The round's quarter from (2.85, 248, 7) to (4, 249.15, 7), with its middle.
    fn quarter() -> [Point3; 3] {
        let h = core::f64::consts::FRAC_1_SQRT_2 * 1.15;
        [
            Point3::new(2.85, 248.0, 7.0),
            Point3::new(4.0 - h, 248.0 + h, 7.0),
            Point3::new(4.0, 249.15, 7.0),
        ]
    }

    fn points(k: Carrier) -> Option<Vec<Point3>> {
        let [a, m, b] = quarter();
        match meet(round(), k, [a, b], TOL, SIN) {
            Meet::Points(ps) => Some(
                ps.into_iter()
                    .filter(|&p| on_stretch(round(), [a, m, b], p, TOL))
                    .collect(),
            ),
            _ => None,
        }
    }

    #[test]
    fn a_line_tangent_to_the_arc_meets_it_at_the_touching_point() {
        // #18064's K: the floor's line y = 249.15 in z = 7
        let ps = points(line([0.0, 249.15, 7.0], [1.0, 0.0, 0.0])).unwrap();
        assert_eq!(ps.len(), 1);
        assert!((ps[0] - Point3::new(4.0, 249.15, 7.0)).length() <= 1e-12);
    }

    #[test]
    fn a_line_crossing_the_arc_meets_it_twice() {
        // y = 248.5 crosses the circle at x = 4 -+ sqrt(1.15^2 - 0.25); only the -x one is
        // on the quarter
        let x = (1.15_f64 * 1.15 - 0.25).sqrt();
        let [a, m, b] = quarter();
        let Meet::Points(all) = meet(
            round(),
            line([0.0, 248.5, 7.0], [1.0, 0.0, 0.0]),
            [a, b],
            TOL,
            SIN,
        ) else {
            panic!("decided");
        };
        assert_eq!(all.len(), 2);
        let on: Vec<_> = all
            .into_iter()
            .filter(|&p| on_stretch(round(), [a, m, b], p, TOL))
            .collect();
        assert_eq!(on.len(), 1);
        assert!((on[0] - Point3::new(4.0 - x, 248.5, 7.0)).length() <= 1e-12);
    }

    #[test]
    fn a_line_apart_from_the_arc_does_not_meet_it() {
        assert!(points(line([0.0, 249.5, 7.0], [1.0, 0.0, 0.0]))
            .unwrap()
            .is_empty());
        // parallel to the plane, above it
        assert!(points(line([0.0, 249.15, 7.5], [1.0, 0.0, 0.0]))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_line_through_the_plane_meets_the_arc_where_it_pierces_it() {
        // the vertical line through (4, 249.15)
        let ps = points(line([4.0, 249.15, 0.0], [0.0, 0.0, 1.0])).unwrap();
        assert_eq!(ps.len(), 1);
        assert!((ps[0] - Point3::new(4.0, 249.15, 7.0)).length() <= 1e-12);
    }

    #[test]
    fn a_near_coplanar_line_crossing_the_arc_inside_is_undecided() {
        // the review's case: the right half of the unit circle in z = 0, and a line through
        // its midpoint (1, 0, 0) rising 1e-4 per unit, below the angle tolerance. The feet
        // of both arc ends sit 1e-4 below the plane, but the line crosses the arc
        let e = Carrier::Circle {
            origin: Point3::new(0.0, 0.0, 0.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: 1.0,
        };
        let stretch = [Point3::new(0.0, -1.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
        let k = line([1.0, 0.0, 0.0], [1.0, 0.0, 1e-4]);
        assert!(matches!(meet(e, k, stretch, TOL, SIN), Meet::Undecided));
        // in the plane the tangent line is still decided, at its touching point
        let Meet::Points(ps) = meet(e, line([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), stretch, TOL, SIN)
        else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 1);
        assert!((ps[0] - Point3::new(1.0, 0.0, 0.0)).length() <= 1e-12);
        // and a line below the plane over the whole circle stays apart
        let k = line([1.0, 0.0, -0.5], [1.0, 0.0, 1e-4]);
        let Meet::Points(ps) = meet(e, k, stretch, TOL, SIN) else {
            panic!("decided");
        };
        assert!(ps.is_empty());
    }

    #[test]
    fn a_shallow_line_through_the_plane_near_the_circle_is_undecided() {
        // the delta review's case: the unit circle in z = 0 and a line through
        // (1.001, 0, 0) with direction (-1, 0, 2e-4), just past the angle tolerance. It
        // crosses the plane 1e-3 off the circle but passes within 2e-7 of it near (1, 0, 0)
        let e = Carrier::Circle {
            origin: Point3::new(0.0, 0.0, 0.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: 1.0,
        };
        let stretch = [Point3::new(0.0, -1.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
        let k = line([1.001, 0.0, 0.0], [-1.0, 0.0, 2e-4]);
        assert!(matches!(meet(e, k, stretch, TOL, SIN), Meet::Undecided));
        // steep through the circle: the crossing itself
        let Meet::Points(ps) = meet(e, line([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), stretch, TOL, SIN)
        else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 1);
        assert!((ps[0] - Point3::new(1.0, 0.0, 0.0)).length() <= 1e-12);
        // steep and 1e-3 off the circle: apart
        let Meet::Points(ps) = meet(
            e,
            line([1.001, 0.0, 0.0], [0.0, 0.0, 1.0]),
            stretch,
            TOL,
            SIN,
        ) else {
            panic!("decided");
        };
        assert!(ps.is_empty());
    }

    #[test]
    fn a_shallow_line_meeting_the_circle_twice_within_its_window_is_undecided() {
        // the second delta review's case: the line leaves the unit circle at (1, 0, 0)
        // along the chord to (cos 4e-3, sin 4e-3, 0), rising 8e-7 over it, so dn is about
        // 2e-4. It crosses the plane on the circle, and comes back within 8e-7 of the
        // circle 4e-3 away: x alone would hide that second contact
        let e = Carrier::Circle {
            origin: Point3::new(0.0, 0.0, 0.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: 1.0,
        };
        let stretch = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(det::cos(0.01f64), det::sin(0.01f64), 0.0),
        ];
        let t = 4e-3f64;
        let k = line([1.0, 0.0, 0.0], [det::cos(t) - 1.0, det::sin(t), 8e-7]);
        assert!(matches!(meet(e, k, stretch, TOL, SIN), Meet::Undecided));
        // steeper (dn about 0.24), the same chord is far outside the window: the crossing
        let k = line([1.0, 0.0, 0.0], [det::cos(t) - 1.0, det::sin(t), 1e-3]);
        let Meet::Points(ps) = meet(e, k, stretch, TOL, SIN) else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 1);
        assert!((ps[0] - Point3::new(1.0, 0.0, 0.0)).length() <= 1e-12);
    }

    #[test]
    fn a_tilted_circle_is_flattened_only_within_the_tolerance() {
        // a circle of the round's radius crossing it, tilted about the x axis by sin θ
        let [a, _, b] = quarter();
        let tilted = |sin: f64| {
            let cos = (1.0 - sin * sin).sqrt();
            Carrier::Circle {
                origin: Point3::new(4.5, 248.0, 7.0),
                x: Point3::new(1.0, 0.0, 0.0),
                y: Point3::new(0.0, cos, sin),
                normal: Point3::new(0.0, -sin, cos),
                radius: 1.15,
            }
        };
        // below the angle tolerance, but 2 r sin θ = 2.3e-4 strays past TOL: undecided
        assert!(matches!(
            meet(round(), tilted(1e-4), [a, b], TOL, SIN),
            Meet::Undecided
        ));
        // 2 r sin θ = 2.3e-8: decided, with both crossings at x = 4.25 on the round
        let Meet::Points(ps) = meet(round(), tilted(1e-8), [a, b], TOL, SIN) else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 2);
        for p in ps {
            assert!((p.x - 4.25).abs() <= 1e-12);
            assert!(((p - Point3::new(4.0, 248.0, 7.0)).length() - 1.15).abs() <= 1e-12);
        }
        // an offset of the centre from the plane counts too
        let lifted = Carrier::Circle {
            origin: Point3::new(4.5, 248.0, 7.0 + 2.0 * TOL),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: 1.15,
        };
        assert!(matches!(
            meet(round(), lifted, [a, b], TOL, SIN),
            Meet::Undecided
        ));
    }

    #[test]
    fn a_face_with_a_wider_zone_meets_e_within_its_own_gap() {
        let k = Kernel::default();
        // Z's zone wider than the range's tolerance: the gap E may have from Z rules
        let wide = z_tolerance(&k, TOL, 1e-6, 1e-4);
        assert!((wide - xsect_tol_3d_from_zones(&k, 1e-6, 1e-4)).abs() <= 0.0);
        assert!(wide > TOL);
        // a narrower Z keeps the range's tolerance
        assert!((z_tolerance(&k, TOL, 1e-9, 1e-9) - TOL).abs() <= 0.0);
        // a K line through the quarter's interior 5 TOL off the round's plane: apart at the
        // range's tolerance, met (so the witness refuses) at Z's
        let [a, m, b] = quarter();
        let kl = [Curve::new(CurveKind::Line(
            crate::curve::line::Line::create_canonical(
                Point3::new(0.0, 248.5, 7.0 + 5.0 * TOL),
                Point3::new(1.0, 0.0, 0.0),
            ),
        ))];
        let narrow = stretch_points(round(), &kl, [a, m, b], TOL, SIN).unwrap();
        assert!(narrow.is_empty());
        let at_z = stretch_points(round(), &kl, [a, m, b], wide, SIN).unwrap();
        assert_eq!(at_z.len(), 1);
    }

    #[test]
    fn the_arc_itself_overlaps() {
        let [a, _, b] = quarter();
        assert!(matches!(
            meet(round(), round(), [a, b], TOL, SIN),
            Meet::Overlap
        ));
    }

    #[test]
    fn circles_tangent_externally_and_internally_meet_once() {
        let [a, _, b] = quarter();
        let other = |cx: f64, r: f64| Carrier::Circle {
            origin: Point3::new(cx, 248.0, 7.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: r,
        };
        for (cx, r, want) in [(0.85, 2.0, 2.85), (3.35, 0.5, 2.85)] {
            let Meet::Points(ps) = meet(round(), other(cx, r), [a, b], TOL, SIN) else {
                panic!("decided");
            };
            // exactly tangent: one point, or two that rounding splits by no more than
            // sqrt(2 r eps)
            assert!(!ps.is_empty() && ps.len() <= 2);
            for p in ps {
                assert!((p - Point3::new(want, 248.0, 7.0)).length() <= 1e-6);
            }
        }
    }

    #[test]
    fn circles_inside_the_tangent_band_that_cross_give_their_true_crossings() {
        // radii 1.15 and 1.149, centres 0.001 + 5e-7 apart: |d - (r1 - r2)| <= TOL, but the
        // circles cross well away from the touching point
        let [a, _, b] = quarter();
        let inner = Carrier::Circle {
            origin: Point3::new(4.0 + 0.001 + 5e-7, 248.0, 7.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 1.0, 0.0),
            normal: Point3::new(0.0, 0.0, 1.0),
            radius: 1.149,
        };
        let Meet::Points(ps) = meet(round(), inner, [a, b], TOL, SIN) else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 2);
        let touch = Point3::new(4.0 + 1.15, 248.0, 7.0);
        for p in ps {
            assert!((p - touch).length() > 1e-3, "{p:?}");
        }
    }

    #[test]
    fn a_line_inside_the_tangent_band_gives_both_crossings() {
        // y = 249.15 - 5e-7: within TOL of tangent, crossing twice sqrt(2 r 5e-7) either side
        let [a, _, b] = quarter();
        let Meet::Points(ps) = meet(
            round(),
            line([0.0, 249.15 - 5e-7, 7.0], [1.0, 0.0, 0.0]),
            [a, b],
            TOL,
            SIN,
        ) else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 2);
        let half = (1.15_f64 * 1.15 - (1.15 - 5e-7) * (1.15 - 5e-7)).sqrt();
        assert!((ps[0].x - (4.0 - half)).abs() <= 1e-9);
        assert!((ps[1].x - (4.0 + half)).abs() <= 1e-9);
    }

    #[test]
    fn a_near_parallel_line_that_changes_side_along_the_stretch_is_undecided() {
        let e = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let stretch = [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)];
        // through (1, 0) at a slope below the angle tolerance: 1e-5 off at either end, on
        // opposite sides
        let k = line([1.0, 0.0, 0.0], [1.0, 1e-5, 0.0]);
        assert!(matches!(meet(e, k, stretch, TOL, SIN), Meet::Undecided));
        // the same slope wholly on one side stays apart
        let k = line([1.0, 1e-3, 0.0], [1.0, 1e-5, 0.0]);
        let Meet::Points(ps) = meet(e, k, stretch, TOL, SIN) else {
            panic!("decided");
        };
        assert!(ps.is_empty());
    }

    #[test]
    fn a_line_along_the_plane_that_leaves_it_along_the_stretch_is_undecided() {
        // a line tilted out of z = 7 below the angle tolerance, in the plane at one end of the
        // quarter and 1e-5 above it at the other
        let [a, _, b] = quarter();
        let dir = b - a;
        let rise = 1e-5 / dir.length();
        let k = line([a.x, a.y, 7.0], [dir.x, dir.y, rise * dir.length()]);
        assert!(matches!(
            meet(round(), k, [a, b], TOL, SIN),
            Meet::Undecided
        ));
    }

    #[test]
    fn a_stretch_inside_the_band_of_nearly_equal_circles_is_kept() {
        // radii 1.15 and 1.14 touching inside: R = r1 r2 / |r1 - r2| ~ 131, so the band is
        // 2 sqrt(2 R TOL) ~ 0.032, well past the 2 sqrt(2 r TOL) ~ 0.003 of the larger radius
        let k = [Curve::from(
            crate::curve::ellipse::Ellipse::new_circle(
                &Kernel::default(),
                Point3::new(4.01, 248.0, 7.0),
                Vec3::X,
                Vec3::Y,
                Extent1d::from_raw(0.0, 360.0),
                1.14,
                &crate::curve::ellipse::EllipseOptions::default(),
            )
            .unwrap(),
        )];
        let at = |angle: f64| {
            Point3::new(
                4.0 + 1.15 * det::cos(angle),
                248.0 + 1.15 * det::sin(angle),
                7.0,
            )
        };
        let s = 0.01 / 1.15;
        assert!(stretch_points(round(), &k, [at(s), at(s / 2.0), at(0.0)], TOL, SIN).is_none());
    }

    #[test]
    fn circles_in_crossed_planes_are_undecided() {
        let [a, _, b] = quarter();
        let upright = Carrier::Circle {
            origin: Point3::new(4.0, 248.0, 7.0),
            x: Point3::new(1.0, 0.0, 0.0),
            y: Point3::new(0.0, 0.0, 1.0),
            normal: Point3::new(0.0, -1.0, 0.0),
            radius: 1.15,
        };
        assert!(matches!(
            meet(round(), upright, [a, b], TOL, SIN),
            Meet::Undecided
        ));
    }

    #[test]
    fn a_line_edge_meets_a_line_once_or_overlaps_it() {
        let e = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let stretch = [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)];
        let Meet::Points(ps) = meet(
            e,
            line([1.0, -1.0, 0.0], [0.0, 1.0, 0.0]),
            stretch,
            TOL,
            SIN,
        ) else {
            panic!("decided");
        };
        assert_eq!(ps.len(), 1);
        assert!((ps[0] - Point3::new(1.0, 0.0, 0.0)).length() <= 1e-12);
        assert!(matches!(
            meet(
                e,
                line([5.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
                stretch,
                TOL,
                SIN
            ),
            Meet::Overlap
        ));
        // skew lines a unit apart
        let Meet::Points(ps) = meet(e, line([1.0, 0.0, 1.0], [0.0, 1.0, 0.0]), stretch, TOL, SIN)
        else {
            panic!("decided");
        };
        assert!(ps.is_empty());
    }

    #[test]
    fn a_stretch_inside_the_tolerance_band_of_a_touch_is_kept() {
        // the band along which y = 249.15 stays within TOL of the round: 2 sqrt(2 r TOL)
        let band = 2.0 * (2.0 * 1.15 * TOL).sqrt();
        let k = [Curve::new(CurveKind::Line(
            crate::curve::line::Line::create_canonical(
                Point3::new(0.0, 249.15, 7.0),
                Point3::new(1.0, 0.0, 0.0),
            ),
        ))];
        let at = |angle: f64| {
            Point3::new(
                4.0 + 1.15 * det::cos(angle),
                248.0 + 1.15 * det::sin(angle),
                7.0,
            )
        };
        let top = core::f64::consts::FRAC_PI_2;
        let short = band / 1.15 * 0.9;
        assert!(stretch_points(
            round(),
            &k,
            [at(top + short), at(top + short / 2.0), at(top)],
            TOL,
            SIN
        )
        .is_none());
        let long = band / 1.15 * 2.0;
        let ps = stretch_points(
            round(),
            &k,
            [at(top + long), at(top + long / 2.0), at(top)],
            TOL,
            SIN,
        )
        .unwrap();
        assert_eq!(ps.len(), 1);
    }
}
