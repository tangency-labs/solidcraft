//! Bevel planetary sets: a bevel sun with N bevel planets whose shafts all meet the sun's at one
//! apex, tilted at the cone angle Σ (sun axis to planet axis; 90° gives a car differential's
//! spider gears), and optionally a second member coaxial with the sun that meshes the planets on
//! their far side: a second sun (side gear, a full differential) or an internal bevel ring (a
//! bevel epicyclic).
//!
//! # Geometry
//!
//! The apex is the set's origin; the sun turns about +z with its teeth below the apex (its back
//! at z = −Re·cos δs), so seen from the apex the sun lies along −z. A planet at azimuth θ has its
//! axis in the plane through the sun axis at θ, at Σ from −z: from the apex towards the planet
//! u = (sin Σ cos θ, sin Σ sin θ, −cos Σ). Each planet–sun pair is an ordinary bevel pair of
//! shaft angle Σ:
//!
//!   tan δs = sin Σ / (zp/zs + cos Σ),  δp = Σ − δs,  Re = m·zs / (2 sin δs) = m·zp / (2 sin δp).
//!
//! The planet's pitch cone touches the sun's along its near element (at δs from −z) and has a
//! second element in the same plane on its far side, at δf = Σ + δp from −z. A member coaxial
//! with the sun that meshes the planets there has its pitch cone through that element, so its
//! cone angle is δf measured from −z, its cone distance Re and its pitch the planets':
//!
//!   zf = 2·Re·sin δf / m = zp·sin(Σ + δp) / sin δp = zs + 2·zp·cos Σ.
//!
//! * δf > 90°: the cone opens upwards, it is an external bevel facing the sun (cone angle
//!   180° − δf about −z, shaft angle 180° − Σ to the planets): the second sun of a differential.
//!   At Σ = 90° it has zs teeth: the classic differential with equal side gears.
//! * δf < 90°: the cone opens downwards around the planets: an internal bevel ring, shaft angle
//!   Σ to the planets with δf − δp = Σ (Σ → 0 is the flat planetary, zr = zs + 2·zp).
//! * δf = 90°: a crown ring (not built).
//!
//! zf must be whole, so with a second member Σ is set by the tooth counts:
//! cos Σ = (zf − zs) / (2·zp). Asked for another angle, the plan takes the nearest whole zf (or
//! the given `teeth3`), builds at the angle that makes, and warns how far it moved.
//!
//! # Phasing and the assembly condition
//!
//! Every member's frame has z pointing at the apex. A member with z teeth and tooth 0 at angle φ
//! has the tooth fraction f = (α − φ)·z / 2π at the local angle α of a pitch element (0: a tooth
//! centred there, ½: a space). Rolling on a shared element keeps f₁ + f₂ fixed for an external
//! pair (they turn opposite ways about their apex-pointing axes) and f₁ − f₂ for an internal one;
//! a mesh needs a tooth against a space: f₁ + f₂ ≡ ½ (external), f_planet − f_ring ≡ ½ (internal).
//!
//! * sun (φs = 0) and planet k at θk: φk = π + π/zp + zs·θk / zp. Any azimuth: a single sun has
//!   no assembly condition, only the planets' room.
//! * second sun (frame z = −z, so azimuth θ is local angle −θ): with planet k's far element at
//!   local angle 0, θk·(zs + zf)/2π + φf·zf/2π + zp/2 ≡ 0, so all planets mesh both suns only when
//!   θk − θ0 is a whole multiple of 2π / (zs + zf); evenly spaced N planets need (zs + zf)/N
//!   whole (the differential's 2·z/N).
//! * ring (frame z = +z): θk·(zs + zf)/2π − φf·zf/2π + zp/2 ≡ 0: the same condition,
//!   (zs + zr)/N whole, as a flat planetary.
//!
//! When it fails the planets go to the nearest positions that mesh (like `planetary()`), with a
//! warning.
//!
//! # Teeth
//!
//! All members are cut by the crown gear of [`crate::octoid`] as the bevel pair is (straight, or
//! spiral with the logarithmic spiral of `spiral_angle`; sun and second sun one hand, planets
//! and ring the other). The ring is a blank with an external bevel cut out of it whose teeth are
//! the ring's spaces: a gear cut by the same crown as the planet from the same side of the
//! common tangent plane, so the two are conjugate, the crown's teeth standing for the ring's.

use std::f64::consts::{PI, TAU};

use super::{
    Body, ConeEnds, ConeFace, Frame, Op, Plan, PreviewGear, Spec, Step, Unit, bevel_hub, bevel_involute, cone_faces, lined_up_cone_teeth, name,
    tooth_warnings, wanted,
};
use crate::axes::num;
use crate::geom::{Outline, Region, V2};
use crate::involute::{
    Internal, PROFILE_TOLERANCE, ToothSides, external_outline, mesh_phase, mesh_phase_internal, side_points, sided_outline, teeth_outline,
};
use crate::mesh3d::MeshPair3;
use crate::octoid::Crown;

/// What meshes the planets besides the sun.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum BevelPlanetMode {
    /// The sun and the planets only.
    #[default]
    Single,
    /// A second sun (side gear) opposite the first: a differential.
    Differential,
    /// An internal bevel ring round the planets: a bevel epicyclic.
    Ring,
}

