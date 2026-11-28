//! Cycloidal gears (clock and instrument gears: epicycloid addenda, hypocycloid dedenda) and the
//! cycloidal drive's lobed disc (an offset epitrochoid) with its pin ring.

use std::f64::consts::{PI, TAU};

use crate::geom::{Outline, Seg, V2};
use crate::involute::{PROFILE_TOLERANCE, ToothFlank, side_points, teeth_outline};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cycloidal {
    pub teeth: u32,
    pub module: f64,
    /// Radius of the circle rolled outside the pitch circle (addendum flank).
    pub rolling_radius_addendum: f64,
    /// Radius of the circle rolled inside (dedendum flank); half the pitch radius gives straight
    /// radial flanks.
    pub rolling_radius_dedendum: f64,
    /// Addendum and dedendum in multiples of the module.
    pub addendum: f64,
    pub dedendum: f64,
    /// Circular backlash at the pitch circle (mm).
    pub backlash: f64,
}

impl Cycloidal {
    pub fn r(&self) -> f64 {
        self.module * f64::from(self.teeth) / 2.0
    }
    /// Standard choice: rolling circles half the pitch radius (radial flanks).
    pub fn with_defaults(teeth: u32, module: f64) -> Self {
        let r = module * f64::from(teeth) / 2.0;
        Self { teeth, module, rolling_radius_addendum: r / 2.0, rolling_radius_dedendum: r / 2.0, addendum: 1.0, dedendum: 1.25, backlash: 0.0 }
    }
}

/// Polar angle reached by an epicycloid (outside) or hypocycloid (inside) traced from the
/// pitch circle, as a function of radius.
fn cycloid_polar(r: f64, a: f64, outside: bool, target: f64) -> f64 {
    // Bisection on the rolling parameter t for the wanted radius.
    let point = |t: f64| -> V2 {
        if outside {
            V2::new((r + a) * t.cos() - a * ((r + a) / a * t).cos(), (r + a) * t.sin() - a * ((r + a) / a * t).sin())
        } else {
            V2::new((r - a) * t.cos() + a * ((r - a) / a * t).cos(), (r - a) * t.sin() - a * ((r - a) / a * t).sin())
        }
    };
    // The radius is monotone over the first half turn of the rolling circle (arc π·a).
    let (mut lo, mut hi) = (0.0, PI * a / r.max(1e-9));
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        let rr = point(mid).len();
        let past = if outside { rr > target } else { rr < target };
        if past {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    point(0.5 * (lo + hi)).angle()
}

pub fn cycloidal_outline(c: &Cycloidal, phase: f64) -> (Outline, ToothFlank) {
    let r = c.r();
    let half_pitch = (PI * c.module / 2.0 - c.backlash) / (2.0 * r);
    let (ra_full, rf) = (r + c.addendum * c.module, r - c.dedendum * c.module);
    let a_add = c.rolling_radius_addendum.max(1e-6);
    // Not clamp: at a tiny module r * 0.999 is below 1e-6 (and clamp panics when min > max).
    let a_ded = c.rolling_radius_dedendum.min(r * 0.999).max(1e-6);
    // Highest radius the epicycloid reaches: r + 2a.
    let mut ra = ra_full.min(r + 2.0 * a_add * 0.999);
    let half = |rr: f64| -> f64 {
        if rr >= r { half_pitch - cycloid_polar(r, a_add, true, rr) } else { half_pitch + cycloid_polar(r, a_ded, false, rr).abs().min(PI) }
    };
    // A pointed (ogival) tip ends where the flanks meet.
    let mut pointed = false;
    if half(ra) <= 1e-4 {
        pointed = true;
        let (mut lo, mut hi) = (r, ra);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if half(mid) > 1e-4 { lo = mid } else { hi = mid }
        }
        ra = lo;
    }
    let lowest = (r - 2.0 * a_ded * 0.999).max(rf);
    let tol = PROFILE_TOLERANCE * c.module;
    let min_gap = 5e-6 * ra.max(c.module);
    // The hypocycloid up to the pitch circle, then the epicycloid: one smooth side (both leave
    // the pitch circle square to it).
    // Graded by the square of the distance from the pitch circle: both cycloids leave it
    // radially with their angle growing as that distance to the power 3/2, which the fit cannot
    // follow in even steps of radius.
    let ded = side_points(&half, &|s| if s >= 1.0 { r } else { r - (r - lowest) * (1.0 - s) * (1.0 - s) }, tol, min_gap);
    let add = side_points(
        &half,
        &|s| {
            if s <= 0.0 {
                r
            } else if s >= 1.0 {
                ra
            } else {
                r + (ra - r) * s * s
            }
        },
        tol,
        min_gap,
    );
    let pitch = TAU / f64::from(c.teeth);
    let (fillet, flank, root_half);
    if ded.len() >= 2 && add.len() >= 2 && ded[0].0 - rf <= min_gap {
        // The dedendum runs down to the root circle: it and the addendum are a spline each,
        // as they were fitted. One spline through both would have to pass the pitch circle,
        // where the curvature jumps (a straight radial dedendum against an epicycloid leaving
        // as the 3/2 power), and would wave about both sides of it.
        let mut ded = ded;
        root_half = ded[0].1.min(pitch / 2.0);
        ded[0] = (rf, root_half);
        fillet = ded;
        flank = add;
    } else {
        let mut side = ded;
        if side.is_empty() {
            side = add;
        } else if !add.is_empty() {
            side.extend(add.into_iter().skip(1));
        }
        if side.len() < 2 {
            side = vec![(lowest, half(lowest)), (ra, half(ra))];
        }
        root_half = side[0].1.min(pitch / 2.0);
        side[0].1 = root_half;
        // Straight radial drop from the hypocycloid's end to the root circle.
        fillet = if side[0].0 - rf > min_gap { vec![(rf, root_half), (side[0].0, root_half)] } else { Vec::new() };
        if fillet.is_empty() {
            side[0].0 = rf;
        }
        flank = side;
    }
    let f = ToothFlank { rf, ra, root_half, tip_half: half(ra).max(0.0), fillet, flank, undercut: false, pointed };
    (teeth_outline(&f, c.teeth, phase), f)
}

