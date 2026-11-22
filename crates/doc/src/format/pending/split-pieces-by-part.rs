//! A Split Body makes a body of each part a piece leaves apart (#15943): a plane through two
//! legs of a body gives a body per leg. Older builds kept such a piece as one body of several
//! parts, and designs saved with them refer to those bodies by name, so this step marks every
//! saved Split `split_keeps_lumps`, which keeps that: an older design opens with the bodies it
//! was saved with. A new Split saves without the mark. The last release asks for an update
//! instead of opening a design that carries this change.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        let Value::Object(o) = feature else { continue };
        if o.get("type").and_then(Value::as_str) == Some("split") {
            o.insert("split_keeps_lumps".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_splits_keep_their_pieces_whole() {
        let mut v = json!({"features": [
            {"type": "box", "body_names": ["Block"]},
            {"type": "split", "body": "Block", "body_names": ["Block", "Block (1)"]},
            {"type": "split_face", "faces": []}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["split_keeps_lumps"] == json!(true)).collect();
        assert_eq!(marked, [false, true, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
