//! Exact planar fit-spline conversion and certified line-image extents.
//!
//! Upstream: none; SolidCraft's natural and cyclic interpolation and projection construction.
//! References: Piegl & Tiller, *The NURBS Book*, §§1.4, 5.2 (convex hulls and knot insertion).
//! Deviations: compact chord-knot controls are accepted only after a whole-Bézier coefficient
//! certificate; ill-conditioned joins retain the defining Bézier spans. Extent subdivision has
//! an explicit depth bound of 48 and a caller work budget; no input-dependent recursion.

use crate::math::Vec2;
use crate::prism::read::PlanarSpline;
use crate::{Result, SolidError};

const OP: &str = "curve::fit_projection";
const MAX_DEGREE: usize = 9;
const MAX_CONTROLS: usize = 16_384;
const MAX_WEIGHT_RATIO: f64 = 1e12;

fn bad<T>(reason: &'static str) -> Result<T> {
    Err(SolidError::invalid_input(OP, reason))
}
fn at<T: Copy>(v: &[T], i: usize) -> Result<T> {
    v.get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP, "inconsistent spline arrays"))
}
fn put<T>(v: &mut [T], i: usize, value: T) -> Result<()> {
    *v.get_mut(i)
        .ok_or_else(|| SolidError::invalid_input(OP, "inconsistent spline arrays"))? = value;
    Ok(())
}
fn finite(p: &Vec2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn bounded(p: &Vec2) -> bool {
    finite(p) && p.x.abs() < 1e9 && p.y.abs() < 1e9
}
// The legacy kernel Vec2 division intentionally suppresses small divisors. Interpolation
// needs component division, and hypot preserves the defining app curve at extreme scales.
fn divided(p: Vec2, d: f64) -> Vec2 {
    Vec2::new(p.x / d, p.y / d)
}
fn distance(a: Vec2, b: Vec2) -> f64 {
    let d = a - b;
    d.x.hypot(d.y)
}
fn lerp(a: Vec2, b: Vec2, t: f64) -> Vec2 {
    a + (b - a) * t
}

/// Check a clamped polynomial or positive-weight rational spline's explicit metadata.
/// The degree is at most nine and no more than `limit` controls are accepted.
pub fn check_control(
    n: usize,
    degree: usize,
    knots: &[f64],
    weights: &[f64],
    limit: usize,
) -> Result<()> {
    if !(1..=MAX_DEGREE).contains(&degree) || n <= degree || n > limit || limit > MAX_CONTROLS {
        return bad(
            "a clamped spline needs more controls than its degree and a bounded control count",
        );
    }
    if knots.len() != n + degree + 1
        || knots.iter().any(|v| !v.is_finite())
        || knots.windows(2).any(|w| matches!(w,[a,b] if b<a))
    {
        return bad("invalid clamped knot count or order");
    }
    let (lo, hi) = (at(knots, degree)?, at(knots, n)?);
    let span = hi - lo;
    if span <= 0.0
        || !span.is_finite()
        || knots.iter().take(degree + 1).any(|k| *k != lo)
        || knots.iter().skip(n).any(|k| *k != hi)
    {
        return bad("the spline knots must clamp a finite nonempty domain");
    }
    let mut run = 0;
    for (i, x) in knots.iter().enumerate().take(n).skip(degree + 1) {
        run = if knots.get(i - 1) == Some(x) {
            run + 1
        } else {
            1
        };
        if *x == lo || *x == hi || run > degree {
            return bad("an interior knot repeats too often");
        }
    }
    if !weights.is_empty() {
        if weights.len() != n || weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
            return bad("one finite positive weight is needed per control");
        }
        let (min, max) = weights
            .iter()
            .fold((f64::INFINITY, 0.0_f64), |(a, b), w| (a.min(*w), b.max(*w)));
        if max / min > MAX_WEIGHT_RATIO {
            return bad("the control weights differ too much");
        }
    }
    Ok(())
}