/// A cycloidal drive: N pins on a circle, a disc with N − 1 lobes on an eccentric.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CycloidalDrive {
    /// Number of ring pins N (the disc has N − 1 lobes; reduction N − 1 : 1).
    pub pins: u32,
    /// Radius of the circle the pin centres are on.
    pub pin_circle_radius: f64,
    pub pin_radius: f64,
    /// Eccentricity of the disc's bearing.
    pub eccentricity: f64,
    /// Clearance taken off the disc's profile (mm, normal offset).
    pub clearance: f64,
}

impl CycloidalDrive {
    pub fn lobes(&self) -> u32 {
        self.pins.saturating_sub(1)
    }
    /// A point of the disc's profile: the epitrochoid the pin centres trace on the disc, offset
    /// inwards by `offset` (the pin radius and clearance; 0 for the pin centre's own path).
    fn profile(&self, t: f64, offset: f64) -> V2 {
        let n = f64::from(self.pins);
        let (rr, e) = (self.pin_circle_radius, self.eccentricity);
        let psi = ((1.0 - n) * t).sin().atan2(rr / (e * n) - ((1.0 - n) * t).cos());
        V2::new(rr * t.cos() - offset * (t + psi).cos() - e * (n * t).cos(), -rr * t.sin() + offset * (t + psi).sin() + e * (n * t).sin())
    }
    /// The disc's profile (centred on its own bearing), as one spline per lobe.
    pub fn disc(&self) -> Outline {
        let rp = self.pin_radius + self.clearance;
        let lobes = self.lobes().max(1);
        let per_lobe = 48;
        let lobe = TAU / f64::from(lobes);
        let point = |t: f64| self.profile(t, rp);
        // Fit points placed by arc length and by how much the profile turns, in equal shares:
        // the flanks' tight bends get the points that even steps of the crank angle would spend
        // on the long, nearly straight stretches (those left the lobes up to 0.1 mm off).
        let fine = 2400;
        let f: Vec<V2> = (0..=fine).map(|i| point(lobe * i as f64 / fine as f64)).collect();
        let len: Vec<f64> = f.windows(2).map(|w| w[0].dist(w[1])).collect();
        let turn: Vec<f64> = (0..fine)
            .map(|i| {
                let bend = |j: usize| {
                    if j == 0 || j >= fine {
                        0.0
                    } else {
                        (f[j] - f[j - 1]).cross(f[j + 1] - f[j]).atan2((f[j] - f[j - 1]).dot(f[j + 1] - f[j])).abs()
                    }
                };
                0.5 * (bend(i) + bend(i + 1))
            })
            .collect();
        let (total_len, total_turn) = (len.iter().sum::<f64>().max(1e-12), turn.iter().sum::<f64>().max(1e-12));
        let mut cum = vec![0.0; fine + 1];
        for i in 0..fine {
            cum[i + 1] = cum[i] + 0.5 * len[i] / total_len + 0.5 * turn[i] / total_turn;
        }
        let mut ts = Vec::with_capacity(per_lobe);
        let mut i = 0;
        for k in 0..per_lobe {
            let want = cum[fine] * k as f64 / per_lobe as f64;
            while i + 1 < fine && cum[i + 1] < want {
                i += 1;
            }
            let part = if cum[i + 1] > cum[i] { ((want - cum[i]) / (cum[i + 1] - cum[i])).clamp(0.0, 1.0) } else { 0.0 };
            ts.push(lobe * (i as f64 + part) / fine as f64);
        }
        let mut pts: Vec<V2> = (0..lobes).flat_map(|j| ts.iter().map(move |t| f64::from(j) * lobe + t)).map(point).collect();
        pts.push(point(0.0));
        // Counter-clockwise.
        let mut area = 0.0;
        for w in pts.windows(2) {
            area += w[0].cross(w[1]);
        }
        if area < 0.0 {
            pts.reverse();
        }
        let mut segs = Vec::new();
        for k in 0..lobes as usize {
            segs.push(Seg::Spline(pts[k * per_lobe..=(k + 1) * per_lobe].to_vec()));
        }
        Outline { segs }
    }
    /// Whether the pins are too big for the lobes: the profile (the pin centres' path on the
    /// disc, offset by the pin radius) doubles back on itself where the path bends tighter than
    /// a pin.
    pub fn undercut(&self) -> bool {
        let n = f64::from(self.pins);
        let (rr, rp, e) = (self.pin_circle_radius, self.pin_radius + self.clearance, self.eccentricity);
        let path = |t: f64| V2::new(rr * t.cos() - e * (n * t).cos(), -rr * t.sin() + e * (n * t).sin());
        let profile = |t: f64| {
            let psi = ((1.0 - n) * t).sin().atan2(rr / (e * n) - ((1.0 - n) * t).cos());
            path(t) + V2::new(-rp * (t + psi).cos(), rp * (t + psi).sin())
        };
        let period = TAU / f64::from(self.lobes().max(1));
        let steps = 720;
        let h = period / steps as f64;
        (0..steps).any(|i| {
            let t = h * i as f64;
            let (a, b) = (path(t + h) - path(t), profile(t + h) - profile(t));
            !(a.dot(b) > 0.02 * a.dot(a))
        })
    }
    /// The disc's smallest radius (the bottom of a valley between lobes).
    pub fn valley_radius(&self) -> f64 {
        self.pin_circle_radius - self.pin_radius - self.clearance - self.eccentricity
    }
    /// The largest eccentricity before the profile loops (cusps): e < rr / N roughly, and the
    /// pin must fit its curvature.
    pub fn max_eccentricity(&self) -> f64 {
        self.pin_circle_radius / f64::from(self.pins)
    }
    pub fn pin_centres(&self) -> Vec<V2> {
        (0..self.pins).map(|k| V2::polar(self.pin_circle_radius, TAU * f64::from(k) / f64::from(self.pins))).collect()
    }
}
