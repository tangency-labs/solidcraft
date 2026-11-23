//! Bevel gear teeth generated on the sphere by a crown gear (the rack of bevel gears): flat
//! flanks through the cone's apex rolling on the pitch cone give octoid teeth. Two bevels cut by
//! the two sides of one crown gear are conjugate along their whole face, at any shaft angle,
//! and neither's tips reach the other's root fillets: Tredgold's back-cone involute is only
//! near this, and the difference shows as interference once the teeth spiral.
//!
//! Everything turns about the apex, so a tooth is the same on every sphere about it: the
//! profile is worked out as directions from the apex and handed back on the plane through the
//! large end's pitch circle (a direction at polar angle θ from the axis meets it at radius
//! h·tan θ, `h` the apex's height over that plane).

use std::f64::consts::PI;

use crate::axes::{cross, dot};
use crate::involute::{FILLET_TOLERANCE, PROFILE_TOLERANCE, ToothFlank, ToothSides, side_points};

type P3 = [f64; 3];

fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    a.map(|v| v / l)
}
fn mix(a: P3, ka: f64, b: P3, kb: f64) -> P3 {
    [0, 1, 2].map(|i| a[i] * ka + b[i] * kb)
}

/// The crown gear tooth that cuts one tooth space of a bevel gear, and the gear's cone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crown {
    /// The gear's pitch cone angle (radians).
    pub delta: f64,
    /// Cone distance at the large end (mm): the lengths below are arcs at this distance.
    pub cone_distance: f64,
    /// The flanks' pressure angle in the transverse section (radians).
    pub pressure_angle: f64,
    /// The gear's addendum and dedendum (the crown tooth is as tall as the dedendum).
    pub addendum: f64,
    pub dedendum: f64,
    /// The crown tooth's tip rounding (the gear's root fillet).
    pub tip_radius: f64,
    /// Half the crown tooth's thickness on the pitch plane: half the gear's space width.
    pub half_thickness: f64,
}

/// The crown tooth's right half on the unit sphere, in the basis (pitch line, across, crown
/// axis): its tip land (0), tip rounding (1) and flank (2), each with a parameter from 0 to 1.
struct Cutter {
    delta: f64,
    h: f64,
    eps_tip: f64,
    psi_c: f64,
    rho: f64,
    q: P3,
    b1: P3,
    b2: P3,
    t_end: f64,
    g0: P3,
    w: P3,
    s_fillet: f64,
    s_top: f64,
}

/// Coarse samples per curve, refined where a radius is wanted.
const SAMPLES: [usize; 3] = [8, 60, 160];

