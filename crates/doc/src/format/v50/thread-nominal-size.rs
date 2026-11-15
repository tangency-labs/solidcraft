//! A thread with no `designation` (its size left automatic) now takes, for a hole, the size
//! whose nominal diameter the hole has (a 12 mm hole is M12); before, it took the size whose tap
//! drill the hole matches (a 12 mm hole was M14, the drill M14 is tapped at). A design saved
//! before keeps its size: each such thread is saved with `old_auto_size`, which an automatic
//! thread made now does not have. A modelled thread in a hole wider than it needs also narrows
//! the bore now, rather than being refused. The last release refuses a design saved with this
//! (the newer format number).
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        let Some(object) = feature.as_object_mut() else { continue };
        if object.get("type").and_then(Value::as_str) != Some("thread") {
            continue;
        }
        if object.get("designation").is_none_or(Value::is_null) {
            object.insert("old_auto_size".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn an_automatic_thread_keeps_the_old_rule_and_a_named_one_is_left_alone() {
        let mut v = json!({"features": [
            {"type": "thread", "face": [6, 0, 5]},
            {"type": "thread", "designation": "M12"},
            {"type": "hole", "thread": "M12"}
        ]});
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][0]["old_auto_size"], json!(true));
        assert!(v["features"][1].get("old_auto_size").is_none());
        assert!(v["features"][2].get("old_auto_size").is_none());
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
