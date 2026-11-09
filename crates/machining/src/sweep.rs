//! Swept volumes of a flat end mill, intersected with rays. A sweep answers, for one ray, which
//! parts of it lie inside the tool at some moment of the move; the dexel stock subtracts those.
//!
//! The tool is a flat cylinder: radius R, cutting from the tip up the axis to the flute length L.
//! Linear moves use closed forms for rays parallel and perpendicular to the axis, and an envelope
//! over the move for other rays. Arcs in a plane perpendicular to the tool axis (G17) use exact
//! windows: the arc's distance to the ray and its height are sinusoids and linear functions of the
//! arc angle, so the parameter values where the tool can touch the ray are solved in closed form,
//! and the envelope runs only inside them.
//!
//! One tie rule throughout: the footprint and the height window are open sets, with an absolute
//! slack of [`EPS_MM`]. A ray exactly on the tool's boundary (a wall at exactly R, a floor at
//! exactly the tip) is not cut, and neither is a ray that only touches it.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::det_math;

/// Arcs with more extra turns than this are refused (a `Move` can come from any source).
pub const MAX_ARC_TURNS: u32 = 1000;

/// The one slack, in millimetres, of every strict comparison in this module.
pub const EPS_MM: f64 = 1e-9;

/// The ray-intersection interface the dexel stock is cut with.
pub trait Sweep {
    /// A box that contains the whole swept volume.
    fn bounds(&self) -> Aabb3;
    /// Appends to `out` the parts of the ray `o + t d` (`t` in `t_range`) inside the swept volume.
    /// The parts may overlap or come unsorted; `out` is not cleared.
    fn ray(&self, o: Vec3, d: Vec3, t_range: (f64, f64), out: &mut Vec<(f64, f64)>);
}

/// A flat end mill: radius, flute length (cutting from the tip up the axis) and the tool axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatTool {
    pub radius: f64,
    pub length: f64,
    pub axis: Vec3,
}

impl FlatTool {
    /// `None` unless the radius and length are positive and finite and the axis is not zero.
    pub fn new(radius: f64, length: f64, axis: Vec3) -> Option<FlatTool> {
        let axis = axis.normalized()?;
        (radius.is_finite() && radius > 0.0 && length.is_finite() && length > 0.0).then_some(FlatTool { radius, length, axis })
    }
}

/// `|d . axis|` within this of 1 is parallel, within it of 0 is perpendicular.
const AXIS_EPS: f64 = 1e-12;
/// Brent evaluation budget per minimisation.
const BRENT_EVALS: usize = 25;
/// Penalty for "no tool here" in the minimisations. Far outside any real coordinate.
const PENALTY: f64 = 1e15;
/// Stands in for an unbounded parameter range; far outside any stock.
const BIG: f64 = 1e12;

fn perp(v: Vec3, u: Vec3) -> Vec3 {
    v - u * v.dot(u)
}

/// The open interval of `t` where `|w + t v| < r - EPS_MM`. Lagrange form: the minimum squared
/// distance is `|w x v|^2 / |v|^2`, so a ray exactly tangent to the circle has a non-positive
/// discriminant and no interval, with no rounding in the decision.
fn chord_open(w: Vec3, v: Vec3, r: f64) -> Option<(f64, f64)> {
    let vv = v.len2();
    if vv < 1e-24 {
        return (w.len() < r - EPS_MM).then_some((-BIG, BIG));
    }
    let disc = vv * r * r - w.cross(v).len2();
    // r^2 - dist^2 > 2 r EPS  is  r - dist > EPS, to first order.
    if disc <= 2.0 * r * EPS_MM * vv {
        return None;
    }
    let c = -w.dot(v) / vv;
    let h = disc.sqrt() / vv;
    Some((c - h, c + h))
}

/// The removed interval of the ray `o + t d` for a flat cylinder with its tip at `p`, unclipped:
/// the open footprint chord intersected with the open height window `0 < a < length`.
fn fiber(o: Vec3, d: Vec3, p: Vec3, tool: &FlatTool) -> Option<(f64, f64)> {
    let u = tool.axis;
    let w = o - p;
    let alpha = w.dot(u);
    let beta = d.dot(u);
    let (mut lo, mut hi) = (-BIG, BIG);
    if beta.abs() < AXIS_EPS {
        if !(alpha > EPS_MM && alpha < tool.length - EPS_MM) {
            return None;
        }
    } else {
        let ta = -alpha / beta;
        let tb = (tool.length - alpha) / beta;
        lo = lo.max(ta.min(tb));
        hi = hi.min(ta.max(tb));
    }
    let (qlo, qhi) = chord_open(perp(w, u), perp(d, u), tool.radius)?;
    lo = lo.max(qlo);
    hi = hi.min(qhi);
    (lo <= hi).then_some((lo, hi))
}

