//! An extrude that ends To Object or starts From Object may save `picked_placements` in its
//! `extent`: where each other component stood, relative to the extrude's own, when the object
//! was picked (`[{component, placement}]`, a 4x4 matrix each). The extrude measures against the
//! model as it was then, so a joint that slides a component does not stretch a body built to
//! another component's face (#16732). Older builds measured against the placements as shown, so
//! a drawer that ended at its frame grew with every slide. A design saved that way gets the
//! placements its file holds as it opens, which are where the extrude was last built, so it
//! opens with the bodies it had and no longer stretches. The last release opens a design that has
//! the field with a newer-format warning, ignores it, and measures against the placements shown.

use std::collections::HashMap;

use serde_json::Value;

use crate::{DocError, Mat, Occurrence, Result};

/// Maximum total `picked_placements` entries synthesized across all legacy extrudes in one file.
const MAX_TOTAL_PICKED_PLACEMENTS: usize = 100_000;

/// World placement of `component` through each component's first occurrence, stopping if the chain
/// is deeper than [`crate::MAX_PLACEMENT_DEPTH`] (which also catches cycles).
fn world_placement(component: u64, first_of: &HashMap<u64, (u64, Mat)>) -> Result<Mat> {
    let mut m = crate::IDENTITY;
    let mut c = component;
    for _ in 0..crate::MAX_PLACEMENT_DEPTH {
        if c == 0 {
            return Ok(m);
        }
        let Some(&(parent, transform)) = first_of.get(&c) else {
            return Ok(m);
        };
        m = crate::mat_mul(&transform, &m);
        c = parent;
    }
    if c == 0 || !first_of.contains_key(&c) {
        return Ok(m);
    }
    Err(crate::assembly::too_many_placements())
}

