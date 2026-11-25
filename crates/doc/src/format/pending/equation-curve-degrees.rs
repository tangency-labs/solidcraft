//! Existing equation curves receive `radians_trig: true` and keep their geometry (#19630).
//! New curves save a `degrees` record in the sketch's equation list. The previous release
//! warns and keeps a sketch with one of these records as unread instead of rebuilding it with
//! the wrong angle convention. Radians-only sketches still use the old record shape.

use serde_json::Value;

use crate::Result;

pub fn step(v: &mut Value) -> Result<()> {
    let features = v.get_mut("features").and_then(Value::as_array_mut);
    for feature in features.into_iter().flatten() {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let equations = feature.pointer_mut("/sketch/equations").and_then(Value::as_array_mut);
        for equation in equations.into_iter().flatten().filter_map(Value::as_object_mut) {
            if equation.contains_key("curve") {
                equation.insert("radians_trig".into(), Value::Bool(true));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn only_equations_in_design_sketch_features_are_marked() {
        let sketch = json!({"curves": [{"id": "s1", "type": "spline"}], "equations": [
            {"curve": "s1", "x": "10 * cos(t)", "y": "10 * sin(t)", "start": "0", "end": "2 * pi", "tolerance": 0.01},
            {"curve": "s2", "y": "sin(x / 10 mm)", "start": "0 mm", "end": "100 mm", "tolerance": 0.01, "variable_unit": "range"},
            {"degrees": {"curve": "s3", "y": "sin(90)", "start": "0", "end": "10", "tolerance": 0.01}}
        ]});
        let mut file = json!({"features": [
            {"type": "sketch", "sketch": sketch.clone()},
            {"type": "future_feature", "sketch": sketch.clone()}
        ], "metadata": {"type": "sketch", "sketch": sketch}});
        let mut expected = file.clone();
        expected["features"][0]["sketch"]["equations"][0]["radians_trig"] = json!(true);
        expected["features"][0]["sketch"]["equations"][1]["radians_trig"] = json!(true);
        super::step(&mut file).unwrap();
        super::step(&mut file).unwrap();
        assert_eq!(file, expected);
    }
}