/// The effective degree and clamped uniform knots of an implicit control spline.
/// A requested degree above the number of controls uses the largest available degree.
pub fn uniform_knots(n: usize, degree: usize) -> Result<(usize, Vec<f64>)> {
    if !(2..=MAX_CONTROLS).contains(&n) || !(1..=MAX_DEGREE).contains(&degree) {
        return bad("invalid control count or requested degree");
    }
    let d = degree.min(n - 1);
    let spans = n - d;
    let mut knots = vec![0.0; d + 1];
    knots.extend((1..spans).map(|i| i as f64 / spans as f64));
    knots.extend(std::iter::repeat_n(1.0, d + 1));
    Ok((d, knots))
}

/// Natural cubic interpolation's Hermite spans, with chord-length parameters.
/// This preserves the original interpolation's operation order and natural end conditions.
pub fn natural_spans(p: &[Vec2]) -> Result<Vec<(Vec2, Vec2, Vec2, Vec2)>> {
    let n = p.len();
    if n < 2 {
        return Ok(Vec::new());
    }
    if n > MAX_CONTROLS || p.iter().any(|p| !finite(p)) {
        return bad("invalid fit points");
    }
    let h: Vec<f64> = p
        .windows(2)
        .map(|w| match w {
            [a, b] => distance(*a, *b).max(1e-9),
            _ => 0.0,
        })
        .collect();
    if h.iter().any(|v| !v.is_finite()) {
        return bad("fit chord lengths are not finite");
    }
    if n == 2 {
        let a = at(p, 0)?;
        let b = at(p, 1)?;
        let d = b - a;
        return Ok(vec![(a, b, d, d)]);
    }
    let m = n - 2;
    let mut diag: Vec<f64> = (0..m).map(|_| 0.0).collect();
    let mut up = diag.clone();
    let mut low = diag.clone();
    let mut rhs: Vec<Vec2> = (0..m).map(|_| Vec2::ZERO).collect();
    for i in 0..m {
        let (h0, h1) = (at(&h, i)?, at(&h, i + 1)?);
        put(&mut diag, i, 2.0 * (h0 + h1))?;
        put(&mut low, i, h0)?;
        put(&mut up, i, h1)?;
        put(
            &mut rhs,
            i,
            (divided(at(p, i + 2)? - at(p, i + 1)?, h1) - divided(at(p, i + 1)? - at(p, i)?, h0))
                * 6.0,
        )?;
    }
    for i in 1..m {
        let w = at(&low, i)? / at(&diag, i - 1)?;
        let d = at(&diag, i)? - w * at(&up, i - 1)?;
        put(&mut diag, i, d)?;
        let r = at(&rhs, i)? - at(&rhs, i - 1)? * w;
        put(&mut rhs, i, r)?;
    }
    let mut sol: Vec<Vec2> = (0..m).map(|_| Vec2::ZERO).collect();
    for i in (0..m).rev() {
        let next = if i + 1 < m {
            at(&sol, i + 1)? * at(&up, i)?
        } else {
            Vec2::ZERO
        };
        put(&mut sol, i, divided(at(&rhs, i)? - next, at(&diag, i)?))?;
    }
    let mm = |i: usize| {
        if i == 0 || i == n - 1 {
            Ok(Vec2::ZERO)
        } else {
            at(&sol, i - 1)
        }
    };
    (0..n - 1)
        .map(|i| {
            let hi = at(&h, i)?;
            let a = at(p, i)?;
            let b = at(p, i + 1)?;
            let slope = divided(b - a, hi);
            let d0 = slope - (mm(i)? * 2.0 + mm(i + 1)?) * (hi / 6.0);
            let d1 = slope + (mm(i)? + mm(i + 1)? * 2.0) * (hi / 6.0);
            Ok((a, b, d0 * hi, d1 * hi))
        })
        .collect()
}

/// Check cyclic interpolation's exact closing sample and regular bounded chords.
pub fn check_periodic(points: &[Vec2]) -> Result<()> {
    if points.len() < 4
        || points.len() > MAX_CONTROLS
        || points.first() != points.last()
        || points.iter().any(|p| !bounded(p))
        || points.windows(2).any(|p| match p {
            [a, b] => {
                let h = distance(*a, *b);
                !h.is_finite() || h <= 1e-9
            }
            _ => true,
        })
    {
        return bad(
            "periodic fit needs three distinct cyclic samples, exact closure, and regular bounded chords",
        );
    }
    let mut distinct = Vec::new();
    for p in points {
        if !distinct.contains(p) {
            distinct.push(*p);
        }
        if distinct.len() == 3 {
            return Ok(());
        }
    }
    bad("periodic fit needs three distinct samples")
}

