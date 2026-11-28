//! Plane sections of a loft between translated spline profiles. If each row has a constant signed
//! height and the heights have opposite signs, the plane equation on the net
//! is linear in the loft parameter and independent of the profile parameter.
//! The isocurve keeps the knots of the profile, including across joined spans.

use super::p2::ClipPurpose;
use super::Plane;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::Result;
use crate::geom::expansion as exact;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, IsoCurveOptions, SurfParamType};
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::Direction;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType};

fn height(plane: &Plane, p: CPoint) -> Option<Vec<f64>> {
    if (p.w != NOW && p.w != 1.0) || p.z == NOZ {
        return None;
    }
    let o = plane.position().origin();
    let n = plane.position().z_axis();
    let component = |x, origin, normal| exact::scale(&exact::sub(&[x], &[origin]), normal);
    let xy = exact::add(&component(p.x, o.x, n.x)?, &component(p.y, o.y, n.y)?);
    Some(exact::add(&xy, &component(p.z, o.z, n.z)?))
}

fn iso_parameter(spline: &BSplineSurface, plane: &Plane) -> Option<(SurfParamType, f64, bool)> {
    let net = spline.gw_nurb()?;
    let (direction, knots, rows, last) = match (net.p(), net.n(), net.q(), net.m()) {
        (1, rows, degree, last) if rows >= 1 && degree > 1 => {
            (SurfParamType::U, net.u_knots(), rows, last)
        }
        (degree, last, 1, rows) if rows >= 1 && degree > 1 => {
            (SurfParamType::V, net.v_knots(), rows, last)
        }
        _ => return None,
    };
    let multi_span = rows > 1;
    // Multi-span degree-1 translated extrusions arise when extending a spline wall.
    // Restrict the multi-span fast path to closed profiles (e.g. periodic spline loops)
    // so open extended extrusions stay on the general surface intersector.
    if multi_span {
        let profile_dir = if direction == SurfParamType::U {
            Direction::V
        } else {
            Direction::U
        };
        if !net.is_closed(profile_dir) {
            return None;
        }
    }
    // A clamped degree-1 knot vector with `rows + 1` control rows has length `rows + 3`:
    // `[k_0, k_0, k_1, ..., k_{rows-1}, k_rows, k_rows]`, with strictly increasing spans.
    if knots.len() != rows.checked_add(3)? {
        return None;
    }
    #[allow(clippy::float_cmp)] // clamped end knots repeated as stored
    let clamped = knots.first() == knots.get(1) && knots.get(rows + 1) == knots.get(rows + 2);
    if !clamped {
        return None;
    }
    let span_knots = knots.get(1..=rows + 1)?;
    if !span_knots.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
        return None;
    }
    let at = |row: usize, i: usize| -> Option<CPoint> {
        if direction == SurfParamType::U {
            net.cpt(row, i).ok()
        } else {
            net.cpt(i, row).ok()
        }
    };
    // The profile and its translates have the same knots and derivatives. On a
    // twisted ruled surface the rails used to construct adjoining rounds also
    // depend on the surface parameterization; keep their usual intersection.
    let displacement = |p: CPoint, q: CPoint| {
        [(p.x, q.x), (p.y, q.y), (p.z, q.z)].map(|(x, y)| exact::sub(&[y], &[x]))
    };
    let mut heights: Vec<Vec<f64>> = Vec::new();
    for row in 0..=rows {
        heights.push(height(plane, at(row, 0)?)?);
    }
    let first_h = heights.first()?;
    let last_h = heights.last()?;
    if exact::sign(first_h) * exact::sign(last_h) != -1 {
        return None;
    }
    let overall = exact::sign(&exact::sub(last_h, first_h));
    if overall == 0 {
        return None;
    }
    for w in heights.windows(2) {
        let [a, b] = w else {
            return None;
        };
        if exact::sign(&exact::sub(b, a)) != overall {
            return None;
        }
    }
    for row in 0..rows {
        let h_row = heights.get(row)?;
        let shift = displacement(at(row, 0)?, at(row + 1, 0)?);
        for i in 1..=last {
            let p = at(row, i)?;
            let q = at(row + 1, i)?;
            if (q.w != NOW && q.w != 1.0)
                || q.z == NOZ
                || !exact::is_zero(&exact::sub(&height(plane, p)?, h_row))
                || !displacement(p, q)
                    .iter()
                    .zip(&shift)
                    .all(|(delta, reference)| exact::is_zero(&exact::sub(delta, reference)))
            {
                return None;
            }
        }
    }
    let lo = *span_knots.first()?;
    let hi = *span_knots.last()?;
    for row in 0..rows {
        let a = heights.get(row)?;
        let b = heights.get(row + 1)?;
        let sa = exact::sign(a);
        let sb = exact::sign(b);
        let k0 = *span_knots.get(row)?;
        let k1 = *span_knots.get(row + 1)?;
        if sa == 0 {
            return (k0 > lo && k0 < hi).then_some((direction, k0, multi_span));
        }
        if sa * sb == -1 {
            let fraction = exact::to_f64(a) / exact::to_f64(&exact::sub(a, b));
            let t = (k1 - k0).mul_add(fraction, k0);
            return (t > k0 && t < k1).then_some((direction, t, multi_span));
        }
    }
    None
}

