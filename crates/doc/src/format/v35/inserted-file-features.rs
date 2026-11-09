//! The feature of a file inserted into a design (STEP, IGES or a mesh file) is marked `inserted`:
//! it made the component tree at whose top it sits, and deleting it removes that tree with it.
//! Older designs are rewritten as they open: an import or mesh import inside a component is
//! marked, except the independent base of a copied component, which goes alone as before. The
//! last release refuses a design saved with this (the newer format number).
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        let Some(object) = feature.as_object_mut() else { continue };
        let in_component = object.get("component").and_then(Value::as_u64).is_some_and(|c| c != 0);
        let inserted = match object.get("type").and_then(Value::as_str) {
            Some("mesh_import") => true,
            // A copied component's independent base: a body re-exported on its own, no file tree.
            Some("import") => {
                let base = object.get("file").and_then(Value::as_str).is_some_and(|f| f.ends_with(" independent base"))
                    && object.get("components").and_then(Value::as_array).is_none_or(Vec::is_empty);
                !base
            }
            _ => false,
        };
        if in_component && inserted {
            object.insert("inserted".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn older_inserted_files_are_marked_but_copied_bases_are_not() {
        let mut v = json!({"features": [
            {"type": "import", "file": "parts.step", "component": 2},
            {"type": "mesh_import", "file": "bracket.stl", "component": 5},
            {"type": "import", "file": "Block independent base", "component": 7},
            {"type": "import", "file": "opened.step"},
            {"type": "extrude", "component": 2}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["inserted"] == json!(true)).collect();
        assert_eq!(marked, [true, true, false, false, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