/// Interpolate a closed C2 curve using the source chord-length parameter.
fn cyclic_beziers(points: &[Vec2]) -> Result<Vec<[Vec2; 4]>> {
    check_periodic(points)?;
    let n = points.len() - 1;
    let h: Vec<f64> = points
        .windows(2)
        .map(|p| match p {
            [a, b] => distance(*a, *b),
            _ => 0.0,
        })
        .collect();
    let previous = |i: usize| if i == 0 { n - 1 } else { i - 1 };
    let mut diagonal = Vec::new();
    let mut rhs = Vec::new();
    for i in 0..n {
        let j = previous(i);
        let a = at(&h, j)?;
        let b = at(&h, i)?;
        diagonal.push(2.0 * (a + b));
        rhs.push(
            (divided(at(points, i + 1)? - at(points, i)?, b)
                - divided(at(points, i)? - at(points, j)?, a))
                * 6.0,
        );
    }
    let corner = at(&h, n - 1)?;
    let gamma = -at(&diagonal, 0)?;
    let d0 = at(&diagonal, 0)? - gamma;
    put(&mut diagonal, 0, d0)?;
    let dn = at(&diagonal, n - 1)? - corner * corner / gamma;
    put(&mut diagonal, n - 1, dn)?;
    let solve = |mut rhs: Vec<Vec2>| -> Result<Vec<Vec2>> {
        let mut diagonal = diagonal.clone();
        for i in 1..n {
            let j = previous(i - 1);
            let d = at(&diagonal, i - 1)?;
            if !d.is_finite() || d.abs() < 1e-12 * (at(&h, i - 1)? + at(&h, j)?) {
                return bad("cyclic interpolation has an ill-conditioned pivot");
            }
            let factor = at(&h, i - 1)? / d;
            let d = at(&diagonal, i)? - factor * at(&h, i - 1)?;
            put(&mut diagonal, i, d)?;
            let r = at(&rhs, i)? - at(&rhs, i - 1)? * factor;
            put(&mut rhs, i, r)?;
        }
        for i in (0..n).rev() {
            let d = at(&diagonal, i)?;
            if !d.is_finite() || d.abs() < 1e-12 * (at(&h, i)? + at(&h, previous(i))?) {
                return bad("cyclic interpolation has an ill-conditioned pivot");
            }
            let next = rhs.get(i + 1).copied().unwrap_or_default() * at(&h, i)?;
            let r = divided(at(&rhs, i)? - next, d);
            if !finite(&r) {
                return bad("cyclic interpolation overflow");
            }
            put(&mut rhs, i, r)?;
        }
        Ok(rhs)
    };
    let mut second = solve(rhs)?;
    let mut correction_rhs: Vec<Vec2> = (0..n).map(|_| Vec2::ZERO).collect();
    put(&mut correction_rhs, 0, Vec2::new(gamma, 0.0))?;
    put(&mut correction_rhs, n - 1, Vec2::new(corner, 0.0))?;
    let correction = solve(correction_rhs)?;
    let denominator = 1.0 + at(&correction, 0)?.x + corner * at(&correction, n - 1)?.x / gamma;
    if !denominator.is_finite() || denominator.abs() <= 1e-12 {
        return bad("cyclic interpolation correction is singular");
    }
    let factor = divided(
        at(&second, 0)? + at(&second, n - 1)? * (corner / gamma),
        denominator,
    );
    for (value, correction) in second.iter_mut().zip(&correction) {
        *value -= factor * correction.x;
    }
    (0..n)
        .map(|i| {
            let next = if i + 1 == n { 0 } else { i + 1 };
            let a = at(points, i)?;
            let b = at(points, i + 1)?;
            let chord = b - a;
            let hi = at(&h, i)?;
            let t0 = chord - (at(&second, i)? * 2.0 + at(&second, next)?) * (hi * hi / 6.0);
            let t1 = chord + (at(&second, i)? + at(&second, next)? * 2.0) * (hi * hi / 6.0);
            let cubic = [a, a + divided(t0, 3.0), b - divided(t1, 3.0), b];
            if cubic.iter().any(|p| !bounded(p)) {
                return bad("cyclic interpolation controls are out of range");
            }
            Ok(cubic)
        })
        .collect()
}

