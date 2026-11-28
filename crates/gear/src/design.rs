//! Design by goal: from what a gear pair must do (a ratio, a size or a centre distance, a load)
//! to ranked tooth counts, modules and profile shifts ([`suggest`]), and a total ratio split
//! into stages ([`split_ratio`]).
//!
//! The numbers of each suggestion are those the pair's plan gives (same centre distance, tip
//! shortening and nominal contact ratio; the safety factors are the plan's rating for the same
//! load), so a suggestion can be applied as it stands ([`Suggestion::spec`]).

use std::f64::consts::PI;

use crate::involute::{BasicRack, Involute, mesh_external, shift_sum_for_center};
use crate::plan::{Kind, Spec};
use crate::strength::{Load, Member, MeshLoad, SF_MIN, SH_MIN, rate_mesh};

/// ISO 54 modules, series I (preferred), in mm.
pub const ISO54_SERIES_1: [f64; 18] = [1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 32.0, 40.0, 50.0];
/// ISO 54 modules, series II (second choice; 6.5 is to be avoided and left out).
pub const ISO54_SERIES_2: [f64; 17] = [1.125, 1.375, 1.75, 2.25, 2.75, 3.5, 4.5, 5.5, 7.0, 9.0, 11.0, 14.0, 18.0, 22.0, 28.0, 36.0, 45.0];
/// Usual fine modules below ISO 54's range (DIN 780 series I), for small printed and
/// instrument gears.
pub const FINE_MODULES: [f64; 10] = [0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9];

/// The standard modules from `min` to `max` (mm), ascending; series II too when `second_choice`.
pub fn standard_modules(min: f64, max: f64, second_choice: bool) -> Vec<f64> {
    let mut m: Vec<f64> = FINE_MODULES
        .iter()
        .chain(&ISO54_SERIES_1)
        .chain(if second_choice { ISO54_SERIES_2.iter() } else { [].iter() })
        .copied()
        .filter(|v| *v >= min - 1e-9 && *v <= max + 1e-9)
        .collect();
    m.sort_by(f64::total_cmp);
    m
}

/// What to rank the suggestions by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Prefer {
    /// The least room taken (centre distance plus the tip radii); strong enough when a load is
    /// given.
    #[default]
    Smallest,
    /// The highest contact ratio (many fine teeth).
    Quietest,
    /// The highest safety factors for the load (a nominal one when none is given).
    Strongest,
}

impl Prefer {
    pub const ALL: [Prefer; 3] = [Prefer::Smallest, Prefer::Quietest, Prefer::Strongest];
    pub fn name(self) -> &'static str {
        match self {
            Prefer::Smallest => "smallest",
            Prefer::Quietest => "quietest",
            Prefer::Strongest => "strongest",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Prefer::Smallest => "Smallest",
            Prefer::Quietest => "Quietest (highest contact ratio)",
            Prefer::Strongest => "Strongest",
        }
    }
    pub fn parse(s: &str) -> Option<Prefer> {
        Prefer::ALL.iter().copied().find(|p| p.name() == s)
    }
}

/// What a gear pair must do. Lengths in mm, angles in degrees; zero means "free" where the
/// field says so.
#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    /// Target ratio: gear 2's teeth over gear 1's (over 1 reduces speed from gear 1 to gear 2).
    pub ratio: f64,
    /// How far the ratio may be off, as a fraction of it (0.01: within 1 %; 0: exact).
    pub ratio_tolerance: f64,
    /// Largest tip diameter either gear may have (0: free).
    pub max_outer_diameter: f64,
    /// Fixed working centre distance (0: free). Reached by tooth counts alone, or by profile
    /// shift when `allow_shift`.
    pub center_distance: f64,
    /// Module range, from the standard series ([`standard_modules`]).
    pub min_module: f64,
    pub max_module: f64,
    /// Offer ISO 54's second-choice modules too.
    pub second_choice_modules: bool,
    pub pressure_angle: f64,
    pub helix: f64,
    pub rack: BasicRack,
    pub backlash: f64,
    /// Face width (0: 10 × module).
    pub face_width: f64,
    /// Fewest teeth on a gear (0: the fewest without undercut, or 8 when shift is allowed).
    /// Without `allow_shift` no gear is undercut whatever this says.
    pub min_teeth: u32,
    /// Most teeth on a gear.
    pub max_teeth: u32,
    /// Profile shift may be used: to avoid undercut on few teeth and to reach a centre distance.
    pub allow_shift: bool,
    /// The load to rank by and report safety factors for (none: not rated). Torque and speed
    /// are gear 1's.
    pub load: Load,
    pub prefer: Prefer,
    /// Most suggestions to return.
    pub limit: usize,
}