fn section_uv_curves(
    plane: &Plane,
    direction: SurfParamType,
    t: f64,
    curve: &Curve,
) -> Result<Option<(Curve, Curve)>> {
    let ivl = curve.natural_interval();
    let (s0, s1) = (ivl.min(), ivl.max());
    if !(s0 < s1 && s0.is_finite() && s1.is_finite()) {
        return Ok(None);
    }
    let (p0, p1) = if direction == SurfParamType::U {
        (CPoint::new(t, s0, NOZ, NOW), CPoint::new(t, s1, NOZ, NOW))
    } else {
        (CPoint::new(s0, t, NOZ, NOW), CPoint::new(s1, t, NOZ, NOW))
    };
    let spline_uv = Curve::from(BSplineCurve::from_nurb(
        NurbCurve::new(vec![p0, p1], 1, vec![s0, s0, s1, s1])?,
        2,
        false,
    ));
    let CurveKind::BSpline(bsp) = curve.kind() else {
        return Ok(None);
    };
    let Some(nurb) = bsp.gw_nurb() else {
        return Ok(None);
    };
    let mut uv_pw = Vec::new();
    for &pt in nurb.cpts() {
        if (pt.w != NOW && pt.w != 1.0) || pt.z == NOZ {
            return Ok(None);
        }
        let uv = plane.project_point_to_uv_domain(Point3::new(pt.x, pt.y, pt.z));
        if !(uv.x.is_finite() && uv.y.is_finite()) {
            return Ok(None);
        }
        uv_pw.push(CPoint::new(uv.x, uv.y, NOZ, NOW));
    }
    let plane_uv = Curve::from(BSplineCurve::from_nurb(
        NurbCurve::new(uv_pw, nurb.degree(), nurb.knots().to_vec())?,
        2,
        false,
    ));
    Ok(Some((spline_uv, plane_uv)))
}

pub(crate) fn intersect(
    k: &Kernel,
    spline: &BSplineSurface,
    domain: &Extent2d,
    plane: &Plane,
    plane_domain: &Extent2d,
    tolerance: f64,
) -> Result<Option<SurfaceIntersection>> {
    let Some((direction, t, multi_span)) = iso_parameter(spline, plane) else {
        return Ok(None);
    };
    let interval = if direction == SurfParamType::U {
        domain.u_interval()
    } else {
        domain.v_interval()
    };
    let natural = spline.natural_uv_domain();
    let interior = interval.min() < t && t < interval.max();
    if !(interior
        && natural.contains_point(domain.min(), 0.0)
        && natural.contains_point(domain.max(), 0.0))
    {
        return Ok(None);
    }
    let iso = spline.create_iso_parametric_curve(
        k,
        direction,
        t,
        IsoCurveOptions {
            domain: Some(*domain),
            uv_iso_curve: false,
        },
    )?;
    let clip = plane.clip_curve(
        k,
        plane_domain,
        &Curve::from(iso.curve),
        tolerance,
        ClipPurpose::Section,
    )?;
    if !clip.proper_crossings || clip.curves.is_empty() {
        return Ok(None);
    }
    let mut result = SurfaceIntersection::default();
    for curve in clip.curves {
        let Some(bound) = plane
            .curve_deviation(&curve)
            .filter(|&gap| gap <= tolerance)
        else {
            return Ok(None);
        };
        let kind = if curve.is_degenerate(k, EFF_ZERO, None)? {
            TsectCurveType::Touching
        } else {
            TsectCurveType::Crossing
        };
        let (uv1, uv2) = if multi_span {
            if kind == TsectCurveType::Touching {
                return Ok(None);
            }
            let Some((spline_uv, plane_uv)) = section_uv_curves(plane, direction, t, &curve)?
            else {
                return Ok(None);
            };
            (Some(spline_uv), Some(plane_uv))
        } else {
            (None, None)
        };
        result.curves_3d.push(curve);
        result.curve_types.push(kind);
        result.deviations.push(bound);
        result.surface1_uv_curves.push(uv1);
        result.surface2_uv_curves.push(uv2);
    }
    Ok(Some(result))
}