/// Interval of `t` where `a0 + t a1` lies strictly inside `(lo, hi)`. With `a1` zero the answer is
/// all or nothing, decided with the slack, so a ray exactly on the strip's edge is not cut.
fn linear_window(a0: f64, a1: f64, lo: f64, hi: f64) -> Option<(f64, f64)> {
    if a1.abs() < AXIS_EPS {
        return (a0 > lo + EPS_MM && a0 < hi - EPS_MM).then_some((-BIG, BIG));
    }
    let t1 = (lo - a0) / a1;
    let t2 = (hi - a0) / a1;
    Some((t1.min(t2), t1.max(t2)))
}

/// The chord of a stadium (segment `q0 q1` swept by a disc of radius `r`, all in the plane
/// perpendicular to `u`) along the line `o + t d`. The stadium is convex, so the union of its
/// three pieces (two discs and the strip between them) is one interval.
fn stadium_chord(o: Vec3, d: Vec3, q0: Vec3, q1: Vec3, r: f64, u: Vec3) -> Option<(f64, f64)> {
    let mut pieces: Vec<(f64, f64)> = Vec::with_capacity(3);
    for q in [q0, q1] {
        pieces.extend(chord_open(o - q, d, r));
    }
    let seg = q1 - q0;
    let len = seg.len();
    if len > AXIS_EPS {
        let f = seg * (1.0 / len);
        let n = u.cross(f);
        let w0 = o - q0;
        if let (Some(a), Some(b)) = (linear_window(w0.dot(f), d.dot(f), 0.0, len), linear_window(w0.dot(n), d.dot(n), -r, r)) {
            let lo = a.0.max(b.0);
            let hi = a.1.min(b.1);
            if lo < hi {
                pieces.push((lo, hi));
            }
        }
    }
    let lo = pieces.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let hi = pieces.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    (lo <= hi).then_some((lo, hi))
}

/// Brent's minimisation of `f` on `[ax, bx]`: returns the abscissa and value of the best point
/// found within `max_eval` evaluations, to tolerance `tol`.
fn brent_min(f: &dyn Fn(f64) -> f64, ax: f64, bx: f64, tol: f64, max_eval: usize) -> (f64, f64) {
    const CGOLD: f64 = 0.381_966_011_250_105;
    const ZEPS: f64 = 1e-12;
    let (mut a, mut b) = if ax <= bx { (ax, bx) } else { (bx, ax) };
    let mut x = a + CGOLD * (b - a);
    let mut w = x;
    let mut v = x;
    let mut fx = f(x);
    let mut fw = fx;
    let mut fv = fx;
    let mut d = 0.0f64;
    let mut e = 0.0f64;
    let mut evals = 1usize;
    while evals < max_eval {
        let xm = 0.5 * (a + b);
        let tol1 = tol + ZEPS;
        let tol2 = 2.0 * tol1;
        if (x - xm).abs() <= tol2 - 0.5 * (b - a) {
            break;
        }
        let mut golden = true;
        if e.abs() > tol1 {
            let r = (x - w) * (fx - fv);
            let mut q = (x - v) * (fx - fw);
            let mut p = (x - v) * q - (x - w) * r;
            q = 2.0 * (q - r);
            if q > 0.0 {
                p = -p;
            }
            q = q.abs();
            let etemp = e;
            e = d;
            if !(p.abs() >= (0.5 * q * etemp).abs() || p <= q * (a - x) || p >= q * (b - x)) {
                d = p / q;
                let u = x + d;
                if u - a < tol2 || b - u < tol2 {
                    d = if xm - x >= 0.0 { tol1 } else { -tol1 };
                }
                golden = false;
            }
        }
        if golden {
            e = if x >= xm { a - x } else { b - x };
            d = CGOLD * e;
        }
        let u = if d.abs() >= tol1 {
            x + d
        } else if d >= 0.0 {
            x + tol1
        } else {
            x - tol1
        };
        let fu = f(u);
        evals += 1;
        if fu <= fx {
            if u >= x {
                a = x;
            } else {
                b = x;
            }
            v = w;
            fv = fw;
            w = x;
            fw = fx;
            x = u;
            fx = fu;
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                v = w;
                fv = fw;
                w = u;
                fw = fu;
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
            }
        }
    }
    (x, fx)
}

