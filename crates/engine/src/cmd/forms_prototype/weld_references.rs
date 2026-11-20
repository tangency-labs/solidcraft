//! Transfer whole-body consumers when Finish consumes a standalone Form.
//! Topology names need actual Weld provenance, never an ordinal/producer swap.
use crate::{EngineError, Result, Session};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) struct Transfer {
    bodies: BTreeSet<String>,
    errors: BTreeMap<u64, Option<String>>,
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::Other(message.into())
}

fn mapped(values: &mut [String], names: &BTreeMap<String, String>) {
    for value in values {
        if let Some(next) = names.get(value) {
            value.clone_from(next);
        }
    }
}

// This final guard covers typed-schema positional slots not exposed as face
// picks by the geometry walk (FaceAt.point, BlendSide.edges, legacy targets).
// It examines only the serialized FeatureKind, never user metadata or labels.
fn associated_pick(value: &Value, bodies: &BTreeSet<String>, depth: usize) -> Result<bool> {
    if depth > 64 {
        return Err(invalid("Weld reference nesting exceeds the editing budget"));
    }
    match value {
        Value::Object(fields) => {
            let named = fields.get("body").and_then(Value::as_str).is_some_and(|body| bodies.contains(body));
            let plural = fields
                .get("bodies")
                .and_then(Value::as_array)
                .is_some_and(|names| names.iter().filter_map(Value::as_str).any(|body| bodies.contains(body)));
            for key in
                ["face", "faces", "edge", "edges", "index", "indices", "vertex", "vertices", "point", "points", "curve", "curves", "pick", "picks"]
            {
                if !(named || (plural && !matches!(key, "point" | "points" | "curve" | "curves"))) {
                    continue;
                }
                if fields.get(key).is_some_and(|entry| !entry.is_null() && entry.as_array().is_none_or(|values| !values.is_empty())) {
                    return Ok(true);
                }
            }
            for entry in fields.values() {
                if associated_pick(entry, bodies, depth + 1)? {
                    return Ok(true);
                }
            }
        }
        Value::Array(values) => {
            for entry in values {
                if associated_pick(entry, bodies, depth + 1)? {
                    return Ok(true);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

pub(super) fn prepare(s: &Session, consumed: &BTreeMap<u64, Arc<crate::doc::Feature>>) -> Result<Transfer> {
    if consumed.is_empty() {
        return Ok(Transfer { bodies: BTreeSet::new(), errors: BTreeMap::new() });
    }
    let errors = s.doc.features.iter().map(|feature| (feature.id, s.model.result(feature.id).and_then(|result| result.error.clone()))).collect();
    let mut transfer = Transfer { bodies: BTreeSet::new(), errors };
    for &id in consumed.keys() {
        let source = s.model.result(id).ok_or_else(|| invalid("the Form selected for Weld has no evaluated source"))?;
        let bodies = source.state.bodies.iter().filter(|body| body.feature == id).collect::<Vec<_>>();
        if bodies.len() != 1 {
            return Err(invalid("Weld requires one unambiguous source body per consumed Form"));
        }
        let body = bodies.first().ok_or_else(|| invalid("Weld source body is missing"))?;
        transfer.bodies.insert(body.name.clone());
    }

    // A disposable typed remap distinguishes producer/topology references from
    // ordinary body selection. All probe identities are absent from the design.
    let used = s.doc.features.iter().map(|feature| feature.id).collect::<BTreeSet<_>>();
    let count = u64::try_from(used.len()).map_err(|_| invalid("Weld identity budget exceeded"))?;
    let probe = (1..=count.saturating_add(1)).find(|id| !used.contains(id)).ok_or_else(|| invalid("Weld probe identity is unavailable"))?;
    let ids = consumed.keys().map(|&id| (id, probe)).collect::<BTreeMap<_, _>>();
    let names = consumed
        .values()
        .map(|feature| {
            let next = if feature.name == "Weld reference probe" { "Weld reference probe 2" } else { "Weld reference probe" };
            (feature.name.clone(), next.to_owned())
        })
        .collect::<BTreeMap<_, _>>();
    let bodies = transfer
        .bodies
        .iter()
        .map(|body| {
            let next = if body == "Weld body probe" { "Weld body probe 2" } else { "Weld body probe" };
            (body.clone(), next.to_owned())
        })
        .collect::<BTreeMap<_, _>>();
    for feature in &s.doc.features {
        if consumed.contains_key(&feature.id) {
            continue;
        }
        if let crate::doc::FeatureKind::Thicken { bodies, .. } = &feature.kind
            && bodies.iter().filter(|body| transfer.bodies.contains(&body.0)).take(2).count() == 2
        {
            return Err(invalid(format!("Weld would merge separate Thicken sources in `{}`", feature.name)));
        }
        let mut body_only = feature.clone();
        body_only.kind.remap_bodies(&bodies);
        mapped(&mut body_only.body_names, &bodies);
        mapped(&mut body_only.participants, &bodies);
        let mut topology = feature.clone();
        let remapped = topology.remap_references_in_place(&ids, &names, &bodies);
        let picks = serde_json::to_value(&feature.kind).map_err(|error| invalid(error.to_string()))?;
        if remapped.is_err() || topology != body_only || associated_pick(&picks, &transfer.bodies, 0)? {
            return Err(invalid(format!("Weld cannot preserve the topology reference in `{}`; its source must be selected again", feature.name)));
        }
    }
    // A certified face assignment cannot be moved onto an unrelated patch by
    // deleting its producer. Keep survivor presentation/frame ownership intact.
    if s.doc.print_faces.values().any(|target| consumed.contains_key(&target.origin_feature) || transfer.bodies.contains(&target.body))
        || s.doc.appearances.faces.iter().any(|face| transfer.bodies.contains(&face.body))
        || s.doc.configs.columns.iter().any(|column| match column {
            crate::doc::config::Column::FaceAppearance(target) => {
                consumed.contains_key(&target.origin_feature) || transfer.bodies.contains(&target.body)
            }
            crate::doc::config::Column::Material(body) | crate::doc::config::Column::Appearance(body) => transfer.bodies.contains(body),
            _ => false,
        })
    {
        return Err(invalid("Weld cannot preserve a consumed Form's face or configuration assignment; remove that ambiguous assignment first"));
    }
    Ok(transfer)
}

impl Transfer {
    pub(super) fn apply(self, s: &mut Session, joined: u64) -> Result<()> {
        if self.bodies.is_empty() {
            return Ok(());
        }
        let result = s.model.result(joined).ok_or_else(|| invalid("the joined Form has no evaluated result"))?;
        let made = result.state.bodies.iter().filter(|body| body.feature == joined).collect::<Vec<_>>();
        if made.len() != 1 {
            return Err(invalid("Weld did not produce one unambiguous joined body"));
        }
        let name = made.first().ok_or_else(|| invalid("the joined Form body is missing"))?.name.clone();
        let bodies = self.bodies.iter().map(|old| (old.clone(), name.clone())).collect::<BTreeMap<_, _>>();
        let at = s.doc.feature_index(joined).ok_or_else(|| invalid("the joined Form feature is missing"))?;
        for (index, feature) in s.doc_mut().features.iter_mut().enumerate() {
            if feature.id == joined {
                continue;
            }
            let original = feature.clone();
            feature.remap_references_in_place(&BTreeMap::new(), &BTreeMap::new(), &bodies)?;
            if *feature != original && index <= at {
                return Err(invalid(format!("Weld would make `{}` depend on a later joined Form", feature.name)));
            }
            // Patterns and mirrors retain one output slot per input. An
            // identical replacement would produce coincident bodies or
            // change saved output slots if the inputs were deduplicated.
            if *feature != original {
                use crate::doc::FeatureKind::{Mirror, Pattern};
                let picks = match &feature.kind {
                    Pattern { bodies, .. } => Some((bodies, &[][..])),
                    Mirror { bodies, placements, .. } => Some((bodies, placements.as_slice())),
                    _ => None,
                };
                if let Some((picks, placements)) = picks {
                    let mut seen = BTreeSet::new();
                    if picks.iter().enumerate().any(|(i, body)| body.0 == name && !seen.insert(placements.get(i).map_or(&[][..], Vec::as_slice))) {
                        return Err(invalid(format!("Weld would merge separate copy sources in `{}`", feature.name)));
                    }
                }
            }
            // Several selected parts now denote one body. Whole-body transforms
            // and removals operate on that union once, rather than twice.
            match &mut feature.kind {
                crate::doc::FeatureKind::Move { bodies, .. }
                | crate::doc::FeatureKind::Scale { bodies, .. }
                | crate::doc::FeatureKind::Remove { bodies } => {
                    let mut seen = BTreeSet::new();
                    bodies.retain(|body| {
                        let name: &str = body.as_ref();
                        seen.insert(name.to_owned())
                    });
                }
                _ => {}
            }
        }
        s.refresh();
        for feature in &s.doc.features {
            let error = s.model.result(feature.id).and_then(|result| result.error.as_ref());
            if error != self.errors.get(&feature.id).and_then(Option::as_ref)
                && let Some(error) = error
            {
                return Err(invalid(format!("Weld cannot preserve `{}`: {error}", feature.name)));
            }
        }
        Ok(())
    }
}
