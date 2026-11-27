//! Sketch links save `sketch_placements` (source sketch ids and source-to-author rigid matrices).
//! A sketch's foreign construction-plane reference also saves its relative `placement`. These
//! references keep live source geometry but hold the assembly context in which the part was
//! drawn. Older designs capture their saved placements when opened, preserving the geometry a
//! cold rebuild previously produced. Root and same-component references remain live. The last
//! release warns about the newer format, ignores these fields while evaluating, and keeps them
//! when saving what it cannot read.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::{DocError, Document, FaceAnchor, Mat, Occurrence, PlaneRef, Result};

/// Maximum total sketch links inspected across one legacy design.
const MAX_TOTAL_SKETCH_LINKS: usize = 200_000;
/// Maximum total captured sketch placements synthesized across one legacy design.
const MAX_TOTAL_SKETCH_PINS: usize = 200_000;

struct OccurrenceIndex<'a> {
    first_of: HashMap<u64, &'a Occurrence>,
    by_id_parent: HashMap<(u64, u64), &'a Occurrence>,
    first_by_id: HashMap<u64, u64>,
    world_cache: HashMap<u64, Mat>,
}

impl<'a> OccurrenceIndex<'a> {
    fn new(occurrences: &'a [Occurrence]) -> Self {
        let mut first_of = HashMap::with_capacity(occurrences.len());
        let mut by_id_parent = HashMap::with_capacity(occurrences.len());
        let mut first_by_id = HashMap::with_capacity(occurrences.len());
        for o in occurrences {
            first_of.entry(o.component).or_insert(o);
            by_id_parent.entry((o.id, o.parent)).or_insert(o);
            first_by_id.entry(o.id).or_insert(o.component);
        }
        Self { first_of, by_id_parent, first_by_id, world_cache: HashMap::new() }
    }

    fn component_transform(&mut self, component: u64) -> Result<Mat> {
        if let Some(&cached) = self.world_cache.get(&component) {
            return Ok(cached);
        }
        let mut m = crate::IDENTITY;
        let mut c = component;
        for _ in 0..crate::MAX_PLACEMENT_DEPTH {
            if c == 0 {
                self.world_cache.insert(component, m);
                return Ok(m);
            }
            let Some(o) = self.first_of.get(&c) else {
                self.world_cache.insert(component, m);
                return Ok(m);
            };
            m = crate::mat_mul(&o.transform, &m);
            c = o.parent;
        }
        if c == 0 || !self.first_of.contains_key(&c) {
            self.world_cache.insert(component, m);
            return Ok(m);
        }
        Err(crate::assembly::too_many_placements())
    }

    fn author_frame(&mut self, anchor: &FaceAnchor, author: u64) -> Result<Mat> {
        let path = &anchor.author_path;
        if author != 0 && path.is_empty() {
            return Err(DocError::Invalid("attached sketch requires an explicit author occurrence path".into()));
        }
        if path.len() > crate::MAX_PLACEMENT_DEPTH {
            return Err(DocError::Invalid("the placement path exceeds work limits".into()));
        }
        let Some((&last, _)) = path.split_last() else {
            return self.component_transform(author);
        };
        let mut frame = crate::IDENTITY;
        let mut component = 0;
        let mut seen = HashSet::with_capacity(path.len());
        for &id in path {
            if id == 0 || !seen.insert(id) {
                return Err(DocError::Invalid("invalid placement path".into()));
            }
            let Some(occurrence) = self.by_id_parent.get(&(id, component)) else {
                return Err(DocError::Invalid("the placement no longer exists".into()));
            };
            frame = crate::mat_mul(&frame, &occurrence.transform);
            component = occurrence.component;
        }
        if self.first_by_id.get(&last).copied() == Some(author) {
            Ok(frame)
        } else {
            Err(DocError::Invalid("selected cylinder path belongs to another component".into()))
        }
    }
}

