//! A two-sided extrude (`distance2`) follows its `direction` (#2700): with `direction: negative`
//! the first distance goes against the sketch normal and `distance2` along it, so either side can
//! be the through-all or To Object one. Older builds ignored `direction` when an extrude had a
//! second distance (the first always went along the normal), so this step makes the two-sided
//! `negative` extrudes of older designs `positive`, which keeps their shape. 0.15.x asks for an
//! update instead of opening a design with a reversed two-sided extrude.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if !matches!(feature.get("type").and_then(Value::as_str), Some("extrude" | "surface_extrude")) {
            continue;
        }
        if let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut)
            && extent.get("distance2").is_some_and(|d| !d.is_null())
            && extent.get("direction").and_then(Value::as_str) == Some("negative")
        {
            extent.insert("direction".into(), Value::from("positive"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn older_reversed_two_sided_extrudes_keep_their_shape() {
        let mut v = json!({"features": [
            {"type": "extrude", "extent": {"distance": "10 mm", "direction": "negative", "distance2": "5 mm"}},
            {"type": "surface_extrude", "extent": {"distance": "10 mm", "direction": "negative", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "10 mm", "direction": "negative"}},
            {"type": "extrude", "extent": {"distance": "10 mm", "direction": "symmetric", "distance2": "5 mm"}},
            {"type": "revolve", "extent": {"direction": "negative", "distance2": "5 mm"}},
            {"type": "sketch"}
        ]});
        super::step(&mut v).unwrap();
        let dirs: Vec<Option<&str>> = v["features"].as_array().unwrap().iter().map(|f| f["extent"]["direction"].as_str()).collect();
        assert_eq!(dirs, [Some("positive"), Some("positive"), Some("negative"), Some("symmetric"), Some("negative"), None]);
        // A design with no features is left alone.
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