impl BevelPlanetMode {
    pub const ALL: [BevelPlanetMode; 3] = [BevelPlanetMode::Single, BevelPlanetMode::Differential, BevelPlanetMode::Ring];
    pub fn name(self) -> &'static str {
        match self {
            BevelPlanetMode::Single => "single",
            BevelPlanetMode::Differential => "differential",
            BevelPlanetMode::Ring => "ring",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            BevelPlanetMode::Single => "Sun and planets",
            BevelPlanetMode::Differential => "Differential (second sun)",
            BevelPlanetMode::Ring => "Bevel ring (epicyclic)",
        }
    }
    pub fn parse(s: &str) -> Option<BevelPlanetMode> {
        BevelPlanetMode::ALL.iter().copied().find(|k| k.name() == s)
    }
    /// The far member's name.
    fn far(self) -> &'static str {
        if self == BevelPlanetMode::Ring { "ring" } else { "sun 2" }
    }
}

/// The set's cones and planet places, worked out before any tooth is cut.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub mode: BevelPlanetMode,
    /// The cone angle built (radians): the one asked for, or the one the second member's teeth
    /// allow.
    pub sigma: f64,
    pub z_sun: u32,
    pub z_planet: u32,
    /// The far member's teeth (0: none).
    pub z_far: u32,
    /// Pitch cone angles (radians): sun and planet, and the far element's angle from −z (the
    /// second sun's own cone angle is π − this).
    pub delta_sun: f64,
    pub delta_planet: f64,
    pub delta_far: f64,
    /// Cone distance at the large end and the face width built (mm).
    pub re: f64,
    pub face: f64,
    /// The planets' azimuths (radians).
    pub azimuths: Vec<f64>,
    /// The mean spiral angle (radians, the sun's hand).
    pub spiral: f64,
    /// The least angle between two neighbouring planet axes (radians; π for one planet).
    pub least_axis_angle: f64,
    pub warnings: Vec<String>,
}

fn deg(v: f64) -> String {
    num(v.to_degrees())
}

/// The geometry of a bevel planetary spec, or why it can't be built.
pub fn geometry(s: &Spec) -> Result<Geometry, String> {
    let mode = s.bevel_planet_mode;
    let (zs, zp) = (s.teeth, s.teeth2);
    let (fs, fp) = (f64::from(zs), f64::from(zp));
    let n = s.planets.max(1);
    let mut warnings = Vec::new();
    let asked = s.shaft_angle.to_radians();
    let mut sigma = asked;
    let mut z_far = 0;
    if mode != BevelPlanetMode::Single {
        let what = if mode == BevelPlanetMode::Ring { "ring" } else { "second sun" };
        z_far = if s.teeth3 > 0 { s.teeth3 } else { (fs + 2.0 * fp * asked.cos()).round().max(3.0) as u32 };
        let c = (f64::from(z_far) - fs) / (2.0 * fp);
        if c.abs() >= 0.996 {
            return Err(format!(
                "a {what} of {z_far} teeth needs a cone angle with cos Σ = ({z_far} − {zs}) / (2 × {zp}) = {}: out of reach; {what} teeth = sun + 2 × planet × cos Σ ({} at {}°)",
                num(c),
                num(fs + 2.0 * fp * asked.cos()),
                num(s.shaft_angle)
            ));
        }
        sigma = c.acos();
        if (sigma - asked).abs() > 0.005f64.to_radians() {
            let near = |z: f64| {
                let c = (z - fs) / (2.0 * fp);
                if c.abs() < 0.996 { format!("{} teeth → {}°", z, deg(c.acos())) } else { String::new() }
            };
            let exact = fs + 2.0 * fp * asked.cos();
            let options: Vec<String> = [exact.floor(), exact.ceil()]
                .iter()
                .filter(|z| **z >= 3.0 && **z != f64::from(z_far))
                .map(|z| near(*z))
                .filter(|t| !t.is_empty())
                .collect();
            warnings.push(format!(
                "the cone angle is {}° (not {}°): a {what} of {z_far} teeth fits only there ({what} teeth = sun + 2 × planet × cos Σ = {}){}",
                deg(sigma),
                num(s.shaft_angle),
                num(exact),
                if options.is_empty() { String::new() } else { format!("; other choices: {}", options.join(", ")) }
            ));
        }
    }
    let ds = sigma.sin().atan2(fp / fs + sigma.cos());
    let dp = sigma - ds;
    let df = sigma + dp;
    let ok = |d: f64| (2.0f64.to_radians()..=85.0f64.to_radians()).contains(&d);
    if !ok(ds) || !ok(dp) {
        return Err(format!(
            "pitch cone angles of {}° (sun) and {}° (planet): each must be 2° to 85°; change the cone angle or bring the tooth counts closer",
            deg(ds),
            deg(dp)
        ));
    }
    match mode {
        BevelPlanetMode::Differential if !ok(PI - df) => {
            return Err(if df <= PI / 2.0 + 5f64.to_radians() {
                format!(
                    "at a {}° cone angle the planets' far side ({}° from the sun's side) faces the sun's way: the member there is a ring, not a second sun; use ring mode, a larger cone angle or bigger planets",
                    deg(sigma),
                    deg(df)
                )
            } else {
                format!("the second sun's pitch cone angle ({}°) is under 2°: use fewer planet teeth or a smaller cone angle", deg(PI - df))
            });
        }
        BevelPlanetMode::Ring if !ok(df) => {
            return Err(format!(
                "at a {}° cone angle the planets' far side is {}° from the sun's side: a ring there would be flatter than 85° (a crown) or face away (that is a second sun: differential mode); lower the cone angle or use smaller planets",
                deg(sigma),
                deg(df)
            ));
        }
        _ => {}
    }
    let re = s.module * fs / (2.0 * ds.sin());
    let face = s.face_width.min(0.45 * re);
    let th0 = s.first_azimuth.to_radians();
    let azimuths: Vec<f64> = if mode == BevelPlanetMode::Single {
        (0..n).map(|k| th0 + TAU * f64::from(k) / f64::from(n)).collect()
    } else {
        let total = f64::from(zs + z_far);
        if (zs + z_far) % n != 0 {
            warnings.push(format!(
                "(sun + {}) / planets = {}/{n} is not whole: the planets are spaced unevenly (the nearest places where they mesh)",
                mode.far(),
                zs + z_far
            ));
        }
        (0..n).map(|k| th0 + TAU * (f64::from(k) * total / f64::from(n)).round() / total).collect()
    };
    // Neighbouring planets: their tip cones about the apex must not meet.
    let mut sorted: Vec<f64> = azimuths.iter().map(|a| (a - th0).rem_euclid(TAU)).collect();
    sorted.sort_by(f64::total_cmp);
    let mut least = PI;
    if n > 1 {
        for i in 0..sorted.len() {
            let d = if i + 1 < sorted.len() { sorted[i + 1] - sorted[i] } else { sorted[0] + TAU - sorted[i] };
            let c = sigma.cos().powi(2) + sigma.sin().powi(2) * d.cos();
            least = least.min(c.clamp(-1.0, 1.0).acos());
        }
    }
    let spiral = s.spiral_angle.to_radians();
    let tip = (s.rack.addendum * s.module * spiral.cos() / re).atan();
    let need = 2.0 * (dp + tip) + 0.25 * s.module / re;
    if n > 1 && least < need {
        warnings.push(format!(
            "neighbouring planets collide: their axes are {}° apart, their tips need {}°; use fewer or smaller planets, or a larger cone angle",
            deg(least),
            deg(need)
        ));
    }
    Ok(Geometry {
        mode,
        sigma,
        z_sun: zs,
        z_planet: zp,
        z_far,
        delta_sun: ds,
        delta_planet: dp,
        delta_far: df,
        re,
        face,
        azimuths,
        spiral,
        least_axis_angle: least,
        warnings,
    })
}

