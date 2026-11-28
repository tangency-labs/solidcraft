//! A screw-swept wall cut by a cylinder on the screw's axis (not upstream;
//! docs/design/screw-surface-record.md, the addendum): the exact intersection, in closed form.
//!
//! The wall's net turns every profile control point on an exact rational quadratic arc, and at
//! one net parameter across the turns every row turns by the same angle and lifts by the same
//! amount ([`super`]'s module docs: `B - S` is the same for every profile point). So the net's
//! point at `(w, t)` lies exactly as far from the axis as the profile's point `P(w)`. A cylinder of
//! radius `R` on the axis therefore meets the wall where `|P(w)|_axis = R`: at each such `w₀`, the
//! net's whole iso curve `B(w₀, ·)` lies on the cylinder, to the root's accuracy and rounding. The
//! intersection is those iso curves, clipped to the cylinder face's heights; on the wall each is a
//! straight line of its parameters.
//!
//! The cases taken: a live screw sweep with no radial law (a law moves the profile off its
//! radius as it turns), a cylinder whose axis is the screw's within `tolerance` over the wall's
//! reach, whose face goes round the whole turn, and profile crossings of the radius that are
//! transversal. Anything else is `None`, and the general intersection runs as before.
//!
//! Deviations: none (no upstream counterpart). No recursion; the work is bounded by the
//! profile's knot spans times [`SAMPLES`] evaluations and [`BISECTIONS`] steps per root.

use super::{Motion, ProceduralSweep};
use crate::curve::curve::Curve;
use crate::error::Result;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface::{IsoCurveOptions, SurfParamType};
use crate::surface::cone::Cone;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use crate::surface::surface::Surface;

/// Samples of the profile's distance from the axis per face, to bracket its crossings of the
/// radius.
const SAMPLES: usize = 256;

/// Bisection steps per crossing: enough to reach the last bit of a parameter.
const BISECTIONS: usize = 128;

/// How far, relative to the radius, a span may be from the conditions of an exact arc
/// ([`on_cylinder`]): rounding of the net's construction and of extracting the iso curve.
const ARC_SLACK: f64 = 1e-11;

/// The most crossings taken; a profile that crosses the radius more often goes the general way.
const MAX_ROOTS: usize = 8;

