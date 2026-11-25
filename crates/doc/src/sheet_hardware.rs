//! Whether a sheet holds the press-in hardware a Hole is the mounting hole of (#17752): the sheet
//! as thick as the hardware needs, and the hole's centre far enough from an edge, a cut-out or a
//! bend. Each finding is a warning on the feature, never an error: a hole is still a valid hole,
//! and a change of gauge or of a bend must not break the timeline. (`sheet.hardware` itself
//! refuses a sheet too thin when it is run.) Distances are exact, from the centre to the lines and
//! arcs of the flat, which is where a sheet's edges and bends are measured: the flat's outline and
//! cut-outs for an edge, and the boundary of a bend's zone, where the flat sheet ends and the bend
//! starts, for a bend (docs/hardware-catalogue.md, "Near a bend").

use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_kernel::services::prism::{loop_circle, loop_holds, same_circle, segment_distance};

use crate::hardware_catalog::{HardwareSize, hardware_size};
use crate::sheet::SheetHardware;
use crate::{Document, Feature, FeatureKind, Hardware, SurfaceSheet};

/// How far a hole may be from a circle to be the same circle (mm).
const SAME: f64 = 1e-6;

/// The least distance from `p` to the loops, and whether a free-form edge was left unmeasured.
fn nearest<'a>(p: Vec2, loops: impl Iterator<Item = &'a Loop2>) -> (f64, bool) {
    let (mut least, mut skipped) = (f64::INFINITY, false);
    for s in loops.flat_map(|l| &l.segs) {
        match segment_distance(s, p) {
            Some(d) => least = least.min(d),
            None => skipped = true,
        }
    }
    (least, skipped)
}

