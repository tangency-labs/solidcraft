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
    /// An internal gear: the teeth point inward from a ring (profile shift is not used).
    pub internal: bool,
    /// Clearance coefficient `c`: the root lies `m (1 + c - x)` inside the pitch circle (0.25 standard).
    pub clearance: f64,
    /// Radius (mm) rounding each corner where a flank meets the root; 0 leaves it sharp.
    pub root_fillet: f64,
    /// Radius (mm) rounding each corner where a flank meets the tip; 0 leaves it sharp.
    pub tip_round: f64,
    /// The backlash makes the teeth thicker instead of thinner (for a mate that carries the clearance).
    pub thicker: bool,
}

impl GearSpec {
    pub fn new(module: f64, teeth: u32, pressure_angle_deg: f64) -> Self {
        Self {
            module,
            teeth,
            pressure_angle: pressure_angle_deg.to_radians(),
            shift: 0.0,
            backlash: 0.0,
            flank_points: 8,
            internal: false,
            clearance: 0.25,
            root_fillet: 0.0,
            tip_round: 0.0,
            thicker: false,
        }
    }
    pub fn pitch_radius(&self) -> f64 {
        self.module * f64::from(self.teeth) / 2.0
    }
    pub fn base_radius(&self) -> f64 {
        self.pitch_radius() * self.pressure_angle.cos()
    }
    /// The tooth tips: outside an external gear, inside an internal one.
    pub fn tip_radius(&self) -> f64 {
        if self.internal {
            return self.pitch_radius() - self.module;
        }
        self.pitch_radius() + self.module * (1.0 + self.shift)
    }
    /// The tooth roots: inside an external gear, outside an internal one.
    pub fn root_radius(&self) -> f64 {
        if self.internal {
            return self.pitch_radius() + self.module * (1.0 + self.clearance);
        }
        self.pitch_radius() - self.module * (1.0 + self.clearance - self.shift)
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
    let backlash = if g.thicker { -g.backlash } else { g.backlash };
    let thickness = g.module * (PI / 2.0 + 2.0 * g.shift * g.pressure_angle.tan()) - backlash;
    let at_base = thickness / (2.0 * g.pitch_radius()) + inv(g.pressure_angle);
    Some(at_base - inv((rb / r).clamp(-1.0, 1.0).acos()))
}

/// The closed outline, counter-clockwise, without repeating the first point. For an internal
/// gear it is the edge of the hole in the ring: the tooth space of a ring gear has the shape of
/// an external tooth, so it is built as one, with the clearance on the tip side and the
/// backlash widening the space.
pub fn outline(g: &GearSpec) -> Result<Vec<[f64; 2]>, String> {
    if !g.backlash.is_finite() || g.backlash < 0.0 || g.backlash >= g.module * PI / 2.0 {
        return Err("the backlash must be from 0 up to a quarter of the circular pitch".into());
    }
    if !g.clearance.is_finite() || !(0.0..=0.6).contains(&g.clearance) {
        return Err("the clearance must be from 0 to 0.6 of the module".into());
    }
    for (what, r) in [("root fillet", g.root_fillet), ("tip rounding", g.tip_round)] {
        if !r.is_finite() || r < 0.0 || r > g.module {
            return Err(format!("the {what} radius must be from 0 up to the module"));
        }
    }
    if !g.internal {
        return build(g, g.tip_radius(), g.root_radius());
    }
    if g.shift != 0.0 {
        return Err("profile shift is for external gears".into());
    }
    let space = GearSpec { internal: false, backlash: -g.backlash, root_fillet: g.tip_round, tip_round: g.root_fillet, ..*g };
    build(&space, g.root_radius(), g.tip_radius())
}

fn build(g: &GearSpec, ra: f64, rf: f64) -> Result<Vec<[f64; 2]>, String> {
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
    let rb = g.base_radius();
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
    // A thicker tooth must still leave room for its neighbour at the root.
    if flank.iter().any(|&(_, h)| h >= pitch / 2.0) {
        return Err("the teeth would run into each other: use less backlash or profile shift".into());
    }
    let mut out = Vec::with_capacity(g.teeth as usize * flank.len() * 2);
    for t in 0..g.teeth {
        let c = pitch * f64::from(t);
        let at = |r: f64, a: f64| [r * (c + a).cos(), r * (c + a).sin()];
        // Up the right flank, then down the left; the chord between two teeth is the root.
        out.extend(flank.iter().map(|&(r, h)| at(r, -h)));
        out.extend(flank.iter().rev().map(|&(r, h)| at(r, h)));
    }
    if g.root_fillet > 0.0 || g.tip_round > 0.0 {
        out = round_corners(&out, flank.len(), g.root_fillet, g.tip_round)?;
    }
    if out.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Err("the gear outline is not finite".into());
    }
    Ok(out)
}