/// The exact cubic Bézier pieces of natural or cyclic chord-length interpolation.
pub fn fit_beziers(points: &[Vec2], periodic: bool) -> Result<Vec<[Vec2; 4]>> {
    if periodic {
        cyclic_beziers(points)
    } else {
        Ok(natural_spans(points)?
            .into_iter()
            .map(|(a, b, t0, t1)| [a, a + divided(t0, 3.0), b - divided(t1, 3.0), b])
            .collect())
    }
}

/// Exact control splines of a fit curve, split to `limit` controls per entity.
/// Compact open curves use cumulative chord parameters; the geometric locus is unchanged.
pub fn fit_splines(points: &[Vec2], periodic: bool, limit: usize) -> Result<Vec<PlanarSpline>> {
    if !(2..=MAX_CONTROLS).contains(&points.len())
        || points.iter().any(|p| !finite(p))
        || !(4..=MAX_CONTROLS).contains(&limit)
    {
        return bad("fit points and the control budget must be finite and bounded");
    }
    let h: Vec<f64> = points
        .windows(2)
        .map(|p| match p {
            [a, b] => distance(*a, *b).max(1e-9),
            _ => 0.0,
        })
        .collect();
    if h.iter().any(|h| !h.is_finite()) {
        return bad("fit chord lengths are not finite");
    }
    let spans = fit_beziers(points, periodic)?;
    if spans.len() != h.len() || spans.iter().flatten().any(|p| !finite(p)) {
        return bad("fit spans cannot be represented accurately");
    }
    let mut out = Vec::new();
    for (spans, h) in spans.chunks(limit - 3).zip(h.chunks(limit - 3)) {
        if let Some(compact) = compact_fit(spans, h)? {
            out.push(compact);
            continue;
        }
        for spans in spans.chunks((limit - 1) / 3) {
            let mut controls = Vec::new();
            let mut knots = vec![0.0; 4];
            for (i, [a, b, c, d]) in spans.iter().enumerate() {
                if i == 0 {
                    controls.push(*a);
                }
                controls.extend([*b, *c, *d]);
                knots.extend(std::iter::repeat_n(
                    (i + 1) as f64,
                    if i + 1 == spans.len() { 4 } else { 3 },
                ));
            }
            check_control(controls.len(), 3, &knots, &[], limit)?;
            out.push(PlanarSpline {
                degree: 3,
                knots,
                points: controls,
                weights: Vec::new(),
            });
        }
    }
    Ok(out)
}