/// The flank gap the backlash asks for (least, at the small end, and most; mm): every member is
/// thinned by the backlash at its large end, as a bevel pair's.
pub fn expected_gap(s: &Spec) -> (f64, f64) {
    let Ok(g) = geometry(s) else { return (0.0, 0.0) };
    let w = s.backlash * s.pressure_angle.to_radians().cos() * g.spiral.cos();
    let (lo, hi) = super::faces_overlap(s, g.re);
    (w * lo / g.re, w * hi / g.re)
}

/// The placement and cone angle from a side-view sketch: the apex (a point), the direction from
/// it along the sun's shaft towards the sun, and along the first planet's shaft towards that
/// planet. Build with `origin` 0, `placement` this frame, `shaft_angle` the angle returned
/// (degrees) and `first_azimuth` 0: the first planet then lies in the sketch's plane.
pub fn placement_from_shafts(apex: [f64; 3], sun_dir: [f64; 3], planet_dir: [f64; 3]) -> Result<(Frame, f64), String> {
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (ls, lp) = (len(sun_dir), len(planet_dir));
    if !(ls > 1e-12 && lp > 1e-12) {
        return Err("the shafts need a direction".into());
    }
    let z = sun_dir.map(|v| -v / ls);
    let u = planet_dir.map(|v| v / lp);
    let along = u[0] * z[0] + u[1] * z[1] + u[2] * z[2];
    let x = [0, 1, 2].map(|i| u[i] - along * z[i]);
    let lx = len(x);
    if lx < 1e-9 {
        return Err("the planet's shaft lies along the sun's: they must meet at an angle".into());
    }
    let sigma = (-along).clamp(-1.0, 1.0).acos().to_degrees();
    Ok((Frame { origin: apex, x: x.map(|v| v / lx), z }, sigma))
}

/// One member's teeth: the taper from its large end (z = 0) towards the apex (+z), as
/// `bevel_member` makes them (or, for a ring, the tool cut out of its blank).
struct Member {
    steps: Vec<Step>,
    /// The outer tip radius as built (a ring's: its root on the large end's plane).
    ra: f64,
    zv: f64,
    /// Where the body ends along its axis (a ring: unused).
    ends: ConeEnds,
}

