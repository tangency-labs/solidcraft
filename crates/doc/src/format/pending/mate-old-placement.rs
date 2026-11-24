//! A mate between two planar faces brings them together: the two faces' centres meet and the
//! parts touch (#14939). A mate saved before that change left the part where it was, level with
//! the face and a gap away from it. This step marks every mate of such an older design
//! `old_placement`, so the mate keeps the placement it was saved with, gap and all; a mate
//! made now leaves it off and brings the two faces together. The last release warns about the
//! newer format number, opens what it can read and keeps the rest.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    mark(v);
    Ok(())
}

/// Mark every mate of the design: the joints of the assembly. A joint that is not a mate keeps
/// no mark, and a design with no assembly has no mate to mark.
fn mark(v: &mut Value) {
    let Some(joints) = v.pointer_mut("/assembly/joints").and_then(Value::as_array_mut) else { return };
    for joint in joints {
        if joint.get("constraint").and_then(Value::as_str) != Some("mate") {
            continue;
        }
        if let Some(joint) = joint.as_object_mut() {
            joint.insert("old_placement".into(), Value::Bool(true));
        }
    }
}
