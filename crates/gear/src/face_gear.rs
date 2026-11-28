//! Face (crown) gears meshing with an involute spur pinion at 90°, axes intersecting.
//!
//! The face gear is cut by the pinion (a shaper of the same size, its teeth taller by the tip
//! clearance so the pinion's tips clear the face gear's roots). In the plane square to a radius R
//! of the face gear, the face gear moves past the pinion like a rack at speed ω₂·R while the
//! pinion turns about its own axis, so the tooth section at R is what the rolling cutter leaves:
//! it rolls on the working pitch radius r_w = R·z₁/z₂ (this ignores the face gear's turning
//! across one tooth, a small effect for z₂ ≫ 1). Where the cutter's involute does the cutting
//! the flank is the straight side of a rack of pressure angle α_w = acos(r_b/r_w); the cutter's
//! tip corners leave the root fillet, and towards the inner end its root fillets take the top of
//! the flank away. The section is found by rolling the cutter's outline through the mesh and
//! keeping what it never reaches, at a few heights; the teeth are the loft through those heights.
//!
//! - Undercut (inner limit): further in than where the line of action at the teeth's top
//!   touches the pinion's base circle (r_w = r_b²/(r₁ − top)), the pinion has no involute left to
//!   meet the top of the flank, which is cut away.
//! - Pointing (outer limit): α_w grows with R, so the tooth narrows to a point at its top.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::geom::{Outline, Seg, V2};
use crate::involute::{BasicRack, Involute, ToothFlank, external_flank, teeth_outline, tooth_half_sampler};

#[derive(Clone, Debug, PartialEq)]
pub struct FaceGear {
    /// The pinion (spur, the module, pressure angle and basic rack of the set).
    pub pinion: Involute,
    /// Face gear teeth.
    pub teeth: u32,
    /// Circular backlash taken off the face gear's teeth.
    pub backlash: f64,
    /// The cutter's outline about the pinion's axis, from the bottom of the tooth space that
    /// points down (−y) up the side of the tooth to its right, to the middle of that tooth's tip.
    cutter: Vec<V2>,
    /// How far either way the cutter turns while that tooth is within reach of the face gear.
    sweep: f64,
}

