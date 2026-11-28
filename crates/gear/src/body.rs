//! What turns a toothed blank into a part: integral shafts (one on each side), edge chamfers,
//! hub hardware and bearing pockets, and the tooth form's tuning for printing (tip rounding,
//! horizontal-expansion compensation, elephant-foot relief), tip relief and crowning.
//!
//! Everything here is off by default: a [`Spec`] that sets none of it plans exactly as before.
//!
//! Conventions. A cylindrical gear's body runs from z = 0 (its back face; on the print bed) to
//! z = face (its front face, where the hub is). The front shaft grows from the front face along
//! +z, the back shaft from the back face along −z. Chamfers are cut by revolving a triangle that
//! overlaps the edge (never one that merely touches it), which the kernel's boolean takes for
//! any tooth form, helical ones included.
//!
//! The horizontal-expansion (XY) compensation model: a printed outline comes out fatter than
//! drawn by about the same amount δ everywhere, so the teeth are drawn δ inside where they should
//! end up. An involute moved δ along its normals is the same involute turned δ / r_b about the
//! centre, so each flank is thinned by exactly that (2·δ / cos α_t more backlash on the pitch
//! circle), the tip circle comes down by δ and the root circle goes down by δ (the rack's
//! dedendum grows by δ / m, the root fillet with it). Bores and shafts are not compensated.

use std::f64::consts::{FRAC_PI_2, PI};

use crate::axes::num;
use crate::geom::{Outline, Region, Seg, V2};
use crate::hub::{Bore, BoreKind, Hardware, Shaft, ShaftEnd, din6885};
use crate::involute::{BasicRack, Involute, ToothFlank, external_flank, shape_tips, teeth_outline};
use crate::plan::{Frame, Kind, Op, Plan, Spec, Step};

/// How far a cut runs past the surface it opens onto (into the air), in mm.
const OVER: f64 = 1.0;

// ---------------------------------------------------------------------------------------------
// The spec's side of it.

impl Kind {
    /// Its teeth take this module's tooth tuning (tip rounding, horizontal-expansion
    /// compensation, elephant-foot relief, tip relief, crowning, tip diameters, the nozzle
    /// check), as far as each type's teeth take it (`plan::left_out` says which, and warns about
    /// the rest). Sprockets, pulleys, splines, face gears and elliptical pairs take none: their
    /// teeth come from their own generators.
    pub fn tuned_teeth(self) -> bool {
        !matches!(self, Kind::Sprocket | Kind::Pulley | Kind::Spline | Kind::FaceGear | Kind::Elliptical)
    }
}

impl Spec {
    /// The basic rack the teeth are really cut with: the chosen rack, with the tip clearance
    /// override and the horizontal-expansion compensation's deeper root.
    pub fn rack_eff(&self) -> BasicRack {
        let mut r = self.rack;
        if self.clearance > 0.0 {
            r.dedendum = r.addendum + self.clearance;
        }
        if self.xy_compensation > 0.0 && self.module > 0.0 {
            r.dedendum += self.xy_compensation / self.module;
        }
        r
    }
    /// The tip clearance coefficient c* in use (the rack's dedendum less its addendum, unless
    /// overridden).
    pub fn clearance_eff(&self) -> f64 {
        if self.clearance > 0.0 { self.clearance } else { self.rack.dedendum - self.rack.addendum }
    }
    /// The extra thinning of a tooth on the pitch circle (mm of arc) that moves both flanks
    /// `xy_compensation` inwards along their normals, for a gear of helix angle `helix_deg`.
    pub fn xy_thinning(&self, helix_deg: f64) -> f64 {
        if !(self.xy_compensation > 0.0) {
            return 0.0;
        }
        let at = (self.pressure_angle.to_radians().tan() / helix_deg.to_radians().cos()).atan();
        2.0 * self.xy_compensation / at.cos()
    }
    /// Gear 2's backlash (its own, or gear 1's when it has none of its own).
    pub fn backlash_of2(&self) -> f64 {
        self.backlash2.unwrap_or(self.backlash)
    }
    /// Like [`Spec::involute`] for the second member of a set (mate, planet): its own backlash.
    pub(crate) fn involute2(&self, teeth: u32, shift: f64, helix_deg: f64) -> Involute {
        let mut g = self.involute(teeth, shift, helix_deg);
        g.backlash += self.backlash_of2() - self.backlash;
        g
    }
    /// Gear 2's face width (gear 1's unless it has its own).
    pub fn face_width_of2(&self) -> f64 {
        if self.face_width2 > 0.0 { self.face_width2 } else { self.face_width }
    }
    pub fn shaft(&self, second: bool, front: bool) -> &Shaft {
        match (second, front) {
            (false, true) => &self.shaft_front,
            (false, false) => &self.shaft_back,
            (true, true) => &self.shaft2_front,
            (true, false) => &self.shaft2_back,
        }
    }
    pub fn hardware_of(&self, second: bool) -> &Hardware {
        if second { &self.hardware2 } else { &self.hardware }
    }
    /// Recommended settings for FDM printing with a nozzle of width `nozzle` at layers
    /// `layer_height` thick (mm; 0.4 and 0.2 are the usual): the `print_fdm` rack at a 25°
    /// pressure angle (stubbier, stronger teeth that tolerate small tooth counts), rounded tips,
    /// enough backlash to turn freely, a modest horizontal-expansion compensation, elephant-foot
    /// relief over the first layer, a first-layer-sized chamfer on the bore, and the nozzle for
    /// the tip land check. Tune `xy_compensation` to the printer (print a pair, measure the
    /// span over teeth against the "span over k teeth" fact).
    pub fn apply_print_defaults(&mut self, nozzle: f64, layer_height: f64) {
        let nozzle = if nozzle.is_finite() && nozzle > 0.0 { nozzle } else { 0.4 };
        let layer = if layer_height.is_finite() && layer_height > 0.0 { layer_height } else { 0.2 };
        let m = self.module.max(1e-9);
        // Only what the type uses is set (`plan::left_out`): sprockets, pulleys, splines, face
        // gears and elliptical pairs make their teeth with their own generators and get only the
        // backlash (a spline's flank clearance), the bore chamfer and the bearing fit; a rack or
        // a ring gear gets the compensation but no tip rounding, and so on.
        use crate::plan::left_out::{Opt, uses};
        if self.kind.tuned_teeth() && self.kind.involute() {
            self.rack = BasicRack::standard("print_fdm").unwrap_or(self.rack);
            self.pressure_angle = self.pressure_angle.max(25.0);
        }
        if uses(self, Opt::Nozzle) {
            self.nozzle = nozzle;
        }
        if uses(self, Opt::TipRound) {
            // A quarter nozzle of radius at the least, but no more than a quarter module.
            self.tip_round = (0.5 * nozzle / m).clamp(0.05, 0.25);
        }
        // (Small modules can't give up as much: these stay well inside what `check` allows.)
        if uses(self, Opt::XyCompensation) {
            self.xy_compensation = (0.125 * nozzle).min(0.05 * m);
        }
        if uses(self, Opt::ElephantFoot) {
            self.elephant_foot = (0.5 * layer).min(0.05 * m);
            self.elephant_foot_height = layer;
        }
        self.backlash = self.backlash.max((0.25 * nozzle + 0.05).min(0.3 * m));
        if uses(self, Opt::BoreChamfer) {
            self.bore_chamfer = self.bore_chamfer.max(layer);
        }
        self.hardware.bearing_fit = self.hardware.bearing_fit.max(0.25 * nozzle);
        self.hardware2.bearing_fit = self.hardware2.bearing_fit.max(0.25 * nozzle);
    }
}

