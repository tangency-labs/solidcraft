//! Timing belt pulleys: GT2 (2, 3, 5 mm), HTD (3M, 5M, 8M), T2.5/T5/T10 and MXL/XL grooves
//! wrapped round the pulley.
//!
//! The groove forms are approximations from published belt dimensions, not the makers' pulley
//! drawings: GT2 and HTD are proprietary curvilinear profiles, modelled from the belt tooth's
//! published radii (tip R3, flank R2 centred on the land line, land fillet R1, height h); T
//! series from the DIN 7721 belt tooth (40° flanks), with the outside diameters of makers'
//! pulley tables (pitch Ø − 0.52, 0.86 and 1.86 mm for T2.5, T5 and T10); MXL and XL from the
//! RMA/ISO 5294 generated groove table for a single tooth count. Each groove is laid out straight and wrapped onto the
//! outside circle (arc length kept at the outside diameter).

use std::f64::consts::{PI, TAU};

use crate::geom::{Outline, Seg, V2};

/// A groove's shape across the belt, depths in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Groove {
    /// Straight flanks at `half_angle` (degrees) from the radial, a flat bottom `bottom_width`
    /// wide at `depth`, fillets `root_r` at the bottom and `tip_r` where the flanks meet the outside.
    Trapezoid { bottom_width: f64, depth: f64, half_angle: f64, root_r: f64, tip_r: f64 },
    /// Curvilinear (GT2, HTD): a bottom arc `tip_r` (the belt tooth's R3) at `depth`, flank arcs
    /// `flank_r` (R2) centred on the outside line, land fillets `fillet_r` (R1).
    Curvilinear { depth: f64, tip_r: f64, flank_r: f64, fillet_r: f64 },
}

/// A timing belt profile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Belt {
    pub id: &'static str,
    pub label: &'static str,
    /// Tooth pitch (mm).
    pub pitch: f64,
    /// Pitch line differential u: the pulley's outside diameter is the pitch diameter − 2u.
    pub pld: f64,
    pub groove: Groove,
    /// Widths the belt is sold in (mm), the first the usual one.
    pub widths: &'static [f64],
}

const fn curvy(depth: f64, tip_r: f64, flank_r: f64, fillet_r: f64) -> Groove {
    Groove::Curvilinear { depth, tip_r, flank_r, fillet_r }
}

const fn trap(bottom_width: f64, depth: f64, half_angle: f64, root_r: f64, tip_r: f64) -> Groove {
    Groove::Trapezoid { bottom_width, depth, half_angle, root_r, tip_r }
}

pub const BELTS: &[Belt] = &[
    Belt { id: "gt2_2", label: "GT2 2 mm", pitch: 2.0, pld: 0.254, groove: curvy(0.75, 0.555, 1.0, 0.15), widths: &[6.0, 9.0, 10.0] },
    Belt { id: "gt2_3", label: "GT2 3 mm", pitch: 3.0, pld: 0.381, groove: curvy(1.14, 0.85, 1.52, 0.25), widths: &[9.0, 6.0, 15.0] },
    Belt { id: "gt2_5", label: "GT2 5 mm", pitch: 5.0, pld: 0.5715, groove: curvy(1.93, 1.45, 2.62, 0.43), widths: &[15.0, 9.0, 25.0] },
    Belt { id: "htd_3m", label: "HTD 3M", pitch: 3.0, pld: 0.381, groove: curvy(1.17, 0.86, 1.8, 0.25), widths: &[9.0, 6.0, 15.0] },
    Belt { id: "htd_5m", label: "HTD 5M", pitch: 5.0, pld: 0.5715, groove: curvy(2.06, 1.49, 3.0, 0.43), widths: &[15.0, 9.0, 25.0] },
    Belt { id: "htd_8m", label: "HTD 8M", pitch: 8.0, pld: 0.686, groove: curvy(3.38, 2.5, 5.0, 0.64), widths: &[20.0, 30.0, 50.0] },
    Belt { id: "t2_5", label: "T2.5", pitch: 2.5, pld: 0.26, groove: trap(0.99, 0.75, 20.0, 0.2, 0.3), widths: &[6.0, 4.0, 10.0] },
    Belt { id: "t5", label: "T5", pitch: 5.0, pld: 0.43, groove: trap(1.78, 1.25, 20.0, 0.4, 0.6), widths: &[10.0, 16.0, 25.0] },
    Belt { id: "t10", label: "T10", pitch: 10.0, pld: 0.93, groove: trap(3.48, 2.6, 20.0, 0.6, 1.0), widths: &[16.0, 25.0, 32.0] },
    Belt { id: "mxl", label: "MXL (0.08 in)", pitch: 2.032, pld: 0.254, groove: trap(0.84, 0.69, 20.0, 0.25, 0.13), widths: &[6.35, 3.18, 4.83] },
    Belt { id: "xl", label: "XL (1/5 in)", pitch: 5.08, pld: 0.254, groove: trap(1.32, 1.65, 25.0, 0.41, 0.64), widths: &[9.53, 6.35, 7.94] },
];

