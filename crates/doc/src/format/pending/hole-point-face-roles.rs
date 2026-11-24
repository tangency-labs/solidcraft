//! The faces of a hole drilled at sketch points take the id of their point in their name.
//! Older holes keep their positional face names while all of their selected points exist.
//! After a selected point is lost, an old positional reference expires so it cannot select a
//! different hole. The last release warns if it opens a design with the new format and preserves
//! data it cannot read.

use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("hole") {
            continue;
        }
        if let Some(points) = feature.get_mut("points").and_then(Value::as_object_mut)
            && !points.get("single").and_then(Value::as_bool).unwrap_or(false)
        {
            points.insert("legacy_face_roles".into(), Value::Bool(true));
        }
    }
    Ok(())
}
