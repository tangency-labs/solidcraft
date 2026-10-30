//! Original, separately compiled command plugin: a native box, not a host-bundled feature.
mod bindings {
    wit_bindgen::generate!({ path: "../../../wit", world: "command-plugin" });
}
use bindings::exports::solidcraft::plugin::command::Guest;
use bindings::solidcraft::plugin::{engine, types::PluginError};
struct ExampleBox;
impl Guest for ExampleBox {
    fn execute(entry: String, params: String) -> Result<String, PluginError> {
        if entry != "make" {
            return Err(PluginError::Invalid("unknown entry".into()));
        }
        let value: serde_json::Value = serde_json::from_str(&params).map_err(|e| PluginError::Invalid(e.to_string()))?;
        for key in ["length", "width", "height"] {
            if !value.get(key).and_then(serde_json::Value::as_f64).is_some_and(|n| n.is_finite() && n > 0. && n <= 10000.) {
                return Err(PluginError::Invalid(format!("{key} must be 0 < millimetres <= 10000")));
            }
        }
        engine::run("solid.box", &params)
    }
}
bindings::export!(ExampleBox with_types_in bindings);
