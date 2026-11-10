//! Retained-parameter subdivision of rational screw spans.
//!
//! Prototype: constructs an untrusted refined net and encloses its exact-real
//! discrepancy from a screw definition, including saved-net mapping, lift and
//! recertified offset-profile terms. Runtime definition evaluation is not yet
//! certified; the candidate must not be attached to a production surface.
//! Allocations and interval comparison work are explicitly bounded; no recursion.

use crate::blend::fillet_section::interval::Ivl;
use crate::error::{Result, SolidError};

const OP: &str = "surface::procedural_sweep::refined_net";

/// Interval enclosures of the old arc's restricted quadratic denominator and
/// the factors that map a symmetric subarc back to the old parameter.
#[derive(Clone, Copy, Debug)]
pub(super) struct SubarcWeights {
    /// Restricted Bernstein weights, in old-parameter order.
    pub(super) old: [Ivl; 3],
    /// Homogeneous multipliers for a symmetric subarc, in the same order.
    pub(super) scale: [Ivl; 3],
    /// The symmetric subarc's middle weight, cos(half-angle).
    pub(super) middle: Ivl,
    /// Enclosure of tan(one quarter of the subarc turn).
    pub(super) quarter_tangent: Ivl,
}

/// Encloses the positive denominator weights on `[a,b]`, with `a,b` in the
/// old span's unit parameter. `tau` is the original definition's computed
/// `tan(half-angle/2)` value, not a separately rounded cosine.
///
/// The denominator of the original rational arc is a quadratic Bernstein
/// polynomial with weights `[1, (1-tau²)/(1+tau²), 1]`. Its restriction has
/// weights `D(a), D[a,b], D(b)`, where `D[a,b]` is its polar form. A symmetric
/// rational quadratic has weights `[1,c,1]`; scaling its homogeneous columns
/// by `[A,sqrt(A C),C]` gives `[A,B,C]` for `c=B/sqrt(A C)` and therefore the
/// same angle at the old parameter. Every arithmetic operation is enclosed.
///
/// # Errors
/// Refuses an invalid interval, a nonfinite or nonpositive input, a span outside
/// the screw builder's parameter range, or an unproved positive denominator.
pub(super) fn subarc_weights(tau: f64, a: f64, b: f64) -> Result<SubarcWeights> {
    let bad = |reason| SolidError::invalid_input(OP, reason);
    if !tau.is_finite()
        || tau <= 0.0
        || tau > 0.5
        || !a.is_finite()
        || !b.is_finite()
        || !(0.0 <= a && a < b && b <= 1.0)
    {
        return Err(bad("invalid original arc or subarc interval"));
    }
    subarc_weights_interval(Ivl::from(tau), Ivl::from(a), Ivl::from(b))
}

fn subarc_weights_interval(tau: Ivl, a: Ivl, b: Ivl) -> Result<SubarcWeights> {
    let bad = |reason| SolidError::invalid_input(OP, reason);
    if !(tau.lo > 0.0 && tau.hi <= 0.5 && a.lo >= 0.0 && a.hi < b.lo && b.hi <= 1.0) {
        return Err(bad(
            "subarc intervals are not ordered inside the original arc",
        ));
    }
    let one = Ivl::from(1.0);
    let tau2 = tau * tau;
    let cosine = (one - tau2) / (one + tau2);
    let polar =
        |x: Ivl, y: Ivl| (one - x) * (one - y) + cosine * ((one - x) * y + x * (one - y)) + x * y;
    let old = [polar(a, a), polar(a, b), polar(b, b)];
    if old.iter().any(|w| !(w.lo > 0.0 && w.hi.is_finite())) {
        return Err(bad("subarc denominator is not proved positive"));
    }
    let geometric = (old[0] * old[2]).sqrt();
    if !(geometric.lo > 0.0 && geometric.hi.is_finite()) {
        return Err(bad("subarc weight scale is not proved positive"));
    }
    let middle = old[1] / geometric;
    if !(middle.lo > 0.0 && middle.hi.is_finite()) {
        return Err(bad("subarc middle weight is not proved positive"));
    }
    // A proper subarc of the original arc has middle weight in (0, 1].
    // Intersecting with that proved geometric range avoids sending a slightly
    // widened upper endpoint past 1 into the square root below.
    if middle.lo > 1.0 {
        return Err(bad(
            "subarc middle weight is inconsistent with a circular arc",
        ));
    }
    let circular_middle = Ivl::new(middle.lo, middle.hi.min(1.0));
    let quarter_tangent = ((one - circular_middle) / (one + circular_middle)).sqrt();
    if !quarter_tangent.hi.is_finite() {
        return Err(bad("subarc turn is not bounded"));
    }
    Ok(SubarcWeights {
        old,
        scale: [old[0], geometric, old[2]],
        middle,
        quarter_tangent,
    })
}

/// Choose equal subdivisions per original span from an allocated lift-only
/// budget. The caller must reserve profile, construction and runtime error
/// separately. On one span theta'(t) is bounded by `4*tan(width/4)/width`,
/// so a parameter cell of width `width/n` turns at most `4*tan(width/4)/n`.
/// Monotonicity of the enclosed lift error then bounds every refined cell.
/// Refuses when the requested budget cannot fit within the station budget.
fn pieces_for_lift(definition: &super::ProceduralSweep, budget: f64) -> Result<usize> {
    use crate::sweep::screw::MAX_STATIONS;
    let bad = |reason| SolidError::invalid_input(OP, reason);
    if !(budget.is_finite() && budget > 0.0) {
        return Err(bad("a positive finite lift budget is required"));
    }
    let motion = definition
        .screw()
        .ok_or_else(|| bad("a screw motion is required"))?;
    if motion.law().is_some() {
        return Err(bad("radial-law refinement is not supported"));
    }
    let originals = definition
        .stations
        .len()
        .checked_sub(1)
        .filter(|&n| n > 0)
        .ok_or_else(|| bad("missing original stations"))?;
    let max_pieces = (MAX_STATIONS as usize)
        .checked_div(originals)
        .ok_or_else(|| bad("missing original stations"))?;
    for pieces in 1..=max_pieces {
        let error = lift_error_for_pieces(definition, motion, pieces)?;
        if error.is_finite() && error <= budget {
            return Ok(pieces);
        }
    }
    Err(bad("the lift cannot meet the station budget"))
}

