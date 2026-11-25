//! The pieces a feature splits a body into take its body names first by the faces the feature
//! cut them apart with, then by their least face name (`lineage::order_lumps`, #21552). Builds
//! before ordered them by their least face name alone: an upstream edit that took away the face
//! holding a piece's least name (a notch that eats the end of a bar) handed the body's name to
//! another piece and a renamed piece's name to a sliver, and later features naming it lost it.
//! Older designs' stored body names, and every later feature naming those bodies, mean the old
//! order, so this step marks each of their features `least_face_piece_order`, which keeps it for
//! them. Features added later order their pieces by the faces they cut. The last release opens a
//! design with this change with a warning about the newer format, and keeps what it cannot read.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if let Value::Object(o) = feature {
            o.insert("least_face_piece_order".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn older_features_keep_naming_their_pieces_by_their_least_face() {
        let mut v = json!({"features": [{"type": "sketch"}, {"type": "box", "body_names": ["Bar"]}, {"type": "combine"}]});
        super::step(&mut v).unwrap();
        assert!(v["features"].as_array().unwrap().iter().all(|f| f["least_face_piece_order"] == true));
        assert_eq!(v["features"][1]["body_names"], json!(["Bar"]));
        let once = v.clone();
        super::step(&mut v).unwrap();
        assert_eq!(v, once);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }

    /// A design saved before the change opens with the mark on its features, and keeps it when
    /// it is saved and opened again; a feature added since has none.
    #[test]
    fn an_older_design_keeps_the_mark_through_saving() {
        let mut d = crate::Document::new("A");
        let base = crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch: solidcraft_sketch::Sketch::new() };
        d.add_feature(base.clone(), Some("Base")).unwrap();
        assert!(d.features.iter().all(|f| !f.least_face_piece_order));
        let mut file: serde_json::Value = serde_json::from_str(&d.to_json()).unwrap();
        assert!(file["features"].as_array().unwrap().iter().all(|f| f.get("least_face_piece_order").is_none()));
        // Format 67 predates this change, even after pending changes are folded.
        file["format"] = json!("solidcraft/67");
        file.as_object_mut().unwrap().remove("format_pending");
        let mut opened = crate::format::read(&file.to_string()).unwrap();
        assert!(opened.features.iter().all(|f| f.least_face_piece_order));
        let again = crate::format::read(&opened.to_json()).unwrap();
        assert_eq!(again.features, opened.features);
        opened.add_feature(base, Some("Later")).unwrap();
        let later = opened.features.iter().find(|f| f.name == "Later").unwrap();
        assert!(!later.least_face_piece_order);
    }
}