#[allow(clippy::too_many_arguments)]
fn member(s: &Spec, g: &Geometry, z: u32, delta: f64, spiral: f64, phase: f64, face: ConeFace, p: &mut Plan, who: &str) -> Option<Member> {
    let m = s.module;
    let re = g.re;
    let h = re * delta.cos();
    let zv = f64::from(z) / delta.cos();
    let v = bevel_involute(s, z, spiral, spiral, 0.0, zv);
    let mn = v.module;

    let crown = crown_of(s, g, delta, spiral);
    let ds = delta.sin();
    if crown.addendum * ds >= 0.5 * h || crown.dedendum >= 0.9 * re * delta.tan() {
        let need = crate::plan::min_cone_teeth(z, crown.addendum * ds / (0.5 * h), crown.dedendum / (0.9 * re * delta.tan()));
        p.refuse(format!("{who}: {}", crate::plan::too_tall_for_cone(delta, z, need, "the cone angle")));
        return None;
    }
    let rm = face.mid;
    let wind = if spiral.abs() > 1e-6 { spiral.tan() / ds } else { 0.0 };
    let skew = |rr: f64| if wind == 0.0 { 0.0 } else { wind * (rr.hypot(h) / rm).ln() };
    let (teeth, mut flank) = lined_up_cone_teeth(&crown, z, mn, &skew, phase, delta, re, face, wind, m);
    flank.undercut = v.undercut();
    tooth_warnings(p, who, &v, &flank);
    Some(Member { steps: teeth.steps, ra: teeth.tip_radius, zv, ends: teeth.ends })
}

/// The crown gear that cuts a member's spaces (as `bevel_member`'s, without profile shift).
fn crown_of(s: &Spec, g: &Geometry, delta: f64, spiral: f64) -> Crown {
    let mn = s.module * spiral.cos();
    Crown {
        delta,
        cone_distance: g.re,
        pressure_angle: (s.pressure_angle.to_radians().tan() / spiral.cos()).atan(),
        addendum: s.rack.addendum * mn,
        dedendum: s.rack.dedendum * mn,
        tip_radius: s.rack.root_radius * mn,
        half_thickness: PI * s.module / 4.0 + s.backlash / 2.0,
    }
}

