//! Whether a sheet metal part meets a fabrication shop's rules (#20113). [`part_of`] reads what
//! the rules ask about from a sheet: its thickness, flat size, each round hole's diameter and
//! clearances (the measurement the press-in hardware notes share, `sheet_hardware::clearance`),
//! the threads and hardware it records. [`check`] holds that against a [`Profile`]: a plain
//! comparison of numbers, so a rule is tested without a model.
//!
//! A rule whose pages disagree uses the strictest value and reports a warning, never an error. A
//! rule the shop gives no value for, and what the check cannot measure, is listed as not checked,
//! with the reason: a part is never called fine for a rule that was not looked at.

use solidcraft_kernel::services::prism::loop_circle;

use crate::sheet_hardware::{clearance, mm};
use crate::shop_rules::{BendLines, Limit, Measured, Profile, Rule, RuleId, Wanted};
use crate::{Document, FeatureKind, SurfaceSheet};

/// How near a hole is to a bend.
#[derive(Clone, Debug, PartialEq)]
pub struct BendGap {
    /// The bend's number in the bend table, from 1.
    pub number: usize,
    /// The bend's inside radius (mm).
    pub radius: f64,
    /// The hole's centre is inside the bend.
    pub inside: bool,
    /// The distance to where the bend starts (mm); 0 inside the bend.
    pub from_start: f64,
    /// The distance to the bend's centre line (mm).
    pub from_line: f64,
}

/// A round hole of the flat, in the flat as it is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub at: [f64; 2],
    pub diameter: f64,
    /// The distance to the nearest edge, cut-out or other hole (mm).
    pub edge: f64,
    pub bends: Vec<BendGap>,
}

/// A threaded feature of the design.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadFact {
    pub feature: u64,
    pub name: String,
    /// The thread is cut into the wall, not only listed.
    pub modelled: bool,
}

/// A hole that is the mounting hole of press-in hardware.
#[derive(Clone, Debug, PartialEq)]
pub struct HardwareHole {
    pub feature: u64,
    /// `M3 nut`.
    pub label: String,
    pub at: [f64; 2],
}

/// One side of a bend: the panel there and how far it reaches from the bend (#20113).
#[derive(Clone, Debug, PartialEq)]
pub struct Flange {
    /// The bend's number in the bend table, from 1.
    pub number: usize,
    /// The side of the bend, as its centre line runs from its start to its end in the flat as it
    /// is read: `left` or `right`.
    pub side: &'static str,
    /// The bend's inside radius (mm).
    pub radius: f64,
    /// The least reach square to the bend, from where the bend starts (mm).
    pub from_start: f64,
    /// ... and from the bend's centre line (mm).
    pub from_line: f64,
    /// The bend's middle, in the flat as it is read.
    pub at: [f64; 2],
}

/// What the flat DXF a part would export holds, as far as the shops' file rules ask: read from the
/// export's own layout (`sheet.export_dxf`'s), for the options the export is asked with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfFacts {
    /// The layers the cut contours are on: the outline, the cut-outs, letters cut through, the
    /// perforation's holes and the junction cuts. Layer `0` when everything is on one.
    pub cut_layers: Vec<String>,
    /// How many bends the flat has.
    pub bends: usize,
    /// How many bend lines the file draws (none when they are left out).
    pub bend_lines: usize,
    /// The bend lines are on a layer with the cut contours.
    pub bend_lines_with_cuts: bool,
    /// Junction cuts of split edges, each drawn as a line out and back: not a closed contour, and
    /// cut twice.
    pub slits: usize,
    /// Cut contours the DXF writer cannot write as closed shapes.
    pub unclosed: usize,
    /// Notes written as text entities: beside bends and beads, and beside hardware holes.
    pub text_notes: usize,
    /// The export is asked for `layers: single`.
    pub single_layer: bool,
    /// Which notes the export is asked for: `bend_notes` and `hardware_notes`.
    pub notes_asked: Vec<&'static str>,
}

