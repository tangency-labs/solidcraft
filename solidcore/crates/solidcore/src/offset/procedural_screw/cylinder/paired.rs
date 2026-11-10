//! Continuum certificate of the actual stored paired pcurves.
//! Existing interval Taylor arithmetic encloses derivatives through order four.
//! A cubic at the cell midpoint plus the fourth-derivative remainder bounds every
//! parameter in the cell. Point evaluations alone never authorize a cell.
//! The screw rotation is rational in its half-angle coordinate; only its fixed
//! midpoint trigonometry uses the shared certified Taylor primitive.

use super::{
    bad, certificate, CircleArc, Cylinder, HeightMap, Ivl, Result, Vec3, Wall, MAX_SPANS, TAU,
};
use crate::blend::fillet_section::interval::sin_cos_certified;
use crate::blend::fillet_section::tay::{tay_sqrt, Tay};
use crate::curve::curve::Curve;
use crate::kernel::Kernel;
use crate::surface::procedural_sweep::screw_offset::atan_interval;

type Jet = Tay<Ivl>;
type Vector = [Jet; 3];
fn constant(v: Ivl) -> Jet {
    Jet::of_terms(v, Ivl::from(0.0))
}
fn number(v: f64) -> Jet {
    constant(Ivl::from(v))
}
fn vector(v: Vec3) -> Vector {
    [number(v.x), number(v.y), number(v.z)]
}
fn add(a: Vector, b: Vector) -> Vector {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x + u, y + v, z + w]
}
fn sub(a: Vector, b: Vector) -> Vector {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x - u, y - v, z - w]
}
fn scale(a: Vector, b: Jet) -> Vector {
    a.map(|x| x * b)
}
fn dot(a: Vector, b: Vector) -> Jet {
    let [x, y, z] = a;
    let [u, v, w] = b;
    x * u + y * v + z * w
}
fn cross(a: Vector, b: Vector) -> Vector {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [y * w - z * v, z * u - x * w, x * v - y * u]
}
fn magnitude(a: [Ivl; 3]) -> Ivl {
    let [x, y, z] = a.map(|v| Ivl::from(v.mag()));
    (x * x + y * y + z * z).sqrt()
}
fn coefficient(a: Vector, n: usize) -> [Ivl; 3] {
    a.map(|x| x.at(n))
}
fn overrun(range: Ivl, lo: f64, hi: f64) -> Ivl {
    Ivl::from(
        (Ivl::from(lo) - range)
            .hi
            .max((range - Ivl::from(hi)).hi)
            .max(0.0),
    )
}

fn screw_point(wall: &Wall<'_>, source: [Ivl; 3], span: [f64; 2], t: Jet) -> Result<Vector> {
    let motion = wall
        .definition
        .screw()
        .ok_or_else(|| bad("missing paired screw motion"))?;
    let [a, b] = span;
    let width = Ivl::from(b) - Ivl::from(a);
    let mid = (Ivl::from(a) + Ivl::from(b)) / Ivl::from(2.0);
    let tau = certificate::tan(width / Ivl::from(4.0))?;
    let q = constant(tau) * (number(2.0) * (t - number(a)) / constant(width) - number(1.0));
    let denominator = number(1.0) + q * q;
    let (sm, cm) = sin_cos_certified(mid);
    let c = (constant(cm) * (number(1.0) - q * q) - constant(sm) * number(2.0) * q) / denominator;
    let s = (constant(sm) * (number(1.0) - q * q) + constant(cm) * number(2.0) * q) / denominator;
    let mut derivative = Jet::zero();
    for n in 0..4 {
        derivative.set(n, q.at(n + 1) * Ivl::from((n + 1) as f64));
    }
    let rate = number(2.0) * derivative / denominator;
    let mut theta = constant(mid + Ivl::from(2.0) * atan_interval(q.at(0)));
    for n in 1..=4 {
        theta.set(n, rate.at(n - 1) / Ivl::from(n as f64));
    }
    let origin = vector(motion.origin());
    let k = vector(motion.axis());
    let r = sub(source.map(constant), origin);
    let rotated = add(
        add(scale(r, c), scale(cross(k, r), s)),
        scale(k, dot(k, r) * (number(1.0) - c)),
    );
    Ok(add(
        add(origin, rotated),
        scale(
            k,
            number(motion.pitch()) * theta / constant(Ivl::checked(TAU, TAU)),
        ),
    ))
}

