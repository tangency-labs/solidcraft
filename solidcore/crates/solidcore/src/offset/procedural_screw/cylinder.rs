//! A coaxial cylinder is the vertical line of the same meridian section.
//!
//! The 3D edge remains a helix on the exact screw wall. Its cylinder angular
//! pcurve is a fractional-linear change of rational circle parameters. Height is
//! interpolated as a function of that circle parameter, with the analytic
//! `sup|theta''| * delta_u^2 / 8` bound. No spatial intersection trace is used.
//! Polar metadata and native-height arithmetic have separate component certificates.
//! Actual stored pcurves have an interval Taylor incidence certificate and checked
//! runtime point caps. Radius-disjoint cases are proved empty by exact meridian
//! isolation. Full-circle charts intersect the root orbit with the requested
//! height slab by interval inversion. Clipped endpoint uncertainty is included
//! in every curve's spatial certificate, and exact stored intervals prove complete
//! coverage. Unresolved boundary contact or partial angular coverage is refused.
//! Deviations: new construction; at most 24,000 pcurve spans, no recursion.

use super::certificate;
use super::{
    append_pair, bad, roots, span, uv_curve, Extent2d, Ivl, Kernel, Point2, Point3, Result,
    ScrewOffsetSection, Surface, SurfaceIntersection, TsectCurveType, Vec3, Wall, MAX_BOXES,
    MAX_WINDINGS, TAU,
};
use crate::blend::fillet_section::recertify::interval_net_distance;
use crate::nurbs::math::UNIT;
use crate::surface::cone::checked::{polar_coefficients, HeightMap};
use crate::surface::cone::Cone;
use crate::surface::surface::SurfaceKind;

mod paired;

const MAX_SPANS: usize = 24_000;

type PolarCoefficient = ([Ivl; 3], Ivl);

/// A certificate for the mathematical polar curve and the converter's endpoint
/// substitution. Runtime degree-two evaluation/projection and the cylinder's
/// height map are separate obligations; this is not an incidence certificate.
#[derive(Clone, Copy, Debug)]
struct PolarArcCertificate {
    angular_error: f64,
    // Native tests retain this diagnostic; production enforces endpoint effects
    // in the complete checked runtime point cap.
    #[cfg(test)]
    endpoint_snap_error: f64,
}

/// Certify one quadratic polar span against the ideal rational circle encoded
/// by its stored endpoint angles and endpoint weights. The existing Bernstein
/// interval-net inequality bounds the entire span, including its endpoints.
/// Endpoint targets are supplied only at the polar curve's natural endpoints.
fn certify_polar_arc(
    actual: &[PolarCoefficient; 3],
    parameter: [f64; 2],
    angle: [f64; 2],
    endpoint_targets: [Option<Ivl>; 2],
    angle_budget: f64,
) -> Result<PolarArcCertificate> {
    if !angle_budget.is_finite() || angle_budget <= 0.0 {
        return Err(bad("polar metadata needs a positive angular budget"));
    }
    let [(_, w0), (_, _), (_, w2)] = *actual;
    let [u0, u1] = parameter;
    let [a0, a1] = angle.map(Ivl::from);
    let width = Ivl::from(u1) - Ivl::from(u0);
    let turn = a1 - a0;
    if width.lo <= 0.0
        || !width.hi.is_finite()
        || turn.mag() >= std::f64::consts::PI
        || w0.lo <= 0.0
        || w2.lo <= 0.0
    {
        return Err(bad("polar metadata has a singular span or weight"));
    }
    let middle = (a0 + a1) / Ivl::from(2.0);
    let half = turn / Ivl::from(2.0);
    let (s0, c0) = certificate::sin_cos(a0)?;
    let (sm, cm) = certificate::sin_cos(middle)?;
    let (s1, c1) = certificate::sin_cos(a1)?;
    let (_, ch) = certificate::sin_cos(half)?;
    if ch.lo <= 0.0 {
        return Err(bad("polar ideal middle weight is not positive"));
    }
    let geometric_mean = (w0 * w2).sqrt();
    let ideal = [
        ([c0 * w0, s0 * w0, Ivl::from(0.0)], w0),
        (
            [cm * geometric_mean, sm * geometric_mean, Ivl::from(0.0)],
            ch * geometric_mean,
        ),
        ([c1 * w2, s1 * w2, Ivl::from(0.0)], w2),
    ];
    let coordinate_error = interval_net_distance(&[actual.to_vec()], &[ideal.to_vec()])?;
    let remaining = Ivl::from(1.0) - Ivl::from(coordinate_error);
    if remaining.lo <= 0.0 {
        return Err(bad(
            "polar curve is not proved away from its angular origin",
        ));
    }
    // A point within delta<1 of a unit vector differs in angle by at most
    // asin(delta) <= delta/(1-delta). This is a uniform bound, not a sample.
    let angular_error = (Ivl::from(coordinate_error) / remaining).hi;
    let ea = w0.sqrt();
    let eb = w2.sqrt();
    let tau = certificate::tan(turn / Ivl::from(4.0))?;
    // Extent1d::classify_point compares abs(fl(u-end)) <= EFF_ZERO, and
    // PolarConversion substitutes the STEP endpoint angle on that branch.
    // Enclose the exact parameter distance permitted by the rounded comparison.
    let snap_distance = (Ivl::from(crate::math::EFF_ZERO) + Ivl::from(f64::from_bits(1)))
        / (Ivl::from(1.0) - Ivl::from(UNIT));
    // The same endpoint classification applies just outside the natural interval.
    // On its extension, a+(b-a)s is at least min(a,b)-|b-a|*snap_distance/width.
    let minimum = Ivl::from(ea.lo.min(eb.lo)) - Ivl::from((eb - ea).mag()) * snap_distance / width;
    if minimum.lo <= 0.0 {
        return Err(bad(
            "endpoint substitution leaves the proved polar parameter chart",
        ));
    }
    let angle_speed =
        (Ivl::from(4.0) * Ivl::from(tau.mag()) * ea * eb / (minimum * minimum * width)).hi;
    let mut endpoint_snap_error = 0.0_f64;
    for (target, endpoint) in endpoint_targets.into_iter().zip([a0, a1]) {
        if let Some(target) = target {
            endpoint_snap_error = endpoint_snap_error.max(
                (Ivl::from((target - endpoint).mag()) + Ivl::from(angle_speed) * snap_distance).hi,
            );
        }
    }
    if !angle_speed.is_finite()
        || !angular_error.is_finite()
        || !endpoint_snap_error.is_finite()
        || angular_error.max(endpoint_snap_error) > angle_budget
    {
        return Err(bad(
            "polar metadata and endpoint substitution exceed the angular budget",
        ));
    }
    Ok(PolarArcCertificate {
        angular_error,
        #[cfg(test)]
        endpoint_snap_error,
    })
}

struct CircleArc {
    parameter: [f64; 2],
    angle: [f64; 2],
    endpoint_scale: [f64; 2],
    tau: f64,
    second_angle_bound: f64,
    metadata: PolarArcCertificate,
    coefficients: [PolarCoefficient; 3],
}

