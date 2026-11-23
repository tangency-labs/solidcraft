//! Certificates for stored linear rational pcurves, after joining and affine maps.
//! For rational linear H/w and G/v, Hv-Gw is quadratic Bernstein with
//! coefficients H0v0-G0w0, (H0v1+H1v0-G0w1-G1w0)/2, H1v1-G1w1.
//! Its convex hull, divided by positive lower weight bounds, bounds the error.
//! Evaluation rounding follows the `gamma_n` absolute-error estimates in Higham,
//! *The accuracy of floating point summation* (1993), §2,
//! <https://nhigham.com/wp-content/uploads/2023/10/high93s.pdf>.

use super::{bad, Curve, Ivl, Result, Wall};
use crate::blend::fillet_section::interval::linear_basis_quotient_rounding;
use crate::nurbs::math::gamma;

pub(super) type Homogeneous = [Ivl; 3];
pub(super) type Span = [Homogeneous; 2];

/// Exact stored knot runs. The canonical export groups nearby knots using
/// `EFF_ZERO`, which is unsuitable for certifying a small but nonzero span.
pub(super) fn linear_knots(
    spline: &crate::nurbs::b_spline_curve::BSplineCurve,
) -> Result<(Vec<f64>, Vec<usize>)> {
    let raw = spline
        .gw_nurb()
        .ok_or_else(|| bad("missing exact pcurve knots"))?;
    if raw.degree() != 1 {
        return Err(bad("pcurve certificate needs degree one"));
    }
    let mut knots = Vec::new();
    let mut multiplicities = Vec::new();
    for &value in raw.knots() {
        if !value.is_finite() || knots.last().is_some_and(|&last| value < last) {
            return Err(bad("pcurve knots must be finite and ordered"));
        }
        match (knots.last(), multiplicities.last_mut()) {
            (Some(&last), Some(count)) if value == last => *count += 1,
            _ => {
                knots.push(value);
                multiplicities.push(1_usize);
            }
        }
    }
    Ok((knots, multiplicities))
}

/// Convert the shared interval primitive's whole-line refusal to this operation's
/// typed error. Ideal pcurve coefficients never assume a libm precision bound.
pub(super) fn tan(x: Ivl) -> Result<Ivl> {
    let value = crate::blend::fillet_section::interval::tan_certified(x);
    if !value.lo.is_finite() || !value.hi.is_finite() {
        return Err(bad("pcurve tangent could not be enclosed"));
    }
    Ok(value)
}

pub(super) fn sin_cos(x: Ivl) -> Result<(Ivl, Ivl)> {
    let (s, c) = crate::blend::fillet_section::interval::sin_cos_certified(x);
    if ![s.lo, s.hi, c.lo, c.hi].iter().all(|v| v.is_finite()) {
        return Err(bad("polar angle could not be enclosed"));
    }
    Ok((s, c))
}

pub(super) fn point(x: Ivl, y: Ivl, weight: Ivl) -> Homogeneous {
    [x * weight, y * weight, weight]
}

pub(super) fn angle_parameter(wall: &Wall<'_>, angle: f64) -> Result<Ivl> {
    let (a, b) = super::span(wall.definition, angle)?;
    let width = Ivl::from(b) - Ivl::from(a);
    let mid = (Ivl::from(a) + Ivl::from(b)) / Ivl::from(2.0);
    Ok(mid
        + width / Ivl::from(2.0) * tan((Ivl::from(angle) - mid) / Ivl::from(2.0))?
            / tan(width / Ivl::from(4.0))?)
}

fn rounding(n: usize) -> Ivl {
    // n is one of 1, 3: n*UNIT and 1-n*UNIT are exact, so the
    // shared gamma's sole rounded division is enclosed by checked.
    let value = gamma(n);
    Ivl::checked(value, value)
}

