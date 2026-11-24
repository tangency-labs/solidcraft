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
//! - `Kernel::fixes.simple_coincidence_crossing_is_point` is on (it decides these ranges
//!   itself, so its results are unchanged);
//! - the classification has no tangency partner (only
//!   [`crate::topology::face::p3::face_intersect_breps_cached`] sets one);
//! - the solution is not a range of the curve on an edge, or the edge is a seam;
//! - the range does not meet the case-3e predicate on its own ends
//!   ([`Curve::approaches_without_crossing`]);
//! - neither end of the range is at an end vertex of E that the classification met;
//! - E has no other face, or the classifying face lies on both sides of it;
//! - G or a Z is not an analytic surface, or their intersection fails;
//! - E or a curve of K is not a line or a circle, a K curve overlaps E, or two circles do
//!   not lie in one plane;
//! - the stretch is no longer than the chord 2 sqrt(2 r tol) along which a carrier stays
//!   within the tolerance of a circle of radius r it touches;
//! - for some Z, no point of K ∩ E lies on the stretch, or one lies away from V.
//!
//! Each Z of a non-manifold edge is tested on its own.
//!
//! Deviations: not upstream.

use super::p1::CurveClassification;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{Solution, SolutionEnd, SolutionType};
use crate::geom::vector3d::Point3;
use crate::geom::vector3d::Vec3;
use crate::intersect::curve_class::p9::BoundaryEdge;
use crate::intersect::point_class::{ClassGeometry, ClassObject};
use crate::kernel::Kernel;
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

/// How the line (`point`, unit `direction`) meets the circle; `line_is_e` says which
/// carrier's points are returned. `reference` is a point of E's stretch, where the line's
/// offset from the circle's plane is measured.
#[allow(clippy::too_many_arguments)]
fn line_circle(
    point: Point3,
    direction: Vec3,
    origin: Point3,
    normal: Vec3,
    radius: f64,
    line_is_e: bool,
    reference: Point3,
    tol: f64,
    sin_ang: f64,
) -> Meet {
    let p = foot(point, direction, reference);
    let dn = direction.dot(normal);
    let on_circle = |x: Point3| -> Option<Point3> {
        let (len, u) = (x - origin).length_and_unit()?;
        (len > EFF_ZERO).then_some(origin + u * radius)
    };
    if dn.abs() <= sin_ang {
        // the line lies along the circle's plane: in it, or apart from it
        let h = (p - origin).dot(normal);
        if h.abs() > tol {
            return Meet::Points(Vec::new());
        }
        let in_plane = p - normal * h;
        let f = foot(in_plane, direction, origin);
        let delta = (f - origin).length();
        if (delta - radius).abs() <= tol {
            // tangent: one point, the foot of the centre (on the circle for a circle E)
            if line_is_e {
                return Meet::Points(vec![f]);
            }
            return match on_circle(f) {
                Some(c) => Meet::Points(vec![c]),
                None => Meet::Undecided,
            };
        }
        if delta < radius {
            let half = (radius * radius - delta * delta).sqrt();
            return Meet::Points(vec![f - direction * half, f + direction * half]);
        }
        return Meet::Points(Vec::new());
    }
    // the line crosses the circle's plane at one point
    let x = p + direction * ((origin - p).dot(normal) / dn);
    if ((x - origin).length() - radius).abs() > tol {
        return Meet::Points(Vec::new());
    }
    if line_is_e {
        Meet::Points(vec![x])
    } else {
        match on_circle(x) {
            Some(c) => Meet::Points(vec![c]),
            None => Meet::Undecided,
        }
    }
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
                // parallel: one line where both stretch ends lie on K's line
                let apart = |x: Point3| (foot(pk, dk, x) - x).length();
                if apart(a) <= tol && apart(b) <= tol {
                    return Meet::Overlap;
                }
                if apart(a) > tol && apart(b) > tol {
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
            if (on_e - on_k).length() <= tol {
                Meet::Points(vec![on_e])
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
        ) => line_circle(
            point,
            direction,
            origin,
            normal,
            radius,
            true,
            midpoint_of(a, b),
            tol,
            sin_ang,
        ),
        (
            Carrier::Circle {
                origin,
                normal,
                radius,
                ..
            },
            Carrier::Line { point, direction },
        ) => line_circle(
            point,
            direction,
            origin,
            normal,
            radius,
            false,
            midpoint_of(a, b),
            tol,
            sin_ang,
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
            let off_plane = |x: Point3| (x - o2).dot(n2).abs();
            if n1.cross(n2).length() > sin_ang || off_plane(a) > tol || off_plane(b) > tol {
                return Meet::Undecided;
            }
            let o2_in = o2 - n1 * (o2 - o1).dot(n1);
            let dvec = o2_in - o1;
            let d = dvec.length();
            if d <= tol && (r1 - r2).abs() <= tol {
                return Meet::Overlap;
            }
            if d <= EFF_ZERO {
                return Meet::Points(Vec::new());
            }
            let u = dvec * (1.0 / d);
            if (d - (r1 + r2)).abs() <= tol {
                return Meet::Points(vec![o1 + u * r1]);
            }
            if (d - (r1 - r2).abs()).abs() <= tol {
                let sense = if r1 >= r2 { 1.0 } else { -1.0 };
                return Meet::Points(vec![o1 + u * (r1 * sense)]);
            }
            if d > (r1 - r2).abs() && d < r1 + r2 {
                let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let half = (r1 * r1 - along * along).max(0.0).sqrt();
                let q = o1 + u * along;
                let v = n1.cross(u);
                return Meet::Points(vec![q - v * half, q + v * half]);
            }
            Meet::Points(Vec::new())
        }
    }
}

