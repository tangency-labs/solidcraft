//! Involute splines: the external spline (shaft) generated like a gear by its basic rack, and the
//! internal spline (hub) whose spaces take the shaft's teeth. DIN 5480 (30°, sized by the
//! reference diameter with the profile shift that sets) and ANSI B92.1 side fit (diametral
//! pitch, no shift), with a flat or a fillet root.
//!
//! Proportions (× module, from the pitch circle moved out by the shift x·m):
//! - DIN 5480 flat root: shaft addendum 0.45 and dedendum 0.60 (the standard's hobbed form,
//!   root radius 0.16), hub tip 0.45 and root 0.55 (broached), so the hub's root diameter is the
//!   reference diameter d_B. Fillet root: the shaft's dedendum 0.84 with root radius 0.54 (the
//!   standard's cold-rolled form), the hub's root 0.65 (its shaped form).
//! - ANSI B92.1: major (N + 1)/P and minor (N − 1)/P; roots (N ∓ 1.35)/P flat, (N ∓ 1.8)/P
//!   fillet. The 30° proportions are used at every pressure angle.
//!
//! A fillet root on the hub is one arc from flank to flank through the major diameter, starting
//! a form clearance (0.1 module) above the shaft's tips. The hub's teeth are kept clear of the
//! shaft's root fillets: where few teeth put the end of the shaft's involute above the hub's
//! minor diameter, the minor diameter is raised to it.

use std::f64::consts::TAU;

use crate::geom::{Outline, Seg, V2};
use crate::involute::{BasicRack, Involute, PROFILE_TOLERANCE, ToothFlank, external_flank, join_ends, side_points, teeth_outline};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SplineStandard {
    #[default]
    Din5480,
    AnsiB92,
}

impl SplineStandard {
    pub const ALL: [(SplineStandard, &'static str, &'static str); 2] =
        [(SplineStandard::Din5480, "din5480", "DIN 5480"), (SplineStandard::AnsiB92, "ansi_b92", "ANSI B92.1 (flat root side fit)")];
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(k, _, _)| *k == self).map(|(_, n, _)| *n).unwrap_or("din5480")
    }
    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(k, _, _)| *k == self).map(|(_, _, l)| *l).unwrap_or("DIN 5480")
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, n, _)| *n == s).map(|(k, _, _)| *k)
    }
}

/// A spline connection: shaft and hub.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spline {
    pub standard: SplineStandard,
    pub module: f64,
    pub teeth: u32,
    /// Pressure angle (radians; 30° for DIN 5480).
    pub pressure_angle: f64,
    /// DIN 5480 reference diameter d_B (0: module × (teeth + 1.1), no shift).
    pub reference_diameter: f64,
    pub fillet_root: bool,
    /// Circular clearance between the flanks at the pitch circle (half off the shaft's teeth,
    /// half added to the hub's spaces).
    pub clearance: f64,
}