/// The same interval certificate used for selection, before net construction.
fn lift_error_for_pieces(
    definition: &super::ProceduralSweep,
    motion: &super::ScrewMotion,
    pieces: usize,
) -> Result<f64> {
    use crate::blend::fillet_section::interval::tan_certified;
    let bad = |reason| SolidError::invalid_input(OP, reason);
    let lambda = Ivl::from(motion.pitch().abs()) / Ivl::from(core::f64::consts::TAU);
    let mut error = 0.0_f64;
    for pair in definition.stations.windows(2) {
        let [a, b] = *pair else {
            return Err(bad("invalid original span"));
        };
        let width = Ivl::from(b) - Ivl::from(a);
        if width.lo <= 0.0 {
            return Err(bad("original stations are not ascending"));
        }
        let original_tan = tan_certified(width * Ivl::from(0.25));
        let sub_tan = tan_certified(original_tan / Ivl::from(pieces as f64));
        let bound = (lambda * Ivl::from(super::lift_error_bound(sub_tan.hi))).hi;
        if bound.is_nan() {
            return Err(bad("invalid lift error enclosure"));
        }
        error = error.max(bound);
    }
    Ok(error)
}

/// Construct a candidate net for retained screw spans, in definition coordinates. This is deliberately **not certified**: the float construction
/// must be compared against interval ideal coefficients before the net is used.
/// The exact definition keeps its original stations; only this net gains knots.
///
/// # Errors
/// Refuses a path, a radial law, invalid subdivision, or a net that exceeds the
/// screw builder's budgets. The supplied fitted profile is not itself certified here.
pub(super) fn candidate_refined_net(
    definition: &super::ProceduralSweep,
    profile: &crate::nurbs::b_spline_curve::BSplineCurve,
    pieces: usize,
) -> Result<crate::nurbs::b_spline_surface::BSplineSurface> {
    use crate::nurbs::b_spline_surface::{BSplineSurface, CanonicalSurfaceData};
    use crate::sweep::screw::{screw_net, MAX_NET_POINTS, MAX_STATIONS};

    let bad = |reason| SolidError::invalid_input(OP, reason);
    let motion = definition
        .screw()
        .ok_or_else(|| bad("a screw motion is required"))?;
    if motion.law().is_some() {
        return Err(bad("a screw without a radial law is required"));
    }
    let stations = &definition.stations;
    if stations.len() < 2 {
        return Err(bad("no original screw spans"));
    }
    if pieces == 0 || pieces as f64 > MAX_STATIONS {
        return Err(bad("invalid refined span count"));
    }
    let total_spans = stations
        .len()
        .checked_sub(1)
        .and_then(|n| n.checked_mul(pieces))
        .filter(|&n| n as f64 <= MAX_STATIONS)
        .ok_or_else(|| bad("refined station budget exceeded"))?;
    let columns = total_spans
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| bad("refined net size overflow"))?;
    let profile_rows = profile.canonical()?.control_points.len();
    if profile_rows
        .checked_mul(columns)
        .is_none_or(|n| n > MAX_NET_POINTS)
    {
        return Err(bad("refined net exceeds the control-point budget"));
    }
    let mut cuts = Vec::new();
    let mut spans = Vec::new();
    for parent in stations.windows(2) {
        let [first, last] = *parent else {
            return Err(bad("invalid original span"));
        };
        let width = last - first;
        if !(width.is_finite() && width > 0.0) {
            return Err(bad("invalid original station span"));
        }
        let mut previous = first;
        if cuts.is_empty() {
            cuts.push(first);
        }
        for i in 1..=pieces {
            let next = if i == pieces {
                last
            } else {
                first + width * (i as f64 / pieces as f64)
            };
            if !(previous < next) {
                return Err(bad("rounded refined cuts are not distinct"));
            }
            let x = (previous - first) / width;
            let y = (next - first) / width;
            let weights = subarc_weights((width * 0.25).tan(), x, y)?;
            let t0 = definition.turn_at(previous).0;
            let t1 = definition.turn_at(next).0;
            let net = screw_net(profile, motion, &[t0, t1])?.canonical()?;
            spans.push((net, weights.scale.map(Ivl::mid)));
            cuts.push(next);
            previous = next;
        }
    }
    let first_span = spans.first().ok_or_else(|| bad("empty refined net"))?;
    let template = &first_span.0;
    let row_count = template
        .control_points
        .len()
        .checked_div(3)
        .ok_or_else(|| bad("invalid subarc net"))?;
    if row_count
        .checked_mul(columns)
        .is_none_or(|n| n > MAX_NET_POINTS)
    {
        return Err(bad("refined net exceeds the control-point budget"));
    }
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for row in 0..row_count {
        for (index, (span, scales)) in spans.iter().enumerate() {
            if span.v_degree != 2 || span.control_points.len() != row_count * 3 {
                return Err(bad("inconsistent refined subarc nets"));
            }
            for column in usize::from(index > 0)..3 {
                let at = row
                    .checked_mul(3)
                    .and_then(|n| n.checked_add(column))
                    .ok_or_else(|| bad("refined control-point index overflow"))?;
                let point = span
                    .control_points
                    .get(at)
                    .copied()
                    .ok_or_else(|| bad("missing subarc point"))?;
                let source_weight = span.weights.get(at).copied().unwrap_or(1.0);
                let weight = source_weight
                    * scales
                        .get(column)
                        .copied()
                        .ok_or_else(|| bad("missing subarc scale"))?;
                if !(weight.is_finite() && weight > 0.0) {
                    return Err(bad("invalid refined net weight"));
                }
                points.push(point);
                weights.push(weight);
            }
        }
    }
    let multiplicities: Vec<_> = (0..cuts.len())
        .map(|i| if i == 0 || i + 1 == cuts.len() { 3 } else { 2 })
        .collect();
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: template.u_degree,
        v_degree: 2,
        control_points: &points,
        form: template.form,
        u_multiplicities: &template.u_multiplicities,
        v_multiplicities: &multiplicities,
        u_knots: &template.u_knots,
        v_knots: &cuts,
        knot_type: template.knot_type,
        weights: Some(&weights),
        uv_domain: None,
    })
}