impl Default for Goal {
    fn default() -> Self {
        Self {
            ratio: 2.0,
            ratio_tolerance: 0.01,
            max_outer_diameter: 0.0,
            center_distance: 0.0,
            min_module: 1.0,
            max_module: 4.0,
            second_choice_modules: false,
            pressure_angle: 20.0,
            helix: 0.0,
            rack: BasicRack::default(),
            backlash: 0.0,
            face_width: 0.0,
            min_teeth: 0,
            max_teeth: 150,
            allow_shift: false,
            load: Load::default(),
            prefer: Prefer::Smallest,
            limit: 12,
        }
    }
}

/// One way to meet a [`Goal`].
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub teeth: u32,
    pub teeth2: u32,
    pub module: f64,
    pub shift: f64,
    pub shift2: f64,
    pub face_width: f64,
    /// teeth2 / teeth, and how far that is from the goal as a fraction of it (signed).
    pub ratio: f64,
    pub ratio_error: f64,
    /// Working centre distance.
    pub center_distance: f64,
    /// Tip diameters.
    pub outer_diameter: f64,
    pub outer_diameter2: f64,
    /// Transverse contact ratio and, with a helix, the total (transverse + overlap).
    pub contact_ratio: f64,
    pub total_contact_ratio: f64,
    /// The lower of the two gears' bending (SF) and contact (SH) safety factors, when the goal
    /// gives a load.
    pub safety_bending: Option<f64>,
    pub safety_contact: Option<f64>,
    /// With a load: both safeties reach the usual minimums (SF 1.4, SH 1.0).
    pub strong_enough: Option<bool>,
    /// The tooth counts share no factor: every tooth meets every tooth of the mate (even wear).
    pub hunting: bool,
    /// What it was ranked by (lower is better; comparable only within one call).
    pub score: f64,
}

impl Suggestion {
    /// The pair as a spec to plan (the goal's tooth form, helix, backlash and load).
    pub fn spec(&self, goal: &Goal) -> Spec {
        Spec {
            kind: Kind::Pair,
            module: self.module,
            teeth: self.teeth,
            teeth2: self.teeth2,
            pressure_angle: goal.pressure_angle,
            helix: goal.helix,
            shift: self.shift,
            shift2: self.shift2,
            rack: goal.rack,
            backlash: goal.backlash,
            face_width: self.face_width,
            load: goal.load.clone(),
            ..Spec::default()
        }
    }
}

/// Validates a goal; an error says which value and why.
pub fn check(g: &Goal) -> Result<(), String> {
    if !(g.ratio.is_finite() && g.ratio > 0.0) {
        return Err("ratio must be greater than 0".into());
    }
    if !(g.ratio_tolerance.is_finite() && (0.0..=0.5).contains(&g.ratio_tolerance)) {
        return Err("ratio tolerance must be between 0 and 0.5".into());
    }
    for (v, what) in [
        (g.max_outer_diameter, "largest outer diameter"),
        (g.center_distance, "centre distance"),
        (g.face_width, "face width"),
        (g.backlash, "backlash"),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("{what} can't be negative"));
        }
    }
    if !(g.min_module.is_finite() && g.min_module > 0.0 && g.max_module.is_finite() && g.max_module >= g.min_module) {
        return Err("module range must be positive, smallest first".into());
    }
    if !(5.0..=45.0).contains(&g.pressure_angle) {
        return Err("pressure angle must be between 5° and 45°".into());
    }
    if !(g.helix.is_finite() && g.helix.abs() < 60.0) {
        return Err("helix angle must be under 60°".into());
    }
    if g.max_teeth < 3 || g.max_teeth > 2000 || g.min_teeth > g.max_teeth {
        return Err("teeth limits must be between 3 and 2000, fewest first".into());
    }
    crate::strength::check(&g.load)
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// A candidate before it is rated.
struct Candidate {
    g1: Involute,
    g2: Involute,
    face: f64,
    mesh: crate::involute::Mesh,
}