/// The ring's teeth cut by the planet itself, as a hob: the planet's tooth (the crown-cut one of
/// [`member`], unspun by its spiral) is swept through the internal mesh and every place it
/// passes is ring space. Everything turns about the apex, so this is worked out as directions:
/// each planet point is a direction, seen from the ring (turned by the mesh) as a radius and an
/// angle on the ring's large-end plane; at each radius the space is as wide as the widest angle
/// the tooth reaches there, widened by the backlash. The result is the tool cut from the ring
/// blank: an external bevel whose teeth are the ring's spaces.
///
/// (A ring cut by the crown that cuts the planets, the way an external mate is, interferes with
/// them by about 0.035 modules near the planets' tips: the crown's envelope on an internal cone
/// is not the planets' conjugate.)
fn ring_member(s: &Spec, g: &Geometry, spiral: f64, phase: f64, p: &mut Plan) -> Option<Member> {
    let m = s.module;
    let (re, b) = (g.re, g.face);
    let (zp, zr) = (g.z_planet, g.z_far);
    let (dp, dr, sigma) = (g.delta_planet, g.delta_far, g.sigma);
    let mn = m * spiral.cos();
    let crown = crown_of(s, g, dp, spiral);
    let space = crown.space_sampler();
    let (rfp, rap) = crown.radii();
    let hp = re * dp.cos();
    let hr = re * dr.cos();
    let pitch_p = PI / f64::from(zp);
    // The planet tooth's outline on its large-end plane, root to tip to root, as (radius, angle).
    let k = 400;
    let mut tooth: Vec<(f64, f64)> = Vec::with_capacity(2 * k + 20);
    for i in 0..=k {
        let rr = rfp + (rap - rfp) * (1e-6 + (1.0 - 2e-6) * i as f64 / k as f64);
        if let Some(sp) = space(rr)
            && pitch_p - sp > 0.0
        {
            tooth.push((rr, -(pitch_p - sp)));
        }
    }
    let n_low = tooth.len();
    if n_low < 10 {
        p.refuse("ring: the planet's teeth couldn't be traced to cut the ring");
        return None;
    }
    let (r_last, tip_h) = (tooth[n_low - 1].0, -tooth[n_low - 1].1);
    for i in 1..20 {
        tooth.push((r_last, -tip_h + 2.0 * tip_h * i as f64 / 20.0));
    }
    for i in (0..n_low).rev() {
        tooth.push((tooth[i].0, -tooth[i].1));
    }
    // The first planet (azimuth 0) and the ring's tip and root radii.
    let (zax, xax) = ([-sigma.sin(), 0.0, sigma.cos()], [sigma.cos(), 0.0, sigma.sin()]);
    let (ha, hf) = (s.rack.addendum * mn, s.rack.dedendum * mn);
    let r_tip = hr * (dr - (ha / re).atan()).tan();
    let r_root = hr * (dr + (hf / re).atan()).tan();
    let bins = 600usize;
    let r_at = |j: usize| r_tip + (r_root - r_tip) * j as f64 / bins as f64;
    let mut widest: Vec<Option<f64>> = vec![None; bins + 1];
    let pitch_r = PI / f64::from(zr);
    // The tooth is in the ring's teeth only within a few pitches of the mesh: half a turn
    // each way covers it.
    let turns = 6000;
    for i in 0..=turns {
        let psi = -0.5 * PI + PI * i as f64 / turns as f64;
        let chi = psi * f64::from(zp) / f64::from(zr);
        let mut prev: Option<(f64, f64)> = None;
        for (rr, a) in &tooth {
            let (sn, c) = (a + psi).sin_cos();
            let d = [-hp * zax[0] + rr * c * xax[0], rr * sn, -hp * zax[2] + rr * c * xax[2]];
            let cur = if d[2] < -1e-9 { Some((hr * d[0].hypot(d[1]) / -d[2], d[1].atan2(d[0]) - chi)) } else { None };
            if let (Some((r1, t1)), Some((r2, t2))) = (prev, cur)
                && t1.abs() < 1.5 * pitch_r
                && t2.abs() < 1.5 * pitch_r
            {
                let (lo, hi) = (r1.min(r2), r1.max(r2));
                let j0 = ((lo - r_tip) / (r_root - r_tip) * bins as f64).ceil().max(0.0);
                let j1 = ((hi - r_tip) / (r_root - r_tip) * bins as f64).floor().min(bins as f64);
                if j1 >= j0 {
                    for (j, w) in widest.iter_mut().enumerate().take(j1 as usize + 1).skip(j0 as usize) {
                        let f = if (r2 - r1).abs() > 1e-15 { (r_at(j) - r1) / (r2 - r1) } else { 0.0 };
                        let th = (t1 + (t2 - t1) * f).abs();
                        if w.is_none_or(|x| th > x) {
                            *w = Some(th);
                        }
                    }
                }
            }
            prev = cur;
        }
    }
    // Where the planet doesn't reach (past its tips) the space keeps the width it has at the
    // last radius it does.
    let (Some(first), Some(last)) = (widest.iter().position(|w| w.is_some()), widest.iter().rposition(|w| w.is_some())) else {
        p.refuse("ring: the planets don't reach the ring's teeth");
        return None;
    };
    // The planet the ring is cut from is already thinned by the backlash: the ring adds its own,
    // so each flank has the backlash between them as a bevel pair has.
    let widen = s.backlash / (re * dr.sin());
    let vals: Vec<f64> = (0..=bins).map(|j| widest[j.clamp(first, last)].unwrap_or(0.0) + widen).collect();
    let land = 0.02 * m;
    if vals.iter().enumerate().any(|(j, v)| (pitch_r - v) * r_at(j) < land / 2.0) {
        p.warn(
            "ring: its teeth come to a point (the planets need wider spaces than the ring's pitch leaves): use more ring teeth or smaller planets",
        );
    }
    let space_half = |r: f64| -> f64 {
        let x = ((r - r_tip) / (r_root - r_tip) * bins as f64).clamp(0.0, bins as f64);
        let j = (x.floor() as usize).min(bins - 1);
        let f = x - j as f64;
        (vals[j] * (1.0 - f) + vals[j + 1] * f).min(pitch_r - land / (2.0 * r))
    };
    let h = hr;
    let rm = re - b / 2.0;
    let wind = if spiral.abs() > 1e-6 { spiral.tan() / dr.sin() } else { 0.0 };
    let skew = |rr: f64| if wind == 0.0 { 0.0 } else { wind * (rr.hypot(h) / rm).ln() };
    let min_gap = 5e-6 * r_root.max(m);
    let side = |sign: f64| -> Vec<Vec<(f64, f64)>> {
        let pts = side_points(&|r| sign * space_half(r) + skew(r), &|t| r_tip + (r_root - r_tip) * t, PROFILE_TOLERANCE * m, min_gap);
        if pts.len() >= 2 { vec![pts] } else { Vec::new() }
    };
    let sides = ToothSides { rf: r_tip, ra: r_root, lower: side(-1.0), upper: side(1.0) };
    if sides.lower.is_empty() || sides.upper.is_empty() {
        p.refuse("ring: its tooth spaces couldn't be traced");
        return None;
    }
    let small = (re - b) / re;
    // The ring's large end: the plane square to the axis where its roots meet the back cone
    // (on the large end's plane its teeth run on past the back cone by their roots, where the
    // planets' tips, which end on their own back cones, can touch them).
    let rho_root = re / ((r_root.atan2(h)) - dr).cos();
    let z0 = (h - rho_root * r_root.atan2(h).cos()).clamp(0.0, 0.5 * h * (1.0 - small));
    let k0 = (h - z0) / h;
    let scale = small / k0;
    let height = h * (1.0 - small) - z0;
    // The ring in one: the region between its rim's outside and the spaces' outline (the
    // planets' cut, an external bevel), tapered as the planets' teeth are. Not a blank with
    // that bevel cut out of it: the kernel's cut came out an open shell and the build kept the
    // whole blank (173 to 175 mm³ in each planet of the 45° default). Its ends are still flat,
    // square to the axis (not trimmed to the back and front cones as the other members are:
    // tried, the hobbed ring's flanks close up at the heel and the planets' tips then touched
    // it by about 0.003 modules).
    let rim = if s.rim > 0.0 { s.rim } else { 3.0 * m };
    let outline = sided_outline(&sides, zr, phase).scaled(k0).rotated(wind * k0.ln());
    let steps = vec![Step::RingTaper { outline, rim: r_root + rim, z0, height, twist: wind * scale.ln(), scale, op: Op::New }];
    Some(Member { steps, ra: r_root, zv: f64::from(zr) / dr.cos(), ends: ConeEnds { back: z0, front: z0 + height, small_root: r_tip * small } })
}

