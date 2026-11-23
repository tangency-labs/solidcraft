//! Parametric reconstruction of imported mechanical mesh bodies.

use serde_json::Value;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;

use super::{
    CommandSpec,
    features::{add_feature, check_expr},
};
use crate::schema::features::FeatureResult;
use crate::schema::mesh::{MeshConvertParams, MeshReduceParams, MeshReduceResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    Result, Sel, Session,
    params::{bad, expr, str_},
};

const MESH_CONVERT_CONTRACT: TypedContract<MeshConvertParams> = TypedContract::new();
const MESH_REDUCE_CONTRACT: TypedContract<MeshReduceParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mesh.convert", "Convert Mesh", crate::checked!("mesh.convert", convert)).begins_edit()
        .icon("import")
        .contract(&MESH_CONVERT_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm): rebuild a closed mesh as an editable solid — a constant-section part as planar and cylindrical faces, any other closed mesh as the faceted solid its triangles describe — with a parametric timeline feature in the mesh body's owning component (the active component must be that component or an ancestor; otherwise activate the named component first); the active component is unchanged; name?"),
    CommandSpec::new("mesh.reduce", "Reduce Mesh", crate::checked!("mesh.reduce", reduce))
        .icon("mesh_reduce")
        .contract(&MESH_REDUCE_CONTRACT)
        .output(output_schema_for::<MeshReduceResult>)
        .params("body?: mesh body name (default: selected body); exactly one of triangle_count: positive integer or percentage: retained percent strictly between 0 and 100; name?: feature name. Creates a separate mesh in the source component, preserving the original. Reports actual triangle count and warns when the target cannot be reached."),
];

/// The mesh body to convert: `body`, else the one selected body.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<String> {
    if let Some(name) = str_(p, "body").filter(|b| !b.trim().is_empty()) {
        return Ok(name.to_owned());
    }
    let selected: Vec<&str> = s.selection.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.as_str()) } else { None }).collect();
    match selected.as_slice() {
        [name] => Ok((*name).to_owned()),
        [] => Err(bad(cmd, "select a mesh body")),
        _ => Err(bad(cmd, "select one mesh body")),
    }
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.convert";
    let state = s.model.state();
    let given = target(s, p, cmd)?;
    // The selection and the Browser name a body of a further placement as the world shows it
    // (`Mesh (Scan:2)`): that name means the body it shows, which the conversion edits in its
    // component, as `appearance.assign` reads it (#9740).
    let body = s.own_body_name(&given).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    if !source.body.is_mesh() {
        return Err(bad(cmd, "select a mesh body"));
    }
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0.05 mm".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    // Conversion edits the component definition, independent of its occurrence placement.
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::MeshConvert { body: body.into(), tolerance });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    result
}

fn reduce(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.reduce";
    let given = target(s, p, cmd)?;
    let body = s.own_body_name(&given).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let state = s.model.state();
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let original = source.body.triangle_mesh().ok_or_else(|| bad(cmd, "select a mesh body"))?.1.len();
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    let count_value = p.get("triangle_count").filter(|v| !v.is_null());
    let percent_value = p.get("percentage").filter(|v| !v.is_null());
    let (triangle_count, percentage, target) = match (count_value, percent_value) {
        (Some(value), None) => {
            let count = value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| bad(cmd, "triangle_count must be a positive integer"))?;
            (Some(count), None, count)
        }
        (None, Some(value)) => {
            let percent = value
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.0 && *n < 100.0)
                .ok_or_else(|| bad(cmd, "percentage must be finite and strictly between 0 and 100"))?;
            (None, Some(percent), ((original as f64 * percent / 100.0).floor() as usize).max(1))
        }
        _ => return Err(bad(cmd, "specify exactly one of triangle_count or percentage")),
    };
    if target >= original {
        return Err(bad(cmd, format!("target {target} must be less than the original {original} triangles")));
    }
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::MeshReduce { body: body.into(), triangle_count, percentage });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    let mut result = result?;
    let name = result["created_bodies"]
        .as_array()
        .and_then(|names| names.first())
        .and_then(Value::as_str)
        .ok_or_else(|| bad(cmd, "reduction did not create a body"))?;
    let achieved = s
        .model
        .state()
        .body(name)
        .and_then(|b| b.body.triangle_mesh())
        .map(|(_, t)| t.len())
        .ok_or_else(|| bad(cmd, "reduction did not create a mesh"))?;
    result["original_triangles"] = original.into();
    result["target_triangles"] = target.into();
    result["achieved_triangles"] = achieved.into();
    Ok(result)
}
