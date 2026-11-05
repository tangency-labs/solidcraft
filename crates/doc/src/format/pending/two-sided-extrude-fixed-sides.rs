//! In a two-sided extrude (`distance2`) with entered distances, Direction fixes the sides and the
//! sign of a distance only turns its own side over; `distance2` no longer follows the first
//! side across the sketch plane. Older builds sent `distance2` to the other side of a negative
//! first distance, so this step gives a literal negative first distance its magnitude and the
//! opposite direction, which keeps the shape. Expressions are left alone, as is any extent that
//! reaches to an object or through all. The last release asks for an update instead of opening
//! a design that carries this change.
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
            && !extent.get("through_all").and_then(Value::as_bool).unwrap_or(false);
        if !plain {
            continue;
        }
        let Some(text) = extent.get("distance").and_then(Value::as_str) else { continue };
        let Some(magnitude) = text.trim().strip_prefix('-').map(str::trim) else { continue };
        if !magnitude.starts_with(|c: char| c.is_ascii_digit() || c == '.')
            || !magnitude.chars().all(|c| c.is_ascii_digit() || c == '.' || c.is_alphabetic() || c == ' ')
        {
            continue;
        }
        let flipped = match extent.get("direction").and_then(Value::as_str) {
            Some("negative") => "positive",
            Some("symmetric") => continue,
            _ => "negative",
        };
        let magnitude = magnitude.to_string();
        extent.insert("distance".into(), Value::from(magnitude));
        extent.insert("direction".into(), Value::from(flipped));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_literal_negative_first_distance_keeps_its_shape() {
        let mut v = json!({"features": [
            {"type": "extrude", "extent": {"distance": "-10 mm", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm", "direction": "negative", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "10 mm", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm"}},
            {"type": "extrude", "extent": {"distance": "-a", "distance2": "5 mm"}},
            {"type": "extrude", "extent": {"distance": "-10 mm", "through_all": true, "distance2": "5 mm"}},
            {"type": "sketch"}
        ]});
        super::step(&mut v).unwrap();
        let got: Vec<(Option<&str>, Option<&str>)> =
            v["features"].as_array().unwrap().iter().map(|f| (f["extent"]["distance"].as_str(), f["extent"]["direction"].as_str())).collect();
        assert_eq!(
            got,
            [
                (Some("10 mm"), Some("negative")),
                (Some("10 mm"), Some("positive")),
                (Some("10 mm"), None),
                (Some("-10 mm"), None),
                (Some("-a"), None),
                (Some("-10 mm"), None),
                (None, None)
            ]
        );
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