/// The pair as its plan makes it: shifted gears' tips shortened to keep the rack's clearance.
fn candidate(goal: &Goal, m: f64, z1: u32, z2: u32, x1: f64, x2: f64) -> Option<Candidate> {
    let gear = |z: u32, x: f64, helix: f64| Involute {
        teeth: z,
        module: m,
        pressure_angle: goal.pressure_angle.to_radians(),
        helix: helix.to_radians(),
        shift: x,
        rack: goal.rack,
        backlash: goal.backlash,
        tip_shortening: 0.0,
        virtual_teeth: 0.0,
    };
    let (mut g1, mut g2) = (gear(z1, x1, goal.helix), gear(z2, x2, -goal.helix));
    let face = if goal.face_width > 0.0 { goal.face_width } else { 10.0 * m };
    let shorten = (x1 + x2) * m - (mesh_external(&g1, &g2, face).center_distance - g1.r() - g2.r());
    if shorten > 1e-9 {
        g1.tip_shortening = shorten;
        g2.tip_shortening = shorten;
    }
    let mesh = mesh_external(&g1, &g2, face);
    // Teeth that work: no undercut, a tip land, tips clear of the mating roots, and contact
    // handed from tooth to tooth with something to spare.
    let sound = |g: &Involute| !g.undercut() && g.thickness_at(g.ra()) >= 0.25 * m && g.rf() > 0.0 && g.ra() > g.rb();
    if !sound(&g1) || !sound(&g2) || mesh.clearance_1 < 0.0 || mesh.clearance_2 < 0.0 || !(mesh.contact_ratio >= 1.1) {
        return None;
    }
    Some(Candidate { g1, g2, face, mesh })
}

/// The profile shift that just clears undercut on `z` teeth (0 when there is none to clear),
/// rounded up to a hundredth.
fn shift_against_undercut(goal: &Goal, z: u32) -> f64 {
    let probe = Involute {
        teeth: z,
        module: 1.0,
        pressure_angle: goal.pressure_angle.to_radians(),
        helix: goal.helix.to_radians(),
        shift: 0.0,
        rack: goal.rack,
        ..Involute::default()
    };
    let least = probe.min_teeth_no_undercut();
    let x = goal.rack.addendum * (least - f64::from(z)) / least;
    if x <= 0.0 { 0.0 } else { (x * 100.0).ceil() / 100.0 + 0.01 }
}

/// The lower bending and contact safety of the pair under `load` (gear 1 driving), as the
/// plan rates a pair (on the gears' full tips).
fn safeties(c: &Candidate, load: &Load) -> (f64, f64) {
    let (t, n) = (load.torque_nm(), load.speed);
    let full = |g: &Involute| Involute { tip_shortening: 0.0, ..*g };
    let c = &Candidate { g1: full(&c.g1), g2: full(&c.g2), face: c.face, mesh: mesh_external(&full(&c.g1), &full(&c.g2), c.face) };
    let cycles = |rpm: f64| if load.life_hours > 0.0 { 60.0 * rpm.abs() * load.life_hours } else { 0.0 };
    let n2 = n * c.g1.z() / c.g2.z();
    let member =
        |name: &str, gear: Involute, material, cycles: f64| Member { name: name.into(), gear, internal: false, material, cycles, idler: false };
    let ml = MeshLoad {
        members: [member("gear 1", c.g1, load.material, cycles(n)), member("gear 2", c.g2, load.material2, cycles(n2))],
        mesh: c.mesh,
        face: c.face,
        overlap: c.mesh.overlap_ratio,
        ft: 2000.0 * t / (2.0 * c.g1.r()),
        v: PI * 2.0 * c.g1.r() * n / 60_000.0,
        ka: load.application_factor,
        quality: load.quality,
        k_beta: 1.0,
    };
    let r = rate_mesh(&ml);
    (r.members[0].s_f.min(r.members[1].s_f), r.members[0].s_h.min(r.members[1].s_h))
}

