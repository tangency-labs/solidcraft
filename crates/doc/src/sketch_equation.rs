//! Equation curves (#8352): the points of a sketch curve worked out from its formulas over the
//! design's parameters. The formulas are evaluated at evenly spaced values of the curve's
//! variable; the sketch draws a cubic fit spline through those points. How many points: as many
//! as it takes for the spline to stay within the curve's tolerance of the formulas, proven
//! between the points (#10414), up to the most a spline takes. When the parameters change, the
//! same number of points is placed again (`Document::apply_equation_values`), before every solve.
//!
//! The proof: on each span of the spline, the formula's variable `t` is mapped linearly onto the
//! span's Bézier parameter `s`, and `|F(t) − B(s(t))|` is bounded over the span in intervals
//! ([`solidcraft_geom::enclosure`]), with the derivative's mean-value form to keep the bound
//! close, splitting the span where the bound is too wide. Every point of the formulas is then
//! within the bound of a point of the spline, and every point of the spline within it of a point
//! of the formulas. A formula that jumps (`floor`, `sign`, `mod`, `tan` at a pole) or leaves a
//! function's domain inside a span can't be proven there, and the curve is refused with where.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use solidcore_record::Vec2;
use solidcraft_geom::enclosure::{Interval, Jet, Real};
use solidcraft_sketch::{EquationCurve, MAX_COORD, MAX_EQUATION_TEXT, MAX_SPLINE_POINTS, Sketch, VariableUnit, fit_beziers_mode};

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
/// Most parts of spans the proof bounds for one point count: it bounds each span once and splits
/// only where the bound is too wide, so this is reached only by a formula far from its spline.
const MAX_CHECKS: usize = 20_000;
/// The narrowest part of a span the proof splits, as a fraction of the span.
const MIN_PART: f64 = 1e-9;

/// An equation curve's points and how well the spline through them follows the formulas.
#[derive(Clone, Debug, PartialEq)]
pub struct EquationFit {
    /// The spline's points in order; a closed curve's last point is its first one again.
    pub points: Vec<Vec2>,
    /// The curve ends where it starts: the spline closes smoothly there.
    pub closed: bool,
    /// How far the spline may stray from the formulas (mm). From [`fit`], a proven bound within
    /// the tolerance. From [`sample`], the proven bound where the check proves one; otherwise an
    /// estimate above the tolerance, which proves nothing (a point found that far off, or what was
    /// left unproven when the check gave up).
    pub deviation: f64,
}

/// Why a range cannot give the variable its unit (#9333): a length and an angle have no unit in
/// common, and two plain numbers have none to give.
const RANGE_UNIT_REFUSED: &str =
    "with the variable in its range's unit, start and end must both be lengths, or one a length and the other a plain number";

/// How bare numbers read in the formulas: angles in radians (a plain variable's `sin(t)` is its
/// sine), lengths in the design's unit.
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

/// The variable as the formulas see it, where it is `at` (a number, or the design's unit).
fn variable_value(eq: &EquationCurve, at: f64, d: Defaults) -> Value {
    match eq.variable_unit {
        VariableUnit::Number => Value::num(at),
        VariableUnit::Range => Value::length(at * d.len),
    }
}

/// A formula's value (mm) where the variable is `at`. The variable hides a parameter of its name.
fn formula(vals: &BTreeMap<String, Value>, e: &str, variable: &str, at: Value, d: Defaults) -> Result<f64> {
    let lookup = |n: &str| {
        if n == variable { Ok(at) } else { vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))) }
    };
    let v = expr::eval_with_defaults(e, &lookup, d)?.to_kind_in(Kind::Length, d)?;
    if v.is_finite() && v.abs() < MAX_COORD { Ok(v) } else { Err(DocError::Expr("the curve goes out of range".into())) }
}

/// Where the variable starts or ends, as a number: a number, an angle (in radians) or a length
/// (in the design's unit); and which of them it is.
fn range_end(vals: &BTreeMap<String, Value>, e: &str, d: Defaults) -> Result<(f64, Kind)> {
    let v = expr::eval_with_defaults(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))), d)?;
    let (out, kind) = match v.kind() {
        Some(kind @ (Kind::Unitless | Kind::Angle)) => (v.v, kind),
        Some(Kind::Length) => (v.v / d.len, Kind::Length),
        None => return Err(DocError::Expr(format!("expected a number, an angle or a length, got {}", v.describe()))),
    };
    if out.is_finite() { Ok((out, kind)) } else { Err(DocError::Expr("not a finite number".into())) }
}

