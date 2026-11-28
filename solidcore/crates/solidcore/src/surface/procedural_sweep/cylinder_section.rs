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
//! Deviations: none (no upstream counterpart). No recursion; the work is bounded: the roots in
//! closed form per profile span, [`SAMPLES`] evaluations and [`BISECTIONS`] steps per clip, and
//! the seam's samples and bisections per crossing.

use super::{Motion, ProceduralSweep};
use crate::curve::curve::Curve;
use crate::error::Result;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface::{IsoCurveOptions, SurfParamType};
use crate::surface::cone::Cone;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use crate::surface::surface::Surface;

/// Samples along a helix to check that its height runs one way.
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
    let w_lo = to_w(p_ivl.min()).min(to_w(p_ivl.max()));
    let w_hi = to_w(p_ivl.min()).max(to_w(p_ivl.max()));
    let Some(ws) = line_crossings(profile, o, k_axis, radius, w_lo, w_hi, tolerance)? else {
        return Ok(None);
    };
    let roots: Vec<f64> = ws.iter().map(|&w| (w - a[0]) / b[0]).collect();
    // No crossing found: leave the pair to the general intersection rather than answer that
    // there is none.
    if roots.is_empty() {
        return Ok(None);
    }
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
        // A cut at an end, or within rounding of one, is the end itself (a helix that starts or
        // stops on the seam).
        let lead = m.pitch() / std::f64::consts::TAU;
        let near = tolerance / (radius * radius + lead * lead).sqrt();
        let mut ends = vec![q0];
        for c in cuts {
            if c - ends.last().copied().unwrap_or(q0) > near && q1 - c > near {
                ends.push(c);
            }
        }
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
            let stretch_3d = iso.curve.natural_interval();
            result.curves_3d.push(Curve::from(iso.curve));
            result.curve_types.push(TsectCurveType::Crossing);
            result
                .deviations
                .push(residual.max(off_radius) + 2.0 * ARC_SLACK * radius + misaligned + rounding);
            // The wall's line, on the piece's own stretch, as the 3D curve is.
            let uv = match iso.uv_iso_curve {
                Some(mut line) => {
                    let mut stretch = stretch_3d;
                    line.trim(k, &mut stretch, TrimMode::Notify)?;
                    Some(line)
                }
                None => None,
            };
            result.surface1_uv_curves.push(uv);
            result.surface2_uv_curves.push(None);
        }
    }
    // Nothing within the faces: the general intersection decides.
    if result.curves_3d.is_empty() {
        return Ok(None);
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
    // The surface itself goes round the whole turn: its natural angular ends are one seam.
    let (lin_lo, lin_hi) = if linear_is_u {
        (domain.u_interval().min(), domain.u_interval().max())
    } else {
        (domain.v_interval().min(), domain.v_interval().max())
    };
    let lin = 0.5 * (lin_lo + lin_hi);
    let corner = |t: f64| {
        if linear_is_u {
            Point2::new(lin, t)
        } else {
            Point2::new(t, lin)
        }
    };
    let gap = (surface.evaluate_point(k, corner(whole.min()))?
        - surface.evaluate_point(k, corner(whole.max()))?)
    .length();
    let scale = 1.0 + (surface.evaluate_point(k, corner(whole.min()))? - origin).length();
    if !(gap <= 1e-12 * scale) {
        return Ok(None);
    }
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

/// The profile parameters in `[lo, hi]` where a profile of straight spans (degree 1) crosses
/// the cylinder of `radius` about the axis through `origin` along `axis`, in closed form: on a span
/// from `A` to `B`, with `a`, `d` the radial parts of `A − origin` and `B − A`, the crossings of
/// `|a + d s| = radius` are the roots of `|d|² s² + 2 a·d s + |a|² − radius² = 0` in `s ∈ [0, 1]`.
/// `None` (take the general way) for a profile of another degree; for a root at a span's end or
/// within `tolerance` of the stretch's ends, where a vertex or a face's edge meets the radius; for a
/// double root or one where the profile meets the radius at less than a thousandth of a radian
/// (a tangent or near-tangent contact); or for more than [`MAX_ROOTS`] crossings.
///
/// # Errors
///
/// Those of reading the profile.
fn line_crossings(
    profile: &BSplineCurve,
    origin: Point3,
    axis: Vec3,
    radius: f64,
    lo: f64,
    hi: f64,
    tolerance: f64,
) -> Result<Option<Vec<f64>>> {
    let c = profile.canonical()?;
    if c.degree != 1 || !c.weights.iter().all(|&w| w == 1.0) || !(lo < hi) {
        return Ok(None);
    }
    let radial = |v: Vec3| v - axis * v.dot(axis);
    let mut roots = Vec::new();
    for (i, pair) in c.control_points.windows(2).enumerate() {
        let [pa, pb] = *pair else {
            continue;
        };
        let (Some(&k0), Some(&k1)) = (c.knots.get(i), c.knots.get(i + 1)) else {
            return Ok(None);
        };
        let a = radial(pa - origin);
        let d = radial(pb - pa);
        let (qa, qb, qc) = (d.dot(d), 2.0 * a.dot(d), a.dot(a) - radius * radius);
        let length = d.length();
        if !(qa > 0.0) {
            // a span along the axis: on the radius everywhere or nowhere
            if (a.length() - radius).abs() <= tolerance {
                return Ok(None);
            }
            continue;
        }
        let disc = qb * qb - 4.0 * qa * qc;
        if !disc.is_finite() {
            return Ok(None);
        }
        // tangent or nearly: the radius touches the span (its foot within tolerance of it)
        let closest = (a - d * (a.dot(d) / qa)).length();
        if (closest - radius).abs() <= tolerance {
            return Ok(None);
        }
        if disc <= 0.0 {
            continue;
        }
        let root = disc.sqrt();
        // the two roots without cancellation
        let q = -0.5 * (qb + qb.signum() * root);
        for s in [q / qa, qc / q] {
            if !s.is_finite() {
                continue;
            }
            let reach = tolerance / length;
            if s < -reach || s > 1.0 + reach {
                continue;
            }
            if s <= reach || s >= 1.0 - reach {
                return Ok(None);
            }
            let w = k0 + (k1 - k0) * s;
            if w <= lo + tolerance || w >= hi - tolerance {
                if w > lo - tolerance && w < hi + tolerance {
                    return Ok(None);
                }
                continue;
            }
            // the profile's angle with the radius there: a grazing crossing goes the general way
            let at = a + d * s;
            let sine = (at.dot(d) / (at.length() * length)).abs();
            if !(sine > 1e-3) {
                return Ok(None);
            }
            roots.push(w);
        }
    }
    if roots.len() > MAX_ROOTS {
        return Ok(None);
    }
    roots.sort_by(f64::total_cmp);
    Ok(Some(roots))
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
        if !(ends <= slack
            && tangents <= slack
            && weights <= slack
            && w0 > 0.0
            && w1 > 0.0
            && w2 > 0.0
            && cos > 0.0)
        {
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
