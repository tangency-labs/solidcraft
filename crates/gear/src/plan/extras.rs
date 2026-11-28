//! Plans for chain sprockets, timing belt pulleys, involute splines and face gears (kept apart
//! from the gear types in `plan.rs`; they share its helpers).

use std::f64::consts::{PI, TAU};

use super::{Body, Frame, Op, Plan, PreviewGear, Spec, Step, Unit, hub_steps, hub_steps_tip, name, tooth_warnings, wanted, with_bore};
use crate::axes::num;
use crate::face_gear::FaceGear;
use crate::geom::{Outline, Region, V2};
use crate::involute::external_outline;
use crate::pulley::{BELTS, Belt, Pulley, belt_by_id};
use crate::spline::{Spline, SplineStandard};
use crate::sprocket::{Sprocket, chain_by_id, chamfer_profile, tooth_width};
use crate::wrap::{belt_length, belt_path, chain_center, chain_links, wrap_angle};

/// A spec whose module stands in for a pitch (`hub_steps` sizes its margins by the module).
fn with_module(s: &Spec, module: f64) -> Spec {
    Spec { module, ..s.clone() }
}

/// The chain the spec names: (label, pitch, roller Ø, inner width, transverse pitch).
pub(crate) fn chain_of(s: &Spec) -> (String, f64, f64, f64, f64) {
    match chain_by_id(s.chain) {
        Some(c) => (c.label.to_string(), c.pitch, c.roller, c.inner_width, c.strand_pitch),
        None => (format!("custom {} mm pitch", num(s.chain_pitch)), s.chain_pitch, s.roller_diameter, s.roller_width, s.strand_pitch),
    }
}

pub(crate) fn belt_of(s: &Spec) -> Belt {
    belt_by_id(s.belt).or(BELTS.first()).copied().unwrap_or(Belt {
        id: "gt2_2",
        label: "GT2 2 mm",
        pitch: 2.0,
        pld: 0.254,
        groove: crate::pulley::Groove::Curvilinear { depth: 0.75, tip_r: 0.555, flank_r: 1.0, fillet_r: 0.15 },
        widths: &[6.0],
    })
}

/// The nearest point of an outline to its centre (an elliptical gear's root at the minor axis).
fn min_radius(o: &Outline) -> f64 {
    o.polyline(0.01).iter().map(|q| q.len()).fold(f64::INFINITY, f64::min)
}