/// Most candidates rated in full in one call (each takes a few milliseconds).
const MOST_RATED: usize = 48;

/// A quick estimate of the safeties: the rating's contact stress without its dynamic and
/// single-pair factors, and a root stress with a typical form factor (YF·YS = 4.4) in place of
/// the measured one. Within a third or so of the rating: enough to put candidates in order.
fn estimate(c: &Candidate, load: &Load) -> (f64, f64) {
    use crate::strength::{contact_ratio_factor, elasticity_factor, helix_factor_contact, zone_factor};
    let ft = 2000.0 * load.torque_nm() / (2.0 * c.g1.r()) * load.application_factor;
    let u = c.g2.z() / c.g1.z();
    let small = c.g1.r().min(c.g2.r());
    let (a, b) = (load.material.data(), load.material2.data());
    let z = zone_factor(&c.g1, c.mesh.working_pressure_angle)
        * elasticity_factor(load.material, load.material2)
        * contact_ratio_factor(c.mesh.contact_ratio, c.mesh.overlap_ratio)
        * helix_factor_contact(c.g1.helix);
    // On the smaller gear, whichever drives: the same force on its smaller radius of curvature.
    let sigma_h = z * (ft * c.g1.r() / small / (2.0 * small * c.face) * (u.max(1.0 / u) + 1.0) / u.max(1.0 / u)).sqrt();
    let sigma_f = ft / (c.face * c.g1.module) * 4.4;
    (a.sigma_fe.min(b.sigma_fe) / sigma_f, a.sigma_h_lim.min(b.sigma_h_lim) / sigma_h)
}