/// The point halfway between `a` and `b`.
fn midpoint_of(a: Point3, b: Point3) -> Point3 {
    a + (b - a) * 0.5
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
                v.dot(y).atan2(v.dot(x))
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
        // a line or circle within `tol` of a circle of radius r stays so along a chord of up
        // to 2 sqrt(2 r tol) about where they touch: a stretch no longer than that may be all
        // coincident within the tolerance, and is kept
        let radius = [e, k_carrier]
            .iter()
            .map(|c| match *c {
                Carrier::Circle { radius, .. } => radius,
                Carrier::Line { .. } => 0.0,
            })
            .fold(0.0, f64::max);
        if radius > 0.0 && (b - a).length() <= 2.0 * (2.0 * radius * tol).sqrt() {
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
/// the kernel's cancellation and unported errors from the surface intersection. Any
/// other failure of the surface intersection keeps the range.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
    if k.fixes.simple_coincidence_crossing_is_point
        || solution.solution_type != SolutionType::RangeOfValues
        || edge.seam.is_some()
    {
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

    // 3. the range ends at an end vertex of the edge that the classification met
    let (v_start, v_end) = crate::topology::edge::start_end_vertices(brep, e)?;
    let mut at_vertex: Vec<RangeEnd> = Vec::new();
    for end in [lo, hi] {
        let on_c = source.evaluate_point(k, end.param(ci)?)?;
        let on_e = target.evaluate_point(k, end.param(ei)?)?;
        for v in [v_start, v_end].into_iter().flatten() {
            let record = brep.vertex(v)?;
            let (point, tol_v) = (record.point, record.tolerance);
            if (on_c - point).length() > tol_v || (on_e - point).length() > tol_v {
                continue;
            }
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
    }
    if at_vertex.is_empty() {
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
    let sin_ang = k.tolerances.ang_tol_rad().sin();
    let mut contact: Option<&RangeEnd> = None;
    for z in zs {
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
            Err(err) if err.is_unported_or_cancelled() => return Err(err),
            Err(_) => return Ok(None),
        };
        let Some(points) = stretch_points(e_carrier, &k_curves, stretch, tolerance, sin_ang) else {
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
            assert_eq!(ps.len(), 1);
            assert!((ps[0] - Point3::new(want, 248.0, 7.0)).length() <= 1e-12);
        }
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
        let at =
            |angle: f64| Point3::new(4.0 + 1.15 * angle.cos(), 248.0 + 1.15 * angle.sin(), 7.0);
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