impl Cutter {
    fn new(c: &Crown) -> Cutter {
        let re = c.cone_distance;
        let a = c.pressure_angle;
        let psi0 = c.half_thickness / re;
        let eps_tip = (c.dedendum / re).atan();
        let g0 = [psi0.cos(), psi0.sin(), 0.0];
        let tau = [-psi0.sin(), psi0.cos(), 0.0];
        // Along the flank from the pitch plane towards the tip, and the flank's outward normal.
        let w = [-a.sin() * tau[0], -a.sin() * tau[1], -a.cos()];
        let n = [a.cos() * tau[0], a.cos() * tau[1], -a.sin()];
        // The rounding: a small circle touching the tip cone and the flank, its centre no
        // further across than the tooth's middle.
        let centre = |rho: f64| -> (f64, f64) {
            let eps_c = eps_tip - rho;
            (eps_c, psi0 + (-(rho.sin() + a.sin() * eps_c.sin()) / (eps_c.cos() * a.cos())).clamp(-1.0, 1.0).asin())
        };
        let mut rho = (c.tip_radius / re).max(1e-9);
        if centre(rho).1 < 0.0 {
            let (mut lo, mut hi) = (1e-9, rho);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if centre(mid).1 >= 0.0 { lo = mid } else { hi = mid }
            }
            rho = lo * 0.999;
        }
        let (eps_c, psi_c) = centre(rho);
        let psi_c = psi_c.max(0.0);
        let at = |eps: f64, psi: f64| -> P3 { [eps.cos() * psi.cos(), eps.cos() * psi.sin(), -eps.sin()] };
        let q = at(eps_c, psi_c);
        let touch = unit(mix(q, 1.0, n, -dot(q, n)));
        let s_fillet = dot(touch, w).atan2(dot(touch, g0));
        let tip = at(eps_tip, psi_c);
        let b1 = unit(mix(tip, 1.0, q, -dot(tip, q)));
        let mut b2 = cross(q, b1);
        let mut t_end = dot(touch, b2).atan2(dot(touch, b1));
        if t_end < 0.0 {
            b2 = b2.map(|v| -v);
            t_end = -t_end;
        }
        // The flank runs on past the pitch plane to beyond where the gear's tip can touch it.
        let s_top = -2.5 * (c.addendum / re).atan().max(0.2 * eps_tip) / a.cos();
        Cutter { delta: c.delta, h: re * c.delta.cos(), eps_tip, psi_c, rho, q, b1, b2, t_end, g0, w, s_fillet, s_top }
    }
    /// The cutter point at `t` along curve `k`, and the curve's direction there.
    fn point(&self, k: usize, t: f64) -> (P3, P3) {
        match k {
            0 => {
                let psi = self.psi_c * t;
                ([self.eps_tip.cos() * psi.cos(), self.eps_tip.cos() * psi.sin(), -self.eps_tip.sin()], [-psi.sin(), psi.cos(), 0.0])
            }
            1 => {
                let a = self.t_end * t;
                let round = mix(self.b1, a.cos(), self.b2, a.sin());
                (mix(self.q, self.rho.cos(), round, self.rho.sin()), mix(self.b1, -a.sin(), self.b2, a.cos()))
            }
            _ => {
                let s = self.s_fillet + (self.s_top - self.s_fillet) * t;
                (mix(self.g0, s.cos(), self.w, s.sin()), mix(self.g0, -s.sin(), self.w, s.cos()))
            }
        }
    }
    /// The gear point that cutter point cuts, as (radius on the large end's plane, angle from
    /// the middle of the tooth space): the crown turned to where the pitch line's sliding is
    /// along the curve there, and the gear turned with it.
    fn gear(&self, k: usize, t: f64) -> (f64, f64) {
        let (v, d) = self.point(k, t);
        let psi = if d[1].abs() > 1e-300 { (d[0] / d[1]).atan() } else { 0.0 };
        let (s, c) = psi.sin_cos();
        let w = [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]];
        let (sd, cd) = self.delta.sin_cos();
        let (x, y, z) = (sd * w[0] + cd * w[2], w[1], -cd * w[0] + sd * w[2]);
        (self.h * x.hypot(y) / (-z).max(1e-12), y.atan2(x) - psi / sd)
    }
}

struct Generated {
    cutter: Cutter,
    curves: [Vec<(f64, f64)>; 3],
}

