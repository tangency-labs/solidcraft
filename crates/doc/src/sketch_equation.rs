//! Equation curves (#8352): the points of a sketch curve worked out from its formulas over the
//! design's parameters. The formulas are evaluated at evenly spaced values of the curve's
//! variable; the sketch draws a cubic fit spline through those points. How many points: as many
//! as it takes for the spline to stay within the curve's tolerance of the formulas, checked
//! between the points, up to the most a spline takes. When the parameters change, the same
//! number of points is placed again (`Document::apply_equation_values`), before every solve.

use std::collections::BTreeMap;

use solidcraft_geom::Vec2;
use solidcraft_sketch::{EquationCurve, MAX_COORD, MAX_EQUATION_TEXT, MAX_SPLINE_POINTS, Sketch, fit_beziers_mode};

use crate::expr::{self, Defaults, Kind, Value};
use crate::{DocError, Result};

/// The tolerance a new equation curve gets (mm).
pub const DEFAULT_TOLERANCE: f64 = 0.01;
/// The tightest and loosest tolerances (mm).
pub const MIN_TOLERANCE: f64 = 1e-4;
pub const MAX_TOLERANCE: f64 = 10.0;
/// The fewest points a fit starts from.
const FIRST_COUNT: usize = 9;
/// The widest range of the variable.
const MAX_RANGE: f64 = 1e6;
/// How many times the point count is raised before the fit gives up.
const MAX_ROUNDS: usize = 12;

/// An equation curve's points and how well the spline through them follows the formulas.
#[derive(Clone, Debug, PartialEq)]
pub struct EquationFit {
    /// The spline's points in order; a closed curve's last point is its first one again.
    pub points: Vec<Vec2>,
    /// The curve ends where it starts: the spline closes smoothly there.
    pub closed: bool,
    /// The largest distance (mm) found between the spline and the formulas.
    pub deviation: f64,
}

/// How bare numbers read in the formulas: angles in radians (the variable is a plain number,
/// `sin(t)` its sine), lengths in the design's unit.
fn defaults(vals: &BTreeMap<String, Value>) -> Defaults {
    let len = vals.get(crate::eval::DESIGN_LEN).map(|v| v.v).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    Defaults { len, ang: 1.0 }
}

fn checked_text<'a>(what: &str, e: &'a str) -> Result<&'a str> {
    let e = e.trim();
    if e.is_empty() {
        return Err(DocError::Invalid(format!("{what} is empty")));
    }
    if e.chars().count() > MAX_EQUATION_TEXT {
        return Err(DocError::Invalid(format!("{what} is longer than {MAX_EQUATION_TEXT} characters")));
    }
    Ok(e)
}

/// A formula's value (mm) where the variable is `at`. The variable hides a parameter of its name.
fn formula(vals: &BTreeMap<String, Value>, e: &str, variable: &str, at: f64, d: Defaults) -> Result<f64> {
    let lookup = |n: &str| {
        if n == variable { Ok(Value::num(at)) } else { vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))) }
    };
    let v = expr::eval_with_defaults(e, &lookup, d)?.to_kind_in(Kind::Length, d)?;
    if v.is_finite() && v.abs() < MAX_COORD { Ok(v) } else { Err(DocError::Expr("the curve goes out of range".into())) }
}

/// Where the variable starts or ends, as the plain number the formulas see: a number, an angle
/// (in radians) or a length (in the design's unit).
fn range_end(vals: &BTreeMap<String, Value>, e: &str, d: Defaults) -> Result<f64> {
    let v = expr::eval_with_defaults(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))), d)?;
    let out = match v.kind() {
        Some(Kind::Unitless | Kind::Angle) => v.v,
        Some(Kind::Length) => v.v / d.len,
        None => return Err(DocError::Expr(format!("expected a number, an angle or a length, got {}", v.describe()))),
    };
    if out.is_finite() { Ok(out) } else { Err(DocError::Expr("not a finite number".into())) }
}

/// The variable's range, checked.
fn range(vals: &BTreeMap<String, Value>, eq: &EquationCurve, d: Defaults) -> Result<(f64, f64)> {
    let a = range_end(vals, checked_text("the start", &eq.start)?, d).map_err(|e| DocError::Expr(format!("start: {e}")))?;
    let b = range_end(vals, checked_text("the end", &eq.end)?, d).map_err(|e| DocError::Expr(format!("end: {e}")))?;
    if (b - a).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0) {
        return Err(DocError::Invalid("the start and the end of the range must differ".into()));
    }
    if (b - a).abs() > MAX_RANGE {
        return Err(DocError::Invalid(format!("the range is wider than {MAX_RANGE}")));
    }
    Ok((a, b))
}