/// Enclose atan on [-1/2, 1/2] using the existing nonnegative enclosure and
/// odd symmetry. `atan_ivl` itself deliberately rejects negative arguments.
fn signed_atan(x: Ivl) -> Ivl {
    let endpoint = |t: f64| {
        if t < 0.0 {
            let positive = super::atan_ivl(Ivl::from(-t));
            Ivl::new(-positive.hi, -positive.lo)
        } else {
            super::atan_ivl(Ivl::from(t))
        }
    };
    Ivl::new(endpoint(x.lo).lo, endpoint(x.hi).hi)
}

/// Select an original definition span for a net cell. A cell may not cross a
/// definition station; its endpoint may coincide with that station.
fn original_span(stations: &[f64], left: f64, right: f64) -> Result<(f64, f64)> {
    let bad = || SolidError::invalid_input(OP, "a refined cell crosses an original station");
    let index = stations
        .partition_point(|&t| t <= left)
        .checked_sub(1)
        .ok_or_else(bad)?;
    let a = *stations.get(index).ok_or_else(bad)?;
    let b = *stations
        .get(index.checked_add(1).ok_or_else(bad)?)
        .ok_or_else(bad)?;
    if !(a <= left && left < right && right <= b) {
        return Err(bad());
    }
    Ok((a, b))
}

type IVec3 = [Ivl; 3];
fn add3([a, b, c]: IVec3, [x, y, z]: IVec3) -> IVec3 {
    [a + x, b + y, c + z]
}
fn sub3([a, b, c]: IVec3, [x, y, z]: IVec3) -> IVec3 {
    [a - x, b - y, c - z]
}
fn mul3([a, b, c]: IVec3, k: Ivl) -> IVec3 {
    [a * k, b * k, c * k]
}
fn dot3([a, b, c]: IVec3, [x, y, z]: IVec3) -> Ivl {
    a * x + b * y + c * z
}
fn cross3([a, b, c]: IVec3, [x, y, z]: IVec3) -> IVec3 {
    [b * z - c * y, c * x - a * z, a * y - b * x]
}
fn point3(p: crate::math::Vec3) -> IVec3 {
    [Ivl::from(p.x), Ivl::from(p.y), Ivl::from(p.z)]
}
fn normalized_axis(axis: crate::math::Vec3) -> Result<(IVec3, Ivl, Ivl)> {
    let k = point3(axis);
    let q = dot3(k, k);
    let length = q.sqrt();
    if !(length.lo > 0.0 && length.hi.is_finite()) {
        return Err(SolidError::invalid_input(
            OP,
            "screw axis is not proved normalizable",
        ));
    }
    Ok((mul3(k, Ivl::from(1.0) / length), q, length))
}