/// What the rules ask about a part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    pub thickness: f64,
    /// The material of the sheet's rule, as written; may be empty.
    pub material: String,
    /// The flat's bounding box (mm).
    pub flat_size: [f64; 2],
    /// Every body of the design.
    pub bodies: Vec<String>,
    /// Recesses cut part-way through the sheet.
    pub recesses: usize,
    pub holes: Vec<Hole>,
    /// Cut-outs that are not round.
    pub cutouts: usize,
    /// Perforation fields: the feature and its hole diameter (mm).
    pub perforations: Vec<(u64, f64)>,
    /// A free-form edge's distance was left unmeasured.
    pub free_form: bool,
    pub threads: Vec<ThreadFact>,
    pub hardware: Vec<HardwareHole>,
    /// Both sides of every bend.
    pub flanges: Vec<Flange>,
    /// Why some flanges were not measured, when they were not.
    pub flange_gaps: Vec<String>,
    /// What the flat DXF would hold; none when it was not read (the file rules are then listed as
    /// not checked).
    pub dxf: Option<DxfFacts>,
}

/// Reads `sheet` of `doc` for the rules: `bodies` are all the design's bodies and `flat_size` the
/// flat's bounding box.
pub fn part_of(doc: &Document, sheet: &SurfaceSheet, bodies: Vec<String>, flat_size: [f64; 2]) -> Part {
    // The flat as it is read, the way a flat pattern is shown.
    let sk = sheet.shown();
    let mut part = Part {
        thickness: sk.thickness,
        material: doc.sheet_rule(Some(sk.rule.as_str())).material,
        flat_size,
        bodies,
        recesses: sk.pockets.len(),
        perforations: sk.perforations.iter().map(|p| (p.feature, p.diameter)).collect(),
        ..Part::default()
    };
    for region in &sk.regions {
        for hole in &region.holes {
            let Some((c, r)) = loop_circle(hole) else {
                part.cutouts += 1;
                continue;
            };
            let near = clearance(&sk, region, hole, c);
            part.free_form |= near.free_form;
            let bends = near
                .bends
                .iter()
                .enumerate()
                .map(|(i, b)| BendGap { number: i + 1, radius: b.radius, inside: b.inside, from_start: b.from_start, from_line: b.from_line })
                .collect();
            part.holes.push(Hole { at: [c.x, c.y], diameter: 2.0 * r, edge: near.edge, bends });
        }
    }
    part.hardware = sk
        .hardware_holes()
        .into_iter()
        .map(|h| HardwareHole { feature: h.feature, label: format!("{} {}", h.thread, h.kind.label()), at: [h.center.x, h.center.y] })
        .collect();
    flanges_of(&sk, &mut part);
    let live = doc.marker.unwrap_or(doc.features.len());
    for f in doc.features.iter().take(live).filter(|f| !f.suppressed) {
        let modelled = match &f.kind {
            FeatureKind::Hole { thread: Some(_), modeled, .. } => *modeled,
            FeatureKind::Thread { modeled, .. } => *modeled,
            _ => continue,
        };
        part.threads.push(ThreadFact { feature: f.id, name: f.name.clone(), modelled });
    }
    part
}

/// The tolerance the flat is cut at the bends' zones with (mm).
const ZONE_CUT_TOLERANCE: f64 = 1e-9;

/// How far each flange reaches from its bend: the flat cut at every bend's zone leaves its panels,
/// and the kernel measures the least reach square to the bend across the panel on each side
/// (`prism::flange_reach`). The reach is over the bend's length less one thickness at each end, so
/// a rounded or chamfered corner of the flange does not decide it.
fn flanges_of(sk: &SurfaceSheet, part: &mut Part) {
    if sk.bends.is_empty() {
        return;
    }
    let zones: Vec<_> = sk.bends.iter().map(|b| b.zone.clone()).collect();
    let cut = solidcraft_kernel::planar_regions_boolean(&sk.regions, &zones, solidcraft_kernel::BoolOp::Cut, ZONE_CUT_TOLERANCE);
    let panels = match cut {
        Ok(p) => p,
        Err(e) => {
            part.flange_gaps.push(format!("the flat could not be cut at the bends' zones, so no flange was measured ({e})"));
            return;
        }
    };
    let mut unmeasured = 0usize;
    for (i, b) in sk.bends.iter().enumerate() {
        let at = [(b.start.x + b.end.x) / 2.0, (b.start.y + b.end.y) / 2.0];
        for (left, side) in [(true, "left"), (false, "right")] {
            match solidcraft_kernel::services::prism::flange_reach(&panels, &b.zone, b.start, b.end, left, sk.thickness) {
                Some(r) => part.flanges.push(Flange { number: i + 1, side, radius: b.radius, from_start: r.from_edge, from_line: r.from_line, at }),
                None => unmeasured += 1,
            }
        }
    }
    if unmeasured > 0 {
        part.flange_gaps
            .push(format!("{unmeasured} flange(s) were not measured: a free-form edge, or no panel standing against the bend's zone on that side"));
    }
}

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The part breaks a rule the shop states.
    Error,
    /// The shop's pages disagree, or the note is advice.
    Warning,
}

