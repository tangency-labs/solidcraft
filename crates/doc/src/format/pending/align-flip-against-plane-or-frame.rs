//! An Align whose side faces no way (a bare point, a vertex, an edge's middle) against a plane or a
//! frame on the other side now turns half round with `flip`, about an axis square to the plane's
//! or frame's way as it is when evaluated (#21331). Older builds ignored `flip` there and only
//! moved the bodies. This step marks every saved flipped Align with a plane or a frame on a side
//! `flip_moves_only`, which keeps that reading, so designs saved before rebuild the bodies they
//! had. Older designs saved again carry `flip_moves_only`; new Aligns never write it. The last
//! release opens a design saved by this build with a warning about its newer format number and
//! only moves the bodies of such an Align, as it always did.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features.iter_mut().filter_map(Value::as_object_mut) {
        let set = |key: &str| feature.get(key).is_some_and(|x| !x.is_null());
        let held = ["from_plane", "to_plane", "from_frame", "to_frame"].iter().any(|k| set(k));
        if feature.get("type").and_then(Value::as_str) == Some("align") && feature.get("flip") == Some(&Value::Bool(true)) && held {
            feature.insert("flip_moves_only".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_flipped_aligns_onto_a_plane_or_frame_keep_only_moving() {
        let mut v = json!({"features": [
            {"type": "align", "from": [0.0, 0.0, 0.0], "to": [0.0, 0.0, 0.0], "to_plane": "XY", "flip": true},
            {"type": "align", "from": [0.0, 0.0, 0.0], "to": [0.0, 0.0, 0.0], "from_frame": {"face": [1.0, 2.0, 3.0]}, "flip": true},
            {"type": "align", "from": [0.0, 0.0, 0.0], "to": [0.0, 0.0, 0.0], "to_plane": "XY"},
            {"type": "align", "from": [0.0, 0.0, 0.0], "to": [0.0, 0.0, 0.0], "to_plane": null, "flip": true},
            {"type": "move", "to_plane": "XY", "flip": true}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["flip_moves_only"] == json!(true)).collect();
        assert_eq!(marked, [true, true, false, false, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