/// Bound only the coefficient construction discrepancy for a saved screw net
/// with an identity parameter map and positive-weight degree-one profile. This is *not* a complete surface certificate: the
/// analytic lift approximation and evaluation-rounding floor must be added and
/// justified separately. The actual net's own knots determine every ideal cell.
pub(super) fn candidate_coefficient_error(
    definition: &super::ProceduralSweep,
    actual: &crate::nurbs::b_spline_surface::BSplineSurface,
) -> Result<f64> {
    use crate::blend::fillet_section::interval::{sin_cos_certified, tan_certified};
    use crate::blend::fillet_section::recertify::{
        interval_net_distance_cached, IntervalWeightCache,
    };
    use crate::sweep::screw::MAX_STATIONS;

    let bad = |reason| SolidError::invalid_input(OP, reason);
    let motion = definition
        .screw()
        .ok_or_else(|| bad("a screw motion is required"))?;
    let (Some(first), Some(last)) = (definition.stations.first(), definition.stations.last())
    else {
        return Err(bad("no original screw spans"));
    };
    let ([a0, a1], [b0, b1], swapped) = definition.definition_map();
    if motion.law().is_some() || [a0, a1] != [0.0, 0.0] || [b0, b1] != [1.0, 1.0] {
        return Err(bad(
            "coefficient certificate requires an unscaled axis permutation",
        ));
    }
    let profile = definition.profile.canonical()?;
    if profile.dim != 3
        || profile.degree != 1
        || profile.control_points.len() < 2
        || profile.knots.len() != profile.control_points.len()
        || profile.knot_multiplicities.len() != profile.knots.len()
        || profile
            .knot_multiplicities
            .iter()
            .enumerate()
            .any(|(i, &m)| {
                m != if i == 0 || i + 1 == profile.knots.len() {
                    2
                } else {
                    1
                }
            })
        || profile.weights.iter().any(|w| !w.is_finite() || *w <= 0.0)
    {
        return Err(bad(
            "prototype requires a positive-weight degree-one profile",
        ));
    }
    let canonical = actual.canonical()?;
    let (profile_degree, profile_knots, profile_mult, turn_degree, cuts, turn_mult) = if swapped {
        (
            canonical.v_degree,
            &canonical.v_knots,
            &canonical.v_multiplicities,
            canonical.u_degree,
            &canonical.u_knots,
            &canonical.u_multiplicities,
        )
    } else {
        (
            canonical.u_degree,
            &canonical.u_knots,
            &canonical.u_multiplicities,
            canonical.v_degree,
            &canonical.v_knots,
            &canonical.v_multiplicities,
        )
    };
    if profile_degree != 1
        || *profile_knots != profile.knots
        || *profile_mult != profile.knot_multiplicities
        || turn_degree != 2
        || cuts.len() < 2
        || (cuts.len() - 1) as f64 > MAX_STATIONS
        || cuts.first() != Some(first)
        || cuts.last() != Some(last)
        || turn_mult
            .iter()
            .enumerate()
            .any(|(i, &m)| m != if i == 0 || i + 1 == cuts.len() { 3 } else { 2 })
    {
        return Err(bad(
            "actual net knots are not a bounded refinement of the original span",
        ));
    }
    let (nu, nv, control) = actual.control_points()?;
    let (rows, columns) = if swapped { (nv, nu) } else { (nu, nv) };
    if rows != profile.control_points.len() || columns != 2 * (cuts.len() - 1) + 1 {
        return Err(bad("actual net has unexpected control dimensions"));
    }
    // Six point comparisons per cell, plus 36 weight comparisons on each
    // cache miss. Charge the same fixed work limit as the unfactored version.
    let mut work = 0_usize;
    let one = Ivl::from(1.0);
    let lambda = Ivl::from(motion.pitch()) / Ivl::from(core::f64::consts::TAU);
    let (axis, _, _) = normalized_axis(motion.axis())?;
    let origin = point3(motion.origin());
    let trig = sin_cos_certified;
    let mut worst = 0.0_f64;
    for (span, pair) in cuts.windows(2).enumerate() {
        let mut weight_cache = IntervalWeightCache::default();
        let [left, right] = *pair else {
            return Err(bad("invalid refined knot span"));
        };
        if !(left < right) {
            return Err(bad("refined knots must ascend"));
        }
        let (first, last) = original_span(&definition.stations, left, right)?;
        let width = Ivl::from(last) - Ivl::from(first);
        if width.lo <= 0.0 {
            return Err(bad("invalid original span"));
        }
        let tau = tan_certified(width * Ivl::from(0.25));
        let midpoint = (Ivl::from(first) + Ivl::from(last)) * Ivl::from(0.5);
        let normalized = |t: f64| {
            let x = (Ivl::from(t) - Ivl::from(first)) / width;
            Ivl::new(x.lo.max(0.0), x.hi.min(1.0))
        };
        let angle =
            |x: Ivl| midpoint + Ivl::from(2.0) * signed_atan(tau * (Ivl::from(2.0) * x - one));
        let (a, b) = (normalized(left), normalized(right));
        let weights = subarc_weights_interval(tau, a, b)?;
        let (t0, t1) = (angle(a), angle(b));
        let tm = (t0 + t1) * Ivl::from(0.5);
        let trig = [trig(t0), trig(tm), trig(t1)];
        // Degree-one profile with simple interior knots has exactly two active
        // controls on each u span. Compare each (1,2) Bezier cell separately.
        for u_span in 0..rows.saturating_sub(1) {
            let mut input = Vec::new();
            let mut ideal = Vec::new();
            for row in u_span..=u_span + 1 {
                let p = profile
                    .control_points
                    .get(row)
                    .copied()
                    .ok_or_else(|| bad("missing profile point"))?;
                let mut input_row = Vec::new();
                let mut ideal_row = Vec::new();
                for col in 0..3 {
                    let angular = 2 * span + col;
                    let j = if swapped {
                        angular.checked_mul(nv).and_then(|n| n.checked_add(row))
                    } else {
                        row.checked_mul(nv).and_then(|n| n.checked_add(angular))
                    }
                    .ok_or_else(|| bad("control index overflow"))?;
                    let h = control
                        .get(j)
                        .ok_or_else(|| bad("missing actual control"))?;
                    input_row.push((
                        [Ivl::from(h.x), Ivl::from(h.y), Ivl::from(h.z)],
                        Ivl::from(h.w),
                    ));
                    let (sin, cos) = trig
                        .get(col)
                        .copied()
                        .ok_or_else(|| bad("missing trig enclosure"))?;
                    let divisor = if col == 1 { weights.middle } else { one };
                    let profile_weight = profile.weights.get(row).copied().unwrap_or(1.0);
                    let w = *weights
                        .old
                        .get(col)
                        .ok_or_else(|| bad("missing ideal weight"))?
                        * Ivl::from(profile_weight);
                    let theta = [t0, tm, t1]
                        .get(col)
                        .copied()
                        .ok_or_else(|| bad("missing ideal angle"))?;
                    let arm = sub3(point3(p), origin);
                    let axial = mul3(axis, dot3(axis, arm));
                    let radial = sub3(arm, axial);
                    let rotated = add3(mul3(radial, cos), mul3(cross3(axis, radial), sin));
                    let point = add3(
                        add3(add3(origin, axial), mul3(rotated, one / divisor)),
                        mul3(axis, lambda * theta),
                    );
                    ideal_row.push((mul3(point, w), w));
                }
                input.push(input_row);
                ideal.push(ideal_row);
            }
            let (bound, reused) = interval_net_distance_cached(&input, &ideal, &mut weight_cache)?;
            work = work
                .checked_add(if reused { 6 } else { 42 })
                .filter(|&n| n <= 8_000_000)
                .ok_or_else(|| bad("interval comparison budget exceeded"))?;
            worst = worst.max(bound);
        }
    }
    Ok(worst)
}