pub fn belt_by_id(id: &str) -> Option<&'static Belt> {
    BELTS.iter().find(|b| b.id == id)
}

impl Groove {
    pub fn depth(&self) -> f64 {
        match *self {
            Groove::Trapezoid { depth, .. } | Groove::Curvilinear { depth, .. } => depth,
        }
    }
    /// The groove across, from the outside line on the left to the outside line on the right
    /// (x across, y = 0 the outside line, negative inwards).
    pub fn profile(&self) -> Vec<V2> {
        let mut right = Vec::new();
        match *self {
            Groove::Curvilinear { depth: h, tip_r: r3, flank_r: r2, fillet_r: r1 } => {
                let c3 = V2::new(0.0, -(h - r3));
                // The flank's centre on the outside line, internally tangent to the bottom arc.
                let b = ((r2 - r3).powi(2) - (h - r3).powi(2)).max(0.0).sqrt();
                let cf = V2::new(-b, 0.0);
                let d = (c3 - cf).norm();
                let p = cf + d * r2;
                arc_pts(c3, r3, -PI / 2.0, (p - c3).angle(), 5, &mut right);
                // Land fillet: centre in the rim, tangent to the outside line and the flank.
                let xc = -b + ((r2 + r1).powi(2) - r1 * r1).max(0.0).sqrt();
                let f = V2::new(xc, -r1);
                let q = cf + (f - cf).norm() * r2;
                arc_pts(cf, r2, (p - cf).angle(), (q - cf).angle(), 5, &mut right);
                arc_pts(f, r1, (q - f).angle(), PI / 2.0, 3, &mut right);
            }
            Groove::Trapezoid { bottom_width, depth, half_angle, root_r, tip_r } => {
                let phi = half_angle.to_radians();
                let (s, c) = phi.sin_cos();
                let n = V2::new(-c, s);
                let xb = bottom_width / 2.0 - root_r * (1.0 - s) / c;
                let cb = V2::new(xb, -depth + root_r);
                let xt = bottom_width / 2.0 + ((depth - tip_r) * s + tip_r) / c;
                let ct = V2::new(xt, -tip_r);
                right.push(V2::new(0.0, -depth));
                let on_flank_b = cb - n * root_r;
                arc_pts(cb, root_r, -PI / 2.0, (on_flank_b - cb).angle(), 3, &mut right);
                let on_flank_t = ct + n * tip_r;
                // The straight flank, a few points so a spline through them stays straight.
                for i in 1..4 {
                    right.push(on_flank_b.lerp(on_flank_t, f64::from(i) / 4.0));
                }
                arc_pts(ct, tip_r, (on_flank_t - ct).angle(), PI / 2.0, 3, &mut right);
            }
        }
        right.dedup_by(|a, b| a.dist(*b) < 1e-9);
        let mut all: Vec<V2> = right.iter().rev().map(|q| V2::new(-q.x, q.y)).collect();
        all.pop();
        all.extend(right);
        all
    }
    /// Half the groove's width at the outside line.
    pub fn half_width(&self) -> f64 {
        self.profile().last().map(|q| q.x).unwrap_or(0.0)
    }
}