impl CircleArc {
    // For endpoint homogeneous scales a²,b², a rational quadratic circle has
    // tan((theta-mid)/2) = tau*((a+b)s-a)/(a+(b-a)s), s=(u-u0)/(u1-u0).
    fn parameter_at(&self, theta: f64) -> Result<f64> {
        let [a, b] = self.endpoint_scale;
        let [t0, t1] = self.angle;
        let q = ((theta - (t0 + t1) * 0.5) * 0.5).tan();
        let denominator = self.tau * (a + b) - q * (b - a);
        let s = a * (q + self.tau) / denominator;
        let [u0, u1] = self.parameter;
        let u = u0 + (u1 - u0) * s;
        if !u.is_finite() || denominator == 0.0 {
            return Err(bad("singular cylinder circle parameterization"));
        }
        Ok(u)
    }

    fn angle_at(&self, u: f64) -> f64 {
        let [u0, u1] = self.parameter;
        let [a, b] = self.endpoint_scale;
        let [t0, t1] = self.angle;
        let s = (u - u0) / (u1 - u0);
        let q = self.tau * ((a + b) * s - a) / (a + (b - a) * s);
        (t0 + t1) * 0.5 + 2.0 * q.atan()
    }
}

struct Cylinder<'a> {
    cone: &'a Cone,
    arcs: Vec<CircleArc>,
    angle: Ivl,
    height_limits: [Ivl; 2],
    native_height: [f64; 2],
    // Cylinder native linear parameter = a + b * cylinder STEP height.
    linear_map: [f64; 2],
    sense: f64,
}

/// The complete underlying angular chart, independent of the trimmed face.
/// A continuation caller may then bound only axial support motion.
pub(super) fn complete_angular_support(
    k: &Kernel,
    cone: &Cone,
    axis: Vec3,
    tolerance: f64,
) -> Result<bool> {
    let domain = cone.rev().base().natural_uv_domain();
    Cylinder::new(k, cone, &domain, axis, tolerance)?.covers_full_circle(&domain)
}

impl<'a> Cylinder<'a> {
    fn new(
        k: &Kernel,
        cone: &'a Cone,
        domain: &Extent2d,
        axis: Vec3,
        tolerance: f64,
    ) -> Result<Self> {
        let frame = cone.canonical();
        let sense = if frame.origin.z_axis() == axis {
            1.0
        } else {
            -1.0
        };
        let native = |u: f64, v: f64| {
            if frame.swap_uv {
                Point2::new(v, u)
            } else {
                Point2::new(u, v)
            }
        };
        let ([u0, u1], [v0, v1]) = if frame.swap_uv {
            (
                [domain.v_min(), domain.v_max()],
                [domain.u_min(), domain.u_max()],
            )
        } else {
            (
                [domain.u_min(), domain.u_max()],
                [domain.v_min(), domain.v_max()],
            )
        };
        let p0 = cone.convert_uv_from_nurbs_to_step(k, native(u0, v0))?;
        let p1 = cone.convert_uv_from_nurbs_to_step(k, native(u1, v1))?;
        let angle = Ivl::new(p0.x.min(p1.x).to_radians(), p0.x.max(p1.x).to_radians());
        let height_map = HeightMap::new(k, cone)?;
        let height = height_map.range(Ivl::new(v0, v1))?;
        let [ha, hb] = height_map.exact_coefficients();
        let natural = cone.rev().base().natural_uv_domain();
        let native_height = if frame.swap_uv {
            natural.u_interval()
        } else {
            natural.v_interval()
        };
        if !v0.is_finite()
            || !v1.is_finite()
            || v0 >= v1
            || v0 < native_height.min()
            || v1 > native_height.max()
        {
            return Err(bad("cylinder height trim lies outside its native chart"));
        }
        let h0 = ha + hb * Ivl::from(v0);
        let h1 = ha + hb * Ivl::from(v1);
        let height_limits = if hb.lo > 0.0 {
            [h0, h1]
        } else if hb.hi < 0.0 {
            [h1, h0]
        } else {
            return Err(bad("cylinder height orientation is unproved"));
        };
        let b = (v1 - v0) / (p1.y - p0.y);
        let a = v0 - b * p0.y;
        if ![angle.lo, angle.hi, height.lo, height.hi, a, b]
            .iter()
            .all(|v| v.is_finite())
            || angle.lo >= angle.hi
            || height.lo >= height.hi
            || b == 0.0
        {
            return Err(bad("invalid cylinder intersection domain"));
        }
        let polar = cone
            .rev()
            .polar_curve()
            .ok_or_else(|| bad("cylinder has no rational angular parameterization"))?;
        let circle = polar.base().canonical()?;
        let raw_controls = polar.base().control_points()?;
        let angle_budget = (Ivl::from(tolerance) / Ivl::from(frame.radius)).lo;
        let polar_frame = polar.canonical();
        let endpoint = |end: bool| {
            let degrees = if end == polar_frame.inside_out {
                polar_frame.domain.min()
            } else {
                polar_frame.domain.max()
            };
            (Ivl::from(degrees) * Ivl::checked(TAU, TAU) / Ivl::from(360.0))
                .hull(Ivl::from(degrees.to_radians()))
        };
        if circle.degree != 2
            || circle.knots.len() < 2
            || circle.knots.len() > MAX_SPANS + 1
            || circle.control_points.len() != 2 * circle.knots.len() - 1
            || circle.weights.len() != circle.control_points.len()
            || circle
                .knot_multiplicities
                .iter()
                .enumerate()
                .any(|(i, &m)| {
                    m != if i == 0 || i + 1 == circle.knots.len() {
                        3
                    } else {
                        2
                    }
                })
        {
            return Err(bad(
                "cylinder angular parameterization must have quadratic Bezier circle spans",
            ));
        }
        let mut arcs = Vec::new();
        for (i, pair) in circle.knots.windows(2).enumerate() {
            k.check_cancelled("offset::procedural_screw::cylinder")?;
            let [from, to] = *pair else { continue };
            let theta0 = cone
                .convert_uv_from_nurbs_to_step(k, native(from, v0))?
                .x
                .to_radians();
            let theta1 = cone
                .convert_uv_from_nurbs_to_step(k, native(to, v0))?
                .x
                .to_radians();
            let weight = |j| {
                circle
                    .weights
                    .get(j)
                    .copied()
                    .ok_or_else(|| bad("missing circle weight"))
            };
            let w0 = weight(2 * i)?;
            let w1 = weight(2 * i + 2)?;
            if ![from, to, theta0, theta1, w0, w1]
                .iter()
                .all(|v| v.is_finite())
                || from >= to
                || w0 <= 0.0
                || w1 <= 0.0
                || theta0 == theta1
                || (theta1 - theta0).abs() >= std::f64::consts::PI
            {
                return Err(bad("invalid cylinder rational circle span"));
            }
            let ea = Ivl::from(w0).sqrt();
            let eb = Ivl::from(w1).sqrt();
            let tau = ((theta1 - theta0) * 0.25).tan();
            let raw = raw_controls
                .get(2 * i..2 * i + 3)
                .ok_or_else(|| bad("polar certificate is missing control points"))?;
            let actual = polar_coefficients(polar, raw)?;
            let metadata = certify_polar_arc(
                &actual,
                [from, to],
                [theta0, theta1],
                [
                    if i == 0 { Some(endpoint(false)) } else { None },
                    if i + 2 == circle.knots.len() {
                        Some(endpoint(true))
                    } else {
                        None
                    },
                ],
                angle_budget,
            )?;
            let t = Ivl::from(
                certificate::tan((Ivl::from(theta1) - Ivl::from(theta0)) / Ivl::from(4.0))?.mag(),
            );
            let low = Ivl::from(ea.lo.min(eb.lo));
            let difference = Ivl::from((eb - ea).mag());
            let width = Ivl::from(to) - Ivl::from(from);
            let second = (Ivl::from(8.0) * t * ea * eb * difference / (low * low * low)
                + Ivl::from(16.0) * t * t * t * ea * ea * eb * eb / (low * low * low * low))
                / (width * width);
            if !second.hi.is_finite() || low.lo <= 0.0 {
                return Err(bad("cylinder angular derivative bound is not finite"));
            }
            arcs.push(CircleArc {
                parameter: [from, to],
                angle: [theta0, theta1],
                endpoint_scale: [w0.sqrt(), w1.sqrt()],
                tau,
                second_angle_bound: second.hi,
                metadata,
                coefficients: actual,
            });
        }
        Ok(Self {
            cone,
            arcs,
            angle,
            height_limits,
            native_height: [v0, v1],
            linear_map: [a, b],
            sense,
        })
    }