/// Extremes over a parameter range of the removed interval `(lo, hi)` at each parameter value:
/// the least `lo` and the greatest `hi`. Samples first, then Brent around the best sample on each
/// side. A `None` fiber means the tool is not on the ray at that parameter.
///
/// shortcut: this sampled envelope is the general path, used for oblique rays and for arcs outside
/// the XY plane. Its samples can miss a window narrower than one step, so a full crossing of a thin
/// feature on an oblique path can be missed, not only a grazing sliver. It costs up to 16 plus
/// 2 x 25 evaluations. NOT NOW: replace it with closed forms for oblique rays when 5-axis needs them.
fn envelope(fib: &dyn Fn(f64) -> Option<(f64, f64)>, a: f64, b: f64, samples: usize, tol: f64) -> Option<(f64, f64)> {
    let x_of = |k: usize| a + (b - a) * (k as f64) / (samples as f64);
    let mut best_lo: Option<(usize, f64)> = None;
    let mut best_hi: Option<(usize, f64)> = None;
    for k in 0..=samples {
        if let Some((l, h)) = fib(x_of(k)) {
            if best_lo.is_none_or(|(_, v)| l < v) {
                best_lo = Some((k, l));
            }
            if best_hi.is_none_or(|(_, v)| h > v) {
                best_hi = Some((k, h));
            }
        }
    }
    let (kl, vl) = best_lo?;
    let (kh, vh) = best_hi?;
    let bracket = |k: usize| (x_of(k.saturating_sub(1)), x_of((k + 1).min(samples)));
    let (pl, ql) = bracket(kl);
    let (_, lo_ref) = brent_min(&|x| fib(x).map_or(PENALTY, |(l, _)| l), pl, ql, tol, BRENT_EVALS);
    let (ph, qh) = bracket(kh);
    let (_, neg_hi_ref) = brent_min(&|x| fib(x).map_or(PENALTY, |(_, h)| -h), ph, qh, tol, BRENT_EVALS);
    let lo = if lo_ref < vl { lo_ref } else { vl };
    let hi = if -neg_hi_ref > vh { -neg_hi_ref } else { vh };
    (lo <= hi).then_some((lo, hi))
}

fn swept_box(base: Aabb3, tool: &FlatTool) -> Aabb3 {
    let shifted = Aabb3 { min: base.min + tool.axis * tool.length, max: base.max + tool.axis * tool.length };
    base.union(&shifted).inflated(tool.radius)
}

/// Clips the removed interval to the ray's range and pushes it if any length is left.
fn push_clipped(found: Option<(f64, f64)>, t_range: (f64, f64), out: &mut Vec<(f64, f64)>) {
    if let Some((a, b)) = found {
        let (lo, hi) = (a.max(t_range.0), b.min(t_range.1));
        if lo < hi {
            out.push((lo, hi));
        }
    }
}

/// A flat end mill moving in a straight line from `from` to `to`, its axis fixed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearFixedAxis {
    pub from: Vec3,
    pub to: Vec3,
    pub tool: FlatTool,
}

impl LinearFixedAxis {
    /// Ray parallel to the axis: the footprint is over the ray for an open interval of s (a
    /// quadratic in s), and the cut is the height window at the ends of that interval.
    fn parallel(&self, o: Vec3, d: Vec3) -> Option<(f64, f64)> {
        let u = self.tool.axis;
        let path = self.to - self.from;
        let g = perp(o - self.from, u);
        let (sa, sb) = chord_open(g, -perp(path, u), self.tool.radius)?;
        let (s0, s1) = (sa.max(0.0), sb.min(1.0));
        // A window touching [0, 1] only at one end is a tangent contact, not a cut.
        if s1 - s0 <= 1e-12 {
            return None;
        }
        // The ray runs along the axis, so the height along it changes by +-1 per unit t.
        let beta = d.dot(u);
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for s in [s0, s1] {
            let alpha = (o - (self.from + path * s)).dot(u);
            let ta = -alpha / beta;
            let tb = (self.tool.length - alpha) / beta;
            lo = lo.min(ta.min(tb));
            hi = hi.max(ta.max(tb));
        }
        (lo <= hi).then_some((lo, hi))
    }

