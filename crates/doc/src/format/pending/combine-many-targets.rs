//! A Combine works on several bodies at once: `targets`, a list, where it used to save `target`,
//! one body name. Plasticity's Boolean card asks for "target bodies" and takes a whole selection,
//! so one tool can cut a row of parts in a single feature instead of one Combine per part.
//!
//! An older design saved a single `target`, which this step puts into a `targets` list of one, so
//! it combines exactly what it always did.
//!
//! The last release refuses a design that has `targets`, since the format number it carries is
//! newer than the one that release reads.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("combine") {
            continue;
        }
        let Some(object) = feature.as_object_mut() else { continue };
        if object.contains_key("targets") {
            continue;
        }
        let target = object.remove("target").filter(|t| !t.is_null());
        object.insert("targets".into(), Value::Array(target.into_iter().collect()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_combine_that_named_one_target_keeps_it() {
        let mut v = json!({"features": [
            {"type": "combine", "target": "Base", "tools": ["Cutter"], "operation": "cut"},
            {"type": "combine", "targets": ["A", "B"], "tools": ["C"], "operation": "cut"},
            {"type": "box"}
        ]});
        super::step(&mut v).unwrap();
        let f = v["features"].as_array().unwrap();
        assert_eq!(f[0]["targets"], json!(["Base"]));
        assert!(f[0].get("target").is_none());
        assert_eq!(f[1]["targets"], json!(["A", "B"]), "a design that already has them is left alone");
        assert!(f[2].get("targets").is_none(), "only a combine is touched");
    }
}
