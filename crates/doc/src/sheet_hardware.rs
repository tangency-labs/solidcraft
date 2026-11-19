//! Whether a sheet holds the press-in hardware a Hole is the mounting hole of (#17752): the sheet
//! as thick as the hardware needs, and the hole's centre far enough from an edge, a cut-out or a
//! bend. Each finding is a warning on the feature, never an error: a hole is still a valid hole,
//! and a change of gauge or of a bend must not break the timeline. (`sheet.hardware` itself
//! refuses a sheet too thin when it is run.) Distances are exact, from the centre to the lines and
//! arcs of the flat, which is where a sheet's edges and bends are measured: the flat's outline and
//! cut-outs for an edge, and the boundary of a bend's zone, where the flat sheet ends and the bend
//! starts, for a bend (docs/hardware-catalogue.md, "Near a bend").

use solidcore_ops::prism::{loop_circle, loop_holds, same_circle, segment_distance};
use solidcore_record::{Loop2, Vec2};

use crate::hardware_catalog::{HardwareSize, hardware_size};
use crate::{Hardware, SurfaceSheet};

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
fn mm(v: f64) -> String {
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

fn distances(before: Option<&SurfaceSheet>, after: &SurfaceSheet, size: &HardwareSize, what: &str) -> Vec<String> {
    let known: Vec<(Vec2, f64)> = before.map(|b| b.regions.iter().flat_map(|r| &r.holes).filter_map(loop_circle).collect()).unwrap_or_default();
    let new = |c: &(Vec2, f64)| !known.iter().any(|k| same_circle(*k, *c, SAME));
    let mut out = Vec::new();
    for region in &after.regions {
        for hole in &region.holes {
            let Some(c) = loop_circle(hole).filter(new) else { continue };
            // Seen as the flat pattern is read: the flat's y runs the other way.
            let at = format!("({}, {})", mm(c.0.x), mm(-c.0.y));
            let (edge, free_form) = nearest(c.0, region.loops().filter(|l| !std::ptr::eq(*l, hole)));
            let (bend, bend_free_form) = nearest(c.0, after.bends.iter().flat_map(|b| b.zone.loops()));
            if edge + 1e-9 < size.edge {
                out.push(format!(
                    "The hole at {at} is {} mm from the nearest edge, cut-out or other hole: {what} needs {} mm",
                    mm(edge),
                    mm(size.edge)
                ));
            }
            // A centre inside a bend's zone is in the bend, however wide the zone: its distance to the
            // zone's boundary says nothing of it (a wide bend's middle is farther from both sides than the minimum).
            if after.bends.iter().any(|b| loop_holds(&b.zone.outer, c.0) == Some(true)) {
                out.push(format!("The hole at {at} is in a bend: {what} needs its centre {} mm from where a bend starts", mm(size.edge)));
            } else if bend + 1e-9 < size.edge {
                out.push(format!("The hole at {at} is {} mm from a bend (to where the bend starts): {what} needs {} mm", mm(bend), mm(size.edge)));
            }
            if free_form || bend_free_form {
                out.push(format!("The hole at {at}: its distance to a free-form edge was not checked"));
            }
        }
    }
    out
}
