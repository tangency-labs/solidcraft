//! Global configuration writes are serialized across editor tabs and control scripts.
use crate::SolveApp;
use serde_json::Value;

fn pending(app: &mut SolveApp) -> bool {
    app.session.cloud_configuring()
}
fn changes(id: &str, params: &Value) -> bool {
    id == "cloud.configure" || (id == "cloud.settings" && params.get("reload").and_then(Value::as_bool) == Some(true))
}
pub(crate) fn refusal(app: &mut SolveApp, id: &str, params: &Value) -> Option<&'static str> {
    (changes(id, params) && pending(app)).then_some("connection settings are still being stored or loaded")
}
pub(crate) fn script_refusal(app: &mut SolveApp, script: &Value) -> Option<&'static str> {
    if !pending(app) {
        return None;
    }
    let steps = script.as_array().or_else(|| script.get("commands").and_then(Value::as_array))?;
    steps.iter().find_map(|step| {
        let id = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)?;
        changes(id, step.get("params").unwrap_or(&Value::Null)).then_some("connection settings are still being stored or loaded")
    })
}
