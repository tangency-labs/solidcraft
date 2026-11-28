//! Load capacity: will a gear set survive its load?
//!
//! # Method
//!
//! Cylindrical (spur, helical, herringbone, internal, pair, planetary) gears follow
//! **ISO 6336:2006/2019 Method B, simplified**:
//!
//! - Part 2 (pitting): σH = ZB/D · ZH · ZE · Zε · Zβ · √(Ft/(d1·b) · (u ± 1)/u) · √(KA · KV · KHβ · KHα),
//!   with ZH, ZE, Zε, Zβ and the single pair contact factors ZB/ZD from the standard's formulas.
//! - Part 3 (tooth root): σF = Ft/(b·mn) · YF · YS · Yβ · KA · KV · KFβ · KFα. YF and YS are
//!   Method B's (load at the outer point of single pair contact, critical section at the 30°
//!   tangent), but measured on the *generated* tooth of the virtual spur gear
//!   ([`crate::involute::tooth_half_sampler`]) rather than from the closed-form trochoid: the
//!   root chord sFn, the bending arm hFe and the fillet radius ρF at the tangent point.
//! - Part 1: KV by Method C (the ISO 6336-1:1996 / DIN 3990-1 approximation with the accuracy
//!   grade constants K1, K2); KHβ = KFβ = KHα = KFα = 1 (well aligned, accurate gears).
//! - Part 2/3 life factors ZNT, YNT from the standard's S–N curves (static to endurance, log-log)
//!   when a life is given; otherwise the endurance values (1). ZL·ZV·ZR·ZW·ZX = 1,
//!   YδrelT·YRrelT·YX = 1. Idler gears (planets) get the mean stress factor YM = 0.7 for their
//!   reversed bending.
//! - Safety factors are against the limits without minimum safeties:
//!   SH = σHlim·ZNT/σH, SF = σFE·YNT·YM/σF (σFE = YST·σFlim, YST = 2).
//!
//! Also the classic **Lewis** bending check (σ = KA·KV·Ft/(b·m·Y), load at the tip, Lewis's
//! inscribed parabola fitted to the generated tooth) for comparison with textbooks.
//!
//! Straight and spiral bevels use the virtual cylindrical gears at mid face (ISO 10300 style,
//! simplified: effective width 0.85·b, KHβ = KFβ = 1.5). Worm sets give forces, efficiency and
//! self-locking (friction after AGMA/Shigley for a steel worm and bronze wheel) and rate the
//! wheel as a helical gear meshing a rack (approximate). Crossed helical gears get forces only.
//!
//! # Warning thresholds
//!
//! A warning when SF < 1.4 or SH < 1.0: the usual minimum safety factors for industrial gears
//! (ISO 6336 leaves them to the user; DIN 3990 / Niemann recommend SFmin ≈ 1.4 for the root, which
//! breaks, and SHmin ≈ 1.0 for the flanks, which pit progressively). Below 1.0 in bending the
//! teeth are expected to break.

use std::f64::consts::PI;

use crate::involute::{Internal, Involute, Mesh, mesh_external, mesh_internal, shift_sum_for_center, tooth_half_sampler};
use crate::plan::{Kind, Plan, Spec, Unit};

/// Gear materials with representative allowable stresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Material {
    /// Normalized carbon steel (C45, ~190 HB).
    SteelNormalized,
    /// Through-hardened (quenched and tempered) alloy steel (42CrMo4, ~300 HB).
    #[default]
    SteelThroughHardened,
    /// Case-hardened (carburized) steel (16MnCr5, 18CrNiMo7-6; 58–62 HRC).
    SteelCaseHardened,
    /// Nitrided steel (31CrMoV9, gas nitrided).
    SteelNitrided,
    /// Grey cast iron (EN-GJL-250).
    CastIron,
    /// Tin bronze (CuSn12, worm wheels).
    Bronze,
    /// POM (acetal), injection moulded or machined.
    Pom,
    /// Nylon PA66, moisture conditioned.
    Pa66,
    /// PLA, 3D printed (FDM).
    Pla,
    /// PETG, 3D printed (FDM).
    Petg,
}

/// A material's data for the rating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialData {
    pub name: &'static str,
    pub label: &'static str,
    /// Allowable contact stress (endurance limit for pitting) σHlim, MPa.
    pub sigma_h_lim: f64,
    /// Nominal bending endurance σFE = YST·σFlim (one-directional load), MPa.
    pub sigma_fe: f64,
    /// Young's modulus (MPa) and Poisson's ratio.
    pub e: f64,
    pub nu: f64,
    /// Life factor curves (static cycles, static factor, endurance cycles) for pitting (ZNT) and
    /// bending (YNT); None: no life factor (taken as 1).
    pub zn: Option<(f64, f64, f64)>,
    pub yn: Option<(f64, f64, f64)>,
    pub plastic: bool,
    pub source: &'static str,
}