    /// The normalized polar chart covers the circle once: exact closure,
    /// strictly signed angular derivative, and a total turn strictly below 4pi.
    fn covers_full_circle(&self, domain: &Extent2d) -> Result<bool> {
        let polar = self
            .cone
            .rev()
            .polar_curve()
            .ok_or_else(|| bad("missing polar chart"))?;
        let natural = polar.base().natural_interval();
        let angular = if self.cone.rev().swap_uv() {
            domain.v_interval()
        } else {
            domain.u_interval()
        };
        if angular.min() != natural.min() || angular.max() != natural.max() {
            return Ok(false);
        }
        let controls = polar.base().control_points()?;
        if controls.is_empty() || controls.first() != controls.last() {
            return Ok(false);
        }
        let mut orientation = 0.0;
        let mut last = None;
        let mut total = Ivl::from(0.0);
        for arc in &self.arcs {
            let [a, b] = arc.angle;
            let sign = (b - a).signum();
            if (orientation != 0.0 && sign != orientation) || last.is_some_and(|value| value != a) {
                return Ok(false);
            }
            orientation = sign;
            last = Some(b);
            let [(p0, _), (p1, _), (p2, _)] = arc.coefficients;
            let cross = |[x, y, _]: [Ivl; 3], [u, v, _]: [Ivl; 3]| x * v - y * u;
            let derivative = [
                Ivl::from(2.0) * cross(p0, p1),
                cross(p0, p2),
                Ivl::from(2.0) * cross(p1, p2),
            ];
            if derivative
                .into_iter()
                .any(|value| (value * Ivl::from(sign)).lo <= 0.0)
            {
                return Ok(false);
            }
            let width = Ivl::from(b) - Ivl::from(a);
            let (_, half_cos) = certificate::sin_cos(width / Ivl::from(2.0))?;
            if half_cos.lo <= arc.metadata.angular_error {
                return Ok(false);
            }
            total = total
                + Ivl::from(width.mag())
                + Ivl::from(2.0) * Ivl::from(arc.metadata.angular_error);
        }
        Ok(orientation != 0.0 && total.hi < (Ivl::from(2.0) * Ivl::checked(TAU, TAU)).lo)
    }

    fn uv(&self, angular: f64, height: f64) -> Result<Point3> {
        let [a, b] = self.linear_map;
        let linear = a + b * height;
        let [lo, hi] = self.native_height;
        if !linear.is_finite() || !lo.is_finite() || !hi.is_finite() || lo >= hi {
            return Err(bad("invalid cylinder pcurve height"));
        }
        // All homogeneous weights are positive. Clamping the height controls to
        // the requested chart therefore keeps the entire pcurve inside its trim.
        // The paired certificate accounts for the resulting displacement.
        let linear = crate::math::clamp_between(linear, lo, hi);
        Ok(if self.cone.rev().swap_uv() {
            Point3::new(linear, angular, 0.0)
        } else {
            Point3::new(angular, linear, 0.0)
        })
    }
}

struct ClippedOrbit {
    angles: [f64; 2],
    parameters: [f64; 2],
    endpoint_error: f64,
}

/// The root's orbit has height base+slope*theta. Nonzero slope makes its
/// intersection with a height slab one interval, including both boundary events.
/// Interval max/min encloses its exact endpoints. A disjoint enclosure proves
/// emptiness; touching or unordered endpoint enclosures remain a typed refusal.
/// The stored endpoints need not be exact roots: their full spatial error is
/// bounded by the orbit speed and composed into every returned curve certificate.
fn clip_height(
    wall: &Wall<'_>,
    source: Ivl,
    base: Ivl,
    slope: Ivl,
    heights: [Ivl; 2],
) -> Result<Option<ClippedOrbit>> {
    if slope.mig() <= 0.0 || !slope.lo.is_finite() || !slope.hi.is_finite() {
        return Err(bad("cylinder height clipping needs nonzero proved pitch"));
    }
    let [bottom, top] = heights;
    let first = (bottom - base) / slope;
    let last = (top - base) / slope;
    let [lower, upper] = if slope.lo > 0.0 {
        [first, last]
    } else {
        [last, first]
    };
    let lower = Ivl::new(lower.lo.max(wall.angle.lo), lower.hi.max(wall.angle.lo));
    let upper = Ivl::new(upper.lo.min(wall.angle.hi), upper.hi.min(wall.angle.hi));
    if ![lower.lo, lower.hi, upper.lo, upper.hi]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(bad("cylinder trim endpoints are not finite"));
    }
    if lower.lo > upper.hi {
        return Ok(None);
    }
    if lower.hi >= upper.lo {
        return Err(bad("cylinder height boundary contact is not resolved"));
    }
    let angles = [lower.mid(), upper.mid()];
    let [begin, end] = angles;
    let parameters = [
        wall.definition.parameter_at_angle(begin)?,
        wall.definition.parameter_at_angle(end)?,
    ];
    let mut error = 0.0_f64;
    for (parameter, target) in parameters.into_iter().zip([lower, upper]) {
        let actual = wall
            .definition
            .mathematical_turn_range(Ivl::from(parameter))?;
        error = error.max((actual - target).mag());
    }
    let endpoint_error =
        (Ivl::from(error) * Ivl::from(wall.section.speed_upper_bounds(source)?[1])).hi;
    if !endpoint_error.is_finite() {
        return Err(bad("cylinder trim endpoint error is not finite"));
    }
    Ok(Some(ClippedOrbit {
        angles,
        parameters,
        endpoint_error,
    }))
}

enum Candidate {
    /// Interval isolation excluded every source parameter at the literal radius.
    RadiusDisjoint,
    /// Incidence is certified, but angular chart coverage is incomplete.
    Curves(SurfaceIntersection),
    /// All radius roots, full circular chart, height and wall trim coverage proved.
    Complete(SurfaceIntersection),
}

pub(super) fn eligible(surface: &Surface, cylinder_surface: &Surface) -> Result<bool> {
    let Some(sweep) = surface.procedural_sweep() else {
        return Ok(false);
    };
    let definition = sweep.definition();
    if !definition.is_normal_offset() || !definition.is_screw_invariant() {
        return Ok(false);
    }
    let (Some(motion), SurfaceKind::Cone(cone)) = (definition.screw(), cylinder_surface.kind())
    else {
        return Ok(false);
    };
    let frame = cone.canonical();
    Ok(frame.semi_angle_deg == 0.0
        && (frame.origin.z_axis() == motion.axis() || frame.origin.z_axis() == -motion.axis())
        && (frame.origin.origin() - motion.origin()).cross(motion.axis()) == Vec3::ZERO
        && definition.screw_offset_section()?.is_some())
}