/// The variable's range, checked.
fn range(vals: &BTreeMap<String, Value>, eq: &EquationCurve, d: Defaults) -> Result<(f64, f64)> {
    let (a, start) = range_end(vals, checked_text("the start", &eq.start)?, d).map_err(|e| DocError::Expr(format!("start: {e}")))?;
    let (b, end) = range_end(vals, checked_text("the end", &eq.end)?, d).map_err(|e| DocError::Expr(format!("end: {e}")))?;
    // A plain number beside a length is in the design's unit, as `range_end` already reads it.
    if eq.variable_unit == VariableUnit::Range
        && !matches!((start, end), (Kind::Length, Kind::Length | Kind::Unitless) | (Kind::Unitless, Kind::Length))
    {
        return Err(DocError::Invalid(RANGE_UNIT_REFUSED.into()));
    }
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
    let value = variable_value(eq, at, d);
    let x = match &eq.x {
        Some(e) => formula(vals, e, variable, value, d).map_err(|e| DocError::Expr(format!("x: {e}")))?,
        // The explicit form: x is the variable, in the design's unit.
        None => {
            let x = at * d.len;
            if !(x.is_finite() && x.abs() < MAX_COORD) {
                return Err(DocError::Expr("the curve goes out of range".into()));
            }
            x
        }
    };
    let y = formula(vals, &eq.y, variable, value, d).map_err(|e| DocError::Expr(format!("y: {e}")))?;
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

/// The formulas, read once, for evaluating in intervals many times.
struct Formulas<'a> {
    vals: &'a BTreeMap<String, Value>,
    eq: &'a EquationCurve,
    x: Option<expr::Compiled>,
    y: expr::Compiled,
    d: Defaults,
}

impl<'a> Formulas<'a> {
    fn new(vals: &'a BTreeMap<String, Value>, eq: &'a EquationCurve, d: Defaults) -> Result<Formulas<'a>> {
        let x = eq.x.as_deref().map(expr::Compiled::new).transpose()?;
        Ok(Formulas { vals, eq, x, y: expr::Compiled::new(&eq.y)?, d })
    }

    /// The formulas' point where the variable is `at`, in `N`, in mm.
    fn at<N: Real>(&self, at: N) -> Result<[N; 2]> {
        let d = self.d;
        let in_len = |x: N| expr::proven(N::exact(d.len).and_then(|l| x.mul(l)));
        let variable = match self.eq.variable_unit {
            VariableUnit::Number => Value { v: at, len: 0, ang: 0 },
            VariableUnit::Range => Value { v: in_len(at)?, len: 1, ang: 0 },
        };
        let name = self.eq.variable();
        let lookup = |n: &str| {
            if n == name {
                Ok(variable)
            } else {
                self.vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")))?.exact::<N>()
            }
        };
        let x = match &self.x {
            Some(c) => c.eval(&lookup, d)?.to_kind_in(Kind::Length, d)?,
            None => in_len(at)?,
        };
        Ok([x, self.y.eval(&lookup, d)?.to_kind_in(Kind::Length, d)?])
    }

    /// `F(t) − B(s(t))` for the cubic Bézier `b`, over a part of its span where `s` runs linearly
    /// from `s0` at `t0` to `s1` at `t1`.
    fn gap<N: Real>(&self, b: &[Vec2; 4], part: Part, t: N) -> Result<[N; 2]> {
        let p = expr::proven;
        let [t0, t1, s0, s1] = [part.t0, part.t1, part.s0, part.s1].map(N::exact);
        let (t0, t1, s0, s1) = (p(t0)?, p(t1)?, p(s0)?, p(s1)?);
        let s = p(t.sub(t0).and_then(|u| t1.sub(t0).and_then(|h| u.div(h))).and_then(|f| s1.sub(s0).and_then(|w| f.mul(w))).and_then(|v| s0.add(v)))?;
        // The piece about the middle of the part's run of `s`, so its bound grows only with the
        // square of how far `s` runs, not with the size of its control points.
        let sc = f64::midpoint(part.s0, part.s1);
        let delta = p(s.sub(p(N::exact(sc))?))?;
        let f = self.at(t)?;
        let mut out = f;
        for (k, o) in out.iter_mut().enumerate() {
            let [c0, c1, c2, c3] = taylor(b, sc, k).map(N::within);
            let horner = c3.and_then(|c3| c3.mul(delta)).and_then(|v| c2.and_then(|c2| v.add(c2))).and_then(|v| v.mul(delta));
            let horner = horner.and_then(|v| c1.and_then(|c1| v.add(c1))).and_then(|v| v.mul(delta)).and_then(|v| c0.and_then(|c0| v.add(c0)));
            *o = p(horner.and_then(|bk| o.sub(bk)))?;
        }
        Ok(out)
    }

    /// The parameter of the piece's point nearest the formulas' point at `t`, from Newton's
    /// method started at `guess`, and the distance to it. The proof maps `t` to that parameter:
    /// any mapping that runs from 0 at the span's start to 1 at its end gives a sound bound, as
    /// the parts' runs of `s` chain from 0 to 1 and so cover the piece, and the nearest point
    /// keeps the bound tight where the spline's chord-length parameter runs unevenly against `t`.
    fn nearest(&self, b: &[Vec2; 4], t: f64, guess: f64) -> Option<(f64, f64)> {
        let [x, y] = self.at::<f64>(t).ok()?;
        Some(nearest_on(b, Vec2::new(x, y), guess))
    }

    /// A proven bound of `|F − B|` over a part of a span.
    fn bound(&self, b: &[Vec2; 4], part: Part) -> Result<f64> {
        let whole = Interval::new(part.t0, part.t1);
        let c = whole.mid();
        let at_mid = self.gap(b, part, Interval::point(c))?;
        let ranges = match Jet::variable(whole).ok().map(|t| self.gap(b, part, t)) {
            Some(Ok(g)) => {
                let mut out = [whole; 2];
                for (k, (g, m)) in g.iter().zip(at_mid).enumerate() {
                    let range = if g.d.lo() > 0.0 || g.d.hi() < 0.0 {
                        // Monotone: its extremes are at the ends.
                        let (lo, hi) = (self.gap(b, part, Interval::point(part.t0))?, self.gap(b, part, Interval::point(part.t1))?);
                        lo[k].hull(hi[k])
                    } else {
                        m + g.d * (whole - Interval::point(c))
                    };
                    out[k] = range.meet(g.v).unwrap_or(g.v);
                }
                out
            }
            // A derivative without a bound (`√` at 0): the values alone.
            _ => self.gap(b, part, whole)?,
        };
        Ok((ranges[0].abs().sqr() + ranges[1].abs().sqr()).sqrt_nonneg().hi())
    }

    /// What the check finds on a span, and an error where the formulas can't be bounded however
    /// narrow the part (a jump, a domain's edge).
    fn span(&self, b: &[Vec2; 4], span: (f64, f64), tol: f64, budget: &mut usize) -> Result<Found> {
        let narrowest = (span.1 - span.0).abs() * MIN_PART;
        let mut parts = vec![Part { t0: span.0, t1: span.1, s0: 0.0, s1: 1.0 }];
        let mut worst: f64 = 0.0;
        while let Some(part) = parts.pop() {
            if *budget == 0 {
                return Ok(Found::Unproven(worst.max(tol) * 2.0));
            }
            *budget -= 1;
            let narrow = (part.t1 - part.t0).abs() <= narrowest;
            let m = f64::midpoint(part.t0, part.t1);
            let (sm, off) = self.nearest(b, m, f64::midpoint(part.s0, part.s1)).unwrap_or((f64::midpoint(part.s0, part.s1), 0.0));
            match self.bound(b, part) {
                Ok(above) if above <= tol => {
                    worst = worst.max(above);
                    continue;
                }
                Ok(above) if narrow => return Ok(Found::Unproven(above)),
                Err(e) if narrow => {
                    return Err(DocError::Invalid(format!(
                        "the formulas can't be shown to stay within the tolerance near {} = {}: {e}",
                        self.eq.variable(),
                        short(m)
                    )));
                }
                _ => {}
            }
            // A point of the formulas already further than the tolerance from the piece: this
            // count falls short (an estimate that only steers the count; it proves nothing).
            if off > tol {
                return Ok(Found::Off { by: off, at: m });
            }
            let sm = sm.clamp(part.s0.min(part.s1), part.s0.max(part.s1));
            parts.push(Part { t1: m, s1: sm, ..part });
            parts.push(Part { t0: m, s0: sm, ..part });
        }
        Ok(Found::Within(worst))
    }
}