/// Representative values. Steels and cast iron: ISO 6336-5:2016 quality MQ at the stated
/// hardness (cross-checked against Roloff/Matek, Maschinenelemente, TB 20-1); life curves from
/// ISO 6336-2/-3 (Tables "life factor"). Bronze: DIN 3996:2019 (CuSn12-GZ). POM and PA66: VDI
/// 2736-2:2014 long-life values at about 20–60 °C against steel. PLA/PETG: no standard exists;
/// conservative estimates from bulk strength (PLA ≈ 50 MPa, PETG ≈ 45 MPa tensile) reduced for
/// layer adhesion and fatigue: test your own prints. E and ν: ISO 6336-2 (metals), datasheets
/// (polymers).
pub const MATERIALS: &[(Material, MaterialData)] = &[
    (
        Material::SteelNormalized,
        MaterialData {
            name: "steel_normalized",
            label: "Steel, normalized (C45)",
            sigma_h_lim: 470.0,
            sigma_fe: 440.0,
            e: 206_000.0,
            nu: 0.3,
            zn: Some((6e5, 1.6, 1e7)),
            yn: Some((1e4, 2.5, 3e6)),
            plastic: false,
            source: "ISO 6336-5 MQ, ~190 HB",
        },
    ),
    (
        Material::SteelThroughHardened,
        MaterialData {
            name: "steel_through_hardened",
            label: "Steel, through-hardened (42CrMo4)",
            sigma_h_lim: 730.0,
            sigma_fe: 600.0,
            e: 206_000.0,
            nu: 0.3,
            zn: Some((1e5, 1.6, 5e7)),
            yn: Some((1e4, 2.5, 3e6)),
            plastic: false,
            source: "ISO 6336-5 MQ, ~300 HB",
        },
    ),
    (
        Material::SteelCaseHardened,
        MaterialData {
            name: "steel_case_hardened",
            label: "Steel, case-hardened (16MnCr5)",
            sigma_h_lim: 1500.0,
            sigma_fe: 1000.0,
            e: 206_000.0,
            nu: 0.3,
            zn: Some((1e5, 1.6, 5e7)),
            yn: Some((1e3, 2.5, 3e6)),
            plastic: false,
            source: "ISO 6336-5 MQ, 58–62 HRC",
        },
    ),
    (
        Material::SteelNitrided,
        MaterialData {
            name: "steel_nitrided",
            label: "Steel, nitrided (31CrMoV9)",
            sigma_h_lim: 1250.0,
            sigma_fe: 840.0,
            e: 206_000.0,
            nu: 0.3,
            zn: Some((1e5, 1.3, 2e6)),
            yn: Some((1e3, 1.6, 3e6)),
            plastic: false,
            source: "ISO 6336-5 MQ, gas nitrided",
        },
    ),
    (
        Material::CastIron,
        MaterialData {
            name: "cast_iron",
            label: "Grey cast iron (EN-GJL-250)",
            sigma_h_lim: 360.0,
            sigma_fe: 140.0,
            e: 118_000.0,
            nu: 0.3,
            zn: Some((1e5, 1.3, 2e6)),
            yn: Some((1e3, 1.1, 3e6)),
            plastic: false,
            source: "ISO 6336-5 MQ, ~200 HB",
        },
    ),
    (
        Material::Bronze,
        MaterialData {
            name: "bronze",
            label: "Tin bronze (CuSn12)",
            sigma_h_lim: 425.0,
            sigma_fe: 190.0,
            e: 103_000.0,
            nu: 0.3,
            zn: None,
            yn: None,
            plastic: false,
            source: "DIN 3996 CuSn12-GZ (approximate)",
        },
    ),
    (
        Material::Pom,
        MaterialData {
            name: "pom",
            label: "POM (acetal)",
            sigma_h_lim: 45.0,
            sigma_fe: 35.0,
            e: 2800.0,
            nu: 0.35,
            zn: None,
            yn: None,
            plastic: true,
            source: "VDI 2736-2, long life, ~20–60 °C (approximate)",
        },
    ),
    (
        Material::Pa66,
        MaterialData {
            name: "pa66",
            label: "Nylon PA66 (conditioned)",
            sigma_h_lim: 40.0,
            sigma_fe: 30.0,
            e: 1700.0,
            nu: 0.4,
            zn: None,
            yn: None,
            plastic: true,
            source: "VDI 2736-2, long life, ~20–60 °C (approximate)",
        },
    ),
    (
        Material::Pla,
        MaterialData {
            name: "pla",
            label: "PLA (3D printed)",
            sigma_h_lim: 30.0,
            sigma_fe: 20.0,
            e: 3000.0,
            nu: 0.36,
            zn: None,
            yn: None,
            plastic: true,
            source: "estimate from bulk strength; no standard",
        },
    ),
    (
        Material::Petg,
        MaterialData {
            name: "petg",
            label: "PETG (3D printed)",
            sigma_h_lim: 25.0,
            sigma_fe: 15.0,
            e: 2000.0,
            nu: 0.38,
            zn: None,
            yn: None,
            plastic: true,
            source: "estimate from bulk strength; no standard",
        },
    ),
];

/// Stands in for a material missing from the table (none is), so that looking one up can't
/// panic. Worked out when compiling, so it can't fail at run time either.
const FALLBACK: &MaterialData = &MATERIALS[0].1;

impl Material {
    pub fn data(self) -> &'static MaterialData {
        MATERIALS.iter().find(|(m, _)| *m == self).map_or(FALLBACK, |(_, d)| d)
    }
    pub fn name(self) -> &'static str {
        self.data().name
    }
    pub fn parse(s: &str) -> Option<Material> {
        MATERIALS.iter().find(|(_, d)| d.name == s).map(|(m, _)| *m)
    }
}

/// The load on a gear set. Not rated unless a torque, or a power and a speed, is given.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    /// Input power on the driving member (kW; 0: from the torque).
    pub power: f64,
    /// Input torque on the driving member (N·m; 0: from the power and speed). Wins over power.
    pub torque: f64,
    /// Speed of the driving member (rpm).
    pub speed: f64,
    /// Gear 1's material (pinion, sun, ring of an internal set, worm wheel), gear 2's (mate,
    /// planets, pinion in a ring, worm) and the planetary ring's (None: as gear 2).
    pub material: Material,
    pub material2: Material,
    pub material3: Option<Material>,
    /// Application factor KA (1.0 uniform; 1.25 light shocks; 1.5 moderate; 1.75+ heavy).
    pub application_factor: f64,
    /// Required life in hours (0: endurance, unlimited).
    pub life_hours: f64,
    /// ISO 1328 accuracy grade for the dynamic factor (5 fine … 12 coarse; 7 typical hobbed and
    /// shaved, 9–10 cut, 10–12 3D printed or moulded).
    pub quality: u8,
}

impl Default for Load {
    fn default() -> Self {
        Self {
            power: 0.0,
            torque: 0.0,
            speed: 0.0,
            material: Material::default(),
            material2: Material::default(),
            material3: None,
            application_factor: 1.0,
            life_hours: 0.0,
            quality: 7,
        }
    }
}

impl Load {
    /// A load is given.
    pub fn rated(&self) -> bool {
        self.torque > 0.0 || (self.power > 0.0 && self.speed > 0.0)
    }
    /// The driving torque (N·m).
    pub fn torque_nm(&self) -> f64 {
        if self.torque > 0.0 {
            self.torque
        } else if self.speed > 0.0 {
            self.power * 60_000.0 / (2.0 * PI * self.speed)
        } else {
            0.0
        }
    }
    /// The power (kW), 0 when there is no speed.
    pub fn power_kw(&self) -> f64 {
        self.torque_nm() * 2.0 * PI * self.speed / 60_000.0
    }
}

