//! Unit directions without an absolute model-length cutoff. Surface normals are
//! dimensionless: scaling a valid surface must not turn its normal into +Z.
use super::{Vec3, MAX_SPLINE_DEGREE};
use crate::eval::vec::VecExt as _;
fn direction(v: Vec3) -> Option<Vec3> {
    let scale = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if !(scale > 0.0 && scale.is_finite()) {
        return None;
    }
    let v = v / scale;
    Some(v / v.len())
}
pub(super) fn normal(a: Vec3, b: Vec3) -> Option<Vec3> {
    let n = direction(a)?.cross(direction(b)?);
    if n.len2() > 1e-20 {
        direction(n)
    } else {
        None
    }
}

/// A clamped rational boundary can be one geometric point even when its
/// homogeneous controls differ in weight. Quotient cancellation then leaves a
/// tiny nonzero tangent after placement. Recognize that pole for normal sampling
/// only; the surface point, derivatives and CAD validation stay unchanged.
/// Inspect only the active degree+1 controls, never an unbounded full row.
pub(super) fn collapsed_boundary(surface: &super::EvalSurface, u: f64, v: f64) -> bool {
    let super::EvalSurface::Nurbs {
        degree,
        knots,
        count,
        ctrl,
        ..
    } = surface
    else {
        return false;
    };
    let uv = [u, v];
    let collapsed = |d: usize| -> Option<bool> {
        let (p, n, t) = (*degree.get(d)?, *count.get(d)?, *uv.get(d)?);
        let k = knots.get(d)?;
        let row = if t == *k.get(p)? && k.get(..=p)?.iter().all(|x| *x == t) {
            0
        } else if t == *k.get(n)? && k.get(n..)?.iter().all(|x| *x == t) {
            n.checked_sub(1)?
        } else {
            return Some(false);
        };
        let other = 1usize.checked_sub(d)?;
        let p = *degree.get(other)?;
        if !(1..=MAX_SPLINE_DEGREE).contains(&p) {
            return Some(false);
        }
        let span = super::span_of(knots.get(other)?, p, *count.get(other)?, *uv.get(other)?);
        let width = *count.first()?;
        let inward = if row == 0 { 1 } else { row.checked_sub(1)? };
        let at = |i: usize, row: usize| -> Option<Vec3> {
            let index = if d == 0 {
                i.checked_mul(width)?.checked_add(row)?
            } else {
                row.checked_mul(width)?.checked_add(i)?
            };
            let &[x, y, z, w] = ctrl.get(index)?;
            let point = Vec3::new(x / w, y / w, z / w);
            point.is_finite().then_some(point)
        };
        let first = span.checked_sub(p)?;
        let anchor = at(first, row)?;
        let mut scale = 0.0_f64;
        let mut deviation = 0.0_f64;
        let max_component = |v: Vec3| v.x.abs().max(v.y.abs()).max(v.z.abs());
        for i in first..=span {
            let point = at(i, row)?;
            deviation = deviation.max(max_component(point - anchor));
            scale = scale.max(max_component(at(i, inward)? - point));
        }
        Some(scale.is_finite() && deviation <= 64.0 * f64::EPSILON * scale)
    };
    collapsed(0).unwrap_or(false) || collapsed(1).unwrap_or(false)
}