pub(super) fn intersect(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    cylinder_surface: &Surface,
    cylinder_domain: &Extent2d,
    tolerance: f64,
) -> Result<Option<SurfaceIntersection>> {
    // Radius absence is a complete geometric proof independent of angular/height
    // clipping. Nonempty candidates also require a complete chart/root/trim proof;
    // an incidence certificate alone cannot authorize clipped or empty results.
    match intersect_candidate(
        k,
        surface,
        domain,
        cylinder_surface,
        cylinder_domain,
        tolerance,
    )? {
        None => Ok(None),
        Some(Candidate::RadiusDisjoint) => Ok(Some(SurfaceIntersection::default())),
        Some(Candidate::Complete(result)) => Ok(Some(result)),
        Some(Candidate::Curves(result)) => Err(bad(if result.curves_3d.is_empty() {
            "cylinder height/angle exclusion is not certified"
        } else {
            "cylinder intersection trim completeness is not certified"
        })),
    }
}

fn intersect_candidate(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    cylinder_surface: &Surface,
    cylinder_domain: &Extent2d,
    tolerance: f64,
) -> Result<Option<Candidate>> {
    if !eligible(surface, cylinder_surface)? {
        return Ok(None);
    }
    let Some(sweep) = surface.procedural_sweep() else {
        return Ok(None);
    };
    if !sweep.definition().is_normal_offset() || !sweep.definition().is_screw_invariant() {
        return Ok(None);
    }
    let Some(motion) = sweep.definition().screw() else {
        return Ok(None);
    };
    let SurfaceKind::Cone(cone) = cylinder_surface.kind_with_nurb(k)? else {
        return Ok(None);
    };
    let frame = cone.canonical();
    if frame.semi_angle_deg != 0.0
        || (frame.origin.z_axis() != motion.axis() && frame.origin.z_axis() != -motion.axis())
        || (frame.origin.origin() - motion.origin()).cross(motion.axis()) != Vec3::ZERO
    {
        return Ok(None);
    }
    let Some(wall) = Wall::new(surface, domain)? else {
        return Ok(None);
    };
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(bad(
            "cylinder intersection needs a positive finite tolerance",
        ));
    }
    let cylinder = Cylinder::new(k, cone, cylinder_domain, motion.axis(), tolerance)?;
    if motion.law().is_some() {
        return Err(bad(
            "cylinder radius isolation needs a constant screw group",
        ));
    }
    let radius = cone
        .coaxial_radius_enclosure(motion.origin(), motion.axis())?
        .ok_or_else(|| {
            bad("cylinder radius isolation needs a proved conformal coaxial placement")
        })?;
    let center = frame.origin.origin();
    let source_origin = motion.origin();
    let axis = motion.axis();
    if [center.x, center.y, center.z]
        .into_iter()
        .zip([source_origin.x, source_origin.y, source_origin.z])
        .zip([axis.x, axis.y, axis.z])
        .any(|((a, b), axis)| axis == 0.0 && a != b)
    {
        return Err(bad(
            "cylinder radius isolation needs exactly coaxial placements",
        ));
    }
    // Pick a meridian chart by proved projection signs, not a sampled normal.
    let point = wall.section.point_enclosure(wall.profile)?;
    let [px, py, pz] = point;
    let origin = motion.origin();
    let projection = |v: Vec3| {
        (px - Ivl::from(origin.x)) * Ivl::from(v.x)
            + (py - Ivl::from(origin.y)) * Ivl::from(v.y)
            + (pz - Ivl::from(origin.z)) * Ivl::from(v.z)
    };
    let radial = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z]
        .into_iter()
        .find(|&axis| axis.dot(motion.axis()) == 0.0 && projection(axis).lo > 0.0)
        .ok_or_else(|| bad("cylinder section has no proved open meridian chart"))?;
    let bounds = wall
        .section
        .meridian_bounds(wall.profile.lo, wall.profile.hi, radial)?;
    // A vertical section wider than the wall's entire height enclosure makes its
    // height parameter auxiliary; the isolated root is the radius equality.
    let margin = (Ivl::from(bounds.height.hi) - Ivl::from(bounds.height.lo))
        .hi
        .max(bounds.radius.hi);
    let lo = (Ivl::from(bounds.height.lo) - Ivl::from(margin)).lo;
    let hi = (Ivl::from(bounds.height.hi) + Ivl::from(margin)).hi;
    let vertical = ScrewOffsetSection::cylinder_reference(radius, [lo, hi], motion, radial)?;
    let mut remaining = MAX_BOXES;
    let roots = roots(
        [&wall.section, &vertical],
        [wall.profile, Ivl::new(0.0, 1.0)],
        radial,
        Ivl::from(0.0),
        tolerance / 64.0,
        &mut remaining,
    )?;
    if roots.is_empty() {
        return Ok(Some(Candidate::RadiusDisjoint));
    }
    let mut out = SurfaceIntersection::default();
    let mut span_budget = MAX_SPANS;
    let (map_a, map_b, swapped) = wall.definition.definition_map();
    let v_trim = if swapped {
        domain.u_interval()
    } else {
        domain.v_interval()
    };
    let u_trim = if swapped {
        domain.v_interval()
    } else {
        domain.u_interval()
    };
    let exact_wall_ends = map_a == [0.0, 0.0]
        && map_b == [1.0, 1.0]
        && wall.definition.stations().first().copied() == Some(v_trim.min())
        && wall.definition.stations().last().copied() == Some(v_trim.max());
    let full_circle = cylinder.covers_full_circle(cylinder_domain)?;
    let mut complete = exact_wall_ends && full_circle;
    for root in roots {
        let start = out.curves_3d.len();
        let root_w = Ivl::from(wall.source[0])
            + (Ivl::from(wall.source[1]) - Ivl::from(wall.source[0])) * root.first;
        complete &= root_w.lo > u_trim.min() && root_w.hi < u_trim.max();
        let [px, py, pz] = wall.section.point_enclosure(root.first)?;
        let center = frame.origin.origin();
        let z = frame.origin.z_axis();
        let base = (px - Ivl::from(center.x)) * Ivl::from(z.x)
            + (py - Ivl::from(center.y)) * Ivl::from(z.y)
            + (pz - Ivl::from(center.z)) * Ivl::from(z.z);
        let slope_box =
            Ivl::from(cylinder.sense) * Ivl::from(motion.pitch()) / Ivl::checked(TAU, TAU);
        let Some(trim) = clip_height(&wall, root.first, base, slope_box, cylinder.height_limits)?
        else {
            continue;
        };
        let x = root.first.mid();
        // At the same theta, this encloses the complete spatial displacement
        // from the midpoint generator to the unique true root, including phase.
        let root_uncertainty = (Ivl::from(wall.section.speed_upper_bounds(root.first)?[0])
            * Ivl::from((root.first - Ivl::from(x)).mag())
            + Ivl::from(trim.endpoint_error))
        .hi;
        let point = wall.section.evaluate(x)?.0;
        let meridian = wall.section.meridian(x, radial)?;
        let reference = radial
            .dot(frame.origin.y_axis())
            .atan2(radial.dot(frame.origin.x_axis()));
        let phase = cylinder.sense * meridian.phase + reference;
        let base_height = (point - frame.origin.origin()).dot(frame.origin.z_axis());
        let rise = motion.pitch() / TAU;
        let [lo, hi] = trim.angles;
        let theta = Ivl::new(lo, hi);
        let physical = Ivl::from(cylinder.sense) * theta + Ivl::from(phase);
        let winds = (physical - cylinder.angle) / Ivl::from(TAU);
        if !winds.lo.is_finite()
            || !winds.hi.is_finite()
            || winds.lo < -(MAX_WINDINGS as f64)
            || winds.hi > MAX_WINDINGS as f64
        {
            return Err(bad("cylinder intersection exceeds its winding budget"));
        }
        let first_winding = winds.lo.ceil() as i64;
        let last_winding = winds.hi.floor() as i64;
        // A proved full polar chart has one boundary per lifted revolution.
        // Build that boundary once and reuse it on both incident pieces: the
        // separately rounded expressions angle.hi-(phase-n*TAU) and
        // angle.lo-(phase-(n+1)*TAU) need not agree. This partitions the whole
        // clipped orbit without changing either height endpoint or accepting
        // a gap/overlap. The stored paired curves still pass the same complete
        // incidence certificate, including their polar endpoint arithmetic.
        let boundary = |n: i64| (cylinder.angle.lo + n as f64 * TAU - phase) / cylinder.sense;
        let mut previous = boundary(first_winding);
        for n in first_winding..=last_winding {
            let shift = phase - n as f64 * TAU;
            let limits = if full_circle {
                let next = boundary(n + 1);
                let pair = [previous, next];
                previous = next;
                pair
            } else {
                [
                    (cylinder.angle.lo - shift) / cylinder.sense,
                    (cylinder.angle.hi - shift) / cylinder.sense,
                ]
            };
            let lo = theta.lo.max(limits[0].min(limits[1]));
            let hi = theta.hi.min(limits[0].max(limits[1]));
            if lo < hi {
                append(
                    k,
                    &wall,
                    &cylinder,
                    x,
                    [lo, hi],
                    shift,
                    base_height,
                    rise,
                    root_uncertainty,
                    tolerance,
                    &mut span_budget,
                    &mut out,
                )?;
            }
        }
        complete &= covers_wall_interval(&out, start, swapped, trim.parameters)?;
    }
    Ok(Some(if complete {
        Candidate::Complete(out)
    } else {
        Candidate::Curves(out)
    }))
}

