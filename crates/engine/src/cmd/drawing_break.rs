//! Read-only W36 geometry preview, independent of the pending persistent view/session APIs.
use serde_json::Value;
use solidcraft_geom::Vec2;
use solidcraft_geom::drawing_break::{Band, BreakMap, MAX_BREAKS};
use solidcraft_geom::drawing_view::Scale;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_break::{BreakBand, PreviewParams, PreviewResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const ID: &str = "drawing.break.preview";
const CONTRACT: TypedContract<PreviewParams> = TypedContract::new();
const MAX_LINES: usize = 2000;
const MAX_PIECES: usize = 20_000;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ID, "Preview View Breaks", run)
        .icon("inspect")
        .contract(&CONTRACT)
        .output(output_schema_for::<PreviewResult>)
        .noundo()
        .doc_only()
        .params("angle_rad: line angle; scale: paper:model; breaks: [{start,end,gap_mm}] in original view-plane mm with paper-mm gaps; lines?: [[[x,y],[x,y]]]; paper_points?: [[x,y]] → clipped straight lines, paper gaps and inverse picks (null inside gaps); read-only preview, no drawing objects"),
];

fn run(session: &mut Session, value: &Value) -> Result<Value> {
    // Refuse oversized arrays before the typed parse clones any input.
    for (name, cap) in [("breaks", MAX_BREAKS), ("lines", MAX_LINES), ("paper_points", MAX_LINES)] {
        if value.get(name).and_then(Value::as_array).is_some_and(|v| v.len() > cap) {
            return Err(bad(ID, format!("{name} exceeds its {cap} record limit")));
        }
    }
    if value.get("scale").and_then(Value::as_str).is_some_and(|s| s.len() > 64) {
        return Err(bad(ID, "scale text exceeds 64 bytes"));
    }
    // The bound applies in strict/warn schema modes too, before their typed parse.
    let checked: super::Run = crate::checked!("drawing.break.preview", CONTRACT, preview);
    checked(session, value)
}

fn preview(_session: &mut Session, value: &Value) -> Result<Value> {
    let p: PreviewParams = serde_json::from_value(value.clone()).map_err(|e| bad(ID, e.to_string()))?;
    let scale: Scale = p.scale.parse().map_err(|e: solidcraft_geom::drawing_view::Invalid| bad(ID, e.to_string()))?;
    let bands: Vec<_> = p.breaks.iter().map(|b| Band { start: b.start, end: b.end, gap_mm: b.gap_mm }).collect();
    let map = BreakMap::new(p.angle_rad, scale, &bands).map_err(|e| bad(ID, e.to_string()))?;
    let mut lines = Vec::new();
    for [a, b] in p.lines {
        for [a, b] in map.line(Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1])).map_err(|e| bad(ID, e.to_string()))? {
            if lines.len() == MAX_PIECES {
                return Err(bad(ID, "more than 20,000 output pieces"));
            }
            lines.push([[a.x, a.y], [b.x, b.y]]);
        }
    }
    let model_points = p
        .paper_points
        .into_iter()
        .map(|p| map.to_model(Vec2::new(p[0], p[1])).map(|p| p.map(|p| [p.x, p.y])).map_err(|e| bad(ID, e.to_string())))
        .collect::<Result<Vec<_>>>()?;
    let n = map.normal();
    let reply = PreviewResult {
        normal: [n.x, n.y],
        shortening_mm: map.shortening_mm(),
        breaks: map.bands().iter().map(|b| BreakBand { start: b.start, end: b.end, gap_mm: b.gap_mm }).collect(),
        gaps: map.paper_gaps(),
        lines,
        model_points,
    };
    serde_json::to_value(reply).map_err(|e| bad(ID, e.to_string()))
}

#[cfg(test)]
#[path = "drawing_break_tests.rs"]
mod tests;