/// Enclose evaluation rounding of a degree-(1,2) tensor net with simple
/// profile knots and double internal turn knots. The actual local knot widths feed the shared
/// degree-one quotient bound. For a quadratic Bezier cell every quotient in
/// the two Cox levels has that same denominator; the endpoint basis values
/// are products of two quotients, and the middle is their two-product sum.
/// The returned bound is physical distance, including rational division.
fn net_evaluation_rounding(net: &crate::nurbs::b_spline_surface::BSplineSurface) -> Result<f64> {
    use crate::blend::fillet_section::interval::linear_basis_quotient_rounding;
    use crate::nurbs::math::{gamma, UNIT};
    let bad = |reason| SolidError::invalid_input(OP, reason);
    let c = net.canonical()?;
    let (u_degree, v_degree) = (c.u_degree, c.v_degree);
    if !matches!((u_degree, v_degree), (1, 2) | (2, 1)) {
        return Err(bad(
            "runtime bound requires degree-(1,2) or degree-(2,1) cells",
        ));
    }
    let count = |knots: &[f64], multiplicities: &[usize], degree: usize| -> Result<usize> {
        if knots.len() < 2
            || knots.len() != multiplicities.len()
            || multiplicities.iter().enumerate().any(|(i, &m)| {
                m != if i == 0 || i + 1 == knots.len() {
                    degree + 1
                } else {
                    degree
                }
            })
        {
            return Err(bad("runtime bound requires Bezier cell multiplicities"));
        }
        knots
            .len()
            .checked_sub(1)
            .and_then(|n| n.checked_mul(degree))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| bad("runtime dimensions overflow"))
    };
    let (nu, nv, control) = net.control_points()?;
    if nu != count(&c.u_knots, &c.u_multiplicities, u_degree)?
        || nv != count(&c.v_knots, &c.v_multiplicities, v_degree)?
    {
        return Err(bad("runtime bound has unexpected control dimensions"));
    }
    let mut wmin = f64::INFINITY;
    let mut wmax = 0.0_f64;
    let mut hmax = 0.0_f64;
    for h in &control {
        if !(h.w.is_finite() && h.w > 0.0 && h.x.is_finite() && h.y.is_finite() && h.z.is_finite())
        {
            return Err(bad(
                "runtime bound requires finite positive homogeneous weights",
            ));
        }
        wmin = wmin.min(h.w);
        wmax = wmax.max(h.w);
        hmax = hmax.max(h.x.abs()).max(h.y.abs()).max(h.z.abs());
    }
    let tiny = Ivl::from(f64::from_bits(1));
    let u = Ivl::from(UNIT);
    let one = Ivl::from(1.0);
    // v is evaluated first: (1,2) uses 13 operations; (2,1) uses 14.
    let g = gamma(if (u_degree, v_degree) == (1, 2) {
        13
    } else {
        14
    });
    let gamma_sum = Ivl::checked(g, g);
    let max_quotient = |knots: &[f64]| -> Result<Ivl> {
        let mut result = Ivl::from(0.0);
        for pair in knots.windows(2) {
            let [a, b] = *pair else {
                return Err(bad("invalid knot span"));
            };
            let current = linear_basis_quotient_rounding(Ivl::from(b) - Ivl::from(a));
            if !current.hi.is_finite() || current.hi >= 0.5 {
                return Err(bad("basis quotient rounding is not proved below one half"));
            }
            result = result.hull(current);
        }
        Ok(result)
    };
    let quadratic_error = |en: Ivl| -> Result<Ivl> {
        let product = Ivl::from(2.0) * en + en * en + u * (one + en) * (one + en) + tiny;
        let middle =
            Ivl::from(2.0) * product + u * (Ivl::from(2.0) + Ivl::from(2.0) * product) + tiny;
        if !middle.hi.is_finite() || middle.hi >= 0.5 {
            return Err(bad("quadratic basis rounding is not proved below one half"));
        }
        Ok(middle)
    };
    let eu = max_quotient(&c.u_knots)?;
    let ev = max_quotient(&c.v_knots)?;
    let eu = if u_degree == 2 {
        quadratic_error(eu)?
    } else {
        eu
    };
    let ev = if v_degree == 2 {
        quadratic_error(ev)?
    } else {
        ev
    };
    let tensor = eu + ev + eu * ev + u * (one + eu) * (one + ev) + tiny;
    let factor = Ivl::from(6.0) * tensor + gamma_sum * (one + Ivl::from(6.0) * tensor);
    let eh = factor * Ivl::from(hmax) + Ivl::from(32.0) * tiny;
    let ew = factor * Ivl::from(wmax) + Ivl::from(32.0) * tiny;
    let den = Ivl::from(wmin) - ew;
    if den.lo < f64::MIN_POSITIVE {
        return Err(bad("runtime weight denominator not normal and positive"));
    }
    // Surface::eval forms reciprocal_w then multiplies each coordinate, so
    // division-to-Euclidean has two roundings. Prove the reciprocal normal
    // even at the largest possible denominator before applying gamma_2.
    let reciprocal_min = one / (Ivl::from(wmax) + ew);
    if reciprocal_min.lo < f64::MIN_POSITIVE {
        return Err(bad("runtime weight reciprocal is not proved normal"));
    }
    let magnitude = Ivl::from(hmax) / Ivl::from(wmin);
    let g2 = gamma(2);
    let error =
        (eh + magnitude * ew) / den + Ivl::checked(g2, g2) * (Ivl::from(hmax) + eh) / den + tiny;
    let distance = (Ivl::from(3.0) * error * error).sqrt();
    if !distance.hi.is_finite() {
        return Err(bad("runtime evaluation error is not finite"));
    }
    Ok(distance.hi)
}