impl FaceGear {
    pub fn new(pinion: Involute, teeth: u32, backlash: f64) -> FaceGear {
        let shaper = Involute { rack: BasicRack { addendum: pinion.rack.dedendum, ..pinion.rack }, backlash: 0.0, tip_shortening: 0.0, ..pinion };
        let f = external_flank(&shaper);
        let half = tooth_half_sampler(&shaper);
        let z1 = pinion.z();
        let centre = -FRAC_PI_2 + PI / z1;
        let mut cutter = Vec::new();
        for i in 0..4 {
            cutter.push(V2::polar(f.rf, -FRAC_PI_2 + (PI / z1 - f.root_half) * f64::from(i) / 4.0));
        }
        let n = 120;
        for i in 0..=n {
            let rr = f.rf + (f.ra - f.rf) * (f64::from(i) / f64::from(n)) * (1.0 - 1e-12);
            let h = if i == 0 { Some(f.root_half) } else { half(rr) };
            if let Some(h) = h.filter(|h| h.is_finite()) {
                cutter.push(V2::polar(rr, centre - h.max(0.0)));
            }
        }
        for i in 0..=3 {
            cutter.push(V2::polar(f.ra, centre - f.tip_half * (1.0 - f64::from(i) / 3.0)));
        }
        cutter.dedup_by(|a, b| a.dist(*b) < 1e-12);
        let top = pinion.rack.addendum * pinion.module;
        let sweep = ((pinion.r() - top) / f.ra.max(1e-12)).clamp(-1.0, 1.0).acos() + PI / z1;
        FaceGear { pinion, teeth, backlash, cutter, sweep }
    }
    fn m(&self) -> f64 {
        self.pinion.module
    }
    pub fn z2(&self) -> f64 {
        f64::from(self.teeth)
    }
    pub fn z1(&self) -> f64 {
        self.pinion.z()
    }
    /// The radius where the face gear's pitch matches the pinion's (m·z₂/2).
    pub fn pitch_radius(&self) -> f64 {
        self.m() * self.z2() / 2.0
    }
    /// The face teeth's top and the tooth spaces' bottom, as heights above the face gear's pitch
    /// plane (the pinion's axis is r₁ above it): clear of the pinion's root and tip by the rack's
    /// clearance.
    pub fn top(&self) -> f64 {
        self.pinion.rack.addendum * self.m()
    }
    pub fn bottom(&self) -> f64 {
        -self.pinion.rack.dedendum * self.m()
    }
    /// The heights the teeth are cut at, from just above the spaces' bottom to the top.
    pub fn heights(&self) -> Vec<f64> {
        let (b, t) = (self.bottom() + 0.03 * self.m(), self.top());
        let n = 4;
        (0..=n).map(|i| b + (t - b) * f64::from(i) / f64::from(n)).collect()
    }
    /// Half the face tooth's thickness (tangential) at radius `r` and each of the `heights`
    /// (rising): what the cutter leaves either side of the tooth's middle, less the backlash. A
    /// height the cutter doesn't reach takes the width above it.
    pub fn half_widths(&self, r: f64, heights: &[f64]) -> Vec<f64> {
        let rw = r * self.z1() / self.z2();
        let r1 = self.pinion.r();
        let ys: Vec<f64> = heights.iter().map(|h| h - r1).collect();
        let steps = 160;
        // The cutter's nearest crossing of each height at each angle, then the least over the
        // angles (sharpened by a parabola through the least and its neighbours).
        let mut at_angle = vec![f64::INFINITY; ys.len()];
        let mut trail: Vec<[f64; 3]> = vec![[f64::INFINITY; 3]; ys.len()];
        let mut least = vec![f64::INFINITY; ys.len()];
        let sharpen = |t: &[f64; 3], least: &mut f64| {
            let [a, b, c] = *t;
            if b.is_finite() && b <= a && b <= c {
                let bend = a - 2.0 * b + c;
                let low = if a.is_finite() && c.is_finite() && bend > 0.0 { (b - (c - a).powi(2) / (8.0 * bend)).max(b - 0.25 * bend) } else { b };
                if low < *least {
                    *least = low;
                }
            }
        };
        for k in -steps..=steps {
            at_angle.iter_mut().for_each(|v| *v = f64::INFINITY);
            let phi = self.sweep * f64::from(k) / f64::from(steps);
            let (s, c) = phi.sin_cos();
            // The cutter turned by φ, seen from the face gear (which has moved r_w·φ along +x).
            let at = |q: V2| V2::new(q.x * c - q.y * s - rw * phi, q.x * s + q.y * c);
            let mut a = match self.cutter.first() {
                Some(q) => at(*q),
                None => break,
            };
            for q in &self.cutter[1..] {
                let b = at(*q);
                let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
                if hi > lo {
                    for (j, y) in ys.iter().enumerate() {
                        if *y >= lo && *y <= hi {
                            let x = a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x);
                            if x < at_angle[j] {
                                at_angle[j] = x;
                            }
                        }
                    }
                }
                a = b;
            }
            for (j, t) in trail.iter_mut().enumerate() {
                *t = [t[1], t[2], at_angle[j]];
                sharpen(t, &mut least[j]);
            }
        }
        for (j, t) in trail.iter().enumerate() {
            sharpen(&[t[1], t[2], f64::INFINITY], &mut least[j]);
        }
        // Downwards, a height out of the cutter's reach is as wide as the one above.
        let mut above = f64::INFINITY;
        for v in least.iter_mut().rev() {
            if v.is_finite() {
                above = *v;
            } else {
                *v = above;
            }
        }
        let pitch = TAU * r / self.z2();
        least.iter().map(|v| if v.is_finite() { v - self.backlash / 2.0 } else { 0.5 * pitch }).collect()
    }
    /// The face tooth's thickness at radius `r` and height `h` (negative: no tooth there).
    pub fn thickness(&self, r: f64, h: f64) -> f64 {
        2.0 * self.half_widths(r, &[h]).first().copied().unwrap_or(0.0)
    }
    /// The smallest radius without undercut at the teeth's top (and with a working pressure
    /// angle of 8° at least, which is what limits a pinion of many teeth).
    pub fn undercut_radius(&self) -> f64 {
        let g = &self.pinion;
        let rw = (g.rb().powi(2) / (g.r() - self.top()).max(1e-9)).max(g.rb() / 8f64.to_radians().cos());
        rw * self.z2() / self.z1()
    }
    /// The largest radius at which the top keeps a land of 0.2 modules.
    pub fn pointed_radius(&self) -> f64 {
        let land = 0.2 * self.m();
        let ok = |r: f64| self.thickness(r, self.top()) >= land;
        let (mut lo, mut hi) = (self.undercut_radius().max(self.pitch_radius() * 0.5), self.pitch_radius() * 4.0);
        if !ok(lo) {
            return lo;
        }
        if ok(hi) {
            return hi;
        }
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if ok(mid) { lo = mid } else { hi = mid }
        }
        lo
    }
    /// The face gear's teeth at each of the `heights` (rising), between radii `ri` and `ro`,
    /// tooth 0 centred on +x: joined at `ri` by an arc (the inside stays solid, a raised hub
    /// inside the teeth). Every section has the same pieces, for a loft.
    pub fn sections(&self, heights: &[f64], ri: f64, ro: f64) -> Vec<Outline> {
        // Five points a side: the side bows gently, and few points keep the loft quick. A tooth
        // never takes more than 85% of the pitch.
        let n = 4;
        let max_half = PI / self.z2() * 0.85;
        let radii: Vec<f64> = (0..=n).map(|i| ri + (ro - ri) * f64::from(i) / f64::from(n)).collect();
        let widths: Vec<Vec<f64>> = radii.iter().map(|r| self.half_widths(*r, heights)).collect();
        (0..heights.len())
            .map(|j| {
                let pts: Vec<(f64, f64)> = radii.iter().zip(&widths).map(|(r, w)| (*r, (w[j] / r).clamp(1e-4, max_half))).collect();
                let (first, last) = (pts.first().map(|p| p.1).unwrap_or(0.0), pts.last().map(|p| p.1).unwrap_or(0.0));
                let f =
                    ToothFlank { rf: ri, ra: ro, root_half: first, tip_half: last, fillet: Vec::new(), flank: pts, undercut: false, pointed: false };
                teeth_outline(&f, self.teeth, 0.0)
            })
            .collect()
    }
    /// The teeth at radius `r` unrolled as a rack of `count` teeth standing on a base `base` deep,
    /// in the pinion's transverse plane with the pinion's axis at the origin and (for an odd
    /// count) a tooth centred on x = 0, under the pinion's space that points down.
    pub fn unrolled(&self, r: f64, count: u32, base: f64) -> Outline {
        let period = TAU * r / self.z2();
        let r1 = self.pinion.r();
        let (b, t) = (self.bottom(), self.top());
        let levels = 12;
        let heights: Vec<f64> = (0..=levels).map(|i| b + 0.03 * self.m() + (t - b - 0.03 * self.m()) * f64::from(i) / f64::from(levels)).collect();
        let widths: Vec<f64> = self.half_widths(r, &heights).iter().map(|w| w.clamp(0.005 * period, 0.425 * period)).collect();
        // One side of a tooth, from the floor up.
        let mut side: Vec<V2> = vec![V2::new(widths[0], b - r1)];
        side.extend(heights.iter().zip(&widths).map(|(h, w)| V2::new(*w, h - r1)));
        let n = count.max(1) as i32;
        let xk = |k: i32| (f64::from(k) - f64::from(n - 1) / 2.0) * period;
        let (x_left, x_right) = (xk(0) - period / 2.0, xk(n - 1) + period / 2.0);
        let (y_bot, y_base) = (b - r1, b - base.max(1e-6) - r1);
        let mut pts = vec![V2::new(x_left, y_base), V2::new(x_right, y_base), V2::new(x_right, y_bot)];
        for k in (0..n).rev() {
            let c = xk(k);
            pts.extend(side.iter().map(|q| V2::new(c + q.x, q.y)));
            pts.extend(side.iter().rev().map(|q| V2::new(c - q.x, q.y)));
        }
        pts.push(V2::new(x_left, y_bot));
        pts.dedup_by(|a, b| a.dist(*b) < 1e-9 * period);
        let segs: Vec<Seg> = (0..pts.len()).map(|i| Seg::Line(pts[i], pts[(i + 1) % pts.len()])).collect();
        Outline { segs }
    }
}