/// Points along an arc about `c` from angle `a0` to `a1` (the short way), `n` steps, without the
/// first point when it repeats the last one pushed.
fn arc_pts(c: V2, r: f64, a0: f64, a1: f64, n: u32, out: &mut Vec<V2>) {
    let mut sweep = a1 - a0;
    while sweep > PI {
        sweep -= TAU;
    }
    while sweep < -PI {
        sweep += TAU;
    }
    for i in 0..=n {
        out.push(c + V2::polar(r, a0 + sweep * f64::from(i) / f64::from(n)));
    }
}

/// A pulley's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pulley {
    pub belt: Belt,
    pub teeth: u32,
}

impl Pulley {
    pub fn pitch_diameter(&self) -> f64 {
        self.belt.pitch * f64::from(self.teeth) / PI
    }
    pub fn outside_diameter(&self) -> f64 {
        self.pitch_diameter() - 2.0 * self.belt.pld
    }
    /// The land left between grooves on the outside circle (arc length; negative when grooves overlap).
    pub fn land(&self) -> f64 {
        let ro = self.outside_diameter() / 2.0;
        TAU * ro / f64::from(self.teeth) - 2.0 * self.belt.groove.half_width()
    }
    /// The grooved outline, counter-clockwise, groove 0 centred at `phase`.
    pub fn outline(&self, phase: f64) -> Outline {
        let ro = self.outside_diameter() / 2.0;
        let prof = even_profile(&self.belt.groove.profile(), 0.04 * self.belt.pitch);
        // The bottom's middle point, where the two halves meet.
        let mid = prof
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.x.abs().total_cmp(&b.1.x.abs()))
            .map(|(i, _)| i)
            .unwrap_or(prof.len() / 2)
            .clamp(1, prof.len().saturating_sub(2).max(1));
        let n = self.teeth.max(3);
        let step = TAU / f64::from(n);
        let wrap = |q: V2, at: f64| V2::polar(ro + q.y, at + q.x / ro);
        let mut segs = Vec::new();
        for k in 0..n {
            let at = phase + step * f64::from(k);
            // Groove k runs clockwise to counter-clockwise: its left side (−x) is the clockwise one.
            let pts: Vec<V2> = prof.iter().map(|q| wrap(*q, at)).collect();
            let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else { continue };
            // Two splines, each half of the groove, meeting at its bottom (the kernel fits short,
            // evenly spaced splines best).
            segs.push(Seg::Spline(pts[..=mid].to_vec()));
            segs.push(Seg::Spline(pts[mid..].to_vec()));
            let next = V2::polar(first.len(), first.angle() + step);
            if last.dist(next) > 1e-6 && (next.angle() - last.angle()).rem_euclid(TAU) < PI {
                segs.push(Seg::Arc { c: V2::ZERO, a: last, b: next, ccw: true });
            }
        }
        crate::involute::join_ends(&mut segs);
        Outline { segs }
    }
}

/// The groove profile with points no closer than `min` (the ends and the bottom point kept), so
/// the splines through them are evenly spaced.
fn even_profile(p: &[V2], min: f64) -> Vec<V2> {
    let n = p.len();
    let mid = n / 2;
    let mut out: Vec<V2> = Vec::with_capacity(n);
    for (i, q) in p.iter().enumerate() {
        let keep = i == 0 || i == mid || i + 1 == n;
        let far = out.last().is_none_or(|l| l.dist(*q) >= min);
        let near_kept = (i + 1 == mid || i + 2 == n) && p.get(i + 1).is_some_and(|nx| nx.dist(*q) < min);
        if keep || (far && !near_kept) {
            out.push(*q);
        }
    }
    out
}