    /// Ray perpendicular to the axis: it sits at one height, so the s values that put the tip
    /// window over it form an interval, and the footprint is the stadium of that segment.
    fn perpendicular(&self, o: Vec3, d: Vec3) -> Option<(f64, f64)> {
        let u = self.tool.axis;
        let len = self.tool.length;
        let path = self.to - self.from;
        let alpha0 = (o - self.from).dot(u);
        let delta = path.dot(u);
        let (s0, s1) = if delta.abs() < AXIS_EPS {
            if !(alpha0 > EPS_MM && alpha0 < len - EPS_MM) {
                return None;
            }
            (0.0, 1.0)
        } else {
            // The s values where the tip window is strictly inside the tool, with the same slack as above.
            let sa = (alpha0 - EPS_MM) / delta;
            let sb = (alpha0 - len + EPS_MM) / delta;
            (sa.min(sb).max(0.0), sa.max(sb).min(1.0))
        };
        // A plunge or ramp that ends exactly at the ray's height gives an empty s range: no cut.
        if s0 >= s1 {
            return None;
        }
        let q0 = perp(self.from + path * s0, u);
        let q1 = perp(self.from + path * s1, u);
        stadium_chord(perp(o, u), d, q0, q1, self.tool.radius, u)
    }

    /// Any other ray: an envelope over the move of the closed-form cylinder fibers.
    fn general(&self, o: Vec3, d: Vec3) -> Option<(f64, f64)> {
        let path = self.to - self.from;
        let fib = |s: f64| fiber(o, d, self.from + path * s, &self.tool);
        envelope(&fib, 0.0, 1.0, 16, 1e-6 / path.len().max(1e-9))
    }
}

impl Sweep for LinearFixedAxis {
    fn bounds(&self) -> Aabb3 {
        let base: Aabb3 = [self.from, self.to].into_iter().collect();
        swept_box(base, &self.tool)
    }

    fn ray(&self, o: Vec3, d: Vec3, t_range: (f64, f64), out: &mut Vec<(f64, f64)>) {
        let beta = d.dot(self.tool.axis).abs();
        let found = if beta > 1.0 - AXIS_EPS {
            self.parallel(o, d)
        } else if beta < AXIS_EPS {
            self.perpendicular(o, d)
        } else {
            self.general(o, d)
        };
        push_clipped(found, t_range, out);
    }
}

/// `base + slope * u + amp * cos(sign * u - phi)` as a function of the arc parameter `u`: the
/// shape of every coordinate and distance of a tip on an arc in a plane perpendicular to the tool
/// axis. Its level sets are solved in closed form when either `slope` or `amp` is zero.
#[derive(Clone, Copy, Debug)]
struct Form {
    base: f64,
    slope: f64,
    amp: f64,
    phi: f64,
}

/// A form that solves in closed form: a pure line or a pure sinusoid.
fn solvable(form: &Form) -> bool {
    form.amp == 0.0 || form.slope == 0.0
}

impl Form {
    fn value(&self, sign: f64, u: f64) -> f64 {
        self.base + self.slope * u + self.amp * det_math::cos(sign * u - self.phi)
    }

    /// The parameter values in `[a, b]` where the form equals `level`, appended to `out`.
    fn roots(&self, sign: f64, level: f64, a: f64, b: f64, out: &mut Vec<f64>) {
        if self.amp == 0.0 {
            if self.slope != 0.0 {
                let u = (level - self.base) / self.slope;
                if u >= a && u <= b {
                    out.push(u);
                }
            }
            return;
        }
        let c = (level - self.base) / self.amp;
        if c.abs() > 1.0 || self.slope != 0.0 {
            return;
        }
        let alpha = det_math::acos(c);
        for ang in [alpha, -alpha] {
            // sign * u - phi = +-alpha + 2 pi m, so u = sign * (phi +- alpha) modulo a turn.
            let u0 = (sign * (self.phi + ang)).rem_euclid(TAU);
            let mut u = u0 + TAU * ((a - u0) / TAU).ceil().max(0.0);
            if u < a {
                u += TAU;
            }
            while u <= b {
                out.push(u);
                u += TAU;
            }
        }
    }
}