/// Validates the fields only these types use.
pub(super) fn check(s: &Spec) -> Result<(), String> {
    use super::Kind;
    match s.kind {
        Kind::Sprocket => {
            if chain_by_id(s.chain).is_none() {
                if s.chain != crate::sprocket::CUSTOM {
                    return Err(format!("unknown chain `{}`", s.chain));
                }
                for (v, what) in [(s.chain_pitch, "chain pitch"), (s.roller_diameter, "roller diameter"), (s.roller_width, "chain inner width")] {
                    if !(v.is_finite() && v > 0.0) {
                        return Err(format!("{what} must be greater than 0"));
                    }
                }
                if s.roller_diameter >= s.chain_pitch {
                    return Err("the roller diameter must be smaller than the pitch".into());
                }
                if s.strands > 1 && s.strand_pitch <= s.roller_width {
                    return Err("the strand pitch must be larger than the chain's inner width".into());
                }
            }
            if !(1..=4).contains(&s.strands) {
                return Err("strands must be between 1 and 4".into());
            }
            if s.teeth < 6 || s.teeth > 250 || (s.teeth2 != 0 && !(6..=250).contains(&s.teeth2)) {
                return Err("sprocket teeth must be between 6 and 250".into());
            }
        }
        Kind::Pulley => {
            if belt_by_id(s.belt).is_none() {
                return Err(format!("unknown belt `{}`", s.belt));
            }
            if s.teeth < 6 || s.teeth > 400 || (s.teeth2 != 0 && !(6..=400).contains(&s.teeth2)) {
                return Err("pulley teeth must be between 6 and 400".into());
            }
        }
        Kind::Spline => {
            if s.teeth < 6 {
                return Err("a spline needs at least 6 teeth".into());
            }
            if !(s.spline_diameter.is_finite() && s.spline_diameter >= 0.0) {
                return Err("the reference diameter must not be negative".into());
            }
            if !(20.0..=45.0).contains(&s.pressure_angle) {
                return Err("a spline's pressure angle must be between 20° and 45° (30° is standard; ANSI B92.1 also has 37.5° and 45°)".into());
            }
            if s.backlash >= PI * s.module / 8.0 {
                return Err(format!("the clearance must be under an eighth of the pitch ({} mm)", num(PI * s.module / 8.0)));
            }
            let sp = spline_of(s);
            let x = sp.shift();
            if !(-0.5..=1.0).contains(&x) {
                return Err(format!(
                    "a reference diameter of {} mm puts the profile shift at {}: for {} teeth of module {} use {} to {} mm",
                    num(s.spline_diameter),
                    num(x),
                    s.teeth,
                    num(s.module),
                    num(s.module * (f64::from(s.teeth) + 1.0)),
                    num(s.module * (f64::from(s.teeth) + 2.0))
                ));
            }
            // The cutter that makes the shaft's teeth must have a tip.
            let rack = sp.rack();
            if PI / 4.0 - rack.dedendum * sp.pressure_angle.tan() < 0.01 {
                return Err(format!(
                    "at {}° the shaft's {} root is too deep for its cutter to have a tip: use a {}smaller pressure angle",
                    num(s.pressure_angle),
                    if s.fillet_root { "fillet" } else { "flat" },
                    if s.fillet_root { "flat root or a " } else { "" }
                ));
            }
        }
        Kind::Elliptical => {
            if !(0.0..=0.7).contains(&s.ellipse_eccentricity) {
                return Err("the ellipse's eccentricity must be between 0 and 0.7".into());
            }
            if !(8..=200).contains(&s.teeth) {
                return Err("an elliptical gear needs 8 to 200 teeth".into());
            }
        }
        Kind::FaceGear if s.teeth2 < s.teeth + 2 => {
            return Err("the face gear needs more teeth than the pinion (at least 2 more)".into());
        }
        _ => {}
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------
// Chain sprockets

pub(super) fn sprocket(s: &Spec, p: &mut Plan) {
    let (label, pitch, roller, b1, pt) = chain_of(s);
    let strands = s.strands.clamp(1, 4);
    let bf1 = tooth_width(pitch, b1, strands);
    let width = f64::from(strands - 1) * pt + bf1;
    let sm = with_module(s, pitch / PI);
    p.preview_note = format!("{label} chain, {} strand{}", strands, if strands > 1 { "s" } else { "" });
    p.fact("chain pitch", pitch, Unit::Mm);
    p.fact("roller diameter", roller, Unit::Mm);
    p.fact("tooth width (one strand)", bf1, Unit::Mm);
    if strands > 1 {
        p.fact("transverse (strand) pitch", pt, Unit::Mm);
    }
    p.fact("width over the teeth", width, Unit::Mm);
    let (z1, z2) = (s.teeth, s.teeth2);
    let build = |z: u32, who: &str, bore: &crate::hub::Bore, phase: f64, p: &mut Plan| -> (Vec<Step>, Outline, f64) {
        let sp = Sprocket { teeth: z, pitch, roller };
        let t = sp.outline(phase, sp.tip_diameter() / 2.0);
        let pre = if z2 == 0 { String::new() } else { format!("{who} ") };
        let (lo, hi) = sp.tip_diameter_range();
        p.fact(format!("{pre}pitch diameter"), sp.pitch_diameter(), Unit::Mm);
        p.fact(format!("{pre}tip diameter (middle of {}…{})", num(lo), num(hi)), 2.0 * t.ra, Unit::Mm);
        p.fact(format!("{pre}root diameter"), sp.root_diameter(), Unit::Mm);
        let how = if z.is_multiple_of(2) { "over opposite roots" } else { "odd teeth: over the nearest opposite roots" };
        p.fact(format!("{pre}caliper diameter ({how})"), sp.caliper_diameter(), Unit::Mm);
        p.fact(format!("{pre}roller seating radius"), sp.seating_radius(), Unit::Mm);
        p.fact(format!("{pre}roller seating angle"), sp.seating_angle().to_degrees(), Unit::Deg);
        p.fact(format!("{pre}tooth flank radius"), sp.flank_radius(), Unit::Mm);
        p.fact(format!("{pre}largest hub (shroud) diameter"), sp.shroud_diameter(), Unit::Mm);
        if t.pointed {
            p.warn(format!("{who}: the teeth come to a point; the tip is lowered to Ø{}", num(2.0 * t.ra)));
        }
        if z < 9 {
            p.warn(format!("{who}: under 9 teeth the chain runs roughly (chordal action); 17 or more is usual"));
        } else if z > 150 {
            p.warn(format!("{who}: over 150 teeth a worn chain rides up the teeth"));
        }
        let dg = sp.shroud_diameter().min(2.0 * t.r_min - 0.1 * pitch);
        if s.hub_diameter > dg && s.hub_length > 0.0 {
            p.warn(format!("{who}: a hub over Ø{} fouls the chain's plates", num(dg)));
        }
        // One toothed disc a strand, joined by a core that clears the chain's plates.
        let teeth = Region { outer: vec![t.outline.clone()], holes: vec![] };
        let gap = pt - bf1;
        let core = (dg / 2.0).max(0.35 * t.r_min);
        if strands > 1 && gap > 1e-6 && core > dg / 2.0 + 1e-9 {
            p.warn(format!("{who}: too few teeth to leave a core between the strands that clears the chain's plates"));
        }
        let mut steps = vec![Step::Prism { region: teeth.clone(), z0: 0.0, height: if gap > 1e-6 { bf1 } else { width }, twist: 0.0, op: Op::New }];
        if gap > 1e-6 {
            for i in 1..strands {
                let z0 = f64::from(i) * pt;
                let disc = Region { outer: vec![Outline::circle(V2::ZERO, core)], holes: vec![] };
                steps.push(Step::Prism { region: disc, z0: z0 - gap, height: gap, twist: 0.0, op: Op::Join });
                steps.push(Step::Prism { region: teeth.clone(), z0, height: bf1, twist: 0.0, op: Op::Join });
            }
        }
        if s.side_chamfer {
            steps.push(Step::Revolve { profile: chamfer_profile(pitch, t.ra, bf1, strands, pt, core), op: Op::Intersect });
        }
        // (`sm` is a copy: hand over its own bore, so gear 2 still gets gear 2's shafts and hardware.)
        let bore = if std::ptr::eq(bore, &s.bore2) { &sm.bore2 } else { &sm.bore };
        hub_steps(&sm, bore, t.r_min, width, &mut steps, p, who);
        (steps, t.outline, sp.pitch_diameter() / 2.0)
    };
    let (st1, o1, r1) = build(z1, "sprocket 1", &s.bore, 0.0, p);
    if z2 == 0 {
        if wanted(s, 1) {
            p.bodies.push(Body { name: name(s, &format!("Sprocket {label} z{z1}")), frame: Frame::at(s.origin), steps: st1 });
        }
        // A strand of chain lying on the upper half, turning with the sprocket.
        let rollers: Vec<Outline> = (0..z1)
            .map(|k| TAU * f64::from(k) / f64::from(z1))
            .filter(|a| a.sin() > 0.05)
            .map(|a| Outline::circle(V2::polar(r1, a), roller / 2.0))
            .collect();
        p.preview.push(PreviewGear::turning("sprocket", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 1.0, r1));
        if !rollers.is_empty() {
            p.preview.push(PreviewGear::turning("chain", Region { outer: rollers, holes: vec![] }, V2::ZERO, 1.0, 0.0));
        }
        return;
    }
    // A pair joined by the chain.
    let r2 = Sprocket { teeth: z2, pitch, roller }.pitch_diameter() / 2.0;
    let a_in = if s.center_distance > 0.0 { s.center_distance } else { 40.0 * pitch };
    let (zf1, zf2) = (f64::from(z1), f64::from(z2));
    let x0 = chain_links(zf1, zf2, a_in, pitch);
    let mut links = x0.ceil();
    if links % 2.0 != 0.0 {
        links += 1.0;
    }
    let a = chain_center(zf1, zf2, links, pitch).unwrap_or(a_in);
    p.fact("speed ratio", zf2 / zf1, Unit::None);
    p.fact("chain links (even, for the centre distance asked)", links, Unit::None);
    p.fact("chain length", links * pitch, Unit::Mm);
    p.fact("centre distance (taut chain)", a, Unit::Mm);
    let wrap = wrap_angle(r1.min(r2), r1.max(r2), a).to_degrees();
    p.fact("wrap on the smaller sprocket", wrap, Unit::Deg);
    if wrap < 120.0 {
        p.warn(format!("the chain wraps only {wrap:.0}° of the smaller sprocket (120° or more is usual)"));
    }
    if a < 30.0 * pitch || a > 80.0 * pitch {
        p.warn(format!("centre distance {} pitches: 30 to 50 is usual (80 at most without a guide)", num(a / pitch)));
    }
    let (st2, o2, _) = build(z2, "sprocket 2", &s.bore2, PI, p);
    let tip2 = Sprocket { teeth: z2, pitch, roller }.tip_diameter() / 2.0;
    let tip1 = Sprocket { teeth: z1, pitch, roller }.tip_diameter() / 2.0;
    if a <= tip1 + tip2 {
        p.warn("the sprockets overlap: lengthen the chain");
    }
    if wanted(s, 1) {
        p.bodies.push(Body { name: name(s, &format!("Sprocket {label} z{z1}")), frame: Frame::at(s.origin), steps: st1 });
    }
    if wanted(s, 2) {
        p.bodies.push(Body { name: name(s, &format!("Sprocket {label} z{z2}")), frame: Frame::at(s.origin).shifted([a, 0.0, 0.0]), steps: st2 });
    }
    if let (Some(outer), Some(inner)) = (belt_path(r1, r2, a, roller / 2.0), belt_path(r1, r2, a, -roller / 2.0)) {
        p.preview.push(PreviewGear::turning("chain", Region { outer: vec![outer], holes: vec![inner] }, V2::ZERO, 0.0, 0.0));
    }
    p.preview.push(PreviewGear::turning("sprocket 1", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 1.0, r1));
    p.preview.push(PreviewGear::turning(
        "sprocket 2",
        with_bore(Region { outer: vec![o2], holes: vec![] }, &s.bore2),
        V2::new(a, 0.0),
        zf1 / zf2,
        r2,
    ));
}

// ------------------------------------------------------------------------------------------
// Timing belt pulleys

/// The centre distance at which an open belt of `length` (pitch line) is taut, if any.
pub(crate) fn belt_center(r1: f64, r2: f64, length: f64) -> Option<f64> {
    let lo = (r1 - r2).abs() + 1e-6;
    if belt_length(r1, r2, lo) > length {
        return None;
    }
    let mut hi = lo.max(1.0);
    while belt_length(r1, r2, hi) < length {
        hi *= 2.0;
        if hi > 1e9 {
            return None;
        }
    }
    let mut lo = lo;
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if belt_length(r1, r2, mid) < length { lo = mid } else { hi = mid }
    }
    Some(0.5 * (lo + hi))
}

pub(super) fn pulley(s: &Spec, p: &mut Plan) {
    let belt = belt_of(s);
    let depth = belt.groove.depth();
    let tooth_len = s.face_width + (0.1 * s.face_width).max(0.5);
    let tf = if s.flanges { (0.4 * belt.pitch).max(1.0) } else { 0.0 };
    let total = tooth_len + 2.0 * tf;
    let sm = with_module(s, belt.pitch / PI);
    p.fact("belt pitch", belt.pitch, Unit::Mm);
    p.fact("pitch line differential (PLD)", belt.pld, Unit::Mm);
    p.fact("groove depth", depth, Unit::Mm);
    p.fact("belt width", s.face_width, Unit::Mm);
    p.fact("toothed length", tooth_len, Unit::Mm);
    p.preview_note = format!("{} groove approximated from published belt dimensions", belt.label);
    if belt.id.starts_with("gt2") || belt.id.starts_with("htd") {
        p.warn(format!("{}: the groove form is proprietary; this one is approximated from published belt tooth dimensions", belt.label));
    } else {
        p.warn(format!("{}: the groove is approximated from the belt's standard tooth dimensions, not the standard's pulley gauge; check the fit on a test piece", belt.label));
    }
    let (z1, z2) = (s.teeth, s.teeth2);
    let build = |z: u32, who: &str, bore: &crate::hub::Bore, phase: f64, p: &mut Plan| -> (Vec<Step>, Outline, f64, f64) {
        let pu = Pulley { belt, teeth: z };
        let pre = if z2 == 0 { String::new() } else { format!("{who} ") };
        let (dp, od) = (pu.pitch_diameter(), pu.outside_diameter());
        p.fact(format!("{pre}pitch diameter"), dp, Unit::Mm);
        p.fact(format!("{pre}outside diameter (pitch Ø − 2·PLD)"), od, Unit::Mm);
        if pu.land() <= 0.05 * belt.pitch {
            p.warn(format!("{who}: the grooves leave no land between them: too few teeth for this belt"));
        } else if z < 12 {
            p.warn(format!("{who}: under 12 teeth bend the belt hard (makers advise 12 to 16 at least)"));
        }
        let o = pu.outline(phase);
        let mut steps =
            vec![Step::Prism { region: Region { outer: vec![o.clone()], holes: vec![] }, z0: 0.0, height: total, twist: 0.0, op: Op::New }];
        if s.flanges {
            let rf = od / 2.0 + 1.5 * (depth + belt.pld) + 0.5;
            p.fact(format!("{pre}flange diameter"), 2.0 * rf, Unit::Mm);
            for z0 in [0.0, tf + tooth_len] {
                steps.push(Step::Prism {
                    region: Region { outer: vec![Outline::circle(V2::ZERO, rf)], holes: vec![] },
                    z0,
                    height: tf,
                    twist: 0.0,
                    op: Op::Join,
                });
            }
        }
        // (`sm` is a copy: hand over its own bore, so gear 2 still gets gear 2's shafts and hardware.)
        let bore = if std::ptr::eq(bore, &s.bore2) { &sm.bore2 } else { &sm.bore };
        hub_steps(&sm, bore, od / 2.0 - depth, total, &mut steps, p, who);
        (steps, o, dp / 2.0, od / 2.0)
    };
    let (st1, o1, r1, _) = build(z1, "pulley 1", &s.bore, 0.0, p);
    let label = belt.label;
    if z2 == 0 {
        if wanted(s, 1) {
            p.bodies.push(Body { name: name(s, &format!("Pulley {label} z{z1}")), frame: Frame::at(s.origin), steps: st1 });
        }
        p.preview.push(PreviewGear::turning("pulley", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 1.0, r1));
        return;
    }
    let r2 = Pulley { belt, teeth: z2 }.pitch_diameter() / 2.0;
    let a_in = if s.center_distance > 0.0 { s.center_distance } else { 1.5 * (r1 + r2) * 2.0 };
    let teeth_belt = (belt_length(r1, r2, a_in) / belt.pitch).round().max(1.0);
    let a = belt_center(r1, r2, teeth_belt * belt.pitch).unwrap_or(a_in);
    p.fact("speed ratio", f64::from(z2) / f64::from(z1), Unit::None);
    p.fact("belt teeth (nearest the centre distance asked)", teeth_belt, Unit::None);
    p.fact("belt length (pitch line)", teeth_belt * belt.pitch, Unit::Mm);
    p.fact("centre distance (taut belt)", a, Unit::Mm);
    let (rs, rl) = (r1.min(r2), r1.max(r2));
    let wrap = wrap_angle(rs, rl, a);
    let zs = f64::from(z1.min(z2));
    let in_mesh = (zs * wrap / TAU).floor();
    p.fact("teeth in mesh on the smaller pulley", in_mesh, Unit::None);
    if in_mesh < 6.0 {
        p.warn(format!("only {in_mesh} teeth in mesh on the smaller pulley (6 or more is usual)"));
    }
    let (st2, o2, _, _) = build(z2, "pulley 2", &s.bore2, PI, p);
    if wanted(s, 1) {
        p.bodies.push(Body { name: name(s, &format!("Pulley {label} z{z1}")), frame: Frame::at(s.origin), steps: st1 });
    }
    if wanted(s, 2) {
        p.bodies.push(Body { name: name(s, &format!("Pulley {label} z{z2}")), frame: Frame::at(s.origin).shifted([a, 0.0, 0.0]), steps: st2 });
    }
    if let (Some(outer), Some(inner)) = (belt_path(r1, r2, a, 1.5 * belt.pld), belt_path(r1, r2, a, -belt.pld - depth)) {
        p.preview.push(PreviewGear::turning("belt", Region { outer: vec![outer], holes: vec![inner] }, V2::ZERO, 0.0, 0.0));
    }
    p.preview.push(PreviewGear::turning("pulley 1", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 1.0, r1));
    p.preview.push(PreviewGear::turning(
        "pulley 2",
        with_bore(Region { outer: vec![o2], holes: vec![] }, &s.bore2),
        V2::new(a, 0.0),
        f64::from(z1) / f64::from(z2),
        r2,
    ));
}

// ------------------------------------------------------------------------------------------
// Involute splines

pub(crate) fn spline_of(s: &Spec) -> Spline {
    Spline {
        standard: s.spline_standard,
        module: s.module,
        teeth: s.teeth,
        pressure_angle: s.pressure_angle.to_radians(),
        reference_diameter: s.spline_diameter,
        fillet_root: s.fillet_root,
        clearance: s.backlash,
    }
}

pub(super) fn spline(s: &Spec, p: &mut Plan) {
    let sp = spline_of(s);
    let m = s.module;
    let shaft = sp.shaft();
    let (o_shaft, f) = sp.shaft_outline(0.0);
    if let Some(d) = f.defect() {
        p.refuse(format!("shaft: {d} ({} teeth, profile shift {}): use more teeth or another reference diameter", s.teeth, num(sp.shift())));
        return;
    }
    let hub_teeth = sp.hub_outline(0.0);
    let (da1, df1) = sp.shaft_diameters();
    let (da2, df2) = sp.hub_diameters();
    let (r_major, hub_pointed) = sp.hub_root_radius();
    let df2 = df2.min(2.0 * r_major);
    let (r_minor, hub_raised) = sp.hub_tip_radius(&f);
    let da2 = da2.max(2.0 * r_minor);
    if hub_raised {
        p.warn(format!(
            "hub: the minor diameter is raised to Ø{} to clear the shaft's root fillets (more teeth or a larger reference diameter avoid it)",
            num(da2)
        ));
    }
    if hub_pointed {
        p.warn(format!("hub: the spaces come to a point; the major diameter is lowered to Ø{}", num(df2)));
    }
    let db = sp.reference_diameter();
    match s.spline_standard {
        SplineStandard::Din5480 => {
            p.preview_note = format!("DIN 5480 – {} × {} × {}", num(db), num(m), s.teeth);
            p.fact("reference diameter d_B", db, Unit::Mm);
            p.fact("profile shift x", sp.shift(), Unit::None);
            if !(-0.06..=0.46).contains(&sp.shift()) {
                p.warn(format!(
                    "profile shift {:.3}: DIN 5480 keeps it between about −0.05 and 0.45; choose the teeth to suit the diameter",
                    sp.shift()
                ));
            }
            if (s.pressure_angle - 30.0).abs() > 1e-6 {
                p.warn("DIN 5480 uses a 30° pressure angle");
            }
        }
        SplineStandard::AnsiB92 => {
            p.preview_note = format!("ANSI B92.1 – {} teeth, {}/{} pitch, {}°", s.teeth, num(25.4 / m), num(50.8 / m), num(s.pressure_angle));
            p.fact("diametral pitch P", 25.4 / m, Unit::None);
            if ![30.0, 37.5, 45.0].iter().any(|a| (s.pressure_angle - a).abs() < 1e-6) {
                p.warn("ANSI B92.1 uses 30°, 37.5° or 45° pressure angles");
            } else if (s.pressure_angle - 30.0).abs() > 1e-6 {
                p.warn("the 30° tooth proportions are used at this pressure angle (the standard's 37.5° and 45° teeth are shorter)");
            }
        }
    }
    p.fact("pitch diameter", sp.pitch_diameter(), Unit::Mm);
    p.fact("base diameter", 2.0 * shaft.rb(), Unit::Mm);
    p.fact("shaft major (tip) diameter", da1.min(2.0 * f.ra), Unit::Mm);
    p.fact("shaft minor (root) diameter", df1, Unit::Mm);
    p.fact("hub minor (tip) diameter", da2, Unit::Mm);
    p.fact("hub major (root) diameter", df2, Unit::Mm);
    p.fact("shaft tooth thickness (pitch)", shaft.thickness(), Unit::Mm);
    p.fact("hub space width (pitch)", sp.hub_space(), Unit::Mm);
    let (mp, dp) = shaft.over_pins(None);
    p.fact(format!("shaft over two Ø{} pins", num(dp)), mp, Unit::Mm);
    if f.pointed {
        p.warn(format!("shaft: the teeth come to a point; the tip is lowered to Ø{}", num(2.0 * f.ra)));
    }
    if da2 / 2.0 <= f.rf + 0.02 * m || df2 / 2.0 <= f.ra {
        p.warn("the hub's teeth touch the shaft's roots or tips: check the diameters");
    }
    let r_out = if s.hub_diameter > 0.0 { s.hub_diameter / 2.0 } else { df2 / 2.0 + if s.rim > 0.0 { s.rim } else { (3.0 * m).max(0.25 * db) } };
    if r_out < df2 / 2.0 + 0.1 * m {
        p.refuse(format!(
            "the hub's outside diameter must be over Ø{} (its major diameter and a wall); it is Ø{}",
            num(df2 + 0.2 * m),
            num(2.0 * r_out)
        ));
        return;
    }
    if r_out <= df2 / 2.0 + 0.5 * m {
        p.warn("the hub's wall is thinner than half a module");
    }
    if s.bore.outline().is_some() && s.bore.reach() >= f.rf - 0.05 * m {
        p.refuse(format!(
            "shaft: the bore (reaching Ø{}) cuts into the tooth roots (Ø{}); use a smaller bore",
            num(2.0 * s.bore.reach()),
            num(2.0 * f.rf)
        ));
        return;
    }
    p.fact("hub outside diameter", 2.0 * r_out, Unit::Mm);
    let face = s.face_width;
    let hub_region = Region { outer: vec![Outline::circle(V2::ZERO, r_out)], holes: vec![hub_teeth] };
    if wanted(s, 1) {
        let mut steps = vec![Step::Prism {
            region: Region { outer: vec![o_shaft.clone()], holes: vec![] },
            z0: -face / 2.0,
            height: 2.0 * face,
            twist: 0.0,
            op: Op::New,
        }];
        if let Some(b) = s.bore.outline() {
            if s.bore.reach() >= f.rf - m {
                p.warn("shaft: under one module of material between the bore and the tooth roots");
            }
            steps.push(Step::Prism {
                region: Region { outer: vec![b], holes: vec![] },
                z0: -face / 2.0 - 1.0,
                height: 2.0 * face + 2.0,
                twist: 0.0,
                op: Op::Cut,
            });
        }
        p.bodies.push(Body { name: name(s, &format!("Spline shaft z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        let steps = vec![Step::Prism { region: hub_region.clone(), z0: 0.0, height: face, twist: 0.0, op: Op::New }];
        p.bodies.push(Body { name: name(s, &format!("Spline hub z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    }
    p.preview.push(PreviewGear::turning("hub", hub_region, V2::ZERO, 1.0, 0.0));
    p.preview.push(PreviewGear::turning("shaft", with_bore(Region { outer: vec![o_shaft], holes: vec![] }, &s.bore), V2::ZERO, 1.0, shaft.r()));
}

// ------------------------------------------------------------------------------------------
// Face gears

pub(super) fn face_gear(s: &Spec, p: &mut Plan) {
    let m = s.module;
    // (The plain rack: the tooth tuning doesn't apply to a face gear, see `left_out`.)
    let plain = Spec { clearance: 0.0, xy_compensation: 0.0, tip_shortening: 0.0, ..s.clone() };
    let pinion = plain.involute(s.teeth, 0.0, 0.0);
    let fg = FaceGear::new(pinion, s.teeth2, s.backlash);
    let (r_min, r_max) = (fg.undercut_radius(), fg.pointed_radius());
    let asked = if s.inner_diameter > 0.0 { s.inner_diameter / 2.0 } else { r_min };
    let mut ri = asked;
    if ri < r_min - 1e-9 {
        p.warn(format!("the teeth's inner end is raised to Ø{}: inside it the pinion undercuts them", num(2.0 * r_min)));
        ri = r_min;
    }
    let mut ro = ri + s.face_width;
    if ro > r_max + 1e-9 {
        p.warn(format!("the teeth's outer end is held to Ø{}: past it they come to a point", num(2.0 * r_max)));
        ro = r_max;
    }
    if ro - ri < m {
        p.refuse(format!(
            "no room for teeth a module long between Ø{} (inside it the pinion undercuts them) and Ø{} (past it they come to a point): use more face gear teeth or fewer pinion teeth{}",
            num(2.0 * r_min),
            num(2.0 * r_max),
            if s.inner_diameter > 0.0 { ", or a smaller inner diameter" } else { "" }
        ));
        return;
    }
    let r1 = pinion.r();
    p.fact("ratio", fg.z2() / fg.z1(), Unit::None);
    p.fact("pinion pitch diameter", 2.0 * r1, Unit::Mm);
    p.fact("face gear pitch diameter (m·z₂)", 2.0 * fg.pitch_radius(), Unit::Mm);
    p.fact("undercut limit diameter", 2.0 * r_min, Unit::Mm);
    p.fact("pointing limit diameter", 2.0 * r_max, Unit::Mm);
    p.fact("teeth inner diameter", 2.0 * ri, Unit::Mm);
    p.fact("teeth outer diameter", 2.0 * ro, Unit::Mm);
    p.fact("tooth length", ro - ri, Unit::Mm);
    p.fact("pinion axis above the pitch plane", r1, Unit::Mm);
    let aw = |r: f64| (pinion.rb() / (r * fg.z1() / fg.z2())).clamp(-1.0, 1.0).acos().to_degrees();
    p.fact("pressure angle at the inner end", aw(ri), Unit::Deg);
    p.fact("pressure angle at the outer end", aw(ro), Unit::Deg);
    let (o1, f1) = external_outline(&pinion, PI / fg.z1());
    tooth_warnings(p, "pinion", &pinion, &f1);
    if s.bore.outline().is_some() && s.bore.reach() >= ri - 0.05 * m {
        p.refuse(format!(
            "face gear: the bore (reaching Ø{}) cuts into the teeth (from Ø{}); use a smaller bore",
            num(2.0 * s.bore.reach()),
            num(2.0 * ri)
        ));
        return;
    }
    let (top, bottom) = (fg.top(), fg.bottom());
    let below = bottom - 0.3 * m;
    let rim = if s.rim > 0.0 { s.rim } else { 2.5 * m };
    if wanted(s, 1) {
        // The teeth, cut at a few heights and lofted; their lowest section again below the
        // spaces' bottom, to stand in the base.
        let heights = fg.heights();
        let mut sections: Vec<(Outline, f64)> = fg.sections(&heights, ri, ro).into_iter().zip(heights).collect();
        if let Some(first) = sections.first().cloned() {
            sections.insert(0, (first.0, below));
        }
        let mut steps = vec![
            Step::Loft { sections, op: Op::New },
            Step::Prism {
                region: Region { outer: vec![Outline::circle(V2::ZERO, ro + m)], holes: vec![] },
                z0: bottom - rim,
                height: rim,
                twist: 0.0,
                op: Op::Join,
            },
        ];
        if let Some(b) = s.bore.outline() {
            steps.push(Step::Prism {
                region: Region { outer: vec![b], holes: vec![] },
                z0: bottom - rim - 1.0,
                height: top - bottom + rim + 2.0,
                twist: 0.0,
                op: Op::Cut,
            });
        }
        p.bodies.push(Body { name: name(s, &format!("Face gear z{}", s.teeth2)), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        let len = ro - ri;
        let mut steps =
            vec![Step::Prism { region: Region { outer: vec![o1.clone()], holes: vec![] }, z0: 0.0, height: len, twist: 0.0, op: Op::New }];
        hub_steps(s, &s.bore2, f1.rf, len, &mut steps, p, "pinion");
        let o = s.origin;
        let frame = Frame { origin: [o[0] + ri, o[1], o[2] + r1], x: [0.0, 0.0, -1.0], z: [1.0, 0.0, 0.0] };
        p.bodies.push(Body { name: name(s, &format!("Face gear pinion z{}", s.teeth)), frame, steps });
    }
    // The preview: the pinion's transverse section and the face gear's section at the middle of
    // the teeth, a rack there, sliding as the pinion turns.
    let rm = 0.5 * (ri + ro);
    let period = TAU * rm / fg.z2();
    // An even count: the outline's own middle is a space, and the preview slides a sliding
    // member half a pitch back, which puts a tooth under the pinion's space.
    let rack = fg.unrolled(rm, 10, rim);
    let rw = rm * fg.z1() / fg.z2();
    p.preview_note = "pinion section, and the face gear's teeth at mid length unrolled as a rack".into();
    let pv = external_outline(&pinion, -PI / 2.0 + PI / fg.z1()).0;
    p.preview.push(PreviewGear::turning("pinion", Region { outer: vec![pv], holes: vec![] }, V2::ZERO, 1.0, rw));
    p.preview.push(PreviewGear {
        name: "face gear".into(),
        region: Region { outer: vec![rack], holes: vec![] },
        center: V2::ZERO,
        speed: 0.0,
        slide: rw,
        pitch_radius: 0.0,
        period,
        orbit: 0.0,
    });
}

// ------------------------------------------------------------------------------------------
// Elliptical pair

pub(super) fn elliptical(s: &Spec, p: &mut Plan) {
    use crate::elliptic::EllipticPair;
    let m = s.module;
    let e = EllipticPair {
        teeth: s.teeth,
        module: m,
        eccentricity: s.ellipse_eccentricity,
        pressure_angle: s.pressure_angle.to_radians(),
        addendum: s.rack.addendum,
        dedendum: s.rack.dedendum,
        backlash: s.backlash,
    };
    let pitch = e.pitch();
    let c1 = EllipticPair::curve1(&pitch);
    let period = PI * m;
    let o1 = e.teeth(&c1, 0.0);
    let o2 = e.teeth(&pitch.curve2, period / 2.0);
    let a = pitch.center;
    p.fact("centre distance", a, Unit::Mm);
    p.fact("ellipse semi-major axis", pitch.a, Unit::Mm);
    p.fact("ellipse semi-minor axis", pitch.b, Unit::Mm);
    p.fact("pitch curve length", period * f64::from(s.teeth), Unit::Mm);
    p.fact("smallest speed ratio ω₂/ω₁", pitch.ratio_min, Unit::None);
    p.fact("largest speed ratio ω₂/ω₁", pitch.ratio_max, Unit::None);
    // Where the ellipse bends most (the ends of its major axis) the teeth are like a small gear's.
    let rho = pitch.b * pitch.b / pitch.a;
    let z_local = 2.0 * rho / m;
    p.fact("teeth of the equivalent gear at the sharpest bend", z_local, Unit::None);
    if z_local < 17.0 * s.rack.addendum {
        p.warn(format!("the ends of the major axis bend like a {z_local:.0}-tooth gear: their teeth are undercut (less eccentricity or more teeth)"));
    }
    if wanted(s, 1) {
        let mut steps =
            vec![Step::Prism { region: Region { outer: vec![o1.clone()], holes: vec![] }, z0: 0.0, height: s.face_width, twist: 0.0, op: Op::New }];
        // Hub, bore, holes, shafts and hardware; no tooth chamfers (tip 0: the tips are no circle).
        hub_steps_tip(s, &s.bore, min_radius(&o1), 0.0, s.face_width, &mut steps, p, "gear 1");
        p.bodies.push(Body { name: name(s, &format!("Elliptical gear z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        let mut steps =
            vec![Step::Prism { region: Region { outer: vec![o2.clone()], holes: vec![] }, z0: 0.0, height: s.face_width, twist: 0.0, op: Op::New }];
        hub_steps_tip(s, &s.bore2, min_radius(&o2), 0.0, s.face_width, &mut steps, p, "gear 2");
        p.bodies.push(Body { name: name(s, &format!("Conjugate gear z{}", s.teeth)), frame: Frame::at(s.origin).shifted([a, 0.0, 0.0]), steps });
    }
    p.preview_note = "shown still: the speed ratio changes through the turn".into();
    p.preview.push(PreviewGear::turning("gear 1", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 0.0, 0.0));
    p.preview.push(PreviewGear::turning("gear 2", with_bore(Region { outer: vec![o2], holes: vec![] }, &s.bore2), V2::new(a, 0.0), 0.0, 0.0));
}
