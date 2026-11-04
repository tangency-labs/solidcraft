//! A driven (reference) dimension saves a parameter name (`param` on the constraint, and a
//! model parameter of that name) that expressions can use (#5582). Its value is what the
//! dimension measures, read-only. An older design's driven dimensions have no name: this step
//! gives each the next free `dN` as the design opens. The last release refuses a design that has
//! them (it asks for an update) instead of treating the name as an ordinary parameter whose
//! value never follows the sketch.
use serde_json::{Value, json};

use crate::expr;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let units = v.get("units").and_then(Value::as_str).unwrap_or("mm").to_string();
    let length_unit = expr::length_unit_or_mm(&units).to_string();
    let scale = expr::unit_info(&length_unit).map_or(1.0, |(_, s)| s);
    // The `dN` numbers taken: parameters and named feature inputs.
    let mut taken: std::collections::HashSet<u64> = {
        let params = v.get("params").and_then(Value::as_array).into_iter().flatten().filter_map(|p| p.get("name")?.as_str());
        let inputs = v
            .get("features")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|f| f.get("param_names")?.as_array())
            .flatten()
            .filter_map(Value::as_str);
        crate::params::numbers_after("d", params.chain(inputs))
    };
    let mut added = Vec::new();
    let mut count = v.get("params").and_then(Value::as_array).map_or(0, Vec::len);
    let Some(Value::Array(features)) = v.get_mut("features") else { return Ok(()) };
    for feature in features.iter_mut().filter(|f| f.get("type").and_then(Value::as_str) == Some("sketch")) {
        let Some(Value::Array(constraints)) = feature.get_mut("sketch").and_then(|s| s.get_mut("constraints")) else { continue };
        for c in constraints.iter_mut() {
            if c.get("driven").and_then(Value::as_bool) != Some(true) || c.get("param").is_some_and(|p| !p.is_null()) {
                continue;
            }
            // What it measures, read from its kind (one that isn't a valid dimension keeps no name).
            let Ok(parsed) = serde_json::from_value::<solidcraft_sketch::Constraint>(c.clone()) else { continue };
            let Some(value) = parsed.kind.value().filter(|v| v.is_finite()) else { continue };
            if count >= crate::MAX_PARAMS {
                break;
            }
            let n = (1..).find(|n| !taken.contains(n)).unwrap_or(1);
            taken.insert(n);
            let name = format!("d{n}");
            let (text, unit) = if parsed.kind.is_angle() {
                (format!("{} deg", (value.to_degrees() * 1e6).round() / 1e6), "deg".to_string())
            } else {
                (format!("{} {length_unit}", (value / scale * 1e6).round() / 1e6), length_unit.clone())
            };
            if let Some(o) = c.as_object_mut() {
                o.insert("param".into(), Value::from(name.as_str()));
                added.push(json!({"name": name, "expr": text, "unit": unit, "model": true}));
                count += 1;
            }
        }
    }
    if added.is_empty() {
        return Ok(());
    }
    match v.get_mut("params") {
        Some(Value::Array(params)) => params.extend(added),
        _ => {
            if let Some(o) = v.as_object_mut() {
                o.insert("params".into(), Value::Array(added));
            }
        }
    }
    Ok(())
}
