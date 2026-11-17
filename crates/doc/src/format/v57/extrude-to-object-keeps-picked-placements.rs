//! An extrude that ends To Object or starts From Object may save `picked_placements` in its
//! `extent`: where each other component stood, relative to the extrude's own, when the object
//! was picked (`[{component, placement}]`, a 4x4 matrix each). The extrude measures against the
//! model as it was then, so a joint that slides a component does not stretch a body built to
//! another component's face (#16732). Older builds measured against the placements as shown, so
//! a drawer that ended at its frame grew with every slide. A design saved that way gets the
//! placements its file holds as it opens, which are where the extrude was last built, so it
//! opens with the bodies it had and no longer stretches. The last release opens a design that has
//! the field with a newer-format warning, ignores it, and measures against the placements shown.

use serde_json::Value;

use crate::{Mat, Occurrence, Result};

pub fn step(v: &mut Value) -> Result<()> {
    let mut occurrences: Vec<Occurrence> = match v.get("occurrences").cloned() {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => match serde_json::from_value(value) {
            Ok(occurrences) => occurrences,
            Err(_) => return Ok(()),
        },
    };
    // A component an older file never placed is placed at the identity as the file opens
    // (`normalise`), so that is where it stands.
    let missing: Vec<(u64, u64)> = v
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c.get("id")?.as_u64()?, c.get("parent").and_then(Value::as_u64).unwrap_or(0))))
        .filter(|(id, _)| *id != 0 && !occurrences.iter().any(|o| o.component == *id))
        .collect();
    for (component, parent) in missing {
        let id = occurrences.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        occurrences.push(Occurrence {
            id,
            component,
            parent,
            name: String::new(),
            transform: crate::IDENTITY,
            grounded: false,
            expressed_moves: Vec::new(),
        });
    }
    if occurrences.is_empty() {
        return Ok(());
    }
    // Each component's world placement: its first occurrence, through its parents.
    let world = |component: u64| -> Mat { crate::assembly::transform_through(component, |c| occurrences.iter().find(|o| o.component == c)) };
    let mut components: Vec<u64> = occurrences.iter().map(|o| o.component).collect();
    components.push(0);
    components.sort_unstable();
    components.dedup();
    let worlds: Vec<(u64, Mat)> = components.iter().map(|c| (*c, world(*c))).collect();
    let world_of = |component: u64| worlds.iter().find(|(c, _)| *c == component).map(|(_, m)| *m).unwrap_or_else(|| world(component));
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        if feature.get("type").and_then(Value::as_str) != Some("extrude") {
            continue;
        }
        let own = feature.get("component").and_then(Value::as_u64).unwrap_or(0);
        let Some(inverse) = crate::mat_inverse(&world_of(own)) else { continue };
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