/// Enclose the exact-real net-to-definition error for the restricted prototype.
/// This is not a runtime evaluation certificate: it omits rounding in evaluating
/// the final net and definition, so it must not authorize keeping a surface.
///
/// On each subarc the interval coefficient error encloses |B-H|. The ideal H
/// differs from S only in lift; endpoint-preserving Möbius reparameterization is
/// onto, so its supremum is bounded by the existing symmetric-arc lift bound.
/// That bound grows with tau: with K(T)=2(1+T²)atan(T)/T, K'(T) is positive
/// because atan(T)<T for T>0; E(T,x) is nonnegative and grows with T for fixed
/// x, and its domain [0,T] grows. Thus each interval's upper tau is safe.
fn exact_real_error_bound(
    definition: &super::ProceduralSweep,
    actual: &crate::nurbs::b_spline_surface::BSplineSurface,
) -> Result<f64> {
    use crate::blend::fillet_section::interval::tan_certified;
    let bad = |reason| SolidError::invalid_input(OP, reason);
    let coefficient = candidate_coefficient_error(definition, actual)?;
    let motion = definition
        .screw()
        .ok_or_else(|| bad("a screw motion is required"))?;
    let canonical = actual.canonical()?;
    let knots = if definition.definition_map().2 {
        canonical.u_knots
    } else {
        canonical.v_knots
    };
    let mut lift = 0.0_f64;
    for pair in knots.windows(2) {
        let [a, b] = *pair else {
            return Err(bad("invalid refined knot span"));
        };
        let (first, last) = original_span(&definition.stations, a, b)?;
        let width = Ivl::from(last) - Ivl::from(first);
        if width.lo <= 0.0 {
            return Err(bad("invalid original span"));
        }
        let tau = tan_certified(width * Ivl::from(0.25));
        let normalize = |t: f64| {
            let x = (Ivl::from(t) - Ivl::from(first)) / width;
            Ivl::new(x.lo.max(0.0), x.hi.min(1.0))
        };
        let w = subarc_weights_interval(tau, normalize(a), normalize(b))?;
        lift = lift.max(super::lift_error_bound(w.quarter_tangent.hi));
    }
    let profile_error = definition
        .offset
        .as_ref()
        .map(|offset| offset.certify(&definition.profile, motion))
        .transpose()?
        .unwrap_or(0.0);
    let lambda = Ivl::from(motion.pitch().abs()) / Ivl::from(core::f64::consts::TAU);
    // Compare raw stored-axis Rodrigues with the exactly normalized axis used
    // by the ideal cells. A positive-weight profile lies in its control hull;
    // profile_error encloses the exact offset section beyond that hull.
    let (_, q, length) = normalized_axis(motion.axis())?;
    let unit = Ivl::from(1.0);
    let mut reach = 0.0_f64;
    for p in &definition.profile.canonical()?.control_points {
        let r = sub3(point3(*p), point3(motion.origin()));
        reach = reach.max(dot3(r, r).sqrt().hi);
    }
    let angles = definition
        .stations
        .iter()
        .fold(0.0_f64, |m, x| m.max(x.abs()));
    let axis_error = (Ivl::from((length - unit).mag())
        + Ivl::from(2.0) * Ivl::from((q - unit).mag()))
        * (Ivl::from(reach) + Ivl::from(profile_error))
        + lambda * Ivl::from((length - unit).mag()) * Ivl::from(angles);
    let bound =
        (Ivl::from(coefficient) + lambda * Ivl::from(lift) + Ivl::from(profile_error) + axis_error)
            .hi;
    if !bound.is_finite() {
        return Err(bad("the exact-real bound is not finite"));
    }
    Ok(bound)
}

/// Review-only mapped-net recertification: normalize a copy of the saved net
/// into definition coordinates, certify the affine/swap conversion against the
/// original saved net, and add the independently rebuilt ideal-cell bound.
/// Only a whole support is accepted; a cut crossing an original station refuses.
/// The saved record's claimed bound is never an input.
#[cfg(test)]
fn mapped_exact_real_error_bound(
    definition: &super::ProceduralSweep,
    actual: &crate::nurbs::b_spline_surface::BSplineSurface,
) -> Result<f64> {
    use crate::blend::fillet_section::recertify::mapped_net_distance;
    use crate::geom::extent2d::Extent2d;
    use crate::nurbs::b_spline_surface::SurfParamType;
    let bad = |reason| SolidError::invalid_input(OP, reason);
    let ([a0, a1], [b0, b1], swapped) = definition.definition_map();
    if ![a0, a1, b0, b1].into_iter().all(f64::is_finite) || b0 == 0.0 || b1 == 0.0 {
        return Err(bad("mapped prototype requires a finite nonsingular map"));
    }
    let (Some(&first), Some(&last)) = (definition.stations.first(), definition.stations.last())
    else {
        return Err(bad("missing original stations"));
    };
    let profile = definition.profile.natural_interval();
    if !(profile.min() < profile.max() && first < last) {
        return Err(bad("invalid definition support"));
    }
    let mut normalized = actual.copy();
    if swapped {
        normalized.swap_uv()?;
    }
    if b0 < 0.0 {
        normalized.reverse(SurfParamType::U)?;
    }
    if b1 < 0.0 {
        normalized.reverse(SurfParamType::V)?;
    }
    normalized.reparameterize(&Extent2d::from_bounds(
        profile.min(),
        first,
        profile.max(),
        last,
    ))?;
    let mapping_error = mapped_net_distance(actual, &normalized, [a0, a1], [b0, b1], swapped)?;
    let mut source = definition.clone();
    source.invalidate_runtime_allowance();
    source.map = super::SweepMap::default();
    let ideal_error = exact_real_error_bound(&source, &normalized)?;
    let bound = (Ivl::from(mapping_error) + Ivl::from(ideal_error)).hi;
    if !bound.is_finite() {
        return Err(bad("mapped prototype bound is not finite"));
    }
    Ok(bound)
}

/// Build a retained-map reference in definition coordinates with a bounded
/// lift-only budget. The profile's own error is certified separately.
pub(super) fn build_refined(
    definition: &super::ProceduralSweep,
    fitted: &crate::nurbs::b_spline_curve::BSplineCurve,
    lift_budget: f64,
) -> Result<(crate::nurbs::b_spline_surface::BSplineSurface, usize)> {
    let pieces = pieces_for_lift(definition, lift_budget)?;
    Ok((candidate_refined_net(definition, fitted, pieces)?, pieces))
}