/// Which corner of a tooth a point of the outline is, if any: `n` points make one flank, and a tooth runs
/// up one flank and down the other (`2 n` points), so its tips are points `n - 1` and `n` and its roots
/// are the last point and the next tooth's first.
#[derive(Clone, Copy, PartialEq)]
enum Corner {
    Tip,
    Root,
}

fn corner_of(i: usize, n: usize) -> Option<Corner> {
    let k = i % (2 * n);
    if k == n - 1 || k == n {
        Some(Corner::Tip)
    } else if k == 0 || k == 2 * n - 1 {
        Some(Corner::Root)
    } else {
        None
    }
}

/// The point `t` along the outline from vertex `i` going `dir` (1 or -1 steps), with the vertices passed on
/// the way. The next corner is a barrier: `None` once `t` would reach it.
fn walk(pts: &[[f64; 2]], n: usize, i: usize, dir: isize, t: f64) -> Option<([f64; 2], usize)> {
    let len = pts.len();
    let (mut cur, mut left, mut passed) = (i, t, 0);
    loop {
        let next = (cur as isize + dir).rem_euclid(len as isize) as usize;
        let seg = (pts[next][0] - pts[cur][0]).hypot(pts[next][1] - pts[cur][1]);
        if left <= seg {
            let f = if seg > 0.0 { left / seg } else { 0.0 };
            return Some(([pts[cur][0] + (pts[next][0] - pts[cur][0]) * f, pts[cur][1] + (pts[next][1] - pts[cur][1]) * f], passed));
        }
        left -= seg;
        if corner_of(next, n).is_some() || next == i {
            return None;
        }
        cur = next;
        passed += 1;
    }
}

/// How far along the outline from vertex `i` going `dir` before the next corner, in mm.
fn room(pts: &[[f64; 2]], n: usize, i: usize, dir: isize) -> f64 {
    let len = pts.len();
    let (mut cur, mut total) = (i, 0.0);
    for _ in 0..len {
        let next = (cur as isize + dir).rem_euclid(len as isize) as usize;
        total += (pts[next][0] - pts[cur][0]).hypot(pts[next][1] - pts[cur][1]);
        if corner_of(next, n).is_some() {
            break;
        }
        cur = next;
    }
    total
}