/// Exact stored endpoints, rather than a near-seam tolerance, prove coverage.
fn covers_wall_interval(
    out: &SurfaceIntersection,
    start: usize,
    swapped: bool,
    domain: [f64; 2],
) -> Result<bool> {
    let curves = out
        .surface1_uv_curves
        .get(start..)
        .ok_or_else(|| bad("missing cylinder wall pcurves"))?;
    let mut intervals = Vec::new();
    for curve in curves {
        let curve = curve
            .as_ref()
            .and_then(|c| c.b_spline_base())
            .ok_or_else(|| bad("missing cylinder wall pcurve"))?;
        let canonical = curve.canonical()?;
        let [a, b] = canonical.control_points.as_slice() else {
            return Ok(false);
        };
        if canonical.degree != 1 || !canonical.weights.is_empty() {
            return Ok(false);
        }
        let (a, b) = if swapped { (a.x, b.x) } else { (a.y, b.y) };
        if !a.is_finite() || !b.is_finite() || a >= b {
            return Ok(false);
        }
        intervals.push([a, b]);
    }
    intervals.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut end = domain[0];
    for [a, b] in intervals {
        if a != end {
            return Ok(false);
        }
        end = b;
    }
    Ok(end == domain[1] && domain[0] < domain[1])
}