/// Validates the load fields.
pub fn check(l: &Load) -> Result<(), String> {
    for (v, what) in [(l.power, "power"), (l.torque, "torque"), (l.speed, "speed"), (l.life_hours, "life")] {
        if !v.is_finite() || v < 0.0 {
            return Err(format!("{what} must not be negative"));
        }
    }
    if l.power > 0.0 && l.torque <= 0.0 && l.speed <= 0.0 {
        return Err("a power needs a speed to give a torque".into());
    }
    if !l.application_factor.is_finite() || !(0.5..=5.0).contains(&l.application_factor) {
        return Err("application factor KA must be between 0.5 and 5".into());
    }
    if !(5..=12).contains(&l.quality) {
        return Err("accuracy grade must be between 5 and 12".into());
    }
    Ok(())
}

/// Life factor from a curve (static N, static factor, endurance N) at `cycles` (0: endurance).
pub fn life_factor(curve: Option<(f64, f64, f64)>, cycles: f64) -> f64 {
    let Some((n0, f0, n1)) = curve else { return 1.0 };
    if cycles <= 0.0 || cycles >= n1 {
        1.0
    } else if cycles <= n0 {
        f0
    } else {
        f0.powf((n1 / cycles).ln() / (n1 / n0).ln())
    }
}

/// Zone factor ZH (ISO 6336-2).
pub fn zone_factor(g: &Involute, working_pressure_angle: f64) -> f64 {
    let bb = (g.helix.sin() * g.pressure_angle.cos()).asin();
    let at = g.alpha_t();
    (2.0 * bb.cos() * working_pressure_angle.cos() / (at.cos().powi(2) * working_pressure_angle.sin())).sqrt()
}

/// Elasticity factor ZE (√MPa, ISO 6336-2).
pub fn elasticity_factor(a: Material, b: Material) -> f64 {
    let (a, b) = (a.data(), b.data());
    (1.0 / (PI * ((1.0 - a.nu * a.nu) / a.e + (1.0 - b.nu * b.nu) / b.e))).sqrt()
}

/// Contact ratio factor Zε (ISO 6336-2).
pub fn contact_ratio_factor(eps_a: f64, eps_b: f64) -> f64 {
    let ea = eps_a.max(1.0);
    if eps_b >= 1.0 { (1.0 / ea).sqrt() } else { ((4.0 - ea) / 3.0 * (1.0 - eps_b) + eps_b / ea).sqrt() }
}

/// Helix factor for pitting Zβ = 1/√cos β (ISO 6336-2:2006).
pub fn helix_factor_contact(helix: f64) -> f64 {
    1.0 / helix.cos().abs().sqrt()
}

/// Helix factor for the root Yβ (ISO 6336-3:2006): 1 − εβ·β/120°, εβ ≤ 1, β ≤ 30°.
pub fn helix_factor_root(helix: f64, eps_b: f64) -> f64 {
    let eb = eps_b.min(1.0);
    let b = helix.abs().to_degrees().min(30.0);
    (1.0 - eb * b / 120.0).max(0.75)
}

/// K1 (spur, helical) per ISO 1328 grade 5..12 for KV Method C (ISO 6336-1:1996 / DIN 3990-1).
const KV_K1: [(f64, f64); 8] = [(7.5, 6.7), (14.9, 13.3), (26.8, 23.9), (39.1, 34.8), (52.8, 47.0), (76.6, 68.2), (102.6, 91.4), (146.3, 130.3)];

/// Dynamic factor KV (ISO 6336-1 Method C). `z1` the pinion's teeth, `u` ≥ 1, `v` pitch line
/// velocity (m/s), `load` = KA·Ft/b (N/mm), `eps_b` the overlap ratio, `helical` a helix at all.
pub fn dynamic_factor(quality: u8, z1: f64, u: f64, v: f64, load: f64, helical: bool, eps_b: f64) -> f64 {
    let (k1s, k1h) = KV_K1[usize::from(quality.clamp(5, 12) - 5)];
    let w = load.max(100.0);
    let base = z1 * v / 100.0 * (u * u / (1.0 + u * u)).sqrt();
    let spur = 1.0 + (k1s / w + 0.0193) * base;
    let hel = 1.0 + (k1h / w + 0.0087) * base;
    if !helical {
        spur
    } else if eps_b >= 1.0 {
        hel
    } else {
        spur - eps_b * (spur - hel)
    }
}

/// The root of a tooth measured for ISO 6336-3 Method B.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootForm {
    /// Form factor YF and stress correction factor YS.
    pub yf: f64,
    pub ys: f64,
    /// Root chord at the 30° tangents, bending arm, fillet radius there (mm).
    pub s_fn: f64,
    pub h_fe: f64,
    pub rho_f: f64,
    /// Load direction angle at the outer point of single pair contact (radians).
    pub alpha_fen: f64,
}

/// The virtual spur gear of a helical gear (normal section), or a near-rack for internal teeth.
pub fn virtual_spur(g: &Involute, internal: bool) -> Involute {
    let bb = (g.helix.sin() * g.pressure_angle.cos()).asin();
    let zn = if internal { 500.0 } else { g.z() / (bb.cos().powi(2) * g.helix.cos()) };
    Involute { teeth: g.teeth.max(1), helix: 0.0, virtual_teeth: zn, ..*g }
}