/// Round the flank-to-root and flank-to-tip corners of every tooth with circular arcs: the arc of radius
/// `rho` that touches the outline `t` back and `t` ahead of the corner, the outline between dropped. The
/// flank is a curve, so `t` is found from the chords to those two points, and each side may give up just
/// under half of the way to the next corner (which may be rounded too).
fn round_corners(pts: &[[f64; 2]], n: usize, root: f64, tip: f64) -> Result<Vec<[f64; 2]>, String> {
    let len = pts.len();
    let mut skip = vec![false; len];
    let mut arcs: Vec<Option<Vec<[f64; 2]>>> = vec![None; len];
    for i in 0..len {
        let (rho, what) = match corner_of(i, n) {
            Some(Corner::Tip) => (tip, "tip rounding"),
            Some(Corner::Root) => (root, "root fillet"),
            None => (0.0, ""),
        };
        if rho <= 0.0 {
            continue;
        }
        let v = pts[i];
        let angle = |a: [f64; 2], b: [f64; 2]| {
            let (ua, ub) = ([a[0] - v[0], a[1] - v[1]], [b[0] - v[0], b[1] - v[1]]);
            let (la, lb) = (ua[0].hypot(ua[1]), ub[0].hypot(ub[1]));
            if !(la > 1e-9 && lb > 1e-9) {
                return None;
            }
            let u = ([ua[0] / la, ua[1] / la], [ub[0] / lb, ub[1] / lb]);
            Some((u.0, u.1, (u.0[0] * u.1[0] + u.0[1] * u.1[1]).clamp(-1.0, 1.0).acos()))
        };
        let fold = || format!("the {what} cannot be rounded where the outline folds back");
        let (_, _, mut phi) = angle(pts[(i + len - 1) % len], pts[(i + 1) % len]).ok_or_else(fold)?;
        let reach = |dir: isize| room(pts, n, i, dir) * 0.49;
        let t_max = reach(-1).min(reach(1));
        if !(0.05..=PI - 0.05).contains(&phi) {
            return Err(fold());
        }
        let too_big = |phi: f64| format!("the {what} radius is too large for the tooth: at most {:.3} mm", t_max * (phi / 2.0).tan());
        let mut t = rho / (phi / 2.0).tan();
        let mut ends = None;
        for _ in 0..6 {
            if t > t_max {
                return Err(too_big(phi));
            }
            let (Some((a, back)), Some((b, fwd))) = (walk(pts, n, i, -1, t), walk(pts, n, i, 1, t)) else {
                return Err(too_big(phi));
            };
            let (_, _, p) = angle(a, b).ok_or_else(fold)?;
            if !(0.05..=PI - 0.05).contains(&p) {
                return Err(fold());
            }
            (phi, ends) = (p, Some((a, b, back, fwd)));
            t = rho / (phi / 2.0).tan();
        }
        if t > t_max {
            return Err(too_big(phi));
        }
        let Some((a, b, back, fwd)) = ends else { return Err(fold()) };
        let (ua, ub, _) = angle(a, b).ok_or_else(fold)?;
        let (t1, t2) = ([v[0] + ua[0] * t, v[1] + ua[1] * t], [v[0] + ub[0] * t, v[1] + ub[1] * t]);
        let bis = [ua[0] + ub[0], ua[1] + ub[1]];
        let bl = bis[0].hypot(bis[1]);
        let reach_c = rho / (phi / 2.0).sin();
        let c = [v[0] + bis[0] / bl * reach_c, v[1] + bis[1] / bl * reach_c];
        let (s1, s2) = ((t1[1] - c[1]).atan2(t1[0] - c[0]), (t2[1] - c[1]).atan2(t2[0] - c[0]));
        let mut d = s2 - s1;
        while d > PI {
            d -= TAU;
        }
        while d < -PI {
            d += TAU;
        }
        let steps = ((d.abs() / (PI / 12.0)).ceil() as usize).clamp(2, 8);
        arcs[i] = Some(
            (0..=steps)
                .map(|k| {
                    let ang = s1 + d * k as f64 / steps as f64;
                    [c[0] + rho * ang.cos(), c[1] + rho * ang.sin()]
                })
                .collect(),
        );
        skip[i] = true;
        for k in 1..=back {
            skip[(i + len - k) % len] = true;
        }
        for k in 1..=fwd {
            skip[(i + k) % len] = true;
        }
    }
    let mut out = Vec::with_capacity(len * 2);
    for i in 0..len {
        if let Some(arc) = &arcs[i] {
            out.extend(arc.iter().copied());
        } else if !skip[i] {
            out.push(pts[i]);
        }
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
    fn an_internal_gear_has_its_teeth_pointing_inward() {
        let mut g = GearSpec::new(2.0, 40, 20.0);
        g.internal = true;
        assert!((g.tip_radius() - 38.0).abs() < 1e-12 && (g.root_radius() - 42.5).abs() < 1e-12);
        let pts = outline(&g).unwrap();
        let (lo, hi) = (g.tip_radius() - 1e-9, g.root_radius() + 1e-9);
        assert!(pts.iter().all(|p| (lo..=hi).contains(&radius(p))));
        assert!((pts.iter().map(radius).fold(f64::MAX, f64::min) - g.tip_radius()).abs() < 1e-9);
        assert!((pts.iter().map(radius).fold(0.0, f64::max) - g.root_radius()).abs() < 1e-9);
    }

    #[test]
    fn an_internal_gear_space_is_as_wide_as_the_external_tooth_it_meshes_with() {
        // On the pitch circle the ring's space equals the external tooth, plus the backlash.
        let ext = GearSpec::new(2.0, 40, 20.0);
        let space = |backlash: f64| {
            let v = GearSpec { backlash: -backlash, ..ext };
            2.0 * half_tooth_angle(&v, v.pitch_radius()).unwrap() * v.pitch_radius()
        };
        assert!((space(0.0) - PI * 2.0 / 2.0).abs() < 1e-9);
        assert!((space(0.1) - (PI + 0.1)).abs() < 1e-9);
    }

    #[test]
    fn rounded_corners_stay_between_the_circles_and_a_standard_fillet_fits() {
        // The usual root fillet is about 0.38 of the module; the tip is eased by a fifth of it.
        for points in [8, 16, 32] {
            let mut g = GearSpec::new(2.0, 24, 20.0);
            g.flank_points = points;
            let sharp = outline(&g).unwrap();
            g.root_fillet = 0.76;
            g.tip_round = 0.4;
            let pts = outline(&g).unwrap();
            // The root between two teeth is a chord, a little inside the root circle; a fillet tangent to it
            // may follow it in by that much.
            let (lo, hi) = (g.root_radius().min(g.base_radius()) - 0.1, g.tip_radius() + 1e-9);
            assert!(pts.iter().all(|p| (lo..=hi).contains(&radius(p))), "{points}");
            assert!(pts.len() > sharp.len() && pts.iter().all(|p| p[0].is_finite() && p[1].is_finite()), "{points}");
            // The tip circle is no longer touched: the rounding takes the corner off.
            assert!(pts.iter().map(radius).fold(0.0, f64::max) < g.tip_radius() - 1e-6, "{points}");
        }
    }

    #[test]
    fn a_rounding_too_large_for_the_tooth_is_refused_with_the_most_it_allows() {
        let mut g = GearSpec::new(2.0, 24, 20.0);
        g.tip_round = 1.9;
        let e = outline(&g).unwrap_err();
        assert!(e.contains("tip rounding radius is too large") && e.contains("at most"), "{e}");
        g.tip_round = 0.0;
        g.root_fillet = 2.0;
        assert!(outline(&g).unwrap_err().contains("root fillet radius is too large"));
    }

    #[test]
    fn clearance_moves_the_roots_and_the_thicker_direction_widens_the_teeth() {
        let mut g = GearSpec::new(2.0, 24, 20.0);
        g.clearance = 0.4;
        assert!((g.root_radius() - (24.0 - 2.0 * 1.4)).abs() < 1e-12);
        g.internal = true;
        assert!((g.root_radius() - (24.0 + 2.0 * 1.4)).abs() < 1e-12);
        g.internal = false;
        g.backlash = 0.2;
        let thin = half_tooth_angle(&g, g.pitch_radius()).unwrap();
        g.thicker = true;
        let thick = half_tooth_angle(&g, g.pitch_radius()).unwrap();
        assert!((2.0 * thin * g.pitch_radius() - (PI - 0.2)).abs() < 1e-9);
        assert!((2.0 * thick * g.pitch_radius() - (PI + 0.2)).abs() < 1e-9);
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
        g.backlash = -0.1;
        assert!(outline(&g).is_err());
        g.backlash = 0.0;
        g.internal = true;
        g.shift = 0.5;
        assert!(outline(&g).is_err());
    }

    #[test]
    fn a_gear_too_small_for_its_pressure_angle_still_gives_a_clean_outline() {
        let g = GearSpec::new(1.0, 8, 25.0);
        let pts = outline(&g).unwrap();
        assert!(pts.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
    }
}
