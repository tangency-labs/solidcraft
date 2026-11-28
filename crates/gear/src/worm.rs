//! A throated (enveloping) worm wheel, generated from its worm the way a hob cuts one.
//!
//! On a plane square to the wheel's axis, a distance `w` off its mid plane, the worm's thread
//! shows as a rack: the thread's axial profile where the plane meets it, pushed along the worm
//! by the screw as the plane leaves the worm's axis (the "offset-plane rack"). Turning the worm
//! slides every one of these racks along at the same speed, so on each plane the wheel's tooth
//! space is what that plane's rack sweeps out rolling on the wheel's pitch circle. The racks
//! differ from plane to plane (their flanks lean and bend more the further off the mid plane),
//! so the wheel's teeth wrap round the worm and touch it along lines, not at a point.
//!
//! The rack here is a hob's: the worm's thread made taller by the root clearance, thicker by
//! the backlash, with a rounded tip corner that cuts the wheel's root fillets.

use std::f64::consts::PI;

use crate::geom::fit_profile;
use crate::involute::{FILLET_TOLERANCE, ToothSides};

/// Curves given by a parameter (0 to 1) as (radius, angle), sampled coarsely and refined where
/// a radius is wanted: the furthest any of them reaches round at a radius is the edge of what
/// they sweep.
pub struct Envelope<F: Fn(usize, f64) -> (f64, f64)> {
    eval: F,
    curves: Vec<Vec<(f64, f64)>>,
}

impl<F: Fn(usize, f64) -> (f64, f64)> Envelope<F> {
    /// `samples`: coarse samples for each curve.
    pub fn new(eval: F, samples: &[usize]) -> Self {
        let curves = samples.iter().enumerate().map(|(k, n)| (0..=*n).map(|i| eval(k, i as f64 / *n as f64)).collect()).collect();
        Envelope { eval, curves }
    }
    /// The largest angle any curve has at radius `rr`, and which curve.
    pub fn extreme(&self, rr: f64) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (k, c) in self.curves.iter().enumerate() {
            let n = c.len() - 1;
            for i in 0..n {
                let (ra, rb) = (c[i].0, c[i + 1].0);
                if !(ra.is_finite() && rb.is_finite() && c[i].1.is_finite() && c[i + 1].1.is_finite())
                    || (ra - rr) * (rb - rr) > 0.0
                    || (ra - rb).abs() <= 1e-12 * rr
                {
                    continue;
                }
                let up = rb > ra;
                let (mut lo, mut hi) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                let mut p = c[i];
                for _ in 0..34 {
                    let mid = 0.5 * (lo + hi);
                    let q = (self.eval)(k, mid);
                    if !q.0.is_finite() {
                        break;
                    }
                    p = q;
                    if (p.0 < rr) == up {
                        lo = mid
                    } else {
                        hi = mid
                    }
                    if hi - lo < 1e-15 {
                        break;
                    }
                }
                if best.is_none_or(|(b, _)| p.1 > b) {
                    best = Some((p.1, k));
                }
            }
        }
        best
    }
}

/// The hob that cuts a worm wheel: the worm's thread in axial section, and where it runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hob {
    /// Centre distance between the worm's and the wheel's axes.
    pub centre: f64,
    /// The wheel's pitch radius (module × teeth / 2): the hob's rack rolls on this circle.
    pub wheel_pitch: f64,
    /// The worm's reference radius, where the thread's thickness is given.
    pub reference: f64,
    /// The hob's tip radius (the worm's plus the root clearance) and tip corner rounding.
    pub tip: f64,
    pub corner: f64,
    /// The lowest radius the thread's straight flank is carried down to.
    pub low: f64,
    /// Half the thread's axial thickness at the reference radius.
    pub half: f64,
    /// Tangent of the thread's axial pressure angle (a straight-sided axial profile, ZA).
    pub tan_axial: f64,
    /// The screw: lead / 2π, negative for a left-hand worm.
    pub screw: f64,
}

/// Fit points for each wheel tooth side's fillet and flank: every section's sides are made of
/// the same number, so the sections loft point to point.
pub const FILLET_POINTS: usize = 6;
pub const FLANK_POINTS: usize = 9;