/// The parameter of the cubic Bézier `b`'s point nearest `target`, by Newton's method from
/// `guess`, and the distance to it.
fn nearest_on(b: &[Vec2; 4], target: Vec2, guess: f64) -> (f64, f64) {
    let point = |s: f64| {
        let r = 1.0 - s;
        b[0] * (r * r * r) + b[1] * (3.0 * s * r * r) + b[2] * (3.0 * s * s * r) + b[3] * (s * s * s)
    };
    let mut s = guess.clamp(0.0, 1.0);
    for _ in 0..8 {
        let r = 1.0 - s;
        let d1 = (b[1] - b[0]) * (3.0 * r * r) + (b[2] - b[1]) * (6.0 * s * r) + (b[3] - b[2]) * (3.0 * s * s);
        let d2 = (b[2] - b[1] * 2.0 + b[0]) * (6.0 * r) + (b[3] - b[2] * 2.0 + b[1]) * (6.0 * s);
        let off = point(s) - target;
        let (g, h) = (off.dot(d1), d1.dot(d1) + off.dot(d2));
        let next = s - g / h;
        if !(h > 0.0 && next.is_finite()) {
            break;
        }
        s = next.clamp(0.0, 1.0);
    }
    (s, point(s).dist(target))
}

/// Most boxes [`surely_far`] looks at before giving up.
const MAX_BOXES: usize = 4096;