/// The name the carrier body ends with ("Carrier (4 planets)"): the assembly and the housing
/// cutout tell it from the gears by it.
pub(crate) const CARRIER: &str = "Carrier (";

/// The carrier: a hub round the apex, inside every member's small end (so it touches no teeth),
/// and a pin of Ø`pin` from the apex along each planet's axis through its bore, out past its back
/// (flush with it in ring mode, where the ring's blank lies beyond).
fn carrier(s: &Spec, g: &Geometry, planet: &ConeEnds, pin: f64, h_planet: f64) -> Body {
    let (re, face) = (g.re, g.face);
    let toe = (re - face).max(0.0);
    let mut near = toe * g.delta_sun.cos().min(g.delta_planet.cos());
    if g.mode == BevelPlanetMode::Differential {
        near = near.min(toe * (PI - g.delta_far).cos().abs());
    }
    let hub = (0.55 * near).max(0.6 * pin);
    let disc = |r: f64| Region { outer: vec![Outline::circle(V2::ZERO, r)], holes: vec![] };
    let mut steps = vec![Step::Prism { region: disc(hub), z0: -hub, height: 2.0 * hub, twist: 0.0, op: Op::New }];
    let past = if g.mode == BevelPlanetMode::Ring { 0.0 } else { s.hub_length.max(0.0) + 1.5 * s.module };
    let length = h_planet - planet.back.min(0.0) + past;
    for th in &g.azimuths {
        // From the apex out along the planet's axis (its frame's z points at the apex).
        let out = rot([g.sigma.sin(), 0.0, -g.sigma.cos()], *th);
        let x = rot([g.sigma.cos(), 0.0, g.sigma.sin()], *th);
        steps.push(Step::Placed {
            frame: Frame { origin: [0.0; 3], x, z: out },
            step: Box::new(Step::Prism { region: disc(pin / 2.0), z0: 0.0, height: length, twist: 0.0, op: Op::Join }),
        });
    }
    let frame = Frame { origin: s.origin, x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
    Body { name: name(s, &format!("{CARRIER}{} planet{})", g.azimuths.len(), if g.azimuths.len() == 1 { "" } else { "s" })), frame, steps }
}

fn rot(v: [f64; 3], t: f64) -> [f64; 3] {
    let (sn, c) = t.sin_cos();
    [v[0] * c - v[1] * sn, v[0] * sn + v[1] * c, v[2]]
}

/// Plans the set.
pub(super) fn build(s: &Spec, p: &mut Plan) {
    let g = match geometry(s) {
        Ok(g) => g,
        Err(e) => {
            p.refuse(e);
            return;
        }
    };
    for w in &g.warnings {
        p.warn(w.clone());
    }
    if s.shift != 0.0 || s.shift2 != 0.0 {
        p.warn("profile shift isn't used on bevel planetary sets (every member meshes two others)");
    }
    let mode = g.mode;
    let (zs, zp, zf) = (g.z_sun, g.z_planet, g.z_far);
    let (fs, fp, ff) = (f64::from(zs), f64::from(zp), f64::from(zf));
    let (sigma, re) = (g.sigma, g.re);
    let apex = s.origin;
    let th0 = s.first_azimuth.to_radians();
    let beta = g.spiral;

    // ---- Teeth. ----
    // The sun, the second sun and the ring share the face width; the planets may have their own
    // (face_width2) and their heels in from the cone distance (face_offset2).
    let Some([f_sun, f_planet]) = cone_faces(s, re, p, ["sun", "planets"]) else { return };
    let Some(sun) = member(s, &g, zs, g.delta_sun, beta, 0.0, f_sun, p, "sun") else { return };
    let Some(planet) = member(s, &g, zp, g.delta_planet, -beta, 0.0, f_planet, p, "planet") else { return };
    let far = match mode {
        BevelPlanetMode::Single => None,
        BevelPlanetMode::Differential => {
            let phase = -th0 * (fs + ff) / ff - PI * fp / ff;
            let Some(m) = member(s, &g, zf, PI - g.delta_far, beta, phase, f_sun, p, "sun 2") else { return };
            Some(m)
        }
        BevelPlanetMode::Ring => {
            // The ring's tooth 0 and the tool's tooth (a ring space) half a pitch on.
            let phase = th0 * (fs + ff) / ff + PI * fp / ff + PI / ff;
            let Some(m) = ring_member(s, &g, -beta, phase, p) else { return };
            Some(m)
        }
    };

    // ---- Bodies, every one: the mesh check wants the whole set when only some are built. ----
    let h_sun = re * g.delta_sun.cos();
    let h_planet = re * g.delta_planet.cos();
    let sun_frame = Frame { origin: [apex[0], apex[1], apex[2] - h_sun], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
    let mut sun_steps = sun.steps.clone();
    bevel_hub(s, &s.bore, m_r(s, zs) * 0.6, &sun.ends, &mut sun_steps, p, "sun");
    let sun_body = Body { name: name(s, &format!("Sun z{zs}")), frame: sun_frame, steps: sun_steps };

    // The carrier (the whole set built): a hub round the apex with a pin along every planet's
    // axis, the planets turning on the pins. A planet with no bore of its own gets one for its pin
    // (half its root Ø at the small end, so its roots keep their wall).
    let whole = s.only == 0;
    let pin_bore = if s.bore2.outline().is_some() { s.bore2.diameter } else { planet.ends.small_root };
    let pin = (pin_bore - 0.2).max(0.0);
    let with_pin_bore;
    let sp: &Spec = if whole && s.bore2.outline().is_none() && pin > 0.5 {
        with_pin_bore =
            Spec { bore2: crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: pin_bore, ..crate::hub::Bore::default() }, ..s.clone() };
        &with_pin_bore
    } else {
        s
    };
    let mut planets = Vec::new();
    for (k, th) in g.azimuths.iter().enumerate() {
        let phase = PI + PI / fp + fs * th / fp;
        let mut steps: Vec<Step> = planet
            .steps
            .iter()
            .map(|st| match st {
                Step::Taper { outline, z0, height, twist, scale, op } => {
                    Step::Taper { outline: outline.rotated(phase), z0: *z0, height: *height, twist: *twist, scale: *scale, op: *op }
                }
                other => other.clone(),
            })
            .collect();
        bevel_hub(sp, &sp.bore2, m_r(s, zp) * 0.6, &planet.ends, &mut steps, p, "planet");
        let ax = rot([-sigma.sin(), 0.0, sigma.cos()], *th);
        let frame = Frame { origin: [0, 1, 2].map(|i| apex[i] - ax[i] * h_planet), x: rot([sigma.cos(), 0.0, sigma.sin()], *th), z: ax };
        planets.push(Body { name: name(s, &format!("Planet {} z{zp}", k + 1)), frame, steps });
    }

    let far_body = far.as_ref().map(|f| match mode {
        BevelPlanetMode::Ring => {
            let frame = Frame { origin: [apex[0], apex[1], apex[2] - re * g.delta_far.cos()], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
            Body { name: name(s, &format!("Ring z{zf}")), frame, steps: f.steps.clone() }
        }
        _ => {
            let mut steps = f.steps.clone();
            bevel_hub(s, &s.bore, m_r(s, zf) * 0.6, &f.ends, &mut steps, p, "sun 2");
            let frame = Frame { origin: [apex[0], apex[1], apex[2] + re * (PI - g.delta_far).cos()], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, -1.0] };
            Body { name: name(s, &format!("Sun 2 z{zf}")), frame, steps }
        }
    });

    // ---- Facts. ----
    let far_name = mode.far();
    p.fact("cone angle (sun axis to planet axes)", sigma.to_degrees(), Unit::Deg);
    p.fact("planets", g.azimuths.len() as f64, Unit::None);
    p.fact("sun pitch cone angle", g.delta_sun.to_degrees(), Unit::Deg);
    p.fact("planet pitch cone angle", g.delta_planet.to_degrees(), Unit::Deg);
    p.fact("cone distance", re, Unit::Mm);
    p.fact("face width", g.face, Unit::Mm);
    p.fact("sun outer pitch diameter", s.module * fs, Unit::Mm);
    p.fact("planet outer pitch diameter", s.module * fp, Unit::Mm);
    p.fact("sun outer tip diameter", 2.0 * sun.ra, Unit::Mm);
    p.fact("planet outer tip diameter", 2.0 * planet.ra, Unit::Mm);
    p.fact("sun virtual teeth", sun.zv, Unit::None);
    p.fact("planet virtual teeth", planet.zv, Unit::None);
    if beta != 0.0 {
        p.fact("mean spiral angle", s.spiral_angle, Unit::Deg);
    }
    for (k, th) in g.azimuths.iter().enumerate() {
        p.fact(format!("planet {} azimuth", k + 1), th.to_degrees(), Unit::Deg);
    }
    if g.azimuths.len() > 1 {
        p.fact("least angle between planet axes", g.least_axis_angle.to_degrees(), Unit::Deg);
    }
    p.fact("ratio (carrier fixed, sun in, planet out)", fs / fp, Unit::None);
    match mode {
        BevelPlanetMode::Single => {
            p.fact("planet turns per carrier turn (sun fixed)", fs / fp, Unit::None);
        }
        BevelPlanetMode::Differential => {
            p.fact(format!("{far_name} teeth"), ff, Unit::None);
            p.fact(format!("{far_name} pitch cone angle"), (PI - g.delta_far).to_degrees(), Unit::Deg);
            p.fact(format!("{far_name} outer pitch diameter"), s.module * ff, Unit::Mm);
            p.fact(format!("{far_name} shaft angle to the planets"), (PI - sigma).to_degrees(), Unit::Deg);
            // About the common axis, one way for both suns.
            p.fact("ratio (carrier fixed, sun in, sun 2 out)", -fs / ff, Unit::None);
            p.fact("ratio (sun 2 fixed, sun in, carrier out)", 1.0 + ff / fs, Unit::None);
            if zs == zf {
                p.fact("carrier = (sun + sun 2) / 2 (equal side gears)", 0.5, Unit::None);
            }
        }
        BevelPlanetMode::Ring => {
            p.fact(format!("{far_name} teeth"), ff, Unit::None);
            p.fact(format!("{far_name} pitch cone angle"), g.delta_far.to_degrees(), Unit::Deg);
            p.fact(format!("{far_name} outer pitch diameter"), s.module * ff, Unit::Mm);
            p.fact("ratio (carrier fixed, sun in, ring out)", -fs / ff, Unit::None);
            p.fact("ratio (ring fixed, sun in, carrier out)", 1.0 + ff / fs, Unit::None);
            p.fact("ratio (sun fixed, ring in, carrier out)", 1.0 + fs / ff, Unit::None);
            if let Some(f) = &far
                && f.zv - planet.zv < 10.0
            {
                p.warn(format!(
                    "ring and planet virtual teeth differ by {} (under 10): their tips may meet going in and out of mesh",
                    num(f.zv - planet.zv)
                ));
            }
        }
    }
    if let Some(f) = &far {
        p.fact(format!("{far_name} virtual teeth"), f.zv, Unit::None);
    }

    // ---- The 3D meshes: every planet with the sun and with the far member. ----
    let (gap_lo, gap_hi) = expected_gap(s);
    let mut meshes = Vec::new();
    for (k, pb) in planets.iter().enumerate() {
        meshes.push(MeshPair3 {
            a: 0,
            b: 1,
            bodies: Some([sun_body.clone(), pb.clone()]),
            ratio: -fs / fp,
            period: TAU / fs,
            gap: (gap_lo, gap_hi),
            scale: s.module,
            labels: Some(["sun".into(), format!("planet {}", k + 1)]),
        });
        if let Some(fb) = &far_body {
            meshes.push(MeshPair3 {
                a: 1,
                b: 2,
                bodies: Some([pb.clone(), fb.clone()]),
                ratio: if mode == BevelPlanetMode::Ring { fp / ff } else { -fp / ff },
                period: TAU / fp,
                // The ring cut by the planet keeps its play only near mid face: towards its ends
                // the flanks close up (to touching at the least).
                gap: (if mode == BevelPlanetMode::Ring { 0.0 } else { gap_lo }, gap_hi),
                scale: s.module,
                labels: Some([format!("planet {}", k + 1), far_name.into()]),
            });
        }
    }
    p.meshes3d = meshes;

    if wanted(s, 1) {
        p.bodies.push(sun_body);
    }
    if wanted(s, 2) {
        p.bodies.extend(planets);
    }
    if wanted(s, 3)
        && let Some(b) = far_body
    {
        p.bodies.push(b);
    }
    if whole && pin > 0.5 {
        p.bodies.push(carrier(s, &g, &planet.ends, pin, h_planet));
        p.fact("carrier pin diameter", pin, Unit::Mm);
    }

    // ---- Preview: one spoke developed on the back cones (Tredgold) at the large end. ----
    let virt = |z: u32, d: f64, helix: f64| {
        bevel_involute(s, (f64::from(z) / d.cos().abs().max(1e-3)).round().clamp(3.0, 2000.0) as u32, beta, helix, 0.0, 0.0)
    };
    let (vs, vp) = (virt(zs, g.delta_sun, beta), virt(zp, g.delta_planet, -beta));
    let (rs, rp) = (vs.r(), vp.r());
    let php = mesh_phase(vs.teeth, 0.0, vp.teeth, 0.0);
    let speed_p = -f64::from(vs.teeth) / f64::from(vp.teeth);
    p.preview_note = format!(
        "one spoke developed on the back cones at the large end (Tredgold), virtual tooth counts rounded; {} planet{} at the cone angle round the sun",
        g.azimuths.len(),
        if g.azimuths.len() == 1 { "" } else { "s" }
    );
    p.preview.push(PreviewGear::turning("sun", Region { outer: vec![external_outline(&vs, 0.0).0], holes: vec![] }, V2::ZERO, 1.0, rs));
    p.preview.push(PreviewGear::turning(
        "planet 1",
        Region { outer: vec![external_outline(&vp, php).0], holes: vec![] },
        V2::new(rs + rp, 0.0),
        speed_p,
        rp,
    ));
    match mode {
        BevelPlanetMode::Single => {}
        BevelPlanetMode::Differential => {
            let vf = virt(zf, PI - g.delta_far, beta);
            let rf = vf.r();
            let ph = mesh_phase(vp.teeth, php, vf.teeth, 0.0);
            p.preview.push(PreviewGear::turning(
                far_name,
                Region { outer: vec![external_outline(&vf, ph).0], holes: vec![] },
                V2::new(rs + 2.0 * rp + rf, 0.0),
                -speed_p * f64::from(vp.teeth) / f64::from(vf.teeth),
                rf,
            ));
        }
        BevelPlanetMode::Ring => {
            let vf = virt(zf, g.delta_far, 0.0);
            let ring = Internal { gear: vf };
            let flank = ring.space_flank();
            let ph = mesh_phase_internal(vp.teeth, php, vf.teeth, 0.0);
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
            let teeth = teeth_outline(&flank, vf.teeth, ph + PI / f64::from(vf.teeth));
            p.preview.push(PreviewGear::turning(
                far_name,
                Region { outer: vec![Outline::circle(V2::ZERO, ring.r_root() + rim)], holes: vec![teeth] },
                V2::new(rs + 2.0 * rp - vf.r(), 0.0),
                speed_p * f64::from(vp.teeth) / f64::from(vf.teeth),
                vf.r(),
            ));
        }
    }
}

/// A member's outer pitch radius.
fn m_r(s: &Spec, z: u32) -> f64 {
    s.module * f64::from(z) / 2.0
}