/// A flat end mill on a circular or helical arc: centre, plane normal, sweep and lift. The tool
/// axis is fixed; the arc's plane may be any plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcFixedAxis {
    /// Arc centre at this piece's start height.
    centre: Vec3,
    e1: Vec3,
    e2: Vec3,
    /// Angle of this piece's start, from `e1`.
    theta0: f64,
    r: f64,
    /// Signed sweep, radians: positive is counter-clockwise about the normal.
    sweep: f64,
    /// Change along the normal over this piece.
    lift: Vec3,
    tool: FlatTool,
    /// The arc plane is perpendicular to the tool axis (the common G17 case).
    axis_normal: bool,
    normal: Vec3,
}

impl ArcFixedAxis {
    /// The arc from `start` to `end` about `centre` in the plane with `normal`, counter-clockwise
    /// when `ccw`, plus `turns` extra full revolutions (at most [`MAX_ARC_TURNS`]). `None` for a
    /// degenerate arc or too many turns.
    pub fn new(start: Vec3, end: Vec3, centre: Vec3, normal: Vec3, ccw: bool, turns: u32, tool: FlatTool) -> Option<ArcFixedAxis> {
        if turns > MAX_ARC_TURNS {
            return None;
        }
        let n = normal.normalized()?;
        let to_start = start - centre;
        let height = to_start.dot(n);
        let centre = centre + n * height;
        let s_in = to_start - n * height;
        let r = s_in.len();
        if !(r.is_finite() && r > 1e-9) {
            return None;
        }
        let e1 = s_in * (1.0 / r);
        let e2 = n.cross(e1);
        let ev = end - centre;
        let a1 = det_math::atan2(ev.dot(e2), ev.dot(e1));
        let base = if ccw {
            if a1 <= 1e-12 { a1 + TAU } else { a1 }
        } else if a1 >= -1e-12 {
            a1 - TAU
        } else {
            a1
        };
        let extra = TAU * f64::from(turns);
        let sweep = if ccw { base + extra } else { base - extra };
        let lift = n * (end - start).dot(n);
        Some(ArcFixedAxis { centre, e1, e2, theta0: 0.0, r, sweep, lift, tool, axis_normal: n.dot(tool.axis).abs() > 1.0 - AXIS_EPS, normal: n })
    }

    /// The arc's length, mm, its helix length: a lift along the normal counts.
    pub fn length(&self) -> f64 {
        let along = self.r * self.sweep.abs();
        (along * along + self.lift.len2()).sqrt()
    }

    /// The tool tip after the fraction `f` (0 at the start, 1 at the end) of the arc. The helix runs
    /// at a steady rate, so the fraction of the angle is the fraction of the length.
    pub fn point(&self, f: f64) -> Vec3 {
        self.tip(f.clamp(0.0, 1.0) * self.sweep.abs())
    }

    /// The arc cut into pieces of at most one revolution each, with the same union. The sim applies
    /// them one after another under one move tag, so a long arc can be pumped between revolutions.
    pub fn revolutions(&self) -> Vec<ArcFixedAxis> {
        self.pieces(TAU)
    }

    /// The arc cut into equal pieces, the fewest of at most `max` radians each, in order, with the same
    /// union. Equal pieces make a step a fixed share of the arc, so a time fraction maps onto steps.
    /// The pieces share one circle and one lift per radian, so the pieces are exact parts of the arc.
    pub fn pieces(&self, max: f64) -> Vec<ArcFixedAxis> {
        let total = self.sweep.abs();
        if !(total > 0.0 && max > 0.0) {
            return Vec::new();
        }
        let sign = self.sweep.signum();
        let count = (total / max).ceil().max(1.0) as usize;
        let len = total / count as f64;
        (0..count)
            .map(|k| {
                let start_u = k as f64 * len;
                ArcFixedAxis {
                    centre: self.centre + self.lift * (start_u / total),
                    theta0: self.theta0 + sign * start_u,
                    sweep: sign * len,
                    lift: self.lift * (len / total),
                    ..*self
                }
            })
            .collect()
    }

