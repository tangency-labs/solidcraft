//! Rebuild a planar loop after cancelling one reversed straight overlap.
//!
//! A bounded, conservative certificate: exactly one overlapping line pair, and
//! precisely one unambiguous remaining closed cycle. Curved stretches are retained.
//! Ambiguous incidence or multiple cycles leaves the face unchanged. No recursion.
//! Upstream: none (regularized inner offset boundaries).

use super::p1::OffsetExecutive;
use super::p2::OffsetNotifications;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::CurveKind;
use crate::error::{EntityRef, Result};
use crate::geom::extent1d::Extent1d;
use crate::geom::object::NotifyOperation;
use crate::geom::solution_array::SolutionType;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p5 as bp5;
use crate::topology::brep::p7::{make_face_with_curve_owners, FaceCurvesInput};
use crate::topology::edge::{self, NotificationStores, NotifyState};
use crate::topology::edgeuse::{p1 as eu, p3 as eu3};
use crate::topology::face::p2 as face;
use crate::topology::topology;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId};

const OP: &str = "offset::offset_executive::spurs";

/// `value`, or a typed defect naming `what` is missing on face `f`.
/// Where along a span's parameter interval it is sampled.
const SAMPLES: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

const EDGE_MISSING: &str =
    "a spur edge use has no edge, though every edge use of a loop stands on an edge";
const CURVE_MISSING: &str =
    "a spur edge has no 3D curve, though every edge of an offset face carries one";
const UV_MISSING: &str =
    "a spur edge use has no UV trim curve after one was requested, though the request builds it";

fn required<T>(value: Option<T>, what: &'static str, f: FaceId) -> Result<T> {
    value.ok_or_else(|| super::cancel::defect(what, Some(f)))
}

fn coincident(a: Point3, b: Point3, tolerance: f64) -> bool {
    let d = a - b;
    let scale = gamma(64) * (a.x.abs() + b.x.abs() + a.y.abs() + b.y.abs() + a.z.abs() + b.z.abs());
    let distance = d.length();
    scale.is_finite()
        && (distance > 0.0 || d == crate::math::Vec3::ZERO)
        && distance <= scale.min(tolerance)
}

#[derive(Clone, Copy)]
struct Span {
    source: EdgeuseId,
    interval: Extent1d,
    start: Point3,
    end: Point3,
    /// Five points along the span, in its walking direction: its shape for the
    /// crossing test (exact for a straight span, a polyline for a curved one).
    shape: [Point3; 5],
}

fn span(k: &Kernel, b: &Brep, f: FaceId, source: EdgeuseId, lo: f64, hi: f64) -> Result<Span> {
    let e = required(eu::edge(b, source)?, EDGE_MISSING, f)?;
    let curve = required(b.edge(e)?.curve.as_ref(), CURVE_MISSING, f)?;
    let a = curve.evaluate_point(k, lo)?;
    let c = curve.evaluate_point(k, hi)?;
    let mut shape = [a; 5];
    for (point, weight) in shape.iter_mut().zip(SAMPLES) {
        *point = curve.evaluate_point(k, lo + (hi - lo) * weight)?;
    }
    let (start, end) = if eu::orientation(b, source)? == OrientType::Same {
        (a, c)
    } else {
        shape.reverse();
        (c, a)
    };
    Ok(Span {
        source,
        interval: Extent1d::from_min_max(lo, hi),
        start,
        end,
        shape,
    })
}

/// Whether two segments touch or cross, a contact within `tolerance` of a shared
/// line included (overlapping collinear segments touch).
fn segments_touch(p1: Point3, p2: Point3, q1: Point3, q2: Point3, tolerance: f64) -> bool {
    let r = p2 - p1;
    let s = q2 - q1;
    let w = q1 - p1;
    let n = r.cross(s);
    let nn = n.dot(n);
    let rr = r.dot(r);
    if rr == 0.0 || !nn.is_finite() {
        return false;
    }
    if nn <= gamma(64) * rr * s.dot(s) {
        // Parallel: they touch only along one line, where their extents overlap.
        let off = |q: Point3| {
            let d = (q - p1).cross(r);
            d.dot(d) <= tolerance * tolerance * rr
        };
        if !(off(q1) && off(q2)) {
            return false;
        }
        let t0 = w.dot(r);
        let t1 = (q2 - p1).dot(r);
        return t0.min(t1).max(0.0) <= t0.max(t1).min(rr);
    }
    let t = w.cross(s).dot(n);
    let u = w.cross(r).dot(n);
    (0.0..=nn).contains(&t) && (0.0..=nn).contains(&u)
}

