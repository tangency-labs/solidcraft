//! Plane sections of a two-row spline loft. If each row has a constant signed
//! height and the heights have opposite signs, the plane equation on the net
//! is linear in the loft parameter and independent of the profile parameter.
//! The isocurve keeps the knots of the profile, including across joined spans.

use super::Plane;
use crate::curve::curve::Curve;
use crate::error::Result;
use crate::geom::expansion as exact;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_surface::{BSplineSurface, IsoCurveOptions, SurfParamType};
use crate::nurbs::math::{CPoint, NOW, NOZ};
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

fn iso_parameter(spline: &BSplineSurface, plane: &Plane) -> Option<(SurfParamType, f64)> {
    let net = spline.gw_nurb()?;
    let (direction, knots, last) = match (net.p(), net.n(), net.q(), net.m()) {
        (1, 1, degree, last) if degree > 1 => (SurfParamType::U, net.u_knots(), last),
        (degree, last, 1, 1) if degree > 1 => (SurfParamType::V, net.v_knots(), last),
        _ => return None,
    };
    let &[lo, start, end, hi] = knots else {
        return None;
    };
    if lo != start || hi != end || lo >= hi {
        return None;
    }
    let row_height = |row| {
        let at = |i| {
            let pt = if direction == SurfParamType::U {
                net.cpt(row, i)
            } else {
                net.cpt(i, row)
            };
            height(plane, pt.ok()?)
        };
        let h = at(0)?;
        (1..=last)
            .all(|i| at(i).is_some_and(|v| exact::is_zero(&exact::sub(&v, &h))))
            .then_some(h)
    };
    let a = row_height(0)?;
    let b = row_height(1)?;
    if exact::sign(&a) * exact::sign(&b) != -1 {
        return None;
    }
    let fraction = exact::to_f64(&a) / exact::to_f64(&exact::sub(&a, &b));
    let t = (hi - lo).mul_add(fraction, lo);
    (t > lo && t < hi).then_some((direction, t))
}

pub(crate) fn intersect(
    k: &Kernel,
    spline: &BSplineSurface,
    domain: &Extent2d,
    plane: &Plane,
    plane_domain: &Extent2d,
    tolerance: f64,
) -> Result<Option<SurfaceIntersection>> {
    let Some((direction, t)) = iso_parameter(spline, plane) else {
        return Ok(None);
    };
    let interval = if direction == SurfParamType::U {
        domain.u_interval()
    } else {
        domain.v_interval()
    };
    let natural = spline.natural_uv_domain();
    if !(interval.min() < t && t < interval.max())
        || !natural.contains_point(domain.min(), 0.0)
        || !natural.contains_point(domain.max(), 0.0)
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
    let clip = plane.clip_curve(k, plane_domain, &Curve::from(iso.curve), tolerance)?;
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
        result.curves_3d.push(curve);
        result.curve_types.push(kind);
        result.deviations.push(bound);
        result.surface1_uv_curves.push(None);
        result.surface2_uv_curves.push(None);
    }
    Ok(Some(result))
}