/// Compact C2 spans on chord knots only when their reconstructed coefficients certify the roundoff bound.
fn compact_fit(spans: &[[Vec2; 4]], h: &[f64]) -> Result<Option<PlanarSpline>> {
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
        return Ok(None);
    };
    let [first_a, first_b, _, _] = *first;
    let [_, _, last_c, last_d] = *last;
    let mut knots = vec![0.0; 4];
    let mut widths = Vec::new();
    let mut t = 0.0;
    for (i, length) in h.iter().enumerate() {
        let next = t + length;
        if next <= t || !next.is_finite() {
            return Ok(None);
        }
        widths.push(next - t);
        t = next;
        knots.extend(std::iter::repeat_n(t, if i + 1 == h.len() { 4 } else { 1 }));
    }
    let mut controls = vec![first_a, first_b];
    for (pair, h) in spans.windows(2).zip(widths.windows(2)) {
        let ([left, right], [h0, h1]) = (pair, h) else {
            return bad("inconsistent compact fit spans");
        };
        let ([_, lb, lc, _], [_, rb, rc, _]) = (*left, *right);
        // Extrapolate the cubic blossom from the longer span: its coefficient is at most one.
        let (b, c, ratio) = if h1 >= h0 {
            (rb, rc, h0 / h1)
        } else {
            (lc, lb, h1 / h0)
        };
        controls.push(b + (b - c) * ratio);
    }
    controls.extend([last_c, last_d]);
    if check_control(controls.len(), 3, &knots, &[], MAX_CONTROLS).is_err() {
        return Ok(None);
    }
    // Bézier coefficient differences bound the entire curve by the convex-hull property.
    let inner: Vec<[Vec2; 2]> = widths
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let a = i
                .checked_sub(1)
                .and_then(|j| widths.get(j))
                .copied()
                .unwrap_or(0.0);
            let c = widths.get(i + 1).copied().unwrap_or(0.0);
            let sum = a + b + c;
            Ok([
                lerp(at(&controls, i + 1)?, at(&controls, i + 2)?, a / sum),
                lerp(at(&controls, i + 2)?, at(&controls, i + 1)?, c / sum),
            ])
        })
        .collect::<Result<_>>()?;
    let scale = spans
        .iter()
        .flatten()
        .fold(1.0_f64, |s, p| s.max(p.x.abs()).max(p.y.abs()));
    let tolerance = scale * f64::EPSILON * 128.0;
    for (i, expected) in spans.iter().enumerate() {
        let [b, c] = at(&inner, i)?;
        let a = if i == 0 {
            first_a
        } else {
            let [_, previous_c] = at(&inner, i - 1)?;
            lerp(
                previous_c,
                b,
                at(&widths, i - 1)? / (at(&widths, i - 1)? + at(&widths, i)?),
            )
        };
        let d = if i + 1 == spans.len() {
            last_d
        } else {
            let [next_b, _] = at(&inner, i + 1)?;
            lerp(
                c,
                next_b,
                at(&widths, i)? / (at(&widths, i)? + at(&widths, i + 1)?),
            )
        };
        if [a, b, c, d]
            .iter()
            .zip(expected)
            .any(|(a, b)| distance(*a, *b) > tolerance || !finite(a))
        {
            return Ok(None);
        }
    }
    Ok(Some(PlanarSpline {
        degree: 3,
        knots,
        points: controls,
        weights: Vec::new(),
    }))
}

/// Homogeneous controls with a shared weight normalization, avoiding meaningless weight scale
/// overflow while preserving the represented curve. The caller validates metadata first.
pub fn homogeneous_controls(points: &[Vec2], weights: &[f64]) -> Result<Vec<[f64; 4]>> {
    if (!weights.is_empty() && weights.len() != points.len())
        || points.iter().any(|p| !finite(p))
        || weights.iter().any(|w| !w.is_finite() || *w <= 0.0)
    {
        return bad("invalid points or weights");
    }
    let max = weights.iter().copied().reduce(f64::max).unwrap_or(1.0);
    Ok(points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let w = weights.get(i).copied().unwrap_or(1.0) / max;
            [q.x * w, q.y * w, 0.0, w]
        })
        .collect())
}

/// A point or the complete line interval when every positive-weight control lies on one line
/// within the affine-map roundoff budget. `None` retains a genuinely planar curve.
pub fn linear_image(
    p: &[Vec2],
    degree: usize,
    knots: &[f64],
    weights: &[f64],
    roundoff: f64,
) -> Result<Option<[Vec2; 2]>> {
    check_control(p.len(), degree, knots, weights, MAX_CONTROLS)?;
    if !roundoff.is_finite() || roundoff < 0.0 || p.iter().any(|p| !finite(p)) {
        return bad("invalid projected controls or roundoff budget");
    }
    let origin = at(p, 0)?;
    let far = p
        .iter()
        .copied()
        .max_by(|a, b| distance(*a, origin).total_cmp(&distance(*b, origin)))
        .unwrap_or(origin);
    if distance(far, origin) <= roundoff {
        return Ok(Some([origin; 2]));
    }
    let length = distance(far, origin);
    if !length.is_finite() || length <= 0.0 {
        return bad("invalid projected direction");
    }
    let axis = divided(far - origin, length);
    if p.iter().any(|p| {
        let d = *p - origin;
        (axis.x * d.y - axis.y * d.x).abs() > roundoff
    }) {
        return Ok(None);
    }
    let scalar: Vec<_> = p
        .iter()
        .map(|p| Vec2::new((*p - origin).dot(axis), 0.0))
        .collect();
    let scale = scalar.iter().fold(1.0_f64, |s, p| s.max(p.x.abs()));
    let accuracy = (scale * f64::EPSILON * 8192.0).max(1e-10);
    let controls = homogeneous_controls(&scalar, weights)?;
    let spans = crate::curve::spline_projection::bezier_spans(degree, knots, &controls)?;
    let mut work = (64 * spans.len() + 4096) * (degree + 1).pow(2);
    let mut range = [f64::INFINITY, f64::NEG_INFINITY];
    for span in spans {
        let r = bezier_scalar_range(&span, accuracy, &mut work)?;
        let [lo, hi] = r;
        let [oldlo, oldhi] = range;
        range = [oldlo.min(lo), oldhi.max(hi)];
    }
    let [lo, hi] = range;
    // One additional accuracy allowance covers scalar mapping and positive-weight extraction.
    Ok(Some([
        origin + axis * (lo - accuracy),
        origin + axis * (hi + accuracy),
    ]))
}

