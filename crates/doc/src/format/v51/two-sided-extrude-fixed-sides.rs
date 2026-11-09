//! In a two-sided extrude (`distance2`) with entered distances, Direction fixes the sides and the
//! sign of a distance only turns its own side over; `distance2` no longer follows the first
//! side across the sketch plane. Older builds sent `distance2` to the other side of a negative
//! first distance, so this step marks every saved two-sided extrude with entered distances
//! `second_follows_first`, which keeps that reading. The distances themselves are left as they
//! were, so an expression keeps the old sides whatever it evaluates to and a feature's named
//! inputs keep their values for the expressions that use them. An extent that reaches to an
//! object or through all already lets the second side follow the first, and a symmetric one has
//! no second side. The last release asks for an update instead of opening a design that carries
//! this change.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if !matches!(feature.get("type").and_then(Value::as_str), Some("extrude" | "surface_extrude")) {
            continue;
        }
        let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut) else { continue };
        let plain = extent.get("distance2").is_some_and(|d| !d.is_null())
            && extent.get("to").is_none_or(Value::is_null)
            && !extent.get("through_all").and_then(Value::as_bool).unwrap_or(false)
            && extent.get("direction").and_then(Value::as_str) != Some("symmetric");
        if plain {
            extent.insert("second_follows_first".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_two_sided_extrudes_keep_their_sides_and_distances() {
        let mut v = json!({"features": [
            {"type": "extrude", "extent": {"distance": "-10 mm", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm", "direction": "negative", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-0 mm", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-width", "distance2": "10 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm", "through_all": true, "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm", "to": [0.0, 0.0, 1.0], "distance2": "5 mm"}},
            {"type": "sketch"}
        ]});
        let before = v.clone();
        super::step(&mut v).unwrap();
        let features = v["features"].as_array().unwrap();
        let marked: Vec<bool> = features.iter().map(|f| f["extent"]["second_follows_first"] == json!(true)).collect();
        assert_eq!(marked, [true, true, true, true, false, false, false, false]);
        for (f, b) in features.iter().zip(before["features"].as_array().unwrap()) {
            assert_eq!((&f["extent"]["distance"], &f["extent"]["direction"]), (&b["extent"]["distance"], &b["extent"]["direction"]));
        }
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
