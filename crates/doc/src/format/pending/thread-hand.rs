//! A thread's direction is saved as `hand` on Hole features too (`right`, the default, not
//! saved, or `left`), as Thread features already save it (`right`, `left` or `both`). Format 39
//! (v0.24.0) saved the direction of both as `handedness: "left"`; this step renames it to
//! `hand`, so a left-hand thread or tapped hole opens left-handed, and drops a `handedness:
//! "right"`. Older designs have neither and need nothing. The last release refuses a design with
//! this change (the newer format number).
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        let threaded = matches!(feature.get("type").and_then(Value::as_str), Some("hole" | "thread"));
        let Some(f) = feature.as_object_mut().filter(|_| threaded) else { continue };
        match f.remove("handedness") {
            None => {}
            Some(Value::String(h)) if h == "right" => {}
            Some(Value::String(h)) if h == "left" => {
                f.insert("hand".into(), Value::from("left"));
            }
            Some(other) => return Err(crate::DocError::Invalid(format!("a thread's `handedness` must be right or left, not {other}"))),
        }
    }
    Ok(())
}