// Adjacent binary64 values, including signed zero, subnormals and infinities.
// These bit operations retain the kernel's Rust 1.85 minimum supported version.
fn next_up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        x
    } else if x == 0.0 {
        f64::from_bits(1)
    } else if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

fn next_down(x: f64) -> f64 {
    -next_up(-x)
}

/// Directed outer bounds for x/w on a positive-weight rational Bézier span. Each bound is
/// within `tolerance` of an attained value. Exhausting depth or work returns a refusal.
pub fn bezier_scalar_range(
    ctrl: &[[f64; 4]],
    tolerance: f64,
    work: &mut usize,
) -> Result<[f64; 2]> {
    type Bound = [[f64; 2]; 2];
    if !(2..=MAX_DEGREE + 1).contains(&ctrl.len())
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || ctrl.iter().any(|p| {
            let [x, y, z, w] = *p;
            !x.is_finite()
                || !y.is_finite()
                || !z.is_finite()
                || !w.is_finite()
                || w <= 0.0
                || !(x / w).is_finite()
        })
    {
        return bad("scalar range needs finite controls, positive weights and tolerance");
    }
    let value = |p: &Bound| -> Result<[f64; 2]> {
        let [[x0, x1], [w0, w1]] = *p;
        if w0 <= 0.0 {
            return bad("scalar range weight lost numerical precision");
        }
        let values = [x0 / w0, x0 / w1, x1 / w0, x1 / w1];
        let lo = next_down(values.iter().copied().fold(f64::INFINITY, f64::min));
        let hi = next_up(values.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        if !(lo.is_finite() && hi.is_finite()) {
            return bad("scalar range overflow");
        }
        Ok([lo, hi])
    };
    let initial: Vec<Bound> = ctrl
        .iter()
        .map(|p| {
            let [x, _, _, w] = *p;
            [[x, x], [w, w]]
        })
        .collect();
    let mut stack = vec![(initial, 0usize)];
    let mut inner = [f64::INFINITY, f64::NEG_INFINITY];
    let mut outer = inner;
    while let Some((points, depth)) = stack.pop() {
        let ranges = points.iter().map(&value).collect::<Result<Vec<_>>>()?;
        let [f0, f1] = ranges
            .first()
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "empty scalar span"))?;
        let [l0, l1] = ranges
            .last()
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "empty scalar span"))?;
        let [i0, i1] = inner;
        inner = [i0.min(f1).min(l1), i1.max(f0).max(l0)];
        let hull = ranges
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[r0, r1], [q0, q1]| {
                [r0.min(*q0), r1.max(*q1)]
            });
        let [h0, h1] = hull;
        let [i0, i1] = inner;
        if h0 >= i0 - tolerance && h1 <= i1 + tolerance {
            let [o0, o1] = outer;
            outer = [o0.min(h0), o1.max(h1)];
            continue;
        }
        let cost = points.len() * points.len();
        if depth >= 48 || *work < cost {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason:
                    "could not certify the projected spline extent within the precision/work limit",
            });
        }
        *work -= cost;
        let mut level = points;
        let mut left = Vec::new();
        let mut right = Vec::new();
        while let (Some(a), Some(b)) = (level.first(), level.last()) {
            left.push(*a);
            right.push(*b);
            level = level
                .windows(2)
                .map(|p| match p {
                    [a, b] => {
                        let [[a00, a01], [a10, a11]] = *a;
                        let [[b00, b01], [b10, b11]] = *b;
                        let midpoint = |lo0: f64, hi0: f64, lo1: f64, hi1: f64| {
                            [
                                next_down(next_down(lo0 * 0.5) + next_down(lo1 * 0.5)),
                                next_up(next_up(hi0 * 0.5) + next_up(hi1 * 0.5)),
                            ]
                        };
                        [midpoint(a00, a01, b00, b01), midpoint(a10, a11, b10, b11)]
                    }
                    _ => [[0.0; 2]; 2],
                })
                .collect();
        }
        right.reverse();
        stack.push((right, depth + 1));
        stack.push((left, depth + 1));
    }
    Ok(outer)
}