impl Spline {
    fn m(&self) -> f64 {
        self.module
    }
    fn z(&self) -> f64 {
        f64::from(self.teeth)
    }
    /// Pitch diameter m·z.
    pub fn pitch_diameter(&self) -> f64 {
        self.m() * self.z()
    }
    /// DIN 5480's reference diameter d_B (ANSI: the major diameter).
    pub fn reference_diameter(&self) -> f64 {
        match self.standard {
            SplineStandard::Din5480 if self.reference_diameter > 0.0 => self.reference_diameter,
            SplineStandard::Din5480 => self.m() * (self.z() + 1.1),
            SplineStandard::AnsiB92 => self.m() * (self.z() + 1.0),
        }
    }
    /// Profile shift coefficient: DIN 5480's x = (d_B − m·z − 1.1·m) / 2m; ANSI none.
    pub fn shift(&self) -> f64 {
        match self.standard {
            SplineStandard::Din5480 => (self.reference_diameter() - self.pitch_diameter() - 1.1 * self.m()) / (2.0 * self.m()),
            SplineStandard::AnsiB92 => 0.0,
        }
    }
    /// The shaft's tip and root diameters.
    pub fn shaft_diameters(&self) -> (f64, f64) {
        let (m, d) = (self.m(), self.pitch_diameter());
        match self.standard {
            SplineStandard::Din5480 => {
                let db = self.reference_diameter();
                (db - 0.2 * m, if self.fillet_root { db - 2.78 * m } else { db - 2.3 * m })
            }
            SplineStandard::AnsiB92 => (d + m, if self.fillet_root { d - 1.8 * m } else { d - 1.35 * m }),
        }
    }
    /// The hub's tip (minor) and root (major) diameters.
    pub fn hub_diameters(&self) -> (f64, f64) {
        let (m, d) = (self.m(), self.pitch_diameter());
        match self.standard {
            SplineStandard::Din5480 => {
                let db = self.reference_diameter();
                (db - 2.0 * m, if self.fillet_root { db + 0.2 * m } else { db })
            }
            SplineStandard::AnsiB92 => (d - m, if self.fillet_root { d + 1.8 * m } else { d + 1.35 * m }),
        }
    }
    /// The basic rack whose cutter makes the shaft's tip and root diameters.
    pub fn rack(&self) -> BasicRack {
        let (m, r, x) = (self.m(), self.pitch_diameter() / 2.0, self.shift());
        let (da, df) = self.shaft_diameters();
        let root_radius = match (self.fillet_root, self.standard) {
            (false, _) => 0.16,
            (true, SplineStandard::Din5480) => 0.54,
            // The full round the cutter's tip allows (the generator caps it).
            (true, SplineStandard::AnsiB92) => 10.0,
        };
        BasicRack { addendum: (da / 2.0 - r) / m - x, dedendum: (r - df / 2.0) / m + x, root_radius }
    }
    /// The shaft as a gear.
    pub fn shaft(&self) -> Involute {
        Involute {
            teeth: self.teeth,
            module: self.m(),
            pressure_angle: self.pressure_angle,
            helix: 0.0,
            shift: self.shift(),
            rack: self.rack(),
            backlash: self.clearance / 2.0,
            tip_shortening: 0.0,
            virtual_teeth: 0.0,
        }
    }
    /// The shaft's generated flank.
    pub fn shaft_flank(&self) -> ToothFlank {
        external_flank(&self.shaft())
    }
    /// The shaft's teeth, tooth 0 centred at `phase`.
    pub fn shaft_outline(&self, phase: f64) -> (Outline, ToothFlank) {
        let f = self.shaft_flank();
        (teeth_outline(&f, self.teeth, phase), f)
    }
    /// The hub's spaces as a gear: the shaft's involutes, wider by the clearance.
    fn hub_gear(&self) -> Involute {
        Involute { backlash: -self.clearance / 2.0, ..self.shaft() }
    }
    /// Space width on the pitch circle of the hub (= the shaft's tooth thickness without clearance,
    /// plus the clearance).
    pub fn hub_space(&self) -> f64 {
        self.hub_gear().thickness()
    }
    /// The hub's minor radius as built: the tip diameter's, raised when the shaft's involute
    /// (flank `f`) starts above it (returns whether it was).
    pub fn hub_tip_radius(&self, f: &ToothFlank) -> (f64, bool) {
        let nominal = self.hub_diameters().0 / 2.0;
        let form = f.flank.first().map(|q| q.0).unwrap_or(f.rf) + 0.02 * self.m();
        if form > nominal { (form.min(f.ra), true) } else { (nominal, false) }
    }
    /// The hub's major radius as built: the root diameter's, lowered when the spaces would come to
    /// a point before it (returns whether they did).
    pub fn hub_root_radius(&self) -> (f64, bool) {
        let g = self.hub_gear();
        let (dt, dr) = self.hub_diameters();
        let (r0, r1) = (dt / 2.0, dr / 2.0);
        let land = 0.005 * self.m();
        let open = |rr: f64| g.half_angle_at(rr) * rr > land;
        if open(r1) || r1 <= r0 {
            return (r1, false);
        }
        let (mut lo, mut hi) = (r0, r1);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if open(mid) { lo = mid } else { hi = mid }
        }
        (lo, true)
    }
    /// The hub's toothed bore, counter-clockwise, a space centred at `phase` (where the shaft has a
    /// tooth). Its flanks are the shaft's involutes widened by the clearance (radial below the
    /// base circle); its root is an arc of the major circle, or for a fillet root one arc from
    /// flank to flank through the major diameter.
    pub fn hub_outline(&self, phase: f64) -> Outline {
        let g = self.hub_gear();
        let (m, z) = (self.m(), self.z());
        let (r0, _) = self.hub_tip_radius(&self.shaft_flank());
        let (r1, _) = self.hub_root_radius();
        let r1 = r1.max(r0 + 0.05 * m);
        let pitch = TAU / z;
        let cap = pitch / 2.0 * 0.995;
        let half = |rr: f64| g.half_angle_at(rr).clamp(0.0, cap);
        // Where the flank hands over to a fillet root: a form clearance above the shaft's tips.
        let r_form = (self.shaft_diameters().0 / 2.0 + 0.1 * m).clamp(r0 + 0.5 * (r1 - r0), r1);
        let fillet = self.fillet_root && r1 - r_form > 0.02 * m && half(r_form) * r_form > 0.02 * m;
        let r_top = if fillet { r_form } else { r1 };
        let mut pts = side_points(&half, &|s| r0 + (r_top - r0) * s, PROFILE_TOLERANCE * m, 5e-6 * r1.max(m));
        if pts.len() < 2 {
            pts = vec![(r0, half(r0)), (r_top, half(r_top))];
        }
        let (h0, h_top) = (half(r0), half(r_top));
        // The fillet arc's centre (on the space's centre line) and where it tops out: at the
        // major circle, or a half circle when the space is narrower than that is deep.
        let a_pt = V2::polar(r_top, h_top);
        let apex = r1.min(a_pt.x + a_pt.y);
        let xc = (apex * apex - r_top * r_top) / (2.0 * (apex - a_pt.x)).max(1e-12);
        let mut segs = Vec::new();
        for k in 0..self.teeth {
            let a = phase + pitch * f64::from(k);
            let lower: Vec<V2> = pts.iter().map(|(r, h)| V2::polar(*r, a - h)).collect();
            let upper: Vec<V2> = pts.iter().rev().map(|(r, h)| V2::polar(*r, a + h)).collect();
            let (top_lo, top_hi) = (V2::polar(r_top, a - h_top), V2::polar(r_top, a + h_top));
            segs.push(Seg::Spline(lower));
            if fillet {
                segs.push(Seg::Arc { c: V2::polar(xc, a), a: top_lo, b: top_hi, ccw: true });
            } else if top_lo.dist(top_hi) > 1e-6 * m {
                segs.push(Seg::Arc { c: V2::ZERO, a: top_lo, b: top_hi, ccw: true });
            }
            segs.push(Seg::Spline(upper));
            if pitch - 2.0 * h0 > 1e-6 {
                segs.push(Seg::Arc { c: V2::ZERO, a: V2::polar(r0, a + h0), b: V2::polar(r0, a + pitch - h0), ccw: true });
            }
        }
        join_ends(&mut segs);
        Outline { segs }
    }
}
