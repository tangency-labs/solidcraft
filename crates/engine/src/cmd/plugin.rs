//! One-shot plugins share the engine's command surface, file scope and undo history.
use super::CommandSpec;
use crate::schema::plugin::{InspectParams, InspectResult, MeshParams, MeshResult, RunParams, RunResult};
use crate::{
    Result, Session, checked,
    params::bad,
    plugin,
    schema::{TypedContract, output_schema_for},
};
use serde_json::Value;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("model.mesh", "Read Body Mesh", checked!("model.mesh", mesh)).icon("inspect").noundo()
        .contract(&TypedContract::<MeshParams>::new()).output(output_schema_for::<MeshResult>)
        .params("body, tolerance?: finite positive chord tolerance in mm (default 0.05); evaluated component-local mesh, at most 8192 vertices and 16384 triangles; no files or model changes"),
    CommandSpec::new("plugin.inspect", "Inspect Plugin", checked!("plugin.inspect", inspect)).icon("inspect").noundo().doc_only()
        .contract(&TypedContract::<InspectParams>::new()).output(output_schema_for::<InspectResult>)
        .params("path: plugin package folder; returns inert manifest and approval hash covering manifest and component; runs no plugin code"),
    CommandSpec::new("plugin.run", "Run Plugin", checked!("plugin.run", run)).icon("play")
        .contract(&TypedContract::<RunParams>::new()).output(output_schema_for::<RunResult>)
        .params("path, approval: exact hash from plugin.inspect after reviewing permissions, entry, params?: JSON; explicit native Wasm component execution, one undo step; errors discard all edits"),
];
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let p: InspectParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.inspect", e.to_string()))?;
    let package = plugin::read_package(&p.path)?;
    let limits_error = package.manifest.runtime.as_ref().and_then(|r| s.plugin_limits.check(&r.limits).err()).map(|e| e.to_string());
    serde_json::to_value(InspectResult {
        manifest: package.manifest,
        approval: package.approval,
        host_limits: s.plugin_limits,
        within_host_limits: limits_error.is_none(),
        limits_error,
        param_warnings: vec![],
    })
    .map_err(|e| bad("plugin.inspect", e.to_string()))
}
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let p: RunParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.run", e.to_string()))?;
    let package = plugin::read_package(&p.path)?;
    if p.approval != package.approval {
        return Err(bad("plugin.run", "approval does not match this manifest and component; inspect and review again"));
    }
    let result = plugin::run(s, package, &p.entry, &p.params)?;
    serde_json::to_value(RunResult { result, param_warnings: vec![] }).map_err(|e| bad("plugin.run", e.to_string()))
}

fn mesh(s: &mut Session, p: &Value) -> Result<Value> {
    let p: MeshParams = serde_json::from_value(p.clone()).map_err(|e| bad("model.mesh", e.to_string()))?;
    if !p.tolerance.is_finite() || p.tolerance <= 0. {
        return Err(bad("model.mesh", "tolerance must be finite and positive"));
    }
    let state = s.model.state();
    let body = state.body(&p.body).ok_or_else(|| bad("model.mesh", "unknown body"))?;
    let mesh = body.body.tessellate(p.tolerance)?;
    if mesh.positions.len() > 8192 || mesh.triangles.len() > 16384 {
        return Err(bad("model.mesh", "mesh exceeds the plugin transfer budget"));
    }
    let component = body.component;
    let result = MeshResult {
        body: p.body,
        component,
        frame: "component-local-mm".into(),
        positions: mesh.positions.iter().map(|v| [v.x, v.y, v.z]).collect(),
        triangles: mesh.triangles.to_vec(),
        param_warnings: vec![],
    };
    serde_json::to_value(result).map_err(|e| bad("model.mesh", e.to_string()))
}