/// Reverse explicit spline metadata by reflecting the knot domain about zero.
/// Negation preserves narrow intervals beside large finite endpoints exactly.
pub fn reversed_metadata(knots: &[f64], weights: &[f64]) -> (Vec<f64>, Vec<f64>) {
    (
        knots.iter().rev().map(|k| -k).collect(),
        weights.iter().rev().copied().collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directed_rounding_preserves_binary64_boundary_values() {
        for (x, expected) in [
            (0.0, f64::from_bits(1)),
            (-0.0, f64::from_bits(1)),
            (-f64::from_bits(1), -0.0),
            (
                f64::MIN_POSITIVE,
                f64::from_bits(f64::MIN_POSITIVE.to_bits() + 1),
            ),
            (1.0, f64::from_bits(1.0f64.to_bits() + 1)),
            (-1.0, f64::from_bits((-1.0f64).to_bits() - 1)),
            (f64::MAX, f64::INFINITY),
            (f64::NEG_INFINITY, -f64::MAX),
            (f64::INFINITY, f64::INFINITY),
        ] {
            assert_eq!(next_up(x).to_bits(), expected.to_bits());
            assert_eq!(next_down(-x).to_bits(), (-expected).to_bits());
        }
        assert!(next_up(f64::NAN).is_nan());
        assert!(next_down(f64::NAN).is_nan());
    }

    #[test]
    fn scalar_range_encloses_interior_and_rational_extrema() {
        let ctrl: Vec<_> = [0., 90., 17., 0.]
            .into_iter()
            .map(|x| [x, 0., 0., 1.])
            .collect();
        let [lo, hi] = bezier_scalar_range(&ctrl, 1e-10, &mut 100_000).unwrap();
        let exact = 44.05262897859362;
        assert!(lo <= 0. && lo > -1e-10);
        assert!(hi >= exact && hi - exact < 1e-10, "{hi}");
        for ratio in [1., 1e6, 1e12] {
            let ctrl = [[0., 0., 0., 1.], [ratio, 0., 0., ratio], [0., 0., 0., 1.]];
            let exact = ratio / (ratio + 1.);
            let r = bezier_scalar_range(&ctrl, 1e-10, &mut 100_000).unwrap();
            assert!(r[0] <= 0. && r[0] > -1e-10);
            assert!(r[1] >= exact && r[1] - exact < 1e-10, "{ratio}: {r:?}");
        }
        assert!(bezier_scalar_range(&ctrl, 1e-10, &mut 0).is_err());
        assert!(bezier_scalar_range(&[[0.; 4]; 4], 1e-10, &mut 100).is_err());
    }

    #[test]
    fn compact_fit_reconstructs_whole_bezier_coefficients() {
        for periodic in [false, true] {
            for count in [4usize, 8, 168, 498, 499, 500] {
                let mut points: Vec<_> = (0..count)
                    .map(|i| {
                        if periodic {
                            let a = std::f64::consts::TAU * i as f64 / (count - 1) as f64;
                            Vec2::new(10. * a.cos(), 7. * a.sin())
                        } else {
                            Vec2::new(
                                0.01 * (i * i) as f64 + i as f64 * 0.1,
                                (i as f64 * 0.37).sin() * 3.,
                            )
                        }
                    })
                    .collect();
                if periodic {
                    points[count - 1] = points[0];
                }
                let expected = fit_beziers(&points, periodic).unwrap();
                let curves = fit_splines(&points, periodic, 500).unwrap();
                if count <= 498 {
                    assert_eq!(curves.len(), 1);
                    assert_eq!(curves[0].points.len(), count + 2);
                }
                let mut actual = Vec::new();
                for curve in curves {
                    assert!(curve.points.len() <= 500);
                    let h = homogeneous_controls(&curve.points, &curve.weights).unwrap();
                    actual.extend(
                        crate::curve::spline_projection::bezier_spans(3, &curve.knots, &h).unwrap(),
                    );
                }
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.into_iter().zip(expected) {
                    for ([x, y, _, w], expected) in actual.into_iter().zip(expected) {
                        assert!(
                            Vec2::new(x / w, y / w).distance(expected) < 1e-8,
                            "count={count}, periodic={periodic}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cyclic_fit_is_c2_on_unequal_chords() {
        let p = [
            Vec2::new(0., 0.),
            Vec2::new(4., 0.),
            Vec2::new(3., 2.),
            Vec2::new(-1., 1.),
            Vec2::new(0., 0.),
        ];
        let curves = fit_beziers(&p, true).unwrap();
        for i in 0..curves.len() {
            let j = (i + 1) % curves.len();
            let (a, b) = (curves[i], curves[j]);
            let (ha, hb) = (p[i].distance(p[i + 1]), p[j].distance(p[j + 1]));
            assert_eq!(a[3], b[0]);
            assert!(((a[3] - a[2]) * (3. / ha) - (b[1] - b[0]) * (3. / hb)).length() < 1e-12);
            assert!(
                ((a[3] - a[2] * 2. + a[1]) * (6. / (ha * ha))
                    - (b[2] - b[1] * 2. + b[0]) * (6. / (hb * hb)))
                    .length()
                    < 1e-12
            );
        }
    }

    #[test]
    fn line_extent_ignores_shared_weight_scale_and_refuses_planar_controls() {
        let controls = [
            Vec2::new(0., 0.),
            Vec2::new(90., 0.),
            Vec2::new(17., 0.),
            Vec2::new(0., 0.),
        ];
        let knots = [0., 0., 0., 0., 1., 1., 1., 1.];
        for scale in [1e-320, 1., 1e300] {
            let [a, b] = linear_image(&controls, 3, &knots, &[scale; 4], 1e-12)
                .unwrap()
                .unwrap();
            assert!(a.x <= 0. && a.x > -1e-8);
            assert!(b.x >= 44.05262897859362 && b.x - 44.05262897859362 < 1e-8);
        }
        let p = [
            Vec2::new(0., 0.),
            Vec2::new(90., 1.),
            Vec2::new(17., 0.),
            Vec2::new(0., 0.),
        ];
        assert!(linear_image(&p, 3, &knots, &[], 1e-12).unwrap().is_none());
        let p = [Vec2::new(5., 0.); 4];
        assert_eq!(
            linear_image(&p, 3, &knots, &[], 1e-12).unwrap(),
            Some([p[0]; 2])
        );
    }

    #[test]
    fn control_metadata_rejects_hostile_values_and_limits_effective_degree() {
        assert_eq!(uniform_knots(2, 9).unwrap(), (1, vec![0., 0., 1., 1.]));
        let knots = [0., 0., 0., 1., 1., 1.];
        assert!(check_control(3, 2, &knots, &[1., 1e12, 1.], 500).is_ok());
        assert!(check_control(3, 2, &knots, &[1., 1e13, 1.], 500).is_err());
        assert!(check_control(3, 2, &knots, &[1., 0., 1.], 500).is_err());
        assert!(check_control(3, 2, &[0., 0., f64::NAN, 1., 1., 1.], &[], 500).is_err());
        assert!(fit_splines(&[Vec2::ZERO; 4], true, 500).is_err());
        assert!(fit_splines(&[Vec2::ZERO, Vec2::new(1., 0.)], false, 3).is_err());
        assert!(uniform_knots(MAX_CONTROLS + 1, 3).is_err());
        assert!(check_control(MAX_CONTROLS + 1, 3, &[], &[], MAX_CONTROLS).is_err());
        assert!(natural_spans(&vec![Vec2::ZERO; MAX_CONTROLS + 1]).is_err());
    }
}
