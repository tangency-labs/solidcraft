//! A Tangent between a line and a circle or an arc is read at the line's end when that end
//! lies on the curve: the end lies on the curve and the line touches it there (#14343). A
//! Tangent of a design saved before that change keeps the plain tangency (the infinite line
//! touches the curve and the line's end does not move). This step marks every Tangent of such
//! an older design `old_join`, so the tangency is read as it was; a Tangent made now leaves it
//! off. The last release warns about the newer format number, opens what it can read and keeps
//! the rest.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    mark(v);
    Ok(())
}

/// Mark every Tangent of the design, wherever it is: a sketch's own constraints.
fn mark(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for (key, x) in o.iter_mut() {
                // Another tool's data is kept as it is (#1015): a Tangent in it is not ours.
                if key != "metadata" {
                    mark(x);
                }
            }
            // A sketch's constraints: the Tangents among them.
            if let Some(constraints) = o.get_mut("constraints").and_then(Value::as_array_mut) {
                for constraint in constraints {
                    if is_tangent(constraint) {
                        if let Some(constraint) = constraint.as_object_mut() {
                            constraint.insert("old_join".into(), Value::Bool(true));
                        }
                    }
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(mark),
        _ => {}
    }
}

/// A sketch's Tangent constraint: its type, and the two curves it holds tangent.
fn is_tangent(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    o.get("type").and_then(Value::as_str) == Some("tangent")
        && o.get("a").is_some_and(Value::is_number)
        && o.get("b").is_some_and(Value::is_number)
}
