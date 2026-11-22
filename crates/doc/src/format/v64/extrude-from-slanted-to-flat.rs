//! An extrude From Object on a flat face at a slant to the extrusion, with To Object on a flat
//! face or a point, now runs from the From face's plane to the To face's plane, so it fills the
//! space between two slanted faces (#18825). Older builds started it on a plane parallel to the
//! sketch, at the level where the From face meets the middle of the profiles, and ran it to the
//! To face from there. This step marks every saved extrude with both a From and a To Object
//! `from_level`, which keeps that reading, so designs saved before rebuild the bodies they had.
//! Extrudes from a face square to the extrusion or from a point build the same either way.
//! Nothing new is saved: a design made with this build stores such an extrude without
//! `from_level`. The last release opens that design with a warning about its newer format number
//! (asking for an update), and builds the extrude from the face's level as it always did, not
//! between the two planes; a design it saved itself, marked by this step, opens the same in both.
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
        let set = |key: &str| extent.get(key).is_some_and(|p| !p.is_null());
        if set("from") && set("to") {
            extent.insert("from_level".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_extrudes_from_and_to_an_object_keep_starting_at_the_from_level() {
        let mut v = json!({"features": [
            {"type": "extrude", "extent": {"distance": "5", "from": [1.0, 2.0, 3.0], "to": [1.0, 2.0, 9.0]}},
            {"type": "extrude", "extent": {"distance": "5", "from": [1.0, 2.0, 3.0]}},
            {"type": "extrude", "extent": {"distance": "5", "from": [1.0, 2.0, 3.0], "to": null}},
            {"type": "extrude", "extent": {"distance": "5", "to": [1.0, 2.0, 3.0]}},
            {"type": "revolve", "extent": {"from": [1.0, 2.0, 3.0], "to": [1.0, 2.0, 9.0]}}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["extent"]["from_level"] == json!(true)).collect();
        assert_eq!(marked, [true, false, false, false, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