pub fn step(value: &mut Value) -> Result<()> {
    let too_many = |key: &str, max: usize| value.get(key).and_then(Value::as_array).is_some_and(|list| list.len() > max);
    if too_many("features", crate::document::MAX_FEATURES)
        || too_many("components", crate::document::MAX_FEATURES)
        || too_many("occurrences", crate::assembly::MAX_OCCURRENCES)
    {
        return Err(DocError::Invalid("document too large".into()));
    }
    // Only assembly frames are needed. Unknown feature types in a newer file must not prevent
    // known sketches from capturing their saved context.
    let mut context = Document::new("sketch reference migration");
    context.components = match serde_json::from_value(value.get("components").cloned().unwrap_or_else(|| json!([]))) {
        Ok(components) => components,
        Err(_) => return Ok(()),
    };
    context.occurrences = match serde_json::from_value(value.get("occurrences").cloned().unwrap_or_else(|| json!([]))) {
        Ok(occurrences) => occurrences,
        Err(_) => return Ok(()),
    };
    context.next_id = context
        .occurrences
        .iter()
        .map(|o| o.id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| DocError::Invalid("too many items in the design".into()))?;
    context.components.retain(|c| c.id != 0);
    context.place_unplaced_components()?;
    let mut index = OccurrenceIndex::new(&context.occurrences);
    let owners: HashMap<u64, u64> = value
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|feature| Some((feature.get("id")?.as_u64()?, feature.get("component").and_then(Value::as_u64).unwrap_or(0))))
        .collect();
    let mut total_links = 0usize;
    let mut total_pins = 0usize;
    for feature in value.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let author = feature.get("component").and_then(Value::as_u64).unwrap_or(0);
        if author == 0 {
            continue;
        }
        let plane = feature.get("plane").cloned().and_then(|p| serde_json::from_value::<PlaneRef>(p).ok());
        let frame = match plane.as_ref().and_then(crate::face_anchor::plane_face_anchor) {
            // A dangling attachment remains a feature error, not a reason to refuse the file.
            // Newer-file salvage also checks individual features without assembly context.
            Some(anchor) => match index.author_frame(anchor, author) {
                Ok(frame) => frame,
                Err(_) => continue,
            },
            None => index.component_transform(author)?,
        };
        let inverse = crate::mat_inverse(&frame).ok_or_else(|| DocError::Invalid("projection author frame is invalid".into()))?;
        if let Some(plane) = feature.get_mut("plane") {
            capture_plane(plane, author, &inverse, &mut index, &owners, 0)?;
        }
        for link in feature.get_mut("sketch").and_then(|s| s.get_mut("links")).and_then(Value::as_array_mut).into_iter().flatten() {
            total_links = total_links.saturating_add(1);
            if total_links > MAX_TOTAL_SKETCH_LINKS {
                return Err(DocError::Invalid("too many sketch links".into()));
            }
            if link.get("sketch_placements").is_some() {
                continue;
            }
            let Some(source) = link.get("source").cloned().and_then(|s| serde_json::from_value::<solidcraft_sketch::LinkSource>(s).ok()) else {
                continue;
            };
            let mut pins = Vec::new();
            for sketch in crate::project::source_sketches(&source)? {
                let Some(owner) = owners.get(&sketch).filter(|owner| **owner != author) else { continue };
                total_pins = total_pins.saturating_add(1);
                if total_pins > MAX_TOTAL_SKETCH_PINS {
                    return Err(DocError::Invalid("too many captured sketch reference placements".into()));
                }
                pins.push(json!({"sketch":sketch, "placement":relative(&mut index, &inverse, *owner)?}));
            }
            if !pins.is_empty()
                && let Some(link) = link.as_object_mut()
            {
                link.insert("sketch_placements".into(), Value::Array(pins));
            }
        }
    }
    Ok(())
}

fn relative(index: &mut OccurrenceIndex<'_>, inverse: &Mat, owner: u64) -> Result<Mat> {
    let placement = crate::mat_mul(inverse, &index.component_transform(owner)?);
    if !crate::is_placement(&placement) {
        return Err(DocError::Invalid("captured sketch reference placement is invalid".into()));
    }
    Ok(placement)
}