/// One rule a part breaks.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub feature: Option<u64>,
    /// Where, in the flat as it is read.
    pub at: Option<[f64; 2]>,
    pub value: Option<f64>,
    pub limit: Option<f64>,
    /// The shop's pages the rule comes from.
    pub sources: Vec<String>,
}

/// A rule the check did not look at, or not wholly, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct NotChecked {
    pub rule: String,
    pub why: String,
}

/// What a check found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub checked: Vec<String>,
    pub not_checked: Vec<NotChecked>,
}

struct Ctx<'a> {
    shop: &'a str,
    part: &'a Part,
    rule: &'a Rule,
    severity: Severity,
}

impl Ctx<'_> {
    fn finding(&self, message: String) -> Finding {
        Finding {
            rule: self.rule.id.as_str().to_string(),
            severity: self.severity,
            message,
            feature: None,
            at: None,
            value: None,
            limit: None,
            sources: self.rule.urls(),
        }
    }

    /// The shop, and its process when the rule names one: `PCBWay's laser cutting`.
    fn who(&self) -> String {
        match &self.rule.process {
            Some(p) => format!("{}'s {p}", self.shop),
            None => self.shop.to_string(),
        }
    }

    /// What a warning from disagreeing pages adds.
    fn note(&self) -> &'static str {
        if self.rule.disagreement { " (the strictest of the values its pages give, which differ)" } else { "" }
    }
}

fn at_text(at: [f64; 2]) -> String {
    format!("({}, {})", mm(at[0]), mm(at[1]))
}

fn not_checked(r: &mut Report, rule: RuleId, why: String) {
    r.not_checked.push(NotChecked { rule: rule.as_str().to_string(), why });
}

fn names(list: &[String], material: &str) -> bool {
    let m = material.to_lowercase();
    list.iter().any(|n| m.contains(&n.to_lowercase()))
}

/// The limits that apply to `material`: those for any material, and those that name it; when
/// none does, every limit, and `false`.
fn applicable<'a>(limits: &'a [Limit], material: &str) -> (Vec<&'a Limit>, bool) {
    let chosen: Vec<&Limit> = limits.iter().filter(|l| l.materials.is_empty() || names(&l.materials, material)).collect();
    if chosen.is_empty() { (limits.iter().collect(), false) } else { (chosen, true) }
}

fn unmatched(r: &mut Report, ctx: &Ctx<'_>) {
    let m = if ctx.part.material.is_empty() { "none given".to_string() } else { ctx.part.material.clone() };
    not_checked(r, ctx.rule.id, format!("the sheet rule's material ({m}) is not one {} names, so every value it gives was applied", ctx.shop));
}

fn most(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.fold(None, |a: Option<f64>, v| Some(a.map_or(v, |x| x.max(v))))
}

fn least(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.fold(None, |a: Option<f64>, v| Some(a.map_or(v, |x| x.min(v))))
}