/// The curve's point where the variable is `at`.
fn point_at(vals: &BTreeMap<String, Value>, eq: &EquationCurve, at: f64, d: Defaults) -> Result<Vec2> {
    let variable = eq.variable();
    let x = match &eq.x {
        Some(e) => formula(vals, e, variable, at, d).map_err(|e| DocError::Expr(format!("x: {e}")))?,
        // The explicit form: x is the variable, in the design's unit.
        None => {
            let x = at * d.len;
            if !(x.is_finite() && x.abs() < MAX_COORD) {
                return Err(DocError::Expr("the curve goes out of range".into()));
            }
            x
        }
    };
    let y = formula(vals, &eq.y, variable, at, d).map_err(|e| DocError::Expr(format!("y: {e}")))?;
    Ok(Vec2::new(x, y))
}

/// The formulas' points at `count` evenly spaced values of the variable, from start to end.
fn samples(vals: &BTreeMap<String, Value>, eq: &EquationCurve, (a, b): (f64, f64), count: usize, d: Defaults) -> Result<Vec<Vec2>> {
    let last = count.saturating_sub(1).max(1) as f64;
    (0..count)
        .map(|i| {
            let at = a + (b - a) * (i as f64 / last);
            point_at(vals, eq, at, d).map_err(|e| DocError::Expr(format!("at {} = {}: {e}", eq.variable(), short(at))))
        })
        .collect()
}