/// Rebuild an independent retained-parameter reference and certify the actual
/// net against it. The full S evaluation allowance is supplied by the checked
/// definition evaluator; serialized bounds never enter this calculation.
///
/// # Errors
/// Refuses unsupported geometry, malformed or oversized nets, unproved mapped
/// cells, or an unproved runtime allowance.
pub(super) fn certify_refined(
    definition: &super::ProceduralSweep,
    actual: &crate::nurbs::b_spline_surface::BSplineSurface,
    reference_pieces: usize,
) -> Result<f64> {
    use crate::blend::fillet_section::recertify::mapped_clamped_net_distance;
    // In definition coordinates, the actual stored coefficients can be compared
    // directly to the interval ideal. This avoids materializing a second net and
    // leaves the mapped comparison for genuinely transformed parameter domains.
    let ([a0, a1], [b0, b1], _) = definition.definition_map();
    let (reference_error, discrepancy) = if [a0, a1] == [0.0, 0.0] && [b0, b1] == [1.0, 1.0] {
        (exact_real_error_bound(definition, actual)?, 0.0)
    } else {
        let mut source = definition.clone();
        source.invalidate_runtime_allowance();
        source.map = super::SweepMap::default();
        let reference = candidate_refined_net(&source, &source.profile, reference_pieces)?;
        let reference_error = exact_real_error_bound(&source, &reference)?;
        let ([a0, a1], [b0, b1], swapped) = definition.definition_map();
        let discrepancy =
            mapped_clamped_net_distance(actual, &reference, [a0, a1], [b0, b1], swapped)?;
        (reference_error, discrepancy)
    };
    let net_rounding = net_evaluation_rounding(actual)?;
    let source_rounding = definition.runtime_evaluation_allowance()?;
    let total = (Ivl::from(reference_error)
        + Ivl::from(discrepancy)
        + Ivl::from(net_rounding)
        + Ivl::from(source_rounding))
    .hi;
    if !total.is_finite() || total <= 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "refined net bound is not finite and positive",
        ));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::procedural_sweep::lift_error_bound;

    #[test]
    fn chooses_the_smallest_certified_subdivision_for_a_long_thread() {
        use crate::math::Vec3;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations: Vec<_> = (0..=560)
            .map(|i| f64::from(i) * std::f64::consts::PI / 28.0)
            .collect();
        let definition = ProceduralSweep::create(&profile, motion.clone(), &stations, 0.1).unwrap();
        assert!(lift_error_for_pieces(&definition, &motion, 2).unwrap() > 5e-7);
        assert!(lift_error_for_pieces(&definition, &motion, 3).unwrap() <= 5e-7);
        let pieces = pieces_for_lift(&definition, 5e-7).unwrap();
        assert_eq!(pieces, 3);
        let net = candidate_refined_net(&definition, &profile, pieces).unwrap();
        let (_, m, _, _) = net.gw_nurb().unwrap().array_sizes();
        assert_eq!(m + 1, 2 * (stations.len() - 1) * pieces + 1);
        assert!(m < 4096);
        let certified = certify_refined(&definition, &net, pieces).unwrap();
        assert!(
            certified <= 2e-6,
            "the complete retained-map certificate is {certified}"
        );
    }

    #[test]
    fn restricted_weights_keep_the_old_angle_and_reduce_the_lift_bound() {
        let tau = (0.3_f64 / 4.0).tan();
        let original = lift_error_bound(tau);
        for (a, b) in [(0.0, 0.5), (0.5, 1.0)] {
            let w = subarc_weights(tau, a, b).unwrap();
            assert!(lift_error_bound(w.quarter_tangent.hi) < original);
            // Samples witness the identity; the enclosure is the polar-form
            // derivation above, not these evaluations.
            for x in [0.13, 0.37, 0.71] {
                let old_t = a + (b - a) * x;
                let old_angle = 2.0 * (tau * (2.0 * old_t - 1.0)).atan();
                let ratio = (w.scale[2].mid() / w.scale[0].mid()).sqrt();
                let local = ratio * x / (1.0 - x + ratio * x);
                let start = 2.0 * (tau * (2.0 * a - 1.0)).atan();
                let end = 2.0 * (tau * (2.0 * b - 1.0)).atan();
                let half = (end - start) * 0.5;
                let new_angle =
                    (start + end) * 0.5 + 2.0 * ((half * 0.5).tan() * (2.0 * local - 1.0)).atan();
                assert!((old_angle - new_angle).abs() < 1e-14);
            }
            assert!(w.old[1].lo <= (w.scale[1] * w.middle).hi);
            assert!(w.old[1].hi >= (w.scale[1] * w.middle).lo);
        }
    }

    #[test]
    fn candidate_refines_lift_without_changing_the_old_turn_map() {
        use crate::math::{Point2, Vec3};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use crate::sweep::screw::screw_net;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.3];
        let definition =
            ProceduralSweep::create(&profile, motion.clone(), &stations, 0.15).unwrap();
        let old = screw_net(&profile, &motion, &stations).unwrap();
        let refined = candidate_refined_net(&definition, &profile, 4).unwrap();
        let coefficient_error = candidate_coefficient_error(&definition, &refined).unwrap();
        assert!(coefficient_error < 1e-9, "{coefficient_error}");
        let exact_bound = exact_real_error_bound(&definition, &refined).unwrap();
        println!(
            "coefficient={coefficient_error:e} exact_real={exact_bound:e} original={:e}",
            definition.bound()
        );
        assert!(exact_bound < definition.bound() * 0.5, "{exact_bound}");
        let net_rounding = net_evaluation_rounding(&refined).unwrap();
        assert!(net_rounding < 1e-9, "{net_rounding}");
        // Reconstruct from the actual stored knot cuts and notice a changed
        // control point; comparison against the net itself cannot do this.
        let mut altered = refined.canonical().unwrap();
        altered.control_points[1].z += 0.001;
        let altered = crate::nurbs::b_spline_surface::BSplineSurface::create_canonical(
            &crate::nurbs::b_spline_surface::CanonicalSurfaceData {
                u_degree: altered.u_degree,
                v_degree: altered.v_degree,
                control_points: &altered.control_points,
                form: altered.form,
                u_multiplicities: &altered.u_multiplicities,
                v_multiplicities: &altered.v_multiplicities,
                u_knots: &altered.u_knots,
                v_knots: &altered.v_knots,
                knot_type: altered.knot_type,
                weights: Some(&altered.weights),
                uv_domain: None,
            },
        )
        .unwrap();
        assert!(candidate_coefficient_error(&definition, &altered).unwrap() > 0.0009);
        // The same stored-coefficient check must work when face U and V are
        // permuted, and must still notice a changed saved control point.
        let mut swapped_definition = definition.clone();
        swapped_definition.map.swapped = true;
        let mut swapped_refined = refined.copy();
        swapped_refined.swap_uv().unwrap();
        assert!(candidate_coefficient_error(&swapped_definition, &swapped_refined).unwrap() < 1e-9);
        let mut swapped_altered = altered.copy();
        swapped_altered.swap_uv().unwrap();
        assert!(
            candidate_coefficient_error(&swapped_definition, &swapped_altered).unwrap() > 0.0009
        );
        let mut before = 0.0_f64;
        let mut after = 0.0_f64;
        for t in [0.019, 0.071, 0.113, 0.181, 0.277] {
            let uv = Point2::new(0.37, t);
            let exact = definition.evaluate_definition_point(uv).unwrap();
            let coarse = old.evaluate_point(uv).unwrap();
            let candidate = refined.evaluate_point(uv).unwrap();
            assert!((candidate.x - exact.x).abs() < 1e-12);
            assert!((candidate.y - exact.y).abs() < 1e-12);
            before = before.max((coarse - exact).length());
            after = after.max((candidate - exact).length());
        }
        assert!(after < before, "candidate {after}, original {before}");
        assert_eq!(definition.stations(), &stations);
        assert_eq!(refined.canonical().unwrap().v_knots.len(), 5);
        assert!(candidate_refined_net(&definition, &profile, 0).is_err());
    }

    #[test]
    fn multi_span_offset_profile_keeps_old_parameters_and_recertifies() {
        use crate::math::{Point2, Vec3};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::offset_profile::OffsetProfile;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};

        let origin = Vec3::new(-1.0, -64.0, 0.0);
        let source = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.5, -64.0, -0.3),
            Vec3::new(9.125, -64.0, -0.7),
        )
        .unwrap();
        let motion = ScrewMotion::new(origin, Vec3::Z, 2.5, None).unwrap();
        let (profile, offset) =
            OffsetProfile::build(&source, &motion, 0.1, [0.0, 1.0], 1e-6).unwrap();
        assert!(profile.canonical().unwrap().control_points.len() > 2);
        let stations = [0.0, 0.23, 0.53, 0.78];
        let mut definition = ProceduralSweep::create(&profile, motion, &stations, 0.15).unwrap();
        definition.offset = Some(offset);
        let net = candidate_refined_net(&definition, &profile, 4).unwrap();
        assert_eq!(definition.stations(), &stations);
        let bound = exact_real_error_bound(&definition, &net).unwrap();
        assert!(bound < 1e-5, "{bound}");
        for t in [0.017, 0.193, 0.289, 0.491, 0.621, 0.731] {
            let uv = Point2::new(0.37, t);
            let actual = net.evaluate_point(uv).unwrap();
            let expected = definition.evaluate_definition_point(uv).unwrap();
            assert!((actual - expected).length() <= bound + 1e-11);
        }
        assert!(original_span(&stations, 0.2, 0.3).is_err());
    }

    #[test]
    fn ten_turn_offset_with_tilted_axis_keeps_the_old_turn_map() {
        use crate::math::{Point2, Vec3};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::offset_profile::OffsetProfile;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use core::f64::consts::TAU;

        let origin = Vec3::new(-1.0, -64.0, 4.0);
        let axis = Vec3::new(0.0, 0.6, 0.8);
        let source = BSplineCurve::create_line_segment(
            3,
            origin + Vec3::X * 9.5 - axis * 0.3,
            origin + Vec3::X * 10.125 - axis * 0.7,
        )
        .unwrap();
        let motion = ScrewMotion::new(origin, axis, 2.5, None).unwrap();
        let (profile, offset) =
            OffsetProfile::build(&source, &motion, 0.1, [0.0, 1.0], 1e-6).unwrap();
        let stations: Vec<_> = (0..=100)
            .map(|i| (f64::from(i) * 10.0 * TAU) / 100.0)
            .collect();
        let mut definition = ProceduralSweep::create(&profile, motion, &stations, 0.32).unwrap();
        definition.offset = Some(offset);
        let pieces = pieces_for_lift(&definition, 3e-6).unwrap();
        let net = candidate_refined_net(&definition, &profile, pieces).unwrap();
        let bound = exact_real_error_bound(&definition, &net).unwrap();
        println!(
            "profile_rows={} refined_spans={} exact_real={bound:e}",
            profile.canonical().unwrap().control_points.len(),
            net.canonical().unwrap().v_knots.len() - 1
        );
        assert!(bound < 1e-5, "{bound}");
        assert_eq!(definition.stations(), stations);
        for t in [0.173, 5.123, 19.321, 41.234, 62.41] {
            let uv = Point2::new(0.37, t);
            let actual = net.evaluate_point(uv).unwrap();
            let expected = definition.evaluate_definition_point(uv).unwrap();
            assert!((actual - expected).length() <= bound + 1e-11);
        }
    }

    #[test]
    fn reversed_swapped_saved_net_is_recertified_against_original_stations() {
        use crate::math::{Point2, Vec3};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::b_spline_surface::SurfParamType;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion, SweepMap};

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.25, 0.5];
        let mut definition = ProceduralSweep::create(&profile, motion, &stations, 0.125).unwrap();
        let mut net = candidate_refined_net(&definition, &profile, 4).unwrap();
        net.reverse(SurfParamType::U).unwrap();
        net.reverse(SurfParamType::V).unwrap();
        net.swap_uv().unwrap();
        definition.map = SweepMap {
            a: [1.0, 0.5],
            b: [-1.0, -1.0],
            swapped: true,
        };
        let bound = mapped_exact_real_error_bound(&definition, &net).unwrap();
        assert!(bound < 1e-5, "{bound}");
        for uv in [Point2::new(0.08, 0.13), Point2::new(0.29, 0.37)] {
            let point = net.evaluate_point(uv).unwrap();
            let exact = definition.evaluate_definition_point(uv).unwrap();
            assert!((point - exact).length() <= bound + 1e-12);
        }
    }

    #[test]
    fn invalid_subarcs_are_refused() {
        for (tau, a, b) in [
            (0.0, 0.0, 1.0),
            (0.1, 0.5, 0.5),
            (0.1, -0.1, 0.5),
            (f64::NAN, 0.0, 1.0),
        ] {
            assert!(subarc_weights(tau, a, b).is_err());
        }
    }
}