/// Whether every point of the spline is proven further than `tol` from every point of `target`.
/// A cubic piece lies inside the box of its control points, and halving a piece in intervals
/// gives control points that enclose each half's exact ones; so a box further than `tol` away
/// rules its part out, and the rest is halved until ruled out. `false` when a part stays close or
/// the work runs out: not proven far.
fn surely_far(pieces: &[[Vec2; 4]], target: [Interval; 2], tol: f64) -> bool {
    let mut stack: Vec<[[Interval; 2]; 4]> = pieces.iter().map(|b| b.map(|q| [Interval::point(q.x), Interval::point(q.y)])).collect();
    let half = Interval::point(0.5);
    let mut boxes = 0;
    while let Some(c) = stack.pop() {
        boxes += 1;
        if boxes > MAX_BOXES {
            return false;
        }
        // The squared gap between the box and the target in each direction, rounded down.
        let mut gap2 = Interval::ZERO;
        for k in 0..2 {
            let lo = c.iter().map(|p| p[k].lo()).fold(f64::INFINITY, f64::min);
            let hi = c.iter().map(|p| p[k].hi()).fold(f64::NEG_INFINITY, f64::max);
            let g = if lo > target[k].hi() {
                Interval::point(lo) - Interval::point(target[k].hi())
            } else if target[k].lo() > hi {
                Interval::point(target[k].lo()) - Interval::point(hi)
            } else {
                Interval::ZERO
            };
            gap2 = gap2 + Interval::point(g.lo().max(0.0)).sqr();
        }
        // Ruled out only when surely further than `tol`: the gap's lower bound against the
        // upper bound of `tol²`.
        if gap2.lo() > Interval::point(tol).sqr().hi() {
            continue;
        }
        // Close, and already small: the curve may come within `tol` here.
        let size = c.iter().flat_map(|p| p.iter()).map(|i| i.width()).fold(0.0, f64::max);
        let spread = (0..2)
            .map(|k| c.iter().map(|p| p[k].hi()).fold(f64::NEG_INFINITY, f64::max) - c.iter().map(|p| p[k].lo()).fold(f64::INFINITY, f64::min))
            .fold(0.0, f64::max);
        if spread.max(size) <= tol {
            return false;
        }
        // de Casteljau at ½ in intervals.
        let mid = |a: [Interval; 2], b: [Interval; 2]| [(a[0] + b[0]) * half, (a[1] + b[1]) * half];
        let (p01, p12, p23) = (mid(c[0], c[1]), mid(c[1], c[2]), mid(c[2], c[3]));
        let (p012, p123) = (mid(p01, p12), mid(p12, p23));
        let m = mid(p012, p123);
        stack.push([c[0], p01, p012, m]);
        stack.push([m, p123, p23, c[3]]);
    }
    true
}

/// What the check found, on a span or the whole curve.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Found {
    /// Proven within the tolerance: the bound.
    Within(f64),
    /// A point of the formulas found about `by` mm from its span's piece, where the variable is
    /// `at`, by Newton's method: the count falls short. A heuristic, which proves nothing.
    Off { by: f64, at: f64 },
    /// Neither, within the work allowed: an estimate above the tolerance.
    Unproven(f64),
}

impl Found {
    /// The deviation a fit reports.
    fn deviation(self) -> f64 {
        match self {
            Found::Within(d) | Found::Off { by: d, .. } | Found::Unproven(d) => d,
        }
    }
}

/// Coordinate `k` of the cubic Bézier `b` as a polynomial in `s − sc`: its four coefficients,
/// each enclosed.
fn taylor(b: &[Vec2; 4], sc: f64, k: usize) -> [Interval; 4] {
    let c = |q: Vec2| Interval::point(if k == 0 { q.x } else { q.y });
    let (p0, p1, p2, p3) = (c(b[0]), c(b[1]), c(b[2]), c(b[3]));
    let (two, three) = (Interval::point(2.0), Interval::point(3.0));
    // The power basis: a0 + a1 s + a2 s² + a3 s³.
    let a1 = three * (p1 - p0);
    let a2 = three * (p2 - two * p1 + p0);
    let a3 = p3 - three * p2 + three * p1 - p0;
    let s = Interval::point(sc);
    [p0 + s * (a1 + s * (a2 + s * a3)), a1 + s * (two * a2 + three * s * a3), a2 + three * s * a3, a3]
}

/// A part of a span: `t` from `t0` to `t1`, mapped linearly onto the piece's `s0` to `s1`.
#[derive(Clone, Copy, Debug)]
struct Part {
    t0: f64,
    t1: f64,
    s0: f64,
    s1: f64,
}

/// What the check finds for the spline through `points`: a bound proven within the tolerance, a
/// point of the formulas found off, or neither within the work allowed.
fn deviation(vals: &BTreeMap<String, Value>, eq: &EquationCurve, (a, b): (f64, f64), points: &[Vec2], closed: bool, d: Defaults) -> Result<Found> {
    let pieces = fit_beziers_mode(points, closed).map_err(|e| DocError::Invalid(e.to_string()))?;
    let f = Formulas::new(vals, eq, d)?;
    let last = points.len().saturating_sub(1).max(1);
    // The spans meet where the samples were taken, and the last one ends at the range's end.
    let t = |i: usize| if i >= last { b } else { a + (b - a) * (i as f64 / last as f64) };
    let mut budget = MAX_CHECKS;
    let (mut worst, mut unproven): (f64, Option<f64>) = (0.0, None);
    for (i, piece) in pieces.iter().enumerate() {
        match f.span(piece, (t(i), t(i + 1)), eq.tolerance, &mut budget)? {
            Found::Within(bound) => worst = worst.max(bound),
            off @ Found::Off { .. } => return Ok(off),
            // Keep looking: a later span may be found off.
            Found::Unproven(e) => unproven = Some(unproven.map_or(e, |u: f64| u.max(e))),
        }
    }
    Ok(unproven.map_or(Found::Within(worst), Found::Unproven))
}