#[allow(clippy::too_many_arguments)]
fn append(
    k: &Kernel,
    wall: &Wall<'_>,
    cylinder: &Cylinder<'_>,
    x: f64,
    angles: [f64; 2],
    shift: f64,
    base_height: f64,
    rise: f64,
    root_uncertainty: f64,
    tolerance: f64,
    budget: &mut usize,
    out: &mut SurfaceIntersection,
) -> Result<()> {
    let [lo, hi] = angles;
    let mut cuts = vec![lo, hi];
    cuts.extend(
        wall.definition
            .stations()
            .iter()
            .copied()
            .filter(|&t| t > lo && t < hi),
    );
    for arc in &cylinder.arcs {
        for t in arc.angle {
            // The lifted chart's two outer boundaries are already represented
            // by lo/hi. Reconstructing either through this different expression
            // can insert a second, ulp-separated cut outside the polar chart.
            // Only genuine interior polar knots subdivide this winding piece.
            if t == cylinder.angle.lo || t == cylinder.angle.hi {
                continue;
            }
            let theta = (t - shift) / cylinder.sense;
            if theta > lo && theta < hi {
                cuts.push(theta);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut points = Vec::new();
    let mut weights = Vec::new();
    let mut knots = Vec::new();
    for interval in cuts.windows(2) {
        let [from, to] = *interval else { continue };
        let mid = (from + to) * 0.5;
        let beta_mid = cylinder.sense * mid + shift;
        let arc = cylinder
            .arcs
            .iter()
            .find(|arc| {
                let [a, b] = arc.angle;
                beta_mid >= a.min(b) && beta_mid <= a.max(b)
            })
            .ok_or_else(|| bad("cylinder angle is outside its rational circle spans"))?;
        let first_u = arc.parameter_at(cylinder.sense * from + shift)?;
        let last_u = arc.parameter_at(cylinder.sense * to + shift)?;
        let width = Ivl::from(last_u) - Ivl::from(first_u);
        let error = Ivl::from(rise.abs()) * Ivl::from(arc.second_angle_bound) * width * width
            / Ivl::from(8.0);
        let mut pieces = 1_usize;
        while (error / Ivl::from(pieces as f64) / Ivl::from(pieces as f64)).hi > tolerance * 0.25 {
            pieces = pieces
                .checked_mul(2)
                .filter(|&n| n <= MAX_SPANS)
                .ok_or_else(|| bad("cylinder pcurve cannot meet its bounded fit budget"))?;
        }
        *budget = budget
            .checked_sub(pieces)
            .ok_or_else(|| bad("cylinder pcurve span budget exceeded"))?;
        let (a0, a1) = span(wall.definition, mid)?;
        let am = (a0 + a1) * 0.5;
        let at = ((a1 - a0) * 0.25).tan();
        let [b0, b1] = arc.angle;
        let q0 = ((cylinder.sense * am + shift - (b0 + b1) * 0.5) * 0.5).tan();
        let [ea, eb] = arc.endpoint_scale;
        for i in 0..pieces {
            for j in [i, i + 1] {
                let u = first_u + (last_u - first_u) * j as f64 / pieces as f64;
                let theta = if j == 0 {
                    from
                } else if j == pieces {
                    to
                } else {
                    (arc.angle_at(u) - shift) / cylinder.sense
                };
                let t = wall.definition.parameter_at_angle(theta)?;
                let z = cylinder.sense * at * (2.0 * (t - a0) / (a1 - a0) - 1.0);
                let qn = z + q0;
                let qd = 1.0 - q0 * z;
                let denominator = (arc.tau * (ea + eb) * qd - (eb - ea) * qn) * arc.tau.signum();
                if !denominator.is_finite() || denominator <= 0.0 {
                    return Err(bad("singular cylinder paired parameter map"));
                }
                points.push(cylinder.uv(u, base_height + cylinder.sense * rise * theta)?);
                weights.push(denominator);
                if j == i {
                    knots.push(t);
                }
            }
        }
    }
    let begin = wall.definition.parameter_at_angle(lo)?;
    let end = wall.definition.parameter_at_angle(hi)?;
    knots.push(end);
    let second_uv = uv_curve(&points, Some(&weights), &knots, &vec![2; knots.len()])?;
    let w = wall.profile_parameter(x);
    let first_points = [begin, end]
        .into_iter()
        .map(|t| {
            let p = wall.definition.face_parameters(Point2::new(w, t))?;
            Ok(Point3::new(p.x, p.y, 0.0))
        })
        .collect::<Result<Vec<_>>>()?;
    let first_uv = uv_curve(&first_points, None, &[begin, end], &[2, 2])?;
    // This compares the actual stored pair over its complete parameter interval.
    // Root localization, height interpolation and polar metadata discrepancies
    // are contained in the direct mathematical difference, not guessed floors.
    let remaining = (Ivl::from(tolerance) - Ivl::from(root_uncertainty)).lo;
    if !remaining.is_finite() || remaining <= 0.0 {
        return Err(bad(
            "isolated cylinder root has unproved spatial endpoint uncertainty",
        ));
    }
    let deviation = (Ivl::from(paired::certify(
        k, wall, cylinder, x, &first_uv, &second_uv, remaining,
    )?) + Ivl::from(root_uncertainty))
    .hi;
    if !deviation.is_finite() || deviation > tolerance {
        return Err(bad(
            "cylinder endpoint uncertainty exceeds the requested tolerance",
        ));
    }
    append_pair(
        k,
        wall,
        first_uv,
        second_uv,
        TsectCurveType::Crossing,
        deviation,
        out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;

    fn cylinder() -> (Kernel, Cone) {
        let k = Kernel::default();
        let cone = Cone::create_canonical(
            &k,
            &Axis2Placement::IDENTITY,
            10.0,
            10.0,
            20.0,
            false,
            false,
        )
        .unwrap();
        (k, cone)
    }

    #[test]
    fn canonical_polar_metadata_is_certified_including_endpoint_snapping() {
        let (k, cone) = cylinder();
        let domain = cone.rev().base().natural_uv_domain();
        let cylinder = Cylinder::new(&k, &cone, &domain, Vec3::Z, 1e-6).unwrap();
        assert!(!cylinder.arcs.is_empty());
        for arc in &cylinder.arcs {
            assert!(arc.metadata.angular_error.is_finite() && arc.metadata.angular_error < 1e-7);
            assert!(
                arc.metadata.endpoint_snap_error.is_finite()
                    && arc.metadata.endpoint_snap_error < 1e-7
            );
        }
        let arc = cylinder.arcs.first().unwrap();
        let polar = cone.rev().polar_curve().unwrap();
        let start = arc.parameter[0];
        let inside = start + crate::math::EFF_ZERO * 0.5;
        let endpoint = polar.convert_t_from_nurbs_to_step(&k, start).unwrap();
        // Since #13558 a value just past the start keeps its own angle instead of being
        // set to the start; whatever the converter returns stays within the certificate.
        let converted = polar.convert_t_from_nurbs_to_step(&k, inside).unwrap();
        assert!(
            converted >= endpoint,
            "a value past the start converts to {converted}, before the start {endpoint}"
        );
        let displacement = (arc.angle_at(inside) - converted.to_radians()).abs();
        assert!(
            displacement <= arc.metadata.endpoint_snap_error,
            "native endpoint displacement {displacement}, certificate {:?}",
            arc.metadata
        );
        let before = start - crate::math::EFF_ZERO * 0.5;
        let snapped_before = polar.convert_t_from_nurbs_to_step(&k, before).unwrap();
        assert_eq!(snapped_before, endpoint);
        let displacement_before = (arc.angle_at(before) - snapped_before.to_radians()).abs();
        assert!(
            displacement_before > 0.0 && displacement_before <= arc.metadata.endpoint_snap_error
        );
        let outside = start + crate::math::EFF_ZERO * 2.0;
        assert!(polar.convert_t_from_nurbs_to_step(&k, outside).unwrap() > endpoint);
    }

    #[test]
    fn perturbed_polar_controls_or_angles_are_refused_by_the_metadata_certificate() {
        let (k, cone) = cylinder();
        let domain = cone.rev().base().natural_uv_domain();
        let cylinder = Cylinder::new(&k, &cone, &domain, Vec3::Z, 1e-6).unwrap();
        let arc = cylinder.arcs.first().unwrap();
        let polar = cone.rev().polar_curve().unwrap();
        let actual =
            polar_coefficients(polar, &polar.base().control_points().unwrap()[..3]).unwrap();
        let target = [Some(Ivl::from(arc.angle[0])), None];
        assert!(certify_polar_arc(&actual, arc.parameter, arc.angle, target, 1e-7).is_ok());
        let mut perturbed = actual;
        perturbed[1].0[0] = perturbed[1].0[0] + Ivl::from(0.01);
        assert!(certify_polar_arc(&perturbed, arc.parameter, arc.angle, target, 1e-7).is_err());
        let wrong_angles = [arc.angle[0], arc.angle[1] + 0.01];
        assert!(certify_polar_arc(&actual, arc.parameter, wrong_angles, target, 1e-7).is_err());
        // A mismatched STEP endpoint cannot hide behind an otherwise accurate
        // interior circle. Its entire EFF_ZERO substitution interval is covered.
        let wrong_endpoint = [Some(Ivl::from(arc.angle[0] + 0.01)), None];
        assert!(
            certify_polar_arc(&actual, arc.parameter, arc.angle, wrong_endpoint, 1e-7).is_err()
        );
    }

    #[test]
    fn height_map_encloses_native_bounded_and_reversed_conversion() {
        let k = Kernel::default();
        for inside in [false, true] {
            for swapped in [false, true] {
                let cone = Cone::create_canonical(
                    &k,
                    &Axis2Placement::IDENTITY,
                    10.0,
                    10.0,
                    20.0,
                    swapped,
                    inside,
                )
                .unwrap();
                let map = HeightMap::new(&k, &cone).unwrap();
                let domain = cone.rev().base().natural_uv_domain();
                let native = if swapped {
                    domain.u_interval()
                } else {
                    domain.v_interval()
                };
                for fraction in [0.0, 0.137, 0.371, 0.829, 1.0] {
                    let t = native.min() + fraction * native.length();
                    let uv = if swapped {
                        Point2::new(t, domain.v_min())
                    } else {
                        Point2::new(domain.u_min(), t)
                    };
                    let actual = cone.convert_uv_from_nurbs_to_step(&k, uv).unwrap().y;
                    let enclosure = map.range(Ivl::from(t)).unwrap();
                    assert!(
                        enclosure.lo <= actual && actual <= enclosure.hi,
                        "height {actual} outside {enclosure:?}; inside={inside}, swapped={swapped}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_complete_cylinder_crossing_and_its_height_trims_are_certified() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use crate::sweep::screw::screw_net;

        let k = Kernel::default();
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 2.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.1];
        let net = screw_net(&profile, &motion, &stations).unwrap();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.05).unwrap();
        // Zero displacement retains the offset class without moving the exact line.
        let wall = definition.normal_offset(&net, 0.0, 1e-4).unwrap();
        let sweep = wall.procedural_sweep().unwrap();
        assert!(sweep.definition().is_normal_offset());
        let domain = sweep.net().natural_uv_domain();
        let cylinder = Surface::from(
            Cone::create_canonical(&k, &Axis2Placement::IDENTITY, 9.5, 9.5, 20.0, false, false)
                .unwrap(),
        );
        let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        let tolerance = 1e-5;
        let candidate =
            intersect_candidate(&k, &wall, &domain, &cylinder, &cylinder_domain, tolerance)
                .unwrap()
                .expect("the affine section crosses the cylinder");
        let Candidate::Complete(candidate) = candidate else {
            panic!("crossing reported radius-disjoint")
        };
        assert!(
            !candidate.curves_3d.is_empty(),
            "the gate must be tested on a real candidate"
        );
        let SurfaceKind::Cone(cone) = cylinder.kind_with_nurb(&k).unwrap() else {
            panic!("cylinder fixture")
        };
        let cylinder_chart = Cylinder::new(&k, cone, &cylinder_domain, Vec3::Z, tolerance).unwrap();
        let wall_chart = Wall::new(&wall, &domain).unwrap().unwrap();
        let first = candidate
            .surface1_uv_curves
            .first()
            .unwrap()
            .as_ref()
            .unwrap();
        let second = candidate
            .surface2_uv_curves
            .first()
            .unwrap()
            .as_ref()
            .unwrap();
        assert!(paired::certify(
            &k,
            &wall_chart,
            &cylinder_chart,
            0.75,
            first,
            second,
            tolerance
        )
        .is_ok());
        let canonical = second.b_spline_base().unwrap().canonical().unwrap();
        let displaced = canonical
            .control_points
            .iter()
            .map(|p| *p + Vec3::Y * 0.01)
            .collect::<Vec<_>>();
        let wrong = uv_curve(
            &displaced,
            Some(&canonical.weights),
            &canonical.knots,
            &canonical.knot_multiplicities,
        )
        .unwrap();
        assert!(paired::certify(
            &k,
            &wall_chart,
            &cylinder_chart,
            0.75,
            first,
            &wrong,
            tolerance
        )
        .is_err());
        // These scalar checks establish the fixture, not the continuum proof.
        for curve in &candidate.curves_3d {
            let interval = curve.natural_interval();
            for fraction in [0.137, 0.371, 0.829] {
                let point = curve
                    .evaluate_point(&k, interval.min() + fraction * interval.length())
                    .unwrap();
                assert!((point.x.hypot(point.y) - 9.5).abs() <= tolerance);
            }
        }
        // Public acceptance requires both the continuum incidence certificate
        // and the independently proved chart/root/trim coverage.
        for (a, da, b, db) in [
            (&wall, &domain, &cylinder, &cylinder_domain),
            (&cylinder, &cylinder_domain, &wall, &domain),
        ] {
            let accepted = super::super::intersect(&k, a, da, b, db, tolerance)
                .unwrap()
                .unwrap();
            assert!(!accepted.curves_3d.is_empty());
            assert!(accepted.deviations.iter().all(|&bound| bound <= tolerance));
        }
        // Executive continuation must preserve the exact carrier and native
        // chart needed by this same complete incidence certificate.
        let continued = Surface::from(
            cone.extended_cylinder_keeping_parameters(&k, 0.21693393959008678)
                .unwrap()
                .unwrap(),
        );
        let continued_domain = continued.natural_uv_domain_with_nurb(&k).unwrap();
        for (a, da, b, db) in [
            (&wall, &domain, &continued, &continued_domain),
            (&continued, &continued_domain, &wall, &domain),
        ] {
            let accepted = super::super::intersect(&k, a, da, b, db, tolerance)
                .unwrap()
                .expect("continued cylinder retains common-section routing");
            assert!(!accepted.curves_3d.is_empty());
            assert!(accepted.deviations.iter().all(|&bound| bound <= tolerance));
        }
        // The actual M20 root cylinders retain conformal frames with these
        // small nonzero components. Their literal radius is r*|X|, not r.
        let placement = Axis2Placement::from_stored(
            Vec3::ZERO,
            Vec3::new(1.7396793334894887e-14, -1.0, 0.0),
            Vec3::new(1.0, 1.7396793334894887e-14, 0.0),
        );
        let rotated = Surface::from(
            Cone::create_canonical(&k, &placement, 9.5, 9.5, 20.0, false, false).unwrap(),
        );
        let rotated_domain = rotated.natural_uv_domain_with_nurb(&k).unwrap();
        let rotated_result =
            super::super::intersect(&k, &wall, &domain, &rotated, &rotated_domain, tolerance)
                .expect("literal conformal cylinder placement")
                .unwrap();
        assert!(!rotated_result.curves_3d.is_empty());
        // The canonical constructor normalizes its supplied frame. Isolation
        // must use the retained placement, not the caller's pre-construction
        // scale: this cylinder has radius 4.75 and is disjoint from the wall.
        let placement = Axis2Placement::from_stored(Vec3::ZERO, 2.0 * Vec3::X, 2.0 * Vec3::Y);
        let scaled = Surface::from(
            Cone::create_canonical(&k, &placement, 4.75, 4.75, 20.0, false, false).unwrap(),
        );
        let scaled_domain = scaled.natural_uv_domain_with_nurb(&k).unwrap();
        let SurfaceKind::Cone(scaled_cone) = scaled.kind_with_nurb(&k).unwrap() else {
            panic!("cylinder fixture")
        };
        assert_eq!(scaled_cone.canonical().origin, Axis2Placement::IDENTITY);
        let scaled_result =
            super::super::intersect(&k, &wall, &domain, &scaled, &scaled_domain, tolerance)
                .expect("isolation uses the retained canonical frame")
                .unwrap();
        assert!(scaled_result.curves_3d.is_empty());
        for height in [1.0, 1.52] {
            let limit = cone
                .convert_uv_from_step_to_nurbs(&k, Point2::new(0.0, height))
                .unwrap();
            let clipped = Extent2d::from_bounds(
                cylinder_domain.u_min(),
                cylinder_domain.v_min(),
                cylinder_domain.u_max(),
                limit.y,
            );
            let result = intersect(&k, &wall, &domain, &cylinder, &clipped, tolerance)
                .expect("monotone orbit height has a certified clipped interval")
                .unwrap();
            if height == 1.0 {
                assert!(result.curves_3d.is_empty());
            } else {
                assert!(!result.curves_3d.is_empty());
                let mut highest = f64::NEG_INFINITY;
                for curve in &result.curves_3d {
                    let interval = curve.natural_interval();
                    for t in [interval.min(), interval.max()] {
                        let p = curve.evaluate_point(&k, t).unwrap();
                        assert!(p.z >= 1.5 - tolerance && p.z <= height + tolerance);
                        highest = highest.max(p.z);
                    }
                }
                assert!((highest - height).abs() <= tolerance);
                assert!(result.deviations.iter().all(|&bound| bound <= tolerance));
            }
        }
    }

    #[test]
    fn height_clipping_covers_seams_and_both_pitch_signs() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use crate::sweep::screw::screw_net;

        let k = Kernel::default();
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 8.0),
            Vec3::new(10.0, 0.0, 10.0),
        )
        .unwrap();
        let stations = (0..=128)
            .map(|i| 2.0 * TAU * f64::from(i) / 128.0)
            .collect::<Vec<_>>();
        let cylinder = Surface::from(
            Cone::create_canonical(&k, &Axis2Placement::IDENTITY, 9.5, 9.5, 20.0, false, false)
                .unwrap(),
        );
        let SurfaceKind::Cone(cone) = cylinder.kind_with_nurb(&k).unwrap() else {
            panic!("cylinder")
        };
        let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        for (pitch, heights) in [(2.5, [10.2, 13.2]), (-2.5, [5.2, 8.2])] {
            let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, pitch, None).unwrap();
            let net = screw_net(&profile, &motion, &stations).unwrap();
            let definition = ProceduralSweep::create(&profile, motion, &stations, 0.05).unwrap();
            let wall = definition.normal_offset(&net, 0.0, 1e-4).unwrap();
            let domain = wall.natural_uv_domain_with_nurb(&k).unwrap();
            let [low, high] = heights;
            let a = cone
                .convert_uv_from_step_to_nurbs(&k, Point2::new(0.0, low))
                .unwrap();
            let b = cone
                .convert_uv_from_step_to_nurbs(&k, Point2::new(0.0, high))
                .unwrap();
            let clipped =
                Extent2d::from_bounds(cylinder_domain.u_min(), a.y, cylinder_domain.u_max(), b.y);
            for (first, da, second, db) in [
                (&wall, &domain, &cylinder, &clipped),
                (&cylinder, &clipped, &wall, &domain),
            ] {
                let result = super::super::intersect(&k, first, da, second, db, 1e-5)
                    .unwrap()
                    .unwrap();
                assert!(
                    result.curves_3d.len() >= 2,
                    "the clipped orbit crosses a seam"
                );
                let mut actual = [f64::INFINITY, f64::NEG_INFINITY];
                for curve in &result.curves_3d {
                    let range = curve.natural_interval();
                    for t in [range.min(), range.max()] {
                        let z = curve.evaluate_point(&k, t).unwrap().z;
                        actual[0] = actual[0].min(z);
                        actual[1] = actual[1].max(z);
                        assert!(z >= low - 1e-5 && z <= high + 1e-5);
                    }
                }
                assert!((actual[0] - low).abs() <= 1e-5);
                assert!((actual[1] - high).abs() <= 1e-5);
                assert!(result.deviations.iter().all(|&d| d <= 1e-5));
            }
        }
    }

    #[test]
    fn multi_turn_offset_cylinder_trims_share_partition_endpoints() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use crate::sweep::screw::screw_net;

        let k = Kernel::default();
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = (0..=640)
            .map(|i| 10.0 * TAU * f64::from(i) / 640.0)
            .collect::<Vec<_>>();
        let net = screw_net(&profile, &motion, &stations).unwrap();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.05).unwrap();
        let wall = definition.normal_offset(&net, 0.1, 1e-4).unwrap();
        let domain = wall.natural_uv_domain_with_nurb(&k).unwrap();
        let cylinder = Surface::from(
            Cone::create_canonical(&k, &Axis2Placement::IDENTITY, 9.5, 9.5, 20.0, false, false)
                .unwrap(),
        );
        let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        let candidate = intersect_candidate(&k, &wall, &domain, &cylinder, &cylinder_domain, 1e-5)
            .unwrap()
            .unwrap();
        assert!(matches!(candidate, Candidate::Complete(_)));
    }

    #[test]
    fn a_lifted_rotated_cylinder_keeps_only_interior_polar_cuts() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use crate::sweep::screw::screw_net;

        let k = Kernel::default();
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(9.125, -64.0, -1.3340812163512963),
            Vec3::new(7.646835306586814, -64.0, -2.1875),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::Z, 2.5, None).unwrap();
        let stations = (0..=560)
            .map(|i| f64::from(i) * (TAU / 56.0))
            .collect::<Vec<_>>();
        let net = screw_net(&profile, &motion, &stations).unwrap();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.06).unwrap();
        let wall = definition.normal_offset(&net, -0.1, 1e-4).unwrap();
        let domain = wall.natural_uv_domain_with_nurb(&k).unwrap();
        let placement = Axis2Placement::from_stored(
            Vec3::new(-1.0, -64.0, 12.967749999999999),
            Vec3::new(-2.45548340466059e-16, 1.0, 0.0),
            Vec3::new(-1.0, -2.45548340466059e-16, 0.0),
        );
        let cylinder = Cone::create_canonical(
            &k,
            &placement,
            8.646835306586814,
            8.646835306586814,
            2.814500000000006,
            true,
            false,
        )
        .unwrap()
        .extended_cylinder_keeping_parameters(&k, 0.21693393959008678)
        .unwrap()
        .unwrap();
        let cylinder = Surface::from(cylinder);
        let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        let result = super::super::intersect(&k, &wall, &domain, &cylinder, &cylinder_domain, 5e-6)
            .expect("literal M20 rotated cylinder needs a complete polar partition")
            .unwrap();
        assert!(!result.curves_3d.is_empty());
    }

    #[test]
    fn a_proved_radially_disjoint_cylinder_returns_empty() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        use crate::sweep::screw::screw_net;
        let k = Kernel::default();
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 2.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.1];
        let net = screw_net(&profile, &motion, &stations).unwrap();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.05).unwrap();
        let wall = definition.normal_offset(&net, 0.0, 1e-4).unwrap();
        let domain = wall.procedural_sweep().unwrap().net().natural_uv_domain();
        let cylinder = Surface::from(
            Cone::create_canonical(
                &k,
                &Axis2Placement::IDENTITY,
                12.0,
                12.0,
                20.0,
                false,
                false,
            )
            .unwrap(),
        );
        let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        for (a, da, b, db) in [
            (&wall, &domain, &cylinder, &cylinder_domain),
            (&cylinder, &cylinder_domain, &wall, &domain),
        ] {
            let result = super::super::intersect(&k, a, da, b, db, 1e-5)
                .unwrap()
                .unwrap();
            assert!(result.curves_3d.is_empty());
        }
    }

    #[test]
    fn cylinder_root_phase_uncertainty_is_not_only_radial_error() {
        use crate::surface::procedural_sweep::ScrewMotion;
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let section = ScrewOffsetSection::new(
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(10.001, 10.0, 2.5 / TAU),
            &motion,
            0.0,
        )
        .unwrap();
        let interval = Ivl::new(0.0, 0.001);
        let a = section.evaluate(interval.lo).unwrap().0;
        let b = section.evaluate(interval.hi).unwrap().0;
        let radial = (a.x.hypot(a.y) - b.x.hypot(b.y)).abs();
        let spatial = (a - b).length();
        assert!(
            radial < 1e-5 && spatial > 1e-3,
            "radial {radial}, endpoint displacement {spatial}"
        );
        let enclosed = (Ivl::from(section.speed_upper_bounds(interval).unwrap()[0])
            * (Ivl::from(interval.hi) - Ivl::from(interval.lo)))
        .hi;
        assert!(enclosed >= spatial);
    }
}
