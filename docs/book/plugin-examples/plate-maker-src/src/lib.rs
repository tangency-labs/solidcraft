wit_bindgen::generate!({ path: "wit", world: "command-plugin" });

use craft::plugin::commands;
use craft::plugin::types::{FieldFailure, PluginError};
use exports::craft::plugin::entry::Guest;
use serde_json::{Value, json};

struct Plugin;

impl Guest for Plugin {
    fn execute(entry: String, params: String) -> Result<String, PluginError> {
        let params: Value = serde_json::from_str(&params).map_err(|e| invalid("params", e))?;
        match entry.as_str() {
            "plate" => {
                let size = params["size"].as_f64().unwrap_or(40.0);
                let thickness = params["thickness"].as_f64().unwrap_or(3.0);
                let args = json!({"length": size, "width": size, "height": thickness});
                // The same command, parameters and reply as `solidcraft-cli exec solid.box`.
                commands::run("solid.box", &args.to_string())?;
                Ok(json!({"plate": [size, size, thickness]}).to_string())
            }
            _ => Err(invalid("entry", format!("no entry `{entry}`"))),
        }
    }
}

fn invalid(field: &str, reason: impl ToString) -> PluginError {
    PluginError::InvalidInput(FieldFailure { field: field.into(), reason: reason.to_string() })
}

export!(Plugin);