/// Holds `part` against `profile`. The findings come in the order of [`RuleId::ALL`], then the
/// order of the part's holes.
pub fn check(profile: &Profile, part: &Part) -> Report {
    let mut r = Report::default();
    for id in RuleId::ALL {
        let Some(rule) = profile.rule(id) else {
            not_checked(&mut r, id, format!("{}'s pages give no value for it", profile.name));
            continue;
        };
        let dxf_rule = matches!(id, RuleId::DxfSingleLayer | RuleId::DxfClosedContours | RuleId::DxfBendLines | RuleId::TextOutlined);
        if dxf_rule && part.dxf.is_none() {
            not_checked(&mut r, id, "the flat DXF was not read, so its file rules were not looked at".to_string());
            continue;
        }
        r.checked.push(id.as_str().to_string());
        let severity = if rule.disagreement { Severity::Warning } else { Severity::Error };
        let ctx = Ctx { shop: &profile.name, part, rule, severity };
        match id {
            RuleId::OneBody => one_body(&ctx, &mut r),
            RuleId::UniformThickness => uniform(&ctx, &mut r),
            RuleId::StockThickness => stock(&ctx, &mut r),
            RuleId::MaxFlat => max_flat(&ctx, &mut r),
            RuleId::HoleMinDiameter => min_diameter(&ctx, &mut r),
            RuleId::HoleToEdge => to_edge(&ctx, &mut r),
            RuleId::HoleToBend => to_bend(&ctx, &mut r),
            RuleId::ThreadModelling => threads(&ctx, &mut r),
            RuleId::HardwareModelling => hardware(&ctx, &mut r),
            RuleId::FlangeMin => flange_min(&ctx, &mut r),
            RuleId::DxfSingleLayer => dxf_single_layer(&ctx, &mut r),
            RuleId::DxfClosedContours => dxf_closed_contours(&ctx, &mut r),
            RuleId::DxfBendLines => dxf_bend_lines(&ctx, &mut r),
            RuleId::TextOutlined => text_outlined(&ctx, &mut r),
        }
    }
    r
}

fn one_body(ctx: &Ctx<'_>, r: &mut Report) {
    let n = ctx.part.bodies.len();
    if n > 1 {
        let list: Vec<&str> = ctx.part.bodies.iter().take(5).map(String::as_str).collect();
        let more = if n > 5 { ", ..." } else { "" };
        r.findings.push(ctx.finding(format!(
            "the design has {n} bodies ({}{more}); {} takes one body in a file{}",
            list.join(", "),
            ctx.who(),
            ctx.note()
        )));
    }
}

fn uniform(ctx: &Ctx<'_>, r: &mut Report) {
    let n = ctx.part.recesses;
    if n > 0 {
        let message =
            format!("{n} recess(es) cut part-way into the sheet make it thinner there; {} wants one thickness all over{}", ctx.who(), ctx.note());
        r.findings.push(ctx.finding(message));
    }
}

fn stock(ctx: &Ctx<'_>, r: &mut Report) {
    let t = ctx.part.thickness;
    let named: Vec<_> = ctx.rule.stock.iter().filter(|s| !s.materials.is_empty() && names(&s.materials, &ctx.part.material)).collect();
    let any: Vec<_> = ctx.rule.stock.iter().filter(|s| s.materials.is_empty()).collect();
    let mut entries: Vec<_> = any.into_iter().chain(named).collect();
    let matched = !entries.is_empty();
    if !matched {
        entries = ctx.rule.stock.iter().collect();
        unmatched(r, ctx);
    }
    if entries.iter().any(|s| s.admits(t)) {
        return;
    }
    let mut listed: Vec<f64> = entries.iter().flat_map(|s| s.thicknesses_mm.iter().copied()).collect();
    listed.sort_by(f64::total_cmp);
    let below = listed.iter().copied().rfind(|v| *v < t);
    let above = listed.iter().copied().find(|v| *v > t);
    let mut parts: Vec<String> = Vec::new();
    for s in &entries {
        if s.min_mm.is_some() || s.max_mm.is_some() {
            let lo = s.min_mm.map_or(String::new(), mm);
            let hi = s.max_mm.map_or(String::new(), mm);
            parts.push(format!("{lo} to {hi} mm"));
        }
    }
    match (below, above) {
        (Some(a), Some(b)) => parts.push(format!("the nearest stock sizes are {} and {} mm", mm(a), mm(b))),
        (Some(a), None) => parts.push(format!("its thickest stock is {} mm", mm(a))),
        (None, Some(b)) => parts.push(format!("its thinnest stock is {} mm", mm(b))),
        (None, None) => {}
    }
    let mut f = ctx.finding(format!("the sheet is {} mm thick, which {} does not stock: {}{}", mm(t), ctx.shop, parts.join("; "), ctx.note()));
    f.value = Some(t);
    r.findings.push(f);
}