/// The wall `wall` (its face's parameters `domain`) cut by the cylinder `cylinder` (the surface
/// `other`, its face's parameters `other_domain`), when the module's cases hold; `None` otherwise.
///
/// # Errors
///
/// Those of evaluating the surfaces and of extracting an iso curve.
pub(crate) fn intersect(
    k: &Kernel,
    wall: (&ProceduralSweep, &BSplineSurface),
    domain: &Extent2d,
    cylinder: &Cone,
    other: &Surface,
    other_domain: &Extent2d,
    tolerance: f64,
) -> Result<Option<SurfaceIntersection>> {
    let (sweep, net) = wall;
    let Motion::Screw(m) = sweep.motion() else {
        return Ok(None);
    };
    if m.law().is_some() || !cylinder.is_cylinder() || !(tolerance > 0.0) {
        return Ok(None);
    }
    let natural = net.natural_uv_domain();
    if !(natural.contains_point(domain.min(), 0.0) && natural.contains_point(domain.max(), 0.0)) {
        return Ok(None);
    }
    // The cylinder's axis, its radius, and the heights and turn its face covers.
    let canonical = cylinder.canonical();
    let (c_origin, c_axis) = (canonical.origin.origin(), canonical.origin.z_axis());
    let radius = canonical.radius;
    let (k_axis, o) = (m.axis(), m.origin());
    let reach = radius + (o - c_origin).length();
    let off = (o - c_origin) - c_axis * (o - c_origin).dot(c_axis);
    let tilt = c_axis.cross(k_axis).length();
    if !(radius > 0.0 && tilt * reach <= 1e-3 * tolerance && off.length() <= 1e-3 * tolerance) {
        return Ok(None);
    }
    // How far the cylinder's axis may be from the screw's over the wall's reach: added to each
    // edge's deviation.
    let misaligned = off.length() + tilt * reach;
    let Some((h_lo, h_hi)) = face_heights(k, other, other_domain, c_origin, c_axis, cylinder)?
    else {
        return Ok(None);
    };
    // Which of the face's parameters is the profile's: (w, t) is (u, v), or (v, u) swapped.
    let (a, b, swapped) = sweep.definition_map();
    let (w_dir, p_ivl, q_ivl) = if swapped {
        (SurfParamType::V, domain.v_interval(), domain.u_interval())
    } else {
        (SurfParamType::U, domain.u_interval(), domain.v_interval())
    };
    let to_w = |p: f64| a[0] + b[0] * p;
    // The profile's distance from the axis less the radius, at the face's profile parameter.
    let profile = sweep.profile();
    let gap = |p: f64| -> Result<f64> {
        let q = profile.evaluate_point(to_w(p))? - o;
        Ok((q - k_axis * q.dot(k_axis)).length() - radius)
    };
    let Some(roots) = crossings(&gap, p_ivl.min(), p_ivl.max(), tolerance)? else {
        return Ok(None);
    };
    let mut result = SurfaceIntersection::default();
    for p0 in roots {
        let residual = gap(p0)?.abs();
        // Along the iso curve the height above the cylinder's origin; its stretch within the
        // face's heights is the intersection.
        let point = |q: f64| {
            let uv = if swapped {
                Point2::new(q, p0)
            } else {
                Point2::new(p0, q)
            };
            net.evaluate_point(uv)
        };
        let height = |q: f64| -> Result<f64> { Ok((point(q)? - c_origin).dot(c_axis)) };
        let Some((q0, q1)) = clip(&height, q_ivl.min(), q_ivl.max(), h_lo, h_hi)? else {
            continue;
        };
        // The cylinder's face is one period of its angle, its seam at the angle of its
        // domain's first corner: the helix is cut there, each piece within the face. The
        // angle is measured from opposite the seam, so the seam is where it wraps.
        let seam = {
            let q = other.evaluate_point(k, other_domain.min())? - c_origin;
            q - c_axis * q.dot(c_axis)
        };
        let Some(x_axis) = (seam.length() > 0.0).then(|| seam * (-1.0 / seam.length())) else {
            return Ok(None);
        };
        let y_axis = c_axis.cross(x_axis);
        let angle = |q: f64| -> Result<f64> {
            let r = point(q)? - c_origin;
            Ok(r.dot(y_axis).atan2(r.dot(x_axis)))
        };
        let samples = 8 * sweep.stations().len() + 64;
        let Some(cuts) = seam_cuts(&angle, q0, q1, samples)? else {
            return Ok(None);
        };
        let mut ends = vec![q0];
        ends.extend(cuts);
        ends.push(q1);
        for piece in ends.windows(2) {
            let [qa, qb] = *piece else {
                continue;
            };
            if !(qa < qb) {
                continue;
            }
            let iso_domain = if swapped {
                Extent2d::from_bounds(qa, p_ivl.min(), qb, p_ivl.max())
            } else {
                Extent2d::from_bounds(p_ivl.min(), qa, p_ivl.max(), qb)
            };
            let iso = net.create_iso_parametric_curve(
                k,
                w_dir,
                p0,
                IsoCurveOptions {
                    domain: Some(iso_domain),
                    uv_iso_curve: true,
                },
            )?;
            // The net's iso curve is on the net exactly. That it is on the cylinder is checked
            // on the curve itself, span by span (`on_cylinder`): a net that is not the
            // definition's construction fails it, and the general intersection runs.
            let Some(off_radius) = on_cylinder(&iso.curve, o, k_axis, radius)? else {
                return Ok(None);
            };
            let rounding = 64.0 * f64::EPSILON * (reach + h_hi.abs().max(h_lo.abs()));
            result.curves_3d.push(Curve::from(iso.curve));
            result.curve_types.push(TsectCurveType::Crossing);
            result
                .deviations
                .push(residual.max(off_radius) + misaligned + rounding);
            result.surface1_uv_curves.push(iso.uv_iso_curve);
            result.surface2_uv_curves.push(None);
        }
    }
    Ok(Some(result))
}