/// Bound actual degree-one pcurve evaluation, then a+b*uv, against ideal
/// homogeneous spans in the mapped coordinates. The caller supplies each
/// exact ideal span at the actual stored knots, not independently rounded knots.
pub(super) fn mapped_error(
    curve: &Curve,
    ideal: &[Span],
    map: ([f64; 2], [f64; 2], bool),
) -> Result<[f64; 2]> {
    let spline = curve
        .b_spline_base()
        .ok_or_else(|| bad("pcurve is not a B-spline"))?;
    let (knots, multiplicities) = linear_knots(spline)?;
    let controls = spline.control_points()?;
    let rational = spline.is_rational()?;
    let internal = multiplicities
        .iter()
        .skip(1)
        .take(knots.len().saturating_sub(2));
    let expected_controls = internal
        .clone()
        .try_fold(2_usize, |count, &m| count.checked_add(m));
    if knots.len() != ideal.len() + 1
        || multiplicities.first() != Some(&2)
        || multiplicities.last() != Some(&2)
        || internal.clone().any(|&m| m != 1 && m != 2)
        || expected_controls != Some(controls.len())
    {
        return Err(bad("pcurve certificate needs linear Bezier spans"));
    }
    let (a, b, swapped) = map;
    if a.iter().chain(b.iter()).any(|v| !v.is_finite()) || b.contains(&0.0) {
        return Err(bad("pcurve certificate has an invalid affine map"));
    }
    let mut error = [0.0_f64; 2];
    let tiny = Ivl::from(f64::from_bits(1));
    let mut control = 0_usize;
    for ((knots, desired), &advance) in knots
        .windows(2)
        .zip(ideal)
        .zip(multiplicities.iter().skip(1))
    {
        let end = control
            .checked_add(2)
            .ok_or_else(|| bad("pcurve control count overflowed"))?;
        let pair = controls
            .get(control..end)
            .ok_or_else(|| bad("pcurve certificate is missing controls"))?;
        control = control
            .checked_add(advance)
            .ok_or_else(|| bad("pcurve control count overflowed"))?;
        let ([p0, p1], [k0, k1]) = (pair, knots) else {
            return Err(bad("pcurve certificate is missing a span"));
        };
        let raw = [p0, p1].map(|p| {
            let uv = if swapped {
                [Ivl::from(p.y), Ivl::from(p.x)]
            } else {
                [Ivl::from(p.x), Ivl::from(p.y)]
            };
            (uv, Ivl::from(if rational { p.w } else { 1.0 }))
        });
        let [(raw0, w0), (raw1, w1)] = raw;
        let [g0, g1] = *desired;
        let [gx0, gy0, v0] = g0;
        let [gx1, gy1, v1] = g1;
        let wmin = w0.lo.min(w1.lo);
        let wmax = w0.hi.max(w1.hi);
        let vmin = v0.lo.min(v1.lo);
        let width = Ivl::from(*k1) - Ivl::from(*k0);
        if !wmin.is_finite()
            || wmin <= 0.0
            || !vmin.is_finite()
            || vmin <= 0.0
            || width.lo <= 0.0
            || !width.hi.is_finite()
        {
            return Err(bad(
                "pcurve weights or knot width have no positive finite bound",
            ));
        }
        let en = linear_basis_quotient_rounding(width);
        if !en.lo.is_finite() || !en.hi.is_finite() {
            return Err(bad(
                "pcurve basis denominator is not proved normal and finite",
            ));
        }
        // crv_eval: two products and one nontrivial addition for each homogeneous
        // coordinate (the first addition to zero is exact). Its absolute rounding
        // contribution is at most four smallest subnormals (including propagation
        // of product errors through the sum), in addition to gamma_3.
        let factor = Ivl::from(2.0) * en + rounding(3) * (Ivl::from(1.0) + Ivl::from(2.0) * en);
        let ew = if rational {
            factor * Ivl::from(wmax) + Ivl::from(4.0) * tiny
        } else {
            Ivl::from(0.0)
        };
        let denominator = Ivl::from(wmin) - ew;
        if denominator.lo < f64::MIN_POSITIVE {
            return Err(bad(
                "pcurve evaluation denominator is not proved normal and positive",
            ));
        }
        let coordinates = raw0
            .into_iter()
            .zip(raw1)
            .zip([gx0, gy0].into_iter().zip([gx1, gy1]));
        for ((((h0, h1), (g0, g1)), (a, b)), out) in
            coordinates.zip(a.into_iter().zip(b)).zip(error.iter_mut())
        {
            let mapped0 = Ivl::from(a) * w0 + Ivl::from(b) * h0;
            let mapped1 = Ivl::from(a) * w1 + Ivl::from(b) * h1;
            let numerator = [
                mapped0 * v0 - g0 * w0,
                (mapped0 * v1 + mapped1 * v0 - g0 * w1 - g1 * w0) / Ivl::from(2.0),
                mapped1 * v1 - g1 * w1,
            ]
            .into_iter()
            .map(Ivl::mag)
            .fold(0.0_f64, f64::max);
            let coefficient_error = Ivl::from(numerator) / (Ivl::from(wmin) * Ivl::from(vmin));
            let hmax = h0.mag().max(h1.mag());
            let eh = factor * Ivl::from(hmax) + Ivl::from(4.0) * tiny;
            let uvmax = Ivl::from(hmax) / Ivl::from(wmin);
            let evaluation = (eh + uvmax * ew) / denominator
                + rounding(1) * (Ivl::from(hmax) + eh) / denominator
                + tiny;
            // Mapping is exact here; its actual arithmetic belongs to the full
            // checked S evaluation allowance composed once in wall_error.
            let mapped_rounding = Ivl::from(b.abs()) * evaluation;
            let bound = (coefficient_error + mapped_rounding).hi;
            if !bound.is_finite() {
                return Err(bad("pcurve certificate overflowed"));
            }
            *out = (*out).max(bound);
        }
    }
    Ok(error)
}