pub fn step(v: &mut Value) -> Result<()> {
    let too_many = |key: &str, max: usize| v.get(key).and_then(Value::as_array).is_some_and(|list| list.len() > max);
    if too_many("features", crate::document::MAX_FEATURES)
        || too_many("components", crate::document::MAX_FEATURES)
        || too_many("occurrences", crate::assembly::MAX_OCCURRENCES)
    {
        return Err(DocError::Invalid("document too large".into()));
    }
    let occurrences: Vec<Occurrence> = match v.get("occurrences").cloned() {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => match serde_json::from_value(value) {
            Ok(occurrences) => occurrences,
            Err(_) => return Ok(()),
        },
    };
    let mut max_id = 0u64;
    let mut first_of: HashMap<u64, (u64, Mat)> = HashMap::with_capacity(occurrences.len());
    for o in &occurrences {
        max_id = max_id.max(o.id);
        first_of.entry(o.component).or_insert((o.parent, o.transform));
    }
    // A component an older file never placed is placed at the identity as the file opens
    // (`normalise`), so that is where it stands.
    let mut missing: Vec<(u64, u64)> = Vec::new();
    for c in v.get("components").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = c.get("id").and_then(Value::as_u64) else { continue };
        if id == 0 || first_of.contains_key(&id) {
            continue;
        }
        let parent = c.get("parent").and_then(Value::as_u64).unwrap_or(0);
        missing.push((id, parent));
    }
    for (component, parent) in missing {
        max_id = max_id.checked_add(1).ok_or_else(|| DocError::Invalid("too many items in the design".into()))?;
        first_of.entry(component).or_insert((parent, crate::IDENTITY));
    }
    if first_of.is_empty() {
        return Ok(());
    }
    let eligible_owns: Vec<u64> = v
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|feature| {
            feature.get("type").and_then(Value::as_str) == Some("extrude")
                && feature.get("extent").and_then(Value::as_object).is_some_and(|extent| {
                    ["to", "from"].iter().any(|key| extent.get(*key).is_some_and(|p| !p.is_null())) && !extent.contains_key("picked_placements")
                })
        })
        .map(|feature| feature.get("component").and_then(Value::as_u64).unwrap_or(0))
        .collect();
    if eligible_owns.is_empty() {
        return Ok(());
    }
    let mut components: Vec<u64> = first_of.keys().copied().collect();
    components.push(0);
    components.sort_unstable();
    components.dedup();
    let mut total_pins = 0usize;
    for &own in &eligible_owns {
        let count = components.len().saturating_sub(usize::from(components.binary_search(&own).is_ok()));
        total_pins = total_pins.saturating_add(count);
        if total_pins > MAX_TOTAL_PICKED_PLACEMENTS {
            return Err(DocError::Invalid("too many picked extrude placements".into()));
        }
    }
    let mut worlds: Vec<(u64, Mat)> = Vec::with_capacity(components.len());
    let mut world_map: HashMap<u64, Mat> = HashMap::with_capacity(components.len());
    for &c in &components {
        let m = world_placement(c, &first_of)?;
        worlds.push((c, m));
        world_map.insert(c, m);
    }
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        if feature.get("type").and_then(Value::as_str) != Some("extrude") {
            continue;
        }
        let own = feature.get("component").and_then(Value::as_u64).unwrap_or(0);
        let own_world = world_map.get(&own).copied().unwrap_or(crate::IDENTITY);
        let Some(inverse) = crate::mat_inverse(&own_world) else { continue };
        let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut) else { continue };
        let picks = ["to", "from"].iter().any(|key| extent.get(*key).is_some_and(|p| !p.is_null()));
        if !picks || extent.contains_key("picked_placements") {
            continue;
        }
        let pins: Vec<Value> = worlds
            .iter()
            .filter(|(c, _)| *c != own)
            .map(|(c, m)| serde_json::json!({"component": c, "placement": crate::mat_mul(&inverse, m)}))
            .collect();
        extent.insert("picked_placements".into(), Value::Array(pins));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    fn translate(x: f64, y: f64, z: f64) -> crate::Mat {
        [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]]
    }

    #[test]
    fn legacy_extrudes_keep_exact_relative_picked_placements() {
        let mut v = json!({
            "components": [
                {"id": 1, "name": "Parent", "parent": 0},
                {"id": 2, "name": "Child", "parent": 1},
                {"id": 3, "name": "Unplaced", "parent": 1},
            ],
            "occurrences": [
                {"id": 10, "component": 1, "parent": 0, "name": "Parent:1", "transform": translate(10.0, 0.0, 0.0)},
                {"id": 11, "component": 2, "parent": 1, "name": "Child:1", "transform": translate(0.0, 5.0, 0.0)},
            ],
            "features": [
                {"id": 100, "type": "extrude", "component": 2, "extent": {"to": [0.0, 0.0, 10.0]}},
                {"id": 101, "type": "extrude", "component": 0, "extent": {"distance": "10"}},
                {"id": 102, "type": "extrude", "component": 1, "extent": {"from": [0.0, 0.0, 0.0], "picked_placements": []}},
            ],
        });
        super::step(&mut v).unwrap();
        let pins = v["features"][0]["extent"]["picked_placements"].as_array().unwrap();
        assert_eq!(pins.len(), 3);
        assert_eq!(pins[0], json!({"component": 0, "placement": translate(-10.0, -5.0, 0.0)}));
        assert_eq!(pins[1], json!({"component": 1, "placement": translate(0.0, -5.0, 0.0)}));
        assert_eq!(pins[2], json!({"component": 3, "placement": translate(0.0, -5.0, 0.0)}));
        assert!(v["features"][1]["extent"].get("picked_placements").is_none());
        assert_eq!(v["features"][2]["extent"]["picked_placements"], json!([]));
    }

    #[test]
    fn u64_max_occurrence_id_with_unplaced_component_errors_without_panicking() {
        let mut v = json!({
            "components": [
                {"id": 1, "name": "A", "parent": 0},
                {"id": 2, "name": "B", "parent": 0},
            ],
            "occurrences": [
                {"id": u64::MAX, "component": 1, "parent": 0, "name": "A:1", "transform": crate::IDENTITY},
            ],
            "features": [
                {"id": 10, "type": "extrude", "component": 0, "extent": {"to": [0.0, 0.0, 1.0]}},
            ],
        });
        let err = super::step(&mut v).unwrap_err().to_string();
        assert!(err.contains("too many items"), "{err}");
    }

    #[test]
    fn crafted_pin_explosion_and_cyclic_parents_are_rejected_fast() {
        let components: Vec<Value> = (1..=500u64).map(|id| json!({"id": id, "name": format!("C{id}"), "parent": 0})).collect();
        let features: Vec<Value> =
            (1..=300u64).map(|id| json!({"id": id, "type": "extrude", "component": 0, "extent": {"to": [0.0, 0.0, 1.0]}})).collect();
        let mut huge = json!({"components": components, "occurrences": [], "features": features});
        let err = super::step(&mut huge).unwrap_err().to_string();
        assert!(err.contains("picked extrude placements"), "{err}");

        let mut cyclic = json!({
            "components": [{"id": 1, "name": "A", "parent": 2}, {"id": 2, "name": "B", "parent": 1}],
            "occurrences": [
                {"id": 10, "component": 1, "parent": 2, "name": "A:1", "transform": crate::IDENTITY},
                {"id": 11, "component": 2, "parent": 1, "name": "B:1", "transform": crate::IDENTITY},
            ],
            "features": [{"id": 1, "type": "extrude", "component": 0, "extent": {"to": [0.0, 0.0, 1.0]}}],
        });
        assert!(super::step(&mut cyclic).is_err());

        let too_many: Vec<Value> = (1..=crate::document::MAX_FEATURES + 1).map(|i| json!({"id": i, "name": "C"})).collect();
        let mut oversized = json!({"components": too_many, "occurrences": [], "features": []});
        assert!(super::step(&mut oversized).is_err());
    }
}
