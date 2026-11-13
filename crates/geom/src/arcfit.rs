//! Arc fitting: free-form segments (conics, cubic and higher Béziers) as chains of circular
//! arcs and lines within a tolerance, tangent-continuous inside each segment (biarcs).
//!
//! Code that works on lines and arcs alone (planar booleans, the folded sheet-metal solid)
//! keeps smooth curves this way with far fewer pieces than a chord polyline of the same
//! tolerance: a biarc's error falls with the cube of its length, a chord's with the square.

use crate::prelude::*;
use std::f64::consts::{PI, TAU};

use solidcore_record::{Loop2, Region2, Seg2, Vec2};

/// Most pieces one segment is fitted with.
pub const MAX_FIT_PIECES: usize = 2048;
/// Deepest subdivision of one segment.
const MAX_DEPTH: u32 = 30;
/// Samples inside each candidate span that must lie within the tolerance.
const SAMPLES: usize = 12;

/// Distance from `p` to a line or arc segment (other segments: to their closest point).
fn dist_to(s: &Seg2, p: Vec2) -> f64 {
    match *s {
        Seg2::Line { a, b } => {
            let d = b - a;
            let l2 = d.len2();
            let t = if l2 > 1e-300 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (a + d * t).dist(p)
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let ang = (p - center).angle();
            let delta = if sweep >= 0.0 { (ang - start).rem_euclid(TAU) } else { (start - ang).rem_euclid(TAU) };
            if delta <= sweep.abs() { (p.dist(center) - radius).abs() } else { p.dist(s.start()).min(p.dist(s.end())) }
        }
        _ => s.point_at(s.closest_param(p)).dist(p),
    }
}

/// The two arcs (or lines) from `p0` leaving along unit `d0` to `p1` arriving along unit `d1`,
/// meeting tangentially: the tangent lines from both ends are cut at one length `d`, and the
/// arcs join at the middle of the line between the two cut points (which is `2 d` long).
fn biarc(p0: Vec2, d0: Vec2, p1: Vec2, d1: Vec2) -> Option<Vec<Seg2>> {
    let v = p1 - p0;
    let vv = v.dot(v);
    if vv < 1e-24 {
        return None;
    }
    let len = vv.sqrt();
    let chord = v / len;
    if d0.cross(chord).abs() < 1e-12 && d1.cross(chord).abs() < 1e-12 && d0.dot(chord) > 0.0 && d1.dot(chord) > 0.0 {
        return Some(vec![Seg2::Line { a: p0, b: p1 }]);
    }
    // |v − d (d0 + d1)| = 2 d, the positive root.
    let t = d0 + d1;
    let vt = v.dot(t);
    let a = 2.0 * (1.0 - d0.dot(d1));
    let d = if a < 1e-12 {
        if vt <= 1e-12 * len {
            return None;
        }
        vv / (2.0 * vt)
    } else {
        (-vt + (vt * vt + a * vv).max(0.0).sqrt()) / a
    };
    if !(d.is_finite() && d > 0.0) {
        return None;
    }
    let j = ((p0 + d0 * d) + (p1 - d1 * d)) * 0.5;
    let mut out = Vec::with_capacity(2);
    if j.dist(p0) > 1e-12 {
        out.push(arc_from(p0, d0, j)?);
    }
    if j.dist(p1) > 1e-12 {
        out.push(arc_from(p1, -d1, j)?.reversed());
    }
    (!out.is_empty()).then_some(out)
}

/// The arc leaving `p` along unit `d` that ends at `q` (a line when `q` lies straight ahead).
fn arc_from(p: Vec2, d: Vec2, q: Vec2) -> Option<Seg2> {
    let c = q - p;
    let cc = c.dot(c);
    let n = d.perp();
    let h = n.dot(c);
    // Sagitta of the arc over the chord: a line when the bulge is negligible.
    if h.abs() * cc.sqrt() < 1e-12 * cc || (cc / (2.0 * h)).abs() > 1e7 {
        return (d.dot(c) > 0.0).then_some(Seg2::Line { a: p, b: q });
    }
    let r = cc / (2.0 * h);
    let center = p + n * r;
    let start = (p - center).angle();
    let mut sweep = (q - center).angle() - start;
    if r > 0.0 {
        while sweep <= 0.0 {
            sweep += TAU;
        }
    } else {
        while sweep >= 0.0 {
            sweep -= TAU;
        }
    }
    (sweep.is_finite() && sweep.abs() <= TAU).then_some(Seg2::Arc { center, radius: r.abs(), start, sweep })
}