/// The heights above `origin` along `axis` the cylinder's face `domain` spans, when the face goes
/// round the whole turn (its angular range is its surface's natural one); `None` otherwise.
fn face_heights(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    origin: Point3,
    axis: Vec3,
    cylinder: &Cone,
) -> Result<Option<(f64, f64)>> {
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let linear_is_u = cylinder.canonical().swap_uv;
    let (turn, whole) = if linear_is_u {
        (domain.v_interval(), natural.v_interval())
    } else {
        (domain.u_interval(), natural.u_interval())
    };
    let span = (whole.max() - whole.min()).abs();
    let slack = 1e-12 * (1.0 + whole.max().abs().max(whole.min().abs()));
    let whole_turn = (turn.min() - whole.min()).abs() <= slack
        && (turn.max() - whole.max()).abs() <= slack
        && span > 0.0;
    if !whole_turn {
        return Ok(None);
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for uv in [domain.min(), domain.max()] {
        let h = (surface.evaluate_point(k, uv)? - origin).dot(axis);
        lo = lo.min(h);
        hi = hi.max(h);
    }
    Ok((lo.is_finite() && hi.is_finite() && lo < hi).then_some((lo, hi)))
}

/// The parameters in `[lo, hi]` where `f` crosses zero, each to the last bit by bisection of a
/// sign change among [`SAMPLES`] samples; `None` (take the general way) for a sample within
/// `tolerance` of zero without a sign change beside it (a tangent or near-tangent contact), for
/// a sample that does not evaluate to a finite number, or for more than [`MAX_ROOTS`] crossings.
fn crossings(
    f: &dyn Fn(f64) -> Result<f64>,
    lo: f64,
    hi: f64,
    tolerance: f64,
) -> Result<Option<Vec<f64>>> {
    if !(lo < hi) {
        return Ok(None);
    }
    let at = |i: usize| {
        if i == SAMPLES {
            hi
        } else {
            lo + (hi - lo) * (i as f64) / (SAMPLES as f64)
        }
    };
    let mut values = Vec::with_capacity(SAMPLES + 1);
    for i in 0..=SAMPLES {
        let v = f(at(i))?;
        if !v.is_finite() {
            return Ok(None);
        }
        values.push(v);
    }
    let mut roots = Vec::new();
    for i in 0..SAMPLES {
        let (Some(&va), Some(&vb)) = (values.get(i), values.get(i + 1)) else {
            continue;
        };
        if va == 0.0 || vb == 0.0 || (va < 0.0) == (vb < 0.0) {
            // no crossing inside: a sample this close to zero is a contact the bracket misses
            if va.abs() <= tolerance && (i == 0 || va != 0.0) && !crossing_beside(&values, i) {
                return Ok(None);
            }
            continue;
        }
        let (mut a, mut b, mut fa) = (at(i), at(i + 1), va);
        for _ in 0..BISECTIONS {
            let mid = 0.5 * (a + b);
            if !(a < mid && mid < b) {
                break;
            }
            let fm = f(mid)?;
            if fm == 0.0 {
                a = mid;
                b = mid;
                break;
            }
            if (fm < 0.0) == (fa < 0.0) {
                a = mid;
                fa = fm;
            } else {
                b = mid;
            }
        }
        roots.push(0.5 * (a + b));
        if roots.len() > MAX_ROOTS {
            return Ok(None);
        }
    }
    if let Some(&last) = values.last() {
        if last.abs() <= tolerance && !crossing_beside(&values, SAMPLES) {
            return Ok(None);
        }
    }
    Ok(Some(roots))
}

/// Whether sample `i` is the end of an interval in which the values change sign.
fn crossing_beside(values: &[f64], i: usize) -> bool {
    let sign_change = |j: usize| match (values.get(j), values.get(j + 1)) {
        (Some(&a), Some(&b)) => a != 0.0 && b != 0.0 && (a < 0.0) != (b < 0.0),
        _ => false,
    };
    sign_change(i) || (i > 0 && sign_change(i - 1))
}

/// The parameters in `[lo, hi]` where `height` lies within `[h_lo, h_hi]`, when that is one
/// interval and `height` runs one way over `[lo, hi]` (checked at [`SAMPLES`] samples); `None`
/// when no stretch lies within or the height does not run one way.
fn clip(
    height: &dyn Fn(f64) -> Result<f64>,
    lo: f64,
    hi: f64,
    h_lo: f64,
    h_hi: f64,
) -> Result<Option<(f64, f64)>> {
    let at = |i: usize| {
        if i == SAMPLES {
            hi
        } else {
            lo + (hi - lo) * (i as f64) / (SAMPLES as f64)
        }
    };
    let mut values = Vec::with_capacity(SAMPLES + 1);
    for i in 0..=SAMPLES {
        values.push(height(at(i))?);
    }
    let rising = values.windows(2).all(|w| matches!(w, [a, b] if a < b));
    let falling = values.windows(2).all(|w| matches!(w, [a, b] if a > b));
    if !(rising || falling) {
        return Ok(None);
    }
    // The parameter where the height reaches `h`, by bisection between `a` and `b`.
    let reach = |h: f64| -> Result<f64> {
        let (mut a, mut b) = (lo, hi);
        for _ in 0..BISECTIONS {
            let mid = 0.5 * (a + b);
            if !(a < mid && mid < b) {
                break;
            }
            let below = height(mid)? < h;
            if below == rising {
                a = mid;
            } else {
                b = mid;
            }
        }
        Ok(0.5 * (a + b))
    };
    let (Some(&first), Some(&last)) = (values.first(), values.last()) else {
        return Ok(None);
    };
    let (lowest, highest) = if rising { (first, last) } else { (last, first) };
    if highest <= h_lo || lowest >= h_hi {
        return Ok(None);
    }
    let start = if lowest < h_lo {
        Some(reach(h_lo)?)
    } else {
        None
    };
    let end = if highest > h_hi {
        Some(reach(h_hi)?)
    } else {
        None
    };
    let (q0, q1) = if rising {
        (start.unwrap_or(lo), end.unwrap_or(hi))
    } else {
        (end.unwrap_or(lo), start.unwrap_or(hi))
    };
    Ok((q0 < q1).then_some((q0, q1)))
}

/// How far `curve` lies from the cylinder of `radius` about the axis through `origin` along the
/// unit `axis`, when its projection onto the plane square to the axis is a chain of exact circular
/// arcs about the axis: degree 2, each Bézier span `(q0, q1, q2)` with weights `(w0, w1, w2)` having
/// its ends at `radius`, its middle point where the end tangents meet (`q0 · (q1 − q0) = 0`,
/// `q2 · (q1 − q2) = 0`) and `w1² / (w0 w2) = (1 + cos θ) / 2`, `θ` the arc's angle (the conditions of
/// a rational quadratic circle). Each holds to [`ARC_SLACK`] of the radius; the answer is the
/// largest end's distance from the radius. `None` for a curve of another form.
///
/// # Errors
///
/// Those of reading the curve.
fn on_cylinder(
    curve: &BSplineCurve,
    origin: Point3,
    axis: Vec3,
    radius: f64,
) -> Result<Option<f64>> {
    let c = curve.canonical()?;
    let n = c.control_points.len();
    let inner_double = c
        .knot_multiplicities
        .iter()
        .skip(1)
        .take(c.knot_multiplicities.len().saturating_sub(2))
        .all(|&m| m >= 2);
    if c.degree != 2 || n < 3 || n % 2 == 0 || !inner_double {
        return Ok(None);
    }
    let weight = |i: usize| -> f64 {
        if c.weights.is_empty() {
            1.0
        } else {
            c.weights.get(i).copied().unwrap_or(f64::NAN)
        }
    };
    let radial = |i: usize| -> Option<Vec3> {
        let q = *c.control_points.get(i)? - origin;
        Some(q - axis * q.dot(axis))
    };
    let slack = ARC_SLACK * radius;
    let mut worst = 0.0_f64;
    for s in 0..(n - 1) / 2 {
        let (Some(q0), Some(q1), Some(q2)) = (radial(2 * s), radial(2 * s + 1), radial(2 * s + 2))
        else {
            return Ok(None);
        };
        let (w0, w1, w2) = (weight(2 * s), weight(2 * s + 1), weight(2 * s + 2));
        let ends = (q0.length() - radius)
            .abs()
            .max((q2.length() - radius).abs());
        let tangents = (q0.dot(q1 - q0).abs() / radius).max(q2.dot(q1 - q2).abs() / radius);
        let cos = q0.dot(q2) / (radius * radius);
        let weights = (w1 * w1 / (w0 * w2) - 0.5 * (1.0 + cos)).abs() * radius;
        if !(ends <= slack && tangents <= slack && weights <= slack && w0 > 0.0 && w2 > 0.0) {
            return Ok(None);
        }
        worst = worst.max(ends);
    }
    Ok(Some(worst))
}

/// The parameters in `(lo, hi)` where `angle` (in `(-pi, pi]`) wraps through `pi`, the
/// cylinder's seam: found between `samples` samples, where the angle jumps by more than a half
/// turn, by bisection on the side it is continuous with. `None` for a step of more than a
/// quarter turn between samples that is not a wrap (the samples are too few to follow it).
fn seam_cuts(
    angle: &dyn Fn(f64) -> Result<f64>,
    lo: f64,
    hi: f64,
    samples: usize,
) -> Result<Option<Vec<f64>>> {
    use std::f64::consts::{FRAC_PI_2, PI};
    let at = |i: usize| {
        if i == samples {
            hi
        } else {
            lo + (hi - lo) * (i as f64) / (samples as f64)
        }
    };
    let mut cuts = Vec::new();
    let mut previous = angle(lo)?;
    for i in 1..=samples {
        let current = angle(at(i))?;
        let step = current - previous;
        if step.abs() > PI {
            let (mut a, mut b, fa) = (at(i - 1), at(i), previous);
            for _ in 0..BISECTIONS {
                let mid = 0.5 * (a + b);
                if !(a < mid && mid < b) {
                    break;
                }
                if (angle(mid)? - fa).abs() < PI {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            cuts.push(0.5 * (a + b));
        } else if step.abs() > FRAC_PI_2 {
            return Ok(None);
        }
        previous = current;
    }
    Ok(Some(cuts))
}