/// A refusal when, with these values, a point of the formulas is further from the whole spline
/// than the tolerance (#10414): the saved count can't follow them, as a wave made finer by a
/// parameter. A curve proven within, or one that merely can't be proven at its count (saved
/// before the proof, or needing more points to prove than to be within), keeps rebuilding. The
/// last few curves found fine are remembered (per thread), so an unchanged rebuild doesn't check
/// them again.
fn shown_off(vals: &BTreeMap<String, Value>, eq: &EquationCurve, span: (f64, f64), points: &[Vec2], closed: bool, d: Defaults) -> Option<String> {
    type Key = (EquationCurve, Vec<(String, u64, i8, i8)>, u64, usize, bool);
    thread_local! {
        static FINE: std::cell::RefCell<Vec<Key>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let texts = [eq.x.as_deref(), Some(eq.y.as_str()), Some(eq.start.as_str()), Some(eq.end.as_str())];
    let mut used: Vec<(String, u64, i8, i8)> =
        texts.into_iter().flatten().flat_map(expr::references).filter_map(|n| vals.get(&n).map(|v| (n, v.v.to_bits(), v.len, v.ang))).collect();
    used.sort();
    used.dedup();
    let key: Key = (eq.clone(), used, d.len.to_bits(), points.len(), closed);
    if FINE.with(|fine| fine.borrow().contains(&key)) {
        return None;
    }
    let refusal = match deviation(vals, eq, span, points, closed, d) {
        // Found off by a heuristic: refused only once proven off.
        Ok(Found::Off { at, .. }) => surely_off(vals, eq, points, closed, d, at).then(|| {
            format!(
                "with these values its {} points leave the formulas' point at {} = {} further than the tolerance of {} mm from the spline: edit the curve to fit it again",
                points.len(),
                eq.variable(),
                short(at),
                short(eq.tolerance)
            )
        }),
        _ => None,
    };
    if refusal.is_none() {
        FINE.with(|fine| {
            let mut fine = fine.borrow_mut();
            if fine.len() >= 64 {
                fine.remove(0);
            }
            fine.push(key);
        });
    }
    refusal
}

/// Whether the formulas' point where the variable is `at` is proven further than the tolerance
/// from every point of the spline through `points`, that point enclosed in intervals.
fn surely_off(vals: &BTreeMap<String, Value>, eq: &EquationCurve, points: &[Vec2], closed: bool, d: Defaults, at: f64) -> bool {
    let Ok(pieces) = fit_beziers_mode(points, closed) else { return false };
    let Ok(target) = Formulas::new(vals, eq, d).and_then(|f| f.at::<Interval>(Interval::point(at))) else { return false };
    surely_far(&pieces, target, eq.tolerance)
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
/// and how far the spline through them may stray.
pub fn sample(vals: &BTreeMap<String, Value>, eq: &EquationCurve, count: usize, closed: Option<bool>) -> Result<EquationFit> {
    sampled(vals, eq, count, closed).map(|(fit, _)| fit)
}

/// [`sample`], with what the check found.
fn sampled(vals: &BTreeMap<String, Value>, eq: &EquationCurve, count: usize, closed: Option<bool>) -> Result<(EquationFit, Found)> {
    let (points, closed, span, d) = place(vals, eq, count, closed)?;
    let found = deviation(vals, eq, span, &points, closed, d)?;
    Ok((EquationFit { points, closed, deviation: found.deviation() }, found))
}

/// The points of `count` samples and whether they close, with the range and how bare numbers
/// read: what placing the points needs, without the proof.
fn place(vals: &BTreeMap<String, Value>, eq: &EquationCurve, count: usize, closed: Option<bool>) -> Result<(Vec<Vec2>, bool, (f64, f64), Defaults)> {
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
    Ok((points, closed, span, d))
}

/// The points of an equation curve, as few as are proven to keep the spline within its tolerance
/// of the formulas. Refused when even the most points a spline takes are not shown close enough:
/// found off, or not proven within.
pub fn fit(vals: &BTreeMap<String, Value>, eq: &EquationCurve) -> Result<EquationFit> {
    let mut count = FIRST_COUNT;
    for _ in 0..MAX_ROUNDS {
        let (fit, found) = sampled(vals, eq, count, None)?;
        if fit.deviation <= eq.tolerance {
            return Ok(fit);
        }
        if count >= MAX_SPLINE_POINTS {
            let tol = short(eq.tolerance);
            return Err(DocError::Invalid(match found {
                // A point of the formulas proven further than the tolerance from the spline, and
                // measured about that far from its piece.
                Found::Off { by, at } if surely_off(vals, eq, &fit.points, fit.closed, defaults(vals), at) => format!(
                    "even {MAX_SPLINE_POINTS} points don't keep the spline within the tolerance of {tol} mm of the formulas (it was found about {} mm off): raise the tolerance or narrow the range",
                    short(by)
                ),
                // Only an estimate above the tolerance, or the check's work ran out: the spline
                // may well be within, but that isn't proven.
                _ => format!(
                    "even {MAX_SPLINE_POINTS} points can't be shown to keep the spline within the tolerance of {tol} mm of the formulas: raise the tolerance or narrow the range"
                ),
            }));
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
    /// them there. A curve that is gone or is no longer a fit point spline is left as it is. A
    /// curve whose formulas are now found further from its spline than its tolerance is an error
    /// ([`shown_off`]).
    pub fn apply_equation_values(&self, vals: &BTreeMap<String, Value>, sk: &mut Sketch) -> Result<()> {
        for k in 0..sk.equations.len() {
            let Some(eq) = sk.equations.get(k).cloned() else { continue };
            let Some(ci) = sk.curve_index(&eq.curve) else { continue };
            let Some(solidcraft_sketch::CurveKind::Spline { pts, control: false, periodic, .. }) = sk.curves.get(ci).map(|c| &c.kind) else {
                continue;
            };
            let (count, closed) = (pts.len(), *periodic);
            let (points, closed, span, d) =
                place(vals, &eq, count, Some(closed)).map_err(|e| DocError::Expr(format!("equation curve {}: {e}", eq.curve)))?;
            if let Some(why) = shown_off(vals, &eq, span, &points, closed, d) {
                return Err(DocError::Expr(format!("equation curve {}: {why}", eq.curve)));
            }
            sk.place_equation_points(ci, &points)?;
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
            variable_unit: VariableUnit::Number,
        }
    }

    /// The same curve with its variable in its range's unit.
    fn with_range_unit(eq: EquationCurve) -> EquationCurve {
        EquationCurve { variable_unit: VariableUnit::Range, ..eq }
    }

    fn with_a(mm: f64, design_len: Option<f64>) -> BTreeMap<String, Value> {
        let mut vals = BTreeMap::new();
        vals.insert("a".to_string(), Value::length(mm));
        if let Some(len) = design_len {
            vals.insert(crate::eval::DESIGN_LEN.to_string(), Value::num(len));
        }
        vals
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

    /// The formulas' point at `t`, worked out apart from the check (plain `f64`).
    fn formula_point(e: &EquationCurve, t: f64) -> Vec2 {
        let name = e.variable();
        let d = Defaults { len: 1.0, ang: 1.0 };
        let lookup = |n: &str| if n == name { Ok(Value::num(t)) } else { Err(DocError::Expr(n.into())) };
        let value = |f: &str| expr::eval_with_defaults(f, &lookup, d).and_then(|v| v.to_kind_in(Kind::Length, d)).unwrap();
        Vec2::new(e.x.as_deref().map_or(t, value), value(&e.y))
    }

    /// An accepted curve is within its reported deviation, and so its tolerance, everywhere: the
    /// formulas measured densely, each point's distance to the spline's polyline at 400 steps a
    /// piece. The sampled check this replaced reported less than this measures on every curve
    /// here (#10414).
    #[test]
    fn an_accepted_curve_is_within_its_deviation_everywhere() {
        let vals = BTreeMap::new();
        for (x, y, a, b, tol) in [
            (None, "10 * (cosh(x / 40) + 0.6 * sinh(x / 40))", 0.0, 120.0, 0.01),
            (Some("20 * cos(t)"), "20 * sin(t)", 0.0, 1.5 * std::f64::consts::PI, 0.01),
            (None, "x * x / 20", -30.0, 30.0, 0.01),
            (Some("t * cos(t)"), "t * sin(t)", 0.0, 6.0 * std::f64::consts::PI, 0.1),
            (None, "5 + 2 * sin(x / 5)", 0.0, 30.0, 0.01),
            (None, "abs(x)", -1.0, 1.0, 0.01),
        ] {
            let mut e = eq(x, y, &a.to_string(), &b.to_string());
            e.tolerance = tol;
            let f = fit(&vals, &e).unwrap();
            assert!(f.deviation <= tol, "{y}: {}", f.deviation);
            let pieces = fit_beziers_mode(&f.points, f.closed).unwrap();
            let line: Vec<Vec2> = pieces
                .iter()
                .flat_map(|p| {
                    (0..=400).map(move |k| {
                        let (s, r) = (k as f64 / 400.0, 1.0 - k as f64 / 400.0);
                        p[0] * (r * r * r) + p[1] * (3.0 * s * r * r) + p[2] * (3.0 * s * s * r) + p[3] * (s * s * s)
                    })
                })
                .collect();
            for k in 0..=3000 {
                let q = formula_point(&e, a + (b - a) * k as f64 / 3000.0);
                let near = line
                    .windows(2)
                    .map(|w| {
                        let d = w[1] - w[0];
                        let u = if d.len2() > 0.0 { ((q - w[0]).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
                        q.dist(w[0] + d * u)
                    })
                    .fold(f64::INFINITY, f64::min);
                assert!(near <= f.deviation, "{y} at step {k}: {near} mm from the spline, more than {}", f.deviation);
            }
        }
    }

    /// A formula that is zero at every place a sampled check would look (#10404) is not drawn as a
    /// flat line 1 mm off: these waves are too fine for 500 points and are refused. The sampled
    /// check accepted each with 9 points. A point proven off is quoted with how far off it was
    /// found; a spline that crisscrosses its wave, never proven within nor off, is refused
    /// without a number, which would be made up.
    #[test]
    fn a_wave_zero_at_every_check_point_is_refused_not_drawn_flat() {
        let vals = BTreeMap::new();
        let off = "even 500 points don't keep the spline within the tolerance";
        let unproven = "even 500 points can't be shown to keep the spline within the tolerance of 0.01 mm of the formulas: raise";
        for (y, tol, want) in [("sin(64 * pi * x)", 1e-4, off), ("sin(800 * pi * x)", 0.01, unproven), ("sin(2048 * pi * x)", 1e-4, off)] {
            let mut e = eq(None, y, "0", "1");
            e.tolerance = tol;
            let err = fit(&vals, &e).map(|f| f.points.len()).unwrap_err().to_string();
            assert!(err.contains(want) && err.contains("mm off") == (want == off), "{y}: {err}");
        }
    }

    /// A formula that jumps can't be followed by a smooth curve: it is refused, saying where.
    #[test]
    fn a_formula_that_jumps_is_refused_where_it_jumps() {
        let vals = BTreeMap::new();
        for (y, at) in [("floor(x)", "x = 1:"), ("sign(x - 0.5)", "x = 0.5:"), ("mod(x, 2)", "x = 2:")] {
            let err = fit(&vals, &eq(None, y, "0", "3")).map(|f| f.points.len()).unwrap_err().to_string();
            assert!(err.contains("can't be shown to stay within the tolerance near") && err.contains(at), "{y}: {err}");
        }
    }

    /// On a rebuild, a curve is refused only when its formulas are proven further than the
    /// tolerance from its spline. The semicircle at 348 points is within the tolerance (measured
    /// densely, from 346 points) though not provable below 352, and keeps rebuilding; a sine the
    /// samples all miss is refused (#10414).
    #[test]
    fn a_rebuild_is_refused_only_when_the_formulas_are_shown_off_the_spline() {
        let vals = BTreeMap::new();
        let shown_off_at = |x: Option<&str>, y: &str, (a, b): (f64, f64), tol: f64, count: usize| {
            let mut e = eq(x, y, &a.to_string(), &b.to_string());
            e.tolerance = tol;
            let (points, closed, span, d) = place(&vals, &e, count, None).unwrap();
            (deviation(&vals, &e, span, &points, closed, d).unwrap(), shown_off(&vals, &e, span, &points, closed, d))
        };
        let (found, refused) = shown_off_at(None, "sqrt(400 - x^2)", (-20.0, 20.0), 0.01, 348);
        assert!(!matches!(found, Found::Within(_)), "proven at 348 points now: pick a count it isn't");
        assert_eq!(refused, None);
        let (_, refused) = shown_off_at(None, "sin(800 * pi * x)", (0.0, 1.0), 0.01, 9);
        assert!(refused.is_some_and(|why| why.contains("further than the tolerance of 0.01 mm from the spline")));
    }

    /// A piece exactly the tolerance away is not ruled out, along an axis or across one, nor one
    /// a step closer; one a step further is.
    #[test]
    fn a_piece_at_the_tolerance_is_not_proven_far() {
        let tol = 0.01;
        let at = |x: f64, y: f64| [Vec2::new(x, y); 4];
        let target = [Interval::ZERO, Interval::ZERO];
        // 0.006² + 0.008² = 0.01² in reals; the doubles are close, so only the exact axis cases
        // can be told apart by a step.
        for piece in [at(tol, 0.0), at(0.0, -tol), at(0.006, 0.008), at(tol.next_down(), 0.0)] {
            assert!(!surely_far(&[piece], target, tol), "{piece:?}");
        }
        assert!(surely_far(&[at(tol.next_up().next_up(), 0.0)], target, tol));
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

    /// The issue's parabola (#9333): y = a^2 / (4 t) over t from a to 3a, where t is a length.
    /// Every point is exact, and the curve is the same in millimetres whatever the design's unit.
    #[test]
    fn a_variable_in_its_ranges_unit_gives_the_exact_parabola_in_any_design_unit() {
        for design_len in [None, Some(25.4)] {
            let vals = with_a(10.0, design_len);
            let e = with_range_unit(eq(Some("t"), "a ^ 2 / (4 * t)", "a", "3 * a"));
            let fit = fit(&vals, &e).unwrap();
            let (first, last) = (fit.points[0], *fit.points.last().unwrap());
            assert!((first - Vec2::new(10.0, 2.5)).len() < 1e-9, "{design_len:?} {first:?}");
            assert!((last - Vec2::new(30.0, 100.0 / 120.0)).len() < 1e-9, "{design_len:?} {last:?}");
            for p in &fit.points {
                assert!((p.y - 100.0 / (4.0 * p.x)).abs() < 1e-9 && (10.0 - 1e-9..=30.0 + 1e-9).contains(&p.x), "{design_len:?} {p:?}");
            }
            assert!(fit.deviation <= DEFAULT_TOLERANCE);
        }
    }

    /// The explicit form names its variable x; bare numbers beside it are in the design's unit.
    #[test]
    fn the_explicit_form_and_bare_numbers_follow_the_design_unit() {
        let vals = with_a(25.4, Some(25.4));
        let fit = fit(&vals, &with_range_unit(eq(None, "x + 1", "0", "a"))).unwrap();
        let (first, last) = (fit.points[0], *fit.points.last().unwrap());
        assert!((first - Vec2::new(0.0, 25.4)).len() < 1e-9, "{first:?}");
        assert!((last - Vec2::new(25.4, 50.8)).len() < 1e-9, "{last:?}");
    }

    /// The rule the issue settled for the range's two ends (#9333).
    #[test]
    fn a_range_gives_its_unit_only_from_lengths_and_plain_numbers_beside_them() {
        // A plain number beside a length is in the design's unit, on either side.
        for (design_len, unit) in [(None, 1.0), (Some(25.4), 25.4)] {
            let vals = with_a(100.0, design_len);
            let up = fit(&vals, &with_range_unit(eq(Some("t"), "t / 2", "1", "a"))).unwrap();
            assert!((up.points[0] - Vec2::new(unit, unit / 2.0)).len() < 1e-9, "{:?}", up.points[0]);
            assert!((*up.points.last().unwrap() - Vec2::new(100.0, 50.0)).len() < 1e-9);
            let down = fit(&vals, &with_range_unit(eq(Some("t"), "t / 2", "a", "1"))).unwrap();
            assert!((down.points[0] - Vec2::new(100.0, 50.0)).len() < 1e-9);
            assert!((*down.points.last().unwrap() - Vec2::new(unit, unit / 2.0)).len() < 1e-9);
        }
        // Nothing else: a length and an angle, and ends with no length to give.
        let vals = with_a(10.0, None);
        for (a, b) in [("a", "90 deg"), ("90 deg", "a"), ("0 deg", "90 deg"), ("0", "90 deg"), ("90 deg", "0"), ("0", "1")] {
            let why = fit(&vals, &with_range_unit(eq(Some("t"), "t", a, b))).unwrap_err().to_string();
            assert!(
                why.contains(
                    "with the variable in its range's unit, start and end must both be lengths, or one a length and the other a plain number"
                ),
                "{a}..{b}: {why}"
            );
        }
        // Off, the same ranges build, with the variable a plain number.
        assert!(fit(&vals, &eq(Some("t"), "t", "a", "90 deg")).is_ok());
    }

    /// Off (#9333): the variable is a plain number over a length range, as saved curves expect.
    #[test]
    fn without_the_flag_the_variable_stays_a_plain_number() {
        let vals = with_a(10.0, None);
        let plain = fit(&vals, &eq(Some("t"), "t * t / 20", "0 mm", "40 mm")).unwrap();
        let last = *plain.points.last().unwrap();
        assert!((last - Vec2::new(40.0, 80.0)).len() < 1e-9, "{last:?}");
        assert!(fit(&vals, &eq(Some("t"), "a ^ 2 / (4 * t)", "a", "3 * a")).unwrap_err().to_string().contains("length^2"));
        // On, that plain-number formula is a length squared, and is refused rather than misread.
        assert!(fit(&vals, &with_range_unit(eq(Some("t"), "t * t / 20", "0 mm", "40 mm"))).is_err());
    }

    #[test]
    fn hostile_formulas_with_the_variable_in_its_ranges_unit_are_refused() {
        let vals = with_a(10.0, Some(25.4));
        for (x, y, a, b) in [
            ("t", "a ^ 2 / (4 * t)", "0", "a"),
            ("t", "sqrt(t)", "a", "3 * a"),
            ("t", "t ^ 1000", "a", "3 * a"),
            ("t", "t", "a", "a"),
            ("t", "t", "0", "1e300 mm"),
            ("t", "t", "a", "1e308 in"),
            ("t", "t", "a", "nan"),
            ("t", "t", "a * a", "3 * a"),
            ("1e300 * t", "t", "a", "3 * a"),
            ("sin(t)", "t", "a", "3 * a"),
            ("t", "unknown_name * t", "a", "3 * a"),
        ] {
            assert!(fit(&vals, &with_range_unit(eq(Some(x), y, a, b))).is_err(), "{x} {y} {a}..{b}");
        }
    }
}