fn max_flat(ctx: &Ctx<'_>, r: &mut Report) {
    let (limits, matched) = applicable(&ctx.rule.limits, &ctx.part.material);
    if !matched {
        unmatched(r, ctx);
    }
    let [x, y] = ctx.part.flat_size;
    let (long, short) = if x >= y { (x, y) } else { (y, x) };
    let long_max = least(limits.iter().filter_map(|l| l.value));
    let short_max = least(limits.iter().filter_map(|l| l.short_value));
    let long_over = long_max.is_some_and(|m| long > m + 1e-9);
    let short_over = short_max.is_some_and(|m| short > m + 1e-9);
    if long_over || short_over {
        let size = match short_max {
            Some(s) => format!("{} x {} mm", mm(long_max.unwrap_or(f64::INFINITY)), mm(s)),
            None => format!("{} mm long", mm(long_max.unwrap_or(f64::INFINITY))),
        };
        let mut f = ctx.finding(format!("the flat is {} x {} mm; {} takes at most {size}{}", mm(long), mm(short), ctx.who(), ctx.note()));
        f.value = Some(long);
        f.limit = long_max;
        r.findings.push(f);
    }
}

/// The least a hole's diameter, edge distance or bend distance may be: the strictest of the
/// limits that apply.
fn strictest(ctx: &Ctx<'_>, r: &mut Report) -> Option<f64> {
    let (limits, matched) = applicable(&ctx.rule.limits, &ctx.part.material);
    if !matched {
        unmatched(r, ctx);
    }
    most(limits.iter().filter_map(|l| l.needed(ctx.part.thickness, 0.0)))
}

fn cutouts_note(ctx: &Ctx<'_>, r: &mut Report) {
    let n = ctx.part.cutouts;
    if n > 0 {
        let why = format!("{n} cut-out(s) that are not round were not measured: the rule is checked for round holes only");
        not_checked(r, ctx.rule.id, why);
    }
}

fn min_diameter(ctx: &Ctx<'_>, r: &mut Report) {
    let Some(need) = strictest(ctx, r) else { return };
    let t = mm(ctx.part.thickness);
    for h in &ctx.part.holes {
        if h.diameter + 1e-9 < need {
            let message = format!(
                "the hole at {} is \u{d8}{} mm; {} needs at least \u{d8}{} mm for {t} mm sheet{}",
                at_text(h.at),
                mm(h.diameter),
                ctx.who(),
                mm(need),
                ctx.note()
            );
            let mut f = ctx.finding(message);
            f.at = Some(h.at);
            f.value = Some(h.diameter);
            f.limit = Some(need);
            r.findings.push(f);
        }
    }
    for (feature, d) in &ctx.part.perforations {
        if *d + 1e-9 < need {
            let message = format!(
                "the perforation holes are \u{d8}{} mm; {} needs at least \u{d8}{} mm for {t} mm sheet{}",
                mm(*d),
                ctx.who(),
                mm(need),
                ctx.note()
            );
            let mut f = ctx.finding(message);
            f.feature = Some(*feature);
            f.value = Some(*d);
            f.limit = Some(need);
            r.findings.push(f);
        }
    }
    cutouts_note(ctx, r);
}

fn to_edge(ctx: &Ctx<'_>, r: &mut Report) {
    let Some(need) = strictest(ctx, r) else { return };
    for h in &ctx.part.holes {
        if h.edge + 1e-9 < need {
            let message = format!(
                "the hole at {} is {} mm from the nearest edge, cut-out or other hole; {} needs at least {} mm{}",
                at_text(h.at),
                mm(h.edge),
                ctx.who(),
                mm(need),
                ctx.note()
            );
            let mut f = ctx.finding(message);
            f.at = Some(h.at);
            f.value = Some(h.edge);
            f.limit = Some(need);
            r.findings.push(f);
        }
    }
    cutouts_note(ctx, r);
    if ctx.part.free_form {
        not_checked(r, ctx.rule.id, "a free-form edge was not measured".to_string());
    }
}