/// `s` moved to start exactly at `p` (an arc keeps its centre; `p` is within a rounding error).
fn with_start(s: Seg2, p: Vec2) -> Seg2 {
    match s {
        Seg2::Line { b, .. } => Seg2::Line { a: p, b },
        other => other,
    }
}

/// `s` moved to end exactly at `p`.
fn with_end(s: Seg2, p: Vec2) -> Seg2 {
    match s {
        Seg2::Line { a, .. } => Seg2::Line { a, b: p },
        other => other,
    }
}

/// Arcs and lines fitted to a profile segment ([`Seg2`]).
pub trait Seg2ArcFit {
    /// A straight line or a circular arc (what [`Seg2::fit_arcs`] produces).
    fn is_line_or_arc(&self) -> bool;
    /// Lines and circular arcs that follow the segment within `tol` (mm), in order, the first
    /// starting at the segment's start and the last ending at its end; consecutive pieces share
    /// their end points and tangents. Lines and arcs come back unchanged. `None` when the
    /// segment is not finite or would need more than [`MAX_FIT_PIECES`] pieces.
    fn fit_arcs(&self, tol: f64) -> Option<Vec<Seg2>>;
}

impl Seg2ArcFit for Seg2 {
    fn is_line_or_arc(&self) -> bool {
        matches!(self, Seg2::Line { .. } | Seg2::Arc { .. })
    }
    fn fit_arcs(&self, tol: f64) -> Option<Vec<Seg2>> {
        if self.is_line_or_arc() {
            return Some(vec![*self]);
        }
        if !(tol.is_finite() && tol > 0.0) {
            return None;
        }
        let tol = tol.max(1e-9);
        let mut out: Vec<Seg2> = Vec::new();
        // Spans still to fit; the last pushed is fitted first, so pushing the right half before
        // the left one keeps the output in parameter order.
        let mut todo = vec![(0.0f64, 1.0f64, 0u32)];
        let mut steps = 0usize;
        while let Some((t0, t1, depth)) = todo.pop() {
            steps += 1;
            if steps > 4 * MAX_FIT_PIECES {
                return None;
            }
            let (p0, p1) = (self.point_at(t0), self.point_at(t1));
            if !(p0.is_finite() && p1.is_finite()) {
                return None;
            }
            if p0.dist(p1) < 1e-12 && self.span_extent(t0, t1) < tol {
                continue;
            }
            let fit = match (self.tangent_out(t0, t1), self.tangent_in(t0, t1)) {
                (Some(d0), Some(d1)) => biarc(p0, d0, p1, d1).filter(|arcs| self.within(arcs, t0, t1, tol)),
                _ => None,
            };
            match fit {
                Some(arcs) => {
                    out.extend(arcs);
                    if out.len() > MAX_FIT_PIECES {
                        return None;
                    }
                }
                None if depth < MAX_DEPTH => {
                    let tm = 0.5 * (t0 + t1);
                    todo.push((tm, t1, depth + 1));
                    todo.push((t0, tm, depth + 1));
                }
                None => {
                    // Too fine to split further (a cusp): the chord is within the tolerance
                    // here or nothing is.
                    let chord = [Seg2::Line { a: p0, b: p1 }];
                    if !self.within(&chord, t0, t1, tol) {
                        return None;
                    }
                    out.push(chord[0]);
                }
            }
        }
        // The pieces join where their spans meet; pin the outer ends to the segment's own.
        if let Some(first) = out.first_mut() {
            *first = with_start(*first, self.start());
        }
        if let Some(last) = out.last_mut() {
            *last = with_end(*last, self.end());
        }
        Some(out)
    }
}

