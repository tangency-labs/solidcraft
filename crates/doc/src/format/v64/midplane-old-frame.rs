//! A midplane between two planes that are parallel to a world plane takes that world plane's
//! frame: its origin on the world axis, not at the point the planes were picked, and its axes
//! the world's (#18260). A midplane of a design saved before that change saves `old_frame`, and
//! keeps the frame it was saved with (the first plane's own origin and axes), so the sketch on
//! it opens where it was drawn. This step marks every midplane of such an older design
//! `old_frame`; a midplane of a design saved now leaves it off and takes the world plane's
//! frame. The last release warns about the newer format number, opens what it can read and
//! keeps the rest.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    mark(v);
    Ok(())
}

/// Mark every midplane in the design, wherever it is: a feature's plane, a plane another plane
/// is built on, a plane in a canvas or a joint.
fn mark(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if is_midplane(o) {
                o.insert("old_frame".into(), Value::Bool(true));
            }
            for (key, x) in o.iter_mut() {
                // Another tool's data is kept as it is (#1015): a midplane in it is not ours.
                if key != "metadata" {
                    mark(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(mark),
        _ => {}
    }
}

/// A midplane reference: its type, and the two planes it is between.
fn is_midplane(o: &serde_json::Map<String, Value>) -> bool {
    o.get("type").and_then(Value::as_str) == Some("midplane") && o.get("a").is_some() && o.get("b").is_some()
}