/// Whether two spans of the chain, not neighbours, touch each other.
fn spans_touch(a: &Span, c: &Span, tolerance: f64) -> bool {
    a.shape.windows(2).any(|first| {
        c.shape.windows(2).any(|second| {
            matches!(
                (first, second),
                ([p, q], [r, s]) if segments_touch(*p, *q, *r, *s, tolerance)
            )
        })
    })
}

/// Return the single retained cycle, declining every ambiguous arrangement.
#[allow(clippy::too_many_lines)] // one bounded overlap and incidence certificate
fn plan(k: &Kernel, b: &Brep, f: FaceId) -> Result<Option<Vec<Span>>> {
    if !b
        .face(f)?
        .surface
        .as_ref()
        .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)))
    {
        return Ok(None);
    }
    let Some(mut fu) = face::upward_faceuse(b, f)? else {
        return Ok(None);
    };
    if eu::faceuse_orientation(b, fu)? != OrientType::Same {
        fu = required(
            eu::faceuse_mate(b, fu)?,
            "a face use of a spur face has no mate, though face uses come in pairs",
            f,
        )?;
    }
    let loops = eu::faceuse_loopuses(b, fu, false, None)?;
    let [lu] = loops.as_slice() else {
        return Ok(None);
    };
    let uses = eu::loopuse_edgeuses(b, *lu)?;
    let mut overlap = None;
    for (i, &a) in uses.iter().enumerate() {
        k.check_cancelled(OP)?;
        let ea = required(eu::edge(b, a)?, EDGE_MISSING, f)?;
        let ca = required(b.edge(ea)?.curve.as_ref(), CURVE_MISSING, f)?;
        let CurveKind::Line(la) = ca.kind() else {
            continue;
        };
        for &c in uses.iter().skip(i + 1) {
            let ec = required(eu::edge(b, c)?, EDGE_MISSING, f)?;
            let cc = required(b.edge(ec)?.curve.as_ref(), CURVE_MISSING, f)?;
            let CurveKind::Line(lc) = cc.kind() else {
                continue;
            };
            if la.line_vector().cross(lc.line_vector()) != crate::math::Vec3::ZERO {
                continue;
            }
            let sign_a = if eu::orientation(b, a)? == OrientType::Same {
                1.0
            } else {
                -1.0
            };
            let sign_c = if eu::orientation(b, c)? == OrientType::Same {
                1.0
            } else {
                -1.0
            };
            if la.line_vector().dot(lc.line_vector()) * sign_a * sign_c >= 0.0 {
                continue;
            }
            let solutions = ca.global_curve_intersect(
                k,
                &b.edge(ea)?.interval,
                cc,
                &b.edge(ec)?.interval,
                b.tolerance(),
                &CurveIntersectOptions::<NoObject>::default(),
            )?;
            for solution in solutions.as_slice() {
                if solution.solution_type != SolutionType::RangeOfValues {
                    continue;
                }
                let a0 = solution.start.param(0)?;
                let a1 = solution.end.param(0)?;
                let c0 = solution.start.param(1)?;
                let c1 = solution.end.param(1)?;
                if !(a0.is_finite() && a1.is_finite() && c0.is_finite() && c1.is_finite())
                    || a0 == a1
                    || c0 == c1
                {
                    continue;
                }
                // A solver's zone coincidence alone is insufficient: the paired
                // overlap ends must coincide to arithmetic error as well.
                if !coincident(
                    ca.evaluate_point(k, a0)?,
                    cc.evaluate_point(k, c0)?,
                    b.tolerance(),
                ) || !coincident(
                    ca.evaluate_point(k, a1)?,
                    cc.evaluate_point(k, c1)?,
                    b.tolerance(),
                ) {
                    continue;
                }
                if overlap.is_some() {
                    return Ok(None);
                }
                overlap = Some((a, a0.min(a1), a0.max(a1), c, c0.min(c1), c0.max(c1)));
            }
        }
    }
    let Some((a, alo, ahi, c, clo, chi)) = overlap else {
        return Ok(None);
    };
    let mut spans = Vec::new();
    for &source in &uses {
        let e = required(eu::edge(b, source)?, EDGE_MISSING, f)?;
        let domain = b.edge(e)?.interval;
        let cut = if source == a {
            Some((alo, ahi))
        } else if source == c {
            Some((clo, chi))
        } else {
            None
        };
        if let Some((lo, hi)) = cut {
            if lo < domain.min() || hi > domain.max() {
                return Ok(None);
            }
            if lo > domain.min() {
                spans.push(span(k, b, f, source, domain.min(), lo)?);
            }
            if hi < domain.max() {
                spans.push(span(k, b, f, source, hi, domain.max())?);
            }
        } else {
            spans.push(span(k, b, f, source, domain.min(), domain.max())?);
        }
    }
    if spans.len() < 2 {
        return Ok(None);
    }
    let Some(first) = spans.pop() else {
        return Ok(None);
    };
    let mut chain = vec![first];
    let bound = spans.len();
    let mut pool: Vec<Option<Span>> = spans.into_iter().map(Some).collect();
    for _ in 0..bound {
        let Some(tail) = chain.last().map(|s| s.end) else {
            return Ok(None);
        };
        let matches: Vec<_> = pool
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.as_ref()
                    .is_some_and(|s| coincident(tail, s.start, b.tolerance()))
                    .then_some(i)
            })
            .collect();
        let [index] = matches.as_slice() else {
            return Ok(None);
        };
        let Some(next) = pool.get_mut(*index).and_then(Option::take) else {
            return Ok(None);
        };
        chain.push(next);
    }
    let Some(last) = chain.last() else {
        return Ok(None);
    };
    if !coincident(last.end, first.start, b.tolerance()) {
        return Ok(None);
    }
    // The endpoint walk shows a closed chain, not a simple one: a retained span
    // that touches another beyond its neighbours leaves an ambiguous boundary.
    let count = chain.len();
    for (i, a) in chain.iter().enumerate() {
        k.check_cancelled(OP)?;
        for (j, c) in chain.iter().enumerate().skip(i.saturating_add(1)) {
            let neighbours = j == i.saturating_add(1) || (i == 0 && j.saturating_add(1) == count);
            if !neighbours && spans_touch(a, c, b.tolerance()) {
                return Ok(None);
            }
        }
    }
    Ok(Some(chain))
}

