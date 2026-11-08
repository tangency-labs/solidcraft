//! The outline of an involute spur gear, as a closed polyline in the gear's own plane.
//!
//! Pure geometry from the public involute equations (ISO 53 proportions: addendum `m (1 + x)`,
//! dedendum `m (1.25 - x)`, profile shift `x`). The gear is centred on the origin with the
//! middle of one tooth on +x. Each flank is sampled, so the tooth is a polyline whose points
//! all lie on the true involute.

use std::f64::consts::{PI, TAU};

/// What defines a spur gear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GearSpec {
    /// Module (mm): pitch diameter divided by tooth count.
    pub module: f64,
    /// Number of teeth.
    pub teeth: u32,
    /// Pressure angle (radians).
    pub pressure_angle: f64,
    /// Profile shift coefficient `x` (0 for a standard gear).
    pub shift: f64,
    /// Backlash (mm) taken from the tooth thickness on the pitch circle.
    pub backlash: f64,
    /// Points along each flank, from the start of the involute to the tip.
    pub flank_points: u32,
}

impl GearSpec {
    pub fn new(module: f64, teeth: u32, pressure_angle_deg: f64) -> Self {
        Self { module, teeth, pressure_angle: pressure_angle_deg.to_radians(), shift: 0.0, backlash: 0.0, flank_points: 8 }
    }
    pub fn pitch_radius(&self) -> f64 {
        self.module * f64::from(self.teeth) / 2.0
    }
    pub fn base_radius(&self) -> f64 {
        self.pitch_radius() * self.pressure_angle.cos()
    }
    pub fn tip_radius(&self) -> f64 {
        self.pitch_radius() + self.module * (1.0 + self.shift)
    }
    pub fn root_radius(&self) -> f64 {
        self.pitch_radius() - self.module * (1.25 - self.shift)
    }
}

fn inv(a: f64) -> f64 {
    a.tan() - a
}

/// Half the angle one tooth spans at radius `r` (radians), or `None` below the base circle.
fn half_tooth_angle(g: &GearSpec, r: f64) -> Option<f64> {
    let rb = g.base_radius();
    if r < rb {
        return None;
    }
    let thickness = g.module * (PI / 2.0 + 2.0 * g.shift * g.pressure_angle.tan()) - g.backlash;
    let at_base = thickness / (2.0 * g.pitch_radius()) + inv(g.pressure_angle);
    Some(at_base - inv((rb / r).clamp(-1.0, 1.0).acos()))
}

