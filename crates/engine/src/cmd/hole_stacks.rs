//! Hole stacks: the places a rivet, bolt or pin can pass through two or more touching bodies
//! (`inspect.hole_stacks`). Each body's bores — its round holes and obround slots, with the
//! wall extents a countersink or chamfer extends — and the stacks across them come from the
//! kernel through [`solidcraft_kernel::services::hole_stacks`], read from the exact faces of the bodies'
//! records; this is the command layer: names, parameters, the standard-parts exclusion and
//! the caps.

use serde_json::Value;

use super::CommandSpec;
use crate::params::{bad, bool_, num, string_list};
use crate::schema::hole_stacks::{
    HoleStacks, HoleStacksParams, Layer as LayerOut, LoneHole, MAX_AXIS_TOLERANCE, MAX_RANGES, MAX_TOUCHING, Slot as SlotOut, Stack as StackOut,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<HoleStacksParams> = TypedContract::new();

/// Bodies one call looks at, at most.
const MAX_BODIES: usize = 10_000;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.hole_stacks", "Find Hole Stacks", crate::checked!("inspect.hole_stacks", hole_stacks))
    .icon("measure")
    .noundo()
    .read_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<HoleStacks>)
    .params(
        "bodies?: [names] (default all but standard parts); diameters?: [[min, max]] (mm; holes and slot widths in any range); slots?: bool (default true: obround slots count); \
         touching?: mm (largest gap between layers, default 0.05); axis_tolerance?: mm (largest offset between hole axes, default 0.1) \
         → stacks: [{axis (unit, from the start into the stack: −z for a vertical stack), start, end: [x,y,z] on the axis, grip_mm, \
         layers: [{body, d_mm, t: [t0, t1] (mm from start), slot?: {length_mm, along}}]}] (two or more bodies each); \
         lone: [{body, d_mm, at, axis, depth_mm, slot?}] (holes in no stack). Standard parts' bodies count only when named in `bodies`",
    )];

/// What the call asks for.
struct Options {
    bodies: Vec<String>,
    diameters: Vec<(f64, f64)>,
    slots: bool,
    touching: f64,
    axis_tolerance: f64,
}

impl Options {
    fn parse(cmd: &str, p: &Value) -> Result<Self> {
        let mm = |k: &str, default: f64, max: f64| -> Result<f64> {
            match p.get(k) {
                None | Some(Value::Null) => Ok(default),
                Some(_) => num(p, k)
                    .filter(|x| x.is_finite() && *x >= 0.0 && *x <= max)
                    .ok_or_else(|| bad(cmd, format!("`{k}` must be a number from 0 to {max}"))),
            }
        };
        let mut diameters = Vec::new();
        match p.get("diameters") {
            None | Some(Value::Null) => {}
            Some(Value::Array(ranges)) if ranges.len() <= MAX_RANGES => {
                for r in ranges {
                    let pair = r.as_array().filter(|a| a.len() == 2).map(|a| (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)));
                    match pair {
                        Some((Some(lo), Some(hi))) if lo.is_finite() && hi.is_finite() && lo <= hi => diameters.push((lo, hi)),
                        _ => return Err(bad(cmd, "`diameters` must be [[min, max], …] in mm, min ≤ max")),
                    }
                }
            }
            Some(_) => return Err(bad(cmd, format!("`diameters` must be a list of at most {MAX_RANGES} [min, max] ranges"))),
        }
        let slots = match p.get("slots") {
            None | Some(Value::Null) => true,
            Some(_) => bool_(p, "slots").ok_or_else(|| bad(cmd, "`slots` must be true or false"))?,
        };
        Ok(Self {
            bodies: string_list(cmd, p, "bodies")?,
            diameters,
            slots,
            touching: mm("touching", 0.05, MAX_TOUCHING)?,
            axis_tolerance: mm("axis_tolerance", 0.1, MAX_AXIS_TOLERANCE)?,
        })
    }
}

fn hole_stacks(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.hole_stacks";
    let o = Options::parse(cmd, p)?;
    let st = s.world_state()?;
    for name in &o.bodies {
        if st.body(name).is_none() {
            return Err(bad(cmd, format!("no body `{name}`")));
        }
    }
    // Standard parts already placed (a washer's or nut's hole) are not part of the assembly's
    // stacks unless asked for by name.
    let mut names: Vec<String> = Vec::new();
    let mut records: Vec<&solidcore_record::brep::Brep> = Vec::new();
    for b in &st.bodies {
        let take = if o.bodies.is_empty() { !s.doc.parts.contains_key(&s.doc.model_body_component(b)) } else { o.bodies.contains(&b.name) };
        if take {
            records.push(b.body.record());
            names.push(b.name.clone());
        }
    }
    if records.len() > MAX_BODIES {
        return Err(bad(cmd, format!("at most {MAX_BODIES} bodies (pass `bodies`)")));
    }
    let out = solidcraft_kernel::services::hole_stacks::hole_stacks(
        &records,
        &solidcraft_kernel::services::hole_stacks::Options {
            touching: o.touching,
            axis_tolerance: o.axis_tolerance,
            slots: o.slots,
            diameters: o.diameters,
        },
    )
    .map_err(|e| bad(cmd, e.to_string()))?;
    let name = |i: usize| names.get(i).map(String::as_str).unwrap_or_default();
    // Millimetres to a nanometre (and no negative zero): readable output, far finer than any fit.
    let round = |x: f64| (x * 1e6).round() / 1e6 + 0.0;
    let round3 = |v: solidcore_record::Vec3| [round(v.x), round(v.y), round(v.z)];
    let slot = |s: solidcraft_kernel::services::hole_stacks::Slot| SlotOut { length_mm: round(s.length), along: round3(s.along) };
    let stacks = out
        .stacks
        .iter()
        .map(|st| StackOut {
            axis: round3(st.axis),
            start: round3(st.start),
            end: round3(st.end),
            grip_mm: round(st.grip),
            layers: st
                .layers
                .iter()
                .map(|l| LayerOut { body: name(l.body).to_owned(), d_mm: round(l.d), t: [round(l.t[0]), round(l.t[1])], slot: l.slot.map(slot) })
                .collect(),
        })
        .collect();
    let lone = out
        .lone
        .iter()
        .map(|h| LoneHole {
            body: name(h.body).to_owned(),
            d_mm: round(h.d),
            at: round3(h.at),
            axis: round3(h.axis),
            depth_mm: round(h.depth),
            slot: h.slot.map(slot),
        })
        .collect();
    serde_json::to_value(HoleStacks { stacks, lone }).map_err(|e| bad(cmd, e.to_string()))
}

#[cfg(test)]
#[path = "hole_stacks_tests.rs"]
mod tests;
