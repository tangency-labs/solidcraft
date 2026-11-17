//! Manufacturing grain carried by a body's construction frame.

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::wood::{Grain, along_grain, edge_direction, unit};

use super::{CommandSpec, not_in_sketch};
use crate::params::bad;
use crate::schema::wood_grain::{GrainDirection, GrainGuideParams, GrainParams, GrainQueryParams, GrainResult, StockAxis};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

#[cfg(test)]
#[path = "wood_grain_tests.rs"]
mod tests;

const GRAIN: TypedContract<GrainParams> = TypedContract::new();
const QUERY: TypedContract<GrainQueryParams> = TypedContract::new();
const GUIDE: TypedContract<GrainGuideParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("wood.grain", "Wood Grain", crate::checked!("wood.grain", grain))
        .at("SOLID", "MODIFY")
        .icon("wood_grain")
        .enabled(not_in_sketch)
        .contract(&GRAIN)
        .output(output_schema_for::<GrainResult>)
        .params("body: name; direction: length|width|thickness|none, [x,y,z] (world direction), or {edge: index, body?: name, face?: planar face index}. Edge indices are model.edges indices on that body. Sets body-local manufacturing grain; none clears it, including a member's default. Returns {body, direction: world unit vector|null, stock_length: mm|null}"),
    CommandSpec::new("wood.grain.query", "Query Wood Grain", crate::checked!("wood.grain.query", query))
        .icon("measure")
        .noundo()
        .contract(&QUERY)
        .output(output_schema_for::<GrainResult>)
        .params("body: name → {body, direction: world unit vector|null, stock_length: mm|null}. Read-only. An unset frame member follows its member axis; other unset bodies have no grain"),
    CommandSpec::new("view.grain", "Grain Guides", crate::checked!("view.grain", guide))
        .icon("style")
        .noundo()
        .doc_only()
        .contract(&GUIDE)
        .output(output_schema_for::<GrainGuideParams>)
        .params("on?: bool (omitted: toggle) → {on?}; show manufacturing grain guides on body faces. Off by default; viewport state only"),
];

fn query(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "wood.grain.query";
    let p: GrainQueryParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    reply(s, cmd, &p.body)
}

fn reply(s: &Session, cmd: &str, name: &str) -> Result<Value> {
    let state = s.world_state()?;
    let body = state.body(name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
    let direction = s.doc.grain_direction(body).map_err(|e| bad(cmd, e.to_string()))?;
    let length = direction.map(|d| along_grain(&body.body, d)).transpose().map_err(|e| bad(cmd, e.to_string()))?;
    serde_json::to_value(GrainResult { body: name.to_string(), direction: direction.map(|v| [v.x, v.y, v.z]), stock_length: length })
        .map_err(|e| bad(cmd, e.to_string()))
}

fn grain(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "wood.grain";
    let p: GrainParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let state = s.world_state()?;
    let body = state.body(&p.body).ok_or_else(|| bad(cmd, format!("no body `{}`", p.body)))?;
    if body.body.is_mesh() {
        return Err(bad(cmd, "manufacturing grain requires a B-rep body"));
    }
    let direction = match p.direction {
        GrainDirection::Axis(StockAxis::None) => None,
        GrainDirection::Axis(axis) => {
            let [length, width, thickness] = crate::kernel::stock_box(&body.body).map_err(|e| bad(cmd, e.to_string()))?.axes.map(Vec3::from);
            Some(match axis {
                StockAxis::Length => length,
                StockAxis::Width => width,
                StockAxis::Thickness => thickness,
                StockAxis::None => return Err(bad(cmd, "invalid stock axis")),
            })
        }
        GrainDirection::Vector(vector) => Some(unit(Vec3::from(vector)).map_err(|e| bad(cmd, e.to_string()))?),
        GrainDirection::Edge(edge) => {
            if edge.body.as_deref().is_some_and(|name| name != p.body) {
                return Err(bad(cmd, "the edge is not on this body"));
            }
            Some(edge_direction(&body.body, edge.edge, edge.face).map_err(|e| bad(cmd, e.to_string()))?)
        }
    };
    // Check that this direction can be measured before editing the document.
    if let Some(direction) = direction {
        along_grain(&body.body, direction).map_err(|e| bad(cmd, e.to_string()))?;
    }
    let frame = body.grain_frame.as_deref().cloned().unwrap_or_default();
    let inverse = solidcraft_doc::mat_inverse(&frame.matrix).ok_or_else(|| bad(cmd, "the body's grain frame is singular"))?;
    let local = direction.map(|d| solidcraft_doc::apply_vector(&inverse, d)).map(unit).transpose().map_err(|e| bad(cmd, e.to_string()))?;
    let key = body.definition_name().to_string();
    s.doc_mut().wood_grain.insert(key, Grain { direction: local.map(|v| [v.x, v.y, v.z]) });
    reply(s, cmd, &p.body)
}

fn guide(_: &mut Session, p: &Value) -> Result<Value> {
    let p: GrainGuideParams = serde_json::from_value(p.clone()).map_err(|e| bad("view.grain", e.to_string()))?;
    serde_json::to_value(p).map_err(|e| bad("view.grain", e.to_string()))
}