/// A length for a message: millimetres to two places.
pub(crate) fn mm(v: f64) -> String {
    let t = format!("{v:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What `after`, the sheet a Hole with `hardware` was cut from, lacks for that hardware: one
/// note for a sheet too thin, then one for each new hole too near an edge or a bend. The holes are
/// the whole circles in `after`'s flat that `before` (the sheet as the feature found it) lacks.
pub(crate) fn findings(before: Option<&SurfaceSheet>, after: &SurfaceSheet, hardware: &Hardware) -> Vec<String> {
    let Some(size) = hardware_size(hardware.kind, &hardware.thread) else {
        return vec![format!("{} {} is not a size in the hardware table", hardware.thread, hardware.kind.label())];
    };
    let what = format!("an {} {}", hardware.thread, hardware.kind.label());
    let mut out = Vec::new();
    if after.thickness + 1e-9 < size.min_sheet {
        out.push(format!("{} is {} mm thick: {what} is made for a sheet of at least {} mm", after.body, mm(after.thickness), mm(size.min_sheet)));
    }
    out.extend(distances(before, after, &size, &what));
    out
}

/// The whole circles in `after`'s flat that `before` lacks, with the loop of each.
fn new_circles<'a>(before: Option<&SurfaceSheet>, after: &'a SurfaceSheet) -> Vec<(&'a Region2, &'a Loop2, (Vec2, f64))> {
    let known: Vec<(Vec2, f64)> = before.map(|b| b.regions.iter().flat_map(|r| &r.holes).filter_map(loop_circle).collect()).unwrap_or_default();
    let new = |c: &(Vec2, f64)| !known.iter().any(|k| same_circle(*k, *c, SAME));
    after
        .regions
        .iter()
        .flat_map(|r| r.holes.iter().map(move |h| (r, h)))
        .filter_map(|(r, h)| loop_circle(h).filter(new).map(|c| (r, h, c)))
        .collect()
}

/// Whose mounting holes a feature cuts (see [`hardware_source`]).
#[derive(Debug, PartialEq)]
pub(crate) enum Source<'a> {
    /// No hole of press-in hardware: a plain hole, or a feature that is no hole.
    None,
    /// Only holes of this one hardware.
    One(&'a Hardware),
    /// Holes of different hardware, or hardware holes among plain ones: what a hole is for cannot be
    /// said of each copy, so none is recorded as hardware.
    Mixed,
}

/// The hardware a feature cuts mounting holes for: a Hole's own, or the one the holes a Pattern or
/// Mirror copies are all for, through nested patterns. A copy is a hardware hole only when the hole
/// it copies is, so a pattern of a nut's hole and a plain hole is [`Source::Mixed`], not a nut's.
pub(crate) fn hardware_source<'a>(doc: &'a Document, f: &'a Feature, depth: usize) -> Source<'a> {
    match &f.kind {
        FeatureKind::Hole { hardware: Some(h), .. } => Source::One(h),
        FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() && depth < 8 => {
            let mut found: Option<&Hardware> = None;
            let mut plain = false;
            for g in features.iter().filter_map(|id| doc.feature(id.0)).filter(|g| !g.suppressed) {
                match hardware_source(doc, g, depth + 1) {
                    Source::Mixed => return Source::Mixed,
                    Source::None => plain = true,
                    Source::One(h) => match found {
                        Some(other) if other != h => return Source::Mixed,
                        _ => found = Some(h),
                    },
                }
            }
            match found {
                Some(_) if plain => Source::Mixed,
                Some(h) => Source::One(h),
                None => Source::None,
            }
        }
        _ => Source::None,
    }
}

/// The mounting holes `after`'s flat has that `before`'s lacks, as the hardware's: for the
/// feature `feature`, a stud's or standoff's `length` (mm) evaluated.
pub(crate) fn marks(
    before: Option<&SurfaceSheet>,
    after: &SurfaceSheet,
    feature: u64,
    hardware: &Hardware,
    length: Option<f64>,
) -> Vec<SheetHardware> {
    let part = hardware_size(hardware.kind, &hardware.thread).map(|s| s.part()).unwrap_or_else(|| hardware.part.clone());
    new_circles(before, after)
        .into_iter()
        .map(|(_, _, (center, radius))| SheetHardware {
            feature,
            kind: hardware.kind,
            thread: hardware.thread.clone(),
            length,
            part: part.clone(),
            center,
            radius,
        })
        .collect()
}

impl SurfaceSheet {
    /// The press-in hardware whose mounting hole the flat still has, in the order they were cut. A
    /// later feature may have cut the hole away; then the hardware is not listed.
    pub fn hardware_holes(&self) -> Vec<&SheetHardware> {
        let circles: Vec<(Vec2, f64)> = self.regions.iter().flat_map(|r| &r.holes).filter_map(loop_circle).collect();
        self.hardware.iter().filter(|h| circles.iter().any(|c| same_circle(*c, (h.center, h.radius), SAME))).collect()
    }
}

/// How near a hole's centre is to what bounds it in the flat: the one measurement the hardware
/// notes and the shop rules check (`shop_check`) share.
pub(crate) struct Clearance {
    /// The least distance to an edge, a cut-out or another hole of the flat (mm).
    pub edge: f64,
    /// For each bend of the flat, in order.
    pub bends: Vec<BendClearance>,
    /// A free-form edge was left unmeasured.
    pub free_form: bool,
}

/// How near a hole's centre is to one bend.
pub(crate) struct BendClearance {
    /// The bend's inside radius (mm).
    pub radius: f64,
    /// The centre is inside the bend's zone, however wide the zone: its distance to the zone's
    /// boundary says nothing of it (a wide bend's middle is farther from both sides than the minimum).
    pub inside: bool,
    /// The distance to where the bend starts, the boundary of its zone, where the flat sheet ends
    /// (mm); 0 for a centre inside the zone.
    pub from_start: f64,
    /// The distance to the bend's centre line (mm); infinite when it cannot be measured.
    pub from_line: f64,
}

/// The clearance of the hole `hole`, of `region`'s flat in `sheet`, whose centre is `centre`.
pub(crate) fn clearance(sheet: &SurfaceSheet, region: &Region2, hole: &Loop2, centre: Vec2) -> Clearance {
    let (edge, mut free_form) = nearest(centre, region.loops().filter(|l| !std::ptr::eq(*l, hole)));
    let mut bends = Vec::new();
    for b in &sheet.bends {
        let (from_start, skipped) = nearest(centre, b.zone.loops());
        free_form |= skipped;
        let inside = loop_holds(&b.zone.outer, centre) == Some(true);
        let from_line = segment_distance(&Seg2::Line { a: b.start, b: b.end }, centre).unwrap_or(f64::INFINITY);
        bends.push(BendClearance { radius: b.radius, inside, from_start: if inside { 0.0 } else { from_start }, from_line });
    }
    Clearance { edge, bends, free_form }
}

fn distances(before: Option<&SurfaceSheet>, after: &SurfaceSheet, size: &HardwareSize, what: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (region, hole, c) in new_circles(before, after) {
        // Seen as the flat pattern is read: the flat's y runs the other way.
        let at = format!("({}, {})", mm(c.0.x), mm(-c.0.y));
        let near = clearance(after, region, hole, c.0);
        let bend = near.bends.iter().map(|b| b.from_start).fold(f64::INFINITY, f64::min);
        if near.edge + 1e-9 < size.edge {
            out.push(format!(
                "The hole at {at} is {} mm from the nearest edge, cut-out or other hole: {what} needs {} mm",
                mm(near.edge),
                mm(size.edge)
            ));
        }
        // A centre inside a bend's zone is in the bend, however wide the zone.
        if near.bends.iter().any(|b| b.inside) {
            out.push(format!("The hole at {at} is in a bend: {what} needs its centre {} mm from where a bend starts", mm(size.edge)));
        } else if bend + 1e-9 < size.edge {
            out.push(format!("The hole at {at} is {} mm from a bend (to where the bend starts): {what} needs {} mm", mm(bend), mm(size.edge)));
        }
        if near.free_form {
            out.push(format!("The hole at {at}: its distance to a free-form edge was not checked"));
        }
    }
    out
}
