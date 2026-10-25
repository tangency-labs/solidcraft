//! A feature that joins saves what it makes of pieces that share no face: `touching`,
//! `separate` (the default, not saved: a piece that touches another only along an edge or at a
//! point becomes a body of its own, pieces that touch nothing stay one body) or `one_body` (every
//! piece in one body). Older builds kept every join's pieces in one body, also those touching
//! along an edge or at a point, so this step gives each feature of an older design that can join
//! `one_body`, which keeps its bodies as they were (only a join with such a contact would
//! change). The last release refuses a design with this change (the newer format number).
use serde_json::Value;

/// Feature types that join without an `operation` saying so: a combine (whose `new` joins too),
/// patterns and mirrors (which repeat a join), and the features that add material to a body.
const JOINING: [&str; 8] = ["combine", "pattern", "mirror", "emboss", "rib", "boss", "snap_fit", "rest"];

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        let joins = feature.get("type").and_then(Value::as_str).is_some_and(|t| JOINING.contains(&t))
            || feature.get("operation").and_then(Value::as_str) == Some("join");
        if joins && let Some(f) = feature.as_object_mut() {
            f.insert("touching".into(), Value::from("one_body"));
        }
    }
    Ok(())
}