/// A number short enough for a message.
fn short(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The size of the curve: the larger side of its box (for closing tolerances).
fn extent(points: &[Vec2]) -> f64 {
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in points {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (hi.x - lo.x).max(hi.y - lo.y).max(0.0)
}

/// Does a curve through these samples end where it starts?
fn closes(points: &[Vec2]) -> bool {
    match (points.first(), points.last()) {
        (Some(a), Some(b)) => points.len() >= 4 && a.dist(*b) <= 1e-7 * extent(points).max(1.0),
        _ => false,
    }
}

/// Refuse samples a spline cannot pass through: two in a row on the same spot, or all of them.
fn check_samples(points: &[Vec2], closed: bool) -> Result<()> {
    let size = extent(points);
    if size <= 1e-9 {
        return Err(DocError::Invalid("the formulas give the same point all along the range".into()));
    }
    let spans = if closed { points.len().saturating_sub(1) } else { points.len() };
    for w in points.windows(2).take(spans) {
        if let [p, q] = w
            && p.dist(*q) <= 1e-9 * size.max(1.0)
        {
            return Err(DocError::Invalid("the curve stands still between two of its points: narrow the range or use fewer points".into()));
        }
    }
    Ok(())
}

/// Distance from `q` to the cubic Bézier `b`, sampled finely.
fn bezier_distance(b: &[Vec2; 4], q: Vec2) -> f64 {
    const STEPS: usize = 32;
    let at = |t: f64| {
        let s = 1.0 - t;
        b[0] * (s * s * s) + b[1] * (3.0 * t * s * s) + b[2] * (3.0 * t * t * s) + b[3] * (t * t * t)
    };
    let mut best = f64::INFINITY;
    let mut prev = at(0.0);
    for i in 1..=STEPS {
        let next = at(i as f64 / STEPS as f64);
        let d = next - prev;
        let l2 = d.len2();
        let t = if l2 > 0.0 { ((q - prev).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min(q.dist(prev + d * t));
        prev = next;
    }
    best
}

/// How far the spline through `points` strays from the formulas: the largest distance from the
/// formulas' points a quarter, a half and three quarters of the way between two samples to the
/// spline's piece between them.
fn deviation(vals: &BTreeMap<String, Value>, eq: &EquationCurve, (a, b): (f64, f64), points: &[Vec2], closed: bool, d: Defaults) -> Result<f64> {
    let pieces = fit_beziers_mode(points, closed).map_err(|e| DocError::Invalid(e.to_string()))?;
    let step = (b - a) / points.len().saturating_sub(1).max(1) as f64;
    let mut worst: f64 = 0.0;
    for (i, piece) in pieces.iter().enumerate() {
        for k in [0.25, 0.5, 0.75] {
            let q = point_at(vals, eq, a + step * (i as f64 + k), d)?;
            worst = worst.max(bezier_distance(piece, q));
        }
    }
    Ok(worst)
}

/// Check an equation's text and tolerance before any evaluation.
pub fn check(eq: &EquationCurve) -> Result<()> {
    if let Some(x) = &eq.x {
        checked_text("x(t)", x)?;
    }
    checked_text(if eq.x.is_some() { "y(t)" } else { "y(x)" }, &eq.y)?;
    checked_text("the start", &eq.start)?;
    checked_text("the end", &eq.end)?;
    if !(eq.tolerance.is_finite() && (MIN_TOLERANCE..=MAX_TOLERANCE).contains(&eq.tolerance)) {
        return Err(DocError::Invalid(format!("the tolerance must be from {MIN_TOLERANCE} mm to {MAX_TOLERANCE} mm")));
    }
    Ok(())
}

/// The points of `count` samples (a closed curve's last one is its first), whether it closes,
/// and how far the spline through them strays.
pub fn sample(vals: &BTreeMap<String, Value>, eq: &EquationCurve, count: usize, closed: Option<bool>) -> Result<EquationFit> {
    check(eq)?;
    if !(2..=MAX_SPLINE_POINTS).contains(&count) {
        return Err(DocError::Invalid(format!("an equation curve takes 2 to {MAX_SPLINE_POINTS} points")));
    }
    let d = defaults(vals);
    let span = range(vals, eq, d)?;
    let mut points = samples(vals, eq, span, count, d)?;
    let closed = closed.unwrap_or_else(|| closes(&points)) && count >= 4;
    if closed
        && let Some(first) = points.first().copied()
        && let Some(last) = points.last_mut()
    {
        *last = first;
    }
    check_samples(&points, closed)?;
    let deviation = deviation(vals, eq, span, &points, closed, d)?;
    Ok(EquationFit { points, closed, deviation })
}

/// The points of an equation curve, as few as keep the spline within its tolerance of the
/// formulas. Refused when even the most points a spline takes are not close enough.
pub fn fit(vals: &BTreeMap<String, Value>, eq: &EquationCurve) -> Result<EquationFit> {
    let mut count = FIRST_COUNT;
    for _ in 0..MAX_ROUNDS {
        let fit = sample(vals, eq, count, None)?;
        if fit.deviation <= eq.tolerance {
            return Ok(fit);
        }
        if count >= MAX_SPLINE_POINTS {
            return Err(DocError::Invalid(format!(
                "even {MAX_SPLINE_POINTS} points stray {} mm from the formulas, more than the tolerance of {} mm: raise the tolerance or narrow the range",
                short(fit.deviation),
                short(eq.tolerance)
            )));
        }
        // A cubic fit's error falls with the fourth power of the spacing: aim a little past
        // what that asks for, at least half as many points again.
        let ratio = (fit.deviation / eq.tolerance).powf(0.25) * 1.2;
        let next = (count as f64 * ratio.max(1.5)).ceil();
        count = if next.is_finite() && next < MAX_SPLINE_POINTS as f64 { next as usize } else { MAX_SPLINE_POINTS };
    }
    Err(DocError::Invalid("the equation curve's points could not be fitted".into()))
}

impl crate::Document {
    /// Place the points of the sketch's equation curves where their formulas put them now (in
    /// place), with the number of points each has. Their points are fixed, so the solve keeps
    /// them there. A curve that is gone or is no longer a fit point spline is left as it is.
    pub fn apply_equation_values(&self, vals: &BTreeMap<String, Value>, sk: &mut Sketch) -> Result<()> {
        for k in 0..sk.equations.len() {
            let Some(eq) = sk.equations.get(k).cloned() else { continue };
            let Some(ci) = sk.curve_index(&eq.curve) else { continue };
            let Some(solidcraft_sketch::CurveKind::Spline { pts, control: false, periodic, .. }) = sk.curves.get(ci).map(|c| &c.kind) else {
                continue;
            };
            let (count, closed) = (pts.len(), *periodic);
            let fit = sample(vals, &eq, count, Some(closed)).map_err(|e| DocError::Expr(format!("equation curve {}: {e}", eq.curve)))?;
            sk.place_equation_points(ci, &fit.points)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eq(x: Option<&str>, y: &str, start: &str, end: &str) -> EquationCurve {
        EquationCurve {
            curve: String::new(),
            x: x.map(str::to_string),
            y: y.into(),
            start: start.into(),
            end: end.into(),
            tolerance: DEFAULT_TOLERANCE,
        }
    }

    #[test]
    fn a_circle_closes_and_stays_within_the_tolerance() {
        let vals = BTreeMap::new();
        let fit = fit(&vals, &eq(Some("10 * cos(t)"), "10 * sin(t)", "0", "2 * pi")).unwrap();
        assert!(fit.closed);
        assert!(fit.deviation <= DEFAULT_TOLERANCE, "{}", fit.deviation);
        for p in &fit.points {
            assert!((p.len() - 10.0).abs() < 1e-9, "{p:?}");
        }
        assert_eq!(fit.points.first(), fit.points.last());
    }

    #[test]
    fn the_variable_hides_a_parameter_and_parameters_drive_the_curve() {
        let mut vals = BTreeMap::new();
        vals.insert("t".to_string(), Value::length(1000.0));
        vals.insert("amp".to_string(), Value::length(5.0));
        let fit = fit(&vals, &eq(None, "amp * sin(x / 10)", "0 mm", "100 mm")).unwrap();
        assert!(!fit.closed);
        let (first, last) = (fit.points[0], *fit.points.last().unwrap());
        assert!((first - Vec2::new(0.0, 0.0)).len() < 1e-9, "{first:?}");
        assert!((last.x - 100.0).abs() < 1e-9 && (last.y - 5.0 * 10f64.sin()).abs() < 1e-9, "{last:?}");
        assert!(fit.points.iter().all(|p| p.y.abs() <= 5.0 + 1e-9));
    }

    #[test]
    fn a_design_in_inches_reads_bare_numbers_in_inches() {
        let mut vals = BTreeMap::new();
        vals.insert(crate::eval::DESIGN_LEN.to_string(), Value::num(25.4));
        let fit = fit(&vals, &eq(Some("t"), "2", "0", "1")).unwrap();
        let last = *fit.points.last().unwrap();
        assert!((last - Vec2::new(25.4, 50.8)).len() < 1e-9, "{last:?}");
    }

    #[test]
    fn more_points_where_the_tolerance_is_tighter() {
        let vals = BTreeMap::new();
        let mut e = eq(Some("t * cos(t)"), "t * sin(t)", "0", "6 * pi");
        e.tolerance = 0.1;
        let loose = fit(&vals, &e).unwrap();
        e.tolerance = 0.005;
        let tight = fit(&vals, &e).unwrap();
        assert!(tight.points.len() > loose.points.len(), "{} vs {}", tight.points.len(), loose.points.len());
        assert!(tight.deviation <= 0.005 && loose.deviation <= 0.1);
    }

    #[test]
    fn hostile_formulas_and_ranges_are_refused() {
        let vals = BTreeMap::new();
        for (x, y, a, b, tol) in [
            (Some("t"), "sqrt(t)", "-1", "1", DEFAULT_TOLERANCE),
            (Some("t"), "1 / t", "-1", "1", DEFAULT_TOLERANCE),
            (Some("t"), "t", "0", "0", DEFAULT_TOLERANCE),
            (Some("t"), "t", "0", "1e300", DEFAULT_TOLERANCE),
            (Some("t"), "t", "0", "nan", DEFAULT_TOLERANCE),
            (Some("t * 1 deg"), "t", "0", "1", DEFAULT_TOLERANCE),
            (Some("1e300 * t"), "t", "0", "1", DEFAULT_TOLERANCE),
            (Some("5"), "5", "0", "1", DEFAULT_TOLERANCE),
            (Some(""), "t", "0", "1", DEFAULT_TOLERANCE),
            (Some("t"), "t", "0", "1", f64::NAN),
            (Some("t"), "t", "0", "1", 0.0),
            (Some("t"), "unknown_name * t", "0", "1", DEFAULT_TOLERANCE),
            (Some("t"), "sin(1e5 * t)", "0", "1000", 1e-4),
        ] {
            let mut e = eq(x, y, a, b);
            e.tolerance = tol;
            assert!(fit(&vals, &e).is_err(), "{x:?} {y} {a}..{b} {tol}");
        }
        let long = "t + ".repeat(400) + "t";
        assert!(fit(&vals, &eq(Some(&long), "t", "0", "1")).is_err());
    }
}
