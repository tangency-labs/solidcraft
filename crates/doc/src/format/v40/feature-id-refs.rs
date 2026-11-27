//! Pattern and mirror seeds (`Pattern`/`Mirror` `features`) and construction plane references
//! (`PlaneRef::Construction`) are saved as feature ids (#1134), like every other feature
//! reference. An older design has its names resolved to ids once as it opens, exactly the way
//! `replay()` resolved them: an exact name match on a feature that is not a sketch or a
//! construction plane, else a feature id written as a number, else a case-insensitive name
//! match; a name that resolves to nothing keeps id 0, which no feature has, so it dangles at
//! evaluation with the same warning or error it always gave. New saves write ids; a rename no
//! longer has to rewrite anything. The last release refuses a design that has them (it refuses
//! the newer format number).

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{DocError, Result};

/// Maximum total pattern/mirror seed entries resolved across one legacy design.
const MAX_SEEDS: usize = 1_000_000;
/// Maximum JSON recursion depth when rewriting construction plane references.
const MAX_DEPTH: usize = 128;

/// Indexed lookup over a design's features, matching `replay()`'s linear-scan resolution order.
struct FeatureIndex {
    ids: HashSet<u64>,
    by_exact_non_sketch: HashMap<String, u64>,
    by_ascii_lower: HashMap<String, u64>,
}

impl FeatureIndex {
    fn build(features: &[Value]) -> Self {
        let mut ids = HashSet::with_capacity(features.len());
        let mut by_exact_non_sketch = HashMap::with_capacity(features.len());
        let mut by_ascii_lower = HashMap::with_capacity(features.len());
        for f in features {
            let Some(o) = f.as_object() else { continue };
            let (Some(id), Some(name), Some(kind)) =
                (o.get("id").and_then(Value::as_u64), o.get("name").and_then(Value::as_str), o.get("type").and_then(Value::as_str))
            else {
                continue;
            };
            ids.insert(id);
            if !matches!(kind, "sketch" | "construction_plane") {
                by_exact_non_sketch.entry(name.to_string()).or_insert(id);
            }
            by_ascii_lower.entry(name.to_ascii_lowercase()).or_insert(id);
        }
        Self { ids, by_exact_non_sketch, by_ascii_lower }
    }

    /// `Document::find_feature` at the JSON level: an id written as a number, then a
    /// case-insensitive name match.
    fn find_feature(&self, name: &str) -> Option<u64> {
        if let Ok(id) = name.parse::<u64>()
            && self.ids.contains(&id)
        {
            return Some(id);
        }
        self.by_ascii_lower.get(&name.to_ascii_lowercase()).copied()
    }

    /// The feature a pattern or mirror seed named: an exact name match on a feature that is not a
    /// sketch or a construction plane (a sketch and its extrude often share a name; the feature
    /// that makes geometry wins), else [`Self::find_feature`]; nothing is id 0, which dangles at
    /// evaluation as the missing name did.
    fn resolve_seed(&self, name: &str) -> u64 {
        self.by_exact_non_sketch.get(name).copied().or_else(|| self.find_feature(name)).unwrap_or(0)
    }

    /// The feature a construction plane reference named ([`Self::find_feature`]); nothing is id 0.
    fn resolve_plane(&self, name: &str) -> u64 {
        self.find_feature(name).unwrap_or(0)
    }
}

/// Rewrite every `{"type": "construction", "name": …}` plane reference in `v` to carry the
/// feature's id. `skip_axis` leaves a revolve's `axis` alone: an `AxisRef::Construction` has
/// the same shape but names a construction axis, which is still keyed by name.
fn rewrite(v: &mut Value, index: &FeatureIndex, skip_axis: bool, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(DocError::Invalid("damaged design: nested too deeply".into()));
    }
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("construction") {
                if let Some(Value::String(name)) = o.get("name") {
                    let id = index.resolve_plane(name);
                    o.remove("name");
                    o.insert("feature".into(), Value::from(id));
                }
                return Ok(());
            }
            for (key, x) in o.iter_mut() {
                if !(skip_axis && key == "axis") {
                    rewrite(x, index, false, depth + 1)?;
                }
            }
        }
        Value::Array(a) => {
            for x in a.iter_mut() {
                rewrite(x, index, false, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn step(v: &mut Value) -> Result<()> {
    let feature_slice = v.get("features").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    if feature_slice.len() > crate::document::MAX_FEATURES {
        return Err(DocError::Invalid("document too large".into()));
    }
    let index = FeatureIndex::build(feature_slice);
    let mut total_seeds = 0usize;
    if let Some(Value::Array(features)) = v.get_mut("features") {
        for f in features.iter_mut() {
            let is_seed_owner = matches!(f.get("type").and_then(Value::as_str), Some("pattern" | "mirror"));
            let skip_axis = matches!(f.get("type").and_then(Value::as_str), Some("revolve" | "surface_revolve"));
            // A pattern's or mirror's seeds: each name becomes the feature's id.
            if is_seed_owner && let Some(Value::Array(seeds)) = f.get_mut("features") {
                if seeds.len() > crate::document::MAX_FEATURES {
                    return Err(DocError::Invalid("too many pattern or mirror seeds".into()));
                }
                total_seeds = total_seeds.saturating_add(seeds.len());
                if total_seeds > MAX_SEEDS {
                    return Err(DocError::Invalid("too many pattern or mirror seeds".into()));
                }
                for s in seeds.iter_mut() {
                    if let Value::String(name) = s {
                        let id = index.resolve_seed(name);
                        *s = Value::from(id);
                    }
                }
            }
            // Construction plane references, everywhere but a revolve's axis.
            rewrite(f, &index, skip_axis, 0)?;
        }
    }
    // References outside the features: canvases placed on a construction plane and the like.
    if let Some(o) = v.as_object_mut() {
        for (key, x) in o.iter_mut() {
            if key != "features" {
                rewrite(x, &index, false, 0)?;
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

    #[test]
    fn large_feature_lists_resolve_in_linear_time_and_oversized_inputs_error() {
        let mut feats: Vec<Value> = (1..=5_000u64).map(|i| json!({"id": i, "name": format!("Box{i}"), "type": "extrude"})).collect();
        let seeds: Vec<Value> = (1..=5_000u64).map(|i| json!(format!("Box{i}"))).collect();
        feats.push(json!({"id": 5_001, "name": "Pat", "type": "pattern", "features": seeds}));
        let out = migrated(design(feats));
        assert_eq!(out["features"][5_000]["features"][0], json!(1));
        assert_eq!(out["features"][5_000]["features"][4_999], json!(5_000));

        let too_many_features: Vec<Value> = vec![json!({"id": 1, "name": "A", "type": "extrude"}); crate::document::MAX_FEATURES + 1];
        assert!(step(&mut design(too_many_features)).is_err());

        let too_many_seeds = vec![json!("Boss"); crate::document::MAX_FEATURES + 1];
        let mut bad_seeds = design(vec![json!({"id": 1, "name": "P", "type": "pattern", "features": too_many_seeds})]);
        assert!(step(&mut bad_seeds).is_err());
    }
}