/// YF and YS by ISO 6336-3 Method B measured on the generated tooth: `eps_a` is the set's
/// transverse contact ratio (it places the outer point of single pair contact).
pub fn root_form(g: &Involute, eps_a: f64, internal: bool) -> RootForm {
    let v = virtual_spur(g, internal);
    let bb = (g.helix.sin() * g.pressure_angle.cos()).asin();
    let ean = (eps_a / bb.cos().powi(2)).max(1.0);
    let mn = v.module;
    let an = v.pressure_angle;
    let (rbn, ran) = (v.rb(), v.ra());
    let pbn = PI * mn * an.cos();
    let along = ((ran * ran - rbn * rbn).max(0.0).sqrt() - pbn * (ean - 1.0)).max(0.0);
    let ren = (along * along + rbn * rbn).sqrt().min(ran);
    let ge = v.half_angle_at(ren);
    let aen = (rbn / ren).clamp(-1.0, 1.0).acos();
    let afen = aen - ge;
    let y_load = ren * (ge.cos() - ge.sin() * afen.tan());
    let half = tooth_half_sampler(&v);
    let rf = v.rf();
    let pt = |r: f64| -> Option<(f64, f64)> { half(r).map(|h| (r * h.sin(), r * h.cos())) };
    // The profile's angle to the tooth centre line at r (90° at the root, small on the flank).
    let dr = 2e-4 * mn;
    let angle = |r: f64| -> Option<f64> {
        let (a, b) = (pt(r - dr)?, pt(r + dr)?);
        Some((a.0 - b.0).atan2(b.1 - a.1))
    };
    let target = 30f64.to_radians();
    let (lo_r, hi_r) = (rf + 2.0 * dr, ren);
    let n = 600;
    let mut bracket = None;
    let mut prev = lo_r;
    for i in 1..=n {
        let r = lo_r + (hi_r - lo_r) * f64::from(i) / f64::from(n);
        if let Some(a) = angle(r)
            && a <= target
        {
            bracket = Some((prev, r));
            break;
        }
        prev = r;
    }
    let (mut a, mut b) = bracket.unwrap_or((lo_r, lo_r));
    for _ in 0..60 {
        let m = 0.5 * (a + b);
        match angle(m) {
            Some(x) if x <= target => b = m,
            _ => a = m,
        }
    }
    let rt = 0.5 * (a + b);
    let (xt, yt) = pt(rt).unwrap_or((0.0, rf));
    // The fillet's radius of curvature there: the circle through three nearby points.
    let d = 0.02 * mn;
    let rho = match (pt((rt - d).max(rf + 1e-9)), pt(rt + d)) {
        (Some(p0), Some(p2)) => circumradius(p0, (xt, yt), p2),
        _ => 0.38 * mn,
    };
    let s_fn = 2.0 * xt;
    let h_fe = (y_load - yt).max(1e-9);
    let yf = 6.0 * (h_fe / mn) * afen.cos() / ((s_fn / mn).powi(2) * an.cos());
    let l = s_fn / h_fe;
    let qs = s_fn / (2.0 * rho);
    let ys = (1.2 + 0.13 * l) * qs.powf(1.0 / (1.21 + 2.3 / l));
    RootForm { yf, ys, s_fn, h_fe, rho_f: rho, alpha_fen: afen }
}

fn circumradius(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    let d = |p: (f64, f64), q: (f64, f64)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
    let area2 = ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs();
    if area2 < 1e-15 { f64::INFINITY } else { d(a, b) * d(b, c) * d(c, a) / (2.0 * area2) }
}

/// Lewis form factor Y (load at the tip along the line of action, Lewis's parabola of uniform
/// strength inscribed in the generated tooth of the virtual spur gear): σ = Ft/(b·m·Y).
pub fn lewis_form_factor(g: &Involute) -> f64 {
    let v = virtual_spur(g, false);
    let mn = v.module;
    let half = tooth_half_sampler(&v);
    let ra = v.ra();
    let ha = v.half_angle_at(ra).max(0.0);
    let aa = (v.rb() / ra).clamp(-1.0, 1.0).acos();
    let af = aa - ha;
    let y_apex = ra * (ha.cos() - ha.sin() * af.tan());
    let rf = v.rf();
    let n = 1500;
    let mut best = f64::INFINITY;
    for i in 1..n {
        let r = rf + (ra - rf) * f64::from(i) / f64::from(n);
        if let Some(h) = half(r) {
            let (x, y) = (r * h.sin(), r * h.cos());
            if y < y_apex - 1e-9 && x > 0.0 {
                best = best.min(x * x / (y_apex - y));
            }
        }
    }
    2.0 * best / (3.0 * mn)
}

/// The Lewis bending stress (MPa).
pub fn lewis_stress(ft: f64, face: f64, module: f64, y: f64, k: f64) -> f64 {
    k * ft / (face * module * y)
}

/// One member of a mesh to rate.
#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub gear: Involute,
    pub internal: bool,
    pub material: Material,
    /// Load cycles over the required life (0: endurance).
    pub cycles: f64,
    /// Bent both ways (an idler or planet): YM = 0.7.
    pub idler: bool,
}

