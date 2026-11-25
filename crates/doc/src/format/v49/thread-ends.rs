//! A modelled thread saves how its ends are finished: `ease_out`, the runout at the inner end,
//! away from where it starts (`standard` 2.5 P, `short` 1.25 P or `{length}`, ISO 3508), and
//! `tip`, which free ends are chamfered (`start`: the end it starts from, `both`: each free end;
//! ISO 4753), with `chamfer_angle` and `chamfer_depth`. A tapped hole saves `modeled` (its thread
//! cut into the wall) and that thread's `ease_out` towards the bottom. Without them, as in an
//! older design, the groove stops square, no end is chamfered and a tapped hole's thread is only
//! listed. Designs saved while these were being made are
//! rewritten as they open: `tip: chamfer` (every free end) becomes `both`, and an `ease_in` (a
//! runout at the start, which now always runs off its free end) is dropped. The last release
//! refuses a design saved with this (the newer format number).
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
        if object.get("tip").and_then(Value::as_str) == Some("chamfer") {
            object.insert("tip".into(), Value::String("both".into()));
        }
        object.remove("ease_in");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_chamfered_tip_becomes_both_ends_and_the_start_runout_goes() {
        let mut v = json!({"features": [
            {"type": "thread", "designation": "M10", "modeled": true, "tip": "chamfer", "ease_in": "standard", "ease_out": "short"},
            {"type": "thread", "designation": "M10", "modeled": true},
            {"type": "chamfer", "tip": "chamfer"}
        ]});
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][0]["tip"], json!("both"));
        assert!(v["features"][0].get("ease_in").is_none());
        assert_eq!(v["features"][0]["ease_out"], json!("short"));
        assert!(v["features"][1].get("tip").is_none());
        assert_eq!(v["features"][2]["tip"], json!("chamfer"), "only threads");
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
