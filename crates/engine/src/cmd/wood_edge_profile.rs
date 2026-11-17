//! Woodworking router profiles, authored as one editable timeline feature.

use serde_json::{Value, json};
use solidcraft_doc::{BodyName, FeatureKind, Position, expr::Kind, kernel};

use super::{CommandSpec, not_in_sketch};
use crate::params::{bad, expr, str_};
use crate::schema::wood_edge_profile::{EdgeProfileParams, EdgeProfileResult, RoutedEdge};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<EdgeProfileParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("wood.edge_profile", "Edge Profile", crate::checked!("wood.edge_profile", edge_profile))
        .at("SOLID", "MODIFY")
        .icon("chamfer")
        .enabled(not_in_sketch)
        .contract(&CONTRACT)
        .output(output_schema_for::<EdgeProfileResult>)
        .params("edges: 1..256 edge picks of one body; profile: chamfer|bevel|roundover|cove|rabbet|ogee|bead|none; radius?: 6 mm; width?: 3 mm (chamfer/bevel), 12 mm (rabbet/ogee); depth?: 12 mm; angle?: 45 deg (bevel); step?: 1 mm (bead); fillet?: 0 mm (roundover top step); tangent_chain?: false; body?; feature?: existing router feature id or name (edit the same feature; omitted edges keep its picks). All profiles require a convex 90-degree corner; paths are straight edges or planar circular arcs. Returns each resolved edge's length_mm on the unmodified body."),
];

fn edge_profile(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "wood.edge_profile";
    let params_error = |e: EngineError| match e {
        EngineError::Internal(..) => e,
        other => bad(cmd, other.to_string()),
    };
    let profile = str_(p, "profile").ok_or_else(|| bad(cmd, "`profile` is required"))?;
    let existing = p.get("feature").map(|f| super::edit::feature_id(s, Some(f), cmd)).transpose()?;
    if let Some(id) = existing {
        if !matches!(s.doc.feature(id).map(|f| &f.kind), Some(FeatureKind::WoodEdgeProfile { .. })) {
            return Err(bad(cmd, "`feature` must name a router edge profile"));
        }
        if p.get("edges").is_some() || p.get("body").is_some() {
            return Err(bad(cmd, "change edge picks with Edit Feature; omit edges and body to change the section"));
        }
    }
    let mut set = json!({"profile": profile});
    for (key, kind) in [
        ("radius", Kind::Length),
        ("width", Kind::Length),
        ("depth", Kind::Length),
        ("angle", Kind::Angle),
        ("step", Kind::Length),
        ("fillet", Kind::Length),
    ] {
        if let Some(e) = expr(p, key) {
            super::features::check_expr(s, &e, kind, cmd, key)?;
            set[key] = Value::String(e.to_string());
        }
    }
    if let Some(chain) = p.get("tangent_chain") {
        set["tangent_chain"] = chain.clone();
    }
    let id = if let Some(id) = existing {
        super::edit::edit_feature(s, &json!({"feature": id, "set": set})).map_err(params_error)?;
        id
    } else {
        if p.get("edges").and_then(Value::as_array).is_none_or(|v| v.is_empty() || v.len() > 256) {
            return Err(bad(cmd, "`edges` must contain 1 to 256 picks of one body"));
        }
        let st = s.model.state();
        let mut owner = None;
        let mut points = Vec::new();
        let mut picked = Vec::new();
        let given = str_(p, "body");
        let edges = p.get("edges").and_then(Value::as_array).ok_or_else(|| bad(cmd, "router edges missing"))?;
        for edge in edges {
            let own = edge.get("body").and_then(Value::as_str);
            if given.zip(own).is_some_and(|(a, b)| a != b) {
                return Err(bad(cmd, "an edge names a different body from body"));
            }
            let (point, only) = match super::features::edge_ref_form(edge).map_err(|why| bad(cmd, why))? {
                super::features::EdgeRefForm::Point(point) => (point, given.or(own)),
                super::features::EdgeRefForm::Index { body, index } => {
                    let index = usize::try_from(index).map_err(|_| bad(cmd, "router edge index is too large"))?;
                    let target = st.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
                    let point = kernel::routed_edge_point(&target.body, index).map_err(EngineError::from).map_err(params_error)?;
                    (point, Some(body))
                }
            };
            let (body, hit) = solidcraft_doc::wood_edge_profile::pick(&st, only, point).map_err(EngineError::from).map_err(params_error)?;
            if owner.is_some_and(|i| i != body) {
                return Err(bad(cmd, "router edges must belong to one body"));
            }
            owner = Some(body);
            if !picked.contains(&hit.edge) {
                picked.push(hit.edge);
                points.push(Position(hit.point));
            }
        }
        let body = owner.and_then(|i| st.bodies.get(i)).ok_or_else(|| bad(cmd, "router body missing"))?.name.clone();
        let size = |key: &str, default: &str| -> solidcraft_doc::Expr { set.get(key).and_then(Value::as_str).unwrap_or(default).into() };
        let kind = FeatureKind::WoodEdgeProfile {
            edges: points,
            body: Some(BodyName::from(body)),
            profile: profile.to_string(),
            radius: size("radius", "6 mm"),
            width: size("width", if matches!(profile, "chamfer" | "bevel") { "3 mm" } else { "12 mm" }),
            depth: size("depth", "12 mm"),
            angle: size("angle", "45 deg"),
            step: size("step", "1 mm"),
            fillet: size("fillet", "0 mm"),
            tangent_chain: p.get("tangent_chain").and_then(Value::as_bool).unwrap_or(false),
        };
        super::features::add_feature(s, p, kind)
            .map_err(params_error)?
            .get("feature")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad(cmd, "router feature was not created"))?
    };
    let st = s.model.state();
    let lengths = st.routed_lengths.get(&id).ok_or_else(|| bad(cmd, "router lengths are missing"))?;
    let reply =
        EdgeProfileResult { feature: id, edges: lengths.iter().enumerate().map(|(edge, &length_mm)| RoutedEdge { edge, length_mm }).collect() };
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

#[cfg(test)]
#[path = "wood_edge_profile_tests.rs"]
mod tests;
