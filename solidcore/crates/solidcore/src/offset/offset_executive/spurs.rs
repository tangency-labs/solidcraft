//! Rebuild a planar loop after cancelling one reversed straight overlap.
//!
//! A bounded, conservative certificate: exactly one overlapping line pair, and
//! precisely one unambiguous remaining closed cycle. Curved stretches are retained.
//! Ambiguous incidence or multiple cycles leaves the face unchanged. No recursion.
//! Upstream: none (regularized inner offset boundaries).

use super::p2::OffsetNotifications;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::CurveKind;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::object::NotifyOperation;
use crate::geom::solution_array::SolutionType;
use crate::kernel::Kernel;
use crate::math::Point3;
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

fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| SolidError::sm_err(OP))
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
}

fn span(k: &Kernel, b: &Brep, source: EdgeuseId, lo: f64, hi: f64) -> Result<Span> {
    let e = required(eu::edge(b, source)?)?;
    let curve = required(b.edge(e)?.curve.as_ref())?;
    let a = curve.evaluate_point(k, lo)?;
    let c = curve.evaluate_point(k, hi)?;
    let (start, end) = if eu::orientation(b, source)? == OrientType::Same {
        (a, c)
    } else {
        (c, a)
    };
    Ok(Span {
        source,
        interval: Extent1d::from_min_max(lo, hi),
        start,
        end,
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
        fu = required(eu::faceuse_mate(b, fu)?)?;
    }
    let loops = eu::faceuse_loopuses(b, fu, false, None)?;
    let [lu] = loops.as_slice() else {
        return Ok(None);
    };
    let uses = eu::loopuse_edgeuses(b, *lu)?;
    let mut overlap = None;
    for (i, &a) in uses.iter().enumerate() {
        k.check_cancelled(OP)?;
        let ea = required(eu::edge(b, a)?)?;
        let ca = required(b.edge(ea)?.curve.as_ref())?;
        let CurveKind::Line(la) = ca.kind() else {
            continue;
        };
        for &c in uses.iter().skip(i + 1) {
            let ec = required(eu::edge(b, c)?)?;
            let cc = required(b.edge(ec)?.curve.as_ref())?;
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
        let e = required(eu::edge(b, source)?)?;
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
                spans.push(span(k, b, source, domain.min(), lo)?);
            }
            if hi < domain.max() {
                spans.push(span(k, b, source, hi, domain.max())?);
            }
        } else {
            spans.push(span(k, b, source, domain.min(), domain.max())?);
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
    for _ in 0..bound {
        let tail = required(chain.last())?.end;
        let matches: Vec<_> = spans
            .iter()
            .enumerate()
            .filter_map(|(i, s)| coincident(tail, s.start, b.tolerance()).then_some(i))
            .collect();
        let [index] = matches.as_slice() else {
            return Ok(None);
        };
        chain.push(spans.remove(*index));
    }
    if !coincident(required(chain.last())?.end, first.start, b.tolerance()) {
        return Ok(None);
    }
    Ok(Some(chain))
}

pub(super) fn repair(k: &mut Kernel, b: &mut Brep, n: &mut OffsetNotifications<'_>) -> Result<()> {
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
        let mut matching_parameters = true;
        for s in &chain {
            let e = required(eu::edge(b, s.source)?)?;
            eu3::get_or_create_uv_trim_curve(k, b, s.source, false, false, true)?.status?;
            let uv = required(eu::uv_trim_curve(b, s.source)?.0)?;
            if !uv.natural_interval().are_equal(&b.edge(e)?.interval, 0.0) {
                matching_parameters = false;
                break;
            }
        }
        if !matching_parameters {
            continue;
        }
        if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
            eprintln!("spurs trace: repairing={f:?} spans={}", chain.len());
        }
        let mut input = FaceCurvesInput {
            loops: vec![chain.len()],
            ..FaceCurvesInput::default()
        };
        let mut curves = Vec::new();
        let mut uvs = Vec::new();
        let mut old_edges = Vec::new();
        for s in &chain {
            let e = required(eu::edge(b, s.source)?)?;
            let mut curve = required(b.edge(e)?.curve.as_ref())?.copy(k)?;
            let mut interval = s.interval;
            curve.trim(k, &mut interval, TrimMode::Notify)?;
            curves.push(Some(curve));
            eu3::get_or_create_uv_trim_curve(k, b, s.source, false, false, true)?.status?;
            let mut uv = required(eu::uv_trim_curve(b, s.source)?.0)?.copy(k)?;
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
        let surface = required(b.face(f)?.surface.as_ref())?.copy(k)?;
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
            let new = required(made.face)?;
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