/// Gear pairs that meet the goal, best first by its preference: tooth counts, a standard
/// module and profile shifts, with the ratio error, centre distance, tip diameters, contact
/// ratio and (with a load) safety factors. Empty when nothing fits (or the goal is not valid:
/// see [`check`]).
pub fn suggest(goal: &Goal) -> Vec<Suggestion> {
    if check(goal).is_err() || goal.limit == 0 {
        return Vec::new();
    }
    let beta = goal.helix.to_radians();
    let tol = goal.ratio_tolerance + 1e-12;
    let mut found: Vec<Candidate> = Vec::new();
    for m in standard_modules(goal.min_module, goal.max_module, goal.second_choice_modules) {
        if goal.backlash >= PI * m / 4.0 {
            continue;
        }
        let mt = m / beta.cos();
        // The fewest teeth: without undercut, unless a shift may clear it.
        let no_undercut =
            Involute { module: m, pressure_angle: goal.pressure_angle.to_radians(), helix: beta, rack: goal.rack, ..Involute::default() }
                .min_teeth_no_undercut()
                .ceil()
                .max(3.0) as u32;
        let fewest = if goal.allow_shift { if goal.min_teeth > 0 { goal.min_teeth } else { 8 } } else { goal.min_teeth.max(no_undercut) }.max(3);
        // The most: the limit, and what the size allows.
        let mut most = goal.max_teeth;
        if goal.max_outer_diameter > 0.0 {
            most = most.min((goal.max_outer_diameter / mt).floor() as u32);
        }
        if goal.center_distance > 0.0 {
            most = most.min((2.0 * goal.center_distance / mt).ceil() as u32 + 3);
        }
        for z1 in fewest..=most {
            let (lo, hi) = (goal.ratio * (1.0 - tol) * f64::from(z1), goal.ratio * (1.0 + tol) * f64::from(z1));
            let (lo, hi) = ((lo - 1e-9).ceil().max(f64::from(fewest)) as u32, (hi + 1e-9).floor().min(f64::from(most)) as u32);
            for z2 in lo..=hi {
                let (x1, x2);
                if goal.center_distance > 0.0 {
                    // Close enough in teeth for a shift to reach (or exact without one).
                    let a0 = mt * f64::from(z1 + z2) / 2.0;
                    if (goal.center_distance - a0).abs() > 1.3 * m {
                        continue;
                    }
                    let probe = |z: u32, helix: f64| Involute {
                        teeth: z,
                        module: m,
                        pressure_angle: goal.pressure_angle.to_radians(),
                        helix,
                        rack: goal.rack,
                        ..Involute::default()
                    };
                    let Some(sum) = shift_sum_for_center(&probe(z1, beta), &probe(z2, -beta), goal.center_distance) else { continue };
                    if sum.abs() < 1e-7 {
                        (x1, x2) = (0.0, 0.0);
                    } else if !goal.allow_shift || !(-0.5..=1.3).contains(&sum) {
                        continue;
                    } else {
                        // Most of a positive sum to the smaller gear, of a negative one to the
                        // larger; never less on a gear than clears its undercut.
                        let (small, large) = (z1.min(z2), z1.max(z2));
                        let share = f64::from(large) / f64::from(small + large);
                        let mut xs = if sum > 0.0 { sum * share } else { sum * (1.0 - share) };
                        xs = xs.max(shift_against_undercut(goal, small).min(sum.max(0.0) + 0.5));
                        let xl = sum - xs;
                        (x1, x2) = if z1 <= z2 { (xs, xl) } else { (xl, xs) };
                    }
                } else if goal.allow_shift {
                    (x1, x2) = (shift_against_undercut(goal, z1), shift_against_undercut(goal, z2));
                } else {
                    (x1, x2) = (0.0, 0.0);
                }
                let Some(c) = candidate(goal, m, z1, z2, x1, x2) else { continue };
                if goal.center_distance > 0.0 && (c.mesh.center_distance - goal.center_distance).abs() > 1e-6 * goal.center_distance.max(1.0) {
                    continue;
                }
                if goal.max_outer_diameter > 0.0 && 2.0 * c.g1.ra().max(c.g2.ra()) > goal.max_outer_diameter + 1e-9 {
                    continue;
                }
                found.push(c);
            }
        }
    }
    let room = |c: &Candidate| c.mesh.center_distance + c.g1.ra() + c.g2.ra();
    // With no load given, "strongest" is judged under a nominal one.
    let given = goal.load.rated();
    let rate = given || goal.prefer == Prefer::Strongest;
    let load = if given { goal.load.clone() } else { Load { torque: 1.0, ..goal.load.clone() } };
    // A ratio off the mark and tooth counts with a common factor count a little against a
    // candidate.
    let handicap = |c: &Candidate| (1.0 + 5.0 * (c.mesh.ratio / goal.ratio - 1.0).abs()) * if gcd(c.g1.teeth, c.g2.teeth) == 1 { 1.0 } else { 1.01 };
    let margin = |(sf, sh): (f64, f64)| (sf / SF_MIN).min(sh / SH_MIN);
    // Lower is better; `rated` is the pair's safeties where the preference needs them.
    let merit = |c: &Candidate, rated: Option<(f64, f64)>| match goal.prefer {
        Prefer::Smallest => room(c) * handicap(c),
        Prefer::Quietest => -c.mesh.total_contact_ratio / handicap(c),
        Prefer::Strongest => -rated.map_or(0.0, margin) / handicap(c),
    };
    // Rating a pair properly takes milliseconds (its root form is measured on the generated
    // tooth), so the candidates are put in order by a quick estimate and only the front of the
    // queue is rated: those the estimate does not write off as far too weak.
    let mut order: Vec<(f64, usize)> = found.iter().enumerate().map(|(i, c)| (merit(c, rate.then(|| estimate(c, &load))), i)).collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let budget = if rate { (4 * goal.limit).clamp(16, MOST_RATED) } else { usize::MAX };
    let mut picked: Vec<(f64, usize, Option<(f64, f64)>)> = Vec::new();
    let (mut spent, mut strong) = (0, 0);
    for (quick, i) in order {
        let c = &found[i];
        if !rate {
            picked.push((quick, i, None));
            continue;
        }
        if spent >= budget || (goal.prefer != Prefer::Strongest && strong >= goal.limit) {
            break;
        }
        if given && goal.prefer != Prefer::Strongest && margin(estimate(c, &load)) < 0.6 {
            continue;
        }
        let exact = safeties(c, &load);
        spent += 1;
        let enough = margin(exact) >= 1.0;
        strong += usize::from(enough);
        // A pair too weak for the load goes behind all that are not.
        let behind = if given && !enough && goal.prefer != Prefer::Strongest { 1e9 } else { 0.0 };
        picked.push((merit(c, Some(exact)) + behind, i, Some(exact)));
    }
    let mut out: Vec<Suggestion> = picked
        .into_iter()
        .map(|(score, i, rated)| {
            let c = &found[i];
            Suggestion {
                teeth: c.g1.teeth,
                teeth2: c.g2.teeth,
                module: c.g1.module,
                shift: c.g1.shift,
                shift2: c.g2.shift,
                face_width: c.face,
                ratio: c.mesh.ratio,
                ratio_error: c.mesh.ratio / goal.ratio - 1.0,
                center_distance: c.mesh.center_distance,
                outer_diameter: 2.0 * c.g1.ra(),
                outer_diameter2: 2.0 * c.g2.ra(),
                contact_ratio: c.mesh.contact_ratio,
                total_contact_ratio: c.mesh.total_contact_ratio,
                safety_bending: rated.filter(|_| given).map(|r| r.0),
                safety_contact: rated.filter(|_| given).map(|r| r.1),
                strong_enough: rated.filter(|_| given).map(|r| margin(r) >= 1.0),
                hunting: gcd(c.g1.teeth, c.g2.teeth) == 1,
                score,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.score
            .total_cmp(&b.score)
            .then(a.ratio_error.abs().total_cmp(&b.ratio_error.abs()))
            .then(a.module.total_cmp(&b.module))
            .then(a.teeth.cmp(&b.teeth))
    });
    out.truncate(goal.limit);
    out
}

// ---------------------------------------------------------------------------------------------
// Multi-stage ratios.

/// Limits on each stage of a gearbox for [`split_ratio`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageLimits {
    /// Fewest and most teeth on a gear.
    pub min_teeth: u32,
    pub max_teeth: u32,
    /// Largest ratio one stage may have.
    pub max_stage_ratio: f64,
    /// How far the total may be off, as a fraction of it; splits within it rank ahead.
    pub tolerance: f64,
}

