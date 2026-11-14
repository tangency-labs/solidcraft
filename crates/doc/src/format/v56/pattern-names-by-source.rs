//! A body Pattern of several bodies reads its stored copy names by source and copy number, so a
//! copy keeps its name when the count changes (#12114). Older builds read them in the order the
//! copies were made and kept the names of copies a lower count dropped, so after a count went
//! down a later source's copies took other copies' names. Designs saved with such names still
//! refer to the copies by them, so this step marks every saved Pattern of several bodies with
//! stored names `pattern_names_in_order`, which keeps that reading: an older design opens with
//! the names it was saved with. The session then stores those names source by source as the
//! copies have them and clears the mark, and later count changes keep each name with its source.
//! The names themselves are left as they were. The last release asks for an update instead of
//! opening a design that carries this change.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        let Value::Object(o) = feature else { continue };
        let several = o.get("type").and_then(Value::as_str) == Some("pattern")
            && o.get("bodies").and_then(Value::as_array).is_some_and(|bodies| bodies.len() > 1)
            && o.get("body_names").and_then(Value::as_array).is_some_and(|names| !names.is_empty());
        if several {
            o.insert("pattern_names_in_order".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_patterns_of_several_bodies_keep_reading_their_names_in_order() {
        let names = json!(["Base (2)", "Base (3)", "Base (4)", "Post (2)", "Post (3)", "Post (4)"]);
        let mut v = json!({"features": [
            {"type": "pattern", "bodies": ["Base", "Post"], "body_names": names, "features": []},
            {"type": "pattern", "bodies": ["Base"], "body_names": ["Base (2)"], "features": []},
            {"type": "pattern", "bodies": ["Base", "Post"], "features": []},
            {"type": "pattern", "bodies": [], "features": [3, 4]},
            {"type": "box", "body_names": ["Base"]}
        ]});
        super::step(&mut v).unwrap();
        let features = v["features"].as_array().unwrap();
        let marked: Vec<bool> = features.iter().map(|f| f["pattern_names_in_order"] == json!(true)).collect();
        assert_eq!(marked, [true, false, false, false, false]);
        assert_eq!(features[0]["body_names"], names);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