fn to_bend(ctx: &Ctx<'_>, r: &mut Report) {
    let (limits, matched) = applicable(&ctx.rule.limits, &ctx.part.material);
    if !matched {
        unmatched(r, ctx);
    }
    let t = ctx.part.thickness;
    for h in &ctx.part.holes {
        // The bend and the limit the hole falls shortest of: (shortfall, need, distance, bend, how).
        let mut worst: Option<(f64, f64, f64, &BendGap, Measured)> = None;
        for b in &h.bends {
            for l in &limits {
                let Some(need) = l.needed(t, b.radius) else { continue };
                let from = l.measured_from();
                let d = if from == Measured::BendLine { b.from_line } else { b.from_start };
                let short = need - d;
                if d + 1e-9 < need && worst.as_ref().is_none_or(|w| short > w.0) {
                    worst = Some((short, need, d, b, from));
                }
            }
        }
        let Some((_, need, d, b, from)) = worst else { continue };
        let message = if b.inside && from == Measured::BendStart {
            format!(
                "the hole at {} is inside bend {}; {} needs holes at least {} mm from a bend{}",
                at_text(h.at),
                b.number,
                ctx.who(),
                mm(need),
                ctx.note()
            )
        } else {
            let to = if from == Measured::BendLine { "the bend line" } else { "where the bend starts" };
            format!(
                "the hole at {} is {} mm from bend {} (to {to}); {} needs at least {} mm{}",
                at_text(h.at),
                mm(d),
                b.number,
                ctx.who(),
                mm(need),
                ctx.note()
            )
        };
        let mut f = ctx.finding(message);
        f.at = Some(h.at);
        f.value = Some(d);
        f.limit = Some(need);
        r.findings.push(f);
    }
    cutouts_note(ctx, r);
    if ctx.part.free_form {
        not_checked(r, ctx.rule.id, "a free-form edge was not measured".to_string());
    }
}

fn flange_min(ctx: &Ctx<'_>, r: &mut Report) {
    let (limits, matched) = applicable(&ctx.rule.limits, &ctx.part.material);
    if !matched {
        unmatched(r, ctx);
    }
    let t = ctx.part.thickness;
    for fl in &ctx.part.flanges {
        // The limit the flange falls shortest of: (shortfall, need, reach, how it is measured).
        let mut worst: Option<(f64, f64, f64, Measured)> = None;
        for l in &limits {
            let Some(need) = l.needed(t, fl.radius) else { continue };
            let from = l.measured_from();
            let d = if from == Measured::BendLine { fl.from_line } else { fl.from_start };
            let short = need - d;
            if d + 1e-9 < need && worst.as_ref().is_none_or(|w| short > w.0) {
                worst = Some((short, need, d, from));
            }
        }
        let Some((_, need, d, from)) = worst else { continue };
        let to = if from == Measured::BendLine { "the bend line" } else { "where the bend starts" };
        let message = format!(
            "the flange on the {} of bend {} at {} reaches only {} mm (from {to}); {} needs flanges at least {} mm long{}",
            fl.side,
            fl.number,
            at_text(fl.at),
            mm(d),
            ctx.who(),
            mm(need),
            ctx.note()
        );
        let mut f = ctx.finding(message);
        f.at = Some(fl.at);
        f.value = Some(d);
        f.limit = Some(need);
        r.findings.push(f);
    }
    for why in &ctx.part.flange_gaps {
        not_checked(r, ctx.rule.id, why.clone());
    }
}

