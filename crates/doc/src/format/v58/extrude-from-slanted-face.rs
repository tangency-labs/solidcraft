//! An extrude From Object on a flat face at a slant to the extrusion now starts on that face, so
//! its start follows the slope, and with a distance its far end is the face moved that far along
//! the extrusion. Older builds started it on a plane parallel to the sketch, at the level where
//! the face meets the middle of the profiles. This step marks every saved extrude with a From
//! Object `from_level`, which keeps that reading, so designs saved before rebuild the bodies
//! they had. Extrudes from a face square to the extrusion or from a point build the same either
//! way. The last release opens a design that has the field with a newer-format warning, ignores
//! it, and starts on the flat plane.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("extrude") {
            continue;
        }
        let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut) else { continue };
        if extent.get("from").is_some_and(|p| !p.is_null()) {
            extent.insert("from_level".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_extrudes_from_an_object_keep_starting_at_its_level() {
        let mut v = json!({"features": [
            {"type": "extrude", "extent": {"distance": "5", "from": [1.0, 2.0, 3.0]}},
            {"type": "extrude", "extent": {"distance": "5", "from": null}},
            {"type": "extrude", "extent": {"distance": "5", "to": [1.0, 2.0, 3.0]}},
            {"type": "revolve", "extent": {"from": [1.0, 2.0, 3.0]}}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["extent"]["from_level"] == json!(true)).collect();
        assert_eq!(marked, [true, false, false, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