/// The closed outline, counter-clockwise, without repeating the first point.
pub fn outline(g: &GearSpec) -> Result<Vec<[f64; 2]>, String> {
    let finite = g.module.is_finite() && g.pressure_angle.is_finite() && g.shift.is_finite() && g.backlash.is_finite();
    if !finite || g.module <= 0.0 {
        return Err("the module must be a positive number".into());
    }
    if g.teeth < 6 || g.teeth > 1000 {
        return Err("the tooth count must be between 6 and 1000".into());
    }
    if !(g.pressure_angle > 0.0 && g.pressure_angle <= 35f64.to_radians()) {
        return Err("the pressure angle must be above 0 and at most 35 degrees".into());
    }
    if g.flank_points < 2 || g.flank_points > 64 {
        return Err("a flank needs between 2 and 64 points".into());
    }
    let (rb, rf, ra) = (g.base_radius(), g.root_radius(), g.tip_radius());
    if !(rf > 0.0) {
        return Err("the root circle is inside the axis: use more teeth or a smaller profile shift".into());
    }
    let tip_half = half_tooth_angle(g, ra).ok_or("the tip circle is inside the base circle")?;
    if tip_half <= 0.0 {
        return Err("the teeth come to a point before the tip: use more teeth, a smaller pressure angle or less profile shift".into());
    }
    // The involute starts on the base circle, or on the root circle when that is larger.
    let start = rb.max(rf);
    let step = (ra - start) / f64::from(g.flank_points - 1);
    let mut flank = Vec::with_capacity(g.flank_points as usize + 1);
    // Below the base circle the flank runs straight along the radius to the root circle.
    if rf < rb {
        let base_half = half_tooth_angle(g, rb).ok_or("the base circle is inconsistent")?;
        flank.push((rf, base_half));
    }
    for i in 0..g.flank_points {
        let r = if i + 1 == g.flank_points { ra } else { start + step * f64::from(i) };
        let half = half_tooth_angle(g, r).ok_or("a flank point is inside the base circle")?;
        flank.push((r, half));
    }
    let pitch = TAU / f64::from(g.teeth);
    let mut out = Vec::with_capacity(g.teeth as usize * flank.len() * 2);
    for t in 0..g.teeth {
        let c = pitch * f64::from(t);
        let at = |r: f64, a: f64| [r * (c + a).cos(), r * (c + a).sin()];
        // Up the right flank, then down the left; the chord between two teeth is the root.
        out.extend(flank.iter().map(|&(r, h)| at(r, -h)));
        out.extend(flank.iter().rev().map(|&(r, h)| at(r, h)));
    }
    if out.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Err("the gear outline is not finite".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius(p: &[f64; 2]) -> f64 {
        p[0].hypot(p[1])
    }

    #[test]
    fn a_standard_gear_has_the_textbook_circles() {
        let g = GearSpec::new(2.0, 20, 20.0);
        assert!((g.pitch_radius() - 20.0).abs() < 1e-12);
        assert!((g.base_radius() - 20.0 * 20f64.to_radians().cos()).abs() < 1e-12);
        assert!((g.tip_radius() - 22.0).abs() < 1e-12);
        assert!((g.root_radius() - 17.5).abs() < 1e-12);
    }

    #[test]
    fn every_point_lies_between_the_root_and_tip_circles() {
        let g = GearSpec::new(2.0, 24, 20.0);
        let pts = outline(&g).unwrap();
        let (lo, hi) = (g.root_radius().min(g.base_radius()) - 1e-9, g.tip_radius() + 1e-9);
        assert!(pts.iter().all(|p| (lo..=hi).contains(&radius(p))));
        assert!((pts.iter().map(radius).fold(0.0, f64::max) - g.tip_radius()).abs() < 1e-9);
    }

    #[test]
    fn the_outline_repeats_every_tooth_pitch() {
        let g = GearSpec::new(1.5, 18, 20.0);
        let pts = outline(&g).unwrap();
        let per = pts.len() / 18;
        let turn = TAU / 18.0;
        for (i, p) in pts.iter().enumerate().skip(per) {
            let q = pts[i - per];
            let (c, s) = (turn.cos(), turn.sin());
            let rot = [q[0] * c - q[1] * s, q[0] * s + q[1] * c];
            assert!((rot[0] - p[0]).abs() < 1e-9 && (rot[1] - p[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn the_tooth_is_half_a_pitch_thick_on_the_pitch_circle() {
        // Standard gear, no backlash: thickness on the pitch circle is pi m / 2.
        let g = GearSpec::new(2.0, 30, 20.0);
        let half = half_tooth_angle(&g, g.pitch_radius()).unwrap();
        assert!((2.0 * half * g.pitch_radius() - PI * g.module / 2.0).abs() < 1e-9);
    }

    #[test]
    fn backlash_thins_the_tooth_by_that_amount() {
        let mut g = GearSpec::new(2.0, 30, 20.0);
        g.backlash = 0.1;
        let half = half_tooth_angle(&g, g.pitch_radius()).unwrap();
        assert!((2.0 * half * g.pitch_radius() - (PI * g.module / 2.0 - 0.1)).abs() < 1e-9);
    }

    #[test]
    fn flank_points_lie_on_the_involute_of_the_base_circle() {
        // A point at radius r on the flank is at angle inv(acos(rb / r)) past the base-circle start.
        let g = GearSpec::new(2.0, 24, 20.0);
        let pts = outline(&g).unwrap();
        let rb = g.base_radius();
        let p = pts[3];
        let r = radius(&p);
        let start = half_tooth_angle(&g, rb).unwrap();
        let expect = -(start - inv((rb / r).acos()));
        assert!((p[1].atan2(p[0]) - expect).abs() < 1e-9);
    }

    #[test]
    fn hostile_parameters_are_refused_not_panics() {
        for (m, z, a) in [
            (0.0, 20, 20.0),
            (-1.0, 20, 20.0),
            (f64::NAN, 20, 20.0),
            (f64::INFINITY, 20, 20.0),
            (2.0, 0, 20.0),
            (2.0, 5, 20.0),
            (2.0, 5000, 20.0),
            (2.0, 20, 0.0),
            (2.0, 20, 90.0),
            (2.0, 20, f64::NAN),
        ] {
            assert!(outline(&GearSpec::new(m, z, a)).is_err(), "{m} {z} {a}");
        }
        let mut g = GearSpec::new(2.0, 20, 20.0);
        g.shift = 50.0;
        assert!(outline(&g).is_err());
        g.shift = f64::NAN;
        assert!(outline(&g).is_err());
        g.shift = 0.0;
        g.backlash = 1e9;
        assert!(outline(&g).is_err());
    }

    #[test]
    fn a_gear_too_small_for_its_pressure_angle_still_gives_a_clean_outline() {
        let g = GearSpec::new(1.0, 8, 25.0);
        let pts = outline(&g).unwrap();
        assert!(pts.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
    }
}