/// What fitting a segment reads of a span of it.
trait Span {
    /// Unit direction of travel leaving `t` towards `t1`.
    fn tangent_out(&self, t: f64, t1: f64) -> Option<Vec2>;
    /// Unit direction of travel arriving at `t1` from `t0`.
    fn tangent_in(&self, t0: f64, t1: f64) -> Option<Vec2>;
    /// Largest distance of the span's sample points from its start (a loop's size).
    fn span_extent(&self, t0: f64, t1: f64) -> f64;
    /// Do `pieces` follow the span `t0..t1` within `tol` both ways?
    fn within(&self, pieces: &[Seg2], t0: f64, t1: f64, tol: f64) -> bool;
    /// Distance from `q` to the span `t0..t1` (closest sample, then Newton steps).
    fn dist_on_span(&self, q: Vec2, t0: f64, t1: f64, samples: &[Vec2]) -> f64;
}

impl Span for Seg2 {
    fn tangent_out(&self, t: f64, t1: f64) -> Option<Vec2> {
        self.derivative(t).normalized().or_else(|| (self.point_at(t + (t1 - t) * 1e-4) - self.point_at(t)).normalized())
    }
    fn tangent_in(&self, t0: f64, t1: f64) -> Option<Vec2> {
        self.derivative(t1).normalized().or_else(|| (self.point_at(t1) - self.point_at(t1 - (t1 - t0) * 1e-4)).normalized())
    }
    fn span_extent(&self, t0: f64, t1: f64) -> f64 {
        let p0 = self.point_at(t0);
        (1..=SAMPLES).map(|i| self.point_at(t0 + (t1 - t0) * i as f64 / (SAMPLES + 1) as f64).dist(p0)).fold(0.0, f64::max)
    }
    fn within(&self, pieces: &[Seg2], t0: f64, t1: f64, tol: f64) -> bool {
        if pieces.iter().any(|p| matches!(p, Seg2::Arc { sweep, .. } if sweep.abs() > PI)) {
            return false;
        }
        let samples: Vec<Vec2> = (0..=SAMPLES + 1).map(|i| self.point_at(t0 + (t1 - t0) * i as f64 / (SAMPLES + 1) as f64)).collect();
        // The curve near the pieces...
        if samples.iter().any(|p| pieces.iter().map(|s| dist_to(s, *p)).fold(f64::INFINITY, f64::min) > tol) {
            return false;
        }
        // ...and the pieces near the curve (their quarter points against the curve's closest
        // point on the span).
        pieces.iter().all(|s| {
            [0.25, 0.5, 0.75].iter().all(|f| {
                let q = s.point_at(*f);
                self.dist_on_span(q, t0, t1, &samples) <= tol
            })
        })
    }
    fn dist_on_span(&self, q: Vec2, t0: f64, t1: f64, samples: &[Vec2]) -> f64 {
        let n = samples.len().saturating_sub(1).max(1);
        let (k, mut best) = samples.iter().enumerate().map(|(i, p)| (i, p.dist(q))).fold((0, f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a });
        let mut t = t0 + (t1 - t0) * k as f64 / n as f64;
        for _ in 0..12 {
            let (p, d) = (self.point_at(t), self.derivative(t));
            let dd = d.dot(d);
            if !(dd > 1e-300) {
                break;
            }
            let nt = (t - (p - q).dot(d) / dd).clamp(t0, t1);
            best = best.min(self.point_at(nt).dist(q));
            if (nt - t).abs() < 1e-15 {
                break;
            }
            t = nt;
        }
        best
    }
}

/// Arcs and lines fitted to a profile loop ([`Loop2`]).
pub trait Loop2ArcFit {
    /// The loop with every free-form segment fitted by lines and arcs (see
    /// [`Seg2::fit_arcs`]). `None` when a segment cannot be fitted.
    fn fit_arcs(&self, tol: f64) -> Option<Loop2>;
    /// Only lines and arcs?
    fn is_line_arc(&self) -> bool;
}

impl Loop2ArcFit for Loop2 {
    fn fit_arcs(&self, tol: f64) -> Option<Loop2> {
        let mut segs = Vec::with_capacity(self.segs.len());
        for s in &self.segs {
            segs.extend(s.fit_arcs(tol)?);
            if segs.len() > MAX_FIT_PIECES * 8 {
                return None;
            }
        }
        Some(Loop2 { segs })
    }
    fn is_line_arc(&self) -> bool {
        self.segs.iter().all(Seg2::is_line_or_arc)
    }
}

