//! A hole at the points of a sketch on an offset, construction or other derived plane now starts
//! at that plane when the plane lies inside a body, instead of at the body's surface (#11538).
//! Older builds moved every such point back to the surface, so this step marks every saved hole
//! at sketch points (not a Single hole) `old_surface_start`, which keeps that reading: designs
//! saved before rebuild their holes where they were. Holes at a position, Single holes and holes
//! made or opened from now on are unchanged. The last release asks for an update instead of
//! opening a design that carries this change.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("hole") {
            continue;
        }
        let at_points =
            feature.get("points").and_then(Value::as_object).is_some_and(|points| !points.get("single").and_then(Value::as_bool).unwrap_or(false));
        if let (true, Some(object)) = (at_points, feature.as_object_mut()) {
            object.insert("old_surface_start".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_holes_at_sketch_points_keep_starting_at_the_surface() {
        let mut v = json!({"features": [
            {"type": "hole", "points": {"sketch": 3, "ids": ["p1"]}},
            {"type": "hole", "points": {"sketch": 3, "ids": ["p1", "p2"], "flip": true}},
            {"type": "hole", "points": {"sketch": 3, "ids": ["p1"], "single": true}},
            {"type": "hole", "position": [0.0, 0.0, 10.0]},
            {"type": "extrude", "points": {"sketch": 3, "ids": ["p1"]}}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["old_surface_start"] == json!(true)).collect();
        assert_eq!(marked, [true, true, false, false, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
