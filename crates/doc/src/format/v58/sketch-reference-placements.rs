//! Sketch links save `sketch_placements` (source sketch ids and source-to-author rigid matrices).
//! A sketch's foreign construction-plane reference also saves its relative `placement`. These
//! references keep live source geometry but hold the assembly context in which the part was
//! drawn. Older designs capture their saved placements when opened, preserving the geometry a
//! cold rebuild previously produced. Root and same-component references remain live. The last
//! release warns about the newer format, ignores these fields while evaluating, and keeps them
//! when saving what it cannot read.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{DocError, Document, Mat, PlaneRef, Result};

pub fn step(value: &mut Value) -> Result<()> {
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
    let missing: Vec<_> = context
        .components
        .iter()
        .filter(|c| c.id != 0 && !context.occurrences.iter().any(|o| o.component == c.id))
        .map(|c| (c.id, c.parent))
        .collect();
    for (component, parent) in missing {
        context.add_occurrence(component, parent, crate::IDENTITY)?;
    }
    let owners: BTreeMap<u64, u64> = value
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|feature| Some((feature.get("id")?.as_u64()?, feature.get("component").and_then(Value::as_u64).unwrap_or(0))))
        .collect();
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
            Some(anchor) => match anchor.author_frame(&context, author) {
                Ok(frame) => frame,
                Err(_) => continue,
            },
            None => context.component_transform(author),
        };
        let inverse = crate::mat_inverse(&frame).ok_or_else(|| DocError::Invalid("projection author frame is invalid".into()))?;
        if let Some(plane) = feature.get_mut("plane") {
            capture_plane(plane, author, &inverse, &context, &owners, 0)?;
        }
        for link in feature.get_mut("sketch").and_then(|s| s.get_mut("links")).and_then(Value::as_array_mut).into_iter().flatten() {
            if link.get("sketch_placements").is_some() {
                continue;
            }
            let Some(source) = link.get("source").cloned().and_then(|s| serde_json::from_value::<solidcraft_sketch::LinkSource>(s).ok()) else {
                continue;
            };
            let mut pins = Vec::new();
            for sketch in crate::project::source_sketches(&source)? {
                let Some(owner) = owners.get(&sketch).filter(|owner| **owner != author) else { continue };
                pins.push(json!({"sketch":sketch, "placement":relative(&context, &inverse, *owner)?}));
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

fn relative(context: &Document, inverse: &Mat, owner: u64) -> Result<Mat> {
    let placement = crate::mat_mul(inverse, &context.component_transform(owner));
    if !crate::is_placement(&placement) {
        return Err(DocError::Invalid("captured sketch reference placement is invalid".into()));
    }
    Ok(placement)
}

fn capture_plane(plane: &mut Value, author: u64, inverse: &Mat, context: &Document, owners: &BTreeMap<u64, u64>, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(DocError::Invalid("sketch plane reference is too deep".into()));
    }
    if plane.get("type").and_then(Value::as_str) == Some("construction")
        && plane.get("placement").is_none_or(Value::is_null)
        && let Some(owner) = plane.get("feature").and_then(Value::as_u64).and_then(|id| owners.get(&id)).filter(|owner| **owner != author)
    {
        let placement = relative(context, inverse, *owner)?;
        if let Some(plane) = plane.as_object_mut() {
            plane.insert("placement".into(), json!(placement));
        }
    }
    if let Some(base) = plane.get_mut("base") {
        capture_plane(base, author, inverse, context, owners, depth + 1)?;
    }
    Ok(())
}