/// Whether a profile region ([`Region2`]) is lines and arcs only.
pub trait Region2ArcFit {
    /// Only lines and arcs?
    fn is_line_arc(&self) -> bool;
}

impl Region2ArcFit for Region2 {
    fn is_line_arc(&self) -> bool {
        self.outer.is_line_arc() && self.holes.iter().all(Loop2::is_line_arc)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

    use super::*;

    /// Largest distance of 401 points of `seg` from `pieces`: 30 times as many as the fit
    /// itself looks at in a span.
    fn max_dev(seg: &Seg2, pieces: &[Seg2]) -> f64 {
        (0..=400)
            .map(|i| seg.point_at(i as f64 / 400.0))
            .map(|p| pieces.iter().map(|s| dist_to(s, p)).fold(f64::INFINITY, f64::min))
            .fold(0.0, f64::max)
    }

    fn chained(seg: &Seg2, pieces: &[Seg2]) {
        assert!(pieces.first().unwrap().start().dist(seg.start()) < 1e-9);
        assert!(pieces.last().unwrap().end().dist(seg.end()) < 1e-9);
        for w in pieces.windows(2) {
            assert!(w[0].end().dist(w[1].start()) < 1e-9, "{w:?}");
            // Tangent-continuous inside the segment (biarc joints and span joints).
            assert!(w[0].end_tangent().dot(w[1].start_tangent()) > 1.0 - 1e-6, "{w:?}");
        }
        assert!(pieces.iter().all(Seg2::is_line_or_arc));
    }

    fn ellipse(c: Vec2, a: f64, b: f64) -> Loop2 {
        let corners = [c + Vec2::new(a, 0.0), c + Vec2::new(0.0, b), c - Vec2::new(a, 0.0), c - Vec2::new(0.0, b)];
        Loop2 {
            segs: (0..4)
                .map(|i| Seg2::Conic { a: corners[i], apex: corners[i] + corners[(i + 1) % 4] - c, b: corners[(i + 1) % 4], w: FRAC_1_SQRT_2 })
                .collect(),
        }
    }

    #[test]
    fn a_circular_arc_given_as_a_conic_comes_back_as_arcs_of_its_circle() {
        // A quarter circle of radius 10 about (5, −3): the biarc through its ends and end
        // tangents lies on the circle itself.
        let c = Vec2::new(5.0, -3.0);
        let q = Seg2::Conic { a: c + Vec2::new(10.0, 0.0), apex: c + Vec2::new(10.0, 10.0), b: c + Vec2::new(0.0, 10.0), w: FRAC_1_SQRT_2 };
        let arcs = q.fit_arcs(1e-6).unwrap();
        assert_eq!(arcs.len(), 2, "{arcs:?}");
        let mut total = 0.0;
        for s in &arcs {
            let Seg2::Arc { center, radius, sweep, .. } = *s else { panic!("{s:?}") };
            assert!(center.dist(c) < 1e-9 && (radius - 10.0).abs() < 1e-9, "{s:?}");
            total += sweep;
        }
        assert!((total - FRAC_PI_2).abs() < 1e-9, "{total}");
        chained(&q, &arcs);
    }

    #[test]
    fn quarter_ellipse_fits_within_the_tolerance_with_few_arcs() {
        let q = ellipse(Vec2::new(200.0, 150.0), 30.0, 12.0).segs[0];
        for tol in [1e-2, 1e-3, 1e-4] {
            let arcs = q.fit_arcs(tol).unwrap();
            chained(&q, &arcs);
            assert!(max_dev(&q, &arcs) <= tol * 1.01, "tol {tol}: {}", max_dev(&q, &arcs));
            // Far fewer pieces than a chord polyline of the same tolerance.
            let chords = q.polyline(tol).len() - 1;
            assert!(arcs.len() * 2 < chords, "tol {tol}: {} arcs vs {chords} chords", arcs.len());
        }
        // The chords of 0.001 mm: √(2·√(30² + 12²) / 0.008) = 89.9, so 90.
        assert_eq!(q.polyline(1e-3).len() - 1, 90);
        assert!(q.fit_arcs(1e-3).unwrap().len() <= 32);
    }

    #[test]
    fn glyph_like_quadratics_cubics_and_s_curves_fit() {
        let segs = [
            Seg2::Conic { a: Vec2::new(0.0, 0.0), apex: Vec2::new(1.0, 2.0), b: Vec2::new(3.0, 0.5), w: 1.0 },
            Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(1.0, 3.0), p2: Vec2::new(2.0, -3.0), p3: Vec2::new(3.0, 0.0) },
            // A cusp-like cubic and one with a coincident control point.
            Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(4.0, 2.0), p2: Vec2::new(-1.0, 2.0), p3: Vec2::new(3.0, 0.0) },
            Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(0.0, 0.0), p2: Vec2::new(2.0, 1.0), p3: Vec2::new(3.0, 0.0) },
            Seg2::Conic { a: Vec2::new(0.0, 0.0), apex: Vec2::new(5.0, 5.0), b: Vec2::new(10.0, 0.0), w: 3.0 },
            crate::bezier(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(2.0, -1.0), Vec2::new(3.0, 2.0), Vec2::new(4.0, 0.0)]).unwrap(),
        ];
        for s in segs {
            let arcs = s.fit_arcs(1e-3).unwrap();
            assert!(max_dev(&s, &arcs) <= 1.01e-3, "{s:?}: {}", max_dev(&s, &arcs));
            assert!(arcs.first().unwrap().start().dist(s.start()) < 1e-9 && arcs.last().unwrap().end().dist(s.end()) < 1e-9);
            for w in arcs.windows(2) {
                assert!(w[0].end().dist(w[1].start()) < 1e-9);
            }
        }
        // A straight "curve" is one line; lines and arcs come back as they are.
        let flat = Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(1.0, 0.0), p2: Vec2::new(2.0, 0.0), p3: Vec2::new(3.0, 0.0) };
        assert_eq!(flat.fit_arcs(1e-3).unwrap(), vec![Seg2::Line { a: Vec2::ZERO, b: Vec2::new(3.0, 0.0) }]);
        let arc = Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: 0.0, sweep: 1.0 };
        assert_eq!(arc.fit_arcs(1e-3).unwrap(), vec![arc]);
        // Hostile input.
        let bad = Seg2::Conic { a: Vec2::new(f64::NAN, 0.0), apex: Vec2::ZERO, b: Vec2::new(1.0, 0.0), w: 1.0 };
        assert!(bad.fit_arcs(1e-3).is_none());
        assert!(segs[0].fit_arcs(0.0).is_none() && segs[0].fit_arcs(f64::NAN).is_none());
    }

    #[test]
    fn fitted_ellipse_loop_keeps_its_area() {
        let lp = ellipse(Vec2::ZERO, 15.0, 8.0);
        let fitted = lp.fit_arcs(1e-3).unwrap();
        assert!(fitted.is_closed(1e-9) && fitted.is_line_arc() && !lp.is_line_arc());
        // π·a·b, within the tolerance times the perimeter (74 mm).
        let exact = PI * 15.0 * 8.0;
        assert!((lp.signed_area() - exact).abs() < 1e-9, "{}", lp.signed_area());
        assert!((fitted.signed_area() - exact).abs() < 1e-3 * 74.0, "{} vs {exact}", fitted.signed_area());
        assert!(fitted.segs.len() * 2 < lp.polyline(1e-3).len(), "{} pieces", fitted.segs.len());
        // As a hole of a 40 × 20 plate.
        let plate = Loop2::polygon(&[Vec2::new(-20.0, -10.0), Vec2::new(20.0, -10.0), Vec2::new(20.0, 10.0), Vec2::new(-20.0, 10.0)]);
        let (free, arcs) = (Region2 { outer: plate.clone(), holes: vec![lp.reversed()] }, Region2 { outer: plate, holes: vec![fitted.reversed()] });
        assert!(arcs.is_line_arc() && !free.is_line_arc());
        assert!((free.area() - (800.0 - exact)).abs() < 1e-9 && (arcs.area() - (800.0 - exact)).abs() < 1e-3 * 74.0, "{}", arcs.area());
    }
}