/// Validates the fields this module builds from (called by [`crate::plan::check`]).
pub(crate) fn check(s: &Spec) -> Result<(), String> {
    let shafts = [
        (&s.shaft_front, "front shaft"),
        (&s.shaft_back, "back shaft"),
        (&s.shaft2_front, "gear 2 front shaft"),
        (&s.shaft2_back, "gear 2 back shaft"),
    ];
    let mut sizes: Vec<(f64, String)> = vec![
        (s.tip_round, "tip rounding".into()),
        (s.xy_compensation, "horizontal-expansion compensation".into()),
        (s.elephant_foot, "elephant-foot relief".into()),
        (s.elephant_foot_height, "elephant-foot height".into()),
        (s.nozzle, "nozzle width".into()),
        (s.clearance, "tip clearance".into()),
        (s.tip_shortening, "tip shortening".into()),
        (s.tip_diameter, "tip diameter".into()),
        (s.tip_diameter2, "gear 2 tip diameter".into()),
        (s.tip_relief, "tip relief".into()),
        (s.tip_relief_height, "tip relief height".into()),
        (s.crowning, "crowning".into()),
        (s.backlash_of2(), "gear 2 backlash".into()),
        (s.face_width2, "gear 2 face width".into()),
        (s.tooth_chamfer_front.size, "front tooth chamfer".into()),
        (s.tooth_chamfer_back.size, "back tooth chamfer".into()),
        (s.bore_chamfer, "bore chamfer".into()),
        (s.hub_chamfer, "hub chamfer".into()),
    ];
    for (sh, what) in shafts {
        for (v, part) in [
            (sh.diameter, "diameter"),
            (sh.length, "length"),
            (sh.step_diameter, "step diameter"),
            (sh.step_length, "step length"),
            (sh.end_length, "end length"),
            (sh.end_size, "end size"),
            (sh.chamfer, "chamfer"),
        ] {
            sizes.push((v, format!("{what} {part}")));
        }
    }
    for (h, what) in [(&s.hardware, "gear 1"), (&s.hardware2, "gear 2")] {
        for (v, part) in [
            (h.set_screw, "set-screw diameter"),
            (h.nut_width, "nut width"),
            (h.nut_thickness, "nut thickness"),
            (h.insert_diameter, "insert diameter"),
            (h.insert_depth, "insert depth"),
            (h.clamp_slit, "clamp slit"),
            (h.clamp_screw, "clamp screw diameter"),
            (h.bearing_od, "bearing diameter"),
            (h.bearing_width, "bearing width"),
            (h.bearing_fit, "bearing fit allowance"),
        ] {
            sizes.push((v, format!("{what} {part}")));
        }
        if h.set_screws > 4 {
            return Err(format!("{what}: 4 set screws at most"));
        }
    }
    for (v, what) in &sizes {
        if !(v.is_finite() && *v >= 0.0) {
            return Err(format!("{what} can't be negative"));
        }
    }
    if !s.face_offset2.is_finite() {
        return Err("gear 2's face offset must be a number".into());
    }
    for (c, what) in [(&s.tooth_chamfer_front, "front"), (&s.tooth_chamfer_back, "back")] {
        if c.size > 0.0 && !(5.0..=85.0).contains(&c.angle) {
            return Err(format!("the {what} tooth chamfer's angle must be between 5° and 85°"));
        }
    }
    if s.tip_round > 0.5 {
        return Err("tip rounding must be 0.5 modules or less".into());
    }
    if s.clearance > 1.0 {
        return Err("tip clearance must be 1 module or less".into());
    }
    let m = s.module;
    if s.xy_compensation > 0.25 * m || s.elephant_foot > 0.25 * m {
        return Err(format!(
            "horizontal-expansion compensation and elephant-foot relief must each be a quarter module ({} mm) or less",
            num(0.25 * m)
        ));
    }
    if s.tip_shortening > 0.5 * s.rack.addendum * m {
        return Err(format!("tip shortening must be half the addendum ({} mm) or less", num(0.5 * s.rack.addendum * m)));
    }
    if s.tip_relief > 0.1 * m || s.crowning > 0.1 * m {
        return Err(format!("tip relief and crowning must each be a tenth of the module ({} mm) or less", num(0.1 * m)));
    }
    // (A spline has its own tooth proportions and ignores the rack: `plan::extras::check`.)
    if s.kind.involute() && s.kind != Kind::Spline {
        // As `plan::check` does for the rack itself: the cutter must still have a tip with the
        // deeper root the clearance, the compensation and the elephant-foot relief ask for.
        let tan = s.pressure_angle.to_radians().tan();
        let dedendum = s.rack_eff().dedendum + s.elephant_foot / m;
        if PI / 4.0 - dedendum * tan < 0.01 {
            return Err(format!(
                "a {}° pressure angle with a root {} modules deep (clearance, horizontal-expansion compensation and elephant-foot relief included) gives a cutter with pointed teeth: lower one of them",
                num(s.pressure_angle),
                num(dedendum)
            ));
        }
        let widest = s.backlash.max(s.backlash_of2())
            + 2.0 * (s.xy_compensation + s.elephant_foot) / s.pressure_angle.to_radians().cos().max(0.1) / s.helix.to_radians().cos().abs().max(0.17);
        if widest >= PI * m / 4.0 {
            return Err(format!(
                "backlash, horizontal-expansion compensation and elephant-foot relief together must thin the teeth by less than a quarter of the pitch ({} mm)",
                num(PI * m / 4.0)
            ));
        }
        if s.kind == Kind::Rack && s.rack_height > 0.0 && s.rack_height < (dedendum - s.shift + 0.25) * m {
            return Err(format!(
                "rack height below the pitch line must be over {} mm (the tooth roots plus a quarter module)",
                num((dedendum - s.shift + 0.25) * m)
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Teeth.

/// A gear's generated flank with the spec's tip relief and tip rounding, and the nozzle check.
pub(crate) fn flank(s: &Spec, g: &Involute, p: &mut Plan, who: &str) -> ToothFlank {
    let mut f = external_flank(g);
    if f.defect().is_some() {
        return f;
    }
    if s.nozzle > 0.0 {
        let land = if f.pointed { 0.0 } else { g.thickness_at(f.ra) };
        if land < 2.0 * s.nozzle {
            p.warn(format!("{who}: the tip land ({} mm) is under two nozzle widths ({} mm), too thin to print cleanly: use a larger module, a lower addendum, less profile shift or fewer teeth", num(land), num(2.0 * s.nozzle)));
        }
    }
    shape(s, g, &mut f, p, who);
    f
}

fn shape(s: &Spec, g: &Involute, f: &mut ToothFlank, p: &mut Plan, who: &str) {
    let round = s.tip_round * s.module;
    if !(round > 0.0 || s.tip_relief > 0.0) {
        return;
    }
    let height = if s.tip_relief_height > 0.0 { s.tip_relief_height } else { 0.4 * s.module };
    let used = shape_tips(f, g, s.tip_relief, height, round);
    if round >= 0.01 * s.module && used < 0.999 * round && !f.pointed {
        if used > 0.0 {
            p.warn(format!("{who}: the tips are too thin for a {} mm rounding; {} mm is used", num(round), num(used)));
        } else {
            p.warn(format!("{who}: the tips are too thin to round"));
        }
    }
}

/// Teeth placed round the centre with the spec's tip relief and rounding (see
/// [`crate::involute::external_outline`]).
pub(crate) fn teeth(s: &Spec, g: &Involute, phase: f64, p: &mut Plan, who: &str) -> (Outline, ToothFlank) {
    let f = flank(s, g, p, who);
    (teeth_outline(&f, g.teeth, phase), f)
}

/// Lowers a gear's tip to the spec's tip diameter, if it gives one for this member.
pub(crate) fn tip_override(s: &Spec, g: &mut Involute, second: bool, p: &mut Plan, who: &str) {
    let d = if second { s.tip_diameter2 } else { s.tip_diameter };
    if !(d > 0.0) {
        return;
    }
    // The diameter asked for is the finished one: the compensation still comes off it.
    let want = d / 2.0 - s.xy_compensation;
    let now = g.ra();
    if want >= now {
        if want > now + 1e-9 {
            p.warn(format!(
                "{who}: a tip diameter of {} is over the teeth's own (Ø{}); it is left as it is",
                num(d),
                num(2.0 * (now + s.xy_compensation))
            ));
        }
        return;
    }
    if want <= g.r() - 0.25 * g.module {
        p.warn(format!("{who}: a tip diameter of {} leaves no addendum (pitch Ø{}); it is left as it is", num(d), num(2.0 * g.r())));
        return;
    }
    g.tip_shortening += now - want;
}

fn turned(r: &Region, a: f64) -> Region {
    if a == 0.0 {
        return r.clone();
    }
    Region { outer: r.outer.iter().map(|o| o.rotated(a)).collect(), holes: r.holes.iter().map(|o| o.rotated(a)).collect() }
}

fn same_pieces(a: &Outline, b: &Outline) -> bool {
    a.segs.len() == b.segs.len() && a.segs.iter().zip(&b.segs).all(|(x, y)| std::mem::discriminant(x) == std::mem::discriminant(y))
}

/// An external cylindrical gear's teeth from z = 0 to `face`: plain, helical or herringbone,
/// with the spec's elephant-foot relief or crowning. `f` is the gear's flank (from [`flank`]),
/// tooth 0 at `phase` on z = 0.
///
/// Elephant-foot relief: the first `elephant_foot_height` of the teeth is drawn
/// `elephant_foot` inside the rest (flanks, tips and roots, as the horizontal-expansion
/// compensation does it), so the squashed first layers don't stand proud of the flanks.
///
/// Crowning (straight teeth only): the teeth are a loft through three sections, the two at the
/// faces thinner by `crowning` on each flank than the one in the middle, so the teeth bear in
/// the middle of the face when the shafts are a little out of line.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tooth_body(s: &Spec, g: &Involute, f: &ToothFlank, phase: f64, face: f64, herringbone: bool, p: &mut Plan, who: &str) -> Vec<Step> {
    // What the teeth are, for drawing them as formulas (gear 2 and the planets are member 2).
    p.teeth_source(crate::equations::TeethSource {
        who: who.into(),
        member: if matches!(who, "gear 2" | "planet") { 2 } else { 1 },
        form: crate::equations::Form::External { g: *g, f: f.clone() },
        backlash: s.backlash,
        face,
        face_key: if (face - s.face_width).abs() <= 1e-12 { "face_width".into() } else { "face_width2".into() },
        phases: Vec::new(),
        extra: Default::default(),
    });
    let outline = teeth_outline(f, g.teeth, phase);
    let twist_total = face * g.helix.tan() / g.r();
    if s.crowning > 0.0 {
        if twist_total != 0.0 || herringbone {
            p.warn(format!("{who}: crowning is only built on straight teeth; left out"));
        } else {
            // No more than leaves most of the tip land (and the root arcs stay root arcs).
            let most = if f.tip_half > 1e-9 { 0.8 * f.tip_half } else { 0.0 };
            let d = (s.crowning / g.rb()).min(most);
            let end = teeth_outline(&f.thinned(d), g.teeth, phase);
            if d > 0.0 && same_pieces(&end, &outline) {
                if d < 0.999 * s.crowning / g.rb() {
                    p.warn(format!("{who}: the tips are too thin for {} mm of crowning; {} mm is used", num(s.crowning), num(d * g.rb())));
                }
                if s.elephant_foot > 0.0 {
                    p.warn(format!("{who}: elephant-foot relief is left out on crowned teeth"));
                }
                return vec![Step::Loft { sections: vec![(end.clone(), 0.0), (outline, face / 2.0), (end, face)], op: Op::New }];
            }
            p.warn(format!("{who}: the teeth are too thin to crown; left out"));
        }
    }
    let full = Region { outer: vec![outline], holes: vec![] };
    let mut foot: Option<(Region, f64)> = None;
    if s.elephant_foot > 0.0 {
        let e = s.elephant_foot;
        let mut inner = *g;
        inner.backlash += 2.0 * e / g.alpha_t().cos();
        inner.tip_shortening += e;
        inner.rack.dedendum += e / g.module;
        let mut fi = external_flank(&inner);
        let h = (if s.elephant_foot_height > 0.0 { s.elephant_foot_height } else { 0.2 }).min(0.25 * face);
        if fi.defect().is_some() || fi.pointed != f.pointed {
            p.warn(format!("{who}: the teeth are too thin for the elephant-foot relief; left out"));
        } else {
            let mut quiet = Plan::default();
            shape(s, &inner, &mut fi, &mut quiet, who);
            foot = Some((Region { outer: vec![teeth_outline(&fi, g.teeth, phase)], holes: vec![] }, h));
        }
    }
    // The tooth's angle at height z, and the heights where the stack changes.
    let half = face / 2.0;
    let angle = |z: f64| -> f64 { if herringbone { twist_total / 2.0 * (1.0 - (z - half).abs() / half) } else { twist_total * z / face } };
    let mut breaks = vec![0.0];
    if let Some((_, h)) = &foot {
        breaks.push(*h);
    }
    if herringbone {
        breaks.push(half);
    }
    breaks.push(face);
    let mut steps = Vec::new();
    for w in breaks.windows(2) {
        let (za, zb) = (w[0], w[1]);
        let region = match &foot {
            Some((r, h)) if zb <= *h * (1.0 + 1e-12) => r,
            _ => &full,
        };
        steps.push(Step::Prism {
            region: turned(region, angle(za)),
            z0: za,
            height: zb - za,
            twist: angle(zb) - angle(za),
            op: if steps.is_empty() { Op::New } else { Op::Join },
        });
    }
    steps
}

// ---------------------------------------------------------------------------------------------
// Shapes.

fn disc(r: f64) -> Region {
    Region { outer: vec![Outline::circle(V2::ZERO, r)], holes: vec![] }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Outline {
    let p = [V2::new(x0, y0), V2::new(x1, y0), V2::new(x1, y1), V2::new(x0, y1)];
    Outline { segs: (0..4).map(|i| Seg::Line(p[i], p[(i + 1) % 4])).collect() }
}

fn cut(region: Region, z0: f64, height: f64) -> Step {
    Step::Prism { region, z0, height, twist: 0.0, op: Op::Cut }
}

/// The triangle (in the radius–z half plane) whose revolution chamfers the edge at radius `r`
/// on the plane z = `z`: `c` wide across the face and `d` deep along the cylinder. `dr` and `dz`
/// (each ±1) point from the edge into the material, across the face and along the cylinder. The
/// triangle overlaps the edge (its corner is out in the air), so nothing of it lies on a face.
pub(crate) fn corner_cut(r: f64, z: f64, dr: f64, dz: f64, c: f64, d: f64) -> Vec<V2> {
    let a = V2::new(r + dr * c, z);
    let b = V2::new(r, z + dz * d);
    let u = (b - a).norm();
    let mut e = 0.1 * c.min(d);
    if dr > 0.0 {
        // The air is towards the axis: stay on this side of it.
        e = e.min(0.5 * r * (b - a).len() / c);
    }
    let (a2, b2) = (a - u * e, b + u * e);
    vec![a2, V2::new(b2.x, a2.y), b2]
}

fn revolve_cut(profile: Vec<V2>) -> Step {
    Step::Revolve { profile, op: Op::Cut }
}

/// A step moved `dz` along its body's axis.
pub(crate) fn shift_z(step: Step, dz: f64) -> Step {
    match step {
        Step::Prism { region, z0, height, twist, op } => Step::Prism { region, z0: z0 + dz, height, twist, op },
        Step::Loft { sections, op } => Step::Loft { sections: sections.into_iter().map(|(o, h)| (o, h + dz)).collect(), op },
        Step::Taper { outline, z0, height, twist, scale, op } => Step::Taper { outline, z0: z0 + dz, height, twist, scale, op },
        Step::RingTaper { outline, rim, z0, height, twist, scale, op } => Step::RingTaper { outline, rim, z0: z0 + dz, height, twist, scale, op },
        Step::Revolve { profile, op } => Step::Revolve { profile: profile.into_iter().map(|q| V2::new(q.x, q.y + dz)).collect(), op },
        Step::Coil { section, pitch, z0, height, left, op } => {
            Step::Coil { section: section.into_iter().map(|q| V2::new(q.x, q.y + dz)).collect(), pitch, z0: z0 + dz, height, left, op }
        }
        Step::Placed { mut frame, step } => {
            frame.origin[2] += dz;
            Step::Placed { frame, step }
        }
        Step::Group { steps, op } => Step::Group { steps: steps.into_iter().map(|st| shift_z(st, dz)).collect(), op },
    }
}

/// The largest radius of the toothed steps built so far (a gear's tip radius).
pub(crate) fn tip_radius(steps: &[Step]) -> f64 {
    let mut r: f64 = 0.0;
    for st in steps {
        match st {
            Step::Prism { region, op: Op::New | Op::Join, .. } => {
                for o in &region.outer {
                    r = r.max(o.max_radius());
                }
            }
            Step::Loft { sections, .. } => {
                for (o, _) in sections {
                    r = r.max(o.max_radius());
                }
            }
            _ => {}
        }
    }
    r
}

// ---------------------------------------------------------------------------------------------
// Shafts.

/// One gear of a set, as the hub, shaft and hardware steps see it: its body runs from z = 0 to
/// `face`.
pub(crate) struct Member<'a> {
    pub who: &'a str,
    /// The set's second member (mate, planet): `shaft2_*`, `hardware2`.
    pub second: bool,
    pub bore: &'a Bore,
    pub root: f64,
    /// Tip radius (0: not known; no tooth chamfers).
    pub tip: f64,
    pub face: f64,
}

/// The shafts a member really gets, and how far its bore runs.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Built {
    pub front: Option<Shaft>,
    pub back: Option<Shaft>,
    /// The bore's ends (the outermost faces along the axis).
    pub lo: f64,
    pub hi: f64,
}

/// Whether the shaft is built: there is one and the bore leaves a wall. Warns otherwise, and
/// when it is hollow or bigger than `limit` (the root circle).
fn usable(sh: &Shaft, bore: &Bore, limit: f64, p: &mut Plan, who: &str, side: &str) -> Option<Shaft> {
    if !sh.exists() {
        return None;
    }
    if bore.outline().is_some() {
        let reach = bore.reach();
        if reach >= 0.9 * sh.min_diameter() / 2.0 {
            p.warn(format!(
                "{who}: the bore (reaching Ø{}) leaves no wall in the {side} shaft (Ø{}); the shaft is left out",
                num(2.0 * reach),
                num(sh.min_diameter())
            ));
            return None;
        }
        p.warn(format!("{who}: the bore runs through the {side} shaft (a hollow shaft)"));
    }
    if sh.max_diameter() / 2.0 > limit {
        p.warn(format!(
            "{who}: the {side} shaft (Ø{}) is bigger than the root circle (Ø{}) and buries the ends of the teeth",
            num(sh.max_diameter()),
            num(2.0 * limit)
        ));
    }
    Some(*sh)
}

/// The shaft's cylinders, from the face at z = `base` along `dir` (±1).
fn shaft_solid(sh: &Shaft, base: f64, dir: f64, steps: &mut Vec<Step>) {
    let mut at = base;
    let mut lengths = vec![(sh.diameter, sh.length)];
    if sh.stepped() {
        lengths.push((sh.step_diameter, sh.step_length));
    }
    for (d, l) in lengths {
        let z0 = if dir > 0.0 { at } else { at - l };
        steps.push(Step::Prism { region: disc(d / 2.0), z0, height: l, twist: 0.0, op: Op::Join });
        at += dir * l;
    }
}

/// The shaft's end chamfer and end feature. `buried`: how much of the shaft's length, from the
/// gear's face, is inside something else (the hub); `reach`: the bore's reach when the shaft is
/// hollow.
#[allow(clippy::too_many_arguments)]
fn shaft_end(sh: &Shaft, base: f64, dir: f64, buried: f64, reach: f64, steps: &mut Vec<Step>, p: &mut Plan, who: &str, side: &str) {
    let total = sh.total();
    let tip = base + dir * total;
    let d = sh.end_diameter();
    let r = d / 2.0;
    // The length the end's diameter runs for, clear of the hub.
    let own = if sh.stepped() { sh.step_length } else { sh.length };
    let free = own.min(total - buried);
    if !(free > 0.0) {
        if sh.end != ShaftEnd::Plain || sh.chamfer > 0.0 {
            p.warn(format!("{who}: the {side} shaft doesn't reach out of the hub; its end is left plain"));
        }
        return;
    }
    if sh.chamfer > 0.0 {
        let c = sh.chamfer.min(0.8 * free).min(0.8 * (r - reach));
        if c > 1e-6 {
            steps.push(revolve_cut(corner_cut(r, tip, -1.0, -dir, c, c)));
        }
    }
    let span = |len: f64| -> (f64, f64) { if dir > 0.0 { (tip - len, len + OVER) } else { (tip - OVER, len + OVER) } };
    let len = (if sh.end_length > 0.0 { sh.end_length } else { 1.5 * d }).min(free);
    let big = 1.2 * r + 0.1;
    let left_out = |p: &mut Plan, why: &str| p.warn(format!("{who}: the {side} shaft's end: {why}; left plain"));
    match sh.end {
        ShaftEnd::Plain => {}
        ShaftEnd::DFlat => {
            let across = if sh.end_size > 0.0 { sh.end_size } else { 0.9 * d };
            if across >= 0.999 * d || across <= 0.5 * d || across - r <= reach {
                left_out(p, &format!("a flat {} across doesn't fit a Ø{} shaft (over half the diameter, under all of it)", num(across), num(d)));
            } else {
                let (z0, h) = span(len);
                steps.push(cut(Region { outer: vec![rect(-big, across - r, big, big)], holes: vec![] }, z0, h));
            }
        }
        ShaftEnd::Keyway => {
            let w = if sh.end_size > 0.0 { sh.end_size } else { din6885(d).map_or(d / 4.0, |k| k.0) };
            let t = (0.6 * w).min(0.3 * d);
            if w >= 0.7 * d || r - t <= reach {
                left_out(p, &format!("a key slot {} wide doesn't fit a Ø{} shaft", num(w), num(d)));
            } else {
                let (z0, h) = span(len);
                steps.push(cut(Region { outer: vec![rect(-w / 2.0, r - t, w / 2.0, big)], holes: vec![] }, z0, h));
            }
        }
        ShaftEnd::Hex => {
            let af = if sh.end_size > 0.0 { sh.end_size } else { 0.85 * d };
            let hex = Bore { kind: BoreKind::Hex, diameter: af, ..Bore::default() };
            match hex.outline() {
                Some(o) if af < 0.999 * d && af / 2.0 > 1.05 * reach && af > 0.3 * d => {
                    let corner = af / 2.0 / (PI / 6.0).cos();
                    let (z0, h) = span(len);
                    steps.push(cut(Region { outer: vec![Outline::circle(V2::ZERO, 1.2 * r.max(corner) + 0.1)], holes: vec![o] }, z0, h));
                }
                _ => left_out(p, &format!("a hexagon {} across flats doesn't fit a Ø{} shaft", num(af), num(d))),
            }
        }
        ShaftEnd::Cross => {
            let w = if sh.end_size > 0.0 { sh.end_size } else { d / 3.0 };
            if w >= 0.9 * d || w <= 0.05 * d || reach > 0.0 {
                left_out(
                    p,
                    &format!("a cross with arms {} wide doesn't fit a Ø{} shaft{}", num(w), num(d), if reach > 0.0 { " with a bore" } else { "" }),
                );
            } else {
                let (a, b) = (w / 2.0, big);
                let corners = vec![rect(a, a, b, b), rect(-b, a, -a, b), rect(-b, -b, -a, -a), rect(a, -b, b, -a)];
                let (z0, h) = span(len);
                steps.push(cut(Region { outer: corners, holes: vec![] }, z0, h));
            }
        }
        ShaftEnd::CentreHole => {
            let dc = if sh.end_size > 0.0 { sh.end_size } else { 0.3 * d };
            let depth = (if sh.end_length > 0.0 { sh.end_length } else { 1.5 * dc }).min(total);
            if dc >= 0.8 * d || reach > 0.0 {
                left_out(p, &format!("a Ø{} centre hole doesn't fit a Ø{} shaft{}", num(dc), num(d), if reach > 0.0 { " with a bore" } else { "" }));
            } else {
                let (z0, h) = span(depth);
                steps.push(cut(disc(dc / 2.0), z0, h));
            }
        }
    }
}

/// Shafts on a body that is not a plain cylindrical gear (a bevel, a worm): the back shaft from
/// z = `z_back` along −z, the front one from z = `z_front` along +z, each warned about when
/// bigger than its `limit`. Returns what was built; [`shaft_ends`] finishes them after the bore
/// is cut.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shafts(
    s: &Spec,
    second: bool,
    bore: &Bore,
    z_back: f64,
    limit_back: f64,
    z_front: f64,
    limit_front: f64,
    steps: &mut Vec<Step>,
    p: &mut Plan,
    who: &str,
) -> Built {
    let front = usable(s.shaft(second, true), bore, limit_front, p, who, "front");
    let back = usable(s.shaft(second, false), bore, limit_back, p, who, "back");
    if let Some(sh) = &back {
        shaft_solid(sh, z_back, -1.0, steps);
    }
    if let Some(sh) = &front {
        shaft_solid(sh, z_front, 1.0, steps);
    }
    Built { front, back, lo: z_back - back.map_or(0.0, |sh| sh.total()), hi: z_front + front.map_or(0.0, |sh| sh.total()) }
}

/// The ends of the shafts [`shafts`] built (`buried_*`: the hub's length over each).
#[allow(clippy::too_many_arguments)]
pub(crate) fn shaft_ends(
    b: &Built,
    bore: &Bore,
    z_back: f64,
    buried_back: f64,
    z_front: f64,
    buried_front: f64,
    steps: &mut Vec<Step>,
    p: &mut Plan,
    who: &str,
) {
    let reach = if bore.outline().is_some() { bore.reach() } else { 0.0 };
    if let Some(sh) = &b.back {
        shaft_end(sh, z_back, -1.0, buried_back, reach, steps, p, who, "back");
    }
    if let Some(sh) = &b.front {
        shaft_end(sh, z_front, 1.0, buried_front, reach, steps, p, who, "front");
    }
}

// ---------------------------------------------------------------------------------------------
// A cylindrical gear's extras, in the order `plan::hub_steps` builds them.

fn hub_of(s: &Spec) -> Option<(f64, f64)> {
    (s.hub_diameter > 0.0 && s.hub_length > 0.0).then_some((s.hub_diameter / 2.0, s.hub_length))
}

/// The bearing pocket's radius and depth, if the member has one that fits.
pub(crate) fn pocket(s: &Spec, m: &Member) -> Option<(f64, f64)> {
    let h = s.hardware_of(m.second);
    if !(h.bearing_od > 0.0 && h.bearing_width > 0.0) || s.shaft(m.second, false).exists() {
        return None;
    }
    let r = (h.bearing_od + h.bearing_fit) / 2.0;
    let deep = m.face + hub_of(s).map_or(0.0, |(_, l)| l);
    (r < m.root - s.module && h.bearing_width < deep - 0.2 * s.module.min(deep)).then_some((r, h.bearing_width))
}

/// After the hub: the shafts' cylinders, then the chamfers on the teeth and the hub.
pub(crate) fn add_solids(s: &Spec, m: &Member, steps: &mut Vec<Step>, p: &mut Plan) -> Built {
    let who = m.who;
    let hub = hub_of(s);
    let reach = if m.bore.outline().is_some() { m.bore.reach() } else { 0.0 };
    let mut b = shafts(s, m.second, m.bore, 0.0, m.root, m.face, m.root, steps, p, who);
    b.hi = b.hi.max(m.face + hub.map_or(0.0, |(_, l)| l));
    if let (Some(sh), Some((hr, hl))) = (&b.front, hub)
        && sh.total() <= hl
        && sh.max_diameter() / 2.0 <= hr
    {
        p.warn(format!("{who}: the front shaft is hidden inside the hub (shafts are measured from the gear's face)"));
    }
    // Tooth chamfers: a ring off the tip circle's edge on each face.
    let front_inner = reach.max(hub.map_or(0.0, |(r, _)| r)).max(b.front.map_or(0.0, |sh| sh.diameter / 2.0));
    let back_inner = reach.max(b.back.map_or(0.0, |sh| sh.diameter / 2.0)).max(pocket(s, m).map_or(0.0, |(r, _)| r));
    let mut depth_left = 0.9 * m.face;
    for (c, z, dz, inner, side) in
        [(&s.tooth_chamfer_back, 0.0, 1.0, back_inner, "back"), (&s.tooth_chamfer_front, m.face, -1.0, front_inner, "front")]
    {
        if !(c.size > 0.0) {
            continue;
        }
        if !(m.tip > 0.0) {
            continue;
        }
        let size = c.size.min(0.8 * (m.tip - inner));
        let depth = (size * c.angle.to_radians().tan()).min(0.5 * depth_left);
        if !(size > 1e-6 && depth > 1e-6) {
            p.warn(format!("{who}: no room for the {side} tooth chamfer; left out"));
            continue;
        }
        if size < c.size || depth < 0.999 * size * c.angle.to_radians().tan() {
            p.warn(format!("{who}: the {side} tooth chamfer is cut down to {} × {} mm to fit", num(size), num(depth)));
        }
        depth_left -= depth;
        steps.push(revolve_cut(corner_cut(m.tip, z, -1.0, dz, size, depth)));
    }
    if s.hub_chamfer > 0.0
        && let Some((hr, hl)) = hub
    {
        let covered = b.front.is_some_and(|sh| sh.total() > hl);
        let inner = reach.max(if covered { b.front.map_or(0.0, |sh| sh.diameter / 2.0) } else { 0.0 });
        let c = s.hub_chamfer.min(0.8 * hl).min(0.8 * (hr - inner));
        if c > 1e-6 {
            steps.push(revolve_cut(corner_cut(hr, m.face + hl, -1.0, -1.0, c, c)));
        } else {
            p.warn(format!("{who}: no room for the hub chamfer; left out"));
        }
    }
    b
}

/// Before the bore: the bearing pocket in the back face.
pub(crate) fn pockets(s: &Spec, m: &Member, steps: &mut Vec<Step>, p: &mut Plan) {
    let h = s.hardware_of(m.second);
    if !(h.bearing_od > 0.0 && h.bearing_width > 0.0) {
        return;
    }
    let who = m.who;
    match pocket(s, m) {
        Some((r, w)) => {
            if m.bore.outline().is_some() && m.bore.reach() >= r {
                p.warn(format!("{who}: the bore is as big as the bearing pocket (Ø{})", num(2.0 * r)));
            }
            steps.push(cut(disc(r), -OVER, w + OVER));
        }
        None if s.shaft(m.second, false).exists() => p.warn(format!("{who}: the bearing pocket is where the back shaft is; left out")),
        None => p.warn(format!(
            "{who}: a Ø{} × {} bearing pocket doesn't fit inside the root circle (Ø{}, less a module of wall) and the gear's depth; left out",
            num(h.bearing_od + h.bearing_fit),
            num(h.bearing_width),
            num(2.0 * m.root)
        )),
    }
}

/// A frame (in the body's) with z pointing out from the axis at angle `a` in the plane z = `zc`,
/// and y along the body's +z.
fn radial_frame(a: f64, zc: f64) -> Frame {
    let (sn, cs) = a.sin_cos();
    Frame { origin: [0.0, 0.0, zc], x: [-sn, cs, 0.0], z: [cs, sn, 0.0] }
}

fn placed(frame: Frame, step: Step) -> Step {
    Step::Placed { frame, step: Box::new(step) }
}

/// After the bore: its chamfers, the shafts' ends, and the hub hardware.
pub(crate) fn add_details(s: &Spec, m: &Member, b: &Built, steps: &mut Vec<Step>, p: &mut Plan) {
    let who = m.who;
    let hub = hub_of(s);
    let bored = m.bore.outline().is_some();
    let reach = if bored { m.bore.reach() } else { 0.0 };
    if s.bore_chamfer > 0.0 && bored {
        let rb = m.bore.diameter / 2.0;
        // What the bore comes out of at each end.
        let low = if let Some(sh) = &b.back {
            Some(sh.end_diameter() / 2.0)
        } else if pocket(s, m).is_some() {
            None
        } else {
            Some(m.root)
        };
        let front_tip = b.front.map_or(0.0, |sh| m.face + sh.total());
        let high = match (&b.front, hub) {
            (Some(sh), _) if front_tip >= b.hi - 1e-9 => sh.end_diameter() / 2.0,
            (_, Some((hr, _))) => hr,
            _ => m.root,
        };
        for (limit, z, dz) in [(low, b.lo, 1.0), (Some(high), b.hi, -1.0)] {
            let Some(limit) = limit else { continue };
            let c = s.bore_chamfer.min(0.8 * (limit - reach)).min(0.4 * (b.hi - b.lo));
            if c > 1e-6 && rb > 0.0 {
                steps.push(revolve_cut(corner_cut(rb, z, 1.0, dz, c, c)));
            }
        }
    }
    shaft_ends(b, m.bore, 0.0, 0.0, m.face, hub.map_or(0.0, |(_, l)| l), steps, p, who);
    let h = s.hardware_of(m.second);
    let wants_hub = h.set_screw > 0.0 || h.nut_width > 0.0 || h.insert_diameter > 0.0 || h.clamp_slit > 0.0 || h.clamp_screw > 0.0;
    if !wants_hub {
        return;
    }
    let Some((hr, hl)) = hub else {
        p.warn(format!("{who}: set screws, nut traps, insert pockets and clamp slits go in a hub (set its diameter and length); left out"));
        return;
    };
    let wall = hr - reach;
    if !(wall > 0.0) {
        p.warn(format!("{who}: the hub has no wall for its hardware; left out"));
        return;
    }
    let zc = m.face + hl / 2.0;
    let count = h.set_screws.clamp(1, 4);
    let frames: Vec<Frame> = (0..count).map(|k| radial_frame(FRAC_PI_2 + FRAC_PI_2 * f64::from(k), zc)).collect();
    if h.set_screw > 0.0 {
        if h.set_screw >= 0.9 * hl {
            p.warn(format!("{who}: a Ø{} set screw doesn't fit a hub {} long; left out", num(h.set_screw), num(hl)));
        } else {
            for f in &frames {
                steps.push(placed(*f, cut(disc(h.set_screw / 2.0), 0.0, hr + OVER)));
            }
        }
    }
    if h.insert_diameter > 0.0 {
        let depth = (if h.insert_depth > 0.0 { h.insert_depth } else { 0.8 * wall }).min(wall);
        if h.insert_diameter >= 0.95 * hl {
            p.warn(format!("{who}: a Ø{} insert pocket doesn't fit a hub {} long; left out", num(h.insert_diameter), num(hl)));
        } else {
            for f in &frames {
                steps.push(placed(*f, cut(disc(h.insert_diameter / 2.0), hr - depth, depth + OVER)));
            }
        }
    }
    if h.nut_width > 0.0 {
        let (w, t) = (h.nut_width, if h.nut_thickness > 0.0 { h.nut_thickness } else { 0.45 * h.nut_width });
        if t >= 0.7 * wall {
            p.warn(format!("{who}: a nut {} thick doesn't fit the hub's {} wall; the nut trap is left out", num(t), num(wall)));
        } else {
            // The slot the nut drops into from the hub's end, across the set screw's hole.
            let mid = reach + wall / 2.0;
            for f in &frames {
                steps.push(placed(
                    *f,
                    cut(Region { outer: vec![rect(-w / 2.0, -(w / 2.0).min(0.45 * hl), w / 2.0, hl / 2.0 + OVER)], holes: vec![] }, mid - t / 2.0, t),
                ));
            }
        }
    }
    if h.clamp_slit > 0.0 {
        if !bored || h.clamp_slit / 2.0 >= 0.9 * m.bore.diameter / 2.0 {
            p.warn(format!("{who}: a clamp slit needs a bore wider than the slit; left out"));
        } else {
            // Through the wall on the side away from the first set screw, down to the gear's face.
            steps.push(cut(
                Region { outer: vec![rect(-h.clamp_slit / 2.0, -(hr + OVER), h.clamp_slit / 2.0, 0.0)], holes: vec![] },
                m.face,
                hl + OVER,
            ));
            if h.clamp_screw > 0.0 {
                if h.clamp_screw >= 0.8 * hl.min(wall) {
                    p.warn(format!("{who}: a Ø{} clamp screw doesn't fit the hub's wall; left out", num(h.clamp_screw)));
                } else {
                    let frame = Frame { origin: [-(hr + OVER), -(reach + wall / 2.0), zc], x: [0.0, 1.0, 0.0], z: [1.0, 0.0, 0.0] };
                    steps.push(placed(frame, cut(disc(h.clamp_screw / 2.0), 0.0, 2.0 * (hr + OVER))));
                }
            }
        }
    } else if h.clamp_screw > 0.0 {
        p.warn(format!("{who}: a clamp screw needs a clamp slit; left out"));
    }
}

/// Tooth chamfers on an internal gear: a ring off the edge of the inner tip circle (radius
/// `tip`) on each face; `outer` is the rim's outside radius.
pub(crate) fn ring_chamfers(s: &Spec, tip: f64, outer: f64, face: f64, steps: &mut Vec<Step>, p: &mut Plan, who: &str) {
    let mut depth_left = 0.9 * face;
    for (c, z, dz, side) in [(&s.tooth_chamfer_back, 0.0, 1.0, "back"), (&s.tooth_chamfer_front, face, -1.0, "front")] {
        if !(c.size > 0.0) {
            continue;
        }
        let size = c.size.min(0.8 * (outer - tip));
        let depth = (size * c.angle.to_radians().tan()).min(0.5 * depth_left);
        if !(size > 1e-6 && depth > 1e-6 && tip > 0.0) {
            p.warn(format!("{who}: no room for the {side} tooth chamfer; left out"));
            continue;
        }
        depth_left -= depth;
        steps.push(revolve_cut(corner_cut(tip, z, 1.0, dz, size, depth)));
    }
}