/// The layers named for a message: `CUT and OUTLINE`.
fn layer_list(layers: &[String]) -> String {
    match layers {
        [] => "no layer".to_string(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn dxf_single_layer(ctx: &Ctx<'_>, r: &mut Report) {
    let Some(d) = &ctx.part.dxf else { return };
    if d.cut_layers.len() > 1 {
        let keep = if d.bend_lines > 0 { " (with bend_lines: false as well, to keep the bend lines off it)" } else { "" };
        r.findings.push(ctx.finding(format!(
            "the DXF would put the cut contours on {} layers ({}); {} wants all cut geometry on one layer: export with layers: single{keep}{}",
            d.cut_layers.len(),
            layer_list(&d.cut_layers),
            ctx.who(),
            ctx.note()
        )));
    } else if d.bend_lines_with_cuts {
        r.findings.push(ctx.finding(format!(
            "with layers: single the {} bend line(s) are on the cut layer too, where {} would cut them: export with bend_lines: false{}",
            d.bend_lines,
            ctx.who(),
            ctx.note()
        )));
    }
}

fn dxf_closed_contours(ctx: &Ctx<'_>, r: &mut Report) {
    let Some(d) = &ctx.part.dxf else { return };
    if d.slits > 0 {
        r.findings.push(ctx.finding(format!(
            "{} cut(s) along the junction of a split edge are drawn in the DXF as a line out and back, not as a closed contour; {} wants closed contours and no line cut twice{}",
            d.slits,
            ctx.who(),
            ctx.note()
        )));
    }
    if d.unclosed > 0 {
        r.findings.push(ctx.finding(format!(
            "{} cut contour(s) cannot be written to the DXF as closed shapes; {} wants closed contours{}",
            d.unclosed,
            ctx.who(),
            ctx.note()
        )));
    }
}

fn dxf_bend_lines(ctx: &Ctx<'_>, r: &mut Report) {
    let Some(d) = &ctx.part.dxf else { return };
    match ctx.rule.bend_lines.unwrap_or(BendLines::Present) {
        BendLines::Present => {
            if d.bends > 0 && d.bend_lines == 0 {
                r.findings.push(ctx.finding(format!(
                    "the DXF leaves out the {} bend line(s); {} wants a line at the centre of each bend in a flat file: export with bend_lines: true{}",
                    d.bends,
                    ctx.who(),
                    ctx.note()
                )));
            } else if d.bend_lines_with_cuts {
                r.findings.push(ctx.finding(format!(
                    "with layers: single the {} bend line(s) are on the cut layer, not apart from the cuts; {} wants them marked as bends: export with layers: roles{}",
                    d.bend_lines,
                    ctx.who(),
                    ctx.note()
                )));
            }
        }
        BendLines::Absent => {
            if d.bend_lines > 0 {
                r.findings.push(ctx.finding(format!(
                    "the DXF draws {} bend line(s); {} would cut them: export with bend_lines: false{}",
                    d.bend_lines,
                    ctx.who(),
                    ctx.note()
                )));
            }
        }
    }
}

fn text_outlined(ctx: &Ctx<'_>, r: &mut Report) {
    let Some(d) = &ctx.part.dxf else { return };
    if d.text_notes > 0 {
        let asked = d.notes_asked.join(" and ");
        r.findings.push(ctx.finding(format!(
            "the DXF would carry {} note(s) as text entities, not as outlines; {} wants text as outlines: export without {asked} (letters in the flat are always outlines){}",
            d.text_notes,
            ctx.who(),
            ctx.note()
        )));
    }
}

fn threads(ctx: &Ctx<'_>, r: &mut Report) {
    let wanted = ctx.rule.wanted.unwrap_or(Wanted::Plain);
    for t in &ctx.part.threads {
        let message = match (wanted, t.modelled) {
            (Wanted::Plain, true) => format!(
                "{} cuts a modelled thread; {} wants a plain hole at the tap size, with the thread named on the order{}",
                t.name,
                ctx.who(),
                ctx.note()
            ),
            (Wanted::Modelled, false) => {
                format!("{} is a thread that is only listed; {} wants threads modelled in the file{}", t.name, ctx.who(), ctx.note())
            }
            _ => continue,
        };
        let mut f = ctx.finding(message);
        f.feature = Some(t.feature);
        r.findings.push(f);
    }
}

fn hardware(ctx: &Ctx<'_>, r: &mut Report) {
    match ctx.rule.wanted.unwrap_or(Wanted::Plain) {
        Wanted::Plain => {
            for h in &ctx.part.hardware {
                let message = format!(
                    "the hole at {} is the mounting hole of an {}; {} takes press-in hardware as an order option, not from the file: name it on the order",
                    at_text(h.at),
                    h.label,
                    ctx.who()
                );
                let mut f = ctx.finding(message);
                f.severity = Severity::Warning;
                f.feature = Some(h.feature);
                f.at = Some(h.at);
                r.findings.push(f);
            }
        }
        Wanted::Modelled => {
            if !ctx.part.hardware.is_empty() {
                let why = format!(
                    "{} wants hardware modelled in the file; a hardware hole is a plain hole with a record of the hardware, and what the shop reads there was not checked",
                    ctx.shop
                );
                not_checked(r, ctx.rule.id, why);
            }
        }
    }
}

#[cfg(test)]
#[path = "shop_check_tests.rs"]
mod tests;