fn pcurve(span: certificate::Span, knots: [f64; 2], t: Jet) -> [Jet; 2] {
    let [[x0, y0, w0], [x1, y1, w1]] = span;
    let [a, b] = knots;
    let s = (t - number(a)) / constant(Ivl::from(b) - Ivl::from(a));
    let w = constant(w0) + constant(w1 - w0) * s;
    [
        (constant(x0) + constant(x1 - x0) * s) / w,
        (constant(y0) + constant(y1 - y0) * s) / w,
    ]
}

fn polar(arc: &CircleArc, u: Jet) -> [Jet; 2] {
    let [a, b] = arc.parameter;
    let s = (u - number(a)) / constant(Ivl::from(b) - Ivl::from(a));
    let r = number(1.0) - s;
    let mut h = [Jet::zero(); 3];
    let mut weight = Jet::zero();
    for ((point, w), basis) in arc
        .coefficients
        .into_iter()
        .zip([r * r, number(2.0) * r * s, s * s])
    {
        h = add(h, scale(point.map(constant), basis));
        weight = weight + constant(w) * basis;
    }
    let [x, y, _] = scale(h, number(1.0) / weight);
    let length = tay_sqrt(&(x * x + y * y));
    [x / length, y / length]
}

fn cylinder_point(
    cylinder: &Cylinder<'_>,
    arc: &CircleArc,
    height: [Ivl; 2],
    uv: [Jet; 2],
) -> Vector {
    let frame = cylinder.cone.canonical();
    let [u, v] = if frame.swap_uv { [uv[1], uv[0]] } else { uv };
    let [x, y] = polar(arc, u);
    let [a, b] = height;
    add(
        add(
            vector(frame.origin.origin()),
            scale(
                add(
                    scale(vector(frame.origin.x_axis()), x),
                    scale(vector(frame.origin.y_axis()), y),
                ),
                number(frame.radius),
            ),
        ),
        scale(vector(frame.origin.z_axis()), constant(a) + constant(b) * v),
    )
}

/// A global native angular Lipschitz bound of the exact normalized polar curve.
/// Positive weights and the independently proved ideal-circle distance give the
/// nonzero projection denominator. It also bounds clamped outer continuations.
fn angular_speed(cylinder: &Cylinder<'_>) -> Result<f64> {
    let frame = cylinder.cone.canonical();
    let placement = Ivl::from(frame.radius)
        * (magnitude([
            Ivl::from(frame.origin.x_axis().x),
            Ivl::from(frame.origin.x_axis().y),
            Ivl::from(frame.origin.x_axis().z),
        ]) + magnitude([
            Ivl::from(frame.origin.y_axis().x),
            Ivl::from(frame.origin.y_axis().y),
            Ivl::from(frame.origin.y_axis().z),
        ]));
    let mut speed = 0.0_f64;
    for arc in &cylinder.arcs {
        let [a, b] = arc.parameter;
        let width = Ivl::from(b) - Ivl::from(a);
        let wmin = arc
            .coefficients
            .iter()
            .map(|(_, w)| w.lo)
            .fold(f64::INFINITY, f64::min);
        let wmax = arc
            .coefficients
            .iter()
            .map(|(_, w)| w.hi)
            .fold(0.0, f64::max);
        let hmax = arc
            .coefficients
            .iter()
            .map(|(h, _)| magnitude(*h).hi)
            .fold(0.0, f64::max);
        let mut dh = 0.0_f64;
        let mut dw = 0.0_f64;
        for pair in arc.coefficients.windows(2) {
            let [(a, wa), (b, wb)] = *pair else { continue };
            let [ax, ay, az] = a;
            let [bx, by, bz] = b;
            dh = dh.max((Ivl::from(2.0) * magnitude([bx - ax, by - ay, bz - az]) / width).hi);
            dw = dw.max((Ivl::from(2.0) * Ivl::from((wb - wa).mag()) / width).hi);
        }
        let radius = Ivl::from(1.0) - Ivl::from(arc.metadata.angular_error);
        if wmin <= 0.0 || radius.lo <= 0.0 {
            return Err(bad("cylinder derivative has no positive denominator"));
        }
        speed = speed.max(
            (placement * (Ivl::from(dh) * Ivl::from(wmax) + Ivl::from(hmax) * Ivl::from(dw))
                / (Ivl::from(wmin) * Ivl::from(wmin) * radius))
                .hi,
        );
    }
    if !speed.is_finite() {
        return Err(bad("cylinder angular speed is not finite"));
    }
    Ok(speed)
}