impl Default for StageLimits {
    fn default() -> Self {
        Self { min_teeth: 17, max_teeth: 120, max_stage_ratio: 6.0, tolerance: 0.005 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stage {
    /// Driving and driven teeth of the stage (the driver is the larger when the total is under 1).
    pub teeth: u32,
    pub teeth2: u32,
    /// teeth2 / teeth.
    pub ratio: f64,
    /// The tooth counts share no factor.
    pub hunting: bool,
}

/// A total ratio as stages in series.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    /// The stages, the lowest ratio first.
    pub stages: Vec<Stage>,
    /// The product of the stage ratios, and how far it is from the total asked for as a
    /// fraction of it (signed).
    pub ratio: f64,
    pub error: f64,
}

/// Splits `total` (driven over driving; over 1 reduces) into `stages` gear pairs in series
/// with near-equal stage ratios, tooth counts within the limits and without a common factor
/// where that costs little. Best first: splits within the tolerance, the most even and
/// hunting-toothed, then the most accurate. Empty when the limits can't reach the total.
pub fn split_ratio(total: f64, stages: u32, limits: &StageLimits) -> Vec<Split> {
    let l = limits;
    if !(total.is_finite() && total > 0.0) || stages == 0 || stages > 8 || l.min_teeth < 3 || l.max_teeth < l.min_teeth || !(l.max_stage_ratio >= 1.0)
    {
        return Vec::new();
    }
    if total < 1.0 {
        // A speed increaser: the reducer for 1 / total, run backwards.
        let mut out = split_ratio(1.0 / total, stages, limits);
        for s in &mut out {
            for st in &mut s.stages {
                *st = Stage { teeth: st.teeth2, teeth2: st.teeth, ratio: 1.0 / st.ratio, hunting: st.hunting };
            }
            s.stages.reverse();
            s.ratio = 1.0 / s.ratio;
            s.error = s.ratio / total - 1.0;
        }
        return out;
    }
    let n = stages as usize;
    let even = total.powf(1.0 / f64::from(stages));
    let cap = l.max_stage_ratio.min(f64::from(l.max_teeth) / f64::from(l.min_teeth));
    if even > cap * (1.0 + l.tolerance.max(0.0)) + 1e-9 {
        return Vec::new();
    }
    // A stage's cost: how far its ratio is from the even one, and a little for a common factor
    // and for size.
    let cost = |z1: u32, z2: u32| -> f64 {
        let r = f64::from(z2) / f64::from(z1);
        (r / even).ln().abs() + if gcd(z1, z2) == 1 { 0.0 } else { 0.05 } + 2e-5 * f64::from(z1 + z2)
    };
    // Stage choices near the even ratio (within about a quarter either way), smaller pinions
    // first: plenty for all but the last stage, which then closes the total.
    let mut near: Vec<(u32, u32)> = Vec::new();
    let pinions = l.min_teeth..=l.max_teeth.min(l.min_teeth + 40);
    for z1 in pinions.clone() {
        let (lo, hi) = (
            (even / 1.25 * f64::from(z1)).ceil().max(f64::from(z1)) as u32,
            (even.min(cap) * 1.25 * f64::from(z1)).floor().min(f64::from(l.max_teeth)) as u32,
        );
        for z2 in lo..=hi {
            if f64::from(z2) / f64::from(z1) <= cap + 1e-9 {
                near.push((z1, z2));
            }
        }
    }
    near.sort_by(|a, b| cost(a.0, a.1).total_cmp(&cost(b.0, b.1)));
    near.truncate(400);
    // Partial gearboxes, the most promising kept at each stage.
    const BEAM: usize = 300;
    let mut beam: Vec<(Vec<(u32, u32)>, f64, f64)> = vec![(Vec::new(), 1.0, 0.0)];
    for _ in 1..n {
        let mut next = Vec::new();
        for (st, prod, c) in &beam {
            // In order, so that the same stages in another order are not tried twice.
            for &(z1, z2) in &near {
                if st.last().is_none_or(|last| (z1, z2) >= *last) {
                    let mut s2 = st.clone();
                    s2.push((z1, z2));
                    next.push((s2, prod * f64::from(z2) / f64::from(z1), c + cost(z1, z2)));
                }
            }
        }
        next.sort_by(|a, b| a.2.total_cmp(&b.2));
        next.truncate(BEAM);
        beam = next;
    }
    let mut out: Vec<(Split, f64)> = Vec::new();
    for (st, prod, c) in &beam {
        let need = total / prod;
        if need > cap * 1.02 || need < 1.0 / 1.02 {
            continue;
        }
        for z1 in l.min_teeth..=l.max_teeth {
            let ideal = need * f64::from(z1);
            for z2 in [ideal.floor() as u32, ideal.ceil() as u32] {
                if z2 < z1.max(l.min_teeth) || z2 > l.max_teeth || f64::from(z2) / f64::from(z1) > cap + 1e-9 {
                    continue;
                }
                let mut all = st.clone();
                all.push((z1, z2));
                let ratio = prod * f64::from(z2) / f64::from(z1);
                let error = ratio / total - 1.0;
                let mut stages: Vec<Stage> =
                    all.iter().map(|&(a, b)| Stage { teeth: a, teeth2: b, ratio: f64::from(b) / f64::from(a), hunting: gcd(a, b) == 1 }).collect();
                stages.sort_by(|a, b| a.ratio.total_cmp(&b.ratio).then(a.teeth.cmp(&b.teeth)));
                // Within the tolerance first; there, evenness and hunting teeth decide, with the
                // error only to break ties. Outside it, the error decides.
                let within = error.abs() <= l.tolerance + 1e-12;
                let rank = if within { c + cost(z1, z2) + error.abs() } else { 1e3 + error.abs() * 1e3 + c + cost(z1, z2) };
                out.push((Split { stages, ratio, error }, rank));
            }
        }
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut best: Vec<Split> = Vec::new();
    for (s, _) in out {
        if !best.iter().any(|b| b.stages == s.stages) {
            best.push(s);
            if best.len() == 8 {
                break;
            }
        }
    }
    best
}

#[cfg(test)]
#[path = "design_tests.rs"]
mod tests;
