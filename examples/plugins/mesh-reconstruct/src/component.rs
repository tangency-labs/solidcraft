//! Generated canonical ABI bindings are isolated from the reconstruction code.
mod bindings {
    wit_bindgen::generate!({ path: "wit", world: "command-plugin" });
}
use super::{Host, MAX_JSON_BYTES, execute};
use bindings::exports::solidcraft::plugin::command::Guest;
use bindings::solidcraft::plugin::{engine, types::PluginError};
struct EngineHost;
impl Host for EngineHost {
    fn run(&mut self, command: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let args = serde_json::to_string(&params).map_err(|e| e.to_string())?;
        if args.len() > MAX_JSON_BYTES {
            return Err("command exceeds 1 MiB".into());
        }
        let reply = engine::run(command, &args).map_err(|e| format!("{e:?}"))?;
        if reply.len() > MAX_JSON_BYTES {
            return Err("reply exceeds 1 MiB".into());
        }
        serde_json::from_str(&reply).map_err(|e| e.to_string())
    }
}
struct MeshReconstruct;
impl Guest for MeshReconstruct {
    fn execute(entry: String, params: String) -> Result<String, PluginError> {
        if params.len() > MAX_JSON_BYTES {
            return Err(PluginError::Limit("parameters exceed 1 MiB".into()));
        }
        let value = serde_json::from_str(&params).map_err(|e| PluginError::Invalid(e.to_string()))?;
        let value = execute(&mut EngineHost, &entry, value).map_err(PluginError::Failed)?;
        serde_json::to_string(&value).map_err(|e| PluginError::Failed(e.to_string()))
    }
}
bindings::export!(MeshReconstruct with_types_in bindings);