impl Generated {
    fn new(c: &Crown) -> Generated {
        let cutter = Cutter::new(c);
        let curves = [0, 1, 2].map(|k| (0..=SAMPLES[k]).map(|i| cutter.gear(k, i as f64 / SAMPLES[k] as f64)).collect());
        Generated { cutter, curves }
    }
    /// The space's half angle at radius `rr` (the furthest any generated curve reaches there,
    /// each crossing refined on the cutter), and which curve.
    fn space_half(&self, rr: f64) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (k, c) in self.curves.iter().enumerate() {
            let n = c.len() - 1;
            for i in 0..n {
                let (ra, rb) = (c[i].0, c[i + 1].0);
                if (ra - rr) * (rb - rr) > 0.0 || (ra - rb).abs() <= 1e-12 * rr {
                    continue;
                }
                let up = rb > ra;
                let (mut lo, mut hi) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                let mut p = c[i];
                for _ in 0..48 {
                    let mid = 0.5 * (lo + hi);
                    p = self.cutter.gear(k, mid);
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

impl Crown {
    /// The gear's tooth space: its half angle at a radius on the large end's plane.
    pub fn space_sampler(&self) -> impl Fn(f64) -> Option<f64> {
        let g = Generated::new(self);
        move |rr: f64| g.space_half(rr).map(|x| x.0)
    }
    /// The radii of the root and tip cones on the large end's plane.
    pub fn radii(&self) -> (f64, f64) {
        let h = self.cone_distance * self.delta.cos();
        (h * (self.delta - (self.dedendum / self.cone_distance).atan()).tan(), h * (self.delta + (self.addendum / self.cone_distance).atan()).tan())
    }
    /// One of the gear's `teeth` teeth on the large end's plane: its two sides, each turned by
    /// `skew` (radians at a radius; 0 for straight teeth, the spiral otherwise), as fit points
    /// within the profile tolerances for module `m`; and the unskewed tooth for the plan's
    /// checks.
    pub fn tooth(&self, teeth: u32, m: f64, skew: &dyn Fn(f64) -> f64) -> (ToothSides, ToothFlank) {
        self.tooth_with(teeth, m, skew, 1.0)
    }
    /// [`Crown::tooth`] with its fit points `coarse` times as far from the curves (fewer of them:
    /// a long spiral taper's loft needs fewer per section to stay lined up within the kernel's
    /// limit).
    pub fn tooth_with(&self, teeth: u32, m: f64, skew: &dyn Fn(f64) -> f64, coarse: f64) -> (ToothSides, ToothFlank) {
        let g = Generated::new(self);
        let pitch_half = PI / f64::from(teeth);
        let (rf, ra_full) = self.radii();
        let half = |rr: f64| -> Option<(f64, usize)> { g.space_half(rr).map(|(s, k)| (pitch_half - s, k)) };
        let root_half = pitch_half - g.curves[0].last().map_or(0.0, |p| p.1);
        let land = 0.02 * m;
        let thick = |rr: f64| half(rr).is_some_and(|(h, _)| h * rr > land / 2.0);
        let mut ra = ra_full;
        let mut pointed = false;
        let mut handover: Option<f64> = None;
        let mut prev = rf;
        let probes = 200;
        for i in 1..=probes {
            let rr = rf + (ra_full - rf) * (f64::from(i) / f64::from(probes)) * (1.0 - 1e-12);
            let Some((h, k)) = half(rr) else {
                prev = rr;
                continue;
            };
            if handover.is_none() && k == 2 {
                handover = Some(bisect(rr, prev, &|x| half(x).is_some_and(|(_, k)| k == 2)));
            }
            if h * rr <= land / 2.0 {
                ra = bisect(prev, rr, &thick);
                pointed = true;
                break;
            }
            prev = rr;
        }
        let handover = handover.filter(|h| *h < ra).unwrap_or(ra);
        let has_flank = handover < ra;
        let on = |rr: f64| -> f64 { half(rr.clamp(rf, ra)).map_or(root_half, |x| x.0) };
        let fillet_half = |rr: f64| -> f64 { if rr <= rf { root_half } else { on(rr.min(handover)) } };
        let flank_half = |rr: f64| -> f64 { on(rr.max(handover)) };
        let min_gap = 5e-6 * ra.abs().max(m);
        let span = handover - rf;
        let side = |sign: f64| -> Vec<Vec<(f64, f64)>> {
            let mut fillet = if span > 0.0 {
                side_points(
                    &|rr| sign * fillet_half(rr) + skew(rr),
                    &|s| if s >= 1.0 { handover } else { rf + span * s * s },
                    coarse * FILLET_TOLERANCE * m,
                    min_gap,
                )
            } else {
                Vec::new()
            };
            let flank = if has_flank {
                side_points(
                    &|rr| sign * flank_half(rr) + skew(rr),
                    &|s| if s >= 1.0 { ra } else { handover + (ra - handover) * s },
                    coarse * PROFILE_TOLERANCE * m,
                    min_gap,
                )
            } else {
                Vec::new()
            };
            if let (Some(a), Some(b)) = (fillet.last_mut(), flank.first()) {
                *a = *b;
            }
            [fillet, flank].into_iter().filter(|p| p.len() >= 2).collect()
        };
        let sides = ToothSides { rf, ra, lower: side(-1.0), upper: side(1.0) };
        let tip_half = on(ra);
        let plain = ToothFlank {
            rf,
            ra,
            root_half: root_half.max(0.0),
            tip_half,
            fillet: if span > 0.0 { vec![(rf, root_half), (handover, on(handover))] } else { Vec::new() },
            flank: if has_flank { vec![(handover, on(handover)), (ra, tip_half)] } else { Vec::new() },
            undercut: false,
            pointed,
        };
        (sides, plain)
    }
}

/// The last value between `lo` (inside) and `hi` (not) that is inside.
fn bisect(mut lo: f64, mut hi: f64, inside: &dyn Fn(f64) -> bool) -> f64 {
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if inside(mid) { lo = mid } else { hi = mid }
    }
    lo
}
