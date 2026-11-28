//! The three inch rules of the plastic rule library ("ABS (0.1 in)", "Nylon (PA6) (0.1 in)",
//! "Polypropylene (PP) (0.1 in)") give their maximum and minimum thickness in inches, as their
//! other lengths are (#22133); they were in millimetres. A design saves a library rule only when
//! it edits it, so an older design that uses one of these rules unedited (as its active rule or
//! a body's rule) would take the new limits. This step saves that rule in the design with the
//! millimetre limits it had, so the design shows what it showed before. The limits are shown and
//! reported only; no feature reads them. The last release warns about the newer format number,
//! opens what it can read and keeps the rest.
use serde_json::{Map, Value, json};

/// Each inch rule, with the maximum and minimum thickness it had before this change.
const OLD_LIMITS: [(&str, &str, &str, &str); 3] = [
    ("ABS (0.1 in)", "ABS Plastic", "3.5 mm", "1.2 mm"),
    ("Nylon (PA6) (0.1 in)", "Nylon 6", "3 mm", "0.8 mm"),
    ("Polypropylene (PP) (0.1 in)", "Polypropylene", "3.8 mm", "0.8 mm"),
];

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(plastic) = v.get_mut("plastic").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let used = used_rules(plastic);
    let own: Vec<String> = match plastic.get("rules").and_then(Value::as_array) {
        Some(rules) => rules.iter().filter_map(|r| r.get("name").and_then(Value::as_str)).map(str::to_string).collect(),
        None => Vec::new(),
    };
    let added: Vec<Value> = OLD_LIMITS
        .iter()
        .filter(|(name, ..)| used.iter().any(|u| u == name) && !own.iter().any(|o| o == name))
        .map(|&(name, material, max, min)| old_rule(name, material, max, min))
        .collect();
    if added.is_empty() {
        return Ok(());
    }
    match plastic.get_mut("rules").and_then(Value::as_array_mut) {
        Some(rules) => rules.extend(added),
        None => {
            plastic.insert("rules".into(), Value::Array(added));
        }
    }
    Ok(())
}

/// The names of the active rule and of the rules assigned to bodies.
fn used_rules(plastic: &Map<String, Value>) -> Vec<String> {
    let active = plastic.get("active").and_then(Value::as_str);
    let assigned = plastic.get("assigned").and_then(Value::as_object).into_iter().flat_map(|a| a.values().filter_map(Value::as_str));
    active.into_iter().chain(assigned).map(str::to_string).collect()
}

/// An inch rule as the library gave it before this change.
fn old_rule(name: &str, material: &str, max: &str, min: &str) -> Value {
    json!({
        "name": name,
        "material": material,
        "thickness": "0.1 in",
        "nominal_radius": "0.5 * Thickness",
        "clearance": "0.004 in",
        "knife_edge": "0.04 in",
        "reveal_height": "0.02 in",
        "thickness_variation": "0.15 * Thickness",
        "draft": "2 deg",
        "max_thickness": max,
        "min_thickness": min,
        "min_draft": "0.5 deg",
    })
}