/// A mesh to rate: gear 1 must be external.
#[derive(Clone, Debug)]
pub struct MeshLoad {
    pub members: [Member; 2],
    pub mesh: Mesh,
    /// Face width carrying load (mm).
    pub face: f64,
    /// Overlap ratio used (herringbone: one half's).
    pub overlap: f64,
    /// Nominal tangential force at the reference circle (N), pitch line velocity (m/s).
    pub ft: f64,
    pub v: f64,
    pub ka: f64,
    pub quality: u8,
    /// Face load factor KHβ = KFβ.
    pub k_beta: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MemberRating {
    pub z_b: f64,
    pub sigma_h: f64,
    pub z_nt: f64,
    pub s_h: f64,
    pub y_f: f64,
    pub y_s: f64,
    pub y_beta: f64,
    pub y_nt: f64,
    pub y_m: f64,
    pub sigma_f: f64,
    pub s_f: f64,
    /// Lewis (external gears only).
    pub lewis_y: f64,
    pub lewis_sigma: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MeshRating {
    pub kv: f64,
    pub z_h: f64,
    pub z_e: f64,
    pub z_eps: f64,
    pub z_beta: f64,
    pub sigma_h0: f64,
    pub members: [MemberRating; 2],
}

/// Rates a cylindrical mesh by ISO 6336 Method B (simplified; see the module notes).
pub fn rate_mesh(l: &MeshLoad) -> MeshRating {
    let [a, b] = &l.members;
    let (g1, g2) = (&a.gear, &b.gear);
    let internal = b.internal;
    let m = &l.mesh;
    let (z1, z2) = (g1.z(), g2.z());
    let u = z2 / z1;
    let ea = m.contact_ratio;
    let eb = l.overlap;
    let z_h = zone_factor(g1, m.working_pressure_angle);
    let z_e = elasticity_factor(a.material, b.material);
    let z_eps = contact_ratio_factor(ea, eb);
    let z_beta = helix_factor_contact(g1.helix);
    let d1 = 2.0 * g1.r();
    let uf = if internal { (u - 1.0) / u } else { (u + 1.0) / u };
    let sigma_h0 = z_h * z_e * z_eps * z_beta * (l.ft / (d1 * l.face) * uf.max(0.0)).sqrt();
    let (zp, uk) = if internal || z1 <= z2 { (z1, u) } else { (z2, 1.0 / u) };
    let kv = dynamic_factor(l.quality, zp, uk, l.v, l.ka * l.ft / l.face, g1.helix != 0.0, eb);
    let k = l.ka * kv * l.k_beta;
    // Single pair contact factors ZB (gear 1) and ZD (gear 2), ISO 6336-2 (external only).
    let zbd = |ga: &Involute, gb: &Involute| -> f64 {
        if internal {
            return 1.0;
        }
        let r = |g: &Involute| ((g.ra() / g.rb()).powi(2) - 1.0).max(0.0).sqrt();
        let p = (r(ga) - 2.0 * PI / ga.z()) * (r(gb) - (ea - 1.0) * 2.0 * PI / gb.z());
        if p <= 0.0 {
            return 1.0;
        }
        let mm = m.working_pressure_angle.tan() / p.sqrt();
        let zb = if eb >= 1.0 { 1.0 } else { mm - eb * (mm - 1.0) };
        zb.max(1.0)
    };
    let zb = [zbd(g1, g2), zbd(g2, g1)];
    let mut members = [MemberRating::default(); 2];
    for (i, mem) in l.members.iter().enumerate() {
        let d = mem.material.data();
        let sigma_h = zb[i] * sigma_h0 * k.sqrt();
        let z_nt = if d.plastic { 1.0 } else { life_factor(d.zn, mem.cycles) };
        let rf = root_form(&mem.gear, ea, mem.internal);
        let y_beta = helix_factor_root(mem.gear.helix, eb);
        let sigma_f = l.ft / (l.face * mem.gear.module) * rf.yf * rf.ys * y_beta * k;
        let y_nt = if d.plastic { 1.0 } else { life_factor(d.yn, mem.cycles) };
        let y_m = if mem.idler { 0.7 } else { 1.0 };
        let (lewis_y, lewis_sigma) = if mem.internal {
            (0.0, 0.0)
        } else {
            let y = lewis_form_factor(&mem.gear);
            (y, lewis_stress(l.ft, l.face, mem.gear.module, y, l.ka * kv))
        };
        members[i] = MemberRating {
            z_b: zb[i],
            sigma_h,
            z_nt,
            s_h: d.sigma_h_lim * z_nt / sigma_h,
            y_f: rf.yf,
            y_s: rf.ys,
            y_beta,
            y_nt,
            y_m,
            sigma_f,
            s_f: d.sigma_fe * y_nt * y_m / sigma_f,
            lewis_y,
            lewis_sigma,
        };
    }
    MeshRating { kv, z_h, z_e, z_eps, z_beta, sigma_h0, members }
}

/// Minimum safeties below which a warning is given.
pub const SF_MIN: f64 = 1.4;
pub const SH_MIN: f64 = 1.0;

fn f(p: &mut Plan, label: impl Into<String>, value: f64) {
    p.fact(label, value, Unit::None);
}

fn warn_member(p: &mut Plan, name: &str, mat: Material, r: &MemberRating) {
    let label = mat.data().label;
    if r.s_f < 1.0 {
        p.warn(format!("{name}: root stress {:.0} MPa is over what {label} takes (SF {:.2}): the teeth will break; use a bigger module, wider face or stronger material", r.sigma_f, r.s_f));
    } else if r.s_f < SF_MIN {
        p.warn(format!("{name}: bending safety SF {:.2} is under {SF_MIN} (root stress {:.0} MPa, {label})", r.s_f, r.sigma_f));
    }
    if r.s_h < SH_MIN {
        p.warn(format!("{name}: contact safety SH {:.2} is under {SH_MIN} (contact stress {:.0} MPa, {label}): the flanks will pit or wear; use a wider face, bigger gears or harder material", r.s_h, r.sigma_h));
    }
}

/// Facts and warnings for one rated mesh; `pre` names the mesh in a set with several.
fn mesh_facts(p: &mut Plan, pre: &str, l: &MeshLoad, r: &MeshRating) {
    let pre = if pre.is_empty() { String::new() } else { format!("{pre}: ") };
    f(p, format!("{pre}pitch line velocity (m/s)"), l.v);
    f(p, format!("{pre}tangential force Ft (N)"), l.ft);
    let g1 = &l.members[0].gear;
    f(p, format!("{pre}radial force Fr (N)"), l.ft * l.mesh.working_pressure_angle.tan());
    if g1.helix != 0.0 {
        f(p, format!("{pre}axial force Fa (N)"), l.ft * g1.helix.tan().abs());
    }
    f(p, format!("{pre}dynamic factor KV"), r.kv);
    f(p, format!("{pre}zone factor ZH"), r.z_h);
    f(p, format!("{pre}elasticity factor ZE (√MPa)"), r.z_e);
    f(p, format!("{pre}contact ratio factor Zε"), r.z_eps);
    if g1.helix != 0.0 {
        f(p, format!("{pre}helix factor Zβ"), r.z_beta);
    }
    for (m, mr) in l.members.iter().zip(r.members.iter()) {
        let n = &m.name;
        f(p, format!("{pre}{n} contact stress σH (MPa)"), mr.sigma_h);
        f(p, format!("{pre}{n} contact safety SH"), mr.s_h);
        f(p, format!("{pre}{n} form factor YF"), mr.y_f);
        f(p, format!("{pre}{n} stress correction YS"), mr.y_s);
        f(p, format!("{pre}{n} root stress σF (MPa)"), mr.sigma_f);
        f(p, format!("{pre}{n} bending safety SF"), mr.s_f);
        if mr.lewis_y > 0.0 {
            f(p, format!("{pre}{n} Lewis form factor Y"), mr.lewis_y);
            f(p, format!("{pre}{n} Lewis bending stress (MPa)"), mr.lewis_sigma);
        }
        warn_member(p, n, m.material, mr);
    }
}

fn load_facts(p: &mut Plan, l: &Load, who: &str) {
    f(p, format!("load: torque on {who} (N·m)"), l.torque_nm());
    if l.speed > 0.0 {
        f(p, "load: power (kW)", l.power_kw());
        f(p, format!("load: speed of {who} (rpm)"), l.speed);
    }
    if l.application_factor != 1.0 {
        f(p, "load: application factor KA", l.application_factor);
    }
}

fn summary(p: &mut Plan, ratings: &[MeshRating], materials: &[Material]) {
    let sf = ratings.iter().flat_map(|r| r.members.iter().map(|m| m.s_f)).fold(f64::INFINITY, f64::min);
    let sh = ratings.iter().flat_map(|r| r.members.iter().map(|m| m.s_h)).fold(f64::INFINITY, f64::min);
    if sf.is_finite() {
        f(p, "minimum bending safety SF", sf);
    }
    if sh.is_finite() {
        f(p, "minimum contact safety SH", sh);
    }
    plastic_note(p, materials);
}

fn plastic_note(p: &mut Plan, materials: &[Material]) {
    if materials.iter().any(|m| m.data().plastic) {
        p.warn("plastic gears: the rating uses ISO 6336 formulas with approximate material values (VDI 2736 for POM/PA66, estimates for 3D prints); heat, moisture and print quality matter more than here; treat it as a rough guide");
    }
}

fn cycles(l: &Load, rpm: f64, per_rev: f64) -> f64 {
    if l.life_hours > 0.0 { 60.0 * rpm.abs() * l.life_hours * per_rev } else { 0.0 }
}

fn member(name: &str, gear: Involute, internal: bool, material: Material, cycles: f64, idler: bool) -> Member {
    Member { name: name.into(), gear, internal, material, cycles, idler }
}

/// Adds the load rating to a plan (the spec's load is given).
pub(crate) fn rate(s: &Spec, p: &mut Plan) {
    let l = &s.load;
    let t = l.torque_nm();
    let n = l.speed;
    let ka = l.application_factor;
    let face = s.face_width;
    match s.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone | Kind::Pair => {
            let helix = if s.kind == Kind::Spur { 0.0 } else { s.helix };
            let g1 = s.involute(s.teeth, s.shift, helix);
            let mut x2 = s.shift2;
            if s.kind == Kind::Pair
                && s.center_distance > 0.0
                && let Some(sum) = shift_sum_for_center(&g1, &s.involute(s.teeth2, 0.0, -helix), s.center_distance)
            {
                x2 = sum - s.shift;
            }
            let g2 = s.involute(s.teeth2, x2, -helix);
            let herring = s.kind == Kind::Herringbone;
            let mesh = mesh_external(&g1, &g2, face);
            let overlap = if herring { mesh.overlap_ratio / 2.0 } else { mesh.overlap_ratio };
            let n2 = n * g1.z() / g2.z();
            let (a, b) = if s.kind == Kind::Pair { ("gear 1", "gear 2") } else { ("gear", "mate") };
            if s.kind != Kind::Pair {
                f(p, "load: rated against a mate of teeth (teeth 2)", f64::from(s.teeth2));
            }
            load_facts(p, l, if s.kind == Kind::Pair { "gear 1" } else { "the gear" });
            let ml = MeshLoad {
                members: [member(a, g1, false, l.material, cycles(l, n, 1.0), false), member(b, g2, false, l.material2, cycles(l, n2, 1.0), false)],
                mesh,
                face,
                overlap,
                ft: 2000.0 * t / (2.0 * g1.r()),
                v: PI * 2.0 * g1.r() * n / 60_000.0,
                ka,
                quality: l.quality,
                k_beta: 1.0,
            };
            let r = rate_mesh(&ml);
            mesh_facts(p, "", &ml, &r);
            if herring {
                f(p, "axial force per half, cancelling (N)", ml.ft * g1.helix.tan().abs());
            }
            summary(p, &[r], &[l.material, l.material2]);
        }
        Kind::Internal => {
            // Gear 1 is the ring (loaded); its pinion has teeth 2.
            let ring = s.involute(s.teeth, s.shift, s.helix);
            let pinion = s.involute(s.teeth2, s.shift2, s.helix);
            let mesh = mesh_internal(&pinion, &Internal { gear: ring }, face);
            f(p, "load: rated against a pinion of teeth (teeth 2)", f64::from(s.teeth2));
            load_facts(p, l, "the ring");
            let np = n * ring.z() / pinion.z();
            let ml = MeshLoad {
                members: [
                    member("pinion", pinion, false, l.material2, cycles(l, np, 1.0), false),
                    member("ring", ring, true, l.material, cycles(l, n, 1.0), false),
                ],
                mesh,
                face,
                overlap: mesh.overlap_ratio,
                ft: 2000.0 * t / (2.0 * ring.r()),
                v: PI * 2.0 * ring.r() * n / 60_000.0,
                ka,
                quality: l.quality,
                k_beta: 1.0,
            };
            let r = rate_mesh(&ml);
            mesh_facts(p, "", &ml, &r);
            summary(p, &[r], &[l.material, l.material2]);
        }
        Kind::Planetary => planetary(s, p),
        Kind::Bevel | Kind::SpiralBevel => bevel(s, p),
        Kind::Worm => worm(s, p),
        Kind::CrossedHelical => crossed(s, p),
        _ => p.warn(format!("load rating isn't available for {}", s.kind.label().to_lowercase())),
    }
}

#[cfg(test)]
#[path = "strength_tests.rs"]
mod tests;

/// Planet mesh load factor Kγ by planet count (typical values, cf. AGMA 6123-B06, no floating
/// member).
pub fn planet_load_factor(planets: u32) -> f64 {
    match planets {
        0..=2 => 1.0,
        3 => 1.1,
        4 => 1.25,
        5 => 1.35,
        6 => 1.44,
        7 => 1.47,
        _ => 1.5,
    }
}

fn planetary(s: &Spec, p: &mut Plan) {
    let l = &s.load;
    let (zs, zp) = (s.teeth, s.teeth2);
    let zr = if s.teeth3 > 0 { s.teeth3 } else { zs + 2 * zp };
    let np = s.planets.max(1);
    let sun = s.involute(zs, s.shift, s.helix);
    let planet = s.involute(zp, s.shift2, -s.helix);
    let ring = s.involute(zr, 0.0, -s.helix);
    let m_sp = mesh_external(&sun, &planet, s.face_width);
    let m_pr = mesh_internal(&planet, &Internal { gear: ring }, s.face_width);
    let kg = planet_load_factor(np);
    // Ring fixed, sun in, carrier out: speeds relative to the carrier.
    let (fzs, fzp, fzr) = (f64::from(zs), f64::from(zp), f64::from(zr));
    let n_sun_rel = l.speed * fzr / (fzs + fzr);
    let n_carrier = l.speed * fzs / (fzs + fzr);
    let n_planet_rel = n_sun_rel * fzs / fzp;
    let ft = 2000.0 * l.torque_nm() / (2.0 * sun.r() * f64::from(np)) * kg;
    let v = PI * 2.0 * sun.r() * n_sun_rel / 60_000.0;
    load_facts(p, l, "the sun");
    f(p, "load: planet load sharing factor Kγ", kg);
    f(p, "load: carrier output torque, ring fixed (N·m)", l.torque_nm() * (1.0 + fzr / fzs));
    let ring_mat = l.material3.unwrap_or(l.material2);
    let common = |members: [Member; 2], mesh: Mesh| MeshLoad {
        members,
        mesh,
        face: s.face_width,
        overlap: mesh.overlap_ratio,
        ft,
        v,
        ka: l.application_factor,
        quality: l.quality,
        k_beta: 1.0,
    };
    let a = common(
        [
            member("sun", sun, false, l.material, cycles(l, n_sun_rel, f64::from(np)), false),
            member("planet", planet, false, l.material2, cycles(l, n_planet_rel, 1.0), true),
        ],
        m_sp,
    );
    let b = common(
        [
            member("planet", planet, false, l.material2, cycles(l, n_planet_rel, 1.0), true),
            member("ring", ring, true, ring_mat, cycles(l, n_carrier, f64::from(np)), false),
        ],
        m_pr,
    );
    let ra = rate_mesh(&a);
    let rb = rate_mesh(&b);
    mesh_facts(p, "sun–planet", &a, &ra);
    mesh_facts(p, "planet–ring", &b, &rb);
    summary(p, &[ra, rb], &[l.material, l.material2, ring_mat]);
}

fn bevel(s: &Spec, p: &mut Plan) {
    let l = &s.load;
    let sigma = s.shaft_angle.to_radians();
    let (z1, z2) = (f64::from(s.teeth), f64::from(s.teeth2));
    let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
    let d2 = sigma - d1;
    let bm = if s.kind == Kind::SpiralBevel { s.spiral_angle.to_radians() } else { 0.0 };
    let re = s.module * z1 / 2.0 / d1.sin();
    let b = s.face_width.min(re * 0.45);
    let mt = s.module * (1.0 - 0.5 * b / re);
    let dm1 = mt * z1;
    let virt = |z: u32, delta: f64, helix: f64, shift: f64| Involute {
        teeth: z,
        module: mt * bm.cos(),
        pressure_angle: s.pressure_angle.to_radians(),
        helix,
        shift,
        rack: s.rack,
        backlash: s.backlash,
        tip_shortening: 0.0,
        virtual_teeth: f64::from(z) / delta.cos(),
    };
    let g1 = virt(s.teeth, d1, bm, s.shift);
    let g2 = virt(s.teeth2, d2, -bm, -s.shift);
    let mesh = mesh_external(&g1, &g2, b);
    let t = l.torque_nm();
    let ft = 2000.0 * t / dm1;
    let an = s.pressure_angle.to_radians();
    load_facts(p, l, "gear 1");
    // Pinion forces, spiral hand and rotation pushing the pinion out of mesh (the usual choice).
    let fa1 = ft / bm.cos() * (an.tan() * d1.sin() + bm.sin() * d1.cos());
    let fr1 = ft / bm.cos() * (an.tan() * d1.cos() - bm.sin() * d1.sin());
    f(p, "gear 1 axial force Fa (N)", fa1);
    f(p, "gear 1 radial force Fr (N)", fr1);
    f(p, "mean pitch diameter gear 1", dm1);
    let ml = MeshLoad {
        members: [
            member("gear 1", g1, false, l.material, cycles(l, l.speed, 1.0), false),
            member("gear 2", g2, false, l.material2, cycles(l, l.speed * z1 / z2, 1.0), false),
        ],
        mesh,
        face: 0.85 * b,
        overlap: mesh.overlap_ratio,
        ft,
        v: PI * dm1 * l.speed / 60_000.0,
        ka: l.application_factor,
        quality: l.quality,
        k_beta: 1.5,
    };
    let r = rate_mesh(&ml);
    mesh_facts(p, "", &ml, &r);
    p.warn("bevel rating: virtual cylindrical gears at mid face (simplified ISO 10300: 0.85 × face width, KHβ = 1.5); check a production design to ISO 10300 or AGMA 2003");
    summary(p, &[r], &[l.material, l.material2]);
}

/// Sliding friction of a steel worm on a bronze wheel against sliding velocity (m/s): AGMA, as in
/// Shigley's Mechanical Engineering Design (Budynas & Nisbett), eq. 15-38.
pub fn worm_friction(vs: f64) -> f64 {
    let v = vs * 196.85; // ft/min
    if v <= 0.0 {
        0.15
    } else if v <= 10.0 {
        0.124 * (-0.074 * v.powf(0.645)).exp()
    } else {
        0.103 * (-0.110 * v.powf(0.450)).exp() + 0.012
    }
}

/// Worm efficiency driving (worm in) and back driving (wheel in; ≤ 0 means it locks).
pub fn worm_efficiency(lead_angle: f64, normal_pressure_angle: f64, mu: f64) -> (f64, f64) {
    let (c, t) = (normal_pressure_angle.cos(), lead_angle.tan());
    ((c - mu * t) / (c + mu / t), (c - mu / t) / (c + mu * t))
}

fn worm(s: &Spec, p: &mut Plan) {
    let l = &s.load;
    let m = s.module;
    let z1 = f64::from(s.starts.max(1));
    let zw = s.teeth;
    let d1 = if s.worm_diameter > 0.0 { s.worm_diameter } else { 10.0 * m };
    let gamma = (PI * m * z1 / (PI * d1)).atan();
    let an = s.pressure_angle.to_radians();
    let wheel = Involute {
        teeth: zw,
        module: m * gamma.cos(),
        pressure_angle: an,
        helix: gamma,
        shift: s.shift / gamma.cos(),
        rack: s.rack,
        backlash: s.backlash,
        tip_shortening: 0.0,
        virtual_teeth: 0.0,
    };
    let ra1 = d1 / 2.0 + s.rack.addendum * m;
    let face = s.face_width.min(2.0 * (ra1 * ra1 - (d1 / 2.0 - m) * (d1 / 2.0 - m)).max(0.0).sqrt() + m).max(s.face_width.min(d1));
    let t1 = l.torque_nm();
    let n1 = l.speed;
    let vs = PI * d1 * n1 / (60_000.0 * gamma.cos());
    let mu = worm_friction(vs);
    let (eta, eta_back) = worm_efficiency(gamma, an, mu);
    let w_t1 = 2000.0 * t1 / d1;
    let w = w_t1 / (an.cos() * gamma.sin() + mu * gamma.cos());
    let ft2 = w * (an.cos() * gamma.cos() - mu * gamma.sin());
    let fr = w * an.sin();
    let d2 = 2.0 * wheel.r();
    let n2 = n1 * z1 / f64::from(zw);
    let v2 = PI * d2 * n2 / 60_000.0;
    load_facts(p, l, "the worm");
    f(p, "worm tangential force = wheel axial force (N)", w_t1);
    f(p, "wheel tangential force = worm axial force (N)", ft2);
    f(p, "separating (radial) force (N)", fr);
    f(p, "wheel output torque (N·m)", ft2 * d2 / 2000.0);
    f(p, "sliding velocity (m/s)", vs);
    f(p, "friction coefficient μ (steel on bronze)", mu);
    f(p, "efficiency, worm driving", eta);
    f(p, "efficiency, wheel driving (≤ 0 locks)", eta_back);
    let rho0 = (0.15 / an.cos()).atan();
    let locks = gamma < rho0;
    f(p, "self-locking at rest (1 yes, 0 no)", if locks { 1.0 } else { 0.0 });
    if l.speed > 0.0 {
        f(p, "heat to remove (kW)", l.power_kw() * (1.0 - eta.max(0.0)));
    }
    // The wheel as a helical gear meshing a rack (the worm's axial section): approximate.
    let ka = l.application_factor;
    let kv = (1200.0 + v2 * 196.85) / 1200.0;
    let at = wheel.alpha_t();
    let ea = (((wheel.ra().powi(2) - wheel.rb().powi(2)).max(0.0)).sqrt() - wheel.r() * at.sin() + s.rack.addendum * wheel.module / at.sin())
        / wheel.base_pitch();
    let eb = 0.0;
    let mat_w = l.material;
    let mat_s = l.material2;
    let z_h = zone_factor(&wheel, at);
    let z_e = elasticity_factor(mat_w, mat_s);
    let z_eps = contact_ratio_factor(ea, eb);
    let sigma_h = z_h * z_e * z_eps * helix_factor_contact(gamma) * (ft2 / (d2 * face)).sqrt() * (ka * kv).sqrt();
    let dw = mat_w.data();
    let cyc = cycles(l, n2, 1.0);
    let s_h = dw.sigma_h_lim * if dw.plastic { 1.0 } else { life_factor(dw.zn, cyc) } / sigma_h;
    let rf = root_form(&wheel, ea, false);
    let sigma_f = ft2 / (face * wheel.module) * rf.yf * rf.ys * helix_factor_root(gamma, eb) * ka * kv;
    let s_f = dw.sigma_fe * if dw.plastic { 1.0 } else { life_factor(dw.yn, cyc) } / sigma_f;
    f(p, "dynamic factor KV (worm)", kv);
    f(p, "wheel contact stress σH, approximate (MPa)", sigma_h);
    f(p, "wheel contact safety SH, approximate", s_h);
    f(p, "wheel form factor YF", rf.yf);
    f(p, "wheel stress correction YS", rf.ys);
    f(p, "wheel root stress σF (MPa)", sigma_f);
    f(p, "wheel bending safety SF", s_f);
    f(p, "minimum bending safety SF", s_f);
    f(p, "minimum contact safety SH", s_h);
    let mr = MemberRating { sigma_h, s_h, sigma_f, s_f, ..MemberRating::default() };
    warn_member(p, "wheel", mat_w, &mr);
    if mat_w != Material::Bronze
        || !matches!(mat_s, Material::SteelCaseHardened | Material::SteelThroughHardened | Material::SteelNitrided | Material::SteelNormalized)
    {
        p.warn("worm friction and efficiency assume a steel worm on a bronze wheel; other pairs run with more friction");
    }
    p.warn("worm rating is approximate (forces, efficiency and a wheel check as a helical gear on a rack); rate a production worm drive to DIN 3996 / ISO/TS 14521 and check its heat");
    plastic_note(p, &[mat_w, mat_s]);
}

fn crossed(s: &Spec, p: &mut Plan) {
    let l = &s.load;
    let sigma = s.shaft_angle.to_radians();
    let b1 = s.helix.to_radians();
    let b2 = sigma - b1;
    let g1 = s.involute(s.teeth, s.shift, b1.to_degrees());
    let an = s.pressure_angle.to_radians();
    let ft1 = 2000.0 * l.torque_nm() / (2.0 * g1.r());
    let fnn = ft1 / (an.cos() * b1.cos());
    load_facts(p, l, "gear 1");
    f(p, "gear 1 tangential force Ft (N)", ft1);
    f(p, "gear 1 axial force Fa (N)", ft1 * b1.tan().abs());
    f(p, "gear 2 tangential force (N)", fnn * an.cos() * b2.cos().abs());
    f(p, "radial force Fr (N)", fnn * an.sin());
    let v1 = PI * 2.0 * g1.r() * l.speed / 60_000.0;
    if l.speed > 0.0 {
        f(p, "sliding velocity (m/s)", v1 * sigma.sin().abs() / b2.cos().abs().max(1e-6));
    }
    p.warn("crossed helical gears touch at a point: no stress rating, only forces; they suit light loads");
    plastic_note(p, &[l.material, l.material2]);
}