    /// The tool tip at arc parameter `u` (radians swept so far, `0..=|sweep|`).
    fn tip(&self, u: f64) -> Vec3 {
        let total = self.sweep.abs();
        let s = if total > 0.0 { u / total } else { 0.0 };
        let (sn, cs) = det_math::sin_cos(self.theta0 + self.sweep.signum() * u);
        self.centre + self.lift * s + (self.e1 * cs + self.e2 * sn) * self.r
    }

    /// The form of `w . tip(u)` for a vector `w` perpendicular to the arc normal.
    fn coordinate_form(&self, w: Vec3) -> Form {
        let total = self.sweep.abs().max(1e-300);
        let wp = (w.dot(self.e1), w.dot(self.e2));
        let amp = self.r * (wp.0 * wp.0 + wp.1 * wp.1).sqrt();
        let phi = det_math::atan2(wp.1, wp.0) - self.theta0;
        let slope = w.dot(self.lift) / total;
        // Rounding leaves a tiny amplitude in a coordinate with no in-plane part: that is none.
        let amp = if amp < 1e-12 { 0.0 } else { amp };
        Form { base: w.dot(self.centre), slope: if slope.abs() < 1e-12 { 0.0 } else { slope }, amp, phi }
    }

    /// The form of the squared distance from `q` to the tip, in the plane perpendicular to the axis.
    fn distance_sq_form(&self, q: Vec3) -> Form {
        let v = perp(self.centre - q, self.tool.axis);
        let vp = (v.dot(self.e1), v.dot(self.e2));
        Form {
            base: v.len2() + self.r * self.r,
            slope: 0.0,
            amp: 2.0 * self.r * (vp.0 * vp.0 + vp.1 * vp.1).sqrt(),
            phi: det_math::atan2(vp.1, vp.0) - self.theta0,
        }
    }

    /// True when no point of the arc is within the tool radius of the ray. Only decided for an arc
    /// in a plane perpendicular to the tool axis, where the distances are exact.
    fn misses(&self, o: Vec3, d: Vec3) -> bool {
        if !self.axis_normal {
            return false;
        }
        let along = d.dot(self.tool.axis).abs();
        let rt = self.tool.radius;
        if along > 1.0 - AXIS_EPS {
            // Distances from the ray's foot to the circle lie in [|dc - r|, dc + r].
            let dc = perp(self.centre - o, self.normal).len();
            (dc - self.r).abs() >= rt
        } else if along < AXIS_EPS {
            // Signed distances from the ray's line to the circle lie in [dc - r, dc + r].
            let dc = self.normal.cross(d).dot(self.centre - o);
            dc - self.r >= rt || dc + self.r <= -rt
        } else {
            false
        }
    }

    /// The constraints `lo < form < hi` that a tool must meet to cut the ray, as exact forms. Only
    /// the forms that solve in closed form are used; an arc or ray with none falls back to sampling.
    fn constraints(&self, o: Vec3, d: Vec3) -> Vec<(Form, f64, f64)> {
        let axis = self.tool.axis;
        let r = self.tool.radius;
        let along = d.dot(axis).abs();
        let mut out = Vec::new();
        if along > 1.0 - AXIS_EPS {
            if self.axis_normal {
                // Parallel, in the XY plane: the footprint, a disc of radius R about the ray's foot.
                out.push((self.distance_sq_form(o), f64::NEG_INFINITY, r * r));
            } else if self.normal.dot(axis).abs() < AXIS_EPS && self.lift.len() < 1e-12 {
                // Parallel, in a plane that holds the axis, with no lift: the arc's projection is a
                // segment, so the footprint is a band across it, of half-width sqrt(R^2 - dn^2).
                let lateral = self.normal.cross(axis);
                let dn = self.normal.dot(self.centre - o);
                let rad2 = r * r - dn * dn;
                if rad2 <= 0.0 {
                    // The arc never comes within R of the ray: an empty band.
                    out.push((Form { base: 0.0, slope: 0.0, amp: 0.0, phi: 0.0 }, 1.0, 0.0));
                } else {
                    let hw = rad2.sqrt();
                    let c = lateral.dot(o);
                    out.push((self.coordinate_form(lateral), c - hw, c + hw));
                }
            }
        } else if along < AXIS_EPS {
            // Perpendicular: within R of the ray's line (its lateral direction is axis x d, which lies
            // in every plane perpendicular to the ray), and the ray at a height inside the tool.
            let w = axis.cross(d);
            let c = w.dot(o);
            let f = self.coordinate_form(w);
            if solvable(&f) {
                out.push((f, c - r, c + r));
            }
            let z0 = o.dot(axis);
            let h = self.coordinate_form(axis);
            if solvable(&h) {
                out.push((h, z0 - self.tool.length, z0));
            }
        }
        out
    }
}