pub(super) fn certify(
    k: &Kernel,
    wall: &Wall<'_>,
    cylinder: &Cylinder<'_>,
    x: f64,
    first: &Curve,
    second: &Curve,
    tolerance: f64,
) -> Result<f64> {
    if cylinder
        .cone
        .rev()
        .polar_curve()
        .is_none_or(|polar| polar.base().is_out_of_bounds_enabled())
    {
        return Err(bad("paired cylinder needs a bounded polar chart"));
    }
    let spline = second
        .b_spline_base()
        .ok_or_else(|| bad("missing cylinder pcurve"))?;
    let (knots, multiplicities) = certificate::linear_knots(spline)?;
    let controls = spline.control_points()?;
    if knots.len() < 2
        || knots.len() > MAX_SPANS + 1
        || controls.len() != knots.len()
        || multiplicities
            .iter()
            .enumerate()
            .any(|(i, &m)| m != if i == 0 || i + 1 == knots.len() { 2 } else { 1 })
    {
        return Err(bad("paired cylinder certificate needs joined linear spans"));
    }
    let rational = spline.is_rational()?;
    let stored = controls
        .windows(2)
        .map(|pair| {
            let [a, b] = pair else {
                return Err(bad("missing cylinder pcurve span"));
            };
            Ok([a, b].map(|p| {
                [
                    Ivl::from(p.x),
                    Ivl::from(p.y),
                    Ivl::from(if rational { p.w } else { 1.0 }),
                ]
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let pcurve_error = certificate::mapped_error(second, &stored, ([0.0, 0.0], [1.0, 1.0], false))?;
    let speed = angular_speed(cylinder)?;
    let height = HeightMap::new(k, cylinder.cone)?.exact_coefficients();
    let frame = cylinder.cone.canonical();
    let height_speed = (Ivl::from(height[1].mag())
        * magnitude([
            Ivl::from(frame.origin.z_axis().x),
            Ivl::from(frame.origin.z_axis().y),
            Ivl::from(frame.origin.z_axis().z),
        ]))
    .hi;
    let [eu, ev] = if frame.swap_uv {
        [pcurve_error[1], pcurve_error[0]]
    } else {
        pcurve_error
    };
    let cylinder_runtime = cylinder
        .cone
        .cylinder_runtime_allowance(k)?
        .ok_or_else(|| bad("cylinder runtime chart is not certified"))?;
    let (Some(&begin), Some(&end)) = (knots.first(), knots.last()) else {
        return Err(bad("missing paired curve interval"));
    };
    let w = Ivl::from(wall.source[0])
        + (Ivl::from(wall.source[1]) - Ivl::from(wall.source[0])) * Ivl::from(x);
    let ideal = [
        certificate::point(w, Ivl::from(begin), Ivl::from(1.0)),
        certificate::point(w, Ivl::from(end), Ivl::from(1.0)),
    ];
    let fixed = (Ivl::from(certificate::wall_error(wall, first, &[ideal])?)
        + Ivl::from(cylinder_runtime)
        + Ivl::from(speed) * Ivl::from(eu)
        + Ivl::from(height_speed) * Ivl::from(ev))
    .hi;
    if !fixed.is_finite() || fixed >= tolerance {
        return Err(bad("paired cylinder runtime terms exceed tolerance"));
    }
    let source = wall.section.point_enclosure(Ivl::from(x))?;
    let turn_speed = wall.section.speed_upper_bounds(Ivl::from(x))?[1];
    let mut global_rate = 0.0_f64;
    for pair in wall.definition.stations().windows(2) {
        let [a, b] = *pair else { continue };
        let width = Ivl::from(b) - Ivl::from(a);
        global_rate = global_rate
            .max((Ivl::from(4.0) * certificate::tan(width / Ivl::from(4.0))? / width).hi);
    }
    let mut bound = 0.0_f64;
    let mut remaining = MAX_SPANS;
    for (knots, stored) in knots.windows(2).zip(stored) {
        let [a, b] = *knots else { continue };
        let mut pending = vec![[a, b]];
        while let Some([lo, hi]) = pending.pop() {
            k.check_cancelled("offset::procedural_screw::cylinder::paired")?;
            remaining = remaining
                .checked_sub(1)
                .ok_or_else(|| bad("paired cylinder Taylor budget exceeded"))?;
            let mid = lo + (hi - lo) * 0.5;
            let (sa, sb) = super::super::span(wall.definition, mid)?;
            let center = Jet::of_terms(Ivl::from(mid), Ivl::from(1.0));
            let whole = Jet::of_terms(Ivl::new(lo, hi), Ivl::from(1.0));
            let uv_center = pcurve(stored, [a, b], center);
            let uv_whole = pcurve(stored, [a, b], whole);
            let [cu, cv] = uv_center;
            let angular = if frame.swap_uv { cv.at(0) } else { cu.at(0) };
            let arc = cylinder
                .arcs
                .iter()
                .find(|arc| angular.mid() >= arc.parameter[0] && angular.mid() <= arc.parameter[1])
                .ok_or_else(|| bad("paired cylinder lies outside its polar spans"))?;
            let dc = sub(
                screw_point(wall, source, [sa, sb], center)?,
                cylinder_point(cylinder, arc, height, uv_center),
            );
            let [dx, dy, dz] = coefficient(dc, 0).map(|v| Ivl::from(v.mig()));
            let minimum = (dx * dx + dy * dy + dz * dz).sqrt().lo;
            if minimum > (Ivl::from(tolerance) - Ivl::from(fixed)).lo {
                return Err(bad(
                    "stored cylinder pcurves exceed tolerance at an enclosed witness",
                ));
            }
            let di = sub(
                screw_point(wall, source, [sa, sb], whole)?,
                cylinder_point(cylinder, arc, height, uv_whole),
            );
            let h = Ivl::from(
                (Ivl::from(mid) - Ivl::from(lo))
                    .hi
                    .max((Ivl::from(hi) - Ivl::from(mid)).hi),
            );
            let mut power = Ivl::from(1.0);
            let mut error = Ivl::from(0.0);
            for n in 0..4 {
                error = error + magnitude(coefficient(dc, n)) * power;
                power = power * h;
            }
            error = error + magnitude(coefficient(di, 4)) * power;
            // The exact definitions are continuous across their stored joins.
            // If rounding sends a chart parameter beyond this cell's chart,
            // Lipschitz continuation bounds both branches from their common end.
            let [u, v] = if frame.swap_uv {
                [uv_whole[1], uv_whole[0]]
            } else {
                uv_whole
            };
            let ext = polar(arc, Jet::of_terms(u.at(0), Ivl::from(1.0)));
            let ext_speed = (Ivl::from(frame.radius)
                * magnitude(coefficient(
                    add(
                        scale(vector(frame.origin.x_axis()), ext[0]),
                        scale(vector(frame.origin.y_axis()), ext[1]),
                    ),
                    1,
                )))
            .hi;
            error = error
                + (Ivl::from(speed) + Ivl::from(ext_speed))
                    * overrun(u.at(0), arc.parameter[0], arc.parameter[1]);
            let local_rate = (Ivl::from(4.0)
                * certificate::tan((Ivl::from(sb) - Ivl::from(sa)) / Ivl::from(4.0))?
                / (Ivl::from(sb) - Ivl::from(sa)))
            .hi;
            error = error
                + Ivl::from(turn_speed)
                    * (Ivl::from(local_rate) + Ivl::from(global_rate))
                    * overrun(Ivl::new(lo, hi), sa, sb);
            if !cylinder.cone.rev().is_out_of_bounds_enabled() {
                let domain = cylinder.cone.rev().step_uv_domain().v_interval();
                let z = height[0] + height[1] * v.at(0);
                error = error
                    + magnitude(coefficient(vector(frame.origin.z_axis()), 0))
                        * overrun(z, domain.min(), domain.max());
            }
            let total = (error + Ivl::from(fixed)).hi;
            if total.is_finite() && total <= tolerance {
                bound = bound.max(total);
                continue;
            }
            if !(lo < mid && mid < hi) {
                return Err(bad("paired cylinder Taylor cell cannot subdivide"));
            }
            pending.push([mid, hi]);
            pending.push([lo, mid]);
        }
    }
    Ok(bound)
}
