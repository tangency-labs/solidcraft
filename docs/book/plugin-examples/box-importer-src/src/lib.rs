wit_bindgen::generate!({ path: "wit", world: "importer-plugin" });

use craft::plugin::types::{FieldFailure, PluginError};
use craft::plugin::{design, source};
use exports::craft::plugin::importer::Guest;
use serde_json::json;

struct Importer;

impl Guest for Importer {
    fn import_file(_entry: String, file: source::FileInfo, _options: String) -> Result<String, PluginError> {
        // The path "" is the file being opened.
        let mut bytes = Vec::new();
        while (bytes.len() as u64) < file.size {
            let chunk = source::read("", bytes.len() as u64, 1 << 20)?;
            if chunk.is_empty() {
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(bytes).map_err(|e| invalid("file", e))?;
        let numbers: Vec<f64> = text.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|e| invalid("file", e))?;
        let [length, width, height] = numbers[..] else {
            return Err(invalid("file", "a .box file holds three numbers: length, width and height"));
        };
        let made = json!({
            "format": "solidcraft/57", "name": "", "units": "mm", "params": [],
            "features": [{"id": 1, "name": "Box1", "suppressed": false, "body_names": ["Body1"],
                "param_names": ["d1", "d2", "d3"], "type": "box", "corner": [0.0, 0.0, 0.0],
                "length": length.to_string(), "width": width.to_string(), "height": height.to_string(),
                "operation": "new_body"}],
            "next_id": 2, "reserved_body_names": ["Body1"], "next_component_color_index": 0
        });
        for chunk in made.to_string().as_bytes().chunks(1 << 20) {
            design::write(chunk)?;
        }
        // The loss report: what could not be carried over. A box loses nothing.
        Ok(json!({"summary": format!("A {length} x {width} x {height} mm box"), "items": []}).to_string())
    }
}

fn invalid(field: &str, reason: impl ToString) -> PluginError {
    PluginError::InvalidInput(FieldFailure { field: field.into(), reason: reason.to_string() })
}

export!(Importer);
