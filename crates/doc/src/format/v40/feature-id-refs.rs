//! Pattern and mirror seeds (`Pattern`/`Mirror` `features`) and construction plane references
//! (`PlaneRef::Construction`) are saved as feature ids (#1134), like every other feature
//! reference. An older design has its names resolved to ids once as it opens, exactly the way
//! `replay()` resolved them: an exact name match on a feature that is not a sketch or a
//! construction plane, else a feature id written as a number, else a case-insensitive name
//! match; a name that resolves to nothing keeps id 0, which no feature has, so it dangles at
//! evaluation with the same warning or error it always gave. New saves write ids; a rename no
//! longer has to rewrite anything. The last release refuses a design that has them (it refuses
//! the newer format number).

use serde_json::Value;

use crate::Result;

/// A design's features as `(id, name, type)`, for resolving names the way `replay()` did.
type Features<'a> = &'a [(u64, String, String)];

/// `Document::find_feature` at the JSON level: an id written as a number, then a
/// case-insensitive name match.
fn find_feature(feats: Features, name: &str) -> Option<u64> {
    name.parse::<u64>()
        .ok()
        .and_then(|id| feats.iter().find(|(i, ..)| *i == id))
        .or_else(|| feats.iter().find(|(_, n, _)| n.eq_ignore_ascii_case(name)))
    .map(|(i, ..)| *i)
}

/// The feature a pattern or mirror seed named: an exact name match on a feature that is not a
/// sketch or a construction plane (a sketch and its extrude often share a name; the feature
/// that makes geometry wins), else [`find_feature`]; nothing is id 0, which dangles at
/// evaluation as the missing name did.
fn resolve_seed(feats: Features, name: &str) -> u64 {
    feats
        .iter()
        .find(|(_, n, t)| n == name && !matches!(t.as_str(), "sketch" | "construction_plane"))
        .map(|(i, ..)| *i)
        .or_else(|| find_feature(feats, name))
        .unwrap_or(0)
}

/// The feature a construction plane reference named ([`find_feature`]); nothing is id 0.
fn resolve_plane(feats: Features, name: &str) -> u64 {
    find_feature(feats, name).unwrap_or(0)
}

/// Rewrite every `{"type": "construction", "name": …}` plane reference in `v` to carry the
/// feature's id. `skip_axis` leaves a revolve's `axis` alone: an `AxisRef::Construction` has
/// the same shape but names a construction axis, which is still keyed by name.
fn rewrite(v: &mut Value, feats: Features, skip_axis: bool) {
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("construction") {
                if let Some(Value::String(name)) = o.get("name") {
                    let id = resolve_plane(feats, name);
                    o.remove("name");
                    o.insert("feature".into(), Value::from(id));
                }
                return;
            }
            for (key, x) in o.iter_mut() {
                if !(skip_axis && key == "axis") {
                    rewrite(x, feats, false);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| rewrite(x, feats, false)),
        _ => {}
    }
}

pub fn step(v: &mut Value) -> Result<()> {
    let feats: Vec<(u64, String, String)> = v
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|f| {
            let id = f.get("id")?.as_u64()?;
            let name = f.get("name")?.as_str()?.to_string();
            let kind = f.get("type")?.as_str()?.to_string();
            Some((id, name, kind))
        })
        .collect();
    if let Some(Value::Array(features)) = v.get_mut("features") {
        for f in features.iter_mut() {
            let kind = f.get("type").and_then(Value::as_str).unwrap_or("").to_string();
            // A pattern's or mirror's seeds: each name becomes the feature's id.
            if matches!(kind.as_str(), "pattern" | "mirror")
                && let Some(Value::Array(seeds)) = f.get_mut("features")
            {
                for s in seeds.iter_mut() {
                    if let Value::String(name) = s {
                        let id = resolve_seed(&feats, name);
                        *s = Value::from(id);
                    }
                }
            }
            // Construction plane references, everywhere but a revolve's axis.
            let skip_axis = matches!(kind.as_str(), "revolve" | "surface_revolve");
            rewrite(f, &feats, skip_axis);
        }
    }
    // References outside the features: canvases placed on a construction plane and the like.
    if let Some(o) = v.as_object_mut() {
        for (key, x) in o.iter_mut() {
            if key != "features" {
                rewrite(x, &feats, false);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::step;
    use serde_json::{Value, json};

    /// A design's feature list: a sketch and an extrude sharing the name `Boss` (the sketch
    /// first, as designs write them), a construction plane, a construction axis and a lower-
    /// cased extrude `boss`.
    fn features() -> Vec<Value> {
        vec![
            json!({"id": 1, "name": "Boss", "type": "sketch"}),
            json!({"id": 2, "name": "Boss", "type": "extrude"}),
            json!({"id": 3, "name": "Mid", "type": "construction_plane"}),
            json!({"id": 4, "name": "Axis1", "type": "construction_axis"}),
            json!({"id": 5, "name": "boss", "type": "extrude"}),
        ]
    }

    fn design(features: Vec<Value>) -> Value {
        json!({"format": "solidcraft/36", "features": features})
    }

    fn migrated(mut v: Value) -> Value {
        step(&mut v).unwrap();
        v
    }

    #[test]
    fn seeds_resolve_exactly_as_replay_resolved_them() {
        let mut feats = features();
        feats.push(json!({"id": 6, "name": "Copies", "type": "pattern", "features": ["Boss", "mid", "5", "Gone"], "pattern": {"type": "circular"}}));
        let out = migrated(design(feats));
        // `Boss`: the exact name on a feature that is not a sketch wins over the sketch and
        // over the case-insensitive `boss`. `mid`: no exact match, so the case-insensitive
        // find_feature fallback. `5`: an id written as a number. `Gone`: dangling, id 0.
        assert_eq!(out["features"][5]["features"], json!([2, 3, 5, 0]));
    }

    #[test]
    fn construction_planes_resolve_by_find_feature_and_dangle_as_zero() {
        let mut feats = features();
        feats.push(json!({"id": 7, "name": "On Mid", "type": "sketch", "plane": {"type": "construction", "name": "MID"}}));
        feats.push(json!({"id": 8, "name": "Lost", "type": "sketch", "plane": {"type": "offset", "distance": "5", "base": {"type": "construction", "name": "Gone"}}}));
        let out = migrated(design(feats));
        assert_eq!(out["features"][5]["plane"], json!({"type": "construction", "feature": 3}));
        assert_eq!(out["features"][6]["plane"]["base"], json!({"type": "construction", "feature": 0}));
    }

    #[test]
    fn a_revolve_axis_naming_a_construction_axis_keeps_its_name() {
        let mut feats = features();
        feats.push(json!({"id": 9, "name": "Turn", "type": "revolve", "axis": {"type": "construction", "name": "Axis1"}}));
        feats.push(json!({"id": 10, "name": "Surface Turn", "type": "surface_revolve", "axis": {"type": "construction", "name": "Axis1"}}));
        let out = migrated(design(feats));
        assert_eq!(out["features"][5]["axis"], json!({"type": "construction", "name": "Axis1"}));
        assert_eq!(out["features"][6]["axis"], json!({"type": "construction", "name": "Axis1"}));
    }

    #[test]
    fn references_outside_the_features_are_migrated_too() {
        let mut d = design(features());
        d["canvases"] = json!([{"id": 1, "plane": {"type": "construction", "name": "Mid"}}]);
        let out = migrated(d);
        assert_eq!(out["canvases"][0]["plane"], json!({"type": "construction", "feature": 3}));
    }
}