impl Hob {
    /// The tip corner: its radius (no bigger than leaves the centre on the thread's own side
    /// of its middle), the centre (radius, axial) and the radius the straight flank ends at.
    fn corner_arc(&self) -> (f64, f64, f64, f64) {
        let ax = self.tan_axial.atan();
        let at = |rho: f64| -> (f64, f64) {
            let rc = self.tip - rho;
            (rc, self.half + (-rho - (rc - self.reference) * ax.sin()) / ax.cos())
        };
        let mut rho = self.corner.max(1e-6);
        if at(rho).1 < 0.0 {
            let (mut lo, mut hi) = (1e-6, rho);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if at(mid).1 >= 0.0 { lo = mid } else { hi = mid }
            }
            rho = lo;
        }
        let (rc, zc) = at(rho);
        (rho, rc, zc.max(0.0), rc + rho * ax.sin())
    }
    /// The wheel's root radius on the plane `w` off the mid plane (the hob's tip cylinder).
    pub fn root_radius(&self, w: f64) -> f64 {
        self.centre - (self.tip * self.tip - w * w).max(0.0).sqrt()
    }
    /// The wheel point cut by the thread point at `t` along its tip corner (`k` = 0) or flank
    /// (1) on the plane `w`, as (radius, angle from the middle of the tooth space × `side`);
    /// `side` +1 for the thread's side further along the worm, −1 for the other.
    /// `corner`: [`Hob::corner_arc`], worked out once by the caller.
    fn cut(&self, corner: (f64, f64, f64, f64), w: f64, side: f64, k: usize, t: f64) -> (f64, f64) {
        let (rho_c, rc, zc, re) = corner;
        let ax = self.tan_axial.atan();
        // The thread's axial profile: radius, axial position and their rates along it.
        let (rho, zeta, d_rho, d_zeta) = if k == 0 {
            let phi = (PI / 2.0 - ax) * t;
            (rc + rho_c * phi.cos(), zc + rho_c * phi.sin(), -rho_c * phi.sin(), rho_c * phi.cos())
        } else {
            let low = self.low.max(w.abs() + 1e-6 * self.reference);
            let rho = re + (low - re) * t;
            (rho, self.half - (rho - self.reference) * self.tan_axial, low - re, -(low - re) * self.tan_axial)
        };
        let q2 = rho * rho - w * w;
        if q2 <= 0.0 {
            return (f64::NAN, f64::NAN);
        }
        let q = q2.sqrt();
        // On the plane: towards the wheel's centre and along the worm; the screw carries the
        // thread along as the plane's cut goes round the worm.
        let (x, y) = (self.centre - q, side * zeta + self.screw * (w / rho).asin());
        let (dx, dy) = (-rho * d_rho / q, side * d_zeta - self.screw * w * d_rho / (rho * q));
        if dy.abs() <= 1e-12 * dx.abs() {
            return (f64::NAN, f64::NAN);
        }
        // The rack slid to where this point's normal passes through the pitch point, and the
        // wheel turned back by the same roll.
        let slide = -y + (self.wheel_pitch - x) * dx / dy;
        (x.hypot(y + slide), side * ((y + slide).atan2(x) - slide / self.wheel_pitch))
    }
    /// The true sides of the wheel's tooth on the plane `w` (as [`Hob::tooth`] gives them, before
    /// fitting): side 0 (lower) or 1's angle from the tooth's centre line at a radius.
    pub fn side_angle(&self, w: f64, teeth: u32) -> impl Fn(usize, f64) -> f64 + '_ {
        let rf = self.root_radius(w);
        let pitch_half = PI / f64::from(teeth);
        let (_, _, zc, _) = self.corner_arc();
        let lean = self.screw * (w / self.tip).clamp(-1.0, 1.0).asin();
        let corner = self.corner_arc();
        let envelopes = [1.0, -1.0].map(|side| Envelope::new(move |k, t| self.cut(corner, w, side, k, t), &[60, 240]));
        let roots = [(lean + zc) / self.wheel_pitch, (zc - lean) / self.wheel_pitch];
        move |i: usize, rr: f64| {
            let space = if rr <= rf * (1.0 + 1e-12) { roots[i] } else { envelopes[i].extreme(rr).map_or(roots[i], |x| x.0) };
            (if i == 0 { -1.0 } else { 1.0 }) * (pitch_half - space)
        }
    }
    /// One of the wheel's `teeth` teeth on the plane `w` off the mid plane, from the root up to
    /// `tip_radius` (lower where the tooth comes to a point), its sides as angles from the
    /// tooth's centre line (the middle of the tooth space the thread sits in is half a pitch
    /// before it). None when no tooth is left.
    pub fn tooth(&self, w: f64, tip_radius: f64, teeth: u32, m: f64) -> Option<ToothSides> {
        self.tooth_with(w, tip_radius, teeth, m, FILLET_POINTS, FLANK_POINTS)
    }
    /// [`Self::tooth`] with `fillet_points` fit points on each fillet and `flank_points` + 1 on
    /// each flank: fewer for a big, steep wheel, whose loft needs many sections within the
    /// kernel's curve limit.
    pub fn tooth_with(&self, w: f64, tip_radius: f64, teeth: u32, m: f64, fillet_points: usize, flank_points: usize) -> Option<ToothSides> {
        let rf = self.root_radius(w);
        let pitch_half = PI / f64::from(teeth);
        let (_, _, zc, _) = self.corner_arc();
        let lean = self.screw * (w / self.tip).clamp(-1.0, 1.0).asin();
        let corner = self.corner_arc();
        let envelopes = [1.0, -1.0].map(|side| Envelope::new(move |k, t| self.cut(corner, w, side, k, t), &[60, 240]));
        // Where each side of the space is at a radius (angle × side), and what cut it.
        let roots = [(lean + zc) / self.wheel_pitch, (zc - lean) / self.wheel_pitch];
        let space = |i: usize, rr: f64| -> (f64, usize) {
            if rr <= rf * (1.0 + 1e-12) { (roots[i], 0) } else { envelopes[i].extreme(rr).unwrap_or((roots[i], 0)) }
        };
        let thickness = |rr: f64| (2.0 * pitch_half - space(0, rr).0 - space(1, rr).0) * rr;
        let land = 0.02 * m;
        if !(tip_radius > rf + 0.2 * m) || !(thickness(rf) > land) {
            return None;
        }
        let mut ra = tip_radius;
        let probes = 48;
        let mut prev = rf;
        let mut handover = [None, None];
        for i in 1..=probes {
            let rr = rf + (tip_radius - rf) * f64::from(i) / f64::from(probes);
            let here = [space(0, rr), space(1, rr)];
            for (side, h) in handover.iter_mut().enumerate() {
                if h.is_none() && here[side].1 == 1 {
                    *h = Some(bisect(rr, prev, &|x| space(side, x).1 == 1));
                }
            }
            if (2.0 * pitch_half - here[0].0 - here[1].0) * rr <= land {
                ra = bisect(prev, rr, &|x| thickness(x) > land);
                break;
            }
            prev = rr;
        }
        if !(ra > rf + 0.2 * m) {
            return None;
        }
        let side = |i: usize| -> Vec<Vec<(f64, f64)>> {
            let sign = if i == 0 { -1.0 } else { 1.0 };
            let angle = |rr: f64| sign * (pitch_half - space(i, rr.clamp(rf, ra)).0);
            // The fillet up to where the flank takes over, each piece a fair share of the side.
            let split = handover[i].unwrap_or(rf).clamp(rf + 0.04 * (ra - rf), rf + 0.6 * (ra - rf));
            let mut fillet = fit_profile(
                &angle,
                &|s| if s >= 1.0 { split } else { rf + (split - rf) * s * s },
                FILLET_TOLERANCE * m,
                fillet_points,
                fillet_points,
            );
            // The flank's points at set places along it (closer at the ends), not crowded where it
            // bends as a fitted side's are: a nearly straight flank's bends are noise that would
            // move its points about from one section to the next, and the loft's skin with them.
            let flank: Vec<(f64, f64)> = (0..=flank_points)
                .map(|j| {
                    let t = j as f64 / flank_points as f64;
                    let rr = split + (ra - split) * (0.6 * t + 0.2 * (1.0 - (PI * t).cos()));
                    (rr, angle(rr))
                })
                .collect();
            if let (Some(a), Some(b)) = (fillet.last_mut(), flank.first()) {
                *a = *b;
            }
            vec![fillet, flank]
        };
        Some(ToothSides { rf, ra, lower: side(0), upper: side(1) })
    }
}

/// The last value between `lo` (inside) and `hi` (not) that is inside.
fn bisect(mut lo: f64, mut hi: f64, inside: &dyn Fn(f64) -> bool) -> f64 {
    for _ in 0..44 {
        let mid = 0.5 * (lo + hi);
        if inside(mid) { lo = mid } else { hi = mid }
    }
    lo
}