impl Sweep for ArcFixedAxis {
    fn bounds(&self) -> Aabb3 {
        let ext = Vec3::new(
            self.r * (self.e1.x * self.e1.x + self.e2.x * self.e2.x).sqrt(),
            self.r * (self.e1.y * self.e1.y + self.e2.y * self.e2.y).sqrt(),
            self.r * (self.e1.z * self.e1.z + self.e2.z * self.e2.z).sqrt(),
        );
        let low: Aabb3 = [self.centre - ext, self.centre + ext].into_iter().collect();
        let high: Aabb3 = [self.centre + self.lift - ext, self.centre + self.lift + ext].into_iter().collect();
        swept_box(low.union(&high), &self.tool)
    }

    fn ray(&self, o: Vec3, d: Vec3, t_range: (f64, f64), out: &mut Vec<(f64, f64)>) {
        let total = self.sweep.abs();
        if !(total > 1e-12) || self.misses(o, d) {
            return;
        }
        // Pieces of at most a quarter turn, cut where each world coordinate of the tip peaks, so
        // the envelope's pieces are monotone in every coordinate.
        let mut pts = vec![0.0, total];
        let mut k = 1.0f64;
        while k * FRAC_PI_2 < total {
            pts.push(k * FRAC_PI_2);
            k += 1.0;
        }
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.feet(axis, total, &mut pts);
        }
        if self.axis_normal {
            let along = d.dot(self.tool.axis).abs();
            if along > 1.0 - AXIS_EPS {
                self.feet(perp(self.centre - o, self.normal), total, &mut pts);
            } else if along < AXIS_EPS {
                self.feet(self.normal.cross(d), total, &mut pts);
            }
        }
        pts.sort_by(|a, b| a.total_cmp(b));
        pts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let constraints = self.constraints(o, d);
        let sign = self.sweep.signum();
        let tol = 1e-6 / self.r.max(1e-3);
        let fib = |u: f64| fiber(o, d, self.tip(u), &self.tool);
        let mut roots: Vec<f64> = Vec::new();
        for pair in pts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            // The windows are exact: the breakpoints are every root of every constraint in the piece,
            // and each sub-interval between them is inside or outside by its midpoint.
            roots.clear();
            for (f, lo, hi) in &constraints {
                if lo.is_finite() {
                    f.roots(sign, *lo, a, b, &mut roots);
                }
                if hi.is_finite() {
                    f.roots(sign, *hi, a, b, &mut roots);
                }
            }
            roots.push(a);
            roots.push(b);
            roots.sort_by(|x, y| x.total_cmp(y));
            roots.dedup_by(|x, y| (*x - *y).abs() < 1e-15);
            for seg in roots.windows(2) {
                let (p, q) = (seg[0], seg[1]);
                let mid = 0.5 * (p + q);
                let inside = constraints.iter().all(|(f, lo, hi)| {
                    let g = f.value(sign, mid);
                    g > *lo && g < *hi
                });
                if inside && let Some(iv) = envelope(&fib, p, q, 8, tol) {
                    push_clipped(Some(iv), t_range, out);
                }
            }
        }
    }
}

impl ArcFixedAxis {
    /// Arc parameters at which the in-plane offset along `w` peaks or troughs: every turn of both
    /// extremes, kept inside `0..total`.
    fn feet(&self, w: Vec3, total: f64, pts: &mut Vec<f64>) {
        let theta = det_math::atan2(w.dot(self.e2), w.dot(self.e1)) - self.theta0;
        for th in [theta, theta + PI] {
            let mut u = (self.sweep.signum() * th).rem_euclid(TAU);
            while u < total {
                if u > 0.0 {
                    pts.push(u);
                }
                u += TAU;
            }
        }
    }
}

#[cfg(test)]
#[path = "sweep_tests.rs"]
mod tests;
