//! A Join extrude To Object may save `to_body_apart: true` in its `extent`: the body it runs to
//! stays its own body and only the Join's targets take the tool. Since 0.24.1 a Join To Object
//! also joins the body it runs to (#6424); designs saved by 0.24.0 and earlier (format 39 and
//! older) were built without that, so their later features name the body it would swallow and
//! the faces the union would merge (#8151). Such a design gets the field on every Join extrude
//! To Object as it opens, and keeps its bodies; designs of format 40 and later are left as they
//! are. The last release refuses a design that has the field (it refuses the newer format
//! number).

use serde_json::Value;

use crate::Result;

/// The first format whose Join extrudes To Object also joined the body they run to.
const JOINS_TO_BODY: u32 = 40;

pub fn step(v: &mut Value) -> Result<()> {
    // The file's own format is still in it: upgrades never rewrite `format`, and a file without
    // one is format 1.
    let version = match v.get("format").and_then(Value::as_str) {
        Some(format) => crate::format::version_of(format),
        None => Some(1),
    };
    if version.is_none_or(|n| n >= JOINS_TO_BODY) {
        return Ok(());
    }
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        let join_extrude = feature.get("type").and_then(Value::as_str) == Some("extrude")
            && feature.get("operation").and_then(Value::as_str) == Some("join");
        if !join_extrude {
            continue;
        }
        if let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut)
            && extent.get("to").is_some_and(|to| !to.is_null())
        {
            extent.insert("to_body_apart".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;

    fn design(format: &str) -> serde_json::Value {
        json!({"format": format, "features": [
            {"type": "extrude", "operation": "join", "extent": {"distance": "0", "to": [0.0, 0.0, 30.0]}},
            {"type": "extrude", "operation": "join", "extent": {"distance": "10 mm"}},
            {"type": "extrude", "operation": "cut", "extent": {"distance": "0", "to": [0.0, 0.0, 30.0]}},
        ]})
    }

    fn apart(v: &serde_json::Value) -> Vec<bool> {
        v["features"].as_array().unwrap().iter().map(|f| f["extent"]["to_body_apart"] == json!(true)).collect()
    }

    #[test]
    fn only_joins_to_an_object_saved_before_format_40_keep_the_body_apart() {
        let mut old = design("solidcraft/39");
        super::step(&mut old).unwrap();
        assert_eq!(apart(&old), [true, false, false]);
        for newer in ["solidcraft/40", "solidcraft/46"] {
            let mut v = design(newer);
            super::step(&mut v).unwrap();
            assert_eq!(apart(&v), [false, false, false], "{newer}");
        }
    }
}
