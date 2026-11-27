//! A surface sweep along a 3D sketch spline follows the spline itself, one wall along it (closed
//! round a closed spline), where older builds swept the lines between the spline's samples, a wall
//! along each (#17828). This step marks every saved surface sweep whose path lists a 3D sketch
//! curve `spline_as_samples`, so a design saved before keeps its walls and what refers to one of
//! them (a pick, a name) still finds it; a new or redefined sweep has no mark. The last release
//! opens a design with the mark with a warning about the newer format, sweeps along the samples as
//! it always did, and keeps the field when the design is saved.
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::DocError;

/// Maximum 3D wires in one sketch (matches `validate_wires` in `format.rs`).
const MAX_WIRES_PER_SKETCH: usize = 20_000;
/// Maximum total path curve entries inspected across all legacy surface sweeps.
const MAX_PATH_ENTRIES: usize = 200_000;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    if features.len() > crate::document::MAX_FEATURES {
        return Err(DocError::Invalid("document too large".into()));
    }
    let needed: HashSet<u64> = features
        .iter()
        .filter(|f| f.get("type").and_then(Value::as_str) == Some("surface_sweep"))
        .filter_map(|f| f.get("path_sketch").and_then(Value::as_u64))
        .collect();
    if needed.is_empty() {
        return Ok(());
    }
    // Each referenced sketch's 3D curve ids, by the sketch feature's id (first matching feature wins).
    let mut wires: HashMap<u64, HashSet<String>> = HashMap::with_capacity(needed.len());
    for f in features.iter() {
        let Some(id) = f.get("id").and_then(Value::as_u64) else { continue };
        if !needed.contains(&id) {
            continue;
        }
        let Some(list) = f.get("sketch").and_then(|s| s.get("wires")).and_then(Value::as_array) else { continue };
        let Entry::Vacant(entry) = wires.entry(id) else { continue };
        if list.len() > MAX_WIRES_PER_SKETCH {
            return Err(DocError::Invalid("damaged design: too many 3D curves in sketch".into()));
        }
        let ids: HashSet<String> = list.iter().filter_map(|w| w.get("id")?.as_str().map(str::to_string)).collect();
        entry.insert(ids);
    }
    let mut total_path_entries = 0usize;
    for feature in features.iter_mut() {
        if feature.get("type").and_then(Value::as_str) != Some("surface_sweep") {
            continue;
        }
        let Some(sketch) = feature.get("path_sketch").and_then(Value::as_u64) else { continue };
        let Some(ids) = wires.get(&sketch) else { continue };
        let Some(path) = feature.get("path").and_then(Value::as_array) else { continue };
        total_path_entries = total_path_entries.saturating_add(path.len());
        if total_path_entries > MAX_PATH_ENTRIES {
            return Err(DocError::Invalid("too many surface sweep path entries".into()));
        }
        let on_wire = !ids.is_empty() && path.iter().any(|c| c.as_str().is_some_and(|c| ids.contains(c)));
        if on_wire && let Some(f) = feature.as_object_mut() {
            f.insert("spline_as_samples".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn saved_surface_sweeps_along_3d_curves_keep_their_sampled_walls() {
        let mut v = json!({"features": [
            {"id": 2, "type": "sketch", "sketch": {"wires": [{"id": "w1"}]}},
            {"id": 3, "type": "sketch", "sketch": {"curves": [{"id": "l1"}]}},
            {"id": 4, "type": "surface_sweep", "path_sketch": 2, "path": ["w1"]},
            {"id": 5, "type": "surface_sweep", "path_sketch": 3, "path": ["l1"]},
            {"id": 6, "type": "surface_sweep", "path_body": "Body1", "path_edges": [[0.0, 0.0, 0.0]]},
            {"id": 7, "type": "sweep", "path_sketch": 2, "path": ["w1"]}
        ]});
        super::step(&mut v).unwrap();
        let marked: Vec<bool> = v["features"].as_array().unwrap().iter().map(|f| f["spline_as_samples"] == json!(true)).collect();
        assert_eq!(marked, [false, false, true, false, false, false]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }

    #[test]
    fn large_surface_sweep_designs_upgrade_linearly_and_caps_reject_crafted_files() {
        let wires: Vec<serde_json::Value> = (0..5_000).map(|i| json!({"id": format!("w{i}")})).collect();
        let mut features = vec![json!({"id": 1, "type": "sketch", "sketch": {"wires": wires}})];
        for i in 0..2_000u64 {
            features.push(json!({"id": 2 + i, "type": "surface_sweep", "path_sketch": 1, "path": [format!("w{i}")]}));
        }
        let mut v = json!({"features": features});
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][1]["spline_as_samples"], json!(true));
        assert_eq!(v["features"][2_000]["spline_as_samples"], json!(true));

        let too_many_features = vec![json!({"id": 1, "type": "box"}); crate::document::MAX_FEATURES + 1];
        assert!(super::step(&mut json!({"features": too_many_features})).is_err());

        let too_many_wires = vec![json!({"id": "w"}); super::MAX_WIRES_PER_SKETCH + 1];
        let mut bad_wires = json!({"features": [
            {"id": 1, "type": "sketch", "sketch": {"wires": too_many_wires}},
            {"id": 2, "type": "surface_sweep", "path_sketch": 1, "path": ["w"]},
        ]});
        assert!(super::step(&mut bad_wires).is_err());

        let too_many_paths = vec![json!("w0"); super::MAX_PATH_ENTRIES + 1];
        let mut bad_paths = json!({"features": [
            {"id": 1, "type": "sketch", "sketch": {"wires": [{"id": "w0"}]}},
            {"id": 2, "type": "surface_sweep", "path_sketch": 1, "path": too_many_paths},
        ]});
        assert!(super::step(&mut bad_paths).is_err());
    }
}
