//! Grease Pencil stores stroke diameter in millimetres. Ordinary sketch curves have
//! no diameter. Preserve diameters from the standalone patch and convert its earlier
//! screen widths at eight points per millimetre. Older releases warn and retain
//! unknown fields.

use serde_json::{Value, json};

pub fn step(design: &mut Value) -> crate::Result<()> {
    for feature in design.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        for wire in feature.pointer_mut("/sketch/wires").and_then(Value::as_array_mut).into_iter().flatten() {
            let Some(wire) = wire.as_object_mut() else { continue };
            let Some(width) = wire.remove("ink_width") else { continue };
            if wire.get("ink_diameter").is_none_or(Value::is_null)
                && let Some(width) = width.as_f64()
            {
                wire.insert("ink_diameter".into(), json!(width / 8.0));
            }
        }
    }
    Ok(())
}
