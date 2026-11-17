//! A surface sweep along a 3D sketch spline follows the spline itself, one wall along it (closed
//! round a closed spline), where older builds swept the lines between the spline's samples, a wall
//! along each (#17828). This step marks every saved surface sweep whose path lists a 3D sketch
//! curve `spline_as_samples`, so a design saved before keeps its walls and what refers to one of
//! them (a pick, a name) still finds it; a new or redefined sweep has no mark. The last release
//! opens a design with the mark with a warning about the newer format, sweeps along the samples as
//! it always did, and keeps the field when the design is saved.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    // Each sketch's 3D curve ids, by the sketch feature's id.
    let wires: Vec<(u64, Vec<String>)> = features
        .iter()
        .filter_map(|f| {
            let id = f.get("id")?.as_u64()?;
            let ids = f.get("sketch")?.get("wires")?.as_array()?.iter().filter_map(|w| w.get("id")?.as_str().map(str::to_string)).collect();
            Some((id, ids))
        })
        .collect();
    for feature in features.iter_mut() {
        if feature.get("type").and_then(Value::as_str) != Some("surface_sweep") {
            continue;
        }
        let Some(sketch) = feature.get("path_sketch").and_then(Value::as_u64) else { continue };
        let Some((_, ids)) = wires.iter().find(|(id, _)| *id == sketch) else { continue };
        let on_wire =
            feature.get("path").and_then(Value::as_array).is_some_and(|p| p.iter().any(|c| c.as_str().is_some_and(|c| ids.iter().any(|w| w == c))));
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
}