fn capture_plane(
    plane: &mut Value,
    author: u64,
    inverse: &Mat,
    index: &mut OccurrenceIndex<'_>,
    owners: &HashMap<u64, u64>,
    depth: usize,
) -> Result<()> {
    if depth > 64 {
        return Err(DocError::Invalid("sketch plane reference is too deep".into()));
    }
    if plane.get("type").and_then(Value::as_str) == Some("construction")
        && plane.get("placement").is_none_or(Value::is_null)
        && let Some(owner) = plane.get("feature").and_then(Value::as_u64).and_then(|id| owners.get(&id)).filter(|owner| **owner != author)
    {
        let placement = relative(index, inverse, *owner)?;
        if let Some(plane) = plane.as_object_mut() {
            plane.insert("placement".into(), json!(placement));
        }
    }
    if let Some(base) = plane.get_mut("base") {
        capture_plane(base, author, inverse, index, owners, depth + 1)?;
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
    fn captures_foreign_plane_and_link_placements_with_unplaced_components() {
        let mut v = json!({
            "components": [
                {"id": 1, "name": "Source", "parent": 0},
                {"id": 2, "name": "Author", "parent": 0},
                {"id": 3, "name": "Unplaced", "parent": 1},
            ],
            "occurrences": [
                {"id": 10, "component": 1, "parent": 0, "name": "Source:1", "transform": translate(20.0, 0.0, 0.0)},
                {"id": 11, "component": 2, "parent": 0, "name": "Author:1", "transform": translate(5.0, 3.0, 0.0)},
            ],
            "features": [
                {"id": 1, "type": "construction_plane", "component": 1},
                {"id": 2, "type": "sketch", "component": 3, "plane": {"type": "origin", "name": "XY"}, "sketch": {}},
                {
                    "id": 3,
                    "type": "sketch",
                    "component": 2,
                    "plane": {"type": "offset", "distance": "5", "base": {"type": "construction", "feature": 1}},
                    "sketch": {
                        "links": [
                            {"id": "l1", "source": {"type": "sketch_point", "sketch": 2, "point": "p1"}}
                        ]
                    }
                }
            ],
        });
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][2]["plane"]["base"]["placement"], json!(translate(15.0, -3.0, 0.0)));
        assert_eq!(v["features"][2]["sketch"]["links"][0]["sketch_placements"], json!([{"sketch": 2, "placement": translate(15.0, -3.0, 0.0)}]));
    }

    #[test]
    fn many_unplaced_components_and_links_finish_fast_and_caps_reject_crafted_files() {
        let components: Vec<Value> = (1..=1000u64).map(|id| json!({"id": id, "name": "P", "parent": 0})).collect();
        let occurrences: Vec<Value> =
            (1..=1000u64).map(|i| json!({"id": 10_000 + i, "component": 1, "parent": 0, "name": format!("P:{i}")})).collect();
        let links: Vec<Value> =
            (0..500).map(|i| json!({"id": format!("l{i}"), "source": {"type": "sketch_point", "sketch": 1, "point": "p1"}})).collect();
        let mut v = json!({
            "components": components,
            "occurrences": occurrences,
            "features": [
                {"id": 1, "type": "sketch", "component": 1, "plane": {"type": "origin", "name": "XY"}, "sketch": {}},
                {"id": 2, "type": "sketch", "component": 2, "plane": {"type": "origin", "name": "XY"}, "sketch": {"links": links}},
            ],
        });
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][1]["sketch"]["links"][499]["sketch_placements"], json!([{"sketch": 1, "placement": crate::IDENTITY}]));

        // Exceeding MAX_PLACEMENT_DEPTH through occurrence parent chain is rejected cleanly.
        let deep_n = (crate::MAX_PLACEMENT_DEPTH + 2) as u64;
        let deep_components: Vec<Value> = (1..=deep_n).map(|id| json!({"id": id, "name": format!("C{id}"), "parent": id - 1})).collect();
        let deep_occurrences: Vec<Value> =
            (1..=deep_n).map(|id| json!({"id": 1000 + id, "component": id, "parent": id - 1, "name": format!("C{id}:1")})).collect();
        let mut deep = json!({
            "components": deep_components,
            "occurrences": deep_occurrences,
            "features": [{"id": 1, "type": "sketch", "component": deep_n, "plane": {"type": "origin", "name": "XY"}, "sketch": {}}],
        });
        assert!(super::step(&mut deep).is_err());

        // Oversized links count is rejected cleanly.
        let too_many_links: Vec<Value> = vec![json!({"id": "l", "sketch_placements": []}); super::MAX_TOTAL_SKETCH_LINKS + 1];
        let mut bloated = json!({
            "components": [{"id": 1, "name": "A", "parent": 0}],
            "occurrences": [{"id": 1, "component": 1, "parent": 0, "name": "A:1"}],
            "features": [{"id": 1, "type": "sketch", "component": 1, "plane": {"type": "origin", "name": "XY"}, "sketch": {"links": too_many_links}}],
        });
        assert!(super::step(&mut bloated).is_err());
    }
}