pub(super) fn wall_error(wall: &Wall<'_>, curve: &Curve, ideal: &[Span]) -> Result<f64> {
    wall_error_with_deviation(wall, curve, ideal, [0.0; 2])
}

/// Additional deviation is an independently proved full-span error between the
/// ideal linear pcurve and its nonlinear definition-coordinate reference.
pub(super) fn wall_error_with_deviation(
    wall: &Wall<'_>,
    curve: &Curve,
    ideal: &[Span],
    deviation: [f64; 2],
) -> Result<f64> {
    let mapped = mapped_error(curve, ideal, wall.definition.definition_map())?;
    let error = std::array::from_fn::<_, 2, _>(|i| {
        (Ivl::from(mapped.get(i).copied().unwrap_or(f64::INFINITY))
            + Ivl::from(deviation.get(i).copied().unwrap_or(f64::INFINITY)))
        .hi
    });
    let mut w = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
    let mut t = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
    for point in ideal.iter().flatten() {
        w = w.hull(point[0] / point[2]);
        t = t.hull(point[1] / point[2]);
    }
    let domain = wall.definition.profile().natural_interval();
    if !domain.min().is_finite() || !domain.max().is_finite() || domain.min() >= domain.max() {
        return Err(bad("pcurve source has invalid profile bounds"));
    }
    let clamped_w = (Ivl::from(domain.min()) - w)
        .hi
        .max((w - Ivl::from(domain.max())).hi)
        .max(0.0);
    w = w + Ivl::new(-error[0], error[0]);
    w = w.hull(Ivl::new(
        crate::math::clamp_between(w.lo, domain.min(), domain.max()),
        crate::math::clamp_between(w.hi, domain.min(), domain.max()),
    ));
    let width = Ivl::from(wall.source[1]) - Ivl::from(wall.source[0]);
    let source = (w - Ivl::from(wall.source[0])) / width;
    let [profile, turn] = wall.section.speed_upper_bounds(source)?;
    let mut turn_rate = 0.0_f64;
    let mut angles = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
    for pair in wall.definition.stations().windows(2) {
        let [a, b] = *pair else { continue };
        angles = angles.hull(Ivl::new(a, b));
        let span = Ivl::from(b) - Ivl::from(a);
        turn_rate = turn_rate.max((Ivl::from(4.0) * tan(span / Ivl::from(4.0))? / span).hi);
    }
    let clamped_t = (Ivl::from(angles.lo) - t)
        .hi
        .max((t - Ivl::from(angles.hi)).hi)
        .max(0.0);
    let outside_angle = Ivl::from(turn_rate) * Ivl::from(clamped_t);
    angles = angles + Ivl::new(-outside_angle.hi, outside_angle.hi);
    let bound = (Ivl::from(profile) / width * (Ivl::from(error[0]) + Ivl::from(clamped_w))
        + Ivl::from(turn) * Ivl::from(turn_rate) * (Ivl::from(error[1]) + Ivl::from(clamped_t))
        + Ivl::from(wall.section.unit_axis_orbit_deviation(source, angles)?)
        + Ivl::from(wall.definition.runtime_evaluation_allowance()?))
    .hi;
    if !bound.is_finite() {
        return Err(bad("pcurve physical error is not finite"));
    }
    Ok(bound)
}
