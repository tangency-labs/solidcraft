//! Non-circular gears: a centre-pivoted elliptical gear and its conjugate.
//!
//! Gear 1's pitch curve is an ellipse turning about its centre. Its mate, also turning about its
//! own centre at distance A, must roll on it without slipping: r₁ + r₂ = A and dφ₂/dφ₁ = r₁/r₂.
//! A is found so that the mate closes after one turn (both have the same teeth), and the mate's
//! pitch curve comes out of the integral (it is very nearly an ellipse too).
//!
//! The teeth are generated as a hob would cut them: the basic rack rolls along each pitch curve
//! (its pitch line tangent to the curve, its length matching the arc length), and the gear keeps
//! what no position of the rack's teeth reaches, up to the tip curve (the pitch curve moved out
//! by the addendum). The result is sampled along rays from the pivot, so each tooth's outline is
//! a fine polyline; undercut flanks that fold back past a ray are approximated.

use std::f64::consts::{PI, TAU};

use crate::geom::{Outline, Seg, V2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipticPair {
    pub teeth: u32,
    pub module: f64,
    /// The ellipse's eccentricity (0 a circle).
    pub eccentricity: f64,
    /// Pressure angle (radians).
    pub pressure_angle: f64,
    /// Addendum and dedendum coefficients.
    pub addendum: f64,
    pub dedendum: f64,
    /// Circular backlash at the pitch curve, taken off each gear's teeth.
    pub backlash: f64,
}

/// The pair's pitch curves and numbers.
#[derive(Clone, Debug)]
pub struct Pitch {
    /// Gear 1's pitch radius at its body angle θ, sampled at θ = 2πi/n.
    pub r1: Vec<f64>,
    /// Gear 2's pitch curve in its body frame (counter-clockwise from its contact point at t = 0).
    pub curve2: Vec<V2>,
    /// Centre distance.
    pub center: f64,
    /// Semi-axes of gear 1's ellipse.
    pub a: f64,
    pub b: f64,
    /// Smallest and largest instantaneous ratio ω₂/ω₁.
    pub ratio_min: f64,
    pub ratio_max: f64,
}

const N: usize = 3600;

impl EllipticPair {
    fn ellipse(&self, b: f64, th: f64) -> f64 {
        let e = self.eccentricity;
        b / (1.0 - e * e * th.cos().powi(2)).max(1e-12).sqrt()
    }
    /// The pitch curves.
    pub fn pitch(&self) -> Pitch {
        let e = self.eccentricity.clamp(0.0, 0.95);
        // Scale so the perimeter holds the teeth at this module.
        let perim = |b: f64| -> f64 {
            let pts: Vec<V2> = (0..=N).map(|i| V2::polar(self.ellipse(b, TAU * i as f64 / N as f64), TAU * i as f64 / N as f64)).collect();
            pts.windows(2).map(|w| w[0].dist(w[1])).sum()
        };
        let want = PI * self.module * f64::from(self.teeth);
        let b = want / perim(1.0);
        let a = b / (1.0 - e * e).sqrt();
        let r1: Vec<f64> = (0..N).map(|i| self.ellipse(b, TAU * i as f64 / N as f64)).collect();
        // The mate's turn for one turn of gear 1 at centre distance `c` (trapezoid rule).
        let turn = |c: f64| -> f64 {
            let dt = TAU / N as f64;
            (0..N).map(|i| r1[i] / (c - r1[i])).sum::<f64>() * dt
        };
        let rmax = r1.iter().copied().fold(0.0, f64::max);
        let (mut lo, mut hi) = (rmax * 1.000001, rmax * 4.0);
        for _ in 0..100 {
            let mid = 0.5 * (lo + hi);
            if turn(mid) > TAU { lo = mid } else { hi = mid }
        }
        let c = 0.5 * (lo + hi);
        // Gear 2: contact at body angle π at t = 0; gear 1's contact point is at θ = −t.
        let dt = TAU / N as f64;
        let mut psi = PI;
        let mut curve2 = Vec::with_capacity(N);
        let (mut rmin_ratio, mut rmax_ratio) = (f64::MAX, 0.0f64);
        for i in 0..N {
            let r = r1[(N - i) % N];
            curve2.push(V2::polar(c - r, psi));
            let k = r / (c - r);
            rmin_ratio = rmin_ratio.min(k);
            rmax_ratio = rmax_ratio.max(k);
            let rn = r1[(N - i - 1) % N];
            psi += 0.5 * (k + rn / (c - rn)) * dt;
        }
        Pitch { r1, curve2, center: c, a, b, ratio_min: rmin_ratio, ratio_max: rmax_ratio }
    }
    /// Gear 1's pitch curve, counter-clockwise from θ = 0.
    pub fn curve1(p: &Pitch) -> Vec<V2> {
        p.r1.iter().enumerate().map(|(i, r)| V2::polar(*r, TAU * i as f64 / p.r1.len() as f64)).collect()
    }
    /// The teeth cut round a closed counter-clockwise pitch curve, the tooth spaces centred at arc
    /// lengths `first_space` + k·pitch from the curve's first point.
    pub fn teeth(&self, curve: &[V2], first_space: f64) -> Outline {
        let n = curve.len();
        let mut s = vec![0.0; n + 1];
        for i in 0..n {
            s[i + 1] = s[i] + curve[i].dist(curve[(i + 1) % n]);
        }
        let total = s[n];
        let z = f64::from(self.teeth.max(3));
        let p = total / z;
        let m = self.module;
        let (ha, hf) = (self.addendum * m, self.dedendum * m);
        let tan = self.pressure_angle.tan();
        // Rays from the pivot, and positions of the cutter, per tooth (fewer for many teeth, whose
        // flanks are nearly straight).
        let (rays_per_tooth, steps_per_tooth) = if self.teeth <= 80 { (48, 32) } else { (32, 16) };
        let rays = (self.teeth.max(3) as usize) * rays_per_tooth;
        let ray_angle = |k: usize| TAU * k as f64 / rays as f64;
        // The tip curve: the pitch radius along each ray plus the addendum.
        // The curve point whose polar angle is nearest (the curves are star-shaped).
        let mut by_angle: Vec<(f64, f64)> = curve.iter().map(|q| (q.angle().rem_euclid(TAU), q.len())).collect();
        by_angle.sort_by(|a, b| a.0.total_cmp(&b.0));
        let pitch_r = |ang: f64| -> f64 {
            let a = ang.rem_euclid(TAU);
            let i = by_angle.partition_point(|q| q.0 < a);
            let (lo, hi) = (by_angle[(i + n - 1) % n], by_angle[i % n]);
            let off = |q: (f64, f64)| {
                let d = (q.0 - a).rem_euclid(TAU);
                d.min(TAU - d)
            };
            if off(lo) < off(hi) { lo.1 } else { hi.1 }
        };
        let mut out: Vec<f64> = (0..rays).map(|k| pitch_r(ray_angle(k)) + ha).collect();
        // Points and frames along the curve.
        let at = |sv: f64| -> (V2, V2) {
            let sv = sv.rem_euclid(total);
            let i = match s.binary_search_by(|x| x.total_cmp(&sv)) {
                Ok(i) => i.min(n - 1),
                Err(i) => i.saturating_sub(1).min(n - 1),
            };
            let (q0, q1) = (curve[i], curve[(i + 1) % n]);
            let seg = (s[i + 1] - s[i]).max(1e-12);
            let t = (q1 - q0).norm();
            (q0.lerp(q1, (sv - s[i]) / seg), t)
        };
        let total_steps = self.teeth.max(3) as usize * steps_per_tooth;
        let backlash = self.backlash / 2.0;
        for j in 0..total_steps {
            let sc = total * j as f64 / total_steps as f64;
            let (c, t) = at(sc);
            let nrm = V2::new(-t.y, t.x); // inwards (left of a counter-clockwise curve)
            // Cutter teeth near the contact point.
            let k0 = ((sc - first_space) / p).round();
            for dk in -1..=1 {
                let sk = first_space + (k0 + f64::from(dk)) * p;
                let x0 = sk - sc;
                let half = |y: f64| p / 4.0 + backlash - y * tan;
                let ys = [-ha - 0.5 * m, hf];
                let quad = [
                    c + t * (x0 - half(ys[0])) + nrm * ys[0],
                    c + t * (x0 - half(ys[1])) + nrm * ys[1],
                    c + t * (x0 + half(ys[1])) + nrm * ys[1],
                    c + t * (x0 + half(ys[0])) + nrm * ys[0],
                ];
                // The rays the quad spans.
                let angs: Vec<f64> = quad.iter().map(|q| q.angle()).collect();
                let base = angs[0];
                let rel: Vec<f64> = angs.iter().map(|a| (a - base + PI).rem_euclid(TAU) - PI).collect();
                let (lo, hi) = rel.iter().fold((f64::MAX, f64::MIN), |(l, h), r| (l.min(*r), h.max(*r)));
                let k_lo = ((base + lo) / TAU * rays as f64).ceil() as i64;
                let k_hi = ((base + hi) / TAU * rays as f64).floor() as i64;
                for kr in k_lo..=k_hi {
                    let k = kr.rem_euclid(rays as i64) as usize;
                    let d = V2::polar(1.0, ray_angle(k));
                    // Nearest crossing of the ray with the quad's edges.
                    for e in 0..4 {
                        let (a, b) = (quad[e], quad[(e + 1) % 4]);
                        let ab = b - a;
                        let den = d.cross(ab);
                        if den.abs() < 1e-15 {
                            continue;
                        }
                        let rho = a.cross(ab) / den;
                        let u = a.cross(d) / den;
                        if (0.0..=1.0).contains(&u) && rho > 0.0 && rho < out[k] {
                            out[k] = rho;
                        }
                    }
                }
            }
        }
        let pts: Vec<V2> = (0..rays).map(|k| V2::polar(out[k], ray_angle(k))).collect();
        let kept = simplify_closed(&pts, 0.002 * m);
        let segs = (0..kept.len()).map(|i| Seg::Line(kept[i], kept[(i + 1) % kept.len()])).collect();
        Outline { segs }
    }
}

/// Drops points that lie within `tol` of the line through their neighbours (closed polyline).
fn simplify_closed(p: &[V2], tol: f64) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::with_capacity(p.len());
    let n = p.len();
    for i in 0..n {
        let prev = *out.last().unwrap_or(&p[(i + n - 1) % n]);
        let next = p[(i + 1) % n];
        let cur = p[i];
        let l = next - prev;
        let len = l.len();
        let off = if len > 1e-12 { (cur - prev).cross(l).abs() / len } else { 0.0 };
        if off > tol || i == 0 {
            out.push(cur);
        }
    }
    out
}