/// Whether each UV trim of the chain puts a parameter of its span where the 3D edge
/// does: equal intervals are necessary, and the two curves must also name the same
/// point of the face's surface at sampled parameters (a UV trim reparameterized
/// against its edge would be cut at a different place).
fn parameters_agree(k: &Kernel, b: &Brep, f: FaceId, chain: &[Span]) -> Result<bool> {
    let surface = required(
        b.face(f)?.surface.as_ref(),
        "a spur face has no surface, though every offset face carries one",
        f,
    )?;
    for s in chain {
        let e = required(eu::edge(b, s.source)?, EDGE_MISSING, f)?;
        let curve = required(b.edge(e)?.curve.as_ref(), CURVE_MISSING, f)?;
        let uv = required(eu::uv_trim_curve(b, s.source)?.0, UV_MISSING, f)?;
        if !uv.natural_interval().are_equal(&b.edge(e)?.interval, 0.0) {
            return Ok(false);
        }
        let (lo, hi) = (s.interval.min(), s.interval.max());
        for weight in SAMPLES {
            let t = lo + (hi - lo) * weight;
            let at = uv.evaluate_point(k, t)?;
            let on_surface = surface.evaluate_point(k, Point2::new(at.x, at.y))?;
            if !coincident(curve.evaluate_point(k, t)?, on_surface, b.tolerance()) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn repair(
    exec: &mut OffsetExecutive,
    k: &mut Kernel,
    b: &mut Brep,
    n: &mut OffsetNotifications<'_>,
) -> Result<()> {
    for f in bp1::faces(b, None)? {
        let Some(chain) = plan(k, b, f)? else {
            continue;
        };
        let Some(signed_use) = super::cancel::signed_sheet_faceuse(b, f)? else {
            continue;
        };
        let original_orientation = b.faceuse(signed_use)?.orientation;
        // Span parameters belong to the 3D edges. Independently parameterized
        // UV trims cannot be shortened at those parameters without a mapping.
        for s in &chain {
            eu3::get_or_create_uv_trim_curve(k, b, s.source, false, false, true)?.status?;
        }
        if !parameters_agree(k, b, f, &chain)? {
            continue;
        }
        let mut input = FaceCurvesInput {
            loops: vec![chain.len()],
            ..FaceCurvesInput::default()
        };
        let mut curves = Vec::new();
        let mut uvs = Vec::new();
        let mut old_edges = Vec::new();
        for s in &chain {
            let e = required(eu::edge(b, s.source)?, EDGE_MISSING, f)?;
            let mut curve = required(b.edge(e)?.curve.as_ref(), CURVE_MISSING, f)?.copy(k)?;
            let mut interval = s.interval;
            curve.trim(k, &mut interval, TrimMode::Notify)?;
            curves.push(Some(curve));
            eu3::get_or_create_uv_trim_curve(k, b, s.source, false, false, true)?.status?;
            let mut uv = required(eu::uv_trim_curve(b, s.source)?.0, UV_MISSING, f)?.copy(k)?;
            let mut interval = s.interval;
            uv.trim(k, &mut interval, TrimMode::Notify)?;
            uvs.push(uv);
            input.orientations.push(eu::orientation(b, s.source)?);
            old_edges.push(e);
        }
        if original_orientation == OrientType::Opposite {
            curves.reverse();
            uvs.reverse();
            old_edges.reverse();
            input.orientations.reverse();
            for orientation in &mut input.orientations {
                *orientation = if *orientation == OrientType::Same {
                    OrientType::Opposite
                } else {
                    OrientType::Same
                };
            }
        }
        input.curves_3d = Some(curves);
        input.uv_curves = Some(uvs);
        let surface = required(
            b.face(f)?.surface.as_ref(),
            "a spur face has no surface, though every offset face carries one",
            f,
        )?
        .copy(k)?;
        let domain = b.face(f)?.uv_domain;
        let region = b.infinite_region();
        let mut stores = NotificationStores {
            records: &mut *n.records,
            attributes: &mut *n.attributes,
            key_of: n.result_key_of,
        };
        NotifyState::from_kernel_stores(&mut stores).with_kernel_env(k, |k, env| {
            let mut owners: Vec<Option<EdgeId>> = Vec::new();
            let key_of = env.key_of;
            let made = make_face_with_curve_owners(
                k,
                b,
                region,
                input,
                surface,
                &domain,
                original_orientation,
                None,
                &|id| key_of(EntityRef::Face(id)),
                &mut owners,
                env,
            )?;
            let new = required(
                made.face,
                "rebuilding a spur face made no face, though its loop of curves is closed",
                f,
            )?;
            exec.replace_face_relations(f, &[new])?;
            let mut pairs = vec![(EntityRef::Face(f), EntityRef::Face(new))];
            for (&old, new) in old_edges.iter().zip(owners) {
                let Some(new) = new else { continue };
                pairs.push((EntityRef::Edge(old), EntityRef::Edge(new)));
                let old_vertices = [edge::start_vertex(b, old)?, edge::end_vertex(b, old)?];
                let new_vertices = [edge::start_vertex(b, new)?, edge::end_vertex(b, new)?];
                for old in old_vertices.into_iter().flatten() {
                    for new in new_vertices.into_iter().flatten() {
                        if coincident(b.vertex(old)?.point, b.vertex(new)?.point, b.tolerance()) {
                            pairs.push((EntityRef::Vertex(old), EntityRef::Vertex(new)));
                        }
                    }
                }
            }
            pairs.sort_unstable();
            pairs.dedup();
            for (source, target) in pairs {
                topology::notify(
                    env.records,
                    env.attributes,
                    env.context.as_deref_mut(),
                    NotifyOperation::Copy,
                    (env.key_of)(source),
                    Some((env.key_of)(target)),
                    None,
                    None,
                )?;
            }
            let editing = b.editing_enabled();
            bp5::remove_faces_notified(k, b, editing, &[f], env)
        })?;
    }
    b.refresh_cached_topology();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_that_cross_in_their_interiors_touch() {
        let a = (Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 4.0, 0.0));
        let c = (Point3::new(0.0, 4.0, 0.0), Point3::new(6.0, 0.0, 0.0));
        assert!(segments_touch(a.0, a.1, c.0, c.1, 1e-7));
    }

    #[test]
    fn separate_and_parallel_segments_do_not_touch() {
        let p = Point3::new(0.0, 0.0, 0.0);
        let q = Point3::new(4.0, 0.0, 0.0);
        let above = Point3::new(0.0, 1.0, 0.0);
        let above_end = Point3::new(4.0, 1.0, 0.0);
        assert!(!segments_touch(p, q, above, above_end, 1e-7));
        let far = Point3::new(9.0, 0.0, 0.0);
        let far_end = Point3::new(12.0, 0.0, 0.0);
        assert!(!segments_touch(p, q, far, far_end, 1e-7));
        assert!(segments_touch(
            p,
            q,
            Point3::new(3.0, 0.0, 0.0),
            far_end,
            1e-7
        ));
    }
}
